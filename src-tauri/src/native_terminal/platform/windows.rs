//! Windows Platform Compositor Target for Native Terminal.
//!
//! Each terminal session owns a `WS_CHILD` HWND parented to the Tauri root window, so
//! wgpu renders into an isolated, independently positioned swapchain instead of the
//! root window shared with WebView2.
//!
//! # Safety Invariants
//!
//! 1. **Handle Lifetime**: `NativeChildViewHandle` retains the owned child HWND, which is
//!    scheduled for owner-thread destruction when the compositor target drops.
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
use tauri::{Runtime, Window};

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
const SW_SHOWNOACTIVATE: i32 = 4;
const SWP_NOACTIVATE: u32 = 0x0010;
const SWP_NOZORDER: u32 = 0x0004;
const SWP_NOMOVE: u32 = 0x0002;
const SWP_NOSIZE: u32 = 0x0001;
const HWND_TOP: isize = 0;
const HTTRANSPARENT: isize = -1;
const WM_NCHITTEST: u32 = 0x0084;
const WM_DESTROY_CHILD: u32 = 0x8000 + 1;
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
    fn PostMessageW(hwnd: Hwnd, msg: u32, wparam: usize, lparam: isize) -> i32;
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
    if msg == WM_DESTROY_CHILD {
        // Window procedures execute on the HWND's owner. No target state is retained
        // by this message: the GPU surface and Rust target have already been dropped.
        if unsafe { DestroyWindow(hwnd) } == 0 {
            tracing::warn!(error = %std::io::Error::last_os_error(), "Failed to destroy native terminal child");
        }
        return 0;
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
    pub fn new<R: Runtime>(window: &Window<R>) -> Result<Self, NativeTerminalError> {
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
        // Detach/close may drop us on a worker while holding the host mutex. Post only:
        // neither ShowWindow nor DestroyWindow may synchronously wait on the UI here.
        // SAFETY: PostMessageW transfers this private, pointer-free message to the
        // child owner; its window procedure needs no Rust state after this Drop.
        if unsafe { PostMessageW(self.handle.hwnd.get() as Hwnd, WM_DESTROY_CHILD, 0, 0) } == 0 {
            let error = std::io::Error::last_os_error();
            // Destroying the parent already destroys its children (ERROR_INVALID_WINDOW_HANDLE).
            if error.raw_os_error() != Some(1400) {
                tracing::warn!(%error, "Failed to queue native terminal child destruction");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    const WM_NCDESTROY: u32 = 0x0082;
    const DROP_COMPLETE: u32 = 0x8000 + 73;

    #[repr(C)]
    #[derive(Default)]
    struct Msg {
        hwnd: Hwnd,
        message: u32,
        wparam: usize,
        lparam: isize,
        time: u32,
        point: [i32; 2],
        private: u32,
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn IsWindow(hwnd: Hwnd) -> i32;
        fn GetWindowLongPtrW(hwnd: Hwnd, index: i32) -> isize;
        fn SendMessageW(hwnd: Hwnd, message: u32, wparam: usize, lparam: isize) -> isize;
        fn PostThreadMessageW(thread: u32, message: u32, wparam: usize, lparam: isize) -> i32;
        fn PeekMessageW(msg: *mut Msg, hwnd: Hwnd, min: u32, max: u32, remove: u32) -> i32;
        fn DispatchMessageW(msg: *const Msg) -> isize;
        fn MsgWaitForMultipleObjectsEx(
            count: u32,
            handles: *const *mut c_void,
            milliseconds: u32,
            mask: u32,
            flags: u32,
        ) -> u32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentThreadId() -> u32;
    }

    #[link(name = "comctl32")]
    unsafe extern "system" {
        fn SetWindowSubclass(
            hwnd: Hwnd,
            callback: Option<
                unsafe extern "system" fn(Hwnd, u32, usize, isize, usize, usize) -> isize,
            >,
            id: usize,
            data: usize,
        ) -> i32;
        fn DefSubclassProc(hwnd: Hwnd, message: u32, wparam: usize, lparam: isize) -> isize;
    }

    unsafe extern "system" fn observe_destroy(
        hwnd: Hwnd,
        message: u32,
        wparam: usize,
        lparam: isize,
        _id: usize,
        data: usize,
    ) -> isize {
        if message == WM_NCDESTROY {
            // The boxed observation lives through owner-thread window cleanup. Subclassing
            // observes, but forwards every message to the actual production child procedure.
            unsafe { (*(data as *mut Vec<u32>)).push(GetCurrentThreadId()) };
        }
        unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
    }

    struct Windows {
        parent: Hwnd,
        child: Hwnd,
        destroyed_on: Box<Vec<u32>>,
    }

    impl Drop for Windows {
        fn drop(&mut self) {
            unsafe {
                if IsWindow(self.child) != 0 {
                    let result = DestroyWindow(self.child);
                    eprintln!(
                        "D6 cleanup surviving child={:?} result={result}",
                        self.child
                    );
                }
                if IsWindow(self.parent) != 0 {
                    let result = DestroyWindow(self.parent);
                    eprintln!("D6 cleanup parent={:?} result={result}", self.parent);
                }
                eprintln!(
                    "D6 cleanup child_live={} parent_live={} destruction_threads={:?}",
                    IsWindow(self.child),
                    IsWindow(self.parent),
                    self.destroyed_on
                );
            }
        }
    }

    #[derive(Debug, Clone, Copy)]
    enum DropFrom {
        Worker,
        Owner,
        DestroyedParent,
    }

    fn exercise_drop(from: DropFrom) {
        // All HWND operations except the production worker Drop happen on this pumping owner.
        let owner = unsafe { GetCurrentThreadId() };
        let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
        ensure_child_class(instance).unwrap();
        let static_class: Vec<u16> = "STATIC\0".encode_utf16().collect();
        let mut windows = Windows {
            parent: unsafe {
                CreateWindowExW(
                    0,
                    static_class.as_ptr(),
                    std::ptr::null(),
                    0,
                    0,
                    0,
                    16,
                    16,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    instance,
                    std::ptr::null_mut(),
                )
            },
            child: std::ptr::null_mut(),
            destroyed_on: Box::default(),
        };
        assert!(!windows.parent.is_null(), "native parent creation failed");
        windows.child = unsafe {
            CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
                CHILD_CLASS_NAME.as_ptr(),
                std::ptr::null(),
                WS_CHILD | WS_CLIPSIBLINGS,
                0,
                0,
                1,
                1,
                windows.parent,
                std::ptr::null_mut(),
                instance,
                std::ptr::null_mut(),
            )
        };
        let hwnd = NonZeroIsize::new(windows.child as isize).expect("native child creation failed");
        let target = WindowsCompositorTarget {
            handle: Arc::new(NativeChildViewHandle {
                hwnd,
                hinstance: NonZeroIsize::new(instance as isize),
            }),
            visibility: Mutex::new(ChildSurfaceVisibility::default()),
        };
        assert_ne!(
            unsafe {
                SetWindowSubclass(
                    windows.child,
                    Some(observe_destroy),
                    1,
                    (&mut *windows.destroyed_on as *mut Vec<u32>) as usize,
                )
            },
            0
        );
        assert_eq!(
            unsafe { GetWindowLongPtrW(windows.child, -16) } & 0x1000_0000,
            0,
            "child must start hidden"
        );
        assert_eq!(
            unsafe { SendMessageW(windows.child, WM_NCHITTEST, 0, 0) },
            HTTRANSPARENT
        );
        target.reveal();
        assert_ne!(
            unsafe { GetWindowLongPtrW(windows.child, -16) } & 0x1000_0000,
            0,
            "first presentation must reveal child (parent remains hidden in this test)"
        );
        eprintln!(
            "D6 created parent={:?} child={:?} owner={owner} case={from:?}; observer subscribed",
            windows.parent, windows.child
        );
        if matches!(from, DropFrom::DestroyedParent) {
            assert_ne!(unsafe { DestroyWindow(windows.parent) }, 0);
        }
        let worker = if matches!(from, DropFrom::Owner) {
            drop(target);
            assert_ne!(unsafe { PostThreadMessageW(owner, DROP_COMPLETE, 0, 0) }, 0);
            None
        } else {
            Some(std::thread::spawn(move || {
                let worker = unsafe { GetCurrentThreadId() };
                eprintln!(
                    "D6 dropping production WindowsCompositorTarget worker={worker} owner={owner}"
                );
                drop(target);
                // FIFO posted-message barrier: any teardown posted by Drop must be dispatched
                // before this marker. No sleep or timeout is used to decide success/failure.
                assert_ne!(unsafe { PostThreadMessageW(owner, DROP_COMPLETE, 0, 0) }, 0);
                worker
            }))
        };
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut completed = false;
        'pump: while Instant::now() < deadline {
            let mut msg = Msg::default();
            while unsafe { PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, 1) } != 0 {
                if msg.message == DROP_COMPLETE && msg.hwnd.is_null() {
                    completed = true;
                    break 'pump;
                }
                unsafe { DispatchMessageW(&msg) };
            }
            // Message-queue event wait, not polling. The deadline only fails a stuck test.
            let remaining = deadline
                .saturating_duration_since(Instant::now())
                .as_millis() as u32;
            let result = unsafe {
                MsgWaitForMultipleObjectsEx(0, std::ptr::null(), remaining, 0x04ff, 0x0004)
            };
            if result != 0 {
                break;
            }
        }
        let observed = windows.destroyed_on.as_ref().clone();
        let child_live = unsafe { IsWindow(windows.child) } != 0;
        eprintln!("D6 barrier={completed} child_live={child_live} destruction_threads={observed:?} owner={owner}");
        // Crucially, RED's surviving child is destroyed on its owner BEFORE the assertion.
        drop(windows);
        assert!(
            completed,
            "owner message queue did not reach worker Drop barrier"
        );
        if let Some(worker) = worker {
            assert_ne!(worker.join().unwrap(), owner);
        }
        assert!(
            !child_live,
            "production WindowsCompositorTarget::drop left child HWND alive after worker barrier"
        );
        assert_eq!(
            observed,
            vec![owner],
            "WM_NCDESTROY must occur exactly once on the owner"
        );
    }

    #[test]
    fn child_is_destroyed_on_owner_thread_when_detached_by_worker() {
        exercise_drop(DropFrom::Worker);
    }

    #[test]
    fn child_is_destroyed_when_dropped_by_owner() {
        exercise_drop(DropFrom::Owner);
    }

    #[test]
    fn drop_after_parent_destruction_is_harmless() {
        exercise_drop(DropFrom::DestroyedParent);
    }
}
