//! Non-consuming mouse hook for native terminal keyboard focus restoration on Windows.
//!
//! When a native terminal child HWND is clicked, the child window's `HTTRANSPARENT`
//! hit-test prevents standard window activation. This low-level mouse hook detects
//! left mouse button up events over native terminal surfaces, restores keyboard focus
//! to the hosting WebView2 child window, and emits `native_terminal_focus` to the frontend.
//!
//! One process-wide hook serves every app window. Each registered top-level window keeps its own
//! state keyed by its root HWND; a click resolves the top-level window under the pointer and only
//! that window's terminal surfaces are hit-tested and notified.

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::Mutex;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use tauri::{Emitter, Manager, Runtime, Window};

use crate::native_terminal::surface_host::{
    NativeTerminalSurfaceHostState, NATIVE_TERMINAL_FOCUS_EVENT,
};

type Hwnd = *mut c_void;
type Hinstance = *mut c_void;
type Hhook = *mut c_void;

const WH_MOUSE_LL: i32 = 14;
const WM_LBUTTONDOWN: u32 = 0x0201;
const WM_LBUTTONUP: u32 = 0x0202;
const HC_ACTION: i32 = 0;
const GA_ROOT: u32 = 2;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct Point {
    x: i32,
    y: i32,
}

#[repr(C)]
struct MsllHookStruct {
    pt: Point,
    mouse_data: u32,
    flags: u32,
    time: u32,
    dw_extra_info: usize,
}

#[link(name = "user32")]
unsafe extern "system" {
    fn SetWindowsHookExW(
        id_hook: i32,
        lpfn: Option<unsafe extern "system" fn(i32, usize, isize) -> isize>,
        hmod: Hinstance,
        dw_thread_id: u32,
    ) -> Hhook;
    fn CallNextHookEx(hhk: Hhook, n_code: i32, wparam: usize, lparam: isize) -> isize;
    fn UnhookWindowsHookEx(hhk: Hhook) -> i32;
    fn ScreenToClient(hwnd: Hwnd, lp_point: *mut Point) -> i32;
    fn GetDpiForWindow(hwnd: Hwnd) -> u32;
    fn FindWindowExW(
        parent: Hwnd,
        child_after: Hwnd,
        class_name: *const u16,
        window_name: *const u16,
    ) -> Hwnd;
    fn SetFocus(hwnd: Hwnd) -> Hwnd;
    fn EnumChildWindows(
        hwnd_parent: Hwnd,
        lp_enum_func: Option<unsafe extern "system" fn(Hwnd, isize) -> i32>,
        lparam: isize,
    ) -> i32;
    fn GetClassNameW(hwnd: Hwnd, lp_class_name: *mut u16, n_max_count: i32) -> i32;
    fn WindowFromPoint(point: Point) -> Hwnd;
    fn GetAncestor(hwnd: Hwnd, ga_flags: u32) -> Hwnd;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> Hinstance;
}

const CHROME_WIDGET_CLASS: &[u16] = &[
    b'C' as u16,
    b'h' as u16,
    b'r' as u16,
    b'o' as u16,
    b'm' as u16,
    b'e' as u16,
    b'_' as u16,
    b'W' as u16,
    b'i' as u16,
    b'd' as u16,
    b'g' as u16,
    b'e' as u16,
    b't' as u16,
    b'W' as u16,
    b'i' as u16,
    b'n' as u16,
    b'_' as u16,
    b'1' as u16,
    0,
];

unsafe extern "system" fn enum_child_find_chrome_widget(hwnd: Hwnd, lparam: isize) -> i32 {
    let result = &mut *(lparam as *mut Option<Hwnd>);
    let mut class_buf = [0u16; 64];
    let len = GetClassNameW(hwnd, class_buf.as_mut_ptr(), class_buf.len() as i32);
    if len > 0 {
        let target_len = CHROME_WIDGET_CLASS.len() - 1;
        if len as usize == target_len
            && class_buf[..target_len] == CHROME_WIDGET_CLASS[..target_len]
        {
            *result = Some(hwnd);
            return 0;
        }
    }
    1
}

