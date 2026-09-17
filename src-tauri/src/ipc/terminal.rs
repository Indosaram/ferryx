// allow: SIZE_OK — IPC module bundling terminal commands, output batching, and macOS libproc CWD resolution within constrained write scope
use crate::daemon::client::{DaemonAttachment, DaemonClient};
use crate::daemon::protocol::{DaemonStreamMessage, TerminalStartup};
use crate::ipc::{run_blocking, IpcError, IpcErrorCode};
use crate::terminal::TerminalSignal;
use crate::worktree::{WorkspaceRegistry, WorktreeError, WorktreeIdentity};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::ipc::{Channel, Response};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

pub const TERMINAL_OUTPUT_EVENT: &str = "terminal_output";
pub const TERMINAL_LIFECYCLE_EVENT: &str = "terminal_lifecycle";

const BATCH_FLUSH_INTERVAL: Duration = Duration::from_millis(10);
const BATCH_MAX_BYTES: usize = 32 * 1024;
const CWD_CACHE_TTL: Duration = Duration::from_millis(500);
const TERMINAL_OUTPUT_FRAME_VERSION: u8 = 1;
const TERMINAL_OUTPUT_FRAME_FIXED_BYTES: usize = 20;
const TERMINAL_OUTPUT_FRAME_HAS_SEQUENCE: u8 = 1 << 0;
const TERMINAL_OUTPUT_FRAME_HAS_DAEMON_EPOCH: u8 = 1 << 1;

static CWD_CACHE: Mutex<Option<HashMap<String, (Instant, PathBuf)>>> = Mutex::new(None);
static TERMINAL_OUTPUT_CHANNEL: Mutex<Option<Channel<Response>>> = Mutex::new(None);

pub fn get_cached_cwd(session_id: &str) -> Option<PathBuf> {
    let mut guard = CWD_CACHE.lock();
    let map = guard.as_mut()?;
    if let Some((cached_at, cwd)) = map.get(session_id) {
        if cached_at.elapsed() < CWD_CACHE_TTL {
            return Some(cwd.clone());
        }
        map.remove(session_id);
    }
    None
}

pub fn update_cached_cwd(session_id: String, cwd: PathBuf) {
    let mut guard = CWD_CACHE.lock();
    let map = guard.get_or_insert_with(HashMap::new);
    map.insert(session_id, (Instant::now(), cwd));
}

pub fn invalidate_cached_cwd(session_id: &str) {
    let mut guard = CWD_CACHE.lock();
    if let Some(map) = guard.as_mut() {
        map.remove(session_id);
    }
}

struct PumpHandle {
    task: tokio::task::JoinHandle<()>,
    stream_task: tokio::task::JoinHandle<()>,
}

static ACTIVE_PUMPS: Mutex<Option<HashMap<String, PumpHandle>>> = Mutex::new(None);

pub fn stop_managed_pump(session_id: &str) {
    let mut guard = ACTIVE_PUMPS.lock();
    if let Some(map) = guard.as_mut() {
        if let Some(pump) = map.remove(session_id) {
            pump.task.abort();
            pump.stream_task.abort();
        }
    }
    crate::terminal::metrics::clear_pending_batch_read(session_id);
}

