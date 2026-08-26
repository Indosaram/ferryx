//! Phase 3 Native Terminal Surface Composition Contract Tests.
//!
//! Validates the pure-Rust contract for embedding a single native terminal rectangle:
//! - Typed logical bounds input (x, y, width, height, scale_factor)
//! - Validation rejecting zero, non-finite, and excessive values
//! - Deterministic conversion from logical coordinates to physical pixels
//! - Deterministic terminal grid dimension calculations (cols, rows) for given cell sizes (e.g. 10x20 px)
//! - Viewport/sub-rectangle positioning preserving non-zero origins

use ferryx_lib::native_terminal::composition::{
    CellMetrics, LogicalBounds, PhysicalBounds, SurfaceCompositionLayout,
};
use ferryx_lib::native_terminal::NativeTerminalError;

#[test]
fn test_composition_bounds_valid_conversion_and_grid_calculation() {
    // 800x600 logical surface with 10x20 physical cells at scale 1.0
    let bounds = LogicalBounds {
        x: 0.0,
        y: 0.0,
        width: 800.0,
        height: 600.0,
        scale_factor: 1.0,
    };
    let cell_metrics = CellMetrics {
        width_px: 10,
        height_px: 20,
    };

    let layout = SurfaceCompositionLayout::compute(&bounds, &cell_metrics)
        .expect("valid logical bounds and cell metrics must compute layout");

    assert_eq!(
        layout.physical_bounds,
        PhysicalBounds {
            x: 0,
            y: 0,
            width: 800,
            height: 600,
        }
    );
    assert_eq!(layout.cols, 80);
    assert_eq!(layout.rows, 30);

    // HiDPI scale factor 2.0: 800x600 logical becomes 1600x1200 physical pixels
    // With 10x20 cells: 1600 / 10 = 160 cols, 1200 / 20 = 60 rows
    let hidpi_bounds = LogicalBounds {
        x: 0.0,
        y: 0.0,
        width: 800.0,
        height: 600.0,
        scale_factor: 2.0,
    };
    let hidpi_layout = SurfaceCompositionLayout::compute(&hidpi_bounds, &cell_metrics)
        .expect("HiDPI layout calculation must succeed");

    assert_eq!(
        hidpi_layout.physical_bounds,
        PhysicalBounds {
            x: 0,
            y: 0,
            width: 1600,
            height: 1200,
        }
    );
    assert_eq!(hidpi_layout.cols, 160);
    assert_eq!(hidpi_layout.rows, 60);
}

#[test]
fn test_composition_sub_rectangle_preserves_nonzero_origin() {
    // Viewport / sub-rectangle positioned within a host window at a nonzero logical offset
    let sub_rect = LogicalBounds {
        x: 120.0,
        y: 64.0,
        width: 800.0,
        height: 480.0,
        scale_factor: 2.0,
    };
    let cell_metrics = CellMetrics {
        width_px: 10,
        height_px: 20,
    };

    let layout = SurfaceCompositionLayout::compute(&sub_rect, &cell_metrics)
        .expect("sub-rectangle composition layout must succeed");

    // Nonzero origin must be retained precisely after scaling
    assert_eq!(layout.physical_bounds.x, 240);
    assert_eq!(layout.physical_bounds.y, 128);
    assert_eq!(layout.physical_bounds.width, 1600);
    assert_eq!(layout.physical_bounds.height, 960);
    assert_eq!(layout.cols, 160);
    assert_eq!(layout.rows, 48);
}

#[test]
fn test_composition_bounds_validation_rejects_zero_and_negative_dimensions() {
    let cell_metrics = CellMetrics {
        width_px: 10,
        height_px: 20,
    };

    let invalid_cases = [
        (
            LogicalBounds {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 600.0,
                scale_factor: 1.0,
            },
            "zero width",
        ),
        (
            LogicalBounds {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 0.0,
                scale_factor: 1.0,
            },
            "zero height",
        ),
        (
            LogicalBounds {
                x: 0.0,
                y: 0.0,
                width: -100.0,
                height: 600.0,
                scale_factor: 1.0,
            },
            "negative width",
        ),
        (
            LogicalBounds {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: -200.0,
                scale_factor: 1.0,
            },
            "negative height",
        ),
        (
            LogicalBounds {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 600.0,
                scale_factor: 0.0,
            },
            "zero scale factor",
        ),
        (
            LogicalBounds {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 600.0,
                scale_factor: -1.5,
            },
            "negative scale factor",
        ),
    ];

    for (bounds, desc) in invalid_cases {
        let res = SurfaceCompositionLayout::compute(&bounds, &cell_metrics);
        assert!(
            matches!(
                res,
                Err(NativeTerminalError::InvalidValue(_))
                    | Err(NativeTerminalError::InvalidDimensions(_, _))
            ),
            "Expected InvalidValue or InvalidDimensions error for {desc}, got: {res:?}"
        );
    }
}

