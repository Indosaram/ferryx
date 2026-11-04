//! ferryx:// links (todo 20). The OS hands a link to the app in three ways: macOS sends an
//! Apple Event (`RunEvent::Opened`, handled by tauri-plugin-deep-link), a cold start on Windows
//! and Linux passes the link as the only argument, and a link opened while the app runs starts a
//! second process whose argv tauri-plugin-single-instance forwards to the running one. All three
//! end in the deep-link plugin's `on_open_url`, which queues the link here and tells the primary
//! window. The webview drains the queue, so a link that arrives before the UI listens is kept.
//! Routing and every confirmation happen in the UI (ui/src/lib/deepLinks.ts); this side never acts
//! on a link.

use std::sync::Mutex;

use tauri::{Emitter, Manager, Runtime};

/// Scheme registered in tauri.conf.json `plugins.deep-link.desktop.schemes` and the MSIX manifest.
pub const DEEP_LINK_SCHEME: &str = "ferryx";
/// Emitted to the primary window when links are queued; the payload is empty.
pub const DEEP_LINK_RECEIVED_EVENT: &str = "deep_link_received";
/// A queue longer than this drops the oldest links; a burst of links is not a user action.
const MAX_PENDING_LINKS: usize = 16;
/// Longer links are not ferryx routes; they are dropped before they reach the webview.
const MAX_LINK_LENGTH: usize = 2048;

/// Links received but not yet taken by the webview.
#[derive(Default)]
pub struct PendingDeepLinks(Mutex<Vec<String>>);

impl PendingDeepLinks {
    fn push_all(&self, links: Vec<String>) {
        let mut pending = self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        pending.extend(links);
        let excess = pending.len().saturating_sub(MAX_PENDING_LINKS);
        pending.drain(..excess);
    }

    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()))
    }
}

/// The ferryx:// links among `urls`, in order. Other schemes and oversized links are dropped.
pub fn ferryx_links<'a>(urls: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    urls.into_iter()
        .filter(|url| url.len() <= MAX_LINK_LENGTH && is_ferryx_link(url))
        .map(str::to_owned)
        .collect()
}

/// Whether `arg` is a ferryx:// link (scheme compared case-insensitively, as URL schemes are).
pub fn is_ferryx_link(arg: &str) -> bool {
    arg.get(..DEEP_LINK_SCHEME.len())
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case(DEEP_LINK_SCHEME))
        && arg.get(DEEP_LINK_SCHEME.len()..DEEP_LINK_SCHEME.len() + 3) == Some("://")
}

/// Whether the single-instance plugin may run. Isolated QA profiles (FERRYX_RUNTIME_DIR or
/// FERRYX_DATA_DIR set) run next to the installed app and must not be folded into it. On Linux the
/// plugin needs a session D-Bus and panics without one, so a headless session skips it.
pub fn single_instance_enabled() -> bool {
    let isolated = ["FERRYX_RUNTIME_DIR", "FERRYX_DATA_DIR"]
        .iter()
        .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()));
    !isolated && session_bus_available()
}

#[cfg(target_os = "linux")]
fn session_bus_available() -> bool {
    std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some_and(|value| !value.is_empty())
        || std::env::var_os("XDG_RUNTIME_DIR")
            .is_some_and(|dir| std::path::Path::new(&dir).join("bus").exists())
}

#[cfg(not(target_os = "linux"))]
fn session_bus_available() -> bool {
    true
}

/// Brings the primary window forward; a second launch (by link or by hand) lands here.
pub fn focus_primary_window<R: Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(window) = app.get_webview_window(crate::window_registry::PRIMARY_WINDOW_LABEL) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Wires the deep-link plugin to the pending queue. Runs in `setup`, after the plugin is set up.
pub fn install<R: Runtime>(app: &tauri::App<R>) {
    use tauri_plugin_deep_link::DeepLinkExt;

    app.manage(PendingDeepLinks::default());
    // An AppImage is not installed, so nothing registered the scheme with the desktop; register it
    // for this AppImage path on every start (the path changes when the file moves).
    #[cfg(target_os = "linux")]
    if std::env::var_os("APPIMAGE").is_some() {
        if let Err(error) = app.deep_link().register_all() {
            tracing::warn!(%error, "Could not register ferryx:// links for this AppImage");
        }
    }
    let handle = app.handle().clone();
    app.deep_link().on_open_url(move |event| {
        let urls: Vec<String> = event.urls().iter().map(|url| url.to_string()).collect();
        queue_links(&handle, ferryx_links(urls.iter().map(String::as_str)));
    });
    // A cold start by link: the plugin read argv (Windows, Linux) before this handler existed.
    match app.deep_link().get_current() {
        Ok(Some(urls)) => {
            let urls: Vec<String> = urls.iter().map(|url| url.to_string()).collect();
            queue_links(app.handle(), ferryx_links(urls.iter().map(String::as_str)));
        }
        Ok(None) => {}
        Err(error) => tracing::warn!(%error, "Could not read the launch deep link"),
    }
}

fn queue_links<R: Runtime>(app: &tauri::AppHandle<R>, links: Vec<String>) {
    if links.is_empty() {
        return;
    }
    tracing::info!(count = links.len(), "Received ferryx:// links");
    if let Some(pending) = app.try_state::<PendingDeepLinks>() {
        pending.push_all(links);
    }
    focus_primary_window(app);
    if let Err(error) = app.emit_to(
        crate::window_registry::PRIMARY_WINDOW_LABEL,
        DEEP_LINK_RECEIVED_EVENT,
        (),
    ) {
        tracing::warn!(?error, "Could not notify the primary window of a deep link");
    }
}

/// Returns and clears the queued ferryx:// links, oldest first.
#[tauri::command]
pub fn cmd_deep_link_take_pending(pending: tauri::State<'_, PendingDeepLinks>) -> Vec<String> {
    pending.take()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_ferryx_links_in_order() {
        let links = ferryx_links([
            "ferryx://settings/remote",
            "https://ferryx.dev/",
            "FERRYX://onboarding/agents",
            "ferryx:/settings",
            "ferry://open",
            "",
        ]);
        assert_eq!(links, vec!["ferryx://settings/remote", "FERRYX://onboarding/agents"]);
    }

    #[test]
    fn drops_oversized_links() {
        let long = format!("ferryx://open?workspace={}", "a".repeat(MAX_LINK_LENGTH));
        assert!(ferryx_links([long.as_str()]).is_empty());
    }

    #[test]
    fn non_ascii_prefix_is_not_a_link_and_does_not_panic() {
        assert!(!is_ferryx_link("ferryé://x"));
        assert!(!is_ferryx_link("ferréé://x"));
        assert!(!is_ferryx_link("ferr"));
    }

    #[test]
    fn pending_queue_is_bounded_and_drained_once() {
        let pending = PendingDeepLinks::default();
        pending.push_all((0..MAX_PENDING_LINKS + 4).map(|i| format!("ferryx://open?workspace={i}")).collect());
        let taken = pending.take();
        assert_eq!(taken.len(), MAX_PENDING_LINKS);
        assert_eq!(taken[0], "ferryx://open?workspace=4");
        assert!(pending.take().is_empty());
    }
}
