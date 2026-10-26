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
    /// omo-gateway's rolling turn/start acknowledgement window (30s).
    pub ack_window: Duration,
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
            ack_window: TURN_START_ACK_WINDOW,
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
    /// The turn must run on `model`. A switch the engine refuses for context size starts a
    /// fresh thread on it instead of falling back to the thread's current model.
    pub pin_model: bool,
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
/// omo-gateway's rolling `turn/start` acknowledgement window.
pub const TURN_START_ACK_WINDOW: Duration = Duration::from_secs(30);
pub const TURN_START_ACK_TIMEOUT: &str = "timeout waiting for turn/start acknowledgement from omo app-server; submission outcome unknown, do not resubmit";

fn frame_turn_id(frame: &Value) -> Option<&str> {
    frame
        .pointer("/params/turnId")
        .or_else(|| frame.pointer("/params/turn/id"))
        .and_then(Value::as_str)
}

fn is_turn_bearing(method: &str, frame: &Value) -> bool {
    method.starts_with("turn/")
        || method.starts_with("item/")
        || method == "thread/status/changed"
        || (method == "error"
            && (frame.pointer("/params/turnId").is_some()
                || frame.pointer("/params/turn").is_some()))
}

/// omo-gateway's `PreAckState`: turn boundaries seen before the acknowledgement and whether a
/// busy refusal waits for the thread to go idle.
#[derive(Default)]
struct PreAckState {
    events: Vec<(bool, String)>,
    completed: std::collections::HashSet<String>,
    refused: std::collections::HashSet<String>,
    awaiting_idle: bool,
    refusals: u32,
    retry_at: Option<tokio::time::Instant>,
}

impl PreAckState {
    fn note_refusal(&mut self, error: &str) -> bool {
        if !is_thread_busy_error(error) {
            return false;
        }
        self.refusals = self.refusals.saturating_add(1);
        self.events.clear();
        self.awaiting_idle = true;
        let delay = Duration::from_secs(1 << self.refusals.saturating_sub(1).min(3));
        self.retry_at = Some(tokio::time::Instant::now() + delay);
        true
    }

    fn retry_due(&self, now: tokio::time::Instant) -> bool {
        self.awaiting_idle && self.retry_at.is_some_and(|at| now >= at)
    }

    fn note_resubmitted(&mut self) {
        self.awaiting_idle = false;
        self.retry_at = None;
        self.events.clear();
    }

    /// Folds one pre-acknowledgement frame of our thread. True when a fresh completion after a
    /// refusal makes the resubmission due now.
    fn observe(&mut self, frame: &Value, _thread_id: &str) -> bool {
        let Some(turn) = frame_turn_id(frame) else {
            return false;
        };
        if [
            frame.pointer("/params/turn/error"),
            frame.pointer("/params/error"),
        ]
        .into_iter()
        .flatten()
        .any(|error| is_thread_busy_error(&error.to_string()))
        {
            self.refused.insert(turn.to_owned());
            self.events.retain(|(_, id)| id != turn);
        }
        if self.refused.contains(turn) {
            return false;
        }
        match frame.get("method").and_then(Value::as_str).unwrap_or("") {
            "turn/started" => self.events.push((true, turn.to_owned())),
            "turn/completed" => {
                let fresh = self.completed.insert(turn.to_owned());
                self.events.push((false, turn.to_owned()));
                if self.awaiting_idle && fresh {
                    self.retry_at = Some(tokio::time::Instant::now());
                    return true;
                }
            }
            _ => {}
        }
        false
    }

    #[cfg(test)]
    fn clone_follow(&mut self, acked: &str) -> (String, bool) {
        let events = self.events.clone();
        let out = self.follow(acked.to_owned());
        self.events = events;
        out
    }

    /// The acknowledged id, or the successor when the run ahead of us completed under it.
    /// The flag says the successor has not started yet.
    fn follow(&mut self, acked: String) -> (String, bool) {
        let taken_over_at = self
            .events
            .iter()
            .position(|(is_start, id)| !is_start && *id == acked);
        let successor = taken_over_at.and_then(|at| {
            self.events[at + 1..]
                .iter()
                .rev()
                .find(|(is_start, id)| *is_start && *id != acked)
                .map(|(_, id)| id.clone())
        });
        let waiting = taken_over_at.is_some() && successor.is_none();
        (successor.unwrap_or(acked), waiting)
    }
}

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

/// What a chat posts when the turn completes: nothing for a silence token; the text; a line
/// saying the work ran but no answer was written when tools ran and the user has seen nothing
/// (a bare "✅ Done." told the owner nothing about what happened); an error for a turn that
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
        return Ok(Some(format!(
            "⚠️ 작업은 실행됐지만(도구 {}회) 최종 답변을 만들지 못했어요. \"결과 알려줘\"라고 보내 주시면 정리해 드릴게요.",
            outcome.tool_calls
        )));
    }
    Ok(Some(outcome.text.clone()))
}

