//! Downloads, print/PDF, find, per-site zoom, file chooser and page notifications for browser
//! tabs on both engines: the Chromium engine (over its line protocol) and the system webview
//! (WKWebView, WebView2, WebKitGTK through wry).
//!
//! New commands live here so `ipc/browser.rs` only gains small hooks.

use crate::browser::download::{registry, DownloadFailureKind};
use crate::browser::site_permissions;
use crate::browser::system::{
    accept_extensions, page_notification_action, page_permission_state, parse_page_notification, step_zoom,
    unique_download_path, PageNotificationAction, PageNotificationMessage, SiteZoomStore, ZoomDirection,
    SITE_ZOOM_FILE,
};
use crate::browser::{BrowserError, BrowserFindResult, BrowserManager};
use crate::ipc::error::{IpcError, IpcErrorCode};
use crate::ipc::run_blocking;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, LazyLock};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tauri_plugin_dialog::DialogExt;

/// Find-in-page results a Chromium tab reports asynchronously; the UI's find bar listens.
pub const BROWSER_FIND_RESULT_EVENT: &str = "browser_find_result";
/// Print-to-PDF finished (Chromium tabs).
pub const BROWSER_PDF_SAVED_EVENT: &str = "browser_pdf_saved";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserFindResultPayload {
    pub browser_id: String,
    pub match_count: u32,
    pub active_match: u32,
    pub final_update: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserPdfSavedPayload {
    pub browser_id: String,
    pub ok: bool,
    pub path: String,
}

/// How downloads the page starts are saved. Mirrors the UI setting
/// `ferryx.browser.settings.askWhereToSave`; the UI pushes it with
/// `cmd_browser_set_download_preferences`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserDownloadPreferences {
    #[serde(default)]
    pub ask_where_to_save: bool,
    /// `None` uses the OS download folder.
    #[serde(default)]
    pub directory: Option<String>,
}

static DOWNLOAD_PREFERENCES: LazyLock<Mutex<BrowserDownloadPreferences>> = LazyLock::new(Default::default);
static SITE_ZOOM: LazyLock<Mutex<Option<SiteZoomStore>>> = LazyLock::new(Default::default);

fn io_error(error: impl std::fmt::Display) -> IpcError {
    IpcError::new(IpcErrorCode::IoError, error.to_string())
}

fn data_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, IpcError> {
    if let Some(dir) = std::env::var_os("FERRYX_DATA_DIR").filter(|dir| !dir.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    app.path().app_data_dir().map_err(io_error)
}

fn download_dir<R: Runtime>(app: &AppHandle<R>) -> PathBuf {
    DOWNLOAD_PREFERENCES
        .lock()
        .directory
        .clone()
        .map(PathBuf::from)
        .or_else(|| app.path().download_dir().ok())
        .or_else(|| app.path().home_dir().ok())
        .unwrap_or_else(std::env::temp_dir)
}

/// Picks where a page-started download goes: the save dialog when the user asked for it,
/// else a free name in the download folder. `None` cancels. Blocking (dialog, disk).
fn choose_download_path<R: Runtime>(app: &AppHandle<R>, suggested: &str) -> Option<PathBuf> {
    let dir = download_dir(app);
    if DOWNLOAD_PREFERENCES.lock().ask_where_to_save {
        let name = crate::browser::system::safe_file_name(suggested);
        return app
            .dialog()
            .file()
            .set_directory(&dir)
            .set_file_name(name)
            .blocking_save_file()
            .and_then(|path| path.into_path().ok());
    }
    if let Err(error) = std::fs::create_dir_all(&dir) {
        tracing::warn!(%error, dir = %dir.display(), "could not create the download folder");
    }
    Some(unique_download_path(&dir, suggested, |path| path.exists()))
}

// ---------------------------------------------------------------------------------------------
// Per-site zoom
// ---------------------------------------------------------------------------------------------

fn with_site_zoom<T>(app: &AppHandle<impl Runtime>, f: impl FnOnce(&mut SiteZoomStore) -> T) -> Result<T, IpcError> {
    let path = data_dir(app)?.join(SITE_ZOOM_FILE);
    let mut guard = SITE_ZOOM.lock();
    let store = guard.get_or_insert_with(|| SiteZoomStore::load(&path));
    Ok(f(store))
}

fn save_site_zoom<R: Runtime>(app: &AppHandle<R>) -> Result<(), IpcError> {
    let path = data_dir(app)?.join(SITE_ZOOM_FILE);
    let snapshot = SITE_ZOOM.lock().clone();
    if let Some(store) = snapshot {
        store.save(&path).map_err(io_error)?;
    }
    Ok(())
}

/// Applies a zoom factor to the tab's engine.
async fn apply_zoom<R: Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    browser_id: &str,
    zoom: f64,
) -> Result<f64, IpcError> {
    let clamped = manager.set_zoom(browser_id, zoom)?;
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    if crate::browser::cef_engine::is_cef_tab(browser_id) {
        crate::browser::cef_engine::system::set_zoom(browser_id, clamped)?;
        return Ok(clamped);
    }
    let state = manager.get_state(browser_id)?;
    let webview = app
        .get_webview(&state.webview_label)
        .ok_or_else(|| BrowserError::WebviewNotFound(state.webview_label.clone()))?;
    webview
        .set_zoom(clamped)
        .map_err(|error| BrowserError::Internal(format!("failed to set browser webview zoom: {error}")))?;
    Ok(clamped)
}

