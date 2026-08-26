//! Contract tests asserting native terminal renderer colors and cursor styling
//! are driven by real terminal preferences with total fallback parsing.

use ferryx_lib::native_terminal::renderer::atlas::GlyphAtlas;
use ferryx_lib::native_terminal::renderer::gpu_context::GpuContext;
use ferryx_lib::native_terminal::renderer::instances::build_row_instances;
use ferryx_lib::native_terminal::renderer::types::{
    parse_hex_color, RendererConfig, RendererTheme, DEFAULT_RENDERER_BACKGROUND,
    DEFAULT_RENDERER_CURSOR, DEFAULT_RENDERER_CURSOR_ACCENT, DEFAULT_RENDERER_FOREGROUND,
    DEFAULT_RENDERER_SELECTION_BG, DEFAULT_RENDERER_SELECTION_FG,
};
use ferryx_lib::native_terminal::{
    CellSnapshot, CellWide, CursorSnapshot, CursorVisualStyle, RenderSnapshot,
};
use ferryx_lib::terminal::preferences::{TerminalPreferences, TerminalThemeColors};

fn make_test_snapshot(cols: u16, rows: u16, text: &str) -> RenderSnapshot {
    let mut grid = Vec::with_capacity(rows as usize);
    for r in 0..rows {
        let mut row_cells = Vec::with_capacity(cols as usize);
        for c in 0..cols {
            let ch = if r == 0 && (c as usize) < text.len() {
                text.chars().nth(c as usize).unwrap_or(' ').to_string()
            } else {
                " ".to_string()
            };
            row_cells.push(CellSnapshot {
                text: ch,
                fg: None,
                bg: None,
                bold: false,
                italic: false,
                underline: false,
                inverse: false,
                wide: CellWide::Narrow,
            });
        }
        grid.push(row_cells);
    }

    RenderSnapshot {
        cols,
        rows,
        cursor: CursorSnapshot {
            x: 0,
            y: 0,
            visible: true,
            blinking: false,
            wide_tail: false,
            visual_style: CursorVisualStyle::Block,
        },
        grid,
    }
}

#[test]
fn test_hex_color_parsing_totality_and_fallbacks() {
    let fallback = [0.1, 0.2, 0.3, 1.0];

    // 6-digit hex with '#'
    let c1 = parse_hex_color("#123456", fallback);
    assert_eq!(
        c1,
        [
            0x12 as f32 / 255.0,
            0x34 as f32 / 255.0,
            0x56 as f32 / 255.0,
            1.0
        ]
    );

    // 6-digit hex without '#'
    let c2 = parse_hex_color("abcdef", fallback);
    assert_eq!(
        c2,
        [
            0xab as f32 / 255.0,
            0xcd as f32 / 255.0,
            0xef as f32 / 255.0,
            1.0
        ]
    );

    // 8-digit hex with alpha (#52525299)
    let c3 = parse_hex_color("#52525299", fallback);
    assert_eq!(
        c3,
        [
            0x52 as f32 / 255.0,
            0x52 as f32 / 255.0,
            0x52 as f32 / 255.0,
            0x99 as f32 / 255.0,
        ]
    );

    // 3-digit shorthand (#f0a)
    let c4 = parse_hex_color("#f0a", fallback);
    assert_eq!(c4, [1.0, 0.0, (10 * 17) as f32 / 255.0, 1.0]);

    // 4-digit shorthand (#f0a8)
    let c5 = parse_hex_color("#f0a8", fallback);
    assert_eq!(
        c5,
        [1.0, 0.0, (10 * 17) as f32 / 255.0, (8 * 17) as f32 / 255.0,]
    );

    // Invalid formats fall back safely without panicking
    assert_eq!(parse_hex_color("", fallback), fallback);
    assert_eq!(parse_hex_color("#", fallback), fallback);
    assert_eq!(parse_hex_color("not-a-color", fallback), fallback);
    assert_eq!(parse_hex_color("#12345", fallback), fallback);
    assert_eq!(parse_hex_color("#gggggg", fallback), fallback);
    assert_eq!(parse_hex_color("#1234567890", fallback), fallback);
}

