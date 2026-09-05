//! C ABI types and structs for libghostty-vt.

use std::ffi::{c_int, c_void};

pub use super::constants::*;

/// Opaque foreign terminal handle.
#[repr(C)]
pub struct GhosttyTerminalImpl {
    _private: [u8; 0],
}
pub type GhosttyTerminal = *mut GhosttyTerminalImpl;

/// Opaque foreign render state handle.
#[repr(C)]
pub struct GhosttyRenderStateImpl {
    _private: [u8; 0],
}
pub type GhosttyRenderState = *mut GhosttyRenderStateImpl;

/// Opaque foreign render-state row iterator handle.
#[repr(C)]
pub struct GhosttyRenderStateRowIteratorImpl {
    _private: [u8; 0],
}
pub type GhosttyRenderStateRowIterator = *mut GhosttyRenderStateRowIteratorImpl;

/// Opaque foreign render-state row cells handle.
#[repr(C)]
pub struct GhosttyRenderStateRowCellsImpl {
    _private: [u8; 0],
}
pub type GhosttyRenderStateRowCells = *mut GhosttyRenderStateRowCellsImpl;

/// Opaque foreign key event handle.
#[repr(C)]
pub struct GhosttyKeyEventImpl {
    _private: [u8; 0],
}
pub type GhosttyKeyEvent = *mut GhosttyKeyEventImpl;

/// Opaque foreign key encoder handle.
#[repr(C)]
pub struct GhosttyKeyEncoderImpl {
    _private: [u8; 0],
}
pub type GhosttyKeyEncoder = *mut GhosttyKeyEncoderImpl;

/// Opaque foreign mouse event handle.
#[repr(C)]
pub struct GhosttyMouseEventImpl {
    _private: [u8; 0],
}
pub type GhosttyMouseEvent = *mut GhosttyMouseEventImpl;

/// Opaque foreign mouse encoder handle.
#[repr(C)]
pub struct GhosttyMouseEncoderImpl {
    _private: [u8; 0],
}
pub type GhosttyMouseEncoder = *mut GhosttyMouseEncoderImpl;

/// Opaque foreign selection gesture handle.
#[repr(C)]
pub struct GhosttySelectionGestureImpl {
    _private: [u8; 0],
}
pub type GhosttySelectionGesture = *mut GhosttySelectionGestureImpl;

/// Opaque foreign selection gesture event handle.
#[repr(C)]
pub struct GhosttySelectionGestureEventImpl {
    _private: [u8; 0],
}
pub type GhosttySelectionGestureEvent = *mut GhosttySelectionGestureEventImpl;

/// Allocator interface for custom allocation strategies.
#[repr(C)]
pub struct GhosttyAllocator {
    pub ctx: *mut c_void,
    pub vtable: *const c_void,
}

/// Borrowed byte string representation matching `include/ghostty/vt/types.h`.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct GhosttyString {
    pub ptr: *const u8,
    pub len: usize,
}

/// Caller-provided byte buffer matching `include/ghostty/vt/types.h`.
#[repr(C)]
#[derive(Debug)]
pub struct GhosttyBuffer {
    pub ptr: *mut u8,
    pub cap: usize,
    pub len: usize,
}

/// RGB color structure matching `include/ghostty/vt/color.h`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct GhosttyColorRgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// Scrollbar state matching `include/ghostty/vt/terminal.h`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct GhosttyTerminalScrollbar {
    pub total: u64,
    pub offset: u64,
    pub len: u64,
}

/// Value union for viewport scroll behavior.
#[repr(C)]
#[derive(Copy, Clone)]
pub union GhosttyTerminalScrollViewportValue {
    pub delta: isize,
    pub row: usize,
    pub _padding: [u64; 2],
}

/// Tagged viewport scroll behavior matching `include/ghostty/vt/terminal.h`.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct GhosttyTerminalScrollViewport {
    pub tag: c_int,
    pub value: GhosttyTerminalScrollViewportValue,
}

/// Point coordinate matching `include/ghostty/vt/point.h`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct GhosttyPointCoordinate {
    pub x: u16,
    pub y: u32,
}

/// Point value union matching `include/ghostty/vt/point.h`.
#[repr(C)]
#[derive(Copy, Clone)]
pub union GhosttyPointValue {
    pub coordinate: GhosttyPointCoordinate,
    pub _padding: [u64; 2],
}

/// Tagged terminal point matching `include/ghostty/vt/point.h`.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct GhosttyPoint {
    pub tag: c_int,
    pub value: GhosttyPointValue,
}

/// Untracked grid reference matching `include/ghostty/vt/grid_ref.h`.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct GhosttyGridRef {
    pub size: usize,
    pub node: *mut c_void,
    pub x: u16,
    pub y: u16,
}

impl Default for GhosttyGridRef {
    fn default() -> Self {
        Self {
            size: std::mem::size_of::<Self>(),
            node: std::ptr::null_mut(),
            x: 0,
            y: 0,
        }
    }
}

/// Snapshot selection matching `include/ghostty/vt/selection.h`.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct GhosttySelection {
    pub size: usize,
    pub start: GhosttyGridRef,
    pub end: GhosttyGridRef,
    pub rectangle: bool,
}

impl Default for GhosttySelection {
    fn default() -> Self {
        Self {
            size: std::mem::size_of::<Self>(),
            start: GhosttyGridRef::default(),
            end: GhosttyGridRef::default(),
            rectangle: false,
        }
    }
}

/// Word-selection options matching `include/ghostty/vt/selection.h`.
#[repr(C)]
pub struct GhosttyTerminalSelectWordOptions {
    pub size: usize,
    pub grid_ref: GhosttyGridRef,
    pub boundary_codepoints: *const u32,
    pub boundary_codepoints_len: usize,
}

