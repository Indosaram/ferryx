//! Window-level file drag destination for Linux (GTK/X11).
//!
//! # Why this exists
//!
//! Nothing in the stack accepts a file drop on Linux. `src-tauri/src/lib.rs` keeps the entire
//! `tauri::WindowEvent::DragDrop` handler behind `#[cfg(target_os = "macos")]`, and the drop
//! destination is installed only by `install_macos_file_drop_destination`. A GTK drag source
//! therefore reports `drag-failed no-target`: no window of ours ever advertises acceptance of
//! `text/uri-list`, so the drop aborts before any delivery is attempted.
//!
//! This module installs a `text/uri-list` destination on the app window so a file manager drag
//! is accepted, resolves the dropped URIs to paths, and hands them to whichever Chromium tab is
//! under the pointer through the existing engine entry point
//! (`crate::browser::cef_engine::drop_files`).
//!
//! The system WebKit half stays a no-op on purpose: WebKitGTK withholds `File` objects from
//! `text/uri-list` drops on non-Cocoa ports (WebKit 2.52.6 `DataTransfer.h:124-132` gates
//! `allowsFileAccess()` to `PLATFORM(COCOA)`), so delivering there would change nothing. A drop
//! over a non-Chromium area is left unconsumed.

use crate::system_file_drop::file_drop_script;
use gtk::gdk;
use gtk::prelude::*;
use tauri::Manager;

/// The XDND target a file manager advertises for a local file drag.
const URI_LIST: &str = "text/uri-list";

/// Installs the drag destination on `window`, routing drops to Chromium tabs by pointer position.
///
/// Must run on the GTK main thread.
pub fn install<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) -> tauri::Result<()> {
    let gtk_window = window.gtk_window()?;
    let app_handle = window.app_handle().clone();

    gtk_window.drag_dest_set(
        gtk::DestDefaults::ALL,
        &[gtk::TargetEntry::new(
            URI_LIST,
            gtk::TargetFlags::OTHER_APP,
            0,
        )],
        gdk::DragAction::COPY,
    );

    // A drop is only accepted where it can actually be routed, so a drag over anything that is
    // not a visible Chromium tab falls through instead of being swallowed.
    {
        let app_handle = app_handle.clone();
        gtk_window.connect_drag_motion(move |_widget, _context, x, y, _time| {
            tab_at(&app_handle, x, y).is_some()
        });
    }

    let drop_handle = app_handle.clone();
    gtk_window.connect_drag_data_received(move |_widget, _context, x, y, selection, _info, _time| {
        let Some(paths) = uri_list_paths(selection) else {
            return;
        };
        let Some((browser_id, engine, point)) = tab_at(&drop_handle, x, y) else {
            tracing::debug!("file drop over no browser tab was left unconsumed");
            return;
        };
        if engine == crate::browser::BrowserEngine::System {
            // The system webview is a child widget placed at (origin) and `point` is already
            // relative to it, so the page's elementFromPoint takes the tab-local point. Adding
            // the origin would land the drop in the wrong element (and off the pane on a tab
            // that does not start at the window origin).
            deliver_to_system_tab(&drop_handle, &browser_id, &paths, point);
            return;
        }
        tauri::async_runtime::spawn(async move {
            let outcomes =
                crate::browser::cef_engine::drop_files(&browser_id, &paths, point.0, point.1).await;
            let failed = outcomes.iter().filter(|(_, result)| result.is_err()).count();
            if failed == 0 {
                tracing::debug!(
                    browser_id,
                    files = paths.len(),
                    "delivered a file drop to the Chromium tab"
                );
            } else {
                tracing::warn!(
                    browser_id,
                    failed,
                    files = paths.len(),
                    "the Chromium engine refused part of a file drop"
                );
            }
        });
    });

    Ok(())
}

/// Resolves a `text/uri-list` selection to local paths, dropping every non-`file:` URI.
fn uri_list_paths(selection: &gtk::SelectionData) -> Option<Vec<String>> {
    parse_uri_list(&selection.data())
}