/// The follow-up sent on the same thread when a turn ran tools but wrote no answer.
pub const FINAL_ANSWER_PROMPT: &str = "You ran tools but did not write a reply. Reply now with the final answer for the user's last message: what you did, the result, and anything left. Do not run more tools unless the answer needs them.";

/// A turn that ran tools, wrote nothing and relayed nothing: the user has seen no answer.
pub fn needs_final_answer(result: &Result<TurnOutcome>) -> bool {
    matches!(result, Ok(outcome) if outcome.text.trim().is_empty()
        && outcome.tool_calls > 0
        && outcome.messages_delivered == 0
        && !is_explicit_silence(&outcome.text))
}

/// The engine refused the requested model because the thread's live context does not fit
/// that model's window ("Model ... cannot switch: target context window ... short").
/// The turn never ran, so it is retried on the thread's current model.
pub fn is_model_switch_rejection(result: &Result<TurnOutcome>) -> bool {
    matches!(result, Err(error) if {
        let lower = error.to_string().to_lowercase();
        lower.contains("cannot switch") && lower.contains("context window")
    })
}

/// The model a failed turn is retried on: the hub's background model.
pub fn fallback_model() -> String {
    crate::model_policy::background_model()
}

/// omo-gateway's `fallback_eligible`: a turn that failed before any side effect, by producing
/// nothing at all or by a quota/overload/model rejection, runs once more on the fallback model.
pub fn fallback_eligible(result: &Result<TurnOutcome>) -> bool {
    match result {
        Ok(outcome) => {
            outcome.text.is_empty() && outcome.tool_calls == 0 && outcome.messages_delivered == 0
        }
        Err(error) => {
            let lower = error.to_string().to_lowercase();
            lower.contains("model update rejected")
                || lower.contains("model not found")
                || lower.contains("unknown model")
                || lower.contains("invalid model")
                || lower.contains("unsupported model")
        }
    }
}

/// omo-gateway's owner-facing Korean line for a failed turn; the raw error stays in the logs.
pub fn owner_failure_text(error: &str) -> String {
    format!(
        "⚠️ 응답을 만들지 못했어요 ({}). 같은 메시지를 다시 보내 주시면 이어서 할게요.",
        owner_failure_reason(error)
    )
}

fn owner_failure_reason(error: &str) -> &'static str {
    let lower = error.to_ascii_lowercase();
    if [
        "no output within",
        "turn interrupted",
        "timeout waiting for turn/start",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
    {
        "모델이 응답하지 않았어요"
    } else if lower.contains("timed out waiting for the turn to finish") {
        "작업이 턴 시간 제한을 넘겨 중단됐어요"
    } else if ["quota", "429", "rate limit"]
        .iter()
        .any(|needle| lower.contains(needle))
    {
        "모델 사용량 한도에 걸렸어요"
    } else {
        "처리 중 오류가 났어요"
    }
}

#[cfg(test)]
tokio::task_local! {
    /// Lets a test run a turn with full access off without touching the process environment.
    pub static FULL_ACCESS_TEST: bool;
}

fn full_access() -> bool {
    #[cfg(test)]
    if let Ok(value) = FULL_ACCESS_TEST.try_with(|value| *value) {
        return value;
    }
    crate::hub_env::var("OMON_FULL_ACCESS").map_or(true, |value| {
        !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "no" | "off"
        )
    })
}

/// omo-gateway is retired: the ferryx hub replaced it, and a second gateway means a second
/// Slack Socket Mode consumer, double replies and a second lane dispatcher. Engine turns have
/// restarted it on their own (`launchctl enable/bootstrap ai.omon.gateway`), so any tool
/// request that would start it is declined outright, without an owner card and regardless of
/// full access. Reading its state (`launchctl print/list`, logs) stays allowed.
pub const RETIRED_GATEWAY_REASON: &str = "omo-gateway is intentionally retired and replaced by the ferryx hub; it must never be started, enabled, kickstarted or loaded again. Do not retry; report instead.";

