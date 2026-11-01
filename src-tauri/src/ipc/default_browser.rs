//! IPC for the default-browser registration, URLs the OS asked Ferryx to open, and the
//! Google sign-in handoff.

use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::browser::default_browser::{
    self, DefaultBrowserStatus, MakeDefaultOutcome, INBOUND_URL_EVENT,
};
use crate::browser::sign_in_handoff::{detect_sign_in_block, SignInBlock};
use crate::ipc::error::IpcError;

/// Settings > Browser "Default browser" row status.
#[tauri::command]
pub async fn cmd_default_browser_status() -> Result<DefaultBrowserStatus, IpcError> {
    crate::ipc::run_blocking(|| Ok(default_browser::status())).await
}

/// Registers Ferryx with the OS and asks it to become the default browser.
#[tauri::command]
pub async fn cmd_default_browser_make_default() -> Result<MakeDefaultOutcome, IpcError> {
    crate::ipc::run_blocking(|| default_browser::make_default().map_err(IpcError::internal)).await
}

/// Drains the inbound URLs queued since the last call. The UI calls it on start and on every
/// `ferryx:inbound-url` event.
#[tauri::command]
pub async fn cmd_default_browser_take_inbound_urls() -> Result<Vec<String>, IpcError> {
    Ok(default_browser::take_inbound_urls())
}

/// Classifies a page as Google's embedded-webview sign-in block.
#[tauri::command]
pub async fn cmd_browser_detect_sign_in_block(
    url: String,
    title: Option<String>,
) -> Result<SignInBlock, IpcError> {
    Ok(detect_sign_in_block(&url, title.as_deref()))
}

/// Queues `url` for the UI and brings the main window forward. Non-http(s) URLs are dropped.
pub fn deliver_inbound_url<R: Runtime>(app: &AppHandle<R>, url: &str) -> bool {
    let Some(url) = default_browser::classify_inbound_url(url) else {
        tracing::warn!("ignored an inbound URL that is not http(s)");
        return false;
    };
    default_browser::queue_inbound_url(url);
    if let Err(error) = app.emit(INBOUND_URL_EVENT, ()) {
        tracing::warn!(%error, "could not notify the UI of an inbound URL; it stays queued");
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
    true
}
