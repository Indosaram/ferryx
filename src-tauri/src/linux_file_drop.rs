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
            chromium_tab_at(&app_handle, x, y).is_some()
        });
    }

    let drop_handle = app_handle.clone();
    gtk_window.connect_drag_data_received(move |_widget, _context, x, y, selection, _info, _time| {
        let Some(paths) = uri_list_paths(selection) else {
            return;
        };
        let Some((browser_id, point)) = chromium_tab_at(&drop_handle, x, y) else {
            tracing::debug!("file drop over no Chromium tab was left unconsumed");
            return;
        };
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

/// Finds the visible Chromium tab under a GTK widget point, returning it with tab-relative
/// CSS-pixel coordinates.
///
/// `BrowserManager::set_bounds` stores each tab's logical rect in window coordinates, the same
/// space GTK reports drag coordinates in, so this is a plain hit test; the engine applies its own
/// child-window offset when it positions the surface.
fn chromium_tab_at<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    x: i32,
    y: i32,
) -> Option<(String, (f64, f64))> {
    let manager = app_handle.try_state::<std::sync::Arc<crate::browser::BrowserManager>>()?;
    let point_x = f64::from(x);
    let point_y = f64::from(y);
    manager
        .list_sessions()
        .into_iter()
        .filter(|session| session.visible && session.engine == crate::browser::BrowserEngine::Chromium)
        .find_map(|session| {
            let bounds = manager.get_bounds(&session.browser_id).ok().flatten()?;
            let inside = point_x >= bounds.x
                && point_x < bounds.x + bounds.width
                && point_y >= bounds.y
                && point_y < bounds.y + bounds.height;
            inside.then(|| {
                (
                    session.browser_id,
                    (point_x - bounds.x, point_y - bounds.y),
                )
            })
        })
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