pub fn starts_retired_gateway(frame: &Value) -> bool {
    static PATTERNS: std::sync::OnceLock<[regex::Regex; 3]> = std::sync::OnceLock::new();
    let patterns = PATTERNS.get_or_init(|| {
        [
            regex::Regex::new(
                r"(?i)launchctl.{0,40}?\b(bootstrap|enable|kickstart|load|start|submit)\b.{0,160}?ai\.omon\.gateway",
            )
            .expect("valid regex"),
            regex::Regex::new(r"omo-gateway-launch").expect("valid regex"),
            regex::Regex::new(r#"omo-gateway[\\"',\s\]]{1,8}run\b"#).expect("valid regex"),
        ]
    });
    let text = frame.to_string();
    patterns.iter().any(|pattern| pattern.is_match(&text))
}

/// Approval verdict for one engine approval frame: `None` approves, `Some(reason)` declines.
pub fn approval_verdict(frame: &Value, cwd: Option<&str>) -> Option<String> {
    let full_access = full_access();
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

/// The owner's answer to an approval card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerAnswer {
    Approve,
    Deny,
    /// No answer before the card expired (or the turn ended).
    Unanswered,
}

/// Asks the owner about a tool request policy would decline (omo-gateway's approval card).
#[async_trait::async_trait]
pub trait Approver: Send + Sync {
    async fn ask(&self, frame: &Value, cwd: Option<&str>, reason: &str) -> OwnerAnswer;
}

tokio::task_local! {
    static APPROVER: std::sync::Arc<dyn Approver>;
}

/// Runs `future` (a turn) with `approver` answering requests full access would not allow.
pub async fn with_approver<F: std::future::Future>(
    approver: std::sync::Arc<dyn Approver>,
    future: F,
) -> F::Output {
    APPROVER.scope(approver, future).await
}

/// omo's reply when the owner did not answer in time.
pub const UNANSWERED_REASON: &str =
    "owner has not answered yet; continue with other work and mention the pending approval in your report";

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
    next_frame_by(ws, until, what)
        .await?
        .ok_or_else(|| HubError::message(format!("timed out waiting for {what}")))
}

