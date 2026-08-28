//! Linux Platform Compositor Target for Native Terminal.
//!
//! Provides an X11/Wayland composition surface target implementing `HasWindowHandle`
//! and `HasDisplayHandle` for wgpu surface creation.
//!
//! # Safety Invariants
//!
//! 1. **Handle Lifetime**: The `NativeChildViewHandle` retains the X11 Window/Wayland surface
//!    handle obtained from the parent window, valid for the lifetime of the compositor target.
//! 2. **Thread Safety**: Linux window and display handles are safe to borrow and query across threads.
//! 3. **Capability Reporting**: Every session owns a real isolated child with an empty input
//!    region -- an `InputOutput` window under X11/XWayland, a `wl_subsurface` under native
//!    Wayland -- so the descriptor reports layer-backed and pointer-transparent on both.

use std::ffi::{c_int, c_ulong, c_void};
use std::num::NonZeroU32;
use std::ptr::NonNull;
use std::sync::{Arc, Mutex};

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle, WindowHandle, XcbDisplayHandle,
    XcbWindowHandle, XlibDisplayHandle, XlibWindowHandle,
};
use tauri::{Runtime, WebviewWindow};

use crate::native_terminal::child_surface::{
    ChildSurfaceGeometry, ChildSurfaceVisibility, WaylandSubsurfaceGeometry,
};
use crate::native_terminal::composition::{
    CompositorTargetKind, LogicalBounds, PlatformCompositorDescriptor,
};
use crate::native_terminal::error::NativeTerminalError;
use crate::native_terminal::platform::wayland_child::WaylandChild;

const INPUT_OUTPUT: u32 = 1;
const COPY_FROM_PARENT: c_ulong = 0;
const SHAPE_INPUT: c_int = 2;
const SHAPE_SET: c_int = 0;

#[link(name = "X11")]
unsafe extern "C" {
    fn XCreateSimpleWindow(
        display: *mut c_void,
        parent: c_ulong,
        x: c_int,
        y: c_int,
        width: u32,
        height: u32,
        border_width: u32,
        border: c_ulong,
        background: c_ulong,
    ) -> c_ulong;
    fn XMoveResizeWindow(
        display: *mut c_void,
        window: c_ulong,
        x: c_int,
        y: c_int,
        width: u32,
        height: u32,
    ) -> c_int;
    fn XMapWindow(display: *mut c_void, window: c_ulong) -> c_int;
    fn XUnmapWindow(display: *mut c_void, window: c_ulong) -> c_int;
    fn XDestroyWindow(display: *mut c_void, window: c_ulong) -> c_int;
    fn XFlush(display: *mut c_void) -> c_int;
}

#[link(name = "Xext")]
unsafe extern "C" {
    fn XShapeCombineRectangles(
        display: *mut c_void,
        window: c_ulong,
        kind: c_int,
        x_off: c_int,
        y_off: c_int,
        rectangles: *const c_void,
        n_rects: c_int,
        op: c_int,
        ordering: c_int,
    );
}

struct X11Child {
    display: *mut c_void,
    window: c_ulong,
}

// SAFETY: the child window and its display connection are only touched from the Tauri main
// thread, and the pointers stay valid for the lifetime of the compositor target.
unsafe impl Send for X11Child {}
unsafe impl Sync for X11Child {}

impl X11Child {
    fn create(display: *mut c_void, parent: c_ulong) -> Option<Self> {
        if display.is_null() || parent == 0 {
            return None;
        }
        // SAFETY: `display` is the live Xlib connection owned by GTK and `parent` is the
        // Tauri window returned by tao, both valid for the duration of this call.
        let window = unsafe {
            XCreateSimpleWindow(
                display,
                parent,
                0,
                0,
                1,
                1,
                0,
                COPY_FROM_PARENT,
                COPY_FROM_PARENT,
            )
        };
        if window == 0 {
            return None;
        }
        // SAFETY: an empty input rectangle list makes the child ignore pointer events so
        // WebKit keeps receiving them; the child window was just created above.
        unsafe {
            XShapeCombineRectangles(
                display,
                window,
                SHAPE_INPUT,
                0,
                0,
                std::ptr::null(),
                0,
                SHAPE_SET,
                0,
            );
            XFlush(display);
        }
        Some(Self { display, window })
    }
}

impl Drop for X11Child {
    fn drop(&mut self) {
        // SAFETY: the window was created by `X11Child::create` and is destroyed exactly once.
        unsafe {
            XUnmapWindow(self.display, self.window);
            XDestroyWindow(self.display, self.window);
            XFlush(self.display);
        }
    }
}

