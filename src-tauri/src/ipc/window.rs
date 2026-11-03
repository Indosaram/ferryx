//! Secondary desktop windows (`ws-<id>`): creation, listing and placement persistence.
//!
//! `main` is created from `tauri.conf.json`; every other app window is built here with the same
//! attributes and loads `index.html?window=<label>` so the UI knows which window it is.
//! Secondary window placement is kept in `windows.json` beside the primary session file (so it
//! follows the same `FERRYX_SESSION_DIR` / dev-runtime isolation as the per-window sessions) as
//! `[{ label, x, y, width, height }]` (logical pixels). The file is rewritten when a secondary
//! window is created, moved, resized or closed by the user, and those windows are recreated after
//! `main` on startup.
//!
//! Feature gate: restoring saved windows and the native "New Window" menu item only run when
//! [`multi_window_enabled`] is true, which reads `FERRYX_MULTI_WINDOW=1`. Preferences live in the
//! frontend (there is no backend preferences store to read `experimental.multiWindow` from), so an
//! env opt-in is the least invasive gate while the feature is QA'd. `cmd_window_new` itself is not
//! gated: the UI decides whether to offer it.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Runtime, State, WebviewUrl, WebviewWindowBuilder, Window};

use crate::ipc::{run_blocking, IpcError, IpcErrorCode};
use crate::window_registry::{
    is_app_window_label, WindowRegistry, PRIMARY_WINDOW_LABEL, SECONDARY_WINDOW_PREFIX,
};

/// File (beside the primary session file) holding the secondary window list.
pub const WINDOWS_FILE_NAME: &str = "windows.json";
/// Env opt-in for restoring secondary windows and the native "New Window" item.
pub const MULTI_WINDOW_ENV: &str = "FERRYX_MULTI_WINDOW";

// Mirrors the `main` window in tauri.conf.json.
const WINDOW_TITLE: &str = "Ferryx";
const DEFAULT_WIDTH: f64 = 1280.0;
const DEFAULT_HEIGHT: f64 = 850.0;
const MIN_WIDTH: f64 = 800.0;
const MIN_HEIGHT: f64 = 600.0;

/// Coalesces bursts of move/resize events into one write.
const SAVE_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(400);

/// Whether the experimental multi-window restore and menu entry are enabled.
pub fn multi_window_enabled() -> bool {
    multi_window_enabled_from(std::env::var(MULTI_WINDOW_ENV).ok().as_deref())
}

fn multi_window_enabled_from(value: Option<&str>) -> bool {
    matches!(value.map(str::trim), Some("1") | Some("true"))
}

/// Requested placement of a new window, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// One entry of `windows.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedWindow {
    pub label: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl PersistedWindow {
    pub fn bounds(&self) -> WindowBounds {
        WindowBounds {
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
        }
    }
}

/// A fresh secondary window label, `ws-<uuid simple>`.
pub fn new_window_label() -> String {
    format!("{SECONDARY_WINDOW_PREFIX}{}", uuid::Uuid::new_v4().simple())
}

/// True for a `ws-` label whose id is non-empty and uses only characters Tauri accepts in labels.
pub fn is_secondary_window_label(label: &str) -> bool {
    label.strip_prefix(SECONDARY_WINDOW_PREFIX).is_some_and(|id| {
        !id.is_empty()
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    })
}

/// Rejects non-finite or empty bounds and clamps the size to the window minimum.
pub fn sanitize_bounds(bounds: WindowBounds) -> Option<WindowBounds> {
    let finite = [bounds.x, bounds.y, bounds.width, bounds.height]
        .iter()
        .all(|value| value.is_finite());
    if !finite || bounds.width <= 0.0 || bounds.height <= 0.0 {
        return None;
    }
    Some(WindowBounds {
        x: bounds.x,
        y: bounds.y,
        width: bounds.width.max(MIN_WIDTH),
        height: bounds.height.max(MIN_HEIGHT),
    })
}

/// Parses `windows.json`. Unreadable content yields an empty list (the file only restores
/// placement, so a damaged file must never block startup); invalid or duplicate entries are
/// dropped and sizes are clamped to the window minimum.
pub fn parse_windows_file(contents: &str) -> Vec<PersistedWindow> {
    let Ok(entries) = serde_json::from_str::<Vec<serde_json::Value>>(contents) else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    entries
        .into_iter()
        .filter_map(|entry| serde_json::from_value::<PersistedWindow>(entry).ok())
        .filter(|entry| is_secondary_window_label(&entry.label))
        .filter_map(|entry| {
            let bounds = sanitize_bounds(entry.bounds())?;
            Some(PersistedWindow {
                label: entry.label,
                x: bounds.x,
                y: bounds.y,
                width: bounds.width,
                height: bounds.height,
            })
        })
        .filter(|entry| seen.insert(entry.label.clone()))
        .collect()
}

