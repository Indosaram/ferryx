//! Safe viewport scrolling and scrollback state.

use std::ffi::c_void;
use std::ptr::NonNull;

use super::composition::PhysicalBounds;
use super::error::NativeTerminalError;
use super::renderer::RectInstance;
use super::sys::ffi::{ghostty_terminal_get, ghostty_terminal_scroll_viewport};
use super::sys::types::{
    GhosttyTerminalImpl, GhosttyTerminalScrollViewport, GhosttyTerminalScrollViewportValue,
    GhosttyTerminalScrollbar, GHOSTTY_SCROLL_VIEWPORT_BOTTOM, GHOSTTY_SCROLL_VIEWPORT_DELTA,
    GHOSTTY_SCROLL_VIEWPORT_ROW, GHOSTTY_SCROLL_VIEWPORT_TOP, GHOSTTY_TERMINAL_DATA_SCROLLBAR,
};

/// Requested viewport movement.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ScrollViewport {
    Top,
    Bottom,
    Delta(isize),
    Row(usize),
}

/// Scrollbar dimensions in full-screen row coordinates.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct ScrollbarState {
    pub total: u64,
    pub offset: u64,
    pub len: u64,
}

/// Scrollbar overlay visibility and state.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct ScrollbarOverlayState {
    pub visible: bool,
    pub metrics: Option<ScrollbarState>,
}

pub const SCROLLBAR_OVERLAY_THUMB_WIDTH_PX: f32 = 6.0;
pub const SCROLLBAR_OVERLAY_RIGHT_INSET_PX: f32 = 5.0;
pub const SCROLLBAR_OVERLAY_MIN_THUMB_PX: f32 = 24.0;
pub const SCROLLBAR_OVERLAY_THUMB_ALPHA: f32 = 0.35;

pub const ATTENTION_FRAME_COLOR: [f32; 4] = [253.0 / 255.0, 230.0 / 255.0, 138.0 / 255.0, 0.95];
pub const ATTENTION_HALO_COLOR: [f32; 4] = [253.0 / 255.0, 230.0 / 255.0, 138.0 / 255.0, 0.22];
pub const ATTENTION_FRAME_THICKNESS_LOGICAL_PX: f32 = 2.0;