/// Line-selection options matching `include/ghostty/vt/selection.h`.
#[repr(C)]
pub struct GhosttyTerminalSelectLineOptions {
    pub size: usize,
    pub grid_ref: GhosttyGridRef,
    pub whitespace: *const u32,
    pub whitespace_len: usize,
    pub semantic_prompt_boundary: bool,
}

/// One-shot selection formatting options matching `include/ghostty/vt/selection.h`.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct GhosttyTerminalSelectionFormatOptions {
    pub size: usize,
    pub emit: c_int,
    pub unwrap: bool,
    pub trim: bool,
    pub selection: *const GhosttySelection,
}

/// Mode query configuration matching `include/ghostty/vt/terminal.h`.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct GhosttyTerminalModeConfig {
    pub mode: u16,
    pub value: bool,
}

/// Style color value union matching `include/ghostty/vt/style.h`.
#[repr(C)]
#[derive(Copy, Clone)]
pub union GhosttyStyleColorValue {
    pub palette: u8,
    pub rgb: GhosttyColorRgb,
    pub _padding: u64,
}

/// Tagged union for style colors matching `include/ghostty/vt/style.h`.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct GhosttyStyleColor {
    pub tag: c_int,
    pub value: GhosttyStyleColorValue,
}

/// Cell style structure matching `include/ghostty/vt/style.h`.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct GhosttyStyle {
    pub size: usize,
    pub fg_color: GhosttyStyleColor,
    pub bg_color: GhosttyStyleColor,
    pub underline_color: GhosttyStyleColor,
    pub bold: u8,
    pub italic: u8,
    pub faint: u8,
    pub blink: u8,
    pub inverse: u8,
    pub invisible: u8,
    pub strikethrough: u8,
    pub overline: u8,
    pub underline: c_int,
}

impl Default for GhosttyStyle {
    fn default() -> Self {
        Self {
            size: std::mem::size_of::<Self>(),
            fg_color: GhosttyStyleColor {
                tag: 0,
                value: GhosttyStyleColorValue { _padding: 0 },
            },
            bg_color: GhosttyStyleColor {
                tag: 0,
                value: GhosttyStyleColorValue { _padding: 0 },
            },
            underline_color: GhosttyStyleColor {
                tag: 0,
                value: GhosttyStyleColorValue { _padding: 0 },
            },
            bold: 0,
            italic: 0,
            faint: 0,
            blink: 0,
            inverse: 0,
            invisible: 0,
            strikethrough: 0,
            overline: 0,
            underline: 0,
        }
    }
}

/// Packed cell representation matching `include/ghostty/vt/screen.h`.
pub type GhosttyCell = u64;

/// Render-state cursor information matching `include/ghostty/vt/render.h`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct GhosttyRenderStateCursor {
    pub size: usize,
    pub viewport_has_value: u8,
    pub viewport_x: u16,
    pub viewport_y: u16,
    pub wide_tail: u8,
    pub visible: u8,
    pub blinking: u8,
    pub password_input: u8,
    pub visual_style: c_int,
}

impl Default for GhosttyRenderStateCursor {
    fn default() -> Self {
        Self {
            size: std::mem::size_of::<Self>(),
            viewport_has_value: 0,
            viewport_x: 0,
            viewport_y: 0,
            wide_tail: 0,
            visible: 1,
            blinking: 0,
            password_input: 0,
            visual_style: 1, // Block
        }
    }
}

/// Mouse position in surface-space pixels matching `ghostty/vt/mouse/event.h`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct GhosttyMousePosition {
    pub x: f32,
    pub y: f32,
}

/// Surface-space position in pixels matching `include/ghostty/vt/types.h`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct GhosttySurfacePosition {
    pub x: f64,
    pub y: f64,
}

/// Selection gesture display geometry matching `include/ghostty/vt/selection.h`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct GhosttySelectionGestureGeometry {
    pub columns: u32,
    pub cell_width: u32,
    pub padding_left: u32,
    pub screen_height: u32,
}

// GhosttySelectionGestureEventType constants matching `ghostty/vt/selection.h`.
pub const GHOSTTY_SELECTION_GESTURE_EVENT_TYPE_PRESS: c_int = 0;
pub const GHOSTTY_SELECTION_GESTURE_EVENT_TYPE_RELEASE: c_int = 1;
pub const GHOSTTY_SELECTION_GESTURE_EVENT_TYPE_DRAG: c_int = 2;

// GhosttySelectionGestureEventOption constants matching `ghostty/vt/selection.h`.
pub const GHOSTTY_SELECTION_GESTURE_EVENT_OPT_REF: c_int = 0;
pub const GHOSTTY_SELECTION_GESTURE_EVENT_OPT_POSITION: c_int = 1;
pub const GHOSTTY_SELECTION_GESTURE_EVENT_OPT_REPEAT_DISTANCE: c_int = 2;
pub const GHOSTTY_SELECTION_GESTURE_EVENT_OPT_TIME_NS: c_int = 3;
pub const GHOSTTY_SELECTION_GESTURE_EVENT_OPT_REPEAT_INTERVAL_NS: c_int = 4;
pub const GHOSTTY_SELECTION_GESTURE_EVENT_OPT_GEOMETRY: c_int = 8;

/// Mouse encoder geometry matching `ghostty/vt/mouse/encoder.h`.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct GhosttyMouseEncoderSize {
    pub size: usize,
    pub screen_width: u32,
    pub screen_height: u32,
    pub cell_width: u32,
    pub cell_height: u32,
    pub padding_top: u32,
    pub padding_bottom: u32,
    pub padding_right: u32,
    pub padding_left: u32,
}