fn best_effort_focus_webview(root_hwnd: Hwnd) {
    unsafe {
        let mut target = FindWindowExW(
            root_hwnd,
            std::ptr::null_mut(),
            CHROME_WIDGET_CLASS.as_ptr(),
            std::ptr::null(),
        );
        if target.is_null() {
            let mut found: Option<Hwnd> = None;
            EnumChildWindows(
                root_hwnd,
                Some(enum_child_find_chrome_widget),
                &mut found as *mut Option<Hwnd> as isize,
            );
            if let Some(h) = found {
                target = h;
            }
        }
        if !target.is_null() {
            let _ = SetFocus(target);
            tracing::debug!("Restored Win32 keyboard focus to WebView2 window {target:?}");
        } else {
            let _ = SetFocus(root_hwnd);
            tracing::debug!(
                "Chrome_WidgetWin_1 child not found; set focus to root window {root_hwnd:?}"
            );
        }
    }
}

/// Per top-level window state for the shared hook.
struct MonitorState {
    /// Sessions hit at a logical point of this window, limited to the sessions it owns.
    hit_test: Box<dyn Fn(f64, f64) -> Option<String> + Send + Sync + 'static>,
    emit_focus: Box<dyn Fn(String) + Send + Sync + 'static>,
}

/// Registered windows keyed by root HWND (as `isize`).
static MONITOR_STATES: Mutex<Option<HashMap<isize, MonitorState>>> = Mutex::new(None);
static HOOK_HANDLE: Mutex<Option<isize>> = Mutex::new(None);
static FOCUS_GATE: crate::native_terminal::focus_gesture::PressReleaseFocusGate =
    crate::native_terminal::focus_gesture::PressReleaseFocusGate::new();

unsafe extern "system" fn mouse_ll_hook_proc(n_code: i32, wparam: usize, lparam: isize) -> isize {
    let is_press = wparam == WM_LBUTTONDOWN as usize;
    if n_code >= HC_ACTION && (is_press || wparam == WM_LBUTTONUP as usize) && lparam != 0 {
        let hook_struct = &*(lparam as *const MsllHookStruct);
        let screen_pt = hook_struct.pt;

        if let Ok(guard) = MONITOR_STATES.lock() {
            if let Some(states) = guard.as_ref() {
                let under_pointer = WindowFromPoint(screen_pt);
                let root_hwnd = if under_pointer.is_null() {
                    std::ptr::null_mut()
                } else {
                    GetAncestor(under_pointer, GA_ROOT)
                };
                let mut hit = None;
                let mut clicked = None;
                if let Some(state) = states.get(&(root_hwnd as isize)) {
                    let mut pt = screen_pt;
                    if ScreenToClient(root_hwnd, &mut pt) != 0 {
                        let dpi = GetDpiForWindow(root_hwnd);
                        let dpi_scale = if dpi == 0 { 1.0 } else { dpi as f64 / 96.0 };
                        let logical_x = pt.x as f64 / dpi_scale;
                        let logical_y = pt.y as f64 / dpi_scale;
                        hit = (state.hit_test)(logical_x, logical_y);
                        clicked = Some(state);
                    }
                }
                // A press outside every registered window still resets the gate, so a drag that
                // starts elsewhere never focuses the pane it is released over.
                if is_press {
                    FOCUS_GATE.press(hit);
                } else if let Some(session_id) = FOCUS_GATE.release(hit) {
                    if let Some(state) = clicked {
                        best_effort_focus_webview(root_hwnd);
                        (state.emit_focus)(session_id);
                    }
                }
            }
        }
    }

    CallNextHookEx(std::ptr::null_mut(), n_code, wparam, lparam)
}

