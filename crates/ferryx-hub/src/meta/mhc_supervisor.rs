//! The retained mhc supervisor runtime: one owned `mhc --mode rpc --multi-session` engine over
//! stdio, with role-scoped sessions.
//!
//! - The engine is checked with `get_protocol_info` before anything else is sent. A build that
//!   lacks a required capability is a named [`SupervisorFault::Capability`] and the runtime
//!   performs no automatic action (no session, no prompt) until a relaunch passes the check.
//! - One serialized owner-ingest session learns from owner-authored text. Conversations are
//!   retained per project/channel scope with memory writes denied and the typed work tools on.
//!   [`DECISION_SLOTS`] decision slots run memory-write-denied sessions without effect tools; an
//!   occupied slot never stalls another slot or owner cancellation. A convergence check always
//!   gets a fresh context.
//! - Durable session identities are persisted and verified on resume; a resumed session that
//!   returns another identity is refused. A session the engine closed (for any reason, known or
//!   not, e.g. `idle_evicted` or `host_shutdown`) is resumed on its next use.
//! - Faults are typed and secret-redacted. There is no model fallback: a provider failure is
//!   reported, never answered by another model, and an engine-side model fallback is refused.
//! - An in-flight turn is never retransmitted after the engine exits.
//! - One process owns the profile at a time: the exclusive OS lock `<data>/mhc-supervisor.lock`
//!   is taken before the launch and released only once the owned child is reaped. A successor
//!   (a handover's new daemon) waits a bounded time for it, then reports
//!   [`SupervisorFault::ProfileBusy`] with nothing launched or sent.
//! - Teardown: the engine lives as long as a hub generation supervises it. When that
//!   generation ends (hub stop, handover retirement) the engine's stdin is closed, it gets
//!   [`SHUTDOWN_GRACE`] to exit, then it is killed and reaped; no call relaunches it until the
//!   next generation supervises again. A process that never owned the engine (the MCP server)
//!   reports [`SupervisorFault::NotRunning`] instead of using another model.
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::ffi::OsString;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, RwLock, Weak};
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot, watch, OwnedMutexGuard, OwnedSemaphorePermit, Semaphore};

use super::mhc_fault::{redact, SupervisorFault, TurnContext, TurnFault};
use super::mhc_profile::{
    profile_lock_held, ConversationScope, MhcProfile, MhcProfileConfig, PersistedSession,
    ProfileLock, RoleSpec, SessionRole, SessionStore, CLIENT_CAPABILITIES, DECISION_SLOTS,
};
use super::mhc_stdio::{BoxedReader, BoxedWriter, RequestError, StdioClient, WORK_REPLY_REQUEST};
use crate::error::HubError;
use crate::lanes::kibitzer::ModelTurn;

/// First relaunch delay after a failed launch or an early exit; doubled per failure up to
/// [`MAX_BACKOFF`].
pub const MIN_BACKOFF: Duration = Duration::from_secs(5);
pub const MAX_BACKOFF: Duration = Duration::from_secs(300);
/// How long a stopping engine gets to exit after its stdin closes before it is killed.
pub const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);
/// How long a killed engine gets to be reaped before its profile lock is released anyway.
pub const KILL_WAIT: Duration = Duration::from_secs(5);
/// An engine that exits sooner than this after its launch counts as a failed launch for the
/// relaunch backoff; one that ran longer resets the backoff.
pub const STABLE_UPTIME: Duration = Duration::from_secs(60);
/// Retry interval while another supervisor holds the profile lock.
const PROFILE_LOCK_POLL: Duration = Duration::from_millis(250);
/// Work-tool requests answered at once; further requests wait in the bounded work queue.
const MAX_WORK_IN_FLIGHT: usize = 8;
/// [`Inner::run_state`] once the engine is stopped with the hub. `0` is unsupervised use
/// (tests, a direct start); any other value is the token of the supervising generation.
const STOPPED: u64 = u64::MAX;
const MAX_TURN_TEXT_BYTES: usize = 1024 * 1024;
const MAX_TURN_EVENTS: usize = 100_000;
const DIALOG_METHODS: [&str; 6] = [
    "select",
    "confirm",
    "input",
    "editor",
    "question",
    "custom_unsupported",
];

/// Resolves when the owned engine process exits; `Some(true)` for a clean exit.
pub type ExitFuture = Pin<Box<dyn Future<Output = Option<bool>> + Send>>;

/// A started engine: its stdout and stdin, and the owned process identity when there is one.
/// `exited` resolves once the owned process is reaped; sending on (or dropping) `kill` kills
/// it first.
pub struct Launched {
    pub reader: BoxedReader,
    pub writer: BoxedWriter,
    pub pid: Option<u32>,
    pub exited: Option<ExitFuture>,
    pub kill: Option<oneshot::Sender<()>>,
}

/// Starts the engine. Production spawns the owned child; tests script a stdio peer.
#[async_trait]
pub trait EngineLauncher: Send + Sync {
    async fn launch(&self, profile: &MhcProfile) -> Result<Launched, SupervisorFault>;
}

/// Spawns `mhc --mode rpc --multi-session --no-model-fallback` with the hub child environment,
/// in the profile directory, with piped stdio and stderr in an owner-only log.
pub struct ProcessLauncher {
    search_path: Option<OsString>,
    pathext: Option<OsString>,
}

impl Default for ProcessLauncher {
    fn default() -> Self {
        Self {
            search_path: None,
            pathext: crate::child_env::platform_pathext(),
        }
    }
}

/// What [`ProcessLauncher`] runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
}

impl ProcessLauncher {
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolves the executable on `path` with `pathext` instead of the hub child PATH.
    pub fn with_search_path(path: OsString, pathext: Option<OsString>) -> Self {
        Self {
            search_path: Some(path),
            pathext,
        }
    }

    fn resolve(&self, program: &str) -> Option<PathBuf> {
        let pathext = self.pathext.as_deref();
        match &self.search_path {
            Some(path) => crate::child_env::which_with_pathext(program, path, pathext),
            None => crate::child_env::which_with_pathext(
                program,
                crate::child_env::child_path(),
                pathext,
            ),
        }
    }

    pub fn spec(&self, profile: &MhcProfile) -> Result<LaunchSpec, SupervisorFault> {
        let program = self
            .resolve(&profile.config.binary)
            .ok_or_else(|| SupervisorFault::missing_executable(&profile.config.binary))?;
        Ok(LaunchSpec {
            program,
            args: profile.launch_args(),
            cwd: profile.cwd().to_path_buf(),
        })
    }
}

#[async_trait]
impl EngineLauncher for ProcessLauncher {
    async fn launch(&self, profile: &MhcProfile) -> Result<Launched, SupervisorFault> {
        let spec = self.spec(profile)?;
        profile
            .prepare()
            .map_err(|error| SupervisorFault::launch(&format!("profile directory: {error}")))?;
        let mut command = crate::child_env::command(&spec.program);
        command
            .args(&spec.args)
            .current_dir(&spec.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true);
        match profile.open_log() {
            Some(log) => command.stderr(log),
            None => command.stderr(Stdio::null()),
        };
        let mut child = command.spawn().map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                SupervisorFault::missing_executable(&profile.config.binary)
            } else {
                SupervisorFault::launch(&error.to_string())
            }
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| SupervisorFault::launch("engine stdin was not piped"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| SupervisorFault::launch("engine stdout was not piped"))?;
        let pid = child.id();
        if let Some(pid) = pid {
            if let Err(error) = profile.record_pid(pid) {
                tracing::warn!(%error, pid, "mhc supervisor pid was not recorded");
            }
        }
        let (kill, killed) = oneshot::channel::<()>();
        let exited: ExitFuture = Box::pin(async move {
            let waited = tokio::select! {
                status = child.wait() => Some(status),
                _ = killed => None,
            };
            let status = match waited {
                Some(status) => status,
                // Asked to stop, or every handle on the engine is gone: kill, then reap.
                None => {
                    let _ = child.kill().await;
                    child.wait().await
                }
            };
            status.ok().map(|status| status.success())
        });
        Ok(Launched {
            reader: Box::new(stdout),
            writer: Box::new(stdin),
            pid,
            exited: Some(exited),
            kill: Some(kill),
        })
    }
}

