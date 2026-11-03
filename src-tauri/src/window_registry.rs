//! Which desktop window owns which terminal session.
//!
//! `main` is the first window; additional windows are labelled `ws-<id>`. A session is owned by
//! exactly one window: the window that created its native surface (or attached it through IPC).
//! Background paths that have only a session id (render scheduling, receipts, per-session events)
//! resolve their target window here instead of hardcoding `main`.
//!
//! Every helper reads the registry through `app.try_state::<WindowRegistry>()`. When the registry
//! is not managed (unit tests, headless harnesses) or a session has no live owner, the helpers fall
//! back to today's single-window behaviour: the primary window, or a broadcast emit.

use std::collections::HashMap;

use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime, Window};

pub const PRIMARY_WINDOW_LABEL: &str = "main";
pub const SECONDARY_WINDOW_PREFIX: &str = "ws-";

/// True for windows that host the Ferryx shell: `main` or a `ws-` secondary window.
pub fn is_app_window_label(label: &str) -> bool {
    label == PRIMARY_WINDOW_LABEL || label.starts_with(SECONDARY_WINDOW_PREFIX)
}

#[derive(Debug, Default)]
struct RegistryInner {
    /// session id -> owning window label.
    owners: HashMap<String, String>,
    /// Most recently focused app window.
    focused: Option<String>,
}

#[derive(Debug, Default)]
pub struct WindowRegistry {
    inner: Mutex<RegistryInner>,
}

impl WindowRegistry {
    pub fn set_owner(&self, session_id: &str, window_label: &str) {
        self.inner
            .lock()
            .owners
            .insert(session_id.to_string(), window_label.to_string());
    }

    pub fn clear_owner(&self, session_id: &str) {
        self.inner.lock().owners.remove(session_id);
    }

    pub fn owner(&self, session_id: &str) -> Option<String> {
        self.inner.lock().owners.get(session_id).cloned()
    }

    /// Sessions owned by `window_label`, sorted for a stable order.
    pub fn sessions_owned_by(&self, window_label: &str) -> Vec<String> {
        let mut sessions: Vec<String> = self
            .inner
            .lock()
            .owners
            .iter()
            .filter(|(_, owner)| owner.as_str() == window_label)
            .map(|(session_id, _)| session_id.clone())
            .collect();
        sessions.sort();
        sessions
    }

    pub fn note_focused(&self, window_label: &str) {
        self.inner.lock().focused = Some(window_label.to_string());
    }

    pub fn focused(&self) -> Option<String> {
        self.inner.lock().focused.clone()
    }

    /// Drops a closed window: returns the sessions it owned (sorted) and clears their ownership.
    pub fn forget_window(&self, window_label: &str) -> Vec<String> {
        let mut inner = self.inner.lock();
        let mut released: Vec<String> = inner
            .owners
            .iter()
            .filter(|(_, owner)| owner.as_str() == window_label)
            .map(|(session_id, _)| session_id.clone())
            .collect();
        for session_id in &released {
            inner.owners.remove(session_id);
        }
        if inner.focused.as_deref() == Some(window_label) {
            inner.focused = None;
        }
        released.sort();
        released
    }
}

/// Live app window with this label, if any.
fn live_app_window<R: Runtime>(app: &AppHandle<R>, label: &str) -> Option<Window<R>> {
    if !is_app_window_label(label) {
        return None;
    }
    app.get_window(label)
}

/// `main`, else the first live app window by label.
pub fn primary_window<R: Runtime>(app: &AppHandle<R>) -> Option<Window<R>> {
    if let Some(window) = app.get_window(PRIMARY_WINDOW_LABEL) {
        return Some(window);
    }
    let mut windows: Vec<(String, Window<R>)> = app
        .windows()
        .into_iter()
        .filter(|(label, _)| is_app_window_label(label))
        .collect();
    windows.sort_by(|a, b| a.0.cmp(&b.0));
    windows.into_iter().next().map(|(_, window)| window)
}

