//! Slack Socket Mode frames. The websocket loop is compiled only with `hub-slack`.
//! Tokens come from `SLACK_APP_TOKEN` / `SLACK_BOT_TOKEN` or the matching `*_FILE` vars.
use serde_json::{json, Value};

use super::approval::slack_approval_op;
use super::router::Inbound;
use crate::agent_control::{AgentControl, AgentOp};
use crate::config::secret_from_env;
use crate::error::{HubError, Result};

pub const PLATFORM: &str = "slack";

#[derive(Debug, Clone, PartialEq)]
pub enum SlackFrame {
    Inbound(Inbound),
    Approval(AgentOp),
    Ignore,
}

pub fn app_token() -> Option<String> {
    secret_from_env("SLACK_APP_TOKEN")
}

pub fn bot_token() -> Option<String> {
    secret_from_env("SLACK_BOT_TOKEN")
}

/// Turns one Socket Mode text frame into an ack plus the inbound or approval it carries.
pub fn handle_frame(text: &str, owner_id: &str) -> Result<(Value, SlackFrame)> {
    let frame: Value = serde_json::from_str(text)
        .map_err(|error| HubError::message(format!("slack frame: {error}")))?;
    let envelope = frame
        .get("envelope_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    if envelope.is_empty() {
        return Err(HubError::message("slack frame has no envelope_id"));
    }
    let ack = json!({ "envelope_id": envelope });
    let kind = frame.get("type").and_then(Value::as_str).unwrap_or("");
    let payload = frame.get("payload").cloned().unwrap_or(Value::Null);
    let parsed = match kind {
        "events_api" => inbound_from_event(&payload)
            .map(SlackFrame::Inbound)
            .unwrap_or(SlackFrame::Ignore),
        "interactive" => match slack_approval_op(&payload, owner_id)? {
            Some(op) => SlackFrame::Approval(op),
            None => SlackFrame::Ignore,
        },
        _ => SlackFrame::Ignore,
    };
    Ok((ack, parsed))
}

