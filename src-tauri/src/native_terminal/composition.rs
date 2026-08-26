//! Pure-Rust surface composition layout module for native terminal embedding.

use super::error::NativeTerminalError;

/// Logical rectangle and display scale for terminal surface placement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LogicalBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub scale_factor: f64,
}

/// Frame coordinates in AppKit points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AppKitFrame {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl AppKitFrame {
    /// Returns true if the given AppKit point falls within this frame.
    #[inline]
    pub fn contains_point(&self, ax: f64, ay: f64) -> bool {
        ax >= self.x && ax < self.x + self.width && ay >= self.y && ay < self.y + self.height
    }
}

/// Physical pixel rectangle for native surface positioning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicalBounds {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl PhysicalBounds {
    /// Returns true if the given physical pixel coordinate falls within these bounds.
    #[inline]
    pub const fn contains(&self, px: u32, py: u32) -> bool {
        px >= self.x
            && px < self.x.saturating_add(self.width)
            && py >= self.y
            && py < self.y.saturating_add(self.height)
    }
}

impl LogicalBounds {
    /// Returns true if the given logical coordinate falls within these bounds.
    #[inline]
    pub fn contains(&self, lx: f64, ly: f64) -> bool {
        lx >= self.x && lx < self.x + self.width && ly >= self.y && ly < self.y + self.height
    }

    /// Computes the exact AppKit child view frame within a parent content view.
    ///
    /// DOM coordinates have origin (0, 0) at top-left with y increasing downwards.
    /// Standard AppKit NSView containers (where `is_superview_flipped == false`)
    /// have origin (0, 0) at bottom-left with y increasing upwards.
    #[inline]
    pub fn to_appkit_frame(
        &self,
        superview_height: f64,
        is_superview_flipped: bool,
    ) -> AppKitFrame {
        let appkit_y = if is_superview_flipped {
            self.y
        } else {
            superview_height - (self.y + self.height)
        };
        AppKitFrame {
            x: self.x,
            y: appkit_y,
            width: self.width,
            height: self.height,
        }
    }
}

/// Physical dimensions of a single terminal cell in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellMetrics {
    pub width_px: u32,
    pub height_px: u32,
}

/// Computed layout containing physical boundaries and terminal grid dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceCompositionLayout {
    pub physical_bounds: PhysicalBounds,
    pub cols: u16,
    pub rows: u16,
}

impl SurfaceCompositionLayout {
    /// Returns true if the given physical pixel point is within this layout's viewport.
    #[inline]
    pub const fn contains_physical_point(&self, px: u32, py: u32) -> bool {
        self.physical_bounds.contains(px, py)
    }

