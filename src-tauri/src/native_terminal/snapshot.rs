//! Deterministic, copied snapshot data types for terminal grid and state.

use std::ffi::c_int;

use super::color::ColorRgb;
use super::cursor::CursorSnapshot;
use super::error::NativeTerminalError;

/// Cell character width semantics.
#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CellWide {
    /// Single column standard character width.
    Narrow,
    /// Multi-column / double-width character (e.g. CJK, emoji).
    Wide,
    /// Spacer tail following a wide character.
    SpacerTail,
    /// Spacer head before a soft-wrapped wide character.
    SpacerHead,
}

impl TryFrom<c_int> for CellWide {
    type Error = NativeTerminalError;

    fn try_from(w: c_int) -> Result<Self, Self::Error> {
        match w {
            0 => Ok(Self::Narrow),
            1 => Ok(Self::Wide),
            2 => Ok(Self::SpacerTail),
            3 => Ok(Self::SpacerHead),
            other => Err(NativeTerminalError::ForeignErrorCode(other)),
        }
    }
}

/// Fully-owned snapshot of a single grid cell.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CellSnapshot {
    /// Renderable text / grapheme cluster (e.g. "A", "東", "🦀", "e\u{0301}").
    /// Empty string indicates a blank / unset cell.
    pub text: String,
    /// Cell width classification.
    pub wide: CellWide,
    /// Resolved foreground color, if explicitly set.
    pub fg: Option<ColorRgb>,
    /// Resolved background color, if explicitly set.
    pub bg: Option<ColorRgb>,
    /// Bold text decoration.
    pub bold: bool,
    /// Italic text decoration.
    pub italic: bool,
    /// Underline text decoration.
    pub underline: bool,
    /// Inverse / reverse video text decoration.
    pub inverse: bool,
    /// Faint / dim text decoration (SGR 2).
    pub faint: bool,
    /// Blinking text decoration (SGR 5).
    pub blink: bool,
    /// Invisible / concealed text decoration (SGR 8).
    pub invisible: bool,
    /// Strikethrough / crossed-out text decoration (SGR 9).
    pub strikethrough: bool,
    /// Overline text decoration (SGR 53).
    pub overline: bool,
}

impl Default for CellSnapshot {
    fn default() -> Self {
        Self {
            text: String::new(),
            wide: CellWide::Narrow,
            fg: None,
            bg: None,
            bold: false,
            italic: false,
            underline: false,
            inverse: false,
            faint: false,
            blink: false,
            invisible: false,
            strikethrough: false,
            overline: false,
        }
    }
}

/// Fully-owned, independent render snapshot of terminal screen and state.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RenderSnapshot {
    pub cols: u16,
    pub rows: u16,
    pub cursor: CursorSnapshot,
    pub grid: Vec<Vec<CellSnapshot>>,
}

impl RenderSnapshot {
    /// Convenience helper to access a cell reference safely with bounds check.
    pub fn cell(&self, col: usize, row: usize) -> Option<&CellSnapshot> {
        self.grid.get(row).and_then(|r| r.get(col))
    }

    /// Convenience helper to reconstruct a text line representation of a row.
    pub fn row_text(&self, row: usize) -> String {
        let Some(cells) = self.grid.get(row) else {
            return String::new();
        };

        let mut line = String::with_capacity(cells.len());
        for cell in cells {
            match cell.wide {
                CellWide::SpacerTail | CellWide::SpacerHead => {}
                CellWide::Narrow | CellWide::Wide => {
                    if cell.text.is_empty() {
                        line.push(' ');
                    } else {
                        line.push_str(&cell.text);
                    }
                }
            }
        }
        line
    }
}
