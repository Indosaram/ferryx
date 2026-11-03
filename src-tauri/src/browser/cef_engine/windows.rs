//! Windows: the engine makes each tab's window a child of the Ferryx main window and places
//! it there. Ferryx only keeps its own windows from painting over those children.

use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM};
use windows_sys::Win32::Graphics::Gdi::{RedrawWindow, RDW_ALLCHILDREN, RDW_ERASE, RDW_INVALIDATE};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, GetParent, GetWindowLongPtrW, GetWindowThreadProcessId, SetWindowLongPtrW,
    SetWindowPos, GWL_STYLE, SWP_ASYNCWINDOWPOS, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SWP_NOZORDER, WS_CLIPSIBLINGS,
};

pub const ENGINE_EXE: &str = "ferryx_engine.exe";

pub fn configure(command: &mut std::process::Command, parent: isize) {
    command.arg(format!("--ferryx-parent-hwnd={parent}"));
}

/// Lets Chromium's sandboxed child processes read the engine files. They run as AppContainer
/// processes, which a user-profile directory does not admit by default, and Chromium then
/// logs "Sandbox cannot access executable" and starts them with weaker isolation.
pub fn grant_sandbox_read(dir: &std::path::Path) -> Result<(), crate::browser::BrowserError> {
    use crate::browser::BrowserError;
    // ALL APPLICATION PACKAGES and ALL RESTRICTED APPLICATION PACKAGES.
    let status = std::process::Command::new("icacls")
        .arg(dir)
        .args(["/grant", "*S-1-15-2-1:(OI)(CI)(RX)", "*S-1-15-2-2:(OI)(CI)(RX)", "/T", "/Q"])
        .stdout(std::process::Stdio::null())
        .status()
        .map_err(|error| BrowserError::CreateFailed(format!("icacls: {error}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(BrowserError::CreateFailed(format!("icacls exited with {status}")))
    }
}

/// Repaints the app's own windows around the tab's window. The engine paints a page's first
/// frame after it attaches, under whatever the webview drew there before. Returns false: the
/// engine has nothing to redo on Windows.
pub fn repaint(parent: isize, window: Option<isize>) -> bool {
    clip_own_siblings(parent);
    if let Some(window) = window {
        // Async: the window belongs to another process, and a hung engine must not block the
        // Ferryx thread that resizes or hides panes.
        unsafe {
            SetWindowPos(
                window as HWND,
                std::ptr::null_mut(),
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
            );
        }
    }
    false
}

unsafe extern "system" fn clip_if_own(window: HWND, lparam: LPARAM) -> BOOL {
    let parent = lparam as HWND;
    let mut owner = 0u32;
    unsafe { GetWindowThreadProcessId(window, &mut owner) };
    if owner == std::process::id() && unsafe { GetParent(window) } == parent {
        let style = unsafe { GetWindowLongPtrW(window, GWL_STYLE) };
        // A style change takes effect only after a frame change, and the area the webview
        // already painted over the engine window must be drawn again.
        unsafe {
            if style & WS_CLIPSIBLINGS as isize == 0 {
                SetWindowLongPtrW(window, GWL_STYLE, style | WS_CLIPSIBLINGS as isize);
                SetWindowPos(
                    window,
                    std::ptr::null_mut(),
                    0,
                    0,
                    0,
                    0,
                    SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            RedrawWindow(window, std::ptr::null(), std::ptr::null_mut(), RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN);
        }
    }
    1
}

/// The app's own webview is a sibling of the engine windows and, without `WS_CLIPSIBLINGS`,
/// paints over them, so the page would stay hidden behind the UI.
fn clip_own_siblings(parent: isize) {
    unsafe { EnumChildWindows(parent as HWND, Some(clip_if_own), parent as LPARAM) };
}