    /// Computes the physical pixel bounds and grid dimensions from logical bounds and cell metrics.
    pub fn compute(
        bounds: &LogicalBounds,
        cell_metrics: &CellMetrics,
    ) -> Result<Self, NativeTerminalError> {
        // Validate finite coordinates, dimensions, and scale factor
        if !bounds.x.is_finite()
            || !bounds.y.is_finite()
            || !bounds.width.is_finite()
            || !bounds.height.is_finite()
            || !bounds.scale_factor.is_finite()
        {
            return Err(NativeTerminalError::InvalidValue(
                "Logical bounds coordinates, dimensions, and scale factor must be finite numbers"
                    .to_string(),
            ));
        }

        // Validate origin coordinates are non-negative
        if bounds.x < 0.0 || bounds.y < 0.0 {
            return Err(NativeTerminalError::InvalidValue(
                "Logical bounds x and y coordinates must be non-negative".to_string(),
            ));
        }

        // Validate positive dimensions and scale factor
        if bounds.width <= 0.0 || bounds.height <= 0.0 {
            return Err(NativeTerminalError::InvalidValue(
                "Logical bounds width and height must be strictly positive".to_string(),
            ));
        }

        if bounds.scale_factor <= 0.0 {
            return Err(NativeTerminalError::InvalidValue(
                "Scale factor must be strictly positive".to_string(),
            ));
        }

        // Validate cell metrics
        if cell_metrics.width_px == 0 || cell_metrics.height_px == 0 {
            return Err(NativeTerminalError::InvalidValue(
                "Cell metrics dimensions must be strictly positive".to_string(),
            ));
        }

        // Scale to physical coordinates
        let phys_x_f64 = (bounds.x * bounds.scale_factor).round();
        let phys_y_f64 = (bounds.y * bounds.scale_factor).round();
        let phys_w_f64 = (bounds.width * bounds.scale_factor).round();
        let phys_h_f64 = (bounds.height * bounds.scale_factor).round();

        if phys_x_f64 > u32::MAX as f64
            || phys_y_f64 > u32::MAX as f64
            || phys_w_f64 > u32::MAX as f64
            || phys_h_f64 > u32::MAX as f64
        {
            return Err(NativeTerminalError::LimitExceeded);
        }

        let physical_x = phys_x_f64 as u32;
        let physical_y = phys_y_f64 as u32;
        let physical_width = phys_w_f64 as u32;
        let physical_height = phys_h_f64 as u32;

        if physical_width == 0 || physical_height == 0 {
            return Err(NativeTerminalError::InvalidDimensions(0, 0));
        }

        let cols = physical_width / cell_metrics.width_px;
        let rows = physical_height / cell_metrics.height_px;

        if cols == 0 || rows == 0 {
            return Err(NativeTerminalError::InvalidDimensions(
                cols.min(u16::MAX as u32) as u16,
                rows.min(u16::MAX as u32) as u16,
            ));
        }

        let cols_u16 = u16::try_from(cols).map_err(|_| NativeTerminalError::LimitExceeded)?;
        let rows_u16 = u16::try_from(rows).map_err(|_| NativeTerminalError::LimitExceeded)?;

        Ok(Self {
            physical_bounds: PhysicalBounds {
                x: physical_x,
                y: physical_y,
                width: physical_width,
                height: physical_height,
            },
            cols: cols_u16,
            rows: rows_u16,
        })
    }
}

/// The kind of native surface composition target used for terminal rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CompositorTargetKind {
    /// Host target is the whole Tauri WebviewWindow.
    /// Occluded on macOS because WKWebView sits in front of the window-level layer.
    RootWebviewWindow,
    /// Dedicated platform layer-backed child view positioned above the webview (macOS NSView).
    NativeChildView,
    /// Dedicated Windows child composition target / window (DirectComposition / Win32 HWND).
    WindowsChildWindow,
    /// Dedicated Linux child composition target / window (X11 subwindow / Wayland subsurface).
    LinuxChildWindow,
    /// Fallback / unsupported platform target.
    UnsupportedFallback,
}

impl CompositorTargetKind {
    pub const fn is_child_compositor(&self) -> bool {
        matches!(
            self,
            Self::NativeChildView | Self::WindowsChildWindow | Self::LinuxChildWindow
        )
    }
}

/// Pure descriptor validating platform composition properties according to Section 7.1/7.3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PlatformCompositorDescriptor {
    pub target_kind: CompositorTargetKind,
    pub pointer_transparent: bool,
    pub layer_backed: bool,
}

impl PlatformCompositorDescriptor {
    /// Returns the active compositor descriptor configured for the current platform.
    pub fn active_for_platform() -> Self {
        #[cfg(target_os = "macos")]
        {
            Self {
                target_kind: CompositorTargetKind::NativeChildView,
                pointer_transparent: true,
                layer_backed: true,
            }
        }
        #[cfg(target_os = "windows")]
        {
            Self {
                target_kind: CompositorTargetKind::WindowsChildWindow,
                pointer_transparent: false,
                layer_backed: false,
            }
        }
        #[cfg(target_os = "linux")]
        {
            Self {
                target_kind: CompositorTargetKind::LinuxChildWindow,
                pointer_transparent: false,
                layer_backed: false,
            }
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            Self {
                target_kind: CompositorTargetKind::UnsupportedFallback,
                pointer_transparent: false,
                layer_backed: false,
            }
        }
    }

