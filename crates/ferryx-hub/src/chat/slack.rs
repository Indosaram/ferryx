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
        if port.chars().all(|c| c.is_ascii_digit()) {
            name
        } else {
            host
        }
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
    if card.options.is_empty()
        || parts
            .iter()
            .any(|part| part.is_empty() || part.contains(':'))
    {
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
///
/// Each `apps.connections.open` and each websocket handshake gives up after 15s. A live
/// socket with no frame for 15s is pinged, and if nothing comes back within 30s of the last
/// frame the socket is dead and the loop reconnects. A `disconnect` frame
/// (`refresh_requested`, `warning`, or any other reason) opens the replacement while the old
/// socket is still read, and closes the old one only after the new handshake finishes.
#[cfg(feature = "hub-slack")]
pub async fn connect_socket_mode<F, Fut>(on_frame: F) -> Result<()>
where
    F: FnMut(String) -> Fut + Send,
    Fut: std::future::Future<Output = ()> + Send,
{
    run_socket_mode(credentials_from_env, SocketPace::production(), on_frame).await
}

fn is_fatal_slack_error(message: &str) -> bool {
    [
        "is not set",
        "invalid_auth",
        "not_authed",
        "account_inactive",
        "token_revoked",
        "not_allowed_token_type",
    ]
    .iter()
    .any(|needle| message.contains(needle))
}

#[cfg(feature = "hub-slack")]
use futures_util::{SinkExt, StreamExt};
#[cfg(feature = "hub-slack")]
use std::time::{Duration, Instant};
#[cfg(feature = "hub-slack")]
use tokio_tungstenite::tungstenite::Message;

#[cfg(feature = "hub-slack")]
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
#[cfg(feature = "hub-slack")]
const PING_AFTER: Duration = Duration::from_secs(15);
#[cfg(feature = "hub-slack")]
const READ_IDLE: Duration = Duration::from_secs(30);
#[cfg(feature = "hub-slack")]
const BACKOFF_INITIAL: Duration = Duration::from_secs(1);
#[cfg(feature = "hub-slack")]
const BACKOFF_CAP: Duration = Duration::from_secs(60);

#[cfg(feature = "hub-slack")]
#[derive(Clone, Copy)]
struct SocketPace {
    connect_timeout: Duration,
    ping_after: Duration,
    read_idle: Duration,
    backoff_initial: Duration,
    backoff_cap: Duration,
}

#[cfg(feature = "hub-slack")]
impl SocketPace {
    fn production() -> Self {
        Self {
            connect_timeout: CONNECT_TIMEOUT,
            ping_after: PING_AFTER,
            read_idle: READ_IDLE,
            backoff_initial: BACKOFF_INITIAL,
            backoff_cap: BACKOFF_CAP,
        }
    }
}

#[cfg(feature = "hub-slack")]
#[derive(Clone)]
struct SocketCredentials {
    api_base: String,
    app_token: String,
    bot_token: String,
}

#[cfg(feature = "hub-slack")]
fn credentials_from_env() -> Result<SocketCredentials> {
    Ok(SocketCredentials {
        api_base: api_base(),
        app_token: app_token().ok_or_else(|| HubError::config("SLACK_APP_TOKEN is not set"))?,
        bot_token: bot_token().ok_or_else(|| HubError::config("SLACK_BOT_TOKEN is not set"))?,
    })
}

#[cfg(feature = "hub-slack")]
type SlackSocket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

#[cfg(feature = "hub-slack")]
enum SocketEvent {
    Continue,
    Replace(String),
    Closed,
}

#[cfg(feature = "hub-slack")]
async fn run_socket_mode<F, Fut, C>(
    mut credentials: C,
    pace: SocketPace,
    mut on_frame: F,
) -> Result<()>
where
    C: FnMut() -> Result<SocketCredentials> + Send,
    F: FnMut(String) -> Fut + Send,
    Fut: std::future::Future<Output = ()> + Send,
{
    let mut backoff = pace.backoff_initial;
    let mut attempt = 0u64;
    loop {
        match drive_session(&mut credentials, pace, &mut attempt, &mut on_frame).await {
            Ok(()) => {
                tracing::info!(attempt, "slack socket mode session ended; reconnecting");
                backoff = pace.backoff_initial;
            }
            Err(error) if is_fatal_slack_error(&error.to_string()) => return Err(error),
            Err(error) => {
                tracing::warn!(attempt, %error, "slack socket mode dropped; reconnecting");
                backoff = (backoff * 2).min(pace.backoff_cap);
                tokio::time::sleep(backoff).await;
                continue;
            }
        }
        tokio::time::sleep(backoff).await;
    }
}

/// One socket, replaced in place when Slack sends `disconnect`.
#[cfg(feature = "hub-slack")]
async fn drive_session<F, Fut, C>(
    credentials: &mut C,
    pace: SocketPace,
    attempt: &mut u64,
    on_frame: &mut F,
) -> Result<()>
where
    C: FnMut() -> Result<SocketCredentials>,
    F: FnMut(String) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let mut socket = connect_once(&credentials()?, pace, attempt).await?;
    let mut last_rx = Instant::now();
    let mut ping_sent = false;
    loop {
        let elapsed = last_rx.elapsed();
        let wait = if ping_sent {
            pace.read_idle.saturating_sub(elapsed)
        } else {
            pace.ping_after.min(pace.read_idle).saturating_sub(elapsed)
        };
        tokio::select! {
            biased;
            incoming = socket.next() => {
                let message = match incoming {
                    Some(Ok(message)) => message,
                    Some(Err(error)) => return Err(HubError::message(error.to_string())),
                    None => return Ok(()),
                };
                ping_sent = false;
                match apply_socket_message(&mut socket, message, pace, on_frame).await? {
                    SocketEvent::Replace(reason) => {
                        tracing::info!(%reason, attempt, "slack socket mode disconnect; opening a replacement");
                        let creds = credentials()?;
                        socket = replace_socket(socket, &creds, pace, attempt, on_frame).await?;
                        // Idle is silence on the socket, not time spent applying a frame.
                        last_rx = Instant::now();
                        ping_sent = false;
                    }
                    SocketEvent::Closed => return Ok(()),
                    SocketEvent::Continue => last_rx = Instant::now(),
                }
            }
            _ = tokio::time::sleep(wait) => {
                let elapsed = last_rx.elapsed();
                if ping_sent || elapsed >= pace.read_idle {
                    return Err(HubError::message(
                        "slack socket idle: no frame or pong within the read deadline",
                    ));
                }
                let write_budget = pace.connect_timeout.min(pace.read_idle.saturating_sub(elapsed));
                let write_budget = if write_budget.is_zero() {
                    Duration::from_millis(1)
                } else {
                    write_budget
                };
                send_timeout(&mut socket, Message::Ping(b"ferryx"[..].into()), write_budget).await?;
                ping_sent = true;
            }
        }
    }
}

