//! One owned mhc stdio JSONL RPC process per hub turn.
use super::protocol;
use crate::effect::Effect;
use crate::error::{HubError, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::{mpsc, oneshot};

pub const APPROVAL_DENIAL_TURN_LIMIT: u32 = 5;
const DENIAL_REASON: &str =
    "denied by gateway policy - do not retry this tool; ask the user for the information instead";
pub const TURN_START_ACK_WINDOW: Duration = Duration::from_secs(30);
/// How long a successful turn's engine gets to exit on its own after stdin closes. A SIGKILL
/// right after a good turn makes mhc record the session as interrupted on the next resume.
const GRACEFUL_EXIT_WINDOW: Duration = Duration::from_secs(5);
/// How long after a turn's final `agent_end` mhc gets to record whether the session settled or
/// is waiting on background work (it records that about a second after the reply).
const LIFECYCLE_SETTLE_WINDOW: Duration = Duration::from_secs(10);
const LIFECYCLE_POLL: Duration = Duration::from_millis(200);
/// How often a held engine re-reads the session lifecycle while it waits for a wake.
const BACKGROUND_RECHECK: Duration = Duration::from_secs(60);
/// The tail of the session file searched for the latest lifecycle record.
const LIFECYCLE_TAIL_BYTES: u64 = 256 * 1024;
pub const BACKGROUND_MAX_DEFAULT: Duration = Duration::from_secs(6 * 3600);
/// mhc's tool permission dialog arrives as a `select` with this title prefix.
const PERMISSION_TITLE_PREFIX: &str = "Permission required:";
const PERMISSION_ALLOW: &str = "Allow once";
const PERMISSION_DENY: &str = "Deny";
/// Real mhc tool ids `--tools` may name. Anything else (notably the legacy omo-gateway
/// toolsets `terminal`, `browser`, `file`) would leave the model with zero tools.
const MHC_TOOL_IDS: &[&str] = &[
    "read",
    "write",
    "edit",
    "bash",
    "eval",
    "todo",
    "web_search",
    "webfetch",
];
pub const TURN_START_ACK_TIMEOUT: &str =
    "timeout waiting for mhc prompt acknowledgement; submission outcome unknown, do not resubmit";
#[derive(Debug, Clone)]
pub struct OmoClientConfig {
    pub binary: String,
    pub idle_timeout: Duration,
    pub total_timeout: Duration,
    pub connect_timeout: Duration,
    pub workspace: Option<protocol::Workspace>,
    pub ack_window: Duration,
    /// How long an engine stays up after its turn for background work (senpi-task children)
    /// it started; the work is stopped and the owner told when it runs longer.
    pub background_max: Duration,
}
impl Default for OmoClientConfig {
    fn default() -> Self {
        Self::new(super::supervisor::mhc_bin())
    }
}
impl OmoClientConfig {
    pub fn new(binary: impl Into<String>) -> Self {
        Self {
            binary: binary.into(),
            idle_timeout: Duration::from_secs(1800),
            total_timeout: Duration::from_secs(3600),
            connect_timeout: Duration::from_secs(15),
            workspace: None,
            ack_window: TURN_START_ACK_WINDOW,
            background_max: BACKGROUND_MAX_DEFAULT,
        }
    }
    pub fn with_env_timeouts(mut self) -> Self {
        let secs = |name: &str| {
            crate::hub_env::var(name)
                .ok()
                .and_then(|v| v.trim().parse::<u64>().ok())
                .filter(|v| *v > 0)
                .map(Duration::from_secs)
        };
        if let Some(v) = secs("OMON_OMO_TURN_TIMEOUT_SECS") {
            self.idle_timeout = v;
        }
        if let Some(v) = secs("OMON_OMO_TURN_TOTAL_TIMEOUT_SECS") {
            self.total_timeout = v;
        }
        if let Some(v) = secs("OMON_OMO_BACKGROUND_MAX_SECS") {
            self.background_max = v;
        }
        self
    }
}
#[derive(Debug, Clone, Default)]
pub struct TurnRequest {
    /// Opaque mhc sessionFile path, stored in legacy omo_thread_id metadata.
    pub thread_id: Option<String>,
    pub system_prompt: Option<String>,
    pub model: Option<String>,
    pub toolsets: Option<Vec<String>>,
    pub text: String,
    pub pin_model: bool,
    /// Where replies of runs the engine starts after this turn (background completion wakes)
    /// go. With a sink the engine stays up while the session waits on background work.
    pub followups: Option<UnboundedSender<String>>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnOutcome {
    pub thread_id: String,
    pub resumed: bool,
    pub turn_id: String,
    pub text: String,
    pub approvals_allowed: u32,
    pub approvals_denied: u32,
    pub messages_delivered: usize,
    pub tool_calls: usize,
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

/// Compatibility constant for consumers; this transport never submits a hidden follow-up.
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
/// Kept for consumer compatibility; the transport never retries the model.
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
        Err(error) => error.to_string().contains("mhc fallback eligible:"),
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

pub const STOPPED: &str = "omo turn stopped by /omo-stop";
struct Guidance {
    text: String,
    reply: oneshot::Sender<bool>,
}
type ActiveKey = (String, String);
fn active() -> &'static Mutex<HashMap<ActiveKey, UnboundedSender<Guidance>>> {
    static ACTIVE: OnceLock<Mutex<HashMap<ActiveKey, UnboundedSender<Guidance>>>> = OnceLock::new();
    ACTIVE.get_or_init(|| Mutex::new(HashMap::new()))
}
struct ActiveGuard(ActiveKey);
impl Drop for ActiveGuard {
    fn drop(&mut self) {
        active()
            .lock()
            .expect("mhc active registry")
            .remove(&self.0);
    }
}
pub async fn steer(_: &OmoClientConfig, thread: &str, turn: &str, text: &str) -> Result<bool> {
    let sender = active()
        .lock()
        .expect("mhc active registry")
        .get(&(thread.into(), turn.into()))
        .cloned();
    let Some(sender) = sender else {
        return Ok(false);
    };
    let (reply, rx) = oneshot::channel();
    if sender
        .send(Guidance {
            text: text.into(),
            reply,
        })
        .is_err()
    {
        return Ok(false);
    }
    Ok(tokio::time::timeout(Duration::from_secs(5), rx)
        .await
        .ok()
        .and_then(|r| r.ok())
        .unwrap_or(false))
}
/// Recovery belongs exclusively to the consumer's single fallback-model attempt.
pub async fn run_turn_recovering(
    c: &OmoClientConfig,
    r: &TurnRequest,
    p: Option<UnboundedSender<String>>,
) -> Result<TurnOutcome> {
    run_turn(c, r, p).await
}

struct Rpc {
    input: ChildStdin,
    lines: tokio::io::Lines<BufReader<ChildStdout>>,
    allowed: u32,
    denied: u32,
    eof: bool,
}
impl Rpc {
    async fn send(&mut self, value: Value) -> Result<()> {
        let mut bytes = serde_json::to_vec(&value).map_err(|e| HubError::message(e.to_string()))?;
        bytes.push(b'\n');
        self.input.write_all(&bytes).await?;
        self.input.flush().await?;
        Ok(())
    }
    async fn read(&mut self) -> Result<Value> {
        loop {
            let Some(line) = self.lines.next_line().await? else {
                self.eof = true;
                return Err(HubError::message(
                    "mhc RPC stdout closed before completing the turn",
                ));
            };
            if !line.trim().is_empty() {
                return serde_json::from_str(&line)
                    .map_err(|e| HubError::message(format!("invalid mhc RPC JSONL: {e}")));
            }
        }
    }
    /// The one approval decision for confirm and permission-select dialogs: the retired
    /// gateway is always declined, otherwise policy decides and the owner is asked about
    /// anything policy would decline. Updates the turn's approval counters.
    async fn decide(&mut self, policy_frame: &Value, cwd: Option<&str>) -> bool {
        let retired = starts_retired_gateway(policy_frame);
        let mut verdict = if retired {
            Some(RETIRED_GATEWAY_REASON.into())
        } else {
            approval_verdict(policy_frame, cwd)
        };
        if !retired {
            if let Some(reason) = verdict.clone() {
                if let Ok(approver) = APPROVER.try_with(std::sync::Arc::clone) {
                    verdict = match approver.ask(policy_frame, cwd, &reason).await {
                        OwnerAnswer::Approve => None,
                        OwnerAnswer::Deny => Some("denied by the owner".into()),
                        OwnerAnswer::Unanswered => Some(UNANSWERED_REASON.into()),
                    };
                }
            }
        }
        if verdict.is_none() {
            self.allowed += 1;
        } else if verdict.as_deref() != Some(UNANSWERED_REASON) {
            self.denied += 1;
        }
        verdict.is_none()
    }
    fn denial_limit(&self) -> Result<()> {
        if self.denied >= APPROVAL_DENIAL_TURN_LIMIT {
            return Err(HubError::message("mhc approval denial limit reached"));
        }
        Ok(())
    }
    async fn ui(&mut self, frame: &Value, cwd: Option<&str>) -> Result<()> {
        if frame["type"] != "extension_ui_request" {
            return Ok(());
        }
        let permission = frame["method"] == "select"
            && frame["title"]
                .as_str()
                .is_some_and(|title| title.starts_with(PERMISSION_TITLE_PREFIX));
        if permission {
            let id = frame
                .get("id")
                .ok_or_else(|| HubError::message("mhc permission select missing id"))?;
            // The title carries the tool, its detail line and patterns; keep the raw frame
            // for the effect classifier and the owner's approval card.
            let policy_frame = json!({"method":"item/tool/requestApproval","id":id,"params":{"command":frame["title"].as_str().unwrap_or(""),"request":frame}});
            let approved = self.decide(&policy_frame, cwd).await;
            let value = if approved {
                PERMISSION_ALLOW
            } else {
                PERMISSION_DENY
            };
            self.send(json!({"type":"extension_ui_response","id":id,"value":value}))
                .await?;
            return self.denial_limit();
        }
        if matches!(frame["method"].as_str(), Some("select" | "input")) {
            // Approval cards provide a binary decision, not arbitrary text. Never fabricate
            // a selection or input from that decision; close the native dialog explicitly.
            return self
                .send(json!({"type":"extension_ui_response","id":frame["id"],"cancelled":true}))
                .await;
        }
        if let Some(response) = protocol::user_input_cancellation(frame) {
            let response: Value =
                serde_json::from_str(&response).map_err(|e| HubError::message(e.to_string()))?;
            return self.send(response).await;
        }
        if frame["method"] != "confirm" {
            return Ok(());
        }
        let id = frame
            .get("id")
            .ok_or_else(|| HubError::message("mhc confirm missing id"))?;
        // Keep the raw title/message in the effect classifier and owner's approval card.
        let policy_frame = json!({"method":"item/tool/requestApproval","id":id,"params":{"command":frame["message"].as_str().unwrap_or(""),"request":frame}});
        let approved = self.decide(&policy_frame, cwd).await;
        self.send(json!({"type":"extension_ui_response","id":id,"confirmed":approved}))
            .await?;
        self.denial_limit()
    }
    async fn request(&mut self, value: Value, cwd: Option<&str>) -> Result<Value> {
        let id = value["id"].clone();
        self.send(value).await?;
        loop {
            let f = self.read().await?;
            if f["type"] == "response" && f["id"] == id {
                if f["success"] != true {
                    return Err(HubError::message(format!(
                        "{}: {}",
                        f["command"], f["error"]
                    )));
                }
                return Ok(f["data"].clone());
            }
            self.ui(&f, cwd).await?;
        }
    }
}

/// Stale paths and legacy IDs start a fresh session.
fn resume_path(id: Option<&str>) -> Option<String> {
    let path = std::path::Path::new(id?);
    path.is_file().then(|| path.to_string_lossy().into_owned())
}
fn message_text(message: &Value) -> String {
    if message["role"] != "assistant" {
        return String::new();
    }
    let raw = match message.get("content") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter(|p| p["type"] == "text")
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    };
    filter_reasoning(&raw)
}
fn message_error(message: &Value) -> Option<String> {
    if message["role"] == "assistant"
        && matches!(message["stopReason"].as_str(), Some("error" | "aborted"))
    {
        Some(
            message["errorMessage"]
                .as_str()
                .unwrap_or("mhc turn interrupted")
                .to_owned(),
        )
    } else {
        None
    }
}
fn failure(error: String, side_effects: bool) -> HubError {
    let lower = error.to_ascii_lowercase();
    let transient = [
        "503",
        "overload",
        "429",
        "quota",
        "no available accounts",
        "rate limit",
        "model not found",
        "unknown model",
        "unsupported model",
        "invalid model",
    ]
    .iter()
    .any(|s| lower.contains(s));
    HubError::message(if !side_effects && transient {
        format!("mhc fallback eligible: {error}")
    } else {
        format!("mhc turn failed: {error}")
    })
}

/// The `--tools` value for the requested names, or `None` (mhc's default tool set) unless
/// every name is a real mhc tool id. Never an empty list.
fn mhc_tools(requested: &[String]) -> Option<String> {
    if requested.is_empty() {
        return None;
    }
    let dropped: Vec<&str> = requested
        .iter()
        .map(String::as_str)
        .filter(|name| !MHC_TOOL_IDS.contains(name))
        .collect();
    if !dropped.is_empty() {
        tracing::warn!(
            dropped = %dropped.join(","),
            "requested toolsets are not mhc tool ids; launching mhc with its default tools"
        );
        return None;
    }
    Some(requested.join(","))
}

struct GuardDirectory(PathBuf);
impl Drop for GuardDirectory {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            tracing::warn!(%error,"mhc temporary launchctl guard cleanup failed");
        }
    }
}