/// What `get_protocol_info` reported. `server_version` is informational only and never
/// compared (development builds report a placeholder).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolInfo {
    pub protocol_version: u64,
    pub server_version: Option<String>,
    pub capabilities: Vec<String>,
}

pub fn parse_protocol_info(data: &Value) -> Result<ProtocolInfo, SupervisorFault> {
    let protocol_version = data
        .get("protocolVersion")
        .and_then(Value::as_u64)
        .filter(|version| *version >= 1)
        .ok_or_else(|| SupervisorFault::protocol("get_protocol_info has no protocolVersion >= 1"))?;
    if data.get("mode").and_then(Value::as_str) != Some("multi") {
        return Err(SupervisorFault::protocol(
            "the engine is not a multi-session host",
        ));
    }
    let capabilities = data
        .get("capabilities")
        .and_then(Value::as_array)
        .ok_or_else(|| SupervisorFault::protocol("get_protocol_info has no capabilities list"))?
        .iter()
        .map(|name| {
            name.as_str()
                .map(str::to_owned)
                .ok_or_else(|| SupervisorFault::protocol("a capability is not a string"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ProtocolInfo {
        protocol_version,
        server_version: data
            .get("serverVersion")
            .and_then(Value::as_str)
            .map(str::to_owned),
        capabilities,
    })
}

fn request_fault(operation: &str, error: RequestError) -> SupervisorFault {
    match error {
        RequestError::Gone(reason) => SupervisorFault::engine_exited(&reason),
        RequestError::Timeout => SupervisorFault::timeout(operation),
        RequestError::Refused {
            command,
            error,
            code,
        } => SupervisorFault::from_refusal(&command, &error, code.as_deref()),
    }
}

/// Runtime state of the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthState {
    Stopped,
    /// Another supervisor still owns the profile lock; the launch waits for it to end.
    Waiting,
    Ready,
    /// The last launch failed; [`SupervisorHealth::fault`] names why. No automatic action runs.
    Degraded,
    /// The engine exited; the next use relaunches it.
    Exited,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupervisorHealth {
    pub state: HealthState,
    /// Increments with every successful launch.
    pub generation: u64,
    pub pid: Option<u32>,
    pub capabilities: Vec<String>,
    pub server_version: Option<String>,
    pub fault: Option<SupervisorFault>,
    /// Redacted exit reason of the last engine.
    pub detail: Option<String>,
    /// Decisions waiting for a free decision slot.
    pub queued_decisions: usize,
}

impl SupervisorHealth {
    fn stopped() -> Self {
        Self {
            state: HealthState::Stopped,
            generation: 0,
            pid: None,
            capabilities: Vec::new(),
            server_version: None,
            fault: None,
            detail: None,
            queued_decisions: 0,
        }
    }
}

/// Worker kind a conversation asked for; the hub decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkerPreference {
    Mhc,
    Pty,
    Any,
}

/// One typed work-tool call. No field is a URL, command, path or free-form payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkToolCall {
    Submit {
        title: String,
        requirements: Vec<String>,
        worker: WorkerPreference,
    },
    Status {
        goal_id: Option<String>,
    },
}

/// A work-tool call with the scope the hub resolved for the calling conversation. Actor,
/// grant and source message are derived by the service from that scope, never from the call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkToolRequest {
    pub request_id: String,
    pub scope: ConversationScope,
    pub call: WorkToolCall,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkToolRejection {
    pub code: String,
    pub message: String,
}

impl WorkToolRejection {
    pub fn new(code: &str, message: &str) -> Self {
        Self {
            code: code.to_owned(),
            message: redact(message),
        }
    }
}

/// The supervisor service side of the typed work tools. The result is the persisted receipt
/// the conversation reports; execution happens only behind the hub's authorization boundary.
#[async_trait]
pub trait WorkToolHandler: Send + Sync {
    async fn handle(&self, request: WorkToolRequest) -> Result<Value, WorkToolRejection>;
}

/// Rejects every call until the supervisor service installs its handler.
pub struct UnavailableWorkTools;

#[async_trait]
impl WorkToolHandler for UnavailableWorkTools {
    async fn handle(&self, _request: WorkToolRequest) -> Result<Value, WorkToolRejection> {
        Err(WorkToolRejection::new(
            "SUPERVISOR_WORK_UNAVAILABLE",
            "the supervisor work service is not installed",
        ))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SubmitPayload {
    title: String,
    requirements: Vec<String>,
    #[serde(default)]
    worker: Option<WorkerPreference>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct StatusPayload {
    #[serde(default)]
    goal_id: Option<String>,
}

fn parse_work_call(data: &Value) -> Result<WorkToolCall, WorkToolRejection> {
    let payload = data.get("payload").cloned().unwrap_or(Value::Null);
    let invalid = |error: serde_json::Error| {
        WorkToolRejection::new("SUPERVISOR_WORK_INVALID", &error.to_string())
    };
    match data.get("action").and_then(Value::as_str) {
        Some("submit") => {
            let payload: SubmitPayload = serde_json::from_value(payload).map_err(invalid)?;
            Ok(WorkToolCall::Submit {
                title: payload.title,
                requirements: payload.requirements,
                worker: payload.worker.unwrap_or(WorkerPreference::Any),
            })
        }
        Some("status") => {
            let payload: StatusPayload = serde_json::from_value(payload).map_err(invalid)?;
            Ok(WorkToolCall::Status {
                goal_id: payload.goal_id,
            })
        }
        _ => Err(WorkToolRejection::new(
            "SUPERVISOR_WORK_UNKNOWN_ACTION",
            "the work request names no known action",
        )),
    }
}

/// What an owner ingest did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IngestOutcome {
    Learned { reply: String },
    /// The source message was already ingested; nothing was sent.
    Duplicate,
}

#[derive(Debug, Clone)]
struct OpenSession {
    handle: String,
    durable_id: String,
}

struct Engine {
    client: Arc<StdioClient>,
    generation: u64,
    pid: Option<u32>,
    info: ProtocolInfo,
    /// Live routing handles of durable roles.
    handles: Mutex<HashMap<String, OpenSession>>,
    /// True when there is an owned process to wait for and kill.
    owned: bool,
    /// Kills the owned process; its exit watcher reaps it.
    kill: Mutex<Option<oneshot::Sender<()>>>,
    /// Turns true when the owned process was reaped.
    exited: watch::Receiver<bool>,
    /// The cross-process profile lock, held until the owned process is reaped.
    lock: Mutex<Option<ProfileLock>>,
    launched_at: tokio::time::Instant,
}

impl Engine {
    fn handles(&self) -> MutexGuard<'_, HashMap<String, OpenSession>> {
        self.handles.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The live handle of a role, unless the engine closed it (for any reason).
    fn live_handle(&self, key: &str) -> Option<OpenSession> {
        let mut handles = self.handles();
        let open = handles.get(key)?.clone();
        if let Some(reason) = self.client.closed_reason(&open.handle) {
            tracing::info!(role = key, reason = %redact(&reason), "the engine closed a supervisor session; it resumes on next use");
            handles.remove(key);
            self.client.forget_closed(&open.handle);
            return None;
        }
        Some(open)
    }

    fn forget(&self, key: &str) {
        self.handles().remove(key);
    }

    /// Closes stdin so the engine's stdio host ends on its own, waits up to `grace` for the
    /// owned process to exit (or, with no owned process, for its stdout to close), then kills
    /// and reaps whatever is left (bounded by [`KILL_WAIT`]). The profile lock is released only
    /// after that. Calling it again is harmless.
    async fn terminate(&self, grace: Duration) {
        self.client.close();
        if tokio::time::timeout(grace, self.ended()).await.is_err() {
            tracing::warn!(generation = self.generation, pid = ?self.pid, "mhc supervisor engine did not exit after stdin closed; it is killed");
            self.kill();
            if tokio::time::timeout(KILL_WAIT, self.ended()).await.is_err() {
                tracing::error!(generation = self.generation, pid = ?self.pid, "mhc supervisor engine was not reaped after the kill; its profile lock is released anyway");
            }
        }
        drop(
            self.lock
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take(),
        );
    }

    /// Resolves once the owned process was reaped, or with none, once its stdout closed.
    async fn ended(&self) {
        if self.owned {
            let mut exited = self.exited.clone();
            let _ = exited.wait_for(|done| *done).await;
        } else {
            self.client.closed().await;
        }
    }

    fn kill(&self) {
        let kill = self
            .kill
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(kill) = kill {
            let _ = kill.send(());
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // The last handle went without a terminate: kill the owned process, and keep the
        // profile lock until it is reaped so no successor launches beside it.
        let kill = self
            .kill
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(kill) = kill {
            let _ = kill.send(());
        }
        let lock = self
            .lock
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let (true, Some(lock)) = (self.owned, lock) {
            let mut exited = self.exited.clone();
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                runtime.spawn(async move {
                    let _ = tokio::time::timeout(KILL_WAIT, exited.wait_for(|done| *done)).await;
                    drop(lock);
                });
            }
        }
    }
}

struct EngineState {
    current: Option<Arc<Engine>>,
    generation: u64,
    last_fault: Option<SupervisorFault>,
    retry_at: Option<tokio::time::Instant>,
    backoff: Duration,
}

/// Exclusive use of one role's context for one turn, released after any abort drain.
enum Lease {
    Slot {
        _permit: OwnedSemaphorePermit,
        slot: usize,
        inner: Arc<Inner>,
    },
    /// Held only so its drop releases the role's exclusive context.
    Exclusive { _guard: OwnedMutexGuard<()> },
}

impl Drop for Lease {
    fn drop(&mut self) {
        // Runs before the permit is released, so the next holder finds the slot free.
        if let Lease::Slot { slot, inner, .. } = self {
            inner.free_slots().push(*slot);
        }
    }
}

/// A turn that must be aborted, drained or closed after its caller got the outcome.
struct Cleanup {
    engine: Arc<Engine>,
    handle: String,
    events: mpsc::UnboundedReceiver<Value>,
    abort: bool,
    close: bool,
    /// The turn's outcome is unknown (a prompt that may still be accepted late, or one the
    /// engine queued or handled instead of starting): a durable role forgets its handle and
    /// the session is closed, so nothing of this turn can reach the role's next turn.
    reset: bool,
}

enum Step {
    More,
    Settled(Result<String, SupervisorFault>),
    Closed(String),
    Refuse(SupervisorFault),
}

#[derive(Default)]
struct TurnCollector {
    text: String,
    error: Option<String>,
    events: usize,
}

fn assistant_text(message: &Value) -> String {
    match message.get("content") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

impl TurnCollector {
    fn observe(&mut self, event: &Value) -> Step {
        self.events += 1;
        if self.events > MAX_TURN_EVENTS {
            return Step::Refuse(SupervisorFault::protocol("the turn exceeded the event limit"));
        }
        match event.get("type").and_then(Value::as_str) {
            Some("message_end") => {
                let Some(message) = event.get("message") else {
                    return Step::More;
                };
                if message.get("role").and_then(Value::as_str) != Some("assistant") {
                    return Step::More;
                }
                match message.get("stopReason").and_then(Value::as_str) {
                    Some("error" | "aborted") => {
                        self.error = Some(
                            message
                                .get("errorMessage")
                                .and_then(Value::as_str)
                                .unwrap_or("the engine ended the turn with an error")
                                .to_owned(),
                        );
                    }
                    _ => {
                        let text = assistant_text(message);
                        if text.len() > MAX_TURN_TEXT_BYTES {
                            return Step::Refuse(SupervisorFault::protocol(
                                "the assistant text exceeds the size limit",
                            ));
                        }
                        self.text = text;
                        self.error = None;
                    }
                }
                Step::More
            }
            // The engine substituted a model: the plan forbids any fallback, so the answer is
            // discarded rather than attributed to the configured model.
            Some("model_changed")
                if event.get("source").and_then(Value::as_str) == Some("fallback") =>
            {
                Step::Refuse(SupervisorFault::provider(
                    "the engine fell back to another model during a supervisor turn; the turn was discarded",
                ))
            }
            Some("agent_settled") => Step::Settled(match self.error.take() {
                Some(error) => Err(SupervisorFault::from_turn_error(&error)),
                None => Ok(std::mem::take(&mut self.text)),
            }),
            Some("session_closed") => Step::Closed(
                event
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or("unspecified")
                    .to_owned(),
            ),
            _ => Step::More,
        }
    }
}

/// A dialog a supervisor session raised is cancelled: nobody answers it, and no decision may
/// wait on a person.
fn ui_cancellation(event: &Value) -> Option<Value> {
    if event.get("type").and_then(Value::as_str) != Some("extension_ui_request") {
        return None;
    }
    let method = event.get("method").and_then(Value::as_str)?;
    if !DIALOG_METHODS.contains(&method) {
        return None;
    }
    let id = event.get("id")?;
    Some(json!({"type": "extension_ui_response", "id": id, "cancelled": true}))
}

/// Resolves when the cancel epoch moves past `epoch`; never when `enabled` is false.
async fn cancel_signal(cancel: &mut watch::Receiver<u64>, epoch: u64, enabled: bool) {
    if !enabled {
        return std::future::pending().await;
    }
    loop {
        if *cancel.borrow_and_update() != epoch {
            return;
        }
        if cancel.changed().await.is_err() {
            return std::future::pending().await;
        }
    }
}

fn cleanup(
    engine: Arc<Engine>,
    handle: String,
    events: mpsc::UnboundedReceiver<Value>,
    abort: bool,
    close: bool,
    reset: bool,
) -> Option<Cleanup> {
    (abort || close || reset).then_some(Cleanup {
        engine,
        handle,
        events,
        abort,
        close,
        reset,
    })
}

/// mhc runs a prompt that starts with `/` as a slash command (`/model` would switch the
/// supervisor's model) instead of a turn; a leading space keeps such text an ordinary turn.
/// A prompt that trims to a bare `.` is mhc's manual-continue shortcut ("resume the most
/// recent intent") on any session with history. mhc trims before comparing, so a space does
/// not escape it; the dot is sent quoted instead.
fn escape_command(message: &str) -> Cow<'_, str> {
    if message.trim() == "." {
        Cow::Borrowed("\".\"")
    } else if message.starts_with('/') {
        Cow::Owned(format!(" {message}"))
    } else {
        Cow::Borrowed(message)
    }
}

struct Inner {
    profile: MhcProfile,
    launcher: Arc<dyn EngineLauncher>,
    store: Mutex<SessionStore>,
    engine: tokio::sync::Mutex<EngineState>,
    health: watch::Sender<SupervisorHealth>,
    slots: Arc<Semaphore>,
    free_slots: Mutex<Vec<usize>>,
    /// Held across a whole owner ingest (dedupe check, turn, record).
    ingest_order: tokio::sync::Mutex<()>,
    /// Held by the ingest turn until any abort drain is done.
    ingest_session: Arc<tokio::sync::Mutex<()>>,
    conversations: Mutex<HashMap<String, ConversationEntry>>,
    cancel: watch::Sender<u64>,
    work: RwLock<Arc<dyn WorkToolHandler>>,
    /// Decisions waiting for a slot; mirrored in [`SupervisorHealth::queued_decisions`].
    queued: AtomicUsize,
    /// `0` unsupervised, [`STOPPED`] after teardown, otherwise the supervising token.
    run_state: AtomicU64,
    next_token: AtomicU64,
}

/// One conversation's scope and the lock serialising its turns.
type ConversationEntry = (ConversationScope, Arc<tokio::sync::Mutex<()>>);

type TurnResult = Result<String, SupervisorFault>;

/// Counts one decision as queued for a slot until dropped.
struct Queued<'a> {
    inner: &'a Inner,
}

impl<'a> Queued<'a> {
    fn enter(inner: &'a Inner) -> Self {
        let queued = inner.queued.fetch_add(1, Ordering::SeqCst) + 1;
        inner
            .health
            .send_modify(|health| health.queued_decisions = queued);
        Self { inner }
    }
}

impl Drop for Queued<'_> {
    fn drop(&mut self) {
        let queued = self.inner.queued.fetch_sub(1, Ordering::SeqCst) - 1;
        self.inner
            .health
            .send_modify(|health| health.queued_decisions = queued);
    }
}

impl Inner {
    fn store(&self) -> MutexGuard<'_, SessionStore> {
        self.store.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn free_slots(&self) -> MutexGuard<'_, Vec<usize>> {
        self.free_slots.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn conversations(&self) -> MutexGuard<'_, HashMap<String, ConversationEntry>> {
        self.conversations.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn work_handler(&self) -> Arc<dyn WorkToolHandler> {
        self.work
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn is_stopped(&self) -> bool {
        self.run_state.load(Ordering::SeqCst) == STOPPED
    }

    /// Ends the current engine if the runtime is still stopped once the engine lock is held
    /// (a newer generation may have taken over meanwhile; then nothing happens).
    async fn stop_engine(&self) {
        let mut state = self.engine.lock().await;
        if !self.is_stopped() {
            return;
        }
        state.retry_at = None;
        state.last_fault = None;
        state.backoff = MIN_BACKOFF;
        if let Some(engine) = state.current.take() {
            engine.terminate(SHUTDOWN_GRACE).await;
            self.note_exit(&engine);
        }
        // Still under the engine lock: a successor launch cannot be overwritten as stopped.
        self.health.send_modify(|health| {
            health.state = HealthState::Stopped;
            health.pid = None;
            health.fault = None;
            health.detail = Some("stopped with the hub".to_owned());
        });
        drop(state);
        tracing::info!("mhc supervisor engine stopped with the hub");
    }

    /// The lowest free decision slot. A permit guarantees one is free.
    fn take_slot(&self) -> usize {
        let mut free = self.free_slots();
        free.sort_unstable_by(|a, b| b.cmp(a));
        free.pop().unwrap_or_else(|| {
            tracing::error!("a decision slot permit was held with no free slot");
            0
        })
    }

    /// The current engine, launching it when there is none. Within the backoff window after a
    /// failed launch or an early exit the last fault is returned without another launch.
    async fn engine(self: &Arc<Self>) -> Result<Arc<Engine>, SupervisorFault> {
        let mut state = self.engine.lock().await;
        if self.is_stopped() {
            if let Some(engine) = state.current.take() {
                engine.terminate(SHUTDOWN_GRACE).await;
                self.note_exit(&engine);
            }
            return Err(SupervisorFault::not_running(
                "the supervisor engine stopped with the hub",
            ));
        }
        if let Some(engine) = state.current.clone() {
            if !engine.client.is_gone() {
                return Ok(engine);
            }
            state.current = None;
            engine.terminate(SHUTDOWN_GRACE).await;
            self.record_exit(&mut state, &engine);
        }
        if let (Some(retry_at), Some(fault)) = (state.retry_at, state.last_fault.clone()) {
            if tokio::time::Instant::now() < retry_at {
                return Err(fault);
            }
        }
        let generation = state.generation + 1;
        match self.launch(generation).await {
            Ok(engine) => {
                state.generation = generation;
                state.current = Some(engine.clone());
                state.last_fault = None;
                state.retry_at = None;
                // The backoff resets once this engine has run for STABLE_UPTIME (record_exit),
                // so an engine that keeps exiting right after its launch still backs off.
                tracing::info!(generation, pid = ?engine.pid, server_version = ?engine.info.server_version, "mhc supervisor engine is ready");
                self.health.send_replace(SupervisorHealth {
                    state: HealthState::Ready,
                    generation,
                    pid: engine.pid,
                    capabilities: engine.info.capabilities.clone(),
                    server_version: engine.info.server_version.clone(),
                    fault: None,
                    detail: None,
                    queued_decisions: 0,
                });
                self.health.send_modify(|health| {
                    health.queued_decisions = self.queued.load(Ordering::SeqCst);
                });
                Ok(engine)
            }
            Err(fault) => {
                if self.is_stopped() {
                    // Stopped while waiting for the profile lock: nothing launched, and the
                    // stop owns the health state, so no backoff or degraded state is recorded.
                    return Err(fault);
                }
                tracing::warn!(%fault, "mhc supervisor engine is degraded; no automatic action runs");
                state.last_fault = Some(fault.clone());
                state.retry_at = Some(tokio::time::Instant::now() + state.backoff);
                state.backoff = (state.backoff * 2).min(MAX_BACKOFF);
                self.health.send_modify(|health| {
                    health.state = HealthState::Degraded;
                    health.pid = None;
                    health.fault = Some(fault.clone());
                });
                Err(fault)
            }
        }
    }

    async fn launch(self: &Arc<Self>, generation: u64) -> Result<Arc<Engine>, SupervisorFault> {
        let lock = self.acquire_profile_lock().await?;
        // The lock wait can last SHUTDOWN_GRACE + drain + KILL_WAIT; a runtime stopped
        // meanwhile launches nothing.
        if self.is_stopped() {
            drop(lock);
            return Err(SupervisorFault::not_running(
                "the supervisor engine stopped with the hub",
            ));
        }
        // Another supervisor process may have written the store while it owned the profile.
        self.store().reload();
        let Launched {
            reader,
            writer,
            pid,
            exited,
            kill,
        } = self.launcher.launch(&self.profile).await?;
        let owned = exited.is_some();
        let (exit_tx, exit_rx) = watch::channel(false);
        if let Some(exited) = exited {
            tokio::spawn(async move {
                let status = exited.await;
                tracing::info!(?status, "mhc supervisor engine process was reaped");
                exit_tx.send_replace(true);
            });
        }
        let (client, work) =
            StdioClient::start(reader, writer, self.profile.config.timeouts.max_line_bytes);
        let checked = handshake(&client, &self.profile).await;
        let mut engine = Engine {
            client,
            generation,
            pid,
            info: ProtocolInfo {
                protocol_version: 0,
                server_version: None,
                capabilities: Vec::new(),
            },
            handles: Mutex::new(HashMap::new()),
            owned,
            kill: Mutex::new(kill),
            exited: exit_rx,
            lock: Mutex::new(Some(lock)),
            launched_at: tokio::time::Instant::now(),
        };
        match checked {
            Ok(info) => engine.info = info,
            Err(fault) => {
                // Nothing was sent past the handshake; the process is ended and reaped before
                // the profile lock is released.
                engine.terminate(SHUTDOWN_GRACE).await;
                if let Some(pid) = pid {
                    self.profile.forget_pid(pid);
                }
                return Err(fault);
            }
        }
        let engine = Arc::new(engine);
        tokio::spawn(serve_work(
            Arc::downgrade(self),
            Arc::downgrade(&engine),
            work,
        ));
        Ok(engine)
    }

    /// Takes the cross-process profile lock, waiting while another supervisor (a handover's
    /// retiring daemon) still owns the engine profile: at least [`SHUTDOWN_GRACE`] plus the
    /// abort drain plus [`KILL_WAIT`]. Past that the launch fails as
    /// [`SupervisorFault::ProfileBusy`] with nothing launched or sent.
    async fn acquire_profile_lock(&self) -> Result<ProfileLock, SupervisorFault> {
        let wait = SHUTDOWN_GRACE + self.profile.config.timeouts.drain + KILL_WAIT;
        let deadline = tokio::time::Instant::now() + wait;
        let mut waiting = false;
        loop {
            match self.profile.try_lock() {
                Ok(Some(lock)) => return Ok(lock),
                Ok(None) => {}
                Err(error) => {
                    return Err(SupervisorFault::launch(&format!(
                        "the profile lock {} could not be taken: {error}",
                        self.profile.lock_path().display()
                    )))
                }
            }
            let now = tokio::time::Instant::now();
            if now >= deadline {
                return Err(SupervisorFault::profile_busy(&format!(
                    "another supervisor still owns {} after {}s",
                    self.profile.lock_path().display(),
                    wait.as_secs()
                )));
            }
            if !waiting {
                waiting = true;
                tracing::info!(lock = %self.profile.lock_path().display(), "another supervisor owns the engine profile; the launch waits for it to end");
                self.health.send_modify(|health| {
                    health.state = HealthState::Waiting;
                    health.pid = None;
                });
            }
            tokio::time::sleep(PROFILE_LOCK_POLL.min(deadline.saturating_duration_since(now)))
                .await;
        }
    }

    fn note_exit(&self, engine: &Engine) {
        let reason = engine
            .client
            .gone_reason()
            .unwrap_or_else(|| "engine transport ended".to_owned());
        tracing::warn!(generation = engine.generation, pid = ?engine.pid, reason = %redact(&reason), "mhc supervisor engine exited; in-flight turns are not resent");
        if let Some(pid) = engine.pid {
            self.profile.forget_pid(pid);
        }
        self.health.send_modify(|health| {
            if health.generation == engine.generation && health.state == HealthState::Ready {
                health.state = HealthState::Exited;
                health.pid = None;
                health.detail = Some(redact(&reason));
            }
        });
    }

    async fn engine_gone(&self, engine: &Arc<Engine>) {
        let mut state = self.engine.lock().await;
        if state
            .current
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, engine))
        {
            state.current = None;
            engine.terminate(SHUTDOWN_GRACE).await;
            self.record_exit(&mut state, engine);
        }
    }

    /// Notes an engine that exited on its own. One that exited within [`STABLE_UPTIME`] of its
    /// launch counts as a failed launch: the next launch waits out the backoff, which doubles.
    /// One that ran longer resets the backoff.
    fn record_exit(&self, state: &mut EngineState, engine: &Engine) {
        self.note_exit(engine);
        let uptime = engine.launched_at.elapsed();
        if uptime >= STABLE_UPTIME {
            state.backoff = MIN_BACKOFF;
            return;
        }
        let reason = engine
            .client
            .gone_reason()
            .unwrap_or_else(|| "engine transport ended".to_owned());
        let fault = SupervisorFault::engine_exited(&format!(
            "the engine exited {}s after its launch: {reason}",
            uptime.as_secs()
        ));
        tracing::warn!(%fault, backoff_secs = state.backoff.as_secs(), "mhc supervisor engine exited early; the relaunch backs off");
        state.last_fault = Some(fault);
        state.retry_at = Some(tokio::time::Instant::now() + state.backoff);
        state.backoff = (state.backoff * 2).min(MAX_BACKOFF);
    }

    async fn retry_delay(&self) -> Duration {
        let state = self.engine.lock().await;
        state
            .retry_at
            .map(|at| at.saturating_duration_since(tokio::time::Instant::now()))
            .unwrap_or(MIN_BACKOFF)
            .max(Duration::from_secs(1))
    }

    /// The routing handle of a role's session: the live one, a resumed persisted one, or a new
    /// one. A resumed session must return the persisted identity or it is closed and refused.
    async fn session(&self, engine: &Engine, spec: &RoleSpec) -> Result<OpenSession, SupervisorFault> {
        // Checked before any command: a role the engine cannot scope opens nothing.
        let missing = self
            .profile
            .missing_role_capabilities(spec, &engine.info.capabilities);
        if !missing.is_empty() {
            return Err(SupervisorFault::Capability { missing });
        }
        if spec.durable {
            if let Some(open) = engine.live_handle(&spec.key) {
                return Ok(open);
            }
        }
        let persisted = if spec.durable {
            self.store().get(&spec.key).cloned()
        } else {
            None
        };
        let durable_id = persisted
            .as_ref()
            .map(|session| session.durable_session_id.clone())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let resume_path = persisted
            .as_ref()
            .and_then(|session| session.session_file.clone())
            .filter(|file| {
                let present = Path::new(file).is_file();
                if !present {
                    tracing::warn!(role = %spec.key, "the persisted supervisor session file is gone; the identity reopens without history");
                }
                present
            });
        let command = self.profile.open_session_command(
            spec,
            &durable_id,
            resume_path.as_deref(),
        );
        let data = engine
            .client
            .request(command, self.profile.config.timeouts.request)
            .await
            .map_err(|error| request_fault("open_session", error))?;
        let handle = data
            .get("sessionId")
            .and_then(Value::as_str)
            .filter(|handle| !handle.is_empty())
            .ok_or_else(|| SupervisorFault::protocol("open_session reply has no sessionId"))?
            .to_owned();
        let state = data.get("state").cloned().unwrap_or(Value::Null);
        let returned = state
            .get("sessionId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        if returned != durable_id {
            engine.client.close_session(&handle);
            tracing::warn!(role = %spec.key, "a resumed supervisor session returned another identity; it was refused");
            return Err(SupervisorFault::SessionMismatch {
                role: spec.key.clone(),
                expected: durable_id,
                actual: redact(&returned),
            });
        }
        let open = OpenSession {
            handle,
            durable_id,
        };
        if spec.durable {
            let session_file = state
                .get("sessionFile")
                .and_then(Value::as_str)
                .filter(|file| Path::new(file).is_absolute())
                .map(str::to_owned);
            let persisted = PersistedSession {
                durable_session_id: open.durable_id.clone(),
                session_file,
                role: spec.role.to_owned(),
            };
            if let Err(error) = self.store().set(&spec.key, persisted) {
                tracing::warn!(%error, role = %spec.key, "the supervisor session identity was not persisted");
            }
            engine.handles().insert(spec.key.clone(), open.clone());
        }
        Ok(open)
    }

    /// Runs one turn in its own task, which owns the lease until any abort drain is done.
    /// The caller gets the outcome as soon as it is known; dropping the caller aborts the turn.
    /// A turn that needs no cleanup releases its lease before the outcome is delivered, so the
    /// caller's next turn finds the same context free.
    async fn run(
        self: Arc<Self>,
        role: SessionRole,
        message: String,
        lease: Lease,
        cancel: watch::Receiver<u64>,
        epoch: u64,
    ) -> TurnResult {
        let spec = self.profile.role_spec(&role);
        let (sender, receiver) = oneshot::channel();
        tokio::spawn(async move {
            let mut sender = sender;
            let mut cancel = cancel;
            let (outcome, after) = self
                .execute(&spec, &message, &mut cancel, epoch, &mut sender)
                .await;
            match after {
                None => {
                    drop(lease);
                    let _ = sender.send(outcome);
                }
                Some(after) => {
                    let _ = sender.send(outcome);
                    self.finish(&spec, after).await;
                    drop(lease);
                }
            }
        });
        receiver.await.unwrap_or(Err(SupervisorFault::Cancelled))
    }

    async fn execute(
        self: &Arc<Self>,
        spec: &RoleSpec,
        message: &str,
        cancel: &mut watch::Receiver<u64>,
        epoch: u64,
        caller: &mut oneshot::Sender<TurnResult>,
    ) -> (TurnResult, Option<Cleanup>) {
        if spec.cancellable && *cancel.borrow() != epoch {
            return (Err(SupervisorFault::Cancelled), None);
        }
        let engine = tokio::select! {
            engine = self.engine() => match engine {
                Ok(engine) => engine,
                Err(fault) => return (Err(fault), None),
            },
            () = cancel_signal(cancel, epoch, spec.cancellable) => return (Err(SupervisorFault::Cancelled), None),
            () = caller.closed() => return (Err(SupervisorFault::Cancelled), None),
        };
        let session = match self.session(&engine, spec).await {
            Ok(session) => session,
            Err(fault) => return (Err(fault), None),
        };
        let handle = session.handle;
        let close = !spec.durable;
        // A durable role whose turn outcome is unknown reopens with a fresh handle.
        let reset = spec.durable;
        let mut events = match engine.client.subscribe(&handle) {
            Ok(events) => events,
            Err(error) => return (Err(request_fault("subscribe", error)), None),
        };
        let client = engine.client.clone();
        let prompt = client.request(
            json!({"type": "prompt", "sessionId": handle, "message": escape_command(message)}),
            self.profile.config.timeouts.request,
        );
        let accepted = tokio::select! {
            accepted = prompt => accepted,
            () = cancel_signal(cancel, epoch, spec.cancellable) => {
                return (Err(SupervisorFault::Cancelled), cleanup(engine, handle, events, true, close, reset));
            }
            () = caller.closed() => {
                return (Err(SupervisorFault::Cancelled), cleanup(engine, handle, events, true, close, reset));
            }
        };
        let data = match accepted {
            Ok(data) => data,
            // No answer in time: the engine may still accept the prompt later, so the outcome
            // is unknown. The turn is aborted and drained, and the role reopens clean.
            Err(RequestError::Timeout) => {
                return (
                    Err(SupervisorFault::timeout("prompt")),
                    cleanup(engine, handle, events, true, close, reset),
                );
            }
            Err(error) => {
                let unknown = matches!(
                    &error,
                    RequestError::Refused { code: Some(code), .. }
                        if code == "unknown_session" || code == "session_closing"
                );
                if unknown {
                    engine.forget(&spec.key);
                }
                engine.client.unsubscribe(&handle);
                return (
                    Err(request_fault("prompt", error)),
                    cleanup(engine, handle, events, false, close, false),
                );
            }
        };
        // Only a started turn produces this prompt's answer. A queued prompt would collect an
        // earlier turn's events; a handled one (a command) runs no turn at all.
        let disposition = data
            .get("disposition")
            .and_then(Value::as_str)
            .unwrap_or("unreported");
        if disposition != "started" {
            let fault = SupervisorFault::protocol(&format!(
                "the engine reported the prompt as {disposition}, not started; it was aborted"
            ));
            return (
                Err(fault),
                cleanup(engine, handle, events, true, close, reset),
            );
        }
        let deadline = tokio::time::Instant::now() + self.profile.config.timeouts.turn;
        let mut collector = TurnCollector::default();
        loop {
            tokio::select! {
                () = cancel_signal(cancel, epoch, spec.cancellable) => {
                    return (Err(SupervisorFault::Cancelled), cleanup(engine, handle, events, true, close, false));
                }
                () = caller.closed() => {
                    return (Err(SupervisorFault::Cancelled), cleanup(engine, handle, events, true, close, false));
                }
                () = tokio::time::sleep_until(deadline) => {
                    return (Err(SupervisorFault::timeout("supervisor turn")), cleanup(engine, handle, events, true, close, false));
                }
                event = events.recv() => {
                    let Some(event) = event else {
                        let reason = engine.client.gone_reason().unwrap_or_else(|| "engine transport ended".to_owned());
                        return (Err(SupervisorFault::engine_exited(&reason)), None);
                    };
                    if let Some(reply) = ui_cancellation(&event) {
                        let _ = engine.client.send(&reply);
                    }
                    match collector.observe(&event) {
                        Step::More => {}
                        Step::Settled(result) => {
                            engine.client.unsubscribe(&handle);
                            return (result, cleanup(engine, handle, events, false, close, false));
                        }
                        Step::Closed(reason) => {
                            engine.forget(&spec.key);
                            engine.client.forget_closed(&handle);
                            engine.client.unsubscribe(&handle);
                            return (Err(SupervisorFault::session_closed(&reason)), None);
                        }
                        Step::Refuse(fault) => {
                            return (Err(fault), cleanup(engine, handle, events, true, close, false));
                        }
                    }
                }
            }
        }
    }

    /// Aborts and drains a turn its caller no longer waits for, then closes a one-shot context.
    /// A context that does not settle in time, or whose turn outcome is unknown (`reset`), is
    /// dropped and closed, so its next use resumes cleanly with a fresh handle.
    async fn finish(&self, spec: &RoleSpec, after: Cleanup) {
        let Cleanup {
            engine,
            handle,
            mut events,
            abort,
            close,
            reset,
        } = after;
        let settled = if abort {
            let _ = engine
                .client
                .send(&json!({"type": "abort", "sessionId": handle}));
            tokio::time::timeout(self.profile.config.timeouts.drain, async {
                while let Some(event) = events.recv().await {
                    if let Some(reply) = ui_cancellation(&event) {
                        let _ = engine.client.send(&reply);
                    }
                    match event.get("type").and_then(Value::as_str) {
                        Some("agent_settled") => return true,
                        Some("session_closed") => return false,
                        _ => {}
                    }
                }
                false
            })
            .await
            .unwrap_or(false)
        } else {
            true
        };
        if (reset || !settled) && !close {
            engine.forget(&spec.key);
            engine.client.close_session(&handle);
        }
        engine.client.unsubscribe(&handle);
        if close {
            engine.client.close_session(&handle);
        }
    }

    /// Answers one typed work request. The scope comes from the hub's own record of which
    /// conversation owns the emitting session; a request from any other session is rejected.
    async fn answer_work(&self, engine: &Engine, record: Value) {
        let data = record.get("data").cloned().unwrap_or(Value::Null);
        let Some(request_id) = data.get("requestId").and_then(Value::as_str).map(str::to_owned)
        else {
            tracing::warn!("a supervisor work request without a requestId was ignored");
            return;
        };
        let durable = data
            .get("sessionId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let routed = record.get("sessionId").and_then(Value::as_str).map(str::to_owned);
        let owner = {
            let handles = engine.handles();
            handles
                .iter()
                .find(|(key, open)| key.starts_with("conversation-") && open.durable_id == durable)
                .map(|(key, open)| (key.clone(), open.handle.clone()))
        };
        let scope = owner.as_ref().and_then(|(key, handle)| {
            if routed.as_deref().is_some_and(|routed| routed != handle) {
                return None;
            }
            self.conversations().get(key).map(|(scope, _)| scope.clone())
        });
        let reply = match scope {
            None => Err(WorkToolRejection::new(
                "SUPERVISOR_WORK_SCOPE",
                "the request did not come from a supervisor conversation",
            )),
            Some(scope) => match parse_work_call(&data) {
                Err(rejection) => Err(rejection),
                Ok(call) => {
                    self.work_handler()
                        .handle(WorkToolRequest {
                            request_id: request_id.clone(),
                            scope,
                            call,
                        })
                        .await
                }
            },
        };
        let Some(handle) = routed.or_else(|| owner.map(|(_, handle)| handle)) else {
            tracing::warn!("a supervisor work request names no session to answer");
            return;
        };
        let data = match reply {
            Ok(result) => json!({"requestId": request_id, "ok": true, "result": result}),
            Err(rejection) => json!({
                "requestId": request_id,
                "ok": false,
                "error": {"code": rejection.code, "message": rejection.message},
            }),
        };
        let command = json!({
            "type": "extension_request",
            "sessionId": handle,
            "name": WORK_REPLY_REQUEST,
            "data": data,
        });
        if let Err(error) = engine
            .client
            .request(command, self.profile.config.timeouts.request)
            .await
        {
            tracing::warn!(fault = %request_fault("extension_request", error), "a supervisor work reply was not delivered");
        }
    }
}

async fn handshake(client: &StdioClient, profile: &MhcProfile) -> Result<ProtocolInfo, SupervisorFault> {
    let timeout = profile.config.timeouts.request;
    let data = client
        .request(json!({"type": "get_protocol_info"}), timeout)
        .await
        .map_err(|error| request_fault("get_protocol_info", error))?;
    let info = parse_protocol_info(&data)?;
    let missing = profile.missing_capabilities(&info.capabilities);
    if !missing.is_empty() {
        return Err(SupervisorFault::Capability { missing });
    }
    client
        .request(
            json!({"type": "set_client_info", "width": 120, "capabilities": CLIENT_CAPABILITIES}),
            timeout,
        )
        .await
        .map_err(|error| request_fault("set_client_info", error))?;
    Ok(info)
}

/// Answers work requests, at most [`MAX_WORK_IN_FLIGHT`] at once; the rest wait in the bounded
/// work queue.
async fn serve_work(inner: Weak<Inner>, engine: Weak<Engine>, mut requests: mpsc::Receiver<Value>) {
    let limit = Arc::new(Semaphore::new(MAX_WORK_IN_FLIGHT));
    while let Some(record) = requests.recv().await {
        let Ok(permit) = limit.clone().acquire_owned().await else {
            return;
        };
        let (Some(inner), Some(engine)) = (inner.upgrade(), engine.upgrade()) else {
            return;
        };
        tokio::spawn(async move {
            inner.answer_work(&engine, record).await;
            drop(permit);
        });
    }
}

/// The retained supervisor engine and its role-scoped sessions.
#[derive(Clone)]
pub struct MhcSupervisor {
    inner: Arc<Inner>,
}

impl MhcSupervisor {
    pub fn new(profile: MhcProfile, launcher: Arc<dyn EngineLauncher>) -> Self {
        let store = SessionStore::load(profile.sessions_path());
        let (health, _) = watch::channel(SupervisorHealth::stopped());
        let (cancel, _) = watch::channel(0u64);
        Self {
            inner: Arc::new(Inner {
                profile,
                launcher,
                store: Mutex::new(store),
                engine: tokio::sync::Mutex::new(EngineState {
                    current: None,
                    generation: 0,
                    last_fault: None,
                    retry_at: None,
                    backoff: MIN_BACKOFF,
                }),
                health,
                slots: Arc::new(Semaphore::new(DECISION_SLOTS)),
                free_slots: Mutex::new((0..DECISION_SLOTS).collect()),
                ingest_order: tokio::sync::Mutex::new(()),
                ingest_session: Arc::new(tokio::sync::Mutex::new(())),
                conversations: Mutex::new(HashMap::new()),
                cancel,
                work: RwLock::new(Arc::new(UnavailableWorkTools)),
                queued: AtomicUsize::new(0),
                run_state: AtomicU64::new(0),
                next_token: AtomicU64::new(1),
            }),
        }
    }

    pub fn profile(&self) -> &MhcProfile {
        &self.inner.profile
    }

    pub fn health(&self) -> SupervisorHealth {
        self.inner.health.borrow().clone()
    }

    pub fn subscribe_health(&self) -> watch::Receiver<SupervisorHealth> {
        self.inner.health.subscribe()
    }

    /// Read-only view of the engine for operators and QA: health, the profile cwd, the
    /// persisted role bindings (durable session id and session file per role key) and the
    /// session context a decision slot opens with, and the native memory identity the engine
    /// reads from the profile's `.omo/omo.jsonc` (`memoryAgent`, null when the file is absent
    /// or names none). Sends nothing to the engine, never launches it and writes no file.
    /// It waits for the engine state lock, so it never shows half of a launch or
    /// exit: every transition the engine logs under that lock (ready, degraded, exited) is
    /// visible to a snapshot taken after the log line. `retryInMs` is how long until a
    /// relaunch is allowed after a failed launch or an early exit.
    pub async fn engine_snapshot(&self) -> Value {
        let (retry_in_ms, health) = {
            let state = self.inner.engine.lock().await;
            let retry = state.retry_at.map(|at| {
                u64::try_from(
                    at.saturating_duration_since(tokio::time::Instant::now())
                        .as_millis(),
                )
                .unwrap_or(u64::MAX)
            });
            (retry, self.health())
        };
        let sessions = self.inner.store().sessions().clone();
        let decision = self.inner.profile.role_spec(&SessionRole::Decision(0));
        let memory_agent = tokio::fs::read_to_string(self.inner.profile.memory_config_path())
            .await
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .and_then(|config| config.pointer("/memory/agent").cloned())
            .unwrap_or(Value::Null);
        json!({
            "state": health.state,
            "generation": health.generation,
            "pid": health.pid,
            "capabilities": health.capabilities,
            "serverVersion": health.server_version,
            "faultCode": health.fault.as_ref().map(SupervisorFault::code),
            "fault": health.fault,
            "detail": health.detail,
            "queuedDecisions": health.queued_decisions,
            "retryInMs": retry_in_ms,
            "profileCwd": self.inner.profile.cwd().to_string_lossy(),
            "sessions": sessions,
            "decisionContext": decision.context,
            "memoryAgent": memory_agent,
        })
    }

    /// Malformed engine lines the current transport skipped.
    pub async fn malformed_lines(&self) -> u64 {
        let state = self.inner.engine.lock().await;
        state
            .current
            .as_ref()
            .map_or(0, |engine| engine.client.malformed_lines())
    }

    /// Installs the supervisor service's work-tool handler (todo 7 provides it).
    pub fn set_work_handler(&self, handler: Arc<dyn WorkToolHandler>) {
        *self
            .inner
            .work
            .write()
            .unwrap_or_else(PoisonError::into_inner) = handler;
    }

    /// Launches and checks the engine now instead of on first use.
    pub async fn start(&self) -> Result<(), SupervisorFault> {
        self.inner.engine().await.map(|_| ())
    }

    async fn decision_turn(&self, prompt: &str, fresh: bool) -> TurnResult {
        let inner = &self.inner;
        let mut cancel = inner.cancel.subscribe();
        let epoch = *cancel.borrow_and_update();
        let permit = match inner.slots.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => {
                let queued = Queued::enter(inner);
                let permit = tokio::select! {
                    permit = inner.slots.clone().acquire_owned() => permit
                        .map_err(|_| SupervisorFault::engine_exited("decision slots are closed"))?,
                    () = cancel_signal(&mut cancel, epoch, true) => return Err(SupervisorFault::Cancelled),
                };
                drop(queued);
                permit
            }
        };
        let slot = inner.take_slot();
        let role = if fresh {
            SessionRole::Convergence
        } else {
            SessionRole::Decision(slot)
        };
        let lease = Lease::Slot {
            _permit: permit,
            slot,
            inner: inner.clone(),
        };
        inner
            .clone()
            .run(role, prompt.to_owned(), lease, cancel, epoch)
            .await
    }

    /// One judgment in a retained decision slot: memory writes denied, no effect tools.
    pub async fn decide(&self, prompt: &str) -> TurnResult {
        self.decision_turn(prompt, false).await
    }

    /// One judgment in a fresh context that sees only `prompt` (contract and attested evidence),
    /// never earlier assessments. The context is closed afterwards.
    pub async fn converge(&self, prompt: &str) -> TurnResult {
        self.decision_turn(prompt, true).await
    }

    /// One turn in the retained conversation of `scope`. The conversation may call the typed
    /// work tools; its memory writes are denied.
    pub async fn converse(&self, scope: &ConversationScope, message: &str) -> TurnResult {
        let key = self
            .inner
            .profile
            .role_spec(&SessionRole::Conversation(scope.clone()))
            .key;
        let lock = {
            let mut conversations = self.inner.conversations();
            let entry = conversations
                .entry(key)
                .or_insert_with(|| (scope.clone(), Arc::new(tokio::sync::Mutex::new(()))));
            if entry.0 != *scope {
                return Err(SupervisorFault::protocol(
                    "two conversation scopes share one key; the conversation was refused",
                ));
            }
            entry.1.clone()
        };
        let guard = lock.lock_owned().await;
        let cancel = self.inner.cancel.subscribe();
        let epoch = *cancel.borrow();
        self.inner
            .clone()
            .run(
                SessionRole::Conversation(scope.clone()),
                message.to_owned(),
                Lease::Exclusive { _guard: guard },
                cancel,
                epoch,
            )
            .await
    }

    /// Sends authenticated owner-authored text to the serialized owner-ingest session, once per
    /// source message id. The caller has already excluded quoted/forwarded content, worker
    /// reports, supervisor replies and task-only overrides.
    pub async fn ingest_owner(&self, source_message_id: &str, text: &str) -> Result<IngestOutcome, SupervisorFault> {
        let source = source_message_id.trim();
        if source.is_empty() || source.len() > 256 || source.chars().any(char::is_control) {
            return Err(SupervisorFault::protocol(
                "an owner source message id must be 1-256 printable characters",
            ));
        }
        let _order = self.inner.ingest_order.lock().await;
        if self.inner.store().was_ingested(source) {
            return Ok(IngestOutcome::Duplicate);
        }
        let guard = self.inner.ingest_session.clone().lock_owned().await;
        let cancel = self.inner.cancel.subscribe();
        let epoch = *cancel.borrow();
        let reply = self
            .inner
            .clone()
            .run(
                SessionRole::OwnerIngest,
                format!("[ferryx owner message {source}]\n{text}"),
                Lease::Exclusive { _guard: guard },
                cancel,
                epoch,
            )
            .await?;
        if let Err(error) = self.inner.store().note_ingested(source) {
            tracing::warn!(%error, "an ingested owner message id was not persisted");
        }
        Ok(IngestOutcome::Learned { reply })
    }

    /// Owner cancellation: every running or queued decision ends as
    /// [`SupervisorFault::Cancelled`] and its engine turn is aborted. Never waits on a slot.
    pub fn cancel_decisions(&self) {
        self.inner.cancel.send_modify(|epoch| *epoch += 1);
    }

    /// Stops the engine with the hub: decisions are cancelled, stdin is closed, the process
    /// gets [`SHUTDOWN_GRACE`] and is then killed. Nothing relaunches it until
    /// [`Self::supervise`] runs again.
    pub async fn shutdown(&self) {
        self.inner.run_state.store(STOPPED, Ordering::SeqCst);
        self.cancel_decisions();
        self.inner.stop_engine().await;
    }

    /// The [`ModelTurn`] the supervisor judge uses.
    pub fn model(&self) -> MhcModel {
        MhcModel {
            supervisor: self.clone(),
        }
    }

    /// Keeps the engine supervised: launches it, watches the owned process, retries failed
    /// launches with a 5s..300s backoff. A turn in flight when the engine exits is never resent;
    /// after an exit the engine relaunches on its next use.
    ///
    /// Calling it re-enables a stopped runtime at once (before the future is polled), so the
    /// new hub generation's first decision does not race it. Dropping the future (the hub
    /// generation's tasks are aborted on stop or handover) stops the engine, unless a newer
    /// generation has started supervising meanwhile.
    pub fn supervise(self) -> impl Future<Output = ()> + Send + 'static {
        let token = self.inner.next_token.fetch_add(1, Ordering::SeqCst);
        self.inner.run_state.store(token, Ordering::SeqCst);
        let guard = StopOnDrop {
            inner: self.inner.clone(),
            token,
        };
        async move {
            let _guard = guard;
            let mut health = self.inner.health.subscribe();
            loop {
                match self.inner.engine().await {
                    Ok(engine) => {
                        engine.client.closed().await;
                        let generation = engine.generation;
                        self.inner.engine_gone(&engine).await;
                        drop(engine);
                        let changed = health
                            .wait_for(|current| {
                                current.generation != generation
                                    || current.state == HealthState::Degraded
                            })
                            .await;
                        if changed.is_err() {
                            return;
                        }
                    }
                    Err(_) => {
                        if self.inner.is_stopped() {
                            // An explicit shutdown: nothing is relaunched until the next
                            // supervise call.
                            return;
                        }
                        let delay = self.inner.retry_delay().await;
                        tokio::time::sleep(delay).await;
                    }
                }
            }
        }
    }
}

