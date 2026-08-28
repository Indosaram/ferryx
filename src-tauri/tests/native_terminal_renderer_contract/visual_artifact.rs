//! Contract test for Phase 2 offscreen frame visual artifact PNG export.

use ferryx_lib::native_terminal::{
    MouseAction, MouseButton, MouseEvent, MousePosition, MouseRendererSize, NativeTerminal,
    NativeTerminalRenderer, OffscreenFrame, RenderSnapshot, RendererConfig, SelectionSnapshot,
    TerminalEngine,
};

#[test]
fn test_real_offscreen_frame_saves_nonempty_png_with_800x480_dimensions() {
    let snapshot: RenderSnapshot = super::snapshot_builder::build_test_snapshot();
    let config = RendererConfig {
        cell_width_px: 10,
        cell_height_px: 20,
        device_scale_factor: 1.0,
        ..Default::default()
    };

    let mut renderer = NativeTerminalRenderer::new(config).expect("renderer creation");
    let frame: OffscreenFrame = renderer
        .render_snapshot(&snapshot, None)
        .expect("render offscreen frame");

    let temp_dir = tempfile::tempdir().expect("create tempdir");
    let png_path = temp_dir.path().join("terminal_snapshot_800x480.png");

    frame
        .save_png(&png_path)
        .expect("save offscreen frame as PNG");

    assert!(
        png_path.exists(),
        "PNG visual artifact file must exist at path"
    );

    let metadata = std::fs::metadata(&png_path).expect("read PNG file metadata");
    assert!(
        metadata.len() > 0,
        "PNG visual artifact file must be non-empty"
    );

    let png_bytes = std::fs::read(&png_path).expect("read saved PNG file");
    // Standard PNG 8-byte magic header: 0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A
    assert!(
        png_bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "saved visual artifact must have valid PNG magic signature"
    );
    assert!(
        png_bytes.len() >= 24,
        "PNG must contain at least PNG signature and 13-byte IHDR chunk"
    );
    // IHDR chunk: 4 bytes chunk length, 4 bytes chunk type "IHDR", 4 bytes width, 4 bytes height
    assert_eq!(
        &png_bytes[12..16],
        b"IHDR",
        "IHDR chunk must immediately follow PNG file signature"
    );
    let width = u32::from_be_bytes(png_bytes[16..20].try_into().expect("IHDR width slice"));
    let height = u32::from_be_bytes(png_bytes[20..24].try_into().expect("IHDR height slice"));
    assert_eq!(
        width, 800,
        "Saved PNG width must match 800px (80 cols * 10px)"
    );
    assert_eq!(
        height, 480,
        "Saved PNG height must match 480px (24 rows * 20px)"
    );
}

#[test]
fn test_pointer_drag_selection_renders_visible_selection_artifact_png() {
    let mut terminal = NativeTerminal::new(80, 24).expect("create native terminal");
    terminal
        .feed(b"select this terminal text")
        .expect("write selectable terminal text");

    let size = MouseRendererSize {
        screen_width: 800,
        screen_height: 480,
        cell_width: 10,
        cell_height: 20,
        padding_top: 0,
        padding_bottom: 0,
        padding_right: 0,
        padding_left: 0,
    };

    let make_event = |action, x, y| MouseEvent {
        action,
        button: (action == MouseAction::Press).then_some(MouseButton::Left),
        position: MousePosition { x, y },
        modifiers: Default::default(),
        size: Some(size),
    };

    // Primary pointer drag across row 0 (from col 0 center x=5 to col 24 end x=250)
    terminal
        .handle_mouse_gesture(&make_event(MouseAction::Press, 5.0, 10.0))
        .expect("start pointer drag selection");
    terminal
        .handle_mouse_gesture(&make_event(MouseAction::Motion, 250.0, 10.0))
        .expect("extend pointer drag selection");
    terminal
        .handle_mouse_gesture(&make_event(MouseAction::Release, 250.0, 10.0))
        .expect("finish pointer drag selection");

    let text = terminal.selection_text().expect("get selection text");
    assert_eq!(
        text.as_deref(),
        Some("select this terminal text"),
        "selection text must match dragged range"
    );

    let range = terminal.selection_range().expect("get selection range");
    assert_eq!(
        range,
        Some((0, 0, 24, 0)),
        "selection range must match columns 0..24 on row 0"
    );

    let snapshot = terminal.render_snapshot().expect("render snapshot");
    let selection = range.map(|(sc, sr, ec, er)| SelectionSnapshot {
        start_col: sc,
        start_row: sr,
        end_col: ec,
        end_row: er,
    });

    let config = RendererConfig {
        cell_width_px: 10,
        cell_height_px: 20,
        device_scale_factor: 1.0,
        ..Default::default()
    };

    let mut renderer = NativeTerminalRenderer::new(config).expect("renderer creation");
    let frame: OffscreenFrame = renderer
        .render_snapshot(&snapshot, selection.as_ref())
        .expect("render offscreen frame with selection");

    let temp_dir = tempfile::tempdir().expect("create tempdir");
    let png_path = temp_dir.path().join("terminal_selection_800x480.png");

    frame
        .save_png(&png_path)
        .expect("save offscreen frame as PNG");

    assert!(png_path.exists());
    let png_bytes = std::fs::read(&png_path).expect("read saved PNG file");
    assert!(png_bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    assert_eq!(&png_bytes[12..16], b"IHDR");
    let width = u32::from_be_bytes(png_bytes[16..20].try_into().expect("IHDR width slice"));
    let height = u32::from_be_bytes(png_bytes[20..24].try_into().expect("IHDR height slice"));
    assert_eq!(width, 800);
    assert_eq!(height, 480);

    // Row 0 col 0 background pixel (x=2, y=2)
    let selected_px_idx = (2 * 800 + 2) * 4;
    let sel_r = frame.pixels[selected_px_idx];
    let sel_g = frame.pixels[selected_px_idx + 1];
    let sel_b = frame.pixels[selected_px_idx + 2];
    assert!(
        sel_b > 100,
        "selected cell background must reflect selection theme color, got RGB({}, {}, {})",
        sel_r,
        sel_g,
        sel_b
    );

    // Unselected cell on row 1 col 0 (x=2, y=22)
    let unselected_px_idx = (22 * 800 + 2) * 4;
    let unsel_r = frame.pixels[unselected_px_idx];
    let unsel_g = frame.pixels[unselected_px_idx + 1];
    let unsel_b = frame.pixels[unselected_px_idx + 2];
    assert_eq!(
        (unsel_r, unsel_g, unsel_b),
        (18, 18, 23),
        "unselected background must match default renderer background #121217"
    );
}