/// Computes physical rectangles forming a pastel glow attention frame inside the viewport edges.
///
/// When `include_bottom_band` is `true`:
/// 8 rectangles are returned (4 halo rectangles underneath, followed by 4 core rectangles).
/// When `include_bottom_band` is `false`:
/// 6 rectangles are returned (top/left/right halo and top/left/right core). The bottom core
/// and bottom halo bands are omitted, and vertical left/right bands extend through the full viewport height.
///
/// Geometry:
/// - If `viewport.width == 0` or `viewport.height == 0` or `thickness <= 0.0`, returns empty vec.
/// - Halo: thickness `3 * thickness` and color RGB(253, 230, 138) alpha 0.22.
/// - Core: thickness `thickness` and color RGB(253, 230, 138) alpha 0.95.
pub fn compute_attention_frame_rects(
    viewport: PhysicalBounds,
    thickness: f32,
    include_bottom_band: bool,
) -> Vec<RectInstance> {
    let viewport_w = viewport.width as f32;
    let viewport_h = viewport.height as f32;
    if viewport_w <= 0.0 || viewport_h <= 0.0 || thickness <= 0.0 {
        return Vec::new();
    }

    let x = viewport.x as f32;
    let y = viewport.y as f32;
    let halo_t = (thickness * 3.0).min(viewport_w / 2.0).min(viewport_h / 2.0);
    let core_t = thickness.min(viewport_w / 2.0).min(viewport_h / 2.0);

    if include_bottom_band {
        let halo_inner_h = (viewport_h - 2.0 * halo_t).max(0.0);
        let core_inner_h = (viewport_h - 2.0 * core_t).max(0.0);

        vec![
            // Halo Top
            RectInstance {
                rect: [x, y, viewport_w, halo_t],
                color: ATTENTION_HALO_COLOR,
            },
            // Halo Bottom
            RectInstance {
                rect: [x, y + viewport_h - halo_t, viewport_w, halo_t],
                color: ATTENTION_HALO_COLOR,
            },
            // Halo Left
            RectInstance {
                rect: [x, y + halo_t, halo_t, halo_inner_h],
                color: ATTENTION_HALO_COLOR,
            },
            // Halo Right
            RectInstance {
                rect: [x + viewport_w - halo_t, y + halo_t, halo_t, halo_inner_h],
                color: ATTENTION_HALO_COLOR,
            },
            // Core Top
            RectInstance {
                rect: [x, y, viewport_w, core_t],
                color: ATTENTION_FRAME_COLOR,
            },
            // Core Bottom
            RectInstance {
                rect: [x, y + viewport_h - core_t, viewport_w, core_t],
                color: ATTENTION_FRAME_COLOR,
            },
            // Core Left
            RectInstance {
                rect: [x, y + core_t, core_t, core_inner_h],
                color: ATTENTION_FRAME_COLOR,
            },
            // Core Right
            RectInstance {
                rect: [x + viewport_w - core_t, y + core_t, core_t, core_inner_h],
                color: ATTENTION_FRAME_COLOR,
            },
        ]
    } else {
        let halo_vertical_h = (viewport_h - halo_t).max(0.0);
        let core_vertical_h = (viewport_h - core_t).max(0.0);

        vec![
            // Halo Top
            RectInstance {
                rect: [x, y, viewport_w, halo_t],
                color: ATTENTION_HALO_COLOR,
            },
            // Halo Left
            RectInstance {
                rect: [x, y + halo_t, halo_t, halo_vertical_h],
                color: ATTENTION_HALO_COLOR,
            },
            // Halo Right
            RectInstance {
                rect: [x + viewport_w - halo_t, y + halo_t, halo_t, halo_vertical_h],
                color: ATTENTION_HALO_COLOR,
            },
            // Core Top
            RectInstance {
                rect: [x, y, viewport_w, core_t],
                color: ATTENTION_FRAME_COLOR,
            },
            // Core Left
            RectInstance {
                rect: [x, y + core_t, core_t, core_vertical_h],
                color: ATTENTION_FRAME_COLOR,
            },
            // Core Right
            RectInstance {
                rect: [x + viewport_w - core_t, y + core_t, core_t, core_vertical_h],
                color: ATTENTION_FRAME_COLOR,
            },
        ]
    }
}

