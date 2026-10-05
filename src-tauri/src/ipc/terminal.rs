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
use std::path::{Path, PathBuf};
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
const TERMINAL_OUTPUT_FRAME_VERSION_V2: u8 = 2;
const TERMINAL_OUTPUT_FRAME_FIXED_BYTES: usize = 20;
const TERMINAL_OUTPUT_FRAME_HAS_SEQUENCE: u8 = 1 << 0;
const TERMINAL_OUTPUT_FRAME_HAS_DAEMON_EPOCH: u8 = 1 << 1;
const TERMINAL_OUTPUT_FRAME_HAS_GAP: u8 = 1 << 2;
const TERMINAL_OUTPUT_FRAME_GAP_BYTES: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalReplayGapFields {
    pub requested_after_sequence: u64,
    pub available_from_sequence: u64,
    pub start_sequence: u64,
    pub end_sequence: u64,
}

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

/// A session cwd is only meaningful as an absolute path. A probe that captured command output
/// (`cwd|rtd info error: No such file or directory`), a relative leftover, or anything carrying
/// control characters must never be treated as a directory, served to callers, or inherited by
/// another pane's spawn.
pub(crate) fn is_plausible_absolute_cwd(path: &Path) -> bool {
    path.is_absolute() && !path.to_string_lossy().chars().any(char::is_control)
}

/// [`is_plausible_absolute_cwd`] plus a filesystem check. Blocking: call it only from
/// `run_blocking` contexts, never from the async request loop.
pub(crate) fn is_usable_terminal_cwd(path: &Path) -> bool {
    is_plausible_absolute_cwd(path) && path.is_dir()
}

/// Discards a requested cwd that is not an existing directory. An inherited cwd (or one pinned by
/// the frontend from stale state) can be a probe's captured error text rather than a path, and
/// treating that as a cwd aborts the spawn after its pane already exists, which strands the pane on
/// the "Shell exited" overlay. Returning `None` makes the caller fall back to the worktree root.
pub(crate) fn select_requested_cwd(requested: Option<PathBuf>) -> Option<PathBuf> {
    match requested {
        Some(cwd) if is_usable_terminal_cwd(&cwd) => Some(cwd),
        Some(rejected) => {
            eprintln!(
                "[cmd_terminal_spawn] stage=discard_cwd reason=not_a_directory value={:?}",
                rejected.to_string_lossy()
            );
            None
        }
        None => None,
    }
}

/// Resolves the cwd a spawn will use. A discarded requested cwd makes the pane spawn in the
/// worktree root, while a requested cwd that exists but leaves the worktree still fails.
fn resolve_spawn_cwd(
    requested: Option<PathBuf>,
    worktree_root: PathBuf,
    canonicalize: impl FnOnce(&Path) -> Result<PathBuf, IpcError>,
) -> Result<PathBuf, IpcError> {
    let Some(requested) = select_requested_cwd(requested) else {
        return Ok(worktree_root);
    };
    let canonical = canonicalize(&requested)?;
    if canonical != worktree_root && !canonical.starts_with(&worktree_root) {
        return Err(IpcError::from(WorktreeError::PathOutsideWorkspace {
            path: requested,
            root: worktree_root,
        }));
    }
    if !canonical.is_dir() {
        return Err(IpcError::from(WorktreeError::InvalidPath {
            path: canonical,
            reason: "terminal cwd must be a directory".into(),
        }));
    }
    Ok(canonical)
}

/// Shape check for a *stored* session cwd that may point at another host (an SSH/paired remote
/// root is a Windows path). Rejects the output-text shapes probes capture (`… error: No such file
/// or directory`) so a poisoned value is never served back to a pane as its cwd.
pub(crate) fn is_plausible_session_cwd_text(value: &str) -> bool {
    if value.is_empty() || value.chars().any(char::is_control) || value.contains('|') {
        return false;
    }
    if Path::new(value).is_absolute() {
        return true;
    }
    // Windows-style absolute paths are not `is_absolute()` when the host is POSIX.
    let bytes = value.as_bytes();
    (bytes.len() >= 3 && bytes[1] == b':' && matches!(bytes[2], b'\\' | b'/'))
        || value.starts_with("\\\\")
}

struct PumpHandle {
    token: PumpToken,
    task: tokio::task::JoinHandle<()>,
    stream_task: tokio::task::JoinHandle<()>,
}

static ACTIVE_PUMPS: Mutex<Option<HashMap<String, PumpHandle>>> = Mutex::new(None);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PumpToken(u64);
static NEXT_PUMP_TOKEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
static PUMP_BINDINGS: Mutex<Option<HashMap<String, PumpToken>>> = Mutex::new(None);

