//! Tauri commands for the browser library: bookmarks, history and omnibox suggestions. The
//! stores live under the app data dir (`FERRYX_DATA_DIR` when an isolated instance sets it), so
//! an isolated profile never sees the real history. Every store call runs on a blocking thread.
//!
//! Emits `browser_library_changed` with `{ kind: "bookmarks" | "history" }` after a change so
//! every window refreshes its view.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::browser::library::bookmarks::{
    Bookmark, BookmarkError, BookmarkFolder, BookmarkTree, ImportedBookmark, LoadOutcome, BAR_ROOT,
};
use crate::browser::library::history::{HistoryEntry, HistoryError, ImportedVisit};
use crate::browser::library::recorder::{HistoryRecorder, RecordAction};
use crate::browser::library::{suggest, BrowserLibrary};
use crate::browser::BrowserState;
use crate::ipc::error::{IpcError, IpcErrorCode};

pub const BROWSER_LIBRARY_CHANGED_EVENT: &str = "browser_library_changed";
const LEGACY_MIGRATION_KEY: &str = "ferryx.browser.history.localStorageMigrated";

static LIBRARY: OnceLock<Arc<BrowserLibrary>> = OnceLock::new();
static RECORDER: OnceLock<HistoryRecorder> = OnceLock::new();

fn recorder() -> &'static HistoryRecorder {
    RECORDER.get_or_init(HistoryRecorder::default)
}

fn library_data_dir<R: tauri::Runtime>(app: &AppHandle<R>) -> Result<PathBuf, IpcError> {
    if let Some(dir) = std::env::var_os("FERRYX_DATA_DIR").filter(|dir| !dir.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    app.path()
        .app_data_dir()
        .map_err(|error| IpcError::new(IpcErrorCode::IoError, error.to_string()))
}

fn library<R: tauri::Runtime>(app: &AppHandle<R>) -> Result<Arc<BrowserLibrary>, IpcError> {
    if let Some(library) = LIBRARY.get() {
        return Ok(Arc::clone(library));
    }
    let dir = library_data_dir(app)?;
    Ok(Arc::clone(LIBRARY.get_or_init(|| Arc::new(BrowserLibrary::new(dir)))))
}

impl From<BookmarkError> for IpcError {
    fn from(error: BookmarkError) -> Self {
        let code = match &error {
            BookmarkError::Io(_) => IpcErrorCode::IoError,
            BookmarkError::Encode(_) => IpcErrorCode::InternalError,
            BookmarkError::NotFound(_) | BookmarkError::Invalid(_) => IpcErrorCode::InvalidArgument,
        };
        IpcError::new(code, error.to_string())
    }
}

impl From<HistoryError> for IpcError {
    fn from(error: HistoryError) -> Self {
        let code = match &error {
            HistoryError::Io(_) => IpcErrorCode::IoError,
            HistoryError::Sql(_) => IpcErrorCode::InternalError,
        };
        IpcError::new(code, error.to_string())
    }
}

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum LibraryChange {
    Bookmarks,
    History,
}

fn notify<R: tauri::Runtime>(app: &AppHandle<R>, change: LibraryChange) {
    let _ = app.emit(BROWSER_LIBRARY_CHANGED_EVENT, change);
}

/// Feeds a browser state change from either engine into history. Called from the places that
/// publish `browser_state_changed`; the write runs on a blocking thread.
pub fn observe_browser_state<R: tauri::Runtime>(app: &AppHandle<R>, state: &BrowserState) {
    let Some(action) = recorder().observe(state) else { return };
    let Ok(library) = library(app) else { return };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = crate::ipc::run_blocking(move || {
            library
                .with_history(|history| match action {
                    RecordAction::Visit { url, title } => {
                        history.record_visit(&url, title.as_deref(), now_ms(), false).map(|_| ())
                    }
                    RecordAction::Title { url, title } => history.update_title(&url, &title),
                })
                .map_err(IpcError::from)
        })
        .await;
        match result {
            Ok(()) => notify(&app, LibraryChange::History),
            Err(error) => tracing::warn!(?error, "could not record a history visit"),
        }
    });
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BookmarksView {
    pub tree: BookmarkTree,
    /// "recoveredFromBackup" or "resetCorrupt" when the store needed recovery at open.
    pub recovery: Option<&'static str>,
}

