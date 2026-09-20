use crate::dag::journal::{
    list_run_summaries, parse_run_checkpoint, resolve_dag_runs_dir, DagRunSnapshot, DagRunSummary,
};
use crate::ipc::error::{IpcError, IpcErrorCode};
use crate::ipc::run_blocking;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::{AppHandle, Emitter, State};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DagRunUpdatedPayload {
    pub project_path: String,
    pub generation: u64,
    pub snapshot: DagRunSnapshot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DagWatchFailureCode {
    HelperMissing,
    CapabilityMissing,
    Authentication,
    Unavailable,
}

impl std::fmt::Display for DagWatchFailureCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HelperMissing => write!(f, "helper_missing"),
            Self::CapabilityMissing => write!(f, "capability_missing"),
            Self::Authentication => write!(f, "authentication"),
            Self::Unavailable => write!(f, "unavailable"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DagWatchFailurePayload {
    pub project_path: String,
    pub host_id: String,
    pub code: DagWatchFailureCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DagWatchProjectResponse {
    pub project_path: String,
    pub generation: Option<u64>,
    pub runs: Vec<DagRunSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<DagWatchFailurePayload>,
}

struct ActiveWatcher {
    generation: u64,
    cancel_tx: tokio::sync::watch::Sender<bool>,
}

static NEXT_WATCHER_GENERATION: AtomicU64 = AtomicU64::new(1);

fn dag_active_watchers() -> &'static Mutex<HashMap<String, ActiveWatcher>> {
    static ACTIVE_WATCHERS: OnceLock<Mutex<HashMap<String, ActiveWatcher>>> = OnceLock::new();
    ACTIVE_WATCHERS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn register_watcher(key: &str) -> Option<(u64, tokio::sync::watch::Receiver<bool>)> {
    let mut watchers = dag_active_watchers()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if watchers.contains_key(key) {
        return None;
    }
    let generation = NEXT_WATCHER_GENERATION.fetch_add(1, Ordering::Relaxed);
    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
    watchers.insert(
        key.to_string(),
        ActiveWatcher {
            generation,
            cancel_tx,
        },
    );
    dag_watched_roots()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(key.to_string());
    Some((generation, cancel_rx))
}

fn unregister_watcher(key: &str, expected_generation: u64) {
    let mut watchers = dag_active_watchers()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(entry) = watchers.get(key) {
        if entry.generation == expected_generation {
            watchers.remove(key);
            dag_watched_roots()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .remove(key);
        }
    }
}

fn watcher_generation(key: &str) -> Option<u64> {
    dag_active_watchers()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(key)
        .map(|watcher| watcher.generation)
}

pub fn dag_watched_roots() -> &'static Mutex<HashSet<String>> {
    static DAG_WATCHED_ROOTS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    DAG_WATCHED_ROOTS.get_or_init(|| Mutex::new(HashSet::new()))
}

fn canonical_project_key(project_path: &str) -> String {
    std::fs::canonicalize(project_path)
        .map(|canonical| canonical.to_string_lossy().to_string())
        .unwrap_or_else(|_| project_path.to_string())
}

fn load_current_snapshots(project_path: &str) -> Vec<DagRunSnapshot> {
    let runs_dir = resolve_dag_runs_dir(Path::new(project_path));
    if !runs_dir.is_dir() {
        return Vec::new();
    }
    let mut snapshots = Vec::new();
    let Ok(entries) = std::fs::read_dir(&runs_dir) else {
        return snapshots;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|ext| ext == "json") {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(snapshot) = parse_run_checkpoint(&content) {
                    snapshots.push(snapshot);
                }
            }
        }
    }
    snapshots.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    snapshots
}

/// Registers a per-project dag journal watcher (idempotent per canonical path) and
/// returns the current run inventory and canonical project path so the UI can
/// hydrate immediately.
#[tauri::command]
pub async fn dag_watch_project<R: tauri::Runtime>(
    project_path: String,
    app: AppHandle<R>,
) -> Result<DagWatchProjectResponse, IpcError> {
    let canonical = run_blocking(move || Ok(canonical_project_key(&project_path))).await?;
    let registration = register_watcher(&canonical);
    let response_generation = registration
        .as_ref()
        .map(|(generation, _)| *generation)
        .or_else(|| watcher_generation(&canonical));
    if let Some((generation, mut cancel_rx)) = registration {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<(String, DagRunSnapshot)>(100);
        crate::dag::watcher::spawn_dag_watcher(PathBuf::from(&canonical), tx);
        let watched_key = canonical.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::select! {
                    changed = cancel_rx.changed() => {
                        if changed.is_err() || *cancel_rx.borrow() {
                            break;
                        }
                    }
                    maybe_item = rx.recv() => {
                        match maybe_item {
                            Some((tagged, snapshot)) => {
                                let payload = DagRunUpdatedPayload {
                                    project_path: tagged,
                                    generation,
                                    snapshot,
                                };
                                if let Err(error) = app.emit("dag-run-updated", &payload) {
                                    tracing::debug!("Failed to emit dag-run-updated event: {error}");
                                }
                            }
                            None => break,
                        }
                    }
                }
            }
            unregister_watcher(&watched_key, generation);
        });
    }
    let snapshot_project = canonical.clone();
    let runs = run_blocking(move || Ok(load_current_snapshots(&snapshot_project))).await?;
    Ok(DagWatchProjectResponse {
        project_path: canonical,
        generation: response_generation,
        runs,
        failure: None,
    })
}

