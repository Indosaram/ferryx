//! One omo app-server turn, speaking the same JSON-RPC sequence omo-gateway uses:
//! connect (Bearer token when one is configured), `initialize` and wait for its reply,
//! `thread/resume` (or `thread/start` when there is nothing to resume) and wait for its reply,
//! `turn/start` and wait for the turn id, then read events until our `turn/completed`.
//!
//! Approval requests follow omo-gateway's policy: full access (the default, `OMON_FULL_ACCESS`
//! unset) approves; `APPROVAL_MODE=always` without full access declines; otherwise local,
//! reversible effects are approved and anything else is declined with omo's reason. Five
//! declines abort the turn. `item/tool/requestUserInput` is cancelled because a chat turn has
//! nobody to answer it mid-turn.
use std::path::PathBuf;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::mpsc::UnboundedSender;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

use super::protocol;
use crate::effect::Effect;
use crate::error::{HubError, Result};

pub const DEFAULT_URL: &str = "ws://127.0.0.1:19742";
pub const APPROVAL_DENIAL_TURN_LIMIT: u32 = 5;
const DENIAL_REASON: &str =
    "denied by gateway policy - do not retry this tool; ask the user for the information instead";

#[derive(Debug, Clone)]
pub struct OmoClientConfig {
    pub url: String,
    pub token: Option<String>,
    /// Longest silence from the engine before the turn is interrupted.
    pub idle_timeout: Duration,
    /// Hard cap for the whole turn, connect included.
    pub total_timeout: Duration,
    pub connect_timeout: Duration,
    pub workspace: Option<protocol::Workspace>,
}

impl OmoClientConfig {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            token: None,
            idle_timeout: Duration::from_secs(1800),
            total_timeout: Duration::from_secs(3600),
            connect_timeout: Duration::from_secs(15),
            workspace: None,
        }
    }

    /// Timeouts from `OMON_OMO_TURN_TIMEOUT_SECS` (idle, default 1800s) and
    /// `OMON_OMO_TURN_TOTAL_TIMEOUT_SECS` (total, default 3600s), as omo-gateway reads them.
    pub fn with_env_timeouts(mut self) -> Self {
        let secs = |name: &str| {
            crate::hub_env::var(name)
                .ok()
                .and_then(|value| value.trim().parse::<u64>().ok())
                .filter(|value| *value > 0)
                .map(Duration::from_secs)
        };
        if let Some(idle) = secs("OMON_OMO_TURN_TIMEOUT_SECS") {
            self.idle_timeout = idle;
        }
        if let Some(total) = secs("OMON_OMO_TURN_TOTAL_TIMEOUT_SECS") {
            self.total_timeout = total;
        }
        self
    }
}

#[derive(Debug, Clone, Default)]
pub struct TurnRequest {
    /// Engine thread to resume. `None`, or a thread the engine no longer has, starts a new one.
    pub thread_id: Option<String>,
    pub system_prompt: Option<String>,
    pub model: Option<String>,
    /// `enabledToolsets` for a new thread.
    pub toolsets: Option<Vec<String>>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnOutcome {
    pub thread_id: String,
    pub resumed: bool,
    pub turn_id: String,
    /// Agent text produced after the last relayed status (reasoning tags removed).
    pub text: String,
    pub approvals_allowed: u32,
    pub approvals_denied: u32,
    /// Interim status messages sent on `progress` during the turn.
    pub messages_delivered: usize,
    pub tool_calls: usize,
}

/// How often an interim narration may reach the user after the first one (omo-gateway: 180s).
pub const INTERIM_REPORT_INTERVAL: Duration = Duration::from_secs(180);

/// omo-gateway's rule: the first narration is the acknowledgement, later ones only when the
/// previous one is old enough.
pub fn should_relay_interim(
    delivered: usize,
    last_relay_at: Option<tokio::time::Instant>,
    now: tokio::time::Instant,
) -> bool {
    delivered == 0
        || last_relay_at.is_none_or(|at| now.duration_since(at) >= INTERIM_REPORT_INTERVAL)
}

/// Removes `<think>`, `<thought>` and `<reasoning>` blocks, as omo-gateway does before posting.
pub fn filter_reasoning(text: &str) -> String {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(
            r"(?is)<\s*(?:think|thought|reasoning)\b[^>]*>.*?(?:<\s*/\s*(?:think|thought|reasoning)\s*>|$)",
        )
        .expect("valid reasoning regex")
    });
    re.replace_all(text, "").trim().to_owned()
}

