//! Contract test for Phase 2 native terminal renderer offscreen frame generation.

use ferryx_lib::native_terminal::composition::PhysicalBounds;
use ferryx_lib::native_terminal::renderer::RendererTheme;
use ferryx_lib::native_terminal::{
    NativeTerminalRenderer, OffscreenFrame, RenderSnapshot, RendererConfig, SelectionSnapshot,
};

#[test]
fn test_render_snapshot_offscreen_frame_with_ansi_cjk_cursor_selection() {
    let snapshot: RenderSnapshot = super::snapshot_builder::build_test_snapshot();

    let selection = SelectionSnapshot {
        start_col: 0,
        start_row: 0,
        end_col: 5,
        end_row: 0,
    };

    let config = RendererConfig {
        cell_width_px: 10,
        cell_height_px: 20,
        device_scale_factor: 1.0,
        ..Default::default()
    };

    let mut renderer = NativeTerminalRenderer::new(config).expect("renderer creation");
    let frame: OffscreenFrame = renderer
        .render_snapshot(&snapshot, Some(&selection))
        .expect("render offscreen frame");

    assert_eq!(frame.width_px, 800, "80 cols * 10px = 800px width");
    assert_eq!(frame.height_px, 480, "24 rows * 20px = 480px height");
    assert_eq!(
        frame.pixels.len(),
        (800 * 480 * 4) as usize,
        "pixel buffer must match RGBA byte size"
    );
    assert!(
        !frame.pixels.iter().all(|&b| b == 0),
        "rendered frame must contain non-zero pixel data"
    );
    assert_eq!(frame.rendered_row_count, 24);
}

#[test]
fn test_viewport_background_covers_residual_pixels_with_imported_theme_color() {
    let snapshot: RenderSnapshot = super::snapshot_builder::build_test_snapshot();

    // 80 cols * 10px = 800px, 24 rows * 20px = 480px
    // Surface and viewport has residual pixels: 805 x 485 (5px residual right and bottom)
    let surface_w = 805;
    let surface_h = 485;
    let viewport = PhysicalBounds {
        x: 0,
        y: 0,
        width: surface_w,
        height: surface_h,
    };

    let imported_background = [
        0x1b as f32 / 255.0,
        0x2c as f32 / 255.0,
        0x3d as f32 / 255.0,
        1.0,
    ];
    let config = RendererConfig {
        cell_width_px: 10,
        cell_height_px: 20,
        device_scale_factor: 1.0,
        theme: RendererTheme {
            background: imported_background,
            ..Default::default()
        },
    };

    let mut renderer = NativeTerminalRenderer::new(config).expect("renderer creation");
    let frame = renderer
        .render_to_offscreen_viewport(&snapshot, None, surface_w, surface_h, viewport)
        .expect("render to offscreen viewport");

    assert_eq!(frame.width_px, surface_w);
    assert_eq!(frame.height_px, surface_h);
    assert_eq!(frame.pixels.len(), (surface_w * surface_h * 4) as usize);

    // Verify every pixel in the entire viewport (including residual gutters) is opaque
    for y in 0..surface_h {
        for x in 0..surface_w {
            let idx = ((y * surface_w + x) * 4) as usize;
            let r = frame.pixels[idx];
            let g = frame.pixels[idx + 1];
            let b = frame.pixels[idx + 2];
            let a = frame.pixels[idx + 3];

            assert_eq!(
                a, 255,
                "Pixel at ({x}, {y}) must be fully opaque (alpha 255), got alpha {a}"
            );

            if x >= 800 || y >= 480 {
                assert!(
                    (r as i32 - 0x1b).abs() <= 2
                        && (g as i32 - 0x2c).abs() <= 2
                        && (b as i32 - 0x3d).abs() <= 2,
                    "Residual pixel at ({x}, {y}) must match imported background [27, 44, 61], got [{r}, {g}, {b}]"
                );
            }
        }
    }
}

#[test]
fn test_retina_scale_2_renderer_viewport_and_config_update() {
    let snapshot: RenderSnapshot = super::snapshot_builder::build_test_snapshot();

    // Start with 1.0x config
    let config_1x = RendererConfig {
        cell_width_px: 10,
        cell_height_px: 20,
        device_scale_factor: 1.0,
        ..Default::default()
    };
    let mut renderer = NativeTerminalRenderer::new(config_1x).expect("renderer creation");

    // Dynamic update to 2.0x Retina config (e.g. 20x40 px cells on Retina)
    let config_2x = RendererConfig {
        cell_width_px: 20,
        cell_height_px: 40,
        device_scale_factor: 2.0,
        ..Default::default()
    };
    renderer
        .update_config(config_2x)
        .expect("update config to 2.0x");
    assert_eq!(renderer.config().device_scale_factor, 2.0);

    // 80 cols * 20px = 1600px, 24 rows * 40px = 960px
    // Surface with residual pixels: 1606 x 968
    let surface_w = 1606;
    let surface_h = 968;
    let viewport = PhysicalBounds {
        x: 0,
        y: 0,
        width: surface_w,
        height: surface_h,
    };

    let frame = renderer
        .render_to_offscreen_viewport(&snapshot, None, surface_w, surface_h, viewport)
        .expect("render to 2x offscreen viewport");

    assert_eq!(frame.width_px, surface_w);
    assert_eq!(frame.height_px, surface_h);

    // Verify all pixels including 2x residual margin are opaque
    for y in [960, 965, 967] {
        for x in [1600, 1603, 1605] {
            let idx = ((y * surface_w + x) * 4) as usize;
            let a = frame.pixels[idx + 3];
            assert_eq!(a, 255, "2x residual pixel at ({x}, {y}) must be opaque");
        }
    }
}
