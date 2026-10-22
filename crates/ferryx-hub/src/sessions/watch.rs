//! Session notices driven by observer events. Delivery waits `notify_after` and
//! holds through KST quiet hours on a timer. Nothing here polls the panes.
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Timelike, Utc};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use tokio::sync::Mutex;

use crate::error::Result;

pub const DEFAULT_NOTIFY_AFTER: Duration = Duration::from_secs(30 * 60);
pub const DEFAULT_QUIET_HOURS: (u32, u32) = (0, 8);
const KST: TimeDelta = TimeDelta::hours(9);

/// Which owner-facing notices are posted to Slack or Discord.
///
/// `Actionable` is the default: questions, approvals, failures, and the result of a
/// task the owner started from chat. Plain turn-complete and progress notices stay
/// in the ledger. `All` also posts those.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyMode {
    Actionable,
    All,
}

impl NotifyMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "actionable" => Some(Self::Actionable),
            "all" => Some(Self::All),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Actionable => "actionable",
            Self::All => "all",
        }
    }
}

/// `FERRYX_HUB_NOTIFY` wins, then `OMON_SESSION_NOTIFY`. An unknown value is ignored.
pub fn notify_mode_from_env() -> Option<NotifyMode> {
    std::env::var("FERRYX_HUB_NOTIFY")
        .ok()
        .or_else(|| std::env::var("OMON_SESSION_NOTIFY").ok())
        .as_deref()
        .and_then(NotifyMode::parse)
}

/// Env wins over `hub_meta.notify_mode`. Missing or unknown storage stays actionable.
pub async fn effective_notify_mode(pool: &SqlitePool) -> Result<NotifyMode> {
    if let Some(mode) = notify_mode_from_env() {
        return Ok(mode);
    }
    let stored: Option<String> =
        sqlx::query_scalar("SELECT value FROM hub_meta WHERE key = 'notify_mode'")
            .fetch_optional(pool)
            .await?;
    Ok(stored
        .as_deref()
        .and_then(NotifyMode::parse)
        .unwrap_or(NotifyMode::Actionable))
}

