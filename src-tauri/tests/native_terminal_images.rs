#![cfg(feature = "native-terminal")]

use base64::Engine;
use ferryx_lib::native_terminal::composition::PhysicalBounds;
use ferryx_lib::native_terminal::renderer::RendererTheme;
use ferryx_lib::native_terminal::{
    NativeTerminal, NativeTerminalRenderer, OffscreenFrame, RendererConfig, TerminalEngine,
};

fn terminal_and_renderer() -> (NativeTerminal, NativeTerminalRenderer) {
    let mut terminal = NativeTerminal::new(8, 6).expect("terminal");
    terminal.resize(8, 6, 8, 8).expect("cell metrics");
    terminal.feed(b"\x1b[?25l").expect("hide cursor");
    let renderer = NativeTerminalRenderer::new(RendererConfig {
        cell_width_px: 8,
        cell_height_px: 8,
        device_scale_factor: 1.0,
        theme: RendererTheme {
            background: [0.0, 0.0, 0.0, 1.0],
            ..Default::default()
        },
    })
    .expect("real GPU renderer");
    (terminal, renderer)
}

fn kitty(command: &str, bytes: &[u8]) -> Vec<u8> {
    format!(
        "\x1b_G{command};{}\x1b\\",
        base64::prelude::BASE64_STANDARD.encode(bytes)
    )
    .into_bytes()
}

fn pixel(frame: &OffscreenFrame, x: u32, y: u32) -> [u8; 4] {
    let offset = ((y * frame.width_px + x) * 4) as usize;
    frame.pixels[offset..offset + 4].try_into().expect("RGBA")
}

fn png_quadrants() -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 2, 2);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("PNG header");
        writer
            .write_image_data(&[
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255,
            ])
            .expect("PNG pixels");
        writer.finish().expect("PNG end");
    }
    bytes
}

fn render(terminal: &NativeTerminal, renderer: &mut NativeTerminalRenderer) -> OffscreenFrame {
    renderer
        .render_snapshot(&terminal.render_snapshot().expect("snapshot"), None)
        .expect("GPU readback")
}

fn place_rgb(terminal: &mut NativeTerminal, rgb: [u8; 3]) {
    terminal.feed(b"\x1b[2;2H").expect("image origin");
    terminal
        .feed(&kitty(
            "a=T,f=24,s=2,v=2,i=71,c=2,r=2,C=1",
            &rgb.repeat(4),
        ))
        .expect("Kitty RGB");
}

#[test]
fn kitty_rgb_output_reaches_real_gpu_pixels() {
    // Given: an actual VT parser and GPU renderer, with a red 2x2 RGB image.
    let (mut terminal, mut renderer) = terminal_and_renderer();
    terminal.feed(b"\x1b[2;2H").expect("image origin");
    let bytes = [255, 0, 0].repeat(4);

    // When: the same inline image bytes a terminal application writes arrive.
    terminal
        .feed(&kitty("a=T,f=24,s=2,v=2,i=71,c=2,r=2,C=1", &bytes))
        .expect("Kitty stream");
    let frame = renderer
        .render_snapshot(&terminal.render_snapshot().expect("snapshot"), None)
        .expect("GPU readback");

    // Then: pixels at the placement are red, with the surrounding grid intact.
    assert_eq!(pixel(&frame, 12, 12), [255, 0, 0, 255]);
    assert_eq!(pixel(&frame, 4, 4), [0, 0, 0, 255]);
    assert_eq!(pixel(&frame, 28, 12), [0, 0, 0, 255]);
}

#[test]
fn kitty_png_fragmented_stream_preserves_orientation_and_color() {
    // Given: an asymmetric PNG and real parser/renderer, not a mocked snapshot.
    let (mut terminal, mut renderer) = terminal_and_renderer();
    terminal.feed(b"\x1b[2;2H").expect("image origin");
    let command = kitty("a=T,f=100,i=72,c=4,r=4,C=1", &png_quadrants());

    // When: escape delimiters, headers and payload arrive across read boundaries.
    for fragment in command.chunks(3) {
        terminal.feed(fragment).expect("fragment");
    }
    let frame = render(&terminal, &mut renderer);

    // Then: all four quadrants retain their orientation and channel ordering.
    assert_eq!(pixel(&frame, 12, 12), [255, 0, 0, 255]);
    assert_eq!(pixel(&frame, 36, 12), [0, 255, 0, 255]);
    assert_eq!(pixel(&frame, 12, 36), [0, 0, 255, 255]);
    assert_eq!(pixel(&frame, 36, 36), [255, 255, 0, 255]);
}