pub fn start_managed_pump<R: Runtime>(
    session_id: String,
    app_handle: AppHandle<R>,
    attachment: DaemonAttachment,
) {
    let mut guard = ACTIVE_PUMPS.lock();
    let map = guard.get_or_insert_with(HashMap::new);

    if let Some(old_pump) = map.remove(&session_id) {
        old_pump.task.abort();
        old_pump.stream_task.abort();
    }
    crate::terminal::metrics::clear_pending_batch_read(&session_id);

    let stream_task = attachment.stream_task;
    let epoch = attachment.epoch;
    let mut messages = attachment.messages;
    let session_id_clone = session_id.clone();
    let app = app_handle.clone();

    let task = tokio::spawn(async move {
        let epoch_str = epoch.to_string();
        let mut buffer = Vec::with_capacity(BATCH_MAX_BYTES);
        let mut last_seq: Option<u64> = None;

        loop {
            let next = if buffer.is_empty() {
                messages.recv().await
            } else {
                tokio::select! {
                    msg = messages.recv() => msg,
                    _ = tokio::time::sleep(BATCH_FLUSH_INTERVAL) => {
                        flush_terminal_output(
                            &app,
                            &session_id_clone,
                            &mut buffer,
                            last_seq.take(),
                            Some(epoch),
                        );
                        continue;
                    }
                }
            };

            match next {
                Some(DaemonStreamMessage::Output {
                    sequence,
                    data,
                    metrics_read_unix_micros,
                    ..
                }) => {
                    crate::terminal::metrics::note_batch_read_timestamp(
                        &session_id_clone,
                        metrics_read_unix_micros,
                    );
                    buffer.extend_from_slice(&data);
                    last_seq = Some(sequence);
                    while buffer.len() < BATCH_MAX_BYTES {
                        match messages.try_recv() {
                            Ok(DaemonStreamMessage::Output {
                                sequence,
                                data,
                                metrics_read_unix_micros,
                                ..
                            }) => {
                                crate::terminal::metrics::note_batch_read_timestamp(
                                    &session_id_clone,
                                    metrics_read_unix_micros,
                                );
                                buffer.extend_from_slice(&data);
                                last_seq = Some(sequence);
                            }
                            Ok(DaemonStreamMessage::Lagged {
                                requested_after_sequence,
                                available_from_sequence,
                                start_sequence,
                                end_sequence,
                                history,
                                ..
                            }) => {
                                flush_terminal_output(
                                    &app,
                                    &session_id_clone,
                                    &mut buffer,
                                    last_seq.take(),
                                    Some(epoch),
                                );
                                emit_terminal_replay_gap(
                                    &app,
                                    &session_id_clone,
                                    requested_after_sequence,
                                    available_from_sequence,
                                    start_sequence,
                                    end_sequence,
                                    &history,
                                    Some(&epoch_str),
                                );
                            }
                            Ok(DaemonStreamMessage::Gap {
                                requested_after_sequence,
                                available_from_sequence,
                                ..
                            }) => {
                                flush_terminal_output(
                                    &app,
                                    &session_id_clone,
                                    &mut buffer,
                                    last_seq.take(),
                                    Some(epoch),
                                );
                                emit_terminal_replay_gap(
                                    &app,
                                    &session_id_clone,
                                    requested_after_sequence,
                                    available_from_sequence,
                                    None,
                                    None,
                                    &[],
                                    Some(&epoch_str),
                                );
                            }
                            Ok(DaemonStreamMessage::RemoteStatus { state, generation, failure, replay_gap, .. }) => {
                                let _ = app.emit("terminal_remote_status", serde_json::json!({"sessionId":session_id_clone,"state":state,"generation":generation,"failure":failure,"replayGap":replay_gap}));
                            }
                            Ok(DaemonStreamMessage::AgentState { .. }) => {}
                            Ok(DaemonStreamMessage::Exit { exit_code, .. }) => {
                                flush_terminal_output(
                                    &app,
                                    &session_id_clone,
                                    &mut buffer,
                                    last_seq.take(),
                                    Some(epoch),
                                );
                                emit_terminal_exit(&app, &session_id_clone, exit_code);
                                return;
                            }
                            Err(_) => break,
                        }
                    }
                    if buffer.len() >= BATCH_MAX_BYTES {
                        flush_terminal_output(
                            &app,
                            &session_id_clone,
                            &mut buffer,
                            last_seq.take(),
                            Some(epoch),
                        );
                    }
                }
                Some(DaemonStreamMessage::Lagged {
                    requested_after_sequence,
                    available_from_sequence,
                    start_sequence,
                    end_sequence,
                    history,
                    ..
                }) => {
                    flush_terminal_output(
                        &app,
                        &session_id_clone,
                        &mut buffer,
                        last_seq.take(),
                        Some(epoch),
                    );
                    emit_terminal_replay_gap(
                        &app,
                        &session_id_clone,
                        requested_after_sequence,
                        available_from_sequence,
                        start_sequence,
                        end_sequence,
                        &history,
                        Some(&epoch_str),
                    );
                }
                Some(DaemonStreamMessage::Gap {
                    requested_after_sequence,
                    available_from_sequence,
                    ..
                }) => {
                    flush_terminal_output(
                        &app,
                        &session_id_clone,
                        &mut buffer,
                        last_seq.take(),
                        Some(epoch),
                    );
                    emit_terminal_replay_gap(
                        &app,
                        &session_id_clone,
                        requested_after_sequence,
                        available_from_sequence,
                        None,
                        None,
                        &[],
                        Some(&epoch_str),
                    );
                }
                Some(DaemonStreamMessage::RemoteStatus { state, generation, failure, replay_gap, .. }) => {
                    let _ = app.emit("terminal_remote_status", serde_json::json!({"sessionId":session_id_clone,"state":state,"generation":generation,"failure":failure,"replayGap":replay_gap}));
                }
                Some(DaemonStreamMessage::AgentState { .. }) => {}
                Some(DaemonStreamMessage::Exit { exit_code, .. }) => {
                    flush_terminal_output(
                        &app,
                        &session_id_clone,
                        &mut buffer,
                        last_seq.take(),
                        Some(epoch),
                    );
                    emit_terminal_exit(&app, &session_id_clone, exit_code);
                    break;
                }
                None => break,
            }
        }

        if !buffer.is_empty() {
            flush_terminal_output(
                &app,
                &session_id_clone,
                &mut buffer,
                last_seq.take(),
                Some(epoch),
            );
        }
        crate::terminal::metrics::clear_pending_batch_read(&session_id_clone);

        let mut guard = ACTIVE_PUMPS.lock();
        if let Some(map) = guard.as_mut() {
            map.remove(&session_id_clone);
        }
    });

    map.insert(session_id, PumpHandle { task, stream_task });
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpawnTerminalRequest {
    pub workspace_id: String,
    pub worktree: Option<WorktreeIdentity>,
    /// Optional inherited CWD for an Orca split/restore. The command validates that
    /// this path exists and remains inside the resolved worktree before spawning.
    pub cwd: Option<PathBuf>,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
    pub client_request_id: Option<String>,
    #[serde(default)]
    pub shell: Option<String>,
    #[serde(default)]
    pub startup: Option<TerminalStartup>,
    /// Spawn inheriting the live working directory of an existing backend session so
    /// split/restore paths do not need a separate getTerminalCwd round trip first.
    /// Ignored when `cwd` is explicitly provided.
    #[serde(default)]
    pub inherit_from_session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpawnTerminalResponse {
    pub session_id: String,
    pub daemon_epoch: String,
    pub session: crate::daemon::protocol::DaemonSessionDetails,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalSessionSummary {
    pub session_id: String,
    pub worktree_path: Option<PathBuf>,
    #[serde(default)]
    pub running: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalCwdResponse {
    pub cwd: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TerminalOutputKind {
    Output,
    ReplayGap,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalOutputPayload {
    pub session_id: String,
    pub kind: TerminalOutputKind,
    pub data: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sequence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub daemon_epoch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_after_sequence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_from_sequence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_sequence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_sequence: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalReplayGap {
    pub requested_after_sequence: String,
    pub available_from_sequence: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachTerminalResponse {
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub daemon_epoch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history_start_sequence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history_end_sequence: Option<String>,
    pub history: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gap: Option<TerminalReplayGap>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TerminalLifecycleState {
    Started,
    Exited,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalLifecyclePayload {
    pub session_id: String,
    pub state: TerminalLifecycleState,
    pub exit_code: Option<i32>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TerminalSignalRequest {
    Interrupt,
    Terminate,
    Kill,
    Stop,
    Continue,
}

impl From<TerminalSignalRequest> for TerminalSignal {
    fn from(value: TerminalSignalRequest) -> Self {
        match value {
            TerminalSignalRequest::Interrupt => TerminalSignal::Interrupt,
            TerminalSignalRequest::Terminate => TerminalSignal::Terminate,
            TerminalSignalRequest::Kill => TerminalSignal::Kill,
            TerminalSignalRequest::Stop => TerminalSignal::Stop,
            TerminalSignalRequest::Continue => TerminalSignal::Continue,
        }
    }
}

fn encode_terminal_output_frame(
    session_id: &str,
    data: &[u8],
    sequence: Option<u64>,
    daemon_epoch: Option<u64>,
) -> Option<Vec<u8>> {
    let session_id = session_id.as_bytes();
    let session_id_len = u16::try_from(session_id.len()).ok()?;
    let mut flags = 0u8;
    if sequence.is_some() {
        flags |= TERMINAL_OUTPUT_FRAME_HAS_SEQUENCE;
    }
    if daemon_epoch.is_some() {
        flags |= TERMINAL_OUTPUT_FRAME_HAS_DAEMON_EPOCH;
    }

    let mut frame =
        Vec::with_capacity(TERMINAL_OUTPUT_FRAME_FIXED_BYTES + session_id.len() + data.len());
    frame.push(TERMINAL_OUTPUT_FRAME_VERSION);
    frame.push(flags);
    frame.extend_from_slice(&session_id_len.to_le_bytes());
    frame.extend_from_slice(&sequence.unwrap_or_default().to_le_bytes());
    frame.extend_from_slice(&daemon_epoch.unwrap_or_default().to_le_bytes());
    frame.extend_from_slice(session_id);
    frame.extend_from_slice(data);
    Some(frame)
}

#[tauri::command]
pub fn cmd_terminal_output_channel(channel: Channel<Response>) {
    *TERMINAL_OUTPUT_CHANNEL.lock() = Some(channel);
}

fn flush_terminal_output<R: Runtime>(
    app: &AppHandle<R>,
    session_id: &str,
    buffer: &mut Vec<u8>,
    sequence: Option<u64>,
    daemon_epoch: Option<u64>,
) -> bool {
    if buffer.is_empty() {
        return true;
    }

    let channel = TERMINAL_OUTPUT_CHANNEL.lock().clone();
    if let Some(channel) = channel {
        if let Some(frame) =
            encode_terminal_output_frame(session_id, buffer, sequence, daemon_epoch)
        {
            match channel.send(Response::new(frame)) {
                Ok(()) => {
                    crate::terminal::metrics::record_channel_send_for_session(session_id);
                    buffer.clear();
                    return true;
                }
                Err(error) => {
                    tracing::debug!("Failed to send terminal output channel frame: {error}");
                    let mut guard = TERMINAL_OUTPUT_CHANNEL.lock();
                    if guard
                        .as_ref()
                        .is_some_and(|current| current.id() == channel.id())
                    {
                        *guard = None;
                    }
                }
            }
        }
    }

    // Compatibility/failure fallback. Normal desktop runtime registers the raw-byte channel
    // before terminals are attached, so large stdout does not take this JSON/base64 path.
    let payload = TerminalOutputPayload {
        session_id: session_id.to_string(),
        kind: TerminalOutputKind::Output,
        data: STANDARD.encode(&buffer),
        sequence: sequence.map(|s| s.to_string()),
        daemon_epoch: daemon_epoch.map(|s| s.to_string()),
        requested_after_sequence: None,
        available_from_sequence: None,
        start_sequence: None,
        end_sequence: None,
    };
    buffer.clear();
    crate::terminal::metrics::clear_pending_batch_read(session_id);
    if let Err(error) = app.emit(TERMINAL_OUTPUT_EVENT, payload) {
        tracing::debug!("Failed to emit terminal output event: {error}");
        false
    } else {
        true
    }
}

fn emit_terminal_replay_gap<R: Runtime>(
    app: &AppHandle<R>,
    session_id: &str,
    requested_after_sequence: u64,
    available_from_sequence: u64,
    start_sequence: Option<u64>,
    end_sequence: Option<u64>,
    history: &[u8],
    daemon_epoch: Option<&str>,
) -> bool {
    let payload = TerminalOutputPayload {
        session_id: session_id.to_string(),
        kind: TerminalOutputKind::ReplayGap,
        data: STANDARD.encode(history),
        sequence: end_sequence.map(|s| s.to_string()),
        daemon_epoch: daemon_epoch.map(|s| s.to_string()),
        requested_after_sequence: Some(requested_after_sequence.to_string()),
        available_from_sequence: Some(available_from_sequence.to_string()),
        start_sequence: start_sequence.map(|s| s.to_string()),
        end_sequence: end_sequence.map(|s| s.to_string()),
    };
    if let Err(error) = app.emit(TERMINAL_OUTPUT_EVENT, payload) {
        tracing::debug!("Failed to emit terminal replay-gap event: {error}");
        false
    } else {
        true
    }
}

fn emit_terminal_exit<R: Runtime>(app: &AppHandle<R>, session_id: &str, exit_code: Option<i32>) {
    let _ = app.emit(
        TERMINAL_LIFECYCLE_EVENT,
        TerminalLifecyclePayload {
            session_id: session_id.to_string(),
            state: TerminalLifecycleState::Exited,
            exit_code,
            reason: None,
        },
    );
}

pub(crate) fn effective_paired_repo_root(
    repo_root: &std::path::Path,
    target_cwd: Option<&std::path::Path>,
) -> std::path::PathBuf {
    let repo_root_str = repo_root.to_string_lossy();
    if !repo_root.as_os_str().is_empty() && !repo_root_str.contains(".orca-worktrees") {
        repo_root.to_path_buf()
    } else if let Some(cwd) = target_cwd {
        let s = cwd.to_string_lossy();
        if let Some(idx) = s.find("/.orca-worktrees") {
            std::path::PathBuf::from(&s[..idx])
        } else if let Some(idx) = s.find("\\.orca-worktrees") {
            std::path::PathBuf::from(&s[..idx])
        } else {
            repo_root.to_path_buf()
        }
    } else {
        repo_root.to_path_buf()
    }
}

pub(crate) fn infer_worktree_slug(
    request_worktree: Option<&WorktreeIdentity>,
    target_cwd: Option<&std::path::Path>,
) -> Option<String> {
    request_worktree
        .map(|w| w.slug.clone())
        .or_else(|| {
            target_cwd.and_then(|cwd| {
                let s = cwd.to_string_lossy();
                let marker_unix = ".orca-worktrees/wt-";
                let marker_win = ".orca-worktrees\\wt-";
                let start_idx = s
                    .find(marker_unix)
                    .map(|i| i + marker_unix.len())
                    .or_else(|| s.find(marker_win).map(|i| i + marker_win.len()))?;
                let rem = &s[start_idx..];
                let end_idx = rem.find(['/', '\\']).unwrap_or(rem.len());
                let slug = &rem[..end_idx];
                if !slug.is_empty() {
                    Some(slug.to_string())
                } else {
                    None
                }
            })
        })
}

pub(crate) fn resolve_paired_spawn_target(
    remote_workspace_id: &str,
    effective_repo_root: &std::path::Path,
    target_cwd: Option<&std::path::Path>,
    worktree_slug: Option<&str>,
) -> (
    String,
    Option<crate::remote::machine_protocol::WorktreeIdentity>,
    Option<String>,
) {
    if let Some(slug) = worktree_slug {
        let worktree_ident = crate::remote::machine_protocol::WorktreeIdentity {
            ws_id: remote_workspace_id.to_string(),
            slug: slug.to_string(),
        };
        // When worktree identity is provided, the remote daemon resolves default_cwd
        // to the worktree root itself. cwd_relative must only carry subdirectories
        // relative to the worktree root, NOT relative to repo_root.
        let sub_rel = if let Some(cwd_path) = target_cwd {
            let cwd_norm = cwd_path.to_string_lossy().replace('\\', "/");
            let repo_norm = effective_repo_root.to_string_lossy().replace('\\', "/");
            let expected_wt_rel = format!(".orca-worktrees/wt-{}", slug);

            if !repo_norm.is_empty() && cwd_norm.starts_with(&repo_norm) {
                let rel = cwd_norm[repo_norm.len()..].trim_start_matches('/');
                if rel == expected_wt_rel {
                    None
                } else if let Some(sub) = rel.strip_prefix(&format!("{}/", expected_wt_rel)) {
                    if !sub.is_empty() {
                        Some(sub.to_string())
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else if cwd_norm.ends_with(&expected_wt_rel) {
                None
            } else if let Some(idx) = cwd_norm.find(&format!("{}/", expected_wt_rel)) {
                let sub = &cwd_norm[idx + expected_wt_rel.len() + 1..];
                if !sub.is_empty() {
                    Some(sub.to_string())
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };
        (remote_workspace_id.to_string(), Some(worktree_ident), sub_rel)
    } else if let Some(cwd_path) = target_cwd {
        let cwd_norm = cwd_path.to_string_lossy().replace('\\', "/");
        let repo_norm = effective_repo_root.to_string_lossy().replace('\\', "/");
        if !repo_norm.is_empty() && cwd_norm == repo_norm {
            (remote_workspace_id.to_string(), None, None)
        } else if !repo_norm.is_empty() && cwd_norm.starts_with(&repo_norm) {
            let rel = cwd_norm[repo_norm.len()..].trim_start_matches('/');
            if !rel.is_empty() && !rel.contains("..") {
                (remote_workspace_id.to_string(), None, Some(rel.to_string()))
            } else {
                (remote_workspace_id.to_string(), None, None)
            }
        } else {
            (remote_workspace_id.to_string(), None, None)
        }
    } else {
        (remote_workspace_id.to_string(), None, None)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PendingCreateStatus {
    Pending,
    Completed { session_id: String },
    Adopted { proxy_session_id: String },
    Cancelled,
    Failed { error: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingCreateRecord {
    pub request_id: String,
    pub host_id: String,
    pub generation: crate::scoped_contracts::Epoch,
    pub workspace_id: Option<String>,
    pub cancelled: bool,
    pub status: PendingCreateStatus,
    pub session: Option<crate::remote::machine_protocol::Session>,
}

static PENDING_CREATES: std::sync::LazyLock<Mutex<std::collections::HashMap<String, PendingCreateRecord>>> =
    std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

pub fn get_pending_creates() -> std::collections::HashMap<String, PendingCreateRecord> {
    PENDING_CREATES.lock().clone()
}

pub fn get_pending_create(request_id: &str) -> Option<PendingCreateRecord> {
    PENDING_CREATES.lock().get(request_id).cloned()
}

pub fn cancel_pending_create(request_id: &str) {
    let mut guard = PENDING_CREATES.lock();
    if let Some(record) = guard.get_mut(request_id) {
        record.cancelled = true;
    }
}

pub fn clear_pending_creates_for_test() {
    PENDING_CREATES.lock().clear();
}

const PENDING_CREATE_BACKGROUND_WINDOW_SECS: u64 = 600;
const PENDING_CREATE_BACKGROUND_POLL_MS: u64 = 250;

/// P10: a create whose journal outcome stayed unknown after the inline burst is
/// NOT abandoned — the logical intent is retained and a bounded background
/// reconciler keeps polling until the remote journal becomes terminal.
fn register_pending_create(
    host_id: &str,
    generation: crate::scoped_contracts::Epoch,
    request_id: &str,
    cancelled: bool,
) {
    PENDING_CREATES.lock().insert(
        request_id.to_string(),
        PendingCreateRecord {
            request_id: request_id.to_string(),
            host_id: host_id.to_string(),
            generation,
            workspace_id: None,
            cancelled,
            status: PendingCreateStatus::Pending,
            session: None,
        },
    );
}

fn spawn_background_create_reconciler(
    daemon_client: &DaemonClient,
    host_id: &str,
    generation: crate::scoped_contracts::Epoch,
    request_id: &str,
) {
    let client = daemon_client.clone();
    let host_id = host_id.to_string();
    let request_id = request_id.to_string();
    tokio::spawn(async move {
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_secs(PENDING_CREATE_BACKGROUND_WINDOW_SECS);
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(PENDING_CREATE_BACKGROUND_POLL_MS)).await;
            if std::time::Instant::now() >= deadline {
                // Window exhausted: leave the record Pending so a later pass
                // (or operator action) can still reconcile it.
                break;
            }
            let (cancelled, still_pending) = {
                let guard = PENDING_CREATES.lock();
                match guard.get(&request_id) {
                    Some(record) => (record.cancelled, matches!(record.status, PendingCreateStatus::Pending)),
                    None => return,
                }
            };
            if !still_pending {
                return;
            }
            let journal_req = crate::paired_host::client::OperationRequest {
                host_id: host_id.clone(),
                generation,
                operation: crate::paired_host::client::Operation::Operation {
                    request_id: request_id.clone(),
                },
            };
            let journal_resp = match client.paired_host_operation(journal_req).await {
                Ok(resp) => resp,
                Err(_) => continue,
            };
            let terminal = match journal_resp.result {
                crate::paired_host::client::OperationResult::Operation(
                    crate::remote::machine_protocol::Operation::Completed { outcome, .. },
                ) => Some(outcome),
                _ => None,
            };
            let Some(outcome) = terminal else { continue };
            match outcome {
                crate::remote::machine_protocol::OperationOutcome::Session { session } => {
                    if cancelled {
                        let close_req = crate::remote::machine_protocol::CloseSessionRequest {
                            request_id: uuid::Uuid::new_v4().to_string(),
                            daemon_epoch: session.target.daemon_epoch.clone(),
                        };
                        let _ = client
                            .paired_host_operation(crate::paired_host::client::OperationRequest {
                                host_id: host_id.clone(),
                                generation,
                                operation: crate::paired_host::client::Operation::CloseSession {
                                    session_id: session.target.session_id.clone(),
                                    request: close_req,
                                },
                            })
                            .await;
                        let mut guard = PENDING_CREATES.lock();
                        if let Some(record) = guard.get_mut(&request_id) {
                            record.status = PendingCreateStatus::Cancelled;
                        }
                    } else {
                        let descriptor = crate::terminal::paired_daemon::Descriptor {
                            host_id: host_id.clone(),
                            generation,
                            target: session.target.clone(),
                            after_sequence: None,
                        };
                        let discovered_session_id = session.target.session_id.clone();
                        let reattach = client.paired_terminal_reattach(descriptor).await;
                        let mut guard = PENDING_CREATES.lock();
                        if let Some(record) = guard.get_mut(&request_id) {
                            match reattach {
                                Ok((proxy_session_id, _)) => {
                                    record.session = Some(session);
                                    record.status = PendingCreateStatus::Adopted { proxy_session_id };
                                }
                                Err(_) => {
                                    record.session = Some(session);
                                    record.status = PendingCreateStatus::Completed {
                                        session_id: discovered_session_id,
                                    };
                                }
                            }
                        }
                    }
                    return;
                }
                crate::remote::machine_protocol::OperationOutcome::Error { error } => {
                    let mut guard = PENDING_CREATES.lock();
                    if let Some(record) = guard.get_mut(&request_id) {
                        record.status = PendingCreateStatus::Failed { error: error.message.clone() };
                    }
                    return;
                }
                _ => {}
            }
        }
    });
}

pub async fn reconcile_ambiguous_create(
    daemon_client: &DaemonClient,
    host_id: &str,
    generation: crate::scoped_contracts::Epoch,
    request_id: &str,
    cancelled: bool,
) -> Result<Option<crate::remote::machine_protocol::Session>, IpcError> {
    const MAX_RECONCILE_ATTEMPTS: usize = 3;
    let mut attempts = 0;

    loop {
        attempts += 1;
        let journal_req = crate::paired_host::client::OperationRequest {
            host_id: host_id.to_string(),
            generation,
            operation: crate::paired_host::client::Operation::Operation {
                request_id: request_id.to_string(),
            },
        };

        match daemon_client.paired_host_operation(journal_req).await {
            Ok(op_resp) => {
                match op_resp.result {
                    crate::paired_host::client::OperationResult::Operation(
                        crate::remote::machine_protocol::Operation::Completed { outcome, .. },
                    ) => {
                        match outcome {
                            crate::remote::machine_protocol::OperationOutcome::Session { session } => {
                                if cancelled {
                                    let close_req = crate::remote::machine_protocol::CloseSessionRequest {
                                        request_id: uuid::Uuid::new_v4().to_string(),
                                        daemon_epoch: session.target.daemon_epoch.clone(),
                                    };
                                    let _ = daemon_client
                                        .paired_host_operation(crate::paired_host::client::OperationRequest {
                                            host_id: host_id.to_string(),
                                            generation,
                                            operation: crate::paired_host::client::Operation::CloseSession {
                                                session_id: session.target.session_id.clone(),
                                                request: close_req,
                                            },
                                        })
                                        .await;
                                    return Ok(None);
                                }
                                return Ok(Some(session));
                            }
                            crate::remote::machine_protocol::OperationOutcome::Error { error } => {
                                let client_err = crate::paired_host::client::ClientError {
                                    code: error.code.clone(),
                                    machine_error: Some(error.clone()),
                                    request_id: if error.request_id.is_empty() {
                                        Some(request_id.to_string())
                                    } else {
                                        Some(error.request_id.clone())
                                    },
                                    ambiguous: false,
                                };
                                return Err(map_client_error(&client_err, Some(host_id), Some(generation)));
                            }
                            _ => {
                                let unknown_err = crate::paired_host::client::ClientError {
                                    code: "OPERATION_OUTCOME_UNKNOWN".to_string(),
                                    machine_error: None,
                                    request_id: Some(request_id.to_string()),
                                    ambiguous: true,
                                };
                                return Err(map_client_error(&unknown_err, Some(host_id), Some(generation)));
                            }
                        }
                    }
                    crate::paired_host::client::OperationResult::Operation(
                        crate::remote::machine_protocol::Operation::Pending { .. },
                    )
                    | crate::paired_host::client::OperationResult::Operation(
                        crate::remote::machine_protocol::Operation::OutcomeUnknown { .. },
                    ) => {
                        if attempts >= MAX_RECONCILE_ATTEMPTS {
                            register_pending_create(host_id, generation, request_id, cancelled);
                            spawn_background_create_reconciler(daemon_client, host_id, generation, request_id);
                            let unknown_err = crate::paired_host::client::ClientError {
                                code: "OPERATION_OUTCOME_UNKNOWN".to_string(),
                                machine_error: None,
                                request_id: Some(request_id.to_string()),
                                ambiguous: true,
                            };
                            return Err(map_client_error(&unknown_err, Some(host_id), Some(generation)));
                        }
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                    _ => {
                        if attempts >= MAX_RECONCILE_ATTEMPTS {
                            register_pending_create(host_id, generation, request_id, cancelled);
                            spawn_background_create_reconciler(daemon_client, host_id, generation, request_id);
                            let unknown_err = crate::paired_host::client::ClientError {
                                code: "OPERATION_OUTCOME_UNKNOWN".to_string(),
                                machine_error: None,
                                request_id: Some(request_id.to_string()),
                                ambiguous: true,
                            };
                            return Err(map_client_error(&unknown_err, Some(host_id), Some(generation)));
                        }
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                }
            }
            Err(e) if e.code == "OPERATION_NOT_FOUND" => {
                return Err(map_client_error(&e, Some(host_id), Some(generation)));
            }
            Err(e) if attempts < MAX_RECONCILE_ATTEMPTS => {
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
            Err(e) => {
                return Err(map_client_error(&e, Some(host_id), Some(generation)));
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CleanupOutcome {
    Success,
    Unknown {
        host_id: String,
        generation: crate::scoped_contracts::Epoch,
        session_id: String,
        daemon_epoch: crate::scoped_contracts::Epoch,
        cleanup_request_id: String,
    },
    Failed {
        error: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanupRecord {
    pub host_id: String,
    pub generation: crate::scoped_contracts::Epoch,
    pub session_id: String,
    pub daemon_epoch: crate::scoped_contracts::Epoch,
    pub cleanup_request_id: String,
    pub attempts: usize,
    pub resolved: bool,
}

static PENDING_CLEANUPS: Mutex<Vec<CleanupRecord>> = Mutex::new(Vec::new());
static EXHAUSTED_CLEANUPS: Mutex<Vec<CleanupRecord>> = Mutex::new(Vec::new());

pub fn get_pending_cleanups() -> Vec<CleanupRecord> {
    PENDING_CLEANUPS.lock().clone()
}

pub fn get_exhausted_cleanups() -> Vec<CleanupRecord> {
    EXHAUSTED_CLEANUPS.lock().clone()
}

pub fn clear_pending_cleanups_for_test() {
    PENDING_CLEANUPS.lock().clear();
    EXHAUSTED_CLEANUPS.lock().clear();
}

pub fn clear_exhausted_cleanups_for_test() {
    EXHAUSTED_CLEANUPS.lock().clear();
}

pub async fn start_cleanup_reaper(daemon_client: Arc<DaemonClient>) {
    // P11: schedule the cleanup reaper from a real lifecycle. One reaper per
    // process; the first pass runs immediately so restart-time leftovers are
    // resolved without waiting for the tick interval.
    static REAPER_STARTED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    if REAPER_STARTED.set(()).is_err() {
        return;
    }
    tokio::spawn(async move {
        loop {
            let resolved = reap_cleanup_unknowns(&daemon_client).await;
            if resolved > 0 {
                eprintln!("[ipc::terminal] cleanup reaper resolved {resolved} ambiguous close(s)");
            }
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        }
    });
}


pub async fn execute_cleanup_close(
    daemon_client: &DaemonClient,
    host_id: &str,
    generation: crate::scoped_contracts::Epoch,
    session_id: &str,
    daemon_epoch: crate::scoped_contracts::Epoch,
    cleanup_request_id: String,
) -> CleanupOutcome {
    let close_req = crate::remote::machine_protocol::CloseSessionRequest {
        request_id: cleanup_request_id.clone(),
        daemon_epoch: daemon_epoch.clone(),
    };
    let op_req = crate::paired_host::client::OperationRequest {
        host_id: host_id.to_string(),
        generation,
        operation: crate::paired_host::client::Operation::CloseSession {
            session_id: session_id.to_string(),
            request: close_req,
        },
    };

    match daemon_client.paired_host_operation(op_req).await {
        Ok(_) => CleanupOutcome::Success,
        Err(e) if e.code == "SESSION_NOT_FOUND" => CleanupOutcome::Success,
        Err(e) if e.ambiguous || matches!(e.code.as_str(), "TIMEOUT" | "OPERATION_OUTCOME_UNKNOWN") => {
            let journal_req = crate::paired_host::client::OperationRequest {
                host_id: host_id.to_string(),
                generation,
                operation: crate::paired_host::client::Operation::Operation {
                    request_id: cleanup_request_id.clone(),
                },
            };
            match daemon_client.paired_host_operation(journal_req).await {
                Ok(resp) => {
                    if let crate::paired_host::client::OperationResult::Operation(
                        crate::remote::machine_protocol::Operation::Completed { .. },
                    ) = resp.result {
                        CleanupOutcome::Success
                    } else {
                        let mut guard = PENDING_CLEANUPS.lock();
                        guard.push(CleanupRecord {
                            host_id: host_id.to_string(),
                            generation,
                            session_id: session_id.to_string(),
                            daemon_epoch,
                            cleanup_request_id: cleanup_request_id.clone(),
                            attempts: 1,
                            resolved: false,
                        });
                        CleanupOutcome::Unknown {
                            host_id: host_id.to_string(),
                            generation,
                            session_id: session_id.to_string(),
                            daemon_epoch,
                            cleanup_request_id,
                        }
                    }
                }
                _ => {
                    let mut guard = PENDING_CLEANUPS.lock();
                    guard.push(CleanupRecord {
                        host_id: host_id.to_string(),
                        generation,
                        session_id: session_id.to_string(),
                        daemon_epoch,
                        cleanup_request_id: cleanup_request_id.clone(),
                        attempts: 1,
                        resolved: false,
                    });
                    CleanupOutcome::Unknown {
                        host_id: host_id.to_string(),
                        generation,
                        session_id: session_id.to_string(),
                        daemon_epoch,
                        cleanup_request_id,
                    }
                }
            }
        }
        Err(e) => CleanupOutcome::Failed { error: e.code },
    }
}

pub async fn reap_cleanup_unknowns(daemon_client: &DaemonClient) -> usize {
    let pending = {
        let mut guard = PENDING_CLEANUPS.lock();
        std::mem::take(&mut *guard)
    };

    let mut remaining = Vec::new();
    let mut resolved_count = 0;
    const MAX_REAP_ATTEMPTS: usize = 3;

    for mut item in pending {
        if item.attempts >= MAX_REAP_ATTEMPTS {
            // P11: exhausted cleanups must be retained, not silently dropped —
            // the uncertainty is still real and must stay observable/diagnosable.
            EXHAUSTED_CLEANUPS.lock().push(item);
            continue;
        }
        item.attempts += 1;

        let journal_req = crate::paired_host::client::OperationRequest {
            host_id: item.host_id.clone(),
            generation: item.generation,
            operation: crate::paired_host::client::Operation::Operation {
                request_id: item.cleanup_request_id.clone(),
            },
        };

        match daemon_client.paired_host_operation(journal_req).await {
            Ok(resp) => {
                if let crate::paired_host::client::OperationResult::Operation(
                    crate::remote::machine_protocol::Operation::Completed { .. },
                ) = resp.result {
                    resolved_count += 1;
                    continue;
                }
            }
            Err(e) => {
                if e.code == "OPERATION_NOT_FOUND" {
                    let retry_req_id = uuid::Uuid::new_v4().to_string();
                    let close_req = crate::remote::machine_protocol::CloseSessionRequest {
                        request_id: retry_req_id.clone(),
                        daemon_epoch: item.daemon_epoch.clone(),
                    };
                    let retry_op = crate::paired_host::client::OperationRequest {
                        host_id: item.host_id.clone(),
                        generation: item.generation,
                        operation: crate::paired_host::client::Operation::CloseSession {
                            session_id: item.session_id.clone(),
                            request: close_req,
                        },
                    };
                    if daemon_client.paired_host_operation(retry_op).await.is_ok() {
                        resolved_count += 1;
                        continue;
                    }
                }
            }
            _ => {}
        }
        remaining.push(item);
    }

    let mut guard = PENDING_CLEANUPS.lock();
    guard.extend(remaining);
    resolved_count
}

#[tauri::command]
pub async fn cmd_terminal_spawn<R: Runtime>(
    app: AppHandle<R>,
    daemon_client: State<'_, Arc<DaemonClient>>,
    registry: State<'_, WorkspaceRegistry>,
    request: SpawnTerminalRequest,
) -> Result<SpawnTerminalResponse, IpcError> {
    let has_worktree = request.worktree.is_some();
    let has_cwd = request.cwd.is_some();
    let has_client_request_id = request.client_request_id.is_some();
    eprintln!(
        "[cmd_terminal_spawn] request received has_worktree={has_worktree} has_cwd={has_cwd} has_client_request_id={has_client_request_id}"
    );

    let cols = request.cols.unwrap_or(80);
    let rows = request.rows.unwrap_or(24);
    let is_remote_workspace = crate::ssh::projects::is_remote(&request.workspace_id);
    let is_paired_workspace = request.workspace_id.starts_with("daemon:")
        || matches!(request.startup, Some(TerminalStartup::PairedDaemon { .. }));
    let spawn_result = if is_remote_workspace {
        if request.startup.is_some() {
            return Err(crate::ssh::projects::unsupported());
        }
        let host_store_path = super::ssh::get_ssh_store_path(&app)?;
        let lookup = host_store_path.clone();
        let id = request.workspace_id.clone();
        run_blocking(move || crate::ssh::projects::resolve(&lookup, &id)).await?;
        // Explicit worktree/cwd spawn at the validated remote worktree path,
        // everything else at the registered root; local process CWD is still never inherited.
        daemon_client
            .spawn_terminal_with_startup(
                request
                    .client_request_id
                    .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                request.workspace_id,
                request.worktree.clone(),
                request.cwd.clone().map(|p| p.to_string_lossy().to_string()),
                cols,
                rows,
                None,
                Some(TerminalStartup::RemoteSsh { host_store_path }),
            )
            .await?
    } else if is_paired_workspace {
        if matches!(request.startup, Some(TerminalStartup::RemoteSsh { .. })) {
            return Err(crate::ssh::projects::unsupported());
        }

        let data_dir = app
            .path()
            .app_data_dir()
            .map_err(|e| IpcError::internal(e.to_string()))?;

        let dir = data_dir.clone();
        let ws_id = request.workspace_id.clone();
        let stored_project = run_blocking(move || {
            Ok::<_, IpcError>(crate::paired_host::projects::resolve_stored_project(&dir, &ws_id))
        })
        .await?;

        let (host_id, remote_workspace_id, repo_root): (String, String, std::path::PathBuf) = match &request.startup {
            Some(TerminalStartup::PairedDaemon { host_id, remote_workspace_id }) => {
                let repo_root = stored_project
                    .as_ref()
                    .map(|p| std::path::PathBuf::from(&p.metadata.repo_root))
                    .unwrap_or_default();
                (host_id.clone(), remote_workspace_id.clone(), repo_root)
            }
            _ => {
                match stored_project {
                    Some(stored) => match stored.target {
                        crate::scoped_contracts::RunTarget::PairedDaemon { host_id } => {
                            (host_id, stored.remote_workspace_id, std::path::PathBuf::from(stored.metadata.repo_root))
                        }
                        _ => (stored.remote_workspace_id.clone(), stored.remote_workspace_id, std::path::PathBuf::from(stored.metadata.repo_root)),
                    },
                    None => {
                        return Err(IpcError::new(
                            crate::ipc::error::IpcErrorCode::WorkspaceNotFound,
                            "Paired daemon project not found. Re-select or re-pair this project.",
                        ));
                    }
                }
            }
        };

        let hosts = daemon_client.paired_host_list().await.map_err(|e| {
            IpcError::internal(e.message)
        })?;
        let host = hosts.into_iter().find(|h| h.host_id == host_id).ok_or_else(|| {
            IpcError::internal(format!("Paired machine '{host_id}' not found in inventory"))
        })?;
        if host.auth_status != crate::paired_host::inventory::AuthStatus::Paired {
            return Err(IpcError::internal("Machine authorization required for paired host"));
        }

        let client_request_id = request
            .client_request_id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        let target_cwd = request.cwd.as_deref();
        let effective_repo_root = effective_paired_repo_root(&repo_root, target_cwd);
        let worktree_slug = infer_worktree_slug(request.worktree.as_ref(), target_cwd);

        let (target_workspace_id, create_worktree, cwd_relative): (
            String,
            Option<crate::remote::machine_protocol::WorktreeIdentity>,
            Option<String>,
        ) = if worktree_slug.is_some() {
            resolve_paired_spawn_target(
                &remote_workspace_id,
                &effective_repo_root,
                target_cwd,
                worktree_slug.as_deref(),
            )
        } else if let Some(cwd_path) = target_cwd {
            let cwd_str = cwd_path.to_string_lossy();
            let is_absolute = cwd_path.is_absolute()
                || cwd_str.starts_with('/')
                || cwd_str.starts_with("\\\\")
                || (cwd_str.len() >= 3
                    && cwd_str.as_bytes()[1] == b':'
                    && matches!(cwd_str.as_bytes()[2], b'/' | b'\\'));

            if !effective_repo_root.as_os_str().is_empty() && cwd_path == effective_repo_root {
                (remote_workspace_id.clone(), None, None)
            } else if !effective_repo_root.as_os_str().is_empty() && cwd_path.starts_with(&effective_repo_root) {
                if let Ok(rel) = cwd_path.strip_prefix(&effective_repo_root) {
                    if rel.components().all(|c| matches!(c, std::path::Component::Normal(_) | std::path::Component::CurDir))
                        && !rel.as_os_str().is_empty()
                    {
                        (remote_workspace_id.clone(), None, Some(rel.to_string_lossy().to_string()))
                    } else {
                        (remote_workspace_id.clone(), None, None)
                    }
                } else {
                    (remote_workspace_id.clone(), None, None)
                }
            } else if is_absolute {
                static REGISTERED_WORKTREES: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, String>>> =
                    std::sync::OnceLock::new();
                let cache_mutex = REGISTERED_WORKTREES.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
                let cached_id = {
                    cache_mutex.lock().unwrap().get(cwd_str.as_ref()).cloned()
                };

                if let Some(cached_id) = cached_id {
                    tracing::info!(
                        path = %cwd_path.display(),
                        remote_ws = %cached_id,
                        "Using cached worktree workspace on paired host"
                    );
                    (cached_id, None, None)
                } else {
                    // External worktree or path outside repo_root:
                    // Register the worktree path on the paired machine so it can serve as a workspace target.
                    let reg_op = crate::paired_host::client::Operation::RegisterProject {
                        request: crate::remote::machine_protocol::RegisterRequest {
                            request_id: uuid::Uuid::new_v4().to_string(),
                            repo_path: cwd_str.to_string(),
                        },
                    };
                    match daemon_client
                        .paired_host_operation(crate::paired_host::client::OperationRequest {
                            host_id: host_id.clone(),
                            generation: host.generation,
                            operation: reg_op,
                        })
                        .await
                    {
                        Ok(crate::paired_host::client::OperationResponse {
                            result:
                                crate::paired_host::client::OperationResult::RegisterProject(data),
                            ..
                        }) => {
                            tracing::info!(
                                path = %cwd_path.display(),
                                remote_ws = %data.remote_workspace_id,
                                "Registered worktree workspace on paired host"
                            );
                            cache_mutex.lock().unwrap().insert(cwd_str.to_string(), data.remote_workspace_id.clone());
                            (data.remote_workspace_id, None, None)
                        }
                        Ok(other) => {
                            tracing::warn!(
                                path = %cwd_path.display(),
                                response = ?other,
                                "Remote machine rejected registration for worktree path, falling back to base remote workspace"
                            );
                            (remote_workspace_id.clone(), None, None)
                        }
                        Err(e) => {
                            tracing::warn!(
                                path = %cwd_path.display(),
                                error = %e.code,
                                "Failed to register remote worktree path, falling back to base remote workspace"
                            );
                            (remote_workspace_id.clone(), None, None)
                        }
                    }
                }
            } else if cwd_path.components().all(|c| matches!(c, std::path::Component::Normal(_) | std::path::Component::CurDir))
                && !cwd_path.as_os_str().is_empty()
            {
                (remote_workspace_id.clone(), None, Some(cwd_str.to_string()))
            } else {
                return Err(IpcError::new(
                    crate::ipc::error::IpcErrorCode::InvalidPath,
                    format!(
                        "Cannot resolve target working directory '{}' on paired workspace",
                        cwd_path.display()
                    ),
                ));
            }
        } else {
            (remote_workspace_id.clone(), None, None)
        };

        let resolved_inherit = match &request.inherit_from_session_id {
            Some(id) if id.starts_with("daemon-session:") => {
                match daemon_client.paired_terminal_descriptor(id.clone()).await {
                    Ok(Some(d)) => Some(d.target.session_id),
                    _ => None,
                }
            }
            other => other.clone(),
        };

        let initial_cwd_relative = if resolved_inherit.is_some() {
            None
        } else {
            cwd_relative.clone()
        };

        let create_request = crate::remote::machine_protocol::CreateSessionRequest {
            request_id: client_request_id.clone(),
            workspace_id: target_workspace_id.clone(),
            worktree: create_worktree.clone(),
            cols,
            rows,
            inherit_from_session_id: resolved_inherit.clone(),
            cwd_relative: initial_cwd_relative,
            startup: crate::remote::machine_protocol::Startup::Shell,
        };

        let mut op_resp = daemon_client
            .paired_host_operation(crate::paired_host::client::OperationRequest {
                host_id: host_id.clone(),
                generation: host.generation,
                operation: crate::paired_host::client::Operation::CreateSession {
                    request: create_request.clone(),
                },
            })
            .await;

        if let Err(ref e) = op_resp {
            if !e.ambiguous && resolved_inherit.is_some() && matches!(e.code.as_str(), "SESSION_NOT_FOUND" | "PARENT_SESSION_MISMATCH" | "SESSION_EXPIRED") {
                tracing::warn!(
                    parent_sid = ?resolved_inherit,
                    error = %e.code,
                    "Retrying paired session spawn without parent session inheritance"
                );
                let fallback_request = crate::remote::machine_protocol::CreateSessionRequest {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    inherit_from_session_id: None,
                    cwd_relative: cwd_relative.clone(),
                    ..create_request
                };
                op_resp = daemon_client
                    .paired_host_operation(crate::paired_host::client::OperationRequest {
                        host_id: host_id.clone(),
                        generation: host.generation,
                        operation: crate::paired_host::client::Operation::CreateSession {
                            request: fallback_request,
                        },
                    })
                    .await;
            }
        }

        let remote_session = match op_resp {
            Ok(resp) => match resp.result {
                crate::paired_host::client::OperationResult::CreateSession(session) => session,
                other => {
                    return Err(IpcError::internal(format!("Unexpected operation result: {other:?}")));
                }
            },
            Err(ref e) if e.ambiguous || matches!(e.code.as_str(), "TIMEOUT" | "OPERATION_OUTCOME_UNKNOWN") => {
                match reconcile_ambiguous_create(&daemon_client, &host_id, host.generation, &client_request_id, false).await? {
                    Some(session) => session,
                    None => {
                        let unknown_err = crate::paired_host::client::ClientError {
                            code: "OPERATION_OUTCOME_UNKNOWN".to_string(),
                            machine_error: None,
                            request_id: Some(client_request_id.clone()),
                            ambiguous: true,
                        };
                        return Err(map_client_error(&unknown_err, Some(&host_id), Some(host.generation)));
                    }
                }
            }
            Err(e) => return Err(map_client_error(&e, Some(&host_id), Some(host.generation))),
        };

        let descriptor = crate::terminal::paired_daemon::Descriptor {
            host_id: host_id.clone(),
            generation: host.generation,
            target: remote_session.target.clone(),
            after_sequence: None,
        };

        let (proxy_session_id, _gen) = match daemon_client
            .paired_terminal_reattach(descriptor.clone())
            .await
        {
            Ok(result) => result,
            Err(e) if matches!(e.code.as_str(), "TIMEOUT" | "HOST_UNAVAILABLE" | "PAIRED_PROXY_UNAVAILABLE") => {
                tracing::warn!(
                    error = %e.code,
                    "Retrying paired terminal reattach once after transient error"
                );
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                match daemon_client.paired_terminal_reattach(descriptor).await {
                    Ok(result) => result,
                    Err(reattach_error) => {
                        let cleanup_req_id = uuid::Uuid::new_v4().to_string();
                        let _cleanup_outcome = execute_cleanup_close(
                            &daemon_client,
                            &host_id,
                            host.generation,
                            &remote_session.target.session_id,
                            remote_session.target.daemon_epoch.clone(),
                            cleanup_req_id,
                        )
                        .await;
                        return Err(map_client_error(&reattach_error, Some(&host_id), Some(host.generation)));
                    }
                }
            }
            Err(reattach_error) => {
                // The remote PTY was already created above; a failed reattach must
                // not leak an idle shell on the paired machine.
                let cleanup_req_id = uuid::Uuid::new_v4().to_string();
                let _cleanup_outcome = execute_cleanup_close(
                    &daemon_client,
                    &host_id,
                    host.generation,
                    &remote_session.target.session_id,
                    remote_session.target.daemon_epoch.clone(),
                    cleanup_req_id,
                )
                .await;
                return Err(map_client_error(&reattach_error, Some(&host_id), Some(host.generation)));
            }
        };

        // Durably store project info for future sessions
        {
            let dir = data_dir.clone();
            let stored_repo_root = if !effective_repo_root.as_os_str().is_empty() {
                effective_repo_root.to_string_lossy().to_string()
            } else {
                let s = &remote_session.cwd;
                if let Some(idx) = s.find("/.orca-worktrees") {
                    s[..idx].to_string()
                } else if let Some(idx) = s.find("\\.orca-worktrees") {
                    s[..idx].to_string()
                } else {
                    s.clone()
                }
            };
            let p = crate::paired_host::projects::Project {
                metadata: crate::remote::machine_protocol::Project {
                    workspace_id: request.workspace_id.clone(),
                    repo_root: stored_repo_root,
                    git_root: None,
                    git_common_dir: None,
                    git_remote: None,
                    git_branch: None,
                    git_head: None,
                    availability: crate::remote::machine_protocol::Availability::Ready,
                    revision: crate::scoped_contracts::Epoch(1),
                },
                remote_workspace_id: remote_workspace_id.clone(),
                target: crate::scoped_contracts::RunTarget::PairedDaemon {
                    host_id: host_id.clone(),
                },
            };
            let _ = run_blocking(move || {
                let _ = crate::paired_host::projects::save_stored_project(&dir, p);
                Ok::<_, IpcError>(())
            }).await;
        }

        crate::daemon::client::DaemonSpawnResult {
            session_id: proxy_session_id.clone(),
            epoch: host.generation.0,
            session: crate::daemon::protocol::DaemonSessionDetails {
                session_id: proxy_session_id,
                workspace_id: Some(request.workspace_id.clone()),
                worktree: request.worktree.clone(),
                cwd: request.cwd.map(|p| p.to_string_lossy().to_string()).or(Some(remote_session.cwd)),
                cols,
                rows,
                running: true,
                start_sequence: Some(remote_session.start_sequence.0),
                end_sequence: Some(remote_session.end_sequence.0),
            },
        }
    } else {
        if matches!(request.startup, Some(TerminalStartup::RemoteSsh { .. })) {
            return Err(crate::ssh::projects::unsupported());
        }
        let registry = (*registry).clone();
        let workspace_id = request.workspace_id.clone();
        let identity = request.worktree.clone();
        // Inherit the live CWD of an existing backend session when the frontend did not
        // pin one, removing the separate getTerminalCwd IPC hop on split/restore paths.
        let requested_cwd = match (request.cwd.clone(), request.inherit_from_session_id.clone()) {
            (Some(cwd), _) => Some(cwd),
            (None, Some(inherit_session_id)) => {
                if let Some(cached) = get_cached_cwd(&inherit_session_id) {
                    Some(cached)
                } else {
                    match daemon_client.describe_session(&inherit_session_id).await {
                        Ok(details) => details.cwd.map(PathBuf::from),
                        Err(err) => {
                            eprintln!(
                            "[cmd_terminal_spawn] stage=inherit_cwd failed session={inherit_session_id} code={:?}",
                            err.code
                        );
                            None
                        }
                    }
                }
            }
            (None, None) => None,
        };

        let (worktree_manager, worktree_root) = match run_blocking(move || {
            registry
                .resolve_terminal_target(&workspace_id, identity.as_ref())
                .map_err(IpcError::from)
        })
        .await
        {
            Ok(target) => target,
            Err(err) => {
                eprintln!(
                    "[cmd_terminal_spawn] stage=resolve_target failed code={:?}",
                    err.code
                );
                return Err(err);
            }
        };

        let worktree_for_validation = worktree_manager.clone();
        let worktree_root_for_validation = worktree_root.clone();
        let cwd = match run_blocking(move || {
            let Some(requested) = requested_cwd else {
                return Ok(worktree_root_for_validation);
            };
            let canonical = worktree_for_validation
                .canonical_allowed_path(&requested)
                .map_err(IpcError::from)?;
            if canonical != worktree_root_for_validation
                && !canonical.starts_with(&worktree_root_for_validation)
            {
                return Err(IpcError::from(WorktreeError::PathOutsideWorkspace {
                    path: requested,
                    root: worktree_root_for_validation,
                }));
            }
            if !canonical.is_dir() {
                return Err(IpcError::from(WorktreeError::InvalidPath {
                    path: canonical,
                    reason: "terminal cwd must be a directory".into(),
                }));
            }
            Ok(canonical)
        })
        .await
        {
            Ok(cwd) => cwd,
            Err(err) => {
                eprintln!(
                    "[cmd_terminal_spawn] stage=validate_cwd failed code={:?}",
                    err.code
                );
                return Err(err);
            }
        };

        let repo_root_str = worktree_manager.repo_root().to_string_lossy().to_string();
        if let Err(err) = daemon_client
            .register_workspace(&request.workspace_id, &repo_root_str)
            .await
        {
            eprintln!(
                "[cmd_terminal_spawn] stage=daemon_register failed code={:?}",
                err.code
            );
            return Err(err);
        }

        let client_request_id = request
            .client_request_id
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let effective_shell = request.shell.filter(|s| !s.trim().is_empty()).or_else(|| {
            crate::terminal::cached_terminal_preferences()
                .default_shell
                .clone()
        });
        let spawn_result = match daemon_client
            .spawn_terminal_with_startup(
                client_request_id,
                request.workspace_id,
                request.worktree,
                Some(cwd.to_string_lossy().to_string()),
                cols,
                rows,
                effective_shell,
                request.startup,
            )
            .await
        {
            Ok(result) => result,
            Err(err) => {
                eprintln!(
                    "[cmd_terminal_spawn] stage=daemon_spawn failed code={:?}",
                    err.code
                );
                return Err(err);
            }
        };

        spawn_result
    };

    let session_id = spawn_result.session_id.clone();
    if let Some(host) = app.try_state::<crate::native_terminal::surface_host::NativeTerminalSurfaceHostState>() {
        host.mark_pending_startup(&session_id);
    }
    let attachment = match daemon_client.attach(&session_id, None).await {
        Ok(attachment) => attachment,
        Err(err) => {
            if let Some(host) = app.try_state::<crate::native_terminal::surface_host::NativeTerminalSurfaceHostState>() {
                host.clear_pending_session(&session_id);
            }
            eprintln!(
                "[cmd_terminal_spawn] stage=daemon_attach failed code={:?}",
                err.code
            );
            return Err(err);
        }
    };
    start_managed_pump(session_id.clone(), app.clone(), attachment);

    let started = TerminalLifecyclePayload {
        session_id: session_id.clone(),
        state: TerminalLifecycleState::Started,
        exit_code: None,
        reason: None,
    };
    if let Err(error) = app.emit(TERMINAL_LIFECYCLE_EVENT, started) {
        if let Some(host) = app.try_state::<crate::native_terminal::surface_host::NativeTerminalSurfaceHostState>() {
            host.clear_pending_session(&session_id);
        }
        eprintln!("[cmd_terminal_spawn] stage=emit_lifecycle failed");
        let _ = daemon_client.close_terminal(&session_id).await;
        return Err(IpcError::internal(format!(
            "failed to emit terminal lifecycle event: {error}"
        )));
    }

    Ok(SpawnTerminalResponse {
        session_id,
        daemon_epoch: spawn_result.epoch.to_string(),
        session: spawn_result.session,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpawnTerminalBatchRequest {
    pub spawns: Vec<SpawnTerminalRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpawnTerminalBatchEntry {
    pub index: usize,
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Batch spawn for workspace restore/recovery: collapses N per-session spawn
/// invocations into one IPC round trip. Each entry spawns independently and
/// reports its own failure without aborting the rest of the batch.
#[tauri::command]
pub async fn cmd_terminal_spawn_batch<R: Runtime>(
    app: AppHandle<R>,
    daemon_client: State<'_, Arc<DaemonClient>>,
    registry: State<'_, WorkspaceRegistry>,
    request: SpawnTerminalBatchRequest,
) -> Result<Vec<SpawnTerminalBatchEntry>, IpcError> {
    let mut entries = Vec::with_capacity(request.spawns.len());
    for (index, spawn) in request.spawns.into_iter().enumerate() {
        match cmd_terminal_spawn(app.clone(), daemon_client.clone(), registry.clone(), spawn).await
        {
            Ok(response) => entries.push(SpawnTerminalBatchEntry {
                index,
                session_id: Some(response.session_id),
                error: None,
            }),
            Err(err) => entries.push(SpawnTerminalBatchEntry {
                index,
                session_id: None,
                error: Some(err.message),
            }),
        }
    }
    Ok(entries)
}

fn hub_attach_retry_deadline() -> std::time::Duration {
    #[cfg(test)]
    {
        if let Ok(ms_str) = std::env::var("FERRYX_TEST_HUB_ATTACH_TIMEOUT_MS") {
            if let Ok(ms) = ms_str.parse::<u64>() {
                return std::time::Duration::from_millis(ms);
            }
        }
    }
    std::time::Duration::from_secs(10)
}

#[tauri::command]
pub async fn cmd_terminal_attach<R: Runtime>(
    app: AppHandle<R>,
    daemon_client: State<'_, Arc<DaemonClient>>,
    session_id: String,
    after_sequence: Option<String>,
) -> Result<AttachTerminalResponse, IpcError> {
    let after_seq = after_sequence.as_deref().and_then(|s| s.parse::<u64>().ok());

    let (attachment, target_session_id) = if session_id.starts_with("daemon-session:") {
        match daemon_client.paired_terminal_descriptor(session_id.clone()).await {
            Ok(Some(mut descriptor)) => {
                let host_id = descriptor.host_id.clone();
                let generation = descriptor.generation;
                if let Some(seq) = after_seq {
                    descriptor.after_sequence = Some(crate::scoped_contracts::Epoch(seq));
                }
                let effective_after_seq = after_seq.or(descriptor.after_sequence.map(|e| e.0));

                let (proxy_session_id, proxy_gen) = daemon_client
                    .paired_terminal_reattach(descriptor)
                    .await
                    .map_err(|client_err| map_client_error(&client_err, Some(&host_id), Some(generation)))?;

                let timeout_duration = hub_attach_retry_deadline();
                let attach_deadline = tokio::time::Instant::now() + timeout_duration;
                let mut poll_interval = std::time::Duration::from_millis(50);

                loop {
                    match daemon_client.attach(&proxy_session_id, effective_after_seq).await {
                        Ok(att) => break (att, proxy_session_id),
                        Err(err) if err.code == IpcErrorCode::SessionNotFound => {
                            if tokio::time::Instant::now() >= attach_deadline {
                                return Err(IpcError::new(
                                    IpcErrorCode::OperationOutcomeUnknown,
                                    format!("Local proxy attachment pending for paired session '{session_id}'"),
                                )
                                .with_details(serde_json::json!({
                                    "pairedProxyPending": true,
                                    "sessionId": session_id,
                                    "proxySessionId": proxy_session_id,
                                    "generation": proxy_gen,
                                    "hostId": host_id,
                                })));
                            }
                            tokio::time::sleep(poll_interval).await;
                            if poll_interval < std::time::Duration::from_millis(250) {
                                poll_interval = poll_interval.saturating_mul(2);
                            }
                        }
                        Err(err) => return Err(err),
                    }
                }
            }
            Ok(None) => {
                let att = daemon_client.attach(&session_id, after_seq).await?;
                (att, session_id.clone())
            }
            Err(e) if e.ambiguous || matches!(e.code.as_str(), "TIMEOUT" | "OPERATION_OUTCOME_UNKNOWN") => {
                return Err(map_client_error(&e, None, None));
            }
            Err(_) => {
                let att = daemon_client.attach(&session_id, after_seq).await?;
                (att, session_id.clone())
            }
        }
    } else {
        let att = daemon_client.attach(&session_id, after_seq).await?;
        (att, session_id.clone())
    };

    let resp = AttachTerminalResponse {
        session_id: attachment.session_id.clone(),
        daemon_epoch: Some(attachment.epoch.to_string()),
        history_start_sequence: attachment.start_sequence.map(|s| s.to_string()),
        history_end_sequence: attachment.end_sequence.map(|s| s.to_string()),
        history: STANDARD.encode(&attachment.history),
        gap: attachment.gap.as_ref().map(|g| TerminalReplayGap {
            requested_after_sequence: g.requested_after_sequence.to_string(),
            available_from_sequence: g.available_from_sequence.to_string(),
        }),
    };

    if session_id != target_session_id {
        stop_managed_pump(&session_id);
    }
    start_managed_pump(target_session_id, app, attachment);

    Ok(resp)
}

#[tauri::command]
pub async fn cmd_terminal_history_snapshot(
    daemon_client: State<'_, Arc<DaemonClient>>,
    session_id: String,
) -> Result<String, IpcError> {
    let attachment = daemon_client.attach(&session_id, None).await?;
    let history = String::from_utf8_lossy(&attachment.history).into_owned();
    attachment.stream_task.abort();
    Ok(history)
}

#[tauri::command]
pub async fn cmd_terminal_describe(
    daemon_client: State<'_, Arc<DaemonClient>>,
    session_id: String,
) -> Result<crate::daemon::protocol::DaemonSessionDetails, IpcError> {
    daemon_client.describe_session(&session_id).await
}

#[tauri::command]
pub async fn cmd_terminal_get_cwd(
    daemon_client: State<'_, Arc<DaemonClient>>,
    session_id: String,
) -> Result<TerminalCwdResponse, IpcError> {
    if let Some(cwd) = get_cached_cwd(&session_id) {
        return Ok(TerminalCwdResponse { cwd });
    }

    let details = daemon_client.describe_session(&session_id).await?;
    let cwd_str = details
        .cwd
        .ok_or_else(|| IpcError::internal("terminal cwd is unavailable"))?;
    let cwd = PathBuf::from(cwd_str);
    update_cached_cwd(session_id, cwd.clone());
    Ok(TerminalCwdResponse { cwd })
}

#[cfg(target_os = "macos")]
mod macos_proc {
    use std::ffi::CStr;
    use std::path::PathBuf;

    const PROC_PIDVNODEPATHINFO: libc::c_int = 9;
    const MAXPATHLEN: usize = 1024;

    #[repr(C)]
    struct VinfoStat {
        vst_dev: u32,
        vst_mode: u16,
        vst_nlink: u16,
        vst_ino: u64,
        vst_uid: libc::uid_t,
        vst_gid: libc::gid_t,
        vst_atime: i64,
        vst_atimensec: i64,
        vst_mtime: i64,
        vst_mtimensec: i64,
        vst_ctime: i64,
        vst_ctimensec: i64,
        vst_birthtime: i64,
        vst_birthtimensec: i64,
        vst_size: libc::off_t,
        vst_blocks: i64,
        vst_blksize: i32,
        vst_flags: u32,
        vst_gen: u32,
        vst_rdev: u32,
        vst_qspare: [i64; 2],
    }

    #[repr(C)]
    struct VnodeInfo {
        vi_stat: VinfoStat,
        vi_type: libc::c_int,
        vi_pad: libc::c_int,
        vi_fsid: libc::fsid_t,
    }

    #[repr(C)]
    struct VnodeInfoPath {
        vip_vi: VnodeInfo,
        vip_path: [libc::c_char; MAXPATHLEN],
    }

    #[repr(C)]
    struct ProcVnodePathInfo {
        pvi_cdir: VnodeInfoPath,
        pvi_rdir: VnodeInfoPath,
    }

    extern "C" {
        fn proc_pidinfo(
            pid: libc::c_int,
            flavor: libc::c_int,
            arg: u64,
            buffer: *mut libc::c_void,
            buffersize: libc::c_int,
        ) -> libc::c_int;
    }

    pub fn get_proc_cwd(pid: u32) -> Option<PathBuf> {
        let mut info = std::mem::MaybeUninit::<ProcVnodePathInfo>::uninit();
        let size = std::mem::size_of::<ProcVnodePathInfo>() as libc::c_int;
        // SAFETY: proc_pidinfo safely writes up to `size` bytes into the uninit buffer.
        let ret = unsafe {
            proc_pidinfo(
                pid as libc::c_int,
                PROC_PIDVNODEPATHINFO,
                0,
                info.as_mut_ptr().cast(),
                size,
            )
        };
        if ret <= 0 {
            return None;
        }
        // SAFETY: proc_pidinfo succeeded (ret > 0) and initialized `info`.
        let info = unsafe { info.assume_init() };
        let path_bytes = &info.pvi_cdir.vip_path;
        let nul_pos = path_bytes.iter().position(|&c| c == 0)?;
        if nul_pos == 0 {
            return None;
        }
        let cstr = unsafe { CStr::from_ptr(path_bytes.as_ptr()) };
        cstr.to_str().ok().map(PathBuf::from)
    }
}

#[cfg(target_os = "windows")]
#[path = "windows_process_cwd.rs"]
mod windows_process_cwd;

pub fn process_cwd(pid: u32) -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        return std::fs::read_link(format!("/proc/{pid}/cwd")).ok();
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(path) = macos_proc::get_proc_cwd(pid) {
            return Some(path);
        }
        let output = std::process::Command::new("/usr/sbin/lsof")
            .args(["-a", "-p", &pid.to_string(), "-d", "cwd", "-Fn"])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let stdout = String::from_utf8(output.stdout).ok()?;
        return stdout
            .lines()
            .find_map(|line| line.strip_prefix('n'))
            .filter(|path| !path.is_empty())
            .map(PathBuf::from);
    }

    #[cfg(target_os = "windows")]
    {
        windows_process_cwd::process_cwd(pid)
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = pid;
        None
    }
}

#[tauri::command]
pub async fn cmd_terminal_write(
    daemon_client: State<'_, Arc<DaemonClient>>,
    session_id: String,
    data: String,
) -> Result<(), IpcError> {
    daemon_client
        .write_terminal(&session_id, data.into_bytes())
        .await
}

#[tauri::command]
pub async fn cmd_terminal_resize(
    daemon_client: State<'_, Arc<DaemonClient>>,
    session_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), IpcError> {
    daemon_client.resize_terminal(&session_id, cols, rows).await
}

#[tauri::command]
pub async fn cmd_terminal_remote_write(daemon_client: State<'_, Arc<DaemonClient>>, session_id: String, generation: u64, data: String) -> Result<(), IpcError> {
    daemon_client.write_terminal_at_generation(&session_id, Some(generation), data.into_bytes()).await
}

#[tauri::command]
pub async fn cmd_terminal_remote_resize(daemon_client: State<'_, Arc<DaemonClient>>, session_id: String, generation: u64, cols: u16, rows: u16) -> Result<(), IpcError> {
    daemon_client.resize_terminal_at_generation(&session_id, Some(generation), cols, rows).await
}

pub(crate) fn remote_control_result(reply: crate::daemon::protocol::DaemonResponse) -> Result<(), IpcError> {
    use crate::daemon::protocol::DaemonResponse;
    match reply {
        DaemonResponse::WriteOk | DaemonResponse::ResizeOk => Ok(()),
        DaemonResponse::RemoteSessionError { failure } => Err(IpcError::internal(failure.to_string()).with_details(serde_json::to_value(failure).map_err(|e| IpcError::internal(e.to_string()))?)),
        DaemonResponse::Error { message, .. } => Err(IpcError::internal(message)),
        _ => Err(IpcError::internal("Unexpected remote control response")),
    }
}

#[tauri::command]
pub async fn cmd_terminal_signal(
    daemon_client: State<'_, Arc<DaemonClient>>,
    session_id: String,
    signal: TerminalSignalRequest,
) -> Result<(), IpcError> {
    daemon_client
        .signal_terminal(&session_id, signal.into())
        .await
}

#[tauri::command]
pub async fn cmd_terminal_remote_status(
    daemon_client: State<'_, Arc<DaemonClient>>,
    session_id: String,
) -> Result<crate::daemon::protocol::DaemonResponse, IpcError> {
    daemon_client.remote_session_status(&session_id).await
}

#[tauri::command]
pub async fn cmd_terminal_remote_retry(
    daemon_client: State<'_, Arc<DaemonClient>>,
    session_id: String,
) -> Result<crate::daemon::protocol::DaemonResponse, IpcError> {
    daemon_client.retry_remote_session(&session_id).await
}

#[tauri::command]
pub async fn cmd_terminal_detach(
    daemon_client: State<'_, Arc<DaemonClient>>,
    session_id: String,
) -> Result<(), IpcError> {
    invalidate_cached_cwd(&session_id);
    daemon_client.detach_terminal(&session_id).await
}

#[tauri::command]
pub async fn cmd_terminal_close(
    daemon_client: State<'_, Arc<DaemonClient>>,
    session_id: String,
) -> Result<(), IpcError> {
    invalidate_cached_cwd(&session_id);
    daemon_client.close_terminal(&session_id).await
}

#[tauri::command]
pub async fn cmd_terminal_hibernate(
    daemon_client: State<'_, Arc<DaemonClient>>,
    session_id: String,
) -> Result<(), IpcError> {
    invalidate_cached_cwd(&session_id);
    daemon_client.hibernate_terminal(&session_id).await
}

#[tauri::command]
pub async fn cmd_terminal_suspend(
    daemon_client: State<'_, Arc<DaemonClient>>,
    session_id: String,
) -> Result<(), IpcError> {
    daemon_client.suspend_terminal(&session_id).await
}

#[tauri::command]
pub async fn cmd_terminal_resume(
    daemon_client: State<'_, Arc<DaemonClient>>,
    session_id: String,
) -> Result<(), IpcError> {
    daemon_client.resume_terminal(&session_id).await
}

#[tauri::command]
pub async fn cmd_terminal_list(
    daemon_client: State<'_, Arc<DaemonClient>>,
) -> Result<Vec<TerminalSessionSummary>, IpcError> {
    let session_ids = daemon_client.list_sessions().await?;
    let mut summaries = Vec::new();
    for session_id in session_ids {
        if let Ok(details) = daemon_client.describe_session(&session_id).await {
            summaries.push(TerminalSessionSummary {
                session_id,
                worktree_path: details.cwd.map(PathBuf::from),
                running: details.running,
            });
        }
    }
    Ok(summaries)
}

pub(crate) fn map_client_error(
    err: &crate::paired_host::client::ClientError,
    host_id: Option<&str>,
    generation: Option<crate::scoped_contracts::Epoch>,
) -> IpcError {
    let code = IpcErrorCode::from_code_str(&err.code);
    let message = err
        .machine_error
        .as_ref()
        .map(|m| m.message.clone())
        .filter(|m| !m.trim().is_empty())
        .unwrap_or_else(|| err.code.clone());

    let mut details = serde_json::Map::new();
    if let Some(ref req_id) = err.request_id {
        details.insert("requestId".to_string(), serde_json::Value::String(req_id.clone()));
    } else if let Some(ref me) = err.machine_error {
        if !me.request_id.is_empty() {
            details.insert("requestId".to_string(), serde_json::Value::String(me.request_id.clone()));
        }
    }

    let ambiguous = err.ambiguous || matches!(err.code.as_str(), "TIMEOUT" | "OPERATION_OUTCOME_UNKNOWN");
    details.insert("ambiguous".to_string(), serde_json::Value::Bool(ambiguous));

    if let Some(ref me) = err.machine_error {
        if let Ok(val) = serde_json::to_value(me) {
            details.insert("machineError".to_string(), val);
        }
    }

    if let Some(h) = host_id {
        details.insert("hostId".to_string(), serde_json::Value::String(h.to_string()));
    }

    if let Some(g) = generation {
        details.insert("generation".to_string(), serde_json::json!(g));
    }

    IpcError::new(code, message).with_details(serde_json::Value::Object(details))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::IpcErrorCode;

    #[test]
    fn test_p08_client_error_surfaces_as_typed_ipc_error_code_and_not_internal() {
        // G009-A & P08: Assert that a daemon/paired failure (e.g. SESSION_NOT_FOUND)
        // surfaces as a TYPED IpcErrorCode variant,
        // preserving requestId, ambiguous, machineError, hostId, generation in details,
        // and FAILS if the mapping is reverted to IpcError::internal("...") (assert code != internal).
        let client_err = crate::paired_host::client::ClientError {
            code: "SESSION_NOT_FOUND".to_string(),
            machine_error: Some(crate::remote::machine_protocol::MachineError {
                code: "SESSION_NOT_FOUND".to_string(),
                message: "Remote session terminated".to_string(),
                retryable: false,
                request_id: "req-test-123".to_string(),
                details: serde_json::Map::new(),
            }),
            request_id: Some("req-test-123".to_string()),
            ambiguous: false,
        };

        let ipc_err = map_client_error(
            &client_err,
            Some("paired-host-42"),
            Some(crate::scoped_contracts::Epoch(10)),
        );

        // Crucial mutation proof assertions:
        assert_eq!(ipc_err.code, IpcErrorCode::SessionNotFound);
        assert_ne!(ipc_err.code, IpcErrorCode::InternalError);
        assert_eq!(ipc_err.message, "Remote session terminated");

        let details = ipc_err.details.expect("expected structured details");
        assert_eq!(details.get("requestId").and_then(|v| v.as_str()), Some("req-test-123"));
        assert_eq!(details.get("ambiguous").and_then(|v| v.as_bool()), Some(false));
        assert_eq!(details.get("hostId").and_then(|v| v.as_str()), Some("paired-host-42"));
        assert_eq!(details.get("generation").and_then(|v| v.as_str()), Some("10"));
        assert!(details.get("machineError").is_some());
    }

    #[test]
    fn test_p08_client_error_variants_and_passthrough_mapping() {
        // Test various produced codes map to typed variants
        for (code, expected) in [
            ("SESSION_EXPIRED", IpcErrorCode::SessionExpired),
            ("PARENT_SESSION_MISMATCH", IpcErrorCode::ParentSessionMismatch),
            ("TIMEOUT", IpcErrorCode::Timeout),
            ("HOST_UNAVAILABLE", IpcErrorCode::HostUnavailable),
            ("OPERATION_OUTCOME_UNKNOWN", IpcErrorCode::OperationOutcomeUnknown),
        ] {
            let err = crate::paired_host::client::ClientError::local(code);
            let ipc = map_client_error(&err, None, None);
            assert_eq!(ipc.code, expected);
            assert_ne!(ipc.code, IpcErrorCode::InternalError);
        }

        // Test unknown code keeps stable passthrough mapping (not INTERNAL_ERROR)
        let custom_err = crate::paired_host::client::ClientError::local("UNKNOWN_PAIRED_CODE_XYZ");
        let ipc = map_client_error(&custom_err, None, None);
        assert_ne!(ipc.code, IpcErrorCode::InternalError);
        let serialized_code = serde_json::to_value(&ipc.code).unwrap();
        assert_eq!(serialized_code, "UNKNOWN_PAIRED_CODE_XYZ");
    }
}

