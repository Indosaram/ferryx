//! Windows Platform Compositor Target for Native Terminal.
//!
//! Provides a Win32 HWND composition surface target implementing `HasWindowHandle`
//! and `HasDisplayHandle` for wgpu surface creation.
//!
//! # Safety Invariants
//!
//! 1. **Handle Lifetime**: The `NativeChildViewHandle` retains the HWND handle obtained
//!    from the parent window, valid for the lifetime of the compositor target.
//! 2. **Thread Safety**: Win32 window handles and Windows display handles are thread-safe
//!    to borrow and query across threads.
//! 3. **Honest Capability Reporting**: The root Tauri window HWND does not establish an isolated,
//!    layer-backed or pointer-transparent child view hierarchy. The descriptor accurately reports
//!    `pointer_transparent: false` and `layer_backed: false` until Phase-4 child HWND clipping is established.

use std::num::NonZeroIsize;
use std::sync::Arc;

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, Win32WindowHandle, WindowHandle, WindowsDisplayHandle,
};
use tauri::{Runtime, WebviewWindow};

use crate::native_terminal::composition::{
    CompositorTargetKind, LogicalBounds, PlatformCompositorDescriptor,
};
use crate::native_terminal::error::NativeTerminalError;

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
        // SAFETY: The Win32 window handle wraps a valid HWND obtained from the parent window.
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

/// Windows native child compositor target.
pub struct WindowsCompositorTarget {
    handle: Arc<NativeChildViewHandle>,
}

// SAFETY: `WindowsCompositorTarget` contains thread-safe handles.
unsafe impl Send for WindowsCompositorTarget {}
unsafe impl Sync for WindowsCompositorTarget {}

impl WindowsCompositorTarget {
    /// Creates a compositor target for the given Tauri window.
    pub fn new<R: Runtime>(window: &WebviewWindow<R>) -> Result<Self, NativeTerminalError> {
        let window_handle = window.window_handle().map_err(|e| {
            NativeTerminalError::GpuPipelineError(format!("Failed to get window handle: {e}"))
        })?;

        let (hwnd, hinstance) = match window_handle.as_raw() {
            RawWindowHandle::Win32(handle) => (handle.hwnd, handle.hinstance),
            _ => {
                return Err(NativeTerminalError::GpuPipelineError(
                    "Expected Win32 window handle on Windows".into(),
                ));
            }
        };

        // Ensure display handle is valid
        let _ = window.display_handle().map_err(|e| {
            NativeTerminalError::GpuPipelineError(format!("Failed to get display handle: {e}"))
        })?;

        let handle = Arc::new(NativeChildViewHandle { hwnd, hinstance });

        Ok(Self { handle })
    }

    /// Returns the raw-window-handle target for wgpu surface creation.
    pub fn surface_target(&self) -> Arc<NativeChildViewHandle> {
        Arc::clone(&self.handle)
    }

    /// Returns the target descriptor for this platform compositor target.
    ///
    /// Honestly reports that without an isolated child HWND, this target is not yet
    /// layer-backed or pointer-transparent.
    pub fn descriptor(&self) -> PlatformCompositorDescriptor {
        PlatformCompositorDescriptor {
            target_kind: CompositorTargetKind::WindowsChildWindow,
            pointer_transparent: false,
            layer_backed: false,
        }
    }

    /// Sets the child view frame to match the active terminal viewport in logical coordinates.
    ///
    /// Since child HWND creation is pending Phase-4 implementation, viewport bounds are not
    /// applied to a child window and cannot clip rendering to a sub-rectangle.
    pub fn update_viewport(&self, _bounds: Option<LogicalBounds>) {}

    /// Reveals the child view. No-op on Windows until Phase-4 child HWND clipping.
    pub fn reveal(&self) {}
}
