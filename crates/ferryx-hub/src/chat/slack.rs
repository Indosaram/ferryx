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

/// Slack Web API base. `FERRYX_HUB_SLACK_API` may point it at a loopback stand-in for a
/// local round-trip check; any other host is ignored so a token is never sent elsewhere.
pub fn api_base() -> String {
    std::env::var("FERRYX_HUB_SLACK_API")
        .ok()
        .and_then(|value| is_loopback_api(&value).then(|| value.trim_end_matches('/').to_owned()))
        .unwrap_or_else(|| "https://slack.com/api".into())
}

fn is_loopback_api(value: &str) -> bool {
    let rest = value
        .strip_prefix("http://")
        .or_else(|| value.strip_prefix("https://"))
        .unwrap_or("");
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = host.rsplit_once(':').map_or(host, |(name, port)| {
        if port.chars().all(|c| c.is_ascii_digit()) { name } else { host }
    });
    !host.contains('@') && matches!(host, "127.0.0.1" | "localhost" | "[::1]")
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

/// Block Kit for a notice with a detected prompt: the text, then one button per option.
/// Each button value is `ferryx:approval:{session}:{prompt}:{revision}:{index}:once`, which
/// `slack_approval_op` turns back into an Answer for the owner only. A part that would not
/// survive the `:` split yields no buttons, so a card is never answerable by the wrong value.
pub fn approval_blocks(text: &str, card: &crate::sessions::watch::ApprovalCard) -> Option<Value> {
    let parts = [&card.session_id, &card.prompt_id, &card.screen_revision];
    if card.options.is_empty() || parts.iter().any(|part| part.is_empty() || part.contains(':')) {
        return None;
    }
    let buttons: Vec<Value> = card
        .options
        .iter()
        .take(25)
        .enumerate()
        .map(|(index, label)| {
            let label: String = if label.trim().is_empty() {
                format!("Option {}", index + 1)
            } else {
                label.chars().take(75).collect()
            };
            json!({
                "type": "button",
                "action_id": format!("ferryx_approval_{index}"),
                "text": {"type": "plain_text", "text": label, "emoji": false},
                "value": format!(
                    "{}{}:{}:{}:{index}:once",
                    "ferryx:approval:", card.session_id, card.prompt_id, card.screen_revision
                ),
            })
        })
        .collect();
    let section: String = text.chars().take(2900).collect();
    Some(json!([
        {"type": "section", "text": {"type": "mrkdwn", "text": section}},
        {"type": "actions", "block_id": format!("ferryx_approval_{}", card.prompt_id), "elements": buttons},
    ]))
}

/// Posts a notice with answer buttons. Without valid buttons it is the plain text post.
pub async fn publish_card(
    session: &super::session::SessionKey,
    text: &str,
    card: &crate::sessions::watch::ApprovalCard,
) -> Result<bool> {
    #[cfg(feature = "hub-slack")]
    {
        return post_message(session, text, approval_blocks(text, card)).await;
    }
    #[cfg(not(feature = "hub-slack"))]
    {
        let _ = (session, text, card);
        Ok(false)
    }
}

/// Posts `text` when `SLACK_BOT_TOKEN` is set. No token reports that nothing was sent.
#[cfg(feature = "hub-slack")]
pub async fn post_if_configured(session: &super::session::SessionKey, text: &str) -> Result<bool> {
    post_message(session, text, None).await
}

#[cfg(feature = "hub-slack")]
async fn post_message(
    session: &super::session::SessionKey,
    text: &str,
    blocks: Option<Value>,
) -> Result<bool> {
    let Some(token) = bot_token() else {
        return Ok(false);
    };
    let mut body = json!({ "channel": session.channel_id, "text": text });
    if let Some(blocks) = blocks {
        body["blocks"] = blocks;
    }
    if let Some(thread) = &session.thread_id {
        body["thread_ts"] = json!(thread);
    }
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/chat.postMessage", api_base()))
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

/// Runs Socket Mode for as long as the hub is up. Slack closes every socket from time to
/// time (a `disconnect` frame, a refresh, a network drop), so each end opens a new one with
/// a capped backoff. Only a refused token (`invalid_auth` and friends) stops the loop.
#[cfg(feature = "hub-slack")]
pub async fn connect_socket_mode<F, Fut>(mut on_frame: F) -> Result<()>
where
    F: FnMut(String) -> Fut + Send,
    Fut: std::future::Future<Output = ()> + Send,
{
    let mut backoff = std::time::Duration::from_secs(1);
    loop {
        match socket_session(&mut on_frame).await {
            Ok(()) => backoff = std::time::Duration::from_secs(1),
            Err(error) if is_fatal_slack_error(&error.to_string()) => return Err(error),
            Err(error) => {
                tracing::warn!(%error, "slack socket mode dropped; reconnecting");
                backoff = (backoff * 2).min(std::time::Duration::from_secs(60));
            }
        }
        tokio::time::sleep(backoff).await;
    }
}

fn is_fatal_slack_error(message: &str) -> bool {
    ["is not set", "invalid_auth", "not_authed", "account_inactive", "token_revoked", "not_allowed_token_type"]
        .iter()
        .any(|needle| message.contains(needle))
}

/// One Socket Mode connection, until Slack asks to disconnect or the socket ends.
#[cfg(feature = "hub-slack")]
async fn socket_session<F, Fut>(on_frame: &mut F) -> Result<()>
where
    F: FnMut(String) -> Fut + Send,
    Fut: std::future::Future<Output = ()> + Send,
{
    let app = app_token().ok_or_else(|| HubError::config("SLACK_APP_TOKEN is not set"))?;
    bot_token().ok_or_else(|| HubError::config("SLACK_BOT_TOKEN is not set"))?;
    let client = reqwest::Client::new();
    let opened = client
        .post(format!("{}/apps.connections.open", api_base()))
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
    tracing::info!("slack socket mode connected");
    while let Some(message) = socket.next().await {
        let message = message.map_err(|error| HubError::message(error.to_string()))?;
        match message {
            tokio_tungstenite::tungstenite::Message::Text(text) => {
                if is_disconnect(&text) {
                    let _ = socket.close(None).await;
                    return Ok(());
                }
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
            tokio_tungstenite::tungstenite::Message::Ping(payload) => {
                let _ = socket
                    .send(tokio_tungstenite::tungstenite::Message::Pong(payload))
                    .await;
            }
            tokio_tungstenite::tungstenite::Message::Close(_) => return Ok(()),
            _ => {}
        }
    }
    Ok(())
}

/// Slack's `{"type":"disconnect"}` frame: this socket is about to close; open another.
pub fn is_disconnect(text: &str) -> bool {
    serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|frame| frame.get("type").and_then(Value::as_str).map(|kind| kind == "disconnect"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::RecordingControl;

    #[test]
    fn the_api_override_is_loopback_only() {
        assert!(is_loopback_api("http://127.0.0.1:9999/api"));
        assert!(is_loopback_api("http://localhost:1/api"));
        assert!(!is_loopback_api("https://evil.example/api"));
        assert!(!is_loopback_api("http://127.0.0.1@evil.example/api"));
        assert!(!is_loopback_api("http://127.0.0.1.evil.example/api"));
    }

    #[test]
    fn a_disconnect_frame_reconnects_and_a_refused_token_stops() {
        assert!(is_disconnect(r#"{"type":"disconnect","reason":"refresh_requested"}"#));
        assert!(!is_disconnect(r#"{"type":"hello"}"#));
        assert!(is_fatal_slack_error("slack connections.open failed: {\"ok\":false,\"error\":\"invalid_auth\"}"));
        assert!(!is_fatal_slack_error("connection reset by peer"));
    }

    #[tokio::test]
    async fn an_approval_card_button_round_trips_into_an_owner_answer() {
        let card = crate::sessions::watch::ApprovalCard {
            session_id: "pane-1".into(),
            prompt_id: "abc123".into(),
            screen_revision: "v1-rev".into(),
            options: vec!["Yes".into(), "No".into()],
        };
        let blocks = approval_blocks("❓ ship it?", &card).unwrap();
        let value = blocks[1]["elements"][1]["value"].as_str().unwrap().to_owned();
        assert_eq!(value, "ferryx:approval:pane-1:abc123:v1-rev:1:once");
        let frame = json!({"envelope_id":"e9","type":"interactive","payload":{"type":"block_actions",
            "user":{"id":"UOWNER"},"actions":[{"value": value}]}}).to_string();
        let control = RecordingControl::default();
        apply_frame(&frame, "UOWNER", "meta", &crate::chat::router::ProfileRouter::new(vec![]), &control)
            .await
            .unwrap();
        match &control.ops()[0].1 {
            AgentOp::Answer { session_id, prompt_id, screen_revision, answer, .. } => {
                assert_eq!((session_id.as_str(), prompt_id.as_str(), screen_revision.as_str()), ("pane-1", "abc123", "v1-rev"));
                assert_eq!(answer["optionIndex"], 1);
            }
            other => panic!("expected an answer, got {other:?}"),
        }
        // A stranger's click on the same card is refused and types nothing.
        let stranger = frame.replace("UOWNER", "USTRANGER");
        let control = RecordingControl::default();
        assert!(apply_frame(&stranger, "UOWNER", "meta", &crate::chat::router::ProfileRouter::new(vec![]), &control).await.is_err());
        assert!(control.ops().is_empty());
        // A part with ':' would be split wrong, so no buttons are offered.
        let bad = crate::sessions::watch::ApprovalCard { prompt_id: "a:b".into(), ..card };
        assert!(approval_blocks("x", &bad).is_none());
    }

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
