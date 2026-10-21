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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotifyPolicy {
    pub notify_after: Duration,
    pub quiet_hours: Option<(u32, u32)>,
}

impl Default for NotifyPolicy {
    fn default() -> Self {
        Self {
            notify_after: DEFAULT_NOTIFY_AFTER,
            quiet_hours: Some(DEFAULT_QUIET_HOURS),
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObserveOutcome {
    pub due: Vec<Notice>,
    pub next_wake: Option<DateTime<Utc>>,
}

struct WatchState {
    streaming: HashMap<String, bool>,
    first_seen: HashMap<String, DateTime<Utc>>,
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
                first_seen: HashMap::new(),
                sent_fallback: HashSet::new(),
            }),
        }
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

        let mut present = HashSet::new();
        let mut due = Vec::new();
        let mut next_wake = None;
        for notice in notices {
            present.insert(notice.dedupe_key.clone());
            if self.already_sent(&notice.dedupe_key).await?
                || state.sent_fallback.contains(&notice.dedupe_key)
            {
                continue;
            }
            let first = *state
                .first_seen
                .entry(notice.dedupe_key.clone())
                .or_insert(now);
            let ready = self.policy.release_at(
                first + chrono::Duration::from_std(self.policy.notify_after).unwrap_or_default(),
            );
            if now >= ready {
                due.push(notice);
            } else {
                next_wake =
                    Some(next_wake.map_or(ready, |current: DateTime<Utc>| current.min(ready)));
            }
        }
        let failed = if obs.machine_failed {
            HashSet::from([obs.machine.as_str()])
        } else {
            HashSet::new()
        };
        let present_ref: HashSet<&str> = present.iter().map(String::as_str).collect();
        retain_first_seen(&mut state.first_seen, &present_ref, &failed);
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
        });
    }
    if let Some(question) = obs.question.as_deref() {
        let question = compact(&question.replace(['\n', '\r'], " "), 160);
        notices.push(Notice {
            dedupe_key: format!("q:{}:{}", obs.target, short_hash(&question)),
            target: obs.target.clone(),
            kind: "question".into(),
            text: format!("❓ {} · {}: {question}", obs.machine, obs.name),
        });
    }
    if obs.watched && was_streaming == Some(true) && !obs.streaming {
        notices.push(Notice {
            dedupe_key: format!("c:{}:{}", obs.target, obs.message_count),
            target: obs.target.clone(),
            kind: "completion".into(),
            text: format!(
                "✅ {} · {} turn complete (`{shown}`)",
                obs.machine, obs.name
            ),
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
        }
    }

    #[tokio::test]
    async fn one_question_and_one_completion_are_deduped_across_restart() {
        let pool = crate::storage::memory().await.unwrap();
        let policy = NotifyPolicy {
            notify_after: Duration::ZERO,
            quiet_hours: None,
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