/// omo-gateway's intentional silence tokens (`[SILENT]`, `NO_REPLY`, "(no response)", "…").
pub fn is_explicit_silence(text: &str) -> bool {
    static NARRATION: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    const SENTINELS: &[&str] = &["[SILENT]", "SILENT", "NO_REPLY", "NO REPLY"];
    let stripped = text.trim();
    if stripped.is_empty() || stripped.chars().count() > 64 {
        return false;
    }
    let normalize = |value: &str| {
        value
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_uppercase()
    };
    if SENTINELS.contains(&normalize(stripped).as_str()) {
        return true;
    }
    let edge = stripped.trim_matches(|c: char| c.is_ascii_punctuation() && c != '[' && c != ']');
    if !edge.is_empty() && SENTINELS.contains(&normalize(edge).as_str()) {
        return true;
    }
    NARRATION
        .get_or_init(|| {
            regex::Regex::new(
                r"(?i)^[\s*_~`]*\(?\s*(silent|silence|no\s+response|no\s+reply)\s*\.?\)?[\s*_~`]*$|^[\s*_~`]*[\x{1f507}\.\x{2026}]+[\s*_~`]*$",
            )
            .expect("valid silence regex")
        })
        .is_match(stripped)
}

/// What a chat posts when the turn completes, following omo-gateway: nothing for a silence
/// token; the text; "✅ Done." when the user has seen nothing yet; an error for a turn that
/// produced no text, no tool call and no interim message.
pub fn final_reply(outcome: &TurnOutcome) -> Result<Option<String>> {
    if is_explicit_silence(&outcome.text) {
        return Ok(None);
    }
    if outcome.text.trim().is_empty() {
        if outcome.tool_calls == 0 && outcome.messages_delivered == 0 {
            return Err(HubError::message(
                "omo turn ended with no content (likely upstream LLM failure)",
            ));
        }
        if outcome.messages_delivered > 0 {
            return Ok(None);
        }
        return Ok(Some("✅ Done.".into()));
    }
    Ok(Some(outcome.text.clone()))
}

