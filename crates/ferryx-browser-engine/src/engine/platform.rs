//! Keeps an engine window over its Ferryx pane.
//!
//! Windows: the frameless window becomes a child window of the Ferryx window
//! (`--ferryx-parent-hwnd`) and is placed in pixels inside it. macOS cannot host another
//! process's view, so the window stays a borderless window ordered directly above the Ferryx
//! window (`--ferryx-parent-window`, its window number) at screen points.
//! Linux (X11, including XWayland): the window is reparented into the Ferryx toplevel X window
//! (`--ferryx-parent-xid`) and placed in pixels inside it, like Windows. A native Wayland Ferryx
//! window has no X id, so Ferryx keeps those tabs on the system webview and never starts the
//! engine with a parent.

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
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus;
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

    /// A child window takes keys only once it has focus (see `focus_on_click`).
    pub fn focus(window: &Window) {
        unsafe { SetFocus(hwnd(window)) };
    }

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

    pub fn focus(_window: &Window) {}
}

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use std::cell::Cell;
    use std::ffi::{c_char, c_int, c_uint, c_ulong, c_void};

    #[link(name = "X11")]
    unsafe extern "C" {
        fn XOpenDisplay(name: *const c_char) -> *mut c_void;
        fn XReparentWindow(display: *mut c_void, window: c_ulong, parent: c_ulong, x: c_int, y: c_int) -> c_int;
        fn XMoveResizeWindow(
            display: *mut c_void,
            window: c_ulong,
            x: c_int,
            y: c_int,
            width: c_uint,
            height: c_uint,
        ) -> c_int;
        fn XRaiseWindow(display: *mut c_void, window: c_ulong) -> c_int;
        fn XSetInputFocus(display: *mut c_void, focus: c_ulong, revert: c_int, time: c_ulong) -> c_int;
        fn XGetInputFocus(display: *mut c_void, focus: *mut c_ulong, revert: *mut c_int) -> c_int;
        fn XSync(display: *mut c_void, discard: c_int) -> c_int;
        fn XFlush(display: *mut c_void) -> c_int;
        fn XSetErrorHandler(handler: Option<unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int>) -> *mut c_void;
    }

    /// Xlib's default error handler exits the process. A pane window can vanish between a
    /// command and its X request (a tab closing while it is resized), which is harmless: log it
    /// and go on.
    unsafe extern "C" fn ignore_x_error(_display: *mut c_void, _event: *mut c_void) -> c_int {
        eprintln!("ferryx-engine: ignored an X error from a pane window request");
        0
    }

    fn install_error_handler() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        // SAFETY: installs a handler that only logs; called before this process's own Xlib
        // connections are opened.
        ONCE.call_once(|| unsafe {
            XSetErrorHandler(Some(ignore_x_error));
        });
    }

    thread_local! {
        /// The engine's own Xlib connection, opened on the UI thread on first use. Chromium
        /// keeps its own; window ids are server-wide, so either connection can move a window.
        static DISPLAY: Cell<*mut c_void> = const { Cell::new(std::ptr::null_mut()) };
    }

    fn display() -> Option<*mut c_void> {
        DISPLAY.with(|cell| {
            if cell.get().is_null() {
                install_error_handler();
                // SAFETY: a null name opens $DISPLAY; the connection lives for the process.
                cell.set(unsafe { XOpenDisplay(std::ptr::null()) });
            }
            let display = cell.get();
            (!display.is_null()).then_some(display)
        })
    }

    fn parent() -> Option<c_ulong> {
        parent_switch("ferryx-parent-xid").map(|value| value as c_ulong)
    }

    fn xid(window: &Window) -> c_ulong {
        window.window_handle()
    }

    pub fn embed(window: &Window) {
        let (Some(parent), Some(display)) = (parent(), display()) else {
            if parent().is_some() {
                eprintln!("ferryx-engine: no X display; the window stays a top-level window");
            }
            return;
        };
        // Unmapped at this point, so the window manager never takes the window as its own: once
        // it is a child of the Ferryx window it moves, stacks and closes with that window.
        // SAFETY: both ids are live windows on this display; the call runs on the UI thread.
        unsafe {
            XReparentWindow(display, xid(window), parent, 0, 0);
            XFlush(display);
        }
        focus_on_click::track(xid(window), parent);
    }

    pub fn handle(window: &Window) -> u64 {
        xid(window)
    }

    fn move_resize(window: &Window, rect: PaneRect) {
        let Some(display) = display() else { return };
        // SAFETY: the window id is live while the CEF window is; UI thread only.
        unsafe {
            XMoveResizeWindow(
                display,
                xid(window),
                rect.x,
                rect.y,
                rect.width.max(1) as c_uint,
                rect.height.max(1) as c_uint,
            );
            XFlush(display);
        }
    }

    pub fn place(window: &Window, rect: PaneRect, visible: bool) {
        if parent().is_none() {
            window.set_bounds(Some(&Rect { x: rect.x, y: rect.y, width: rect.width, height: rect.height }));
        }
        // Showing and hiding go through CEF, as on Windows: a window Views still believes hidden
        // never paints. The pixel rectangle is applied after showing, so a show that restores
        // the window's earlier size cannot undo it; Chromium follows the ConfigureNotify.
        if !visible {
            window.hide();
        } else if window.is_visible() == 0 {
            window.show();
        }
        if parent().is_some() {
            move_resize(window, rect);
        }
    }

    /// Puts the window back on top of the Ferryx window's other children (the GTK webview can
    /// restack its own subwindows).
    pub fn raise(window: &Window) {
        let (Some(_), Some(display)) = (parent(), display()) else { return };
        // SAFETY: as in `move_resize`.
        unsafe {
            XRaiseWindow(display, xid(window));
            XFlush(display);
        }
    }

    /// Gives the window X keyboard focus (the window manager only ever focuses the Ferryx
    /// toplevel), for Ferryx's own focus requests such as Mod+L.
    pub fn focus(window: &Window) {
        let (Some(_), Some(display)) = (parent(), display()) else {
            eprintln!("ferryx-engine: focus: no parent window or X display; X focus unchanged");
            return;
        };
        let target = xid(window);
        let (mut focused, mut revert) = (0, 0);
        // SAFETY: as in `move_resize`; RevertToParent (2) hands focus back to Ferryx if the
        // window goes away. XSync makes the read below see this request's outcome.
        unsafe {
            XSetInputFocus(display, target, 2, 0);
            XSync(display, 0);
            XGetInputFocus(display, &mut focused, &mut revert);
        }
        eprintln!("ferryx-engine: focus: XSetInputFocus({target:#x}), X focus now {focused:#x}");
    }

    /// The window manager focuses the Ferryx toplevel on a click and never a child window of
    /// another client, so the page would get clicks but no keys. A thread with its own X
    /// connection watches XInput2 raw button releases (any client may select those on the root
    /// window) and moves X keyboard focus to the engine window under the pointer, or back to the
    /// Ferryx window when the click landed elsewhere in it.
    mod focus_on_click {
        use std::collections::HashSet;
        use std::ffi::{c_char, c_int, c_uint, c_ulong, c_void};
        use std::sync::{Mutex, Once};

        const REVERT_TO_PARENT: c_int = 2;
        const GENERIC_EVENT: c_int = 35;
        const XI_ALL_MASTER_DEVICES: c_int = 1;
        const XI_RAW_BUTTON_RELEASE: c_int = 16;

        /// Read only by Xlib, hence the underscores.
        #[repr(C)]
        struct XiEventMask {
            _deviceid: c_int,
            _mask_len: c_int,
            _mask: *mut u8,
        }

        /// The leading fields of an `XGenericEventCookie`, inside a full-size `XEvent`.
        #[repr(C)]
        struct GenericEvent {
            kind: c_int,
            _serial: c_ulong,
            _send_event: c_int,
            _display: *mut c_void,
            extension: c_int,
            evtype: c_int,
            _rest: [c_ulong; 20],
        }

        #[link(name = "X11")]
        unsafe extern "C" {
            fn XOpenDisplay(name: *const c_char) -> *mut c_void;
            fn XDefaultRootWindow(display: *mut c_void) -> c_ulong;
            fn XQueryExtension(
                display: *mut c_void,
                name: *const c_char,
                opcode: *mut c_int,
                event: *mut c_int,
                error: *mut c_int,
            ) -> c_int;
            fn XNextEvent(display: *mut c_void, event: *mut GenericEvent) -> c_int;
            fn XQueryPointer(
                display: *mut c_void,
                window: c_ulong,
                root: *mut c_ulong,
                child: *mut c_ulong,
                root_x: *mut c_int,
                root_y: *mut c_int,
                x: *mut c_int,
                y: *mut c_int,
                mask: *mut c_uint,
            ) -> c_int;
            fn XGetInputFocus(display: *mut c_void, focus: *mut c_ulong, revert: *mut c_int) -> c_int;
            fn XGetGeometry(
                display: *mut c_void,
                drawable: c_ulong,
                root: *mut c_ulong,
                x: *mut c_int,
                y: *mut c_int,
                width: *mut c_uint,
                height: *mut c_uint,
                border: *mut c_uint,
                depth: *mut c_uint,
            ) -> c_int;
            fn XSetInputFocus(display: *mut c_void, focus: c_ulong, revert: c_int, time: c_ulong) -> c_int;
            fn XFlush(display: *mut c_void) -> c_int;
        }

        #[link(name = "Xi")]
        unsafe extern "C" {
            fn XIQueryVersion(display: *mut c_void, major: *mut c_int, minor: *mut c_int) -> c_int;
            fn XISelectEvents(display: *mut c_void, window: c_ulong, masks: *mut XiEventMask, count: c_int) -> c_int;
        }

        static PANES: Mutex<Option<(c_ulong, HashSet<c_ulong>)>> = Mutex::new(None);

        pub fn track(window: c_ulong, parent: c_ulong) {
            {
                let mut panes = PANES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                panes.get_or_insert_with(|| (parent, HashSet::new())).1.insert(window);
            }
            static ONCE: Once = Once::new();
            ONCE.call_once(|| {
                if let Err(error) = std::thread::Builder::new().name("ferryx-x11-focus".into()).spawn(watch) {
                    eprintln!("ferryx-engine: focus watcher not started: {error}");
                }
            });
        }

        /// Where keyboard focus should go after a click, given the Ferryx window's child under
        /// the pointer (0 for Ferryx's own area, `None` outside the window) and the window
        /// holding focus now: `None` leaves focus alone.
        pub(super) fn focus_target(
            parent: c_ulong,
            panes: &HashSet<c_ulong>,
            under_pointer: Option<c_ulong>,
            focused: c_ulong,
        ) -> Option<c_ulong> {
            match under_pointer {
                Some(child) if panes.contains(&child) => (focused != child).then_some(child),
                Some(_) if panes.contains(&focused) => Some(parent),
                _ => None,
            }
        }

        fn watch() {
            // SAFETY: this connection is opened, used and kept by this thread only.
            let display = unsafe { XOpenDisplay(std::ptr::null()) };
            if display.is_null() {
                eprintln!("ferryx-engine: focus watcher has no X display");
                return;
            }
            let (mut opcode, mut first_event, mut first_error) = (0, 0, 0);
            let present = unsafe {
                XQueryExtension(display, c"XInputExtension".as_ptr(), &mut opcode, &mut first_event, &mut first_error)
            };
            // Raw events need XI 2.0+, and the version this client announces decides their delivery: an
            // XI 2.0 client gets no raw event while any client grabs the device, and every button press
            // opens an implicit grab for the window under the pointer, so the release never arrived.
            // From XI 2.1 raw events are delivered whatever the grab.
            let (mut major, mut minor) = (2, 2);
            if present == 0 || unsafe { XIQueryVersion(display, &mut major, &mut minor) } != 0 {
                eprintln!("ferryx-engine: no XInput extension; clicking a page does not give it keyboard focus");
                return;
            }
            eprintln!("ferryx-engine: focus watcher: XInput {major}.{minor}, watching raw button releases");
            let mut bits = [0u8; 4];
            bits[(XI_RAW_BUTTON_RELEASE / 8) as usize] |= 1 << (XI_RAW_BUTTON_RELEASE % 8);
            let mut mask =
                XiEventMask { _deviceid: XI_ALL_MASTER_DEVICES, _mask_len: bits.len() as c_int, _mask: bits.as_mut_ptr() };
            // SAFETY: `mask` and `bits` outlive the call; the root window belongs to `display`.
            unsafe {
                XISelectEvents(display, XDefaultRootWindow(display), &mut mask, 1);
                XFlush(display);
            }
            loop {
                let mut event = GenericEvent {
                    kind: 0,
                    _serial: 0,
                    _send_event: 0,
                    _display: std::ptr::null_mut(),
                    extension: 0,
                    evtype: 0,
                    _rest: [0; 20],
                };
                // SAFETY: `event` is at least as large as an XEvent (24 longs).
                unsafe { XNextEvent(display, &mut event) };
                if event.kind != GENERIC_EVENT || event.extension != opcode || event.evtype != XI_RAW_BUTTON_RELEASE {
                    continue;
                }
                let Some((parent, panes)) =
                    PANES.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
                else {
                    continue;
                };
                let (mut root, mut child, mut mask) = (0, 0, 0);
                let (mut root_x, mut root_y, mut x, mut y) = (0, 0, 0, 0);
                let (mut gx, mut gy, mut width, mut height, mut border, mut depth) = (0, 0, 0, 0, 0, 0);
                // SAFETY: plain out-parameters; a vanished parent makes the calls return 0.
                let inside = unsafe {
                    XQueryPointer(display, parent, &mut root, &mut child, &mut root_x, &mut root_y, &mut x, &mut y, &mut mask) != 0
                        && XGetGeometry(display, parent, &mut root, &mut gx, &mut gy, &mut width, &mut height, &mut border, &mut depth) != 0
                } && x >= 0
                    && y >= 0
                    && (x as c_uint) < width
                    && (y as c_uint) < height;
                let (mut focused, mut revert) = (0, 0);
                unsafe { XGetInputFocus(display, &mut focused, &mut revert) };
                let under_pointer = inside.then_some(child);
                let target = focus_target(parent, &panes, under_pointer, focused);
                eprintln!(
                    "ferryx-engine: focus watcher: release, parent {parent:#x}, under pointer {under_pointer:x?}, focused {focused:#x}, panes {panes:x?}, target {target:x?}"
                );
                if let Some(target) = target {
                    // SAFETY: `target` is a viewable window of this server; a stale id only
                    // produces an X error the default handler logs.
                    unsafe {
                        XSetInputFocus(display, target, REVERT_TO_PARENT, 0);
                        XFlush(display);
                    }
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::focus_on_click::focus_target;
        use std::collections::HashSet;

        #[test]
        fn a_click_moves_keyboard_focus_between_the_page_and_ferryx() {
            let panes: HashSet<_> = [11, 12].into_iter().collect();
            // Into a page that does not have focus yet.
            assert_eq!(focus_target(1, &panes, Some(11), 1), Some(11));
            // Into the page that already has it.
            assert_eq!(focus_target(1, &panes, Some(11), 11), None);
            // Onto the Ferryx UI while a page has focus.
            assert_eq!(focus_target(1, &panes, Some(99), 12), Some(1));
            assert_eq!(focus_target(1, &panes, Some(0), 11), Some(1));
            // Onto the Ferryx UI while Ferryx has focus, and outside the window.
            assert_eq!(focus_target(1, &panes, Some(99), 1), None);
            assert_eq!(focus_target(1, &panes, None, 11), None);
        }
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
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

    pub fn focus(_window: &Window) {}
}

pub use imp::{embed, focus, handle, place, raise};