#[test]
fn kitty_retransmit_same_id_replaces_cached_gpu_pixels() {
    // Given: image 71 already has an uploaded red texture.
    let (mut terminal, mut renderer) = terminal_and_renderer();
    place_rgb(&mut terminal, [255, 0, 0]);
    assert_eq!(pixel(&render(&terminal, &mut renderer), 12, 12), [255, 0, 0, 255]);

    // When: the producer replaces that ID with identical dimensions, but blue pixels.
    place_rgb(&mut terminal, [0, 0, 255]);

    // Then: neither the CPU snapshot nor the GPU cache reuses stale image content.
    assert_eq!(pixel(&render(&terminal, &mut renderer), 12, 12), [0, 0, 255, 255]);
}

#[test]
fn kitty_delete_removes_previously_rendered_image() {
    // Given: an image that has already been presented.
    let (mut terminal, mut renderer) = terminal_and_renderer();
    place_rgb(&mut terminal, [255, 0, 0]);
    assert_eq!(pixel(&render(&terminal, &mut renderer), 12, 12), [255, 0, 0, 255]);

    // When: the producer deletes the image and all its placements.
    terminal.feed(&kitty("a=d,d=I,i=71", &[])).expect("delete");

    // Then: the next frame exposes the background instead of a cached texture.
    assert_eq!(pixel(&render(&terminal, &mut renderer), 12, 12), [0, 0, 0, 255]);
}

#[test]
fn kitty_scroll_moves_image_with_terminal_content() {
    // Given: an image anchored on the second grid row.
    let (mut terminal, mut renderer) = terminal_and_renderer();
    place_rgb(&mut terminal, [255, 0, 0]);

    // When: a line feed at the bottom scrolls the terminal by one row.
    terminal.feed(b"\x1b[6;1H\n").expect("scroll");
    let frame = render(&terminal, &mut renderer);

    // Then: the image moves up eight pixels and leaves no stale tail.
    assert_eq!(pixel(&frame, 12, 4), [255, 0, 0, 255]);
    assert_eq!(pixel(&frame, 12, 20), [0, 0, 0, 255]);
}

#[test]
fn kitty_reset_discards_image_state() {
    // Given: a populated image cache.
    let (mut terminal, mut renderer) = terminal_and_renderer();
    place_rgb(&mut terminal, [255, 0, 0]);
    assert_eq!(pixel(&render(&terminal, &mut renderer), 12, 12), [255, 0, 0, 255]);

    // When: the terminal receives a full reset.
    terminal.feed(b"\x1bc\x1b[?25l").expect("RIS");

    // Then: the old image is absent from the rendered frame.
    assert_eq!(pixel(&render(&terminal, &mut renderer), 12, 12), [0, 0, 0, 255]);
}

#[test]
fn kitty_session_switch_does_not_alias_identical_image_ids() {
    // Given: two sessions use ID 71, with different pixels.
    let (mut first, mut renderer) = terminal_and_renderer();
    let mut second = NativeTerminal::new(8, 6).expect("second terminal");
    second.resize(8, 6, 8, 8).expect("second metrics");
    second.feed(b"\x1b[?25l").expect("second cursor");
    place_rgb(&mut first, [255, 0, 0]);
    place_rgb(&mut second, [0, 0, 255]);
    assert_eq!(pixel(&render(&first, &mut renderer), 12, 12), [255, 0, 0, 255]);

    // When: the renderer displays the other session and then returns.
    let blue = render(&second, &mut renderer);
    let red = render(&first, &mut renderer);

    // Then: an image ID is not mistaken for a process-global texture identity.
    assert_eq!(pixel(&blue, 12, 12), [0, 0, 255, 255]);
    assert_eq!(pixel(&red, 12, 12), [255, 0, 0, 255]);
}