/// The next JSON frame, or `None` once `until` passes.
async fn next_frame_by(
    ws: &mut Ws,
    until: tokio::time::Instant,
    what: &str,
) -> Result<Option<Value>> {
    loop {
        let message = match tokio::time::timeout_at(until, ws.next()).await {
            Err(_) => return Ok(None),
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
            return Ok(Some(value));
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
        if starts_retired_gateway(frame) {
            tracing::warn!(%method, "declined a tool request that would start the retired omo-gateway");
            *denied += 1;
            send(
                ws,
                protocol::approval_denial_with_reason(id, RETIRED_GATEWAY_REASON),
            )
            .await?;
            return Ok(true);
        }
        let mut verdict = approval_verdict(frame, cwd);
        if let Some(reason) = verdict.clone() {
            if let Ok(approver) = APPROVER.try_with(std::sync::Arc::clone) {
                verdict = match approver.ask(frame, cwd, &reason).await {
                    OwnerAnswer::Approve => None,
                    OwnerAnswer::Deny => Some("denied by the owner".into()),
                    OwnerAnswer::Unanswered => Some(UNANSWERED_REASON.into()),
                };
            }
        }
        match verdict {
            None => {
                *allowed += 1;
                send(ws, protocol::approval_allow_response(id)).await?;
            }
            Some(reason) => {
                if reason != UNANSWERED_REASON {
                    *denied += 1;
                }
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
/// The error a turn ends with when `/omo-stop` interrupted it.
pub const STOPPED: &str = "omo turn stopped by /omo-stop";

/// Injects guidance into a running turn (`turn/steer`, omo-gateway's instant steer) over a
/// short-lived connection. False when the engine refused it (the turn ended or moved on).
pub async fn steer(
    config: &OmoClientConfig,
    thread_id: &str,
    turn_id: &str,
    guidance: &str,
) -> Result<bool> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut ws = connect(config, deadline).await?;
    let (mut allowed, mut denied) = (0u32, 0u32);
    send(&mut ws, protocol::initialize_request()).await?;
    let reply = await_reply(
        &mut ws,
        1,
        deadline,
        "the initialize reply",
        None,
        (&mut allowed, &mut denied),
    )
    .await?;
    if let Some(error) = rpc_error(&reply) {
        return Err(HubError::message(format!("omo initialize failed: {error}")));
    }
    let request = json!({
        "jsonrpc": "2.0", "id": 9_500, "method": "turn/steer",
        "params": {
            "threadId": thread_id,
            "expectedTurnId": turn_id,
            "input": [{"type": "text", "text": guidance}]
        }
    });
    send(&mut ws, request.to_string()).await?;
    let reply = await_reply(
        &mut ws,
        9_500,
        deadline,
        "the turn/steer reply",
        None,
        (&mut allowed, &mut denied),
    )
    .await;
    let _ = ws.close(None).await;
    match reply {
        Ok(reply) => match rpc_error(&reply) {
            None => Ok(true),
            Some(error) => {
                tracing::warn!(%error, "turn/steer rejected by the omo app-server");
                Ok(false)
            }
        },
        Err(error) => {
            tracing::warn!(%error, "turn/steer got no answer");
            Ok(false)
        }
    }
}

/// `run_turn` plus the two recoveries a chat turn needs before it reports to the owner:
/// a model switch the engine refused for context size runs on the thread's current model
/// (2026-10-24 23:15Z the owner got "응답을 만들지 못했어요" instead), and a turn that ran
/// tools without writing a reply is asked once, on the same thread, for its final answer.
pub async fn run_turn_recovering(
    config: &OmoClientConfig,
    request: &TurnRequest,
    progress: Option<UnboundedSender<String>>,
) -> Result<TurnOutcome> {
    let mut first = run_turn(config, request, progress.clone()).await;
    if is_model_switch_rejection(&first) && request.model.is_some() {
        let retry = if request.pin_model {
            tracing::warn!(error = ?first.as_ref().err().map(ToString::to_string), "model switch refused; retrying on a fresh thread with the pinned model");
            TurnRequest {
                thread_id: None,
                ..request.clone()
            }
        } else {
            tracing::warn!(error = ?first.as_ref().err().map(ToString::to_string), "model switch refused; retrying on the thread's current model");
            TurnRequest {
                model: None,
                ..request.clone()
            }
        };
        first = run_turn(config, &retry, progress.clone()).await;
    }
    if !needs_final_answer(&first) {
        return first;
    }
    let Ok(outcome) = &first else {
        return first;
    };
    let ask = TurnRequest {
        thread_id: Some(outcome.thread_id.clone()),
        model: None,
        text: FINAL_ANSWER_PROMPT.to_owned(),
        ..request.clone()
    };
    match run_turn(config, &ask, progress).await {
        Ok(answer) if !answer.text.trim().is_empty() || answer.messages_delivered > 0 => {
            Ok(TurnOutcome {
                tool_calls: outcome.tool_calls + answer.tool_calls,
                ..answer
            })
        }
        Ok(_) => first,
        Err(error) => {
            tracing::warn!(%error, "final-answer follow-up failed");
            first
        }
    }
}

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

    // omo-gateway awaits the custom-provider-aware settings RPC on every turn: a fresh thread
    // otherwise runs on the engine default and a custom-provider model returns nothing.
    if let Some(model) = request
        .model
        .as_deref()
        .filter(|model| !model.trim().is_empty())
    {
        send(
            &mut ws,
            protocol::thread_settings_update_request(&thread_id, model),
        )
        .await?;
        let reply = await_reply(
            &mut ws,
            4,
            handshake_until,
            "the thread/settings/update reply",
            cwd,
            (&mut allowed, &mut denied),
        )
        .await?;
        if let Some(error) = rpc_error(&reply) {
            return Err(HubError::message(format!(
                "omo model update rejected: {error}"
            )));
        }
    }

    // turn/start as omo-gateway submits it (`omo_backend.rs`): the app-server acknowledges only
    // once our prompt is dequeued, so the wait is a 30s window that this thread's activity
    // keeps extending. Until then the thread runs something else; its finished messages are
    // relayed, and an explicit busy refusal resubmits after 1, 2, 4, 8s or the next completion.
    let submit = protocol::turn_start_request(&thread_id, &request.text, request.model.as_deref());
    send(&mut ws, submit.clone()).await?;
    let mut ack_deadline = (tokio::time::Instant::now() + config.ack_window).min(deadline);
    let mut pre_ack = PreAckState::default();
    let (turn_id, mut awaiting_successor) = loop {
        let now = tokio::time::Instant::now();
        if pre_ack.awaiting_idle {
            if now >= deadline {
                return Err(HubError::message(
                    "turn/start refused: the thread is still processing another turn; not resubmitted",
                ));
            }
            if pre_ack.retry_due(now) {
                send(&mut ws, submit.clone()).await?;
                pre_ack.note_resubmitted();
                ack_deadline = (tokio::time::Instant::now() + config.ack_window).min(deadline);
                tracing::info!(
                    attempt = pre_ack.refusals,
                    "resubmitted the prompt that turn/start refused"
                );
                continue;
            }
        } else if now >= ack_deadline {
            return Err(HubError::message(TURN_START_ACK_TIMEOUT));
        }
        let wait = if pre_ack.awaiting_idle {
            pre_ack.retry_at.unwrap_or(deadline).min(deadline)
        } else {
            ack_deadline
        };
        let Some(frame) = next_frame_by(&mut ws, wait, "the turn/start reply").await? else {
            continue;
        };
        if is_response_to(&frame, 3) {
            if let Some(error) = rpc_error(&frame) {
                if pre_ack.note_refusal(&error) {
                    tracing::warn!(
                        attempt = pre_ack.refusals,
                        "turn/start refused: the thread is still processing"
                    );
                    continue;
                }
                return Err(HubError::message(format!("omo turn/start failed: {error}")));
            }
            let acked = frame
                .pointer("/result/turn/id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
                .ok_or_else(|| HubError::message("turn/start acknowledgement missing turn id"))?
                .to_owned();
            break pre_ack.follow(acked);
        }
        if frame.get("id").is_some() && frame.get("method").is_some() {
            answer_server_request(&mut ws, &frame, cwd, &mut allowed, &mut denied).await?;
            continue;
        }
        let method = frame.get("method").and_then(Value::as_str).unwrap_or("");
        if !is_turn_bearing(method, &frame)
            || frame.pointer("/params/threadId").and_then(Value::as_str) != Some(thread_id.as_str())
        {
            continue;
        }
        if pre_ack.refusals == 0 {
            ack_deadline =
                ack_deadline.max((tokio::time::Instant::now() + config.ack_window).min(deadline));
        }
        if pre_ack.observe(&frame, &thread_id)
            || frame_turn_id(&frame).is_some_and(|id| pre_ack.refused.contains(id))
        {
            continue;
        }
        if method == "item/completed"
            && frame.pointer("/params/item/type").and_then(Value::as_str) == Some("agentMessage")
        {
            let text = frame
                .pointer("/params/item/text")
                .and_then(Value::as_str)
                .unwrap_or("");
            let scrubbed = filter_reasoning(text);
            if !scrubbed.trim().is_empty() && !is_explicit_silence(&scrubbed) {
                if let Some(progress) = progress.as_ref() {
                    let _ = progress.send(scrubbed);
                }
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
    let mut turn_id = turn_id;
    let mut last_activity = tokio::time::Instant::now();
    // `/omo-stop` and `/omo-steer` find this turn by its session (chat::turns).
    let registry_key = crate::chat::turns::current_key();
    if let Some(key) = &registry_key {
        crate::chat::turns::started(key, &thread_id, &turn_id);
    }
    let mut cancel = registry_key
        .as_deref()
        .and_then(crate::chat::turns::cancelled);
    loop {
        let frame = {
            {
                let until = (last_activity + config.idle_timeout).min(deadline);
                let next = match cancel.as_mut() {
                    Some(cancel) => tokio::select! {
                        frame = next_frame(&mut ws, until, "the turn to finish") => frame,
                        Ok(_) = cancel.wait_for(|stop| *stop) => Err(HubError::message(STOPPED)),
                    },
                    None => next_frame(&mut ws, until, "the turn to finish").await,
                };
                match next {
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
        // omo-gateway follows only this thread and this turn id. When the run our prompt
        // queued behind completed under our id, the next turn that starts carries our prompt.
        if is_turn_bearing(method, &frame) {
            if frame.pointer("/params/threadId").and_then(Value::as_str) != Some(thread_id.as_str())
            {
                continue;
            }
            if let Some(frame_turn) = frame_turn_id(&frame) {
                if frame_turn != turn_id {
                    if !(awaiting_successor
                        && delivered == 0
                        && content.is_empty()
                        && method == "turn/started")
                    {
                        continue;
                    }
                    tracing::info!(successor = %frame_turn, "following the turn that took over our queued prompt");
                    turn_id = frame_turn.to_owned();
                    awaiting_successor = false;
                }
            }
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
                    #[cfg(feature = "hub-slack")]
                    if let Some(key) = crate::chat::turns::current_key() {
                        crate::chat::turn_status::record_tool_activity(&key, item_type);
                    }
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
            "turn/completed" if !awaiting_successor => {
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
        /// Command the approval request names (default `ls`).
        pub approval_command: Option<String>,
        pub thread_id: String,
        /// Before acknowledging turn/start, run another turn on the thread for this many
        /// ticks (one frame each), relaying one finished message.
        pub queued_ticks: u32,
        pub tick: std::time::Duration,
        /// Never acknowledge turn/start.
        pub no_ack: bool,
        /// Refuse a turn/start that names a model with this error, as the engine does when the
        /// thread's live context does not fit that model's window.
        pub switch_error: Option<String>,
        /// The first turn/start on a connection runs a tool and writes no reply.
        pub silent_first_turn: bool,
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
                            Some("thread/settings/update") => {
                                ws.send(out(json!({"jsonrpc":"2.0","id":id,"result":{}})))
                                    .await
                                    .unwrap();
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
                                if script.no_ack {
                                    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                                    continue;
                                }
                                if let Some(error) = &script.switch_error {
                                    if frame.pointer("/params/model").is_some() {
                                        ws.send(out(json!({"jsonrpc":"2.0","id":id,"error":{"code":-32000,"message":error}}))).await.unwrap();
                                        continue;
                                    }
                                }
                                if script.silent_first_turn
                                    && log.frames.lock().unwrap().iter().filter(|f| f["method"] == "turn/start").count() == 1
                                {
                                    ws.send(out(json!({"jsonrpc":"2.0","id":id,"result":{"turn":{"id":"turn-1","status":"inProgress"}}}))).await.unwrap();
                                    ws.send(out(json!({"jsonrpc":"2.0","method":"item/started","params":{"threadId":thread,"turnId":"turn-1","item":{"id":"tool-silent","type":"commandExecution"}}}))).await.unwrap();
                                    ws.send(out(json!({"jsonrpc":"2.0","method":"turn/completed","params":{"threadId":thread,"turn":{"id":"turn-1","status":"completed"}}}))).await.unwrap();
                                    continue;
                                }
                                if script.queued_ticks > 0 {
                                    ws.send(out(json!({"jsonrpc":"2.0","method":"turn/started","params":{"threadId":thread,"turn":{"id":"turn-0"}}}))).await.unwrap();
                                    for tick in 0..script.queued_ticks {
                                        tokio::time::sleep(script.tick).await;
                                        ws.send(out(json!({"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"threadId":thread,"turnId":"turn-0","delta":format!("bg{tick} ")}}))).await.unwrap();
                                    }
                                    ws.send(out(json!({"jsonrpc":"2.0","method":"item/completed","params":{"threadId":thread,"turnId":"turn-0","item":{"type":"agentMessage","text":"background run finished"}}}))).await.unwrap();
                                    // Another thread's frame on the same socket is ignored.
                                    ws.send(out(json!({"jsonrpc":"2.0","method":"item/completed","params":{"threadId":"thr-other","turnId":"turn-9","item":{"type":"agentMessage","text":"not ours"}}}))).await.unwrap();
                                    ws.send(out(json!({"jsonrpc":"2.0","method":"turn/completed","params":{"threadId":thread,"turn":{"id":"turn-0","status":"completed"}}}))).await.unwrap();
                                }
                                ws.send(out(json!({"jsonrpc":"2.0","id":id,"result":{"turn":{"id":"turn-1","status":"inProgress"}}}))).await.unwrap();
                                if script.approval {
                                    ws.send(out(json!({"jsonrpc":"2.0","id":77,"method":"item/commandExecution/requestApproval","params":{"threadId":thread,"command":script.approval_command.clone().unwrap_or_else(|| "ls".into())}}))).await.unwrap();
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
            approval_command: None,
            thread_id: "thr-new".into(),
            queued_ticks: 0,
            tick: Duration::ZERO,
            no_ack: false,
            switch_error: None,
            silent_first_turn: false,
        }
    }

    /// 2026-10-23 05:06Z: a prompt queued behind another run on its thread failed after a
    /// fixed 120s wait. omo-gateway keeps a 30s window open while the thread is active,
    /// relays the earlier run's finished message, and gives up only after 30s of silence.
    #[tokio::test]
    async fn turn_start_waits_like_omo_while_the_thread_runs_another_turn() {
        let (url, _log) = serve(Script {
            queued_ticks: 6,
            tick: Duration::from_millis(150),
            ..script(&["our answer"])
        })
        .await;
        let mut config = OmoClientConfig::new(url);
        config.ack_window = Duration::from_millis(400);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let outcome = run_turn(
            &config,
            &TurnRequest {
                text: "queued".into(),
                ..Default::default()
            },
            Some(tx),
        )
        .await
        .unwrap();
        assert_eq!(outcome.turn_id, "turn-1");
        assert_eq!(outcome.text, "our answer");
        assert_eq!(rx.recv().await.as_deref(), Some("background run finished"));
        assert!(rx.try_recv().is_err(), "another thread's message leaked");

        let (url, _log) = serve(Script {
            no_ack: true,
            ..script(&["never"])
        })
        .await;
        let mut config = OmoClientConfig::new(url);
        config.ack_window = Duration::from_millis(300);
        let started = std::time::Instant::now();
        let error = run_turn(&config, &TurnRequest::default(), None)
            .await
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("submission outcome unknown, do not resubmit"),
            "{error}"
        );
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_prompt_whose_turn_id_the_run_ahead_completed_follows_the_successor() {
        let mut pre = PreAckState::default();
        let frame = |method: &str, id: &str| json!({"method": method, "params": {"threadId": "t", "turn": {"id": id}}});
        pre.observe(&frame("turn/started", "a"), "t");
        pre.observe(&frame("turn/completed", "a"), "t");
        assert_eq!(pre.clone_follow("a"), ("a".to_owned(), true));
        pre.observe(&frame("turn/started", "b"), "t");
        assert_eq!(pre.clone_follow("a"), ("b".to_owned(), false));
        assert_eq!(pre.clone_follow("z"), ("z".to_owned(), false));
        assert!(pre.note_refusal("thread is already processing a turn"));
        assert!(pre.awaiting_idle);
        assert!(pre.observe(&frame("turn/completed", "b"), "t"));
    }

    #[tokio::test]
    async fn a_turn_waits_for_each_reply_and_returns_the_last_message() {
        let (url, log) = serve(Script {
            approval: true,
            approval_command: None,
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

    #[test]
    fn only_requests_that_would_start_the_retired_gateway_are_recognized() {
        let frame = |command: &str| json!({"method":"item/commandExecution/requestApproval","params":{"command":command}});
        for blocked in [
            "launchctl enable gui/501/ai.omon.gateway",
            "launchctl bootstrap gui/$(id -u) /Users/x/Library/LaunchAgents/ai.omon.gateway.plist 2>&1 | head -5",
            "/bin/launchctl kickstart -k gui/501/ai.omon.gateway",
            "launchctl load -w ~/Library/LaunchAgents/ai.omon.gateway-watchdog.plist",
            "nohup /Users/x/.omon/bin/omo-gateway run &",
            "/Users/x/.omon/bin/omo-gateway-launch",
        ] {
            assert!(starts_retired_gateway(&frame(blocked)), "{blocked}");
        }
        // A tool call whose arguments carry the command in another shape.
        assert!(starts_retired_gateway(
            &json!({"method":"item/tool/requestApproval","params":{"arguments":{"code":"subprocess.run([\"/Users/x/.omon/bin/omo-gateway\",\"run\"])"}}})
        ));
        for allowed in [
            "launchctl print gui/501/ai.omon.gateway",
            "launchctl list | grep ai.omon",
            "launchctl bootout gui/501/ai.omon.gateway",
            "launchctl disable gui/501/ai.omon.gateway",
            "tail -n 50 ~/.omon/gateway.log",
            "/Users/x/.omon/bin/omo-gateway autonomy outcome k --status failed",
            "launchctl kickstart -k gui/501/ai.omon.tracker-proxy",
        ] {
            assert!(!starts_retired_gateway(&frame(allowed)), "{allowed}");
        }
    }

    #[tokio::test]
    async fn a_request_to_start_the_retired_gateway_is_declined_even_with_full_access() {
        let (url, log) = serve(Script {
            approval: true,
            approval_command: Some("launchctl enable gui/501/ai.omon.gateway".into()),
            ..script(&["done"])
        })
        .await;
        let outcome = run_turn(
            &OmoClientConfig::new(url),
            &TurnRequest {
                text: "hello".into(),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
        assert_eq!(outcome.approvals_allowed, 0);
        assert_eq!(outcome.approvals_denied, 1);
        let frames = log.frames.lock().unwrap().clone();
        let answer = frames
            .iter()
            .find(|f| f.get("id") == Some(&json!(77)))
            .unwrap();
        assert_ne!(answer["result"]["decision"], "accept");
        assert!(answer.to_string().contains("intentionally retired"));
    }

    /// 2026-10-24 23:15Z: turn/start refused the requested model for context size and the
    /// owner got a failure line. The turn reruns on the thread's model and answers.
    #[tokio::test]
    async fn a_model_switch_refused_for_context_size_reruns_on_the_current_model() {
        let (url, log) = serve(Script {
            switch_error: Some("Model \"mahoquot/claude-opus-5-5\" cannot switch: target context window 400000 tokens is 1200 tokens short of the 401200-token requirement".into()),
            ..script(&["answer on the current model"])
        })
        .await;
        let outcome = run_turn_recovering(
            &OmoClientConfig::new(url),
            &TurnRequest {
                text: "hello".into(),
                model: Some("mahoquot/claude-opus-5-5".into()),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
        assert_eq!(outcome.text, "answer on the current model");
        let frames = log.frames.lock().unwrap().clone();
        let starts: Vec<&Value> = frames
            .iter()
            .filter(|frame| frame["method"] == "turn/start")
            .collect();
        assert_eq!(starts.len(), 2);
        assert!(starts[0].pointer("/params/model").is_some());
        assert!(starts[1].pointer("/params/model").is_none());
    }

    /// A background turn pinned to its model never falls back to the thread's model: a refused
    /// switch starts a fresh thread on the pinned model.
    #[tokio::test]
    async fn a_pinned_model_switch_refusal_starts_a_fresh_thread_on_that_model() {
        let (url, log) = serve(Script {
            switch_error: Some("Model \"mahoquot/gpt-6-luna\" cannot switch: target context window 272000 tokens is 84000 tokens short of the 356000-token requirement".into()),
            ..script(&["unused"])
        })
        .await;
        let _ = run_turn_recovering(
            &OmoClientConfig::new(url),
            &TurnRequest {
                thread_id: Some("thr-old".into()),
                text: "event".into(),
                model: Some("mahoquot/gpt-6-luna".into()),
                pin_model: true,
                ..Default::default()
            },
            None,
        )
        .await;
        let frames = log.frames.lock().unwrap().clone();
        let methods: Vec<&str> = frames
            .iter()
            .filter_map(|frame| frame["method"].as_str())
            .collect();
        assert_eq!(
            methods.iter().filter(|method| **method == "thread/resume").count(),
            1
        );
        assert_eq!(
            methods.iter().filter(|method| **method == "thread/start").count(),
            1
        );
        let starts: Vec<&Value> = frames
            .iter()
            .filter(|frame| frame["method"] == "turn/start")
            .collect();
        assert_eq!(starts.len(), 2);
        for start in starts {
            assert_eq!(start["params"]["model"], "mahoquot/gpt-6-luna");
        }
    }

    /// A turn that ran tools and wrote no reply is asked for its answer on the same thread
    /// instead of posting a bare "Done".
    #[tokio::test]
    async fn a_tool_only_turn_is_asked_for_its_final_answer() {
        let (url, log) = serve(Script {
            silent_first_turn: true,
            ..script(&["here is what I found"])
        })
        .await;
        let outcome = run_turn_recovering(
            &OmoClientConfig::new(url),
            &TurnRequest {
                text: "check it".into(),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
        assert_eq!(outcome.text, "here is what I found");
        assert_eq!(outcome.tool_calls, 1);
        let frames = log.frames.lock().unwrap().clone();
        let starts: Vec<&Value> = frames
            .iter()
            .filter(|frame| frame["method"] == "turn/start")
            .collect();
        assert_eq!(starts.len(), 2);
        assert_eq!(starts[1].pointer("/params/input/0/text").and_then(Value::as_str), Some(FINAL_ANSWER_PROMPT));
        let resumes = frames.iter().filter(|frame| frame["method"] == "thread/resume").count();
        assert_eq!(resumes, 1, "the follow-up resumes the same thread");
    }

    #[tokio::test]
    async fn a_fresh_thread_gets_the_model_through_settings_update_like_omo() {
        let (url, log) = serve(script(&["hi"])).await;
        let outcome = run_turn(
            &OmoClientConfig::new(url),
            &TurnRequest {
                text: "hello".into(),
                model: Some("mahoquot/claude-opus-5-5".into()),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
        assert_eq!(outcome.text, "hi");
        let frames = log.frames.lock().unwrap().clone();
        let methods: Vec<_> = frames
            .iter()
            .filter_map(|f| f.get("method").and_then(Value::as_str))
            .collect();
        assert_eq!(
            methods,
            [
                "initialize",
                "thread/start",
                "thread/settings/update",
                "turn/start"
            ]
        );
        let update = &frames[2]["params"];
        assert_eq!(update["threadId"], "thr-new");
        assert_eq!(update["model"], "mahoquot/claude-opus-5-5");
        assert!(update["effort"]
            .as_str()
            .is_some_and(|effort| !effort.is_empty()));
    }

    #[test]
    fn an_empty_turn_falls_back_and_the_owner_sees_omos_korean_line() {
        let empty = TurnOutcome {
            thread_id: "t".into(),
            resumed: false,
            turn_id: "u".into(),
            text: String::new(),
            approvals_allowed: 0,
            approvals_denied: 0,
            messages_delivered: 0,
            tool_calls: 0,
        };
        assert!(fallback_eligible(&Ok(empty.clone())));
        let worked = TurnOutcome {
            tool_calls: 1,
            ..empty
        };
        assert!(!fallback_eligible(&Ok(worked)));
        assert!(fallback_eligible(&Err(HubError::message(
            "omo model update rejected: unknown model"
        ))));
        assert!(!fallback_eligible(&Err(HubError::message(
            "omo turn failed: boom"
        ))));
        assert_eq!(
            owner_failure_text("omo turn ended with no content (likely upstream LLM failure)"),
            "⚠️ 응답을 만들지 못했어요 (처리 중 오류가 났어요). 같은 메시지를 다시 보내 주시면 이어서 할게요."
        );
        assert!(owner_failure_text("HTTP 429").contains("모델 사용량 한도에 걸렸어요"));
        assert!(
            owner_failure_text("timed out waiting for the turn to finish")
                .contains("턴 시간 제한을 넘겨 중단됐어요")
        );
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
            Some("⚠️ 작업은 실행됐지만(도구 2회) 최종 답변을 만들지 못했어요. \"결과 알려줘\"라고 보내 주시면 정리해 드릴게요.")
        );
        assert!(needs_final_answer(&Ok(outcome("", 0, 2))));
        assert!(!needs_final_answer(&Ok(outcome("", 1, 2))));
        assert!(!needs_final_answer(&Ok(outcome("hi", 0, 2))));
        assert!(!needs_final_answer(&Ok(outcome("", 0, 0))));
        assert!(is_model_switch_rejection(&Err(HubError::message(
            "turn/start failed: Model \"x\" cannot switch: target context window 400000 tokens is 12 tokens short"
        ))));
        assert!(!is_model_switch_rejection(&Err(HubError::message("model not found"))));
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
            pin_model: false,
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