fn bookmarks_view(library: &BrowserLibrary) -> Result<BookmarksView, IpcError> {
    let tree = library.with_bookmarks(|store| Ok(store.tree().clone()))?;
    let recovery = match library.bookmark_recovery() {
        Some(LoadOutcome::RecoveredFromBackup) => Some("recoveredFromBackup"),
        Some(LoadOutcome::ResetCorrupt) => Some("resetCorrupt"),
        _ => None,
    };
    Ok(BookmarksView { tree, recovery })
}

/// Runs a bookmark mutation, saves atomically, and answers the new tree.
async fn mutate_bookmarks<R, T, F>(app: &AppHandle<R>, change: F) -> Result<(T, BookmarksView), IpcError>
where
    R: tauri::Runtime,
    T: Send + 'static,
    F: FnOnce(&mut crate::browser::library::bookmarks::BookmarkStore) -> Result<T, BookmarkError> + Send + 'static,
{
    let library = library(app)?;
    let result = crate::ipc::run_blocking(move || {
        let value = library.with_bookmarks(|store| {
            let value = change(store)?;
            store.save()?;
            Ok(value)
        })?;
        Ok((value, bookmarks_view(&library)?))
    })
    .await?;
    notify(app, LibraryChange::Bookmarks);
    Ok(result)
}

#[tauri::command]
pub async fn cmd_browser_bookmarks_get<R: tauri::Runtime>(app: AppHandle<R>) -> Result<BookmarksView, IpcError> {
    let library = library(&app)?;
    crate::ipc::run_blocking(move || bookmarks_view(&library)).await
}

#[tauri::command]
pub async fn cmd_browser_bookmark_add<R: tauri::Runtime>(
    app: AppHandle<R>,
    title: String,
    url: String,
    parent_id: Option<String>,
) -> Result<Bookmark, IpcError> {
    let parent = parent_id.unwrap_or_else(|| BAR_ROOT.to_string());
    Ok(mutate_bookmarks(&app, move |store| store.add_bookmark(&title, &url, &parent)).await?.0)
}

#[tauri::command]
pub async fn cmd_browser_bookmark_folder_create<R: tauri::Runtime>(
    app: AppHandle<R>,
    name: String,
    parent_id: Option<String>,
) -> Result<BookmarkFolder, IpcError> {
    let parent = parent_id.unwrap_or_else(|| BAR_ROOT.to_string());
    Ok(mutate_bookmarks(&app, move |store| store.create_folder(&name, &parent)).await?.0)
}

#[tauri::command]
pub async fn cmd_browser_bookmark_update<R: tauri::Runtime>(
    app: AppHandle<R>,
    id: String,
    title: Option<String>,
    url: Option<String>,
) -> Result<BookmarksView, IpcError> {
    Ok(mutate_bookmarks(&app, move |store| store.update(&id, title.as_deref(), url.as_deref())).await?.1)
}

#[tauri::command]
pub async fn cmd_browser_bookmark_move<R: tauri::Runtime>(
    app: AppHandle<R>,
    id: String,
    parent_id: String,
    index: Option<usize>,
) -> Result<BookmarksView, IpcError> {
    Ok(mutate_bookmarks(&app, move |store| store.move_item(&id, &parent_id, index)).await?.1)
}

#[tauri::command]
pub async fn cmd_browser_bookmark_delete<R: tauri::Runtime>(
    app: AppHandle<R>,
    id: String,
) -> Result<BookmarksView, IpcError> {
    Ok(mutate_bookmarks(&app, move |store| store.delete(&id)).await?.1)
}

/// Bulk insert for the import lane: `root` is "bar" or "other".
#[tauri::command]
pub async fn cmd_browser_bookmarks_import<R: tauri::Runtime>(
    app: AppHandle<R>,
    root: String,
    entries: Vec<ImportedBookmark>,
) -> Result<usize, IpcError> {
    Ok(mutate_bookmarks(&app, move |store| store.import(&root, &entries)).await?.0)
}

#[tauri::command]
pub async fn cmd_browser_history_list<R: tauri::Runtime>(
    app: AppHandle<R>,
    query: Option<String>,
    before: Option<i64>,
    limit: Option<usize>,
) -> Result<Vec<HistoryEntry>, IpcError> {
    let library = library(&app)?;
    crate::ipc::run_blocking(move || {
        Ok(library.with_history(|history| {
            history.list(query.as_deref().unwrap_or(""), before, limit.unwrap_or(200))
        })?)
    })
    .await
}

