//! omo-gateway's turn status message (report/status.rs): while an owner turn runs, a status
//! message appears in its Slack thread after 3 minutes ("작업 중: <headline> · N분 · <tool>"),
//! is edited every 3 minutes and whenever the turn writes an interim progress note, and is
//! deleted when the turn ends with an answer or failure notice in the same conversation, so the
//! owner never keeps a bare "완료 · N분" line. Only a turn that ended without any message (a
//! stop) leaves its final line ("실패 · <reason>"). Lines are in the owner's language. The
//! owner's local 00:00-07:59 holds new posts and interim edits unless the owner wrote within
//! the last 10 minutes.
//! `OMON_STATUS_MESSAGE_ENABLED=0|false|no|off` turns it off. Only the owner's lead sessions
//! get one: a DM with the owner, or a `SLACK_HOME_CHANNEL` conversation.
//!
//! A status line is progress narration, an `internal` report
//! (docs/design/hub-owner-reporting.md section 4): it is never posted to the owner's DM. Only
//! home-channel conversations still show it.
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};

use super::session::SessionKey;

pub const STATUS_INTERVAL: Duration = Duration::from_secs(180);

tokio::task_local! {
    /// Test hook: a shorter status interval for turns run inside this scope.
    pub static INTERVAL_OVERRIDE: Duration;
}

pub fn interval() -> Duration {
    INTERVAL_OVERRIDE
        .try_with(|interval| *interval)
        .unwrap_or(STATUS_INTERVAL)
}
pub const TOOL_ACTIVITY_TTL: Duration = Duration::from_secs(600);
const OWNER_RECENT_SECS: i64 = 10 * 60;
const HEADLINE_CHARS: usize = 80;
const REASON_CHARS: usize = 200;
const PROGRESS_CHARS: usize = 300;

pub fn enabled() -> bool {
    !matches!(
        crate::hub_env::var("OMON_STATUS_MESSAGE_ENABLED")
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref(),
        Ok("0" | "false" | "no" | "off")
    )
}

pub fn in_quiet_hours(now: DateTime<Utc>) -> bool {
    crate::locale::current().local_hour(now) < 8
}

/// omo's `is_slack_owner_lead_session`.
pub fn is_owner_lead(key: &SessionKey, owner: &str) -> bool {
    if key.guild_id.is_none() && key.user_id == owner {
        return true;
    }
    crate::hub_env::var("SLACK_HOME_CHANNEL")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .any(|home| !home.is_empty() && home == key.channel_id)
}

fn tools() -> &'static Mutex<HashMap<String, (String, Instant)>> {
    static TOOLS: OnceLock<Mutex<HashMap<String, (String, Instant)>>> = OnceLock::new();
    TOOLS.get_or_init(Default::default)
}

/// The most recent tool a session's running turn started, shown in interim edits.
pub fn record_tool_activity(storage_key: &str, tool: &str) {
    let tool = tool.trim();
    if tool.is_empty() {
        return;
    }
    let now = Instant::now();
    let mut map = tools().lock().expect("tool activity");
    map.retain(|_, (_, at)| now.duration_since(*at) < TOOL_ACTIVITY_TTL);
    map.insert(
        storage_key.to_owned(),
        (tool.chars().take(60).collect(), now),
    );
}

pub fn clear_tool_activity(storage_key: &str) {
    tools().lock().expect("tool activity").remove(storage_key);
}

fn last_tool(storage_key: &str) -> Option<String> {
    let mut map = tools().lock().expect("tool activity");
    match map.get(storage_key) {
        Some((tool, at)) if at.elapsed() < TOOL_ACTIVITY_TTL => Some(tool.clone()),
        Some(_) => {
            map.remove(storage_key);
            None
        }
        None => None,
    }
}

/// Drops leading `[2026-10-23 09:00 KST]`-style stamps the prompt renderer adds. A lane-event
/// turn (`[LANE_EVENT]\n<lane> <kind>: <body>`) shows the first event's body, not the
/// internal lane and kind names.
fn headline(owner_text: &str) -> String {
    let mut text = owner_text.trim_start();
    while text.starts_with('[') {
        if let Some(events) = text.strip_prefix("[LANE_EVENT]") {
            let first = events.trim_start().lines().next().unwrap_or_default();
            let body = first.split_once(": ").map_or(first, |(_, body)| body);
            let label = crate::locale::current().pick("받은 알림 처리", "Handling alert");
            return clip(&format!("{label}: {body}"));
        }
        match text.find(']') {
            Some(end) => text = text[end + 1..].trim_start(),
            None => break,
        }
    }
    clip(text)
}

