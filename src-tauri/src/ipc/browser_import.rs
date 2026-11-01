//! IPC for importing from other browsers and password-manager exports.
//!
//! Whole-command failures are `BROWSER_IMPORT_FAILED` with `details.reason`:
//! `unknown-source`, `invalid-profile`, `private-profile`, `busy`, `no-data-dir`.
//! Per-type failures are in each result's `error.code` (see `browser::import`).

use crate::browser::import::cookie_sink::CookieSink;
use crate::browser::import::run::{
    detect_sources, import_password_file, ImportRun, ImportSource, PasswordFileFormat, ProfileSink,
};
use crate::browser::import::staging::{pending_dir, StagingWriter};
use crate::browser::import::{ImportDataType, ImportReport, ImportTypeResult};
use crate::browser::{BrowserManager, BrowserProfileId};
use crate::ipc::error::{IpcError, IpcErrorCode};
use parking_lot::Mutex;
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

/// The import in progress (one at a time per app), so it can be cancelled.
static ACTIVE: Mutex<Option<Arc<ImportRun>>> = Mutex::new(None);

fn import_failed(reason: &str, message: impl Into<String>) -> IpcError {
    IpcError::new(IpcErrorCode::BrowserImportFailed, message)
        .with_details(serde_json::json!({ "reason": reason }))
}

fn target_profile(profile_id: &str) -> Result<BrowserProfileId, IpcError> {
    let profile = BrowserProfileId::from_id(profile_id)
        .ok_or_else(|| import_failed("invalid-profile", format!("unknown profile: {profile_id}")))?;
    if profile.is_private() {
        return Err(import_failed(
            "private-profile",
            "private browsing keeps nothing; choose another profile",
        ));
    }
    Ok(profile)
}

/// The sink for `profile`: staging under the data dir, cookies into its Chromium cache (when
/// the engine is installed) and its open system webviews.
fn profile_sink<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &Arc<BrowserManager>,
    profile: &BrowserProfileId,
    source_name: &str,
) -> Result<ProfileSink, IpcError> {
    let data_dir = crate::ipc::browser::engine_data_dir(app)
        .map_err(|error| import_failed("no-data-dir", error.message))?;
    let staging = StagingWriter::new(pending_dir(&data_dir, profile.as_str()), source_name);

    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    let (cef_cache_dir, notify): (Option<PathBuf>, crate::browser::import::cookie_sink::EngineNotify) = {
        let installed = crate::browser::cef_engine::engine_dir(&data_dir).is_some();
        let dir = installed.then(|| crate::browser::data_paths::cef_profile_dir(&data_dir, profile.as_str()));
        let name = profile.as_str().to_string();
        (
            dir,
            Box::new(move |path: &std::path::Path| crate::browser::cef_engine::import_cookies(&name, path)),
        )
    };
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    let (cef_cache_dir, notify): (Option<PathBuf>, crate::browser::import::cookie_sink::EngineNotify) =
        (None, Box::new(|_: &std::path::Path| false));

    let labels = manager.webview_labels_for_profile(profile);
    let app = app.clone();
    let apply = Box::new(move |cookies: &[cookie::Cookie<'static>]| -> Result<usize, String> {
        let mut applied = 0;
        for webview in labels.iter().filter_map(|label| app.get_webview(label)) {
            for cookie in cookies {
                webview.set_cookie(cookie.clone()).map_err(|error| error.to_string())?;
            }
            applied += 1;
        }
        Ok(applied)
    });
    Ok(ProfileSink::new(staging, CookieSink::new(cef_cache_dir, notify, apply)))
}

