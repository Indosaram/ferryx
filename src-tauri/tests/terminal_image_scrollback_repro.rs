#![cfg(feature = "native-terminal")]

use base64::Engine;
use ferryx_lib::native_terminal::renderer::RendererTheme;
use ferryx_lib::native_terminal::{
    NativeTerminal, NativeTerminalRenderer, OffscreenFrame, RendererConfig, ScrollViewport,
    TerminalEngine,
};
use std::path::PathBuf;

const CELL_W: u32 = 8;
const CELL_H: u32 = 16;

fn new_terminal(cols: u16, rows: u16) -> NativeTerminal {
    let mut terminal = NativeTerminal::new(cols, rows).expect("terminal");
    terminal
        .resize(cols, rows, CELL_W, CELL_H)
        .expect("cell metrics");
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
    .expect("real GPU renderer")
}

fn kitty(command: &str, bytes: &[u8]) -> Vec<u8> {
    format!(
        "\x1b_G{command};{}\x1b\\",
        base64::prelude::BASE64_STANDARD.encode(bytes)
    )
    .into_bytes()
}

fn red_image_transmission(id: u32, cells: (u32, u32)) -> Vec<u8> {
    let width = cells.0 * CELL_W;
    let height = cells.1 * CELL_H;
    let pixels = [255u8, 0, 0].repeat((width * height) as usize);
    kitty(
        &format!(
            "a=T,f=24,s={width},v={height},i={id},c={},r={},C=1,q=2",
            cells.0, cells.1
        ),
        &pixels,
    )
}

fn pixel(frame: &OffscreenFrame, x: u32, y: u32) -> [u8; 4] {
    let offset = ((y * frame.width_px + x) * 4) as usize;
    frame.pixels[offset..offset + 4].try_into().expect("RGBA")
}

fn evidence_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("FERRYX_IMAGE_EVIDENCE_DIR")?);
    std::fs::create_dir_all(&dir).expect("evidence dir");
    Some(dir)
}

fn save_frame(frame: &OffscreenFrame, name: &str) {
    let Some(dir) = evidence_dir() else { return };
    let path = dir.join(format!("{name}.png"));
    frame.save_png(path.clone()).expect("save frame");
    println!("saved {}", path.display());
}

fn describe_placements(terminal: &NativeTerminal) -> Vec<String> {
    terminal
        .render_snapshot()
        .expect("snapshot")
        .images
        .iter()
        .map(|placement| {
            format!(
                "id={} row={} col={} {}x{}px",
                placement.image.id,
                placement.viewport_row,
                placement.viewport_col,
                placement.pixel_width,
                placement.pixel_height
            )
        })
        .collect()
}

fn describe_scrollbar(terminal: &NativeTerminal) -> String {
    let state = terminal.scrollbar().expect("scrollbar");
    format!(
        "total={} offset={} len={} scrollback_rows={}",
        state.total,
        state.offset,
        state.len,
        terminal.scrollback_rows().expect("scrollback rows")
    )
}

fn render(renderer: &mut NativeTerminalRenderer, terminal: &NativeTerminal) -> OffscreenFrame {
    renderer
        .render_snapshot(&terminal.render_snapshot().expect("snapshot"), None)
        .expect("readback")
}

fn assert_image_present(terminal: &NativeTerminal, context: &str) {
    let placements = describe_placements(terminal);
    assert!(
        !placements.is_empty(),
        "no image placement captured at {context}; the probe would be vacuous"
    );
}

