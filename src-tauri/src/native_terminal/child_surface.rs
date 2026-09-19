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

/// Remembers the geometry last pushed to the compositor so identical bounds stop at the
/// boundary instead of becoming a platform call.
///
/// Every frame re-sends the pane rectangle, but the rectangle only changes when the pane is
/// actually resized. Without this latch each frame issues a `SetWindowPos` / `setFrame` /
/// `wl_subsurface.set_position` that asks the compositor to move a surface to where it already
/// is, which is a per-frame relayout for every visible pane.
#[derive(Debug, Default)]
pub struct GeometryLatch {
    last_applied: std::sync::Mutex<Option<ChildSurfaceGeometry>>,
}

impl GeometryLatch {
    /// Returns true only when `next` differs from the last applied geometry, recording it.
    pub fn needs_apply(&self, next: ChildSurfaceGeometry) -> bool {
        let Ok(mut last_applied) = self.last_applied.lock() else {
            // A poisoned latch must not silently freeze the surface in a stale position.
            return true;
        };
        if *last_applied == Some(next) {
            return false;
        }
        *last_applied = Some(next);
        true
    }

    /// Drops the memo so the next call re-applies, for reattach and surface recreation where
    /// the compositor no longer holds the geometry we think it does.
    pub fn invalidate(&self) {
        if let Ok(mut last_applied) = self.last_applied.lock() {
            *last_applied = None;
        }
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

        // Without a viewport destination, both the position and extent must be integral
        // surface-local coordinates. Snap shared edges, not independent sizes, so adjacent
        // panes agree. Multiply only afterwards to keep buffers divisible by their scale.
        let buffer_scale = bounds.scale_factor.round().max(1.0);
        if buffer_scale > i32::MAX as f64 || bounds.width <= 0.0 || bounds.height <= 0.0 {
            return None;
        }
        let position_x = bounds.x.round().clamp(0.0, i32::MAX as f64);
        let position_y = bounds.y.round().clamp(0.0, i32::MAX as f64);
        let right = (bounds.x.max(0.0) + bounds.width).round();
        let bottom = (bounds.y.max(0.0) + bounds.height).round();
        let width = (right - position_x) * buffer_scale;
        let height = (bottom - position_y) * buffer_scale;
        if width < 1.0 || height < 1.0 || width > u32::MAX as f64 || height > u32::MAX as f64 {
            return None;
        }

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

#[cfg(test)]
mod geometry_latch_tests {
    use super::{ChildSurfaceGeometry, GeometryLatch};

    fn geometry(x: i32, width: u32) -> ChildSurfaceGeometry {
        ChildSurfaceGeometry { x, y: 0, width, height: 600 }
    }

    #[test]
    fn repeated_identical_geometry_applies_exactly_once() {
        let latch = GeometryLatch::default();
        let bounds = geometry(10, 800);
        assert!(latch.needs_apply(bounds), "first placement must reach the compositor");
        let extra = (0..64).filter(|_| latch.needs_apply(bounds)).count();
        assert_eq!(extra, 0, "re-sending identical bounds must not mutate the platform surface");
    }

    #[test]
    fn a_changed_rectangle_applies_again() {
        let latch = GeometryLatch::default();
        assert!(latch.needs_apply(geometry(10, 800)));
        assert!(latch.needs_apply(geometry(10, 801)), "a resize must reach the compositor");
        assert!(latch.needs_apply(geometry(11, 801)), "a move must reach the compositor");
        assert!(!latch.needs_apply(geometry(11, 801)), "and then settle");
    }

    #[test]
    fn invalidate_forces_reapply_after_reattach() {
        let latch = GeometryLatch::default();
        let bounds = geometry(10, 800);
        assert!(latch.needs_apply(bounds));
        assert!(!latch.needs_apply(bounds));
        latch.invalidate();
        assert!(latch.needs_apply(bounds), "after reattach the compositor no longer holds our geometry");
    }
}
