//! Canonical test scenario generator for native terminal renderer verification.

use crate::native_terminal::color::ColorRgb;
use crate::native_terminal::cursor::{CursorSnapshot, CursorVisualStyle};
use crate::native_terminal::renderer::types::SelectionSnapshot;
use crate::native_terminal::snapshot::{CellSnapshot, CellWide, RenderSnapshot};

fn cell_wide(i: usize) -> CellWide {
    match i {
        0 | 2 | 5 => CellWide::Wide,
        1 | 3 | 6 => CellWide::SpacerTail,
        _ => CellWide::Narrow,
    }
}

fn cell(
    t: &str,
    w: CellWide,
    fg: Option<ColorRgb>,
    bg: Option<ColorRgb>,
    b: bool,
    it: bool,
) -> CellSnapshot {
    CellSnapshot {
        text: t.into(),
        wide: w,
        fg,
        bg,
        bold: b,
        italic: it,
        underline: false,
        inverse: false,
    }
}

pub fn canonical_scenario() -> (RenderSnapshot, SelectionSnapshot) {
    let mut g = vec![vec![cell("", CellWide::Narrow, None, None, false, false); 80]; 24];
    let fg = Some(ColorRgb {
        r: 255,
        g: 100,
        b: 50,
    });
    let bg = Some(ColorRgb {
        r: 10,
        g: 20,
        b: 30,
    });
    for (i, ch) in "Hello Ferryx Native WGPU Terminal!".chars().enumerate() {
        if i < 80 {
            g[0][i] = cell(&ch.to_string(), CellWide::Narrow, fg, bg, true, false);
        }
    }
    let chars = ["東", "", "京", "", " ", "🦀", "", " ", "e\u{0301}"];
    for (i, t) in chars.into_iter().enumerate() {
        g[1][i] = cell(t, cell_wide(i), None, None, false, i == 8);
    }
    let s = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz!@#$%^&*()-_=+[]";
    for i in 0..80 {
        g[2][i] = cell(
            &(s[i % s.len()] as char).to_string(),
            CellWide::Narrow,
            None,
            None,
            false,
            false,
        );
    }
    (
        RenderSnapshot {
            cols: 80,
            rows: 24,
            cursor: CursorSnapshot {
                x: 10,
                y: 0,
                visible: true,
                blinking: false,
                wide_tail: false,
                visual_style: CursorVisualStyle::Block,
            },
            grid: g,
        },
        SelectionSnapshot {
            start_col: 0,
            start_row: 0,
            end_col: 5,
            end_row: 0,
        },
    )
}
