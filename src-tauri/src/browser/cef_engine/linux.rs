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

#[link(name = "X11")]
unsafe extern "C" {
    fn XOpenDisplay(name: *const std::ffi::c_char) -> *mut std::ffi::c_void;
    fn XSetInputFocus(
        display: *mut std::ffi::c_void,
        focus: std::ffi::c_ulong,
        revert_to: std::ffi::c_int,
        time: std::ffi::c_ulong,
    ) -> std::ffi::c_int;
    fn XFlush(display: *mut std::ffi::c_void) -> std::ffi::c_int;
}

thread_local! {
    /// A connection of Ferryx's own for focus requests, opened on the main thread on first use;
    /// window ids are server-wide, so it can focus the toplevel GTK created.
    static FOCUS_DISPLAY: std::cell::Cell<*mut std::ffi::c_void> = const { std::cell::Cell::new(std::ptr::null_mut()) };
}

/// Moves X keyboard focus to the Ferryx toplevel `xid`, away from an engine child window.
pub fn take_x_focus(xid: isize) {
    const REVERT_TO_PARENT: std::ffi::c_int = 2;
    FOCUS_DISPLAY.with(|cell| {
        if cell.get().is_null() {
            // SAFETY: a null name opens $DISPLAY; the connection lives for the process.
            cell.set(unsafe { XOpenDisplay(std::ptr::null()) });
        }
        let display = cell.get();
        if display.is_null() {
            tracing::warn!("no X display to move keyboard focus back to Ferryx");
            return;
        }
        // SAFETY: `xid` is the live Ferryx toplevel on this server (`parent_xid`).
        unsafe {
            XSetInputFocus(display, xid as std::ffi::c_ulong, REVERT_TO_PARENT, 0);
            XFlush(display);
        }
    });
}
