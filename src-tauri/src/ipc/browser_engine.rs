//! The Chromium engine's install state as the UI sees it: typed install errors, rollback, and
//! the notice a tab gets when it had to open on the system webview instead.

use crate::browser::engine_install::{self, EngineFallbackNotice, InstallError};
use crate::browser::{BrowserEngine, BrowserManager, BrowserState, CreateBrowserRequest};
use crate::ipc::error::{IpcError, IpcErrorCode};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

/// Emitted with an [`EngineFallbackNotice`] when a tab opens on the system webview because the
/// Chromium engine failed.
pub const BROWSER_ENGINE_FALLBACK_EVENT: &str = "browser_engine_fallback";

/// `BROWSER_ENGINE_INSTALL_FAILED` with the typed reason in `details`, except where an existing
/// code says it better (no build for this platform).
pub fn install_error(error: InstallError) -> IpcError {
    let code = match error {
        InstallError::Unsupported { .. } => IpcErrorCode::UnsupportedCapability,
        _ => IpcErrorCode::BrowserEngineInstallFailed,
    };
    let details = serde_json::to_value(&error).unwrap_or(serde_json::Value::Null);
    IpcError::new(code, error.to_string()).with_details(details)
}

/// Re-opens a tab the engine failed to open as a system-webview tab, and records why for its
/// pane. macOS WebKit cannot hold a named profile, so such a tab keeps the engine's error.
pub fn fall_back_to_system_webview<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &Arc<BrowserManager>,
    request: &CreateBrowserRequest,
    browser_id: &str,
    error: IpcError,
) -> Result<BrowserState, IpcError> {
    if cfg!(target_os = "macos") && request.profile.as_ref().is_some_and(|profile| profile.is_named()) {
        return Err(error);
    }
    tracing::warn!(browser_id, "Chromium engine failed, using the system webview: {}", error.message);
    let state = match manager.get_state(browser_id) {
        Ok(state) => state,
        Err(_) => {
            let state = manager.register_session(CreateBrowserRequest {
                browser_id: Some(browser_id.to_string()),
                ..request.clone()
            })?;
            crate::ipc::browser::record_session_created(&state.browser_id);
            state
        }
    };
    let state = manager.set_engine(&state.browser_id, BrowserEngine::System)?;
    let notice = engine_install::record_fallback(&state.browser_id, &error.message);
    let _ = app.emit(BROWSER_ENGINE_FALLBACK_EVENT, notice);
    Ok(state)
}

/// The fallback notice of a tab, once: the pane asks when it mounts, in case the event fired
/// before it listened.
#[tauri::command]
pub async fn cmd_browser_engine_take_fallback(browser_id: String) -> Result<Option<EngineFallbackNotice>, IpcError> {
    Ok(engine_install::take_fallback(&browser_id))
}

/// Makes the previously installed engine active again. New tabs (and engines started after
/// their profile's last tab closed) use it.
#[tauri::command]
pub async fn cmd_browser_engine_rollback<R: tauri::Runtime>(
    app: AppHandle<R>,
) -> Result<engine_install::EngineStatus, IpcError> {
    let data_dir = crate::ipc::browser::engine_data_dir(&app)?;
    let inactive = crate::ipc::browser::engine_inactive_reason(&app);
    crate::ipc::run_blocking(move || {
        let _guard = engine_install::InstallGuard::acquire().map_err(install_error)?;
        let status =
            engine_install::rollback(&data_dir, crate::ipc::browser::ENGINE_EXECUTABLE).map_err(install_error)?;
        Ok(engine_install::mark_inactive(status, inactive))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_errors_carry_their_reason_in_details() {
        let error = install_error(InstallError::BadSignature);
        assert_eq!(error.code, IpcErrorCode::BrowserEngineInstallFailed);
        assert_eq!(error.details, Some(serde_json::json!({ "reason": "badSignature" })));
        let error = install_error(InstallError::DiskFull { detail: "no space".into() });
        assert_eq!(error.details, Some(serde_json::json!({ "reason": "diskFull", "detail": "no space" })));
        let error = install_error(InstallError::Unsupported { platform: "linux".into() });
        assert_eq!(error.code, IpcErrorCode::UnsupportedCapability);
    }

    #[test]
    fn a_failed_engine_tab_reopens_on_the_system_webview_with_a_notice() {
        let app = tauri::test::mock_app();
        let manager = Arc::new(BrowserManager::new());
        let request = CreateBrowserRequest {
            browser_id: None,
            workspace_id: Some("ws".into()),
            worktree_path: None,
            url: "https://example.com/".into(),
            profile: None,
            zoom_factor: None,
            bounds: None,
            visible: Some(false),
        };
        let failure = IpcError::new(IpcErrorCode::BrowserCreateFailed, "engine did not start");
        let state = fall_back_to_system_webview(app.handle(), &manager, &request, "tab-fallback", failure)
            .expect("fallback");
        assert_eq!(state.browser_id, "tab-fallback");
        assert_eq!(state.engine, BrowserEngine::System);
        assert_eq!(manager.get_state("tab-fallback").map(|state| state.engine), Ok(BrowserEngine::System));
        assert_eq!(
            engine_install::take_fallback("tab-fallback").map(|notice| notice.message),
            Some("engine did not start".into())
        );
    }
}