/// Approval verdict for one engine approval frame: `None` approves, `Some(reason)` declines.
pub fn approval_verdict(frame: &Value, cwd: Option<&str>) -> Option<String> {
    let full_access = crate::hub_env::var("OMON_FULL_ACCESS").map_or(true, |value| {
        !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "no" | "off"
        )
    });
    if full_access {
        return None;
    }
    let always = crate::hub_env::var("APPROVAL_MODE")
        .or_else(|_| crate::hub_env::var("APPROVAL_POLICY"))
        .is_ok_and(|value| value.trim() == "always");
    if always {
        return Some(DENIAL_REASON.into());
    }
    let classification = crate::effect::classify_frame(frame, cwd);
    match classification.effect {
        Effect::Read | Effect::Draft | Effect::WriteLocal | Effect::VcsLocal => None,
        other => Some(format!(
            "{DENIAL_REASON} ({other}: {})",
            classification.summary
        )),
    }
}

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn connect(config: &OmoClientConfig, deadline: tokio::time::Instant) -> Result<Ws> {
    let connect_until = (tokio::time::Instant::now() + config.connect_timeout).min(deadline);
    loop {
        let mut request = config
            .url
            .as_str()
            .into_client_request()
            .map_err(|error| HubError::message(format!("omo app-server url: {error}")))?;
        if let Some(token) = config.token.as_deref().filter(|token| !token.is_empty()) {
            let value = format!("Bearer {token}")
                .parse()
                .map_err(|_| HubError::message("omo app-server token is not a valid header"))?;
            request.headers_mut().insert("Authorization", value);
        }
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            tokio_tungstenite::connect_async(request),
        )
        .await;
        let last_error = match result {
            Ok(Ok((ws, _))) => return Ok(ws),
            Ok(Err(error)) => error.to_string(),
            Err(_) => "handshake timed out".to_owned(),
        };
        if tokio::time::Instant::now() + Duration::from_millis(500) >= connect_until {
            return Err(HubError::message(format!(
                "failed to connect to omo app-server at {}: {last_error}",
                config.url
            )));
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

async fn send(ws: &mut Ws, text: String) -> Result<()> {
    ws.send(Message::text(text))
        .await
        .map_err(|error| HubError::message(format!("omo app-server send: {error}")))
}

/// Next JSON frame, or an error at `until`. Pings, pongs and binary frames are skipped.
async fn next_frame(ws: &mut Ws, until: tokio::time::Instant, what: &str) -> Result<Value> {
    loop {
        let message = match tokio::time::timeout_at(until, ws.next()).await {
            Err(_) => return Err(HubError::message(format!("timed out waiting for {what}"))),
            Ok(None) => {
                return Err(HubError::message(format!(
                    "omo app-server closed the connection while waiting for {what}"
                )))
            }
            Ok(Some(Err(error))) => {
                return Err(HubError::message(format!("omo app-server read: {error}")))
            }
            Ok(Some(Ok(message))) => message,
        };
        let text = match message {
            Message::Text(text) => text.to_string(),
            Message::Close(_) => {
                return Err(HubError::message(format!(
                    "omo app-server closed the connection while waiting for {what}"
                )))
            }
            _ => continue,
        };
        if let Ok(value) = serde_json::from_str::<Value>(&text) {
            return Ok(value);
        }
    }
}

fn is_response_to(frame: &Value, id: u64) -> bool {
    frame.get("method").is_none() && frame.get("id").and_then(Value::as_u64) == Some(id)
}

fn rpc_error(frame: &Value) -> Option<String> {
    let error = frame.get("error")?;
    Some(
        error
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| error.to_string()),
    )
}

/// Answers a server request that arrives before the turn starts, so the engine never waits on us.
async fn answer_server_request(
    ws: &mut Ws,
    frame: &Value,
    cwd: Option<&str>,
    allowed: &mut u32,
    denied: &mut u32,
) -> Result<bool> {
    let (Some(method), Some(id)) = (frame.get("method").and_then(Value::as_str), frame.get("id"))
    else {
        return Ok(false);
    };
    if let Some(reply) = protocol::user_input_cancellation(frame) {
        send(ws, reply).await?;
        return Ok(true);
    }
    if protocol::is_approval_request(method) {
        match approval_verdict(frame, cwd) {
            None => {
                *allowed += 1;
                send(ws, protocol::approval_allow_response(id)).await?;
            }
            Some(reason) => {
                *denied += 1;
                send(ws, protocol::approval_denial_with_reason(id, &reason)).await?;
            }
        }
        return Ok(true);
    }
    send(
        ws,
        json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": format!("{method} is not supported by this client")}})
            .to_string(),
    )
    .await?;
    Ok(true)
}

/// Waits for the reply to request `id`, answering any server request on the way.
async fn await_reply(
    ws: &mut Ws,
    id: u64,
    until: tokio::time::Instant,
    what: &str,
    cwd: Option<&str>,
    counters: (&mut u32, &mut u32),
) -> Result<Value> {
    let (allowed, denied) = counters;
    loop {
        let frame = next_frame(ws, until, what).await?;
        if is_response_to(&frame, id) {
            return Ok(frame);
        }
        answer_server_request(ws, &frame, cwd, allowed, denied).await?;
    }
}

fn is_missing_thread_error(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    lower.contains("no rollout found")
        || lower.contains("thread not found")
        || lower.contains("unknown thread")
}

fn is_thread_busy_error(error: &str) -> bool {
    error.to_ascii_lowercase().contains("already processing")
}

/// Runs one turn. Every completed agent message except the last is sent on `progress` as it
/// lands, so a long turn shows its interim replies. The last one is the outcome text.
pub async fn run_turn(
    config: &OmoClientConfig,
    request: &TurnRequest,
    progress: Option<UnboundedSender<String>>,
) -> Result<TurnOutcome> {
    let deadline = tokio::time::Instant::now() + config.total_timeout;
    let cwd = config
        .workspace
        .as_ref()
        .map(|workspace| workspace.cwd.clone());
    let cwd = cwd.as_deref();
    let mut ws = connect(config, deadline).await?;
    let mut allowed = 0u32;
    let mut denied = 0u32;
    let handshake_until = (tokio::time::Instant::now() + Duration::from_secs(60)).min(deadline);

    send(&mut ws, protocol::initialize_request()).await?;
    let reply = await_reply(
        &mut ws,
        1,
        handshake_until,
        "the initialize reply",
        cwd,
        (&mut allowed, &mut denied),
    )
    .await?;
    if let Some(error) = rpc_error(&reply) {
        return Err(HubError::message(format!("omo initialize failed: {error}")));
    }

    let mut resumed = false;
    let mut thread_id = None;
    if let Some(existing) = request.thread_id.as_deref().filter(|id| !id.is_empty()) {
        send(&mut ws, protocol::thread_resume_request(existing)).await?;
        let reply = await_reply(
            &mut ws,
            2,
            handshake_until,
            "the thread/resume reply",
            cwd,
            (&mut allowed, &mut denied),
        )
        .await?;
        match rpc_error(&reply) {
            None => {
                resumed = true;
                thread_id = Some(
                    reply
                        .pointer("/result/thread/id")
                        .and_then(Value::as_str)
                        .unwrap_or(existing)
                        .to_owned(),
                );
            }
            Some(error) if is_missing_thread_error(&error) => {
                tracing::info!("omo thread is gone; starting a fresh one");
            }
            Some(error) => {
                return Err(HubError::message(format!(
                    "omo thread/resume failed: {error}"
                )))
            }
        }
    }
    let thread_id = match thread_id {
        Some(id) => id,
        None => {
            send(
                &mut ws,
                protocol::thread_start_request(
                    request.system_prompt.as_deref(),
                    request.model.as_deref(),
                    config.workspace.as_ref(),
                    request.toolsets.as_deref(),
                ),
            )
            .await?;
            let reply = await_reply(
                &mut ws,
                2,
                handshake_until,
                "the thread/start reply",
                cwd,
                (&mut allowed, &mut denied),
            )
            .await?;
            if let Some(error) = rpc_error(&reply) {
                return Err(HubError::message(format!(
                    "omo thread/start failed: {error}"
                )));
            }
            reply
                .pointer("/result/thread/id")
                .or_else(|| reply.pointer("/result/threadId"))
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
                .ok_or_else(|| HubError::message("failed to obtain thread id from thread/start"))?
                .to_owned()
        }
    };

    // turn/start, retrying while the engine still finishes an earlier run on this thread.
    let mut early: Vec<Value> = Vec::new();
    let mut backoff = Duration::from_secs(2);
    let turn_id = loop {
        send(
            &mut ws,
            protocol::turn_start_request(&thread_id, &request.text, request.model.as_deref()),
        )
        .await?;
        let until = (tokio::time::Instant::now() + Duration::from_secs(120)).min(deadline);
        let reply = loop {
            let frame = next_frame(&mut ws, until, "the turn/start reply").await?;
            if is_response_to(&frame, 3) {
                break frame;
            }
            if !answer_server_request(&mut ws, &frame, cwd, &mut allowed, &mut denied).await? {
                early.push(frame);
            }
        };
        match rpc_error(&reply) {
            None => {
                break reply
                    .pointer("/result/turn/id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned()
            }
            Some(error) if is_thread_busy_error(&error) => {
                if tokio::time::Instant::now() + backoff >= deadline {
                    return Err(HubError::message(format!("omo turn/start failed: {error}")));
                }
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(30));
            }
            Some(error) => {
                return Err(HubError::message(format!("omo turn/start failed: {error}")))
            }
        }
    };

    // omo-gateway's buffer: deltas accumulate, a completed agent message replaces them, and a
    // tool start relays (or drops) what accumulated as an interim status.
    let mut content = String::new();
    let mut delivered = 0usize;
    let mut last_relay_at: Option<tokio::time::Instant> = None;
    let mut tool_calls = 0usize;
    let mut started_items: std::collections::HashSet<String> = std::collections::HashSet::new();
    let ours = |frame: &Value| {
        let id = frame.pointer("/params/turn/id").and_then(Value::as_str);
        let thread = frame.pointer("/params/threadId").and_then(Value::as_str);
        (turn_id.is_empty() || id.is_none() || id == Some(turn_id.as_str()))
            && (thread.is_none() || thread == Some(thread_id.as_str()))
    };
    let mut pending: std::collections::VecDeque<Value> = early.into();
    let mut last_activity = tokio::time::Instant::now();
    loop {
        let frame = match pending.pop_front() {
            Some(frame) => frame,
            None => {
                let until = (last_activity + config.idle_timeout).min(deadline);
                match next_frame(&mut ws, until, "the turn to finish").await {
                    Ok(frame) => frame,
                    Err(error) => {
                        let interrupt = json!({
                            "jsonrpc": "2.0", "id": 9_002, "method": "turn/interrupt",
                            "params": {"threadId": thread_id, "turnId": turn_id}
                        });
                        let _ = send(&mut ws, interrupt.to_string()).await;
                        let _ = ws.close(None).await;
                        return Err(error);
                    }
                }
            }
        };
        last_activity = tokio::time::Instant::now();
        let method = frame.get("method").and_then(Value::as_str).unwrap_or("");
        if frame.get("id").is_some() && !method.is_empty() {
            answer_server_request(&mut ws, &frame, cwd, &mut allowed, &mut denied).await?;
            if denied >= APPROVAL_DENIAL_TURN_LIMIT {
                let interrupt = json!({
                    "jsonrpc": "2.0", "id": 9_002, "method": "turn/interrupt",
                    "params": {"threadId": thread_id, "turnId": turn_id}
                });
                let _ = send(&mut ws, interrupt.to_string()).await;
                let _ = ws.close(None).await;
                return Err(HubError::message(format!(
                    "tool approval denied {denied} times by gateway policy; turn aborted"
                )));
            }
            continue;
        }
        match method {
            "item/agentMessage/delta" => {
                if let Some(delta) = frame.pointer("/params/delta").and_then(Value::as_str) {
                    content.push_str(delta);
                }
            }
            "item/started" => {
                let item_id = frame
                    .pointer("/params/item/id")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let item_type = frame
                    .pointer("/params/item/type")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let fresh = item_id.is_empty() || started_items.insert(item_id.to_owned());
                if fresh && !matches!(item_type, "agentMessage" | "userMessage" | "reasoning" | "")
                {
                    tool_calls += 1;
                    let now = tokio::time::Instant::now();
                    if !content.trim().is_empty()
                        && !should_relay_interim(delivered, last_relay_at, now)
                    {
                        content.clear();
                    }
                    if !content.trim().is_empty() {
                        let scrubbed = filter_reasoning(&content);
                        if !scrubbed.trim().is_empty() && !is_explicit_silence(&scrubbed) {
                            if let Some(progress) = progress.as_ref() {
                                if progress.send(scrubbed).is_ok() {
                                    delivered += 1;
                                    last_relay_at = Some(now);
                                    content.clear();
                                }
                            } else {
                                content.clear();
                            }
                        } else {
                            content.clear();
                        }
                    }
                }
            }
            "item/completed" => {
                if frame.pointer("/params/item/type").and_then(Value::as_str)
                    == Some("agentMessage")
                {
                    if let Some(text) = frame.pointer("/params/item/text").and_then(Value::as_str) {
                        content.clear();
                        content.push_str(text);
                    }
                }
            }
            "turn/completed" if ours(&frame) => {
                let _ = ws.close(None).await;
                let status = frame.pointer("/params/turn/status").and_then(Value::as_str);
                return match status {
                    Some("interrupted") => Err(HubError::message("omo turn interrupted")),
                    Some("failed") => {
                        let error = frame
                            .pointer("/params/turn/error/message")
                            .or_else(|| frame.pointer("/params/turn/error"))
                            .and_then(Value::as_str)
                            .unwrap_or("turn failed");
                        Err(HubError::message(format!("omo turn failed: {error}")))
                    }
                    _ => Ok(TurnOutcome {
                        thread_id,
                        resumed,
                        turn_id,
                        text: filter_reasoning(&content),
                        approvals_allowed: allowed,
                        approvals_denied: denied,
                        messages_delivered: delivered,
                        tool_calls,
                    }),
                };
            }
            _ => {}
        }
    }
}

/// Token for the app-server this hub supervises: the token file next to the hub state, when
/// it exists. A daemon somebody else started without a token file is reached unauthenticated.
pub fn token_from_file(path: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|token| token.trim().to_owned())
        .filter(|token| !token.is_empty())
}

