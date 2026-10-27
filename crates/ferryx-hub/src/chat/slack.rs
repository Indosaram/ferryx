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
    Command(super::commands::SlashInvocation),
    Ignore,
}

/// Slack Web API base. `FERRYX_HUB_SLACK_API` may point it at a loopback stand-in for a
/// local round-trip check; any other host is ignored so a token is never sent elsewhere.
pub fn api_base() -> String {
    #[cfg(test)]
    if let Some((base, _)) = slack_test_api() {
        return base;
    }
    crate::hub_env::var("FERRYX_HUB_SLACK_API")
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
    #[cfg(test)]
    if let Some((_, token)) = slack_test_api() {
        return Some(token);
    }
    secret_from_env("SLACK_BOT_TOKEN")
}

// Visible only to the test task that entered `SLACK_API_TEST.scope`. Other tests keep
// reading the process environment, so a click-feedback test cannot redirect them.
#[cfg(test)]
tokio::task_local! {
    static SLACK_API_TEST: (String, String);
}

#[cfg(test)]
fn slack_test_api() -> Option<(String, String)> {
    SLACK_API_TEST.try_with(|pair| pair.clone()).ok()
}

/// Runs `future` with the Slack Web API pointed at a test stand-in (base URL, token).
#[cfg(all(test, feature = "hub-slack"))]
pub(crate) async fn with_test_api<F: std::future::Future>(
    base: String,
    token: String,
    future: F,
) -> F::Output {
    SLACK_API_TEST.scope((base, token), future).await
}

/// Spawns `future`, keeping a test's Slack API stand-in visible inside the new task.
#[cfg(feature = "hub-slack")]
pub(crate) fn spawn_carrying_test_api<F>(future: F) -> tokio::task::JoinHandle<()>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    #[cfg(test)]
    if let Some(pair) = slack_test_api() {
        return tokio::spawn(SLACK_API_TEST.scope(pair, future));
    }
    tokio::spawn(future)
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
        "interactive" => match slack_approval_op(&payload, owner_id, "local")? {
            Some(op) => SlackFrame::Approval(op),
            None => SlackFrame::Ignore,
        },
        "slash_commands" => {
            let invocation = super::slash::invocation_from_payload(&payload);
            if invocation.command.is_empty() {
                SlackFrame::Ignore
            } else {
                SlackFrame::Command(invocation)
            }
        }
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
    let channel_id = event.get("channel").and_then(Value::as_str)?.to_owned();
    let audio = audio_message(event);
    let ts = event.get("ts").and_then(Value::as_str).map(str::to_owned);
    if audio {
        if let Some(ts) = ts.as_deref() {
            crate::voice::pending_voice_replies().note(&channel_id, ts);
        }
    }
    let mut text = event
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    if text.trim().is_empty() {
        if !audio {
            return None;
        }
        text = "[voice message]".into();
    }
    Some(Inbound {
        platform: PLATFORM.into(),
        guild_id: None,
        channel_id,
        thread_id: event
            .get("thread_ts")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| audio.then_some(ts).flatten()),
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

fn audio_message(event: &Value) -> bool {
    event
        .get("files")
        .and_then(Value::as_array)
        .is_some_and(|files| {
            files.iter().any(|file| {
                file.get("mimetype")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .starts_with("audio/")
                    || file.get("subtype").and_then(Value::as_str) == Some("slack_audio")
            })
        })
}

/// Applies one Socket Mode frame. An approval is answered, then the card or an ephemeral
/// note tells the clicker what happened. An inbound message is fenced and sent to the
/// meta-agent pane, or to the lane pane a profile names. Nothing is read back.
pub async fn apply_frame(
    text: &str,
    owner_id: &str,
    meta_session_id: &str,
    profiles: &super::router::ProfileRouter,
    control: &dyn AgentControl,
) -> Result<SlackFrame> {
    apply_frame_with(
        text,
        owner_id,
        meta_session_id,
        profiles,
        control,
        None,
        None,
    )
    .await
}

/// Same as [`apply_frame`], and a `slash_commands` frame runs `/omo-*` when `pool` is set.
/// The reply is posted when a bot token is configured. No token leaves the command applied
/// and reports that nothing was sent.
pub async fn apply_frame_with(
    text: &str,
    owner_id: &str,
    meta_session_id: &str,
    profiles: &super::router::ProfileRouter,
    control: &dyn AgentControl,
    pool: Option<&sqlx::SqlitePool>,
    bot_user_id: Option<&str>,
) -> Result<SlackFrame> {
    #[cfg(feature = "hub-slack")]
    let turns = super::slack_turn::installed();
    #[cfg(not(feature = "hub-slack"))]
    let turns: Option<&'static ()> = None;
    apply_frame_routed(
        text,
        owner_id,
        meta_session_id,
        profiles,
        control,
        pool,
        bot_user_id,
        turns,
    )
    .await
}

#[cfg(feature = "hub-slack")]
type TurnConfig = super::slack_turn::SlackTurnConfig;
#[cfg(not(feature = "hub-slack"))]
type TurnConfig = ();

/// [`apply_frame_with`] with an explicit omo turn configuration (`None`: messages go to panes).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn apply_frame_routed(
    text: &str,
    owner_id: &str,
    meta_session_id: &str,
    profiles: &super::router::ProfileRouter,
    control: &dyn AgentControl,
    pool: Option<&sqlx::SqlitePool>,
    bot_user_id: Option<&str>,
    turns: Option<&'static TurnConfig>,
) -> Result<SlackFrame> {
    #[cfg(not(feature = "hub-slack"))]
    let _ = turns;
    let frame_value: Value = serde_json::from_str(text)
        .map_err(|error| HubError::message(format!("slack frame: {error}")))?;
    if frame_value.get("type").and_then(Value::as_str) == Some("interactive") {
        #[cfg(feature = "hub-slack")]
        if let (Some(turns), Some(payload)) = (turns, frame_value.get("payload")) {
            if super::turn_approval::handle_click(payload, &turns.allowed_users).await {
                return Ok(SlackFrame::Ignore);
            }
        }
        return apply_interactive(&frame_value, owner_id, control).await;
    }
    let (_ack, frame) = handle_frame(text, owner_id)?;
    if let SlackFrame::Inbound(inbound) = &frame {
        if let Some(pool) = pool {
            if let Err(error) = crate::inbox_watch::note_gateway(pool, inbound, owner_id).await {
                tracing::warn!(%error, "slack message was not recorded for the inbox");
            }
            // omo: the owner's reply in a finished lane's thread reopens that lane.
            if let (Some(thread_ts), Some(message_id)) =
                (inbound.thread_id.as_deref(), inbound.event_id.as_deref())
            {
                let reply = crate::lanes::owner_replies::OwnerReply {
                    channel: &inbound.channel_id,
                    thread_ts,
                    author: &inbound.user_id,
                    owner: owner_id,
                    message_id,
                    text: &inbound.text,
                };
                match crate::lanes::owner_replies::intercept(pool, control, &reply).await {
                    Ok(true) => return Ok(SlackFrame::Ignore),
                    Ok(false) => {}
                    Err(error) => tracing::warn!(%error, "lane owner reply was not applied"),
                }
            }
        }
    }
    // With the omo turn path installed, a message no profile routes to a lane pane becomes an
    // mhc RPC turn answered in its Slack thread, as omo-gateway did. The meta pane is
    // not used for it: no such pane exists unless the supervisor preset is launched.
    #[cfg(feature = "hub-slack")]
    if let (Some(turns), Some(pool)) = (turns, pool) {
        let lane_routed = match &frame {
            SlackFrame::Inbound(inbound) => {
                super::router::inbound_pane(inbound, profiles, meta_session_id) != meta_session_id
            }
            _ => false,
        };
        if !lane_routed {
            if super::slack_turn::intercept(turns, pool, &frame_value).await {
                return Ok(frame);
            }
            if matches!(frame, SlackFrame::Inbound(_)) {
                return Ok(SlackFrame::Ignore);
            }
        }
    }
    if let SlackFrame::Inbound(inbound) = &frame {
        super::router::route_inbound(inbound, profiles, meta_session_id, control).await?;
        if let Some(pool) = pool {
            if let Err(error) =
                super::router::remember_inbound_origin(pool, inbound, profiles, meta_session_id)
                    .await
            {
                tracing::warn!(%error, "slack chat origin was not stored");
            }
        }
    }
    if let (SlackFrame::Command(invocation), Some(pool)) = (&frame, pool) {
        let model = crate::meta::backend::RpcModel::new(crate::meta::supervisor::mhc_bin());
        let model_ref = Some(&model as &dyn crate::lanes::kibitzer::ModelTurn);
        let env = super::slash::SlashEnv {
            pool,
            owner_id,
            bot_user_id: bot_user_id.unwrap_or("bot"),
            control,
            meta_session_id,
            huddle_enabled: matches!(
                crate::hub_env::var("OMON_SLACK_HUDDLE_ENABLED")
                    .ok()
                    .as_deref()
                    .map(str::trim)
                    .map(|value| value.to_ascii_lowercase()),
                Some(value) if matches!(value.as_str(), "1" | "true" | "yes" | "on")
            ),
            model: model_ref,
        };
        let reply = super::slash::execute(&env, invocation).await?;
        post_slash_reply(
            &invocation.channel_id,
            &invocation.user_id,
            invocation.thread_ts.as_deref(),
            &reply,
        )
        .await?;
    }
    Ok(frame)
}

