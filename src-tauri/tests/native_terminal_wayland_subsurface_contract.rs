//! Contract for the pure geometry a Wayland subsurface child needs.
//!
//! Wayland differs from X11 and Win32 in one decisive way: `wl_subsurface.set_position` takes
//! parent-surface-local **logical** coordinates, while the attached buffer is sized in
//! **physical** pixels and divided back down by `wl_surface.set_buffer_scale`. Reusing the
//! X11/Win32 `ChildSurfaceGeometry` (which scales the origin into physical pixels) would place
//! the terminal at `scale * offset` on any HiDPI output, so the Wayland path needs its own
//! conversion and its own test.

use ferryx_lib::native_terminal::child_surface::{ChildSurfaceGeometry, WaylandSubsurfaceGeometry};
use ferryx_lib::native_terminal::composition::{
    CellMetrics, LogicalBounds, SurfacePresentationGeometry,
};
use ferryx_lib::native_terminal::renderer::font_manager::derived_cell_metrics_for_scale;
use ferryx_lib::native_terminal::surface_host::{
    NativeTerminalBoundsRequest, NativeTerminalSurfaceHostState,
};

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
fn buffer_extent_is_divisible_by_scale_when_logical_extent_is_fractional() {
    let geometry =
        WaylandSubsurfaceGeometry::from_logical_bounds(&bounds(0.0, 0.0, 400.5, 300.0, 2.0))
            .expect("valid fractional bounds");
    println!("fractional logical extent: {geometry:?}");
    assert_eq!(geometry.physical_width % geometry.buffer_scale as u32, 0);
    assert_eq!(geometry.physical_height % geometry.buffer_scale as u32, 0);
}

#[test]
fn host_extent_matches_wayland_buffer_extent_at_fractional_scale() {
    let bounds = bounds(10.0, 20.0, 400.0, 300.0, 1.5);
    let child = WaylandSubsurfaceGeometry::from_logical_bounds(&bounds).unwrap();
    let request = NativeTerminalBoundsRequest {
        session_id: "wayland-geometry-contract".into(),
        // The same resolution used by render and warm attach before request.layout.
        bounds: SurfacePresentationGeometry::WaylandSubsurface
            .resolve(bounds)
            .unwrap(),
    };
    let layout = request
        .layout(CellMetrics {
            width_px: 20,
            height_px: 40,
        })
        .unwrap();
    println!("host={layout:?}; child={child:?}");
    assert_eq!(
        (layout.physical_bounds.width, layout.physical_bounds.height),
        (child.physical_width, child.physical_height),
    );
    assert_eq!(request.bounds.scale_factor, child.buffer_scale as f64);
    assert_eq!((layout.cols, layout.rows), (40, 15));
}

#[test]
fn shared_fractional_edges_agree_in_child_host_and_stored_grid() {
    let state = NativeTerminalSurfaceHostState::default();
    for scale in [1.0, 1.5, 2.0, 2.5] {
        let panes = [
            bounds(10.25, 20.25, 400.5, 300.5, scale),
            bounds(410.75, 20.25, 400.5, 300.5, scale),
            bounds(10.25, 320.75, 400.5, 300.5, scale),
        ];
        let children =
            panes.map(|bounds| WaylandSubsurfaceGeometry::from_logical_bounds(&bounds).unwrap());
        assert_eq!(
            children[0].position_x
                + (children[0].physical_width / children[0].buffer_scale as u32) as i32,
            children[1].position_x
        );
        assert_eq!(
            children[0].position_y
                + (children[0].physical_height / children[0].buffer_scale as u32) as i32,
            children[2].position_y
        );
        for (index, (bounds, child)) in panes.into_iter().zip(children).enumerate() {
            let presentation = SurfacePresentationGeometry::WaylandSubsurface
                .resolve(bounds)
                .unwrap();
            assert_eq!(
                SurfacePresentationGeometry::WaylandSubsurface
                    .resolve(presentation)
                    .unwrap(),
                presentation
            );
            assert_eq!(
                WaylandSubsurfaceGeometry::from_logical_bounds(&presentation).unwrap(),
                child
            );
            let metrics = derived_cell_metrics_for_scale(presentation.scale_factor);
            let session_id = format!("pane-{index}");
            let layout = state
                .prepare_session_layout(
                    NativeTerminalBoundsRequest {
                        session_id: session_id.clone(),
                        bounds: presentation,
                    },
                    metrics,
                )
                .unwrap();
            assert_eq!(
                (layout.physical_bounds.width, layout.physical_bounds.height),
                (child.physical_width, child.physical_height)
            );
            assert_eq!(layout.physical_bounds.width % child.buffer_scale as u32, 0);
            assert_eq!(layout.physical_bounds.height % child.buffer_scale as u32, 0);
            assert_eq!(layout.cols as u32, child.physical_width / metrics.width_px);
            assert_eq!(
                layout.rows as u32,
                child.physical_height / metrics.height_px
            );
            assert_eq!(state.session_layout(&session_id), Some(layout));
            assert_eq!(
                state.session_logical_bounds(&session_id),
                Some(presentation)
            );
            assert_eq!(state.session_cell_metrics(&session_id), Some(metrics));
            println!("DPR={scale}; pane={index}; presentation={presentation:?}; child={child:?}; metrics={metrics:?}; stored_layout={layout:?}");
        }
    }
}

#[test]
fn default_presentation_preserves_macos_windows_and_x11_fractional_geometry() {
    let original = bounds(10.25, 20.25, 400.5, 300.5, 1.5);
    let resolved = SurfacePresentationGeometry::Default
        .resolve(original)
        .unwrap();
    assert_eq!(resolved, original);
    let child = ChildSurfaceGeometry::from_logical_bounds(&resolved).unwrap();
    assert_eq!(
        (child.x, child.y, child.width, child.height),
        (15, 30, 601, 451)
    );
    let frame = resolved.to_appkit_frame(800.0, true);
    assert_eq!(
        (frame.x, frame.y, frame.width, frame.height),
        (10.25, 20.25, 400.5, 300.5)
    );
    let layout = NativeTerminalBoundsRequest {
        session_id: "default-geometry-contract".into(),
        bounds: resolved,
    }
    .layout(CellMetrics {
        width_px: 15,
        height_px: 30,
    })
    .unwrap();
    assert_eq!(
        (layout.physical_bounds.width, layout.physical_bounds.height),
        (601, 451)
    );
    assert_eq!((layout.cols, layout.rows), (40, 15));
}

#[test]
fn wayland_rejects_unrepresentable_scale_and_collapsed_logical_edges() {
    for bounds in [
        bounds(0.0, 0.0, 400.0, 300.0, i32::MAX as f64 + 1.0),
        bounds(10.1, 20.1, 0.2, 300.0, 2.0),
        bounds(10.1, 20.1, 400.0, 0.2, 2.0),
        bounds(0.0, 0.0, u32::MAX as f64, 300.0, 2.0),
    ] {
        assert!(SurfacePresentationGeometry::WaylandSubsurface
            .resolve(bounds)
            .is_err());
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