#[test]
fn test_renderer_theme_conversion_from_terminal_preferences() {
    let mut theme_colors = TerminalThemeColors::default();
    theme_colors.background = "#123456".into();
    theme_colors.foreground = "#abcdef".into();
    theme_colors.cursor = "#ffffff".into();
    theme_colors.cursor_accent = "#000000".into();
    theme_colors.selection_background = "#52525299".into();
    theme_colors.selection_foreground = Some("#fedcba".into());

    let theme = RendererTheme::from(&theme_colors);
    assert_eq!(
        theme.background,
        [
            0x12 as f32 / 255.0,
            0x34 as f32 / 255.0,
            0x56 as f32 / 255.0,
            1.0
        ]
    );
    assert_eq!(
        theme.foreground,
        [
            0xab as f32 / 255.0,
            0xcd as f32 / 255.0,
            0xef as f32 / 255.0,
            1.0
        ]
    );
    assert_eq!(
        theme.selection_background,
        [
            0x52 as f32 / 255.0,
            0x52 as f32 / 255.0,
            0x52 as f32 / 255.0,
            0x99 as f32 / 255.0,
        ]
    );
    assert_eq!(
        theme.selection_foreground,
        [
            0xfe as f32 / 255.0,
            0xdc as f32 / 255.0,
            0xba as f32 / 255.0,
            1.0
        ]
    );

    let prefs = TerminalPreferences {
        font_family: "monospace".into(),
        font_size: 13.0,
        macos_option_as_alt: false,
        cursor_style: "underline".into(),
        theme: theme_colors,
        source: ferryx_lib::terminal::preferences::TerminalPreferencesSource::Defaults,
        status: ferryx_lib::terminal::preferences::TerminalPreferencesStatus::Imported,
        source_path: None,
    };

    let theme_from_prefs = RendererTheme::from(&prefs);
    assert_eq!(theme_from_prefs.cursor_style, CursorVisualStyle::Underline);
    assert_eq!(theme_from_prefs.background, theme.background);
    assert_eq!(theme_from_prefs.foreground, theme.foreground);
}

#[test]
fn test_renderer_theme_invalid_hex_falls_back_to_documented_defaults() {
    let mut theme_colors = TerminalThemeColors::default();
    theme_colors.background = "invalid-hex-color".into();
    theme_colors.foreground = "###".into();
    theme_colors.cursor = "zzz".into();
    theme_colors.cursor_accent = "".into();
    theme_colors.selection_background = "#12345".into();
    theme_colors.selection_foreground = Some("not-hex".into());

    let theme = RendererTheme::from(&theme_colors);
    assert_eq!(theme.background, DEFAULT_RENDERER_BACKGROUND);
    assert_eq!(theme.foreground, DEFAULT_RENDERER_FOREGROUND);
    assert_eq!(theme.cursor, DEFAULT_RENDERER_CURSOR);
    assert_eq!(theme.cursor_accent, DEFAULT_RENDERER_CURSOR_ACCENT);
    assert_eq!(theme.selection_background, DEFAULT_RENDERER_SELECTION_BG);
    assert_eq!(theme.selection_foreground, DEFAULT_RENDERER_SELECTION_FG);
}

#[test]
fn test_rendered_row_instances_honor_preference_theme_colors() {
    let gpu = GpuContext::new().expect("gpu context");
    let mut atlas = GlyphAtlas::new(&gpu.device);

    let mut theme_colors = TerminalThemeColors::default();
    theme_colors.background = "#123456".into();
    theme_colors.foreground = "#abcdef".into();

    let prefs = TerminalPreferences {
        font_family: "monospace".into(),
        font_size: 13.0,
        macos_option_as_alt: false,
        cursor_style: "block".into(),
        theme: theme_colors,
        source: ferryx_lib::terminal::preferences::TerminalPreferencesSource::Defaults,
        status: ferryx_lib::terminal::preferences::TerminalPreferencesStatus::Imported,
        source_path: None,
    };

    let theme = RendererTheme::from(&prefs);
    let config = RendererConfig {
        cell_width_px: 10,
        cell_height_px: 20,
        device_scale_factor: 1.0,
        theme,
    };

    let snapshot = make_test_snapshot(10, 5, "A");
    // Row 1 has no cursor (cursor is on row 0)
    let (bg_instances, _glyph_instances) =
        build_row_instances(1, &snapshot, None, &config, &mut atlas, &gpu.queue);

    let expected_bg = [
        0x12 as f32 / 255.0,
        0x34 as f32 / 255.0,
        0x56 as f32 / 255.0,
        1.0,
    ];
    let expected_fg = [
        0xab as f32 / 255.0,
        0xcd as f32 / 255.0,
        0xef as f32 / 255.0,
        1.0,
    ];

    assert_eq!(bg_instances[0].color, expected_bg);

    // Row 0 has glyph 'A' at col 0
    let (_bg_0, glyph_0) = build_row_instances(0, &snapshot, None, &config, &mut atlas, &gpu.queue);
    assert!(
        !glyph_0.is_empty(),
        "Glyph instance for 'A' should be created"
    );
    // The cursor is at col 0 with Block style, so col 0 glyph will have cursor_accent or fg_color.
    // Let's check non-cursor glyph on row 1 by putting 'B' on row 1:
    let mut snapshot_with_row1_glyph = snapshot.clone();
    snapshot_with_row1_glyph.grid[1][0].text = "B".into();
    let (_bg_1, glyph_1) = build_row_instances(
        1,
        &snapshot_with_row1_glyph,
        None,
        &config,
        &mut atlas,
        &gpu.queue,
    );
    assert!(
        !glyph_1.is_empty(),
        "Glyph instance on row 1 should be created"
    );
    assert_eq!(glyph_1[0].color, expected_fg);
}