#[derive(Debug, Clone)]
enum LinuxWindowHandleInner {
    Xlib {
        window: c_ulong,
        /// An Xlib visual ID, or 0 if unknown (`raw-window-handle` uses a plain
        /// `c_ulong` here, unlike the XCB variant which is `Option<NonZeroU32>`).
        visual_id: c_ulong,
    },
    Xcb {
        window: NonZeroU32,
        visual_id: Option<NonZeroU32>,
    },
    Wayland {
        surface: NonNull<c_void>,
    },
}

#[derive(Debug, Clone)]
enum LinuxDisplayHandleInner {
    Xlib {
        display: Option<NonNull<c_void>>,
        screen: c_int,
    },
    Xcb {
        connection: Option<NonNull<c_void>>,
        screen: c_int,
    },
    Wayland {
        display: NonNull<c_void>,
    },
}

/// Safe handle implementing `HasWindowHandle` and `HasDisplayHandle` for wgpu surface creation on Linux.
pub struct NativeChildViewHandle {
    window_inner: LinuxWindowHandleInner,
    display_inner: LinuxDisplayHandleInner,
}

// SAFETY: Linux display and window handles/pointers are safe to send and reference across threads.
unsafe impl Send for NativeChildViewHandle {}
unsafe impl Sync for NativeChildViewHandle {}

impl HasWindowHandle for NativeChildViewHandle {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let raw = match &self.window_inner {
            LinuxWindowHandleInner::Xlib { window, visual_id } => {
                let mut handle = XlibWindowHandle::new(*window);
                handle.visual_id = *visual_id;
                RawWindowHandle::Xlib(handle)
            }
            LinuxWindowHandleInner::Xcb { window, visual_id } => {
                let mut handle = XcbWindowHandle::new(*window);
                handle.visual_id = *visual_id;
                RawWindowHandle::Xcb(handle)
            }
            LinuxWindowHandleInner::Wayland { surface } => {
                let handle = WaylandWindowHandle::new(*surface);
                RawWindowHandle::Wayland(handle)
            }
        };
        // SAFETY: The raw window handle is borrowed from a valid initialized Linux window target.
        unsafe { Ok(WindowHandle::borrow_raw(raw)) }
    }
}

impl HasDisplayHandle for NativeChildViewHandle {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        let raw = match &self.display_inner {
            LinuxDisplayHandleInner::Xlib { display, screen } => {
                let handle = XlibDisplayHandle::new(*display, *screen);
                RawDisplayHandle::Xlib(handle)
            }
            LinuxDisplayHandleInner::Xcb { connection, screen } => {
                let handle = XcbDisplayHandle::new(*connection, *screen);
                RawDisplayHandle::Xcb(handle)
            }
            LinuxDisplayHandleInner::Wayland { display } => {
                let handle = WaylandDisplayHandle::new(*display);
                RawDisplayHandle::Wayland(handle)
            }
        };
        // SAFETY: The raw display handle wraps a valid Linux X11/Wayland display connection.
        unsafe { Ok(DisplayHandle::borrow_raw(raw)) }
    }
}

enum LinuxChild {
    X11(X11Child),
    Wayland(WaylandChild),
}

/// Linux native child compositor target.
pub struct LinuxCompositorTarget {
    handle: Arc<NativeChildViewHandle>,
    child: Option<LinuxChild>,
    visibility: Mutex<ChildSurfaceVisibility>,
}

// SAFETY: `LinuxCompositorTarget` contains thread-safe handles.
unsafe impl Send for LinuxCompositorTarget {}
unsafe impl Sync for LinuxCompositorTarget {}