/// Computes the physical rectangle for the GPU-rendered scrollbar thumb overlay.
///
/// Geometry:
/// - If `total == 0`, `len == 0`, or `len >= total` (no scrollback), returns `None`.
/// - Width: 6px physical.
/// - Right inset: 5px physical from viewport right edge (`viewport.x + viewport.width - 5 - 6`).
/// - Height: `max(24.0, viewport_h * len / total)` clamped to `viewport_h`.
/// - Top: `(offset / (total - len)) * (viewport_h - thumb_h)` clamped to `[0.0, available_travel]`.
/// - Color: foreground RGB with 0.35 alpha.
pub fn compute_scrollbar_overlay_rect(
    viewport: PhysicalBounds,
    total: u64,
    offset: u64,
    len: u64,
    fg_color: [f32; 4],
) -> Option<RectInstance> {
    if total == 0 || len == 0 || len >= total {
        return None;
    }
    let viewport_w = viewport.width as f32;
    let viewport_h = viewport.height as f32;
    if viewport_w <= 0.0 || viewport_h <= 0.0 {
        return None;
    }

    let raw_thumb_h = viewport_h * (len as f32 / total as f32);
    let thumb_h = raw_thumb_h
        .max(SCROLLBAR_OVERLAY_MIN_THUMB_PX)
        .min(viewport_h);
    let max_offset = (total - len) as f32;
    let scroll_ratio = if max_offset > 0.0 {
        (offset as f32 / max_offset).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let available_travel = (viewport_h - thumb_h).max(0.0);
    let thumb_top = scroll_ratio * available_travel;

    let thumb_x = viewport.x as f32 + viewport_w
        - SCROLLBAR_OVERLAY_RIGHT_INSET_PX
        - SCROLLBAR_OVERLAY_THUMB_WIDTH_PX;
    let thumb_y = viewport.y as f32 + thumb_top;

    Some(RectInstance {
        rect: [thumb_x, thumb_y, SCROLLBAR_OVERLAY_THUMB_WIDTH_PX, thumb_h],
        color: [
            fg_color[0],
            fg_color[1],
            fg_color[2],
            SCROLLBAR_OVERLAY_THUMB_ALPHA,
        ],
    })
}

pub fn scroll_viewport(handle: NonNull<GhosttyTerminalImpl>, behavior: ScrollViewport) {
    let behavior = match behavior {
        ScrollViewport::Top => GhosttyTerminalScrollViewport {
            tag: GHOSTTY_SCROLL_VIEWPORT_TOP,
            value: GhosttyTerminalScrollViewportValue { _padding: [0; 2] },
        },
        ScrollViewport::Bottom => GhosttyTerminalScrollViewport {
            tag: GHOSTTY_SCROLL_VIEWPORT_BOTTOM,
            value: GhosttyTerminalScrollViewportValue { _padding: [0; 2] },
        },
        ScrollViewport::Delta(delta) => GhosttyTerminalScrollViewport {
            tag: GHOSTTY_SCROLL_VIEWPORT_DELTA,
            value: GhosttyTerminalScrollViewportValue { delta },
        },
        ScrollViewport::Row(row) => GhosttyTerminalScrollViewport {
            tag: GHOSTTY_SCROLL_VIEWPORT_ROW,
            value: GhosttyTerminalScrollViewportValue { row },
        },
    };

    // SAFETY: Category: Foreign State Mutation.
    // Invariant: handle is a live terminal and behavior exactly matches the tagged C union layout.
    unsafe { ghostty_terminal_scroll_viewport(handle.as_ptr(), behavior) };
}

pub fn query_scrollbar(
    handle: NonNull<GhosttyTerminalImpl>,
) -> Result<ScrollbarState, NativeTerminalError> {
    let mut raw = GhosttyTerminalScrollbar::default();
    // SAFETY: Category: Foreign Data Extraction.
    // Invariant: &mut raw points to writable stack storage matching GhosttyTerminalScrollbar.
    let result = unsafe {
        ghostty_terminal_get(
            handle.as_ptr(),
            GHOSTTY_TERMINAL_DATA_SCROLLBAR,
            &mut raw as *mut GhosttyTerminalScrollbar as *mut c_void,
        )
    };
    NativeTerminalError::from_c_result(result, "ghostty_terminal_get(Scrollbar)")?;
    Ok(ScrollbarState {
        total: raw.total,
        offset: raw.offset,
        len: raw.len,
    })
}

/// Derives the number of terminal rows to scroll from a macOS scroll wheel event delta.
///
/// Sign convention:
/// - On macOS Cocoa NSEvent, rolling the wheel down (or swiping up in natural scrolling) yields `scrollingDeltaY < 0`.
/// - In Ferryx native terminal viewport coordinates, positive delta moves the viewport down toward newer content.
/// - Therefore, negative `scrolling_delta_y` translates to positive rows (scroll down toward bottom),
///   and positive `scrolling_delta_y` translates to negative rows (scroll up toward older scrollback).
///
/// Precise scrolling deltas:
/// - Generated by trackpads or continuous scroll devices in logical pixels.
/// - Divided by a cell-height constant of 10.0 logical px (matching the typical terminal font line height).
/// - Non-zero sub-cell deltas fall back to +/-1 row to ensure responsiveness.
///
/// Line scrolling deltas:
/// - Generated by traditional stepped mouse wheels (in lines).
/// - Rounded to whole rows or fall back to sign (+/-1).
pub fn macos_wheel_scroll_rows(scrolling_delta_y: f64, has_precise_deltas: bool) -> i16 {
    if scrolling_delta_y == 0.0 || !scrolling_delta_y.is_finite() {
        return 0;
    }

    const SCROLL_CELL_HEIGHT_PX: f64 = 10.0;

    let rows = if has_precise_deltas {
        let raw = (-scrolling_delta_y / SCROLL_CELL_HEIGHT_PX).trunc() as i64;
        if raw == 0 {
            if scrolling_delta_y < 0.0 {
                1
            } else {
                -1
            }
        } else {
            raw
        }
    } else {
        let raw = (-scrolling_delta_y).round() as i64;
        if raw == 0 {
            if scrolling_delta_y < 0.0 {
                1
            } else {
                -1
            }
        } else {
            raw
        }
    };

    rows.clamp(i16::MIN as i64, i16::MAX as i64) as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_macos_wheel_scroll_rows_zero_and_non_finite() {
        assert_eq!(macos_wheel_scroll_rows(0.0, true), 0);
        assert_eq!(macos_wheel_scroll_rows(0.0, false), 0);
        assert_eq!(macos_wheel_scroll_rows(-0.0, true), 0);
        assert_eq!(macos_wheel_scroll_rows(-0.0, false), 0);
        assert_eq!(macos_wheel_scroll_rows(f64::NAN, true), 0);
        assert_eq!(macos_wheel_scroll_rows(f64::INFINITY, true), 0);
        assert_eq!(macos_wheel_scroll_rows(f64::NEG_INFINITY, false), 0);
    }

    #[test]
    fn test_macos_wheel_scroll_rows_precise_sign_and_scale() {
        // Negative delta (wheel down on macOS) -> positive rows (down toward bottom/newer content)
        assert_eq!(macos_wheel_scroll_rows(-35.0, true), 3);
        assert_eq!(macos_wheel_scroll_rows(-100.0, true), 10);

        // Positive delta (wheel up on macOS) -> negative rows (up toward top/older content)
        assert_eq!(macos_wheel_scroll_rows(35.0, true), -3);
        assert_eq!(macos_wheel_scroll_rows(100.0, true), -10);

        // Sub-cell precise movement (less than 10.0 px) preserves direction via +/-1 row fallback
        assert_eq!(macos_wheel_scroll_rows(-3.0, true), 1);
        assert_eq!(macos_wheel_scroll_rows(-0.5, true), 1);
        assert_eq!(macos_wheel_scroll_rows(3.0, true), -1);
        assert_eq!(macos_wheel_scroll_rows(0.5, true), -1);
    }

    #[test]
    fn test_macos_wheel_scroll_rows_line_deltas() {
        // Line-based (notched wheel): 1 notch down -> +1 row, 1 notch up -> -1 row
        assert_eq!(macos_wheel_scroll_rows(-1.0, false), 1);
        assert_eq!(macos_wheel_scroll_rows(1.0, false), -1);
        assert_eq!(macos_wheel_scroll_rows(-4.0, false), 4);
        assert_eq!(macos_wheel_scroll_rows(4.0, false), -4);

        // Fractional line deltas
        assert_eq!(macos_wheel_scroll_rows(-0.2, false), 1);
        assert_eq!(macos_wheel_scroll_rows(0.2, false), -1);
        assert_eq!(macos_wheel_scroll_rows(-2.7, false), 3);
        assert_eq!(macos_wheel_scroll_rows(2.7, false), -3);
    }

    #[test]
    fn test_compute_scrollbar_overlay_rect_no_scrollback_returns_none() {
        let viewport = PhysicalBounds {
            x: 0,
            y: 0,
            width: 800,
            height: 600,
        };
        let fg = [1.0, 1.0, 1.0, 1.0];

        // total <= len means no scrollback
        assert_eq!(
            compute_scrollbar_overlay_rect(viewport, 24, 0, 24, fg),
            None
        );
        assert_eq!(
            compute_scrollbar_overlay_rect(viewport, 10, 0, 24, fg),
            None
        );
        assert_eq!(compute_scrollbar_overlay_rect(viewport, 0, 0, 0, fg), None);
        assert_eq!(
            compute_scrollbar_overlay_rect(viewport, 100, 0, 0, fg),
            None
        );

        // Zero viewport size
        let zero_vp = PhysicalBounds {
            x: 0,
            y: 0,
            width: 0,
            height: 600,
        };
        assert_eq!(
            compute_scrollbar_overlay_rect(zero_vp, 100, 0, 24, fg),
            None
        );
    }

    #[test]
    fn test_compute_scrollbar_overlay_rect_normal_case_exact_geometry() {
        let viewport = PhysicalBounds {
            x: 10,
            y: 20,
            width: 800,
            height: 600,
        };
        let fg = [0.8, 0.6, 0.4, 1.0];

        // total: 1000, len: 100, offset: 0 -> thumb_h = 600 * 100/1000 = 60.0px
        // thumb_x = 10 + 800 - 5 - 6 = 799.0
        // thumb_y = 20 + 0 = 20.0
        let rect_top = compute_scrollbar_overlay_rect(viewport, 1000, 0, 100, fg).unwrap();
        assert_eq!(rect_top.rect, [799.0, 20.0, 6.0, 60.0]);
        assert_eq!(rect_top.color, [0.8, 0.6, 0.4, 0.35]);

        // Scrolled all the way down: offset = 1000 - 100 = 900
        // thumb_y = 20 + (600 - 60) = 560.0
        let rect_bottom = compute_scrollbar_overlay_rect(viewport, 1000, 900, 100, fg).unwrap();
        assert_eq!(rect_bottom.rect, [799.0, 560.0, 6.0, 60.0]);

        // Scrolled halfway: offset = 450
        // thumb_y = 20 + (600 - 60) * 0.5 = 290.0
        let rect_mid = compute_scrollbar_overlay_rect(viewport, 1000, 450, 100, fg).unwrap();
        assert_eq!(rect_mid.rect, [799.0, 290.0, 6.0, 60.0]);
    }

    #[test]
    fn test_compute_scrollbar_overlay_rect_min_thumb_clamp() {
        let viewport = PhysicalBounds {
            x: 0,
            y: 0,
            width: 800,
            height: 600,
        };
        let fg = [1.0, 1.0, 1.0, 1.0];

        // total: 10000, len: 24 -> raw thumb = 600 * 24 / 10000 = 1.44px -> clamped to 24px
        let rect = compute_scrollbar_overlay_rect(viewport, 10000, 0, 24, fg).unwrap();
        assert_eq!(rect.rect[2], 6.0);
        assert_eq!(rect.rect[3], 24.0);

        // Scrolled to bottom: offset = 10000 - 24 = 9976
        // thumb_y = 600 - 24 = 576.0
        let rect_bottom = compute_scrollbar_overlay_rect(viewport, 10000, 9976, 24, fg).unwrap();
        assert_eq!(rect_bottom.rect, [789.0, 576.0, 6.0, 24.0]);
    }

    #[test]
    fn test_compute_attention_frame_rects_zero_or_negative_returns_empty() {
        let zero_w = PhysicalBounds {
            x: 10,
            y: 20,
            width: 0,
            height: 600,
        };
        assert!(compute_attention_frame_rects(zero_w, 2.0, true).is_empty());
        assert!(compute_attention_frame_rects(zero_w, 2.0, false).is_empty());

        let zero_h = PhysicalBounds {
            x: 10,
            y: 20,
            width: 800,
            height: 0,
        };
        assert!(compute_attention_frame_rects(zero_h, 2.0, true).is_empty());
        assert!(compute_attention_frame_rects(zero_h, 2.0, false).is_empty());

        let normal = PhysicalBounds {
            x: 10,
            y: 20,
            width: 800,
            height: 600,
        };
        assert!(compute_attention_frame_rects(normal, 0.0, true).is_empty());
        assert!(compute_attention_frame_rects(normal, 0.0, false).is_empty());
        assert!(compute_attention_frame_rects(normal, -1.0, true).is_empty());
        assert!(compute_attention_frame_rects(normal, -1.0, false).is_empty());
    }

    #[test]
    fn test_compute_attention_frame_rects_exact_geometry() {
        let viewport = PhysicalBounds {
            x: 10,
            y: 20,
            width: 800,
            height: 600,
        };
        let thickness = 2.0;

        // Full 4-sided frame mode (include_bottom_band = true) -> 8 rects
        let rects_full = compute_attention_frame_rects(viewport, thickness, true);
        assert_eq!(rects_full.len(), 8);

        // Halo band: 4 rects (6px thickness, alpha 0.22)
        assert_eq!(rects_full[0].rect, [10.0, 20.0, 800.0, 6.0]);
        assert_eq!(rects_full[0].color, ATTENTION_HALO_COLOR);

        assert_eq!(rects_full[1].rect, [10.0, 614.0, 800.0, 6.0]);
        assert_eq!(rects_full[1].color, ATTENTION_HALO_COLOR);

        assert_eq!(rects_full[2].rect, [10.0, 26.0, 6.0, 588.0]);
        assert_eq!(rects_full[2].color, ATTENTION_HALO_COLOR);

        assert_eq!(rects_full[3].rect, [804.0, 26.0, 6.0, 588.0]);
        assert_eq!(rects_full[3].color, ATTENTION_HALO_COLOR);

        // Core band: 4 rects (2px thickness, alpha 0.95)
        assert_eq!(rects_full[4].rect, [10.0, 20.0, 800.0, 2.0]);
        assert_eq!(rects_full[4].color, ATTENTION_FRAME_COLOR);

        assert_eq!(rects_full[5].rect, [10.0, 618.0, 800.0, 2.0]);
        assert_eq!(rects_full[5].color, ATTENTION_FRAME_COLOR);

        assert_eq!(rects_full[6].rect, [10.0, 22.0, 2.0, 596.0]);
        assert_eq!(rects_full[6].color, ATTENTION_FRAME_COLOR);

        assert_eq!(rects_full[7].rect, [808.0, 22.0, 2.0, 596.0]);
        assert_eq!(rects_full[7].color, ATTENTION_FRAME_COLOR);

        // 3-sided frame mode (include_bottom_band = false) -> 6 rects with extended verticals
        let rects_no_bottom = compute_attention_frame_rects(viewport, thickness, false);
        assert_eq!(rects_no_bottom.len(), 6);

        // Halo band: Top + Left (extended to full height: 600 - 6 = 594) + Right (extended: 594)
        assert_eq!(rects_no_bottom[0].rect, [10.0, 20.0, 800.0, 6.0]);
        assert_eq!(rects_no_bottom[0].color, ATTENTION_HALO_COLOR);

        assert_eq!(rects_no_bottom[1].rect, [10.0, 26.0, 6.0, 594.0]);
        assert_eq!(rects_no_bottom[1].color, ATTENTION_HALO_COLOR);

        assert_eq!(rects_no_bottom[2].rect, [804.0, 26.0, 6.0, 594.0]);
        assert_eq!(rects_no_bottom[2].color, ATTENTION_HALO_COLOR);

        // Core band: Top + Left (extended to full height: 600 - 2 = 598) + Right (extended: 598)
        assert_eq!(rects_no_bottom[3].rect, [10.0, 20.0, 800.0, 2.0]);
        assert_eq!(rects_no_bottom[3].color, ATTENTION_FRAME_COLOR);

        assert_eq!(rects_no_bottom[4].rect, [10.0, 22.0, 2.0, 598.0]);
        assert_eq!(rects_no_bottom[4].color, ATTENTION_FRAME_COLOR);

        assert_eq!(rects_no_bottom[5].rect, [808.0, 22.0, 2.0, 598.0]);
        assert_eq!(rects_no_bottom[5].color, ATTENTION_FRAME_COLOR);
    }
}
