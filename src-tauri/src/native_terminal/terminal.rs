//! Native terminal instance implementation.

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::atomic::Ordering;

use super::bell::TerminalContext;
use super::color::ColorRgb;
use super::cursor::CursorState;
use super::engine::TerminalEngine;
use super::error::NativeTerminalError;
use super::guards::SelectionGestureGuard;
use super::key::KeyEvent;
use super::key_encoder::{encode_key_event, encode_key_event_with_option_as_alt};
use super::lifecycle::{create_native_terminal, teardown_native_terminal};
use super::mouse::MouseEvent;
use super::mouse_encoder::encode_mouse_event;
use super::paste::{
    bracketed_paste_enabled, encode_paste, encode_paste_with_override, paste_is_safe,
};
use super::queries::{
    query_cols, query_cursor_position, query_cursor_state, query_default_background,
    query_default_cursor_color, query_default_foreground, query_mouse_tracking_enabled,
    query_palette, query_rows, query_scrollback_rows, query_title, query_total_rows,
};
use super::render_pass::capture_render_snapshot;
use super::scroll::{query_scrollbar, scroll_viewport, ScrollViewport, ScrollbarState};
use super::search::search_grid;
use super::selection::{
    apply_mouse_gesture, clear_selection, create_selection_gesture, reset_selection_gesture,
    select_all, select_line_at, select_word_at, selection_range, selection_text,
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
use crate::terminal::TerminalThemeColors;

const ONE_DARK_DEFAULTS: [&str; 16] = [
    "#1d1f21", "#cc6666", "#b5bd68", "#f0c674", "#81a2be", "#b294bb", "#8abeb7", "#c5c8c6",
    "#666666", "#d54e53", "#b9ca4a", "#e7c547", "#7aa6da", "#c397d8", "#70c0b1", "#eaeaea",
];

pub(crate) fn full_palette(theme: &TerminalThemeColors) -> [ColorRgb; 256] {
    let mut palette = [ColorRgb::default(); 256];

    let named = [
        &theme.black,
        &theme.red,
        &theme.green,
        &theme.yellow,
        &theme.blue,
        &theme.magenta,
        &theme.cyan,
        &theme.white,
        &theme.bright_black,
        &theme.bright_red,
        &theme.bright_green,
        &theme.bright_yellow,
        &theme.bright_blue,
        &theme.bright_magenta,
        &theme.bright_cyan,
        &theme.bright_white,
    ];

    for (i, color_str) in named.iter().enumerate() {
        palette[i] = ColorRgb::from_hex(color_str)
            .or_else(|| ColorRgb::from_hex(ONE_DARK_DEFAULTS[i]))
            .unwrap_or_default();
    }

    let mut idx = 16;
    for r in 0..6u8 {
        for g in 0..6u8 {
            for b in 0..6u8 {
                palette[idx] = ColorRgb::new(
                    if r == 0 { 0 } else { r * 40 + 55 },
                    if g == 0 { 0 } else { g * 40 + 55 },
                    if b == 0 { 0 } else { b * 40 + 55 },
                );
                idx += 1;
            }
        }
    }

    for i in 232..=255 {
        let gray = ((i - 232) as u8) * 10 + 8;
        palette[i] = ColorRgb::new(gray, gray, gray);
    }

    for &(override_idx, ref hex) in &theme.palette_overrides {
        if let Some(color) = ColorRgb::from_hex(hex) {
            palette[override_idx as usize] = color;
        }
    }

    palette
}

/// A Rust-owned virtual terminal instance backed by `libghostty-vt`.
///
/// Implements `TerminalEngine`. Access to a single terminal must be serialized.
pub struct NativeTerminal {
    handle: NonNull<GhosttyTerminalImpl>,
    context: Box<TerminalContext>,
    gesture: SelectionGestureGuard,
}

// SAFETY: Category: Thread Transfer Safety.
// Invariant: NativeTerminal owns its GhosttyTerminalImpl handle and pinned heap context exclusively.
// All access across threads is serialized by Mutex or ownership transfer. No thread-local C state is used.
unsafe impl Send for NativeTerminal {}

impl Drop for NativeTerminal {
    fn drop(&mut self) {
        self.gesture.free_with_terminal(self.handle.as_ptr());
        teardown_native_terminal(self.handle);
    }
}

impl NativeTerminal {
    /// Creates a new native virtual terminal with the specified cell dimensions.
    pub fn new(cols: u16, rows: u16) -> Result<Self, NativeTerminalError> {
        let (handle, context) = create_native_terminal(cols, rows)?;
        let gesture = match create_selection_gesture() {
            Ok(g) => g,
            Err(e) => {
                teardown_native_terminal(handle);
                return Err(e);
            }
        };
        Ok(Self {
            handle,
            context,
            gesture,
        })
    }

    /// Handles a selection gesture mouse event (press, drag, release) when mouse tracking is disabled.
    pub fn handle_mouse_gesture(&mut self, event: &MouseEvent) -> Result<(), NativeTerminalError> {
        apply_mouse_gesture(self.handle, &self.gesture, event)
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

    pub fn apply_theme_preferences(
        &mut self,
        theme: &TerminalThemeColors,
    ) -> Result<(), NativeTerminalError> {
        if let Some(fg) = ColorRgb::from_hex(&theme.foreground) {
            self.set_default_foreground(fg)?;
        }
        if let Some(bg) = ColorRgb::from_hex(&theme.background) {
            self.set_default_background(bg)?;
        }
        if let Some(cursor) = ColorRgb::from_hex(&theme.cursor) {
            self.set_default_cursor_color(cursor)?;
        }
        self.set_palette(full_palette(theme))
    }
}

impl TerminalEngine for NativeTerminal {
    fn reset(&mut self) {
        // SAFETY: Category: Foreign State Mutation.
        // Invariant: self.handle is verified non-null and valid terminal instance.
        unsafe {
            ghostty_terminal_reset(self.handle.as_ptr());
        }
        reset_selection_gesture(&self.gesture, self.handle);
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

    fn bracketed_paste_enabled(&self) -> Result<bool, NativeTerminalError> {
        bracketed_paste_enabled(self.handle)
    }

    fn encode_paste(&self, text: &str) -> Result<Vec<u8>, NativeTerminalError> {
        encode_paste(self.handle, text)
    }

    fn encode_paste_with_bracketed_override(
        &self,
        text: &str,
        bracketed_override: Option<bool>,
    ) -> Result<Vec<u8>, NativeTerminalError> {
        encode_paste_with_override(self.handle, text, bracketed_override)
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

#[cfg(test)]
mod tests {
    use super::NativeTerminal;
    use crate::native_terminal::color::ColorRgb;
    use crate::native_terminal::engine::TerminalEngine;
    use crate::native_terminal::terminal::full_palette;
    use crate::native_terminal::{
        MouseAction, MouseButton, MouseEvent, MousePosition, MouseRendererSize,
    };
    use crate::terminal::TerminalThemeColors;

    #[test]
    fn test_full_palette_defaults_cube_grays_and_overrides() {
        let default_theme = TerminalThemeColors::default();
        let palette = full_palette(&default_theme);

        // Cube values
        assert_eq!(palette[16], ColorRgb::new(0, 0, 0)); // #000000
        assert_eq!(palette[17], ColorRgb::new(0, 0, 0x5f)); // #00005f
        assert_eq!(palette[57], ColorRgb::new(0x5f, 0, 0xff)); // #5f00ff
        assert_eq!(palette[231], ColorRgb::new(255, 255, 255)); // #ffffff

        // Gray ramp values
        assert_eq!(palette[232], ColorRgb::new(8, 8, 8)); // #080808
        assert_eq!(palette[255], ColorRgb::new(0xee, 0xee, 0xee)); // #eeeeee

        // Override wins
        let mut override_theme = TerminalThemeColors::default();
        override_theme
            .palette_overrides
            .push((200, "#123456".to_string()));
        let overridden_palette = full_palette(&override_theme);
        assert_eq!(overridden_palette[200], ColorRgb::new(0x12, 0x34, 0x56));
    }

    #[test]
    fn test_apply_theme_preferences_on_terminal() {
        let mut terminal = NativeTerminal::new(80, 24).expect("create native terminal");
        let mut theme = TerminalThemeColors::default();
        theme.foreground = "#112233".to_string();
        theme.background = "#445566".to_string();
        theme.cursor = "#778899".to_string();
        theme.palette_overrides.push((200, "#abcdef".to_string()));

        terminal
            .apply_theme_preferences(&theme)
            .expect("apply theme preferences");

        assert_eq!(
            terminal.default_foreground().expect("foreground"),
            ColorRgb::new(0x11, 0x22, 0x33)
        );
        assert_eq!(
            terminal.default_background().expect("background"),
            ColorRgb::new(0x44, 0x55, 0x66)
        );
        assert_eq!(
            terminal.default_cursor_color().expect("cursor"),
            ColorRgb::new(0x77, 0x88, 0x99)
        );
        let palette = terminal.palette().expect("palette");
        assert_eq!(palette[200], ColorRgb::new(0xab, 0xcd, 0xef));
    }

    #[test]
    fn native_terminal_ffi_probe_osc_2_reports_title_change_and_value() {
        let mut terminal = NativeTerminal::new(80, 24).expect("create live native terminal");
        assert!(
            !terminal.take_title_changed(),
            "new terminal has no pending title event"
        );

        terminal
            .feed(b"\x1b]2;some-agent-title\x07")
            .expect("feed OSC 2 title through NativeTerminal::feed");

        let title_changed = terminal.take_title_changed();
        let title = terminal.title().expect("query title after OSC 2 callback");
        println!("title_changed={title_changed} title={title:?}");
        assert!(
            title_changed,
            "OSC 2 did not invoke the registered title callback"
        );
        assert_eq!(title, "some-agent-title");
        assert!(
            !terminal.take_title_changed(),
            "title event was not drained"
        );
    }

    #[test]
    fn native_terminal_ffi_probe_bel_reports_counter_observation() {
        let mut terminal = NativeTerminal::new(80, 24).expect("create live native terminal");
        assert_eq!(
            terminal.take_bell_count(),
            0,
            "new terminal has no pending bells"
        );

        terminal
            .feed(&[0x07])
            .expect("feed BEL through NativeTerminal::feed");

        let bell_count = terminal.take_bell_count();
        println!("bell_count={bell_count}");
        assert_eq!(
            bell_count, 1,
            "BEL did not invoke the registered bell callback"
        );
        assert_eq!(
            terminal.take_bell_count(),
            0,
            "bell observation was not drained"
        );
    }

    #[test]
    fn native_terminal_pointer_drag_installs_a_text_selection() {
        let mut terminal = NativeTerminal::new(80, 24).expect("create native terminal");
        terminal
            .feed(b"select this terminal text")
            .expect("write selectable terminal text");
        let size = MouseRendererSize {
            screen_width: 800,
            screen_height: 480,
            cell_width: 10,
            cell_height: 20,
            padding_top: 0,
            padding_bottom: 0,
            padding_right: 0,
            padding_left: 0,
        };
        let event = |action, x| MouseEvent {
            action,
            button: (action == MouseAction::Press).then_some(MouseButton::Left),
            position: MousePosition { x, y: 10.0 },
            modifiers: Default::default(),
            size: Some(size),
            timestamp_ns: None,
        };

        terminal
            .handle_mouse_gesture(&event(MouseAction::Press, 0.0))
            .expect("start selection drag");
        terminal
            .handle_mouse_gesture(&event(MouseAction::Motion, 60.0))
            .expect("extend selection drag");
        terminal
            .handle_mouse_gesture(&event(MouseAction::Release, 60.0))
            .expect("finish selection drag");

        assert_eq!(
            terminal
                .selection_text()
                .expect("read selected terminal text"),
            Some("select".to_string()),
        );
    }

    #[test]
    fn native_terminal_double_click_selects_word_and_triple_click_selects_line() {
        let mut terminal = NativeTerminal::new(80, 24).expect("create native terminal");
        terminal
            .feed(b"hello world ferryx\r\n")
            .expect("write selectable terminal text");
        let size = MouseRendererSize {
            screen_width: 800,
            screen_height: 480,
            cell_width: 10,
            cell_height: 20,
            padding_top: 0,
            padding_bottom: 0,
            padding_right: 0,
            padding_left: 0,
        };
        let event = |action, x, time_ns| MouseEvent {
            action,
            button: (action == MouseAction::Press).then_some(MouseButton::Left),
            position: MousePosition { x, y: 10.0 },
            modifiers: Default::default(),
            size: Some(size),
            timestamp_ns: Some(time_ns),
        };

        // Click 1 on "world" (x = 75.0, col 7, 'o')
        terminal
            .handle_mouse_gesture(&event(MouseAction::Press, 75.0, 1_000_000_000))
            .expect("first press");
        terminal
            .handle_mouse_gesture(&event(MouseAction::Release, 75.0, 1_050_000_000))
            .expect("first release");
        // Single click without drag should not select any text
        assert_eq!(
            terminal.selection_text().expect("query selection text"),
            None,
        );

        // Click 2 (double click) within 200ms at same location
        terminal
            .handle_mouse_gesture(&event(MouseAction::Press, 75.0, 1_200_000_000))
            .expect("second press (double click)");
        terminal
            .handle_mouse_gesture(&event(MouseAction::Release, 75.0, 1_250_000_000))
            .expect("second release");
        assert_eq!(
            terminal.selection_text().expect("query word selection"),
            Some("world".to_string()),
        );

        // Click 3 (triple click) within 150ms at same location
        terminal
            .handle_mouse_gesture(&event(MouseAction::Press, 75.0, 1_350_000_000))
            .expect("third press (triple click)");
        terminal
            .handle_mouse_gesture(&event(MouseAction::Release, 75.0, 1_400_000_000))
            .expect("third release");
        assert_eq!(
            terminal.selection_text().expect("query line selection"),
            Some("hello world ferryx".to_string()),
        );

        // Click 4 after interval expired (> 500ms, e.g. at 2.5s) resets to single click and clears selection
        terminal
            .handle_mouse_gesture(&event(MouseAction::Press, 75.0, 2_500_000_000))
            .expect("single press after timeout");
        terminal
            .handle_mouse_gesture(&event(MouseAction::Release, 75.0, 2_550_000_000))
            .expect("single release after timeout");
        assert_eq!(
            terminal.selection_text().expect("query cleared selection"),
            None,
        );
    }

    #[test]
    fn native_terminal_double_click_drag_extends_selection_by_word() {
        let mut terminal = NativeTerminal::new(80, 24).expect("create native terminal");
        terminal
            .feed(b"alpha beta gamma delta epsilon\r\n")
            .expect("write selectable terminal text");
        let size = MouseRendererSize {
            screen_width: 800,
            screen_height: 480,
            cell_width: 10,
            cell_height: 20,
            padding_top: 0,
            padding_bottom: 0,
            padding_right: 0,
            padding_left: 0,
        };
        let event = |action, x, time_ns: Option<u64>| MouseEvent {
            action,
            button: (action == MouseAction::Press).then_some(MouseButton::Left),
            position: MousePosition { x, y: 10.0 },
            modifiers: Default::default(),
            size: Some(size),
            timestamp_ns: time_ns,
        };

        // Double click on "beta" (col 7, x = 75.0)
        terminal
            .handle_mouse_gesture(&event(MouseAction::Press, 75.0, Some(1_000_000_000)))
            .expect("press 1");
        terminal
            .handle_mouse_gesture(&event(MouseAction::Release, 75.0, Some(1_050_000_000)))
            .expect("release 1");
        terminal
            .handle_mouse_gesture(&event(MouseAction::Press, 75.0, Some(1_200_000_000)))
            .expect("press 2 (double click)");
        assert_eq!(
            terminal.selection_text().expect("query word selection"),
            Some("beta".to_string()),
        );

        // Drag to "delta" (col 19, x = 195.0) - should extend selection to full words "beta gamma delta"
        terminal
            .handle_mouse_gesture(&event(MouseAction::Motion, 195.0, None))
            .expect("drag motion");
        assert_eq!(
            terminal.selection_text().expect("query word drag selection"),
            Some("beta gamma delta".to_string()),
        );

        terminal
            .handle_mouse_gesture(&event(MouseAction::Release, 195.0, None))
            .expect("release drag");
        assert_eq!(
            terminal.selection_text().expect("query final selection"),
            Some("beta gamma delta".to_string()),
        );
    }

    #[test]
    fn test_selection_with_scrollback() {
        let mut terminal = NativeTerminal::new(80, 24).expect("create native terminal");
        for i in 0..50 {
            terminal.feed(format!("line {}\r\n", i).as_bytes()).unwrap();
        }
        let size = MouseRendererSize {
            screen_width: 800,
            screen_height: 480,
            cell_width: 10,
            cell_height: 20,
            padding_top: 0,
            padding_bottom: 0,
            padding_right: 0,
            padding_left: 0,
        };
        let event = |action, x, y, time_ns| MouseEvent {
            action,
            button: (action == MouseAction::Press).then_some(MouseButton::Left),
            position: MousePosition { x, y },
            modifiers: Default::default(),
            size: Some(size),
            timestamp_ns: Some(time_ns),
        };

        // Double click on row 10 in the visible viewport (which has line 37)
        let y = 10.0 * 20.0 + 10.0;
        terminal.handle_mouse_gesture(&event(MouseAction::Press, 15.0, y, 1_000_000_000)).unwrap();
        terminal.handle_mouse_gesture(&event(MouseAction::Release, 15.0, y, 1_050_000_000)).unwrap();
        terminal.handle_mouse_gesture(&event(MouseAction::Press, 15.0, y, 1_150_000_000)).unwrap();
        terminal.handle_mouse_gesture(&event(MouseAction::Release, 15.0, y, 1_200_000_000)).unwrap();

        assert_eq!(terminal.selection_text().unwrap(), Some("line".to_string()));
        // Viewport selection range must map to row 10 of the visible viewport
        assert_eq!(terminal.selection_range().unwrap(), Some((0, 10, 3, 10)));

        // Scroll to top: line 37 is now below the viewport, so viewport selection range is None
        terminal.scroll_viewport(crate::native_terminal::ScrollViewport::Top).unwrap();
        assert_eq!(terminal.selection_range().unwrap(), None);
        // But the active selection text remains retained
        assert_eq!(terminal.selection_text().unwrap(), Some("line".to_string()));

        // Scroll back to bottom: line 37 is visible at row 10 again
        terminal.scroll_viewport(crate::native_terminal::ScrollViewport::Bottom).unwrap();
        assert_eq!(terminal.selection_range().unwrap(), Some((0, 10, 3, 10)));
    }
}