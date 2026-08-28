//! Contract for the pure geometry a Wayland subsurface child needs.
//!
//! Wayland differs from X11 and Win32 in one decisive way: `wl_subsurface.set_position` takes
//! parent-surface-local **logical** coordinates, while the attached buffer is sized in
//! **physical** pixels and divided back down by `wl_surface.set_buffer_scale`. Reusing the
//! X11/Win32 `ChildSurfaceGeometry` (which scales the origin into physical pixels) would place
//! the terminal at `scale * offset` on any HiDPI output, so the Wayland path needs its own
//! conversion and its own test.

use ferryx_lib::native_terminal::child_surface::WaylandSubsurfaceGeometry;
use ferryx_lib::native_terminal::composition::LogicalBounds;

fn bounds(x: f64, y: f64, width: f64, height: f64, scale_factor: f64) -> LogicalBounds {
    LogicalBounds {
        x,
        y,
        width,
        height,
        scale_factor,
    }
}

#[test]
fn position_stays_logical_while_buffer_is_physical() {
    let geometry =
        WaylandSubsurfaceGeometry::from_logical_bounds(&bounds(236.0, 44.0, 1032.0, 781.0, 2.0))
            .expect("integral bounds must convert");

    // The origin must NOT be multiplied by the scale factor: that is the X11/Win32 rule and
    // it would push the surface off by an extra 236x44 logical pixels on a 2x output.
    assert_eq!(geometry.position_x, 236);
    assert_eq!(geometry.position_y, 44);
    assert_eq!(geometry.buffer_scale, 2);
    assert_eq!(geometry.physical_width, 2064);
    assert_eq!(geometry.physical_height, 1562);
}

#[test]
fn unscaled_output_keeps_buffer_equal_to_logical_size() {
    let geometry =
        WaylandSubsurfaceGeometry::from_logical_bounds(&bounds(0.0, 0.0, 800.0, 600.0, 1.0))
            .expect("1x bounds must convert");

    assert_eq!(geometry.buffer_scale, 1);
    assert_eq!(geometry.physical_width, 800);
    assert_eq!(geometry.physical_height, 600);
}

#[test]
fn fractional_scale_rounds_up_to_an_integer_buffer_scale() {
    // `wl_surface.set_buffer_scale` is an integer protocol argument, so a 1.5x output must be
    // rendered at an integer density. Rounding to 2 keeps glyphs crisp; truncating to 1 would
    // render a blurry terminal on exactly the laptop panels that report 1.5.
    let geometry =
        WaylandSubsurfaceGeometry::from_logical_bounds(&bounds(10.0, 20.0, 400.0, 300.0, 1.5))
            .expect("fractional scale must convert");

    assert_eq!(geometry.buffer_scale, 2);
    assert_eq!(geometry.physical_width, 800);
    assert_eq!(geometry.physical_height, 600);
    assert_eq!(geometry.position_x, 10);
    assert_eq!(geometry.position_y, 20);
}

#[test]
fn buffer_scale_never_drops_below_one() {
    let geometry =
        WaylandSubsurfaceGeometry::from_logical_bounds(&bounds(0.0, 0.0, 400.0, 300.0, 0.5))
            .expect("sub-unit scale must still convert");

    assert_eq!(geometry.buffer_scale, 1);
    assert_eq!(geometry.physical_width, 400);
    assert_eq!(geometry.physical_height, 300);
}

#[test]
fn negative_origin_is_clamped_to_the_parent_surface() {
    let geometry =
        WaylandSubsurfaceGeometry::from_logical_bounds(&bounds(-12.0, -30.0, 400.0, 300.0, 1.0))
            .expect("negative origin must clamp rather than fail");

    assert_eq!(geometry.position_x, 0);
    assert_eq!(geometry.position_y, 0);
}

#[test]
fn sub_pixel_size_is_rejected_instead_of_committing_a_zero_buffer() {
    assert!(
        WaylandSubsurfaceGeometry::from_logical_bounds(&bounds(0.0, 0.0, 0.2, 10.0, 1.0)).is_none(),
        "a buffer that rounds to zero width must not be committed"
    );
    assert!(
        WaylandSubsurfaceGeometry::from_logical_bounds(&bounds(0.0, 0.0, 10.0, 0.2, 1.0)).is_none(),
        "a buffer that rounds to zero height must not be committed"
    );
}

#[test]
fn non_finite_bounds_are_rejected() {
    for bad in [
        bounds(f64::NAN, 0.0, 400.0, 300.0, 1.0),
        bounds(0.0, f64::INFINITY, 400.0, 300.0, 1.0),
        bounds(0.0, 0.0, f64::NAN, 300.0, 1.0),
        bounds(0.0, 0.0, 400.0, f64::NAN, 1.0),
        bounds(0.0, 0.0, 400.0, 300.0, f64::NAN),
        bounds(0.0, 0.0, 400.0, 300.0, 0.0),
    ] {
        assert!(
            WaylandSubsurfaceGeometry::from_logical_bounds(&bad).is_none(),
            "non-finite or non-positive bounds must be rejected: {bad:?}"
        );
    }
}