    /// Validates that this descriptor meets Phase-3 desktop composition invariants.
    pub fn validate_desktop_composition(&self) -> Result<(), NativeTerminalError> {
        if self.target_kind == CompositorTargetKind::RootWebviewWindow {
            return Err(NativeTerminalError::GpuPipelineError(
                "Root WebviewWindow cannot be used as native terminal composition target because WKWebView occludes the layer".into(),
            ));
        }
        if self.target_kind == CompositorTargetKind::UnsupportedFallback {
            return Err(NativeTerminalError::GpuPipelineError(
                "Unsupported platform for native terminal composition".into(),
            ));
        }
        match self.target_kind {
            CompositorTargetKind::NativeChildView => {
                if !self.layer_backed {
                    return Err(NativeTerminalError::GpuPipelineError(
                        "Native terminal composition target must be layer-backed".into(),
                    ));
                }
                if !self.pointer_transparent {
                    return Err(NativeTerminalError::GpuPipelineError(
                        "Native terminal child view must be pointer-transparent for web focus routing".into(),
                    ));
                }
            }
            CompositorTargetKind::WindowsChildWindow => {
                if !self.layer_backed {
                    return Err(NativeTerminalError::GpuPipelineError(
                        "Windows native terminal composition target is not layer-backed (child window clipping not established)".into(),
                    ));
                }
                if !self.pointer_transparent {
                    return Err(NativeTerminalError::GpuPipelineError(
                        "Windows native terminal child window is not pointer-transparent for web focus routing (child window clipping not established)".into(),
                    ));
                }
            }
            CompositorTargetKind::LinuxChildWindow => {
                if !self.layer_backed {
                    return Err(NativeTerminalError::GpuPipelineError(
                        "Linux native terminal composition target is not layer-backed (subsurface clipping not established)".into(),
                    ));
                }
                if !self.pointer_transparent {
                    return Err(NativeTerminalError::GpuPipelineError(
                        "Linux native terminal child window is not pointer-transparent for web focus routing (subsurface clipping not established)".into(),
                    ));
                }
            }
            CompositorTargetKind::RootWebviewWindow | CompositorTargetKind::UnsupportedFallback => {
                unreachable!()
            }
        }
        Ok(())
    }

