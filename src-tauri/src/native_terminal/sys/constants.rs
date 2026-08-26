//! C ABI integer constants matching Ghostty C headers.

use std::ffi::c_int;

// GhosttyResult raw C integer constants matching `ghostty/vt/types.h`.
pub const GHOSTTY_SUCCESS: c_int = 0;
pub const GHOSTTY_OUT_OF_MEMORY: c_int = -1;
pub const GHOSTTY_INVALID_VALUE: c_int = -2;
pub const GHOSTTY_OUT_OF_SPACE: c_int = -3;
pub const GHOSTTY_NO_VALUE: c_int = -4;
pub const GHOSTTY_IO_ERROR: c_int = -5;
pub const GHOSTTY_LIMIT_EXCEEDED: c_int = -6;
pub const GHOSTTY_REJECTED: c_int = -7;

// GhosttyTerminalData C integer constants matching `ghostty/vt/terminal.h`.
pub const GHOSTTY_TERMINAL_DATA_COLS: c_int = 1;
pub const GHOSTTY_TERMINAL_DATA_ROWS: c_int = 2;
pub const GHOSTTY_TERMINAL_DATA_CURSOR_X: c_int = 3;
pub const GHOSTTY_TERMINAL_DATA_CURSOR_Y: c_int = 4;
pub const GHOSTTY_TERMINAL_DATA_CURSOR_PENDING_WRAP: c_int = 5;
pub const GHOSTTY_TERMINAL_DATA_CURSOR_VISIBLE: c_int = 7;
pub const GHOSTTY_TERMINAL_DATA_SCROLLBAR: c_int = 9;
pub const GHOSTTY_TERMINAL_DATA_MOUSE_TRACKING: c_int = 11;
pub const GHOSTTY_TERMINAL_DATA_TITLE: c_int = 12;
pub const GHOSTTY_TERMINAL_DATA_TOTAL_ROWS: c_int = 14;
pub const GHOSTTY_TERMINAL_DATA_SCROLLBACK_ROWS: c_int = 15;
pub const GHOSTTY_TERMINAL_DATA_COLOR_FOREGROUND: c_int = 18;
pub const GHOSTTY_TERMINAL_DATA_COLOR_BACKGROUND: c_int = 19;
pub const GHOSTTY_TERMINAL_DATA_COLOR_CURSOR: c_int = 20;
pub const GHOSTTY_TERMINAL_DATA_COLOR_PALETTE: c_int = 21;
pub const GHOSTTY_TERMINAL_DATA_SELECTION: c_int = 31;
pub const GHOSTTY_TERMINAL_DATA_MODE: c_int = 37;

// GhosttyTerminalOption C integer constants matching `ghostty/vt/terminal.h`.
pub const GHOSTTY_TERMINAL_OPT_USERDATA: c_int = 0;
pub const GHOSTTY_TERMINAL_OPT_BELL: c_int = 2;
pub const GHOSTTY_TERMINAL_OPT_TITLE_CHANGED: c_int = 5;
pub const GHOSTTY_TERMINAL_OPT_TITLE: c_int = 9;
pub const GHOSTTY_TERMINAL_OPT_COLOR_FOREGROUND: c_int = 11;
pub const GHOSTTY_TERMINAL_OPT_COLOR_BACKGROUND: c_int = 12;
pub const GHOSTTY_TERMINAL_OPT_COLOR_CURSOR: c_int = 13;
pub const GHOSTTY_TERMINAL_OPT_COLOR_PALETTE: c_int = 14;
pub const GHOSTTY_TERMINAL_OPT_SELECTION: c_int = 21;

// GhosttyTerminalScrollViewportTag matching `ghostty/vt/terminal.h`.
pub const GHOSTTY_SCROLL_VIEWPORT_TOP: c_int = 0;
pub const GHOSTTY_SCROLL_VIEWPORT_BOTTOM: c_int = 1;
pub const GHOSTTY_SCROLL_VIEWPORT_DELTA: c_int = 2;
pub const GHOSTTY_SCROLL_VIEWPORT_ROW: c_int = 3;

// GhosttyPointTag matching `ghostty/vt/point.h`.
pub const GHOSTTY_POINT_TAG_VIEWPORT: c_int = 1;
pub const GHOSTTY_POINT_TAG_SCREEN: c_int = 2;

