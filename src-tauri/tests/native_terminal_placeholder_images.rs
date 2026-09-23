#![cfg(feature = "native-terminal")]

use base64::Engine;
use ferryx_lib::native_terminal::renderer::RendererTheme;
use ferryx_lib::native_terminal::{
    NativeTerminal, NativeTerminalRenderer, OffscreenFrame, RendererConfig, ScrollViewport,
    TerminalEngine,
};

const CELL_W: u32 = 8;
const CELL_H: u32 = 16;
const PLACEHOLDER: char = '\u{10EEEE}';

const ROW_0: char = '\u{0305}';
const ROW_1: char = '\u{030D}';
const ROW_2: char = '\u{030E}';
const ROW_3: char = '\u{0310}';
const COL_0: char = '\u{0305}';
const COL_1: char = '\u{030D}';
const COL_2: char = '\u{030E}';
const HIGH_OUT_OF_RANGE: char = '\u{a8ea}';

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

fn new_terminal(cols: u16, rows: u16) -> NativeTerminal {
    let mut terminal = NativeTerminal::new(cols, rows).expect("terminal");
    terminal
        .resize(cols, rows, CELL_W, CELL_H)
        .expect("metrics");
    terminal.feed(b"\x1b[?25l").expect("hide cursor");
    terminal
}

fn new_renderer() -> NativeTerminalRenderer {
    NativeTerminalRenderer::new(RendererConfig {
        cell_width_px: CELL_W,
        cell_height_px: CELL_H,
        device_scale_factor: 1.0,
        theme: RendererTheme {
            background: [0.0, 0.0, 0.0, 1.0],
            ..Default::default()
        },
    })
    .expect("renderer")
}

fn transmit_virtual_image(
    terminal: &mut NativeTerminal,
    image_id: u32,
    width: u32,
    height: u32,
    grid: (u32, u32),
    placement_id: Option<u32>,
) {
    let pixels = [255u8, 0, 0].repeat((width * height) as usize);
    let placement = placement_id.map_or(String::new(), |id| format!(",p={id}"));
    let command = format!(
        "a=T,f=24,s={width},v={height},i={image_id},U=1,c={},r={}{placement},q=2",
        grid.0, grid.1
    );
    terminal
        .feed(&kitty(&command, &pixels))
        .expect("virtual image");
}

fn place_virtual_placement(
    terminal: &mut NativeTerminal,
    image_id: u32,
    grid: (u32, u32),
    placement_id: u32,
) {
    let command = format!(
        "a=p,i={image_id},U=1,c={},r={},p={placement_id},q=2",
        grid.0, grid.1
    );
    terminal
        .feed(format!("\x1b_G{command}\x1b\\").as_bytes())
        .expect("virtual placement");
}

fn placeholder_row_with_id(image_id: u32, cells: &str) -> Vec<u8> {
    let r = (image_id >> 16) & 0xff;
    let g = (image_id >> 8) & 0xff;
    let b = image_id & 0xff;
    format!("\x1b[38;2;{r};{g};{b}m{cells}\x1b[39m").into_bytes()
}

fn explicit_cell(row: char, column: char) -> String {
    format!("{PLACEHOLDER}{row}{column}")
}

fn block(image_id: u32, columns: u32, rows: u32) -> Vec<u8> {
    let mut output = Vec::new();
    for row in 1..=rows {
        let row_diacritic = if row == 1 { ROW_0 } else { ROW_1 };
        let mut cells = String::new();
        for column in 0..columns {
            cells.push_str(&explicit_cell(
                row_diacritic,
                if column == 0 {
                    COL_0
                } else if column == 1 {
                    COL_1
                } else {
                    COL_2
                },
            ));
        }
        output.extend(placeholder_row_with_id(image_id, &cells));
        output.extend_from_slice(b"\r\n");
    }
    output
}

#[test]
fn placeholder_cells_resolve_to_aspect_preserved_tiles() {
    let mut terminal = new_terminal(8, 4);
    terminal.feed(b"\x1b[H").expect("home");
    transmit_virtual_image(&mut terminal, 4242, 2, 2, (2, 2), None);
    terminal
        .feed(&block(4242, 2, 2))
        .expect("placeholder cells");

    let snapshot = terminal.render_snapshot().expect("snapshot");
    let placements: Vec<(i32, i32, [u32; 4], Option<[u32; 4]>)> = snapshot
        .images
        .iter()
        .map(|placement| {
            (
                placement.viewport_row,
                placement.viewport_col,
                placement.source,
                placement.dest_px,
            )
        })
        .collect();
    assert_eq!(
        placements,
        vec![
            (0, 0, [0, 0, 2, 1], Some([0, 8, 16, 8])),
            (1, 0, [0, 1, 2, 1], Some([0, 16, 16, 8])),
        ],
        "a square image in a 2x2 grid of 8x16 cells must be letterboxed, not stretched"
    );
}

