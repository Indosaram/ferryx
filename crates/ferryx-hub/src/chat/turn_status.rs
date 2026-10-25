//! omo-gateway's turn status message (report/status.rs): while an owner turn runs, a status
//! message appears in its Slack thread after 3 minutes ("작업 중: <headline> · N분 · <tool>"),
//! is edited every 3 minutes, and ends as "완료 · N분" or "실패 · <reason>". KST 00:00-07:59
//! holds new posts and interim edits unless the owner wrote within the last 10 minutes.
//! `OMON_STATUS_MESSAGE_ENABLED=0|false|no|off` turns it off. Only the owner's lead sessions
//! get one: a DM with the owner, or a `SLACK_HOME_CHANNEL` conversation.
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use chrono::{DateTime, Timelike, Utc};

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

pub fn enabled() -> bool {
    !matches!(
        crate::hub_env::var("OMON_STATUS_MESSAGE_ENABLED")
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref(),
        Ok("0" | "false" | "no" | "off")
    )
}

pub fn in_quiet_hours(now: DateTime<Utc>) -> bool {
    now.with_timezone(&chrono_tz::Asia::Seoul).hour() < 8
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
            return clip(&format!("받은 알림 처리: {body}"));
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
        }
    }

    pub fn next_due(&self) -> DateTime<Utc> {
        self.started_at + self.interval * self.next_tick as i32
    }

    fn held(&self, now: DateTime<Utc>) -> bool {
        in_quiet_hours(now) && (now - self.owner_message_at).num_seconds() > OWNER_RECENT_SECS
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
        let mut body = format!("작업 중: {} · {minutes}분", self.headline);
        match self.ts.clone() {
            None => {
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
                if let Some(tool) = last_tool(&self.storage_key) {
                    body.push_str(&format!(" · {tool}"));
                }
                if let Err(error) =
                    super::slack_web::update_message(&self.channel, &ts, &body).await
                {
                    tracing::warn!(%error, "turn status message was not edited");
                }
            }
        }
    }

    /// The final edit; a turn that never posted sends nothing. Allowed in quiet hours.
    pub async fn finish(self, now: DateTime<Utc>, outcome: Result<(), String>) {
        clear_tool_activity(&self.storage_key);
        let Some(ts) = self.ts else {
            return;
        };
        let minutes = (now - self.started_at).num_minutes().max(0);
        let body = match outcome {
            Ok(()) => format!("완료 · {minutes}분"),
            Err(reason) => format!(
                "실패 · {}",
                reason.chars().take(REASON_CHARS).collect::<String>()
            ),
        };
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
}