pub fn reserve_managed_pump(session_id: &str) -> PumpToken {
    let mut pumps = ACTIVE_PUMPS.lock();
    let mut bindings = PUMP_BINDINGS.lock();
    let token = PumpToken(NEXT_PUMP_TOKEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    bindings.get_or_insert_with(HashMap::new).insert(session_id.into(), token);
    if let Some(previous) = pumps.as_mut().and_then(|map| map.remove(session_id)) {
        previous.task.abort();
        previous.stream_task.abort();
    }
    token
}

pub fn stop_managed_pump_token(session_id: &str, token: PumpToken) {
    let mut pumps = ACTIVE_PUMPS.lock();
    let mut bindings = PUMP_BINDINGS.lock();
    if bindings.as_ref().and_then(|map| map.get(session_id)).copied() != Some(token) { return; }
    bindings.as_mut().map(|map| map.remove(session_id));
    if let Some(pump) = pumps.as_mut().and_then(|map| map.remove(session_id)) {
        pump.task.abort();
        pump.stream_task.abort();
    }
}

pub fn stop_managed_pump(session_id: &str) {
    let mut guard = ACTIVE_PUMPS.lock();
    if let Some(bindings) = PUMP_BINDINGS.lock().as_mut() { bindings.remove(session_id); }
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
    let token = reserve_managed_pump(&session_id);
    start_managed_pump_token(session_id, app_handle, attachment, token);
}

pub fn start_managed_pump_token<R: Runtime>(
    session_id: String, app_handle: AppHandle<R>, attachment: DaemonAttachment, token: PumpToken,
) {
    let mut guard = ACTIVE_PUMPS.lock();
    let bindings = PUMP_BINDINGS.lock();
    if bindings.as_ref().and_then(|map| map.get(&session_id)).copied() != Some(token) {
        attachment.stream_task.abort();
        return;
    }
    drop(bindings);
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
                        let bindings = PUMP_BINDINGS.lock();
                        if bindings.as_ref().and_then(|map| map.get(&session_id_clone)).copied() != Some(token) {
                            return;
                        }
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

            let bindings = PUMP_BINDINGS.lock();
            if bindings.as_ref().and_then(|map| map.get(&session_id_clone)).copied() != Some(token) {
                return;
            }
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
                            Ok(DaemonStreamMessage::RemoteStatus {
                                state,
                                generation,
                                failure,
                                replay_gap,
                                ..
                            }) => {
                                let _ = app.emit("terminal_remote_status", serde_json::json!({"sessionId":session_id_clone,"state":state,"generation":generation,"failure":failure,"replayGap":replay_gap}));
                            }
                            Ok(DaemonStreamMessage::AgentState { .. })
                            | Ok(DaemonStreamMessage::DagRunUpdated { .. })
                            | Ok(DaemonStreamMessage::DagInventory { .. }) => {}
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
                Some(DaemonStreamMessage::RemoteStatus {
                    state,
                    generation,
                    failure,
                    replay_gap,
                    ..
                }) => {
                    let _ = app.emit("terminal_remote_status", serde_json::json!({"sessionId":session_id_clone,"state":state,"generation":generation,"failure":failure,"replayGap":replay_gap}));
                }
                Some(DaemonStreamMessage::AgentState { .. })
                | Some(DaemonStreamMessage::DagRunUpdated { .. })
                | Some(DaemonStreamMessage::DagInventory { .. }) => {}
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
            drop(bindings);
        }

        let bindings = PUMP_BINDINGS.lock();
        if bindings.as_ref().and_then(|map| map.get(&session_id_clone)).copied() != Some(token) {
            return;
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
        drop(bindings);

        let mut guard = ACTIVE_PUMPS.lock();
        if let Some(map) = guard.as_mut() {
            if map.get(&session_id_clone).is_some_and(|pump| pump.token == token) {
                map.remove(&session_id_clone);
            }
        }
    });

    map.insert(session_id, PumpHandle { token, task, stream_task });
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
    #[serde(default)]
    pub create_only: Option<bool>,
    #[serde(default)]
    pub prepared_local_split: Option<crate::daemon::protocol::PreparedLocalSplit>,
    #[serde(default)]
    pub remaining_ms: Option<u64>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incarnation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attach_tuple: Option<crate::daemon::protocol::PaneAttachTuple>,
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
    gap: Option<TerminalReplayGapFields>,
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
    if gap.is_some() {
        flags |= TERMINAL_OUTPUT_FRAME_HAS_GAP;
    }

    let version = if gap.is_some() {
        TERMINAL_OUTPUT_FRAME_VERSION_V2
    } else {
        TERMINAL_OUTPUT_FRAME_VERSION
    };
    let gap_len = if gap.is_some() {
        TERMINAL_OUTPUT_FRAME_GAP_BYTES
    } else {
        0
    };

    let mut frame =
        Vec::with_capacity(TERMINAL_OUTPUT_FRAME_FIXED_BYTES + session_id.len() + gap_len + data.len());
    frame.push(version);
    frame.push(flags);
    frame.extend_from_slice(&session_id_len.to_le_bytes());
    frame.extend_from_slice(&sequence.unwrap_or_default().to_le_bytes());
    frame.extend_from_slice(&daemon_epoch.unwrap_or_default().to_le_bytes());
    frame.extend_from_slice(session_id);
    if let Some(gap) = gap {
        frame.extend_from_slice(&gap.requested_after_sequence.to_le_bytes());
        frame.extend_from_slice(&gap.available_from_sequence.to_le_bytes());
        frame.extend_from_slice(&gap.start_sequence.to_le_bytes());
        frame.extend_from_slice(&gap.end_sequence.to_le_bytes());
    }
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
            encode_terminal_output_frame(session_id, buffer, sequence, daemon_epoch, None)
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
    let channel = TERMINAL_OUTPUT_CHANNEL.lock().clone();
    if let Some(channel) = channel {
        let epoch_u64 = daemon_epoch.and_then(|s| s.parse::<u64>().ok());
        let gap_fields = TerminalReplayGapFields {
            requested_after_sequence,
            available_from_sequence,
            start_sequence: start_sequence.unwrap_or_default(),
            end_sequence: end_sequence.unwrap_or_default(),
        };
        if let Some(frame) = encode_terminal_output_frame(
            session_id,
            history,
            end_sequence,
            epoch_u64,
            Some(gap_fields),
        ) {
            match channel.send(Response::new(frame)) {
                Ok(()) => {
                    crate::terminal::metrics::record_channel_send_for_session(session_id);
                    return true;
                }
                Err(error) => {
                    tracing::debug!("Failed to send terminal replay gap channel frame: {error}");
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
    request_worktree.map(|w| w.slug.clone()).or_else(|| {
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
        (
            remote_workspace_id.to_string(),
            Some(worktree_ident),
            sub_rel,
        )
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

/// Transport-level paired-op failures where the host may or may not have applied
/// the request: the host journal dedups by request id, so resubmitting the same
/// id is idempotent and a journal reconcile is safe to attempt.
pub(crate) fn is_retryable_paired_transport_error(code: &str) -> bool {
    matches!(
        code,
        "PAIRED_HOST_INVALID_RESPONSE" | "TIMEOUT" | "HOST_UNAVAILABLE"
    )
}

/// A create response that never arrived intact is not proof the request was
/// rejected — the journal may still complete it. Such codes must flow into the
/// bounded journal reconcile instead of failing the user action outright.
pub(crate) fn should_reconcile_create_error(ambiguous: bool, code: &str) -> bool {
    ambiguous
        || matches!(
            code,
            "TIMEOUT" | "OPERATION_OUTCOME_UNKNOWN" | "PAIRED_HOST_INVALID_RESPONSE"
        )
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

static PENDING_CREATES: std::sync::LazyLock<
    Mutex<std::collections::HashMap<String, PendingCreateRecord>>,
> = std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

pub fn get_pending_creates() -> std::collections::HashMap<String, PendingCreateRecord> {
    ensure_pending_creates_loaded();
    PENDING_CREATES.lock().clone()
}

pub fn get_pending_create(request_id: &str) -> Option<PendingCreateRecord> {
    ensure_pending_creates_loaded();
    PENDING_CREATES.lock().get(request_id).cloned()
}

pub fn cancel_pending_create(request_id: &str) {
    ensure_pending_creates_loaded();
    {
        let mut guard = PENDING_CREATES.lock();
        if let Some(record) = guard.get_mut(request_id) {
            record.cancelled = true;
        }
    }
    persist_pending_creates();
}

// P10 (round 2): pending creates survive process restarts. The paired host's
// operation journal is the durable record; the local file keeps the logical
// intent (adopt-or-cancel) so the reaper revisits records after a restart.
static PENDING_CREATES_STORE_LOADED: std::sync::OnceLock<()> = std::sync::OnceLock::new();

#[cfg(not(test))]
fn default_pending_creates_path() -> Option<std::path::PathBuf> {
    if let Ok(dir) = std::env::var("FERRYX_PAIRED_PENDING_CREATES_DIR") {
        return Some(std::path::PathBuf::from(dir).join("paired_pending_creates.json"));
    }
    crate::remote::auth::canonical_remote_dir().map(|dir| dir.join("paired_pending_creates.json"))
}
#[cfg(test)]
fn default_pending_creates_path() -> Option<std::path::PathBuf> {
    None
}

fn ensure_pending_creates_loaded() {
    if PENDING_CREATES_STORE_LOADED.set(()).is_err() {
        return;
    }
    let Some(path) = default_pending_creates_path() else {
        return;
    };
    let Ok(data) = std::fs::read(&path) else {
        return;
    };
    if let Ok(map) =
        serde_json::from_slice::<std::collections::HashMap<String, PendingCreateRecord>>(&data)
    {
        PENDING_CREATES.lock().extend(map);
    } else {
        eprintln!("[ipc::terminal] pending-create store unreadable; starting empty");
    }
}

fn persist_pending_creates() {
    let Some(path) = default_pending_creates_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let guard = PENDING_CREATES.lock();
    // Only still-reconcilable records belong on disk; terminal records are
    // dropped so the file never grows unboundedly.
    let live: std::collections::HashMap<&String, &PendingCreateRecord> = guard
        .iter()
        .filter(|(_, r)| matches!(r.status, PendingCreateStatus::Pending))
        .collect();
    if let Ok(bytes) = serde_json::to_vec_pretty(&live) {
        let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
        if std::fs::write(&tmp, &bytes).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
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
pub(crate) fn register_pending_create(
    host_id: &str,
    generation: crate::scoped_contracts::Epoch,
    request_id: &str,
    cancelled: bool,
) {
    ensure_pending_creates_loaded();
    {
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
    persist_pending_creates();
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
        let deadline = std::time::Instant::now()
            + std::time::Duration::from_secs(PENDING_CREATE_BACKGROUND_WINDOW_SECS);
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(
                PENDING_CREATE_BACKGROUND_POLL_MS,
            ))
            .await;
            if std::time::Instant::now() >= deadline {
                // Window exhausted: leave the record Pending so a later pass
                // (or operator action) can still reconcile it.
                break;
            }
            let (cancelled, still_pending) = {
                let guard = PENDING_CREATES.lock();
                match guard.get(&request_id) {
                    Some(record) => (
                        record.cancelled,
                        matches!(record.status, PendingCreateStatus::Pending),
                    ),
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
            resolve_pending_create_terminal(
                client.clone(),
                host_id.clone(),
                generation,
                request_id.clone(),
                cancelled,
                outcome,
            )
            .await;
            return;
        }
    });
}

/// P10: shared terminal resolution for a pending create whose journal outcome
/// became known. Used by the bounded background reconciler and the long-lived
/// pending-create reaper so both adopt/close identically.
async fn resolve_pending_create_terminal(
    client: DaemonClient,
    host_id: String,
    generation: crate::scoped_contracts::Epoch,
    request_id: String,
    cancelled: bool,
    outcome: crate::remote::machine_protocol::OperationOutcome,
) {
    match outcome {
        crate::remote::machine_protocol::OperationOutcome::Session { session } => {
            if cancelled {
                let close_req = crate::remote::machine_protocol::CloseSessionRequest {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    daemon_epoch: session.target.daemon_epoch.clone(),
                };
                let close_res = client
                    .paired_host_operation(crate::paired_host::client::OperationRequest {
                        host_id: host_id.clone(),
                        generation,
                        operation: crate::paired_host::client::Operation::CloseSession {
                            session_id: session.target.session_id.clone(),
                            request: close_req,
                        },
                    })
                    .await;
                {
                    let mut guard = PENDING_CREATES.lock();
                    if let Some(record) = guard.get_mut(&request_id) {
                        if close_res.is_ok() {
                            record.status = PendingCreateStatus::Cancelled;
                        } else {
                            record.session = Some(session);
                        }
                    }
                }
                persist_pending_creates();
            } else {
                let descriptor = crate::terminal::paired_daemon::Descriptor {
                    host_id: host_id.clone(),
                    generation,
                    target: session.target.clone(),
                    after_sequence: None,
                };
                let _discovered_session_id = session.target.session_id.clone();
                let reattach = client.paired_terminal_reattach(descriptor).await;
                {
                    let mut guard = PENDING_CREATES.lock();
                    if let Some(record) = guard.get_mut(&request_id) {
                        match reattach {
                            Ok((proxy_session_id, _)) => {
                                record.session = Some(session);
                                record.status = PendingCreateStatus::Adopted { proxy_session_id };
                            }
                            Err(_) => {
                                record.session = Some(session);
                            }
                        }
                    }
                }
                persist_pending_creates();
            }
        }
        crate::remote::machine_protocol::OperationOutcome::Error { error } => {
            {
                let mut guard = PENDING_CREATES.lock();
                if let Some(record) = guard.get_mut(&request_id) {
                    record.status = PendingCreateStatus::Failed {
                        error: error.message.clone(),
                    };
                }
            }
            persist_pending_creates();
        }
        _ => {}
    }
}

const PENDING_CREATE_REAPER_INTERVAL_SECS: u64 = 60;

/// P10 (round 2): production scanner for still-pending creates. The bounded
/// background reconciler only covers the spawn window; this reaper keeps
/// revisiting records that stayed Pending — including ones loaded from disk
/// after a restart — so a late terminal journal outcome is adopted or closed
/// instead of leaking a live remote session.
pub async fn start_pending_create_reaper(daemon_client: Arc<DaemonClient>) {
    static REAPER_STARTED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    if REAPER_STARTED.set(()).is_err() {
        return;
    }
    tokio::spawn(async move {
        loop {
            let pending: Vec<PendingCreateRecord> = {
                ensure_pending_creates_loaded();
                let guard = PENDING_CREATES.lock();
                guard
                    .values()
                    .filter(|r| matches!(r.status, PendingCreateStatus::Pending))
                    .cloned()
                    .collect()
            };
            for record in pending {
                let journal_req = crate::paired_host::client::OperationRequest {
                    host_id: record.host_id.clone(),
                    generation: record.generation,
                    operation: crate::paired_host::client::Operation::Operation {
                        request_id: record.request_id.clone(),
                    },
                };
                let terminal = match daemon_client.paired_host_operation(journal_req).await {
                    Ok(resp) => match resp.result {
                        crate::paired_host::client::OperationResult::Operation(
                            crate::remote::machine_protocol::Operation::Completed {
                                outcome, ..
                            },
                        ) => Some(outcome),
                        _ => None,
                    },
                    Err(_) => None,
                };
                if let Some(outcome) = terminal {
                    resolve_pending_create_terminal(
                        (*daemon_client).clone(),
                        record.host_id.clone(),
                        record.generation,
                        record.request_id.clone(),
                        record.cancelled,
                        outcome,
                    )
                    .await;
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(
                PENDING_CREATE_REAPER_INTERVAL_SECS,
            ))
            .await;
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
                    ) => match outcome {
                        crate::remote::machine_protocol::OperationOutcome::Session { session } => {
                            if cancelled {
                                let close_req =
                                    crate::remote::machine_protocol::CloseSessionRequest {
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
                            return Err(map_client_error(
                                &client_err,
                                Some(host_id),
                                Some(generation),
                            ));
                        }
                        _ => {
                            let unknown_err = crate::paired_host::client::ClientError {
                                code: "OPERATION_OUTCOME_UNKNOWN".to_string(),
                                machine_error: None,
                                request_id: Some(request_id.to_string()),
                                ambiguous: true,
                            };
                            return Err(map_client_error(
                                &unknown_err,
                                Some(host_id),
                                Some(generation),
                            ));
                        }
                    },
                    crate::paired_host::client::OperationResult::Operation(
                        crate::remote::machine_protocol::Operation::Pending { .. },
                    )
                    | crate::paired_host::client::OperationResult::Operation(
                        crate::remote::machine_protocol::Operation::OutcomeUnknown { .. },
                    ) => {
                        if attempts >= MAX_RECONCILE_ATTEMPTS {
                            register_pending_create(host_id, generation, request_id, cancelled);
                            spawn_background_create_reconciler(
                                daemon_client,
                                host_id,
                                generation,
                                request_id,
                            );
                            let unknown_err = crate::paired_host::client::ClientError {
                                code: "OPERATION_OUTCOME_UNKNOWN".to_string(),
                                machine_error: None,
                                request_id: Some(request_id.to_string()),
                                ambiguous: true,
                            };
                            return Err(map_client_error(
                                &unknown_err,
                                Some(host_id),
                                Some(generation),
                            ));
                        }
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                    _ => {
                        if attempts >= MAX_RECONCILE_ATTEMPTS {
                            register_pending_create(host_id, generation, request_id, cancelled);
                            spawn_background_create_reconciler(
                                daemon_client,
                                host_id,
                                generation,
                                request_id,
                            );
                            let unknown_err = crate::paired_host::client::ClientError {
                                code: "OPERATION_OUTCOME_UNKNOWN".to_string(),
                                machine_error: None,
                                request_id: Some(request_id.to_string()),
                                ambiguous: true,
                            };
                            return Err(map_client_error(
                                &unknown_err,
                                Some(host_id),
                                Some(generation),
                            ));
                        }
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                }
            }
            Err(e) if e.code == "OPERATION_NOT_FOUND" => {
                return Err(map_client_error(&e, Some(host_id), Some(generation)));
            }
            Err(_e) if attempts < MAX_RECONCILE_ATTEMPTS => {
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
            Err(e) => {
                register_pending_create(host_id, generation, request_id, cancelled);
                spawn_background_create_reconciler(daemon_client, host_id, generation, request_id);
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

#[cfg(not(test))]
fn default_pending_cleanups_path() -> Option<std::path::PathBuf> {
    if let Ok(dir) = std::env::var("FERRYX_PAIRED_PENDING_CLEANUPS_DIR") {
        return Some(std::path::PathBuf::from(dir).join("paired_pending_cleanups.json"));
    }
    crate::remote::auth::canonical_remote_dir().map(|dir| dir.join("paired_pending_cleanups.json"))
}
#[cfg(test)]
fn default_pending_cleanups_path() -> Option<std::path::PathBuf> {
    if let Ok(dir) = std::env::var("FERRYX_PAIRED_PENDING_CLEANUPS_DIR") {
        return Some(std::path::PathBuf::from(dir).join("paired_pending_cleanups.json"));
    }
    None
}

fn ensure_pending_cleanups_loaded() {
    static LOADED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    if LOADED.set(()).is_err() {
        return;
    }
    let Some(path) = default_pending_cleanups_path() else {
        return;
    };
    if !path.exists() {
        return;
    }
    let data = match std::fs::read(&path) {
        Ok(d) => d,
        Err(_) => return,
    };
    #[derive(serde::Deserialize)]
    struct SavedCleanups {
        pending: Vec<CleanupRecord>,
        exhausted: Vec<CleanupRecord>,
    }
    if let Ok(saved) = serde_json::from_slice::<SavedCleanups>(&data) {
        PENDING_CLEANUPS.lock().extend(saved.pending);
        EXHAUSTED_CLEANUPS.lock().extend(saved.exhausted);
    } else {
        eprintln!("[ipc::terminal] pending-cleanup store unreadable; starting empty");
    }
}

fn persist_pending_cleanups() {
    let Some(path) = default_pending_cleanups_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    #[derive(serde::Serialize)]
    struct SavedCleanups<'a> {
        pending: &'a Vec<CleanupRecord>,
        exhausted: &'a Vec<CleanupRecord>,
    }
    let pending_guard = PENDING_CLEANUPS.lock();
    let exhausted_guard = EXHAUSTED_CLEANUPS.lock();
    let saved = SavedCleanups {
        pending: &*pending_guard,
        exhausted: &*exhausted_guard,
    };
    if let Ok(bytes) = serde_json::to_vec_pretty(&saved) {
        let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
        if std::fs::write(&tmp, &bytes).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

pub fn get_pending_cleanups() -> Vec<CleanupRecord> {
    ensure_pending_cleanups_loaded();
    PENDING_CLEANUPS.lock().clone()
}

pub fn get_exhausted_cleanups() -> Vec<CleanupRecord> {
    ensure_pending_cleanups_loaded();
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
    ensure_pending_cleanups_loaded();
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
        Err(e)
            if e.ambiguous
                || matches!(e.code.as_str(), "TIMEOUT" | "OPERATION_OUTCOME_UNKNOWN") =>
        {
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
                        crate::remote::machine_protocol::Operation::Completed {
                            outcome: crate::remote::machine_protocol::OperationOutcome::NoContent,
                            ..
                        },
                    ) = resp.result
                    {
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
                        drop(guard);
                        persist_pending_cleanups();
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
                    drop(guard);
                    persist_pending_cleanups();
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
    ensure_pending_cleanups_loaded();
    // P11: the reaper works on a cloned snapshot while every unresolved record
    // stays in the authoritative pending vector — persistable at all times, so a
    // restart during any network await cannot lose an outstanding cleanup intent.
    let snapshot: Vec<CleanupRecord> = PENDING_CLEANUPS.lock().clone();

    let mut resolved_count = 0;
    const MAX_REAP_ATTEMPTS: usize = 3;

    for mut item in snapshot {
        if item.attempts >= MAX_REAP_ATTEMPTS {
            // P11: exhausted cleanups must be retained, not silently dropped —
            // the uncertainty is still real and must stay observable/diagnosable.
            retain_pending_cleanups(&item.cleanup_request_id);
            EXHAUSTED_CLEANUPS.lock().push(item);
            persist_pending_cleanups();
            continue;
        }
        item.attempts += 1;
        // Persist the attempt bump before the network await: a crash mid-await
        // keeps both the record and its attempt progression on disk.
        upsert_pending_cleanups(item.clone());
        persist_pending_cleanups();

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
                    crate::remote::machine_protocol::Operation::Completed { outcome, .. },
                ) = resp.result
                {
                    retain_pending_cleanups(&item.cleanup_request_id);
                    if matches!(
                        outcome,
                        crate::remote::machine_protocol::OperationOutcome::Error { .. }
                    ) {
                        item.resolved = true;
                        EXHAUSTED_CLEANUPS.lock().push(item);
                    }
                    resolved_count += 1;
                    persist_pending_cleanups();
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
                        retain_pending_cleanups(&item.cleanup_request_id);
                        resolved_count += 1;
                        persist_pending_cleanups();
                        continue;
                    }
                }
            }
        }
    }
    resolved_count
}

fn retain_pending_cleanups(cleanup_request_id: &str) {
    PENDING_CLEANUPS
        .lock()
        .retain(|r| r.cleanup_request_id != cleanup_request_id);
}

fn upsert_pending_cleanups(record: CleanupRecord) {
    let mut guard = PENDING_CLEANUPS.lock();
    if let Some(existing) = guard
        .iter_mut()
        .find(|r| r.cleanup_request_id == record.cleanup_request_id)
    {
        *existing = record;
    } else {
        guard.push(record);
    }
}

fn split_stage_deadline(remaining_ms: u64, cap: u64) -> Result<tokio::time::Instant, IpcError> {
    let budget = crate::daemon::protocol::clip_stage_budget(remaining_ms, cap);
    if budget == 0 {
        return Err(IpcError::new(IpcErrorCode::SpawnAttemptTimeout, "Split attempt budget exhausted"));
    }
    Ok(tokio::time::Instant::now() + Duration::from_millis(budget))
}

fn split_wire_epoch(operation: crate::daemon::protocol::SplitOperationResult)
    -> crate::daemon::protocol::SplitOperationResult<String>
{
    use crate::daemon::protocol::SplitOperationResult as R;
    match operation {
        R::Absent { can_create } => R::Absent { can_create },
        R::Pending { cancel_requested } => R::Pending { cancel_requested },
        R::Created { session_id, daemon_epoch, session, ownership } => R::Created {
            session_id, daemon_epoch: daemon_epoch.to_string(), session, ownership },
        R::Cancelled => R::Cancelled,
        R::Exited => R::Exited,
        R::Failed { error, no_child } => R::Failed { error, no_child },
        R::Unknown { reason } => R::Unknown { reason },
    }
}

// ---------------------------------------------------------------------------
// Pane-liveness QA producers behind `local-split-qa` + `native-terminal`.
//
// The GUI half of the private barrier channel (`crate::ipc::qa_barrier`): the
// create stage's authoritative identity, the held attach handshake, the cancel
// acknowledgement with its authoritative cleanup record, and the two runner
// controls this lane consumes - `retry` (the same-ID retry with an advanced
// attempt generation) and `split-concurrent-batch` (the bounded concurrent split
// load with duplicate and fingerprint-conflict pairs). The daemon half
// (handover transfer/rollback, held remote RPC) is `crate::daemon::qa_producers`.
//
// Each emitter fires from the real stage it names - the returned create result,
// the attach that is about to install its pump, the daemon's own cancel reply -
// and every field is a value this process observed. A stage that cannot be
// established from real observations settles truthfully instead of being
// upgraded into a pass.
#[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
mod qa_split_producers {
    use super::*;
    use crate::daemon::protocol::{
        clip_stage_budget, DaemonSessionDetails, PaneAttachTuple, PreparedLocalSplit,
        SplitAttachAttempt, SplitIdentity, SplitOperationResult, STAGE_ATTACH_OR_LISTENER_MAX_MS,
        STAGE_CREATE_OR_STATUS_MAX_MS,
    };
    use crate::ipc::qa_barrier::{self, QaBarrierChannel, ReleaseOutcome};
    use serde_json::{json, Value};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::Duration;

    pub const SPLIT_CREATE: &str = "split-create";
    pub const ATTACH_HANDSHAKE: &str = "attach-handshake";
    pub const CANCEL_ACK: &str = "cancel-ack";
    const TRIGGER_HANDOVER: &str = "trigger-handover";
    const TRIGGER_HANDOVER_ABORT: &str = "trigger-handover-abort";
    const SPLIT_CANCEL: &str = "split-cancel";
    const WATCH_TICK_MS: u64 = 25;
    const CHANNEL_BOOT_WAIT_MS: u64 = 2_000;
    const CANCEL_IDENTITY_WAIT_MS: u64 = 1_000;
    const DAEMON_CANCEL_BUDGET_MS: u64 = 2_500;
    const LIVENESS_PROBE_MS: u64 = 500;
    /// Runner control: perform the same-ID retry the stalled attach asked for
    /// (`barrierHub.command('retry', ...)` in split-attach-stall).
    const RETRY: &str = "retry";
    /// Runner control: drive the bounded concurrent split batch
    /// (`barrierHub.command('split-concurrent-batch', ...)` in split-concurrent).
    const SPLIT_CONCURRENT_BATCH: &str = "split-concurrent-batch";
    /// Bounded wait for the operation identity a retry must reuse. The scenario
    /// retries a creation that already happened, so the wait only stops a
    /// control from being serviced against an absent record.
    const RETRY_IDENTITY_WAIT_MS: u64 = 2_000;
    /// A retry re-attaches the existing backend: it gets the attach/listener
    /// stage cap, never a create budget.
    const RETRY_ATTACH_BUDGET_MS: u64 = STAGE_ATTACH_OR_LISTENER_MAX_MS;
    /// One batch request gets the same bounded create/status stage budget the
    /// product gives a single user-visible attempt, so no QA request can drive an
    /// unbounded wait against the daemon.
    const BATCH_REQUEST_BUDGET_MS: u64 = STAGE_CREATE_OR_STATUS_MAX_MS;
    /// Hard cap on the batch the product will drive, whatever the control asks
    /// for: the concurrent load stays bounded by construction.
    const BATCH_REQUEST_CAP: u64 = 16;
    /// Durable-request namespace of the batch, so a QA batch can never collide
    /// with a real UI request identity.
    const BATCH_REQUEST_PREFIX: &str = "qa-split-concurrent";

    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    pub fn channel() -> Option<Arc<QaBarrierChannel>> {
        qa_barrier::active_channel()
    }

    /// Async control read: the file read itself is blocking filesystem I/O, so it runs on the
    /// blocking pool while the watcher keeps its exact 25 ms tick.
    async fn read_control(dir: &Path, name: &str, channel: &Arc<QaBarrierChannel>) -> Option<Value> {
        let dir = dir.to_owned();
        let name = name.to_owned();
        let channel = Arc::clone(channel);
        match crate::ipc::run_blocking(move || Ok(read_control_in(&dir, &name, &channel))).await {
            Ok(control) => control,
            // A read that cannot complete is "no control observed yet", never an armed control.
            Err(_) => None,
        }
    }

    fn barrier_dir() -> Option<PathBuf> {
        std::env::var("FERRYX_QA_BARRIER_DIR")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from)
    }

    /// Synchronous half of the control read: a control that does not echo this run's nonces is
    /// never honored. Callers on the async watchers run this through `run_blocking`.
    fn read_control_in(dir: &Path, name: &str, channel: &QaBarrierChannel) -> Option<Value> {
        let text = std::fs::read_to_string(dir.join(format!("{name}.request.json"))).ok()?;
        let value: Value = serde_json::from_str(&text).ok()?;
        if value.get("runId").and_then(Value::as_str) != Some(channel.run_id()) {
            return None;
        }
        let expected = channel.operation_id()?;
        if value.get("operationId").and_then(Value::as_str) != Some(expected.as_str()) {
            return None;
        }
        Some(value)
    }

    /// `Date.prototype.toISOString()` shape (`YYYY-MM-DDTHH:MM:SS.sssZ`), parsed
    /// by hand so the reported dispatch latency never depends on an optional
    /// date-formatting feature being compiled in.
    fn parse_rfc3339_ms(value: &str) -> Option<u64> {
        let bytes = value.as_bytes();
        if bytes.len() < 24
            || bytes[4] != b'-'
            || bytes[7] != b'-'
            || bytes[10] != b'T'
            || bytes[13] != b':'
            || bytes[16] != b':'
            || bytes[19] != b'.'
        {
            return None;
        }
        let number = |range: std::ops::Range<usize>| value.get(range)?.parse::<i64>().ok();
        let year = number(0..4)?;
        let month = number(5..7)?;
        let day = number(8..10)?;
        let hour = number(11..13)?;
        let minute = number(14..16)?;
        let second = number(17..19)?;
        let millis = number(20..23)?;
        if !(1..=12).contains(&month)
            || !(1..=31).contains(&day)
            || !(0..=23).contains(&hour)
            || !(0..=59).contains(&minute)
            || !(0..=60).contains(&second)
        {
            return None;
        }
        let y = if month <= 2 { year - 1 } else { year };
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = y - era * 400;
        let mp = (month + 9) % 12;
        let doy = (153 * mp + 2) / 5 + day - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        let days = era * 146_097 + doe - 719_468;
        let seconds = days * 86_400 + hour * 3_600 + minute * 60 + second;
        u64::try_from(seconds * 1_000 + millis).ok()
    }

    fn operation_state<Epoch>(state: &SplitOperationResult<Epoch>) -> &'static str {
        match state {
            SplitOperationResult::Absent { .. } => "absent",
            SplitOperationResult::Pending { .. } => "pending",
            SplitOperationResult::Created { .. } => "created",
            SplitOperationResult::Cancelled => "cancelled",
            SplitOperationResult::Exited => "exited",
            SplitOperationResult::Failed { .. } => "failed",
            SplitOperationResult::Unknown { .. } => "unknown",
        }
    }

    /// The split operation this process really prepared and (when it got that
    /// far) really created. The cancel watcher needs it because the runner's
    /// cancel control carries no identity of its own, and because a
    /// cancel-before-create must not require the created id.
    #[derive(Debug, Clone)]
    struct RecordedSplit {
        identity: SplitIdentity,
        workspace_id: String,
        source_backend_session_id: Option<String>,
        prepared_at_ms: u64,
        owned_session_id: Option<String>,
        created_at_ms: Option<u64>,
    }

    static RECORDED_SPLIT: std::sync::Mutex<Option<RecordedSplit>> = std::sync::Mutex::new(None);

    fn lock_recorded() -> std::sync::MutexGuard<'static, Option<RecordedSplit>> {
        RECORDED_SPLIT
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The attempt the product really bound. The seven-field binding the attach
    /// stage was about to install names its own incarnation and attempt
    /// generation, and those are exactly the two values a same-ID retry must
    /// retain and advance - so they are read from the real binding instead of
    /// being assumed.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct RecordedAttempt {
        incarnation: Option<String>,
        attempt_generation: u64,
    }

    static RECORDED_ATTEMPT: std::sync::Mutex<Option<RecordedAttempt>> =
        std::sync::Mutex::new(None);

    fn lock_recorded_attempt() -> std::sync::MutexGuard<'static, Option<RecordedAttempt>> {
        RECORDED_ATTEMPT
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Records the binding the split attach really offered, for the SAME backend
    /// this process created. A binding for another session never overwrites the
    /// recorded attempt.
    fn record_bound_attempt(binding: &PaneAttachTuple) {
        let mut guard = lock_recorded_attempt();
        let owns_binding = match lock_recorded().as_ref().and_then(|record| record.owned_session_id.as_deref()) {
            Some(owned) => owned == binding.backend_session_id,
            None => true,
        };
        if !owns_binding {
            return;
        }
        let previous = guard.as_ref().map(|attempt| attempt.attempt_generation);
        *guard = Some(RecordedAttempt {
            incarnation: binding.incarnation.clone(),
            attempt_generation: previous.map_or(binding.attempt_generation, |previous| {
                previous.max(binding.attempt_generation)
            }),
        });
    }

    pub fn record_prepared(identity: &SplitIdentity, request: &SpawnTerminalRequest) {
        *lock_recorded_attempt() = None;
        let mut guard = lock_recorded();
        *guard = Some(RecordedSplit {
            identity: identity.clone(),
            workspace_id: request.workspace_id.clone(),
            source_backend_session_id: request.inherit_from_session_id.clone(),
            prepared_at_ms: now_ms(),
            owned_session_id: None,
            created_at_ms: None,
        });
    }

    fn record_created(identity: &SplitIdentity, session_id: &str) {
        let mut guard = lock_recorded();
        match guard.as_mut() {
            Some(record) if record.identity.request_id == identity.request_id => {
                record.owned_session_id = Some(session_id.to_string());
                record.created_at_ms = Some(now_ms());
            }
            _ => {
                *guard = Some(RecordedSplit {
                    identity: identity.clone(),
                    workspace_id: String::new(),
                    source_backend_session_id: None,
                    prepared_at_ms: now_ms(),
                    owned_session_id: Some(session_id.to_string()),
                    created_at_ms: Some(now_ms()),
                });
            }
        }
    }

    pub fn split_create_payload(
        prepared: &PreparedLocalSplit,
        session_id: &str,
        daemon_epoch: u64,
        session: &DaemonSessionDetails,
        recorded: Option<&RecordedSplit>,
    ) -> Value {
        json!({
            "stage": SPLIT_CREATE,
            "backendSessionId": session_id,
            "incarnation": session.incarnation,
            "daemonEpoch": daemon_epoch.to_string(),
            "running": session.running,
            "workspaceId": session.workspace_id,
            "worktree": session.worktree,
            "cwd": session.cwd,
            "requestId": prepared.identity.request_id,
            "clientRequestId": prepared.identity.request_id,
            "originEpoch": prepared.identity.origin_epoch,
            "expiresAtUnixMs": prepared.identity.expires_at_unix_ms,
            "sourceBackendSessionId": recorded.and_then(|r| r.source_backend_session_id.clone()),
            "preparedAtMs": recorded.map(|r| r.prepared_at_ms),
            "createdAtMs": now_ms(),
            "producerStage": "create-local-split",
        })
    }

    /// The create stage's authoritative result: the identity the runner
    /// correlates plus the created backend session, its incarnation and epoch,
    /// exactly as the journal-confirmed create returned them. Async because the
    /// settlement's receipt append is synchronous file I/O and is awaited
    /// through the off-runtime wrapper.
    pub async fn record_and_emit_split_create(
        prepared: &PreparedLocalSplit,
        session_id: &str,
        daemon_epoch: u64,
        session: &DaemonSessionDetails,
    ) {
        record_created(&prepared.identity, session_id);
        let Some(channel) = channel() else {
            return;
        };
        let Some(operation_id) = channel.operation_id() else {
            return;
        };
        let recorded = lock_recorded().clone();
        let payload = split_create_payload(
            prepared,
            session_id,
            daemon_epoch,
            session,
            recorded.as_ref(),
        );
        qa_barrier::append_receipt_off_runtime(
            &channel,
            SPLIT_CREATE,
            &operation_id,
            payload,
        )
        .await;
    }

    pub enum AttachHold {
        NotApplicable,
        Released,
        Failed(IpcError),
    }

    fn attach_hold_payload(
        session_id: &str,
        binding: &PaneAttachTuple,
        attempt: &SplitAttachAttempt,
        budget_ms: u64,
        actionable: bool,
        release_outcome: &str,
        session_still_alive: Option<bool>,
    ) -> Value {
        json!({
            "stage": ATTACH_HANDSHAKE,
            "actionable": actionable,
            "retryMustReuseId": actionable,
            "releaseOutcome": release_outcome,
            "backendSessionId": session_id,
            "incarnation": binding.incarnation,
            "daemonEpoch": binding.daemon_epoch,
            "frontendSessionId": attempt.frontend_session_id,
            "attemptGeneration": attempt.generation,
            "bindingKey": binding.binding_key,
            "requestId": attempt.identity.request_id,
            "originEpoch": attempt.identity.origin_epoch,
            "attemptBudgetMs": budget_ms,
            "backendSessionStillAlive": session_still_alive,
            "failureClass": if actionable { Some("attach-stall") } else { None },
            "settledAtMs": now_ms(),
        })
    }

    /// Holds the REAL attach stage: called from the split-attach branch of
    /// `cmd_terminal_attach` after the binding is validated and before the pump
    /// is reserved, so a held attempt never installs anything. The hold is
    /// bounded by the attach stage budget, and its settlement is the actionable
    /// same-ID retry failure the runner awaits.
    pub async fn hold_attach_handshake(
        daemon_client: &Arc<DaemonClient>,
        session_id: &str,
        binding: &PaneAttachTuple,
        attempt: &SplitAttachAttempt,
    ) -> AttachHold {
        // Recorded before any barrier decision: the runner's later `retry` control
        // must be fenced against the attempt this process really bound, whether or
        // not the attach-handshake barrier was armed for this run.
        record_bound_attempt(binding);
        let Some(channel) = channel() else {
            return AttachHold::NotApplicable;
        };
        let Some(spec) = channel.spec(ATTACH_HANDSHAKE) else {
            return AttachHold::NotApplicable;
        };
        let Some(operation_id) = channel.operation_id() else {
            return AttachHold::NotApplicable;
        };
        if spec.operation_id != operation_id {
            return AttachHold::NotApplicable;
        }
        if channel
            .target_backend_session_id_for(ATTACH_HANDSHAKE)
            .is_none()
        {
            // The arm binding writes the bound-ack file with a synchronous
            // write, so it runs on the blocking pool.
            if qa_barrier::bind_target_session_off_runtime(
                &channel,
                ATTACH_HANDSHAKE,
                &operation_id,
                session_id,
            )
            .await
            .is_err()
            {
                return AttachHold::NotApplicable;
            }
        } else if !channel.matches_target_session(ATTACH_HANDSHAKE, session_id) {
            return AttachHold::NotApplicable;
        }

        let budget_ms = clip_stage_budget(attempt.remaining_ms, STAGE_ATTACH_OR_LISTENER_MAX_MS).max(1);
        qa_barrier::write_held_off_runtime(
            &channel,
            &spec,
            session_id,
            ATTACH_HANDSHAKE,
            attach_hold_payload(session_id, binding, attempt, budget_ms, false, "held", None),
        )
        .await;

        let mut bounded = spec.clone();
        bounded.deadline_ms = budget_ms.min(spec.deadline_ms);
        match channel.wait_for_release(&bounded).await {
            ReleaseOutcome::Released => {
                qa_barrier::append_receipt_off_runtime(
                    &channel,
                    ATTACH_HANDSHAKE,
                    &operation_id,
                    attach_hold_payload(
                        session_id,
                        binding,
                        attempt,
                        budget_ms,
                        false,
                        ReleaseOutcome::Released.as_str(),
                        None,
                    ),
                )
                .await;
                AttachHold::Released
            }
            ReleaseOutcome::DeadlineExceeded => {
                let probe_deadline = tokio::time::Instant::now()
                    + Duration::from_millis(LIVENESS_PROBE_MS.min(budget_ms));
                let alive = daemon_client
                    .describe_session_bounded_until(session_id, probe_deadline)
                    .await
                    .is_ok();
                qa_barrier::append_receipt_off_runtime(
                    &channel,
                    ATTACH_HANDSHAKE,
                    &operation_id,
                    attach_hold_payload(
                        session_id,
                        binding,
                        attempt,
                        budget_ms,
                        true,
                        ReleaseOutcome::DeadlineExceeded.as_str(),
                        Some(alive),
                    ),
                )
                .await;
                AttachHold::Failed(
                    IpcError::new(
                        IpcErrorCode::SpawnAttemptTimeout,
                        "Attach did not complete within the attempt budget. Retry to reuse the same shell.",
                    )
                    .with_details(json!({
                        "stage": "attach",
                        "backendSessionId": session_id,
                        "incarnation": binding.incarnation,
                        "attemptGeneration": attempt.generation,
                        "retryMustReuseId": true,
                        "backendSessionStillAlive": alive,
                        "delivery": "notSent",
                    })),
                )
            }
        }
    }

    pub fn cleanup_is_authoritative(
        cancelled: bool,
        owned_creation_removed: Option<bool>,
        source_still_present: Option<bool>,
    ) -> bool {
        cancelled
            && owned_creation_removed != Some(false)
            && source_still_present != Some(false)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn cancel_ack_payload(
        record: &RecordedSplit,
        phase: &str,
        duplicate: bool,
        cancel_ack_ms: u64,
        timer_dispatch_latency_ms: Option<u64>,
        operation_state: &str,
        status_state: Option<&str>,
        owned_creation_removed: Option<bool>,
        source_still_present: Option<bool>,
    ) -> Value {
        let cancelled = operation_state == "cancelled" || status_state == Some("cancelled");
        json!({
            "stage": CANCEL_ACK,
            "phase": phase,
            "duplicateCancel": duplicate,
            "cancelAckMs": cancel_ack_ms,
            "timerDispatchLatencyMs": timer_dispatch_latency_ms,
            // The daemon's cancel path tombstones the request identity; it never
            // needs the created session id, which is why a cancel-before-create
            // is acknowledged at all.
            "createdIdRequired": false,
            "requestId": record.identity.request_id,
            "originEpoch": record.identity.origin_epoch,
            "workspaceId": record.workspace_id,
            "ownedSessionId": record.owned_session_id,
            "createdAtMs": record.created_at_ms,
            "operationState": operation_state,
            "cleanupReceipt": {
                "authoritative": cleanup_is_authoritative(
                    cancelled,
                    owned_creation_removed,
                    source_still_present,
                ),
                "operationStateAfterCancel": status_state,
                "ownedCreationRemoved": owned_creation_removed,
                "sourceBackendSessionId": record.source_backend_session_id,
                "sourceStillPresent": source_still_present,
                "duplicateCancel": duplicate,
                "verifiedAtMs": now_ms(),
            },
        })
    }

    /// The runner's `retry` control, field for field
    /// (`scripts/lib/qa-scenarios/split-scenarios.mjs`): the durable request and
    /// backend identity the retry must reuse, plus the attempt generation it must
    /// advance to.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct RetryCommand {
        backend_session_id: String,
        incarnation: Option<String>,
        daemon_epoch: Option<String>,
        attempt_generation: u64,
        client_request_id: Option<String>,
    }

    fn optional_control_string(command: &Value, field: &str) -> Option<String> {
        command
            .get(field)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    }

    impl RetryCommand {
        /// A control that cannot name a concrete backend session and a concrete
        /// generation is refused, never guessed: a wildcard or anonymous retry
        /// would not be the same-ID retry the contract requires.
        fn parse(command: &Value) -> Result<Self, String> {
            let backend_session_id = optional_control_string(command, "backendSessionId")
                .filter(|value| value != "*")
                .ok_or_else(|| "retry requires a concrete backendSessionId".to_string())?;
            let attempt_generation = command
                .get("attemptGeneration")
                .and_then(Value::as_u64)
                .ok_or_else(|| "retry requires a numeric attemptGeneration".to_string())?;
            Ok(Self {
                backend_session_id,
                incarnation: optional_control_string(command, "incarnation"),
                daemon_epoch: optional_control_string(command, "daemonEpoch"),
                attempt_generation,
                client_request_id: optional_control_string(command, "clientRequestId"),
            })
        }
    }

    /// Same-ID retry contract, checked against the operation this process really
    /// recorded: request and backend identity are retained and only the attempt
    /// generation advances. Every refusal names the divergence instead of
    /// retrying under an identity the durable journal does not own.
    fn retry_fence(
        command: &RetryCommand,
        record: &RecordedSplit,
        previous: Option<&RecordedAttempt>,
    ) -> Result<(), String> {
        if let Some(carried) = &command.client_request_id {
            if carried != &record.identity.request_id {
                return Err(format!(
                    "retry must reuse the durable request identity '{}', got '{carried}'",
                    record.identity.request_id
                ));
            }
        }
        if let Some(owned) = &record.owned_session_id {
            if owned != &command.backend_session_id {
                return Err(format!(
                    "retry must reuse the created backend session '{owned}', got '{}'",
                    command.backend_session_id
                ));
            }
        }
        if let Some(previous) = previous {
            if let (Some(carried), Some(recorded)) = (&command.incarnation, &previous.incarnation) {
                if carried != recorded {
                    return Err(format!(
                        "retry must reuse the recorded incarnation '{recorded}', got '{carried}'"
                    ));
                }
            }
            if command.attempt_generation <= previous.attempt_generation {
                return Err(format!(
                    "retry must advance attemptGeneration beyond the bound {}, got {}",
                    previous.attempt_generation, command.attempt_generation
                ));
            }
        }
        Ok(())
    }

    /// The retry's settlement, from the values this process really observed: the
    /// operation state the journal answered with, the incarnation the daemon
    /// reports for the session, and the epoch of the attachment the retry really
    /// installed. `outcome` is `reattached` only for a real attachment;
    /// `refused` carries the contract fence that rejected the command and
    /// `attach-failed` the real attach error - never a fabricated success.
    #[allow(clippy::too_many_arguments)]
    fn retry_payload(
        command: &RetryCommand,
        record: &RecordedSplit,
        previous: Option<&RecordedAttempt>,
        outcome: &str,
        refusal_reason: Option<&str>,
        status_state: Option<&str>,
        session: Option<&DaemonSessionDetails>,
        attach_epoch: Option<u64>,
        history: Option<(Option<u64>, Option<u64>)>,
    ) -> Value {
        json!({
            "stage": RETRY,
            "outcome": outcome,
            "refusalReason": refusal_reason,
            // The runner's contract wording: a retry may never mint a new id.
            "retryMustReuseId": true,
            "backendSessionId": command.backend_session_id,
            "incarnation": session
                .and_then(|details| details.incarnation.clone())
                .or_else(|| command.incarnation.clone()),
            "daemonEpoch": command.daemon_epoch,
            "attachEpoch": attach_epoch.map(|epoch| epoch.to_string()),
            "attemptGeneration": command.attempt_generation,
            "previousAttemptGeneration": previous.map(|attempt| attempt.attempt_generation),
            "requestId": record.identity.request_id,
            "clientRequestId": command.client_request_id,
            "originEpoch": record.identity.origin_epoch,
            "statusOperationState": status_state,
            "workspaceId": session
                .and_then(|details| details.workspace_id.clone())
                .or_else(|| Some(record.workspace_id.clone())),
            "historyStartSequence": history
                .and_then(|(start, _)| start.map(|value| value.to_string())),
            "historyEndSequence": history
                .and_then(|(_, end)| end.map(|value| value.to_string())),
            "settledAtMs": now_ms(),
        })
    }

    /// Performs the REAL same-ID retry through the product's own client path:
    /// the durable journal is asked first (status before any action), and the
    /// retry then re-attaches the SAME backend session. The attachment is
    /// disposed at once because the frontend owns the pane's pump; this proves
    /// the retry landed on the same backend incarnation without minting an id.
    async fn handle_retry(
        daemon_client: &Arc<DaemonClient>,
        channel: &Arc<QaBarrierChannel>,
        command: &RetryCommand,
    ) -> Result<(), String> {
        let operation_id = channel
            .operation_id()
            .ok_or_else(|| "no QA operation nonce".to_string())?;
        let record = wait_for_recorded_split_within(RETRY_IDENTITY_WAIT_MS)
            .await
            .ok_or_else(|| "no split operation identity was recorded in this process".to_string())?;
        let previous = lock_recorded_attempt().clone();
        let deadline =
            tokio::time::Instant::now() + Duration::from_millis(RETRY_ATTACH_BUDGET_MS);
        let settle = |outcome: &str,
                      refusal: Option<&str>,
                      state: Option<&str>,
                      session: Option<&DaemonSessionDetails>,
                      epoch: Option<u64>,
                      history: Option<(Option<u64>, Option<u64>)>| {
            channel.append_receipt(
                RETRY,
                &operation_id,
                retry_payload(
                    command,
                    &record,
                    previous.as_ref(),
                    outcome,
                    refusal,
                    state,
                    session,
                    epoch,
                    history,
                ),
            );
        };

        if let Err(reason) = retry_fence(command, &record, previous.as_ref()) {
            settle("refused", Some(reason.as_str()), None, None, None, None);
            eprintln!("FERRYX_QA_RETRY_REFUSED: {reason}");
            return Ok(());
        }

        // Status first, exactly like the product's own retry: the durable journal
        // is the only authority on whether this request really owns the session.
        let (status_state, session) = match daemon_client
            .local_split_status_until(&record.identity, deadline)
            .await
        {
            Ok(SplitOperationResult::Created {
                session_id,
                session,
                ..
            }) if session_id == command.backend_session_id => ("created", Some(session)),
            Ok(operation) => {
                let state = operation_state(&operation).to_string();
                let reason = format!(
                    "retry requires a journal-confirmed creation of '{}'; the durable status is {state}",
                    command.backend_session_id
                );
                settle("refused", Some(reason.as_str()), Some(state.as_str()), None, None, None);
                eprintln!("FERRYX_QA_RETRY_REFUSED: {reason}");
                return Ok(());
            }
            Err(error) => {
                let reason = format!(
                    "retry status reconciliation failed: {} ({:?})",
                    error.message, error.code
                );
                settle("refused", Some(reason.as_str()), None, None, None, None);
                eprintln!("FERRYX_QA_RETRY_REFUSED: {reason}");
                return Ok(());
            }
        };

        // The incarnation the daemon reports for the session is authoritative; a
        // retry that names a different owner is a stale binding, not a retry.
        if let (Some(carried), Some(live)) = (
            &command.incarnation,
            session.as_ref().and_then(|details| details.incarnation.as_ref()),
        ) {
            if carried != live {
                let reason = format!(
                    "retry incarnation '{carried}' differs from the authoritative owner '{live}'"
                );
                settle("refused", Some(reason.as_str()), Some(status_state), session.as_ref(), None, None);
                eprintln!("FERRYX_QA_RETRY_REFUSED: {reason}");
                return Ok(());
            }
        }

        match daemon_client
            .attach_until(&command.backend_session_id, None, deadline)
            .await
        {
            Ok(attachment) => {
                let epoch = attachment.epoch;
                let history = (attachment.start_sequence, attachment.end_sequence);
                attachment.stream_task.abort();
                settle(
                    "reattached",
                    None,
                    Some(status_state),
                    session.as_ref(),
                    Some(epoch),
                    Some(history),
                );
                Ok(())
            }
            Err(error) => {
                let reason = format!(
                    "same-ID retry attach failed: {} ({:?})",
                    error.message, error.code
                );
                settle(
                    "attach-failed",
                    Some(reason.as_str()),
                    Some(status_state),
                    session.as_ref(),
                    None,
                    None,
                );
                eprintln!("FERRYX_QA_RETRY_UNSETTLED: {reason}");
                Ok(())
            }
        }
    }

    /// Bounded watch for the runner's `retry` control. The scenario pre-arms no
    /// retry barrier, so this control file is the only channel between the runner
    /// and the product; it is read through the channel's own nonce check, serviced
    /// at most once, and left pending (never answered with an invented outcome)
    /// while the operation identity it must reuse is not recorded yet.
    async fn run_retry_watcher(daemon_client: Arc<DaemonClient>) {
        let Some(channel) = wait_for_channel().await else {
            return;
        };
        let Some(dir) = barrier_dir() else {
            return;
        };
        let mut handled: Vec<String> = Vec::new();
        let mut unserviced_reported = false;
        loop {
            tokio::time::sleep(Duration::from_millis(WATCH_TICK_MS)).await;
            let Some(request) = read_control(&dir, RETRY, &channel).await else {
                continue;
            };
            let issued_at = request
                .get("issuedAt")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            if handled.iter().any(|seen| seen == &issued_at) {
                continue;
            }
            let command = match RetryCommand::parse(&request) {
                Ok(command) => command,
                Err(reason) => {
                    handled.push(issued_at);
                    eprintln!("FERRYX_QA_RETRY_UNSERVICEABLE: {reason}");
                    continue;
                }
            };
            match handle_retry(&daemon_client, &channel, &command).await {
                Ok(()) => handled.push(issued_at),
                Err(error) => {
                    if !unserviced_reported {
                        unserviced_reported = true;
                        eprintln!(
                            "FERRYX_QA_RETRY_UNSERVICED: {error}; the command stays pending"
                        );
                    }
                }
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum BatchRequestKind {
        /// Same durable identity and same fingerprint: must be answered from the
        /// record, never by minting a second session.
        Duplicate,
        /// Same durable identity, different fingerprint: must be rejected by the
        /// journal's real fingerprint check.
        Conflict,
    }

    impl BatchRequestKind {
        fn as_str(self) -> &'static str {
            match self {
                BatchRequestKind::Duplicate => "duplicate",
                BatchRequestKind::Conflict => "conflict",
            }
        }
    }

    /// One bounded wave of the batch: `requests` identical real split requests
    /// driven together. Waves run in order, so each wave sees exactly the durable
    /// record the previous waves left behind.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct BatchWave {
        request_id: String,
        kind: BatchRequestKind,
        cols: u16,
        rows: u16,
        requests: u64,
    }

    /// The bounded batch the runner asked for. `count` real split requests over
    /// two durable identities, in four ordered waves: two waves of exact
    /// duplicates of one request (the first may create, the second must be
    /// answered from the record the first left behind), then a wave that
    /// establishes the second identity's base fingerprint, and finally a wave of
    /// that same identity carrying a conflicting fingerprint - which the journal's
    /// own pre-check rejects, deterministically. The count is capped so the
    /// concurrent load stays bounded whatever the control asks for.
    fn batch_plan(
        count: u64,
        test_conflicts: bool,
        base_cols: u16,
        base_rows: u16,
    ) -> Vec<BatchWave> {
        let total = count.clamp(1, BATCH_REQUEST_CAP);
        let duplicate_id = format!("{BATCH_REQUEST_PREFIX}-duplicate");
        let conflict_id = format!("{BATCH_REQUEST_PREFIX}-conflict");
        let conflicting_cols = if base_cols == u16::MAX {
            base_cols - 1
        } else {
            base_cols + 1
        };
        let wave = |request_id: &str, kind: BatchRequestKind, cols: u16, requests: u64| {
            BatchWave {
                request_id: request_id.to_string(),
                kind,
                cols,
                rows: base_rows,
                requests,
            }
        };
        if !test_conflicts {
            // Two waves on one identity: the second can only be answered from the
            // record the first left behind.
            let first = (total + 1) / 2;
            let second = total - first;
            let mut waves = vec![wave(
                &duplicate_id,
                BatchRequestKind::Duplicate,
                base_cols,
                first,
            )];
            if second > 0 {
                waves.push(wave(
                    &duplicate_id,
                    BatchRequestKind::Duplicate,
                    base_cols,
                    second,
                ));
            }
            return waves;
        }
        // Quarter split with the remainder pushed into the later waves, so the
        // conflicting wave is never empty when the control asked for conflicts.
        let mut sizes = [total / 4, total / 4, total / 4, total / 4];
        for index in 0..(total % 4) as usize {
            sizes[3 - index] += 1;
        }
        let mut waves = Vec::with_capacity(sizes.len());
        for (index, requests) in sizes.into_iter().enumerate() {
            if requests == 0 {
                continue;
            }
            waves.push(match index {
                0 | 1 => wave(&duplicate_id, BatchRequestKind::Duplicate, base_cols, requests),
                2 => wave(&conflict_id, BatchRequestKind::Conflict, base_cols, requests),
                _ => wave(
                    &conflict_id,
                    BatchRequestKind::Conflict,
                    conflicting_cols,
                    requests,
                ),
            });
        }
        waves
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct BatchOutcome {
        request_id: String,
        kind: BatchRequestKind,
        outcome: &'static str,
        session_id: Option<String>,
        error_code: Option<String>,
        error_message: Option<String>,
    }

    impl BatchOutcome {
        fn json(&self) -> Value {
            json!({
                "requestId": self.request_id,
                "kind": self.kind.as_str(),
                "outcome": self.outcome,
                "sessionId": self.session_id,
                "errorCode": self.error_code,
                "errorMessage": self.error_message,
            })
        }
    }

    /// The batch's settlement. The idempotency claim is derived from evidence -
    /// more than one duplicate request answered by ONE session id - and a batch
    /// that produced two distinct sessions reports that instead of hiding it.
    #[allow(clippy::too_many_arguments)]
    fn batch_payload(
        requested_count: u64,
        test_conflicts: bool,
        workspace_id: &str,
        request_ids: &[String],
        plan: &[BatchWave],
        outcomes: &[BatchOutcome],
        created_session_ids: &[String],
        closed_session_ids: &[String],
        cleanup_verified: Option<bool>,
        unsettled_reason: Option<&str>,
    ) -> Value {
        let group = |kind: BatchRequestKind| -> Vec<&BatchOutcome> {
            outcomes.iter().filter(|outcome| outcome.kind == kind).collect()
        };
        let session_ids = |list: &[&BatchOutcome]| -> Vec<String> {
            let mut ids: Vec<String> = list
                .iter()
                .filter_map(|outcome| outcome.session_id.clone())
                .collect();
            ids.sort();
            ids.dedup();
            ids
        };
        let duplicates = group(BatchRequestKind::Duplicate);
        let conflicts = group(BatchRequestKind::Conflict);
        let duplicate_ids = session_ids(&duplicates);
        let duplicate_answers = duplicates
            .iter()
            .filter(|outcome| outcome.session_id.is_some())
            .count();
        let mut conflict_codes: Vec<String> = conflicts
            .iter()
            .filter(|outcome| outcome.outcome == "rejected")
            .filter_map(|outcome| outcome.error_code.clone())
            .collect();
        conflict_codes.sort();
        conflict_codes.dedup();
        json!({
            "stage": SPLIT_CONCURRENT_BATCH,
            "requestedCount": requested_count,
            "requestCount": outcomes.len(),
            "plannedRequests": plan.iter().map(|wave| wave.requests).sum::<u64>(),
            "testConflicts": test_conflicts,
            "boundedRequestCap": BATCH_REQUEST_CAP,
            // The load the product accepted is bounded by the runner's cap and by
            // the widest wave, never by the requested number alone.
            "waveCount": plan.len(),
            "boundedConcurrency": plan.iter().map(|wave| wave.requests).max().unwrap_or(0),
            "workspaceId": workspace_id,
            "requestIds": request_ids,
            "duplicateRequests": duplicates.len(),
            "conflictRequests": conflicts.len(),
            // One durable identity per group: many requests, one record, one PTY.
            "duplicateDistinctSessionIds": duplicate_ids,
            "conflictDistinctSessionIds": session_ids(&conflicts),
            "duplicateAnsweredFromRecord": duplicate_answers > 1 && duplicate_ids.len() == 1,
            "conflictRejected": conflicts
                .iter()
                .filter(|outcome| outcome.outcome == "rejected")
                .count(),
            "conflictRejectionCodes": conflict_codes,
            "createdSessionIds": created_session_ids,
            "closedSessionIds": closed_session_ids,
            "cleanupVerified": cleanup_verified,
            "outcomes": outcomes
                .iter()
                .map(BatchOutcome::json)
                .collect::<Vec<Value>>(),
            "unsettledReason": unsettled_reason,
            "settledAtMs": now_ms(),
        })
    }

    /// The real template the batch drives: workspace, cwd, worktree and geometry
    /// read from a live daemon session. Nothing is invented, and a daemon with no
    /// such session leaves the control pending instead of answering it.
    struct BatchTemplate {
        workspace_id: String,
        worktree: Option<crate::worktree::WorktreeIdentity>,
        cwd: String,
        cols: u16,
        rows: u16,
    }

    impl BatchTemplate {
        fn with_identity(&self, identity: SplitIdentity) -> PreparedLocalSplit {
            PreparedLocalSplit {
                identity,
                workspace_id: self.workspace_id.clone(),
                worktree: self.worktree.clone(),
                cwd: self.cwd.clone(),
                shell: None,
                cols: self.cols,
                rows: self.rows,
            }
        }
    }

    async fn batch_template(daemon_client: &Arc<DaemonClient>) -> Option<BatchTemplate> {
        let mut sessions = daemon_client.list_sessions().await.ok()?;
        sessions.sort();
        for session_id in sessions {
            let Ok(details) = daemon_client.describe_session(&session_id).await else {
                continue;
            };
            let (Some(workspace_id), Some(cwd)) =
                (details.workspace_id.clone(), details.cwd.clone())
            else {
                continue;
            };
            if workspace_id.trim().is_empty() || cwd.trim().is_empty() {
                continue;
            }
            return Some(BatchTemplate {
                workspace_id,
                worktree: details.worktree.clone(),
                cwd,
                cols: details.cols,
                rows: details.rows,
            });
        }
        None
    }

    /// Drives one wave of the batch: `wave.requests` identical requests in flight
    /// together - that is the duplicate/conflict race the scenario needs - so the
    /// in-flight count is the wave size and the load stays bounded by
    /// construction. `create_local_split_until` is the product's own create path,
    /// so a duplicate really re-reads the durable record and a conflicting
    /// fingerprint is really rejected by the journal.
    async fn drive_batch_wave(
        daemon_client: &Arc<DaemonClient>,
        prepared: &PreparedLocalSplit,
        wave: &BatchWave,
    ) -> Vec<BatchOutcome> {
        let mut futures = Vec::with_capacity(wave.requests as usize);
        for _ in 0..wave.requests {
            let variant = prepared.clone();
            let request_id = wave.request_id.clone();
            let kind = wave.kind;
            futures.push(async move {
                let deadline =
                    tokio::time::Instant::now() + Duration::from_millis(BATCH_REQUEST_BUDGET_MS);
                match daemon_client.create_local_split_until(&variant, deadline).await {
                    Ok(result) => BatchOutcome {
                        request_id,
                        kind,
                        outcome: "settled",
                        session_id: Some(result.session_id),
                        error_code: None,
                        error_message: None,
                    },
                    Err(error) => {
                        let rejected = error.code == IpcErrorCode::SpawnRequestConflict;
                        BatchOutcome {
                            request_id,
                            kind,
                            outcome: if rejected { "rejected" } else { "unsettled" },
                            session_id: None,
                            error_code: Some(format!("{:?}", error.code)),
                            error_message: Some(error.message),
                        }
                    }
                }
            });
        }
        futures_util::future::join_all(futures).await
    }

    /// Drives the runner's `split-concurrent-batch` control through the real
    /// client path, then cleans up the sessions it really created and verifies
    /// the cleanup against the daemon's own inventory.
    async fn handle_concurrent_batch(
        daemon_client: &Arc<DaemonClient>,
        channel: &Arc<QaBarrierChannel>,
        count: u64,
        test_conflicts: bool,
    ) -> Result<(), String> {
        let operation_id = channel
            .operation_id()
            .ok_or_else(|| "no QA operation nonce".to_string())?;
        let Some(template) = batch_template(daemon_client).await else {
            return Err(
                "no live daemon session with a workspace and cwd is available for the concurrent batch"
                    .to_string(),
            );
        };
        let plan = batch_plan(count, test_conflicts, template.cols, template.rows);
        let mut request_ids: Vec<String> = Vec::new();
        for wave in &plan {
            if !request_ids.contains(&wave.request_id) {
                request_ids.push(wave.request_id.clone());
            }
        }
        let mut outcomes: Vec<BatchOutcome> = Vec::new();
        // One real admission per durable identity, before any of its requests.
        let mut identities: Vec<(String, SplitIdentity)> = Vec::new();
        for request_id in &request_ids {
            let admission_deadline =
                tokio::time::Instant::now() + Duration::from_millis(BATCH_REQUEST_BUDGET_MS);
            match daemon_client
                .prepare_local_split_until(request_id, admission_deadline)
                .await
            {
                Ok(identity) => identities.push((request_id.clone(), identity)),
                Err(error) => {
                    let reason = format!(
                        "batch admission for '{request_id}' failed: {} ({:?})",
                        error.message, error.code
                    );
                    qa_barrier::append_receipt_off_runtime(
                        channel,
                        SPLIT_CONCURRENT_BATCH,
                        &operation_id,
                        batch_payload(
                            count,
                            test_conflicts,
                            &template.workspace_id,
                            &request_ids,
                            &plan,
                            &outcomes,
                            &[],
                            &[],
                            None,
                            Some(reason.as_str()),
                        ),
                    )
                    .await;
                    return Err(reason);
                }
            }
        }
        // Waves run in plan order, so a duplicate wave really meets the record an
        // earlier wave left behind and the conflicting wave really meets the base
        // fingerprint its identity already published.
        for (request_id, identity) in &identities {
            for wave in plan.iter().filter(|wave| &wave.request_id == request_id) {
                let prepared = template.with_identity(identity.clone());
                outcomes.extend(drive_batch_wave(daemon_client, &prepared, wave).await);
            }
        }

        // Real cleanup: every session the batch really created is closed again and
        // the close is verified against the daemon's own inventory.
        let mut created: Vec<String> = outcomes
            .iter()
            .filter_map(|outcome| outcome.session_id.clone())
            .collect();
        created.sort();
        created.dedup();
        let mut closed: Vec<String> = Vec::new();
        for session_id in &created {
            match daemon_client.close_terminal(session_id).await {
                Ok(()) => closed.push(session_id.clone()),
                Err(error) => eprintln!(
                    "FERRYX_QA_BATCH_CLOSE_FAILED: {session_id}: {}",
                    error.message
                ),
            }
        }
        let inventory = daemon_client.list_sessions().await.unwrap_or_default();
        let cleanup_verified = created
            .iter()
            .all(|session_id| !inventory.contains(session_id));
        qa_barrier::append_receipt_off_runtime(
            channel,
            SPLIT_CONCURRENT_BATCH,
            &operation_id,
            batch_payload(
                count,
                test_conflicts,
                &template.workspace_id,
                &request_ids,
                &plan,
                &outcomes,
                &created,
                &closed,
                Some(cleanup_verified),
                None,
            ),
        )
        .await;
        Ok(())
    }

    /// Bounded watch for the runner's `split-concurrent-batch` control. The
    /// command stays pending (reported once) while no live session can supply the
    /// real workspace the batch must drive, and is serviced at most once.
    async fn run_concurrent_batch_watcher(daemon_client: Arc<DaemonClient>) {
        let Some(channel) = wait_for_channel().await else {
            return;
        };
        let Some(dir) = barrier_dir() else {
            return;
        };
        let mut handled: Vec<String> = Vec::new();
        let mut unserviced_reported = false;
        loop {
            tokio::time::sleep(Duration::from_millis(WATCH_TICK_MS)).await;
            let Some(request) = read_control(&dir, SPLIT_CONCURRENT_BATCH, &channel).await else {
                continue;
            };
            let issued_at = request
                .get("issuedAt")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            if handled.iter().any(|seen| seen == &issued_at) {
                continue;
            }
            let count = request.get("count").and_then(Value::as_u64).unwrap_or(0);
            if count == 0 {
                handled.push(issued_at);
                eprintln!(
                    "FERRYX_QA_BATCH_UNSERVICEABLE: split-concurrent-batch requires a positive count"
                );
                continue;
            }
            let test_conflicts = request
                .get("testConflicts")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            match handle_concurrent_batch(&daemon_client, &channel, count, test_conflicts).await {
                Ok(()) => handled.push(issued_at),
                Err(error) => {
                    if !unserviced_reported {
                        unserviced_reported = true;
                        eprintln!(
                            "FERRYX_QA_BATCH_UNSERVICED: {error}; the command stays pending"
                        );
                    }
                }
            }
        }
    }

    async fn wait_for_channel() -> Option<Arc<QaBarrierChannel>> {
        let deadline = tokio::time::Instant::now() + Duration::from_millis(CHANNEL_BOOT_WAIT_MS);
        loop {
            if let Some(channel) = channel() {
                return Some(channel);
            }
            if tokio::time::Instant::now() >= deadline {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(WATCH_TICK_MS)).await;
        }
    }

    /// Starts the GUI-lane watchers. Called from the same boot path that installs
    /// the channel, so a launch without the runner env returns immediately.
    pub fn start_watchers(daemon_client: Arc<DaemonClient>) {
        let cancel_client = Arc::clone(&daemon_client);
        tauri::async_runtime::spawn(async move { run_cancel_watcher(cancel_client).await });
        let retry_client = Arc::clone(&daemon_client);
        tauri::async_runtime::spawn(async move { run_retry_watcher(retry_client).await });
        let batch_client = Arc::clone(&daemon_client);
        tauri::async_runtime::spawn(async move { run_concurrent_batch_watcher(batch_client).await });
        tauri::async_runtime::spawn(async move { run_handover_watcher(daemon_client).await });
    }

    async fn run_cancel_watcher(daemon_client: Arc<DaemonClient>) {
        let Some(channel) = wait_for_channel().await else {
            return;
        };
        let Some(dir) = barrier_dir() else {
            return;
        };
        let mut handled: Vec<String> = Vec::new();
        loop {
            tokio::time::sleep(Duration::from_millis(WATCH_TICK_MS)).await;
            let Some(request) = read_control(&dir, SPLIT_CANCEL, &channel).await else {
                continue;
            };
            let issued_at = request
                .get("issuedAt")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let phase = request
                .get("phase")
                .and_then(Value::as_str)
                .unwrap_or("while-creating")
                .to_string();
            let key = format!("{phase}|{issued_at}");
            if handled.iter().any(|seen| seen == &key) {
                continue;
            }
            handled.push(key);
            let duplicate = handled.len() > 1;
            if let Err(error) = handle_cancel(&daemon_client, &channel, &phase, &issued_at, duplicate).await
            {
                eprintln!("FERRYX_QA_CANCEL_UNSETTLED: {error}");
            }
        }
    }

    async fn wait_for_recorded_split() -> Option<RecordedSplit> {
        wait_for_recorded_split_within(CANCEL_IDENTITY_WAIT_MS).await
    }

    /// Bounded wait for the operation identity a control must reuse. A control
    /// that finds no recorded operation stays pending instead of being answered
    /// against an identity this process never established.
    async fn wait_for_recorded_split_within(wait_ms: u64) -> Option<RecordedSplit> {
        let deadline = tokio::time::Instant::now() + Duration::from_millis(wait_ms);
        loop {
            if let Some(record) = lock_recorded().clone() {
                return Some(record);
            }
            if tokio::time::Instant::now() >= deadline {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(WATCH_TICK_MS)).await;
        }
    }

    async fn handle_cancel(
        daemon_client: &Arc<DaemonClient>,
        channel: &Arc<QaBarrierChannel>,
        phase: &str,
        issued_at: &str,
        duplicate: bool,
    ) -> Result<(), String> {
        let observed_at_ms = now_ms();
        let dispatch_ms = parse_rfc3339_ms(issued_at).map(|issued| observed_at_ms.saturating_sub(issued));
        let record = wait_for_recorded_split()
            .await
            .ok_or_else(|| "no split operation identity was recorded in this process".to_string())?;
        let operation_id = channel
            .operation_id()
            .ok_or_else(|| "no QA operation nonce".to_string())?;
        let deadline =
            tokio::time::Instant::now() + Duration::from_millis(DAEMON_CANCEL_BUDGET_MS);
        let operation = daemon_client
            .cancel_local_split_until(&record.identity, deadline)
            .await
            .map_err(|error| error.message)?;
        let cancel_ack_ms = now_ms().saturating_sub(observed_at_ms);
        let status = daemon_client
            .local_split_status_until(
                &record.identity,
                tokio::time::Instant::now() + Duration::from_millis(DAEMON_CANCEL_BUDGET_MS),
            )
            .await
            .ok();
        let sessions = daemon_client.list_sessions().await.unwrap_or_default();
        let owned_creation_removed = record
            .owned_session_id
            .as_ref()
            .map(|id| !sessions.contains(id));
        let source_still_present = record
            .source_backend_session_id
            .as_ref()
            .map(|id| sessions.contains(id));
        qa_barrier::append_receipt_off_runtime(
            channel,
            CANCEL_ACK,
            &operation_id,
            cancel_ack_payload(
                &record,
                phase,
                duplicate,
                cancel_ack_ms,
                dispatch_ms,
                operation_state(&operation),
                status.as_ref().map(operation_state),
                owned_creation_removed,
                source_still_present,
            ),
        )
        .await;
        Ok(())
    }

    /// The runner's handover controls are executed through the REAL product
    /// path: the GUI asks the running daemon to upgrade, which is what performs
    /// the session-ownership handover (or, for the abort scenario the runner
    /// armed, its rollback).
    async fn run_handover_watcher(daemon_client: Arc<DaemonClient>) {
        let Some(channel) = wait_for_channel().await else {
            return;
        };
        let Some(dir) = barrier_dir() else {
            return;
        };
        let mut triggered = false;
        loop {
            tokio::time::sleep(Duration::from_millis(WATCH_TICK_MS)).await;
            if triggered {
                continue;
            }
            let handover_armed = read_control(&dir, TRIGGER_HANDOVER, &channel).await;
            let handover_abort_armed = read_control(&dir, TRIGGER_HANDOVER_ABORT, &channel).await;
            if handover_armed.is_none() && handover_abort_armed.is_none() {
                continue;
            }
            triggered = true;
            if let Err(error) = daemon_client.upgrade_binary().await {
                eprintln!("FERRYX_QA_HANDOVER_TRIGGER_FAILED: {}", error.message);
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::daemon::protocol::{
            DaemonSessionDetails, SplitAttachAttempt, SplitIdentity, STAGE_ATTACH_OR_LISTENER_MAX_MS,
        };

        fn identity() -> SplitIdentity {
            SplitIdentity {
                request_id: "req-1".into(),
                origin_epoch: "4".into(),
                expires_at_unix_ms: 1_700_000_000_000,
            }
        }

        fn prepared() -> PreparedLocalSplit {
            PreparedLocalSplit {
                identity: identity(),
                workspace_id: "ws-1".into(),
                worktree: None,
                cwd: "/repo".into(),
                shell: None,
                cols: 80,
                rows: 24,
            }
        }

        fn session_details() -> DaemonSessionDetails {
            let mut details = DaemonSessionDetails::new(
                "session-1".into(),
                Some("ws-1".into()),
                None,
                Some("/repo".into()),
                80,
                24,
                true,
                None,
                None,
                None,
                false,
            );
            details.incarnation = Some("inc-1".into());
            details
        }

        fn binding() -> PaneAttachTuple {
            PaneAttachTuple {
                backend_session_id: "session-1".into(),
                incarnation: Some("inc-1".into()),
                daemon_epoch: "7".into(),
                frontend_session_id: "front-1".into(),
                pane_identity: "pane-1".into(),
                binding_key: "session-1:7:0:".into(),
                attempt_generation: 2,
            }
        }

        fn attempt() -> SplitAttachAttempt {
            SplitAttachAttempt {
                identity: identity(),
                frontend_session_id: "front-1".into(),
                generation: 2,
                remaining_ms: 3_500,
            }
        }

        #[test]
        fn split_create_payload_carries_the_authoritative_identity() {
            let recorded = RecordedSplit {
                identity: identity(),
                workspace_id: "ws-1".into(),
                source_backend_session_id: Some("session-source".into()),
                prepared_at_ms: 1_700_000_000_100,
                owned_session_id: Some("session-1".into()),
                created_at_ms: Some(1_700_000_000_200),
            };
            let payload = split_create_payload(
                &prepared(),
                "session-1",
                7,
                &session_details(),
                Some(&recorded),
            );
            assert_eq!(payload["stage"], json!(SPLIT_CREATE));
            assert_eq!(payload["backendSessionId"], json!("session-1"));
            assert_eq!(payload["incarnation"], json!("inc-1"));
            assert_eq!(payload["daemonEpoch"], json!("7"));
            assert_eq!(payload["requestId"], json!("req-1"));
            assert_eq!(payload["originEpoch"], json!("4"));
            assert_eq!(payload["sourceBackendSessionId"], json!("session-source"));
        }

        #[test]
        fn attach_hold_failure_is_actionable_and_keeps_the_same_id() {
            let payload = attach_hold_payload(
                "session-1",
                &binding(),
                &attempt(),
                STAGE_ATTACH_OR_LISTENER_MAX_MS,
                true,
                ReleaseOutcome::DeadlineExceeded.as_str(),
                Some(true),
            );
            assert_eq!(payload["actionable"], json!(true));
            assert_eq!(payload["retryMustReuseId"], json!(true));
            assert_eq!(payload["backendSessionId"], json!("session-1"));
            assert_eq!(payload["incarnation"], json!("inc-1"));
            assert_eq!(payload["attemptGeneration"], json!(2));
            assert_eq!(payload["releaseOutcome"], json!("deadline-exceeded"));
            assert_eq!(payload["backendSessionStillAlive"], json!(true));
        }

        #[test]
        fn attach_hold_release_is_not_reported_as_a_failure() {
            let payload = attach_hold_payload(
                "session-1",
                &binding(),
                &attempt(),
                STAGE_ATTACH_OR_LISTENER_MAX_MS,
                false,
                ReleaseOutcome::Released.as_str(),
                None,
            );
            assert_eq!(payload["actionable"], json!(false));
            assert_eq!(payload["releaseOutcome"], json!("released"));
        }

        #[test]
        fn cancel_cleanup_is_authoritative_only_on_real_evidence() {
            assert!(cleanup_is_authoritative(true, Some(true), Some(true)));
            assert!(cleanup_is_authoritative(true, None, Some(true)));
            assert!(cleanup_is_authoritative(true, Some(true), None));
            assert!(!cleanup_is_authoritative(false, Some(true), Some(true)));
            assert!(!cleanup_is_authoritative(true, Some(false), Some(true)));
            assert!(!cleanup_is_authoritative(true, Some(true), Some(false)));
        }

        #[test]
        fn cancel_ack_payload_reports_the_runner_contract_fields() {
            let record = RecordedSplit {
                identity: identity(),
                workspace_id: "ws-1".into(),
                source_backend_session_id: Some("session-source".into()),
                prepared_at_ms: 1_700_000_000_100,
                owned_session_id: Some("session-1".into()),
                created_at_ms: Some(1_700_000_000_200),
            };
            let payload = cancel_ack_payload(
                &record,
                "while-creating",
                false,
                120,
                Some(8),
                "cancelled",
                Some("cancelled"),
                Some(true),
                Some(true),
            );
            assert!(payload["cancelAckMs"].as_u64().is_some());
            assert_eq!(payload["timerDispatchLatencyMs"], json!(8));
            assert_eq!(payload["cleanupReceipt"]["authoritative"], json!(true));
            assert_eq!(payload["createdIdRequired"], json!(false));
            assert_eq!(payload["cleanupReceipt"]["ownedCreationRemoved"], json!(true));
            assert_eq!(payload["cleanupReceipt"]["sourceStillPresent"], json!(true));
        }

        #[test]
        fn cancel_ack_refuses_authority_without_a_cancelled_operation() {
            let record = RecordedSplit {
                identity: identity(),
                workspace_id: "ws-1".into(),
                source_backend_session_id: None,
                prepared_at_ms: 0,
                owned_session_id: None,
                created_at_ms: None,
            };
            let payload = cancel_ack_payload(
                &record,
                "before-create",
                false,
                90,
                None,
                "created",
                Some("created"),
                None,
                None,
            );
            assert_eq!(payload["cleanupReceipt"]["authoritative"], json!(false));
            assert_eq!(payload["createdIdRequired"], json!(false));
        }

        #[test]
        fn rfc3339_controls_are_parsed_to_milliseconds() {
            assert_eq!(parse_rfc3339_ms("1970-01-01T00:00:00.000Z"), Some(0));
            assert_eq!(parse_rfc3339_ms("2000-01-01T00:00:00.000Z"), Some(946_684_800_000));
            let earlier = parse_rfc3339_ms("2026-10-03T00:00:00.000Z").unwrap();
            let later = parse_rfc3339_ms("2026-10-03T00:00:01.000Z").unwrap();
            assert_eq!(later - earlier, 1_000);
            assert_eq!(parse_rfc3339_ms("not-a-timestamp"), None);
            assert_eq!(parse_rfc3339_ms("2026-13-03T00:00:00.000Z"), None);
        }

        #[test]
        fn control_files_are_read_only_for_this_run() {
            let root = tempfile::tempdir().unwrap();
            let dir = root.path().join("barriers");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join(format!("{SPLIT_CANCEL}.arm.json")),
                serde_json::to_vec(&json!({
                    "name": SPLIT_CANCEL,
                    "runId": "qa-run-gui",
                    "operationId": "op-1",
                    "deadlineMs": 2_000,
                }))
                .unwrap(),
            )
            .unwrap();
            let channel = QaBarrierChannel::new(dir.clone(), "qa-run-gui".into());
            channel.scan_and_ack_arms();
            std::fs::write(
                dir.join(format!("{SPLIT_CANCEL}.request.json")),
                serde_json::to_vec(&json!({
                    "name": SPLIT_CANCEL,
                    "runId": "qa-run-OTHER",
                    "operationId": "op-1",
                    "phase": "while-creating",
                }))
                .unwrap(),
            )
            .unwrap();
            assert!(read_control_in(&dir, SPLIT_CANCEL, &channel).is_none());
            std::fs::write(
                dir.join(format!("{SPLIT_CANCEL}.request.json")),
                serde_json::to_vec(&json!({
                    "name": SPLIT_CANCEL,
                    "runId": "qa-run-gui",
                    "operationId": "op-OTHER",
                    "phase": "while-creating",
                }))
                .unwrap(),
            )
            .unwrap();
            assert!(read_control_in(&dir, SPLIT_CANCEL, &channel).is_none());
            std::fs::write(
                dir.join(format!("{SPLIT_CANCEL}.request.json")),
                serde_json::to_vec(&json!({
                    "name": SPLIT_CANCEL,
                    "runId": "qa-run-gui",
                    "operationId": "op-1",
                    "phase": "while-creating",
                }))
                .unwrap(),
            )
            .unwrap();
            assert!(read_control_in(&dir, SPLIT_CANCEL, &channel).is_some());
        }

        fn retry_control() -> Value {
            // Verbatim shape of the runner's `retry` control
            // (`runSplitAttachStallScenario`, split-scenarios.mjs:150-160).
            json!({
                "name": RETRY,
                "runId": "qa-run-gui",
                "operationId": "op-1",
                "issuedAt": "2026-10-04T00:00:00.000Z",
                "backendSessionId": "session-1",
                "incarnation": "inc-1",
                "daemonEpoch": "7",
                "attemptGeneration": 2,
                "clientRequestId": "req-1",
            })
        }

        fn recorded_split() -> RecordedSplit {
            RecordedSplit {
                identity: identity(),
                workspace_id: "ws-1".into(),
                source_backend_session_id: Some("session-source".into()),
                prepared_at_ms: 1_700_000_000_100,
                owned_session_id: Some("session-1".into()),
                created_at_ms: Some(1_700_000_000_200),
            }
        }

        fn recorded_attempt() -> RecordedAttempt {
            RecordedAttempt {
                incarnation: Some("inc-1".into()),
                attempt_generation: 1,
            }
        }

        #[test]
        fn retry_command_parses_the_runner_field_names() {
            let command = RetryCommand::parse(&retry_control()).expect("runner control parses");
            assert_eq!(command.backend_session_id, "session-1");
            assert_eq!(command.incarnation.as_deref(), Some("inc-1"));
            assert_eq!(command.daemon_epoch.as_deref(), Some("7"));
            assert_eq!(command.attempt_generation, 2);
            assert_eq!(command.client_request_id.as_deref(), Some("req-1"));

            // The runner sends `null` for the optional identity fields; a null is
            // absent, never an empty string that could match something.
            let mut nulls = retry_control();
            nulls["incarnation"] = Value::Null;
            nulls["daemonEpoch"] = Value::Null;
            nulls["clientRequestId"] = Value::Null;
            let command = RetryCommand::parse(&nulls).expect("nulls are absent fields");
            assert_eq!(command.incarnation, None);
            assert_eq!(command.daemon_epoch, None);
            assert_eq!(command.client_request_id, None);
        }

        #[test]
        fn retry_command_refuses_identity_it_cannot_name() {
            let mut wildcard = retry_control();
            wildcard["backendSessionId"] = json!("*");
            assert!(RetryCommand::parse(&wildcard).is_err());
            let mut missing = retry_control();
            missing["backendSessionId"] = json!("   ");
            assert!(RetryCommand::parse(&missing).is_err());
            let mut unnumbered = retry_control();
            unnumbered["attemptGeneration"] = json!("2");
            assert!(RetryCommand::parse(&unnumbered).is_err());
        }

        #[test]
        fn retry_fence_retains_identity_and_advances_only_the_generation() {
            let record = recorded_split();
            let attempt = recorded_attempt();
            let command = RetryCommand::parse(&retry_control()).unwrap();
            assert!(retry_fence(&command, &record, Some(&attempt)).is_ok());

            // A retry that re-uses the generation the product already bound is not
            // a new attempt.
            let mut stale = command.clone();
            stale.attempt_generation = 1;
            assert!(retry_fence(&stale, &record, Some(&attempt)).is_err());

            // Neither the request identity, the backend session, nor the
            // incarnation may change under the same retry.
            let mut other_request = command.clone();
            other_request.client_request_id = Some("req-2".into());
            assert!(retry_fence(&other_request, &record, Some(&attempt)).is_err());
            let mut other_backend = command.clone();
            other_backend.backend_session_id = "session-2".into();
            assert!(retry_fence(&other_backend, &record, Some(&attempt)).is_err());
            let mut other_incarnation = command.clone();
            other_incarnation.incarnation = Some("inc-2".into());
            assert!(retry_fence(&other_incarnation, &record, Some(&attempt)).is_err());

            // With no recorded attempt yet, the fence cannot invent a previous
            // generation and must not refuse the retry.
            assert!(retry_fence(&command, &record, None).is_ok());
        }

        #[test]
        fn retry_payload_reports_the_real_retry_settlement() {
            let command = RetryCommand::parse(&retry_control()).unwrap();
            let record = recorded_split();
            let attempt = recorded_attempt();
            let details = session_details();
            let payload = retry_payload(
                &command,
                &record,
                Some(&attempt),
                "reattached",
                None,
                Some("created"),
                Some(&details),
                Some(9),
                Some((Some(1), Some(4))),
            );
            assert_eq!(payload["stage"], json!(RETRY));
            assert_eq!(payload["outcome"], json!("reattached"));
            assert_eq!(payload["retryMustReuseId"], json!(true));
            assert_eq!(payload["backendSessionId"], json!("session-1"));
            assert_eq!(payload["incarnation"], json!("inc-1"));
            assert_eq!(payload["daemonEpoch"], json!("7"));
            assert_eq!(payload["attemptGeneration"], json!(2));
            assert_eq!(payload["previousAttemptGeneration"], json!(1));
            assert_eq!(payload["requestId"], json!("req-1"));
            assert_eq!(payload["clientRequestId"], json!("req-1"));
            assert_eq!(payload["originEpoch"], json!("4"));
            assert_eq!(payload["statusOperationState"], json!("created"));
            assert_eq!(payload["attachEpoch"], json!("9"));
            assert_eq!(payload["historyStartSequence"], json!("1"));
            assert_eq!(payload["historyEndSequence"], json!("4"));
            assert_eq!(payload["refusalReason"], Value::Null);

            // A refusal carries the reason and none of the values it cannot prove.
            let refused = retry_payload(
                &command,
                &record,
                Some(&attempt),
                "refused",
                Some("retry must advance attemptGeneration beyond the bound 1, got 1"),
                None,
                None,
                None,
                None,
            );
            assert_eq!(refused["outcome"], json!("refused"));
            assert_eq!(
                refused["refusalReason"],
                json!("retry must advance attemptGeneration beyond the bound 1, got 1")
            );
            assert_eq!(refused["attachEpoch"], Value::Null);
            assert_eq!(refused["historyEndSequence"], Value::Null);
            assert_eq!(refused["statusOperationState"], Value::Null);
        }

        #[test]
        fn concurrent_batch_plan_is_bounded_and_pairs_duplicates_with_conflicts() {
            let plan = batch_plan(16, true, 80, 24);
            // Four ordered waves: two duplicate waves on one identity, then the
            // conflicting identity's base fingerprint, then its conflicting one.
            let sizes: Vec<u64> = plan.iter().map(|wave| wave.requests).collect();
            assert_eq!(sizes, vec![4, 4, 4, 4]);
            assert_eq!(plan.iter().map(|wave| wave.requests).sum::<u64>(), 16);
            let duplicate_waves: Vec<&BatchWave> = plan
                .iter()
                .filter(|wave| wave.kind == BatchRequestKind::Duplicate)
                .collect();
            let conflict_waves: Vec<&BatchWave> = plan
                .iter()
                .filter(|wave| wave.kind == BatchRequestKind::Conflict)
                .collect();
            assert_eq!(duplicate_waves.len(), 2);
            assert_eq!(conflict_waves.len(), 2);
            // One durable identity per kind: the duplicates share the request the
            // record already owns, the conflicts share the other one.
            assert!(duplicate_waves
                .iter()
                .all(|wave| wave.request_id == duplicate_waves[0].request_id));
            assert!(conflict_waves
                .iter()
                .all(|wave| wave.request_id == conflict_waves[0].request_id));
            assert_ne!(duplicate_waves[0].request_id, conflict_waves[0].request_id);
            assert!(duplicate_waves[0].request_id.starts_with(BATCH_REQUEST_PREFIX));
            assert!(conflict_waves[0].request_id.starts_with(BATCH_REQUEST_PREFIX));
            // The conflicting wave really carries a different fingerprint, and it
            // runs AFTER the base fingerprint of its own identity is durable.
            assert_eq!(conflict_waves[0].cols, 80);
            assert_eq!(conflict_waves[1].cols, 81);
            let base_index = plan
                .iter()
                .position(|wave| {
                    wave.kind == BatchRequestKind::Conflict && wave.cols == 80
                })
                .expect("the base conflict wave exists");
            let conflicting_index = plan
                .iter()
                .position(|wave| {
                    wave.kind == BatchRequestKind::Conflict && wave.cols == 81
                })
                .expect("the conflicting wave exists");
            assert!(
                base_index < conflicting_index,
                "the conflicting fingerprint must be offered after the base one"
            );
            // The control's count is capped, and the widest wave bounds the load.
            let capped = batch_plan(4_000, true, 80, 24);
            assert_eq!(
                capped.iter().map(|wave| wave.requests).sum::<u64>(),
                BATCH_REQUEST_CAP
            );
            assert!(capped.iter().all(|wave| wave.requests <= BATCH_REQUEST_CAP));
            assert_eq!(
                batch_plan(0, true, 80, 24)
                    .iter()
                    .map(|wave| wave.requests)
                    .sum::<u64>(),
                1
            );
            // A count with no conflict pairs stays duplicate-only, and its second
            // wave can only be answered from the first wave's record.
            let duplicates_only = batch_plan(3, false, 80, 24);
            assert!(duplicates_only
                .iter()
                .all(|wave| wave.kind == BatchRequestKind::Duplicate));
            assert_eq!(
                duplicates_only
                    .iter()
                    .map(|wave| wave.requests)
                    .sum::<u64>(),
                3
            );
            assert!(duplicates_only.len() >= 2);
            // Geometry at the type's edge never overflows into a panic.
            let edge = batch_plan(4, true, u16::MAX, 24);
            assert!(edge
                .iter()
                .all(|wave| wave.cols == u16::MAX || wave.cols == u16::MAX - 1));
        }

        #[test]
        fn concurrent_batch_payload_claims_only_evidence_backed_idempotency() {
            let settled = |request_id: &str, kind: BatchRequestKind, session_id: Option<&str>| BatchOutcome {
                request_id: request_id.into(),
                kind,
                outcome: "settled",
                session_id: session_id.map(str::to_string),
                error_code: None,
                error_message: None,
            };
            let duplicate_id = format!("{BATCH_REQUEST_PREFIX}-duplicate");
            let conflict_id = format!("{BATCH_REQUEST_PREFIX}-conflict");
            let request_ids = vec![duplicate_id.clone(), conflict_id.clone()];
            let plan = batch_plan(16, true, 80, 24);

            // A single request can never prove that a duplicate was answered from
            // an existing record.
            let single = vec![settled(&duplicate_id, BatchRequestKind::Duplicate, Some("session-1"))];
            let payload = batch_payload(
                16,
                true,
                "ws-1",
                &request_ids,
                &plan,
                &single,
                &["session-1".into()],
                &[],
                Some(true),
                None,
            );
            assert_eq!(payload["stage"], json!(SPLIT_CONCURRENT_BATCH));
            assert_eq!(payload["requestedCount"], json!(16));
            assert_eq!(payload["plannedRequests"], json!(16));
            assert_eq!(payload["boundedRequestCap"], json!(BATCH_REQUEST_CAP));
            assert_eq!(payload["waveCount"], json!(4));
            assert_eq!(payload["boundedConcurrency"], json!(4));
            assert_eq!(payload["duplicateAnsweredFromRecord"], json!(false));
            assert_eq!(payload["conflictRejected"], json!(0));
            assert_eq!(payload["cleanupVerified"], json!(true));

            // Two duplicate requests answered by ONE session id is the evidence.
            let mut outcomes = vec![
                settled(&duplicate_id, BatchRequestKind::Duplicate, Some("session-1")),
                settled(&duplicate_id, BatchRequestKind::Duplicate, Some("session-1")),
                BatchOutcome {
                    request_id: conflict_id.clone(),
                    kind: BatchRequestKind::Conflict,
                    outcome: "rejected",
                    session_id: None,
                    error_code: Some("SpawnRequestConflict".into()),
                    error_message: Some(
                        "Split request identity was reused with different parameters".into(),
                    ),
                },
            ];
            let payload = batch_payload(
                16,
                true,
                "ws-1",
                &request_ids,
                &plan,
                &outcomes,
                &["session-1".into()],
                &["session-1".into()],
                Some(true),
                None,
            );
            assert_eq!(payload["duplicateAnsweredFromRecord"], json!(true));
            assert_eq!(payload["duplicateDistinctSessionIds"], json!(["session-1"]));
            assert_eq!(payload["conflictDistinctSessionIds"], json!([]));
            assert_eq!(payload["conflictRejected"], json!(1));
            assert_eq!(payload["conflictRejectionCodes"], json!(["SpawnRequestConflict"]));
            assert_eq!(payload["closedSessionIds"], json!(["session-1"]));
            assert_eq!(payload["outcomes"][2]["kind"], json!("conflict"));
            assert_eq!(payload["outcomes"][2]["outcome"], json!("rejected"));
            assert_eq!(payload["outcomes"][2]["errorCode"], json!("SpawnRequestConflict"));
            assert_eq!(payload["outcomes"][2]["sessionId"], Value::Null);

            // A second distinct session is a duplicate creation: the payload must
            // report it instead of claiming idempotency.
            outcomes.push(settled(&duplicate_id, BatchRequestKind::Duplicate, Some("session-2")));
            let payload = batch_payload(
                16,
                true,
                "ws-1",
                &request_ids,
                &plan,
                &outcomes,
                &["session-1".into(), "session-2".into()],
                &[],
                None,
                Some("batch cleanup was not verified"),
            );
            assert_eq!(
                payload["duplicateDistinctSessionIds"],
                json!(["session-1", "session-2"])
            );
            assert_eq!(payload["duplicateAnsweredFromRecord"], json!(false));
            assert_eq!(payload["cleanupVerified"], Value::Null);
            assert_eq!(
                payload["unsettledReason"],
                json!("batch cleanup was not verified")
            );
        }

        #[test]
        fn retry_and_batch_controls_are_read_only_for_this_run() {
            let root = tempfile::tempdir().unwrap();
            let dir = root.path().join("barriers");
            std::fs::create_dir_all(&dir).unwrap();
            let channel = QaBarrierChannel::new(dir.clone(), "qa-run-gui".into());
            // The runner stamps one operation nonce into every arm AND into the
            // private env (`FERRYX_QA_OPERATION_ID`). A channel built by hand has
            // neither, and `read_control` refuses EVERY control while
            // `operation_id()` is `None` - including the correlated control this
            // test expects honored. Establishing the identity from an arm is
            // exactly what the passing sibling
            // `control_files_are_read_only_for_this_run` does.
            std::fs::write(
                dir.join(format!("{RETRY}.arm.json")),
                serde_json::to_vec(&json!({
                    "name": RETRY,
                    "runId": "qa-run-gui",
                    "operationId": "op-1",
                    "deadlineMs": 2_000,
                }))
                .unwrap(),
            )
            .unwrap();
            channel.scan_and_ack_arms();
            for name in [RETRY, SPLIT_CONCURRENT_BATCH] {
                std::fs::write(
                    dir.join(format!("{name}.request.json")),
                    serde_json::to_vec(&json!({
                        "name": name,
                        "runId": "qa-run-OTHER",
                        "operationId": "op-1",
                    }))
                    .unwrap(),
                )
                .unwrap();
                assert!(read_control_in(&dir, name, &channel).is_none());
                std::fs::write(
                    dir.join(format!("{name}.request.json")),
                    serde_json::to_vec(&json!({
                        "name": name,
                        "runId": "qa-run-gui",
                        "operationId": "op-1",
                        "count": 16,
                        "testConflicts": true,
                        "backendSessionId": "session-1",
                        "attemptGeneration": 2,
                    }))
                    .unwrap(),
                )
                .unwrap();
                assert!(read_control_in(&dir, name, &channel).is_some());
            }
        }
    }
}

/// Boot entry for the GUI-lane QA watchers (cancel acknowledgement and the real
/// handover trigger). Does nothing without the runner's private barrier env.
#[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
pub fn start_qa_split_watchers(daemon_client: std::sync::Arc<DaemonClient>) {
    qa_split_producers::start_watchers(daemon_client);
}

#[tauri::command]
pub async fn cmd_terminal_spawn_operation(
    daemon_client: State<'_, Arc<DaemonClient>>,
    registry: State<'_, WorkspaceRegistry>,
    request: crate::daemon::protocol::SplitOperationRequest,
) -> Result<crate::daemon::protocol::SplitOperationResponse, IpcError> {
    use crate::daemon::protocol::*;
    match request {
        SplitOperationRequest::Prepare { request_id, request, remaining_ms } => {
            #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
            eprintln!(
                "FERRYX_QA_SPLIT_OP_RECEIVED: variant=prepare request_id={request_id} workspace_id={}",
                request.workspace_id
            );
            if request_id.trim().is_empty() || request.startup.is_some()
                || crate::ssh::projects::is_remote(&request.workspace_id)
                || request.workspace_id.starts_with("daemon:")
            {
                #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
                eprintln!(
                    "FERRYX_QA_SPLIT_OP_REFUSED: request_id={request_id} workspace_id={} blank_request_id={} startup_present={} remote_workspace={} paired_workspace={}",
                    request.workspace_id,
                    request_id.trim().is_empty(),
                    request.startup.is_some(),
                    crate::ssh::projects::is_remote(&request.workspace_id),
                    request.workspace_id.starts_with("daemon:")
                );
                return Err(IpcError::new(IpcErrorCode::InvalidArgument, "Prepare requires a local shell workspace and request identity"));
            }
            let deadline = split_stage_deadline(remaining_ms, STAGE_CREATE_OR_STATUS_MAX_MS)?;
            let identity = daemon_client.prepare_local_split_until(&request_id, deadline).await?;
            #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
            qa_split_producers::record_prepared(&identity, &request);
            let inherited = match (&request.cwd, &request.inherit_from_session_id) {
                (None, Some(parent)) => {
                    let probe_deadline = deadline.min(tokio::time::Instant::now()
                        + Duration::from_millis(STAGE_CWD_PROBE_MAX_MS));
                    daemon_client.describe_session_until(parent, &identity, probe_deadline)
                        .await?.cwd.map(PathBuf::from)
                }
                _ => request.cwd.clone(),
            };
            let workspaces = (*registry).clone();
            let workspace = request.workspace_id.clone();
            let worktree = request.worktree.clone();
            let (repo_root, cwd, default_shell) = tokio::time::timeout_at(deadline, run_blocking(move || {
                let (manager, root) = workspaces.resolve_terminal_target(&workspace, worktree.as_ref())
                    .map_err(IpcError::from)?;
                let cwd = resolve_spawn_cwd(inherited, root, |path| manager.canonical_allowed_path(path).map_err(IpcError::from))?;
                Ok((manager.repo_root().to_string_lossy().into_owned(), cwd,
                    crate::terminal::cached_terminal_preferences().default_shell.clone()))
            })).await.map_err(|_| IpcError::new(IpcErrorCode::SpawnAttemptTimeout, "Split preparation timed out"))??;
            tokio::time::timeout_at(deadline, daemon_client.register_workspace(&request.workspace_id, &repo_root))
                .await.map_err(|_| IpcError::new(IpcErrorCode::SpawnAttemptTimeout, "Split workspace registration timed out"))??;
            #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
            eprintln!(
                "FERRYX_QA_SPLIT_OP_PREPARED: request_id={} origin_epoch={} workspace_id={} cwd={}",
                identity.request_id,
                identity.origin_epoch,
                request.workspace_id,
                cwd.display()
            );
            Ok(SplitOperationResponse::Prepare { prepared: PreparedLocalSplit {
                identity, workspace_id: request.workspace_id, worktree: request.worktree,
                cwd: cwd.to_string_lossy().into_owned(), shell: request.shell.or(default_shell),
                cols: request.cols.unwrap_or(80), rows: request.rows.unwrap_or(24),
            } })
        }
        SplitOperationRequest::Status { identity, remaining_ms } => {
            #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
            eprintln!(
                "FERRYX_QA_SPLIT_OP_RECEIVED: variant=status request_id={} origin_epoch={}",
                identity.request_id, identity.origin_epoch
            );
            let deadline = split_stage_deadline(remaining_ms, STAGE_CREATE_OR_STATUS_MAX_MS)?;
            Ok(SplitOperationResponse::Status { operation: split_wire_epoch(
                daemon_client.local_split_status_until(&identity, deadline).await?) })
        }
        SplitOperationRequest::Cancel { identity, remaining_ms } => {
            #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
            eprintln!(
                "FERRYX_QA_SPLIT_OP_RECEIVED: variant=cancel request_id={} origin_epoch={}",
                identity.request_id, identity.origin_epoch
            );
            let deadline = split_stage_deadline(remaining_ms, CANCEL_ACK_MAX_MS)?;
            Ok(SplitOperationResponse::Cancel { operation: split_wire_epoch(
                daemon_client.cancel_local_split_until(&identity, deadline).await?) })
        }
    }
}

#[tauri::command]
pub async fn cmd_terminal_spawn<R: Runtime>(
    app: AppHandle<R>,
    daemon_client: State<'_, Arc<DaemonClient>>,
    registry: State<'_, WorkspaceRegistry>,
    request: SpawnTerminalRequest,
) -> Result<SpawnTerminalResponse, IpcError> {
    if request.create_only == Some(true) || request.prepared_local_split.is_some() {
        #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
        eprintln!(
            "FERRYX_QA_SPLIT_CREATE_RECEIVED: create_only={} prepared_local_split={} request_id={} workspace_id={}",
            request.create_only == Some(true),
            request.prepared_local_split.is_some(),
            request.prepared_local_split.as_ref()
                .map(|prepared| prepared.identity.request_id.as_str())
                .unwrap_or("-"),
            request.workspace_id
        );
        let prepared = request.prepared_local_split.as_ref().ok_or_else(||
            IpcError::new(IpcErrorCode::InvalidArgument, "Create-only requires preparedLocalSplit"))?;
        if request.create_only != Some(true) || prepared.workspace_id != request.workspace_id
            || prepared.worktree != request.worktree || request.startup.is_some()
        {
            #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
            eprintln!(
                "FERRYX_QA_SPLIT_CREATE_REFUSED: request_id={} create_only_not_true={} workspace_mismatch={} worktree_mismatch={} startup_present={}",
                prepared.identity.request_id,
                request.create_only != Some(true),
                prepared.workspace_id != request.workspace_id,
                prepared.worktree != request.worktree,
                request.startup.is_some()
            );
            return Err(IpcError::new(IpcErrorCode::InvalidArgument, "Prepared split identity does not match create request"));
        }
        let deadline = split_stage_deadline(request.remaining_ms.unwrap_or(0),
            crate::daemon::protocol::STAGE_CREATE_OR_STATUS_MAX_MS)?;
        let result = daemon_client.create_local_split_until(prepared, deadline).await?;
        #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
        qa_split_producers::record_and_emit_split_create(
            prepared,
            &result.session_id,
            result.epoch,
            &result.session,
        )
        .await;
        #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
        eprintln!(
            "FERRYX_QA_SPLIT_CREATE_READY: request_id={} session_id={} daemon_epoch={}",
            prepared.identity.request_id, result.session_id, result.epoch
        );
        return Ok(SpawnTerminalResponse { session_id: result.session_id,
            daemon_epoch: result.epoch.to_string(), session: result.session });
    }
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
            Ok::<_, IpcError>(crate::paired_host::projects::resolve_stored_project(
                &dir, &ws_id,
            ))
        })
        .await?;

        let (host_id, mut remote_workspace_id, repo_root): (String, String, std::path::PathBuf) =
            match &request.startup {
                Some(TerminalStartup::PairedDaemon {
                    host_id,
                    remote_workspace_id,
                }) => {
                    let repo_root = stored_project
                        .as_ref()
                        .filter(|p| matches!(&p.target,
                            crate::scoped_contracts::RunTarget::PairedDaemon { host_id: stored_host }
                                if stored_host == host_id))
                        .map(|p| std::path::PathBuf::from(&p.metadata.repo_root))
                        .unwrap_or_default();
                    (host_id.clone(), remote_workspace_id.clone(), repo_root)
                }
                _ => match stored_project {
                    Some(stored) => match stored.target {
                        crate::scoped_contracts::RunTarget::PairedDaemon { host_id } => (
                            host_id,
                            stored.remote_workspace_id,
                            std::path::PathBuf::from(stored.metadata.repo_root),
                        ),
                        _ => (
                            stored.remote_workspace_id.clone(),
                            stored.remote_workspace_id,
                            std::path::PathBuf::from(stored.metadata.repo_root),
                        ),
                    },
                    None => {
                        return Err(IpcError::new(
                            crate::ipc::error::IpcErrorCode::WorkspaceNotFound,
                            "Paired daemon project not found. Re-select or re-pair this project.",
                        ));
                    }
                },
            };

        let hosts = daemon_client
            .paired_host_list()
            .await
            .map_err(|e| IpcError::internal(e.message))?;
        let host = hosts
            .into_iter()
            .find(|h| h.host_id == host_id)
            .ok_or_else(|| {
                IpcError::internal(format!("Paired machine '{host_id}' not found in inventory"))
            })?;
        if host.auth_status != crate::paired_host::inventory::AuthStatus::Paired {
            return Err(IpcError::internal(
                "Machine authorization required for paired host",
            ));
        }

        let inventory = daemon_client
            .paired_host_operation(crate::paired_host::client::OperationRequest {
                host_id: host_id.clone(),
                generation: host.generation,
                operation: crate::paired_host::client::Operation::Projects,
            })
            .await
            .map_err(|e| map_client_error(&e, Some(&host_id), Some(host.generation)))?;
        let crate::paired_host::client::OperationResult::Projects(projects) = inventory.result else {
            return Err(IpcError::internal("Unexpected paired project inventory response"));
        };
        if let Some(current_id) = crate::paired_host::projects::recover_remote_id(
            &projects, &host_id, &remote_workspace_id, &repo_root.to_string_lossy(),
        ) {
            remote_workspace_id = current_id.to_owned();
        }

        // The paired host journal only accepts bare hyphenated UUIDs as request
        // ids, while local flows carry descriptive ids such as
        // `shell-replacement-<uuid>` or `restart-<session>-<uuid>`. Normalize at
        // this boundary so the host submit, journal reconciliation, and pending
        // create records all share the same host-safe id.
        let client_request_id = crate::remote::machine_protocol::host_request_id(
            request.client_request_id.as_deref().unwrap_or_default(),
        );

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
            } else if !effective_repo_root.as_os_str().is_empty()
                && cwd_path.starts_with(&effective_repo_root)
            {
                if let Ok(rel) = cwd_path.strip_prefix(&effective_repo_root) {
                    if rel.components().all(|c| {
                        matches!(
                            c,
                            std::path::Component::Normal(_) | std::path::Component::CurDir
                        )
                    }) && !rel.as_os_str().is_empty()
                    {
                        (
                            remote_workspace_id.clone(),
                            None,
                            Some(rel.to_string_lossy().to_string()),
                        )
                    } else {
                        (remote_workspace_id.clone(), None, None)
                    }
                } else {
                    (remote_workspace_id.clone(), None, None)
                }
            } else if is_absolute {
                let cached_id = projects.projects.iter().find(|p| {
                    p.metadata.repo_root == cwd_str
                        && p.metadata.availability == crate::remote::machine_protocol::Availability::Ready
                }).map(|p| p.remote_workspace_id.clone());

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
                    // The host journal dedups by request id, so a bounded resubmit after a transport-level
                    // failure (malformed or lost response) is idempotent and keeps a transient tunnel
                    // hiccup from failing the user's worktree open.
                    let reg_request_id = uuid::Uuid::new_v4().to_string();
                    let reg_op = || crate::paired_host::client::Operation::RegisterProject {
                        request: crate::remote::machine_protocol::RegisterRequest {
                            request_id: reg_request_id.clone(),
                            repo_path: cwd_str.to_string(),
                        },
                    };
                    let mut reg_resp = daemon_client
                        .paired_host_operation(crate::paired_host::client::OperationRequest {
                            host_id: host_id.clone(),
                            generation: host.generation,
                            operation: reg_op(),
                        })
                        .await;
                    {
                        let mut resubmits = 0;
                        loop {
                            let retryable = match &reg_resp {
                                Err(e) => is_retryable_paired_transport_error(&e.code),
                                Ok(_) => false,
                            };
                            if !retryable || resubmits >= 2 {
                                break;
                            }
                            resubmits += 1;
                            tokio::time::sleep(std::time::Duration::from_millis(300 * resubmits))
                                .await;
                            reg_resp = daemon_client
                                .paired_host_operation(
                                    crate::paired_host::client::OperationRequest {
                                        host_id: host_id.clone(),
                                        generation: host.generation,
                                        operation: reg_op(),
                                    },
                                )
                                .await;
                        }
                    }
                    match reg_resp {
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
            } else if cwd_path.components().all(|c| {
                matches!(
                    c,
                    std::path::Component::Normal(_) | std::path::Component::CurDir
                )
            }) && !cwd_path.as_os_str().is_empty()
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

        let mut active_request_id = client_request_id.clone();

        let mut op_resp = daemon_client
            .paired_host_operation(crate::paired_host::client::OperationRequest {
                host_id: host_id.clone(),
                generation: host.generation,
                operation: crate::paired_host::client::Operation::CreateSession {
                    request: create_request.clone(),
                },
            })
            .await;

        // Bounded transport retry: PAIRED_HOST_INVALID_RESPONSE / TIMEOUT / HOST_UNAVAILABLE
        // mean the response was lost or malformed, not that the request was rejected.
        // The host journal dedups by request id, so resubmitting the same id is
        // idempotent; without this, a transient tunnel hiccup surfaces as a failed
        // user action.
        {
            let mut resubmits = 0;
            loop {
                let retryable = match &op_resp {
                    Err(e) => is_retryable_paired_transport_error(&e.code),
                    Ok(_) => false,
                };
                if !retryable || resubmits >= 2 {
                    break;
                }
                resubmits += 1;
                tokio::time::sleep(std::time::Duration::from_millis(300 * resubmits)).await;
                op_resp = daemon_client
                    .paired_host_operation(crate::paired_host::client::OperationRequest {
                        host_id: host_id.clone(),
                        generation: host.generation,
                        operation: crate::paired_host::client::Operation::CreateSession {
                            request: create_request.clone(),
                        },
                    })
                    .await;
            }
        }

        if let Err(ref e) = op_resp {
            if !e.ambiguous
                && resolved_inherit.is_some()
                && matches!(
                    e.code.as_str(),
                    "SESSION_NOT_FOUND" | "PARENT_SESSION_MISMATCH" | "SESSION_EXPIRED"
                )
            {
                tracing::warn!(
                    parent_sid = ?resolved_inherit,
                    error = %e.code,
                    "Retrying paired session spawn without parent session inheritance"
                );
                let fallback_request_id = uuid::Uuid::new_v4().to_string();
                active_request_id = fallback_request_id.clone();
                let fallback_request = crate::remote::machine_protocol::CreateSessionRequest {
                    request_id: fallback_request_id,
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
                    return Err(IpcError::internal(format!(
                        "Unexpected operation result: {other:?}"
                    )));
                }
            },
            Err(ref e) if should_reconcile_create_error(e.ambiguous, &e.code) => {
                match reconcile_ambiguous_create(
                    &daemon_client,
                    &host_id,
                    host.generation,
                    &active_request_id,
                    false,
                )
                .await?
                {
                    Some(session) => session,
                    None => {
                        let unknown_err = crate::paired_host::client::ClientError {
                            code: "OPERATION_OUTCOME_UNKNOWN".to_string(),
                            machine_error: None,
                            request_id: Some(active_request_id.clone()),
                            ambiguous: true,
                        };
                        return Err(map_client_error(
                            &unknown_err,
                            Some(&host_id),
                            Some(host.generation),
                        ));
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
            Err(e)
                if matches!(
                    e.code.as_str(),
                    "TIMEOUT" | "HOST_UNAVAILABLE" | "PAIRED_PROXY_UNAVAILABLE" | "PAIRED_HOST_INVALID_RESPONSE"
                ) =>
            {
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
                        return Err(map_client_error(
                            &reattach_error,
                            Some(&host_id),
                            Some(host.generation),
                        ));
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
                return Err(map_client_error(
                    &reattach_error,
                    Some(&host_id),
                    Some(host.generation),
                ));
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
            })
            .await;
        }

        // The attach fence proves identity by comparing the pane binding's incarnation with the
        // owner's authoritative answer, so this spawn must project the proxy's own incarnation.
        // Leaving the field empty (the previous behaviour) made the binding unbuildable, so every
        // paired attach was rejected as unprovable before it could reach the fence's owner check.
        let incarnation = match daemon_client.describe_session(&proxy_session_id).await {
            Ok(details) => details.incarnation,
            Err(error) => {
                tracing::warn!(
                    session_id = %proxy_session_id,
                    %error,
                    "paired spawn could not read the proxy incarnation; attach re-reads it from the owner"
                );
                None
            }
        };

        crate::daemon::client::DaemonSpawnResult {
            session_id: proxy_session_id.clone(),
            epoch: host.generation.0,
            session: crate::daemon::protocol::DaemonSessionDetails {
                session_id: proxy_session_id,
                workspace_id: Some(request.workspace_id.clone()),
                worktree: request.worktree.clone(),
                cwd: request
                    .cwd
                    .map(|p| p.to_string_lossy().to_string())
                    .or(Some(remote_session.cwd)),
                cols,
                rows,
                running: true,
                start_sequence: Some(remote_session.start_sequence.0),
                end_sequence: Some(remote_session.end_sequence.0),
                last_output_age_ms: None,
                suspended: false,
                reader_paused: None,
                kernel_stopped: None,
                registry_suspended: None,
                suspension_source: None,
                // The paired proxy actor's own incarnation, as reported by its owning daemon.
                incarnation,
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
            resolve_spawn_cwd(
                requested_cwd,
                worktree_root_for_validation,
                |requested| {
                    worktree_for_validation
                        .canonical_allowed_path(requested)
                        .map_err(IpcError::from)
                },
            )
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
    if let Some(host) =
        app.try_state::<crate::native_terminal::surface_host::NativeTerminalSurfaceHostState>()
    {
        host.mark_pending_startup(&session_id);
    }
    let attachment = match daemon_client.attach(&session_id, None).await {
        Ok(attachment) => attachment,
        Err(err) => {
            if let Some(host) = app
                .try_state::<crate::native_terminal::surface_host::NativeTerminalSurfaceHostState>(
            ) {
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
        if let Some(host) =
            app.try_state::<crate::native_terminal::surface_host::NativeTerminalSurfaceHostState>()
        {
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
    split_attempt: Option<crate::daemon::protocol::SplitAttachAttempt>,
    attach_tuple: Option<crate::daemon::protocol::PaneAttachTuple>,
) -> Result<AttachTerminalResponse, IpcError> {
    if let Some(binding) = attach_tuple {
        if binding.backend_session_id != session_id || binding.incarnation.is_none() {
            return Err(IpcError::new(IpcErrorCode::InvalidArgument, "Attach binding must identify the exact backend incarnation"));
        }
        let remaining = split_attempt.as_ref().map(|attempt| attempt.remaining_ms)
            .unwrap_or(crate::daemon::protocol::STAGE_ATTACH_OR_LISTENER_MAX_MS);
        let deadline = split_stage_deadline(remaining, crate::daemon::protocol::STAGE_ATTACH_OR_LISTENER_MAX_MS)?;
        #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
        {
            if let Some(attempt) = split_attempt.as_ref() {
                match qa_split_producers::hold_attach_handshake(
                    &daemon_client,
                    &session_id,
                    &binding,
                    attempt,
                )
                .await
                {
                    qa_split_producers::AttachHold::Failed(error) => return Err(error),
                    qa_split_producers::AttachHold::NotApplicable
                    | qa_split_producers::AttachHold::Released => {}
                }
            }
        }
        let token = reserve_managed_pump(&session_id);
        let description = if let Some(attempt) = &split_attempt {
            if attempt.frontend_session_id != binding.frontend_session_id
                || attempt.generation != binding.attempt_generation
            { return Err(IpcError::spawn_request_conflict("Attach attempt differs from durable pane binding")); }
            match daemon_client.local_split_status_until(&attempt.identity, deadline).await? {
                crate::daemon::protocol::SplitOperationResult::Created { session_id: owned, session, .. }
                    if owned == session_id => session,
                _ => return Err(IpcError::new(IpcErrorCode::OperationOutcomeUnknown,
                    "Attach requires a journal-confirmed creation for this request")),
            }
        } else {
            daemon_client.describe_session_bounded_until(&session_id, deadline).await?
        };
        if description.incarnation != binding.incarnation {
            return Err(IpcError::spawn_request_conflict("Attach incarnation differs from authoritative owner"));
        }
        let attachment = daemon_client.attach_until(&session_id,
            after_sequence.as_deref().and_then(|value| value.parse().ok()), deadline).await?;
        let mut authoritative = binding;
        authoritative.daemon_epoch = attachment.epoch.to_string();
        let response = AttachTerminalResponse {
            session_id: attachment.session_id.clone(), daemon_epoch: Some(authoritative.daemon_epoch.clone()),
            incarnation: description.incarnation, attach_tuple: Some(authoritative),
            history_start_sequence: attachment.start_sequence.map(|value| value.to_string()),
            history_end_sequence: attachment.end_sequence.map(|value| value.to_string()),
            history: STANDARD.encode(&attachment.history),
            gap: attachment.gap.as_ref().map(|gap| TerminalReplayGap {
                requested_after_sequence: gap.requested_after_sequence.to_string(),
                available_from_sequence: gap.available_from_sequence.to_string(),
            }),
        };
        start_managed_pump_token(session_id, app, attachment, token);
        return Ok(response);
    }
    if split_attempt.is_some() {
        return Err(IpcError::unsupported_capability("Split attach requires paneIdentity and bindingKey in attachTuple; upgrade the frontend binding request"));
    }
    tokio::time::timeout(
        std::time::Duration::from_secs(30),
        terminal_attach(app, daemon_client, session_id.clone(), after_sequence),
    )
    .await
    .map_err(|_| {
        IpcError::new(
            IpcErrorCode::Timeout,
            "Terminal attachment timed out. Retry reconnecting.",
        )
        .with_details(serde_json::json!({ "sessionId": session_id, "phase": "attach" }))
    })?
}

async fn terminal_attach<R: Runtime>(
    app: AppHandle<R>,
    daemon_client: State<'_, Arc<DaemonClient>>,
    session_id: String,
    after_sequence: Option<String>,
) -> Result<AttachTerminalResponse, IpcError> {
    let pump_token = reserve_managed_pump(&session_id);
    let after_seq = after_sequence
        .as_deref()
        .and_then(|s| s.parse::<u64>().ok());

    let (attachment, target_session_id) = if session_id.starts_with("daemon-session:") {
        match daemon_client
            .paired_terminal_descriptor(session_id.clone())
            .await
        {
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
                    .map_err(|client_err| {
                        map_client_error(&client_err, Some(&host_id), Some(generation))
                    })?;

                let timeout_duration = hub_attach_retry_deadline();
                let attach_deadline = tokio::time::Instant::now() + timeout_duration;
                let mut poll_interval = std::time::Duration::from_millis(50);

                loop {
                    match daemon_client
                        .attach(&proxy_session_id, effective_after_seq)
                        .await
                    {
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
            Err(e)
                if e.ambiguous
                    || matches!(e.code.as_str(), "TIMEOUT" | "OPERATION_OUTCOME_UNKNOWN") =>
            {
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

    #[cfg(feature = "native-terminal")]
    let binding = app.try_state::<crate::native_terminal::surface_host::NativeTerminalSurfaceHostState>()
        .and_then(|state| state.session_attach_tuple(&target_session_id));
    #[cfg(not(feature = "native-terminal"))]
    let binding: Option<crate::daemon::protocol::PaneAttachTuple> = None;
    let Some(mut binding) = binding else {
        attachment.stream_task.abort();
        return Err(IpcError::unsupported_capability(
            "Attach requires the persisted seven-field pane binding; pass attachTuple"));
    };
    let description = daemon_client.describe_session(&target_session_id).await?;
    if description.incarnation.is_none() || description.incarnation != binding.incarnation {
        attachment.stream_task.abort();
        return Err(IpcError::spawn_request_conflict("Attach binding incarnation cannot be proven"));
    }
    binding.daemon_epoch = attachment.epoch.to_string();
    let resp = AttachTerminalResponse {
        session_id: attachment.session_id.clone(),
        incarnation: description.incarnation,
        attach_tuple: Some(binding),
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
    if target_session_id == session_id {
        start_managed_pump_token(target_session_id, app, attachment, pump_token);
    } else {
        start_managed_pump(target_session_id, app, attachment);
    }

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
        if is_plausible_absolute_cwd(&cwd) {
            return Ok(TerminalCwdResponse { cwd });
        }
        invalidate_cached_cwd(&session_id);
    }

    let details = daemon_client.describe_session(&session_id).await?;
    let cwd_str = details
        .cwd
        .ok_or_else(|| IpcError::internal("terminal cwd is unavailable"))?;
    let cwd = PathBuf::from(cwd_str);
    if !is_plausible_absolute_cwd(&cwd) {
        return Err(IpcError::internal("terminal cwd is unavailable"));
    }
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
pub async fn cmd_terminal_remote_write(
    daemon_client: State<'_, Arc<DaemonClient>>,
    session_id: String,
    generation: u64,
    data: String,
) -> Result<(), IpcError> {
    daemon_client
        .write_terminal_at_generation(&session_id, Some(generation), data.into_bytes())
        .await
}

#[tauri::command]
pub async fn cmd_terminal_remote_resize(
    daemon_client: State<'_, Arc<DaemonClient>>,
    session_id: String,
    generation: u64,
    cols: u16,
    rows: u16,
) -> Result<(), IpcError> {
    daemon_client
        .resize_terminal_at_generation(&session_id, Some(generation), cols, rows)
        .await
}

pub(crate) fn remote_control_result(
    reply: crate::daemon::protocol::DaemonResponse,
) -> Result<(), IpcError> {
    use crate::daemon::protocol::DaemonResponse;
    match reply {
        DaemonResponse::WriteOk | DaemonResponse::ResizeOk => Ok(()),
        DaemonResponse::RemoteSessionError { failure } => {
            Err(IpcError::internal(failure.to_string()).with_details(
                serde_json::to_value(failure).map_err(|e| IpcError::internal(e.to_string()))?,
            ))
        }
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
        details.insert(
            "requestId".to_string(),
            serde_json::Value::String(req_id.clone()),
        );
    } else if let Some(ref me) = err.machine_error {
        if !me.request_id.is_empty() {
            details.insert(
                "requestId".to_string(),
                serde_json::Value::String(me.request_id.clone()),
            );
        }
    }

    let ambiguous =
        err.ambiguous || matches!(err.code.as_str(), "TIMEOUT" | "OPERATION_OUTCOME_UNKNOWN");
    details.insert("ambiguous".to_string(), serde_json::Value::Bool(ambiguous));

    if let Some(ref me) = err.machine_error {
        if let Ok(val) = serde_json::to_value(me) {
            details.insert("machineError".to_string(), val);
        }
    }

    if let Some(h) = host_id {
        details.insert(
            "hostId".to_string(),
            serde_json::Value::String(h.to_string()),
        );
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
    fn pane_liveness_pump_generation_stale_teardown_preserves_new_binding() {
        let session = format!("pump-test-{}", uuid::Uuid::new_v4());
        let old = reserve_managed_pump(&session);
        let current = reserve_managed_pump(&session);
        stop_managed_pump_token(&session, old);
        assert_eq!(PUMP_BINDINGS.lock().as_ref().unwrap().get(&session), Some(&current));
        stop_managed_pump_token(&session, current);
        assert!(!PUMP_BINDINGS.lock().as_ref().unwrap().contains_key(&session));
    }

    #[test]
    fn local_split_reliability_stage_budget_is_clipped() {
        assert!(split_stage_deadline(0, 9000).is_err());
        assert_eq!(crate::daemon::protocol::clip_stage_budget(1000, 9000), 1000);
        assert_eq!(crate::daemon::protocol::clip_stage_budget(9000, 3000), 3000);
    }

    #[tokio::test]
    async fn pane_liveness_pump_generation_old_teardown_preserves_new_stream_atomically() {
        use tauri::Manager;
        let session = format!("pump-teardown-{}", uuid::Uuid::new_v4());
        let old = reserve_managed_pump(&session);
        let current = reserve_managed_pump(&session);
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets())).unwrap();
        let (sender, messages) = tokio::sync::mpsc::channel(1);
        let stream_task = tokio::spawn(std::future::pending::<()>());
        let closed = sender.closed();
        tokio::pin!(closed);
        start_managed_pump_token(session.clone(), app.handle().clone(), DaemonAttachment {
            session_id: session.clone(), epoch: 1, start_sequence: None, end_sequence: None,
            gap: None, history: bytes::Bytes::new(), history_segments: Vec::new(),
            pty_cols: None, pty_rows: None, remote_generation: None, messages, stream_task,
        }, current);
        stop_managed_pump_token(&session, old);
        assert_eq!(PUMP_BINDINGS.lock().as_ref().unwrap().get(&session), Some(&current));
        assert_eq!(ACTIVE_PUMPS.lock().as_ref().unwrap().get(&session).unwrap().token, current);
        stop_managed_pump_token(&session, current);
        tokio::time::timeout(Duration::from_secs(1), closed).await.unwrap();
    }

    #[tokio::test]
    async fn pane_liveness_pump_generation_old_completion_cannot_install_new_stream() {
        use tauri::Manager;
        let session = format!("pump-install-{}", uuid::Uuid::new_v4());
        let old = reserve_managed_pump(&session);
        let current = reserve_managed_pump(&session);
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets())).unwrap();
        let (sender, messages) = tokio::sync::mpsc::channel(1);
        let stream_task = tokio::spawn(std::future::pending::<()>());
        let closed = sender.closed();
        tokio::pin!(closed);
        start_managed_pump_token(session.clone(), app.handle().clone(), DaemonAttachment {
            session_id: session.clone(), epoch: 1, start_sequence: None, end_sequence: None,
            gap: None, history: bytes::Bytes::new(), history_segments: Vec::new(),
            pty_cols: None, pty_rows: None, remote_generation: None, messages, stream_task,
        }, old);
        tokio::time::timeout(Duration::from_secs(1), closed).await.unwrap();
        assert_eq!(PUMP_BINDINGS.lock().as_ref().unwrap().get(&session), Some(&current));
        assert!(!ACTIVE_PUMPS.lock().as_ref().is_some_and(|map| map.contains_key(&session)));
        stop_managed_pump_token(&session, current);
    }

    #[tokio::test(start_paused = true)]
    async fn reconnect_attach_settles_when_daemon_never_answers_handshake() {
        use tauri::Manager;
        use tokio::io::{AsyncBufReadExt, BufReader};

        // Given an isolated daemon endpoint which accepts but never responds.
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("silent-attach");
        #[cfg(unix)]
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        #[cfg(not(unix))]
        let listener = {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            std::fs::write(&path, listener.local_addr().unwrap().port().to_string()).unwrap();
            listener
        };
        let client = Arc::new(DaemonClient::new_with_socket(path));
        let app = tauri::test::mock_builder()
            .manage(client)
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let (observed, handshake) = tokio::sync::oneshot::channel();
        let peer = async {
            let (stream, _) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            assert!(reader.read_line(&mut line).await.unwrap() > 0);
            observed.send(()).unwrap();
            line.clear();
            // Cancellation must release this connection, not send PTY Close.
            assert_eq!(reader.read_line(&mut line).await.unwrap(), 0);
        };
        let action = async {
            // When the real IPC entry point attempts the reconnect attachment.
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(120),
                cmd_terminal_attach(app.handle().clone(), app.state::<Arc<DaemonClient>>(),
                    "qa-existing-session".into(), None, None, None),
            ).await;
            // Then the command itself must settle, not this test's safety bound.
            assert!(result.is_ok(), "reconnect attach exceeded its bounded-error contract");
            let error = result.unwrap().err().expect("attachment must fail");
            assert_eq!(error.code, IpcErrorCode::Timeout);
            assert_eq!(error.details.unwrap()["phase"], "attach");
        };
        let clock = async {
            handshake.await.unwrap();
            tokio::time::advance(std::time::Duration::from_secs(60)).await;
        };
        tokio::join!(peer, action, clock);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn p11_reap_keeps_unresolved_cleanup_in_authoritative_and_persisted_state() {
        static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _env_guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("FERRYX_PAIRED_PENDING_CLEANUPS_DIR", dir.path());
        clear_pending_cleanups_for_test();
        clear_exhausted_cleanups_for_test();

        let record = |request_id: &str, attempts: usize| CleanupRecord {
            host_id: format!("host-{request_id}"),
            generation: crate::scoped_contracts::Epoch(1),
            session_id: format!("sess-{request_id}"),
            daemon_epoch: crate::scoped_contracts::Epoch(1),
            cleanup_request_id: request_id.to_string(),
            attempts,
            resolved: false,
        };
        PENDING_CLEANUPS.lock().push(record("cleanup-exhausted", 3));
        PENDING_CLEANUPS
            .lock()
            .push(record("cleanup-unresolved", 0));

        let socket = dir.path().join("never-answers.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        let client = crate::daemon::client::DaemonClient::new_with_socket(socket);

        let reap = reap_cleanup_unknowns(&client);
        let _ = tokio::time::timeout(std::time::Duration::from_millis(250), reap).await;

        assert!(get_pending_cleanups()
            .iter()
            .any(|r| r.cleanup_request_id == "cleanup-unresolved"));
        assert!(get_exhausted_cleanups()
            .iter()
            .any(|r| r.cleanup_request_id == "cleanup-exhausted"));

        let data = std::fs::read(dir.path().join("paired_pending_cleanups.json")).unwrap();
        #[derive(serde::Deserialize)]
        struct Saved {
            pending: Vec<serde_json::Value>,
            exhausted: Vec<serde_json::Value>,
        }
        let saved: Saved = serde_json::from_slice(&data).unwrap();
        assert!(saved
            .pending
            .iter()
            .any(|v| v.get("cleanup_request_id").and_then(|s| s.as_str())
                == Some("cleanup-unresolved")));
        assert!(saved
            .exhausted
            .iter()
            .any(|v| v.get("cleanup_request_id").and_then(|s| s.as_str())
                == Some("cleanup-exhausted")));

        std::env::remove_var("FERRYX_PAIRED_PENDING_CLEANUPS_DIR");
        clear_pending_cleanups_for_test();
        clear_exhausted_cleanups_for_test();
        drop(listener);
    }

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
        assert_eq!(
            details.get("requestId").and_then(|v| v.as_str()),
            Some("req-test-123")
        );
        assert_eq!(
            details.get("ambiguous").and_then(|v| v.as_bool()),
            Some(false)
        );
        assert_eq!(
            details.get("hostId").and_then(|v| v.as_str()),
            Some("paired-host-42")
        );
        assert_eq!(
            details.get("generation").and_then(|v| v.as_str()),
            Some("10")
        );
        assert!(details.get("machineError").is_some());
    }

    #[test]
    fn test_p08_client_error_variants_and_passthrough_mapping() {
        // Test various produced codes map to typed variants
        for (code, expected) in [
            ("SESSION_EXPIRED", IpcErrorCode::SessionExpired),
            (
                "PARENT_SESSION_MISMATCH",
                IpcErrorCode::ParentSessionMismatch,
            ),
            ("TIMEOUT", IpcErrorCode::Timeout),
            ("HOST_UNAVAILABLE", IpcErrorCode::HostUnavailable),
            (
                "OPERATION_OUTCOME_UNKNOWN",
                IpcErrorCode::OperationOutcomeUnknown,
            ),
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

    #[test]
    fn test_encode_terminal_output_frame_v2_gap() {
        let session_id = "test-session-gap";
        let history = b"\x1b[32mhello replay\x1b[0m";
        let sequence = Some(500u64);
        let daemon_epoch = Some(42u64);
        let gap = TerminalReplayGapFields {
            requested_after_sequence: 100,
            available_from_sequence: 200,
            start_sequence: 201,
            end_sequence: 500,
        };

        let frame = encode_terminal_output_frame(
            session_id,
            history,
            sequence,
            daemon_epoch,
            Some(gap),
        )
        .expect("frame should encode");

        // Version 2
        assert_eq!(frame[0], 2);
        // Flags: HAS_SEQUENCE (1<<0) | HAS_DAEMON_EPOCH (1<<1) | HAS_GAP (1<<2) = 7
        assert_eq!(
            frame[1] & TERMINAL_OUTPUT_FRAME_HAS_GAP,
            TERMINAL_OUTPUT_FRAME_HAS_GAP
        );
        assert_eq!(frame[1], (1 << 0) | (1 << 1) | (1 << 2));

        // Session ID length (u16le) at offset 2..4
        let session_id_len = u16::from_le_bytes(frame[2..4].try_into().unwrap()) as usize;
        assert_eq!(session_id_len, session_id.len());

        // Sequence (u64le) at offset 4..12
        let seq = u64::from_le_bytes(frame[4..12].try_into().unwrap());
        assert_eq!(seq, 500);

        // Daemon epoch (u64le) at offset 12..20
        let epoch = u64::from_le_bytes(frame[12..20].try_into().unwrap());
        assert_eq!(epoch, 42);

        // Session id bytes at 20..20+session_id_len
        assert_eq!(&frame[20..20 + session_id_len], session_id.as_bytes());

        // Gap fields (4x u64le) at offset 20+session_id_len .. 20+session_id_len+32
        let gap_offset = 20 + session_id_len;
        let req_after = u64::from_le_bytes(frame[gap_offset..gap_offset + 8].try_into().unwrap());
        assert_eq!(req_after, 100);
        let avail_from =
            u64::from_le_bytes(frame[gap_offset + 8..gap_offset + 16].try_into().unwrap());
        assert_eq!(avail_from, 200);
        let start_seq =
            u64::from_le_bytes(frame[gap_offset + 16..gap_offset + 24].try_into().unwrap());
        assert_eq!(start_seq, 201);
        let end_seq =
            u64::from_le_bytes(frame[gap_offset + 24..gap_offset + 32].try_into().unwrap());
        assert_eq!(end_seq, 500);

        // Data offset: immediately after the 32-byte gap block
        let data_offset = gap_offset + 32;
        assert_eq!(&frame[data_offset..], history);
        assert_eq!(frame.len(), data_offset + history.len());
    }

    #[test]
    fn test_encode_terminal_output_frame_v1_unchanged() {
        let session_id = "test-session-v1";
        let data = b"regular output";
        let sequence = Some(123u64);
        let daemon_epoch = Some(456u64);

        let frame =
            encode_terminal_output_frame(session_id, data, sequence, daemon_epoch, None)
                .expect("frame should encode");

        // Version 1
        assert_eq!(frame[0], 1);
        // Flags: HAS_SEQUENCE | HAS_DAEMON_EPOCH, no HAS_GAP
        assert_eq!(frame[1] & TERMINAL_OUTPUT_FRAME_HAS_GAP, 0);
        assert_eq!(frame[1], (1 << 0) | (1 << 1));

        let session_id_len = u16::from_le_bytes(frame[2..4].try_into().unwrap()) as usize;
        let data_offset = 20 + session_id_len;
        assert_eq!(&frame[data_offset..], data);
        assert_eq!(frame.len(), data_offset + data.len());
    }

    #[test]
    fn poisoned_session_cwd_is_never_treated_as_a_path() {
        let poisoned = PathBuf::from("cwd|rtd info error: No such file or directory");
        assert!(!is_plausible_absolute_cwd(&poisoned));
        assert!(!is_usable_terminal_cwd(&poisoned));
        assert!(!is_plausible_session_cwd_text(&poisoned.to_string_lossy()));
        assert_eq!(select_requested_cwd(Some(poisoned)), None);
    }

    #[test]
    fn relative_and_absent_cwds_fall_back_to_the_worktree_root() {
        assert_eq!(select_requested_cwd(Some(PathBuf::from("relative/path"))), None);
        assert_eq!(
            select_requested_cwd(Some(PathBuf::from("/ferryx-absent-cwd-for-test"))),
            None
        );
        let existing = std::env::temp_dir();
        assert_eq!(select_requested_cwd(Some(existing.clone())), Some(existing));
    }

    #[test]
    fn session_cwd_text_accepts_posix_and_windows_absolute_paths() {
        assert!(is_plausible_session_cwd_text("/home/indo/project"));
        assert!(is_plausible_session_cwd_text("C:\\Users\\sook\\work\\steam"));
        assert!(is_plausible_session_cwd_text("\\\\host\\share"));
        assert!(!is_plausible_session_cwd_text(""));
        assert!(!is_plausible_session_cwd_text("cwd|rtd info error: No such file or directory"));
        assert!(!is_plausible_session_cwd_text("/tmp/with\nnewline"));
    }

    #[test]
    fn poisoned_requested_cwd_spawns_in_the_worktree_root() {
        let root = PathBuf::from("/repo");
        let mut canonicalized = false;
        let resolved = resolve_spawn_cwd(
            Some(PathBuf::from("cwd|rtd info error: No such file or directory")),
            root.clone(),
            |_| {
                canonicalized = true;
                Ok(root.clone())
            },
        );
        assert_eq!(resolved, Ok(root));
        assert!(!canonicalized, "a discarded cwd must not reach canonicalization");
    }

    #[test]
    fn usable_requested_cwd_is_canonicalized() {
        let root = tempfile::tempdir().expect("root");
        let inside = root.path().join("sub");
        std::fs::create_dir_all(&inside).expect("create sub dir");
        let canonical = std::fs::canonicalize(&inside).expect("canonicalize");
        let resolved = resolve_spawn_cwd(Some(inside), canonical.clone(), |_| Ok(canonical.clone()));
        assert_eq!(resolved, Ok(canonical));
    }

    #[test]
    fn existing_requested_cwd_outside_the_worktree_is_rejected() {
        let root = tempfile::tempdir().expect("root");
        let outside = tempfile::tempdir().expect("outside");
        let outside_path = outside.path().to_path_buf();
        let err = resolve_spawn_cwd(
            Some(outside_path.clone()),
            root.path().to_path_buf(),
            |requested| Ok(requested.to_path_buf()),
        )
        .expect_err("an existing cwd outside the worktree must be rejected");
        assert_eq!(err.code, IpcErrorCode::PathOutsideWorkspace);
    }
}
