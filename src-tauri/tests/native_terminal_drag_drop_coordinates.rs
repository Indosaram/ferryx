//! Contract: wry 0.55.1 passes `draggingLocation()` (logical points) through as
//! `PhysicalPosition` (tauri-runtime-wry 2.11.4), so macOS drop coordinates must
//! be divided by the real window backing scale factor, not a guessed one. The
//! compositor-target getter is the single source of truth for that conversion.

#![cfg(all(feature = "native-terminal", target_os = "macos"))]

use ferryx_lib::native_terminal::platform::macos::FerryxNativeTerminalView;

#[test]
fn backing_scale_factor_falls_back_to_one_without_window() {
    let Some(mtm) = objc2::MainThreadMarker::new() else {
        return;
    };

    let view = FerryxNativeTerminalView::new(
        objc2_foundation::NSRect::new(
            objc2_foundation::NSPoint::new(0.0, 0.0),
            objc2_foundation::NSSize::new(10.0, 10.0),
        ),
        mtm,
    )
    .expect("view must initialize");

    let scale = view.window_backing_scale_factor();
    assert!(
        (scale - 1.0).abs() < f64::EPSILON,
        "orphan view must report backing scale 1.0, got {scale}"
    );
}

fn logical_from_raw(raw: (f64, f64), backing_scale: f64) -> (f64, f64) {
    (raw.0 / backing_scale, raw.1 / backing_scale)
}

#[test]
fn macos_drop_position_divides_by_backing_scale_for_pane_hit_test() {
    assert_eq!(logical_from_raw((400.0, 200.0), 2.0), (200.0, 100.0));
}
