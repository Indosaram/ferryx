//! omo-gateway's Slack approval card for omo turns. With full access off, a tool request the
//! gateway policy would decline is put to the owner instead: a card in the session's Slack
//! thread with Allow Once / Allow Session / Always Allow / Deny (`omon:approval:{id}:{choice}`,
//! omo's values). The engine waits on the answer up to `APPROVAL_TIMEOUT_SECS` (omo: 1800s);
//! an unanswered card, or one whose turn ended, is marked expired. Only allowed users may
//! click; "Allow Session" covers the same action signature for the rest of the session and
//! "Always Allow" adds it to the approval allowlist.
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde_json::{json, Value};
use sqlx::SqlitePool;
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::meta::omo_client::{Approver, OwnerAnswer};

const PREFIX: &str = "omon:approval:";
pub const EXPIRED_TEXT: &str = "⌛ Approval expired";
pub const STALE_APPROVAL: &str =
    "이 승인 요청은 이미 처리됐거나 만료됐어요. 다시 누르지 않으셔도 돼요.";
pub const NOT_AUTHORIZED: &str = "You are not authorized to answer this approval request.";
const REASON_LIMIT: usize = 1500;
const COMMAND_LIMIT: usize = 2500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Once,
    Session,
    Always,
    Deny,
}

impl Choice {
    fn parse(raw: &str) -> Option<Self> {
        Some(match raw {
            "once" => Self::Once,
            "session" => Self::Session,
            "always" => Self::Always,
            "deny" => Self::Deny,
            _ => return None,
        })
    }

    pub fn label(self, user: &str) -> String {
        let label = match self {
            Self::Once => "✅ Approved once",
            Self::Session => "✅ Approved for this session",
            Self::Always => "✅ Always approved",
            Self::Deny => "❌ Denied",
        };
        format!("{label} by <@{user}>")
    }
}

struct Pending {
    answer: oneshot::Sender<Choice>,
}

fn pending() -> &'static Mutex<HashMap<Uuid, Pending>> {
    static PENDING: OnceLock<Mutex<HashMap<Uuid, Pending>>> = OnceLock::new();
    PENDING.get_or_init(Default::default)
}

fn session_grants() -> &'static Mutex<HashSet<(String, String)>> {
    static GRANTS: OnceLock<Mutex<HashSet<(String, String)>>> = OnceLock::new();
    GRANTS.get_or_init(Default::default)
}

