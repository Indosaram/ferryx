use serde::{Deserialize, Serialize};

use crate::ipc::IpcError;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum NativeTerminalClipboardContent {
    Text { text: String },
    Image,
    Empty,
}

#[tauri::command]
pub async fn cmd_native_terminal_attach() -> Result<(), IpcError> {
    Err(IpcError::native_terminal_unsupported())
}

#[tauri::command]
pub async fn cmd_native_terminal_detach() -> Result<(), IpcError> {
    Err(IpcError::native_terminal_unsupported())
}

#[tauri::command]
pub async fn cmd_native_terminal_set_bounds() -> Result<(), IpcError> {
    Err(IpcError::native_terminal_unsupported())
}

#[tauri::command]
pub async fn cmd_native_terminal_set_focus() -> Result<(), IpcError> {
    Err(IpcError::native_terminal_unsupported())
}

#[tauri::command]
pub async fn cmd_native_terminal_send_input() -> Result<(), IpcError> {
    Err(IpcError::native_terminal_unsupported())
}

#[tauri::command]
pub async fn cmd_native_terminal_scroll() -> Result<(), IpcError> {
    Err(IpcError::native_terminal_unsupported())
}

#[tauri::command]
pub async fn cmd_native_terminal_scrollbar() -> Result<(), IpcError> {
    Err(IpcError::native_terminal_unsupported())
}

#[tauri::command]
pub async fn cmd_native_terminal_select() -> Result<(), IpcError> {
    Err(IpcError::native_terminal_unsupported())
}

#[tauri::command]
pub async fn cmd_native_terminal_copy_selection() -> Result<(), IpcError> {
    Err(IpcError::native_terminal_unsupported())
}

#[tauri::command]
pub async fn cmd_native_terminal_paste() -> Result<(), IpcError> {
    Err(IpcError::native_terminal_unsupported())
}

#[tauri::command]
pub async fn cmd_native_terminal_mouse() -> Result<(), IpcError> {
    Err(IpcError::native_terminal_unsupported())
}

#[tauri::command]
pub async fn cmd_native_terminal_search() -> Result<(), IpcError> {
    Err(IpcError::native_terminal_unsupported())
}

#[tauri::command]
pub async fn cmd_native_terminal_clipboard_content(
) -> Result<NativeTerminalClipboardContent, IpcError> {
    Err(IpcError::native_terminal_unsupported())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::IpcErrorCode;

    #[tokio::test]
    async fn disabled_native_terminal_commands_return_typed_unsupported_error() {
        let results = vec![
            cmd_native_terminal_attach().await,
            cmd_native_terminal_detach().await,
            cmd_native_terminal_set_bounds().await,
            cmd_native_terminal_set_focus().await,
            cmd_native_terminal_send_input().await,
            cmd_native_terminal_scroll().await,
            cmd_native_terminal_scrollbar().await,
            cmd_native_terminal_select().await,
            cmd_native_terminal_copy_selection().await,
            cmd_native_terminal_paste().await,
            cmd_native_terminal_mouse().await,
            cmd_native_terminal_search().await,
            cmd_native_terminal_clipboard_content().await.map(|_| ()),
        ];

        for result in results {
            let error = result.expect_err("disabled command must not succeed");
            assert_eq!(error.code, IpcErrorCode::NativeTerminalUnsupported);
            assert!(error.message.contains("native-terminal"));
        }
    }
}