/// Opens the next socket while `old` is still read, then closes `old`.
#[cfg(feature = "hub-slack")]
async fn replace_socket<F, Fut>(
    mut old: SlackSocket,
    credentials: &SocketCredentials,
    pace: SocketPace,
    attempt: &mut u64,
    on_frame: &mut F,
) -> Result<SlackSocket>
where
    F: FnMut(String) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let mut pending = std::pin::pin!(connect_once(credentials, pace, attempt));
    let mut handed_off_frames = 0u32;
    loop {
        tokio::select! {
            biased;
            // Keep acking the old socket until the replacement handshake finishes,
            // but don't let a backlog postpone the new socket forever.
            incoming = old.next(), if handed_off_frames < 32 => {
                handed_off_frames = handed_off_frames.saturating_add(1);
                match incoming {
                    Some(Ok(message)) => match apply_socket_message(&mut old, message, pace, on_frame).await? {
                        SocketEvent::Closed => return pending.await,
                        SocketEvent::Continue | SocketEvent::Replace(_) => {}
                    },
                    Some(Err(_)) | None => return pending.await,
                }
            }
            connected = &mut pending => {
                let new_socket = connected?;
                let _ = tokio::time::timeout(pace.connect_timeout, old.close(None)).await;
                return Ok(new_socket);
            }
        }
    }
}

