use crate::ipc::{run_blocking, IpcError};
use crate::terminal::{load_terminal_preferences, TerminalPreferences};

#[tauri::command]
pub async fn cmd_terminal_preferences() -> Result<TerminalPreferences, IpcError> {
    run_blocking(|| Ok(load_terminal_preferences())).await
}