#[test]
fn test_rendered_row_instances_fallback_on_invalid_hex() {
    let gpu = GpuContext::new().expect("gpu context");
    let mut atlas = GlyphAtlas::new(&gpu.device);

    let mut theme_colors = TerminalThemeColors::default();
    theme_colors.background = "invalid-color".into();
    theme_colors.foreground = "also-invalid".into();

    let theme = RendererTheme::from(&theme_colors);
    let config = RendererConfig {
        cell_width_px: 10,
        cell_height_px: 20,
        device_scale_factor: 1.0,
        theme,
    };

    let mut snapshot = make_test_snapshot(10, 5, "X");
    snapshot.cursor.visible = false;

    let (bg_instances, glyph_instances) =
        build_row_instances(0, &snapshot, None, &config, &mut atlas, &gpu.queue);

    assert_eq!(bg_instances[0].color, DEFAULT_RENDERER_BACKGROUND);
    assert!(!glyph_instances.is_empty());
    assert_eq!(glyph_instances[0].color, DEFAULT_RENDERER_FOREGROUND);
}

#[test]
fn test_renderer_cursor_visual_style_preference_and_unfocused_hollow() {
    let gpu = GpuContext::new().expect("gpu context");
    let mut atlas = GlyphAtlas::new(&gpu.device);

    let mut theme_colors = TerminalThemeColors::default();
    theme_colors.cursor = "#ff0000".into();

    let prefs = TerminalPreferences {
        font_family: "monospace".into(),
        font_size: 13.0,
        macos_option_as_alt: false,
        cursor_style: "underline".into(),
        theme: theme_colors,
        source: ferryx_lib::terminal::preferences::TerminalPreferencesSource::Defaults,
        status: ferryx_lib::terminal::preferences::TerminalPreferencesStatus::Imported,
        source_path: None,
    };

    let theme = RendererTheme::from(&prefs);
    let config = RendererConfig {
        cell_width_px: 10,
        cell_height_px: 20,
        device_scale_factor: 1.0,
        theme,
    };

    // Focused snapshot with default Block visual_style -> honors preferences 'underline'
    let mut snapshot_focused = make_test_snapshot(10, 5, " ");
    snapshot_focused.cursor.visual_style = CursorVisualStyle::Block;
    let (bg_focused, _) =
        build_row_instances(0, &snapshot_focused, None, &config, &mut atlas, &gpu.queue);

    // Should have 10 cell background rects + 1 underline rect decoration = 11 total
    assert_eq!(bg_focused.len(), 10 + 1);
    let underline_dec = &bg_focused[1];
    assert_eq!(underline_dec.rect, [0.0, 18.0, 10.0, 2.0]);
    assert_eq!(underline_dec.color, [1.0, 0.0, 0.0, 1.0]);

    // Unfocused snapshot with BlockHollow visual_style -> preserves hollow rectangle
    let mut snapshot_unfocused = make_test_snapshot(10, 5, " ");
    snapshot_unfocused.cursor.visual_style = CursorVisualStyle::BlockHollow;
    let (bg_unfocused, _) = build_row_instances(
        0,
        &snapshot_unfocused,
        None,
        &config,
        &mut atlas,
        &gpu.queue,
    );

    // BlockHollow adds 4 border rects right after cell 0 (indices 1..4) + 9 other cells = 14 total
    assert_eq!(bg_unfocused.len(), 10 + 4);
    assert_eq!(bg_unfocused[1].color, [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(bg_unfocused[2].color, [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(bg_unfocused[3].color, [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(bg_unfocused[4].color, [1.0, 0.0, 0.0, 1.0]);
}
