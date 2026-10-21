use crate::ipc::{run_blocking, IpcError, IpcErrorCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SwitchDebugEntry {
    pub run_id: String,
    pub sequence: u64,
    pub event: String,
    pub wall_time_ms: f64,
    #[serde(default)]
    pub details: Value,
}

pub const MAX_CONCURRENT_SINK_TASKS: usize = 16;
pub const MAX_ENTRY_BYTES: usize = 4096;
pub const MAX_LOG_FILE_BYTES: u64 = 5 * 1024 * 1024;

static DEBUG_SINK_IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);
static DEBUG_SINK_DROPS: AtomicU64 = AtomicU64::new(0);
/// Open append handles plus bytes written, keyed by log path. Holding the lock also serializes
/// rotation. Tracking the size in memory avoids a `metadata` call per entry.
static SINK_FILES: Mutex<Option<HashMap<std::path::PathBuf, (std::fs::File, u64)>>> =
    Mutex::new(None);

static NATIVE_DEBUG_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static NATIVE_DEBUG_RUN_ID: Mutex<Option<String>> = Mutex::new(None);

type ScopedSinkFn = Arc<dyn Fn(&SwitchDebugEntry) + Send + Sync>;
static SCOPED_SINK_REGISTRY: Mutex<Option<HashMap<String, (u64, ScopedSinkFn)>>> = Mutex::new(None);
static SCOPED_SINK_TOKEN_GEN: AtomicU64 = AtomicU64::new(1);

