#![cfg(feature = "native-terminal")]

use ferryx_lib::native_terminal::{NativeTerminal, ScrollViewport, TerminalEngine};
use std::path::PathBuf;

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
        read("FERRYX_PROBE_ROWS", 91),
    )
}

fn depth(terminal: &NativeTerminal) -> u64 {
    let state = terminal.scrollbar().expect("scrollbar");
    state.total.saturating_sub(state.len)
}

fn total(terminal: &NativeTerminal) -> u64 {
    terminal.scrollbar().expect("scrollbar").total
}

fn history_rows(terminal: &mut NativeTerminal) -> Vec<String> {
    let deep = depth(terminal);
    let mut rows = Vec::with_capacity(deep as usize);
    for offset in (0..deep).rev() {
        terminal
            .scroll_viewport(ScrollViewport::Row(offset as usize))
            .expect("row");
        rows.push(
            terminal
                .render_snapshot()
                .expect("snapshot")
                .row_text(0)
                .trim_end()
                .to_string(),
        );
    }
    terminal
        .scroll_viewport(ScrollViewport::Bottom)
        .expect("bottom");
    rows
}

#[test]
fn replay_keeps_the_reference_scrollback_depth() {
    let Ok(path) = std::env::var("FERRYX_SESSION_HISTORY").map(PathBuf::from) else {
        println!("FERRYX_SESSION_HISTORY is unset: skipping the scrollback integrity probe");
        return;
    };
    let bytes = std::fs::read(path).expect("session history");
    let (cols, rows) = geometry();
    let mut terminal = NativeTerminal::new(cols, rows).expect("terminal");
    terminal.resize(cols, rows, 8, 16).expect("cell metrics");

    let chunk = 4096usize;
    let mut peak_total = 0u64;
    let mut losses = Vec::new();
    for (index, slice) in bytes.chunks(chunk).enumerate() {
        terminal.feed(slice).expect("replay chunk");
        let current = total(&terminal);
        if current < peak_total {
            losses.push(format!(
                "chunk {index}: total dropped {peak_total} -> {current}"
            ));
        }
        peak_total = peak_total.max(current);
    }

    let deep = depth(&terminal);
    let retained = history_rows(&mut terminal);
    if let Some(out) = std::env::var_os("FERRYX_PROBE_HISTORY_OUT") {
        std::fs::write(PathBuf::from(out), retained.join("\n")).expect("history dump");
    }

    println!("geometry {cols}x{rows}, bytes {}", bytes.len());
    println!("[result] history depth: {deep}");
    println!("[result] total rows: {peak_total}");
    println!("[result] total decreases: {}", losses.len());
    for line in &losses {
        println!("[loss] {line}");
    }

    assert!(
        losses.is_empty(),
        "scrollback rows were dropped:\n{}",
        losses.join("\n")
    );

    if let Ok(minimum) = std::env::var("FERRYX_PROBE_MIN_DEPTH") {
        let minimum: u64 = minimum.parse().expect("FERRYX_PROBE_MIN_DEPTH");
        assert!(
            deep >= minimum,
            "history depth {deep} is below the reference depth {minimum} for these bytes"
        );
    }
}
