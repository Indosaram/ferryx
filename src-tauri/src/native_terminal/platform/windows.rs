//! Windows Platform Compositor Target for Native Terminal.
//!
//! Each terminal session owns a `WS_CHILD` HWND parented to the Tauri root window, so
//! wgpu renders into an isolated, independently positioned swapchain instead of the
//! root window shared with WebView2.
//!
//! # Safety Invariants
//!
//! 1. **Handle Lifetime**: `NativeChildViewHandle` retains the owned child HWND, which is
//!    destroyed with the compositor target.
//! 2. **Thread Affinity**: The child HWND is created and destroyed on the Tauri main thread,
//!    which owns the parent window and pumps its message queue.
//! 3. **Pointer Transparency**: `WM_NCHITTEST` answers `HTTRANSPARENT` so pointer input keeps
//!    routing to the WebView2 chrome behind the child.

use std::ffi::c_void;
use std::num::NonZeroIsize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, Win32WindowHandle, WindowHandle, WindowsDisplayHandle,
};
use tauri::{Runtime, WebviewWindow};

use crate::native_terminal::child_surface::{ChildSurfaceGeometry, ChildSurfaceVisibility};
use crate::native_terminal::composition::{
    CompositorTargetKind, LogicalBounds, PlatformCompositorDescriptor,
};
use crate::native_terminal::error::NativeTerminalError;

type Hwnd = *mut c_void;
type Hinstance = *mut c_void;

const WS_CHILD: u32 = 0x4000_0000;
const WS_CLIPSIBLINGS: u32 = 0x0400_0000;
const WS_EX_NOACTIVATE: u32 = 0x0800_0000;
const WS_EX_TRANSPARENT: u32 = 0x0000_0020;
const SW_HIDE: i32 = 0;
const SW_SHOWNOACTIVATE: i32 = 4;
const SWP_NOACTIVATE: u32 = 0x0010;
const SWP_NOZORDER: u32 = 0x0004;
const SWP_NOMOVE: u32 = 0x0002;
const SWP_NOSIZE: u32 = 0x0001;
const HWND_TOP: isize = 0;
const HTTRANSPARENT: isize = -1;
const WM_NCHITTEST: u32 = 0x0084;
const CS_HREDRAW: u32 = 0x0002;
const CS_VREDRAW: u32 = 0x0001;
const CS_OWNDC: u32 = 0x0020;

#[repr(C)]
struct WndClassExW {
    cb_size: u32,
    style: u32,
    lpfn_wnd_proc: Option<unsafe extern "system" fn(Hwnd, u32, usize, isize) -> isize>,
    cb_cls_extra: i32,
    cb_wnd_extra: i32,
    h_instance: Hinstance,
    h_icon: *mut c_void,
    h_cursor: *mut c_void,
    hbr_background: *mut c_void,
    lpsz_menu_name: *const u16,
    lpsz_class_name: *const u16,
    h_icon_sm: *mut c_void,
}

#[link(name = "user32")]
unsafe extern "system" {
    fn RegisterClassExW(class: *const WndClassExW) -> u16;
    fn CreateWindowExW(
        ex_style: u32,
        class_name: *const u16,
        window_name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: Hwnd,
        menu: *mut c_void,
        instance: Hinstance,
        param: *mut c_void,
    ) -> Hwnd;
    fn DefWindowProcW(hwnd: Hwnd, msg: u32, wparam: usize, lparam: isize) -> isize;
    fn DestroyWindow(hwnd: Hwnd) -> i32;
    fn SetWindowPos(
        hwnd: Hwnd,
        insert_after: Hwnd,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        flags: u32,
    ) -> i32;
    fn ShowWindow(hwnd: Hwnd, cmd: i32) -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> Hinstance;
}