/// Applies a save pass: live windows replace or append their entry, user-closed windows are
/// removed, and entries of windows that are neither (already torn down by app quit) are kept.
pub fn merge_windows(
    existing: Vec<PersistedWindow>,
    live: &[PersistedWindow],
    closed: &HashSet<String>,
) -> Vec<PersistedWindow> {
    let mut merged: Vec<PersistedWindow> = existing
        .into_iter()
        .filter(|entry| !closed.contains(&entry.label))
        .map(|entry| {
            live.iter()
                .find(|candidate| candidate.label == entry.label)
                .cloned()
                .unwrap_or(entry)
        })
        .collect();
    for entry in live {
        if !closed.contains(&entry.label) && !merged.iter().any(|kept| kept.label == entry.label) {
            merged.push(entry.clone());
        }
    }
    merged
}

fn windows_file_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, IpcError> {
    let session_path = crate::ipc::session::get_session_file_path(app)?;
    Ok(windows_file_beside(&session_path))
}

/// `windows.json` lives in the directory of the primary session file.
fn windows_file_beside(session_path: &Path) -> PathBuf {
    session_path
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .join(WINDOWS_FILE_NAME)
}

fn read_windows_file(path: &Path) -> Vec<PersistedWindow> {
    match std::fs::read_to_string(path) {
        Ok(contents) => parse_windows_file(&contents),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "Could not read the saved window list");
            Vec::new()
        }
    }
}

fn write_windows_file(path: &Path, windows: &[PersistedWindow]) -> Result<(), IpcError> {
    let io_error = |error: std::io::Error| {
        IpcError::new(
            IpcErrorCode::IoError,
            format!("Failed to write {}: {error}", path.display()),
        )
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(io_error)?;
    }
    let json = serde_json::to_vec_pretty(windows)
        .map_err(|error| IpcError::internal(format!("Failed to encode the window list: {error}")))?;
    let tmp_path = path.with_extension("json.tmp");
    std::fs::write(&tmp_path, json).map_err(io_error)?;
    std::fs::rename(&tmp_path, path).map_err(|error| {
        let _ = std::fs::remove_file(&tmp_path);
        io_error(error)
    })
}

/// Labels the user closed since the last save pass.
fn closed_labels() -> &'static Mutex<HashSet<String>> {
    static CLOSED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    CLOSED.get_or_init(|| Mutex::new(HashSet::new()))
}

static SAVE_SCHEDULED: AtomicBool = AtomicBool::new(false);

/// Current logical placement of a live window.
fn window_placement<R: Runtime>(window: &Window<R>) -> Option<PersistedWindow> {
    let scale = window.scale_factor().ok().filter(|scale| *scale > 0.0)?;
    let position = window.outer_position().ok()?.to_logical::<f64>(scale);
    let size = window.inner_size().ok()?.to_logical::<f64>(scale);
    Some(PersistedWindow {
        label: window.label().to_string(),
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    })
}

fn live_secondary_placements<R: Runtime>(app: &AppHandle<R>) -> Vec<PersistedWindow> {
    let closed = closed_labels().lock().clone();
    let mut placements: Vec<PersistedWindow> = app
        .windows()
        .into_iter()
        .filter(|(label, _)| is_secondary_window_label(label) && !closed.contains(label))
        // A minimized window reports a meaningless position; keep its last saved placement.
        .filter(|(_, window)| !matches!(window.is_minimized(), Ok(true)))
        .filter_map(|(_, window)| window_placement(&window))
        .collect();
    placements.sort_by(|a, b| a.label.cmp(&b.label));
    placements
}

/// Writes one merged snapshot of the secondary window list.
async fn save_windows<R: Runtime>(app: &AppHandle<R>) -> Result<(), IpcError> {
    let path = windows_file_path(app)?;
    let live = live_secondary_placements(app);
    let closed = std::mem::take(&mut *closed_labels().lock());
    run_blocking(move || {
        let merged = merge_windows(read_windows_file(&path), &live, &closed);
        write_windows_file(&path, &merged)
    })
    .await
}

/// Schedules a debounced save; calls within the debounce window collapse into one write.
pub fn schedule_windows_save<R: Runtime>(app: &AppHandle<R>) {
    if SAVE_SCHEDULED.swap(true, Ordering::AcqRel) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(SAVE_DEBOUNCE).await;
        SAVE_SCHEDULED.store(false, Ordering::Release);
        if let Err(error) = save_windows(&app).await {
            tracing::warn!(?error, "Could not save the window list");
        }
    });
}

