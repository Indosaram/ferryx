#![cfg(feature = "native-terminal")]

use ferryx_lib::native_terminal::renderer::RendererTheme;
use ferryx_lib::native_terminal::{
    NativeTerminal, NativeTerminalRenderer, RenderSnapshot, RendererConfig, ScrollViewport,
    TerminalEngine,
};
use std::ops::Range;
use std::path::PathBuf;

const CELL_W: u32 = 8;
const CELL_H: u32 = 16;

/// Geometry override so a probe can replay at the exact size another emulator used
/// (`FERRYX_PROBE_COLS` / `FERRYX_PROBE_ROWS`). Defaults keep the original harness size.
fn geometry() -> (u16, u16) {
    let read = |key: &str, fallback: u16| {
        std::env::var(key)
            .ok()
            .and_then(|value| value.parse::<u16>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(fallback)
    };
    (
        read("FERRYX_PROBE_COLS", 110),
        read("FERRYX_PROBE_ROWS", 81),
    )
}

/// Print the viewport's text rows so another emulator's buffer can be diffed row by row.
fn print_viewport_rows(label: &str, snapshot: &RenderSnapshot) {
    if std::env::var_os("FERRYX_PROBE_TEXT").is_none() {
        return;
    }
    for row in 0..snapshot.rows as usize {
        println!("[text:{label}:{row}] {}", snapshot.row_text(row));
    }
}

fn history_path() -> Option<PathBuf> {
    std::env::var_os("FERRYX_SESSION_HISTORY").map(PathBuf::from)
}

fn evidence_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("FERRYX_IMAGE_EVIDENCE_DIR")?);
    std::fs::create_dir_all(&dir).expect("evidence dir");
    Some(dir)
}

fn save_frame(frame: &ferryx_lib::native_terminal::OffscreenFrame, name: &str) {
    let Some(dir) = evidence_dir() else { return };
    frame
        .save_png(dir.join(format!("{name}.png")))
        .expect("save frame");
}

fn describe(terminal: &NativeTerminal) -> String {
    let state = terminal.scrollbar().expect("scrollbar");
    format!(
        "total={} offset={} len={} scrollback_rows={}",
        state.total,
        state.offset,
        state.len,
        terminal.scrollback_rows().expect("scrollback rows")
    )
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

fn blank_runs(snapshot: &RenderSnapshot) -> Vec<Range<usize>> {
    let mut runs = Vec::new();
    let mut start: Option<usize> = None;
    for row in 0..snapshot.rows as usize {
        if snapshot.row_text(row).trim().is_empty() {
            start = start.or(Some(row));
        } else if let Some(begin) = start.take() {
            runs.push(begin..row);
        }
    }
    if let Some(begin) = start {
        runs.push(begin..snapshot.rows as usize);
    }
    runs
}

fn report(label: &str, terminal: &NativeTerminal, snapshot: &RenderSnapshot) {
    println!("[{label}] {}", describe(terminal));
    println!("[{label}] placements {:?}", describe_placements(terminal));
    let runs: Vec<Range<usize>> = blank_runs(snapshot)
        .into_iter()
        .filter(|run| run.end - run.start >= 2)
        .collect();
    let blank_rows: usize = runs.iter().map(|run| run.end - run.start).sum();
    println!(
        "[{label}] blank rows {blank_rows} in {} runs: {:?}",
        runs.len(),
        runs
    );
}

#[test]
fn replay_live_session_history_shows_images_and_scrollback() {
    let Some(path) = history_path() else {
        println!("FERRYX_SESSION_HISTORY is unset: skipping the live session replay probe");
        return;
    };
    let bytes = std::fs::read(path).expect("session history");
    let (cols, rows) = geometry();
    println!("replay geometry {cols}x{rows}");
    let mut terminal = NativeTerminal::new(cols, rows).expect("terminal");
    terminal
        .resize(cols, rows, CELL_W, CELL_H)
        .expect("metrics");
    for chunk in bytes.chunks(8191) {
        terminal.feed(chunk).expect("replay");
    }
    println!("replay bytes {}", bytes.len());

    let live = terminal.render_snapshot().expect("snapshot");
    report("live", &terminal, &live);
    print_viewport_rows("bottom", &live);

    terminal.scroll_viewport(ScrollViewport::Top).expect("top");
    let top = terminal.render_snapshot().expect("snapshot");
    report("top", &terminal, &top);
    print_viewport_rows("top", &top);

    let Some(mut renderer) = evidence_dir().map(|_| {
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
    }) else {
        return;
    };
    save_frame(
        &renderer.render_snapshot(&live, None).expect("readback"),
        "live-00-bottom",
    );
    save_frame(
        &renderer.render_snapshot(&top, None).expect("readback"),
        "live-01-top",
    );
    terminal
        .scroll_viewport(ScrollViewport::Bottom)
        .expect("bottom");
    for step in 1..=5 {
        terminal
            .scroll_viewport(ScrollViewport::Delta(-9))
            .expect("up");
        let snapshot = terminal.render_snapshot().expect("snapshot");
        println!(
            "step {step} {} placements {:?}",
            describe(&terminal),
            describe_placements(&terminal)
        );
        save_frame(
            &renderer.render_snapshot(&snapshot, None).expect("readback"),
            &format!("live-{step:02}-up"),
        );
    }
}
