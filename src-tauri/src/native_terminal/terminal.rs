//! Native terminal instance implementation.

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::atomic::Ordering;

use super::bell::TerminalContext;
use super::color::ColorRgb;
use super::cursor::CursorState;
use super::engine::TerminalEngine;
use super::error::NativeTerminalError;
use super::key::KeyEvent;
use super::key_encoder::{encode_key_event, encode_key_event_with_option_as_alt};
use super::lifecycle::{create_native_terminal, teardown_native_terminal};
use super::mouse::MouseEvent;
use super::mouse_encoder::encode_mouse_event;
use super::paste::{encode_paste, paste_is_safe};
use super::queries::{
    query_cols, query_cursor_position, query_cursor_state, query_default_background,
    query_default_cursor_color, query_default_foreground, query_mouse_tracking_enabled,
    query_palette, query_rows, query_scrollback_rows, query_title, query_total_rows,
};
use super::render_pass::capture_render_snapshot;
use super::scroll::{query_scrollbar, scroll_viewport, ScrollViewport, ScrollbarState};
use super::search::search_grid;
use super::selection::{
    clear_selection, select_all, select_line_at, select_word_at, selection_range, selection_text,
};
use super::snapshot::RenderSnapshot;
use super::sys::ffi::{
    ghostty_terminal_reset, ghostty_terminal_resize, ghostty_terminal_set,
    ghostty_terminal_vt_write,
};
use super::sys::types::{
    GhosttyColorRgb, GhosttyString, GhosttyTerminalImpl, GHOSTTY_TERMINAL_OPT_COLOR_BACKGROUND,
    GHOSTTY_TERMINAL_OPT_COLOR_CURSOR, GHOSTTY_TERMINAL_OPT_COLOR_FOREGROUND,
    GHOSTTY_TERMINAL_OPT_COLOR_PALETTE, GHOSTTY_TERMINAL_OPT_TITLE,
};

/// A Rust-owned virtual terminal instance backed by `libghostty-vt`.
///
/// Implements `TerminalEngine`. Access to a single terminal must be serialized.
pub struct NativeTerminal {
    handle: NonNull<GhosttyTerminalImpl>,
    context: Box<TerminalContext>,
}

// SAFETY: Category: Thread Transfer Safety.
// Invariant: NativeTerminal owns its GhosttyTerminalImpl handle and pinned heap context exclusively.
// All access across threads is serialized by Mutex or ownership transfer. No thread-local C state is used.
unsafe impl Send for NativeTerminal {}

impl Drop for NativeTerminal {
    fn drop(&mut self) {
        teardown_native_terminal(self.handle);
    }
}

impl NativeTerminal {
    /// Creates a new native virtual terminal with the specified cell dimensions.
    pub fn new(cols: u16, rows: u16) -> Result<Self, NativeTerminalError> {
        let (handle, context) = create_native_terminal(cols, rows)?;
        Ok(Self { handle, context })
    }

    fn set_color_option(
        &mut self,
        option: i32,
        color: ColorRgb,
        context: &'static str,
    ) -> Result<(), NativeTerminalError> {
        let raw = GhosttyColorRgb::from(color);
        // SAFETY: Category: Foreign Option Configuration.
        // Invariant: raw matches GhosttyColorRgb and is borrowed only for the synchronous set call.
        let result = unsafe {
            ghostty_terminal_set(
                self.handle.as_ptr(),
                option,
                &raw as *const GhosttyColorRgb as *const c_void,
            )
        };
        NativeTerminalError::from_c_result(result, context)
    }

    pub fn set_default_foreground(&mut self, color: ColorRgb) -> Result<(), NativeTerminalError> {
        self.set_color_option(
            GHOSTTY_TERMINAL_OPT_COLOR_FOREGROUND,
            color,
            "ghostty_terminal_set(ColorForeground)",
        )
    }

