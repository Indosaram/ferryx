//! Pure placement and reveal rules shared by the Windows and Linux native child surfaces.

use crate::native_terminal::composition::LogicalBounds;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChildSurfaceGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl ChildSurfaceGeometry {
    pub fn from_logical_bounds(bounds: &LogicalBounds) -> Option<Self> {
        if !bounds.x.is_finite()
            || !bounds.y.is_finite()
            || !bounds.width.is_finite()
            || !bounds.height.is_finite()
            || !bounds.scale_factor.is_finite()
            || bounds.scale_factor <= 0.0
        {
            return None;
        }

        let scale = bounds.scale_factor;
        let width = (bounds.width * scale).round();
        let height = (bounds.height * scale).round();
        if width < 1.0 || height < 1.0 || width > u32::MAX as f64 || height > u32::MAX as f64 {
            return None;
        }

        let x = (bounds.x * scale).round().clamp(0.0, i32::MAX as f64);
        let y = (bounds.y * scale).round().clamp(0.0, i32::MAX as f64);

        Some(Self {
            x: x as i32,
            y: y as i32,
            width: width as u32,
            height: height as u32,
        })
    }
}

/// Placement for a `wl_subsurface`, whose position is parent-surface-local **logical**
/// coordinates while its buffer is sized in physical pixels and divided back down by the
/// integer `wl_surface.set_buffer_scale`. This is why the X11/Win32 `ChildSurfaceGeometry`
/// cannot be reused: scaling the origin would offset the terminal on any HiDPI output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaylandSubsurfaceGeometry {
    pub position_x: i32,
    pub position_y: i32,
    pub buffer_scale: i32,
    pub physical_width: u32,
    pub physical_height: u32,
}

impl WaylandSubsurfaceGeometry {
    pub fn from_logical_bounds(bounds: &LogicalBounds) -> Option<Self> {
        if !bounds.x.is_finite()
            || !bounds.y.is_finite()
            || !bounds.width.is_finite()
            || !bounds.height.is_finite()
            || !bounds.scale_factor.is_finite()
            || bounds.scale_factor <= 0.0
        {
            return None;
        }

        // `set_buffer_scale` only accepts integers, so a 1.5x output renders at 2x and is
        // downscaled by the compositor rather than rendering blurry at 1x.
        let buffer_scale = bounds.scale_factor.round().max(1.0);
        let width = (bounds.width * buffer_scale).round();
        let height = (bounds.height * buffer_scale).round();
        if width < 1.0 || height < 1.0 || width > u32::MAX as f64 || height > u32::MAX as f64 {
            return None;
        }

        let position_x = bounds.x.round().clamp(0.0, i32::MAX as f64);
        let position_y = bounds.y.round().clamp(0.0, i32::MAX as f64);

        Some(Self {
            position_x: position_x as i32,
            position_y: position_y as i32,
            buffer_scale: buffer_scale as i32,
            physical_width: width as u32,
            physical_height: height as u32,
        })
    }
}

/// Created hidden so the compositor never shows an unconfigured swapchain; mapped by
/// the first successful present; once detached never resurrected by an in-flight present.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ChildSurfaceVisibility {
    mapped: bool,
    detached: bool,
}

impl ChildSurfaceVisibility {
    pub fn is_visible(&self) -> bool {
        self.mapped && !self.detached
    }

    pub fn should_map_on_present(&self) -> bool {
        !self.mapped && !self.detached
    }

    pub fn mark_presented(&mut self) {
        if !self.detached {
            self.mapped = true;
        }
    }

    pub fn mark_detached(&mut self) {
        self.detached = true;
        self.mapped = false;
    }
}