#[test]
fn test_composition_bounds_validation_rejects_non_finite_values() {
    let cell_metrics = CellMetrics {
        width_px: 10,
        height_px: 20,
    };

    let non_finite_cases = [
        (
            LogicalBounds {
                x: f64::NAN,
                y: 0.0,
                width: 800.0,
                height: 600.0,
                scale_factor: 1.0,
            },
            "NaN x coordinate",
        ),
        (
            LogicalBounds {
                x: 0.0,
                y: f64::INFINITY,
                width: 800.0,
                height: 600.0,
                scale_factor: 1.0,
            },
            "infinite y coordinate",
        ),
        (
            LogicalBounds {
                x: 0.0,
                y: 0.0,
                width: f64::NAN,
                height: 600.0,
                scale_factor: 1.0,
            },
            "NaN width",
        ),
        (
            LogicalBounds {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: f64::INFINITY,
                scale_factor: 1.0,
            },
            "infinite height",
        ),
        (
            LogicalBounds {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 600.0,
                scale_factor: f64::NAN,
            },
            "NaN scale factor",
        ),
    ];

    for (bounds, desc) in non_finite_cases {
        let res = SurfaceCompositionLayout::compute(&bounds, &cell_metrics);
        assert!(
            matches!(res, Err(NativeTerminalError::InvalidValue(_))),
            "Expected InvalidValue error for {desc}, got: {res:?}"
        );
    }
}

#[test]
fn test_composition_bounds_validation_rejects_excessive_dimensions() {
    let cell_metrics = CellMetrics {
        width_px: 10,
        height_px: 20,
    };

    let excessive_cases = [
        (
            LogicalBounds {
                x: 0.0,
                y: 0.0,
                width: 1e9,
                height: 600.0,
                scale_factor: 1.0,
            },
            "excessive logical width",
        ),
        (
            LogicalBounds {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 1e9,
                scale_factor: 1.0,
            },
            "excessive logical height",
        ),
        (
            LogicalBounds {
                x: 0.0,
                y: 0.0,
                width: 655360.0, // cols calculation would exceed u16::MAX
                height: 600.0,
                scale_factor: 1.0,
            },
            "logical width exceeding u16 cols limit",
        ),
    ];

    for (bounds, desc) in excessive_cases {
        let res = SurfaceCompositionLayout::compute(&bounds, &cell_metrics);
        assert!(
            matches!(
                res,
                Err(NativeTerminalError::InvalidValue(_))
                    | Err(NativeTerminalError::InvalidDimensions(_, _))
                    | Err(NativeTerminalError::LimitExceeded)
            ),
            "Expected validation rejection for {desc}, got: {res:?}"
        );
    }
}