fn inbound_from_event(payload: &Value) -> Option<Inbound> {
    let event = payload.get("event")?;
    if event.get("type").and_then(Value::as_str) != Some("message") {
        return None;
    }
    if event.get("bot_id").is_some()
        || event.get("subtype").and_then(Value::as_str) == Some("bot_message")
    {
        return None;
    }
    let text = event.get("text").and_then(Value::as_str)?.to_owned();
    if text.trim().is_empty() {
        return None;
    }
    Some(Inbound {
        platform: PLATFORM.into(),
        guild_id: None,
        channel_id: event.get("channel").and_then(Value::as_str)?.to_owned(),
        thread_id: event
            .get("thread_ts")
            .and_then(Value::as_str)
            .map(str::to_owned),
        user_id: event
            .get("user")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
        text,
        parent_chat_id: None,
        event_id: event
            .get("client_msg_id")
            .or_else(|| event.get("ts"))
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

/// Applies one Socket Mode frame. An approval is answered. An inbound message is fenced and
/// sent to the meta-agent pane, or to the lane pane a profile names. Nothing is read back.
pub async fn apply_frame(
    text: &str,
    owner_id: &str,
    meta_session_id: &str,
    profiles: &super::router::ProfileRouter,
    control: &dyn AgentControl,
) -> Result<SlackFrame> {
    let (_ack, frame) = handle_frame(text, owner_id)?;
    match &frame {
        SlackFrame::Approval(op) => {
            control.execute(None, op.clone()).await?;
        }
        SlackFrame::Inbound(inbound) => {
            super::router::route_inbound(inbound, profiles, meta_session_id, control).await?;
        }
        SlackFrame::Ignore => {}
    }
    Ok(frame)
}

/// The ack Slack requires, independent of whether we will act on the payload.
pub fn ack_for(text: &str) -> Result<Value> {
    let frame: Value = serde_json::from_str(text)
        .map_err(|error| HubError::message(format!("slack frame: {error}")))?;
    let envelope = frame
        .get("envelope_id")
        .and_then(Value::as_str)
        .unwrap_or("");
    if envelope.is_empty() {
        return Err(HubError::message("slack frame has no envelope_id"));
    }
    Ok(json!({ "envelope_id": envelope }))
}

/// Posts `text` when `SLACK_BOT_TOKEN` is set. No token reports that nothing was sent.
#[cfg(feature = "hub-slack")]
pub async fn post_if_configured(session: &super::session::SessionKey, text: &str) -> Result<bool> {
    let Some(token) = bot_token() else {
        return Ok(false);
    };
    let mut body = json!({ "channel": session.channel_id, "text": text });
    if let Some(thread) = &session.thread_id {
        body["thread_ts"] = json!(thread);
    }
    let client = reqwest::Client::new();
    let response = client
        .post("https://slack.com/api/chat.postMessage")
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .map_err(|error| HubError::message(error.to_string()))?;
    let value: Value = response
        .json()
        .await
        .map_err(|error| HubError::message(error.to_string()))?;
    if value.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err(HubError::message(format!(
            "slack chat.postMessage failed: {value}"
        )));
    }
    Ok(true)
}

pub async fn publish(session: &super::session::SessionKey, text: &str) -> Result<bool> {
    #[cfg(feature = "hub-slack")]
    {
        return post_if_configured(session, text).await;
    }
    #[cfg(not(feature = "hub-slack"))]
    {
        let _ = (session, text);
        Ok(false)
    }
}

#[cfg(feature = "hub-slack")]
pub async fn connect_socket_mode<F, Fut>(mut on_frame: F) -> Result<()>
where
    F: FnMut(String) -> Fut + Send,
    Fut: std::future::Future<Output = ()> + Send,
{
    let app = app_token().ok_or_else(|| HubError::config("SLACK_APP_TOKEN is not set"))?;
    let bot = bot_token().ok_or_else(|| HubError::config("SLACK_BOT_TOKEN is not set"))?;
    let client = reqwest::Client::new();
    let opened = client
        .post("https://slack.com/api/apps.connections.open")
        .bearer_auth(app)
        .send()
        .await
        .map_err(|error| HubError::message(error.to_string()))?;
    let body: Value = opened
        .json()
        .await
        .map_err(|error| HubError::message(error.to_string()))?;
    if body.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err(HubError::message(format!(
            "slack connections.open failed: {body}"
        )));
    }
    let url = body
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| HubError::message("slack socket url missing"))?;
    let (mut socket, _) = tokio_tungstenite::connect_async(url)
        .await
        .map_err(|error| HubError::message(error.to_string()))?;
    use futures_util::{SinkExt, StreamExt};
    let _ = bot;
    while let Some(message) = socket.next().await {
        let message = message.map_err(|error| HubError::message(error.to_string()))?;
        if let tokio_tungstenite::tungstenite::Message::Text(text) = message {
            if let Ok(ack) = ack_for(&text) {
                socket
                    .send(tokio_tungstenite::tungstenite::Message::Text(
                        ack.to_string().into(),
                    ))
                    .await
                    .map_err(|error| HubError::message(error.to_string()))?;
                on_frame(text.to_string()).await;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::RecordingControl;

    #[test]
    fn a_message_frame_acks_and_becomes_an_inbound() {
        let frame = r#"{"envelope_id":"e1","type":"events_api","payload":{"event":{"type":"message","channel":"C1","user":"U1","text":"hi"}}}"#;
        let (ack, parsed) = handle_frame(frame, "UOWNER").unwrap();
        assert_eq!(ack["envelope_id"], "e1");
        match parsed {
            SlackFrame::Inbound(inbound) => {
                assert_eq!(inbound.channel_id, "C1");
                assert_eq!(inbound.text, "hi");
            }
            other => panic!("expected inbound, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn an_approval_frame_answers_through_agent_control() {
        let frame = r#"{"envelope_id":"e2","type":"interactive","payload":{"type":"block_actions","user":{"id":"UOWNER"},"actions":[{"value":"ferryx:approval:pane-1:prompt-9:rev-42:0:once"}]}}"#;
        let control = RecordingControl::default();
        let parsed = apply_frame(
            frame,
            "UOWNER",
            "meta",
            &crate::chat::router::ProfileRouter::new(vec![]),
            &control,
        )
        .await
        .unwrap();
        assert!(matches!(parsed, SlackFrame::Approval(_)));
        assert!(matches!(control.ops()[0].1, AgentOp::Answer { .. }));
    }

    #[tokio::test]
    async fn an_inbound_frame_is_fenced_and_sent_to_the_meta_pane() {
        let frame = r#"{"envelope_id":"e3","type":"events_api","payload":{"event":{"type":"message","channel":"C1","user":"U1","text":"status?","ts":"1.2"}}}"#;
        let control = RecordingControl::default();
        apply_frame(
            frame,
            "UOWNER",
            "meta-pane",
            &crate::chat::router::ProfileRouter::new(vec![]),
            &control,
        )
        .await
        .unwrap();
        match &control.ops()[0].1 {
            AgentOp::Send {
                session_id,
                text,
                request_id,
            } => {
                assert_eq!(session_id, "meta-pane");
                assert!(text.contains("status?"));
                assert!(text.contains("origin: slack"));
                assert!(request_id.contains("1.2"));
            }
            other => panic!("expected a send, got {other:?}"),
        }
        assert!(control
            .ops()
            .iter()
            .all(|(_, op)| !matches!(op, AgentOp::Read { .. })));
    }
}
