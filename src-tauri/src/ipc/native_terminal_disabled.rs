use serde::{Deserialize, Serialize};

use crate::ipc::IpcError;

pub const NATIVE_TERMINAL_PRESENTATION_RECEIPT_CAPABILITY: &str = "unsupported";

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
pub async fn cmd_native_terminal_close() -> Result<(), IpcError> {
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
pub async fn cmd_native_terminal_set_scrollbar_overlay() -> Result<(), IpcError> {
    Err(IpcError::native_terminal_unsupported())
}

#[tauri::command]
pub async fn cmd_native_terminal_set_preedit() -> Result<(), IpcError> {
    Err(IpcError::native_terminal_unsupported())
}

#[tauri::command]
pub async fn cmd_native_terminal_set_attention_frame() -> Result<(), IpcError> {
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

/// Without the native terminal there is no native clipboard writer; the frontend falls back to
/// the WebView clipboard API on this error.
#[tauri::command]
pub async fn cmd_clipboard_write_text() -> Result<(), IpcError> {
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
pub async fn cmd_native_terminal_line_at() -> Result<(), IpcError> {
    Err(IpcError::native_terminal_unsupported())
}

#[tauri::command]
pub async fn cmd_native_terminal_hyperlink_at() -> Result<Option<String>, IpcError> {
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
    async fn pane_liveness_native_binding_disabled_render_never_reports_success() {
        let error = cmd_native_terminal_set_bounds().await.expect_err("unsupported presentation");
        assert_eq!(error.code, IpcErrorCode::NativeTerminalUnsupported);
        assert!(cmd_native_terminal_attach().await.is_err());
    }

    #[tokio::test]
    async fn disabled_native_terminal_commands_return_typed_unsupported_error() {
        let results = vec![
            cmd_native_terminal_attach().await,
            cmd_native_terminal_detach().await,
            cmd_native_terminal_close().await,
            cmd_native_terminal_set_bounds().await,
            cmd_native_terminal_set_focus().await,
            cmd_native_terminal_send_input().await,
            cmd_native_terminal_scroll().await,
            cmd_native_terminal_scrollbar().await,
            cmd_native_terminal_set_scrollbar_overlay().await,
            cmd_native_terminal_set_attention_frame().await,
            cmd_native_terminal_select().await,
            cmd_native_terminal_copy_selection().await,
            cmd_native_terminal_paste().await,
            cmd_native_terminal_mouse().await,
            cmd_native_terminal_search().await,
            cmd_native_terminal_hyperlink_at().await.map(|_| ()),
            cmd_native_terminal_clipboard_content().await.map(|_| ()),
        ];

        for result in results {
            let error = result.expect_err("disabled command must not succeed");
            assert_eq!(error.code, IpcErrorCode::NativeTerminalUnsupported);
            assert!(error.message.contains("native-terminal"));
        }
    }
}
