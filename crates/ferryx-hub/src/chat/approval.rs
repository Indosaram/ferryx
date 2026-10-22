//! Approval buttons become one agent-control Answer. The screen revision travels with the click.
use serde_json::{json, Value};

use crate::agent_control::AgentOp;
use crate::error::{HubError, Result};

const FERRYX_PREFIX: &str = "ferryx:approval:";
const OMON_PREFIX: &str = "omon:approval:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalClick {
    pub session_id: String,
    pub prompt_id: String,
    pub screen_revision: String,
    pub option_index: u32,
    pub decision: String,
}

pub fn parse_approval_value(value: &str) -> Option<ApprovalClick> {
    if let Some(rest) = value.strip_prefix(FERRYX_PREFIX) {
        let mut parts = rest.split(':');
        let session_id = parts.next()?.to_owned();
        let prompt_id = parts.next()?.to_owned();
        let screen_revision = parts.next()?.to_owned();
        let option_index: u32 = parts.next()?.parse().ok()?;
        let decision = parts.next().unwrap_or("once").to_owned();
        if session_id.is_empty()
            || prompt_id.is_empty()
            || screen_revision.is_empty()
            || parts.next().is_some()
        {
            return None;
        }
        return Some(ApprovalClick {
            session_id,
            prompt_id,
            screen_revision,
            option_index,
            decision,
        });
    }
    None
}

/// `omon:approval:{id}:{decision}` plus the session and revision the card was rendered from.
pub fn parse_omon_approval(
    value: &str,
    session_id: &str,
    screen_revision: &str,
) -> Option<ApprovalClick> {
    let rest = value.strip_prefix(OMON_PREFIX)?;
    let (prompt_id, decision) = rest.split_once(':')?;
    if prompt_id.is_empty()
        || decision.is_empty()
        || session_id.is_empty()
        || screen_revision.is_empty()
    {
        return None;
    }
    let option_index = match decision {
        "once" | "approve" => 0,
        "session" => 1,
        "always" => 2,
        "deny" | "reject" => 3,
        _ => return None,
    };
    Some(ApprovalClick {
        session_id: session_id.to_owned(),
        prompt_id: prompt_id.to_owned(),
        screen_revision: screen_revision.to_owned(),
        option_index,
        decision: decision.to_owned(),
    })
}

pub fn answer_op(click: &ApprovalClick, request_id: &str) -> AgentOp {
    AgentOp::Answer {
        session_id: click.session_id.clone(),
        prompt_id: click.prompt_id.clone(),
        screen_revision: click.screen_revision.clone(),
        answer: json!({ "optionIndex": click.option_index }),
        request_id: request_id.to_owned(),
    }
}

/// Idempotency key for one prompt on one screen. The option is not part of the key, so
/// clicking the same option again replays and a different option conflicts as already answered.
/// An empty host is this machine.
pub fn approval_request_id(host_id: &str, click: &ApprovalClick) -> String {
    let host = {
        let trimmed = host_id.trim();
        if trimmed.is_empty() {
            "local"
        } else {
            trimmed
        }
    };
    format!(
        "slack-approval:{host}:{}:{}:{}",
        click.session_id, click.screen_revision, click.prompt_id
    )
}

/// How a failed answer should be explained to the person who clicked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickFailure {
    /// Same card, different option, or the card's answer was already sent.
    AlreadyAnswered,
    /// The first click with this id is still in the daemon.
    InFlight,
    /// The screen moved on; the prompt the card named is gone.
    Stale,
    /// Anything else. The card itself should show the error.
    Other,
}

pub fn click_failure_kind(message: &str) -> ClickFailure {
    if message.contains("already in flight") {
        ClickFailure::InFlight
    } else if message.contains("already used with a different payload")
        || message.contains("already sent")
        || message.contains("already answered")
    {
        ClickFailure::AlreadyAnswered
    } else if message.contains("screen changed") || message.contains("no longer on screen") {
        ClickFailure::Stale
    } else {
        ClickFailure::Other
    }
}