/// Cmd/Ctrl +, -, 0 on a tab: steps the zoom, remembers it for the page's site, applies it.
#[tauri::command]
pub async fn cmd_browser_zoom_step<R: Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
    direction: ZoomDirection,
) -> Result<f64, IpcError> {
    let state = manager.get_state(&browser_id)?;
    let next = step_zoom(state.zoom_factor, direction);
    let applied = apply_zoom(&app, &manager, &browser_id, next).await?;
    // A private tab's zoom is not remembered, like its history.
    if !state.profile_id.is_private() {
        let url = state.url.clone();
        let store_app = app.clone();
        run_blocking(move || {
            with_site_zoom(&store_app, |store| store.set(&url, applied))?;
            save_site_zoom(&store_app)
        })
        .await?;
    }
    Ok(applied)
}

/// Applies the remembered zoom of the page's site after a navigation; returns it when one
/// was applied. Called by the UI on address changes and by both engines' load hooks.
#[tauri::command]
pub async fn cmd_browser_apply_site_zoom<R: Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
) -> Result<Option<f64>, IpcError> {
    apply_site_zoom(&app, &manager, &browser_id).await
}

pub async fn apply_site_zoom<R: Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    browser_id: &str,
) -> Result<Option<f64>, IpcError> {
    let state = manager.get_state(browser_id)?;
    if state.profile_id.is_private() {
        return Ok(None);
    }
    let url = state.url.clone();
    let store_app = app.clone();
    let remembered = run_blocking(move || with_site_zoom(&store_app, |store| store.get(&url))).await?;
    let target = remembered.unwrap_or(1.0);
    if (target - state.zoom_factor).abs() < 1e-6 {
        return Ok(remembered);
    }
    apply_zoom(app, manager, browser_id, target).await.map(Some)
}

// ---------------------------------------------------------------------------------------------
// Find (Chromium tabs; the system webview keeps the script-based find in ipc/browser.rs)
// ---------------------------------------------------------------------------------------------

