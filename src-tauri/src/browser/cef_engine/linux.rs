//! Linux (X11, and XWayland when Ferryx runs with `GDK_BACKEND=x11`): the engine reparents each
//! tab's window into the Ferryx toplevel X window and places it there in pixels, as on Windows.
//! A native Wayland Ferryx window has no X window id and another client's surface cannot be
//! placed inside it, so those sessions keep browser tabs on the system webview
//! (see [`parent_xid`] and `EngineStatus::Inactive`).

use raw_window_handle::{HasWindowHandle, RawWindowHandle};

pub const ENGINE_EXE: &str = "ferryx_engine";

/// Why an installed engine cannot run in this session, shown in Settings.
pub const WAYLAND_REASON: &str = "wayland-session";

pub fn configure(command: &mut std::process::Command, parent: isize) {
    command.arg(format!("--ferryx-parent-xid={parent}"));
}

/// The X window id of a Ferryx window, or `None` on a native Wayland window.
pub fn parent_xid<R: tauri::Runtime>(window: &tauri::Window<R>) -> Option<isize> {
    match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::Xlib(handle) => isize::try_from(handle.window).ok().filter(|xid| *xid > 0),
        RawWindowHandle::Xcb(handle) => Some(handle.window.get() as isize),
        _ => None,
    }
}

/// The webview can restack its own X subwindows over the engine's: returns true so the engine
/// raises its shown windows again.
pub fn repaint(_parent: isize, _window: Option<isize>) -> bool {
    true
}
