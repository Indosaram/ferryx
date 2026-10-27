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
    async fn ui(&mut self, frame: &Value, cwd: Option<&str>) -> Result<()> {
        if frame["type"] != "extension_ui_request" {
            return Ok(());
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
        let retired = starts_retired_gateway(&policy_frame);
        let mut verdict = if retired {
            Some(RETIRED_GATEWAY_REASON.into())
        } else {
            approval_verdict(&policy_frame, cwd)
        };
        if !retired {
            if let Some(reason) = verdict.clone() {
                if let Ok(approver) = APPROVER.try_with(std::sync::Arc::clone) {
                    verdict = match approver.ask(&policy_frame, cwd, &reason).await {
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
        self.send(json!({"type":"extension_ui_response","id":id,"confirmed":verdict.is_none()}))
            .await?;
        if self.denied >= APPROVAL_DENIAL_TURN_LIMIT {
            return Err(HubError::message("mhc approval denial limit reached"));
        }
        Ok(())
    }
    async fn request(&mut self, value: Value, cwd: Option<&str>) -> Result<Value> {
        let id = value["id"].clone();
        self.send(value).await?;
        loop {
            let f = self.read().await?;
            if f["type"] == "response" && f["id"] == id {
                if f["success"] != true {
                    let error = format!("{}: {}", f["command"], f["error"]);
                    return Err(if f["command"] == "set_model" {
                        failure(error, false)
                    } else {
                        HubError::message(error)
                    });
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

struct GuardDirectory(PathBuf);
impl Drop for GuardDirectory {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            tracing::warn!(%error,"mhc temporary launchctl guard cleanup failed");
        }
    }
}

pub async fn run_turn(
    config: &OmoClientConfig,
    request: &TurnRequest,
    progress: Option<UnboundedSender<String>>,
) -> Result<TurnOutcome> {
    let stored_id = request.thread_id.clone();
    let (child_path, _guard, session_path) = tokio::task::spawn_blocking(move || -> Result<_> {
        let path = crate::child_env::child_path().to_owned();
        // A private guard must never write into the running user's ~/.ferryx/hub.
        let dir = std::env::temp_dir().join(format!("ferryx-mhc-guard-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir)?;
        let guard = GuardDirectory(dir);
        let bin = super::supervisor::install_launchctl_guard(&guard.0)?;
        let mut paths = vec![bin];
        paths.extend(std::env::split_paths(&path));
        let path = std::env::join_paths(paths).map_err(|e| HubError::message(e.to_string()))?;
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
    let mut instructions = request.system_prompt.clone().unwrap_or_default();
    if let Some(w) = &config.workspace {
        cmd.current_dir(&w.cwd);
        instructions.push_str("\nAdditional workspace roots:\n");
        instructions.push_str(&w.roots.join("\n"));
    }
    if !instructions.is_empty() {
        cmd.arg("--append-system-prompt").arg(instructions);
    }
    if let Some(tools) = &request.toolsets {
        cmd.arg("--tools").arg(tools.join(","));
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| HubError::message(format!("spawn mhc RPC: {e}")))?;
    let mut rpc = Rpc {
        input: child.stdin.take().expect("piped mhc stdin"),
        lines: BufReader::new(child.stdout.take().expect("piped mhc stdout")).lines(),
        allowed: 0,
        denied: 0,
        eof: false,
    };
    let process_group = child.id();
    let key = crate::chat::turns::current_key();
    let mut cancel = key.as_deref().and_then(crate::chat::turns::cancelled);
    let work = run_owned(
        &mut rpc,
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
    if rpc.eof {
        // Only inspect exit after stdout is drained: racing wait() against reads can hide
        // a successful final boundary already buffered in the pipe.
        let status = match tokio::time::timeout(Duration::from_secs(1), child.wait()).await {
            Ok(Ok(status)) => status.to_string(),
            Ok(Err(error)) => format!("status unavailable: {error}"),
            Err(_) => "process still running after stdout EOF".to_owned(),
        };
        result = Err(HubError::message(format!(
            "mhc RPC process exited before completing the turn ({status})"
        )));
    }
    // No path may orphan the owned engine. Closing stdin first lets the host observe EOF;
    // kill + wait bounds cleanup even when an extension or provider refuses cancellation.
    if result.is_err() {
        let abort = protocol::command("abort", "abort");
        match tokio::time::timeout(Duration::from_secs(1), rpc.send(abort)).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => tracing::warn!(%error,"mhc abort write failed"),
            Err(error) => tracing::warn!(%error,"mhc abort write timed out"),
        }
    }
    drop(rpc);
    reap(&mut child, process_group).await;
    result
}
async fn reap(child: &mut Child, process_group: Option<u32>) {
    #[cfg(not(unix))]
    let _ = process_group;
    #[cfg(unix)]
    if let Some(pid) = process_group {
        // Tool subprocesses inherit the private group; stop them with their owned engine.
        let result = unsafe { libc::kill(-(pid as i32), libc::SIGKILL) };
        if result != 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH) {
            tracing::warn!(error=%std::io::Error::last_os_error(), "mhc RPC process group kill failed");
        }
    }
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
    // This native RPC persists retry.enabled=false in global agent settings. No supported
    // CLI retry override exists; restoring it would race other sessions and reenable retry.
    let mut retry = protocol::command("retry", "set_auto_retry");
    retry["enabled"] = json!(false);
    tokio::time::timeout(config.connect_timeout, rpc.request(retry, cwd))
        .await
        .map_err(|_| HubError::message("timeout disabling mhc auto retry"))??;
    if let Some(model) = request.model.as_deref().filter(|s| !s.trim().is_empty()) {
        let (provider, model_id) = model.split_once('/').unwrap_or(("", model));
        let mut command = protocol::command("model", "set_model");
        command["provider"] = json!(provider);
        command["modelId"] = json!(model_id);
        tokio::time::timeout(config.connect_timeout, rpc.request(command, cwd))
            .await
            .map_err(|_| HubError::message("timeout selecting mhc model"))??;
    }
    let mut thinking = protocol::command("thinking", "set_thinking_level");
    thinking["level"] = json!(protocol::default_effort());
    tokio::time::timeout(config.connect_timeout, rpc.request(thinking, cwd))
        .await
        .map_err(|_| HubError::message("timeout selecting mhc thinking level"))??;
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
import json, sys, pathlib
root = pathlib.Path(__file__).parent
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
assert '--mode' in sys.argv and 'rpc' in sys.argv
assert sys.argv[1:4] == ['--mode','rpc','--no-model-fallback']
assert '--multi-session' not in sys.argv
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
    elif kind=='set_auto_retry':
        assert c['enabled']==False
        answer(c)
    elif kind in ['set_model','set_thinking_level']: answer(c)
    elif kind=='prompt':
        scenario=c['message']
        if scenario=='exit': sys.exit(17)
        answer(c)
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
            event('agent_end',messages=[],willRetry=False)
            if scenario=='success-exit': sys.exit(0)
    elif kind=='extension_ui_response':
        assert c['id'] != 'status'
        if scenario=='approval': assert c['confirmed']==True
        elif scenario in ['retired','owner-deny']: assert c['confirmed']==False
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
        for prompt in commands
            .iter()
            .enumerate()
            .filter(|(_, c)| c["type"] == "prompt")
        {
            assert!(commands[..prompt.0]
                .iter()
                .any(|c| c["type"] == "set_auto_retry" && c["enabled"] == false));
        }
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
