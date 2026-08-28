//! Windows and Linux must host each terminal session in an isolated native child
//! surface positioned inside the root window, hidden until its first present.

#![cfg(feature = "native-terminal")]

use ferryx_lib::native_terminal::child_surface::{ChildSurfaceGeometry, ChildSurfaceVisibility};
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
fn child_surface_geometry_preserves_scaled_origin() {
    let geometry =
        ChildSurfaceGeometry::from_logical_bounds(&bounds(40.0, 25.0, 300.0, 150.0, 2.0))
            .expect("positive bounds must produce child geometry");

    assert_eq!(
        (geometry.x, geometry.y),
        (80, 50),
        "child surface must be positioned at the scaled pane origin, not the window origin"
    );
    assert_eq!(
        (geometry.width, geometry.height),
        (600, 300),
        "child surface must be sized to the scaled pane extent"
    );
}

#[test]
fn child_surface_geometry_rejects_non_positive_extent() {
    assert!(
        ChildSurfaceGeometry::from_logical_bounds(&bounds(10.0, 10.0, 0.0, 120.0, 1.0)).is_none(),
        "zero width must not produce a child surface placement"
    );
    assert!(
        ChildSurfaceGeometry::from_logical_bounds(&bounds(10.0, 10.0, 120.0, 0.2, 2.0)).is_none(),
        "sub-pixel height must not produce a child surface placement"
    );
    assert!(
        ChildSurfaceGeometry::from_logical_bounds(&bounds(10.0, 10.0, f64::NAN, 120.0, 1.0))
            .is_none(),
        "non-finite bounds must not produce a child surface placement"
    );
}

#[test]
fn child_surface_geometry_clamps_negative_origin_into_parent() {
    let geometry =
        ChildSurfaceGeometry::from_logical_bounds(&bounds(-12.0, -3.0, 100.0, 50.0, 1.0))
            .expect("negative origin must still yield a placement clamped into the parent");

    assert_eq!(
        (geometry.x, geometry.y),
        (0, 0),
        "child surface origin must be clamped into the parent client area"
    );
}

#[test]
fn child_surface_stays_hidden_until_first_present() {
    let mut visibility = ChildSurfaceVisibility::default();

    assert!(
        !visibility.is_visible(),
        "child surface must start hidden so an unconfigured surface is never composited"
    );
    assert!(
        visibility.should_map_on_present(),
        "the first successful present must map the child surface"
    );

    visibility.mark_presented();

    assert!(
        visibility.is_visible(),
        "child surface is mapped after present"
    );
    assert!(
        !visibility.should_map_on_present(),
        "subsequent presents must not re-map an already mapped child surface"
    );
}

#[test]
fn detached_child_surface_is_not_resurrected_by_a_late_present() {
    let mut visibility = ChildSurfaceVisibility::default();
    visibility.mark_presented();
    visibility.mark_detached();

    assert!(
        !visibility.is_visible(),
        "detach must hide the child surface"
    );
    assert!(
        !visibility.should_map_on_present(),
        "a present racing after detach must not resurrect a dead child surface"
    );

    visibility.mark_presented();

    assert!(
        !visibility.is_visible(),
        "a detached child surface must stay hidden even if a stale present arrives"
    );
}
