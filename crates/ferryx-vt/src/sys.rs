#![allow(non_camel_case_types, dead_code)]

use std::ffi::{c_int, c_void};

pub type GhosttyResult = c_int;
pub const SUCCESS: GhosttyResult = 0;
pub const OUT_OF_SPACE: GhosttyResult = -3;
pub const NO_VALUE: GhosttyResult = -4;

#[repr(C)]
pub struct TerminalImpl {
    _p: [u8; 0],
}
pub type Terminal = *mut TerminalImpl;

#[repr(C)]
pub struct TrackedImpl {
    _p: [u8; 0],
}
pub type Tracked = *mut TrackedImpl;

#[repr(C)]
pub struct RenderStateImpl {
    _p: [u8; 0],
}
pub type RenderState = *mut RenderStateImpl;

pub type Cell = u64;
pub type Row = u64;

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct GString {
    pub ptr: *const u8,
    pub len: usize,
}

impl GString {
    pub fn bytes(&self) -> &[u8] {
        if self.ptr.is_null() || self.len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
        }
    }
}

impl Default for GString {
    fn default() -> Self {
        GString { ptr: std::ptr::null(), len: 0 }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct PointCoordinate {
    pub x: u16,
    pub y: u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub union PointValue {
    pub coordinate: PointCoordinate,
    pub _padding: [u64; 2],
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Point {
    pub tag: c_int,
    pub value: PointValue,
}

pub const POINT_ACTIVE: c_int = 0;
pub const POINT_HISTORY: c_int = 3;

impl Point {
    pub fn new(tag: c_int, x: u16, y: u32) -> Self {
        Point { tag, value: PointValue { coordinate: PointCoordinate { x, y } } }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct GridRef {
    pub size: usize,
    pub node: *mut c_void,
    pub x: u16,
    pub y: u16,
}

impl Default for GridRef {
    fn default() -> Self {
        GridRef { size: std::mem::size_of::<Self>(), node: std::ptr::null_mut(), x: 0, y: 0 }
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub union StyleColorValue {
    pub palette: u8,
    pub rgb: Rgb,
    pub _padding: u64,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct StyleColor {
    pub tag: c_int,
    pub value: StyleColorValue,
}

pub const STYLE_COLOR_PALETTE: c_int = 1;
pub const STYLE_COLOR_RGB: c_int = 2;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Style {
    pub size: usize,
    pub fg: StyleColor,
    pub bg: StyleColor,
    pub underline_color: StyleColor,
    pub bold: bool,
    pub italic: bool,
    pub faint: bool,
    pub blink: bool,
    pub inverse: bool,
    pub invisible: bool,
    pub strikethrough: bool,
    pub overline: bool,
    pub underline: c_int,
}

impl Default for Style {
    fn default() -> Self {
        let none = StyleColor { tag: 0, value: StyleColorValue { _padding: 0 } };
        Style {
            size: std::mem::size_of::<Self>(),
            fg: none,
            bg: none,
            underline_color: none,
            bold: false,
            italic: false,
            faint: false,
            blink: false,
            inverse: false,
            invisible: false,
            strikethrough: false,
            overline: false,
            underline: 0,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct ModeConfig {
    pub mode: u16,
    pub value: bool,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct SizeReport {
    pub rows: u16,
    pub columns: u16,
    pub cell_width: u32,
    pub cell_height: u32,
}

#[repr(C)]
pub struct DeviceAttributes {
    pub conformance_level: u16,
    pub features: [u16; 64],
    pub num_features: usize,
    pub device_type: u16,
    pub firmware_version: u16,
    pub rom_cartridge: u16,
    pub unit_id: u32,
}

#[repr(C)]
pub struct ClipboardContent {
    pub mime: GString,
    pub data: GString,
}

#[repr(C)]
pub struct ClipboardWrite {
    pub size: usize,
    pub location: c_int,
    pub contents: *const ClipboardContent,
    pub contents_len: usize,
}

#[repr(C)]
pub struct DesktopNotification {
    pub size: usize,
    pub title: GString,
    pub body: GString,
}

pub const CLIPBOARD_STANDARD: c_int = 0;
pub const CLIPBOARD_SELECTION: c_int = 1;
pub const CLIPBOARD_PRIMARY: c_int = 2;
pub const CLIPBOARD_WRITE_SUCCESS: c_int = 0;

pub type WritePtyFn = unsafe extern "C" fn(Terminal, *mut c_void, *const u8, usize);
pub type BellFn = unsafe extern "C" fn(Terminal, *mut c_void);
pub type SizeFn = unsafe extern "C" fn(Terminal, *mut c_void, *mut SizeReport) -> bool;
pub type DeviceAttributesFn = unsafe extern "C" fn(Terminal, *mut c_void, *mut DeviceAttributes) -> bool;
pub type ClipboardWriteFn = unsafe extern "C" fn(Terminal, *mut c_void, *const ClipboardWrite) -> c_int;
pub type NotificationFn = unsafe extern "C" fn(Terminal, *mut c_void, *const DesktopNotification);

pub const OPT_USERDATA: c_int = 0;
pub const OPT_WRITE_PTY: c_int = 1;
pub const OPT_BELL: c_int = 2;
pub const OPT_SIZE: c_int = 6;
pub const OPT_DEVICE_ATTRIBUTES: c_int = 8;
pub const OPT_CLIPBOARD_WRITE: c_int = 26;
pub const OPT_SCROLLBACK_MAX_BYTES: c_int = 27;
pub const OPT_SCROLLBACK_MAX_LINES: c_int = 28;
pub const OPT_DESKTOP_NOTIFICATION: c_int = 29;

pub const DATA_COLS: c_int = 1;
pub const DATA_ROWS: c_int = 2;
pub const DATA_CURSOR_X: c_int = 3;
pub const DATA_CURSOR_Y: c_int = 4;
pub const DATA_ACTIVE_SCREEN: c_int = 6;
pub const DATA_TITLE: c_int = 12;
pub const DATA_SCROLLBACK_ROWS: c_int = 15;
pub const DATA_COLOR_FOREGROUND: c_int = 18;
pub const DATA_COLOR_BACKGROUND: c_int = 19;
pub const DATA_COLOR_CURSOR: c_int = 20;
pub const DATA_COLOR_PALETTE: c_int = 21;
pub const DATA_COLOR_FOREGROUND_DEFAULT: c_int = 22;
pub const DATA_COLOR_BACKGROUND_DEFAULT: c_int = 23;
pub const DATA_COLOR_CURSOR_DEFAULT: c_int = 24;
pub const DATA_COLOR_PALETTE_DEFAULT: c_int = 25;
pub const DATA_MODE: c_int = 37;

pub const RS_CURSOR_VISUAL_STYLE: c_int = 10;
pub const RS_CURSOR_VISIBLE: c_int = 11;
pub const RS_CURSOR_BLINKING: c_int = 12;

pub const CELL_WIDE: c_int = 3;
pub const ROW_WRAP: c_int = 1;

pub const WIDE_NARROW: u32 = 0;
pub const WIDE_WIDE: u32 = 1;
pub const WIDE_SPACER_TAIL: u32 = 2;
pub const WIDE_SPACER_HEAD: u32 = 3;

pub const fn dec_mode(v: u16) -> u16 {
    v & 0x7FFF
}
pub const fn ansi_mode(v: u16) -> u16 {
    (v & 0x7FFF) | 0x8000
}

extern "C" {
    pub fn ghostty_terminal_new(allocator: *const c_void, terminal: *mut Terminal, cols: u16, rows: u16) -> GhosttyResult;
    pub fn ghostty_terminal_free(terminal: Terminal);
    pub fn ghostty_terminal_resize(terminal: Terminal, cols: u16, rows: u16, cw: u32, ch: u32) -> GhosttyResult;
    pub fn ghostty_terminal_set(terminal: Terminal, option: c_int, value: *const c_void) -> GhosttyResult;
    pub fn ghostty_terminal_vt_write(terminal: Terminal, data: *const u8, len: usize);
    pub fn ghostty_terminal_get(terminal: Terminal, data: c_int, out: *mut c_void) -> GhosttyResult;
    pub fn ghostty_terminal_grid_ref(terminal: Terminal, point: Point, out: *mut GridRef) -> GhosttyResult;
    pub fn ghostty_terminal_grid_ref_track(terminal: Terminal, point: Point, out: *mut Tracked) -> GhosttyResult;
    pub fn ghostty_tracked_grid_ref_free(r: Tracked);
    pub fn ghostty_tracked_grid_ref_has_value(r: Tracked) -> bool;
    pub fn ghostty_tracked_grid_ref_point(r: Tracked, tag: c_int, out: *mut PointCoordinate) -> GhosttyResult;
    pub fn ghostty_grid_ref_cell(r: *const GridRef, out: *mut Cell) -> GhosttyResult;
    pub fn ghostty_grid_ref_row(r: *const GridRef, out: *mut Row) -> GhosttyResult;
    pub fn ghostty_grid_ref_graphemes(r: *const GridRef, buf: *mut u32, buf_len: usize, out_len: *mut usize) -> GhosttyResult;
    pub fn ghostty_grid_ref_hyperlink_uri(r: *const GridRef, buf: *mut u8, buf_len: usize, out_len: *mut usize) -> GhosttyResult;
    pub fn ghostty_grid_ref_style(r: *const GridRef, out: *mut Style) -> GhosttyResult;
    pub fn ghostty_cell_get(cell: Cell, data: c_int, out: *mut c_void) -> GhosttyResult;
    pub fn ghostty_row_get(row: Row, data: c_int, out: *mut c_void) -> GhosttyResult;
    pub fn ghostty_render_state_new(allocator: *const c_void, out: *mut RenderState) -> GhosttyResult;
    pub fn ghostty_render_state_free(state: RenderState);
    pub fn ghostty_render_state_update(state: RenderState, terminal: Terminal) -> GhosttyResult;
    pub fn ghostty_render_state_get(state: RenderState, data: c_int, out: *mut c_void) -> GhosttyResult;
}