fn root_hwnd_of<R: Runtime>(window: &Window<R>) -> Option<isize> {
    let window_handle = match window.window_handle() {
        Ok(h) => h,
        Err(e) => {
            tracing::debug!(
                label = window.label(),
                "windows terminal focus: failed to get window handle: {e}"
            );
            return None;
        }
    };
    match window_handle.as_raw() {
        RawWindowHandle::Win32(handle) => Some(handle.hwnd.get()),
        _ => {
            tracing::debug!(
                label = window.label(),
                "windows terminal focus: expected Win32 window handle"
            );
            None
        }
    }
}

fn ensure_hook_installed() {
    let Ok(mut guard) = HOOK_HANDLE.lock() else {
        return;
    };
    if guard.is_some() {
        return;
    }
    // SAFETY: Installing WH_MOUSE_LL hook on the current process module image.
    let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
    let hook = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_ll_hook_proc), instance, 0) };
    if hook.is_null() {
        tracing::warn!("windows terminal focus: SetWindowsHookExW failed");
    } else {
        *guard = Some(hook as isize);
    }
}

/// Registers one top-level app window with the shared low-level mouse hook, installing the hook on
/// first use. Clicks inside this window hit-test only the sessions it owns and emit
/// `native_terminal_focus` to this window. Returns the root HWND to pass to
/// [`unregister_windows_terminal_focus_window`] when the window closes, or `None` when the window
/// has no Win32 handle.
pub fn register_windows_terminal_focus_window<R: Runtime>(
    window: &Window<R>,
    surface_host: NativeTerminalSurfaceHostState,
) -> Option<isize> {
    let root_hwnd = root_hwnd_of(window)?;
    let label = window.label().to_string();

    let hit_app = window.app_handle().clone();
    let hit_label = label.clone();
    let hit_test = Box::new(move |logical_x: f64, logical_y: f64| {
        surface_host.session_at_logical_point_matching(logical_x, logical_y, |session_id| {
            crate::window_registry::window_for_session(&hit_app, session_id)
                .is_some_and(|owner| owner.label() == hit_label)
        })
    });

    let emit_app = window.app_handle().clone();
    let emit_focus = Box::new(move |session_id: String| {
        if let Some(window) = emit_app.get_window(&label) {
            let _ = window.emit(NATIVE_TERMINAL_FOCUS_EVENT, session_id);
        } else {
            let _ = emit_app.emit(NATIVE_TERMINAL_FOCUS_EVENT, session_id);
        }
    });

    if let Ok(mut guard) = MONITOR_STATES.lock() {
        guard.get_or_insert_with(HashMap::new).insert(
            root_hwnd,
            MonitorState {
                hit_test,
                emit_focus,
            },
        );
    }
    ensure_hook_installed();
    Some(root_hwnd)
}

/// Removes a window registered with [`register_windows_terminal_focus_window`]. The shared hook
/// stays installed for the remaining windows.
pub fn unregister_windows_terminal_focus_window(root_hwnd: isize) {
    if let Ok(mut guard) = MONITOR_STATES.lock() {
        if let Some(states) = guard.as_mut() {
            states.remove(&root_hwnd);
        }
    }
}

/// Installs a low-level mouse hook on Windows to restore keyboard focus to WebView2
/// when clicking inside an active native terminal child surface of the `main` window.
pub fn install_windows_terminal_focus_monitor<R: Runtime>(
    app: &tauri::App<R>,
) -> tauri::Result<()> {
    let Some(window) = app.get_window(crate::window_registry::PRIMARY_WINDOW_LABEL) else {
        tracing::debug!("install_windows_terminal_focus_monitor: main window not found");
        return Ok(());
    };
    let surface_host = app
        .state::<NativeTerminalSurfaceHostState>()
        .inner()
        .clone();
    register_windows_terminal_focus_window(&window, surface_host);
    Ok(())
}

/// Uninstalls the low-level mouse hook if currently active.
pub fn uninstall_windows_terminal_focus_monitor() {
    if let Ok(mut guard) = HOOK_HANDLE.lock() {
        if let Some(hook_val) = guard.take() {
            unsafe {
                UnhookWindowsHookEx(hook_val as Hhook);
            }
        }
    }
    if let Ok(mut guard) = MONITOR_STATES.lock() {
        *guard = None;
    }
}