#[cfg(feature = "hub-slack")]
async fn connect_once(
    credentials: &SocketCredentials,
    pace: SocketPace,
    attempt: &mut u64,
) -> Result<SlackSocket> {
    *attempt = attempt.saturating_add(1);
    let attempt = *attempt;
    if credentials.app_token.is_empty() {
        return Err(HubError::config("SLACK_APP_TOKEN is not set"));
    }
    if credentials.bot_token.is_empty() {
        return Err(HubError::config("SLACK_BOT_TOKEN is not set"));
    }
    tracing::info!(attempt, "slack socket mode connecting");
    let url = match tokio::time::timeout(
        pace.connect_timeout,
        open_socket_url(credentials, pace.connect_timeout),
    )
    .await
    {
        Err(_) => return Err(HubError::message("slack connections.open timed out")),
        Ok(result) => result?,
    };
    let socket =
        match tokio::time::timeout(pace.connect_timeout, tokio_tungstenite::connect_async(&url))
            .await
        {
            Err(_) => return Err(HubError::message("slack websocket handshake timed out")),
            Ok(result) => {
                result
                    .map_err(|error| HubError::message(error.to_string()))?
                    .0
            }
        };
    tracing::info!(attempt, "slack socket mode connected");
    Ok(socket)
}

#[cfg(feature = "hub-slack")]
async fn open_socket_url(credentials: &SocketCredentials, timeout: Duration) -> Result<String> {
    let mut builder = reqwest::Client::builder()
        .connect_timeout(timeout)
        .timeout(timeout);
    if is_loopback_api(&credentials.api_base) {
        builder = builder.no_proxy();
    }
    let client = builder
        .build()
        .map_err(|error| HubError::message(error.to_string()))?;
    let opened = client
        .post(format!(
            "{}/apps.connections.open",
            credentials.api_base.trim_end_matches('/')
        ))
        .bearer_auth(&credentials.app_token)
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
    body.get("url")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| HubError::message("slack socket url missing"))
}

#[cfg(feature = "hub-slack")]
async fn apply_socket_message<F, Fut>(
    socket: &mut SlackSocket,
    message: Message,
    pace: SocketPace,
    on_frame: &mut F,
) -> Result<SocketEvent>
where
    F: FnMut(String) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    match message {
        Message::Text(text) => {
            let text = text.to_string();
            if let Some(reason) = disconnect_reason(&text) {
                return Ok(SocketEvent::Replace(reason));
            }
            if let Ok(ack) = ack_for(&text) {
                send_timeout(
                    socket,
                    Message::Text(ack.to_string().into()),
                    pace.connect_timeout,
                )
                .await?;
                on_frame(text).await;
            }
            Ok(SocketEvent::Continue)
        }
        Message::Ping(payload) => {
            send_timeout(socket, Message::Pong(payload), pace.connect_timeout).await?;
            Ok(SocketEvent::Continue)
        }
        Message::Close(_) => Ok(SocketEvent::Closed),
        _ => Ok(SocketEvent::Continue),
    }
}

#[cfg(feature = "hub-slack")]
async fn send_timeout(socket: &mut SlackSocket, message: Message, timeout: Duration) -> Result<()> {
    tokio::time::timeout(timeout, socket.send(message))
        .await
        .map_err(|_| HubError::message("slack socket write timed out"))?
        .map_err(|error| HubError::message(error.to_string()))
}