/// Starts or continues a native find in a Chromium tab. The count arrives as
/// `browser_find_result` events; the returned value only says the search was sent.
#[cfg(any(target_os = "windows", target_os = "macos"))]
pub fn cef_find(browser_id: &str, query: &str, backwards: bool) -> Result<BrowserFindResult, IpcError> {
    use crate::browser::cef_engine::system;
    if query.trim().is_empty() {
        system::stop_find(browser_id)?;
        return Ok(BrowserFindResult { match_count: 0, found: false });
    }
    let previous = CEF_FIND_QUERIES.lock().insert(browser_id.to_string(), query.to_string());
    let find_next = previous.as_deref() == Some(query);
    system::find(browser_id, query, !backwards, find_next)?;
    let last = CEF_FIND_COUNTS.lock().get(browser_id).copied().filter(|_| find_next).unwrap_or(0);
    Ok(BrowserFindResult { match_count: last as usize, found: last > 0 })
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
pub fn cef_clear_find(browser_id: &str) -> Result<(), IpcError> {
    CEF_FIND_QUERIES.lock().remove(browser_id);
    CEF_FIND_COUNTS.lock().remove(browser_id);
    Ok(crate::browser::cef_engine::system::stop_find(browser_id)?)
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
static CEF_FIND_QUERIES: LazyLock<Mutex<std::collections::HashMap<String, String>>> = LazyLock::new(Default::default);
#[cfg(any(target_os = "windows", target_os = "macos"))]
static CEF_FIND_COUNTS: LazyLock<Mutex<std::collections::HashMap<String, u32>>> = LazyLock::new(Default::default);

// ---------------------------------------------------------------------------------------------
// Print and Save as PDF
// ---------------------------------------------------------------------------------------------

/// Opens the print dialog for the tab (Cmd/Ctrl+P, toolbar). On the system webview it is the
/// platform's print panel, which also offers Save as PDF (macOS, Windows, Linux).
#[tauri::command]
pub async fn cmd_browser_print<R: Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
) -> Result<(), IpcError> {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    if crate::browser::cef_engine::is_cef_tab(&browser_id) {
        return Ok(crate::browser::cef_engine::system::print(&browser_id)?);
    }
    let state = manager.get_state(&browser_id)?;
    let webview = app
        .get_webview(&state.webview_label)
        .ok_or_else(|| BrowserError::WebviewNotFound(state.webview_label.clone()))?;
    webview
        .print()
        .map_err(|error| IpcError::new(IpcErrorCode::Unsupported, format!("print failed: {error}")))
}

/// Saves the page as a PDF after a save dialog. Chromium tabs write it directly (the result
/// arrives as `browser_pdf_saved`); system-webview tabs get the print panel, whose PDF button
/// is the platform's own save path. Returns false when the user cancelled the dialog.
#[tauri::command]
pub async fn cmd_browser_save_pdf<R: Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
) -> Result<bool, IpcError> {
    let state = manager.get_state(&browser_id)?;
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    if crate::browser::cef_engine::is_cef_tab(&browser_id) {
        let title = state.title.clone().unwrap_or_default();
        let suggested = format!("{}.pdf", if title.trim().is_empty() { "page" } else { title.trim() });
        let dialog_app = app.clone();
        let path = run_blocking(move || {
            let dir = download_dir(&dialog_app);
            Ok(dialog_app
                .dialog()
                .file()
                .add_filter("PDF", &["pdf"])
                .set_directory(&dir)
                .set_file_name(crate::browser::system::safe_file_name(&suggested))
                .blocking_save_file()
                .and_then(|path| path.into_path().ok()))
        })
        .await?;
        let Some(path) = path else { return Ok(false) };
        crate::browser::cef_engine::system::print_pdf(&browser_id, &path.to_string_lossy())?;
        return Ok(true);
    }
    let webview = app
        .get_webview(&state.webview_label)
        .ok_or_else(|| BrowserError::WebviewNotFound(state.webview_label.clone()))?;
    webview
        .print()
        .map_err(|error| IpcError::new(IpcErrorCode::Unsupported, format!("print failed: {error}")))?;
    Ok(true)
}

// ---------------------------------------------------------------------------------------------
// Downloads
// ---------------------------------------------------------------------------------------------

#[tauri::command]
pub async fn cmd_browser_set_download_preferences(preferences: BrowserDownloadPreferences) -> Result<(), IpcError> {
    *DOWNLOAD_PREFERENCES.lock() = preferences;
    Ok(())
}

