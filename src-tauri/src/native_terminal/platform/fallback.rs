//! Fallback / unsupported platform compositor target for non-supported platforms.

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
};
use std::sync::Arc;
use tauri::{Runtime, WebviewWindow};

use crate::native_terminal::composition::{
    CompositorTargetKind, LogicalBounds, PlatformCompositorDescriptor,
};
use crate::native_terminal::error::NativeTerminalError;

/// Fallback dummy child view handle that returns an error on handle borrowing.
pub struct NativeChildViewHandle;

// SAFETY: Dummy handle is stateless and safe across threads.
unsafe impl Send for NativeChildViewHandle {}
unsafe impl Sync for NativeChildViewHandle {}

impl HasWindowHandle for NativeChildViewHandle {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        Err(HandleError::Unavailable)
    }
}

impl HasDisplayHandle for NativeChildViewHandle {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Err(HandleError::Unavailable)
    }
}

pub struct FallbackCompositorTarget {
    handle: Arc<NativeChildViewHandle>,
}

impl FallbackCompositorTarget {
    pub fn new<R: Runtime>(_window: &WebviewWindow<R>) -> Result<Self, NativeTerminalError> {
        Ok(Self {
            handle: Arc::new(NativeChildViewHandle),
        })
    }

    pub fn surface_target(&self) -> Arc<NativeChildViewHandle> {
        Arc::clone(&self.handle)
    }

    pub fn descriptor(&self) -> PlatformCompositorDescriptor {
        PlatformCompositorDescriptor {
            target_kind: CompositorTargetKind::UnsupportedFallback,
            pointer_transparent: false,
            layer_backed: false,
        }
    }

    pub fn update_viewport(&self, _bounds: Option<LogicalBounds>) {}
}