#[test]
fn test_viewport_containment_and_interaction_boundary_seam() {
    let logical_bounds = LogicalBounds {
        x: 220.0,
        y: 38.0,
        width: 800.0,
        height: 600.0,
        scale_factor: 2.0,
    };
    let cell_metrics = CellMetrics {
        width_px: 10,
        height_px: 20,
    };
    let layout = SurfaceCompositionLayout::compute(&logical_bounds, &cell_metrics)
        .expect("valid composition layout");

    // Physical bounds: x=440, y=76, w=1600, h=1200
    assert_eq!(layout.physical_bounds.x, 440);
    assert_eq!(layout.physical_bounds.y, 76);
    assert_eq!(layout.physical_bounds.width, 1600);
    assert_eq!(layout.physical_bounds.height, 1200);

    // Points inside terminal viewport
    assert!(layout.contains_physical_point(440, 76));
    assert!(layout.contains_physical_point(500, 200));
    assert!(layout.contains_physical_point(2039, 1275));
    assert!(logical_bounds.contains(220.0, 38.0));
    assert!(logical_bounds.contains(500.0, 300.0));

    // Points outside terminal viewport (TabBar, New Tab button, Pane split controls, Sidebar, Settings)
    // 1. TabBar / New Tab button (y < 38)
    assert!(!layout.contains_physical_point(440, 30));
    assert!(!layout.contains_physical_point(600, 10));
    assert!(!logical_bounds.contains(300.0, 15.0));

    // 2. Sidebar (x < 220)
    assert!(!layout.contains_physical_point(100, 200));
    assert!(!logical_bounds.contains(100.0, 200.0));

    // 3. Right / Bottom boundary overshoot
    assert!(!layout.contains_physical_point(2040, 500));
    assert!(!layout.contains_physical_point(500, 1276));
    assert!(!logical_bounds.contains(1020.1, 300.0));
    assert!(!logical_bounds.contains(500.0, 638.1));

    // Platform interaction boundary contract:
    // Active macOS compositor is pointer-transparent and cannot intercept events outside its viewport
    let descriptor =
        ferryx_lib::native_terminal::composition::PlatformCompositorDescriptor::active_for_platform(
        );
    #[cfg(target_os = "macos")]
    {
        assert!(!descriptor.can_intercept_event_outside_viewport());
        assert!(descriptor.pointer_transparent);
        assert!(descriptor.validate_desktop_composition().is_ok());
    }
}

#[test]
fn test_child_view_frame_strictly_scoped_to_terminal_viewport_without_covering_chrome() {
    // Parent window dimensions in AppKit points
    let window_width = 1200.0;
    let window_height = 800.0;
    let _ = window_width;

    // Active terminal pane bounds with nonzero origin:
    // TabBar occupies y in [0.0, 38.0]
    // Sidebar occupies x in [0.0, 220.0]
    // Terminal viewport occupies x: 220.0, y: 38.0, width: 800.0, height: 600.0
    let terminal_bounds = LogicalBounds {
        x: 220.0,
        y: 38.0,
        width: 800.0,
        height: 600.0,
        scale_factor: 2.0,
    };

    // 1. Standard unflipped AppKit superview (bottom-left origin)
    let appkit_frame = terminal_bounds.to_appkit_frame(window_height, false);

    // Frame position and dimensions must strictly match the viewport:
    // x = 220.0, width = 800.0, height = 600.0
    // y in AppKit = window_height - (y_dom + height_dom) = 800.0 - (38.0 + 600.0) = 162.0
    assert_eq!(appkit_frame.x, 220.0);
    assert_eq!(appkit_frame.y, 162.0);
    assert_eq!(appkit_frame.width, 800.0);
    assert_eq!(appkit_frame.height, 600.0);

    // Points inside the owning terminal viewport MUST be contained
    assert!(appkit_frame.contains_point(220.0, 162.0)); // bottom-left of terminal
    assert!(appkit_frame.contains_point(500.0, 400.0)); // center of terminal
    assert!(appkit_frame.contains_point(1019.9, 761.9)); // top-right of terminal

    // Points outside the owning terminal viewport MUST NOT be contained in the child frame:
    // A. TabBar & New Tab button (DOM y: 15.0 -> AppKit y: 785.0)
    assert!(!appkit_frame.contains_point(300.0, 785.0));
    assert!(!appkit_frame.contains_point(220.0, 785.0));

    // B. Left Sidebar (DOM x: 100.0 -> AppKit x: 100.0)
    assert!(!appkit_frame.contains_point(100.0, 400.0));
    assert!(!appkit_frame.contains_point(0.0, 162.0));

    // C. Pane Split Controls / Floating Header area (DOM y: 20.0 -> AppKit y: 780.0)
    assert!(!appkit_frame.contains_point(400.0, 780.0));

    // D. Area below terminal (DOM y: 650.0 -> AppKit y: 150.0, below y=162.0)
    assert!(!appkit_frame.contains_point(500.0, 150.0));

    // 2. Flipped AppKit superview (top-left origin)
    let flipped_frame = terminal_bounds.to_appkit_frame(window_height, true);
    assert_eq!(flipped_frame.x, 220.0);
    assert_eq!(flipped_frame.y, 38.0);
    assert_eq!(flipped_frame.width, 800.0);
    assert_eq!(flipped_frame.height, 600.0);
    assert!(!flipped_frame.contains_point(300.0, 15.0)); // TabBar
    assert!(!flipped_frame.contains_point(100.0, 200.0)); // Sidebar
}