unsafe extern "system" fn child_wnd_proc(
    hwnd: Hwnd,
    msg: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    if msg == WM_NCHITTEST {
        return HTTRANSPARENT;
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

const CHILD_CLASS_NAME: &[u16] = &[
    b'F' as u16,
    b'e' as u16,
    b'r' as u16,
    b'r' as u16,
    b'y' as u16,
    b'x' as u16,
    b'N' as u16,
    b'a' as u16,
    b't' as u16,
    b'i' as u16,
    b'v' as u16,
    b'e' as u16,
    b'T' as u16,
    b'e' as u16,
    b'r' as u16,
    b'm' as u16,
    0,
];

static CLASS_REGISTERED: AtomicBool = AtomicBool::new(false);

fn ensure_child_class(instance: Hinstance) -> Result<(), NativeTerminalError> {
    if CLASS_REGISTERED.load(Ordering::Acquire) {
        return Ok(());
    }
    let class = WndClassExW {
        cb_size: std::mem::size_of::<WndClassExW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW | CS_OWNDC,
        lpfn_wnd_proc: Some(child_wnd_proc),
        cb_cls_extra: 0,
        cb_wnd_extra: 0,
        h_instance: instance,
        h_icon: std::ptr::null_mut(),
        h_cursor: std::ptr::null_mut(),
        hbr_background: std::ptr::null_mut(),
        lpsz_menu_name: std::ptr::null(),
        lpsz_class_name: CHILD_CLASS_NAME.as_ptr(),
        h_icon_sm: std::ptr::null_mut(),
    };
    // SAFETY: `class` is a fully initialized WNDCLASSEXW whose string pointer outlives the call.
    let atom = unsafe { RegisterClassExW(&class) };
    if atom == 0 {
        // A concurrent registration of the same class name is not an error for our purposes;
        // CreateWindowExW below fails loudly if the class truly does not exist.
        CLASS_REGISTERED.store(true, Ordering::Release);
        return Ok(());
    }
    CLASS_REGISTERED.store(true, Ordering::Release);
    Ok(())
}

/// Safe handle implementing `HasWindowHandle` and `HasDisplayHandle` for wgpu surface creation on Windows.
pub struct NativeChildViewHandle {
    hwnd: NonZeroIsize,
    hinstance: Option<NonZeroIsize>,
}

// SAFETY: Windows HWND / HINSTANCE handles are thread-safe to reference across threads.
unsafe impl Send for NativeChildViewHandle {}
unsafe impl Sync for NativeChildViewHandle {}

impl HasWindowHandle for NativeChildViewHandle {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let mut handle = Win32WindowHandle::new(self.hwnd);
        handle.hinstance = self.hinstance;
        let raw = RawWindowHandle::Win32(handle);
        // SAFETY: The Win32 window handle wraps the child HWND owned by this compositor target.
        unsafe { Ok(WindowHandle::borrow_raw(raw)) }
    }
}

impl HasDisplayHandle for NativeChildViewHandle {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        let handle = WindowsDisplayHandle::new();
        let raw = RawDisplayHandle::Windows(handle);
        // SAFETY: Windows display handle is stateless and always valid.
        unsafe { Ok(DisplayHandle::borrow_raw(raw)) }
    }
}

pub struct WindowsCompositorTarget {
    handle: Arc<NativeChildViewHandle>,
    visibility: Mutex<ChildSurfaceVisibility>,
}

// SAFETY: `WindowsCompositorTarget` contains thread-safe handles.
unsafe impl Send for WindowsCompositorTarget {}
unsafe impl Sync for WindowsCompositorTarget {}

