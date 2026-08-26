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
//! 3. **Honest Capability Reporting**: The root Tauri window surface does not establish an isolated,
//!    layer-backed or pointer-transparent child subsurface hierarchy. The descriptor accurately reports
//!    `pointer_transparent: false` and `layer_backed: false` until Phase-4 child subsurface clipping is established.

use std::ffi::{c_int, c_ulong, c_void};
use std::num::NonZeroU32;
use std::ptr::NonNull;
use std::sync::Arc;

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle, WindowHandle, XcbDisplayHandle,
    XcbWindowHandle, XlibDisplayHandle, XlibWindowHandle,
};
use tauri::{Runtime, WebviewWindow};

use crate::native_terminal::composition::{
    CompositorTargetKind, LogicalBounds, PlatformCompositorDescriptor,
};
use crate::native_terminal::error::NativeTerminalError;

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

/// Linux native child compositor target.
pub struct LinuxCompositorTarget {
    handle: Arc<NativeChildViewHandle>,
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

        let handle = Arc::new(NativeChildViewHandle {
            window_inner,
            display_inner,
        });

        Ok(Self { handle })
    }

    /// Returns the raw-window-handle target for wgpu surface creation.
    pub fn surface_target(&self) -> Arc<NativeChildViewHandle> {
        Arc::clone(&self.handle)
    }

    /// Returns the target descriptor for this platform compositor target.
    ///
    /// Honestly reports that without an isolated X11 child window or Wayland subsurface,
    /// this target is not yet layer-backed or pointer-transparent.
    pub fn descriptor(&self) -> PlatformCompositorDescriptor {
        PlatformCompositorDescriptor {
            target_kind: CompositorTargetKind::LinuxChildWindow,
            pointer_transparent: false,
            layer_backed: false,
        }
    }

    /// Sets the child view frame to match the active terminal viewport in logical coordinates.
    ///
    /// Since child subsurface creation is pending Phase-4 implementation, viewport bounds are
    /// not applied to a subsurface and cannot clip rendering to a sub-rectangle.
    pub fn update_viewport(&self, _bounds: Option<LogicalBounds>) {}

    /// Reveals the child view. No-op on Linux until Phase-4 child subsurface clipping.
    pub fn reveal(&self) {}
}