/// One owned mhc RPC process. The launchctl guard directory sits on its PATH, so it lives
/// exactly as long as the process; `group` kills the private process group if the
/// engine is dropped without a reap (a hub shutdown while it is held).
struct Engine {
    child: Child,
    rpc: Rpc,
    group: GroupKill,
    _launch: GuardDirectory,
}
struct GroupKill(Option<u32>);
impl GroupKill {
    fn kill(&mut self) {
        #[cfg(unix)]
        if let Some(pid) = self.0.take() {
            // Tool subprocesses inherit the private group; stop them with their owned engine.
            let result = unsafe { libc::kill(-(pid as i32), libc::SIGKILL) };
            if result != 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH) {
                tracing::warn!(error=%std::io::Error::last_os_error(), "mhc RPC process group kill failed");
            }
        }
        #[cfg(not(unix))]
        let _ = self.0.take();
    }
}
impl Drop for GroupKill {
    fn drop(&mut self) {
        self.kill();
    }
}

async fn spawn_engine(config: &OmoClientConfig, request: &TurnRequest) -> Result<(Engine, Option<String>)> {
    let stored_id = request.thread_id.clone();
    let binary = PathBuf::from(&config.binary);
    let (child_path, guard, session_path) = tokio::task::spawn_blocking(move || -> Result<_> {
        let path = crate::child_env::child_path().to_owned();
        // A private guard must never write into the running user's ~/.ferryx/hub.
        let dir = std::env::temp_dir().join(format!("ferryx-mhc-guard-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir)?;
        let guard = GuardDirectory(dir);
        let bin = super::supervisor::install_launchctl_guard(&guard.0)?;
        let path = super::supervisor::mhc_child_path(&binary, vec![bin], &path)
            .map_err(|e| HubError::message(e.to_string()))?;
        Ok((path, guard, resume_path(stored_id.as_deref())))
    })
    .await
    .map_err(|e| HubError::message(format!("mhc launch preparation failed: {e}")))??;
    let mut cmd = crate::child_env::command(&config.binary);
    cmd.env("PATH", child_path);
    cmd.args(["--mode", "rpc", "--no-model-fallback"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.as_std_mut().process_group(0);
    }
    if let Some(dir) = super::supervisor::agent_dir() {
        cmd.env("MAHO_CODING_AGENT_DIR", dir);
    }

    cmd.env("NO_FALLBACK", "1");
    if let Some(path) = session_path.as_deref() {
        cmd.arg("--session").arg(path);
    }
    // Launch flags select model and thinking for this process only; the matching RPC
    // commands persist into the global agent settings and hot-reload mhc mid-turn.
    if let Some(model) = request.model.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        cmd.arg("--model").arg(model);
    }
    cmd.arg("--thinking").arg(protocol::default_effort());
    let mut instructions = request.system_prompt.clone().unwrap_or_default();
    if let Some(w) = &config.workspace {
        cmd.current_dir(&w.cwd);
        instructions.push_str("\nAdditional workspace roots:\n");
        instructions.push_str(&w.roots.join("\n"));
    }
    if !instructions.is_empty() {
        cmd.arg("--append-system-prompt").arg(instructions);
    }
    if let Some(tools) = request.toolsets.as_deref().and_then(mhc_tools) {
        cmd.arg("--tools").arg(tools);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| HubError::message(format!("spawn mhc RPC: {e}")))?;
    let rpc = Rpc {
        input: child.stdin.take().expect("piped mhc stdin"),
        lines: BufReader::new(child.stdout.take().expect("piped mhc stdout")).lines(),
        allowed: 0,
        denied: 0,
        eof: false,
    };
    let group = GroupKill(child.id());
    Ok((
        Engine {
            child,
            rpc,
            group,
            _launch: guard,
        },
        session_path,
    ))
}

pub async fn run_turn(
    config: &OmoClientConfig,
    request: &TurnRequest,
    progress: Option<UnboundedSender<String>>,
) -> Result<TurnOutcome> {
    // A session whose last turn left background work running still has its engine: the turn
    // runs there, because a second process on the same session would restore the background
    // children without their running state.
    let (mut engine, session_path) = match take_held(request.thread_id.as_deref()).await {
        Some(engine) => {
            tracing::info!(session = ?request.thread_id, "turn runs on the session's held mhc engine");
            (engine, request.thread_id.clone())
        }
        None => spawn_engine(config, request).await?,
    };
    engine.rpc.allowed = 0;
    engine.rpc.denied = 0;
    let key = crate::chat::turns::current_key();
    let mut cancel = key.as_deref().and_then(crate::chat::turns::cancelled);
    let work = run_owned(
        &mut engine.rpc,
        config,
        request,
        progress,
        key.as_deref(),
        session_path,
    );
    let mut result = tokio::select! {
        result = tokio::time::timeout(config.total_timeout,work) => result.unwrap_or_else(|_|Err(HubError::message("timed out waiting for the turn to finish"))),
        _ = async {
            match cancel.as_mut() {
                Some(rx) => { if !*rx.borrow() { let _ = rx.changed().await; } }
                None => std::future::pending::<()>().await,
            }
        } => Err(HubError::message(STOPPED)),
    };
    if engine.rpc.eof {
        // Only inspect exit after stdout is drained: racing wait() against reads can hide
        // a successful final boundary already buffered in the pipe.
        let status = match tokio::time::timeout(Duration::from_secs(1), engine.child.wait()).await {
            Ok(Ok(status)) => status.to_string(),
            Ok(Err(error)) => format!("status unavailable: {error}"),
            Err(_) => "process still running after stdout EOF".to_owned(),
        };
        result = Err(HubError::message(format!(
            "mhc RPC process exited before completing the turn ({status})"
        )));
    }
    // Background work the turn started (senpi-task children) lives inside this process:
    // closing it now would kill that work. A holder keeps the engine until the session settles,
    // so a completion wake runs as a follow-up turn whose reply reaches the owner. It registers
    // before the reply is posted, so the session's next turn always finds it.
    if let (Ok(outcome), Some(followups)) = (&result, request.followups.as_ref()) {
        let holder = hold(
            Box::new(engine),
            config.clone(),
            register_held(&outcome.thread_id),
            followups.clone(),
        );
        let _holder = match APPROVER.try_with(std::sync::Arc::clone) {
            Ok(approver) => tokio::spawn(APPROVER.scope(approver, holder)),
            Err(_) => tokio::spawn(holder),
        };
        return result;
    }
    close(engine, result.is_ok()).await;
    result
}

/// Ends an engine. No path may orphan it: closing stdin first lets the host observe EOF;
/// kill + wait bounds cleanup even when an extension or provider refuses cancellation.
async fn close(engine: Engine, succeeded: bool) {
    let Engine {
        mut child,
        mut rpc,
        mut group,
        _launch,
    } = engine;
    if !succeeded {
        let abort = protocol::command("abort", "abort");
        match tokio::time::timeout(Duration::from_secs(1), rpc.send(abort)).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => tracing::warn!(%error,"mhc abort write failed"),
            Err(error) => tracing::warn!(%error,"mhc abort write timed out"),
        }
    }
    drop(rpc);
    if succeeded {
        // Closed stdin asks mhc to finish and record the session cleanly. reap still runs
        // afterwards: it kills tool subprocesses left in the private group and is the
        // fallback for an engine that does not exit in time.
        match tokio::time::timeout(GRACEFUL_EXIT_WINDOW, child.wait()).await {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => tracing::warn!(%error, "mhc RPC graceful exit wait failed"),
            Err(_) => tracing::warn!("mhc RPC did not exit after stdin close; killing it"),
        }
    }
    reap(&mut child, &mut group).await;
}
async fn reap(child: &mut Child, group: &mut GroupKill) {
    group.kill();
    match child.try_wait() {
        Ok(Some(_)) => {}
        Ok(None) => {
            if let Err(e) = child.kill().await {
                tracing::warn!(error=%e,"mhc RPC child kill failed");
            }
        }
        Err(e) => {
            tracing::warn!(error=%e,"mhc RPC child status failed");
            if let Err(error) = child.start_kill() {
                tracing::warn!(%error,"mhc RPC child fallback kill failed");
            }
        }
    }
    if let Err(e) = child.wait().await {
        tracing::warn!(error=%e,"mhc RPC child reap failed");
    }
}

/// The last `session-lifecycle` record mhc wrote to the session file.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Lifecycle {
    state: String,
    run_active: bool,
    /// Background sources and children the session still waits on; a session waiting on the
    /// owner (`waiting_human`) can have them too.
    pending_work: bool,
}
impl Lifecycle {
    fn waiting(&self) -> bool {
        self.pending_work
            || matches!(self.state.as_str(), "waiting_background" | "waiting_children")
    }
}
async fn latest_lifecycle(path: &str) -> Option<Lifecycle> {
    use std::io::SeekFrom;
    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    let mut file = tokio::fs::File::open(path).await.ok()?;
    let len = file.metadata().await.ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(LIFECYCLE_TAIL_BYTES)))
        .await
        .ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).await.ok()?;
    String::from_utf8_lossy(&bytes)
        .lines()
        .rev()
        .filter(|line| line.contains("\"session-lifecycle\""))
        .find_map(|line| {
            let entry: Value = serde_json::from_str(line).ok()?;
            if entry["customType"] != "session-lifecycle" {
                return None;
            }
            let data = &entry["data"];
            let waits = &data["waitingOn"];
            let background = waits["background"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|source| source["activeCount"].as_u64().unwrap_or(1) > 0);
            Some(Lifecycle {
                state: data["state"].as_str()?.to_owned(),
                run_active: data["runActive"].as_bool().unwrap_or(false),
                pending_work: background || waits["children"].as_u64().unwrap_or(0) > 0,
            })
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Settle {
    /// Nothing the session waits on: the engine can close.
    Done,
    /// The session waits on background work or children it started.
    Waiting,
    /// A follow-up run (a completion wake) already started.
    Started,
}
/// What the session waits on once its run ended. mhc records it shortly after the final
/// `agent_end`; stdout is drained meanwhile so the engine never blocks on a full pipe. A
/// session file without lifecycle records (an older mhc) settles at once.
async fn settle(rpc: &mut Rpc, thread: &str, cwd: Option<&str>) -> Result<Settle> {
    let until = tokio::time::Instant::now() + LIFECYCLE_SETTLE_WINDOW;
    loop {
        match latest_lifecycle(thread).await {
            None => return Ok(Settle::Done),
            Some(lifecycle) if !lifecycle.run_active => {
                return Ok(if lifecycle.waiting() {
                    Settle::Waiting
                } else {
                    Settle::Done
                })
            }
            Some(_) if tokio::time::Instant::now() >= until => return Ok(Settle::Done),
            Some(_) => {}
        }
        tokio::select! {
            frame = rpc.read() => {
                let frame = frame?;
                rpc.ui(&frame, cwd).await?;
                if frame["type"] == "agent_start" {
                    return Ok(Settle::Started);
                }
            }
            _ = tokio::time::sleep(LIFECYCLE_POLL) => {}
        }
    }
}

/// A turn that wants a held engine: the holder answers between follow-up runs.
type Handover = oneshot::Sender<Box<Engine>>;
type HeldEntry = (String, UnboundedSender<Handover>);
fn held() -> &'static Mutex<HashMap<String, HeldEntry>> {
    static HELD: OnceLock<Mutex<HashMap<String, HeldEntry>>> = OnceLock::new();
    HELD.get_or_init(|| Mutex::new(HashMap::new()))
}
fn unregister_held(thread: &str, id: &str) {
    let mut held = held().lock().expect("mhc held registry");
    if held.get(thread).is_some_and(|(owner, _)| owner == id) {
        held.remove(thread);
    }
}
struct HeldGuard {
    thread: String,
    id: String,
}
impl Drop for HeldGuard {
    fn drop(&mut self) {
        unregister_held(&self.thread, &self.id);
    }
}
async fn take_held(thread: Option<&str>) -> Option<Engine> {
    let sender = held()
        .lock()
        .expect("mhc held registry")
        .get(thread?)
        .map(|(_, sender)| sender.clone())?;
    let (reply, engine) = oneshot::channel();
    sender.send(reply).ok()?;
    // A holder that ended first drops the request; the turn then starts a fresh engine.
    engine.await.ok().map(|engine| *engine)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HoldEnd {
    Settled,
    Expired,
    Exited,
    Failed,
}
pub const BACKGROUND_EXPIRED: &str = "⚠️ 백그라운드 작업이 최대 대기 시간을 넘겨 중단됐어요. 필요하면 다시 시켜 주세요.";
pub const BACKGROUND_LOST: &str = "⚠️ 백그라운드 작업을 실행하던 엔진이 끝나 버려 결과를 받지 못했어요. 필요하면 다시 시켜 주세요.";

/// A registered holder: the session's next turn asks it for the engine.
struct HeldSlot {
    guard: HeldGuard,
    sender: UnboundedSender<Handover>,
    handovers: mpsc::UnboundedReceiver<Handover>,
}
fn register_held(thread: &str) -> HeldSlot {
    let id = uuid::Uuid::new_v4().to_string();
    let (sender, handovers) = mpsc::unbounded_channel::<Handover>();
    held()
        .lock()
        .expect("mhc held registry")
        .insert(thread.to_owned(), (id.clone(), sender.clone()));
    HeldSlot {
        guard: HeldGuard {
            thread: thread.to_owned(),
            id,
        },
        sender,
        handovers,
    }
}
impl HeldSlot {
    /// Gives the engine to a waiting turn; hands it back when that turn is gone.
    fn hand_over(&self, reply: Handover, engine: Box<Engine>) -> std::result::Result<(), Box<Engine>> {
        unregister_held(&self.guard.thread, &self.guard.id);
        reply.send(engine).inspect_err(|_| {
            held().lock().expect("mhc held registry").insert(
                self.guard.thread.clone(),
                (self.guard.id.clone(), self.sender.clone()),
            );
        })
    }
}

/// Keeps a finished turn's engine while its session waits on background work: completion
/// wakes run as follow-up turns whose replies go to `followups`, the session's next turn takes
/// the engine over, and `background_max` bounds the whole hold. A session that waits on
/// nothing closes its engine as before.
async fn hold(
    mut engine: Box<Engine>,
    config: OmoClientConfig,
    mut slot: HeldSlot,
    followups: UnboundedSender<String>,
) {
    let thread = slot.guard.thread.clone();
    let cwd = config.workspace.as_ref().map(|w| w.cwd.clone());
    let cwd = cwd.as_deref();
    let deadline = tokio::time::Instant::now() + config.background_max;
    let mut announced = false;
    let end = 'hold: loop {
        let state = match tokio::time::timeout_at(deadline, settle(&mut engine.rpc, &thread, cwd)).await {
            Err(_) => break HoldEnd::Expired,
            Ok(Err(_)) => break HoldEnd::Exited,
            Ok(Ok(state)) => state,
        };
        match state {
            // A turn that asked meanwhile gets no engine and starts its own once this one is
            // closed.
            Settle::Done => break HoldEnd::Settled,
            Settle::Started => {
                let run = follow_up(&mut engine.rpc, cwd, config.idle_timeout, &followups);
                match tokio::time::timeout_at(deadline, run).await {
                    Err(_) => break HoldEnd::Expired,
                    Ok(Err(error)) => {
                        tracing::warn!(session = %thread, %error, "background follow-up turn failed");
                        let _ = followups.send(owner_failure_text(&error.to_string()));
                        break HoldEnd::Failed;
                    }
                    Ok(Ok(())) => continue,
                }
            }
            Settle::Waiting => {}
        }
        if !announced {
            announced = true;
            tracing::info!(session = %thread, "mhc engine kept for its session's background work");
        }
        loop {
            let recheck = tokio::time::Instant::now() + BACKGROUND_RECHECK;
            tokio::select! {
                _ = tokio::time::sleep_until(deadline) => break 'hold HoldEnd::Expired,
                Some(reply) = slot.handovers.recv() => match slot.hand_over(reply, engine) {
                    Ok(()) => {
                        tracing::info!(session = %thread, "held mhc engine handed to the session's next turn");
                        return;
                    }
                    Err(back) => engine = back,
                },
                frame = engine.rpc.read() => match frame {
                    Err(_) => break 'hold HoldEnd::Exited,
                    Ok(frame) => {
                        if let Err(error) = engine.rpc.ui(&frame, cwd).await {
                            tracing::warn!(session = %thread, %error, "held mhc engine request failed");
                            break 'hold HoldEnd::Failed;
                        }
                        if frame["type"] == "agent_start" {
                            let run = follow_up(&mut engine.rpc, cwd, config.idle_timeout, &followups);
                            match tokio::time::timeout_at(deadline, run).await {
                                Err(_) => break 'hold HoldEnd::Expired,
                                Ok(Err(error)) => {
                                    tracing::warn!(session = %thread, %error, "background follow-up turn failed");
                                    let _ = followups.send(owner_failure_text(&error.to_string()));
                                    break 'hold HoldEnd::Failed;
                                }
                                Ok(Ok(())) => continue 'hold,
                            }
                        }
                    }
                },
                _ = tokio::time::sleep_until(recheck) => continue 'hold,
            }
        }
    };
    match end {
        HoldEnd::Expired => {
            tracing::warn!(session = %thread, limit_secs = config.background_max.as_secs(), "background work outlived its limit; stopping its engine");
            let _ = followups.send(BACKGROUND_EXPIRED.to_owned());
        }
        HoldEnd::Exited => {
            tracing::warn!(session = %thread, "held mhc engine exited before its background work settled");
            let _ = followups.send(BACKGROUND_LOST.to_owned());
        }
        HoldEnd::Settled if announced => {
            tracing::info!(session = %thread, "background work settled; closing its mhc engine")
        }
        HoldEnd::Settled | HoldEnd::Failed => {}
    }
    close(*engine, end == HoldEnd::Settled).await;
    // Only now does a turn that asked for this engine hear there is none and start its own,
    // so two engines never run the same session file.
    drop(slot);
    drop(followups);
}

/// One engine-started run (a completion wake): relays its replies like a turn's interim
/// messages, the last one included.
async fn follow_up(
    rpc: &mut Rpc,
    cwd: Option<&str>,
    idle: Duration,
    sink: &UnboundedSender<String>,
) -> Result<()> {
    let progress = Some(sink.clone());
    let mut pending = String::new();
    let mut delivered = 0;
    let mut seen = std::collections::HashSet::new();
    loop {
        let f = tokio::time::timeout(idle, rpc.read())
            .await
            .map_err(|_| HubError::message("mhc no output within idle timeout"))??;
        rpc.ui(&f, cwd).await?;
        match f["type"].as_str().unwrap_or("") {
            "message_end" => {
                let message = &f["message"];
                if let Some(error) = message_error(message) {
                    return Err(failure(error, true));
                }
                if message["role"] != "assistant" {
                    continue;
                }
                let identity = message
                    .get("id")
                    .filter(|v| !v.is_null())
                    .map(Value::to_string)
                    .unwrap_or_else(|| message.to_string());
                if !seen.insert(identity) {
                    continue;
                }
                let text = message_text(message);
                if !text.is_empty() {
                    relay(&mut pending, &progress, &mut delivered);
                    pending = text;
                }
            }
            "tool_execution_start" => relay(&mut pending, &progress, &mut delivered),
            "agent_end" => {
                if f["willRetry"].as_bool().is_none() || f["willRetry"] == true {
                    continue;
                }
                if f["aborted"] == true {
                    return Err(HubError::message("mhc turn interrupted"));
                }
                for message in f["messages"].as_array().into_iter().flatten() {
                    if let Some(error) = message_error(message) {
                        return Err(failure(error, true));
                    }
                }
                relay(&mut pending, &progress, &mut delivered);
                return Ok(());
            }
            "error" => {
                return Err(failure(
                    f["error"].as_str().unwrap_or("mhc RPC error").to_owned(),
                    true,
                ))
            }
            _ => {}
        }
    }
}

async fn run_owned(
    rpc: &mut Rpc,
    config: &OmoClientConfig,
    request: &TurnRequest,
    progress: Option<UnboundedSender<String>>,
    key: Option<&str>,
    path: Option<String>,
) -> Result<TurnOutcome> {
    let cwd = config.workspace.as_ref().map(|w| w.cwd.as_str());
    let data = tokio::time::timeout(
        config.connect_timeout,
        rpc.request(protocol::command("state", "get_state"), cwd),
    )
    .await
    .map_err(|_| HubError::message("timeout opening mhc session"))??;
    let thread = data["sessionFile"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| HubError::message("mhc get_state missing sessionFile"))?
        .to_owned();
    // Model and thinking come from launch flags (run_turn). The set_model,
    // set_thinking_level and set_auto_retry RPCs rewrite the global agent settings, which
    // hot-reloads mhc and makes the next prompt fail with a stale extension context.
    let turn = uuid::Uuid::new_v4().to_string();
    let (sender, mut guidance) = mpsc::unbounded_channel();
    let active_key = (thread.clone(), turn.clone());
    active()
        .lock()
        .expect("mhc active registry")
        .insert(active_key.clone(), sender);
    let _guard = ActiveGuard(active_key);
    if let Some(key) = key {
        crate::chat::turns::started(key, &thread, &turn);
    }
    let mut prompt = protocol::command("3", "prompt");
    prompt["message"] = json!(request.text);
    rpc.send(prompt).await?;
    let mut acked = false;
    let mut started = false;
    let ack_until = tokio::time::Instant::now() + config.ack_window;
    let mut idle_until = tokio::time::Instant::now() + config.idle_timeout;
    let mut pending = String::new();
    let mut delivered = 0;
    let mut tools = 0;
    let mut seen_messages = std::collections::HashSet::new();
    let mut seen_tools = std::collections::HashSet::new();
    let mut steer_id = 100u64;
    let mut pending_steers: HashMap<String, oneshot::Sender<bool>> = HashMap::new();
    loop {
        let until = if acked || started {
            idle_until
        } else {
            ack_until.min(idle_until)
        };
        let f = tokio::select! {
            f = tokio::time::timeout_at(until,rpc.read()) => f.map_err(|_|HubError::message(if acked || started { "mhc no output within idle timeout" } else { TURN_START_ACK_TIMEOUT }))??,
            Some(g) = guidance.recv() => {
                steer_id += 1;
                let id = steer_id.to_string();
                let mut command = protocol::command(&id,"steer"); command["message"]=json!(g.text);
                rpc.send(command).await?;
                pending_steers.insert(id,g.reply);
                continue;
            }
        };
        idle_until = tokio::time::Instant::now() + config.idle_timeout;
        if f["type"] == "response" {
            if let Some(id) = f["id"].as_str() {
                if let Some(reply) = pending_steers.remove(id) {
                    let _ = reply.send(f["success"] == true);
                    continue;
                }
            }
            if f["id"] == "3" {
                if f["success"] != true {
                    return Err(failure(
                        f["error"].as_str().unwrap_or("prompt rejected").to_owned(),
                        tools > 0 || delivered > 0 || !pending.is_empty(),
                    ));
                }
                acked = true;
            }
            continue;
        }
        rpc.ui(&f, cwd).await?;
        match f["type"].as_str().unwrap_or("") {
            "agent_start" => started = true,
            "message_end" if started => {
                let message = &f["message"];
                if let Some(error) = message_error(message) {
                    return Err(failure(
                        error,
                        tools > 0 || delivered > 0 || !pending.is_empty(),
                    ));
                }
                if message["role"] != "assistant" {
                    continue;
                }
                let identity = message
                    .get("id")
                    .filter(|v| !v.is_null())
                    .map(Value::to_string)
                    .unwrap_or_else(|| message.to_string());
                if !seen_messages.insert(identity) {
                    continue;
                }
                let text = message_text(message);
                if !text.is_empty() {
                    relay(&mut pending, &progress, &mut delivered);
                    pending = text;
                }
            }
            "tool_execution_start" if started => {
                let id = f["toolCallId"]
                    .as_str()
                    .ok_or_else(|| HubError::message("mhc tool start missing toolCallId"))?;
                if seen_tools.insert(id.to_owned()) {
                    tools += 1;
                    relay(&mut pending, &progress, &mut delivered);
                }
            }
            "agent_end" if started => {
                // mhc emits a raw boundary before its enriched retry decision.
                if f["willRetry"].as_bool().is_none() {
                    continue;
                }
                if f["aborted"] == true {
                    return Err(HubError::message("mhc turn interrupted"));
                }
                if f["willRetry"] == true {
                    continue;
                }
                if let Some(messages) = f["messages"].as_array() {
                    for message in messages {
                        if let Some(error) = message_error(message) {
                            return Err(failure(
                                error,
                                tools > 0 || delivered > 0 || !pending.is_empty(),
                            ));
                        }
                    }
                }
                return Ok(TurnOutcome {
                    thread_id: thread,
                    resumed: path.is_some(),
                    turn_id: turn,
                    text: pending,
                    approvals_allowed: rpc.allowed,
                    approvals_denied: rpc.denied,
                    messages_delivered: delivered,
                    tool_calls: tools,
                });
            }
            "error" => {
                return Err(failure(
                    f["error"].as_str().unwrap_or("mhc RPC error").to_owned(),
                    tools > 0 || delivered > 0 || !pending.is_empty(),
                ))
            }
            _ => {}
        }
    }
}
fn relay(pending: &mut String, progress: &Option<UnboundedSender<String>>, delivered: &mut usize) {
    if pending.is_empty() || is_explicit_silence(pending) {
        pending.clear();
        return;
    }
    if let Some(sender) = progress {
        let text = std::mem::take(pending);
        if sender.send(text).is_ok() {
            *delivered += 1;
        }
    } else {
        pending.clear();
    }
}
/// The workspace omo-gateway gives Slack turns: `<root>/agents/bot-<persona>` plus
/// `<root>/shared`, where root is `OMON_WORKSPACE_ROOT` or `~/.ferryx/hub/workspace`. Absent when the
/// directory does not exist, so the engine keeps its own default.
pub fn slack_workspace() -> Option<protocol::Workspace> {
    let root = crate::hub_env::var("OMON_WORKSPACE_ROOT")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            crate::hub_env::var_os("HOME")
                .map(|home| PathBuf::from(home).join(".ferryx/hub/workspace"))
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

#[cfg(all(test, unix))]
mod rpc_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    struct Script {
        dir: PathBuf,
        config: OmoClientConfig,
    }
    impl Script {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("hub-mhc-rpc-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            let binary = dir.join("mhc");
            std::fs::write(&binary,r#"#!/usr/bin/env python3
import json, os, sys, pathlib, threading
root = pathlib.Path(__file__).parent
with (root / 'pids').open('a') as out: out.write(str(os.getpid())+'\n')
session = root / 'session.jsonl'
log = root / 'commands.jsonl'
scenario = ''
def emit(v):
    print(json.dumps(v),flush=True)
def event(kind, **kw):
    emit(dict(type=kind,**kw))
def answer(c, data=None):
    emit(dict(type='response',id=c['id'],command=c['type'],success=True,data=data))
def message(text, ident, **kw):
    event('message_end',message=dict(role='assistant',id=ident,content=[dict(type='text',text=text)],**kw))
def lifecycle(state, active):
    with session.open('a') as out: out.write(json.dumps(dict(type='custom',customType='session-lifecycle',data=dict(state=state,runActive=active)))+'\n')
def wake():
    # A senpi-task child finishes once the test opens the release pipe for writing.
    with open(root / 'release') as gate: gate.read()
    lifecycle('running',True)
    event('agent_start')
    message('background done','wake-1')
    lifecycle('done',False)
    event('agent_end',messages=[],willRetry=False)
assert '--mode' in sys.argv and 'rpc' in sys.argv
assert sys.argv[1:4] == ['--mode','rpc','--no-model-fallback']
assert '--multi-session' not in sys.argv
(root / 'argv.json').write_text(json.dumps(sys.argv[1:]))
for line in sys.stdin:
    c=json.loads(line)
    with log.open('a') as out: out.write(json.dumps(c)+'\n')
    kind=c['type']
    assert 'sessionId' not in c
    assert kind != 'open_session'
    if kind=='get_state':
        if '--session' in sys.argv: assert sys.argv[sys.argv.index('--session')+1]==str(session)
        session.write_text(json.dumps(dict(type='session',id='durable-id'))+'\n')
        answer(c,dict(sessionId='durable-id',sessionFile=str(session)))
    elif kind in ['set_auto_retry','set_model','set_thinking_level']:
        emit(dict(type='response',id=c['id'],command=kind,success=False,error='persists global settings'))
    elif kind=='prompt':
        scenario=c['message']
        if scenario=='exit': sys.exit(17)
        answer(c)
        lifecycle('running',True)
        if scenario=='background':
            os.mkfifo(root / 'release')
            threading.Thread(target=wake,daemon=True).start()
        event('agent_start')
        event('extension_ui_request',id='status',method='setStatus',statusKey='test')
        if scenario=='retry':
            message('not settled','retry-message')
            event('agent_end',messages=[])
            event('agent_end',messages=[],willRetry=True)
            message('settled','retry-final')
            event('agent_end',messages=[],willRetry=False)
        elif scenario=='503':
            message('', 'err',stopReason='error',errorMessage='HTTP 503 overloaded')
        elif scenario=='approval':
            event('extension_ui_request',id='approve',method='confirm',title='Run command',message='ls')
        elif scenario=='owner-deny':
            event('extension_ui_request',id='approve',method='confirm',title='Run command',message='curl -X POST https://example.com --data payload')
        elif scenario=='retired':
            event('extension_ui_request',id='approve',method='confirm',title='Run command',message='launchctl enable gui/501/ai.omon.gateway')
        elif scenario=='permission':
            event('extension_ui_request',id='perm',method='select',title='Permission required: bash\n\nCommand: $ curl -X POST https://example.com --data payload\n\nPatterns:\n  - curl *',options=['Allow once','Allow always','Deny','Deny with feedback'],timeout=None)
        elif scenario in ['select','input']:
            event('extension_ui_request',id='dialog',method=scenario,title='Choose',options=['one','two'])
        elif scenario=='question':
            event('extension_ui_request',id='question',method='question',requestId='q',toolCallId='tool',waitForAnswer=True,questions=[],timeout=30,askedAtMs=0,deadlineAtMs=30,remainingMs=30)
        elif scenario in ['steer','stop']:
            message('ready','ready')
            event('tool_execution_start',toolCallId='tool-1',toolName='read',args={})
        else:
            message('interim','m1')
            message('interim','m1')
            event('tool_execution_start',toolCallId='tool-1',toolName='read',args={})
            event('tool_execution_start',toolCallId='tool-1',toolName='read',args={})
            message('final','m2')
            event('agent_end',messages=[])
            # Recorded before the boundary so a released wake always comes after it.
            lifecycle('waiting_background' if scenario=='background' else 'done',False)
            event('agent_end',messages=[],willRetry=False)
            if scenario=='success-exit': sys.exit(0)
    elif kind=='extension_ui_response':
        assert c['id'] != 'status'
        if scenario=='approval': assert c['confirmed']==True
        elif scenario in ['retired','owner-deny']: assert c['confirmed']==False
        elif scenario=='permission': assert c['value'] in ['Allow once','Deny']
        else: assert c['cancelled']==True
        message('final','m2')
        event('agent_end',messages=[])
        event('agent_end',messages=[],willRetry=False)
    elif kind=='steer':
        assert c['message']=='guidance'
        answer(c)
        message('steered','m2')
        event('agent_end',messages=[])
        event('agent_end',messages=[],willRetry=False)
"#).unwrap();
            std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
            let mut config = OmoClientConfig::new(binary.to_string_lossy());
            config.total_timeout = Duration::from_secs(10);
            config.idle_timeout = Duration::from_secs(5);
            Self { dir, config }
        }
        fn commands(&self) -> Vec<Value> {
            std::fs::read_to_string(self.dir.join("commands.jsonl"))
                .unwrap()
                .lines()
                .map(|s| serde_json::from_str(s).unwrap())
                .collect()
        }
        fn argv(&self) -> Vec<String> {
            serde_json::from_str(&std::fs::read_to_string(self.dir.join("argv.json")).unwrap())
                .unwrap()
        }
        fn last_ui_response(&self) -> Value {
            self.commands()
                .into_iter()
                .rev()
                .find(|frame| frame["type"] == "extension_ui_response")
                .unwrap()
        }
        async fn run(
            &self,
            text: &str,
            thread_id: Option<String>,
            progress: Option<UnboundedSender<String>>,
        ) -> Result<TurnOutcome> {
            run_turn_recovering(
                &self.config,
                &TurnRequest {
                    text: text.into(),
                    thread_id,
                    model: Some("mahoquot/gpt-6-luna".into()),
                    ..Default::default()
                },
                progress,
            )
            .await
        }
        async fn run_followed(
            &self,
            text: &str,
            thread_id: Option<String>,
        ) -> (Result<TurnOutcome>, mpsc::UnboundedReceiver<String>) {
            let (followups, replies) = mpsc::unbounded_channel();
            let result = run_turn(
                &self.config,
                &TurnRequest {
                    text: text.into(),
                    thread_id,
                    followups: Some(followups),
                    ..Default::default()
                },
                None,
            )
            .await;
            (result, replies)
        }
        fn session(&self) -> String {
            self.dir.join("session.jsonl").to_string_lossy().into_owned()
        }
        fn pids(&self) -> Vec<i32> {
            std::fs::read_to_string(self.dir.join("pids"))
                .unwrap()
                .lines()
                .map(|line| line.parse().unwrap())
                .collect()
        }
        /// Lets the fixture's background task finish: its reader blocks until a writer opens.
        async fn release_background(&self) {
            let path = self.dir.join("release");
            tokio::task::spawn_blocking(move || std::fs::write(path, b"go"))
                .await
                .unwrap()
                .unwrap();
        }
        async fn run_with_tools(&self, toolsets: &[&str]) -> Result<TurnOutcome> {
            run_turn(
                &self.config,
                &TurnRequest {
                    text: "normal".into(),
                    toolsets: Some(toolsets.iter().map(|s| (*s).to_owned()).collect()),
                    ..Default::default()
                },
                None,
            )
            .await
        }
    }
    impl Drop for Script {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.dir).unwrap();
        }
    }

    #[test]
    fn only_existing_session_files_resume() {
        let script = Script::new();
        let file = script.dir.join("session.jsonl");
        std::fs::write(&file, "{}\n").unwrap();
        assert_eq!(
            resume_path(file.to_str()),
            Some(file.to_string_lossy().into_owned())
        );
        assert!(resume_path(Some("old-appserver-thread")).is_none());
        std::fs::remove_file(&file).unwrap();
        assert!(resume_path(file.to_str()).is_none());
    }
    #[tokio::test]
    async fn starts_resumes_and_relays_completed_interim_exactly_once() {
        let script = Script::new();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let first = script.run("normal", None, Some(tx)).await.unwrap();
        assert!(!first.resumed);
        assert_eq!(first.text, "final");
        assert_eq!(first.messages_delivered, 1);
        assert_eq!(first.tool_calls, 1);
        assert_eq!(rx.try_recv().unwrap(), "interim");
        assert!(matches!(
            rx.try_recv(),
            Err(mpsc::error::TryRecvError::Disconnected)
        ));
        let second = script
            .run(
                "normal",
                Some(
                    script
                        .dir
                        .join("session.jsonl")
                        .to_string_lossy()
                        .into_owned(),
                ),
                None,
            )
            .await
            .unwrap();
        assert!(second.resumed);
        assert_eq!(second.thread_id, first.thread_id);
        let commands = script.commands();
        assert_eq!(
            commands.iter().filter(|c| c["type"] == "get_state").count(),
            2
        );
        assert!(!commands.iter().any(|c| matches!(
            c["type"].as_str(),
            Some("set_model" | "set_thinking_level" | "set_auto_retry")
        )));
    }
    #[tokio::test]
    async fn model_and_thinking_are_launch_flags_not_persisted_rpcs() {
        let script = Script::new();
        script.run("normal", None, None).await.unwrap();
        let argv = script.argv();
        let flag = |name: &str| {
            argv.iter()
                .position(|arg| arg == name)
                .map(|index| argv[index + 1].clone())
        };
        assert_eq!(flag("--model").as_deref(), Some("mahoquot/gpt-6-luna"));
        assert_eq!(flag("--thinking"), Some(protocol::default_effort()));
        assert!(!script.commands().iter().any(|c| matches!(
            c["type"].as_str(),
            Some("set_model" | "set_thinking_level" | "set_auto_retry")
        )));
    }
    #[tokio::test]
    async fn legacy_toolsets_launch_without_tools_flag() {
        let script = Script::new();
        script
            .run_with_tools(&["terminal", "browser", "file"])
            .await
            .unwrap();
        assert!(!script.argv().iter().any(|arg| arg == "--tools"));
        script.run_with_tools(&["read", "terminal"]).await.unwrap();
        assert!(!script.argv().iter().any(|arg| arg == "--tools"));
        script.run_with_tools(&[]).await.unwrap();
        assert!(!script.argv().iter().any(|arg| arg == "--tools"));
        script.run_with_tools(&["read", "bash"]).await.unwrap();
        let argv = script.argv();
        let index = argv.iter().position(|arg| arg == "--tools").unwrap();
        assert_eq!(argv[index + 1], "read,bash");
    }
    #[tokio::test]
    async fn permission_select_is_answered_by_policy() {
        let script = Script::new();
        let allowed = FULL_ACCESS_TEST
            .scope(true, script.run("permission", None, None))
            .await
            .unwrap();
        assert_eq!(allowed.approvals_allowed, 1);
        let response = script.last_ui_response();
        assert_eq!(response["id"], "perm");
        assert_eq!(response["value"], "Allow once");
        assert!(response.get("cancelled").is_none());
        let denied = FULL_ACCESS_TEST
            .scope(
                false,
                with_approver(
                    std::sync::Arc::new(Deny),
                    script.run("permission", None, None),
                ),
            )
            .await
            .unwrap();
        assert_eq!(denied.approvals_denied, 1);
        assert_eq!(denied.approvals_allowed, 0);
        let response = script.last_ui_response();
        assert_eq!(response["id"], "perm");
        assert_eq!(response["value"], "Deny");
    }
    #[tokio::test]
    async fn native_confirm_approvals_questions_and_retired_gateway_block() {
        let script = Script::new();
        let allowed = FULL_ACCESS_TEST
            .scope(true, script.run("approval", None, None))
            .await
            .unwrap();
        assert_eq!(allowed.approvals_allowed, 1);
        let denied = FULL_ACCESS_TEST
            .scope(true, script.run("retired", None, None))
            .await
            .unwrap();
        assert_eq!(denied.approvals_denied, 1);
        assert_eq!(
            script.run("question", None, None).await.unwrap().text,
            "final"
        );
    }
    struct Deny;
    #[async_trait::async_trait]
    impl Approver for Deny {
        async fn ask(&self, frame: &Value, _: Option<&str>, _: &str) -> OwnerAnswer {
            assert!(!matches!(frame["method"].as_str(), Some("select" | "input")));
            OwnerAnswer::Deny
        }
    }
    #[tokio::test]
    async fn nonlocal_confirmation_reaches_owner_approver() {
        let script = Script::new();
        // The nonlocal effect is declined by the owner when full access is off.
        let result = FULL_ACCESS_TEST
            .scope(
                false,
                with_approver(
                    std::sync::Arc::new(Deny),
                    script.run("owner-deny", None, None),
                ),
            )
            .await
            .unwrap();
        assert_eq!(result.approvals_denied, 1);
        for method in ["select", "input"] {
            assert_eq!(
                with_approver(std::sync::Arc::new(Deny), script.run(method, None, None))
                    .await
                    .unwrap()
                    .text,
                "final"
            );
            let commands = script.commands();
            let response = commands
                .iter()
                .rev()
                .find(|frame| frame["type"] == "extension_ui_response")
                .unwrap();
            assert_eq!(response["id"], "dialog");
            assert_eq!(response["cancelled"], true);
        }
    }

    #[tokio::test]
    async fn overload_is_eligible_but_no_hidden_retry_occurs() {
        let script = Script::new();
        let result = script.run("503", None, None).await;
        assert!(fallback_eligible(&result));
        assert_eq!(
            script
                .commands()
                .iter()
                .filter(|c| c["type"] == "prompt")
                .count(),
            1
        );
        assert_eq!(
            script.run("normal", None, None).await.unwrap().text,
            "final"
        );
        assert_eq!(
            script
                .commands()
                .iter()
                .filter(|c| c["type"] == "prompt")
                .count(),
            2
        );
        assert!(!fallback_eligible(&Err(failure(
            "503 overloaded".into(),
            true
        ))));
    }
    #[tokio::test]
    async fn retry_boundary_does_not_complete_the_turn() {
        let script = Script::new();
        assert_eq!(
            script.run("retry", None, None).await.unwrap().text,
            "settled"
        );
        assert_eq!(
            script
                .commands()
                .iter()
                .filter(|c| c["type"] == "prompt")
                .count(),
            1
        );
    }
    #[tokio::test]
    async fn buffered_success_survives_immediate_process_exit() {
        let script = Script::new();
        let result = script.run("success-exit", None, None).await.unwrap();
        assert_eq!(result.text, "final");
    }
    #[tokio::test]
    async fn immediate_process_exit_is_reported() {
        let script = Script::new();
        let error = script
            .run("exit", None, None)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("exited"), "{error}");
        assert!(error.contains("17"), "{error}");
    }
    #[tokio::test]
    async fn active_process_receives_steer_and_stop_without_polling() {
        for scenario in ["steer", "stop"] {
            let script = Script::new();
            let key = format!("rpc-test-{}", uuid::Uuid::new_v4());
            let _guard = crate::chat::turns::begin(&key, "test");
            let (tx, mut rx) = mpsc::unbounded_channel();
            let config = script.config.clone();
            let task_key = key.clone();
            let turn = tokio::spawn(crate::chat::turns::scope(task_key, async move {
                run_turn(
                    &config,
                    &TurnRequest {
                        text: scenario.into(),
                        ..Default::default()
                    },
                    Some(tx),
                )
                .await
            }));
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(5), rx.recv())
                    .await
                    .unwrap()
                    .as_deref(),
                Some("ready")
            );
            let (thread, id) = crate::chat::turns::ids(&key).unwrap();
            if scenario == "steer" {
                assert!(steer(&script.config, &thread, &id, "guidance")
                    .await
                    .unwrap());
                assert_eq!(turn.await.unwrap().unwrap().text, "steered");
            } else {
                assert!(crate::chat::turns::stop(&key));
                assert!(turn
                    .await
                    .unwrap()
                    .unwrap_err()
                    .to_string()
                    .contains(STOPPED));
            }
            assert!(!steer(&script.config, &thread, &id, "too late")
                .await
                .unwrap());
        }
    }
    #[tokio::test]
    async fn the_latest_lifecycle_record_decides_whether_the_session_waits() {
        let script = Script::new();
        let path = script.dir.join("lifecycle.jsonl");
        let record = |state: &str, active: bool, background: u64, children: u64| {
            json!({"type":"custom","customType":"session-lifecycle","data":{"state":state,"runActive":active,
                "waitingOn":{"children":children,"background":[{"source":"senpi-task","activeCount":background}],"human":[]}}})
            .to_string()
        };
        let path_str = path.to_str().unwrap();
        assert_eq!(latest_lifecycle(path_str).await, None);
        std::fs::write(&path, "{\"type\":\"session\"}\n").unwrap();
        assert_eq!(latest_lifecycle(path_str).await, None);
        // The evidence: the final reply, then waiting_background with one senpi-task child.
        std::fs::write(
            &path,
            [
                record("running", true, 1, 1),
                json!({"type":"custom","customType":"work-custody","data":{}}).to_string(),
                record("waiting_background", false, 1, 0),
            ]
            .join("\n"),
        )
        .unwrap();
        let latest = latest_lifecycle(path_str).await.unwrap();
        assert!(!latest.run_active && latest.waiting());
        // A question to the owner does not hide background work still running.
        std::fs::write(&path, record("waiting_human", false, 1, 0)).unwrap();
        assert!(latest_lifecycle(path_str).await.unwrap().waiting());
        std::fs::write(&path, record("done", false, 0, 0)).unwrap();
        assert!(!latest_lifecycle(path_str).await.unwrap().waiting());
    }
    fn alive(pid: i32) -> bool {
        unsafe { libc::kill(pid, 0) == 0 }
    }
    async fn closed(replies: &mut mpsc::UnboundedReceiver<String>) {
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(10), replies.recv())
                .await
                .expect("the held engine closes"),
            None
        );
    }
    #[tokio::test]
    async fn background_work_outlives_its_turn_and_its_wake_reply_is_delivered() {
        let script = Script::new();
        let (result, mut replies) = script.run_followed("background", None).await;
        assert_eq!(result.unwrap().text, "final");
        let pid = script.pids()[0];
        // The turn ended but the engine that runs the background task is still up.
        assert!(alive(pid));
        script.release_background().await;
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(10), replies.recv())
                .await
                .unwrap()
                .as_deref(),
            Some("background done")
        );
        closed(&mut replies).await;
        assert!(!alive(pid));
    }
    #[tokio::test]
    async fn the_next_turn_takes_over_a_held_engine() {
        let script = Script::new();
        let (first, mut replies) = script.run_followed("background", None).await;
        first.unwrap();
        let second = script
            .run("normal", Some(script.session()), None)
            .await
            .unwrap();
        assert!(second.resumed);
        assert_eq!(second.text, "final");
        // One process served both turns, so the background task was never restored.
        assert_eq!(script.pids().len(), 1);
        assert_eq!(
            script
                .commands()
                .iter()
                .filter(|c| c["type"] == "prompt")
                .count(),
            2
        );
        closed(&mut replies).await;
        assert!(!alive(script.pids()[0]));
    }
    #[tokio::test]
    async fn background_work_is_stopped_at_its_limit_and_the_owner_told() {
        let mut script = Script::new();
        script.config.background_max = Duration::from_millis(500);
        let (result, mut replies) = script.run_followed("background", None).await;
        result.unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(10), replies.recv())
                .await
                .unwrap()
                .as_deref(),
            Some(BACKGROUND_EXPIRED)
        );
        closed(&mut replies).await;
        assert!(!alive(script.pids()[0]));
    }
    #[tokio::test]
    async fn turns_without_a_follow_up_sink_or_background_work_close_their_engine() {
        let script = Script::new();
        script.run("background", None, None).await.unwrap();
        let (result, mut replies) = script.run_followed("normal", None).await;
        result.unwrap();
        closed(&mut replies).await;
        let pids = script.pids();
        assert_eq!(pids.len(), 2);
        assert!(pids.iter().all(|pid| !alive(*pid)));
    }
    #[tokio::test]
    #[ignore = "requires installed mhc and isolated agent credentials"]
    async fn live_single_session_mhc_turn() {
        let isolated =
            std::env::var("FERRYX_RPC_LIVE_HOME").expect("run live test with isolated HOME");
        assert_eq!(std::env::var("HOME").unwrap(), isolated);
        assert_eq!(
            super::super::supervisor::agent_dir().unwrap(),
            PathBuf::from(&isolated).join("agent")
        );
        let config = OmoClientConfig {
            total_timeout: Duration::from_secs(180),
            ..Default::default()
        };
        let outcome = run_turn(
            &config,
            &TurnRequest {
                model: Some("mahoquot/gpt-6-luna".into()),
                text: "Reply with exactly FERRYX_RPC_LIVE_OK. Do not call tools.".into(),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
        assert!(
            outcome.text.contains("FERRYX_RPC_LIVE_OK"),
            "LIVE_SESSION_FILE={} LIVE_TEXT={}",
            outcome.thread_id,
            outcome.text
        );
        assert!(
            std::path::Path::new(&outcome.thread_id).is_file(),
            "LIVE_SESSION_FILE={}",
            outcome.thread_id
        );
    }
}