/// The workspace omo-gateway gives Slack turns: `<root>/agents/bot-<persona>` plus
/// `<root>/shared`, where root is `OMON_WORKSPACE_ROOT` or `~/.omon/workspace`. Absent when the
/// directory does not exist, so the engine keeps its own default.
pub fn slack_workspace() -> Option<protocol::Workspace> {
    let root = crate::hub_env::var("OMON_WORKSPACE_ROOT")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            crate::hub_env::var_os("HOME")
                .map(|home| PathBuf::from(home).join(".omon").join("workspace"))
        })?;
    let persona = crate::hub_env::var("OMON_SLACK_PERSONA_FROM")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "1465631383862120451".into());
    let cwd = root.join("agents").join(format!("bot-{persona}"));
    if !cwd.is_dir() {
        return None;
    }
    Some(protocol::Workspace {
        cwd: cwd.display().to_string(),
        roots: vec![
            cwd.display().to_string(),
            root.join("shared").display().to_string(),
        ],
    })
}

#[cfg(test)]
pub mod mock {
    //! A scripted omo app-server: answers the handshake, then streams agent messages and a
    //! `turn/completed`. It records every frame and the Authorization header it saw.
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::net::TcpListener;

    #[derive(Clone, Default)]
    pub struct MockLog {
        pub frames: Arc<Mutex<Vec<Value>>>,
        pub auth: Arc<Mutex<Vec<Option<String>>>>,
    }

