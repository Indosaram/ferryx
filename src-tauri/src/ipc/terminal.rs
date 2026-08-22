use crate::ipc::{run_blocking, IpcError};
use crate::terminal::{PtyError, PtySessionState, TerminalService, TerminalSignal};
use crate::worktree::{WorkspaceRegistry, WorktreeError, WorktreeIdentity};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use portable_pty::CommandBuilder;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Runtime, State};

pub const TERMINAL_OUTPUT_EVENT: &str = "terminal_output";
pub const TERMINAL_LIFECYCLE_EVENT: &str = "terminal_lifecycle";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpawnTerminalRequest {
    pub workspace_id: String,
    pub worktree: Option<WorktreeIdentity>,
    /// Optional inherited CWD for an Orca split/restore.  The command validates that
    /// this path exists and remains inside the resolved worktree before spawning.
    pub cwd: Option<PathBuf>,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalOutputPayload {
    pub session_id: String,
    pub data: String,
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

#[tauri::command]
pub async fn cmd_terminal_spawn<R: Runtime>(
    app: AppHandle<R>,
    terminal_service: State<'_, Arc<TerminalService>>,
    registry: State<'_, WorkspaceRegistry>,
    request: SpawnTerminalRequest,
) -> Result<SpawnTerminalResponse, IpcError> {
    let cols = request.cols.unwrap_or(80);
    let rows = request.rows.unwrap_or(24);
    let registry = (*registry).clone();
    let workspace_id = request.workspace_id.clone();
    let identity = request.worktree.clone();
    let requested_cwd = request.cwd.clone();

    let (worktree_manager, worktree_root) = run_blocking(move || {
        registry
            .resolve_terminal_target(&workspace_id, identity.as_ref())
            .map_err(IpcError::from)
    })
    .await?;

    let worktree_for_validation = worktree_manager.clone();
    let worktree_root_for_validation = worktree_root.clone();
    let cwd = run_blocking(move || {
        let Some(requested) = requested_cwd else {
            return Ok(worktree_root_for_validation);
        };
        let canonical = worktree_for_validation
            .canonical_allowed_path(&requested)
            .map_err(IpcError::from)?;
        if canonical != worktree_root_for_validation && !canonical.starts_with(&worktree_root_for_validation) {
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
    .await?;

    let mut cmd = CommandBuilder::new_default_prog();
    cmd.cwd(&cwd);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("PROMPT_EOL_MARK", "");
    if let Ok(lang) = std::env::var("LANG") {
        cmd.env("LANG", lang);
    } else {
        cmd.env("LANG", "en_US.UTF-8");
    }
    if let Ok(lc_all) = std::env::var("LC_ALL") {
        cmd.env("LC_ALL", lc_all);
    } else {
        cmd.env("LC_ALL", "en_US.UTF-8");
    }
    if let Ok(lc_ctype) = std::env::var("LC_CTYPE") {
        cmd.env("LC_CTYPE", lc_ctype);
    } else {
        cmd.env("LC_CTYPE", "en_US.UTF-8");
    }
    let (session_id, mut broadcast_rx) = terminal_service
        .spawn_in_worktree(cmd, cols, rows, &worktree_manager, &worktree_root)
        .map_err(IpcError::from)?;
    let session = terminal_service
        .get_session(&session_id)
        .ok_or_else(|| IpcError::internal(format!("PTY session '{session_id}' disappeared during spawn")))?;

    let started = TerminalLifecyclePayload {
        session_id: session_id.clone(),
        state: TerminalLifecycleState::Started,
        exit_code: None,
        reason: None,
    };
    if let Err(error) = app.emit(TERMINAL_LIFECYCLE_EVENT, started) {
        let _ = terminal_service.close_session(&session_id).await;
        return Err(IpcError::internal(format!(
            "failed to emit terminal lifecycle event: {error}"
        )));
    }

    let session_id_clone = session_id.clone();
    let app_handle = app.clone();
    tokio::spawn(async move {
        loop {
            match broadcast_rx.recv().await {
                Ok(chunk) => {
                    let payload = TerminalOutputPayload {
                        session_id: session_id_clone.clone(),
                        data: STANDARD.encode(chunk),
                    };
                    if let Err(error) = app_handle.emit(TERMINAL_OUTPUT_EVENT, payload) {
                        tracing::debug!("Failed to emit terminal output event: {error}");
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(lag)) => {
                    tracing::warn!("Tauri desktop subscriber lagged by {lag} messages");
                    continue;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    break;
                }
            }
        }

        for _ in 0..100 {
            let lifecycle = match session.state() {
                PtySessionState::Exited { code } => Some(TerminalLifecyclePayload {
                    session_id: session_id_clone.clone(),
                    state: TerminalLifecycleState::Exited,
                    exit_code: code,
                    reason: None,
                }),
                PtySessionState::Failed { reason } => Some(TerminalLifecyclePayload {
                    session_id: session_id_clone.clone(),
                    state: TerminalLifecycleState::Failed,
                    exit_code: None,
                    reason: Some(reason),
                }),
                PtySessionState::Starting | PtySessionState::Running | PtySessionState::Closing => {
                    None
                }
            };

            if let Some(payload) = lifecycle {
                if let Err(error) = app_handle.emit(TERMINAL_LIFECYCLE_EVENT, payload) {
                    tracing::debug!("Failed to emit terminal lifecycle event: {error}");
                }
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    });

    Ok(SpawnTerminalResponse { session_id })
}

#[tauri::command]
pub async fn cmd_terminal_get_cwd(
    terminal_service: State<'_, Arc<TerminalService>>,
    session_id: String,
) -> Result<TerminalCwdResponse, IpcError> {
    let session = terminal_service
        .get_session(&session_id)
        .ok_or_else(|| IpcError::from(PtyError::SessionNotFound(session_id.clone())))?;
    let pid = session.pid();
    let fallback = session.worktree_path();
    let cwd = run_blocking(move || {
        let resolved = pid.and_then(process_cwd).or(fallback);
        resolved
            .map(|cwd| TerminalCwdResponse { cwd })
            .ok_or_else(|| IpcError::internal("terminal cwd is unavailable"))
    })
    .await?;
    Ok(cwd)
}

fn process_cwd(pid: u32) -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        return std::fs::read_link(format!("/proc/{pid}/cwd")).ok();
    }

    #[cfg(target_os = "macos")]
    {
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
    terminal_service: State<'_, Arc<TerminalService>>,
    session_id: String,
    data: String,
) -> Result<(), IpcError> {
    terminal_service
        .write_input(&session_id, data.as_bytes())
        .map_err(IpcError::from)
}

#[tauri::command]
pub async fn cmd_terminal_resize(
    terminal_service: State<'_, Arc<TerminalService>>,
    session_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), IpcError> {
    terminal_service
        .resize(&session_id, cols, rows)
        .map_err(IpcError::from)
}

#[tauri::command]
pub async fn cmd_terminal_signal(
    terminal_service: State<'_, Arc<TerminalService>>,
    session_id: String,
    signal: TerminalSignalRequest,
) -> Result<(), IpcError> {
    terminal_service
        .signal(&session_id, signal.into())
        .map_err(IpcError::from)
}

#[tauri::command]
pub async fn cmd_terminal_close(
    terminal_service: State<'_, Arc<TerminalService>>,
    session_id: String,
) -> Result<(), IpcError> {
    terminal_service
        .close_session(&session_id)
        .await
        .map_err(IpcError::from)
}

#[tauri::command]
pub async fn cmd_terminal_list(
    terminal_service: State<'_, Arc<TerminalService>>,
) -> Result<Vec<TerminalSessionSummary>, IpcError> {
    Ok(terminal_service
        .list_sessions()
        .into_iter()
        .filter_map(|session_id| {
            terminal_service
                .get_session(&session_id)
                .map(|session| TerminalSessionSummary {
                    session_id,
                    worktree_path: session.worktree_path(),
                })
        })
        .collect())
}