/// Window event hook: placement changes of a secondary window are persisted.
pub fn note_window_geometry_changed<R: Runtime>(window: &Window<R>) {
    if is_secondary_window_label(window.label()) {
        schedule_windows_save(window.app_handle());
    }
}

/// Window event hook: a user-closed secondary window is dropped from the saved list. App quit
/// does not request a close per window, so windows open at quit stay listed and come back.
pub fn note_window_close_requested<R: Runtime>(window: &Window<R>) {
    if is_secondary_window_label(window.label()) {
        closed_labels().lock().insert(window.label().to_string());
        schedule_windows_save(window.app_handle());
    }
}

#[cfg(all(target_os = "windows", feature = "native-terminal"))]
fn windows_focus_registrations() -> &'static Mutex<std::collections::HashMap<String, isize>> {
    static REGISTRATIONS: OnceLock<Mutex<std::collections::HashMap<String, isize>>> =
        OnceLock::new();
    REGISTRATIONS.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

/// Window event hook: releases per-window native state of a destroyed secondary window.
pub fn note_window_destroyed<R: Runtime>(window: &Window<R>) {
    // Adoptions queued for a window that never booted must not outlive it; the sessions they
    // named were re-owned to this window and are handed on as orphans by the destroy hook.
    if let Some(pending) = window.app_handle().try_state::<PendingWindowAdoptions>() {
        pending.forget(window.label());
    }
    #[cfg(all(target_os = "windows", feature = "native-terminal"))]
    if let Some(root_hwnd) = windows_focus_registrations().lock().remove(window.label()) {
        crate::native_terminal::platform::windows_focus::unregister_windows_terminal_focus_window(
            root_hwnd,
        );
    }
}

/// Builds a secondary app window with the `main` window's attributes and runs the per-window
/// setup. `bounds` places it; without them it opens centered at the default size.
async fn build_app_window<R: Runtime>(
    app: &AppHandle<R>,
    label: &str,
    bounds: Option<WindowBounds>,
) -> Result<String, IpcError> {
    if !is_secondary_window_label(label) {
        return Err(IpcError::new(
            IpcErrorCode::InvalidArgument,
            format!("{label} is not a secondary window label"),
        ));
    }
    if app.get_window(label).is_some() {
        return Ok(label.to_string());
    }
    let url = WebviewUrl::App(PathBuf::from(format!("index.html?window={label}")));
    let builder = WebviewWindowBuilder::new(app, label, url)
        .title(WINDOW_TITLE)
        .min_inner_size(MIN_WIDTH, MIN_HEIGHT)
        .resizable(true)
        .fullscreen(false)
        .transparent(true);
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);
    let builder = match bounds.and_then(sanitize_bounds) {
        Some(bounds) => builder
            .inner_size(bounds.width, bounds.height)
            .position(bounds.x, bounds.y),
        None => builder.inner_size(DEFAULT_WIDTH, DEFAULT_HEIGHT).center(),
    };
    let window = builder
        .build()
        .map_err(|error| IpcError::internal(format!("Could not create window {label}: {error}")))?;

    // AppKit setup (file drop view, first responder) must run on the main thread.
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let target = window.clone();
    window
        .run_on_main_thread(move || {
            let result = crate::configure_app_window(&target).map_err(|error| error.to_string());
            let _ = sender.send(result);
        })
        .map_err(|error| IpcError::internal(format!("Could not schedule window setup: {error}")))?;
    match receiver.await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => tracing::warn!(label, %error, "Window setup failed"),
        Err(_) => tracing::warn!(label, "Main thread stopped before window setup completed"),
    }

    #[cfg(all(target_os = "windows", feature = "native-terminal"))]
    {
        let surface_host = app
            .state::<crate::native_terminal::surface_host::NativeTerminalSurfaceHostState>()
            .inner()
            .clone();
        if let Some(root_hwnd) =
            crate::native_terminal::platform::windows_focus::register_windows_terminal_focus_window(
                &window.as_ref().window(),
                surface_host,
            )
        {
            windows_focus_registrations()
                .lock()
                .insert(label.to_string(), root_hwnd);
        }
    }

    schedule_windows_save(app);
    Ok(label.to_string())
}

/// Opens a new secondary window and returns its label.
pub async fn create_app_window<R: Runtime>(
    app: &AppHandle<R>,
    bounds: Option<WindowBounds>,
) -> Result<String, IpcError> {
    build_app_window(app, &new_window_label(), bounds).await
}

