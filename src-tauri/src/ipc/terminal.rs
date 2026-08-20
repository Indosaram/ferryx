use crate::terminal::PtyManager;
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
pub struct TerminalOutputPayload {
    pub session_id: String,
    pub data: String,
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

    let session_id_clone = session_id.clone();
    let app_handle = app.clone();

    tokio::spawn(async move {
        let event_name = format!("terminal_output:{}", session_id_clone);
        while let Some(chunk) = rx.recv().await {
            let encoded = String::from_utf8_lossy(&chunk).to_string();
            let payload = TerminalOutputPayload {
                session_id: session_id_clone.clone(),
                data: encoded,
            };
            if let Err(e) = app_handle.emit(&event_name, payload) {
                tracing::debug!("Failed to emit terminal output event: {}", e);
                break;
            }
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
pub async fn cmd_terminal_close(
    pty_manager: State<'_, Arc<PtyManager>>,
    session_id: String,
) -> Result<(), String> {
    let _ = pty_manager.kill(&session_id);
    pty_manager.remove_session(&session_id);
    Ok(())
}

#[tauri::command]
pub async fn cmd_terminal_list(
    pty_manager: State<'_, Arc<PtyManager>>,
) -> Result<Vec<String>, String> {
    Ok(pty_manager.list_sessions())
}