#[tauri::command]
pub async fn cmd_browser_history_suggest<R: tauri::Runtime>(
    app: AppHandle<R>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<HistoryEntry>, IpcError> {
    let library = library(&app)?;
    crate::ipc::run_blocking(move || {
        Ok(library.with_history(|history| history.suggest(&query, limit.unwrap_or(8)))?)
    })
    .await
}

#[tauri::command]
pub async fn cmd_browser_history_delete<R: tauri::Runtime>(app: AppHandle<R>, url: String) -> Result<bool, IpcError> {
    let library = library(&app)?;
    let removed = crate::ipc::run_blocking(move || Ok(library.with_history(|history| history.delete_url(&url))?)).await?;
    notify(&app, LibraryChange::History);
    Ok(removed)
}

/// Clears visits in `[from, to)` (ms since the Unix epoch); both absent clears everything.
#[tauri::command]
pub async fn cmd_browser_history_clear<R: tauri::Runtime>(
    app: AppHandle<R>,
    from: Option<i64>,
    to: Option<i64>,
) -> Result<(), IpcError> {
    let library = library(&app)?;
    crate::ipc::run_blocking(move || {
        library.with_history(|history| match (from, to) {
            (None, None) => history.clear_all(),
            (from, to) => history.clear_range(from.unwrap_or(0), to.unwrap_or(i64::MAX)).map(|_| ()),
        })?;
        Ok(())
    })
    .await?;
    notify(&app, LibraryChange::History);
    Ok(())
}

/// Bulk insert for the import lane.
#[tauri::command]
pub async fn cmd_browser_history_import<R: tauri::Runtime>(
    app: AppHandle<R>,
    visits: Vec<ImportedVisit>,
) -> Result<usize, IpcError> {
    let library = library(&app)?;
    let inserted = crate::ipc::run_blocking(move || Ok(library.with_history(|history| history.import(&visits))?)).await?;
    notify(&app, LibraryChange::History);
    Ok(inserted)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyMigration {
    pub imported: usize,
    pub already_migrated: bool,
}

/// One-time move of the UI's old localStorage history into the host store. A second call
/// (another window, a later launch) imports nothing.
#[tauri::command]
pub async fn cmd_browser_history_migrate_legacy<R: tauri::Runtime>(
    app: AppHandle<R>,
    visits: Vec<ImportedVisit>,
) -> Result<LegacyMigration, IpcError> {
    let library = library(&app)?;
    let result = crate::ipc::run_blocking(move || {
        Ok(library.with_history(|history| {
            if history.meta(LEGACY_MIGRATION_KEY)?.is_some() {
                return Ok(LegacyMigration { imported: 0, already_migrated: true });
            }
            let imported = history.import(&visits)?;
            history.set_meta(LEGACY_MIGRATION_KEY, &now_ms().to_string())?;
            Ok(LegacyMigration { imported, already_migrated: false })
        })?)
    })
    .await?;
    if result.imported > 0 {
        notify(&app, LibraryChange::History);
    }
    Ok(result)
}

/// The UI owns the "remember browsing history" setting and reports it here; nothing is
/// recorded until it does.
#[tauri::command]
pub fn cmd_browser_history_set_enabled(enabled: bool) {
    recorder().set_enabled(enabled);
}

/// Opt-in search-engine suggestions. Any failure (offline, timeout, unexpected body) is an
/// error the omnibox answers by showing local suggestions only.
#[tauri::command]
pub async fn cmd_browser_search_suggest(engine: String, query: String) -> Result<Vec<String>, IpcError> {
    let unavailable = |message: String| IpcError::new(IpcErrorCode::HostUnavailable, message);
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    let url = suggest::suggest_url(&engine, &query)
        .ok_or_else(|| IpcError::new(IpcErrorCode::InvalidArgument, format!("unknown search engine: {engine}")))?;
    let client = reqwest::Client::builder()
        .user_agent(concat!("Ferryx/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_millis(1500))
        .build()
        .map_err(|error| IpcError::internal(error.to_string()))?;
    let response = client.get(url).send().await.map_err(|error| unavailable(error.to_string()))?;
    if !response.status().is_success() {
        return Err(unavailable(format!("suggest endpoint answered {}", response.status())));
    }
    let body = response.bytes().await.map_err(|error| unavailable(error.to_string()))?;
    suggest::parse_open_search(&body, 6).ok_or_else(|| unavailable("suggest endpoint sent an unexpected body".into()))
}