/// Slack `block_actions` from the owner. A stranger's click is refused and produces no op.
/// `host_id` is part of the answer's idempotency key.
pub fn slack_approval_op(
    payload: &Value,
    owner_id: &str,
    host_id: &str,
) -> Result<Option<AgentOp>> {
    if payload.get("type").and_then(Value::as_str) != Some("block_actions") {
        return Ok(None);
    }
    let user = payload
        .pointer("/user/id")
        .and_then(Value::as_str)
        .unwrap_or("");
    let values: Vec<&str> = payload
        .get("actions")
        .and_then(Value::as_array)
        .map(|actions| {
            actions
                .iter()
                .filter_map(|action| action.get("value").and_then(Value::as_str))
                .filter(|value| value.starts_with(FERRYX_PREFIX) || value.starts_with(OMON_PREFIX))
                .collect()
        })
        .unwrap_or_default();
    if values.is_empty() {
        return Ok(None);
    }
    if user.is_empty() || user != owner_id {
        return Err(HubError::message(
            "slack approval click is not from the owner",
        ));
    }
    let session_hint = payload
        .pointer("/ferryx/sessionId")
        .and_then(Value::as_str)
        .unwrap_or("");
    let revision_hint = payload
        .pointer("/ferryx/screenRevision")
        .and_then(Value::as_str)
        .unwrap_or("");
    for value in values {
        let click = parse_approval_value(value)
            .or_else(|| parse_omon_approval(value, session_hint, revision_hint));
        if let Some(click) = click {
            return Ok(Some(answer_op(
                &click,
                &approval_request_id(host_id, &click),
            )));
        }
    }
    Err(HubError::message("slack approval value is malformed"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_owner_click_becomes_an_answer_and_a_stranger_does_not() {
        let payload = json!({
            "type": "block_actions",
            "user": {"id": "UOWNER"},
            "actions": [{"value": "ferryx:approval:pane-1:prompt-9:rev-42:0:once"}]
        });
        let op = slack_approval_op(&payload, "UOWNER", "local")
            .unwrap()
            .unwrap();
        match op {
            AgentOp::Answer {
                session_id,
                prompt_id,
                screen_revision,
                answer,
                request_id,
            } => {
                assert_eq!(session_id, "pane-1");
                assert_eq!(prompt_id, "prompt-9");
                assert_eq!(screen_revision, "rev-42");
                assert_eq!(answer["optionIndex"], 0);
                assert_eq!(request_id, "slack-approval:local:pane-1:rev-42:prompt-9");
            }
            other => panic!("expected answer, got {other:?}"),
        }
        let stranger = json!({
            "type": "block_actions",
            "channel": {"id": "C0C9XG5FP8W"},
            "user": {"id": "USTRANGER"},
            "actions": [{"value": "ferryx:approval:pane-1:prompt-9:rev-42:0:once"}]
        });
        assert!(slack_approval_op(&stranger, "UOWNER", "local").is_err());
        let owner_in_channel = json!({
            "type": "block_actions",
            "channel": {"id": "C0C9XG5FP8W"},
            "user": {"id": "UOWNER"},
            "actions": [{"value": "ferryx:approval:pane-1:prompt-9:rev-42:0:once"}]
        });
        assert!(slack_approval_op(&owner_in_channel, "UOWNER", "").is_ok());
    }

    fn request_id_of(session: &str, revision: &str, host: &str, index: u32) -> String {
        let payload = json!({
            "type": "block_actions",
            "user": {"id": "UOWNER"},
            "actions": [{"value": format!("ferryx:approval:{session}:same-prompt:{revision}:{index}:once")}]
        });
        match slack_approval_op(&payload, "UOWNER", host)
            .unwrap()
            .unwrap()
        {
            AgentOp::Answer { request_id, .. } => request_id,
            other => panic!("expected answer, got {other:?}"),
        }
    }

    #[test]
    fn the_same_prompt_text_on_two_sessions_does_not_share_a_request_id() {
        let pane_a = request_id_of("pane-a", "rev-1", "local", 2);
        let pane_b = request_id_of("pane-b", "rev-1", "local", 2);
        let next_revision = request_id_of("pane-a", "rev-2", "local", 2);
        let other_host = request_id_of("pane-a", "rev-1", "host-b", 2);
        let same_option_again = request_id_of("pane-a", "rev-1", "local", 2);
        let other_option = request_id_of("pane-a", "rev-1", "local", 0);
        assert_ne!(pane_a, pane_b);
        assert_ne!(pane_a, next_revision);
        assert_ne!(pane_a, other_host);
        assert_eq!(pane_a, same_option_again);
        // The option is not part of the key: a repeat of the same option replays,
        // and a different option conflicts as already answered.
        assert_eq!(pane_a, other_option);
        assert_eq!(
            click_failure_kind(
                "REQUEST_CONFLICT this request id was already used with a different payload"
            ),
            ClickFailure::AlreadyAnswered
        );
        assert_eq!(
            click_failure_kind(
                "REQUEST_CONFLICT the screen changed since the card was read; nothing was sent"
            ),
            ClickFailure::Stale
        );
        assert_eq!(
            click_failure_kind("REQUEST_CONFLICT this request id is already in flight; its outcome must be resolved before it is reused"),
            ClickFailure::InFlight
        );
    }

    #[test]
    fn an_omon_decision_maps_onto_an_option_index() {
        let click = parse_omon_approval("omon:approval:prompt-9:deny", "pane-1", "rev-1").unwrap();
        assert_eq!(click.option_index, 3);
        assert!(parse_omon_approval("omon:approval:not:extra:once", "pane-1", "rev-1").is_none());
    }
}