    pub fn set_default_background(&mut self, color: ColorRgb) -> Result<(), NativeTerminalError> {
        self.set_color_option(
            GHOSTTY_TERMINAL_OPT_COLOR_BACKGROUND,
            color,
            "ghostty_terminal_set(ColorBackground)",
        )
    }

    pub fn set_default_cursor_color(&mut self, color: ColorRgb) -> Result<(), NativeTerminalError> {
        self.set_color_option(
            GHOSTTY_TERMINAL_OPT_COLOR_CURSOR,
            color,
            "ghostty_terminal_set(ColorCursor)",
        )
    }

    pub fn set_palette(&mut self, palette: [ColorRgb; 256]) -> Result<(), NativeTerminalError> {
        let raw = palette.map(GhosttyColorRgb::from);
        // SAFETY: Category: Foreign Array Option Configuration.
        // Invariant: raw is a contiguous array of exactly 256 GhosttyColorRgb values borrowed synchronously.
        let result = unsafe {
            ghostty_terminal_set(
                self.handle.as_ptr(),
                GHOSTTY_TERMINAL_OPT_COLOR_PALETTE,
                raw.as_ptr() as *const c_void,
            )
        };
        NativeTerminalError::from_c_result(result, "ghostty_terminal_set(ColorPalette)")
    }
}

impl TerminalEngine for NativeTerminal {
    fn reset(&mut self) {
        // SAFETY: Category: Foreign State Mutation.
        // Invariant: self.handle is verified non-null and valid terminal instance.
        unsafe {
            ghostty_terminal_reset(self.handle.as_ptr());
        }
    }

    fn dimensions(&self) -> Result<(u16, u16), NativeTerminalError> {
        let cols = self.cols()?;
        let rows = self.rows()?;
        Ok((cols, rows))
    }

    fn cols(&self) -> Result<u16, NativeTerminalError> {
        query_cols(self.handle)
    }

    fn rows(&self) -> Result<u16, NativeTerminalError> {
        query_rows(self.handle)
    }

    fn resize(
        &mut self,
        cols: u16,
        rows: u16,
        cell_width_px: u32,
        cell_height_px: u32,
    ) -> Result<(), NativeTerminalError> {
        if cols == 0 || rows == 0 {
            return Err(NativeTerminalError::InvalidDimensions(cols, rows));
        }

        // SAFETY: Category: Foreign State Mutation.
        // Invariant: self.handle is valid; non-zero dimensions; non-unwinding ABI.
        let result = unsafe {
            ghostty_terminal_resize(
                self.handle.as_ptr(),
                cols,
                rows,
                cell_width_px,
                cell_height_px,
            )
        };
        NativeTerminalError::from_c_result(result, "ghostty_terminal_resize")
    }

    fn feed(&mut self, data: &[u8]) -> Result<(), NativeTerminalError> {
        if data.is_empty() {
            return Ok(());
        }

        // SAFETY: Category: Buffer Borrowing across FFI.
        // Invariant: data.as_ptr() is valid for data.len() bytes; borrowed only synchronously.
        unsafe {
            ghostty_terminal_vt_write(self.handle.as_ptr(), data.as_ptr(), data.len());
        }

        Ok(())
    }

    fn set_title(&mut self, title: &str) -> Result<(), NativeTerminalError> {
        let g_str = GhosttyString {
            ptr: title.as_ptr(),
            len: title.len(),
        };

        // SAFETY: Category: Struct Layout and String Passing.
        // Invariant: &g_str matches GhosttyString { ptr, len }; bytes copied synchronously.
        let result = unsafe {
            ghostty_terminal_set(
                self.handle.as_ptr(),
                GHOSTTY_TERMINAL_OPT_TITLE,
                &g_str as *const _ as *const c_void,
            )
        };

        NativeTerminalError::from_c_result(result, "ghostty_terminal_set(Title)")
    }

    fn title(&self) -> Result<String, NativeTerminalError> {
        query_title(self.handle)
    }

    fn take_title_changed(&mut self) -> bool {
        self.context.title_updated.swap(false, Ordering::AcqRel)
    }