async fn post_slash_reply(
    channel: &str,
    user: &str,
    thread: Option<&str>,
    reply: &super::slash::SlashReply,
) -> Result<bool> {
    #[cfg(feature = "hub-slack")]
    {
        if bot_token().is_none() {
            return Ok(false);
        }
        if reply.ephemeral {
            return slack_method(
                "chat.postEphemeral",
                json!({"channel": channel, "user": user, "text": reply.text}),
            )
            .await;
        }
        let mut body = json!({"channel": channel, "text": reply.text});
        if let Some(thread) = thread {
            body["thread_ts"] = json!(thread);
        }
        return slack_method("chat.postMessage", body).await;
    }
    #[cfg(not(feature = "hub-slack"))]
    {
        let _ = (channel, user, thread, reply);
        Ok(false)
    }
}

struct ClickTarget {
    channel_id: String,
    message_ts: String,
    option_label: String,
    message: Value,
}

fn click_target(payload: &Value, op: Option<&AgentOp>) -> ClickTarget {
    let channel_id = payload
        .pointer("/channel/id")
        .or_else(|| payload.pointer("/container/channel_id"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let message_ts = payload
        .pointer("/message/ts")
        .or_else(|| payload.pointer("/container/message_ts"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let from_button = payload
        .pointer("/actions/0/text/text")
        .and_then(Value::as_str)
        .filter(|label| !label.trim().is_empty())
        .map(str::to_owned);
    let option_label = from_button.unwrap_or_else(|| match op {
        Some(AgentOp::Answer { answer, .. }) => {
            let index = answer
                .get("optionIndex")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            format!("option {}", index + 1)
        }
        _ => "that option".into(),
    });
    ClickTarget {
        channel_id,
        message_ts,
        option_label,
        message: payload.get("message").cloned().unwrap_or(Value::Null),
    }
}

fn answered_line(label: &str, user_id: &str, at: chrono::DateTime<chrono::Utc>) -> String {
    let unix = at.timestamp();
    let fallback = at.format("%Y-%m-%d %H:%M UTC");
    // `{date_short_pretty}` and `{time}` are Slack tokens. Each viewer sees their own zone.
    // The UTC string is the fallback for a client that does not expand `<!date^…>`.
    format!(
        "✅ {label} — answered by <@{user_id}> at <!date^{unix}^{{date_short_pretty}} {{time}}|{fallback}>"
    )
}

fn card_blocks(message: &Value, line: &str) -> Value {
    let mut blocks = Vec::new();
    if let Some(existing) = message.get("blocks").and_then(Value::as_array) {
        for block in existing {
            if block.get("type").and_then(Value::as_str) == Some("actions") {
                continue;
            }
            blocks.push(block.clone());
        }
    }
    if blocks.is_empty() {
        if let Some(text) = message.get("text").and_then(Value::as_str) {
            if !text.is_empty() {
                blocks.push(json!({"type": "section", "text": {"type": "mrkdwn", "text": text}}));
            }
        }
    }
    blocks.push(json!({"type": "context", "elements": [{"type": "mrkdwn", "text": line}]}));
    Value::Array(blocks)
}

fn remember_label(request_id: &str, label: &str) {
    if request_id.is_empty() || label.is_empty() {
        return;
    }
    ANSWERED_LABELS
        .lock()
        .expect("answered labels")
        .insert(request_id.to_owned(), label.to_owned());
}

fn remembered_label(request_id: &str) -> Option<String> {
    ANSWERED_LABELS
        .lock()
        .expect("answered labels")
        .get(request_id)
        .cloned()
}

static ANSWERED_LABELS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, String>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

fn answer_request_id(op: &AgentOp) -> &str {
    match op {
        AgentOp::Answer { request_id, .. } => request_id,
        _ => "",
    }
}

fn log_interactive(user: &str, outcome: &str, request_id: &str, reason: &str) {
    tracing::info!(user, outcome, request_id, reason, "slack interactive frame");
}

async fn apply_interactive(
    frame_value: &Value,
    owner_id: &str,
    control: &dyn AgentControl,
) -> Result<SlackFrame> {
    let payload = frame_value.get("payload").cloned().unwrap_or(Value::Null);
    let user = payload
        .pointer("/user/id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    log_interactive(&user, "received", "", "");
    let target = click_target(&payload, None);
    match slack_approval_op(&payload, owner_id, "local") {
        Err(error) if error.to_string().contains("not from the owner") => {
            log_interactive(&user, "refused", "", "not from the owner");
            let _ = post_ephemeral(&target, &user, "only the owner can answer").await;
            Ok(SlackFrame::Ignore)
        }
        Err(error) => {
            log_interactive(&user, "ignored", "", &error.to_string());
            Err(error)
        }
        Ok(None) => {
            if crate::chat::buttons::payload_has_button(&payload) {
                match crate::chat::buttons::run_button_clicks(
                    &payload,
                    owner_id,
                    &crate::chat::buttons::button_actions_path(),
                )
                .await
                {
                    Ok(lines) => {
                        let text = if lines.is_empty() {
                            "완료했습니다.".to_owned()
                        } else {
                            lines.join("\n")
                        };
                        let _ = post_ephemeral(&target, &user, &text).await;
                        log_interactive(&user, "button", "", "");
                        Ok(SlackFrame::Ignore)
                    }
                    Err(error) => {
                        let message = error.to_string();
                        let _ = post_ephemeral(&target, &user, &message).await;
                        log_interactive(&user, "button_failed", "", &message);
                        Ok(SlackFrame::Ignore)
                    }
                }
            } else {
                log_interactive(&user, "ignored", "", "");
                Ok(SlackFrame::Ignore)
            }
        }
        Ok(Some(op)) => {
            let target = click_target(&payload, Some(&op));
            let request_id = answer_request_id(&op).to_owned();
            log_interactive(&user, "accepted", &request_id, "");
            match control.execute(None, op.clone()).await {
                Ok(_) => {
                    remember_label(&request_id, &target.option_label);
                    log_interactive(&user, "answered", &request_id, "");
                    let line = answered_line(&target.option_label, &user, chrono::Utc::now());
                    let _ = update_card(&target, &line).await;
                    Ok(SlackFrame::Approval(op))
                }
                Err(error) => {
                    let message = error.to_string();
                    log_interactive(&user, "answer_failed", &request_id, &message);
                    match super::approval::click_failure_kind(&message) {
                        super::approval::ClickFailure::AlreadyAnswered => {
                            let text = match remembered_label(&request_id) {
                                Some(label) => format!("already answered with {label}"),
                                None => "already answered".into(),
                            };
                            let _ = post_ephemeral(&target, &user, &text).await;
                        }
                        super::approval::ClickFailure::InFlight => {
                            let _ =
                                post_ephemeral(&target, &user, "that click is already in progress")
                                    .await;
                        }
                        super::approval::ClickFailure::Stale => {
                            let _ =
                                post_ephemeral(&target, &user, "prompt no longer on screen").await;
                            let _ = update_card(&target, "⚠️ prompt no longer on screen").await;
                        }
                        super::approval::ClickFailure::Other => {
                            let shown: String = message.chars().take(240).collect();
                            let _ = post_ephemeral(&target, &user, &shown).await;
                            let _ = update_card(&target, &format!("⚠️ {shown}")).await;
                        }
                    }
                    Ok(SlackFrame::Ignore)
                }
            }
        }
    }
}

async fn post_ephemeral(target: &ClickTarget, user_id: &str, text: &str) -> Result<bool> {
    if target.channel_id.is_empty() || user_id.is_empty() {
        return Ok(false);
    }
    slack_web(
        "chat.postEphemeral",
        json!({ "channel": target.channel_id, "user": user_id, "text": text }),
    )
    .await
}

async fn update_card(target: &ClickTarget, line: &str) -> Result<bool> {
    if target.channel_id.is_empty() || target.message_ts.is_empty() {
        return Ok(false);
    }
    slack_web(
        "chat.update",
        json!({
            "channel": target.channel_id,
            "ts": target.message_ts,
            "text": line,
            "blocks": card_blocks(&target.message, line),
        }),
    )
    .await
}

async fn slack_web(method: &str, body: Value) -> Result<bool> {
    #[cfg(feature = "hub-slack")]
    {
        return slack_method(method, body).await;
    }
    #[cfg(not(feature = "hub-slack"))]
    {
        let _ = (method, body);
        Ok(false)
    }
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
    if bot_token().is_none() {
        return Ok(false);
    }
    let (text, tables) = if blocks.is_none() {
        crate::table_image::transform_markdown_tables(text)
    } else {
        (text.to_owned(), Vec::new())
    };
    let mut body = json!({ "channel": session.channel_id, "text": text });
    if let Some(blocks) = blocks {
        body["blocks"] = blocks;
    }
    if let Some(thread) = &session.thread_id {
        body["thread_ts"] = json!(thread);
    }
    let posted = slack_method("chat.postMessage", body).await?;
    if posted {
        let _ = attach_voice_reply(
            &session.channel_id,
            session.thread_id.as_deref(),
            session.thread_id.as_deref(),
            &text,
        )
        .await;
        upload_tables(session, &tables).await?;
    }
    Ok(posted)
}

#[cfg(feature = "hub-slack")]
async fn upload_tables(
    session: &super::session::SessionKey,
    tables: &[crate::table_image::RenderedTable],
) -> Result<()> {
    for (index, table) in tables.iter().enumerate() {
        if let Err(error) = upload_external(
            &session.channel_id,
            session.thread_id.as_deref(),
            &table.filename,
            table.image_bytes.clone(),
            &table.filename,
        )
        .await
        {
            let fallback = crate::table_image::fallback_text(&tables[index..]);
            if !fallback.is_empty() {
                let mut body = json!({ "channel": session.channel_id, "text": fallback });
                if let Some(thread) = &session.thread_id {
                    body["thread_ts"] = json!(thread);
                }
                let _ = slack_method("chat.postMessage", body).await;
            }
            return Err(error);
        }
    }
    Ok(())
}

/// Slack external upload: getUploadURLExternal, POST the bytes, completeUploadExternal.
#[cfg(feature = "hub-slack")]
pub async fn upload_external(
    channel: &str,
    thread_ts: Option<&str>,
    filename: &str,
    bytes: Vec<u8>,
    title: &str,
) -> Result<bool> {
    if bot_token().is_none() || bytes.is_empty() {
        return Ok(false);
    }
    let ticket = slack_form(
        "files.getUploadURLExternal",
        &[("filename", filename), ("length", &bytes.len().to_string())],
    )
    .await?;
    if ticket.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err(HubError::message(format!(
            "slack files.getUploadURLExternal failed: {ticket}"
        )));
    }
    let upload_url = ticket
        .get("upload_url")
        .and_then(Value::as_str)
        .ok_or_else(|| HubError::message("slack upload ticket has no upload_url"))?;
    if !upload_host_allowed(upload_url) {
        return Err(HubError::message(
            "slack upload_url host is not slack or loopback",
        ));
    }
    let file_id = ticket
        .get("file_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let mut builder = reqwest::Client::builder().timeout(std::time::Duration::from_secs(60));
    if is_loopback_api(upload_url) {
        builder = builder.no_proxy();
    }
    let client = builder
        .build()
        .map_err(|error| HubError::message(error.to_string()))?;
    let uploaded = client
        .post(upload_url)
        .body(bytes)
        .send()
        .await
        .map_err(|error| HubError::message(error.to_string()))?;
    if !uploaded.status().is_success() {
        return Err(HubError::message(format!(
            "slack file bytes failed: {}",
            uploaded.status()
        )));
    }
    let mut body = json!({
        "files": [{ "id": file_id, "title": title }],
        "channel_id": channel,
    });
    if let Some(thread_ts) = thread_ts {
        body["thread_ts"] = json!(thread_ts);
    }
    slack_method("files.completeUploadExternal", body).await
}

#[cfg(feature = "hub-slack")]
fn upload_host_allowed(url: &str) -> bool {
    is_loopback_api(url)
        || url.starts_with("https://slack.com/")
        || url.starts_with("https://files.slack.com/")
}

/// omo's `owner_dm_key` inputs: the owner's IM channel (`conversations.open`) and the bot user
/// (`auth.test`).
#[cfg(feature = "hub-slack")]
pub async fn open_owner_dm(owner: &str) -> Option<(String, String)> {
    let auth = slack_form("auth.test", &[]).await.ok()?;
    let bot = auth.get("user_id").and_then(Value::as_str)?.to_owned();
    let opened = slack_form("conversations.open", &[("users", owner)])
        .await
        .ok()?;
    let channel = opened
        .pointer("/channel/id")
        .and_then(Value::as_str)?
        .to_owned();
    Some((channel, bot))
}

#[cfg(feature = "hub-slack")]
async fn slack_form(method: &str, fields: &[(&str, &str)]) -> Result<Value> {
    let Some(token) = bot_token() else {
        return Ok(json!({"ok": false}));
    };
    let base = api_base();
    let mut builder = reqwest::Client::builder().timeout(std::time::Duration::from_secs(15));
    if is_loopback_api(&base) {
        builder = builder.no_proxy();
    }
    let client = builder
        .build()
        .map_err(|error| HubError::message(error.to_string()))?;
    let mut form = std::collections::HashMap::new();
    for (key, value) in fields {
        form.insert(*key, *value);
    }
    let response = client
        .post(format!("{base}/{method}"))
        .bearer_auth(token)
        .form(&form)
        .send()
        .await
        .map_err(|error| HubError::message(error.to_string()))?;
    response
        .json()
        .await
        .map_err(|error| HubError::message(error.to_string()))
}

#[cfg(feature = "hub-slack")]
async fn attach_voice_reply(
    channel: &str,
    thread: Option<&str>,
    reply_to: Option<&str>,
    text: &str,
) -> Result<bool> {
    #[cfg(all(feature = "hub-slack", feature = "hub-voice"))]
    {
        let settings =
            crate::voice::VoiceSettings::from_lookup(&|name| crate::hub_env::var(name).ok());
        if !settings.slack_voice_reply() {
            return Ok(false);
        }
        if !crate::voice::pending_voice_replies().take(channel, reply_to) {
            return Ok(false);
        }
        let Some(endpoint) = settings.tts().cloned() else {
            return Ok(false);
        };
        let pcm = crate::voice::synthesize_pcm(&endpoint, settings.tts_voice(), text)
            .await
            .map_err(HubError::message)?;
        if pcm.is_empty() {
            return Ok(false);
        }
        let samples = crate::voice::pcm_bytes_to_samples(&pcm);
        let wav = crate::voice::pcm_to_wav(
            crate::voice::OPENAI_PCM_RATE,
            crate::voice::OPENAI_PCM_CHANNELS,
            &samples,
        )
        .map_err(HubError::message)?;
        return upload_external(channel, thread, "voice-reply.wav", wav, "Voice reply").await;
    }
    #[cfg(not(all(feature = "hub-slack", feature = "hub-voice")))]
    {
        let _ = (channel, thread, reply_to, text);
        Ok(false)
    }
}

/// One Slack Web API call with the bot token, for callers outside this module.
#[cfg(feature = "hub-slack")]
pub(crate) async fn call_method(method: &str, body: Value) -> Result<bool> {
    slack_method(method, body).await
}

#[cfg(feature = "hub-slack")]
async fn slack_method(method: &str, body: Value) -> Result<bool> {
    let Some(token) = bot_token() else {
        return Ok(false);
    };
    let base = api_base();
    let mut builder = reqwest::Client::builder().timeout(std::time::Duration::from_secs(15));
    if is_loopback_api(&base) {
        builder = builder.no_proxy();
    }
    let client = builder
        .build()
        .map_err(|error| HubError::message(error.to_string()))?;
    let response = client
        .post(format!("{base}/{method}"))
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
        return Err(HubError::message(format!("slack {method} failed: {value}")));
    }
    Ok(true)
}

/// omo-gateway's Slack send (`egress::send`): tables become images first, then each
/// `mrkdwn::render` chunk is posted as is, and the table images follow in the thread.
#[cfg(feature = "hub-slack")]
pub async fn deliver_omo(channel: &str, thread: Option<&str>, text: &str) -> Result<bool> {
    if bot_token().is_none() {
        return Ok(false);
    }
    let session = super::session::SessionKey::new(
        "slack",
        None::<String>,
        channel,
        thread.map(str::to_owned),
        "cron",
    );
    let (text, tables) = crate::table_image::transform_markdown_tables(text);
    let mut posted = false;
    for chunk in super::mrkdwn::render(&text) {
        let mut body = json!({ "channel": channel, "text": chunk });
        if let Some(thread) = thread {
            body["thread_ts"] = json!(thread);
        }
        posted |= slack_method("chat.postMessage", body).await?;
    }
    if posted {
        let _ = attach_voice_reply(channel, thread, thread, &text).await;
        upload_tables(&session, &tables).await?;
    }
    Ok(posted)
}

/// Posts one cron or lane delivery into a Slack channel or thread.
#[cfg(feature = "hub-slack")]
pub async fn deliver(channel: &str, thread: Option<&str>, text: &str) -> Result<bool> {
    let session = super::session::SessionKey::new(
        "slack",
        None::<String>,
        channel,
        thread.map(str::to_owned),
        "cron",
    );
    post_message(&session, text, None).await
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
    run_socket_mode_tracked(
        &SOCKET_MODE,
        credentials_from_env,
        SocketPace::production(),
        on_frame,
    )
    .await
}

/// Socket Mode state for `ferryx hub status`. It holds no token and no socket URL.
#[derive(Debug, Default)]
pub struct SocketModeState {
    running: std::sync::atomic::AtomicBool,
    connected: std::sync::atomic::AtomicBool,
    connects: std::sync::atomic::AtomicU64,
    last_error: std::sync::Mutex<Option<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SocketModeStatus {
    /// The Socket Mode task is alive (it may be between reconnects).
    pub running: bool,
    /// A websocket is open right now.
    pub connected: bool,
    /// Websockets opened since the daemon started.
    pub connects: u64,
    pub last_error: Option<String>,
}

#[cfg_attr(not(feature = "hub-slack"), allow(dead_code))]
impl SocketModeState {
    pub const fn new() -> Self {
        Self {
            running: std::sync::atomic::AtomicBool::new(false),
            connected: std::sync::atomic::AtomicBool::new(false),
            connects: std::sync::atomic::AtomicU64::new(0),
            last_error: std::sync::Mutex::new(None),
        }
    }

    pub fn snapshot(&self) -> SocketModeStatus {
        use std::sync::atomic::Ordering;
        SocketModeStatus {
            running: self.running.load(Ordering::SeqCst),
            connected: self.connected.load(Ordering::SeqCst),
            connects: self.connects.load(Ordering::SeqCst),
            last_error: self.last_error.lock().ok().and_then(|error| error.clone()),
        }
    }

    fn mark_connected(&self) {
        use std::sync::atomic::Ordering;
        self.connects.fetch_add(1, Ordering::SeqCst);
        self.connected.store(true, Ordering::SeqCst);
        if let Ok(mut error) = self.last_error.lock() {
            *error = None;
        }
    }

    fn mark_disconnected(&self, error: Option<&str>) {
        self.connected
            .store(false, std::sync::atomic::Ordering::SeqCst);
        if let (Some(message), Ok(mut slot)) = (error, self.last_error.lock()) {
            *slot = Some(redact_socket_error(message));
        }
    }

    /// Marks the task running until the returned guard drops. An aborted task (`hub stop`)
    /// drops the guard too, so a stopped hub never reports a live socket.
    fn session(&self) -> SocketModeSession<'_> {
        self.running
            .store(true, std::sync::atomic::Ordering::SeqCst);
        SocketModeSession(self)
    }
}

#[cfg_attr(not(feature = "hub-slack"), allow(dead_code))]
struct SocketModeSession<'a>(&'a SocketModeState);

impl Drop for SocketModeSession<'_> {
    fn drop(&mut self) {
        use std::sync::atomic::Ordering;
        self.0.connected.store(false, Ordering::SeqCst);
        self.0.running.store(false, Ordering::SeqCst);
    }
}

#[cfg_attr(not(feature = "hub-slack"), allow(dead_code))]
/// Slack error text for status output: one line, no socket URL (it carries a ticket), bounded.
fn redact_socket_error(message: &str) -> String {
    let mut out: String = message
        .split_whitespace()
        .map(|word| {
            if word.contains("wss://") || word.contains("ws://") || word.contains("xox") {
                "<redacted>"
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    if out.chars().count() > 200 {
        out = out.chars().take(200).collect::<String>() + "...";
    }
    out
}

static SOCKET_MODE: SocketModeState = SocketModeState::new();

/// The daemon's Socket Mode state.
pub fn socket_mode_status() -> SocketModeStatus {
    SOCKET_MODE.snapshot()
}

#[cfg(any(test, feature = "hub-slack"))]
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

#[cfg(all(test, feature = "hub-slack"))]
async fn run_socket_mode<F, Fut, C>(credentials: C, pace: SocketPace, on_frame: F) -> Result<()>
where
    C: FnMut() -> Result<SocketCredentials> + Send,
    F: FnMut(String) -> Fut + Send,
    Fut: std::future::Future<Output = ()> + Send,
{
    run_socket_mode_tracked(&SocketModeState::new(), credentials, pace, on_frame).await
}

#[cfg(feature = "hub-slack")]
async fn run_socket_mode_tracked<F, Fut, C>(
    state: &SocketModeState,
    mut credentials: C,
    pace: SocketPace,
    mut on_frame: F,
) -> Result<()>
where
    C: FnMut() -> Result<SocketCredentials> + Send,
    F: FnMut(String) -> Fut + Send,
    Fut: std::future::Future<Output = ()> + Send,
{
    let _session = state.session();
    let mut backoff = pace.backoff_initial;
    let mut attempt = 0u64;
    loop {
        let session =
            drive_session(&mut credentials, pace, &mut attempt, &mut on_frame, state).await;
        state.mark_disconnected(
            session
                .as_ref()
                .err()
                .map(|error| error.to_string())
                .as_deref(),
        );
        match session {
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
    state: &SocketModeState,
) -> Result<()>
where
    C: FnMut() -> Result<SocketCredentials>,
    F: FnMut(String) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let mut socket = connect_once(&credentials()?, pace, attempt).await?;
    state.mark_connected();
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
        .is_ok());
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
    async fn an_owner_reply_in_a_closed_lane_thread_reopens_the_lane_instead_of_routing() {
        use crate::lanes::state::{insert_lane, set_pane, transition, LaneStatus};
        let pool = crate::storage::memory().await.unwrap();
        insert_lane(&pool, "ledger", "Ledger", "lead", "/tmp", "file marker")
            .await
            .unwrap();
        set_pane(&pool, "ledger", "lane-pane").await.unwrap();
        transition(&pool, "ledger", LaneStatus::Working, Some("launched"))
            .await
            .unwrap();
        transition(&pool, "ledger", LaneStatus::Closed, Some("forced: done"))
            .await
            .unwrap();
        sqlx::query("UPDATE lanes SET thread_channel_id = 'slack:C1:50.1' WHERE key = 'ledger'")
            .execute(&pool)
            .await
            .unwrap();
        let frame = r#"{"envelope_id":"e9","type":"events_api","payload":{"event":{"type":"message","channel":"C1","user":"UOWNER","text":"one more thing","ts":"60.1","thread_ts":"50.1"}}}"#;
        let control = RecordingControl::default();
        let routed = apply_frame_routed(
            frame,
            "UOWNER",
            "meta-pane",
            &crate::chat::router::ProfileRouter::new(vec![]),
            &control,
            Some(&pool),
            None,
            None,
        )
        .await
        .unwrap();
        assert!(matches!(routed, SlackFrame::Ignore));
        let sends: Vec<String> = control
            .ops()
            .into_iter()
            .filter_map(|(_, op)| match op {
                AgentOp::Send { session_id, .. } => Some(session_id),
                _ => None,
            })
            .collect();
        assert_eq!(sends, vec!["lane-pane".to_owned()]);
        let status: String = sqlx::query_scalar("SELECT status FROM lanes WHERE key = 'ledger'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(status, "working");
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

    #[tokio::test]
    async fn a_slash_command_frame_runs_omo_yolo() {
        let pool = crate::storage::memory().await.unwrap();
        let frame = r#"{"envelope_id":"e9","type":"slash_commands","payload":{"command":"/omo-yolo","text":"on","user_id":"UOWNER","channel_id":"C1","team_id":"T1"}}"#;
        let control = RecordingControl::default();
        let parsed = apply_frame_with(
            frame,
            "UOWNER",
            "meta",
            &crate::chat::router::ProfileRouter::new(vec![]),
            &control,
            Some(&pool),
            Some("B1"),
        )
        .await
        .unwrap();
        assert!(matches!(parsed, SlackFrame::Command(_)));
        let on: i64 =
            sqlx::query_scalar("SELECT yolo FROM action_loop_state WHERE session_key LIKE '%C1%'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(on, 1);
        let (_ack, ignored) = handle_frame(
            r#"{"envelope_id":"e10","type":"slash_commands","payload":{"text":"on"}}"#,
            "UOWNER",
        )
        .unwrap();
        assert!(matches!(ignored, SlackFrame::Ignore));
    }

    #[cfg(feature = "hub-slack")]
    #[derive(Clone)]
    struct SlackRec(std::sync::Arc<std::sync::Mutex<Vec<(String, Value)>>>);

    #[cfg(feature = "hub-slack")]
    async fn record_update(
        axum::extract::State(rec): axum::extract::State<SlackRec>,
        axum::Json(body): axum::Json<Value>,
    ) -> axum::Json<Value> {
        rec.0
            .lock()
            .expect("slack rec")
            .push(("chat.update".into(), body));
        axum::Json(json!({"ok": true}))
    }

    #[cfg(feature = "hub-slack")]
    async fn record_ephemeral(
        axum::extract::State(rec): axum::extract::State<SlackRec>,
        axum::Json(body): axum::Json<Value>,
    ) -> axum::Json<Value> {
        rec.0
            .lock()
            .expect("slack rec")
            .push(("chat.postEphemeral".into(), body));
        axum::Json(json!({"ok": true}))
    }

    #[cfg(feature = "hub-slack")]
    async fn serve_slack_api() -> (u16, std::sync::Arc<std::sync::Mutex<Vec<(String, Value)>>>) {
        let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let app = axum::Router::new()
            .route("/chat.update", axum::routing::post(record_update))
            .route("/chat.postEphemeral", axum::routing::post(record_ephemeral))
            .with_state(SlackRec(log.clone()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (port, log)
    }

    #[cfg(feature = "hub-slack")]
    #[tokio::test]
    async fn e2e_a_slack_message_becomes_an_omo_turn_answered_in_its_thread() {
        use crate::chat::slack_turn::SlackTurnConfig;
        use crate::meta::rpc_mock::{serve, Script};
        use crate::meta::omo_client::OmoClientConfig;
        use std::sync::{Arc, Mutex};

        // Mock Slack Web API: records every method call and answers ok.
        #[derive(Clone)]
        struct Calls(Arc<Mutex<Vec<(String, Value)>>>, tokio::sync::watch::Sender<u64>);
        async fn record(
            axum::extract::State(calls): axum::extract::State<Calls>,
            axum::extract::Path(method): axum::extract::Path<String>,
            axum::Json(body): axum::Json<Value>,
        ) -> axum::Json<Value> {
            calls.0.lock().unwrap().push((method, body));
            calls.1.send_modify(|version| *version += 1);
            axum::Json(json!({"ok": true, "ts": "999.1"}))
        }
        let calls = Calls(Arc::new(Mutex::new(Vec::new())), tokio::sync::watch::channel(0).0);
        let app = axum::Router::new()
            .route("/{method}", axum::routing::post(record))
            .with_state(calls.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        // An executable mhc JSONL peer produces interim and final assistant messages.
        let (binary, omo) = serve(Script {
            messages: vec!["looking now".into(), "**All good**: 3 items".into()],
            approval: true,
            live: false,
        }).await;
        let config: &'static SlackTurnConfig = Box::leak(Box::new(SlackTurnConfig {
            omo: OmoClientConfig::new(binary),
            allowed_users: vec!["UOWNER".into()],
            free_channels: vec!["CHOME".into()],
            default_model: Some("model-x".into()),
            allowed_channels: Vec::new(),
            ignored_channels: Vec::new(),
            thread_require_mention: false,
            auto_thread: true,
            allow_bots: crate::chat::slack_turn::AllowBots::None,
            allow_all_users: false,
        }));
        let pool = crate::storage::memory().await.unwrap();
        let control = crate::agent_control::RecordingControl::default();
        let profiles = crate::chat::router::ProfileRouter::new(vec![]);
        let frame = |user: &str, ts: &str, thread: Option<&str>, text: &str| {
            let mut event = json!({"type": "message", "user": user, "channel": "CHOME", "ts": ts, "text": text});
            if let Some(thread) = thread {
                event["thread_ts"] = json!(thread);
            }
            json!({"envelope_id": format!("env-{ts}"), "type": "events_api", "payload": {
                "team_id": "T1", "authorizations": [{"user_id": "UBOT"}], "event": event
            }})
            .to_string()
        };
        let posts = |calls: &Calls| -> Vec<Value> {
            calls
                .0
                .lock()
                .unwrap()
                .iter()
                .filter(|(m, _)| m == "chat.postMessage")
                .map(|(_, b)| b.clone())
                .collect()
        };
        let wait_posts = |n: usize| {
            let calls = calls.clone();
            let mut changed = calls.1.subscribe();
            async move {
                tokio::time::timeout(std::time::Duration::from_secs(10), async {
                    loop {
                        if posts(&calls).len() >= n && calls.0.lock().unwrap().iter()
                            .filter(|(m, b)| m == "reactions.add" && b["name"] == "white_check_mark")
                            .count() >= n / 2 { return; }
                        changed.changed().await.unwrap();
                    }
                }).await.expect("Slack replies and completion reactions arrived");
            }
        };

        SLACK_API_TEST
            .scope(
                (format!("http://127.0.0.1:{port}"), "xoxb-test".into()),
                async {
                    let first = frame("UOWNER", "100.1", None, "how are things?");
                    apply_frame_routed(
                        &first,
                        "UOWNER",
                        "meta",
                        &profiles,
                        &control,
                        Some(&pool),
                        None,
                        Some(config),
                    )
                    .await
                    .unwrap();
                    // Two posts (interim + final) and the eyes / check reactions.
                    wait_posts(2).await;
                    let replies = posts(&calls);
                    assert_eq!(replies.len(), 2, "{replies:?}");
                    assert!(replies
                        .iter()
                        .all(|r| r["channel"] == "CHOME" && r["thread_ts"] == "100.1"));
                    assert_eq!(replies[0]["text"], "looking now");
                    assert_eq!(replies[1]["text"], "*All good*: 3 items");
                    let log = calls.0.lock().unwrap().clone();
                    assert!(log.iter().any(|(m, b)| m == "reactions.add"
                        && b["name"] == "eyes"
                        && b["timestamp"] == "100.1"));
                    assert!(log
                        .iter()
                        .any(|(m, b)| m == "reactions.remove" && b["name"] == "eyes"));
                    assert!(log
                        .iter()
                        .any(|(m, b)| m == "reactions.add" && b["name"] == "white_check_mark"));

                    // The same message redelivered by Slack does not run again; a stranger is ignored.
                    apply_frame_routed(
                        &first,
                        "UOWNER",
                        "meta",
                        &profiles,
                        &control,
                        Some(&pool),
                        None,
                        Some(config),
                    )
                    .await
                    .unwrap();
                    let stranger = frame("USTRANGER", "100.5", None, "hi");
                    apply_frame_routed(
                        &stranger,
                        "UOWNER",
                        "meta",
                        &profiles,
                        &control,
                        Some(&pool),
                        None,
                        Some(config),
                    )
                    .await
                    .unwrap();

                    // A reply in the thread resumes the same engine thread.
                    let follow = frame("UOWNER", "100.2", Some("100.1"), "and now?");
                    apply_frame_routed(
                        &follow,
                        "UOWNER",
                        "meta",
                        &profiles,
                        &control,
                        Some(&pool),
                        None,
                        Some(config),
                    )
                    .await
                    .unwrap();
                    wait_posts(4).await;
                },
            )
            .await;

        assert_eq!(posts(&calls).len(), 4);
        assert!(
            control.ops().is_empty(),
            "no pane was used: {:?}",
            control.ops()
        );
        let frames = omo.frames.lock().unwrap().clone();
        assert_eq!(frames.iter().filter(|f| f["type"] == "get_state").count(), 2);
        let launches: Vec<_> = frames.iter().filter(|f| f["type"] == "fixture_launch").collect();
        let args = launches[1]["args"].as_array().unwrap();
        let resume = args.iter().position(|arg| arg == "--session").unwrap();
        assert_eq!(args[resume + 1], omo.session_file.to_string_lossy().as_ref());
        assert!(frames.iter().all(|frame| frame.get("sessionId").is_none()));
        let prompts: Vec<_> = frames.iter().filter(|f| f["type"] == "prompt").collect();
        assert_eq!(prompts.len(), 2);
        assert!(frames.iter().any(|f| f["type"] == "set_model" && f["modelId"] == "model-x"));
        assert_eq!(prompts[0]["message"], "[Thu 1970-01-01 00:01:40 UTC] how are things?");
        let approval = frames.iter().find(|f| f["type"] == "extension_ui_response").unwrap();
        assert_eq!(approval["id"], "77");
        assert_eq!(approval["confirmed"], true);
        let state: String = sqlx::query_scalar(
            "SELECT state_json FROM sessions WHERE channel_id = 'CHOME' AND thread_id = '100.1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(state.contains(omo.session_file.to_string_lossy().as_ref()));
    }

    #[cfg(feature = "hub-slack")]
    struct Scripted {
        steps: std::sync::Mutex<std::collections::VecDeque<std::result::Result<Value, String>>>,
        ops: std::sync::Mutex<Vec<AgentOp>>,
    }

    #[cfg(feature = "hub-slack")]
    impl Scripted {
        fn new(steps: Vec<std::result::Result<Value, String>>) -> Self {
            Self {
                steps: std::sync::Mutex::new(steps.into()),
                ops: std::sync::Mutex::new(Vec::new()),
            }
        }
    }

    #[cfg(feature = "hub-slack")]
    #[async_trait::async_trait]
    impl AgentControl for Scripted {
        async fn execute(&self, _host_id: Option<&str>, op: AgentOp) -> Result<Value> {
            self.ops.lock().expect("ops").push(op);
            match self.steps.lock().expect("steps").pop_front() {
                Some(Ok(value)) => Ok(value),
                Some(Err(message)) => Err(HubError::message(message)),
                None => Ok(json!({"ok": true})),
            }
        }
    }

    #[cfg(feature = "hub-slack")]
    fn click_frame(user: &str, value: &str, label: &str) -> String {
        json!({
            "envelope_id": "e-click",
            "type": "interactive",
            "payload": {
                "type": "block_actions",
                "user": {"id": user},
                "channel": {"id": "CHOME"},
                "message": {
                    "ts": "1.0",
                    "text": "❓ pane: ship it?",
                    "blocks": [
                        {"type": "section", "text": {"type": "mrkdwn", "text": "❓ pane: ship it?"}},
                        {"type": "actions", "block_id": "ferryx_approval_same", "elements": [{"type": "button"}]}
                    ]
                },
                "actions": [{"value": value, "text": {"type": "plain_text", "text": label}}]
            }
        })
        .to_string()
    }

    #[cfg(feature = "hub-slack")]
    fn calls_named<'a>(log: &'a [(String, Value)], method: &str) -> Vec<&'a Value> {
        log.iter()
            .filter(|(name, _)| name == method)
            .map(|(_, body)| body)
            .collect()
    }

    #[cfg(feature = "hub-slack")]
    #[tokio::test]
    async fn an_answer_updates_the_card_and_a_conflict_is_ephemeral() {
        let (port, log) = serve_slack_api().await;
        SLACK_API_TEST
            .scope(
                (format!("http://127.0.0.1:{port}"), "xoxb-test".into()),
                async {
                    let control = Scripted::new(vec![
                        Ok(json!({"ok": true})),
                        Ok(json!({"ok": true})),
                        Err(
                            "REQUEST_CONFLICT this request id was already used with a different payload"
                                .into(),
                        ),
                    ]);
                    let profiles = crate::chat::router::ProfileRouter::new(vec![]);
                    let ship = click_frame(
                        "UOWNER",
                        "ferryx:approval:pane-1:ship-prompt:rev-1:2:once",
                        "Ship it",
                    );
                    apply_frame(&ship, "UOWNER", "meta", &profiles, &control)
                        .await
                        .unwrap();
                    apply_frame(&ship, "UOWNER", "meta", &profiles, &control)
                        .await
                        .unwrap();
                    let other = click_frame(
                        "UOWNER",
                        "ferryx:approval:pane-1:ship-prompt:rev-1:0:once",
                        "No",
                    );
                    apply_frame(&other, "UOWNER", "meta", &profiles, &control)
                        .await
                        .unwrap();
                    let recorded = log.lock().expect("log").clone();
                    let updates = calls_named(&recorded, "chat.update");
                    let notes = calls_named(&recorded, "chat.postEphemeral");
                    assert_eq!(
                        updates.len(),
                        2,
                        "resolved card refreshed on the repeat: {recorded:?}"
                    );
                    assert_eq!(updates[0]["channel"], "CHOME");
                    assert_eq!(updates[0]["ts"], "1.0");
                    let blocks = updates[0]["blocks"].as_array().unwrap();
                    assert!(
                        blocks.iter().all(|block| block["type"] != "actions"),
                        "{blocks:?}"
                    );
                    let line = blocks
                        .iter()
                        .find(|block| block["type"] == "context")
                        .unwrap()["elements"][0]["text"]
                        .as_str()
                        .unwrap();
                    assert!(
                        line.starts_with("✅ Ship it — answered by <@UOWNER> at <!date^"),
                        "{line}"
                    );
                    let (unix, rest) = line.split_once("<!date^").unwrap().1.split_once('^').unwrap();
                    assert!(unix.chars().all(|c| c.is_ascii_digit()), "{line}");
                    let fallback = chrono::DateTime::from_timestamp(unix.parse().unwrap(), 0)
                        .unwrap()
                        .format("%Y-%m-%d %H:%M UTC");
                    assert_eq!(
                        rest,
                        format!("{{date_short_pretty}} {{time}}|{fallback}>"),
                        "{line}"
                    );
                    assert!(notes.len() == 1, "{recorded:?}");
                    assert_eq!(notes[0]["user"], "UOWNER");
                    assert_eq!(notes[0]["channel"], "CHOME");
                    assert_eq!(notes[0]["text"], "already answered with Ship it");
                    let ids: Vec<_> = control
                        .ops
                        .lock()
                        .unwrap()
                        .iter()
                        .map(|op| match op {
                            AgentOp::Answer { request_id, .. } => request_id.clone(),
                            other => panic!("expected answers, got {other:?}"),
                        })
                        .collect();
                    assert_eq!(ids.len(), 3);
                    assert!(ids.iter().all(|id| id == &ids[0]), "{ids:?}");
                },
            )
            .await;
    }

    #[cfg(feature = "hub-slack")]
    #[tokio::test]
    async fn a_non_owner_click_is_ephemeral_and_a_stale_card_shows_the_error() {
        let (port, log) = serve_slack_api().await;
        SLACK_API_TEST
            .scope(
                (format!("http://127.0.0.1:{port}"), "xoxb-test".into()),
                async {
                    let control = Scripted::new(vec![Err(
                        "REQUEST_CONFLICT the screen changed since the card was read; nothing was sent"
                            .into(),
                    )]);
                    let profiles = crate::chat::router::ProfileRouter::new(vec![]);
                    let stranger = click_frame(
                        "USTRANGER",
                        "ferryx:approval:pane-9:stale-prompt:rev-9:1:once",
                        "Ship it",
                    );
                    apply_frame(&stranger, "UOWNER", "meta", &profiles, &control)
                        .await
                        .unwrap();
                    assert!(control.ops.lock().unwrap().is_empty());
                    let owner = click_frame(
                        "UOWNER",
                        "ferryx:approval:pane-9:stale-prompt:rev-9:1:once",
                        "Ship it",
                    );
                    apply_frame(&owner, "UOWNER", "meta", &profiles, &control)
                        .await
                        .unwrap();
                    let recorded = log.lock().expect("log").clone();
                    let notes = calls_named(&recorded, "chat.postEphemeral");
                    let updates = calls_named(&recorded, "chat.update");
                    assert_eq!(notes.len(), 2, "{recorded:?}");
                    assert_eq!(notes[0]["user"], "USTRANGER");
                    assert_eq!(notes[0]["text"], "only the owner can answer");
                    assert_eq!(notes[1]["user"], "UOWNER");
                    assert_eq!(notes[1]["text"], "prompt no longer on screen");
                    assert_eq!(updates.len(), 1, "{recorded:?}");
                    let blocks = updates[0]["blocks"].as_array().unwrap();
                    assert!(blocks.iter().all(|block| block["type"] != "actions"));
                    let line = blocks
                        .iter()
                        .find(|block| block["type"] == "context")
                        .unwrap()["elements"][0]["text"]
                        .as_str()
                        .unwrap();
                    assert!(line.contains("prompt no longer on screen"), "{line}");
                },
            )
            .await;
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
    #[tokio::test]
    async fn socket_mode_state_reports_a_live_socket_and_clears_when_the_task_is_aborted() {
        let http = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let ws = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let http_port = http.local_addr().unwrap().port();
        let ws_port = ws.local_addr().unwrap().port();
        spawn_connections_open(http, format!("ws://127.0.0.1:{ws_port}/"));
        tokio::spawn(async move {
            while let Ok((tcp, _)) = ws.accept().await {
                tokio::spawn(async move {
                    use futures_util::StreamExt;
                    let Ok(mut socket) = tokio_tungstenite::accept_async(tcp).await else {
                        return;
                    };
                    // Answer pings so the client keeps this socket.
                    while let Some(Ok(_)) = socket.next().await {}
                });
            }
        });
        static STATE: SocketModeState = SocketModeState::new();
        assert!(!STATE.snapshot().running);
        let pace = test_pace();
        let creds = test_credentials(format!("http://127.0.0.1:{http_port}"));
        let client = tokio::spawn(async move {
            let _ = run_socket_mode_tracked(&STATE, move || Ok(creds.clone()), pace, |_| async {})
                .await;
        });
        tokio::time::timeout(Duration::from_secs(3), async {
            while !STATE.snapshot().connected {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("socket mode never reported connected");
        let live = STATE.snapshot();
        assert!(live.running);
        assert!(live.connects >= 1);
        assert_eq!(live.last_error, None);
        client.abort();
        let _ = client.await;
        let stopped = STATE.snapshot();
        assert!(
            !stopped.running,
            "an aborted task (hub stop) is not running"
        );
        assert!(!stopped.connected, "an aborted task has no socket");
    }

    #[test]
    fn socket_errors_in_status_drop_urls_and_tokens() {
        let message = redact_socket_error(
            "connect failed wss://wss-primary.slack.com/link/?ticket=abc xoxb-123 refused",
        );
        assert_eq!(message, "connect failed <redacted> <redacted> refused");
        assert!(redact_socket_error(&"x".repeat(500)).chars().count() <= 203);
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

    #[cfg(feature = "hub-slack")]
    #[tokio::test]
    async fn a_spoken_reply_uploads_the_wav_and_completes_it() {
        use axum::extract::State;
        let bytes_seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
        let ticket_seen = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let complete_seen = std::sync::Arc::new(std::sync::Mutex::new(Value::Null));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        #[derive(Clone)]
        struct UploadState {
            port: u16,
            bytes: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
            ticket: std::sync::Arc<std::sync::Mutex<String>>,
            complete: std::sync::Arc<std::sync::Mutex<Value>>,
        }
        let state = UploadState {
            port,
            bytes: bytes_seen.clone(),
            ticket: ticket_seen.clone(),
            complete: complete_seen.clone(),
        };
        let app = axum::Router::new()
            .route(
                "/files.getUploadURLExternal",
                axum::routing::post(|State(state): State<UploadState>, body: String| async move {
                    *state.ticket.lock().expect("ticket") = body;
                    axum::Json(json!({
                        "ok": true,
                        "upload_url": format!("http://127.0.0.1:{}/bytes", state.port),
                        "file_id": "F1"
                    }))
                }),
            )
            .route(
                "/bytes",
                axum::routing::post(|State(state): State<UploadState>, body: axum::body::Bytes| async move {
                    *state.bytes.lock().expect("bytes") = body.to_vec();
                    axum::http::StatusCode::OK
                }),
            )
            .route(
                "/files.completeUploadExternal",
                axum::routing::post(|State(state): State<UploadState>, axum::Json(body): axum::Json<Value>| async move {
                    *state.complete.lock().expect("complete") = body;
                    axum::Json(json!({"ok": true}))
                }),
            )
            .with_state(state);
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let wav = vec![1_u8, 2, 3, 4];
        SLACK_API_TEST
            .scope(
                (format!("http://127.0.0.1:{port}"), "xoxb-test".into()),
                upload_external(
                    "C1",
                    Some("1700.1"),
                    "voice-reply.wav",
                    wav.clone(),
                    "Voice reply",
                ),
            )
            .await
            .unwrap();
        let ticket = ticket_seen.lock().expect("ticket").clone();
        assert!(ticket.contains("filename=voice-reply.wav"), "{ticket}");
        assert!(ticket.contains("length=4"), "{ticket}");
        assert_eq!(*bytes_seen.lock().expect("bytes"), wav);
        let complete = complete_seen.lock().expect("complete").clone();
        assert_eq!(complete["channel_id"], "C1");
        assert_eq!(complete["thread_ts"], "1700.1");
        assert_eq!(complete["files"][0]["id"], "F1");
        assert_eq!(complete["files"][0]["title"], "Voice reply");
    }
}