    #[derive(Clone)]
    pub struct Script {
        pub messages: Vec<String>,
        pub resume_error: Option<String>,
        pub approval: bool,
        pub thread_id: String,
    }

    #[allow(clippy::result_large_err)]
    pub async fn serve(script: Script) -> (String, MockLog) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let log = MockLog::default();
        let served = log.clone();
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let log = served.clone();
                let script = script.clone();
                tokio::spawn(async move {
                    let auth = Arc::new(Mutex::new(None));
                    let seen = auth.clone();
                    let callback = move |req: &tokio_tungstenite::tungstenite::handshake::server::Request,
                                         resp: tokio_tungstenite::tungstenite::handshake::server::Response| {
                        *seen.lock().unwrap() = req
                            .headers()
                            .get("authorization")
                            .and_then(|value| value.to_str().ok())
                            .map(str::to_owned);
                        Ok(resp)
                    };
                    let Ok(mut ws) = tokio_tungstenite::accept_hdr_async(stream, callback).await
                    else {
                        return;
                    };
                    log.auth.lock().unwrap().push(auth.lock().unwrap().clone());
                    let out = |value: Value| Message::text(value.to_string());
                    while let Some(Ok(Message::Text(text))) = ws.next().await {
                        let frame: Value = serde_json::from_str(&text).unwrap();
                        log.frames.lock().unwrap().push(frame.clone());
                        let id = frame.get("id").cloned().unwrap_or(Value::Null);
                        match frame.get("method").and_then(Value::as_str) {
                            Some("initialize") => {
                                ws.send(out(
                                    json!({"jsonrpc":"2.0","id":id,"result":{"userAgent":"mock"}}),
                                ))
                                .await
                                .unwrap();
                            }
                            Some("thread/resume") => {
                                let reply = match &script.resume_error {
                                    Some(error) => {
                                        json!({"jsonrpc":"2.0","id":id,"error":{"code":-32000,"message":error}})
                                    }
                                    None => {
                                        json!({"jsonrpc":"2.0","id":id,"result":{"thread":{"id":frame.pointer("/params/threadId")}}})
                                    }
                                };
                                ws.send(out(reply)).await.unwrap();
                            }
                            Some("thread/start") => {
                                // A notification before the reply: the client must keep waiting for id 2.
                                ws.send(out(json!({"jsonrpc":"2.0","method":"thread/started","params":{"thread":{"id":script.thread_id}}}))).await.unwrap();
                                ws.send(out(json!({"jsonrpc":"2.0","id":id,"result":{"thread":{"id":script.thread_id}}}))).await.unwrap();
                            }
                            Some("turn/start") => {
                                let thread = frame
                                    .pointer("/params/threadId")
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                    .to_owned();
                                ws.send(out(json!({"jsonrpc":"2.0","id":id,"result":{"turn":{"id":"turn-1","status":"inProgress"}}}))).await.unwrap();
                                if script.approval {
                                    ws.send(out(json!({"jsonrpc":"2.0","id":77,"method":"item/commandExecution/requestApproval","params":{"threadId":thread,"command":"ls"}}))).await.unwrap();
                                    // Wait for the answer before continuing.
                                    if let Some(Ok(Message::Text(text))) = ws.next().await {
                                        log.frames
                                            .lock()
                                            .unwrap()
                                            .push(serde_json::from_str(&text).unwrap());
                                    }
                                }
                                for (index, message) in script.messages.iter().enumerate() {
                                    if index > 0 {
                                        ws.send(out(json!({"jsonrpc":"2.0","method":"item/started","params":{"threadId":thread,"turnId":"turn-1","item":{"id":format!("tool-{index}"),"type":"commandExecution"}}}))).await.unwrap();
                                    }
                                    for chunk in message.as_bytes().chunks(4) {
                                        ws.send(out(json!({"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"threadId":thread,"turnId":"turn-1","delta":String::from_utf8_lossy(chunk)}}))).await.unwrap();
                                    }
                                    ws.send(out(json!({"jsonrpc":"2.0","method":"item/completed","params":{"threadId":thread,"turnId":"turn-1","item":{"type":"agentMessage","text":message}}}))).await.unwrap();
                                }
                                ws.send(out(json!({"jsonrpc":"2.0","method":"turn/completed","params":{"threadId":thread,"turn":{"id":"turn-1","status":"completed"}}}))).await.unwrap();
                            }
                            _ => {}
                        }
                    }
                });
            }
        });
        (url, log)
    }
}