    fn cursor_position(&self) -> Result<(u16, u16), NativeTerminalError> {
        query_cursor_position(self.handle)
    }

    fn cursor_state(&self) -> Result<CursorState, NativeTerminalError> {
        query_cursor_state(self.handle)
    }

    fn render_snapshot(&self) -> Result<RenderSnapshot, NativeTerminalError> {
        capture_render_snapshot(self.handle.as_ptr())
    }

    fn encode_key(&self, event: &KeyEvent) -> Result<Vec<u8>, NativeTerminalError> {
        encode_key_event(self.handle.as_ptr(), event)
    }

    fn encode_key_with_option_as_alt(
        &self,
        event: &KeyEvent,
        option_as_alt: bool,
    ) -> Result<Vec<u8>, NativeTerminalError> {
        encode_key_event_with_option_as_alt(self.handle.as_ptr(), event, option_as_alt)
    }

    fn encode_mouse(&self, event: &MouseEvent) -> Result<Vec<u8>, NativeTerminalError> {
        encode_mouse_event(self.handle.as_ptr(), event)
    }

    fn scroll_viewport(&mut self, behavior: ScrollViewport) -> Result<(), NativeTerminalError> {
        scroll_viewport(self.handle, behavior);
        Ok(())
    }

    fn select_all(&mut self) -> Result<(), NativeTerminalError> {
        select_all(self.handle)
    }

    fn select_word_at(&mut self, col: u16, row: u16) -> Result<(), NativeTerminalError> {
        select_word_at(self.handle, col, row)
    }

    fn select_line_at(&mut self, row: u16) -> Result<(), NativeTerminalError> {
        select_line_at(self.handle, row)
    }

    fn selection_text(&self) -> Result<Option<String>, NativeTerminalError> {
        selection_text(self.handle)
    }

    fn clear_selection(&mut self) -> Result<(), NativeTerminalError> {
        clear_selection(self.handle)
    }

    fn selection_range(&self) -> Result<Option<(u16, u16, u16, u16)>, NativeTerminalError> {
        selection_range(self.handle)
    }

    fn encode_paste(&self, text: &str) -> Result<Vec<u8>, NativeTerminalError> {
        encode_paste(self.handle, text)
    }

    fn paste_is_safe(&self, text: &str) -> bool {
        paste_is_safe(text)
    }

    fn total_rows(&self) -> Result<usize, NativeTerminalError> {
        query_total_rows(self.handle)
    }

    fn scrollback_rows(&self) -> Result<usize, NativeTerminalError> {
        query_scrollback_rows(self.handle)
    }

    fn scrollbar(&self) -> Result<ScrollbarState, NativeTerminalError> {
        query_scrollbar(self.handle)
    }

    fn mouse_tracking_enabled(&self) -> Result<bool, NativeTerminalError> {
        query_mouse_tracking_enabled(self.handle)
    }

    fn default_foreground(&self) -> Result<ColorRgb, NativeTerminalError> {
        query_default_foreground(self.handle)
    }

    fn default_background(&self) -> Result<ColorRgb, NativeTerminalError> {
        query_default_background(self.handle)
    }

    fn default_cursor_color(&self) -> Result<ColorRgb, NativeTerminalError> {
        query_default_cursor_color(self.handle)
    }

    fn palette(&self) -> Result<[ColorRgb; 256], NativeTerminalError> {
        query_palette(self.handle)
    }

    fn search_grid(
        &self,
        query: &str,
        case_sensitive: bool,
    ) -> Result<Vec<(u16, u16, u16)>, NativeTerminalError> {
        search_grid(
            self.handle,
            self.cols()?,
            self.total_rows()?,
            query,
            case_sensitive,
        )
    }

    fn bell_count(&self) -> u64 {
        self.context.bell_counter.load(Ordering::Relaxed)
    }

    fn take_bell_count(&mut self) -> u64 {
        self.context.bell_counter.swap(0, Ordering::Relaxed)
    }
}