pub fn timeout_from(raw: Option<&str>) -> Duration {
    Duration::from_secs(
        raw.and_then(|raw| raw.trim().parse::<u64>().ok())
            .filter(|secs| *secs > 0)
            .unwrap_or(1800),
    )
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn truncate(text: &str, limit: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= limit {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(limit).collect();
    out.push('…');
    out
}

/// omo's `approval_blocks`.
pub fn approval_blocks(id: Uuid, command: &str, reason: &str) -> Value {
    let reason = escape(&truncate(reason, REASON_LIMIT));
    let command = escape(&truncate(command, COMMAND_LIMIT)).replace("```", "`\u{200b}``");
    let button = |label: &str, choice: &str, style: Option<&str>| {
        let value = format!("{PREFIX}{id}:{choice}");
        let mut button = json!({
            "type": "button",
            "text": {"type": "plain_text", "text": label},
            "action_id": value,
            "value": value,
        });
        if let Some(style) = style {
            button["style"] = json!(style);
        }
        button
    };
    json!([
        {"type": "section", "text": {"type": "mrkdwn", "text": format!("*Approval needed*\n{reason}")}},
        {"type": "section", "text": {"type": "mrkdwn", "text": format!("```\n{command}\n```")}},
        {"type": "actions", "block_id": format!("{PREFIX}{id}"), "elements": [
            button("Allow Once", "once", Some("primary")),
            button("Allow Session", "session", Some("primary")),
            button("Always Allow", "always", None),
            button("Deny", "deny", Some("danger")),
        ]},
    ])
}

fn text_blocks(text: &str) -> Value {
    json!([{"type": "section", "text": {"type": "mrkdwn", "text": text}}])
}

async fn update_card(channel: &str, ts: &str, text: &str) {
    let body = json!({"channel": channel, "ts": ts, "text": text, "blocks": text_blocks(text)});
    if let Err(error) = super::slack_web::post_json("chat.update", &body).await {
        tracing::warn!(%error, "approval card was not updated");
    }
}

/// Removes the pending entry when the ask ends on any path (answer, timeout, the turn being
/// dropped), and expires a card nobody answered.
struct CardGuard {
    id: Uuid,
    channel: String,
    ts: String,
    answered: bool,
}

impl Drop for CardGuard {
    fn drop(&mut self) {
        pending()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .remove(&self.id);
        if !self.answered {
            let (channel, ts) = (self.channel.clone(), self.ts.clone());
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                let task = async move { update_card(&channel, &ts, EXPIRED_TEXT).await };
                #[cfg(test)]
                {
                    let _ = handle;
                    super::slack::spawn_carrying_test_api(task);
                }
                #[cfg(not(test))]
                handle.spawn(task);
            }
        }
    }
}

/// The approver of one Slack (or dashboard-on-Slack) session's turn.
pub struct SlackApprover {
    pub pool: SqlitePool,
    pub session_key: String,
    pub channel: String,
    pub thread: Option<String>,
    pub timeout: Duration,
}

impl SlackApprover {
    pub fn new(
        pool: SqlitePool,
        session_key: String,
        channel: String,
        thread: Option<String>,
    ) -> Self {
        Self {
            pool,
            session_key,
            channel,
            thread,
            timeout: timeout_from(crate::hub_env::var("APPROVAL_TIMEOUT_SECS").ok().as_deref()),
        }
    }
}

#[async_trait::async_trait]
impl Approver for SlackApprover {
    async fn ask(&self, frame: &Value, cwd: Option<&str>, reason: &str) -> OwnerAnswer {
        let classification = crate::effect::classify_frame(frame, cwd);
        let signature = classification.signature.clone();
        if session_grants()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .contains(&(self.session_key.clone(), signature.clone()))
            || crate::ledger::is_allowed(&self.pool, &signature)
                .await
                .unwrap_or(false)
        {
            return OwnerAnswer::Approve;
        }
        let engine_reason = frame
            .pointer("/params/reason")
            .and_then(Value::as_str)
            .unwrap_or(reason);
        let card_reason = format!(
            "{engine_reason}\neffect: {} | target: {}",
            classification.effect, classification.target
        );
        let id = Uuid::new_v4();
        let (tx, rx) = oneshot::channel();
        pending()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .insert(id, Pending { answer: tx });
        let mut body = json!({
            "channel": self.channel,
            "text": format!("Approval needed: {}", truncate(&classification.summary, 200)),
            "blocks": approval_blocks(id, &classification.summary, &card_reason),
        });
        if let Some(thread) = &self.thread {
            body["thread_ts"] = json!(thread);
        }
        let ts = match super::slack_web::post_json("chat.postMessage", &body).await {
            Ok(value) => value
                .get("ts")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            Err(error) => {
                pending()
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .remove(&id);
                tracing::warn!(%error, "approval card was not posted; declining");
                return OwnerAnswer::Unanswered;
            }
        };
        let mut guard = CardGuard {
            id,
            channel: self.channel.clone(),
            ts,
            answered: false,
        };
        let choice = match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(choice)) => choice,
            _ => return OwnerAnswer::Unanswered,
        };
        guard.answered = true;
        match choice {
            Choice::Deny => OwnerAnswer::Deny,
            Choice::Once => OwnerAnswer::Approve,
            Choice::Session => {
                session_grants()
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .insert((self.session_key.clone(), signature));
                OwnerAnswer::Approve
            }
            Choice::Always => {
                if let Err(error) = crate::ledger::allow(&self.pool, &signature).await {
                    tracing::warn!(%error, "always-allow was not stored");
                }
                OwnerAnswer::Approve
            }
        }
    }
}

