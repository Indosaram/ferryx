//! Contract test for Phase 2 offscreen frame visual artifact PNG export.

use ferryx_lib::native_terminal::{
    NativeTerminalRenderer, OffscreenFrame, RenderSnapshot, RendererConfig,
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