#[cfg(test)]
mod tests {
    use super::mock::*;
    use super::*;

    fn script(messages: &[&str]) -> Script {
        Script {
            messages: messages.iter().map(|m| (*m).to_owned()).collect(),
            resume_error: None,
            approval: false,
            thread_id: "thr-new".into(),
        }
    }

    #[tokio::test]
    async fn a_turn_waits_for_each_reply_and_returns_the_last_message() {
        let (url, log) = serve(Script {
            approval: true,
            ..script(&["first", "final answer"])
        })
        .await;
        let mut config = OmoClientConfig::new(url);
        config.token = Some("tok-123".into());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let outcome = run_turn(
            &config,
            &TurnRequest {
                text: "hello".into(),
                system_prompt: Some("sys".into()),
                ..Default::default()
            },
            Some(tx),
        )
        .await
        .unwrap();
        assert_eq!(outcome.thread_id, "thr-new");
        assert_eq!(outcome.turn_id, "turn-1");
        assert!(!outcome.resumed);
        assert_eq!(outcome.text, "final answer");
        assert_eq!(rx.recv().await.as_deref(), Some("first"));
        assert_eq!(outcome.approvals_allowed, 1);
        assert_eq!(
            log.auth.lock().unwrap()[0].as_deref(),
            Some("Bearer tok-123")
        );
        let frames = log.frames.lock().unwrap().clone();
        let methods: Vec<_> = frames
            .iter()
            .filter_map(|f| f.get("method").and_then(Value::as_str))
            .collect();
        assert_eq!(methods, ["initialize", "thread/start", "turn/start"]);
        assert_eq!(frames[1]["params"]["developerInstructions"], "sys");
        assert_eq!(frames[2]["params"]["threadId"], "thr-new");
        assert_eq!(frames[2]["params"]["input"][0]["text"], "hello");
        let answer = frames
            .iter()
            .find(|f| f.get("id") == Some(&json!(77)))
            .unwrap();
        assert_eq!(answer["result"]["decision"], "accept");
    }