fn approval_values(payload: &Value) -> Vec<&str> {
    payload
        .get("actions")
        .and_then(Value::as_array)
        .map(|actions| {
            actions
                .iter()
                .filter_map(|action| action.get("value").and_then(Value::as_str))
                .filter(|value| value.starts_with(PREFIX))
                .collect()
        })
        .unwrap_or_default()
}

fn parse_value(value: &str) -> Option<(Uuid, Choice)> {
    let (id, choice) = value.strip_prefix(PREFIX)?.split_once(':')?;
    Some((Uuid::parse_str(id).ok()?, Choice::parse(choice)?))
}

/// A `block_actions` click on one of these cards. Returns false when the payload carries no
/// card of an omo turn (the click belongs to someone else, e.g. a pane approval).
pub async fn handle_click(payload: &Value, allowed_users: &[String]) -> bool {
    let values = approval_values(payload);
    let ours: Vec<(Uuid, Choice)> = values.iter().filter_map(|v| parse_value(v)).collect();
    let known = {
        let pending = pending()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        ours.iter().any(|(id, _)| pending.contains_key(id))
    };
    if ours.is_empty() || (!known && payload.pointer("/ferryx/sessionId").is_some()) {
        return false;
    }
    let user = payload
        .pointer("/user/id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let channel = payload
        .pointer("/channel/id")
        .or_else(|| payload.pointer("/container/channel_id"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let ts = payload
        .pointer("/container/message_ts")
        .or_else(|| payload.pointer("/message/ts"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    if !known {
        // omo: an unknown or expired request id clears the stale card.
        if !channel.is_empty() && !ts.is_empty() {
            update_card(&channel, &ts, STALE_APPROVAL).await;
        }
        return true;
    }
    if user.is_empty() || !allowed_users.iter().any(|id| id == user) {
        tracing::warn!(user, "rejected Slack approval click from unauthorized user");
        if let Some(url) = payload.get("response_url").and_then(Value::as_str) {
            let _ = reqwest::Client::new()
                .post(url)
                .json(&json!({"response_type": "ephemeral", "replace_original": false, "text": NOT_AUTHORIZED}))
                .send()
                .await;
        }
        return true;
    }
    for (id, choice) in ours {
        let entry = pending()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .remove(&id);
        if let Some(entry) = entry {
            if entry.answer.send(choice).is_ok() && !channel.is_empty() && !ts.is_empty() {
                update_card(&channel, &ts, &choice.label(user)).await;
            }
            break;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cards_carry_omo_values_and_escape_text() {
        let id = Uuid::nil();
        let blocks = approval_blocks(id, "rm -rf <x> ```", "why & how");
        let elements = blocks[2]["elements"].as_array().unwrap();
        let values: Vec<&str> = elements
            .iter()
            .map(|e| e["value"].as_str().unwrap())
            .collect();
        assert_eq!(
            values,
            vec![
                format!("{PREFIX}{id}:once"),
                format!("{PREFIX}{id}:session"),
                format!("{PREFIX}{id}:always"),
                format!("{PREFIX}{id}:deny"),
            ]
        );
        assert!(blocks[0]["text"]["text"]
            .as_str()
            .unwrap()
            .contains("why &amp; how"));
        assert!(blocks[1]["text"]["text"]
            .as_str()
            .unwrap()
            .contains("&lt;x&gt;"));
        assert_eq!(
            Choice::Session.label("U1"),
            "✅ Approved for this session by <@U1>"
        );
        assert_eq!(timeout_from(None), Duration::from_secs(1800));
        assert_eq!(timeout_from(Some("5")), Duration::from_secs(5));
        assert!(parse_value(&format!("{PREFIX}{id}:bogus")).is_none());
    }
}