/// Whether this notice kind is posted to the owner channel for `mode`.
///
/// Suppressed kinds are still recorded in the ledger. An explicit chat origin is a
/// separate reply and is not decided here.
pub fn owner_post(mode: NotifyMode, kind: &str) -> bool {
    if mode == NotifyMode::All {
        return true;
    }
    matches!(
        kind,
        "question"
            | "blocked"
            | "approval"
            | "failure"
            | "error"
            | "crash"
            | "failed"
            | "remediation"
            | "explicit"
            | "origin_result"
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotifyPolicy {
    pub notify_after: Duration,
    pub quiet_hours: Option<(u32, u32)>,
    pub mode: NotifyMode,
}

impl Default for NotifyPolicy {
    fn default() -> Self {
        Self {
            notify_after: DEFAULT_NOTIFY_AFTER,
            quiet_hours: Some(DEFAULT_QUIET_HOURS),
            mode: NotifyMode::Actionable,
        }
    }
}

impl NotifyPolicy {
    pub fn from_env() -> Self {
        let mut policy = Self::default();
        if let Some(secs) = std::env::var("FERRYX_HUB_NOTIFY_AFTER_SECS")
            .ok()
            .or_else(|| std::env::var("OMON_SESSION_NOTIFY_AFTER_SECS").ok())
            .and_then(|value| value.trim().parse::<u64>().ok())
        {
            policy.notify_after = Duration::from_secs(secs);
        }
        if let Ok(value) = std::env::var("FERRYX_HUB_QUIET_HOURS")
            .or_else(|_| std::env::var("OMON_SESSION_QUIET_HOURS"))
        {
            policy.quiet_hours = parse_quiet_hours(&value);
        }
        if let Some(mode) = notify_mode_from_env() {
            policy.mode = mode;
        }
        policy
    }

    pub fn is_quiet(self, now: DateTime<Utc>) -> bool {
        let Some((start, end)) = self.quiet_hours else {
            return false;
        };
        let hour = (now + KST).hour();
        if start <= end {
            (start..end).contains(&hour)
        } else {
            hour >= start || hour < end
        }
    }

    /// Instant at which a notice that became ready at `ready` may be sent.
    pub fn release_at(self, ready: DateTime<Utc>) -> DateTime<Utc> {
        if !self.is_quiet(ready) {
            return ready;
        }
        let Some((_, end)) = self.quiet_hours else {
            return ready;
        };
        let kst = ready + KST;
        let mut end_naive = kst.date_naive().and_hms_opt(end % 24, 0, 0).expect("hour");
        if kst.time() >= end_naive.time() {
            end_naive += TimeDelta::days(1);
        }
        DateTime::<Utc>::from_naive_utc_and_offset(end_naive, Utc) - KST
    }
}

fn parse_quiet_hours(value: &str) -> Option<(u32, u32)> {
    if value.trim().eq_ignore_ascii_case("off") {
        return None;
    }
    let (start, end) = value.trim().split_once('-')?;
    let start = start.trim().parse::<u32>().ok().filter(|hour| *hour < 24)?;
    let end = end.trim().parse::<u32>().ok().filter(|hour| *hour <= 24)?;
    (start != end).then_some((start, end % 24))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub dedupe_key: String,
    pub target: String,
    pub kind: String,
    pub text: String,
    /// The pending prompt behind a question or blocked notice, so the owner can answer it
    /// from the notice. None for a completion or a pane with no detected prompt.
    pub approval: Option<ApprovalCard>,
}

/// Plain text for an owner escalation that has no answer card. A prompt that already
/// has a card is posted once, as that card, so the owner does not also get
/// "approval … needs the owner".
pub fn escalation_plain_text(prompt_id: &str, target: &str, has_card: bool) -> Option<String> {
    if has_card {
        None
    } else {
        Some(format!("approval {prompt_id} on {target} needs the owner"))
    }
}

/// A detected prompt a notice can carry as buttons. An answer is checked against
/// `screen_revision`, so a card for a screen that moved on is refused, not misfired.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ApprovalCard {
    pub session_id: String,
    pub prompt_id: String,
    pub screen_revision: String,
    pub options: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Observation {
    pub target: String,
    pub machine: String,
    pub session_id: String,
    pub name: String,
    pub message_count: u64,
    pub streaming: bool,
    pub blocking_question: bool,
    pub question: Option<String>,
    pub last_assistant: String,
    pub watched: bool,
    pub machine_failed: bool,
    /// The turn ended in a failure, error, or crash. A streaming-to-idle edge then
    /// emits `failure` instead of `completion`.
    pub failed: bool,
    pub approval: Option<ApprovalCard>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObserveOutcome {
    pub due: Vec<Notice>,
    pub next_wake: Option<DateTime<Utc>>,
}

struct WatchState {
    streaming: HashMap<String, bool>,
    /// Notices waiting for their release time, with the time each was first seen.
    pending: HashMap<String, (Notice, DateTime<Utc>)>,
    sent_fallback: HashSet<String>,
}

pub struct Watcher {
    pool: SqlitePool,
    policy: NotifyPolicy,
    state: Mutex<WatchState>,
}

impl Watcher {
    pub fn new(pool: SqlitePool, policy: NotifyPolicy) -> Self {
        Self {
            pool,
            policy,
            state: Mutex::new(WatchState {
                streaming: HashMap::new(),
                pending: HashMap::new(),
                sent_fallback: HashSet::new(),
            }),
        }
    }

    pub fn mode(&self) -> NotifyMode {
        self.policy.mode
    }

    pub async fn on_observation(
        &self,
        obs: &Observation,
        now: DateTime<Utc>,
    ) -> Result<ObserveOutcome> {
        let mut state = self.state.lock().await;
        let was_streaming = state.streaming.get(&obs.target).copied();
        let notices = session_notices(obs, was_streaming);
        state.streaming.insert(obs.target.clone(), obs.streaming);

        // Observations arrive one session at a time, so this one only speaks for its own
        // target. A question or block that is no longer on this target's screen is withdrawn;
        // a completion or a failure already happened and stays until it is sent. Other
        // targets' pending notices are untouched.
        let present: HashSet<String> = notices
            .iter()
            .map(|notice| notice.dedupe_key.clone())
            .collect();
        let machine_failed = obs.machine_failed;
        state.pending.retain(|key, (notice, _)| {
            notice.target != obs.target
                || present.contains(key)
                || notice.kind == "completion"
                || notice.kind == "failure"
                || machine_failed
        });
        for notice in notices {
            if self.already_sent(&notice.dedupe_key).await?
                || state.sent_fallback.contains(&notice.dedupe_key)
            {
                continue;
            }
            // A held notice keeps its first-seen time but carries the latest prompt revision.
            let approval = notice.approval.clone();
            state
                .pending
                .entry(notice.dedupe_key.clone())
                .and_modify(|(held, _)| held.approval = approval)
                .or_insert((notice, now));
        }

        let mut due = Vec::new();
        let mut next_wake = None;
        let keys: Vec<String> = state.pending.keys().cloned().collect();
        for key in keys {
            let Some((_, first)) = state.pending.get(&key).cloned() else {
                continue;
            };
            let ready = self.policy.release_at(
                first + chrono::Duration::from_std(self.policy.notify_after).unwrap_or_default(),
            );
            if now >= ready {
                if let Some((notice, _)) = state.pending.remove(&key) {
                    due.push(notice);
                }
            } else {
                next_wake =
                    Some(next_wake.map_or(ready, |current: DateTime<Utc>| current.min(ready)));
            }
        }
        due.sort_by(|left, right| left.dedupe_key.cmp(&right.dedupe_key));
        Ok(ObserveOutcome { due, next_wake })
    }

    pub async fn is_watched(pool: &SqlitePool, target: &str) -> Result<bool> {
        let found: Option<String> =
            sqlx::query_scalar("SELECT target FROM session_watches WHERE target = ?")
                .bind(target)
                .fetch_optional(pool)
                .await?;
        Ok(found.is_some())
    }

    pub async fn deliver(&self, notice: &Notice, now: DateTime<Utc>) -> Result<bool> {
        if self.already_sent(&notice.dedupe_key).await? {
            return Ok(false);
        }
        let inserted = sqlx::query(
            "INSERT OR IGNORE INTO session_watch_notices(dedupe_key, target, kind, sent_at) VALUES(?,?,?,?)",
        )
        .bind(&notice.dedupe_key)
        .bind(&notice.target)
        .bind(&notice.kind)
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await;
        match inserted {
            Ok(done) => Ok(done.rows_affected() > 0),
            Err(_) => {
                self.state
                    .lock()
                    .await
                    .sent_fallback
                    .insert(notice.dedupe_key.clone());
                Ok(true)
            }
        }
    }

    async fn already_sent(&self, key: &str) -> Result<bool> {
        let found: Option<String> =
            sqlx::query_scalar("SELECT dedupe_key FROM session_watch_notices WHERE dedupe_key = ?")
                .bind(key)
                .fetch_optional(&self.pool)
                .await?;
        Ok(found.is_some())
    }
}

fn session_notices(obs: &Observation, was_streaming: Option<bool>) -> Vec<Notice> {
    let mut notices = Vec::new();
    let shown = format!("{}:{}", obs.machine, short_id(&obs.session_id));
    if obs.blocking_question && obs.question.is_none() {
        notices.push(Notice {
            dedupe_key: format!("b:{}:{}", obs.target, obs.message_count),
            target: obs.target.clone(),
            kind: "blocked".into(),
            text: format!(
                "⛔ {} · {}: waiting on a terminal question (`{shown}`)",
                obs.machine, obs.name
            ),
            approval: obs.approval.clone(),
        });
    }
    if let Some(question) = obs.question.as_deref() {
        let question = compact(&question.replace(['\n', '\r'], " "), 160);
        notices.push(Notice {
            dedupe_key: format!("q:{}:{}", obs.target, short_hash(&question)),
            target: obs.target.clone(),
            kind: "question".into(),
            text: format!("❓ {} · {}: {question}", obs.machine, obs.name),
            approval: obs.approval.clone(),
        });
    }
    if obs.watched && was_streaming == Some(true) && !obs.streaming {
        let (prefix, kind, headline) = if obs.failed {
            (
                "f",
                "failure",
                format!("⛔ {} · {} turn failed (`{shown}`)", obs.machine, obs.name),
            )
        } else {
            (
                "c",
                "completion",
                format!(
                    "✅ {} · {} turn complete (`{shown}`)",
                    obs.machine, obs.name
                ),
            )
        };
        let mut text = headline;
        let excerpt = compact(&obs.last_assistant.replace(['\n', '\r'], " "), 300);
        let excerpt = excerpt.trim();
        if !excerpt.is_empty() {
            text.push_str("\n↳ ");
            text.push_str(excerpt);
        }
        notices.push(Notice {
            dedupe_key: format!("{prefix}:{}:{}", obs.target, obs.message_count),
            target: obs.target.clone(),
            kind: kind.into(),
            text,
            approval: None,
        });
    }
    if obs.machine_failed {
        notices.push(Notice {
            dedupe_key: format!("f:{}", obs.target),
            target: obs.target.clone(),
            kind: "failure".into(),
            text: format!(
                "⛔ {} · {}: worker unavailable (`{shown}`)",
                obs.machine, obs.name
            ),
            approval: None,
        });
    }
    notices
}

pub fn short_hash(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn short_id(id: &str) -> &str {
    id.char_indices().nth(13).map_or(id, |(end, _)| &id[..end])
}

pub fn compact(value: &str, max: usize) -> String {
    value
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .take(max)
        .collect()
}

/// Machine is the second colon-separated segment of `{kind}:{target}:...`.
pub fn notice_machine(key: &str) -> Option<&str> {
    key.split_once(':')?
        .1
        .split_once(':')
        .map(|(machine, _)| machine)
}

pub fn retain_first_seen(
    first_seen: &mut HashMap<String, DateTime<Utc>>,
    present: &HashSet<&str>,
    failed_machines: &HashSet<&str>,
) {
    first_seen.retain(|key, _| {
        present.contains(key.as_str())
            || notice_machine(key).is_some_and(|machine| failed_machines.contains(machine))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn question(streaming: bool) -> Observation {
        Observation {
            target: "host-a:sess-1".into(),
            machine: "host-a".into(),
            session_id: "sess-1".into(),
            name: "agent".into(),
            message_count: 4,
            streaming,
            blocking_question: false,
            question: Some("ship it?".into()),
            last_assistant: "done".into(),
            watched: true,
            machine_failed: false,
            failed: false,
            approval: None,
        }
    }

    #[tokio::test]
    async fn one_question_and_one_completion_are_deduped_across_restart() {
        let pool = crate::storage::memory().await.unwrap();
        let policy = NotifyPolicy {
            notify_after: Duration::ZERO,
            quiet_hours: None,
            mode: NotifyMode::Actionable,
        };
        let now = DateTime::parse_from_rfc3339("2026-10-21T02:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let watcher = Watcher::new(pool.clone(), policy);
        let first = watcher.on_observation(&question(true), now).await.unwrap();
        assert_eq!(first.due.len(), 1);
        assert_eq!(first.due[0].kind, "question");
        assert!(watcher.deliver(&first.due[0], now).await.unwrap());

        let second = watcher.on_observation(&question(false), now).await.unwrap();
        assert_eq!(second.due.len(), 1);
        assert_eq!(second.due[0].kind, "completion");
        assert!(watcher.deliver(&second.due[0], now).await.unwrap());

        let restarted = Watcher::new(pool, policy);
        let again_q = restarted
            .on_observation(&question(true), now)
            .await
            .unwrap();
        assert!(again_q.due.is_empty(), "the question was already sent");
        let again_c = restarted
            .on_observation(&question(false), now)
            .await
            .unwrap();
        assert!(again_c.due.is_empty(), "the completion was already sent");
    }

    #[tokio::test]
    async fn another_sessions_event_does_not_reset_a_held_notice_and_a_completion_survives() {
        let pool = crate::storage::memory().await.unwrap();
        let policy = NotifyPolicy {
            notify_after: Duration::from_secs(60),
            quiet_hours: None,
            mode: NotifyMode::Actionable,
        };
        let start = DateTime::parse_from_rfc3339("2026-10-21T02:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let watcher = Watcher::new(pool, policy);
        let held = watcher
            .on_observation(&question(true), start)
            .await
            .unwrap();
        assert!(held.due.is_empty());

        let mut other = question(false);
        other.target = "host-a:sess-2".into();
        other.session_id = "sess-2".into();
        other.question = None;
        let later = start + TimeDelta::seconds(30);
        watcher.on_observation(&other, later).await.unwrap();

        // The first session's turn ends while its question is still up.
        let mut done = question(false);
        done.message_count = 5;
        let after_delay = start + TimeDelta::seconds(61);
        let outcome = watcher.on_observation(&done, after_delay).await.unwrap();
        let kinds: Vec<_> = outcome
            .due
            .iter()
            .map(|notice| notice.kind.as_str())
            .collect();
        assert_eq!(
            kinds,
            vec!["question"],
            "the question kept its first-seen time"
        );

        // Quiet again, the completion that was held is still released on time.
        let mut idle = done.clone();
        idle.question = None;
        let released = watcher
            .on_observation(&idle, after_delay + TimeDelta::seconds(61))
            .await
            .unwrap();
        let kinds: Vec<_> = released
            .due
            .iter()
            .map(|notice| notice.kind.as_str())
            .collect();
        assert_eq!(kinds, vec!["completion"]);
    }

    #[test]
    fn quiet_hours_hold_a_notice_until_08_kst() {
        let policy = NotifyPolicy::default();
        let quiet = DateTime::parse_from_rfc3339("2026-10-21T16:30:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(policy.is_quiet(quiet));
        let release = policy.release_at(quiet);
        assert_eq!(
            release,
            DateTime::parse_from_rfc3339("2026-10-21T23:00:00Z")
                .unwrap()
                .with_timezone(&Utc)
        );
    }

    #[test]
    fn a_card_is_the_only_notice_and_a_cardless_escalation_stays_plain_text() {
        assert!(escalation_plain_text("same-prompt", "local:pane", true).is_none());
        let plain = escalation_plain_text("same-prompt", "local:pane", false).unwrap();
        assert!(plain.contains("same-prompt"));
        assert!(plain.contains("local:pane"));
        assert!(plain.contains("needs the owner"));
    }

    #[test]
    fn actionable_mode_posts_questions_and_failures_and_holds_turn_complete() {
        assert!(owner_post(NotifyMode::Actionable, "question"));
        assert!(owner_post(NotifyMode::Actionable, "blocked"));
        assert!(owner_post(NotifyMode::Actionable, "failure"));
        assert!(owner_post(NotifyMode::Actionable, "remediation"));
        assert!(!owner_post(NotifyMode::Actionable, "completion"));
        assert!(!owner_post(NotifyMode::Actionable, "progress"));
        assert!(!owner_post(NotifyMode::Actionable, "working"));
        assert!(owner_post(NotifyMode::All, "completion"));
        assert!(owner_post(NotifyMode::All, "progress"));
        assert_eq!(NotifyMode::parse(" ALL "), Some(NotifyMode::All));
        assert_eq!(
            NotifyMode::parse("actionable"),
            Some(NotifyMode::Actionable)
        );
        assert!(NotifyMode::parse("verbose").is_none());
    }

    #[test]
    fn notice_machine_reads_the_second_segment() {
        assert_eq!(notice_machine("q:my:x:abc"), Some("my"));
        let mut seen = HashMap::new();
        let now = Utc::now();
        seen.insert("q:my:x:abc".into(), now);
        let present = HashSet::new();
        let failed = HashSet::from(["my"]);
        retain_first_seen(&mut seen, &present, &failed);
        assert_eq!(seen.len(), 1);
        let none = HashSet::new();
        retain_first_seen(&mut seen, &present, &none);
        assert!(seen.is_empty());
    }
}
