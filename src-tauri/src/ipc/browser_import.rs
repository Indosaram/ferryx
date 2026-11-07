//! IPC for importing from other browsers and password-manager exports.
//!
//! Whole-command failures are `BROWSER_IMPORT_FAILED` with `details.reason`:
//! `unknown-source`, `invalid-profile`, `private-profile`, `busy`, `no-data-dir`.
//! Per-type failures are in each result's `error.code` (see `browser::import`).
//!
//! When the records are read, the same blocking task moves the staged bookmarks, history and
//! passwords into Ferryx's stores and deletes the staged files (`browser::import::ingest`), so
//! each result counts what the stores kept.

use crate::browser::import::cookie_sink::CookieSink;
use crate::browser::import::ingest::{self, PasswordVault};
use crate::browser::import::run::{
    detect_sources, import_password_file, ImportRun, ImportSource, PasswordFileFormat, ProfileSink,
};
use crate::browser::import::staging::{pending_dir, StagingWriter};
use crate::browser::import::{ImportDataType, ImportReport, ImportTypeResult};
use crate::browser::library::BrowserLibrary;
use crate::browser::{BrowserManager, BrowserProfileId};
use crate::ipc::error::{IpcError, IpcErrorCode};
use parking_lot::Mutex;
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

/// What holds the one import slot. Both kinds share the pending dir and its ingest.
enum Active {
    /// A browser import, kept so it can be cancelled.
    Run(Arc<ImportRun>),
    PasswordFile,
}

/// The import in progress (one at a time per app).
static ACTIVE: Mutex<Option<Active>> = Mutex::new(None);

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

/// Where the staged records of one import go: the pending dir, the library and the vault.
struct Stores {
    pending: PathBuf,
    library: Arc<BrowserLibrary>,
    vault: crate::ipc::browser_vault::AppVault,
}

impl Stores {
    fn of<R: tauri::Runtime>(app: &AppHandle<R>, profile: &BrowserProfileId) -> Result<Self, IpcError> {
        let data_dir = crate::ipc::browser::engine_data_dir(app)
            .map_err(|error| import_failed("no-data-dir", error.message))?;
        Ok(Self {
            pending: pending_dir(&data_dir, profile.as_str()),
            library: crate::ipc::browser_library::library(app)?,
            vault: crate::ipc::browser_vault::app_vault(app)?,
        })
    }

    /// Blocking: stores and deletes the staged files, then puts the counts into `results`.
    fn ingest(&self, results: &mut Vec<ImportTypeResult>) {
        let ingested = ingest::ingest_pending(&self.pending, &self.library, &self.vault as &dyn PasswordVault);
        ingest::apply(results, ingested);
    }
}

fn library_changed(results: &[ImportTypeResult]) -> bool {
    results.iter().any(|result| {
        matches!(result.data_type, ImportDataType::Bookmarks | ImportDataType::History) && result.imported > 0
    })
}

/// The sink for `profile`: staging under the data dir, cookies into its Chromium cache (when
/// `engine_installed`, checked by the caller off the async thread) and its open system webviews.
fn profile_sink<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &Arc<BrowserManager>,
    profile: &BrowserProfileId,
    source_name: &str,
    engine_installed: bool,
) -> Result<ProfileSink, IpcError> {
    let data_dir = crate::ipc::browser::engine_data_dir(app)
        .map_err(|error| import_failed("no-data-dir", error.message))?;
    let staging = StagingWriter::new(pending_dir(&data_dir, profile.as_str()), source_name);

    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    let (cef_cache_dir, notify): (Option<PathBuf>, crate::browser::import::cookie_sink::EngineNotify) = {
        let dir = engine_installed.then(|| crate::browser::data_paths::cef_profile_dir(&data_dir, profile.as_str()));
        let name = profile.as_str().to_string();
        (
            dir,
            Box::new(move |path: &std::path::Path| crate::browser::cef_engine::import_cookies(&name, path)),
        )
    };
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    let (cef_cache_dir, notify): (Option<PathBuf>, crate::browser::import::cookie_sink::EngineNotify) = {
        let _ = engine_installed;
        (None, Box::new(|_: &std::path::Path| false))
    };

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

impl ActiveGuard {
    /// Takes the import slot, or the typed `busy` error when another import holds it.
    fn claim(active: Active) -> Result<Self, IpcError> {
        let mut slot = ACTIVE.lock();
        if slot.is_some() {
            return Err(import_failed("busy", "another import is running"));
        }
        *slot = Some(active);
        Ok(Self)
    }
}

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
    let blocking_app = app.clone();
    let (browser, engine_installed) = crate::ipc::run_blocking(move || {
        let browser = detect_sources()
            .into_iter()
            .find(|(source, _)| source.id == source_id)
            .map(|(_, browser)| browser)
            .ok_or_else(|| import_failed("unknown-source", format!("no importable browser {source_id}")))?;
        #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
        let engine_installed = crate::ipc::browser::engine_data_dir(&blocking_app)
            .is_ok_and(|data_dir| crate::browser::cef_engine::engine_dir(&data_dir).is_some());
        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
        let engine_installed = {
            let _ = blocking_app;
            false
        };
        Ok((browser, engine_installed))
    })
    .await?;

    let sink = profile_sink(&app, &manager, &profile, &browser.display_name, engine_installed)?;
    let stores = Stores::of(&app, &profile)?;
    let password_file = request.password_csv_path.map(PathBuf::from);
    let has_password_file = password_file.is_some();
    let run = Arc::new(ImportRun::new(Arc::new(sink), password_file));
    let guard = ActiveGuard::claim(Active::Run(Arc::clone(&run)))?;
    let types = request.types;
    let report = crate::ipc::run_blocking(move || {
        let _guard = guard;
        let mut report = run.run(&browser, &types, has_password_file);
        stores.ingest(&mut report.results);
        Ok(report)
    })
    .await?;
    if library_changed(&report.results) {
        crate::ipc::browser_library::notify_imported(&app);
    }
    Ok(report)
}

#[tauri::command]
pub async fn cmd_browser_import_cancel() -> Result<bool, IpcError> {
    Ok(match ACTIVE.lock().as_ref() {
        Some(Active::Run(run)) => {
            run.cancel();
            true
        }
        Some(Active::PasswordFile) | None => false,
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
    // A password export has no cookies, so the Chromium engine is never needed.
    let sink = profile_sink(&app, manager.inner(), &profile, &source_name, false)?;
    let stores = Stores::of(&app, &profile)?;
    let guard = ActiveGuard::claim(Active::PasswordFile)?;
    let mut results = crate::ipc::run_blocking(move || {
        let _guard = guard;
        let mut results = vec![import_password_file(&sink, request.format, &file)];
        stores.ingest(&mut results);
        Ok(results)
    })
    .await?;
    if library_changed(&results) {
        crate::ipc::browser_library::notify_imported(&app);
    }
    // `results[0]` is the passwords result; others are files an interrupted import left behind.
    Ok(results.swap_remove(0))
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

    #[test]
    fn a_second_import_is_busy_until_the_first_ends() {
        let first = ActiveGuard::claim(Active::PasswordFile).unwrap();
        let Err(busy) = ActiveGuard::claim(Active::PasswordFile) else {
            panic!("a second import must be refused while the first runs");
        };
        assert_eq!(busy.code, IpcErrorCode::BrowserImportFailed);
        assert_eq!(busy.details.unwrap()["reason"], "busy");
        drop(first);
        drop(ActiveGuard::claim(Active::PasswordFile).unwrap());
    }
}