/// Recreates the saved secondary windows. Called once after `main` is set up, and only when
/// [`multi_window_enabled`].
pub async fn restore_saved_windows<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<Vec<String>, IpcError> {
    let path = windows_file_path(app)?;
    let saved = run_blocking(move || Ok(read_windows_file(&path))).await?;
    let mut restored = Vec::with_capacity(saved.len());
    for entry in saved {
        match build_app_window(app, &entry.label, Some(entry.bounds())).await {
            Ok(label) => restored.push(label),
            Err(error) => tracing::warn!(label = %entry.label, ?error, "Could not restore window"),
        }
    }
    Ok(restored)
}

/// Open app window labels, `main` first and the rest sorted.
pub fn app_window_labels<R: Runtime>(app: &AppHandle<R>) -> Vec<String> {
    let mut labels: Vec<String> = app
        .windows()
        .into_keys()
        .filter(|label| is_app_window_label(label))
        .collect();
    labels.sort_by(|a, b| {
        (a != PRIMARY_WINDOW_LABEL)
            .cmp(&(b != PRIMARY_WINDOW_LABEL))
            .then_with(|| a.cmp(b))
    });
    labels
}

/// Opens a new desktop window and returns its label (`ws-<id>`).
#[tauri::command]
pub async fn cmd_window_new<R: Runtime>(
    app: AppHandle<R>,
    bounds: Option<WindowBounds>,
) -> Result<String, IpcError> {
    create_app_window(&app, bounds).await
}

/// Labels of the open app windows (`main` and `ws-*`).
#[tauri::command]
pub fn cmd_window_list<R: Runtime>(app: AppHandle<R>) -> Vec<String> {
    app_window_labels(&app)
}

/// Event telling one window to adopt backend sessions moved to it from another window.
/// Payload: [`WindowSessionsAdoptPayload`]. Emitted only to the target window (`emit_to`).
pub const WINDOW_SESSIONS_ADOPT_EVENT: &str = "window_sessions_adopt";

/// Live backend sessions a window should adopt as tabs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowSessionsAdoptPayload {
    pub backend_session_ids: Vec<String>,
    /// Make the adopted tab the active one.
    pub activate: bool,
    /// Workspace the tab came from, so a window showing another project can switch to it first.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    /// Native attach generation each session last had, so the target can attach with a newer
    /// generation than the source's now-detached binding (older ones are rejected as stale).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attempt_generations: BTreeMap<String, u64>,
}

/// What [`PendingWindowAdoptions::deliver`] decided for a request.
#[derive(Debug, PartialEq, Eq)]
pub enum AdoptionDelivery {
    /// The window is listening: emit the request to it now.
    Emit(WindowSessionsAdoptPayload),
    /// The window has not booted yet: the request waits for its `take`.
    Queued,
}

#[derive(Debug, Default)]
struct PendingAdoptionsInner {
    /// Windows that already pulled their queue, so their event listener is registered.
    booted: HashSet<String>,
    queued: HashMap<String, Vec<WindowSessionsAdoptPayload>>,
}

/// Adoption requests for windows that have not booted yet, keyed by window label.
///
/// A window registers its `window_sessions_adopt` listener first and then calls
/// `cmd_window_take_pending_adoptions`. `take` marks the window booted under the same lock that
/// drains the queue, so a request is either in the drained queue or emitted to a live listener,
/// never lost between the two.
#[derive(Debug, Default)]
pub struct PendingWindowAdoptions {
    inner: Mutex<PendingAdoptionsInner>,
}

impl PendingWindowAdoptions {
    pub fn deliver(&self, label: &str, payload: WindowSessionsAdoptPayload) -> AdoptionDelivery {
        let mut inner = self.inner.lock();
        if inner.booted.contains(label) {
            return AdoptionDelivery::Emit(payload);
        }
        inner.queued.entry(label.to_string()).or_default().push(payload);
        AdoptionDelivery::Queued
    }

    /// Puts back a request whose emit failed; the window is treated as not booted again.
    pub fn requeue(&self, label: &str, payload: WindowSessionsAdoptPayload) {
        let mut inner = self.inner.lock();
        inner.booted.remove(label);
        inner.queued.entry(label.to_string()).or_default().push(payload);
    }

    /// Drains the queued requests of `label` in arrival order and marks the window booted.
    pub fn take(&self, label: &str) -> Vec<WindowSessionsAdoptPayload> {
        let mut inner = self.inner.lock();
        inner.booted.insert(label.to_string());
        inner.queued.remove(label).unwrap_or_default()
    }

    /// Drops everything known about a destroyed window.
    pub fn forget(&self, label: &str) {
        let mut inner = self.inner.lock();
        inner.booted.remove(label);
        inner.queued.remove(label);
    }
}

/// Which of the requested sessions a window may hand over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovePlan {
    pub movable: Vec<String>,
    /// Sessions another window owns; they are left alone.
    pub skipped: Vec<String>,
}