pub fn set_scoped_sink_for_operation<F>(operation_id: &str, sink: F) -> impl Drop
where
    F: Fn(&SwitchDebugEntry) + Send + Sync + 'static,
{
    let token = SCOPED_SINK_TOKEN_GEN.fetch_add(1, Ordering::Relaxed);
    let mut guard = SCOPED_SINK_REGISTRY.lock().unwrap_or_else(|p| p.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    map.insert(operation_id.to_string(), (token, Arc::new(sink)));

    struct ScopedOpSinkGuard {
        key: String,
        token: u64,
    }
    impl Drop for ScopedOpSinkGuard {
        fn drop(&mut self) {
            let mut guard = SCOPED_SINK_REGISTRY.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(map) = guard.as_mut() {
                if let Some((existing_token, _)) = map.get(&self.key) {
                    if *existing_token == self.token {
                        map.remove(&self.key);
                    }
                }
            }
        }
    }
    ScopedOpSinkGuard {
        key: operation_id.to_string(),
        token,
    }
}

pub fn native_debug_run_id() -> String {
    let mut guard = NATIVE_DEBUG_RUN_ID.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(id) = guard.as_ref() {
        id.clone()
    } else {
        let id = format!("native-{}", &uuid::Uuid::new_v4().to_string()[..8]);
        *guard = Some(id.clone());
        id
    }
}

pub fn debug_sink_drop_count() -> u64 {
    DEBUG_SINK_DROPS.load(Ordering::Relaxed)
}

pub fn debug_sink_in_flight() -> usize {
    DEBUG_SINK_IN_FLIGHT.load(Ordering::Relaxed)
}

pub fn reset_debug_sink_metrics_for_test() {
    DEBUG_SINK_IN_FLIGHT.store(0, Ordering::SeqCst);
    DEBUG_SINK_DROPS.store(0, Ordering::SeqCst);
    NATIVE_DEBUG_SEQUENCE.store(0, Ordering::SeqCst);
}

pub struct SinkTaskGuard;

impl SinkTaskGuard {
    pub fn try_acquire() -> Option<Self> {
        let in_flight = DEBUG_SINK_IN_FLIGHT.fetch_add(1, Ordering::SeqCst);
        if in_flight >= MAX_CONCURRENT_SINK_TASKS {
            DEBUG_SINK_IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
            DEBUG_SINK_DROPS.fetch_add(1, Ordering::Relaxed);
            None
        } else {
            Some(Self)
        }
    }
}

impl Drop for SinkTaskGuard {
    fn drop(&mut self) {
        DEBUG_SINK_IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn is_release_persisted_event(event: &str) -> bool {
    matches!(
        event,
        "terminal.surface.input.accepted"
            | "terminal.surface.input.dispatch"
            | "terminal.surface.input.stage.backend_write_start"
            | "terminal.surface.input.stage.backend_write"
            | "terminal.surface.input.stage.backend_write_result"
            | "terminal.render.vt_consumed"
            | "terminal.surface.presentation.receipt"
            | "terminal.surface.presented"
            | "terminal.surface.bounds_acknowledged"
            | "terminal.surface.input.dropped.overflow"
            | "terminal.surface.input.dropped.rate"
            | "terminal.surface.input.dropped.owner_mismatch"
            | "terminal.surface.input.dropped.summary"
            | "terminal.surface.input.in_flight_slow"
            | "terminal.surface.input.error.recovering"
            | "terminal.surface.input.failed"
            | "terminal.surface.input.sent"
            | "terminal.surface.presentation.stale"
    )
}

pub fn switch_debug_sink_enabled(debug_build: bool, env_flag: Option<&str>) -> bool {
    debug_build || env_flag == Some("1")
}

fn switch_debug_sink_enabled_here() -> bool {
    switch_debug_sink_enabled(
        cfg!(debug_assertions),
        std::env::var("FERRYX_SWITCH_DEBUG").ok().as_deref(),
    )
}

pub fn should_persist_event(event: &str) -> bool {
    should_persist_entry(switch_debug_sink_enabled_here(), event)
}

pub fn should_persist_entry(debug_enabled: bool, event: &str) -> bool {
    debug_enabled || is_release_persisted_event(event)
        || event.starts_with("terminal.surface.input.dropped")
        || event.starts_with("terminal.surface.input.gate")
        || event.starts_with("terminal.surface.input.slow")
        || event.starts_with("terminal.surface.input.in_flight")
        || event.starts_with("terminal.surface.input.backend")
        || event.starts_with("terminal.surface.input.stage")
        || event.starts_with("terminal.surface.input.stall")
        || event.starts_with("app.watchdog")
        || event.starts_with("terminal.render.")
        || event.starts_with("terminal.open.")
        || event.starts_with("project.register.")
        || event.starts_with("terminal.surface.attach.")
        || event.starts_with("terminal.surface.bounds.")
        || event.starts_with("terminal.surface.detach.")
        || event == "terminal.surface.skipped"
        || event == "terminal.surface.cover"
        || is_release_persisted_navigation_event(event)
}

/// Worktree selection, workspace swaps and restores are rare, user-driven transitions. Without
/// them a release build cannot explain a row that opens another project's tabs or a "New
/// Terminal" that produces nothing. `workspace.render` fires on every render and stays debug-only.
pub fn is_release_persisted_navigation_event(event: &str) -> bool {
    event.starts_with("worktree.")
        || (event.starts_with("workspace.") && event != "workspace.render")
}

const SWITCH_DEBUG_FILE: &str = "ferryx-switch-debug.jsonl";
/// Per-chunk render receipts get their own file so their volume cannot rotate the rare
/// lifecycle events out of the main log within minutes.
const SWITCH_DEBUG_RENDER_FILE: &str = "ferryx-switch-debug-render.jsonl";

fn switch_debug_file_for_event(event: &str) -> &'static str {
    if event == "terminal.render.vt_consumed" {
        SWITCH_DEBUG_RENDER_FILE
    } else {
        SWITCH_DEBUG_FILE
    }
}

fn switch_debug_path(root: &std::path::Path, file_name: &str) -> std::path::PathBuf {
    root.join(file_name)
}

pub fn is_allowlisted_detail_key(k: &str) -> bool {
    matches!(
        k,
        "nested"
            | "operationId"
            | "backendSessionId"
            | "localSessionId"
            | "attemptedIncarnation"
            | "attemptedDaemonEpoch"
            | "classification"
            | "errorSource"
            | "reportedSessionId"
            | "reportedDaemonEpoch"
            | "reportedIncarnation"
            | "retryCount"
            | "bannerRetryThreshold"
            | "refreshedIdentityAfterDetach"
            | "sessionId"
            | "paneIdentity"
            | "bindingKey"
            | "attemptGeneration"
            | "daemonEpoch"
            | "vtEpoch"
            | "incarnation"
            | "payloadBytes"
            | "durationMs"
            | "success"
            | "errorCode"
            | "consumedSequence"
            | "cursorCol"
            | "cursorRow"
            | "cellWidthPx"
            | "cellHeightPx"
            | "presented"
            | "renderDeferred"
            | "queuedAt"
            | "dispatchedAt"
            | "startedAt"
            | "wallTimeMs"
            | "sequence"
            | "runId"
            | "event"
            | "reason"
            | "totalDroppedInStall"
            | "dropsSinceLastReport"
            | "consecutiveDrops"
            | "queuedEntries"
            | "queuedBytes"
            | "runningAgeMs"
            | "remoteConnectionState"
            | "total"
            | "recovered"
            | "bounds"
            | "scaleFactor"
            | "x"
            | "y"
            | "width"
            | "height"
            | "generation"
            | "covered"
            | "visible"
            | "surfaceVisible"
            | "interactive"
            | "suspended"
            | "isExited"
            | "totalBytes"
            | "dropReason"
            | "textLength"
            | "commandLength"
            | "clipboardLength"
            | "inputLength"
            | "preeditLength"
            | "keyEventPresent"
            // Workspace / worktree navigation identity, so a release log can say which project
            // and which worktree a selection, swap, registration or terminal open concerned.
            | "workspaceId"
            | "workspaceIds"
            | "persistedWorkspaceIds"
            | "currentWorkspaceId"
            | "ownerWorkspaceId"
            | "requestedWorkspaceId"
            | "registeredWorkspaceId"
            | "mountedWorkspaceId"
            | "fromWorkspaceId"
            | "toWorkspaceId"
            | "incomingStateWorkspaceId"
            | "repoRoot"
            | "worktreePath"
            | "requestedPath"
            | "preferredPath"
            | "activeWorktreePath"
            | "incomingActiveWorktreePath"
            | "listedPaths"
            | "tabId"
            | "activeTabId"
            | "parkedActiveTabId"
            | "tabCount"
            | "activeTabCount"
            | "parkedTabCount"
            | "sessionCount"
            | "worktreeCount"
            | "listedCount"
            | "liveCount"
            | "initialWorktreeCount"
            | "incomingWorktreeCount"
            | "incomingTabCount"
            | "incomingSessionCount"
            | "outgoingTabCount"
            | "outgoingSessionCount"
            | "allowCreate"
            | "pendingCrossProject"
            | "adopted"
            | "cancelled"
            | "current"
            | "stage"
            | "source"
            | "status"
            | "outcome"
            | "code"
            | "registrationAttempt"
            | "recoveredFromHmr"
            | "backendOverride"
    )
}

pub fn normalize_error_code(raw: &str) -> &'static str {
    match raw {
        "TIMEOUT" | "timeout" => "TIMEOUT",
        "SESSION_NOT_FOUND" | "session_not_found" => "SESSION_NOT_FOUND",
        "STALE_GENERATION" | "stale_generation" => "STALE_GENERATION",
        "QUEUE_OVERFLOW" | "queue_overflow" => "QUEUE_OVERFLOW",
        "IO_ERROR" | "io_error" => "IO_ERROR",
        "PERMISSION_DENIED" | "permission_denied" => "PERMISSION_DENIED",
        "DISCONNECTED" | "disconnected" => "DISCONNECTED",
        "ALREADY_EXISTS" | "already_exists" => "ALREADY_EXISTS",
        "INVALID_VALUE" | "invalid_value" => "INVALID_VALUE",
        "INTERNAL_ERROR" | "internal_error" => "INTERNAL_ERROR",
        "BOUNDS_ERROR" | "bounds_error" => "BOUNDS_ERROR",
        "UNKNOWN_BOUNDS_ERROR" => "UNKNOWN_BOUNDS_ERROR",
        "PAIRED_HOST_DISCONNECTED" => "PAIRED_HOST_DISCONNECTED",
        "PAIRED_HOST_INVALID_RESPONSE" => "PAIRED_HOST_INVALID_RESPONSE",
        "PAIRED_PROXY_MISSING" => "PAIRED_PROXY_MISSING",
        _ => "UNKNOWN_ERROR",
    }
}

pub fn sanitize_switch_debug_details(_event: &str, details: &Value) -> Value {
    fn sanitize_value(v: &Value) -> Value {
        match v {
            Value::Object(map) => {
                let mut sanitized = serde_json::Map::new();
                for (k, val) in map {
                    if k == "text"
                        || k == "keyEvent"
                        || k == "clipboard"
                        || k == "input"
                        || k == "command"
                        || k == "preedit"
                    {
                        if let Value::String(s) = val {
                            sanitized.insert(format!("{k}Length"), Value::from(s.len()));
                        } else {
                            sanitized.insert(format!("{k}Present"), Value::from(!val.is_null()));
                        }
                    } else if k == "error" {
                        let code = match val {
                            Value::Object(err_obj) => err_obj
                                .get("code")
                                .and_then(Value::as_str)
                                .map(normalize_error_code)
                                .unwrap_or("UNKNOWN_ERROR"),
                            Value::String(err_str) => normalize_error_code(err_str),
                            _ => "UNKNOWN_ERROR",
                        };
                        sanitized.insert("errorCode".to_string(), Value::from(code));
                    } else if is_allowlisted_detail_key(k) {
                        sanitized.insert(k.clone(), sanitize_value(val));
                    }
                }
                Value::Object(sanitized)
            }
            Value::Array(arr) => Value::Array(arr.iter().map(sanitize_value).collect()),
            other => other.clone(),
        }
    }
    sanitize_value(details)
}

pub(crate) fn log_native_switch_debug(entry: Value) {
    let event = entry.get("event").and_then(Value::as_str).unwrap_or("").to_string();
    if !should_persist_event(&event) {
        return;
    }
    let Some(_guard) = SinkTaskGuard::try_acquire() else {
        return;
    };

    let full_entry = if entry.get("runId").is_some() && entry.get("sequence").is_some() {
        match serde_json::from_value::<SwitchDebugEntry>(entry) {
            Ok(mut e) => {
                e.details = sanitize_switch_debug_details(&e.event, &e.details);
                e
            }
            Err(_) => return,
        }
    } else {
        let details = entry.get("details").cloned().unwrap_or(Value::Null);
        let sanitized_details = sanitize_switch_debug_details(&event, &details);
        let wall_time_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|d| d.as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        let seq = NATIVE_DEBUG_SEQUENCE.fetch_add(1, Ordering::SeqCst) + 1;
        SwitchDebugEntry {
            run_id: native_debug_run_id(),
            sequence: seq,
            event,
            wall_time_ms,
            details: sanitized_details,
        }
    };

    let op_key = full_entry
        .details
        .get("operationId")
        .or_else(|| full_entry.details.get("sessionId"))
        .and_then(Value::as_str);

    if let Some(k) = op_key {
        let hook = {
            let guard = SCOPED_SINK_REGISTRY.lock().unwrap_or_else(|p| p.into_inner());
            guard.as_ref().and_then(|map| map.get(k).map(|(_, h)| Arc::clone(h)))
        };
        if let Some(hook) = hook {
            hook(&full_entry);
        }
    }

    tauri::async_runtime::spawn_blocking(move || {
        let _guard = _guard;
        let file_name = switch_debug_file_for_event(&full_entry.event);
        if let Err(error) = append_switch_debug_entry(&std::env::temp_dir(), file_name, &full_entry) {
            tracing::warn!(?error, "Could not persist native switch debug entry");
        }
    });
}

fn append_switch_debug_entry(
    root: &std::path::Path,
    file_name: &str,
    entry: &impl Serialize,
) -> Result<(), IpcError> {
    use std::fs::OpenOptions;
    use std::io::Write;

    let serialized = serde_json::to_string(entry)
        .map_err(|error| IpcError::internal(format!("serialize switch debug entry: {error}")))?;

    if serialized.len() > MAX_ENTRY_BYTES {
        DEBUG_SINK_DROPS.fetch_add(1, Ordering::Relaxed);
        return Err(IpcError::new(
            IpcErrorCode::PayloadTooLarge,
            format!(
                "Switch debug entry exceeds max size of {MAX_ENTRY_BYTES} bytes (actual: {})",
                serialized.len()
            ),
        ));
    }

    let path = switch_debug_path(root, file_name);
    let mut sink_files = SINK_FILES.lock().unwrap_or_else(|p| p.into_inner());
    let files = sink_files.get_or_insert_with(HashMap::new);

    let open = |path: &std::path::Path| -> Result<(std::fs::File, u64), IpcError> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|error| IpcError::internal(format!("open switch debug log: {error}")))?;
        let len = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
        Ok((file, len))
    };

    if !files.contains_key(&path) {
        let opened = open(&path)?;
        files.insert(path.clone(), opened);
    }
    if files.get(&path).is_some_and(|(_, len)| *len >= MAX_LOG_FILE_BYTES) {
        files.remove(&path);
        let rotated_path = root.join(format!("{file_name}.1"));
        let _ = std::fs::rename(&path, &rotated_path);
        let opened = open(&path)?;
        files.insert(path.clone(), opened);
    }

    let Some((file, len)) = files.get_mut(&path) else {
        return Err(IpcError::internal("switch debug log handle missing"));
    };
    match writeln!(file, "{serialized}") {
        Ok(()) => {
            *len += serialized.len() as u64 + 1;
            Ok(())
        }
        Err(error) => {
            // Drop the handle so the next entry reopens the log.
            files.remove(&path);
            Err(IpcError::internal(format!("write switch debug log: {error}")))
        }
    }
}

