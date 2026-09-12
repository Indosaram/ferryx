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

    static OVERLAY_CONTAINER: OnceLock<Mutex<Option<LinuxBrowserOverlay>>> = OnceLock::new();

    fn get_overlay_container() -> &'static Mutex<Option<LinuxBrowserOverlay>> {
        OVERLAY_CONTAINER.get_or_init(|| Mutex::new(None))
    }

    pub fn ensure_overlay_initialized<R: tauri::Runtime>(
        window: &tauri::Window<R>,
    ) -> Result<(), BrowserError> {
        let mut guard = get_overlay_container().lock().unwrap();
        if guard.is_some() {
            return Ok(());
        }

        let vbox = window
            .default_vbox()
            .map_err(|e| BrowserError::CreateFailed(format!("Failed to get default vbox: {e}")))?;

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

        *guard = Some(LinuxBrowserOverlay {
            overlay,
            fixed,
            webviews: HashMap::new(),
        });

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

        child
            .with_webview(move |platform_webview| {
                let widget = platform_webview.inner().clone();
                if let Some(parent) = widget.parent() {
                    if let Ok(container) = parent.downcast::<gtk::Container>() {
                        container.remove(&widget);
                    }
                }

                let mut guard = get_overlay_container().lock().unwrap();
                if let Some(ref mut container) = *guard {
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
        let guard = get_overlay_container().lock().unwrap();
        if let Some(ref container) = *guard {
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
        let guard = get_overlay_container().lock().unwrap();
        if let Some(ref container) = *guard {
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
        let mut guard = get_overlay_container().lock().unwrap();
        if let Some(ref mut container) = *guard {
            if let Some(widget) = container.webviews.remove(browser_id) {
                container.fixed.remove(&widget);
                return Ok(true);
            }
        }
        Ok(false)
    }
}