/// Result of `cmd_window_move_sessions`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowMoveOutcome {
    pub target_label: String,
    pub moved: Vec<String>,
    pub skipped: Vec<String>,
    /// True when the target had not booted and the request waits for its first pull.
    pub queued: bool,
}

/// Validates a move request without touching any window or session. A session may leave the
/// calling window only when that window owns it or nobody has recorded an owner yet.
pub fn plan_move(
    registry: &WindowRegistry,
    source_label: &str,
    target_label: Option<&str>,
    backend_session_ids: &[String],
) -> Result<MovePlan, IpcError> {
    if let Some(target) = target_label {
        if !is_app_window_label(target) {
            return Err(IpcError::new(
                IpcErrorCode::InvalidArgument,
                format!("{target} is not an app window"),
            ));
        }
        if target == source_label {
            return Err(IpcError::new(
                IpcErrorCode::InvalidArgument,
                "The sessions already live in that window",
            ));
        }
    }
    let mut seen = HashSet::new();
    let mut plan = MovePlan {
        movable: Vec::new(),
        skipped: Vec::new(),
    };
    for id in backend_session_ids {
        if id.is_empty() || !seen.insert(id.as_str()) {
            continue;
        }
        match registry.owner(id) {
            Some(owner) if owner != source_label => plan.skipped.push(id.clone()),
            _ => plan.movable.push(id.clone()),
        }
    }
    if plan.movable.is_empty() {
        return Err(IpcError::new(
            IpcErrorCode::InvalidArgument,
            "None of the sessions can be moved out of this window",
        ));
    }
    Ok(plan)
}

/// Flips ownership of the planned sessions to `target_label` and tells only that window to adopt
/// them. Nothing here closes, kills or re-spawns a session: the daemon PTY keeps running and the
/// target attaches a new surface to it, replaying from the ring buffer.
pub fn apply_move<R: Runtime>(
    app: &AppHandle<R>,
    registry: &WindowRegistry,
    pending: &PendingWindowAdoptions,
    target_label: &str,
    plan: MovePlan,
    workspace_id: Option<String>,
    attempt_generations: BTreeMap<String, u64>,
) -> Result<WindowMoveOutcome, IpcError> {
    if !is_app_window_label(target_label) || app.get_window(target_label).is_none() {
        return Err(IpcError::new(
            IpcErrorCode::InvalidArgument,
            format!("Window {target_label} is not open"),
        ));
    }
    for session_id in &plan.movable {
        registry.set_owner(session_id, target_label);
    }
    let payload = WindowSessionsAdoptPayload {
        backend_session_ids: plan.movable.clone(),
        activate: true,
        workspace_id,
        attempt_generations,
    };
    let queued = match pending.deliver(target_label, payload) {
        AdoptionDelivery::Queued => true,
        AdoptionDelivery::Emit(payload) => {
            match app.emit_to(target_label, WINDOW_SESSIONS_ADOPT_EVENT, payload.clone()) {
                Ok(()) => false,
                Err(error) => {
                    tracing::warn!(%error, target = target_label, "Could not deliver an adoption request; queued it");
                    pending.requeue(target_label, payload);
                    true
                }
            }
        }
    };
    Ok(WindowMoveOutcome {
        target_label: target_label.to_string(),
        moved: plan.movable,
        skipped: plan.skipped,
        queued,
    })
}

/// Moves live terminal sessions to another window (`None` opens a new one).
///
/// The caller must already have detached its native surfaces; the sessions are never closed.
/// Keystrokes typed while a session is between windows are not buffered: the source stops
/// sending input once its surface is detached and the target does only after its attach.
#[tauri::command]
pub async fn cmd_window_move_sessions<R: Runtime>(
    app: AppHandle<R>,
    window: Window<R>,
    registry: State<'_, WindowRegistry>,
    pending: State<'_, PendingWindowAdoptions>,
    target_label: Option<String>,
    backend_session_ids: Vec<String>,
    workspace_id: Option<String>,
) -> Result<WindowMoveOutcome, IpcError> {
    let plan = plan_move(
        &registry,
        window.label(),
        target_label.as_deref(),
        &backend_session_ids,
    )?;
    let target = match target_label {
        Some(label) => label,
        None => create_app_window(&app, None).await?,
    };
    let attempt_generations: BTreeMap<String, u64> = app
        .try_state::<crate::native_terminal::surface_host::NativeTerminalSurfaceHostState>()
        .map(|host| {
            plan.movable
                .iter()
                .filter_map(|id| host.session_attempt_generation(id).map(|generation| (id.clone(), generation)))
                .collect()
        })
        .unwrap_or_default();
    apply_move(&app, &registry, &pending, &target, plan, workspace_id, attempt_generations)
}