impl LinuxCompositorTarget {
    /// Creates a compositor target for the given Tauri window.
    pub fn new<R: Runtime>(window: &WebviewWindow<R>) -> Result<Self, NativeTerminalError> {
        let window_handle = window.window_handle().map_err(|e| {
            NativeTerminalError::GpuPipelineError(format!("Failed to get window handle: {e}"))
        })?;
        let display_handle = window.display_handle().map_err(|e| {
            NativeTerminalError::GpuPipelineError(format!("Failed to get display handle: {e}"))
        })?;

        let window_inner = match window_handle.as_raw() {
            RawWindowHandle::Xlib(h) => LinuxWindowHandleInner::Xlib {
                window: h.window,
                visual_id: h.visual_id,
            },
            RawWindowHandle::Xcb(h) => LinuxWindowHandleInner::Xcb {
                window: h.window,
                visual_id: h.visual_id,
            },
            RawWindowHandle::Wayland(h) => LinuxWindowHandleInner::Wayland { surface: h.surface },
            other => {
                return Err(NativeTerminalError::GpuPipelineError(format!(
                    "Unsupported window handle type on Linux: {other:?}"
                )));
            }
        };

        let display_inner = match display_handle.as_raw() {
            RawDisplayHandle::Xlib(d) => LinuxDisplayHandleInner::Xlib {
                display: d.display,
                screen: d.screen,
            },
            RawDisplayHandle::Xcb(d) => LinuxDisplayHandleInner::Xcb {
                connection: d.connection,
                screen: d.screen,
            },
            RawDisplayHandle::Wayland(d) => LinuxDisplayHandleInner::Wayland { display: d.display },
            other => {
                return Err(NativeTerminalError::GpuPipelineError(format!(
                    "Unsupported display handle type on Linux: {other:?}"
                )));
            }
        };

        let child = match (&window_inner, &display_inner) {
            (
                LinuxWindowHandleInner::Xlib { window, .. },
                LinuxDisplayHandleInner::Xlib { display, .. },
            ) => display
                .and_then(|display| X11Child::create(display.as_ptr(), *window))
                .map(LinuxChild::X11),
            (
                LinuxWindowHandleInner::Wayland { surface },
                LinuxDisplayHandleInner::Wayland { display },
            ) if std::env::var_os("FERRYX_DISABLE_WAYLAND_SUBSURFACE").is_none() => {
                WaylandChild::create(display.as_ptr(), *surface).map(LinuxChild::Wayland)
            }
            _ => None,
        };

        let window_inner = match (&child, window_inner) {
            (Some(LinuxChild::X11(child)), LinuxWindowHandleInner::Xlib { visual_id, .. }) => {
                LinuxWindowHandleInner::Xlib {
                    window: child.window,
                    visual_id,
                }
            }
            (Some(LinuxChild::Wayland(child)), LinuxWindowHandleInner::Wayland { .. }) => {
                LinuxWindowHandleInner::Wayland {
                    surface: child.surface(),
                }
            }
            (_, window_inner) => window_inner,
        };

        let handle = Arc::new(NativeChildViewHandle {
            window_inner,
            display_inner,
        });

        Ok(Self {
            handle,
            child,
            visibility: Mutex::new(ChildSurfaceVisibility::default()),
        })
    }

    /// Returns the raw-window-handle target for wgpu surface creation.
    pub fn surface_target(&self) -> Arc<NativeChildViewHandle> {
        Arc::clone(&self.handle)
    }

    /// Reports child capabilities only when a real isolated child surface was created, whether
    /// that is an X11 child window or a Wayland subsurface.
    pub fn descriptor(&self) -> PlatformCompositorDescriptor {
        let has_child = self.child.is_some();
        PlatformCompositorDescriptor {
            target_kind: CompositorTargetKind::LinuxChildWindow,
            pointer_transparent: has_child,
            layer_backed: has_child,
        }
    }

    pub fn update_viewport(&self, bounds: Option<LogicalBounds>) {
        let Some(child) = self.child.as_ref() else {
            return;
        };
        let Some(bounds) = bounds.as_ref() else {
            return;
        };
        match child {
            LinuxChild::X11(child) => {
                let Some(geometry) = ChildSurfaceGeometry::from_logical_bounds(bounds) else {
                    return;
                };
                // SAFETY: the child window and display belong to this target and stay valid
                // until drop.
                unsafe {
                    XMoveResizeWindow(
                        child.display,
                        child.window,
                        geometry.x,
                        geometry.y,
                        geometry.width,
                        geometry.height,
                    );
                    XFlush(child.display);
                }
            }
            LinuxChild::Wayland(child) => {
                let Some(geometry) = WaylandSubsurfaceGeometry::from_logical_bounds(bounds) else {
                    return;
                };
                child.set_geometry(
                    geometry.position_x,
                    geometry.position_y,
                    geometry.buffer_scale,
                );
            }
        }
    }

    pub fn reveal(&self) {
        let Some(child) = self.child.as_ref() else {
            return;
        };
        let Ok(mut visibility) = self.visibility.lock() else {
            return;
        };
        if !visibility.should_map_on_present() {
            return;
        }
        match child {
            LinuxChild::X11(child) => {
                // SAFETY: the child window and display belong to this target and stay valid
                // until drop.
                unsafe {
                    XMapWindow(child.display, child.window);
                    XFlush(child.display);
                }
            }
            // A subsurface has no map call: it becomes visible once a buffer is committed, and
            // wgpu has already attached one by the time `reveal` runs.
            LinuxChild::Wayland(child) => child.commit(),
        }
        visibility.mark_presented();
    }
}

impl Drop for LinuxCompositorTarget {
    fn drop(&mut self) {
        if let Ok(mut visibility) = self.visibility.lock() {
            visibility.mark_detached();
        }
    }
}