/// Parses the body of a `text/uri-list` selection into local paths.
///
/// Per RFC 2483 the payload is CRLF-separated and lines starting with `#` are comments; anything
/// that is not a local `file:` URI is skipped rather than failing the whole drop.
fn parse_uri_list(raw: &[u8]) -> Option<Vec<String>> {
    let text = std::str::from_utf8(raw).ok()?;
    let paths: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            gtk::glib::filename_from_uri(line)
                .ok()
                .map(|(path, _hostname)| path.to_string_lossy().into_owned())
        })
        .collect();
    (!paths.is_empty()).then_some(paths)
}

/// Finds the visible tab under a GTK widget point, returning it with tab-relative CSS-pixel
/// coordinates.
///
/// `BrowserManager::set_bounds` stores each tab's logical rect in window coordinates, the same
/// space GTK reports drag coordinates in, so this is a plain hit test; the engine applies its own
/// child-window offset when it positions the surface.
fn tab_at<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    x: i32,
    y: i32,
) -> Option<(String, crate::browser::BrowserEngine, (f64, f64))> {
    let manager = app_handle.try_state::<std::sync::Arc<crate::browser::BrowserManager>>()?;
    let point_x = f64::from(x);
    let point_y = f64::from(y);
    manager
        .list_sessions()
        .into_iter()
        .filter(|session| session.visible)
        .find_map(|session| {
            let bounds = manager.get_bounds(&session.browser_id).ok().flatten()?;
            let inside = point_x >= bounds.x
                && point_x < bounds.x + bounds.width
                && point_y >= bounds.y
                && point_y < bounds.y + bounds.height;
            // Tab-relative, which is what both the Chromium drop protocol and the system
            // webview's elementFromPoint expect.
            inside.then_some((
                session.browser_id,
                session.engine,
                (point_x - bounds.x, point_y - bounds.y),
            ))
        })
}

/// Hands a drop to a system-webview tab by running a script in that tab's own WebView.
///
/// WebKitGTK refuses to build `File` objects from a `text/uri-list` drag on non-Cocoa ports
/// (WebKit `DataTransfer.h` gates `allowsFileAccess()` to `PLATFORM(COCOA)`), so the page's own
/// `drop` handler can never see the files. The page still gets its normal event sequence: the
/// script synthesises the `DragEvent`s with a `DataTransfer` carrying the dropped paths as file
/// entries, so a page that reads `dataTransfer.files` works exactly as it does for a Chromium tab.
fn deliver_to_system_tab<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    browser_id: &str,
    paths: &[String],
    point: (f64, f64),
) {
    let Some(manager) =
        app_handle.try_state::<std::sync::Arc<crate::browser::BrowserManager>>()
    else {
        return;
    };
    let Ok(state) = manager.get_state(browser_id) else {
        return;
    };
    let webview_label = state.webview_label.clone();
    // A browser tab is a child webview added to the main window, so it is not a window:
    // look it up as a webview or the lookup always misses.
    let Some(webview) = app_handle.get_webview(webview_label.as_str()) else {
        tracing::warn!(browser_id, %webview_label, "no webview for the dropped-on tab");
        return;
    };
    let browser_id = browser_id.to_string();
    let script = file_drop_script(paths, point);
    tauri::async_runtime::spawn(async move {
        if let Err(error) = crate::ipc::browser::eval_webview(webview, script).await {
            tracing::warn!(browser_id, %error, "the page refused a file drop");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::parse_uri_list;

    #[test]
    fn a_uri_list_resolves_local_files_and_skips_comments() {
        let raw = b"# a comment\r\nfile:///tmp/one.html\r\nfile:///tmp/two.png\r\n";
        assert_eq!(
            parse_uri_list(raw),
            Some(vec![
                "/tmp/one.html".to_string(),
                "/tmp/two.png".to_string()
            ])
        );
    }

    #[test]
    fn a_non_file_uri_does_not_fail_the_drop() {
        let raw = b"https://example.com/page\r\nfile:///tmp/planted.txt\r\n";
        assert_eq!(
            parse_uri_list(raw),
            Some(vec!["/tmp/planted.txt".to_string()])
        );
    }

    #[test]
    fn a_list_without_local_files_is_not_a_drop() {
        assert_eq!(parse_uri_list(b"https://example.com/only\r\n"), None);
        assert_eq!(parse_uri_list(b""), None);
        assert_eq!(parse_uri_list(b"# only a comment\r\n"), None);
    }
}