/// The registry's focused window if still alive, else any focused app window, else the primary.
pub fn focused_window<R: Runtime>(app: &AppHandle<R>) -> Option<Window<R>> {
    let noted = app
        .try_state::<WindowRegistry>()
        .and_then(|registry| registry.focused());
    if let Some(window) = noted.and_then(|label| live_app_window(app, &label)) {
        return Some(window);
    }
    let mut focused: Vec<(String, Window<R>)> = app
        .windows()
        .into_iter()
        .filter(|(label, window)| {
            is_app_window_label(label) && matches!(window.is_focused(), Ok(true))
        })
        .collect();
    focused.sort_by(|a, b| a.0.cmp(&b.0));
    if let Some((_, window)) = focused.into_iter().next() {
        return Some(window);
    }
    primary_window(app)
}

/// Live owner window of a session, when the registry is managed and knows one.
fn live_owner_window<R: Runtime>(app: &AppHandle<R>, session_id: &str) -> Option<Window<R>> {
    let label = app
        .try_state::<WindowRegistry>()
        .and_then(|registry| registry.owner(session_id))?;
    live_app_window(app, &label)
}

/// The window owning `session_id` if it is still alive, else the primary window.
pub fn window_for_session<R: Runtime>(app: &AppHandle<R>, session_id: &str) -> Option<Window<R>> {
    live_owner_window(app, session_id).or_else(|| primary_window(app))
}

/// Emits a session-scoped event to the session's owner window when it is known and alive;
/// otherwise broadcasts exactly like `app.emit`.
pub fn emit_to_session_owner<R: Runtime, S: Serialize + Clone>(
    app: &AppHandle<R>,
    session_id: &str,
    event: &str,
    payload: S,
) -> tauri::Result<()> {
    match live_owner_window(app, session_id) {
        Some(window) => app.emit_to(window.label(), event, payload),
        None => app.emit(event, payload),
    }
}

/// Records `window_label` as the owner of `session_id` when the registry is managed.
pub fn record_owner<R: Runtime>(app: &AppHandle<R>, session_id: &str, window_label: &str) {
    if let Some(registry) = app.try_state::<WindowRegistry>() {
        registry.set_owner(session_id, window_label);
    }
}

