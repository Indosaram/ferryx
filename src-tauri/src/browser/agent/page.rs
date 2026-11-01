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
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    {
        crate::browser::cef_engine::is_cef_tab(browser_id)
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
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
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    if is_engine_tab(browser_id) {
        return super::cef_backend::eval(browser_id, &script).await;
    }
    let label = manager.get_webview_label(browser_id)?;
    let webview = app.get_webview(&label).ok_or(BrowserError::WebviewNotFound(label))?;
    crate::ipc::browser::eval_webview(webview, script).await
}

/// Evaluates an agent script (`observe.js`): in an engine tab inside its isolated world, in a
/// webview tab in the page's world (the script builds its own clean realm there).
pub async fn eval_agent<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    browser_id: &str,
    script: String,
) -> Result<String, BrowserError> {
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    if is_engine_tab(browser_id) {
        return super::cef_backend::eval_isolated(browser_id, &script).await;
    }
    eval(app, manager, browser_id, script).await
}

/// Whether agent scripts in this tab run in an isolated world.
pub fn agent_world_isolated(browser_id: &str) -> bool {
    is_engine_tab(browser_id)
}

/// Runs an element action addressed by ref, bound to the fingerprint its observe recorded.
pub async fn act_element<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    browser_id: &str,
    action: &BrowserAutomationAction,
    expect: &str,
) -> Result<(), BrowserError> {
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    if is_engine_tab(browser_id) {
        return super::cef_backend::act_element(browser_id, action, expect).await;
    }
    let script = super::observe::act_script(action, expect, false)
        .ok_or_else(|| BrowserError::AutomationFailed("the action addresses no element".into()))?;
    super::observe::parse_act(&eval(app, manager, browser_id, script).await?)
}

/// Runs the trusted-input form of an action that addresses no element, in an engine tab.
/// `Ok(false)` means the caller runs its DOM script instead (always for webview tabs).
pub async fn trusted_act(browser_id: &str, action: &BrowserAutomationAction) -> Result<bool, BrowserError> {
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    if is_engine_tab(browser_id) {
        return super::cef_backend::act(browser_id, action).await;
    }
    let _ = (browser_id, action);
    Ok(false)
}

/// Captures the tab's visible page: CDP for an engine tab, native webview capture otherwise.
pub async fn capture<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &Arc<BrowserManager>,
    browser_id: &str,
    options: SnapshotOptions,
) -> Result<BrowserSnapshot, IpcError> {
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    if is_engine_tab(browser_id) {
        return Ok(super::cef_backend::capture(browser_id).await?);
    }
    let state = manager.get_state(browser_id)?;
    crate::browser::capture_browser_snapshot(app, &state.webview_label, options).await
}