/// Stops the engine when the supervising generation's future is dropped.
struct StopOnDrop {
    inner: Arc<Inner>,
    token: u64,
}

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        let ours = self
            .inner
            .run_state
            .compare_exchange(self.token, STOPPED, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok();
        if !ours {
            return;
        }
        self.inner.cancel.send_modify(|epoch| *epoch += 1);
        let inner = self.inner.clone();
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => {
                runtime.spawn(async move { inner.stop_engine().await });
            }
            Err(_) => tracing::warn!(
                "mhc supervisor stopped outside a runtime; its engine ends when stdin closes with this process"
            ),
        }
    }
}

/// [`ModelTurn`] over the supervisor's decision slots. It never substitutes another model.
#[derive(Clone)]
pub struct MhcModel {
    supervisor: MhcSupervisor,
}

#[async_trait]
impl ModelTurn for MhcModel {
    async fn complete(&self, prompt: &str) -> crate::error::Result<String> {
        self.supervisor.decide(prompt).await.map_err(HubError::from)
    }

    async fn complete_typed(&self, prompt: &str, context: TurnContext) -> Result<String, TurnFault> {
        let result = match context {
            TurnContext::Retained => self.supervisor.decide(prompt).await,
            TurnContext::Fresh => self.supervisor.converge(prompt).await,
        };
        result.map_err(TurnFault::Supervisor)
    }