// GhosttySelectionOrder and GhosttyFormatterFormat matching selection/types headers.
pub const GHOSTTY_SELECTION_ORDER_FORWARD: c_int = 0;
pub const GHOSTTY_FORMATTER_FORMAT_PLAIN: c_int = 0;

// Packed DEC private mode 2004 matching `GHOSTTY_MODE_BRACKETED_PASTE`.
pub const GHOSTTY_MODE_BRACKETED_PASTE: u16 = 2004;

// GhosttyRenderStateData C integer constants matching `ghostty/vt/render.h`.
pub const GHOSTTY_RENDER_STATE_DATA_COLS: c_int = 1;
pub const GHOSTTY_RENDER_STATE_DATA_ROWS: c_int = 2;
pub const GHOSTTY_RENDER_STATE_DATA_ROW_ITERATOR: c_int = 4;
pub const GHOSTTY_RENDER_STATE_DATA_CURSOR: c_int = 18;

// GhosttyRenderStateRowData C integer constants matching `ghostty/vt/render.h`.
pub const GHOSTTY_RENDER_STATE_ROW_DATA_CELLS: c_int = 3;

// GhosttyRenderStateRowCellsData C integer constants matching `ghostty/vt/render.h`.
pub const GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_RAW: c_int = 1;
pub const GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_STYLE: c_int = 2;
pub const GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_BG_COLOR: c_int = 5;
pub const GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_FG_COLOR: c_int = 6;
pub const GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_UTF8: c_int = 9;

// GhosttyCellData C integer constants matching `ghostty/vt/screen.h`.
pub const GHOSTTY_CELL_DATA_WIDE: c_int = 3;

// GhosttyMouseEncoderOption matching `ghostty/vt/mouse/encoder.h`.
pub const GHOSTTY_MOUSE_ENCODER_OPT_SIZE: c_int = 2;

// GhosttyKey physical key code constants matching `ghostty/vt/key/event.h`.
pub const GHOSTTY_KEY_UNIDENTIFIED: c_int = 0;
pub const GHOSTTY_KEY_DIGIT_0: c_int = 6;
pub const GHOSTTY_KEY_A: c_int = 20;
pub const GHOSTTY_KEY_BACKSPACE: c_int = 53;
pub const GHOSTTY_KEY_ENTER: c_int = 58;
pub const GHOSTTY_KEY_SPACE: c_int = 63;
pub const GHOSTTY_KEY_TAB: c_int = 64;
pub const GHOSTTY_KEY_DELETE: c_int = 68;
pub const GHOSTTY_KEY_END: c_int = 69;
pub const GHOSTTY_KEY_HOME: c_int = 71;
pub const GHOSTTY_KEY_INSERT: c_int = 72;
pub const GHOSTTY_KEY_PAGE_DOWN: c_int = 73;
pub const GHOSTTY_KEY_PAGE_UP: c_int = 74;
pub const GHOSTTY_KEY_ARROW_DOWN: c_int = 75;
pub const GHOSTTY_KEY_ARROW_LEFT: c_int = 76;
pub const GHOSTTY_KEY_ARROW_RIGHT: c_int = 77;
pub const GHOSTTY_KEY_ARROW_UP: c_int = 78;
pub const GHOSTTY_KEY_ESCAPE: c_int = 120;
pub const GHOSTTY_KEY_F1: c_int = 121;
pub const GHOSTTY_KEY_F2: c_int = 122;
pub const GHOSTTY_KEY_F3: c_int = 123;
pub const GHOSTTY_KEY_F4: c_int = 124;
pub const GHOSTTY_KEY_F5: c_int = 125;
pub const GHOSTTY_KEY_F6: c_int = 126;
pub const GHOSTTY_KEY_F7: c_int = 127;
pub const GHOSTTY_KEY_F8: c_int = 128;
pub const GHOSTTY_KEY_F9: c_int = 129;
pub const GHOSTTY_KEY_F10: c_int = 130;
pub const GHOSTTY_KEY_F11: c_int = 131;
pub const GHOSTTY_KEY_F12: c_int = 132;

// GhosttyKeyEncoderOption / GhosttyOptionAsAlt constants matching `ghostty/vt/key/encoder.h`.
pub const GHOSTTY_KEY_ENCODER_OPT_MACOS_OPTION_AS_ALT: c_int = 6;
pub const GHOSTTY_OPTION_AS_ALT_FALSE: c_int = 0;
pub const GHOSTTY_OPTION_AS_ALT_TRUE: c_int = 1;