#[test]
fn partially_scrolled_block_keeps_every_tile_on_its_own_row() {
    let mut terminal = new_terminal(8, 3);
    let mut renderer = new_renderer();
    // A four row block in a three row viewport: the top row scrolls above the
    // viewport, so the placement origin is negative.
    transmit_virtual_image(&mut terminal, 9000, 8, 64, (1, 4), None);
    let mut cells = String::new();
    for row in [ROW_0, ROW_1, ROW_2, ROW_3] {
        cells.clear();
        cells.push_str(&explicit_cell(row, COL_0));
        terminal
            .feed(&placeholder_row_with_id(9000, &cells))
            .expect("row");
        terminal.feed(b"\r\n").expect("newline");
    }

    let snapshot = terminal.render_snapshot().expect("snapshot");
    let tiles: Vec<(i32, Option<[u32; 4]>)> = snapshot
        .images
        .iter()
        .map(|placement| (placement.viewport_row, placement.dest_px))
        .collect();
    // The block occupies four rows and its trailing newline pushes two of them
    // above the three row viewport, so the visible tiles are its last two.
    assert_eq!(
        tiles,
        vec![(0, Some([0, 0, 8, 16])), (1, Some([0, 16, 8, 16])),],
        "a tile must be drawn on its own viewport row even when the block's top scrolled off"
    );
    let frame = renderer.render_snapshot(&snapshot, None).expect("readback");
    assert_eq!(pixel(&frame, 4, 4), [255, 0, 0, 255], "first visible row");
}

#[test]
fn placeholder_tiles_reach_real_gpu_pixels_and_survive_scrollback() {
    let mut terminal = new_terminal(8, 5);
    let mut renderer = new_renderer();
    terminal.feed(b"\x1b[H").expect("home");
    transmit_virtual_image(&mut terminal, 5252, 2, 2, (2, 2), None);
    terminal
        .feed(&block(5252, 2, 2))
        .expect("placeholder cells");

    let frame = renderer
        .render_snapshot(&terminal.render_snapshot().expect("snapshot"), None)
        .expect("readback");
    assert_eq!(pixel(&frame, 4, 4), [0, 0, 0, 255], "letterbox above row 0");
    assert_eq!(pixel(&frame, 4, 12), [255, 0, 0, 255], "tile of row 0");
    assert_eq!(
        pixel(&frame, 12, 12),
        [255, 0, 0, 255],
        "tile of row 0 col 1"
    );
    assert_eq!(pixel(&frame, 4, 20), [255, 0, 0, 255], "tile of row 1");
    assert_eq!(pixel(&frame, 4, 40), [0, 0, 0, 255], "below the block");

    for index in 0..6 {
        terminal
            .feed(format!("LINE {index}\r\n").as_bytes())
            .expect("push the block into history");
    }
    let scrollback_rows = terminal.scrollback_rows().expect("scrollback rows");
    assert!(
        scrollback_rows >= 3,
        "the placeholder block must scroll into history (got {scrollback_rows})"
    );
    terminal
        .scroll_viewport(ScrollViewport::Top)
        .expect("oldest viewport");
    let scrolled = terminal.render_snapshot().expect("snapshot");
    let rows: Vec<i32> = scrolled
        .images
        .iter()
        .map(|placement| placement.viewport_row)
        .collect();
    assert_eq!(
        rows,
        vec![0, 1],
        "scrolled back, the tiles must still sit on their placeholder rows"
    );
    let frame = renderer.render_snapshot(&scrolled, None).expect("readback");
    assert_eq!(
        pixel(&frame, 4, 12),
        [255, 0, 0, 255],
        "the scrolled-back tile must still render"
    );
    assert_eq!(
        pixel(&frame, 4, 20),
        [255, 0, 0, 255],
        "the second scrolled-back tile must still render"
    );
}

#[test]
fn indented_block_uses_image_columns_for_its_source() {
    let mut terminal = new_terminal(8, 3);
    terminal.feed(b"\x1b[H").expect("home");
    transmit_virtual_image(&mut terminal, 6100, 2, 1, (2, 1), None);
    terminal.feed(b"\x1b[1;3H").expect("indent by two cells");
    terminal
        .feed(&block(6100, 2, 1))
        .expect("placeholder cells");

    let snapshot = terminal.render_snapshot().expect("snapshot");
    let placements: Vec<(i32, i32, [u32; 4], Option<[u32; 4]>)> = snapshot
        .images
        .iter()
        .map(|placement| {
            (
                placement.viewport_row,
                placement.viewport_col,
                placement.source,
                placement.dest_px,
            )
        })
        .collect();
    assert_eq!(
        placements,
        vec![(0, 2, [0, 0, 2, 1], Some([16, 4, 16, 8]))],
        "screen columns must place the tile; image columns must source it"
    );
}

#[test]
fn compact_placeholder_encoding_resolves() {
    let mut terminal = new_terminal(8, 3);
    terminal.feed(b"\x1b[H").expect("home");
    transmit_virtual_image(&mut terminal, 6200, 3, 1, (3, 1), None);
    let compact = format!("{PLACEHOLDER}{ROW_0}{PLACEHOLDER}{PLACEHOLDER}");
    terminal
        .feed(&placeholder_row_with_id(6200, &compact))
        .expect("compact row");

    let snapshot = terminal.render_snapshot().expect("snapshot");
    let placements: Vec<(i32, i32, [u32; 4])> = snapshot
        .images
        .iter()
        .map(|placement| {
            (
                placement.viewport_row,
                placement.viewport_col,
                placement.source,
            )
        })
        .collect();
    assert_eq!(
        placements,
        vec![(0, 0, [0, 0, 3, 1])],
        "the protocol's compact row (one row diacritic, bare continuations) must resolve"
    );
}

