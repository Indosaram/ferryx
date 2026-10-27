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
//! - Teardown: the engine lives as long as a hub generation supervises it. When that
//!   generation ends (hub stop, handover retirement) the engine's stdin is closed, it gets
//!   [`SHUTDOWN_GRACE`] to exit, then it is killed; no call relaunches it until the next
//!   generation supervises again. A process that never owned the engine (the MCP server)
//!   reports [`SupervisorFault::NotRunning`] instead of using another model.
use std::collections::HashMap;
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
    ConversationScope, MhcProfile, PersistedSession, RoleSpec, SessionRole, SessionStore,
    CLIENT_CAPABILITIES, DECISION_SLOTS,
};
use super::mhc_stdio::{BoxedReader, BoxedWriter, RequestError, StdioClient, WORK_REPLY_REQUEST};
use crate::error::HubError;
use crate::lanes::kibitzer::ModelTurn;

/// First relaunch delay after a failed launch; doubled per failure up to [`MAX_BACKOFF`].
pub const MIN_BACKOFF: Duration = Duration::from_secs(5);
pub const MAX_BACKOFF: Duration = Duration::from_secs(300);
/// How long a stopping engine gets to exit after its stdin closes before it is killed.
pub const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);
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
pub struct Launched {
    pub reader: BoxedReader,
    pub writer: BoxedWriter,
    pub pid: Option<u32>,
    pub exited: Option<ExitFuture>,
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
        let exited: ExitFuture = Box::pin(async move {
            child.wait().await.ok().map(|status| status.success())
        });
        Ok(Launched {
            reader: Box::new(stdout),
            writer: Box::new(stdin),
            pid,
            exited: Some(exited),
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
    exit_task: Option<tokio::task::JoinHandle<Option<bool>>>,
    /// Turns true when the owned process exited.
    exited: watch::Receiver<bool>,
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
    /// whatever is left.
    async fn terminate(&self, grace: Duration) {
        self.client.close();
        let owned = self.exit_task.is_some();
        let mut exited = self.exited.clone();
        let ended = tokio::time::timeout(grace, async {
            if owned {
                let _ = exited.wait_for(|done| *done).await;
            } else {
                self.client.closed().await;
            }
        })
        .await
        .is_ok();
        if !ended {
            tracing::warn!(generation = self.generation, pid = ?self.pid, "mhc supervisor engine did not exit after stdin closed; it is killed");
        }
        if let Some(task) = &self.exit_task {
            task.abort();
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // Dropping the exit watcher drops the child, which kills it (kill_on_drop).
        if let Some(task) = &self.exit_task {
            task.abort();
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
    Exclusive(OwnedMutexGuard<()>),
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
) -> Option<Cleanup> {
    (abort || close).then_some(Cleanup {
        engine,
        handle,
        events,
        abort,
        close,
    })
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
    conversations: Mutex<HashMap<String, (ConversationScope, Arc<tokio::sync::Mutex<()>>)>>,
    cancel: watch::Sender<u64>,
    work: RwLock<Arc<dyn WorkToolHandler>>,
    /// Decisions waiting for a slot; mirrored in [`SupervisorHealth::queued_decisions`].
    queued: AtomicUsize,
    /// `0` unsupervised, [`STOPPED`] after teardown, otherwise the supervising token.
    run_state: AtomicU64,
    next_token: AtomicU64,
}

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

    fn conversations(
        &self,
    ) -> MutexGuard<'_, HashMap<String, (ConversationScope, Arc<tokio::sync::Mutex<()>>)>> {
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
        drop(state);
        self.health.send_modify(|health| {
            health.state = HealthState::Stopped;
            health.pid = None;
            health.fault = None;
            health.detail = Some("stopped with the hub".to_owned());
        });
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
    /// failed launch the last fault is returned without another launch.
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
            self.note_exit(&engine);
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
                state.backoff = MIN_BACKOFF;
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
        let Launched {
            reader,
            writer,
            pid,
            exited,
        } = self.launcher.launch(&self.profile).await?;
        let (exit_tx, exit_rx) = watch::channel(false);
        let exit_task = exited.map(|exited| {
            tokio::spawn(async move {
                let status = exited.await;
                exit_tx.send_replace(true);
                status
            })
        });
        let (client, work) =
            StdioClient::start(reader, writer, self.profile.config.timeouts.max_line_bytes);
        let info = match handshake(&client, &self.profile).await {
            Ok(info) => info,
            Err(fault) => {
                if let Some(task) = exit_task {
                    task.abort();
                }
                if let Some(pid) = pid {
                    self.profile.forget_pid(pid);
                }
                return Err(fault);
            }
        };
        let engine = Arc::new(Engine {
            client,
            generation,
            pid,
            info,
            handles: Mutex::new(HashMap::new()),
            exit_task,
            exited: exit_rx,
        });
        tokio::spawn(serve_work(
            Arc::downgrade(self),
            Arc::downgrade(&engine),
            work,
        ));
        Ok(engine)
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
        }
        drop(state);
        self.note_exit(engine);
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
        let command = self
            .profile
            .open_session_command(spec, &durable_id, resume_path.as_deref());
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
            let _ = engine
                .client
                .send(&json!({"type": "close_session", "sessionId": handle}));
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
        let mut events = match engine.client.subscribe(&handle) {
            Ok(events) => events,
            Err(error) => return (Err(request_fault("subscribe", error)), None),
        };
        let client = engine.client.clone();
        let prompt = client.request(
            json!({"type": "prompt", "sessionId": handle, "message": message}),
            self.profile.config.timeouts.request,
        );
        let accepted = tokio::select! {
            accepted = prompt => accepted,
            () = cancel_signal(cancel, epoch, spec.cancellable) => {
                return (Err(SupervisorFault::Cancelled), cleanup(engine, handle, events, true, close));
            }
            () = caller.closed() => {
                return (Err(SupervisorFault::Cancelled), cleanup(engine, handle, events, true, close));
            }
        };
        if let Err(error) = accepted {
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
                cleanup(engine, handle, events, false, close),
            );
        }
        let deadline = tokio::time::Instant::now() + self.profile.config.timeouts.turn;
        let mut collector = TurnCollector::default();
        loop {
            tokio::select! {
                () = cancel_signal(cancel, epoch, spec.cancellable) => {
                    return (Err(SupervisorFault::Cancelled), cleanup(engine, handle, events, true, close));
                }
                () = caller.closed() => {
                    return (Err(SupervisorFault::Cancelled), cleanup(engine, handle, events, true, close));
                }
                () = tokio::time::sleep_until(deadline) => {
                    return (Err(SupervisorFault::timeout("supervisor turn")), cleanup(engine, handle, events, true, close));
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
                            return (result, cleanup(engine, handle, events, false, close));
                        }
                        Step::Closed(reason) => {
                            engine.forget(&spec.key);
                            engine.client.forget_closed(&handle);
                            engine.client.unsubscribe(&handle);
                            return (Err(SupervisorFault::session_closed(&reason)), None);
                        }
                        Step::Refuse(fault) => {
                            return (Err(fault), cleanup(engine, handle, events, true, close));
                        }
                    }
                }
            }
        }
    }

    /// Aborts and drains a turn its caller no longer waits for, then closes a one-shot context.
    /// A context that does not settle in time is dropped, so its next use resumes cleanly.
    async fn finish(&self, spec: &RoleSpec, after: Cleanup) {
        let Cleanup {
            engine,
            handle,
            mut events,
            abort,
            close,
        } = after;
        if abort {
            let _ = engine
                .client
                .send(&json!({"type": "abort", "sessionId": handle}));
            let settled = tokio::time::timeout(self.profile.config.timeouts.drain, async {
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
            .unwrap_or(false);
            if !settled && !close {
                engine.forget(&spec.key);
                let _ = engine
                    .client
                    .send(&json!({"type": "close_session", "sessionId": handle}));
            }
        }
        engine.client.unsubscribe(&handle);
        if close {
            let _ = engine
                .client
                .send(&json!({"type": "close_session", "sessionId": handle}));
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

async fn serve_work(inner: Weak<Inner>, engine: Weak<Engine>, mut requests: mpsc::UnboundedReceiver<Value>) {
    while let Some(record) = requests.recv().await {
        let (Some(inner), Some(engine)) = (inner.upgrade(), engine.upgrade()) else {
            return;
        };
        tokio::spawn(async move {
            inner.answer_work(&engine, record).await;
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
                Lease::Exclusive(guard),
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
                Lease::Exclusive(guard),
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
}

static INSTALLED: OnceLock<MhcSupervisor> = OnceLock::new();

/// The process-wide supervisor, created by `make` on first call. Later hub generations reuse it.
pub fn install_with(make: impl FnOnce() -> MhcSupervisor) -> MhcSupervisor {
    INSTALLED.get_or_init(make).clone()
}

pub fn installed() -> Option<MhcSupervisor> {
    INSTALLED.get().cloned()
}

/// The supervisor judge's model when the mhc supervisor is configured for this hub (installed
/// here, or `FERRYX_HUB_MHC_SUPERVISOR` on); `None` otherwise. It is then the only model: it
/// never falls back to the omo app-server.
pub fn configured_model() -> Option<Arc<dyn ModelTurn>> {
    let configured = installed().is_some()
        || (!cfg!(test) && super::mhc_profile::MhcProfileConfig::from_env().is_some());
    configured.then(|| Arc::new(InstalledModel) as Arc<dyn ModelTurn>)
}

#[cfg(test)]
mod tests;