#[tauri::command]
pub async fn cmd_browser_import_sources() -> Result<Vec<ImportSource>, IpcError> {
    crate::ipc::run_blocking(|| Ok(detect_sources().into_iter().map(|(source, _)| source).collect()))
        .await
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserImportRunRequest {
    pub source_id: String,
    pub profile_id: String,
    pub types: Vec<ImportDataType>,
    /// Safari: the passwords CSV the user exported.
    #[serde(default)]
    pub password_csv_path: Option<String>,
}

/// Clears [`ACTIVE`] when the import ends, however it ends.
struct ActiveGuard;

impl Drop for ActiveGuard {
    fn drop(&mut self) {
        *ACTIVE.lock() = None;
    }
}

#[tauri::command]
pub async fn cmd_browser_import_run<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    request: BrowserImportRunRequest,
) -> Result<ImportReport, IpcError> {
    let profile = target_profile(&request.profile_id)?;
    let manager = Arc::clone(manager.inner());
    let source_id = request.source_id.clone();
    let browser = crate::ipc::run_blocking(move || {
        detect_sources()
            .into_iter()
            .find(|(source, _)| source.id == source_id)
            .map(|(_, browser)| browser)
            .ok_or_else(|| import_failed("unknown-source", format!("no importable browser {source_id}")))
    })
    .await?;

    let sink = profile_sink(&app, &manager, &profile, &browser.display_name)?;
    let password_file = request.password_csv_path.map(PathBuf::from);
    let has_password_file = password_file.is_some();
    let run = Arc::new(ImportRun::new(Arc::new(sink), password_file));
    {
        let mut active = ACTIVE.lock();
        if active.is_some() {
            return Err(import_failed("busy", "another import is running"));
        }
        *active = Some(Arc::clone(&run));
    }
    let types = request.types;
    crate::ipc::run_blocking(move || {
        let _guard = ActiveGuard;
        Ok(run.run(&browser, &types, has_password_file))
    })
    .await
}

#[tauri::command]
pub async fn cmd_browser_import_cancel() -> Result<bool, IpcError> {
    Ok(match ACTIVE.lock().as_ref() {
        Some(run) => {
            run.cancel();
            true
        }
        None => false,
    })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserImportPasswordFileRequest {
    pub format: PasswordFileFormat,
    pub path: String,
    pub profile_id: String,
}

#[tauri::command]
pub async fn cmd_browser_import_password_file<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    request: BrowserImportPasswordFileRequest,
) -> Result<ImportTypeResult, IpcError> {
    let profile = target_profile(&request.profile_id)?;
    let file = PathBuf::from(&request.path);
    let source_name = file
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "password export".to_string());
    let sink = profile_sink(&app, manager.inner(), &profile, &source_name)?;
    crate::ipc::run_blocking(move || Ok(import_password_file(&sink, request.format, &file))).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_and_malformed_profiles_are_refused_with_a_reason() {
        let reason = |result: Result<BrowserProfileId, IpcError>| {
            let error = result.unwrap_err();
            assert_eq!(error.code, IpcErrorCode::BrowserImportFailed);
            error.details.unwrap()["reason"].as_str().unwrap().to_string()
        };
        assert_eq!(reason(target_profile("private")), "private-profile");
        assert_eq!(reason(target_profile("../etc")), "invalid-profile");
        assert_eq!(target_profile("work").unwrap(), BrowserProfileId::Named("work".into()));
    }

    #[test]
    fn the_error_code_round_trips_through_its_wire_string() {
        let wire = serde_json::to_value(IpcErrorCode::BrowserImportFailed).unwrap();
        assert_eq!(wire, "BROWSER_IMPORT_FAILED");
        assert_eq!(IpcErrorCode::from_code_str("BROWSER_IMPORT_FAILED"), IpcErrorCode::BrowserImportFailed);
    }

    #[test]
    fn requests_use_camel_case_fields() {
        let request: BrowserImportRunRequest = serde_json::from_value(serde_json::json!({
            "sourceId": "chrome:Default",
            "profileId": "default",
            "types": ["bookmarks", "cookies"],
        }))
        .unwrap();
        assert_eq!(request.types, vec![ImportDataType::Bookmarks, ImportDataType::Cookies]);
        assert_eq!(request.password_csv_path, None);
        let file: BrowserImportPasswordFileRequest = serde_json::from_value(serde_json::json!({
            "format": "bitwardenJson", "path": "/tmp/x.json", "profileId": "default",
        }))
        .unwrap();
        assert_eq!(file.format, PasswordFileFormat::BitwardenJson);
    }
}