fn clip(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(HEADLINE_CHARS)
        .collect()
}

/// The running line: `작업 중: <headline> · N분[ · <tool>]` (English `Working on: ... · N min`),
/// then the turn's latest progress note on its own line.
fn working_line(headline: &str, minutes: i64, tool: Option<&str>, progress: Option<&str>) -> String {
    let mut body = if crate::locale::current().is_korean() {
        format!("작업 중: {headline} · {minutes}분")
    } else {
        format!("Working on: {headline} · {minutes} min")
    };
    if let Some(tool) = tool {
        body.push_str(&format!(" · {tool}"));
    }
    if let Some(progress) = progress {
        body.push('\n');
        body.push_str(progress);
    }
    body
}

/// The closing line a status message keeps when it is not deleted.
fn final_line(minutes: i64, outcome: &Result<(), String>) -> String {
    let korean = crate::locale::current().is_korean();
    match outcome {
        Ok(()) if korean => format!("완료 · {minutes}분"),
        Ok(()) => format!("Done · {minutes} min"),
        Err(reason) => {
            let reason: String = reason.chars().take(REASON_CHARS).collect();
            if korean { format!("실패 · {reason}") } else { format!("Failed · {reason}") }
        }
    }
}

/// A finished turn's status message is deleted when the turn succeeded or posted a message of
/// its own (answer or failure notice): a closing line next to it would be a bare status post.
fn deleted_on_finish(outcome: &Result<(), String>, answered: bool) -> bool {
    outcome.is_ok() || answered
}

/// The gate report for one status line: always `internal`.
pub fn status_report(storage_key: &str, body: &str) -> crate::report_gate::Report {
    crate::report_gate::Report::new(
        format!("turn-status:{storage_key}"),
        "turn_status",
        format!("turn-status:{storage_key}"),
        crate::report_gate::Salience::Internal,
        body,
    )
}

/// True when the status line may be posted: the gate keeps internal reports out of the owner
/// DM (a conversation with no guild).
fn may_post(storage_key: &str, body: &str) -> bool {
    let owner_dm =
        SessionKey::from_storage_key(storage_key).is_ok_and(|key| key.guild_id.is_none());
    !owner_dm
        || crate::report_gate::decide(&status_report(storage_key, body), None, None, false)
            == crate::report_gate::Decision::Sent
}

pub struct StatusMessage {
    storage_key: String,
    channel: String,
    thread: Option<String>,
    headline: String,
    started_at: DateTime<Utc>,
    owner_message_at: DateTime<Utc>,
    interval: chrono::Duration,
    ts: Option<String>,
    next_tick: i64,
    progress: Option<String>,
}

impl StatusMessage {
    pub fn new(
        storage_key: &str,
        channel: &str,
        thread: Option<&str>,
        owner_text: &str,
        now: DateTime<Utc>,
        interval: Duration,
    ) -> Self {
        clear_tool_activity(storage_key);
        Self {
            storage_key: storage_key.to_owned(),
            channel: channel.to_owned(),
            thread: thread.map(str::to_owned),
            headline: headline(owner_text),
            started_at: now,
            owner_message_at: now,
            interval: chrono::Duration::from_std(interval)
                .unwrap_or_else(|_| chrono::Duration::seconds(180)),
            ts: None,
            next_tick: 1,
            progress: None,
        }
    }

    pub fn next_due(&self) -> DateTime<Utc> {
        self.started_at + self.interval * self.next_tick as i32
    }

    fn held(&self, now: DateTime<Utc>) -> bool {
        in_quiet_hours(now) && (now - self.owner_message_at).num_seconds() > OWNER_RECENT_SECS
    }

