use crate::ipc::{run_blocking, IpcError};
use crate::terminal::{PtyManager, PtySessionState, TerminalSignal};
use crate::worktree::{WorkspaceRegistry, WorktreeIdentity};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use portable_pty::CommandBuilder;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Runtime, State};

pub const TERMINAL_OUTPUT_EVENT: &str = "terminal_output";
pub const TERMINAL_LIFECYCLE_EVENT: &str = "terminal_lifecycle";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpawnTerminalRequest {
    pub workspace_id: String,
    pub worktree: Option<WorktreeIdentity>,
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
    pty_manager: State<'_, Arc<PtyManager>>,
    registry: State<'_, WorkspaceRegistry>,
    request: SpawnTerminalRequest,
) -> Result<SpawnTerminalResponse, IpcError> {
    let cols = request.cols.unwrap_or(80);
    let rows = request.rows.unwrap_or(24);
    let registry = (*registry).clone();
    let workspace_id = request.workspace_id.clone();
    let identity = request.worktree.clone();

    let (worktree_manager, cwd) = run_blocking(move || {
        registry
            .resolve_terminal_target(&workspace_id, identity.as_ref())
            .map_err(IpcError::from)
    })
    .await?;

    let mut cmd = CommandBuilder::new_default_prog();
    cmd.cwd(&cwd);
    let (session_id, mut rx) = pty_manager
        .spawn_in_worktree(cmd, cols, rows, &worktree_manager, &cwd)
        .map_err(IpcError::from)?;
    let session = pty_manager
        .get_session(&session_id)
        .ok_or_else(|| IpcError::internal(format!("PTY session '{session_id}' disappeared during spawn")))?;

    let started = TerminalLifecyclePayload {
        session_id: session_id.clone(),
        state: TerminalLifecycleState::Started,
        exit_code: None,
        reason: None,
    };
    if let Err(error) = app.emit(TERMINAL_LIFECYCLE_EVENT, started) {
        let _ = pty_manager.close_session(&session_id).await;
        return Err(IpcError::internal(format!(
            "failed to emit terminal lifecycle event: {error}"
        )));
    }

    let session_id_clone = session_id.clone();
    let app_handle = app.clone();
    tokio::spawn(async move {
        while let Some(chunk) = rx.recv().await {
            let payload = TerminalOutputPayload {
                session_id: session_id_clone.clone(),
                data: STANDARD.encode(chunk),
            };
            if let Err(error) = app_handle.emit(TERMINAL_OUTPUT_EVENT, payload) {
                tracing::debug!("Failed to emit terminal output event: {error}");
                break;
            }
        }
        drop(rx);

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
pub async fn cmd_terminal_write(
    pty_manager: State<'_, Arc<PtyManager>>,
    session_id: String,
    data: String,
) -> Result<(), IpcError> {
    pty_manager
        .write_input(&session_id, data.as_bytes())
        .map_err(IpcError::from)
}

#[tauri::command]
pub async fn cmd_terminal_resize(
    pty_manager: State<'_, Arc<PtyManager>>,
    session_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), IpcError> {
    pty_manager
        .resize(&session_id, cols, rows)
        .map_err(IpcError::from)
}

#[tauri::command]
pub async fn cmd_terminal_signal(
    pty_manager: State<'_, Arc<PtyManager>>,
    session_id: String,
    signal: TerminalSignalRequest,
) -> Result<(), IpcError> {
    pty_manager
        .signal(&session_id, signal.into())
        .map_err(IpcError::from)
}

#[tauri::command]
pub async fn cmd_terminal_close(
    pty_manager: State<'_, Arc<PtyManager>>,
    session_id: String,
) -> Result<(), IpcError> {
    pty_manager
        .close_session(&session_id)
        .await
        .map_err(IpcError::from)
}

#[tauri::command]
pub async fn cmd_terminal_list(
    pty_manager: State<'_, Arc<PtyManager>>,
) -> Result<Vec<TerminalSessionSummary>, IpcError> {
    Ok(pty_manager
        .list_sessions()
        .into_iter()
        .filter_map(|session_id| {
            pty_manager
                .get_session(&session_id)
                .map(|session| TerminalSessionSummary {
                    session_id,
                    worktree_path: session.writer_worktree(),
                })
        })
        .collect())
}
