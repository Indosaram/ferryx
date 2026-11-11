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

use std::collections::{BTreeSet, HashMap};

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
    /// session id -> every window presenting it (owner included). Session events reach all of them.
    viewers: HashMap<String, BTreeSet<String>>,
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

    /// Adds `window_label` as a viewer of `session_id`; the first viewer also becomes the owner.
    pub fn add_viewer(&self, session_id: &str, window_label: &str) {
        let mut inner = self.inner.lock();
        inner
            .viewers
            .entry(session_id.to_string())
            .or_default()
            .insert(window_label.to_string());
        inner
            .owners
            .entry(session_id.to_string())
            .or_insert_with(|| window_label.to_string());
    }

    /// Removes one viewer. When it owned the session, ownership moves to the first remaining
    /// viewer; with no viewer left the owner is kept (the session is merely backgrounded).
    pub fn remove_viewer(&self, session_id: &str, window_label: &str) {
        let mut inner = self.inner.lock();
        let next = match inner.viewers.get_mut(session_id) {
            Some(viewers) => {
                viewers.remove(window_label);
                viewers.iter().next().cloned()
            }
            None => None,
        };
        if inner.viewers.get(session_id).is_some_and(BTreeSet::is_empty) {
            inner.viewers.remove(session_id);
        }
        if let Some(next) = next {
            if inner.owners.get(session_id).map(String::as_str) == Some(window_label) {
                inner.owners.insert(session_id.to_string(), next);
            }
        }
    }

    /// Windows presenting `session_id` (owner included), sorted.
    pub fn viewers_of(&self, session_id: &str) -> Vec<String> {
        let inner = self.inner.lock();
        let mut labels: BTreeSet<String> =
            inner.viewers.get(session_id).cloned().unwrap_or_default();
        if let Some(owner) = inner.owners.get(session_id) {
            labels.insert(owner.clone());
        }
        labels.into_iter().collect()
    }

    pub fn clear_owner(&self, session_id: &str) {
        let mut inner = self.inner.lock();
        inner.owners.remove(session_id);
        inner.viewers.remove(session_id);
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

    /// Drops a closed window. Sessions it owned that another window still presents are handed to
    /// that window (ownership transfer, never reported). Returns only the sessions left with no
    /// viewer (sorted) - those are the orphans another window must adopt - and clears their owner.
    pub fn forget_window(&self, window_label: &str) -> Vec<String> {
        let mut inner = self.inner.lock();
        for viewers in inner.viewers.values_mut() {
            viewers.remove(window_label);
        }
        inner.viewers.retain(|_, viewers| !viewers.is_empty());
        let owned: Vec<String> = inner
            .owners
            .iter()
            .filter(|(_, owner)| owner.as_str() == window_label)
            .map(|(session_id, _)| session_id.clone())
            .collect();
        let mut released = Vec::new();
        for session_id in owned {
            let survivor = inner
                .viewers
                .get(&session_id)
                .and_then(|viewers| viewers.iter().next().cloned());
            match survivor {
                Some(survivor) => {
                    inner.owners.insert(session_id, survivor);
                }
                None => {
                    inner.owners.remove(&session_id);
                    released.push(session_id);
                }
            }
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

/// Emits a session-scoped event to every live window presenting the session (owner and viewers);
/// when none is known and alive, broadcasts exactly like `app.emit`.
pub fn emit_to_session_owner<R: Runtime, S: Serialize + Clone>(
    app: &AppHandle<R>,
    session_id: &str,
    event: &str,
    payload: S,
) -> tauri::Result<()> {
    let labels: Vec<String> = app
        .try_state::<WindowRegistry>()
        .map(|registry| registry.viewers_of(session_id))
        .unwrap_or_default()
        .into_iter()
        .filter(|label| live_app_window(app, label).is_some())
        .collect();
    if labels.is_empty() {
        return app.emit(event, payload);
    }
    for label in labels {
        app.emit_to(label.as_str(), event, payload.clone())?;
    }
    Ok(())
}

/// Records `window_label` as a viewer of `session_id` (owner if it has none) when the registry
/// is managed.
pub fn record_viewer<R: Runtime>(app: &AppHandle<R>, session_id: &str, window_label: &str) {
    if let Some(registry) = app.try_state::<WindowRegistry>() {
        registry.add_viewer(session_id, window_label);
    }
}

/// Removes `window_label` from the viewers of `session_id` when the registry is managed.
pub fn forget_viewer<R: Runtime>(app: &AppHandle<R>, session_id: &str, window_label: &str) {
    if let Some(registry) = app.try_state::<WindowRegistry>() {
        registry.remove_viewer(session_id, window_label);
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

    #[test]
    fn session_events_reach_every_viewer_window_once() {
        let (app, main, secondary) = two_window_app(true);
        let handle = app.handle();
        record_viewer(handle, "s-shared", "main");
        record_viewer(handle, "s-shared", "ws-test");
        let main_events = record_payloads(&main, "registry-test-event");
        let ws_events = record_payloads(&secondary, "registry-test-event");

        emit_to_session_owner(handle, "s-shared", "registry-test-event", "both").unwrap();
        assert_eq!(main_events.try_recv().unwrap(), "\"both\"");
        assert_eq!(ws_events.try_recv().unwrap(), "\"both\"");
        assert!(main_events.try_recv().is_err(), "one delivery per window");
        assert!(ws_events.try_recv().is_err(), "one delivery per window");
    }

    #[test]
    fn first_viewer_owns_and_ownership_transfers_on_viewer_removal() {
        let registry = WindowRegistry::default();
        registry.add_viewer("s", "main");
        registry.add_viewer("s", "ws-b");
        assert_eq!(registry.owner("s").as_deref(), Some("main"));
        assert_eq!(registry.viewers_of("s"), vec!["main", "ws-b"]);
        registry.remove_viewer("s", "main");
        assert_eq!(registry.owner("s").as_deref(), Some("ws-b"));
        assert_eq!(registry.viewers_of("s"), vec!["ws-b"]);
    }

    #[test]
    fn closing_a_window_hands_shared_sessions_to_a_surviving_viewer() {
        let registry = WindowRegistry::default();
        registry.add_viewer("shared", "main");
        registry.add_viewer("shared", "ws-clone");
        registry.add_viewer("solo", "main");
        // Only the session no other window presents is orphaned.
        assert_eq!(registry.forget_window("main"), vec!["solo"]);
        assert_eq!(registry.owner("shared").as_deref(), Some("ws-clone"));
        assert_eq!(registry.owner("solo"), None);
        assert_eq!(registry.viewers_of("shared"), vec!["ws-clone"]);
        // Closing the clone instead leaves the original as the owner.
        let registry = WindowRegistry::default();
        registry.add_viewer("shared", "main");
        registry.add_viewer("shared", "ws-clone");
        assert!(registry.forget_window("ws-clone").is_empty());
        assert_eq!(registry.owner("shared").as_deref(), Some("main"));
    }
}
