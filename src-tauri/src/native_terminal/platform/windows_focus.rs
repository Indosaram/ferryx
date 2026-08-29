//! Non-consuming mouse hook for native terminal keyboard focus restoration on Windows.
//!
//! When a native terminal child HWND is clicked, the child window's `HTTRANSPARENT`
//! hit-test prevents standard window activation. This low-level mouse hook detects
//! left mouse button up events over native terminal surfaces, restores keyboard focus
//! to the hosting WebView2 child window, and emits `native_terminal_focus` to the frontend.

use std::ffi::c_void;
use std::sync::Mutex;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use tauri::{Emitter, Manager, Runtime};

use crate::native_terminal::surface_host::{
    NativeTerminalSurfaceHostState, NATIVE_TERMINAL_FOCUS_EVENT,
};

type Hwnd = *mut c_void;
type Hinstance = *mut c_void;
type Hhook = *mut c_void;

const WH_MOUSE_LL: i32 = 14;
const WM_LBUTTONUP: u32 = 0x0202;
const HC_ACTION: i32 = 0;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct POINT {
    x: i32,
    y: i32,
}

#[repr(C)]
struct MSLLHOOKSTRUCT {
    pt: POINT,
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
    fn ScreenToClient(hwnd: Hwnd, lp_point: *mut POINT) -> i32;
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
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> Hinstance;
}

const CHROME_WIDGET_CLASS: &[u16] = &[
    b'C' as u16, b'h' as u16, b'r' as u16, b'o' as u16, b'm' as u16, b'e' as u16,
    b'_' as u16, b'W' as u16, b'i' as u16, b'd' as u16, b'g' as u16, b'e' as u16,
    b't' as u16, b'W' as u16, b'i' as u16, b'n' as u16, b'_' as u16, b'1' as u16,
    0,
];

unsafe extern "system" fn enum_child_find_chrome_widget(hwnd: Hwnd, lparam: isize) -> i32 {
    let result = &mut *(lparam as *mut Option<Hwnd>);
    let mut class_buf = [0u16; 64];
    let len = GetClassNameW(hwnd, class_buf.as_mut_ptr(), class_buf.len() as i32);
    if len > 0 {
        let target_len = CHROME_WIDGET_CLASS.len() - 1;
        if len as usize == target_len && &class_buf[..target_len] == &CHROME_WIDGET_CLASS[..target_len] {
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
            tracing::debug!("Chrome_WidgetWin_1 child not found; set focus to root window {root_hwnd:?}");
        }
    }
}

struct MonitorState {
    root_hwnd: isize,
    surface_host: NativeTerminalSurfaceHostState,
    emit_focus: Box<dyn Fn(String) + Send + Sync + 'static>,
}

static MONITOR_STATE: Mutex<Option<MonitorState>> = Mutex::new(None);
static HOOK_HANDLE: Mutex<Option<isize>> = Mutex::new(None);

unsafe extern "system" fn mouse_ll_hook_proc(
    n_code: i32,
    wparam: usize,
    lparam: isize,
) -> isize {
    if n_code >= HC_ACTION && wparam == WM_LBUTTONUP as usize && lparam != 0 {
        let hook_struct = &*(lparam as *const MSLLHOOKSTRUCT);
        let mut pt = hook_struct.pt;

        if let Ok(guard) = MONITOR_STATE.lock() {
            if let Some(state) = guard.as_ref() {
                let root_hwnd = state.root_hwnd as Hwnd;
                if ScreenToClient(root_hwnd, &mut pt) != 0 {
                    let dpi = GetDpiForWindow(root_hwnd);
                    let dpi_scale = if dpi == 0 { 1.0 } else { dpi as f64 / 96.0 };
                    let logical_x = pt.x as f64 / dpi_scale;
                    let logical_y = pt.y as f64 / dpi_scale;

                    if let Some(session_id) =
                        state.surface_host.session_at_logical_point(logical_x, logical_y)
                    {
                        best_effort_focus_webview(root_hwnd);
                        (state.emit_focus)(session_id);
                    }
                }
            }
        }
    }

    CallNextHookEx(std::ptr::null_mut(), n_code, wparam, lparam)
}

/// Installs a low-level mouse hook on Windows to restore keyboard focus to WebView2
/// when clicking inside an active native terminal child surface.
pub fn install_windows_terminal_focus_monitor<R: Runtime>(
    app: &tauri::App<R>,
) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window("main") else {
        tracing::debug!("install_windows_terminal_focus_monitor: main window not found");
        return Ok(());
    };

    let window_handle = match window.window_handle() {
        Ok(h) => h,
        Err(e) => {
            tracing::debug!("install_windows_terminal_focus_monitor: failed to get window handle: {e}");
            return Ok(());
        }
    };

    let root_hwnd = match window_handle.as_raw() {
        RawWindowHandle::Win32(handle) => handle.hwnd.get() as Hwnd,
        _ => {
            tracing::debug!("install_windows_terminal_focus_monitor: expected Win32 window handle");
            return Ok(());
        }
    };

    let surface_host = app
        .state::<NativeTerminalSurfaceHostState>()
        .inner()
        .clone();

    let app_handle = app.handle().clone();
    let emit_focus = Box::new(move |session_id: String| {
        if let Some(window) = app_handle.get_webview_window("main") {
            let _ = window.emit(NATIVE_TERMINAL_FOCUS_EVENT, session_id);
        } else {
            let _ = app_handle.emit(NATIVE_TERMINAL_FOCUS_EVENT, session_id);
        }
    });

    if let Ok(mut guard) = MONITOR_STATE.lock() {
        *guard = Some(MonitorState {
            root_hwnd: root_hwnd as isize,
            surface_host,
            emit_focus,
        });
    }

    // SAFETY: Installing WH_MOUSE_LL hook on the current process module image.
    let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
    let hook = unsafe {
        SetWindowsHookExW(
            WH_MOUSE_LL,
            Some(mouse_ll_hook_proc),
            instance,
            0,
        )
    };

    if hook.is_null() {
        tracing::warn!("install_windows_terminal_focus_monitor: SetWindowsHookExW failed");
    } else if let Ok(mut guard) = HOOK_HANDLE.lock() {
        *guard = Some(hook as isize);
    }

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
    if let Ok(mut guard) = MONITOR_STATE.lock() {
        *guard = None;
    }
}