/// Clears the owner of `session_id` when the registry is managed.
pub fn release_owner<R: Runtime>(app: &AppHandle<R>, session_id: &str) {
    if let Some(registry) = app.try_state::<WindowRegistry>() {
        registry.clear_owner(session_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tauri::Listener;

    type MockWindow = Window<tauri::test::MockRuntime>;

    #[test]
    fn app_window_labels_are_main_and_ws_prefixed() {
        assert!(is_app_window_label("main"));
        assert!(is_app_window_label("ws-1234"));
        assert!(!is_app_window_label("browser-1"));
        assert!(!is_app_window_label("mainx"));
        assert!(!is_app_window_label("ws"));
    }

    #[test]
    fn owner_is_set_replaced_and_cleared() {
        let registry = WindowRegistry::default();
        assert_eq!(registry.owner("s1"), None);
        registry.set_owner("s1", "main");
        assert_eq!(registry.owner("s1").as_deref(), Some("main"));
        registry.set_owner("s1", "ws-a");
        assert_eq!(registry.owner("s1").as_deref(), Some("ws-a"));
        registry.clear_owner("s1");
        assert_eq!(registry.owner("s1"), None);
        registry.clear_owner("never-owned");
        assert_eq!(registry.owner("never-owned"), None);
    }

    #[test]
    fn sessions_owned_by_lists_only_that_window_sorted() {
        let registry = WindowRegistry::default();
        registry.set_owner("b", "ws-a");
        registry.set_owner("a", "ws-a");
        registry.set_owner("c", "main");
        assert_eq!(registry.sessions_owned_by("ws-a"), vec!["a", "b"]);
        assert_eq!(registry.sessions_owned_by("main"), vec!["c"]);
        assert!(registry.sessions_owned_by("ws-z").is_empty());
    }

    #[test]
    fn forget_window_releases_its_sessions_and_its_focus_only() {
        let registry = WindowRegistry::default();
        registry.set_owner("b", "ws-a");
        registry.set_owner("a", "ws-a");
        registry.set_owner("c", "main");
        registry.note_focused("ws-a");
        assert_eq!(registry.forget_window("ws-a"), vec!["a", "b"]);
        assert_eq!(registry.owner("a"), None);
        assert_eq!(registry.owner("b"), None);
        assert_eq!(registry.owner("c").as_deref(), Some("main"));
        assert_eq!(registry.focused(), None);

        registry.note_focused("main");
        assert!(registry.forget_window("ws-gone").is_empty());
        assert_eq!(registry.focused().as_deref(), Some("main"));
    }

    fn two_window_app(managed: bool) -> (tauri::App<tauri::test::MockRuntime>, MockWindow, MockWindow) {
        let mut builder = tauri::test::mock_builder();
        if managed {
            builder = builder.manage(WindowRegistry::default());
        }
        let app = builder
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let main = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
            .build()
            .unwrap();
        let secondary =
            tauri::WebviewWindowBuilder::new(&app, "ws-test", tauri::WebviewUrl::default())
                .build()
                .unwrap();
        let main = main.as_ref().window();
        let secondary = secondary.as_ref().window();
        (app, main, secondary)
    }

    #[test]
    fn window_for_session_resolves_the_live_owner_else_primary() {
        let (app, _main, _secondary) = two_window_app(true);
        let handle = app.handle();
        record_owner(handle, "s-ws", "ws-test");
        record_owner(handle, "s-gone", "ws-closed");

        assert_eq!(window_for_session(handle, "s-ws").unwrap().label(), "ws-test");
        assert_eq!(window_for_session(handle, "s-unknown").unwrap().label(), "main");
        assert_eq!(window_for_session(handle, "s-gone").unwrap().label(), "main");

        release_owner(handle, "s-ws");
        assert_eq!(window_for_session(handle, "s-ws").unwrap().label(), "main");
    }

    #[test]
    fn helpers_fall_back_to_primary_without_a_managed_registry() {
        let (app, _main, _secondary) = two_window_app(false);
        let handle = app.handle();
        record_owner(handle, "s1", "ws-test");
        assert_eq!(window_for_session(handle, "s1").unwrap().label(), "main");
        assert_eq!(primary_window(handle).unwrap().label(), "main");
    }

    #[test]
    fn primary_window_falls_back_to_first_app_window_when_main_is_absent() {
        let app = tauri::test::mock_builder()
            .manage(WindowRegistry::default())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        assert!(primary_window(app.handle()).is_none());
        for label in ["ws-b", "browser-x", "ws-a"] {
            tauri::WebviewWindowBuilder::new(&app, label, tauri::WebviewUrl::default())
                .build()
                .unwrap();
        }
        assert_eq!(primary_window(app.handle()).unwrap().label(), "ws-a");
    }

    #[test]
    fn focused_window_prefers_the_live_registry_focus() {
        let (app, _main, _secondary) = two_window_app(true);
        let handle = app.handle();
        let registry = handle.state::<WindowRegistry>();
        registry.note_focused("ws-test");
        assert_eq!(focused_window(handle).unwrap().label(), "ws-test");
        registry.note_focused("ws-closed");
        let fallback = focused_window(handle).expect("a live app window remains");
        assert!(is_app_window_label(fallback.label()));
    }

    fn record_payloads(window: &MockWindow, event: &str) -> std::sync::mpsc::Receiver<String> {
        let (sender, receiver) = std::sync::mpsc::channel::<String>();
        let sender = Arc::new(parking_lot::Mutex::new(sender));
        window.listen(event, move |event| {
            let _ = sender.lock().send(event.payload().to_string());
        });
        receiver
    }

    #[test]
    fn session_events_reach_only_the_owner_window() {
        let (app, main, secondary) = two_window_app(true);
        let handle = app.handle();
        record_owner(handle, "s-ws", "ws-test");
        let main_events = record_payloads(&main, "registry-test-event");
        let ws_events = record_payloads(&secondary, "registry-test-event");

        emit_to_session_owner(handle, "s-ws", "registry-test-event", "owned").unwrap();
        assert_eq!(ws_events.try_recv().unwrap(), "\"owned\"");
        assert!(main_events.try_recv().is_err(), "main must not receive ws-test's session event");

        // No known owner: unchanged broadcast.
        emit_to_session_owner(handle, "s-unknown", "registry-test-event", "all").unwrap();
        assert_eq!(ws_events.try_recv().unwrap(), "\"all\"");
        assert_eq!(main_events.try_recv().unwrap(), "\"all\"");
    }

    #[test]
    fn session_events_broadcast_without_a_managed_registry() {
        let (app, main, _secondary) = two_window_app(false);
        let main_events = record_payloads(&main, "registry-test-event");
        emit_to_session_owner(app.handle(), "s1", "registry-test-event", 7u32).unwrap();
        assert_eq!(main_events.try_recv().unwrap(), "7");
    }
}