#[test]
fn tui_body_region_image_keeps_body_and_scrollback_consistent() {
    let mut terminal = new_terminal(40, 24);
    let mut renderer = new_renderer();

    terminal
        .feed(b"\x1b[1;20r")
        .expect("scroll region rows 1-20");
    for index in 0..8 {
        terminal
            .feed(format!("\x1b[{};3HBODY {:02}", index + 1, index).as_bytes())
            .expect("body line");
    }
    terminal.feed(b"\x1b[3;6H").expect("image origin");
    terminal
        .feed(&red_image_transmission(91, (6, 3)))
        .expect("kitty image");
    for row in 21..24 {
        terminal
            .feed(format!("\x1b[{row};3Hstatus {row}").as_bytes())
            .expect("status bar");
    }

    println!("initial placements {:?}", describe_placements(&terminal));
    println!("initial {}", describe_scrollbar(&terminal));
    assert_image_present(&terminal, "initial placement inside the body region");
    let frame = render(&mut renderer, &terminal);
    assert_eq!(pixel(&frame, 44, 40), [255, 0, 0, 255], "image pixels");
    save_frame(&frame, "tui-00-initial");

    for step in 1..=6 {
        terminal
            .feed(format!("\x1b[20;3H\nNEW BODY LINE {step:02}").as_bytes())
            .expect("body scroll");
        for row in 21..24 {
            terminal
                .feed(format!("\x1b[{row};3Hstatus {row}").as_bytes())
                .expect("status bar");
        }
        println!(
            "scroll {step} placements {:?}",
            describe_placements(&terminal)
        );
        println!("scroll {step} {}", describe_scrollbar(&terminal));
        save_frame(
            &render(&mut renderer, &terminal),
            &format!("tui-{step:02}-scrolled"),
        );
    }

    let snapshot = terminal.render_snapshot().expect("snapshot");
    for row in 0..20 {
        println!("row {row:2}: {:?}", snapshot.row_text(row));
    }

    assert_eq!(
        terminal.scrollback_rows().expect("scrollback rows"),
        0,
        "a partial DECSTBM region must not push rows into scrollback"
    );
}

#[test]
fn inline_image_pushed_into_scrollback_still_tracks_its_text_row() {
    let mut terminal = new_terminal(40, 10);
    let mut renderer = new_renderer();

    for index in 0..5 {
        terminal
            .feed(format!("LINE {:02}\r\n", index).as_bytes())
            .expect("line");
    }
    terminal
        .scroll_viewport(ScrollViewport::Bottom)
        .expect("live edge");
    terminal.feed(b"\x1b[4;4H").expect("image origin");
    terminal
        .feed(&red_image_transmission(93, (6, 3)))
        .expect("kitty image");
    println!("placed {:?}", describe_placements(&terminal));
    assert_image_present(&terminal, "placement at the live edge");
    save_frame(&render(&mut renderer, &terminal), "push-00-placed");
    terminal.feed(b"\x1b[10;1H").expect("cursor below the image");

    for index in 5..25 {
        terminal
            .feed(format!("LINE {:02}\r\n", index).as_bytes())
            .expect("line");
    }
    println!("after push {}", describe_scrollbar(&terminal));
    println!("at live edge placements {:?}", describe_placements(&terminal));
    save_frame(&render(&mut renderer, &terminal), "push-01-live-edge");

    terminal
        .scroll_viewport(ScrollViewport::Top)
        .expect("oldest retained row");
    let mut saw_image_with_anchor = false;
    for step in 0..24 {
        let snapshot = terminal.render_snapshot().expect("snapshot");
        let rows: Vec<String> = (0..10).map(|row| snapshot.row_text(row)).collect();
        let anchor_row = rows
            .iter()
            .position(|row| row.trim_end() == "LINE 03")
            .map(|row| row as i32);
        let image_row = snapshot
            .images
            .first()
            .map(|placement| placement.viewport_row);
        println!(
            "step {step:2} {} anchor row {anchor_row:?} image row {image_row:?}",
            describe_scrollbar(&terminal)
        );
        if anchor_row == image_row && image_row.is_some() {
            saw_image_with_anchor = true;
        }
        save_frame(
            &render(&mut renderer, &terminal),
            &format!("push-{step:02}-scrolled"),
        );
        terminal
            .scroll_viewport(ScrollViewport::Delta(1))
            .expect("scroll down one row");
    }
    assert!(
        saw_image_with_anchor,
        "a scrollback image must render on the row of the text it was placed on"
    );
}