/// Drops every cached sink handle so tests can rewrite or delete log files out of band.
#[cfg(test)]
fn close_cached_switch_debug_files() {
    *SINK_FILES.lock().unwrap_or_else(|p| p.into_inner()) = None;
}

#[tauri::command]
pub async fn cmd_switch_debug_log(mut entry: SwitchDebugEntry) -> Result<(), IpcError> {
    if !should_persist_event(&entry.event) {
        return Ok(());
    }
    let Some(guard) = SinkTaskGuard::try_acquire() else {
        return Ok(());
    };
    entry.details = sanitize_switch_debug_details(&entry.event, &entry.details);
    let op_key = entry
        .details
        .get("operationId")
        .or_else(|| entry.details.get("sessionId"))
        .and_then(Value::as_str);

    if let Some(k) = op_key {
        let hook = {
            let guard = SCOPED_SINK_REGISTRY.lock().unwrap_or_else(|p| p.into_inner());
            guard.as_ref().and_then(|map| map.get(k).map(|(_, h)| Arc::clone(h)))
        };
        if let Some(hook) = hook {
            hook(&entry);
        }
    }
    run_blocking(move || {
        let _guard = guard;
        let file_name = switch_debug_file_for_event(&entry.event);
        append_switch_debug_entry(&std::env::temp_dir(), file_name, &entry)
    })
    .await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PaneLivenessVerdict {
    Idle,
    BlockedInQueue,
    BlockedInIpcWrite,
    BlockedInReaderPaused,
    BlockedInKernelStopped,
    BlockedInAttributedSuspension,
    BlockedInPresentation,
    UnknownApplicationResponse,
    Unknown,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneLivenessSnapshot {
    pub telemetry_available: bool,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub vt_session_id: Option<String>,
    #[serde(default)]
    pub operation_id: Option<String>,
    #[serde(default)]
    pub stage: Option<String>,
    #[serde(default)]
    pub queued_head_age_ms: Option<u64>,
    #[serde(default)]
    pub queued_count: Option<usize>,
    #[serde(default)]
    pub executing_running_age_ms: Option<u64>,
    #[serde(default)]
    pub write_pending_ms: Option<u64>,
    #[serde(default)]
    pub write_success: Option<bool>,
    #[serde(default)]
    pub reader_paused: Option<bool>,
    #[serde(default)]
    pub kernel_stopped: Option<bool>,
    #[serde(default)]
    pub registry_suspended: Option<bool>,
    #[serde(default)]
    pub suspended: Option<bool>,
    #[serde(default)]
    pub suspension_source: Option<String>,
    #[serde(default)]
    pub verified_actuation_receipt: Option<bool>,
    #[serde(default)]
    pub daemon_epoch: Option<String>,
    #[serde(default)]
    pub vt_epoch: Option<String>,
    #[serde(default)]
    pub hub_start_sequence: Option<u64>,
    #[serde(default)]
    pub hub_end_sequence: Option<u64>,
    #[serde(default)]
    pub vt_consumed_sequence: Option<u64>,
    #[serde(default)]
    pub has_unpresented_frames: Option<bool>,
    #[serde(default)]
    pub presentation_receipt_received: Option<bool>,
}

pub fn classify_pane_liveness(snapshot: &PaneLivenessSnapshot) -> PaneLivenessVerdict {
    if !snapshot.telemetry_available {
        return PaneLivenessVerdict::Unknown;
    }

    if snapshot.reader_paused == Some(true) {
        return PaneLivenessVerdict::BlockedInReaderPaused;
    }

    if snapshot.suspended == Some(true) || snapshot.kernel_stopped == Some(true) {
        if snapshot.verified_actuation_receipt == Some(true)
            && snapshot.suspension_source.as_deref() == Some("ferryx-lifecycle")
        {
            return PaneLivenessVerdict::BlockedInAttributedSuspension;
        }
        if snapshot.kernel_stopped == Some(true)
            || snapshot.suspension_source.as_deref() == Some("external-kernel")
        {
            return PaneLivenessVerdict::BlockedInKernelStopped;
        }
        return PaneLivenessVerdict::Unknown;
    }

    let is_slow_execution = snapshot.executing_running_age_ms.map_or(false, |age| age > 250)
        || snapshot.write_pending_ms.map_or(false, |ms| ms > 250);

    if is_slow_execution {
        return PaneLivenessVerdict::BlockedInIpcWrite;
    }

    if snapshot.has_unpresented_frames == Some(true) {
        return PaneLivenessVerdict::BlockedInPresentation;
    }

    let hub_session = snapshot.session_id.as_deref();
    let vt_session = snapshot
        .vt_session_id
        .as_deref()
        .or(snapshot.session_id.as_deref());
    if let (Some(daemon_epoch), Some(vt_epoch), Some(h_sess), Some(v_sess)) =
        (snapshot.daemon_epoch.as_deref(), snapshot.vt_epoch.as_deref(), hub_session, vt_session)
    {
        if daemon_epoch == vt_epoch && h_sess == v_sess && !h_sess.is_empty() {
            if let (Some(hub_end), Some(vt_consumed)) =
                (snapshot.hub_end_sequence, snapshot.vt_consumed_sequence)
            {
                if hub_end > vt_consumed {
                    return PaneLivenessVerdict::BlockedInPresentation;
                }
            }
        }
    }

    if snapshot.queued_head_age_ms.map_or(false, |age| age > 250) {
        return PaneLivenessVerdict::BlockedInQueue;
    }

    if matches!(
        snapshot.stage.as_deref(),
        Some("dispatch") | Some("backend_write_start")
    ) || snapshot.executing_running_age_ms.is_some()
    {
        return PaneLivenessVerdict::Unknown;
    }

    if snapshot.write_success == Some(true) && snapshot.presentation_receipt_received != Some(true) {
        if let (Some(daemon_epoch), Some(vt_epoch), Some(h_sess), Some(v_sess)) =
            (snapshot.daemon_epoch.as_deref(), snapshot.vt_epoch.as_deref(), hub_session, vt_session)
        {
            if daemon_epoch == vt_epoch && h_sess == v_sess && !h_sess.is_empty() {
                if let (Some(hub_end), Some(vt_consumed)) =
                    (snapshot.hub_end_sequence, snapshot.vt_consumed_sequence)
                {
                    if hub_end <= vt_consumed {
                        return PaneLivenessVerdict::BlockedInPresentation;
                    }
                }
            }
        }
        return PaneLivenessVerdict::Unknown;
    }

    let has_identity = snapshot
        .session_id
        .as_ref()
        .map_or(false, |id| !id.trim().is_empty());
    let no_queued = snapshot.queued_head_age_ms.is_none() && snapshot.queued_count.unwrap_or(0) == 0;
    let no_executing = snapshot.executing_running_age_ms.is_none();
    let no_write_pending = snapshot.write_pending_ms.is_none();
    let is_not_stopped = snapshot.suspended == Some(false) && snapshot.kernel_stopped == Some(false);
    let is_not_paused = snapshot.reader_paused == Some(false);
    let is_not_lagged = snapshot.has_unpresented_frames == Some(false);

    if has_identity
        && no_queued
        && no_executing
        && no_write_pending
        && is_not_stopped
        && is_not_paused
        && is_not_lagged
    {
        PaneLivenessVerdict::Idle
    } else {
        PaneLivenessVerdict::Unknown
    }
}

#[cfg(feature = "native-terminal")]
pub fn collect_pane_liveness(
    session_id: &str,
    state: &crate::native_terminal::surface_host::NativeTerminalSurfaceHostState,
) -> PaneLivenessVerdict {
    match state.session_liveness_observation(session_id) {
        Some(snapshot) => classify_pane_liveness(&snapshot),
        None => PaneLivenessVerdict::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsed_entries_append_to_injected_portable_root() {
        let root = tempfile::tempdir().expect("owned sink root");
        let path = switch_debug_path(root.path(), SWITCH_DEBUG_FILE);
        assert_eq!(path.parent(), Some(root.path()));
        let wire = serde_json::json!({
            "runId": "p02-owned", "sequence": 7, "event": "wheel.receipt",
            "wallTimeMs": 123.0, "details": {"cell": [2, 3], "ctrl": true}
        });
        let entry: SwitchDebugEntry = serde_json::from_value(wire.clone()).expect("parsed event");
        append_switch_debug_entry(root.path(), SWITCH_DEBUG_FILE, &entry).expect("first append");
        append_switch_debug_entry(root.path(), SWITCH_DEBUG_FILE, &entry).expect("second append");
        let text = std::fs::read_to_string(path).expect("read actual sink");
        let entries: Vec<Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).expect("JSONL record"))
            .collect();
        assert_eq!(entries, vec![wire.clone(), wire]);
        close_cached_switch_debug_files();
        root.close().expect("remove owned sink");
    }

    #[test]
    fn invalid_sink_root_returns_error() {
        let root = tempfile::tempdir().expect("owned sink root");
        let invalid = root.path().join("file-not-directory");
        assert_eq!(
            switch_debug_path(&invalid, SWITCH_DEBUG_FILE).parent(),
            Some(invalid.as_path())
        );
        std::fs::write(&invalid, b"sentinel").expect("create non-directory");
        assert!(
            append_switch_debug_entry(&invalid, SWITCH_DEBUG_FILE, &serde_json::json!({"event": "test"}))
                .is_err()
        );
        assert_eq!(std::fs::read(&invalid).expect("read sentinel"), b"sentinel");
        root.close().expect("remove owned sink");
    }

    #[test]
    fn debug_builds_always_trace() {
        assert!(switch_debug_sink_enabled(true, None));
    }

    #[test]
    fn release_builds_stay_silent_by_default() {
        assert!(!switch_debug_sink_enabled(false, None));
        assert!(should_persist_entry(false, "terminal.surface.input.sent"));
        assert!(should_persist_entry(
            false,
            "terminal.surface.input.dropped.overflow"
        ));
        assert!(should_persist_entry(
            false,
            "terminal.surface.input.dropped.outage"
        ));
        assert!(should_persist_entry(
            false,
            "terminal.surface.input.gate.unclaimed"
        ));
        assert!(should_persist_entry(
            false,
            "terminal.surface.input.slow"
        ));
        assert!(should_persist_entry(
            false,
            "terminal.surface.input.in_flight_slow"
        ));
        assert!(should_persist_entry(
            false,
            "terminal.surface.input.backend.slow"
        ));
        assert!(should_persist_entry(
            false,
            "terminal.surface.input.stage.slow"
        ));
        assert!(should_persist_entry(
            false,
            "terminal.surface.input.stall.resolved"
        ));
        assert!(should_persist_entry(
            false,
            "app.watchdog.hang_detected"
        ));
        assert!(should_persist_entry(
            false,
            "app.watchdog.recovered"
        ));
        assert!(should_persist_entry(false, "terminal.render.attach"));
        assert!(should_persist_entry(false, "terminal.render.reattach"));
        assert!(should_persist_entry(false, "terminal.render.redraw"));
        assert!(should_persist_entry(false, "terminal.open.discarded"));
        assert!(should_persist_entry(false, "project.register.error"));
        assert!(should_persist_entry(false, "terminal.surface.attach.error"));
        assert!(should_persist_entry(false, "terminal.surface.bounds.error"));
        assert!(should_persist_entry(false, "terminal.surface.bounds.start"));
        assert!(should_persist_entry(false, "terminal.surface.detach.complete"));
        assert!(should_persist_entry(false, "terminal.surface.cover"));
        assert!(!should_persist_entry(false, "terminal.lifecycle.present"));
    }

    #[test]
    fn release_builds_persist_worktree_and_workspace_navigation() {
        assert!(should_persist_entry(false, "worktree.select.requested"));
        assert!(should_persist_entry(false, "worktree.ensure.restore-parked"));
        assert!(should_persist_entry(false, "worktree.refresh.listed"));
        assert!(should_persist_entry(false, "workspace.store.swap"));
        assert!(should_persist_entry(false, "workspace.restore.complete"));
        assert!(!should_persist_entry(false, "workspace.render"));
    }

    #[test]
    fn navigation_identity_details_survive_sanitization() {
        let raw = serde_json::json!({
            "currentWorkspaceId": "lectures",
            "ownerWorkspaceId": "mahoquot",
            "worktreePath": "/repo/mahoquot",
            "listedPaths": ["/repo/lectures"],
            "tabCount": 2,
            "message": "free-form error text",
        });
        let sanitized = sanitize_switch_debug_details("worktree.select.requested", &raw);
        assert_eq!(sanitized.get("currentWorkspaceId").and_then(Value::as_str), Some("lectures"));
        assert_eq!(sanitized.get("ownerWorkspaceId").and_then(Value::as_str), Some("mahoquot"));
        assert_eq!(sanitized.get("worktreePath").and_then(Value::as_str), Some("/repo/mahoquot"));
        assert_eq!(sanitized.get("listedPaths"), Some(&serde_json::json!(["/repo/lectures"])));
        assert_eq!(sanitized.get("tabCount").and_then(Value::as_u64), Some(2));
        assert_eq!(sanitized.get("message"), None);
    }

    #[test]
    fn render_receipts_rotate_in_their_own_file() {
        let root = tempfile::tempdir().expect("owned sink root");
        let render = SwitchDebugEntry {
            run_id: "r".into(),
            sequence: 1,
            event: "terminal.render.vt_consumed".into(),
            wall_time_ms: 1.0,
            details: serde_json::json!({"consumedSequence": 1}),
        };
        let lifecycle = SwitchDebugEntry {
            event: "worktree.select.requested".into(),
            ..render.clone()
        };
        for entry in [&render, &lifecycle] {
            append_switch_debug_entry(root.path(), switch_debug_file_for_event(&entry.event), entry)
                .expect("append");
        }
        let main = std::fs::read_to_string(root.path().join(SWITCH_DEBUG_FILE)).expect("main log");
        let render_log =
            std::fs::read_to_string(root.path().join(SWITCH_DEBUG_RENDER_FILE)).expect("render log");
        assert!(main.contains("worktree.select.requested") && !main.contains("vt_consumed"));
        assert!(render_log.contains("vt_consumed") && !render_log.contains("worktree.select"));

        close_cached_switch_debug_files();
        std::fs::write(
            root.path().join(SWITCH_DEBUG_RENDER_FILE),
            vec![b'x'; MAX_LOG_FILE_BYTES as usize],
        )
        .expect("fill render log");
        append_switch_debug_entry(root.path(), SWITCH_DEBUG_RENDER_FILE, &render).expect("rotate");
        assert!(root.path().join(format!("{SWITCH_DEBUG_RENDER_FILE}.1")).exists());
        let main_after = std::fs::read_to_string(root.path().join(SWITCH_DEBUG_FILE)).expect("main log");
        assert_eq!(main_after, main, "render rotation must not touch the lifecycle log");
        close_cached_switch_debug_files();
        root.close().expect("remove owned sink");
    }

    #[test]
    fn release_builds_trace_when_opted_in() {
        assert!(switch_debug_sink_enabled(false, Some("1")));
    }

    #[test]
    fn release_builds_ignore_any_other_flag_value() {
        assert!(!switch_debug_sink_enabled(false, Some("true")));
        assert!(!switch_debug_sink_enabled(false, Some("0")));
    }
}

#[cfg(test)]
mod pane_liveness_diagnostics_tests {
    use super::*;
    use crate::daemon::protocol::{DaemonResponse, DaemonSessionDetails};

    #[test]
    fn pane_liveness_diagnostics_release_allowlist_filtering() {
        assert!(is_release_persisted_event("terminal.surface.input.accepted"));
        assert!(is_release_persisted_event("terminal.surface.input.dispatch"));
        assert!(is_release_persisted_event("terminal.surface.input.stage.backend_write_start"));
        assert!(is_release_persisted_event("terminal.surface.input.stage.backend_write"));
        assert!(is_release_persisted_event("terminal.render.vt_consumed"));
        assert!(is_release_persisted_event("terminal.surface.presentation.receipt"));
        assert!(is_release_persisted_event("terminal.surface.presented"));
        assert!(!is_release_persisted_event("project.select"));
        assert!(!is_release_persisted_event("workspace.swap"));
        assert!(!is_release_persisted_event("arbitrary.event"));
    }

    #[test]
    fn pane_liveness_diagnostics_sink_concurrency_and_drop_accounting() {
        reset_debug_sink_metrics_for_test();
        let mut guards = Vec::new();
        for _ in 0..MAX_CONCURRENT_SINK_TASKS {
            let guard = SinkTaskGuard::try_acquire().expect("acquire slot within bound");
            guards.push(guard);
        }
        assert_eq!(debug_sink_in_flight(), MAX_CONCURRENT_SINK_TASKS);
        assert_eq!(debug_sink_drop_count(), 0);

        let rejected = SinkTaskGuard::try_acquire();
        assert!(rejected.is_none());
        assert_eq!(debug_sink_drop_count(), 1);

        drop(guards.pop());
        assert_eq!(debug_sink_in_flight(), MAX_CONCURRENT_SINK_TASKS - 1);

        let reacquired = SinkTaskGuard::try_acquire().expect("slot freed after drop");
        guards.push(reacquired);
        assert_eq!(debug_sink_in_flight(), MAX_CONCURRENT_SINK_TASKS);
        assert_eq!(debug_sink_drop_count(), 1);

        drop(guards);
        assert_eq!(debug_sink_in_flight(), 0);
    }

    #[test]
    fn pane_liveness_diagnostics_entry_size_bound() {
        reset_debug_sink_metrics_for_test();
        let root = tempfile::tempdir().expect("owned sink root");
        let valid_entry = SwitchDebugEntry {
            run_id: "test-run".into(),
            sequence: 1,
            event: "terminal.surface.input.accepted".into(),
            wall_time_ms: 1000.0,
            details: serde_json::json!({"operationId": "req-1"}),
        };
        append_switch_debug_entry(root.path(), SWITCH_DEBUG_FILE, &valid_entry).expect("append within bound");

        let oversized_text = "x".repeat(MAX_ENTRY_BYTES + 100);
        let oversized_entry = SwitchDebugEntry {
            run_id: "test-run".into(),
            sequence: 2,
            event: "terminal.surface.input.accepted".into(),
            wall_time_ms: 1001.0,
            details: serde_json::json!({"operationId": oversized_text}),
        };
        let err = append_switch_debug_entry(root.path(), SWITCH_DEBUG_FILE, &oversized_entry);
        assert!(err.is_err());
        assert_eq!(debug_sink_drop_count(), 1);
        close_cached_switch_debug_files();
        root.close().expect("remove owned sink");
    }

    #[test]
    fn pane_liveness_diagnostics_sanitization_removes_sensitive_payloads() {
        let raw = serde_json::json!({
            "operationId": "req-42",
            "text": "my-secret-password",
            "keyEvent": {"key": "Enter", "code": 13},
            "clipboard": "sensitive clipboard content",
            "command": "rm -rf /",
            "preedit": "korean-text",
            "error": "Error: /Users/indo/secret/file.rs:42: failed",
            "nested": {
                "command": "sh -c evil",
                "operationId": "nested-req",
            },
            "backendSessionId": "sess-uuid-1",
        });
        let sanitized = sanitize_switch_debug_details("terminal.surface.input.accepted", &raw);
        assert_eq!(sanitized.get("operationId").and_then(Value::as_str), Some("req-42"));
        assert_eq!(sanitized.get("backendSessionId").and_then(Value::as_str), Some("sess-uuid-1"));
        assert_eq!(sanitized.get("text"), None);
        assert_eq!(sanitized.get("textLength").and_then(Value::as_u64), Some(18));
        assert_eq!(sanitized.get("keyEvent"), None);
        assert_eq!(sanitized.get("keyEventPresent").and_then(Value::as_bool), Some(true));
        assert_eq!(sanitized.get("clipboard"), None);
        assert_eq!(sanitized.get("clipboardLength").and_then(Value::as_u64), Some(27));
        assert_eq!(sanitized.get("command"), None);
        assert_eq!(sanitized.get("commandLength").and_then(Value::as_u64), Some(8));
        assert_eq!(sanitized.get("preedit"), None);
        assert_eq!(sanitized.get("preeditLength").and_then(Value::as_u64), Some(11));
        assert_eq!(sanitized.get("errorCode").and_then(Value::as_str), Some("UNKNOWN_ERROR"));
        let nested = sanitized.get("nested").and_then(Value::as_object).unwrap();
        assert_eq!(nested.get("command"), None);
        assert_eq!(nested.get("commandLength").and_then(Value::as_u64), Some(10));
        assert_eq!(nested.get("operationId").and_then(Value::as_str), Some("nested-req"));
    }

    #[test]
    fn pane_liveness_diagnostics_held_writer_precedence() {
        let held_snapshot = PaneLivenessSnapshot {
            telemetry_available: true,
            session_id: Some("session-barrier".into()),
            stage: Some("backend_write_start".into()),
            executing_running_age_ms: Some(300),
            queued_head_age_ms: Some(500),
            ..Default::default()
        };
        assert_eq!(
            classify_pane_liveness(&held_snapshot),
            PaneLivenessVerdict::BlockedInIpcWrite
        );

        let released_snapshot = PaneLivenessSnapshot {
            telemetry_available: true,
            session_id: Some("session-barrier".into()),
            stage: None,
            executing_running_age_ms: None,
            queued_head_age_ms: None,
            queued_count: Some(0),
            reader_paused: Some(false),
            kernel_stopped: Some(false),
            suspended: Some(false),
            has_unpresented_frames: Some(false),
            ..Default::default()
        };
        assert_eq!(
            classify_pane_liveness(&released_snapshot),
            PaneLivenessVerdict::Idle
        );
    }

    #[test]
    fn pane_liveness_diagnostics_prearmed_held_reader_barrier() {
        let paused_snapshot = PaneLivenessSnapshot {
            telemetry_available: true,
            session_id: Some("session-reader-barrier".into()),
            reader_paused: Some(true),
            queued_head_age_ms: Some(400),
            ..Default::default()
        };
        assert_eq!(
            classify_pane_liveness(&paused_snapshot),
            PaneLivenessVerdict::BlockedInReaderPaused
        );

        let resumed_snapshot = PaneLivenessSnapshot {
            telemetry_available: true,
            session_id: Some("session-reader-barrier".into()),
            reader_paused: Some(false),
            queued_head_age_ms: Some(400),
            ..Default::default()
        };
        assert_eq!(
            classify_pane_liveness(&resumed_snapshot),
            PaneLivenessVerdict::BlockedInQueue
        );
    }


    #[test]
    fn pane_liveness_diagnostics_full_wire_envelope_codec() {
        let legacy_wire = r#"{
            "type": "describeSessionOk",
            "session": {
                "sessionId": "legacy-session-1",
                "cols": 80,
                "rows": 24,
                "running": true
            }
        }"#;
        let decoded_legacy: DaemonResponse =
            serde_json::from_str(legacy_wire).expect("decode legacy DaemonResponse envelope");
        match decoded_legacy {
            DaemonResponse::DescribeSessionOk { session, .. } => {
                assert_eq!(session.session_id, "legacy-session-1");
                assert_eq!(session.running, true);
                assert_eq!(session.suspended, false);
                assert_eq!(session.reader_paused, None);
                assert_eq!(session.kernel_stopped, None);
                assert_eq!(session.registry_suspended, None);
                assert_eq!(session.suspension_source, None);
            }
            _ => panic!("expected DescribeSessionOk"),
        }

        let new_wire = r#"{
            "type": "describeSessionOk",
            "session": {
                "sessionId": "new-session-1",
                "cols": 120,
                "rows": 40,
                "running": true,
                "readerPaused": true,
                "kernelStopped": false,
                "registrySuspended": true,
                "suspensionSource": "unknown"
            }
        }"#;
        let decoded_new: DaemonResponse =
            serde_json::from_str(new_wire).expect("decode new DaemonResponse envelope with diagnostics");
        match decoded_new {
            DaemonResponse::DescribeSessionOk { session, .. } => {
                assert_eq!(session.session_id, "new-session-1");
                assert_eq!(session.reader_paused, Some(true));
                assert_eq!(session.kernel_stopped, Some(false));
                assert_eq!(session.registry_suspended, Some(true));
                assert_eq!(session.suspension_source.as_deref(), Some("unknown"));
            }
            _ => panic!("expected DescribeSessionOk"),
        }

        let details = DaemonSessionDetails::new(
            "sess-compat".into(),
            Some("ws-1".into()),
            None,
            Some("/path".into()),
            80,
            24,
            true,
            Some(1),
            Some(10),
            None,
            false,
        );
        let resp = DaemonResponse::DescribeSessionOk { session: details, daemon_epoch: None };
        let serialized_wire = serde_json::to_string(&resp).expect("serialize DaemonResponse envelope");
        assert!(!serialized_wire.contains("readerPaused"));
        assert!(!serialized_wire.contains("kernelStopped"));
        assert!(!serialized_wire.contains("registrySuspended"));
        assert!(!serialized_wire.contains("suspensionSource"));
    }
}
