// allow: SIZE_OK — IPC module bundling terminal commands, output batching, and macOS libproc CWD resolution within constrained write scope
use crate::daemon::client::{DaemonAttachment, DaemonClient};
use crate::daemon::protocol::DaemonStreamMessage;
use crate::ipc::{run_blocking, IpcError};
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
use tauri::{AppHandle, Emitter, Runtime, State};

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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpawnTerminalResponse {
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalSessionSummary {
    pub session_id: String,
    pub worktree_path: Option<PathBuf>,
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
}

impl From<TerminalSignalRequest> for TerminalSignal {
    fn from(value: TerminalSignalRequest) -> Self {
        match value {
            TerminalSignalRequest::Interrupt => TerminalSignal::Interrupt,
            TerminalSignalRequest::Terminate => TerminalSignal::Terminate,
            TerminalSignalRequest::Kill => TerminalSignal::Kill,
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
    let registry = (*registry).clone();
    let workspace_id = request.workspace_id.clone();
    let identity = request.worktree.clone();
    let requested_cwd = request.cwd.clone();

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
    let effective_shell = request
        .shell
        .filter(|s| !s.trim().is_empty())
        .or_else(|| crate::terminal::cached_terminal_preferences().default_shell.clone());
    let session_id = match daemon_client
        .spawn_terminal(
            client_request_id,
            request.workspace_id,
            request.worktree,
            Some(cwd.to_string_lossy().to_string()),
            cols,
            rows,
            effective_shell,
        )
        .await
    {
        Ok(session_id) => session_id,
        Err(err) => {
            eprintln!(
                "[cmd_terminal_spawn] stage=daemon_spawn failed code={:?}",
                err.code
            );
            return Err(err);
        }
    };

    let attachment = match daemon_client.attach(&session_id, None).await {
        Ok(attachment) => attachment,
        Err(err) => {
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
        eprintln!("[cmd_terminal_spawn] stage=emit_lifecycle failed");
        let _ = daemon_client.close_terminal(&session_id).await;
        return Err(IpcError::internal(format!(
            "failed to emit terminal lifecycle event: {error}"
        )));
    }

    Ok(SpawnTerminalResponse { session_id })
}

#[tauri::command]
pub async fn cmd_terminal_attach<R: Runtime>(
    app: AppHandle<R>,
    daemon_client: State<'_, Arc<DaemonClient>>,
    session_id: String,
    after_sequence: Option<String>,
) -> Result<AttachTerminalResponse, IpcError> {
    let after_seq = after_sequence.and_then(|s| s.parse::<u64>().ok());
    let attachment = daemon_client.attach(&session_id, after_seq).await?;

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

    start_managed_pump(session_id, app, attachment);

    Ok(resp)
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

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
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
pub async fn cmd_terminal_close(
    daemon_client: State<'_, Arc<DaemonClient>>,
    session_id: String,
) -> Result<(), IpcError> {
    invalidate_cached_cwd(&session_id);
    daemon_client.close_terminal(&session_id).await
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
            });
        }
    }
    Ok(summaries)
}