/// Reason on Slack's `{"type":"disconnect"}` frame, when this socket is about to close.
/// `refresh_requested` and `warning` are the routine ones; an empty reason is still a disconnect.
pub fn disconnect_reason(text: &str) -> Option<String> {
    let frame: Value = serde_json::from_str(text).ok()?;
    if frame.get("type").and_then(Value::as_str) != Some("disconnect") {
        return None;
    }
    Some(
        frame
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
    )
}

/// Slack's `{"type":"disconnect"}` frame: this socket is about to close; open another.
pub fn is_disconnect(text: &str) -> bool {
    disconnect_reason(text).is_some()
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
        assert_eq!(
            disconnect_reason(r#"{"type":"disconnect","reason":"refresh_requested"}"#).as_deref(),
            Some("refresh_requested")
        );
        assert_eq!(
            disconnect_reason(r#"{"type":"disconnect","reason":"warning"}"#).as_deref(),
            Some("warning")
        );
        assert_eq!(
            disconnect_reason(r#"{"type":"disconnect"}"#).as_deref(),
            Some("")
        );
        assert!(is_disconnect(
            r#"{"type":"disconnect","reason":"refresh_requested"}"#
        ));
        assert!(!is_disconnect(r#"{"type":"hello"}"#));
        assert!(is_fatal_slack_error(
            "slack connections.open failed: {\"ok\":false,\"error\":\"invalid_auth\"}"
        ));
        assert!(!is_fatal_slack_error("connection reset by peer"));
        assert!(!is_fatal_slack_error("slack connections.open timed out"));
        assert!(!is_fatal_slack_error("slack websocket handshake timed out"));
        assert!(!is_fatal_slack_error(
            "slack socket idle: no frame or pong within the read deadline"
        ));
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
        let value = blocks[1]["elements"][1]["value"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(value, "ferryx:approval:pane-1:abc123:v1-rev:1:once");
        let frame =
            json!({"envelope_id":"e9","type":"interactive","payload":{"type":"block_actions",
            "user":{"id":"UOWNER"},"actions":[{"value": value}]}})
            .to_string();
        let control = RecordingControl::default();
        apply_frame(
            &frame,
            "UOWNER",
            "meta",
            &crate::chat::router::ProfileRouter::new(vec![]),
            &control,
        )
        .await
        .unwrap();
        match &control.ops()[0].1 {
            AgentOp::Answer {
                session_id,
                prompt_id,
                screen_revision,
                answer,
                ..
            } => {
                assert_eq!(
                    (
                        session_id.as_str(),
                        prompt_id.as_str(),
                        screen_revision.as_str()
                    ),
                    ("pane-1", "abc123", "v1-rev")
                );
                assert_eq!(answer["optionIndex"], 1);
            }
            other => panic!("expected an answer, got {other:?}"),
        }
        // A stranger's click on the same card is refused and types nothing.
        let stranger = frame.replace("UOWNER", "USTRANGER");
        let control = RecordingControl::default();
        assert!(apply_frame(
            &stranger,
            "UOWNER",
            "meta",
            &crate::chat::router::ProfileRouter::new(vec![]),
            &control
        )
        .await
        .is_err());
        assert!(control.ops().is_empty());
        // A part with ':' would be split wrong, so no buttons are offered.
        let bad = crate::sessions::watch::ApprovalCard {
            prompt_id: "a:b".into(),
            ..card
        };
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

    #[cfg(feature = "hub-slack")]
    fn test_pace() -> SocketPace {
        SocketPace {
            connect_timeout: Duration::from_millis(300),
            ping_after: Duration::from_millis(200),
            read_idle: Duration::from_millis(500),
            backoff_initial: Duration::from_millis(40),
            backoff_cap: Duration::from_millis(80),
        }
    }

    #[cfg(feature = "hub-slack")]
    fn test_credentials(api_base: String) -> SocketCredentials {
        SocketCredentials {
            api_base,
            app_token: "xapp-test".into(),
            bot_token: "xoxb-test".into(),
        }
    }

    /// Accepts TCP and reads, but never writes a response.
    #[cfg(feature = "hub-slack")]
    fn spawn_stalled_accepts(
        listener: tokio::net::TcpListener,
    ) -> tokio::sync::mpsc::UnboundedReceiver<Instant> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                let tx = tx.clone();
                tokio::spawn(async move {
                    let _ = tx.send(Instant::now());
                    let mut buf = [0u8; 1024];
                    loop {
                        match tokio::io::AsyncReadExt::read(&mut sock, &mut buf).await {
                            Ok(0) | Err(_) => break,
                            Ok(_) => {}
                        }
                    }
                });
            }
        });
        rx
    }

    #[cfg(feature = "hub-slack")]
    async fn read_http_headers(sock: &mut tokio::net::TcpStream) -> bool {
        use tokio::io::AsyncReadExt;
        let mut buf = Vec::new();
        let mut tmp = [0u8; 1024];
        loop {
            if buf.windows(4).any(|window| window == b"\r\n\r\n") {
                return true;
            }
            if buf.len() > 65_536 {
                return false;
            }
            match sock.read(&mut tmp).await {
                Ok(0) | Err(_) => return false,
                Ok(n) => buf.extend_from_slice(&tmp[..n]),
            }
        }
    }

    #[cfg(feature = "hub-slack")]
    fn spawn_connections_open(listener: tokio::net::TcpListener, ws_url: String) {
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                let ws_url = ws_url.clone();
                tokio::spawn(async move {
                    if !read_http_headers(&mut sock).await {
                        return;
                    }
                    let body = format!(r#"{{"ok":true,"url":"{ws_url}"}}"#);
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ =
                        tokio::io::AsyncWriteExt::write_all(&mut sock, response.as_bytes()).await;
                });
            }
        });
    }

    #[cfg(feature = "hub-slack")]
    async fn next_within(
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<Instant>,
        budget: Duration,
        what: &str,
    ) -> Instant {
        tokio::time::timeout(budget, rx.recv())
            .await
            .unwrap_or_else(|_| panic!("{what} did not happen within {budget:?}"))
            .unwrap_or_else(|| panic!("{what} channel closed"))
    }

    #[cfg(feature = "hub-slack")]
    #[tokio::test]
    async fn an_open_that_accepts_tcp_and_never_answers_reconnects_within_the_connect_timeout() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut accepts = spawn_stalled_accepts(listener);
        let pace = test_pace();
        let creds = test_credentials(format!("http://127.0.0.1:{port}"));
        let client = tokio::spawn(async move {
            let _ = run_socket_mode(move || Ok(creds.clone()), pace, |_| async {}).await;
        });
        let first = next_within(&mut accepts, Duration::from_secs(2), "first http accept").await;
        let second = next_within(
            &mut accepts,
            pace.connect_timeout + pace.backoff_cap + Duration::from_secs(2),
            "second http accept",
        )
        .await;
        let gap = second.saturating_duration_since(first);
        let budget = pace.connect_timeout + pace.backoff_cap + Duration::from_secs(1);
        assert!(
            gap < budget,
            "stalled apps.connections.open was retried after {gap:?}, budget {budget:?}"
        );
        client.abort();
    }

    #[cfg(feature = "hub-slack")]
    #[tokio::test]
    async fn a_websocket_handshake_that_stalls_after_accept_reconnects_within_the_connect_timeout()
    {
        let http = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let ws = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let http_port = http.local_addr().unwrap().port();
        let ws_port = ws.local_addr().unwrap().port();
        spawn_connections_open(http, format!("ws://127.0.0.1:{ws_port}/"));
        let mut accepts = spawn_stalled_accepts(ws);
        let pace = test_pace();
        let creds = test_credentials(format!("http://127.0.0.1:{http_port}"));
        let client = tokio::spawn(async move {
            let _ = run_socket_mode(move || Ok(creds.clone()), pace, |_| async {}).await;
        });
        let first = next_within(
            &mut accepts,
            Duration::from_secs(2),
            "first websocket accept",
        )
        .await;
        let second = next_within(
            &mut accepts,
            pace.connect_timeout * 2 + pace.backoff_cap + Duration::from_secs(2),
            "second websocket accept",
        )
        .await;
        let gap = second.saturating_duration_since(first);
        // HTTP open is fast; the stalled handshake itself is capped by connect_timeout.
        let budget = pace.connect_timeout + pace.backoff_cap + Duration::from_secs(1);
        assert!(
            gap < budget,
            "stalled websocket handshake was retried after {gap:?}, budget {budget:?}"
        );
        client.abort();
    }

    #[cfg(feature = "hub-slack")]
    #[tokio::test]
    async fn a_silent_socket_is_pinged_and_reconnects_within_the_read_idle() {
        use futures_util::StreamExt;
        let http = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let ws = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let http_port = http.local_addr().unwrap().port();
        let ws_port = ws.local_addr().unwrap().port();
        spawn_connections_open(http, format!("ws://127.0.0.1:{ws_port}/"));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(async move {
            let mut generation = 0u32;
            loop {
                let Ok((tcp, _)) = ws.accept().await else {
                    break;
                };
                generation += 1;
                let tx = tx.clone();
                let which = generation;
                tokio::spawn(async move {
                    let mut socket = match tokio_tungstenite::accept_async(tcp).await {
                        Ok(socket) => socket,
                        Err(_) => return,
                    };
                    let _ = tx.send((which, "open", Instant::now()));
                    if which == 1 {
                        // Read the client's ping, then stop. A further read would flush
                        // tungstenite's automatic pong and hide the dead socket.
                        if let Ok(Some(Ok(message))) =
                            tokio::time::timeout(Duration::from_secs(2), socket.next()).await
                        {
                            if message.is_ping() {
                                let _ = tx.send((which, "ping", Instant::now()));
                            }
                        }
                    }
                    std::future::pending::<()>().await;
                });
            }
        });
        let pace = test_pace();
        let creds = test_credentials(format!("http://127.0.0.1:{http_port}"));
        let client = tokio::spawn(async move {
            let _ = run_socket_mode(move || Ok(creds.clone()), pace, |_| async {}).await;
        });
        let opened = next_within_kind(&mut rx, Duration::from_secs(2), 1, "open").await;
        let _ping = next_within_kind(&mut rx, Duration::from_secs(2), 1, "ping").await;
        let reopened = next_within_kind(&mut rx, Duration::from_secs(3), 2, "open").await;
        let gap = reopened.saturating_duration_since(opened);
        let budget =
            pace.read_idle + pace.connect_timeout + pace.backoff_cap + Duration::from_secs(1);
        assert!(
            gap < budget,
            "silent socket reconnected after {gap:?}, budget {budget:?}"
        );
        client.abort();
    }

    #[cfg(feature = "hub-slack")]
    async fn next_within_kind(
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<(u32, &'static str, Instant)>,
        budget: Duration,
        generation: u32,
        kind: &str,
    ) -> Instant {
        let deadline = Instant::now() + budget;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                panic!("{kind} for connection {generation} did not happen within {budget:?}");
            }
            let (which, event, at) = tokio::time::timeout(remaining, rx.recv())
                .await
                .unwrap_or_else(|_| {
                    panic!("{kind} for connection {generation} did not happen within {budget:?}")
                })
                .unwrap_or_else(|| panic!("socket event channel closed"));
            if which == generation && event == kind {
                return at;
            }
        }
    }

    #[cfg(feature = "hub-slack")]
    #[tokio::test]
    async fn a_disconnect_frame_opens_the_next_socket_before_the_old_one_closes() {
        use futures_util::{SinkExt, StreamExt};
        use std::sync::atomic::{AtomicBool, Ordering};
        let http = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let ws = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let http_port = http.local_addr().unwrap().port();
        let ws_port = ws.local_addr().unwrap().port();
        spawn_connections_open(http, format!("ws://127.0.0.1:{ws_port}/"));
        let first_open = std::sync::Arc::new(AtomicBool::new(false));
        let overlapped = std::sync::Arc::new(AtomicBool::new(false));
        let second_tcp = std::sync::Arc::new(tokio::sync::Notify::new());
        let (frames_tx, mut frames_rx) = tokio::sync::mpsc::unbounded_channel();
        let first_open_server = first_open.clone();
        let overlapped_server = overlapped.clone();
        let second_tcp_server = second_tcp.clone();
        tokio::spawn(async move {
            let mut generation = 0u32;
            loop {
                let Ok((tcp, _)) = ws.accept().await else {
                    break;
                };
                generation += 1;
                let which = generation;
                let first_open_server = first_open_server.clone();
                let overlapped_server = overlapped_server.clone();
                let second_tcp_server = second_tcp_server.clone();
                tokio::spawn(async move {
                    if which == 2 && first_open_server.load(Ordering::SeqCst) {
                        overlapped_server.store(true, Ordering::SeqCst);
                    }
                    if which == 2 {
                        second_tcp_server.notify_waiters();
                        // Hold the handshake so a frame on the old socket is still readable.
                        tokio::time::sleep(Duration::from_millis(150)).await;
                    }
                    let mut socket = match tokio_tungstenite::accept_async(tcp).await {
                        Ok(socket) => socket,
                        Err(_) => return,
                    };
                    if which == 1 {
                        first_open_server.store(true, Ordering::SeqCst);
                        let wait_second = second_tcp_server.notified();
                        tokio::pin!(wait_second);
                        wait_second.as_mut().enable();
                        let disconnect = r#"{"type":"disconnect","reason":"refresh_requested"}"#;
                        if socket
                            .send(tokio_tungstenite::tungstenite::Message::Text(
                                disconnect.to_owned().into(),
                            ))
                            .await
                            .is_err()
                        {
                            return;
                        }
                        let _ = tokio::time::timeout(Duration::from_secs(2), wait_second).await;
                        let event = r#"{"envelope_id":"e-handoff","type":"events_api","payload":{"event":{"type":"message","channel":"C1","user":"U1","text":"during-handoff","ts":"9"}}}"#;
                        let _ = socket
                            .send(tokio_tungstenite::tungstenite::Message::Text(
                                event.to_owned().into(),
                            ))
                            .await;
                        while let Some(item) = socket.next().await {
                            if item.is_err() {
                                break;
                            }
                        }
                        first_open_server.store(false, Ordering::SeqCst);
                    } else {
                        std::future::pending::<()>().await;
                    }
                });
            }
        });
        let pace = test_pace();
        let creds = test_credentials(format!("http://127.0.0.1:{http_port}"));
        let client = tokio::spawn(async move {
            let _ = run_socket_mode(
                move || Ok(creds.clone()),
                pace,
                move |text| {
                    let frames_tx = frames_tx.clone();
                    async move {
                        let _ = frames_tx.send(text);
                    }
                },
            )
            .await;
        });
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut saw_frame = false;
        while Instant::now() < deadline && !(overlapped.load(Ordering::SeqCst) && saw_frame) {
            if let Ok(Some(text)) =
                tokio::time::timeout(Duration::from_millis(30), frames_rx.recv()).await
            {
                if text.contains("during-handoff") {
                    saw_frame = true;
                }
            }
        }
        assert!(
            overlapped.load(Ordering::SeqCst),
            "replacement handshake was not accepted while the old socket was still open"
        );
        assert!(
            saw_frame,
            "frame sent on the old socket during handoff was dropped"
        );
        client.abort();
    }
}