    #[tokio::test]
    async fn a_known_thread_is_resumed_and_a_lost_one_starts_fresh() {
        let (url, log) = serve(script(&["again"])).await;
        let config = OmoClientConfig::new(url);
        let outcome = run_turn(
            &config,
            &TurnRequest {
                thread_id: Some("thr-old".into()),
                text: "more".into(),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
        assert!(outcome.resumed);
        assert_eq!(outcome.thread_id, "thr-old");
        assert_eq!(log.auth.lock().unwrap()[0], None);

        let (url, log) = serve(Script {
            resume_error: Some("no rollout found for thread thr-old".into()),
            ..script(&["fresh"])
        })
        .await;
        let outcome = run_turn(
            &OmoClientConfig::new(url),
            &TurnRequest {
                thread_id: Some("thr-old".into()),
                text: "more".into(),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
        assert!(!outcome.resumed);
        assert_eq!(outcome.thread_id, "thr-new");
        let methods: Vec<String> = log
            .frames
            .lock()
            .unwrap()
            .iter()
            .filter_map(|f| f.get("method").and_then(Value::as_str).map(str::to_owned))
            .collect();
        assert_eq!(
            methods,
            ["initialize", "thread/resume", "thread/start", "turn/start"]
        );
    }

    #[test]
    fn final_reply_and_silence_follow_omo_gateway() {
        let outcome = |text: &str, delivered: usize, tools: usize| TurnOutcome {
            thread_id: "t".into(),
            resumed: false,
            turn_id: "u".into(),
            text: text.into(),
            approvals_allowed: 0,
            approvals_denied: 0,
            messages_delivered: delivered,
            tool_calls: tools,
        };
        assert_eq!(
            final_reply(&outcome("hi", 0, 0)).unwrap().as_deref(),
            Some("hi")
        );
        assert_eq!(final_reply(&outcome("[SILENT]", 0, 0)).unwrap(), None);
        assert_eq!(final_reply(&outcome("NO_REPLY.", 0, 0)).unwrap(), None);
        assert_eq!(
            final_reply(&outcome("", 0, 2)).unwrap().as_deref(),
            Some("✅ Done.")
        );
        assert_eq!(final_reply(&outcome("", 1, 2)).unwrap(), None);
        assert!(final_reply(&outcome("", 0, 0)).is_err());
        assert_eq!(filter_reasoning("<think>x</think> answer"), "answer");
        assert!(!is_explicit_silence("silently fixed the build"));
        let now = tokio::time::Instant::now();
        assert!(should_relay_interim(0, None, now));
        assert!(!should_relay_interim(
            1,
            Some(now),
            now + Duration::from_secs(10)
        ));
        assert!(should_relay_interim(
            1,
            Some(now),
            now + Duration::from_secs(181)
        ));
    }

    /// Runs two turns (start, then resume) against a real omo app-server:
    /// `OMO_REAL_URL=ws://127.0.0.1:PORT OMO_REAL_TOKEN_FILE=... cargo test -- --ignored real_omo`.
    #[tokio::test]
    #[ignore]
    async fn real_omo_app_server_starts_and_resumes_a_thread() {
        let url = std::env::var("OMO_REAL_URL").expect("OMO_REAL_URL");
        let token_file = std::env::var("OMO_REAL_TOKEN_FILE").expect("OMO_REAL_TOKEN_FILE");
        let mut config = OmoClientConfig::new(url);
        config.token = token_from_file(std::path::Path::new(&token_file));
        config.total_timeout = Duration::from_secs(600);
        let mut request = TurnRequest {
            thread_id: None,
            system_prompt: None,
            model: std::env::var("OMO_REAL_MODEL").ok(),
            toolsets: None,
            text: "Reply with exactly the word PONG and nothing else.".into(),
        };
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let first = run_turn(&config, &request, Some(tx))
            .await
            .expect("first turn");
        while let Ok(interim) = rx.try_recv() {
            eprintln!("interim: {} chars", interim.len());
        }
        eprintln!(
            "first: thread={} resumed={} reply={:?} tools={}",
            first.thread_id, first.resumed, first.text, first.tool_calls
        );
        assert!(!first.resumed);
        assert!(first.text.to_uppercase().contains("PONG"));
        request.thread_id = Some(first.thread_id.clone());
        request.text = "Reply with exactly the word AGAIN and nothing else.".into();
        let second = run_turn(&config, &request, None)
            .await
            .expect("second turn");
        eprintln!(
            "second: thread={} resumed={} reply={:?}",
            second.thread_id, second.resumed, second.text
        );
        assert!(second.resumed);
        assert_eq!(second.thread_id, first.thread_id);
        let mut bad = config.clone();
        bad.token = Some("wrong".into());
        bad.connect_timeout = Duration::from_secs(2);
        assert!(
            run_turn(&bad, &request, None).await.is_err(),
            "a wrong token must be refused"
        );
    }

    #[tokio::test]
    async fn an_unreachable_server_fails_within_the_connect_window() {
        let mut config = OmoClientConfig::new("ws://127.0.0.1:9");
        config.connect_timeout = Duration::from_millis(600);
        let started = std::time::Instant::now();
        let error = run_turn(
            &config,
            &TurnRequest {
                text: "x".into(),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(error.contains("failed to connect"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