    /// Evaluates whether this compositor descriptor could intercept pointer or keyboard events
    /// outside the active terminal viewport.
    ///
    /// When `pointer_transparent` is true, the native child view cannot intercept AppKit events,
    /// allowing clicks on TabBar, New Tab, Pane Split, and Sidebar controls to reach React handlers.
    #[inline]
    pub const fn can_intercept_event_outside_viewport(&self) -> bool {
        !self.pointer_transparent
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_desktop_composition_accepts_supported_platform_kinds() {
        let macos_desc = PlatformCompositorDescriptor {
            target_kind: CompositorTargetKind::NativeChildView,
            pointer_transparent: true,
            layer_backed: true,
        };
        assert!(macos_desc.validate_desktop_composition().is_ok());

        let windows_desc = PlatformCompositorDescriptor {
            target_kind: CompositorTargetKind::WindowsChildWindow,
            pointer_transparent: true,
            layer_backed: true,
        };
        assert!(windows_desc.validate_desktop_composition().is_ok());

        let linux_desc = PlatformCompositorDescriptor {
            target_kind: CompositorTargetKind::LinuxChildWindow,
            pointer_transparent: true,
            layer_backed: true,
        };
        assert!(linux_desc.validate_desktop_composition().is_ok());
    }

    #[test]
    fn test_validate_desktop_composition_rejects_unsupported_fallback() {
        let fallback_desc = PlatformCompositorDescriptor {
            target_kind: CompositorTargetKind::UnsupportedFallback,
            pointer_transparent: false,
            layer_backed: false,
        };
        let err = fallback_desc.validate_desktop_composition().unwrap_err();
        assert!(
            matches!(&err, NativeTerminalError::GpuPipelineError(msg) if msg.contains("Unsupported platform")),
            "Expected Unsupported platform error, got: {err:?}"
        );
    }

    #[test]
    fn test_validate_desktop_composition_rejects_root_webview_window() {
        let root_desc = PlatformCompositorDescriptor {
            target_kind: CompositorTargetKind::RootWebviewWindow,
            pointer_transparent: true,
            layer_backed: true,
        };
        let err = root_desc.validate_desktop_composition().unwrap_err();
        assert!(
            matches!(&err, NativeTerminalError::GpuPipelineError(msg) if msg.contains("Root WebviewWindow")),
            "Expected Root WebviewWindow error, got: {err:?}"
        );
    }

    #[test]
    fn test_validate_desktop_composition_rejects_missing_layer_backing() {
        let desc = PlatformCompositorDescriptor {
            target_kind: CompositorTargetKind::WindowsChildWindow,
            pointer_transparent: true,
            layer_backed: false,
        };
        let err = desc.validate_desktop_composition().unwrap_err();
        assert!(
            matches!(&err, NativeTerminalError::GpuPipelineError(msg) if msg.contains("layer-backed")),
            "Expected layer-backed error, got: {err:?}"
        );
    }

    #[test]
    fn test_validate_desktop_composition_rejects_missing_pointer_transparency() {
        let desc = PlatformCompositorDescriptor {
            target_kind: CompositorTargetKind::LinuxChildWindow,
            pointer_transparent: false,
            layer_backed: true,
        };
        let err = desc.validate_desktop_composition().unwrap_err();
        assert!(
            matches!(&err, NativeTerminalError::GpuPipelineError(msg) if msg.contains("pointer-transparent")),
            "Expected pointer-transparent error, got: {err:?}"
        );
    }

    #[test]
    fn test_active_for_platform_honest_descriptor_and_validation() {
        let active = PlatformCompositorDescriptor::active_for_platform();
        #[cfg(target_os = "macos")]
        {
            assert_eq!(active.target_kind, CompositorTargetKind::NativeChildView);
            assert!(
                active.pointer_transparent,
                "macOS must honestly report pointer_transparent"
            );
            assert!(
                active.layer_backed,
                "macOS must honestly report layer_backed"
            );
            assert!(active.validate_desktop_composition().is_ok());
        }
        #[cfg(target_os = "windows")]
        {
            assert_eq!(active.target_kind, CompositorTargetKind::WindowsChildWindow);
            assert!(
                !active.pointer_transparent,
                "Windows root handle must not falsely report pointer_transparent"
            );
            assert!(
                !active.layer_backed,
                "Windows root handle must not falsely report layer_backed"
            );
            let err = active.validate_desktop_composition().unwrap_err();
            assert!(
                matches!(&err, NativeTerminalError::GpuPipelineError(msg) if msg.contains("Windows native terminal")),
                "Expected Windows validation error, got: {err:?}"
            );
        }
        #[cfg(target_os = "linux")]
        {
            assert_eq!(active.target_kind, CompositorTargetKind::LinuxChildWindow);
            assert!(
                !active.pointer_transparent,
                "Linux root handle must not falsely report pointer_transparent"
            );
            assert!(
                !active.layer_backed,
                "Linux root handle must not falsely report layer_backed"
            );
            let err = active.validate_desktop_composition().unwrap_err();
            assert!(
                matches!(&err, NativeTerminalError::GpuPipelineError(msg) if msg.contains("Linux native terminal")),
                "Expected Linux validation error, got: {err:?}"
            );
        }
    }

    #[test]
    fn test_is_child_compositor() {
        assert!(CompositorTargetKind::NativeChildView.is_child_compositor());
        assert!(CompositorTargetKind::WindowsChildWindow.is_child_compositor());
        assert!(CompositorTargetKind::LinuxChildWindow.is_child_compositor());
        assert!(!CompositorTargetKind::RootWebviewWindow.is_child_compositor());
        assert!(!CompositorTargetKind::UnsupportedFallback.is_child_compositor());
    }
}