#[test]
fn upscaled_image_keeps_every_placeholder_row() {
    let mut terminal = new_terminal(8, 3);
    terminal.feed(b"\x1b[H").expect("home");
    transmit_virtual_image(&mut terminal, 6300, 1, 1, (1, 2), None);
    let mut cells = String::new();
    for row in [ROW_0, ROW_1] {
        cells.clear();
        cells.push_str(&explicit_cell(row, COL_0));
        terminal
            .feed(&placeholder_row_with_id(6300, &cells))
            .expect("row");
        terminal.feed(b"\r\n").expect("newline");
    }

    let snapshot = terminal.render_snapshot().expect("snapshot");
    let rows: Vec<(i32, [u32; 4])> = snapshot
        .images
        .iter()
        .map(|placement| (placement.viewport_row, placement.source))
        .collect();
    assert_eq!(
        rows,
        vec![(0, [0, 0, 1, 1]), (1, [0, 0, 1, 1])],
        "a one-pixel-high image spread over two rows must not lose a row"
    );
}

#[test]
fn palette_indexed_placeholder_uses_the_index_as_image_id() {
    let mut terminal = new_terminal(8, 3);
    terminal.feed(b"\x1b[H").expect("home");
    transmit_virtual_image(&mut terminal, 42, 1, 1, (1, 1), None);
    let cells = explicit_cell(ROW_0, COL_0);
    terminal
        .feed(format!("\x1b[38;5;42m{cells}\x1b[39m").as_bytes())
        .expect("palette indexed row");

    let snapshot = terminal.render_snapshot().expect("snapshot");
    assert_eq!(
        snapshot.images.len(),
        1,
        "an indexed foreground color carries the image id as its palette index"
    );
    assert_eq!(snapshot.images[0].image.id, 42);
}

#[test]
fn invalid_high_diacritic_does_not_alias_another_image() {
    let mut terminal = new_terminal(8, 3);
    terminal.feed(b"\x1b[H").expect("home");
    transmit_virtual_image(&mut terminal, 700, 1, 1, (1, 1), None);
    let cells = format!("{PLACEHOLDER}{ROW_0}{COL_0}{HIGH_OUT_OF_RANGE}");
    terminal
        .feed(&placeholder_row_with_id(700, &cells))
        .expect("row");

    let snapshot = terminal.render_snapshot().expect("snapshot");
    assert_eq!(
        snapshot.images.len(),
        1,
        "a high-byte diacritic above 255 is invalid and must be treated as absent"
    );
    assert_eq!(snapshot.images[0].image.id, 700);
}

#[test]
fn underline_color_selects_the_placement_and_bounds_its_grid() {
    let mut terminal = new_terminal(8, 3);
    terminal.feed(b"\x1b[H").expect("home");
    transmit_virtual_image(&mut terminal, 8100, 4, 1, (2, 1), Some(7));
    place_virtual_placement(&mut terminal, 8100, (4, 1), 8);
    let cells = explicit_cell(ROW_0, COL_0) + &explicit_cell(ROW_0, COL_1);
    terminal
        .feed(format!("\x1b[4m\x1b[38;2;0;31;164m\x1b[58;5;7m{cells}\x1b[59m\x1b[39m").as_bytes())
        .expect("row with an underline placement id");

    let selected: Vec<(u32, [u32; 4], Option<[u32; 4]>)> = terminal
        .render_snapshot()
        .expect("snapshot")
        .images
        .iter()
        .map(|placement| (placement.placement_id, placement.source, placement.dest_px))
        .collect();
    assert_eq!(selected.len(), 1);
    assert_eq!(
        selected[0].0, 7,
        "an underline placement id must select the matching placement, not any placement of the image"
    );
    assert_eq!(selected[0].1, [0, 0, 4, 1]);

    let mut second = new_terminal(8, 3);
    second.feed(b"\x1b[H").expect("home");
    transmit_virtual_image(&mut second, 8100, 4, 1, (2, 1), Some(7));
    place_virtual_placement(&mut second, 8100, (4, 1), 8);
    second
        .feed(format!("\x1b[4m\x1b[38;2;0;31;164m\x1b[58;5;8m{cells}\x1b[59m\x1b[39m").as_bytes())
        .expect("row with the other placement id");
    let other: Vec<(u32, [u32; 4], Option<[u32; 4]>)> = second
        .render_snapshot()
        .expect("snapshot")
        .images
        .iter()
        .map(|placement| (placement.placement_id, placement.source, placement.dest_px))
        .collect();
    assert_eq!(other.len(), 1);
    assert_eq!(
        other[0].0, 8,
        "the other placement id selects the other grid"
    );
    assert_ne!(
        other[0].2, selected[0].2,
        "each placement id brings its own grid geometry"
    );
}
