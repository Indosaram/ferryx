//! Platform-specific compositor targets for native terminal rendering.

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(all(target_os = "windows", feature = "native-terminal"))]
pub mod windows_focus;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "linux")]
mod wayland_child;

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub mod fallback;

use std::sync::Arc;
use tauri::{Runtime, WebviewWindow};

use crate::native_terminal::composition::{LogicalBounds, PlatformCompositorDescriptor};
use crate::native_terminal::error::NativeTerminalError;

#[cfg(target_os = "macos")]
pub use macos::NativeChildViewHandle;

#[cfg(target_os = "windows")]
pub use windows::NativeChildViewHandle;

#[cfg(all(target_os = "windows", feature = "native-terminal"))]
pub use windows_focus::{
    install_windows_terminal_focus_monitor, uninstall_windows_terminal_focus_monitor,
};

#[cfg(target_os = "linux")]
pub use linux::NativeChildViewHandle;

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub use fallback::NativeChildViewHandle;

/// Platform-agnostic compositor target wrapping platform-specific child view handles.
pub struct PlatformCompositorTarget {
    #[cfg(target_os = "macos")]
    inner: macos::MacosCompositorTarget,
    #[cfg(target_os = "windows")]
    inner: windows::WindowsCompositorTarget,
    #[cfg(target_os = "linux")]
    inner: linux::LinuxCompositorTarget,
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    inner: fallback::FallbackCompositorTarget,
}

impl PlatformCompositorTarget {
    #[cfg(target_os = "macos")]
    pub fn window_backing_scale_factor(&self) -> f64 {
        self.inner.window_backing_scale_factor()
    }

    /// Creates and attaches the platform compositor target for the given window.
    pub fn new<R: Runtime>(window: &WebviewWindow<R>) -> Result<Self, NativeTerminalError> {
        #[cfg(target_os = "macos")]
        {
            let inner = macos::MacosCompositorTarget::new(window)?;
            Ok(Self { inner })
        }
        #[cfg(target_os = "windows")]
        {
            let inner = windows::WindowsCompositorTarget::new(window)?;
            Ok(Self { inner })
        }
        #[cfg(target_os = "linux")]
        {
            let inner = linux::LinuxCompositorTarget::new(window)?;
            Ok(Self { inner })
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            let inner = fallback::FallbackCompositorTarget::new(window)?;
            Ok(Self { inner })
        }
    }

    /// Returns the target descriptor for this platform compositor target.
    pub fn descriptor(&self) -> PlatformCompositorDescriptor {
        self.inner.descriptor()
    }

    /// Sets the child view frame to match the active terminal viewport in platform coordinates.
    pub fn update_viewport(&self, bounds: Option<LogicalBounds>) {
        self.inner.update_viewport(bounds);
    }

    /// Reveals the compositor target after a frame has been presented successfully.
    ///
    /// Every child-view platform creates its surface hidden so the compositor never shows
    /// an unconfigured swapchain.
    pub fn reveal_after_present(&self) {
        #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
        self.inner.reveal();
    }

    /// Restores the hosting WKWebView as the window's first responder on macOS.
    pub fn restore_first_responder<R: Runtime>(&self, _window: &WebviewWindow<R>) {
        #[cfg(target_os = "macos")]
        self.inner.restore_webview_first_responder(_window);
    }

    /// Returns the raw-window-handle target for wgpu surface creation.
    pub fn surface_target(&self) -> Arc<NativeChildViewHandle> {
        self.inner.surface_target()
    }
}