    /// Every supervisor turn is bounded by [`MhcTimeouts::turn`](super::mhc_profile::MhcTimeouts)
    /// and reports a typed timeout itself.
    fn owns_deadline(&self) -> bool {
        true
    }

    fn is_supervisor_engine(&self) -> bool {
        true
    }
}

/// [`ModelTurn`] that resolves the process-wide supervisor at call time. A judge built before
/// the hub installed the engine, or in a process that never owns it, reports
/// [`SupervisorFault::NotRunning`] rather than reaching for another model.
pub struct InstalledModel;

impl InstalledModel {
    fn supervisor() -> Result<MhcSupervisor, SupervisorFault> {
        installed().ok_or_else(|| {
            SupervisorFault::not_running(
                "the mhc supervisor engine is owned by the hub daemon, not this process",
            )
        })
    }
}

#[async_trait]
impl ModelTurn for InstalledModel {
    async fn complete(&self, prompt: &str) -> crate::error::Result<String> {
        let supervisor = Self::supervisor().map_err(HubError::from)?;
        supervisor.model().complete(prompt).await
    }

    async fn complete_typed(&self, prompt: &str, context: TurnContext) -> Result<String, TurnFault> {
        let supervisor = Self::supervisor().map_err(TurnFault::Supervisor)?;
        supervisor.model().complete_typed(prompt, context).await
    }