    /// An interim progress note of the turn: it replaces the previous note in the status
    /// message instead of becoming a message of its own. A posted status message is edited at
    /// once (outside quiet hours); otherwise the next tick shows it.
    pub async fn note_progress(&mut self, text: &str, now: DateTime<Utc>) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        self.progress = Some(text.chars().take(PROGRESS_CHARS).collect());
        let Some(ts) = self.ts.clone() else {
            return;
        };
        if self.held(now) {
            return;
        }
        let minutes = (now - self.started_at).num_minutes().max(0);
        let tool = last_tool(&self.storage_key);
        let body = working_line(&self.headline, minutes, tool.as_deref(), self.progress.as_deref());
        if !may_post(&self.storage_key, &body) {
            return;
        }
        if let Err(error) = super::slack_web::update_message(&self.channel, &ts, &body).await {
            tracing::warn!(%error, "turn status message was not edited with progress");
        }
    }

    pub async fn tick(&mut self, now: DateTime<Utc>) {
        if now < self.next_due() {
            return;
        }
        let elapsed = (now - self.started_at).num_milliseconds().max(0);
        self.next_tick = elapsed / self.interval.num_milliseconds().max(1) + 1;
        if self.held(now) {
            return;
        }
        let minutes = (now - self.started_at).num_minutes().max(0);
        match self.ts.clone() {
            None => {
                let body = working_line(&self.headline, minutes, None, self.progress.as_deref());
                if !may_post(&self.storage_key, &body) {
                    return;
                }
                let mut message = serde_json::json!({"channel": self.channel, "text": body});
                if let Some(thread) = &self.thread {
                    message["thread_ts"] = serde_json::json!(thread);
                }
                match super::slack_web::post_json("chat.postMessage", &message).await {
                    Ok(value) => {
                        self.ts = value
                            .get("ts")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_owned)
                    }
                    Err(error) => tracing::warn!(%error, "turn status message was not posted"),
                }
            }
            Some(ts) => {
                let tool = last_tool(&self.storage_key);
                let body = working_line(&self.headline, minutes, tool.as_deref(), self.progress.as_deref());
                if !may_post(&self.storage_key, &body) {
                    return;
                }
                if let Err(error) =
                    super::slack_web::update_message(&self.channel, &ts, &body).await
                {
                    tracing::warn!(%error, "turn status message was not edited");
                }
            }
        }
    }

    /// Ends the status message; a turn that never posted sends nothing. When `answered` (the
    /// turn posted its answer or a failure notice) or the turn succeeded, the message is
    /// deleted; otherwise, or when the delete fails, it is edited to its closing line. Allowed
    /// in quiet hours.
    pub async fn finish(self, now: DateTime<Utc>, outcome: Result<(), String>, answered: bool) {
        clear_tool_activity(&self.storage_key);
        let Some(ts) = self.ts else {
            return;
        };
        if deleted_on_finish(&outcome, answered) {
            let message = serde_json::json!({"channel": self.channel, "ts": ts});
            match super::slack_web::post_json("chat.delete", &message).await {
                Ok(_) => return,
                Err(error) => tracing::warn!(%error, "turn status message was not deleted"),
            }
        }
        let minutes = (now - self.started_at).num_minutes().max(0);
        let body = final_line(minutes, &outcome);
        if let Err(error) = super::slack_web::update_message(&self.channel, &ts, &body).await {
            tracing::warn!(%error, "turn status message was not finished");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_lead_sessions_quiet_hours_and_headlines_follow_omo() {
        let dm = SessionKey::new("slack", None::<String>, "D1", None::<String>, "UOWNER");
        assert!(is_owner_lead(&dm, "UOWNER"));
        let other = SessionKey::new("slack", Some("T1"), "CRANDOM", Some("1.1"), "U2");
        assert!(!is_owner_lead(&other, "UOWNER"));
        let night = DateTime::parse_from_rfc3339("2026-10-23T03:00:00+09:00")
            .unwrap()
            .with_timezone(&Utc);
        let day = DateTime::parse_from_rfc3339("2026-10-23T09:00:00+09:00")
            .unwrap()
            .with_timezone(&Utc);
        assert!(in_quiet_hours(night));
        assert!(!in_quiet_hours(day));
        assert_eq!(
            headline("[2026-10-23 09:00 KST]  deploy   the\nhub"),
            "deploy the hub"
        );
        assert_eq!(
            headline("[LANE_EVENT]\n- action_loop: [카톡 · 봇 처리] 여신님: 번역 가능?\n- note: x"),
            "받은 알림 처리: [카톡 · 봇 처리] 여신님: 번역 가능?"
        );
        record_tool_activity("k", "commandExecution");
        assert_eq!(last_tool("k").as_deref(), Some("commandExecution"));
        clear_tool_activity("k");
        assert_eq!(last_tool("k"), None);
    }

    #[test]
    fn status_lines_are_internal_and_stay_out_of_the_owner_dm() {
        let dm = SessionKey::new("slack", None::<String>, "D1", None::<String>, "UOWNER");
        let home = SessionKey::new("slack", Some("T1"), "CHOME", Some("1.1"), "UOWNER");
        let body = "작업 중: deploy the hub · 3분";
        assert_eq!(
            crate::report_gate::decide(&status_report(&dm.storage_key(), body), None, None, false),
            crate::report_gate::Decision::Internal
        );
        assert!(!may_post(&dm.storage_key(), body));
        assert!(may_post(&home.storage_key(), body));
    }

    #[test]
    fn status_lines_follow_the_owner_locale_and_end_without_a_bare_status_post() {
        assert_eq!(working_line("deploy", 3, Some("read"), None), "작업 중: deploy · 3분 · read");
        assert_eq!(final_line(5, &Ok(())), "완료 · 5분");
        assert_eq!(final_line(5, &Err("boom".into())), "실패 · boom");
        // A succeeded turn, or one that posted its own answer or notice, deletes the line.
        assert!(deleted_on_finish(&Ok(()), false));
        assert!(deleted_on_finish(&Err("boom".into()), true));
        assert!(!deleted_on_finish(&Err("stopped by the owner".into()), false));
        crate::locale::with(crate::locale::OwnerLocale::english_utc(), || {
            assert_eq!(
                working_line("deploy", 3, None, Some("Checking the logs.")),
                "Working on: deploy · 3 min\nChecking the logs."
            );
            assert_eq!(final_line(5, &Ok(())), "Done · 5 min");
            assert_eq!(final_line(5, &Err("boom".into())), "Failed · boom");
            assert_eq!(
                headline("[LANE_EVENT]\n- action_loop: CI failed"),
                "Handling alert: CI failed"
            );
            // Quiet hours follow the owner's zone: 03:00 UTC is quiet in UTC, not in Seoul.
            let at = DateTime::parse_from_rfc3339("2026-10-23T03:00:00Z").unwrap().with_timezone(&Utc);
            assert!(in_quiet_hours(at));
        });
        let at = DateTime::parse_from_rfc3339("2026-10-23T03:00:00Z").unwrap().with_timezone(&Utc);
        assert!(!in_quiet_hours(at));
    }

    #[tokio::test]
    async fn progress_notes_edit_the_status_message_instead_of_posting() {
        let (base, calls) = super::super::slack_web::mock::serve("UBOT").await;
        let home = SessionKey::new("slack", Some("T1"), "CHOME", Some("1.1"), "");
        let start = DateTime::parse_from_rfc3339("2026-10-23T12:00:00+09:00").unwrap().with_timezone(&Utc);
        super::super::slack::with_test_api(base, "xoxb-test".into(), async {
            let mut status = StatusMessage::new(&home.storage_key(), "CHOME", Some("1.1"), "find page 18", start, STATUS_INTERVAL);
            // Before the status message exists, a note is only remembered.
            status.note_progress("찾아보겠습니다.", start).await;
            assert!(calls.lock().unwrap().is_empty());
            status.tick(start + chrono::Duration::minutes(3)).await;
            for note in ["확인하겠습니다.", "검색 중입니다."] {
                status.note_progress(note, start + chrono::Duration::minutes(4)).await;
            }
            status.finish(start + chrono::Duration::minutes(6), Ok(()), true).await;
        })
        .await;
        let calls = calls.lock().unwrap().clone();
        let methods: Vec<&str> = calls.iter().map(|(method, _)| method.as_str()).collect();
        assert_eq!(methods, ["chat.postMessage", "chat.update", "chat.update", "chat.delete"], "{calls:?}");
        assert_eq!(calls[0].1["text"], "작업 중: find page 18 · 3분\n찾아보겠습니다.");
        assert_eq!(calls[2].1["text"], "작업 중: find page 18 · 4분\n검색 중입니다.");
        assert_eq!(calls[3].1["ts"], calls[1].1["ts"]);
    }
}