impl WindowsCompositorTarget {
    /// Creates a hidden child HWND parented to the Tauri window for isolated wgpu rendering.
    pub fn new<R: Runtime>(window: &WebviewWindow<R>) -> Result<Self, NativeTerminalError> {
        let window_handle = window.window_handle().map_err(|e| {
            NativeTerminalError::GpuPipelineError(format!("Failed to get window handle: {e}"))
        })?;

        let parent_hwnd = match window_handle.as_raw() {
            RawWindowHandle::Win32(handle) => handle.hwnd,
            _ => {
                return Err(NativeTerminalError::GpuPipelineError(
                    "Expected Win32 window handle on Windows".into(),
                ));
            }
        };

        let _ = window.display_handle().map_err(|e| {
            NativeTerminalError::GpuPipelineError(format!("Failed to get display handle: {e}"))
        })?;

        // SAFETY: Passing a null module name returns the handle of the current process image.
        let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
        ensure_child_class(instance)?;

        // SAFETY: The class is registered above and the parent HWND comes from the live Tauri window.
        let child = unsafe {
            CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
                CHILD_CLASS_NAME.as_ptr(),
                std::ptr::null(),
                WS_CHILD | WS_CLIPSIBLINGS,
                0,
                0,
                1,
                1,
                parent_hwnd.get() as Hwnd,
                std::ptr::null_mut(),
                instance,
                std::ptr::null_mut(),
            )
        };

        let child_hwnd = NonZeroIsize::new(child as isize).ok_or_else(|| {
            NativeTerminalError::GpuPipelineError(
                "Failed to create native terminal child window".into(),
            )
        })?;

        let handle = Arc::new(NativeChildViewHandle {
            hwnd: child_hwnd,
            hinstance: NonZeroIsize::new(instance as isize),
        });

        Ok(Self {
            handle,
            visibility: Mutex::new(ChildSurfaceVisibility::default()),
        })
    }

    pub fn surface_target(&self) -> Arc<NativeChildViewHandle> {
        Arc::clone(&self.handle)
    }

    pub fn descriptor(&self) -> PlatformCompositorDescriptor {
        PlatformCompositorDescriptor {
            target_kind: CompositorTargetKind::WindowsChildWindow,
            pointer_transparent: true,
            layer_backed: true,
        }
    }

    /// Moves and resizes the child HWND to the pane rectangle in physical pixels.
    pub fn update_viewport(&self, bounds: Option<LogicalBounds>) {
        let Some(geometry) = bounds
            .as_ref()
            .and_then(ChildSurfaceGeometry::from_logical_bounds)
        else {
            return;
        };
        // SAFETY: The child HWND is owned by this target and remains valid until drop.
        unsafe {
            SetWindowPos(
                self.handle.hwnd.get() as Hwnd,
                std::ptr::null_mut(),
                geometry.x,
                geometry.y,
                geometry.width as i32,
                geometry.height as i32,
                SWP_NOACTIVATE | SWP_NOZORDER,
            );
        }
    }

    /// Shows the child HWND once the first frame has been presented.
    pub fn reveal(&self) {
        let Ok(mut visibility) = self.visibility.lock() else {
            return;
        };
        if !visibility.should_map_on_present() {
            return;
        }
        // SAFETY: The child HWND is owned by this target and remains valid until drop.
        unsafe {
            ShowWindow(self.handle.hwnd.get() as Hwnd, SW_SHOWNOACTIVATE);
            // WebView2 is created after the terminal child, so it sits above it in the sibling
            // z-order and would occlude every presented frame. Raise the terminal child to the
            // top of the parent's child z-order once it has something to show.
            SetWindowPos(
                self.handle.hwnd.get() as Hwnd,
                HWND_TOP as Hwnd,
                0,
                0,
                0,
                0,
                SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
            );
        }
        visibility.mark_presented();
    }
}

impl Drop for WindowsCompositorTarget {
    fn drop(&mut self) {
        if let Ok(mut visibility) = self.visibility.lock() {
            visibility.mark_detached();
        }
        // SAFETY: The child HWND was created by this target and is destroyed exactly once.
        unsafe {
            ShowWindow(self.handle.hwnd.get() as Hwnd, SW_HIDE);
            DestroyWindow(self.handle.hwnd.get() as Hwnd);
        }
    }
}