    fn owns_deadline(&self) -> bool {
        true
    }

    fn is_supervisor_engine(&self) -> bool {
        true
    }
}

static INSTALLED: OnceLock<MhcSupervisor> = OnceLock::new();

/// The process-wide supervisor, created by `make` on first call. Later hub generations reuse it.
pub fn install_with(make: impl FnOnce() -> MhcSupervisor) -> MhcSupervisor {
    INSTALLED.get_or_init(make).clone()
}

pub fn installed() -> Option<MhcSupervisor> {
    INSTALLED.get().cloned()
}

/// [`MhcSupervisor::engine_snapshot`] of the engine installed in this process, or
/// [`SupervisorFault::NotRunning`] when this process owns none.
pub async fn installed_engine_snapshot() -> Result<Value, SupervisorFault> {
    match installed() {
        Some(supervisor) => Ok(supervisor.engine_snapshot().await),
        None => Err(SupervisorFault::not_running(
            "no mhc supervisor engine is installed in this process",
        )),
    }
}

/// The supervisor judge's model when the mhc supervisor is configured for this hub; `None`
/// otherwise. It is then the only model: it never falls back to the omo app-server. Besides an
/// engine installed in this process, the decision reads only sources every hub process shares
/// (see [`configured_in`]), never one process's environment, so the MCP server and CLI agree
/// with the daemon and report [`SupervisorFault::NotRunning`] instead of using the omo model.
pub fn configured_model() -> Option<Arc<dyn ModelTurn>> {
    // Tests never read the developer's hub env file or data directory.
    let configured = installed().is_some()
        || (!cfg!(test)
            && configured_in(
                crate::hub_env::file_env(),
                super::supervisor::state_dir_from_home().as_deref(),
            ));
    model_when(configured)
}

/// Whether this hub's supervisor is mhc, from shared sources only:
/// [`super::mhc_profile::ENABLE_KEY`] in the hub env file entries (`shared_env`), or a live
/// engine owner holding the profile lock under `state_dir` (covers a daemon that still runs the
/// engine after the file changed).
pub fn configured_in(shared_env: &BTreeMap<String, String>, state_dir: Option<&Path>) -> bool {
    MhcProfileConfig::enabled_in(shared_env) || state_dir.is_some_and(profile_lock_held)
}

/// The judge model for a hub whose supervisor is (or is not) mhc.
pub fn model_when(configured: bool) -> Option<Arc<dyn ModelTurn>> {
    configured.then(|| Arc::new(InstalledModel) as Arc<dyn ModelTurn>)
}

#[cfg(test)]
mod tests;
