//! Keeps an engine window over its Ferryx pane.
//!
//! Windows: the frameless window becomes a child window of the Ferryx window
//! (`--ferryx-parent-hwnd`) and is placed in pixels inside it. macOS cannot host another
//! process's view, so the window stays a borderless window ordered directly above the Ferryx
//! window (`--ferryx-parent-window`, its window number) at screen points.

use cef::*;
use ferryx_browser_engine_protocol::Rect as PaneRect;

fn parent_switch(name: &str) -> Option<isize> {
    let command_line = command_line_get_global()?;
    CefString::from(&command_line.switch_value(Some(&CefString::from(name))))
        .to_string()
        .parse::<isize>()
        .ok()
        .filter(|value| *value > 0)
}

#[cfg(target_os = "windows")]
mod imp {
    use super::*;
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetForegroundWindow, SetParent, SetWindowLongPtrW, SetWindowPos,
        GWL_EXSTYLE, GWL_STYLE, HWND_TOP, SWP_FRAMECHANGED, SWP_NOACTIVATE,
        SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WS_CAPTION, WS_CHILD,
        WS_CLIPSIBLINGS, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW, WS_POPUP, WS_THICKFRAME,
    };

    fn parent() -> Option<HWND> {
        parent_switch("ferryx-parent-hwnd").map(|value| value as HWND)
    }

    fn hwnd(window: &Window) -> HWND {
        window.window_handle().0 as HWND
    }

    pub fn embed(window: &Window) {
        let Some(parent) = parent() else { return };
        let hwnd = hwnd(window);
        unsafe {
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE)
                & !((WS_POPUP | WS_CAPTION | WS_THICKFRAME) as isize)
                | (WS_CHILD | WS_CLIPSIBLINGS) as isize;
            SetWindowLongPtrW(hwnd, GWL_STYLE, style);
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE)
                & !((WS_EX_APPWINDOW | WS_EX_TOOLWINDOW) as isize);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex);
            SetParent(hwnd, parent);
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
            // A child window left active takes no keys: give activation back to Ferryx, which
            // the click hook then focuses into the pane.
            SetForegroundWindow(parent);
        }
        focus_on_click::install();
    }

    pub fn handle(window: &Window) -> u64 {
        hwnd(window) as usize as u64
    }

    pub fn place(window: &Window, rect: PaneRect, visible: bool) {
        let hwnd = hwnd(window);
        unsafe { SetWindowPos(hwnd, HWND_TOP, rect.x, rect.y, rect.width, rect.height, SWP_NOACTIVATE) };
        // Showing and hiding go through CEF: a window Views still believes hidden never
        // paints, whatever Win32 shows.
        if !visible {
            window.hide();
        } else if window.is_visible() == 0 {
            window.show();
            // Showing activates the window, and a child window left active takes no keys: give
            // activation back to Ferryx, which the click hook then focuses into the pane.
            if let Some(parent) = parent() {
                unsafe { SetForegroundWindow(parent) };
            }
        }
    }

    pub fn raise(_window: &Window) {}

    /// A child window is never activated, so Chrome never takes keyboard focus for it on its
    /// own: clicks reach the page while keys keep going to Ferryx. A mouse hook on the UI thread
    /// gives the clicked pane window keyboard focus.
    mod focus_on_click {
        use super::parent;
        use std::sync::Once;
        use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
        use windows_sys::Win32::System::Threading::GetCurrentThreadId;
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetFocus, SetFocus};
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            CallNextHookEx, GetParent, IsChild, SetWindowsHookExW, HC_ACTION, MOUSEHOOKSTRUCT,
            WH_MOUSE, WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_RBUTTONDOWN,
        };

        /// The pane window (a direct child of the Ferryx window) that contains `hwnd`.
        fn pane_root(hwnd: HWND, ferryx: HWND) -> Option<HWND> {
            let mut current = hwnd;
            while !current.is_null() {
                let up = unsafe { GetParent(current) };
                if up == ferryx {
                    return Some(current);
                }
                current = up;
            }
            None
        }

        unsafe extern "system" fn on_mouse(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
            if code == HC_ACTION as i32
                && matches!(wparam as u32, WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN)
                && let Some(ferryx) = parent()
                && let Some(root) = pane_root(unsafe { (*(lparam as *const MOUSEHOOKSTRUCT)).hwnd }, ferryx)
            {
                let focus = unsafe { GetFocus() };
                if focus != root && unsafe { IsChild(root, focus) } == 0 {
                    unsafe { SetFocus(root) };
                }
            }
            unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
        }

        pub fn install() {
            static ONCE: Once = Once::new();
            ONCE.call_once(|| {
                let hook = unsafe {
                    SetWindowsHookExW(WH_MOUSE, Some(on_mouse), std::ptr::null_mut(), GetCurrentThreadId())
                };
                if hook.is_null() {
                    eprintln!("ferryx-engine: focus hook not installed");
                }
            });
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use objc2::msg_send;
    use objc2_app_kit::{NSView, NSWindow};

    fn ns_window(window: &Window) -> Option<objc2::rc::Retained<NSWindow>> {
        let view = window.window_handle() as *const NSView;
        // SAFETY: a Views window handle is its content NSView, alive while the window is; this
        // runs on the UI (main) thread.
        unsafe { view.as_ref() }.and_then(|view| view.window())
    }

    pub fn embed(_window: &Window) {}

    pub fn handle(_window: &Window) -> u64 {
        0
    }

    pub fn place(window: &Window, rect: PaneRect, visible: bool) {
        window.set_bounds(Some(&Rect { x: rect.x, y: rect.y, width: rect.width, height: rect.height }));
        if visible {
            // CEF's show() activates the engine app, which would take every click and key away
            // from Ferryx each time a pane moves: show once, then only reorder.
            if window.is_visible() == 0 {
                window.show();
            }
            raise(window);
        } else {
            window.hide();
        }
    }

    /// Orders the window directly above the Ferryx window without activating either.
    pub fn raise(window: &Window) {
        let (Some(parent), Some(ns)) = (parent_switch("ferryx-parent-window"), ns_window(window)) else {
            return;
        };
        // NSWindowAbove = 1.
        let _: () = unsafe { msg_send![&*ns, orderWindow: 1isize, relativeTo: parent] };
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod imp {
    use super::*;

    pub fn embed(_window: &Window) {
        let _ = parent_switch("ferryx-parent-window");
    }

    pub fn handle(_window: &Window) -> u64 {
        0
    }

    pub fn place(window: &Window, rect: PaneRect, visible: bool) {
        window.set_bounds(Some(&Rect { x: rect.x, y: rect.y, width: rect.width, height: rect.height }));
        if visible {
            window.show();
        } else {
            window.hide();
        }
    }

    pub fn raise(_window: &Window) {}
}

pub use imp::{embed, handle, place, raise};
