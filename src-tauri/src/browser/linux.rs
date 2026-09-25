#[cfg(target_os = "linux")]
pub mod implementation {
    use crate::browser::security::BrowserError;
    use crate::browser::LogicalRect;
    use gtk::prelude::*;
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    pub struct LinuxBrowserOverlay {
        pub overlay: gtk::Overlay,
        pub fixed: gtk::Fixed,
        pub webviews: HashMap<String, webkit2gtk::WebView>,
    }

    // SAFETY: GTK widgets are only accessed from the main thread via run_on_main_thread.
    unsafe impl Send for LinuxBrowserOverlay {}
    unsafe impl Sync for LinuxBrowserOverlay {}

    /// Overlay containers keyed by window label. Every window owns its own
    /// overlay/fixed pair, so a browser webview created in a second window can
    /// never be parented into the first window's overlay.
    ///
    /// Keying alone is only sound while the entry's window is alive, so every
    /// registration needs its counterpart `remove_overlay_for_window` on window
    /// destroy, and every lookup treats a dead window's entry as absent.
    static OVERLAY_CONTAINERS: OnceLock<Mutex<HashMap<String, LinuxBrowserOverlay>>> =
        OnceLock::new();

    fn get_overlay_containers() -> &'static Mutex<HashMap<String, LinuxBrowserOverlay>> {
        OVERLAY_CONTAINERS.get_or_init(|| Mutex::new(HashMap::new()))
    }

    /// Keying rule for the per-window overlay store: an overlay is only ever
    /// found under the label it was registered with, with no fallback to
    /// another window's container.
    fn lookup_by_label<'a, V>(
        containers: &'a HashMap<String, V>,
        window_label: &str,
    ) -> Option<&'a V> {
        containers.get(window_label)
    }

    /// Whether a registered container still belongs to a live window. GTK
    /// unparents a window's whole widget subtree when the window is destroyed,
    /// so an overlay whose parent is gone can only come from a dead window.
    fn overlay_is_live(container: &LinuxBrowserOverlay) -> bool {
        container.overlay.parent().is_some()
    }

    /// Keying rule plus liveness: an entry left behind by a destroyed window is
    /// reported as absent instead of being handed back, so a window label that
    /// is reused after its window closed can never return the previous overlay.
    fn lookup_live_by_label<'a, V>(
        containers: &'a HashMap<String, V>,
        window_label: &str,
        is_live: impl Fn(&V) -> bool,
    ) -> Option<&'a V> {
        lookup_by_label(containers, window_label).filter(|container| is_live(*container))
    }

    /// Drops every container whose window no longer exists, so a dead window's
    /// widgets can never answer a scan for a live browser.
    fn prune_dead_overlays(containers: &mut HashMap<String, LinuxBrowserOverlay>) {
        containers.retain(|_, container| overlay_is_live(container));
    }

    /// Takes the entry registered for `window_label` out of the map so the
    /// caller can drop its GTK widgets. Main thread only, like every other
    /// overlay accessor here.
    fn take_by_label<V>(containers: &mut HashMap<String, V>, window_label: &str) -> Option<V> {
        containers.remove(window_label)
    }

    /// The counterpart every registration needs: the store is keyed by window
    /// label, so without this call a destroyed window's overlay (and every
    /// webview it holds) is retained for the process lifetime, and a window that
    /// later reuses the label inherits it. Call it from the window's destroy
    /// hook, on the main thread.
    pub fn remove_overlay_for_window(window_label: &str) -> Result<bool, BrowserError> {
        let mut containers = get_overlay_containers().lock().unwrap();
        Ok(take_by_label(&mut containers, window_label).is_some())
    }

    pub fn ensure_overlay_initialized<R: tauri::Runtime>(
        window: &tauri::Window<R>,
    ) -> Result<(), BrowserError> {
        let window_label = window.label().to_string();
        let mut containers = get_overlay_containers().lock().unwrap();
        if lookup_live_by_label(&containers, &window_label, overlay_is_live).is_some() {
            return Ok(());
        }
        // The label is either free or holds the overlay of a destroyed window
        // that reused this label; a new window must not inherit that overlay.
        let _ = take_by_label(&mut containers, &window_label);

        let vbox = window
            .default_vbox()
            .map_err(|e| BrowserError::CreateFailed(format!("Failed to get default vbox: {e}")))?;

        // ASSUMPTION: `default_vbox` exposes no stable identity for the main
        // webview widget, so the overlay target is the vbox's first child.
        // Tauri packs the window's main webview as that first child; if a
        // future Tauri version prepends another widget, this must switch to an
        // identity-based lookup.
        let children = vbox.children();
        let main_widget = children.first().ok_or_else(|| {
            BrowserError::CreateFailed("default vbox has no children to overlay".into())
        })?;

        vbox.remove(main_widget);

        let overlay = gtk::Overlay::new();
        let fixed = gtk::Fixed::new();
        fixed.set_can_focus(false);

        overlay.add(main_widget);
        overlay.add_overlay(&fixed);
        overlay.set_overlay_pass_through(&fixed, true);

        vbox.pack_start(&overlay, true, true, 0);
        overlay.show_all();

        containers.insert(
            window_label,
            LinuxBrowserOverlay {
                overlay,
                fixed,
                webviews: HashMap::new(),
            },
        );

        Ok(())
    }

    pub fn attach_child_to_overlay<R: tauri::Runtime>(
        window: &tauri::Window<R>,
        browser_id: &str,
        child: &tauri::Webview<R>,
        bounds: &LogicalRect,
    ) -> Result<(), BrowserError> {
        ensure_overlay_initialized(window)?;

        let browser_id_owned = browser_id.to_string();
        let bounds_clone = bounds.clone();
        let window_label = window.label().to_string();

        child
            .with_webview(move |platform_webview| {
                let widget = platform_webview.inner().clone();
                if let Some(parent) = widget.parent() {
                    if let Ok(container) = parent.downcast::<gtk::Container>() {
                        container.remove(&widget);
                    }
                }

                let mut containers = get_overlay_containers().lock().unwrap();
                // A reused window label must not receive the dead window's overlay.
                let stale =
                    lookup_live_by_label(&containers, &window_label, overlay_is_live).is_none();
                if stale {
                    let _ = take_by_label(&mut containers, &window_label);
                }
                if let Some(container) = containers.get_mut(&window_label) {
                    let x = bounds_clone.x.round().max(0.0) as i32;
                    let y = bounds_clone.y.round().max(0.0) as i32;
                    let w = bounds_clone.width.round().max(1.0) as i32;
                    let h = bounds_clone.height.round().max(1.0) as i32;

                    widget.set_size_request(w, h);
                    container.fixed.put(&widget, x, y);
                    widget.show_all();
                    container.webviews.insert(browser_id_owned, widget);
                }
            })
            .map_err(|e| BrowserError::CreateFailed(format!("with_webview failed: {e}")))?;

        Ok(())
    }

    pub fn update_child_bounds(
        browser_id: &str,
        bounds: &LogicalRect,
    ) -> Result<bool, BrowserError> {
        let mut containers = get_overlay_containers().lock().unwrap();
        prune_dead_overlays(&mut containers);
        // Bounds updates carry no window label, so scan every window's
        // container. Browser ids are unique per session, so at most one
        // window can hold the widget.
        for container in containers.values_mut() {
            if let Some(widget) = container.webviews.get(browser_id) {
                let x = bounds.x.round().max(0.0) as i32;
                let y = bounds.y.round().max(0.0) as i32;
                let w = bounds.width.round().max(1.0) as i32;
                let h = bounds.height.round().max(1.0) as i32;

                widget.set_size_request(w, h);
                container.fixed.move_(widget, x, y);
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn set_child_visible(browser_id: &str, visible: bool) -> Result<bool, BrowserError> {
        let mut containers = get_overlay_containers().lock().unwrap();
        prune_dead_overlays(&mut containers);
        for container in containers.values_mut() {
            if let Some(widget) = container.webviews.get(browser_id) {
                if visible {
                    widget.show();
                } else {
                    widget.hide();
                }
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn detach_child(browser_id: &str) -> Result<bool, BrowserError> {
        let mut containers = get_overlay_containers().lock().unwrap();
        prune_dead_overlays(&mut containers);
        for container in containers.values_mut() {
            if let Some(widget) = container.webviews.remove(browser_id) {
                container.fixed.remove(&widget);
                return Ok(true);
            }
        }
        Ok(false)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn overlay_store_is_keyed_by_window_label() {
            // Stand-in for `LinuxBrowserOverlay`: the keying rule is independent
            // of the GTK types, which cannot be constructed off the main thread.
            let mut containers: HashMap<String, u32> = HashMap::new();
            assert_eq!(lookup_by_label(&containers, "main"), None);

            containers.insert("main".to_string(), 1);
            assert_eq!(lookup_by_label(&containers, "main"), Some(&1));

            // A second window registers its own container without displacing
            // the first window's entry.
            containers.insert("secondary".to_string(), 2);
            assert_eq!(lookup_by_label(&containers, "main"), Some(&1));
            assert_eq!(lookup_by_label(&containers, "secondary"), Some(&2));
            assert_eq!(lookup_by_label(&containers, "missing"), None);
        }

        #[test]
        fn removing_a_window_label_drops_only_that_overlay() {
            // Same stand-in as above: the removal rule is independent of the
            // GTK types, which cannot be constructed off the main thread.
            let mut containers: HashMap<String, u32> = HashMap::new();
            containers.insert("main".to_string(), 1);
            containers.insert("secondary".to_string(), 2);

            assert_eq!(take_by_label(&mut containers, "secondary"), Some(2));
            assert_eq!(lookup_by_label(&containers, "secondary"), None);
            assert_eq!(lookup_by_label(&containers, "main"), Some(&1));

            // Removing a label that is already gone is a no-op, not an error.
            assert_eq!(take_by_label(&mut containers, "secondary"), None);
            assert_eq!(lookup_by_label(&containers, "main"), Some(&1));
        }

        #[test]
        fn a_dead_windows_overlay_is_reported_as_absent() {
            let mut containers: HashMap<String, u32> = HashMap::new();
            containers.insert("main".to_string(), 1);
            // 2 stands in for a container whose window was destroyed.
            containers.insert("secondary".to_string(), 2);
            let is_live = |value: &u32| *value != 2;

            // A reused label must not be answered with the dead window's overlay.
            assert_eq!(
                lookup_live_by_label(&containers, "secondary", is_live),
                None
            );
            assert_eq!(lookup_live_by_label(&containers, "main", is_live), Some(&1));
            assert_eq!(lookup_live_by_label(&containers, "missing", is_live), None);
        }
    }
}