#[tauri::command]
pub async fn cmd_browser_download_pause(download_id: String, pause: bool) -> Result<bool, IpcError> {
    Ok(crate::browser::download::pause_download(&download_id, pause))
}

/// Registry ids of live Chromium engine downloads by (profile, engine download id). Engine
/// ids restart with each engine process, so the registry gets a fresh id per download.
#[cfg(any(target_os = "windows", target_os = "macos"))]
static CEF_DOWNLOADS: LazyLock<Mutex<std::collections::HashMap<(String, u32), String>>> =
    LazyLock::new(Default::default);

fn failure_kind(failure: ferryx_browser_engine_protocol::DownloadFailure) -> (DownloadFailureKind, &'static str) {
    use ferryx_browser_engine_protocol::DownloadFailure;
    match failure {
        DownloadFailure::DiskFull => (DownloadFailureKind::DiskFull, "not enough disk space"),
        DownloadFailure::AccessDenied => (DownloadFailureKind::AccessDenied, "the folder cannot be written"),
        DownloadFailure::Network => (DownloadFailureKind::Network, "network error"),
        DownloadFailure::Server => (DownloadFailureKind::Server, "the server refused the download"),
        DownloadFailure::Other => (DownloadFailureKind::Other, "download failed"),
    }
}

/// Applies one engine download update to the registry (pure over the registry, so tested).
pub(crate) fn apply_download_update(
    registry: &crate::browser::download::DownloadRegistry,
    id: &str,
    received: u64,
    total: Option<u64>,
    state: ferryx_browser_engine_protocol::DownloadState,
    path: &str,
) {
    use ferryx_browser_engine_protocol::DownloadState;
    registry.update_native(id, received, total, Some(path));
    match state {
        DownloadState::InProgress => registry.set_paused(id, false),
        DownloadState::Paused => registry.set_paused(id, true),
        DownloadState::Complete => registry.set_committed(id),
        DownloadState::Cancelled => registry.set_cancelled(id),
        DownloadState::Failed(failure) => {
            let (kind, message) = failure_kind(failure);
            registry.set_failed_kind(id, kind, message.to_string());
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Chromium engine events
// ---------------------------------------------------------------------------------------------

/// Handles the engine events this module owns; returns the event back when it is not one.
#[cfg(any(target_os = "windows", target_os = "macos"))]
pub fn handle_cef_event<R: Runtime>(
    app: &AppHandle<R>,
    manager: &Arc<BrowserManager>,
    profile: &str,
    event: ferryx_browser_engine_protocol::Event,
) -> Option<ferryx_browser_engine_protocol::Event> {
    use crate::browser::cef_engine::system;
    use ferryx_browser_engine_protocol::Event;
    match event {
        Event::Loading { ref tab, loading: false, .. } => {
            if let Ok(state) = manager.get_state(tab) {
                on_webview_page_loaded(app, manager, tab, &state.url);
            }
            return Some(event);
        }
        Event::FindResult { tab, count, active, final_update } => {
            CEF_FIND_COUNTS.lock().insert(tab.clone(), count);
            let _ = app.emit(
                BROWSER_FIND_RESULT_EVENT,
                BrowserFindResultPayload { browser_id: tab, match_count: count, active_match: active, final_update },
            );
        }
        Event::PdfDone { tab, ok, path } => {
            let _ = app.emit(BROWSER_PDF_SAVED_EVENT, BrowserPdfSavedPayload { browser_id: tab, ok, path });
        }
        Event::DownloadBegin { download, total, url, name, .. } => {
            let id = format!("cef-{}", uuid::Uuid::new_v4());
            CEF_DOWNLOADS.lock().insert((profile.to_string(), download), id.clone());
            let app = app.clone();
            let profile = profile.to_string();
            tauri::async_runtime::spawn(async move {
                let chooser = app.clone();
                let suggested = name.clone();
                let path = run_blocking(move || Ok(choose_download_path(&chooser, &suggested)))
                    .await
                    .ok()
                    .flatten();
                let Some(path) = path else {
                    CEF_DOWNLOADS.lock().remove(&(profile.clone(), download));
                    system::download_path(&profile, download, "");
                    return;
                };
                let path = path.to_string_lossy().to_string();
                let control = Arc::new(system::EngineDownloadControl { profile: profile.clone(), download });
                registry().register_native(id, url, path.clone(), total, Some(control), true);
                system::download_path(&profile, download, &path);
            });
        }
        Event::DownloadUpdate { download, received, total, state, path } => {
            let key = (profile.to_string(), download);
            let id = CEF_DOWNLOADS.lock().get(&key).cloned();
            if let Some(id) = id {
                apply_download_update(registry(), &id, received, total, state, &path);
                if matches!(
                    state,
                    ferryx_browser_engine_protocol::DownloadState::Complete
                        | ferryx_browser_engine_protocol::DownloadState::Cancelled
                        | ferryx_browser_engine_protocol::DownloadState::Failed(_)
                ) {
                    CEF_DOWNLOADS.lock().remove(&key);
                }
            }
        }
        Event::FileDialog { request, mode, accept, default_path, .. } => {
            let app = app.clone();
            let profile = profile.to_string();
            tauri::async_runtime::spawn(async move {
                let paths = run_blocking(move || Ok(pick_files(&app, mode, &accept, &default_path)))
                    .await
                    .unwrap_or_default();
                system::file_chosen(&profile, request, paths);
            });
        }
        Event::PageMessage { tab, url, message } => {
            handle_page_notification(app, manager, &tab, &url, &message);
        }
        other => return Some(other),
    }
    None
}

/// Runs a page's file chooser as a native dialog. Blocking.
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn pick_files<R: Runtime>(
    app: &AppHandle<R>,
    mode: ferryx_browser_engine_protocol::FileDialogMode,
    accept: &[String],
    default_path: &str,
) -> Vec<String> {
    use ferryx_browser_engine_protocol::FileDialogMode;
    let mut dialog = app.dialog().file();
    let extensions = accept_extensions(accept);
    if !extensions.is_empty() {
        let refs: Vec<&str> = extensions.iter().map(String::as_str).collect();
        dialog = dialog.add_filter("Accepted files", &refs);
    }
    if !default_path.is_empty() {
        let default = std::path::Path::new(default_path);
        if default.is_dir() {
            dialog = dialog.set_directory(default);
        } else if let Some(name) = default.file_name() {
            dialog = dialog.set_file_name(name.to_string_lossy());
        }
    }
    let into = |path: tauri_plugin_dialog::FilePath| path.into_path().ok().map(|path| path.to_string_lossy().to_string());
    match mode {
        FileDialogMode::Open => dialog.blocking_pick_file().and_then(into).into_iter().collect(),
        FileDialogMode::OpenMultiple => {
            dialog.blocking_pick_files().unwrap_or_default().into_iter().filter_map(into).collect()
        }
        FileDialogMode::OpenFolder => dialog.blocking_pick_folder().and_then(into).into_iter().collect(),
        FileDialogMode::Save => dialog.blocking_save_file().and_then(into).into_iter().collect(),
    }
}

// ---------------------------------------------------------------------------------------------
// Page notifications (both engines)
// ---------------------------------------------------------------------------------------------

// ---------------------------------------------------------------------------------------------
// System webview hooks
// ---------------------------------------------------------------------------------------------

/// The system webview's native download hook (WKDownload, WebView2 DownloadOperation,
/// WebKitDownload through wry). A download the page starts lands in the download folder and is
/// tracked on the shelf; with "ask where to save" it is handed to the pane's Save prompt
/// instead, because the hook runs on the main thread and cannot wait for a dialog. The engine
/// gives no handle to stop it, so the shelf shows it without Cancel.
pub fn webview_download_handler<R: Runtime>(
    browser_id: String,
) -> impl Fn(tauri::Webview<R>, tauri::webview::DownloadEvent<'_>) -> bool + Send + Sync + 'static {
    let live: Arc<Mutex<std::collections::HashMap<String, String>>> = Arc::default();
    move |webview, event| match event {
        tauri::webview::DownloadEvent::Requested { url, destination } => {
            let url = url.to_string();
            if DOWNLOAD_PREFERENCES.lock().ask_where_to_save {
                let _ = webview.app_handle().emit(
                    crate::browser::BROWSER_DOWNLOAD_REQUESTED_EVENT,
                    crate::browser::BrowserDownloadRequestedPayload {
                        browser_id: browser_id.clone(),
                        target_url: url,
                        requires_confirmation: false,
                    },
                );
                return false;
            }
            let suggested = destination
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| url.rsplit('/').next().unwrap_or_default().to_string());
            let dir = download_dir(webview.app_handle());
            let path = unique_download_path(&dir, &suggested, |path| path.exists());
            *destination = path.clone();
            let id = format!("wv-{}", uuid::Uuid::new_v4());
            live.lock().insert(url.clone(), id.clone());
            registry().register_native(id, url, path.to_string_lossy().to_string(), None, None, false);
            true
        }
        tauri::webview::DownloadEvent::Finished { url, path, success } => {
            let Some(id) = live.lock().remove(url.as_str()) else { return true };
            if let Some(path) = path {
                registry().update_native(&id, 0, None, Some(&path.to_string_lossy()));
            }
            if success {
                registry().set_committed(&id);
            } else {
                registry().set_failed(&id, "download failed".to_string());
            }
            true
        }
        _ => true,
    }
}

/// After a system-webview page loaded: tell its Notification shim the site's permission and
/// apply the site's remembered zoom.
pub fn on_webview_page_loaded<R: Runtime>(app: &AppHandle<R>, manager: &Arc<BrowserManager>, browser_id: &str, url: &str) {
    push_page_permission(app, manager, browser_id, url);
    let app = app.clone();
    let manager = Arc::clone(manager);
    let browser_id = browser_id.to_string();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = apply_site_zoom(&app, &manager, &browser_id).await {
            tracing::debug!(%error, "could not apply the site zoom");
        }
    });
}

/// Sends the page's Notification shim a permission state.
async fn answer_page_permission<R: Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    browser_id: &str,
    state: ferryx_browser_engine_protocol::PagePermission,
) {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    if crate::browser::cef_engine::is_cef_tab(browser_id) {
        if let Err(error) = crate::browser::cef_engine::system::notification_permission(browser_id, state) {
            tracing::debug!(%error, "could not answer a page notification request");
        }
        return;
    }
    let script = ferryx_browser_engine_protocol::notification_permission_script(state);
    if let Err(error) = crate::ipc::browser::eval_browser_session(app, manager, browser_id, &script).await {
        tracing::debug!(%error, "could not answer a page notification request");
    }
}