#[tauri::command]
pub async fn dag_watch_paired_project<R: tauri::Runtime>(
    workspace_id: String,
    remote_path: String,
    app: AppHandle<R>,
    daemon_client: State<'_, Arc<crate::daemon::client::DaemonClient>>,
) -> Result<DagWatchProjectResponse, IpcError> {
    use tauri::Manager;

    let synthetic_key = format!("paired:{workspace_id}:{remote_path}");

    // The subscription is bound to the paired host that owns this workspace, not to
    // the desktop-side path string. Without a stored paired project there is no
    // authenticated identity to bind, so no watcher is registered at all.
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| IpcError::internal(e.to_string()))?;
    let lookup_id = workspace_id.clone();
    let stored = run_blocking(move || {
        Ok(crate::paired_host::projects::resolve_stored_project(
            &data_dir, &lookup_id,
        ))
    })
    .await?
    .ok_or_else(|| {
        IpcError::new(
            IpcErrorCode::WorkspaceNotFound,
            "Paired daemon project not found. Re-select or re-pair this project.",
        )
    })?;
    let crate::scoped_contracts::RunTarget::PairedDaemon { host_id } = stored.target.clone() else {
        return Err(IpcError::new(
            IpcErrorCode::WorkspaceNotFound,
            "Project is not a paired daemon project.",
        ));
    };
    let host = daemon_client
        .paired_host_list()
        .await
        .map_err(|e| IpcError::internal(e.code))?
        .into_iter()
        .find(|candidate| candidate.host_id == host_id)
        .ok_or_else(|| {
            IpcError::internal(format!("Paired machine '{host_id}' not found in inventory"))
        })?;
    if host.auth_status != crate::paired_host::inventory::AuthStatus::Paired {
        return Err(IpcError::internal(
            "Machine authorization required for paired host",
        ));
    }
    let binding_host_id = host.host_id.clone();
    let binding_workspace = stored.remote_workspace_id.clone();

    let registration = register_watcher(&synthetic_key);
    let response_generation = registration
        .as_ref()
        .map(|(generation, _)| *generation)
        .or_else(|| watcher_generation(&synthetic_key));

    if let Some((generation, mut cancel_rx)) = registration {
        let watched_key = synthetic_key.clone();
        let app_clone = app.clone();
        let client = Arc::clone(&daemon_client);
        let ws_id = workspace_id.clone();
        let rpath = remote_path.clone();

        tauri::async_runtime::spawn(async move {
            let mut retry_delay = tokio::time::Duration::from_millis(1000);
            'outer: loop {
                if *cancel_rx.borrow() {
                    break 'outer;
                }

                // Identity is pinned to (host, remote workspace); the grant generation
                // is re-resolved per attempt because a re-pair rotates it and would
                // otherwise strand this subscription on a retired credential. A host
                // that is gone or no longer machine-granted is not substituted.
                let current = tokio::select! {
                    biased;
                    _ = cancel_rx.changed() => break 'outer,
                    hosts = client.paired_host_list() => hosts,
                };
                let binding = match current.ok().and_then(|hosts| {
                    hosts
                        .into_iter()
                        .find(|candidate| candidate.host_id == binding_host_id)
                        .filter(|candidate| {
                            candidate.auth_status
                                == crate::paired_host::inventory::AuthStatus::Paired
                        })
                }) {
                    Some(host) => crate::daemon::protocol::PairedDagBinding {
                        host_id: binding_host_id.clone(),
                        generation: host.generation,
                        remote_workspace_id: binding_workspace.clone(),
                    },
                    None => {
                        tracing::debug!(workspace_id = %ws_id, "Paired host no longer authorized for DAG streaming");
                        tokio::select! {
                            biased;
                            _ = cancel_rx.changed() => break 'outer,
                            _ = tokio::time::sleep(retry_delay) => {}
                        }
                        retry_delay =
                            std::cmp::min(retry_delay * 2, tokio::time::Duration::from_secs(10));
                        continue;
                    }
                };

                // Connecting is itself cancellable: an unwatch during the handshake
                // must not be deferred until the subscription completes.
                let attempt = tokio::select! {
                    biased;
                    _ = cancel_rx.changed() => break 'outer,
                    attempt = client.subscribe_dag_bound(&ws_id, &rpath, Some(binding)) => attempt,
                };

                match attempt {
                    Ok(mut rx) => {
                        retry_delay = tokio::time::Duration::from_millis(1000);
                        loop {
                            // Biased on cancellation so a queued frame can never be
                            // emitted after unwatch under this watcher generation.
                            tokio::select! {
                                biased;
                                changed = cancel_rx.changed() => {
                                    if changed.is_err() || *cancel_rx.borrow() {
                                        break 'outer;
                                    }
                                }
                                maybe_msg = rx.recv() => {
                                    match maybe_msg {
                                        Some(msg) => {
                                            match msg {
                                                crate::daemon::protocol::DaemonStreamMessage::DagInventory {
                                                    runs,
                                                    ..
                                                } => {
                                                    let key = format!("paired:{ws_id}:{rpath}");
                                                    for snapshot in runs {
                                                        let payload = DagRunUpdatedPayload {
                                                            project_path: key.clone(),
                                                            generation,
                                                            snapshot,
                                                        };
                                                        let _ = app_clone.emit("dag-run-updated", &payload);
                                                    }
                                                }
                                                crate::daemon::protocol::DaemonStreamMessage::DagRunUpdated {
                                                    snapshot,
                                                    ..
                                                } => {
                                                    let key = format!("paired:{ws_id}:{rpath}");
                                                    let payload = DagRunUpdatedPayload {
                                                        project_path: key,
                                                        generation,
                                                        snapshot: *snapshot,
                                                    };
                                                    let _ = app_clone.emit("dag-run-updated", &payload);
                                                }
                                                _ => {}
                                            }
                                        }
                                        None => {
                                            tracing::info!(workspace_id = %ws_id, "Paired DAG stream disconnected, will reconnect");
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(error) => {
                        tracing::warn!(%error, workspace_id = %ws_id, "Failed to subscribe to paired DAG stream, will retry");
                    }
                }

                tokio::select! {
                    changed = cancel_rx.changed() => {
                        if changed.is_err() || *cancel_rx.borrow() {
                            break 'outer;
                        }
                    }
                    _ = tokio::time::sleep(retry_delay) => {}
                }
                retry_delay = std::cmp::min(retry_delay * 2, tokio::time::Duration::from_secs(10));
            }

            unregister_watcher(&watched_key, generation);
        });
    }

    Ok(DagWatchProjectResponse {
        project_path: synthetic_key,
        generation: response_generation,
        runs: Vec::new(),
        failure: None,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SshWatchFailureKind {
    HelperMissing(String),
    CapabilityMissing(String),
    Authentication(String),
    Transport(String),
    Other(String),
}

impl SshWatchFailureKind {
    pub fn to_code(&self) -> DagWatchFailureCode {
        match self {
            Self::HelperMissing(_) => DagWatchFailureCode::HelperMissing,
            Self::CapabilityMissing(_) => DagWatchFailureCode::CapabilityMissing,
            Self::Authentication(_) => DagWatchFailureCode::Authentication,
            Self::Transport(_) | Self::Other(_) => DagWatchFailureCode::Unavailable,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::HelperMissing(msg)
            | Self::CapabilityMissing(msg)
            | Self::Authentication(msg)
            | Self::Transport(msg)
            | Self::Other(msg) => msg,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SshWatchDecision {
    Cancel,
    Retry {
        consecutive_terminal_failures: u32,
        consecutive_transport_failures: u32,
    },
    Terminal(DagWatchFailurePayload),
}

pub const DEFAULT_TERMINAL_FAILURE_BUDGET: u32 = 3;
pub const DEFAULT_TRANSPORT_FAILURE_BUDGET: u32 = 10;

pub(crate) fn classify_ssh_ipc_error(err: &IpcError) -> SshWatchFailureKind {
    let stage = err
        .details
        .as_ref()
        .and_then(|d| d.get("stage"))
        .and_then(|s| s.as_str())
        .unwrap_or("");
    let stderr = err
        .details
        .as_ref()
        .and_then(|d| d.get("stderr"))
        .and_then(|s| s.as_str())
        .unwrap_or("");

    if err.code == IpcErrorCode::CliExecutableNotFound
        || stage == "helper_missing"
        || stage == "helper_permissions"
        || err.message.contains("FERRYX_ERR_HELPER_MISSING")
        || err.message.contains("FERRYX_ERR_HELPER_NOT_EXECUTABLE")
        || stderr.contains("FERRYX_ERR_HELPER_MISSING")
        || stderr.contains("FERRYX_ERR_HELPER_NOT_EXECUTABLE")
    {
        return SshWatchFailureKind::HelperMissing(err.message.clone());
    }

    let lower_msg = err.message.to_lowercase();
    let lower_stderr = stderr.to_lowercase();
    if err.code == IpcErrorCode::Unauthorized
        || err.code == IpcErrorCode::PermissionDenied
        || lower_msg.contains("permission denied")
        || lower_msg.contains("authentication failed")
        || lower_msg.contains("host key verification failed")
        || lower_stderr.contains("permission denied")
        || lower_stderr.contains("host key verification failed")
        || lower_stderr.contains("authentication failed")
    {
        return SshWatchFailureKind::Authentication(err.message.clone());
    }

    if stage == "transport"
        || err.code == IpcErrorCode::Timeout
        || err.code == IpcErrorCode::HostUnavailable
        || err.code == IpcErrorCode::IoError
        || lower_msg.contains("connection closed")
        || lower_msg.contains("connection refused")
        || lower_msg.contains("timed out")
        || lower_msg.contains("ssh operation timed out")
    {
        return SshWatchFailureKind::Transport(err.message.clone());
    }

    SshWatchFailureKind::Other(err.message.clone())
}

pub(crate) fn classify_bridge_error(err: &crate::ssh::bridge::BridgeError) -> SshWatchFailureKind {
    match err {
        crate::ssh::bridge::BridgeError::SshSetup(ipc_err) => classify_ssh_ipc_error(ipc_err),
        crate::ssh::bridge::BridgeError::ProcessExited { code, stderr } => {
            let lower_stderr = stderr.to_lowercase();
            if *code == Some(127) || stderr.contains("FERRYX_ERR_HELPER_MISSING") {
                SshWatchFailureKind::HelperMissing(format!(
                    "Helper process missing (exit 127): {stderr}"
                ))
            } else if *code == Some(126) || stderr.contains("FERRYX_ERR_HELPER_NOT_EXECUTABLE") {
                SshWatchFailureKind::HelperMissing(format!(
                    "Helper process not executable (exit 126): {stderr}"
                ))
            } else if *code == Some(255)
                && (lower_stderr.contains("permission denied")
                    || lower_stderr.contains("host key verification failed")
                    || lower_stderr.contains("authentication failed"))
            {
                SshWatchFailureKind::Authentication(format!(
                    "SSH authentication failed: {stderr}"
                ))
            } else {
                SshWatchFailureKind::Transport(format!(
                    "SSH process exited ({code:?}): {stderr}"
                ))
            }
        }
        crate::ssh::bridge::BridgeError::HostMismatch { expected, actual } => {
            SshWatchFailureKind::Authentication(format!(
                "Host mismatch: expected {expected}, actual {actual}"
            ))
        }
        crate::ssh::bridge::BridgeError::Timeout(msg) => {
            SshWatchFailureKind::Transport(format!("Operation timed out: {msg}"))
        }
        crate::ssh::bridge::BridgeError::ConnectionClosed => {
            SshWatchFailureKind::Transport("Connection closed".into())
        }
        crate::ssh::bridge::BridgeError::Io(io_err) => {
            SshWatchFailureKind::Transport(io_err.to_string())
        }
        crate::ssh::bridge::BridgeError::ProcessSpawn(msg) => {
            SshWatchFailureKind::Transport(format!("Process spawn failed: {msg}"))
        }
        other => SshWatchFailureKind::Other(other.to_string()),
    }
}

pub(crate) fn decide_ssh_watch_step(
    cancelled: bool,
    failure: &SshWatchFailureKind,
    consecutive_terminal_failures: u32,
    consecutive_transport_failures: u32,
    host_id: &str,
    project_path: &str,
    generation: Option<u64>,
) -> SshWatchDecision {
    decide_ssh_watch_step_with_budget(
        cancelled,
        failure,
        consecutive_terminal_failures,
        consecutive_transport_failures,
        host_id,
        project_path,
        generation,
        DEFAULT_TERMINAL_FAILURE_BUDGET,
        DEFAULT_TRANSPORT_FAILURE_BUDGET,
    )
}

pub(crate) fn decide_ssh_watch_step_with_budget(
    cancelled: bool,
    failure: &SshWatchFailureKind,
    consecutive_terminal_failures: u32,
    consecutive_transport_failures: u32,
    host_id: &str,
    project_path: &str,
    generation: Option<u64>,
    terminal_budget: u32,
    transport_budget: u32,
) -> SshWatchDecision {
    if cancelled {
        return SshWatchDecision::Cancel;
    }

    match failure {
        SshWatchFailureKind::HelperMissing(msg)
        | SshWatchFailureKind::CapabilityMissing(msg)
        | SshWatchFailureKind::Authentication(msg) => {
            let next_terminal = consecutive_terminal_failures + 1;
            if next_terminal >= terminal_budget {
                SshWatchDecision::Terminal(DagWatchFailurePayload {
                    project_path: project_path.to_string(),
                    host_id: host_id.to_string(),
                    code: failure.to_code(),
                    message: msg.clone(),
                    generation,
                })
            } else {
                SshWatchDecision::Retry {
                    consecutive_terminal_failures: next_terminal,
                    consecutive_transport_failures: 0,
                }
            }
        }
        SshWatchFailureKind::Transport(msg) => {
            let next_transport = consecutive_transport_failures + 1;
            if next_transport >= transport_budget {
                SshWatchDecision::Terminal(DagWatchFailurePayload {
                    project_path: project_path.to_string(),
                    host_id: host_id.to_string(),
                    code: DagWatchFailureCode::Unavailable,
                    message: format!(
                        "Repeated transport failure ({next_transport} attempts): {msg}"
                    ),
                    generation,
                })
            } else {
                SshWatchDecision::Retry {
                    consecutive_terminal_failures: 0,
                    consecutive_transport_failures: next_transport,
                }
            }
        }
        SshWatchFailureKind::Other(msg) => {
            let next_transport = consecutive_transport_failures + 1;
            if next_transport >= transport_budget {
                SshWatchDecision::Terminal(DagWatchFailurePayload {
                    project_path: project_path.to_string(),
                    host_id: host_id.to_string(),
                    code: DagWatchFailureCode::Unavailable,
                    message: msg.clone(),
                    generation,
                })
            } else {
                SshWatchDecision::Retry {
                    consecutive_terminal_failures: 0,
                    consecutive_transport_failures: next_transport,
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SshCapabilityRetryAction {
    Retry,
    Cancel,
}

pub(crate) async fn wait_ssh_capability_retry(
    cancel_rx: &mut tokio::sync::watch::Receiver<bool>,
    retry_delay: &mut tokio::time::Duration,
    max_retry_delay: tokio::time::Duration,
) -> SshCapabilityRetryAction {
    if *cancel_rx.borrow() {
        return SshCapabilityRetryAction::Cancel;
    }
    tokio::select! {
        biased;
        changed = cancel_rx.changed() => {
            if changed.is_err() || *cancel_rx.borrow() {
                return SshCapabilityRetryAction::Cancel;
            }
        }
        _ = tokio::time::sleep(*retry_delay) => {}
    }
    *retry_delay = std::cmp::min(*retry_delay * 2, max_retry_delay);
    SshCapabilityRetryAction::Retry
}

#[tauri::command]
pub async fn dag_watch_ssh_project<R: tauri::Runtime>(
    workspace_id: String,
    remote_path: String,
    app: AppHandle<R>,
) -> Result<DagWatchProjectResponse, IpcError> {
    let synthetic_key = format!("ssh:{workspace_id}:{remote_path}");
    let registration = register_watcher(&synthetic_key);
    let response_generation = registration
        .as_ref()
        .map(|(generation, _)| *generation)
        .or_else(|| watcher_generation(&synthetic_key));

    if let Some((generation, mut cancel_rx)) = registration {
        let watched_key = synthetic_key.clone();
        let app_clone = app.clone();
        let ws_id = workspace_id.clone();

        tauri::async_runtime::spawn(async move {
            let mut retry_delay = tokio::time::Duration::from_millis(1500);
            let mut consecutive_terminal_failures = 0u32;
            let mut consecutive_transport_failures = 0u32;

            'outer: loop {
                if *cancel_rx.borrow() {
                    break 'outer;
                }

                let store = match crate::ipc::ssh::get_ssh_store_path(&app_clone) {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!(error = %e, "Failed to get SSH store path for DAG watch, will retry");
                        let failure = classify_ssh_ipc_error(&e);
                        match decide_ssh_watch_step(
                            *cancel_rx.borrow(),
                            &failure,
                            consecutive_terminal_failures,
                            consecutive_transport_failures,
                            &ws_id,
                            &watched_key,
                            Some(generation),
                        ) {
                            SshWatchDecision::Cancel => break 'outer,
                            SshWatchDecision::Terminal(payload) => {
                                let _ = app_clone.emit("dag-watch-status", &payload);
                                break 'outer;
                            }
                            SshWatchDecision::Retry {
                                consecutive_terminal_failures: term,
                                consecutive_transport_failures: trans,
                            } => {
                                consecutive_terminal_failures = term;
                                consecutive_transport_failures = trans;
                            }
                        }
                        tokio::select! {
                            changed = cancel_rx.changed() => {
                                if changed.is_err() || *cancel_rx.borrow() {
                                    break 'outer;
                                }
                            }
                            _ = tokio::time::sleep(retry_delay) => {}
                        }
                        retry_delay =
                            std::cmp::min(retry_delay * 2, tokio::time::Duration::from_secs(10));
                        continue;
                    }
                };

                let ws_lookup = ws_id.clone();
                let resolved = crate::ipc::run_blocking(move || {
                    crate::ssh::projects::resolve(&store, &ws_lookup)
                })
                .await;

                let (project, host) = match resolved {
                    Ok(pair) => pair,
                    Err(e) => {
                        tracing::warn!(error = %e, workspace_id = %ws_id, "Failed to resolve SSH project for DAG watch, will retry");
                        let failure = classify_ssh_ipc_error(&e);
                        match decide_ssh_watch_step(
                            *cancel_rx.borrow(),
                            &failure,
                            consecutive_terminal_failures,
                            consecutive_transport_failures,
                            &ws_id,
                            &watched_key,
                            Some(generation),
                        ) {
                            SshWatchDecision::Cancel => break 'outer,
                            SshWatchDecision::Terminal(payload) => {
                                let _ = app_clone.emit("dag-watch-status", &payload);
                                break 'outer;
                            }
                            SshWatchDecision::Retry {
                                consecutive_terminal_failures: term,
                                consecutive_transport_failures: trans,
                            } => {
                                consecutive_terminal_failures = term;
                                consecutive_transport_failures = trans;
                            }
                        }
                        tokio::select! {
                            changed = cancel_rx.changed() => {
                                if changed.is_err() || *cancel_rx.borrow() {
                                    break 'outer;
                                }
                            }
                            _ = tokio::time::sleep(retry_delay) => {}
                        }
                        retry_delay =
                            std::cmp::min(retry_delay * 2, tokio::time::Duration::from_secs(10));
                        continue;
                    }
                };

                let detected = tokio::select! {
                    biased;
                    _ = cancel_rx.changed() => break 'outer,
                    result = crate::ssh::runtime::detect(&host) => result,
                };
                let environment = match detected {
                    Ok(env) => env,
                    Err(e) => {
                        tracing::warn!(error = %e, host_id = %host.id, "Failed to detect SSH runtime for DAG watch, will retry");
                        let failure = classify_ssh_ipc_error(&e);
                        match decide_ssh_watch_step(
                            *cancel_rx.borrow(),
                            &failure,
                            consecutive_terminal_failures,
                            consecutive_transport_failures,
                            &host.id,
                            &watched_key,
                            Some(generation),
                        ) {
                            SshWatchDecision::Cancel => break 'outer,
                            SshWatchDecision::Terminal(payload) => {
                                let _ = app_clone.emit("dag-watch-status", &payload);
                                break 'outer;
                            }
                            SshWatchDecision::Retry {
                                consecutive_terminal_failures: term,
                                consecutive_transport_failures: trans,
                            } => {
                                consecutive_terminal_failures = term;
                                consecutive_transport_failures = trans;
                            }
                        }
                        tokio::select! {
                            changed = cancel_rx.changed() => {
                                if changed.is_err() || *cancel_rx.borrow() {
                                    break 'outer;
                                }
                            }
                            _ = tokio::time::sleep(retry_delay) => {}
                        }
                        retry_delay =
                            std::cmp::min(retry_delay * 2, tokio::time::Duration::from_secs(10));
                        continue;
                    }
                };

                let location = match crate::ssh::helper_setup::default_location(&host, &environment)
                {
                    Ok(loc) => loc,
                    Err(e) => {
                        tracing::warn!(error = %e, host_id = %host.id, "Failed to resolve helper location for DAG watch, will retry");
                        let failure = classify_ssh_ipc_error(&e);
                        match decide_ssh_watch_step(
                            *cancel_rx.borrow(),
                            &failure,
                            consecutive_terminal_failures,
                            consecutive_transport_failures,
                            &host.id,
                            &watched_key,
                            Some(generation),
                        ) {
                            SshWatchDecision::Cancel => break 'outer,
                            SshWatchDecision::Terminal(payload) => {
                                let _ = app_clone.emit("dag-watch-status", &payload);
                                break 'outer;
                            }
                            SshWatchDecision::Retry {
                                consecutive_terminal_failures: term,
                                consecutive_transport_failures: trans,
                            } => {
                                consecutive_terminal_failures = term;
                                consecutive_transport_failures = trans;
                            }
                        }
                        tokio::select! {
                            changed = cancel_rx.changed() => {
                                if changed.is_err() || *cancel_rx.borrow() {
                                    break 'outer;
                                }
                            }
                            _ = tokio::time::sleep(retry_delay) => {}
                        }
                        retry_delay =
                            std::cmp::min(retry_delay * 2, tokio::time::Duration::from_secs(10));
                        continue;
                    }
                };

                let connected = tokio::select! {
                    biased;
                    _ = cancel_rx.changed() => break 'outer,
                    result = crate::ssh::bridge::SshBridgeClient::connect(
                        &host, &environment, &location,
                    ) => result,
                };
                let client = match connected {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::warn!(error = %e, host_id = %host.id, "Failed to connect SshBridgeClient for DAG watch, will retry");
                        let failure = classify_bridge_error(&e);
                        match decide_ssh_watch_step(
                            *cancel_rx.borrow(),
                            &failure,
                            consecutive_terminal_failures,
                            consecutive_transport_failures,
                            &host.id,
                            &watched_key,
                            Some(generation),
                        ) {
                            SshWatchDecision::Cancel => break 'outer,
                            SshWatchDecision::Terminal(payload) => {
                                let _ = app_clone.emit("dag-watch-status", &payload);
                                break 'outer;
                            }
                            SshWatchDecision::Retry {
                                consecutive_terminal_failures: term,
                                consecutive_transport_failures: trans,
                            } => {
                                consecutive_terminal_failures = term;
                                consecutive_transport_failures = trans;
                            }
                        }
                        tokio::select! {
                            changed = cancel_rx.changed() => {
                                if changed.is_err() || *cancel_rx.borrow() {
                                    break 'outer;
                                }
                            }
                            _ = tokio::time::sleep(retry_delay) => {}
                        }
                        retry_delay =
                            std::cmp::min(retry_delay * 2, tokio::time::Duration::from_secs(10));
                        continue;
                    }
                };

                if !client.supports_dag_stream() {
                    tracing::warn!(
                        host_id = %host.id,
                        workspace_id = %ws_id,
                        "SSH helper does not advertise dagSubscribeV1 capability, will retry"
                    );
                    let _ = client.close().await;
                    let failure = SshWatchFailureKind::CapabilityMissing(
                        "SSH helper does not advertise dagSubscribeV1 capability".into(),
                    );
                    match decide_ssh_watch_step(
                        *cancel_rx.borrow(),
                        &failure,
                        consecutive_terminal_failures,
                        consecutive_transport_failures,
                        &host.id,
                        &watched_key,
                        Some(generation),
                    ) {
                        SshWatchDecision::Cancel => break 'outer,
                        SshWatchDecision::Terminal(payload) => {
                            let _ = app_clone.emit("dag-watch-status", &payload);
                            break 'outer;
                        }
                        SshWatchDecision::Retry {
                            consecutive_terminal_failures: term,
                            consecutive_transport_failures: trans,
                        } => {
                            consecutive_terminal_failures = term;
                            consecutive_transport_failures = trans;
                        }
                    }
                    match wait_ssh_capability_retry(
                        &mut cancel_rx,
                        &mut retry_delay,
                        tokio::time::Duration::from_secs(10),
                    )
                    .await
                    {
                        SshCapabilityRetryAction::Retry => continue,
                        SshCapabilityRetryAction::Cancel => break 'outer,
                    }
                }

                retry_delay = tokio::time::Duration::from_millis(1500);

                let stream_result = tokio::select! {
                    biased;
                    _ = cancel_rx.changed() => None,
                    result = async {
                        client.project_register(&project.workspace_id, &project.repo_root).await?;
                        let (mut subscription, mut frame) = client
                            .dag_subscribe(&host, &environment, &location, &project.workspace_id)
                            .await?;
                        loop {
                            for dropped in &frame.dropped {
                                tracing::warn!(?dropped, workspace_id = %ws_id, "SSH DAG checkpoint omitted by helper");
                            }
                            for value in frame.runs {
                                match parse_run_checkpoint(&value.to_string()) {
                                    Ok(snapshot) => {
                                        consecutive_terminal_failures = 0;
                                        consecutive_transport_failures = 0;
                                        let payload = DagRunUpdatedPayload {
                                            project_path: watched_key.clone(),
                                            generation,
                                            snapshot,
                                        };
                                        let _ = app_clone.emit("dag-run-updated", &payload);
                                    }
                                    Err(error) => tracing::warn!(%error, "Invalid SSH DAG checkpoint"),
                                }
                            }
                            if frame.closed {
                                return Err(crate::ssh::bridge::BridgeError::ConnectionClosed);
                            }
                            let wait = if frame.more {
                                tokio::time::Duration::ZERO
                            } else {
                                tokio::time::Duration::from_secs(25)
                            };
                            frame = subscription.next_frame(wait).await?;
                        }
                    } => Some(result),
                };

                let _ = client.close().await;
                match stream_result {
                    None => break 'outer,
                    Some(Err(error)) => {
                        tracing::warn!(%error, workspace_id = %ws_id, "SSH DAG stream failed, will reconnect");
                        let failure = classify_bridge_error(&error);
                        match decide_ssh_watch_step(
                            *cancel_rx.borrow(),
                            &failure,
                            consecutive_terminal_failures,
                            consecutive_transport_failures,
                            &host.id,
                            &watched_key,
                            Some(generation),
                        ) {
                            SshWatchDecision::Cancel => break 'outer,
                            SshWatchDecision::Terminal(payload) => {
                                let _ = app_clone.emit("dag-watch-status", &payload);
                                break 'outer;
                            }
                            SshWatchDecision::Retry {
                                consecutive_terminal_failures: term,
                                consecutive_transport_failures: trans,
                            } => {
                                consecutive_terminal_failures = term;
                                consecutive_transport_failures = trans;
                            }
                        }
                    }
                    Some(Ok::<(), crate::ssh::bridge::BridgeError>(())) => break 'outer,
                }
                tokio::select! {
                    changed = cancel_rx.changed() => {
                        if changed.is_err() || *cancel_rx.borrow() {
                            break 'outer;
                        }
                    }
                    _ = tokio::time::sleep(retry_delay) => {}
                }
                retry_delay = std::cmp::min(retry_delay * 2, tokio::time::Duration::from_secs(10));
            }

            unregister_watcher(&watched_key, generation);
        });
    }

    Ok(DagWatchProjectResponse {
        project_path: synthetic_key,
        generation: response_generation,
        runs: Vec::new(),
        failure: None,
    })
}

#[tauri::command]
pub async fn dag_unwatch_project(project_path: String) -> Result<(), IpcError> {
    let mut watchers = dag_active_watchers()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(entry) = watchers.remove(&project_path) {
        let _ = entry.cancel_tx.send(true);
        dag_watched_roots()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&project_path);
    }
    Ok(())
}

/// Scans one level of sibling directories under each registered project root for
/// repositories that carry a dag journal, so runs rooted in a directory Ferryx never
/// registered (e.g. an agent session resumed from a different cwd) still reach the store.
#[tauri::command]
pub async fn dag_discover_watch_roots(project_roots: Vec<String>) -> Result<Vec<String>, IpcError> {
    run_blocking(move || {
        let mut discovered: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for root in &project_roots {
            let path = PathBuf::from(root);
            if !path.is_absolute() {
                continue;
            }
            let Some(parent) = path.parent() else {
                continue;
            };
            let Ok(entries) = std::fs::read_dir(parent) else {
                continue;
            };
            for entry in entries.flatten() {
                let child = entry.path();
                if !child.is_dir() {
                    continue;
                }
                let Some(name) = child.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                if name.starts_with('.') || name == "node_modules" || name == "target" {
                    continue;
                }
                if !child.join(".omo/senpi-task/dag/runs").is_dir() {
                    continue;
                }
                let key = child.to_string_lossy().to_string();
                if key == *root || !seen.insert(key.clone()) {
                    continue;
                }
                discovered.push(key);
            }
        }
        Ok(discovered)
    })
    .await
}

#[tauri::command]
pub async fn dag_list_runs(project_path: String) -> Result<Vec<DagRunSummary>, IpcError> {
    run_blocking(move || {
        let path = PathBuf::from(&project_path);
        let runs_dir = resolve_dag_runs_dir(&path);
        if !runs_dir.exists() {
            return Ok(Vec::new());
        }
        list_run_summaries(&runs_dir).map_err(|err| match err {
            crate::dag::journal::DagJournalError::Json(_) => {
                IpcError::new(IpcErrorCode::ParseError, err.to_string())
            }
            crate::dag::journal::DagJournalError::Io(_) => {
                IpcError::new(IpcErrorCode::IoError, err.to_string())
            }
            crate::dag::journal::DagJournalError::UnknownWaveNode { .. } => {
                IpcError::new(IpcErrorCode::ParseError, err.to_string())
            }
        })
    })
    .await
}

#[tauri::command]
pub async fn dag_get_run(
    project_path: String,
    run_id: String,
) -> Result<Option<DagRunSnapshot>, IpcError> {
    run_blocking(move || {
        let path = PathBuf::from(&project_path);
        let runs_dir = resolve_dag_runs_dir(&path);
        if !runs_dir.exists() {
            return Ok(None);
        }

        let clean_id = run_id.strip_prefix("dag_").unwrap_or(&run_id);
        let candidates = [
            runs_dir.join(format!("dag_{clean_id}.json")),
            runs_dir.join(format!("{run_id}.json")),
            runs_dir.join(&run_id),
        ];

        for candidate in &candidates {
            if candidate.is_file() {
                let content = std::fs::read_to_string(candidate).map_err(|e| {
                    IpcError::new(IpcErrorCode::IoError, format!("failed to read file: {e}"))
                })?;
                let snapshot = parse_run_checkpoint(&content).map_err(|e| {
                    IpcError::new(
                        IpcErrorCode::ParseError,
                        format!("failed to parse checkpoint: {e}"),
                    )
                })?;
                return Ok(Some(snapshot));
            }
        }

        if let Ok(entries) = std::fs::read_dir(&runs_dir) {
            for entry in entries.flatten() {
                let entry_path = entry.path();
                if entry_path.is_file() && entry_path.extension().is_some_and(|ext| ext == "json") {
                    if let Ok(content) = std::fs::read_to_string(&entry_path) {
                        if let Ok(snapshot) = parse_run_checkpoint(&content) {
                            if snapshot.run_id == run_id || snapshot.run_id == clean_id {
                                return Ok(Some(snapshot));
                            }
                        }
                    }
                }
            }
        }

        Ok(None)
    })
    .await
}

#[tauri::command]
pub async fn dag_read_node_artifact(
    project_path: String,
    relative_path: String,
) -> Result<String, IpcError> {
    run_blocking(move || {
        let trimmed = relative_path.trim();
        if trimmed.is_empty() {
            return Err(IpcError::new(
                IpcErrorCode::InvalidArgument,
                "artifact relative path cannot be empty",
            ));
        }

        let rel = Path::new(trimmed);
        if rel.is_absolute() {
            return Err(IpcError::new(
                IpcErrorCode::InvalidArgument,
                "artifact path must be relative, not absolute",
            ));
        }

        for comp in rel.components() {
            if matches!(comp, std::path::Component::ParentDir) {
                return Err(IpcError::new(
                    IpcErrorCode::PermissionDenied,
                    "parent directory traversal ('..') is strictly forbidden",
                ));
            }
        }

        let canonical_project = std::fs::canonicalize(&project_path).map_err(|e| {
            IpcError::new(
                IpcErrorCode::NotFound,
                format!("failed to canonicalize project path: {e}"),
            )
        })?;

        let candidate = canonical_project.join(rel);
        let candidate_nested = canonical_project.join(".omo/senpi-task").join(rel);

        let target_path = if candidate.is_file() {
            candidate
        } else if candidate_nested.is_file() {
            candidate_nested
        } else {
            return Err(IpcError::new(
                IpcErrorCode::NotFound,
                format!("artifact file not found at: {trimmed}"),
            ));
        };

        let canonical_target = std::fs::canonicalize(&target_path).map_err(|e| {
            IpcError::new(
                IpcErrorCode::IoError,
                format!("failed to canonicalize artifact path: {e}"),
            )
        })?;

        if !canonical_target.starts_with(&canonical_project) {
            return Err(IpcError::new(
                IpcErrorCode::PermissionDenied,
                "artifact path escapes project root boundary",
            ));
        }

        std::fs::read_to_string(&canonical_target).map_err(|e| {
            IpcError::new(
                IpcErrorCode::IoError,
                format!("failed to read artifact file: {e}"),
            )
        })
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_F107: &str =
        include_str!("../dag/testdata/dag_f107f318-ac78-46a2-b8c6-584b4e10eaa7.json");

    #[test]
    fn dag_run_snapshot_forwards_checkpoint_session_ids() {
        let with_session_ids = r#"{
            "runId": "dag-session-ids",
            "runKey": "session-ids",
            "name": "Session ids",
            "status": "running",
            "rootSessionId": "01a055f9-a8de-7619-a1f5-81ca62e3d3b1",
            "parentSessionId": "01a055f9-a8de-7619-a1f5-81ca62e3d3b2",
            "nodes": [],
            "edges": [],
            "waves": [],
            "criticalPath": [],
            "bottlenecks": []
        }"#;
        let without_session_ids = r#"{
            "runId": "dag-no-session-ids",
            "runKey": "no-session-ids",
            "name": "No session ids",
            "status": "completed",
            "nodes": [],
            "edges": [],
            "waves": [],
            "criticalPath": [],
            "bottlenecks": []
        }"#;

        let with_ids = parse_run_checkpoint(with_session_ids).expect("parse checkpoint with ids");
        assert_eq!(
            with_ids.root_session_id.as_deref(),
            Some("01a055f9-a8de-7619-a1f5-81ca62e3d3b1")
        );
        assert_eq!(
            with_ids.parent_session_id.as_deref(),
            Some("01a055f9-a8de-7619-a1f5-81ca62e3d3b2")
        );
        let serialized = serde_json::to_value(&with_ids).expect("serialize checkpoint with ids");
        assert_eq!(
            serialized["rootSessionId"].as_str(),
            Some("01a055f9-a8de-7619-a1f5-81ca62e3d3b1")
        );
        assert_eq!(
            serialized["parentSessionId"].as_str(),
            Some("01a055f9-a8de-7619-a1f5-81ca62e3d3b2")
        );

        let without_ids =
            parse_run_checkpoint(without_session_ids).expect("parse checkpoint without ids");
        assert_eq!(without_ids.root_session_id, None);
        assert_eq!(without_ids.parent_session_id, None);
        let serialized =
            serde_json::to_value(&without_ids).expect("serialize checkpoint without ids");
        assert!(serialized.get("rootSessionId").is_none());
        assert!(serialized.get("parentSessionId").is_none());
    }

    #[tokio::test]
    async fn test_dag_list_runs_empty_for_missing_dir() {
        let temp = tempfile::tempdir().expect("tempdir");
        let result = dag_list_runs(temp.path().to_string_lossy().to_string())
            .await
            .expect("runs list");
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_dag_list_and_get_runs_success() {
        let temp = tempfile::tempdir().expect("tempdir");
        let runs_dir = temp.path().join(".omo/senpi-task/dag/runs");
        std::fs::create_dir_all(&runs_dir).expect("create runs dir");
        let file_path = runs_dir.join("dag_f107f318-ac78-46a2-b8c6-584b4e10eaa7.json");
        std::fs::write(&file_path, FIXTURE_F107).expect("write checkpoint");

        let project_str = temp.path().to_string_lossy().to_string();
        let list = dag_list_runs(project_str.clone()).await.expect("list runs");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].run_id, "dag_f107f318-ac78-46a2-b8c6-584b4e10eaa7");

        let run_direct = dag_get_run(
            project_str.clone(),
            "f107f318-ac78-46a2-b8c6-584b4e10eaa7".into(),
        )
        .await
        .expect("get run");
        assert!(run_direct.is_some());
        assert_eq!(
            run_direct.unwrap().run_id,
            "dag_f107f318-ac78-46a2-b8c6-584b4e10eaa7"
        );

        let run_with_prefix = dag_get_run(
            project_str.clone(),
            "dag_f107f318-ac78-46a2-b8c6-584b4e10eaa7".into(),
        )
        .await
        .expect("get run with prefix");
        assert!(run_with_prefix.is_some());

        let missing = dag_get_run(project_str, "nonexistent".into())
            .await
            .expect("get missing run");
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn test_dag_read_node_artifact_security_and_success() {
        let temp = tempfile::tempdir().expect("tempdir");
        let project_str = temp.path().to_string_lossy().to_string();

        let artifact_dir = temp.path().join(".omo/senpi-task/dag/results/dag_123");
        std::fs::create_dir_all(&artifact_dir).expect("create artifact dir");
        let artifact_file = artifact_dir.join("step_1.txt");
        std::fs::write(&artifact_file, "Hello from deliverable artifact!").expect("write artifact");

        // 1. Successful read
        let content = dag_read_node_artifact(
            project_str.clone(),
            ".omo/senpi-task/dag/results/dag_123/step_1.txt".into(),
        )
        .await
        .expect("read artifact");
        assert_eq!(content, "Hello from deliverable artifact!");

        // 2. Successful read via nested fallback
        let content_nested =
            dag_read_node_artifact(project_str.clone(), "dag/results/dag_123/step_1.txt".into())
                .await
                .expect("read artifact nested");
        assert_eq!(content_nested, "Hello from deliverable artifact!");

        // 3. Reject empty
        let err_empty = dag_read_node_artifact(project_str.clone(), "".into())
            .await
            .unwrap_err();
        assert_eq!(err_empty.code, IpcErrorCode::InvalidArgument);

        // 4. Reject parent directory traversal
        let err_traversal = dag_read_node_artifact(project_str.clone(), "../secret.txt".into())
            .await
            .unwrap_err();
        assert_eq!(err_traversal.code, IpcErrorCode::PermissionDenied);

        // 5. Reject absolute path
        let err_abs = dag_read_node_artifact(project_str.clone(), "/etc/passwd".into())
            .await
            .unwrap_err();
        assert_eq!(err_abs.code, IpcErrorCode::InvalidArgument);

        // 6. NotFound for nonexistent file
        let err_missing =
            dag_read_node_artifact(project_str, "dag/results/dag_123/nonexistent.txt".into())
                .await
                .unwrap_err();
        assert_eq!(err_missing.code, IpcErrorCode::NotFound);
    }

    #[tokio::test]
    async fn test_dag_discover_watch_roots_finds_sibling_dag_repos() {
        let temp = tempfile::tempdir().expect("tempdir");
        let farm = temp.path();
        let alpha = farm.join("alpha");
        let gamma = farm.join("gamma");
        let hidden = farm.join(".hidden");
        for dir in [&alpha, &gamma, &hidden] {
            std::fs::create_dir_all(dir.join(".omo/senpi-task/dag/runs")).expect("create dag dir");
        }
        std::fs::create_dir_all(farm.join("plain")).expect("create plain dir");
        std::fs::write(
            alpha.join(".omo/senpi-task/dag/runs/dag_f107f318-ac78-46a2-b8c6-584b4e10eaa7.json"),
            FIXTURE_F107,
        )
        .expect("write checkpoint");

        let discovered = dag_discover_watch_roots(vec![alpha.to_string_lossy().to_string()])
            .await
            .expect("discover");
        let mut sorted = discovered.clone();
        sorted.sort();
        assert_eq!(sorted, vec![gamma.to_string_lossy().to_string()]);

        let empty = dag_discover_watch_roots(vec![
            ".".to_string(),
            "relative/path".to_string(),
            farm.join("empty-farm")
                .join("missing")
                .to_string_lossy()
                .to_string(),
        ])
        .await
        .expect("discover tolerant");
        assert!(empty.is_empty());

        let mut alpha_discovered = dag_discover_watch_roots(vec![
            gamma.to_string_lossy().to_string(),
            hidden.to_string_lossy().to_string(),
        ])
        .await
        .expect("discover reverse");
        alpha_discovered.sort();
        let mut expected = vec![
            alpha.to_string_lossy().to_string(),
            gamma.to_string_lossy().to_string(),
        ];
        expected.sort();
        assert_eq!(alpha_discovered, expected);
    }

    #[tokio::test]
    async fn test_dag_watch_project_canonical_key_and_dedup() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let (_temp, canonical_path, relative_path) = run_blocking(|| {
            let temp = tempfile::tempdir().expect("tempdir");
            let runs_dir = temp.path().join(".omo/senpi-task/dag/runs");
            std::fs::create_dir_all(&runs_dir).expect("create runs dir");
            std::fs::write(
                runs_dir.join("dag_f107f318-ac78-46a2-b8c6-584b4e10eaa7.json"),
                FIXTURE_F107,
            )
            .expect("write checkpoint");

            let canonical_path = std::fs::canonicalize(temp.path())
                .expect("canonicalize")
                .to_string_lossy()
                .to_string();
            std::fs::create_dir_all(temp.path().join("sub")).expect("create sub");
            let relative_path = format!("{}/sub/..", temp.path().display());

            Ok((temp, canonical_path, relative_path))
        })
        .await
        .expect("set up project");

        let res1 = dag_watch_project(relative_path.clone(), app.handle().clone())
            .await
            .expect("watch project relative");

        assert_eq!(res1.project_path, canonical_path);
        assert_eq!(res1.runs.len(), 1);
        assert_eq!(
            res1.runs[0].run_id,
            "dag_f107f318-ac78-46a2-b8c6-584b4e10eaa7"
        );
        let serialized = serde_json::to_value(&res1).expect("serialize response");
        assert_eq!(
            serialized["projectPath"].as_str(),
            Some(canonical_path.as_str())
        );
        assert!(serialized.get("project_path").is_none());

        let (has_canonical_root, has_relative_root) = {
            let watched = dag_watched_roots()
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            (
                watched.contains(&canonical_path),
                watched.contains(&relative_path),
            )
        };
        assert!(has_canonical_root);
        assert!(!has_relative_root);

        let res2 = dag_watch_project(canonical_path.clone(), app.handle().clone())
            .await
            .expect("watch project canonical");

        assert_eq!(res2.project_path, canonical_path);
        assert_eq!(res2.runs.len(), 1);
        assert!(res1.generation.is_some());
        assert_eq!(res2.generation, res1.generation);
        assert_eq!(
            res2.runs[0].run_id,
            "dag_f107f318-ac78-46a2-b8c6-584b4e10eaa7"
        );
        dag_unwatch_project(canonical_path.clone()).await.unwrap();
        let res3 = dag_watch_project(canonical_path.clone(), app.handle().clone())
            .await
            .expect("rewatch project");
        assert!(res3.generation > res2.generation);
        dag_unwatch_project(canonical_path).await.unwrap();
    }

    #[tokio::test]
    async fn test_dag_unwatch_cancels_active_watcher_and_syncs_roots() {
        let test_key = "test:unwatch:sync".to_string();
        let (gen, cancel_rx) = register_watcher(&test_key).expect("register watcher");
        assert!(!*cancel_rx.borrow());
        assert!(dag_watched_roots().lock().unwrap().contains(&test_key));

        // Attempting duplicate registration fails
        assert!(register_watcher(&test_key).is_none());

        // Unwatch signals cancellation and removes from roots
        dag_unwatch_project(test_key.clone()).await.unwrap();
        assert!(*cancel_rx.borrow());
        assert!(!dag_watched_roots().lock().unwrap().contains(&test_key));

        // Old task exiting with stale generation cannot clobber a new watcher
        let (gen2, _) = register_watcher(&test_key).expect("register again");
        assert_ne!(gen, gen2);
        unregister_watcher(&test_key, gen); // stale gen
        assert!(dag_watched_roots().lock().unwrap().contains(&test_key));

        // Correct generation unregisters cleanly
        unregister_watcher(&test_key, gen2);
        assert!(!dag_watched_roots().lock().unwrap().contains(&test_key));
    }

    #[tokio::test]
    async fn test_ssh_capability_retry_wait_backs_off_and_doubles_delay() {
        tokio::time::pause();

        let (_cancel_tx, mut cancel_rx) = tokio::sync::watch::channel(false);
        let mut retry_delay = tokio::time::Duration::from_millis(1500);
        let max_retry_delay = tokio::time::Duration::from_secs(10);

        // Iteration 1: 1500ms delay -> advances virtual time by at least 1500ms, doubles to 3000ms
        let start = tokio::time::Instant::now();
        let action = wait_ssh_capability_retry(&mut cancel_rx, &mut retry_delay, max_retry_delay).await;
        assert_eq!(action, SshCapabilityRetryAction::Retry);
        assert!(start.elapsed() >= tokio::time::Duration::from_millis(1500));
        assert!(start.elapsed() < tokio::time::Duration::from_millis(1600));
        assert_eq!(retry_delay, tokio::time::Duration::from_millis(3000));

        // Iteration 2: 3000ms delay -> advances virtual time by at least 3000ms, doubles to 6000ms
        let start = tokio::time::Instant::now();
        let action = wait_ssh_capability_retry(&mut cancel_rx, &mut retry_delay, max_retry_delay).await;
        assert_eq!(action, SshCapabilityRetryAction::Retry);
        assert!(start.elapsed() >= tokio::time::Duration::from_millis(3000));
        assert!(start.elapsed() < tokio::time::Duration::from_millis(3100));
        assert_eq!(retry_delay, tokio::time::Duration::from_millis(6000));

        // Iteration 3: 6000ms delay -> advances virtual time by at least 6000ms, capped at 10000ms
        let start = tokio::time::Instant::now();
        let action = wait_ssh_capability_retry(&mut cancel_rx, &mut retry_delay, max_retry_delay).await;
        assert_eq!(action, SshCapabilityRetryAction::Retry);
        assert!(start.elapsed() >= tokio::time::Duration::from_millis(6000));
        assert!(start.elapsed() < tokio::time::Duration::from_millis(6100));
        assert_eq!(retry_delay, tokio::time::Duration::from_secs(10));
    }

    #[tokio::test]
    async fn test_ssh_capability_retry_wait_cancels_in_flight() {
        use std::future::Future;
        tokio::time::pause();

        let (cancel_tx, mut cancel_rx) = tokio::sync::watch::channel(false);
        let mut retry_delay = tokio::time::Duration::from_secs(30);
        let max_retry_delay = tokio::time::Duration::from_secs(60);

        let mut fut = std::pin::pin!(wait_ssh_capability_retry(
            &mut cancel_rx,
            &mut retry_delay,
            max_retry_delay
        ));

        // Poll once to enter Pending state (sleeping)
        std::future::poll_fn(|cx| {
            assert!(fut.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;

        // In-flight signal arrives while sleeping
        cancel_tx.send(true).expect("send cancel");

        // Second poll resolves immediately to Cancel with zero virtual time elapsed
        let start = tokio::time::Instant::now();
        let action = std::future::poll_fn(|cx| fut.as_mut().poll(cx)).await;
        assert_eq!(action, SshCapabilityRetryAction::Cancel);
        assert_eq!(start.elapsed(), tokio::time::Duration::ZERO);
    }

    #[tokio::test]
    async fn test_ssh_capability_retry_wait_cancels_if_already_signaled() {
        let (_cancel_tx, mut cancel_rx) = tokio::sync::watch::channel(true);
        let mut retry_delay = tokio::time::Duration::from_secs(30);
        let max_retry_delay = tokio::time::Duration::from_secs(60);

        let action = wait_ssh_capability_retry(&mut cancel_rx, &mut retry_delay, max_retry_delay).await;
        assert_eq!(action, SshCapabilityRetryAction::Cancel);
    }

    #[test]
    fn test_ssh_watch_decision_helper_missing_becomes_terminal_after_budget() {
        // Given: a helper missing failure on an SSH host
        let host_id = "test-host-1";
        let project_path = "ssh:ws-1:/remote/repo";
        let failure = SshWatchFailureKind::HelperMissing(
            "Remote helper binary is not installed at '/home/user/.ferryx/bin/ferryx-remote-helper'.".into(),
        );

        // Attempt 1: not terminal, retries
        let step1 = decide_ssh_watch_step(false, &failure, 0, 0, host_id, project_path, Some(42));
        assert_eq!(
            step1,
            SshWatchDecision::Retry {
                consecutive_terminal_failures: 1,
                consecutive_transport_failures: 0,
            }
        );

        // Attempt 2: not terminal, retries
        let step2 = decide_ssh_watch_step(false, &failure, 1, 0, host_id, project_path, Some(42));
        assert_eq!(
            step2,
            SshWatchDecision::Retry {
                consecutive_terminal_failures: 2,
                consecutive_transport_failures: 0,
            }
        );

        // Attempt 3: terminal budget (3) reached -> becomes terminal
        let step3 = decide_ssh_watch_step(false, &failure, 2, 0, host_id, project_path, Some(42));
        match step3 {
            SshWatchDecision::Terminal(payload) => {
                assert_eq!(payload.code, DagWatchFailureCode::HelperMissing);
                assert_eq!(payload.host_id, host_id);
                assert_eq!(payload.project_path, project_path);
                assert_eq!(payload.generation, Some(42));
                let json = serde_json::to_string_pretty(&payload).unwrap();
                println!("STRUCTURED PAYLOAD [helper_missing]:\n{json}");
            }
            other => panic!("Expected Terminal decision, got: {:?}", other),
        }
    }

    #[test]
    fn test_ssh_watch_decision_capability_missing_is_terminal() {
        // Given: helper connects but does not advertise dagSubscribeV1
        let host_id = "test-host-macos";
        let project_path = "ssh:ws-2:/remote/repo";
        let failure = SshWatchFailureKind::CapabilityMissing(
            "SSH helper does not advertise dagSubscribeV1 capability".into(),
        );

        // When: consecutive attempts reach terminal budget (3 attempts)
        let decision = decide_ssh_watch_step(false, &failure, 2, 0, host_id, project_path, Some(10));

        // Then: it surfaces structured capability_missing failure
        match decision {
            SshWatchDecision::Terminal(payload) => {
                assert_eq!(payload.code, DagWatchFailureCode::CapabilityMissing);
                assert_eq!(payload.host_id, host_id);
                assert_eq!(payload.project_path, project_path);
                assert_eq!(payload.generation, Some(10));
                let json = serde_json::to_string_pretty(&payload).unwrap();
                println!("STRUCTURED PAYLOAD [capability_missing]:\n{json}");
            }
            other => panic!("Expected Terminal decision for capability_missing, got: {:?}", other),
        }
    }

    #[test]
    fn test_ssh_watch_decision_plain_transport_failure_is_not_terminal() {
        // Given: a plain transport failure (e.g. transient connection drop or timeout)
        let host_id = "test-host-transient";
        let project_path = "ssh:ws-3:/remote/repo";
        let failure = SshWatchFailureKind::Transport("Connection timed out".into());

        // When: plain transport failure occurs on attempt 1, 2, or 3
        for attempt in 0..3 {
            let decision = decide_ssh_watch_step(
                false,
                &failure,
                0,
                attempt,
                host_id,
                project_path,
                Some(1),
            );
            // Then: plain transport failure must NOT be terminal, it must continue retrying
            assert_eq!(
                decision,
                SshWatchDecision::Retry {
                    consecutive_terminal_failures: 0,
                    consecutive_transport_failures: attempt + 1,
                }
            );
        }
    }

    #[test]
    fn test_ssh_watch_decision_cancellation_still_wins() {
        // Given: watcher was cancelled (unwatched)
        let host_id = "test-host-cancel";
        let project_path = "ssh:ws-4:/remote/repo";

        // When: cancellation flag is set, even if a terminal failure condition exists
        let failure1 = SshWatchFailureKind::HelperMissing("Missing helper".into());
        let decision1 = decide_ssh_watch_step(true, &failure1, 5, 0, host_id, project_path, None);
        assert_eq!(decision1, SshWatchDecision::Cancel);

        let failure2 = SshWatchFailureKind::CapabilityMissing("No capability".into());
        let decision2 = decide_ssh_watch_step(true, &failure2, 5, 0, host_id, project_path, None);
        assert_eq!(decision2, SshWatchDecision::Cancel);

        let failure3 = SshWatchFailureKind::Transport("Connection reset".into());
        let decision3 = decide_ssh_watch_step(true, &failure3, 0, 5, host_id, project_path, None);
        assert_eq!(decision3, SshWatchDecision::Cancel);
    }

    #[test]
    fn test_classify_ssh_ipc_and_bridge_errors() {
        // Helper missing by code
        let err_missing = IpcError::new(IpcErrorCode::CliExecutableNotFound, "not found");
        assert_eq!(
            classify_ssh_ipc_error(&err_missing),
            SshWatchFailureKind::HelperMissing("not found".into())
        );

        // Helper missing by stage
        let err_stage_missing = IpcError::new(IpcErrorCode::IoError, "missing")
            .with_details(serde_json::json!({"stage": "helper_missing"}));
        assert_eq!(
            classify_ssh_ipc_error(&err_stage_missing),
            SshWatchFailureKind::HelperMissing("missing".into())
        );

        // Helper permissions by stage
        let err_stage_perm = IpcError::new(IpcErrorCode::Unsupported, "permissions")
            .with_details(serde_json::json!({"stage": "helper_permissions"}));
        assert_eq!(
            classify_ssh_ipc_error(&err_stage_perm),
            SshWatchFailureKind::HelperMissing("permissions".into())
        );

        // Authentication by code
        let err_auth = IpcError::new(IpcErrorCode::Unauthorized, "access denied");
        assert_eq!(
            classify_ssh_ipc_error(&err_auth),
            SshWatchFailureKind::Authentication("access denied".into())
        );

        // Authentication by message
        let err_auth_msg = IpcError::new(IpcErrorCode::IoError, "Permission denied (publickey)");
        assert_eq!(
            classify_ssh_ipc_error(&err_auth_msg),
            SshWatchFailureKind::Authentication("Permission denied (publickey)".into())
        );

        // Transport by stage
        let err_trans = IpcError::new(IpcErrorCode::IoError, "timed out")
            .with_details(serde_json::json!({"stage": "transport"}));
        assert_eq!(
            classify_ssh_ipc_error(&err_trans),
            SshWatchFailureKind::Transport("timed out".into())
        );

        // Bridge process exit 127 (helper missing)
        let bridge_missing = crate::ssh::bridge::BridgeError::ProcessExited {
            code: Some(127),
            stderr: "FERRYX_ERR_HELPER_MISSING".into(),
        };
        assert!(matches!(
            classify_bridge_error(&bridge_missing),
            SshWatchFailureKind::HelperMissing(_)
        ));

        // Bridge process exit 255 (auth failure)
        let bridge_auth = crate::ssh::bridge::BridgeError::ProcessExited {
            code: Some(255),
            stderr: "Permission denied (publickey,password)".into(),
        };
        assert!(matches!(
            classify_bridge_error(&bridge_auth),
            SshWatchFailureKind::Authentication(_)
        ));
    }
}
