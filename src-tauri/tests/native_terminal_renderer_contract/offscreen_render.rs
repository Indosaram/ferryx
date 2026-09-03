//! Contract test for Phase 2 native terminal renderer offscreen frame generation.

use ferryx_lib::native_terminal::composition::PhysicalBounds;
use ferryx_lib::native_terminal::renderer::font_manager::FontManager;
use ferryx_lib::native_terminal::renderer::RendererTheme;
use ferryx_lib::native_terminal::{
    CellWide, ColorRgb, CursorSnapshot, CursorVisualStyle, NativeTerminalRenderer, OffscreenFrame,
    RenderSnapshot, RendererConfig, ScrollbarOverlayState, ScrollbarState, SelectionSnapshot,
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
fn test_glyph_pixels_blend_once_and_leave_uncovered_pixels_as_cell_background() {
    // Given: one opaque cell with deliberately distinct foreground and background channels.
    let font = FontManager::global();
    let metrics = font.cell_metrics();
    let foreground = ColorRgb {
        r: 214,
        g: 163,
        b: 91,
    };
    let background = ColorRgb {
        r: 19,
        g: 37,
        b: 73,
    };
    let snapshot = RenderSnapshot {
        cols: 1,
        rows: 1,
        cursor: CursorSnapshot {
            x: 0,
            y: 0,
            visible: false,
            blinking: false,
            wide_tail: false,
            visual_style: CursorVisualStyle::Block,
        },
        grid: vec![vec![super::snapshot_builder::make_cell(
            "A",
            CellWide::Narrow,
            Some(foreground),
            Some(background),
            false,
            false,
            false,
            false,
        )]],
    };
    let config = RendererConfig {
        cell_width_px: metrics.width_px,
        cell_height_px: metrics.height_px,
        device_scale_factor: 1.0,
        ..Default::default()
    };
    let mask = font
        .rasterize_glyph("A", metrics.width_px, metrics.height_px, false, false)
        .into_buffer();

    // When: the real WGPU renderer draws that glyph.
    let mut renderer = NativeTerminalRenderer::new(config).expect("renderer creation");
    let frame = renderer
        .render_snapshot(&snapshot, None)
        .expect("render LCD glyph");

    // Then: covered pixels blend exactly once and uncovered pixels stay the cell background.
    let bg = [background.r, background.g, background.b];
    let fg = [foreground.r, foreground.g, foreground.b];
    let mut covered = 0;
    let mut uncovered = 0;
    for (mask_pixel, frame_pixel) in mask.chunks_exact(4).zip(frame.pixels.chunks_exact(4)) {
        let coverage = mask_pixel[3] as f32 / 255.0;
        for channel in 0..3 {
            let expected = bg[channel] as f32 * (1.0 - coverage) + fg[channel] as f32 * coverage;
            assert!(
                (frame_pixel[channel] as f32 - expected).abs() <= 2.0,
                "channel {channel} at coverage {coverage} must blend once: expected {expected}, got {}",
                frame_pixel[channel]
            );
        }
        assert_eq!(frame_pixel[3], 255, "opaque cell remains opaque");
        if mask_pixel[3] == 0 {
            uncovered += 1;
        } else {
            covered += 1;
        }
    }
    assert!(covered > 0, "the host font must provide covered glyph pixels");
    assert!(
        uncovered > 0,
        "the glyph cell must retain uncovered background pixels"
    );
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

#[test]
fn test_render_snapshot_offscreen_frame_with_color_emoji() {
    let cols = 20u16;
    let rows = 2u16;
    let mut grid = vec![vec![super::snapshot_builder::empty_cell(); cols as usize]; rows as usize];
    grid[0][0] = super::snapshot_builder::make_cell(
        "(",
        ferryx_lib::native_terminal::CellWide::Narrow,
        None,
        None,
        false,
        false,
        false,
        false,
    );
    grid[0][1] = super::snapshot_builder::make_cell(
        "😺",
        ferryx_lib::native_terminal::CellWide::Wide,
        None,
        None,
        false,
        false,
        false,
        false,
    );
    grid[0][2] = super::snapshot_builder::make_cell(
        "",
        ferryx_lib::native_terminal::CellWide::SpacerTail,
        None,
        None,
        false,
        false,
        false,
        false,
    );
    grid[0][3] = super::snapshot_builder::make_cell(
        ")",
        ferryx_lib::native_terminal::CellWide::Narrow,
        None,
        None,
        false,
        false,
        false,
        false,
    );

    let snapshot = RenderSnapshot {
        cols,
        rows,
        cursor: ferryx_lib::native_terminal::CursorSnapshot {
            x: 0,
            y: 0,
            visible: false,
            blinking: false,
            wide_tail: false,
            visual_style: ferryx_lib::native_terminal::CursorVisualStyle::Block,
        },
        grid,
    };

    let config = RendererConfig {
        cell_width_px: 10,
        cell_height_px: 20,
        device_scale_factor: 1.0,
        ..Default::default()
    };

    let mut renderer = match NativeTerminalRenderer::new(config) {
        Ok(r) => r,
        Err(_) => return,
    };

    let frame = renderer
        .render_snapshot(&snapshot, None)
        .expect("render snapshot with color emoji");

    assert_eq!(frame.width_px, 200);
    assert_eq!(frame.height_px, 40);

    let non_zero_pixels = frame.pixels.chunks_exact(4).filter(|p| p[3] > 0).count();
    assert!(
        non_zero_pixels > 0,
        "rendered frame with emoji must produce visible pixels"
    );
}

#[test]
fn test_render_to_offscreen_viewport_with_scrollbar_overlay() {
    let snapshot: RenderSnapshot = super::snapshot_builder::build_test_snapshot();
    let surface_w = 800;
    let surface_h = 480;
    let viewport = PhysicalBounds {
        x: 0,
        y: 0,
        width: surface_w,
        height: surface_h,
    };

    let background = [0.0, 0.0, 0.0, 1.0];
    let foreground = [1.0, 1.0, 1.0, 1.0];
    let config = RendererConfig {
        cell_width_px: 10,
        cell_height_px: 20,
        device_scale_factor: 1.0,
        theme: RendererTheme {
            background,
            foreground,
            ..Default::default()
        },
    };

    let mut renderer = NativeTerminalRenderer::new(config).expect("renderer creation");

    // Render with overlay disabled
    let frame_no_overlay = renderer
        .render_to_offscreen_viewport(&snapshot, None, surface_w, surface_h, viewport)
        .expect("render without overlay");

    // Render with overlay enabled: total=1000, len=100, offset=0
    // Thumb: width 6px, right inset 5px -> x in [789, 795), y in [0, 48) (since 480 * 100/1000 = 48)
    let overlay_state = ScrollbarOverlayState {
        visible: true,
        metrics: Some(ScrollbarState {
            total: 1000,
            offset: 0,
            len: 100,
        }),
    };

    let frame_with_overlay = renderer
        .render_to_offscreen_viewport_with_overlay(
            &snapshot,
            None,
            surface_w,
            surface_h,
            viewport,
            Some(&overlay_state),
        )
        .expect("render with overlay");

    // Check pixel at thumb location (x=790, y=10)
    let thumb_x = 790;
    let thumb_y = 10;
    let thumb_idx = ((thumb_y * surface_w + thumb_x) * 4) as usize;

    let no_overlay_pixel = &frame_no_overlay.pixels[thumb_idx..thumb_idx + 4];
    let with_overlay_pixel = &frame_with_overlay.pixels[thumb_idx..thumb_idx + 4];

    // With white foreground @ 0.35 alpha blended onto black background, color channels will be around 89 (255 * 0.35)
    assert_ne!(
        no_overlay_pixel, with_overlay_pixel,
        "pixel at thumb location must be modified by overlay"
    );
    assert!(
        with_overlay_pixel[0] > no_overlay_pixel[0],
        "overlay pixel red channel must be brighter due to blended white foreground"
    );
}

#[test]
fn test_render_to_offscreen_viewport_with_attention_frame() {
    let snapshot: RenderSnapshot = super::snapshot_builder::build_test_snapshot();
    let surface_w = 800;
    let surface_h = 480;
    let viewport = PhysicalBounds {
        x: 0,
        y: 0,
        width: surface_w,
        height: surface_h,
    };

    let background = [0.0, 0.0, 0.0, 1.0];
    let foreground = [1.0, 1.0, 1.0, 1.0];
    let config = RendererConfig {
        cell_width_px: 10,
        cell_height_px: 20,
        device_scale_factor: 1.0,
        theme: RendererTheme {
            background,
            foreground,
            ..Default::default()
        },
    };

    let mut renderer = NativeTerminalRenderer::new(config).expect("renderer creation");

    let frame_no_attention = renderer
        .render_to_offscreen_viewport(&snapshot, None, surface_w, surface_h, viewport)
        .expect("render without attention frame");

    let frame_with_attention = renderer
        .render_to_offscreen_viewport_with_overlay_and_attention(
            &snapshot, None, surface_w, surface_h, viewport, None, true,
        )
        .expect("render with attention frame");

    // Top border pixel at (x=10, y=1) should be colored with pastel amber (RGB 253, 230, 138)
    let border_x = 10;
    let border_y = 1;
    let border_idx = ((border_y * surface_w + border_x) * 4) as usize;

    let no_att_pixel = &frame_no_attention.pixels[border_idx..border_idx + 4];
    let with_att_pixel = &frame_with_attention.pixels[border_idx..border_idx + 4];

    assert_ne!(
        no_att_pixel, with_att_pixel,
        "pixel at top border must be modified by attention frame"
    );
    // Pastel amber red channel is dominant (> 200)
    assert!(
        with_att_pixel[0] > 200,
        "attention frame pixel red channel should reflect pastel amber color"
    );

    // Halo pixel at (x=10, y=4) is in the 6px halo band but outside the 2px core band
    let halo_x = 10;
    let halo_y = 4;
    let halo_idx = ((halo_y * surface_w + halo_x) * 4) as usize;
    let no_att_halo = &frame_no_attention.pixels[halo_idx..halo_idx + 4];
    let with_att_halo = &frame_with_attention.pixels[halo_idx..halo_idx + 4];
    assert_ne!(
        no_att_halo, with_att_halo,
        "pixel in halo band must be modified by attention halo"
    );
    assert!(
        with_att_halo[0] > 30 && with_att_halo[0] < 120,
        "halo pixel should reflect low-alpha blend"
    );
}
