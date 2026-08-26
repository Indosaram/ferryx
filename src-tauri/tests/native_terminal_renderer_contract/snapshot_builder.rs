//! Snapshot builder helper for native terminal renderer contract tests.

use ferryx_lib::native_terminal::{
    CellSnapshot, CellWide, ColorRgb, CursorSnapshot, CursorVisualStyle, RenderSnapshot,
};

pub fn empty_cell() -> CellSnapshot {
    CellSnapshot {
        text: String::new(),
        wide: CellWide::Narrow,
        fg: None,
        bg: None,
        bold: false,
        italic: false,
        underline: false,
        inverse: false,
    }
}

pub fn make_cell(
    text: &str,
    wide: CellWide,
    fg: Option<ColorRgb>,
    bg: Option<ColorRgb>,
    bold: bool,
    italic: bool,
    underline: bool,
    inverse: bool,
) -> CellSnapshot {
    CellSnapshot {
        text: text.to_string(),
        wide,
        fg,
        bg,
        bold,
        italic,
        underline,
        inverse,
    }
}

pub fn build_test_snapshot() -> RenderSnapshot {
    let cols = 80u16;
    let rows = 24u16;
    let mut grid = vec![vec![empty_cell(); cols as usize]; rows as usize];

    // Row 0: ANSI RGB styled cells
    let rgb_fg = Some(ColorRgb {
        r: 255,
        g: 100,
        b: 50,
    });
    let rgb_bg = Some(ColorRgb {
        r: 10,
        g: 20,
        b: 30,
    });
    grid[0][0] = make_cell(
        "H",
        CellWide::Narrow,
        rgb_fg,
        rgb_bg,
        true,
        false,
        false,
        false,
    );
    grid[0][1] = make_cell(
        "e",
        CellWide::Narrow,
        rgb_fg,
        rgb_bg,
        true,
        false,
        false,
        false,
    );
    grid[0][2] = make_cell(
        "l",
        CellWide::Narrow,
        rgb_fg,
        rgb_bg,
        true,
        false,
        false,
        false,
    );
    grid[0][3] = make_cell(
        "l",
        CellWide::Narrow,
        rgb_fg,
        rgb_bg,
        true,
        false,
        false,
        false,
    );
    grid[0][4] = make_cell(
        "o",
        CellWide::Narrow,
        rgb_fg,
        rgb_bg,
        true,
        false,
        false,
        false,
    );

    // Row 1: CJK, Emoji, and Unicode combining marks
    grid[1][0] = make_cell("東", CellWide::Wide, None, None, false, false, false, false);
    grid[1][1] = make_cell(
        "",
        CellWide::SpacerTail,
        None,
        None,
        false,
        false,
        false,
        false,
    );
    grid[1][2] = make_cell("京", CellWide::Wide, None, None, false, false, false, false);
    grid[1][3] = make_cell(
        "",
        CellWide::SpacerTail,
        None,
        None,
        false,
        false,
        false,
        false,
    );
    grid[1][4] = make_cell(
        " ",
        CellWide::Narrow,
        None,
        None,
        false,
        false,
        false,
        false,
    );
    grid[1][5] = make_cell("🦀", CellWide::Wide, None, None, false, false, false, false);
    grid[1][6] = make_cell(
        "",
        CellWide::SpacerTail,
        None,
        None,
        false,
        false,
        false,
        false,
    );
    grid[1][7] = make_cell(
        " ",
        CellWide::Narrow,
        None,
        None,
        false,
        false,
        false,
        false,
    );
    grid[1][8] = make_cell(
        "e\u{0301}",
        CellWide::Narrow,
        None,
        None,
        false,
        true,
        false,
        false,
    );

    // Row 2: Long continuous text filling all 80 columns
    let long_line_bytes =
        b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz!@#$%^&*()-_=+[]";
    for col in 0..(cols as usize) {
        let ch = (long_line_bytes[col % long_line_bytes.len()] as char).to_string();
        grid[2][col] = make_cell(
            &ch,
            CellWide::Narrow,
            None,
            None,
            false,
            false,
            false,
            false,
        );
    }

    RenderSnapshot {
        cols,
        rows,
        cursor: CursorSnapshot {
            x: 10,
            y: 0,
            visible: true,
            blinking: false,
            wide_tail: false,
            visual_style: CursorVisualStyle::Block,
        },
        grid,
    }
}
