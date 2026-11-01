//! One page-access seam for both engines: a Chromium (CEF) engine tab is reached over the
//! engine's in-process DevTools API, a system webview tab through its webview. The observe/act
//! entry points in `ipc::browser` call these instead of picking an engine themselves.

use std::sync::Arc;

use tauri::{AppHandle, Manager};

use crate::browser::model::BrowserAutomationAction;
use crate::browser::snapshot_source::{BrowserSnapshot, SnapshotOptions};
use crate::browser::{BrowserError, BrowserManager};
use crate::ipc::error::IpcError;

/// Whether the tab is served by the Chromium engine.
pub fn is_engine_tab(browser_id: &str) -> bool {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        crate::browser::cef_engine::is_cef_tab(browser_id)
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = browser_id;
        false
    }
}

/// Evaluates a page script in the tab and returns the JSON text of its value.
pub async fn eval<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    browser_id: &str,
    script: String,
) -> Result<String, BrowserError> {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    if is_engine_tab(browser_id) {
        return super::cef_backend::eval(browser_id, &script).await;
    }
    let label = manager.get_webview_label(browser_id)?;
    let webview = app.get_webview(&label).ok_or(BrowserError::WebviewNotFound(label))?;
    crate::ipc::browser::eval_webview(webview, script).await
}

/// Runs the trusted-input form of an action in an engine tab. `Ok(false)` means the caller
/// runs its DOM script instead (always for webview tabs).
pub async fn trusted_act(
    browser_id: &str,
    action: &BrowserAutomationAction,
    selector: Option<&str>,
) -> Result<bool, BrowserError> {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    if is_engine_tab(browser_id) {
        return super::cef_backend::act(browser_id, action, selector).await;
    }
    let _ = (browser_id, action, selector);
    Ok(false)
}

/// Captures the tab's visible page: CDP for an engine tab, native webview capture otherwise.
pub async fn capture<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &Arc<BrowserManager>,
    browser_id: &str,
    options: SnapshotOptions,
) -> Result<BrowserSnapshot, IpcError> {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    if is_engine_tab(browser_id) {
        return Ok(super::cef_backend::capture(browser_id).await?);
    }
    let state = manager.get_state(browser_id)?;
    crate::browser::capture_browser_snapshot(app, &state.webview_label, options).await
}
