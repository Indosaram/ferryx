use crate::terminal::{PtyManager, PtySessionState, TerminalSignal};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use portable_pty::CommandBuilder;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Runtime, State};

#[derive(Debug, Serialize, Deserialize)]
pub struct SpawnTerminalRequest {
    pub cwd: Option<PathBuf>,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
    pub command: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalOutputPayload {
    pub session_id: String,
    pub data: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
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
    request: Option<SpawnTerminalRequest>,
) -> Result<String, String> {
    let req = request.unwrap_or(SpawnTerminalRequest {
        cwd: None,
        cols: None,
        rows: None,
        command: None,
    });

    let cols = req.cols.unwrap_or(80);
    let rows = req.rows.unwrap_or(24);

    let mut cmd = if let Some(cmd_str) = req.command {
        let mut builder = CommandBuilder::new("/bin/sh");
        builder.arg("-c");
        builder.arg(cmd_str);
        builder
    } else {
        CommandBuilder::new_default_prog()
    };

    if let Some(cwd) = req.cwd {
        cmd.cwd(cwd);
    }

    let (session_id, mut rx) = pty_manager
        .spawn(cmd, cols, rows)
        .map_err(|e| e.to_string())?;
    let session = pty_manager
        .get_session(&session_id)
        .ok_or_else(|| format!("PTY session '{session_id}' disappeared during spawn"))?;

    let started = TerminalLifecyclePayload {
        session_id: session_id.clone(),
        state: TerminalLifecycleState::Started,
        exit_code: None,
        reason: None,
    };
    app.emit("terminal_lifecycle", started)
        .map_err(|e| format!("Failed to emit terminal lifecycle event: {e}"))?;

    let session_id_clone = session_id.clone();
    let app_handle = app.clone();

    tokio::spawn(async move {
        while let Some(chunk) = rx.recv().await {
            let payload = TerminalOutputPayload {
                session_id: session_id_clone.clone(),
                data: STANDARD.encode(chunk),
            };
            if let Err(error) = app_handle.emit("terminal_output", payload) {
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
                if let Err(error) = app_handle.emit("terminal_lifecycle", payload) {
                    tracing::debug!("Failed to emit terminal lifecycle event: {error}");
                }
                break;
            }

            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    });

    Ok(session_id)
}

#[tauri::command]
pub async fn cmd_terminal_write(
    pty_manager: State<'_, Arc<PtyManager>>,
    session_id: String,
    data: String,
) -> Result<(), String> {
    pty_manager
        .write_input(&session_id, data.as_bytes())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cmd_terminal_resize(
    pty_manager: State<'_, Arc<PtyManager>>,
    session_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    pty_manager
        .resize(&session_id, cols, rows)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cmd_terminal_signal(
    pty_manager: State<'_, Arc<PtyManager>>,
    session_id: String,
    signal: TerminalSignalRequest,
) -> Result<(), String> {
    pty_manager
        .signal(&session_id, signal.into())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cmd_terminal_close(
    pty_manager: State<'_, Arc<PtyManager>>,
    session_id: String,
) -> Result<(), String> {
    pty_manager
        .close_session(&session_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cmd_terminal_list(
    pty_manager: State<'_, Arc<PtyManager>>,
) -> Result<Vec<String>, String> {
    Ok(pty_manager.list_sessions())
}