#[test]
fn kitty_split_viewport_applies_origin_and_clips_overflow() {
    // Given: a 2-cell-wide image beginning at the last terminal column.
    let (mut terminal, mut renderer) = terminal_and_renderer();
    terminal.feed(b"\x1b[2;8H").expect("right edge");
    terminal
        .feed(&kitty("a=T,f=24,s=2,v=2,i=73,c=2,r=2,C=1", &[255, 0, 0].repeat(4)))
        .expect("edge image");

    // When: the native surface path draws this pane inside an offset viewport.
    let frame = renderer
        .render_to_offscreen_viewport(
            &terminal.render_snapshot().expect("snapshot"),
            None,
            96,
            80,
            PhysicalBounds { x: 16, y: 16, width: 64, height: 48 },
        )
        .expect("viewport readback");

    // Then: image coordinates include the pane origin and never cross its edge.
    assert_eq!(pixel(&frame, 76, 28), [255, 0, 0, 255]);
    assert_eq!(pixel(&frame, 84, 28), [0, 0, 0, 255]);
    assert_eq!(pixel(&frame, 60, 12), [0, 0, 0, 255]);
}

#[test]
fn kitty_png_visual_evidence_uses_real_terminal_surface_path() {
    // Given: a PNG with four distinguishable quadrants and surrounding terminal text.
    let mut terminal = NativeTerminal::new(80, 24).expect("terminal");
    terminal.resize(80, 24, 8, 16).expect("metrics");
    terminal
        .feed(b"\x1b[?25l\x1b[2;4HFerryx PNG image rendering\x1b[4;4H")
        .expect("heading");
    let png = png_quadrants();
    terminal
        .feed(&kitty("a=T,f=100,i=74,c=24,r=12,C=1", &png))
        .expect("PNG");
    terminal
        .feed(b"\x1b[17;4HTop: red / green    Bottom: blue / yellow")
        .expect("legend");
    let mut renderer = NativeTerminalRenderer::new(RendererConfig {
        cell_width_px: 8,
        cell_height_px: 16,
        ..Default::default()
    })
    .expect("GPU renderer");

    // When: the desktop viewport entry point renders the parsed terminal.
    let frame = renderer
        .render_to_offscreen_viewport(
            &terminal.render_snapshot().expect("snapshot"),
            None,
            640,
            384,
            PhysicalBounds { x: 0, y: 0, width: 640, height: 384 },
        )
        .expect("surface readback");

    // Then: the image and its orientation are correct in the final composed pixels.
    assert_eq!(pixel(&frame, 40, 64), [255, 0, 0, 255]);
    assert_eq!(pixel(&frame, 200, 64), [0, 255, 0, 255]);
    assert_eq!(pixel(&frame, 40, 224), [0, 0, 255, 255]);
    assert_eq!(pixel(&frame, 200, 224), [255, 255, 0, 255]);
    if let Some(directory) = std::env::var_os("FERRYX_IMAGE_EVIDENCE_DIR") {
        let directory = std::path::PathBuf::from(directory);
        frame.save_png(directory.join("native-png.png")).expect("save frame");
        std::fs::write(directory.join("quadrants.png"), png).expect("save input");
    }
}

#[test]
fn kitty_png_above_gpu_texture_limit_does_not_block_text_frames() {
    // Given: a valid PNG wider than WGPU's requested texture limit.
    let (mut terminal, mut renderer) = terminal_and_renderer();
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 9000, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("large PNG header");
        writer.write_image_data(&[255, 0, 0, 255].repeat(9000)).expect("large PNG");
        writer.finish().expect("large PNG end");
    }
    terminal
        .feed(&kitty("a=T,f=100,i=75,c=1,r=1,C=1", &bytes))
        .expect("large image stream");
    assert_eq!(terminal.render_snapshot().expect("parsed image").images.len(), 1);

    // When: text updates arrive while the oversized placement remains visible.
    terminal.feed(b"\x1b[3;1H\x1b[48;2;0;255;0m ").expect("text update");
    let frame = render(&terminal, &mut renderer);

    // Then: an unsupported texture cannot stop the entire terminal repaint.
    assert_eq!(pixel(&frame, 4, 20), [0, 255, 0, 255]);
}
