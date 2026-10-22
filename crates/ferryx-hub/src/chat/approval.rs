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

/// Slack `block_actions` from the owner. A stranger's click is refused and produces no op.
pub fn slack_approval_op(payload: &Value, owner_id: &str) -> Result<Option<AgentOp>> {
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
                &format!("slack-approval-{}", click.prompt_id),
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
        let op = slack_approval_op(&payload, "UOWNER").unwrap().unwrap();
        match op {
            AgentOp::Answer {
                session_id,
                prompt_id,
                screen_revision,
                answer,
                ..
            } => {
                assert_eq!(session_id, "pane-1");
                assert_eq!(prompt_id, "prompt-9");
                assert_eq!(screen_revision, "rev-42");
                assert_eq!(answer["optionIndex"], 0);
            }
            other => panic!("expected answer, got {other:?}"),
        }
        let stranger = json!({
            "type": "block_actions",
            "channel": {"id": "C0C9XG5FP8W"},
            "user": {"id": "USTRANGER"},
            "actions": [{"value": "ferryx:approval:pane-1:prompt-9:rev-42:0:once"}]
        });
        assert!(slack_approval_op(&stranger, "UOWNER").is_err());
        let owner_in_channel = json!({
            "type": "block_actions",
            "channel": {"id": "C0C9XG5FP8W"},
            "user": {"id": "UOWNER"},
            "actions": [{"value": "ferryx:approval:pane-1:prompt-9:rev-42:0:once"}]
        });
        assert!(slack_approval_op(&owner_in_channel, "UOWNER").is_ok());
    }

    #[test]
    fn an_omon_decision_maps_onto_an_option_index() {
        let click = parse_omon_approval("omon:approval:prompt-9:deny", "pane-1", "rev-1").unwrap();
        assert_eq!(click.option_index, 3);
        assert!(parse_omon_approval("omon:approval:not:extra:once", "pane-1", "rev-1").is_none());
    }
}
