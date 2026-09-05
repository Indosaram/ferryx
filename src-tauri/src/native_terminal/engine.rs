//! Public terminal engine trait hiding all Ghostty FFI details.

use super::color::ColorRgb;
use super::cursor::CursorState;
use super::error::NativeTerminalError;
use super::key::KeyEvent;
use super::mouse::MouseEvent;
use super::scroll::{ScrollViewport, ScrollbarState};
use super::snapshot::RenderSnapshot;

/// Public abstract interface for a terminal emulator engine.
pub trait TerminalEngine {
    /// Ingest raw bytes from a PTY or input source into the VT stream parser.
    fn feed(&mut self, data: &[u8]) -> Result<(), NativeTerminalError>;

    /// Ingest a UTF-8 string slice into the VT stream parser.
    fn feed_str(&mut self, text: &str) -> Result<(), NativeTerminalError> {
        self.feed(text.as_bytes())
    }

    /// Resize the terminal grid and update pixel metrics.
    fn resize(
        &mut self,
        cols: u16,
        rows: u16,
        cell_width_px: u32,
        cell_height_px: u32,
    ) -> Result<(), NativeTerminalError>;

    /// Query the current (cols, rows) dimensions.
    fn dimensions(&self) -> Result<(u16, u16), NativeTerminalError>;

    /// Query column count.
    fn cols(&self) -> Result<u16, NativeTerminalError>;

    /// Query row count.
    fn rows(&self) -> Result<u16, NativeTerminalError>;

    /// Query cursor coordinates (x, y) 0-indexed.
    fn cursor_position(&self) -> Result<(u16, u16), NativeTerminalError>;

    /// Query full dynamic cursor state.
    fn cursor_state(&self) -> Result<CursorState, NativeTerminalError>;

    /// Capture an independent, fully-copied render snapshot of the visible viewport.
    fn render_snapshot(&self) -> Result<RenderSnapshot, NativeTerminalError>;

    /// Full terminal reset (RIS).
    fn reset(&mut self);

    /// Query current window title.
    fn title(&self) -> Result<String, NativeTerminalError>;

    /// Set window title manually.
    fn set_title(&mut self, title: &str) -> Result<(), NativeTerminalError>;

    /// Returns true if a title change event occurred via escape sequences since last check.
    fn take_title_changed(&mut self) -> bool;

    /// Encode a keyboard event into terminal escape sequence bytes based on current modes.
    fn encode_key(&self, event: &KeyEvent) -> Result<Vec<u8>, NativeTerminalError>;

    /// Encode a keyboard event, treating macOS Option as Alt/Meta only when `option_as_alt`.
    fn encode_key_with_option_as_alt(
        &self,
        event: &KeyEvent,
        option_as_alt: bool,
    ) -> Result<Vec<u8>, NativeTerminalError>;

    /// Encode a mouse event into terminal escape sequence bytes based on current tracking/format modes.
    fn encode_mouse(&self, event: &MouseEvent) -> Result<Vec<u8>, NativeTerminalError>;

    /// Move the visible viewport within retained scrollback.
    fn scroll_viewport(&mut self, behavior: ScrollViewport) -> Result<(), NativeTerminalError>;

    /// Select all selectable content and install it as the active selection.
    fn select_all(&mut self) -> Result<(), NativeTerminalError>;

    /// Select the word at a visible viewport cell.
    fn select_word_at(&mut self, col: u16, row: u16) -> Result<(), NativeTerminalError>;

    /// Select the line at a visible viewport row.
    fn select_line_at(&mut self, row: u16) -> Result<(), NativeTerminalError>;

    /// Copy the active selection into owned UTF-8 text.
    fn selection_text(&self) -> Result<Option<String>, NativeTerminalError>;

    /// Clear the active selection.
    fn clear_selection(&mut self) -> Result<(), NativeTerminalError>;

    /// Return ordered full-screen selection bounds as (start_col, start_row, end_col, end_row).
    fn selection_range(&self) -> Result<Option<(u16, u16, u16, u16)>, NativeTerminalError>;

    /// Whether bracketed paste mode (DEC mode 2004) is currently active.
    fn bracketed_paste_enabled(&self) -> Result<bool, NativeTerminalError>;

    /// Encode clipboard text according to the terminal's current bracketed-paste mode.
    fn encode_paste(&self, text: &str) -> Result<Vec<u8>, NativeTerminalError>;

    /// Encode clipboard text with an optional override for bracketed paste mode.
    fn encode_paste_with_bracketed_override(
        &self,
        text: &str,
        bracketed_override: Option<bool>,
    ) -> Result<Vec<u8>, NativeTerminalError>;

    /// Conservatively check whether clipboard text can inject terminal commands.
    fn paste_is_safe(&self, text: &str) -> bool;

    /// Query total active-screen rows including retained scrollback.
    fn total_rows(&self) -> Result<usize, NativeTerminalError>;

    /// Query retained scrollback row count.
    fn scrollback_rows(&self) -> Result<usize, NativeTerminalError>;

    /// Query scrollbar dimensions and viewport offset.
    fn scrollbar(&self) -> Result<ScrollbarState, NativeTerminalError>;

    /// Whether any terminal mouse-tracking mode is active.
    fn mouse_tracking_enabled(&self) -> Result<bool, NativeTerminalError>;

    /// Query the effective configured foreground color.
    fn default_foreground(&self) -> Result<ColorRgb, NativeTerminalError>;

    /// Query the effective configured background color.
    fn default_background(&self) -> Result<ColorRgb, NativeTerminalError>;

    /// Query the effective configured cursor color.
    fn default_cursor_color(&self) -> Result<ColorRgb, NativeTerminalError>;

    /// Query the current 256-color palette.
    fn palette(&self) -> Result<[ColorRgb; 256], NativeTerminalError>;

    /// Search full-screen text, including retained scrollback, returning inclusive column bounds.
    fn search_grid(
        &self,
        query: &str,
        case_sensitive: bool,
    ) -> Result<Vec<(u16, u16, u16)>, NativeTerminalError>;

    /// Returns the cumulative number of BEL (0x07) characters received.
    fn bell_count(&self) -> u64;

    /// Resets and returns the cumulative BEL count.
    fn take_bell_count(&mut self) -> u64;
}