/// Adoption requests queued for the calling window while it booted. A window calls this once its
/// `window_sessions_adopt` listener is registered; later requests arrive as events.
#[tauri::command]
pub fn cmd_window_take_pending_adoptions<R: Runtime>(
    window: Window<R>,
    pending: State<'_, PendingWindowAdoptions>,
) -> Vec<WindowSessionsAdoptPayload> {
    pending.take(window.label())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_file_sits_beside_the_primary_session_file() {
        let session = Path::new("/qa/sessions/dev/session_state.json");
        assert_eq!(
            windows_file_beside(session),
            PathBuf::from("/qa/sessions/dev").join(WINDOWS_FILE_NAME)
        );
    }

    fn entry(label: &str, x: f64) -> PersistedWindow {
        PersistedWindow {
            label: label.into(),
            x,
            y: 20.0,
            width: 1000.0,
            height: 700.0,
        }
    }

    #[test]
    fn new_labels_are_valid_unique_secondary_labels() {
        let first = new_window_label();
        let second = new_window_label();
        assert!(is_secondary_window_label(&first), "{first}");
        assert!(is_app_window_label(&first));
        assert_eq!(first.len(), SECONDARY_WINDOW_PREFIX.len() + 32);
        assert_ne!(first, second);
    }

    #[test]
    fn secondary_label_validation() {
        assert!(is_secondary_window_label("ws-test"));
        assert!(is_secondary_window_label("ws-0a1b_2-C"));
        for invalid in ["main", "ws-", "ws-a b", "ws-a/b", "browser-1", "WS-abc", ""] {
            assert!(!is_secondary_window_label(invalid), "{invalid}");
        }
    }

    #[test]
    fn parse_round_trips_written_entries() {
        let windows = vec![entry("ws-a", 10.0), entry("ws-b", -1200.5)];
        let json = serde_json::to_string(&windows).unwrap();
        assert!(json.contains("\"label\":\"ws-a\""));
        assert_eq!(parse_windows_file(&json), windows);
    }

    #[test]
    fn parse_tolerates_damage_and_drops_bad_entries() {
        assert!(parse_windows_file("").is_empty());
        assert!(parse_windows_file("{not json").is_empty());
        assert!(parse_windows_file("{\"label\":\"ws-a\"}").is_empty());
        let parsed = parse_windows_file(
            r#"[
                {"label":"ws-a","x":1,"y":2,"width":300,"height":200},
                {"label":"main","x":0,"y":0,"width":1280,"height":850},
                {"label":"ws-bad","x":0,"y":0,"width":0,"height":850},
                {"label":"ws-missing","x":0,"y":0},
                {"label":"ws-a","x":9,"y":9,"width":900,"height":900},
                "garbage",
                {"label":"ws-b","x":5,"y":6,"width":1300,"height":900,"extra":true}
            ]"#,
        );
        assert_eq!(
            parsed,
            vec![
                PersistedWindow {
                    label: "ws-a".into(),
                    x: 1.0,
                    y: 2.0,
                    width: MIN_WIDTH,
                    height: MIN_HEIGHT,
                },
                PersistedWindow {
                    label: "ws-b".into(),
                    x: 5.0,
                    y: 6.0,
                    width: 1300.0,
                    height: 900.0,
                },
            ]
        );
    }

    #[test]
    fn sanitize_rejects_non_finite_and_empty_bounds() {
        let ok = WindowBounds {
            x: 0.0,
            y: 0.0,
            width: 900.0,
            height: 650.0,
        };
        assert_eq!(sanitize_bounds(ok), Some(ok));
        assert_eq!(sanitize_bounds(WindowBounds { x: f64::NAN, ..ok }), None);
        assert_eq!(
            sanitize_bounds(WindowBounds {
                width: f64::INFINITY,
                ..ok
            }),
            None
        );
        assert_eq!(sanitize_bounds(WindowBounds { height: -1.0, ..ok }), None);
    }

    #[test]
    fn bounds_deserialize_from_camel_case_json() {
        let bounds: WindowBounds =
            serde_json::from_str(r#"{"x":1.5,"y":2,"width":900,"height":700}"#).unwrap();
        assert_eq!(
            bounds,
            WindowBounds {
                x: 1.5,
                y: 2.0,
                width: 900.0,
                height: 700.0,
            }
        );
    }

    #[test]
    fn merge_updates_appends_removes_and_keeps_torn_down_entries() {
        let existing = vec![entry("ws-a", 1.0), entry("ws-b", 2.0), entry("ws-c", 3.0)];
        let live = vec![entry("ws-a", 10.0), entry("ws-d", 4.0)];
        let closed: HashSet<String> = ["ws-b".to_string()].into();
        let merged = merge_windows(existing, &live, &closed);
        assert_eq!(
            merged,
            vec![entry("ws-a", 10.0), entry("ws-c", 3.0), entry("ws-d", 4.0)]
        );
    }

    #[test]
    fn merge_never_resurrects_a_closed_live_window() {
        let closed: HashSet<String> = ["ws-a".to_string()].into();
        assert!(merge_windows(vec![], &[entry("ws-a", 1.0)], &closed).is_empty());
    }

    #[test]
    fn file_round_trip_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join(WINDOWS_FILE_NAME);
        assert!(read_windows_file(&path).is_empty());
        let windows = vec![entry("ws-a", 1.0)];
        write_windows_file(&path, &windows).unwrap();
        assert_eq!(read_windows_file(&path), windows);
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn multi_window_gate_reads_only_explicit_opt_in() {
        assert!(multi_window_enabled_from(Some("1")));
        assert!(multi_window_enabled_from(Some(" true ")));
        assert!(!multi_window_enabled_from(None));
        assert!(!multi_window_enabled_from(Some("0")));
        assert!(!multi_window_enabled_from(Some("")));
    }

    type MockWindow = tauri::WebviewWindow<tauri::test::MockRuntime>;

    fn adopt(ids: &[&str]) -> WindowSessionsAdoptPayload {
        WindowSessionsAdoptPayload {
            backend_session_ids: ids.iter().map(|id| id.to_string()).collect(),
            activate: true,
            workspace_id: None,
            attempt_generations: BTreeMap::new(),
        }
    }

    fn two_window_app() -> (tauri::App<tauri::test::MockRuntime>, MockWindow, MockWindow) {
        let app = tauri::test::mock_app();
        let main = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
            .build()
            .expect("main window");
        let secondary =
            tauri::WebviewWindowBuilder::new(&app, "ws-test", tauri::WebviewUrl::default())
                .build()
                .expect("secondary window");
        (app, main, secondary)
    }

    fn record_adoptions(window: &MockWindow) -> std::sync::mpsc::Receiver<String> {
        use tauri::Listener;
        let (sender, receiver) = std::sync::mpsc::channel();
        window.listen(WINDOW_SESSIONS_ADOPT_EVENT, move |event| {
            let _ = sender.send(event.payload().to_owned());
        });
        receiver
    }

    #[test]
    fn pending_adoptions_queue_until_the_window_boots_then_emit_directly() {
        let pending = PendingWindowAdoptions::default();
        assert_eq!(pending.deliver("ws-a", adopt(&["s1"])), AdoptionDelivery::Queued);
        assert_eq!(pending.deliver("ws-a", adopt(&["s2"])), AdoptionDelivery::Queued);
        // Another window's queue is independent.
        assert_eq!(pending.deliver("ws-b", adopt(&["s3"])), AdoptionDelivery::Queued);

        assert_eq!(pending.take("ws-a"), vec![adopt(&["s1"]), adopt(&["s2"])]);
        assert!(pending.take("ws-a").is_empty(), "each request is delivered once");
        assert_eq!(
            pending.deliver("ws-a", adopt(&["s4"])),
            AdoptionDelivery::Emit(adopt(&["s4"]))
        );
        assert_eq!(pending.take("ws-b"), vec![adopt(&["s3"])]);
    }

    #[test]
    fn requeue_and_forget_reset_a_window() {
        let pending = PendingWindowAdoptions::default();
        assert!(pending.take("ws-a").is_empty());
        pending.requeue("ws-a", adopt(&["s1"]));
        assert_eq!(pending.deliver("ws-a", adopt(&["s2"])), AdoptionDelivery::Queued);
        pending.forget("ws-a");
        assert!(pending.take("ws-a").is_empty(), "a destroyed window keeps no queue");
        assert_eq!(
            pending.deliver("ws-a", adopt(&["s3"])),
            AdoptionDelivery::Emit(adopt(&["s3"])),
            "take after forget booted it again"
        );
        pending.forget("ws-a");
        assert_eq!(pending.deliver("ws-a", adopt(&["s4"])), AdoptionDelivery::Queued);
    }

    #[test]
    fn adopt_payload_serializes_camel_case_without_an_empty_workspace() {
        let json = serde_json::to_value(adopt(&["s1"])).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "backendSessionIds": ["s1"], "activate": true })
        );
        let with_workspace = WindowSessionsAdoptPayload {
            workspace_id: Some("w1".into()),
            ..adopt(&["s1"])
        };
        assert_eq!(serde_json::to_value(with_workspace).unwrap()["workspaceId"], "w1");
    }

    #[test]
    fn plan_move_keeps_owned_and_unowned_sessions_and_skips_foreign_ones() {
        let registry = WindowRegistry::default();
        registry.set_owner("mine", "main");
        registry.set_owner("theirs", "ws-other");
        let ids: Vec<String> = ["mine", "unowned", "theirs", "mine", ""]
            .iter()
            .map(|id| id.to_string())
            .collect();
        let plan = plan_move(&registry, "main", Some("ws-test"), &ids).unwrap();
        assert_eq!(plan.movable, vec!["mine", "unowned"]);
        assert_eq!(plan.skipped, vec!["theirs"]);
    }

    #[test]
    fn plan_move_rejects_bad_targets_and_empty_plans() {
        let registry = WindowRegistry::default();
        registry.set_owner("theirs", "ws-other");
        let ids = vec!["s1".to_string()];
        assert!(plan_move(&registry, "main", Some("browser-1"), &ids).is_err());
        assert!(plan_move(&registry, "main", Some("main"), &ids).is_err());
        assert!(plan_move(&registry, "main", None, &["theirs".to_string()]).is_err());
        assert!(plan_move(&registry, "main", None, &[]).is_err());
        assert!(plan_move(&registry, "main", None, &ids).is_ok());
    }

    #[test]
    fn move_to_a_booted_window_flips_ownership_and_emits_only_to_it() {
        let (app, main, secondary) = two_window_app();
        let registry = WindowRegistry::default();
        registry.set_owner("s1", "main");
        let pending = PendingWindowAdoptions::default();
        assert!(pending.take("ws-test").is_empty());
        let main_events = record_adoptions(&main);
        let ws_events = record_adoptions(&secondary);

        let plan = plan_move(&registry, "main", Some("ws-test"), &["s1".to_string()]).unwrap();
        let outcome = apply_move(
            app.handle(),
            &registry,
            &pending,
            "ws-test",
            plan,
            Some("w1".into()),
            BTreeMap::from([("s1".to_string(), 4)]),
        )
        .unwrap();

        assert_eq!(
            outcome,
            WindowMoveOutcome {
                target_label: "ws-test".into(),
                moved: vec!["s1".into()],
                skipped: vec![],
                queued: false,
            }
        );
        assert_eq!(registry.owner("s1").as_deref(), Some("ws-test"));
        let payload: serde_json::Value =
            serde_json::from_str(&ws_events.try_recv().expect("target is told to adopt")).unwrap();
        assert_eq!(
            payload,
            serde_json::json!({
                "backendSessionIds": ["s1"],
                "activate": true,
                "workspaceId": "w1",
                "attemptGenerations": { "s1": 4 },
            })
        );
        assert!(main_events.try_recv().is_err(), "the source window is not a target");
    }

    #[test]
    fn move_to_a_window_that_has_not_booted_queues_for_its_first_pull() {
        let (app, main, secondary) = two_window_app();
        let registry = WindowRegistry::default();
        let pending = PendingWindowAdoptions::default();
        let main_events = record_adoptions(&main);
        let ws_events = record_adoptions(&secondary);

        let plan = plan_move(&registry, "main", Some("ws-test"), &["s1".to_string()]).unwrap();
        let outcome =
            apply_move(app.handle(), &registry, &pending, "ws-test", plan, None, BTreeMap::new())
                .unwrap();

        assert!(outcome.queued);
        assert_eq!(registry.owner("s1").as_deref(), Some("ws-test"));
        assert!(ws_events.try_recv().is_err(), "nothing is emitted to a window that cannot listen yet");
        assert!(main_events.try_recv().is_err());
        assert_eq!(pending.take("ws-test"), vec![adopt(&["s1"])]);
    }

    #[test]
    fn move_to_a_closed_window_is_refused_without_changing_ownership() {
        let (app, _main, _secondary) = two_window_app();
        let registry = WindowRegistry::default();
        registry.set_owner("s1", "main");
        let pending = PendingWindowAdoptions::default();
        let plan = MovePlan {
            movable: vec!["s1".into()],
            skipped: vec![],
        };
        assert!(
            apply_move(app.handle(), &registry, &pending, "ws-gone", plan, None, BTreeMap::new())
                .is_err()
        );
        assert_eq!(registry.owner("s1").as_deref(), Some("main"));
        assert!(pending.take("ws-gone").is_empty());
    }

    #[test]
    fn window_list_puts_main_first_and_skips_non_app_windows() {
        let app = tauri::test::mock_app();
        for label in ["ws-b", "main", "ws-a", "preview-child"] {
            tauri::WebviewWindowBuilder::new(&app, label, tauri::WebviewUrl::default())
                .build()
                .expect("mock window");
        }
        assert_eq!(app_window_labels(app.handle()), vec!["main", "ws-a", "ws-b"]);
    }
}