/// Pushes the site's notification permission into a page that finished loading.
pub fn push_page_permission<R: Runtime>(app: &AppHandle<R>, manager: &Arc<BrowserManager>, browser_id: &str, url: &str) {
    let state = page_permission_state(url, site_decider(manager, browser_id));
    let app = app.clone();
    let manager = Arc::clone(manager);
    let browser_id = browser_id.to_string();
    tauri::async_runtime::spawn(async move {
        answer_page_permission(&app, &manager, &browser_id, state).await;
    });
}

/// The tab's remembered site decisions (its profile's); a tab Ferryx does not know is denied.
fn site_decider(
    manager: &BrowserManager,
    browser_id: &str,
) -> impl Fn(&str, site_permissions::PermissionKind) -> site_permissions::SitePermissionDecision {
    let profile = manager.get_state(browser_id).ok().map(|state| state.profile_id.as_str().to_string());
    move |origin, kind| match profile.as_deref() {
        Some(profile) => site_permissions::site_decision(profile, origin, kind),
        None => site_permissions::SitePermissionDecision::Denied,
    }
}

/// A message from a page's Notification shim (either engine).
pub fn handle_page_notification<R: Runtime>(
    app: &AppHandle<R>,
    manager: &Arc<BrowserManager>,
    browser_id: &str,
    url: &str,
    raw: &str,
) {
    let Some(message) = parse_page_notification(raw) else { return };
    match page_notification_action(url, &message, site_decider(manager, browser_id)) {
        // An undecided site asked: the pane prompts, and the answer goes to the page's shim.
        PageNotificationAction::Ignore if message == PageNotificationMessage::Request => {
            let app = app.clone();
            let manager = Arc::clone(manager);
            let target = browser_id.to_string();
            let answer = site_permissions::Answer::new(Box::new(move |allow| {
                use ferryx_browser_engine_protocol::PagePermission;
                let state = if allow { PagePermission::Granted } else { PagePermission::Denied };
                tauri::async_runtime::spawn(async move {
                    answer_page_permission(&app, &manager, &target, state).await;
                });
            }));
            site_permissions::request_for_browser(
                browser_id,
                Some(url.to_string()),
                vec![site_permissions::PermissionKind::Notifications],
                answer,
            );
        }
        PageNotificationAction::Ignore => {}
        PageNotificationAction::Answer(state) => {
            let app = app.clone();
            let manager = Arc::clone(manager);
            let browser_id = browser_id.to_string();
            tauri::async_runtime::spawn(async move {
                answer_page_permission(&app, &manager, &browser_id, state).await;
            });
        }
        PageNotificationAction::Show { title, body } => {
            let backend = crate::ipc::notifications::TauriNotificationBackend::new(app.clone());
            tauri::async_runtime::spawn(async move {
                let result = run_blocking(move || {
                    use crate::notification::NativeNotificationBackend;
                    backend
                        .submit(&crate::notification::NotificationContent {
                            title,
                            body,
                            sound: crate::notification::NotificationSound::System,
                            target: None,
                        })
                        .map_err(IpcError::internal)
                })
                .await;
                if let Err(error) = result {
                    tracing::warn!(%error, "a page notification could not be shown");
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::download::{DownloadRegistry, DownloadStatus};
    use ferryx_browser_engine_protocol::{DownloadFailure, DownloadState};

    #[test]
    fn engine_updates_drive_the_download_record_to_completion() {
        let registry = DownloadRegistry::new();
        registry.register_native("d1".into(), "https://a.test/f".into(), "/tmp/f".into(), None, None, false);
        apply_download_update(&registry, "d1", 10, Some(100), DownloadState::InProgress, "/tmp/f");
        apply_download_update(&registry, "d1", 40, Some(100), DownloadState::Paused, "/tmp/f");
        assert_eq!(registry.get("d1").unwrap().status, DownloadStatus::Paused);
        apply_download_update(&registry, "d1", 60, Some(100), DownloadState::InProgress, "/tmp/f");
        apply_download_update(&registry, "d1", 100, Some(100), DownloadState::Complete, "/tmp/f");
        let record = registry.get("d1").unwrap();
        assert_eq!((record.status, record.received_bytes, record.total_bytes), (DownloadStatus::Completed, 100, Some(100)));
        apply_download_update(&registry, "d1", 100, Some(100), DownloadState::Cancelled, "/tmp/f");
        assert_eq!(registry.get("d1").unwrap().status, DownloadStatus::Completed, "terminal states stick");
    }

    #[test]
    fn an_engine_disk_full_failure_is_reported_as_such() {
        let registry = DownloadRegistry::new();
        registry.register_native("d2".into(), "https://a.test/f".into(), "/tmp/f".into(), None, None, false);
        apply_download_update(&registry, "d2", 5, None, DownloadState::Failed(DownloadFailure::DiskFull), "/tmp/f");
        let record = registry.get("d2").unwrap();
        assert_eq!(record.status, DownloadStatus::Failed);
        assert_eq!(record.failure, Some(DownloadFailureKind::DiskFull));
    }
}
