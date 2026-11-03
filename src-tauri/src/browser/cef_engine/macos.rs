//! macOS: AppKit cannot host another process's view inside the Ferryx window, so the engine
//! keeps each tab's window borderless and ordered directly above the Ferryx window at the
//! pane's screen rectangle.

pub const ENGINE_EXE: &str = "ferryx_engine.app/Contents/MacOS/ferryx_engine";

pub fn configure(command: &mut std::process::Command, parent: isize) {
    command.arg(format!("--ferryx-parent-window={parent}"));
}

/// The AppKit window number of a Ferryx window, which the engine orders its windows above.
pub fn window_number<R: tauri::Runtime>(window: &tauri::Window<R>) -> Option<isize> {
    let raw = window.ns_window().ok()?;
    // SAFETY: tauri hands out the window's live NSWindow, and this runs while it exists.
    let ns_window = unsafe { (raw as *const objc2_app_kit::NSWindow).as_ref() }?;
    Some(ns_window.windowNumber())
}

/// Another app's window may have come between the Ferryx window and the engine's windows:
/// returns true so the engine puts its shown windows back above.
pub fn repaint(_parent: isize, _window: Option<isize>) -> bool {
    true
}
