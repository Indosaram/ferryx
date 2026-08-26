//! Terminal property and state query helpers.

use std::ffi::c_void;
use std::ptr::NonNull;

use super::color::ColorRgb;
use super::cursor::CursorState;
use super::error::NativeTerminalError;
use super::sys::ffi::ghostty_terminal_get;
use super::sys::types::{
    GhosttyColorRgb, GhosttyString, GhosttyTerminalImpl, GHOSTTY_TERMINAL_DATA_COLOR_BACKGROUND,
    GHOSTTY_TERMINAL_DATA_COLOR_CURSOR, GHOSTTY_TERMINAL_DATA_COLOR_FOREGROUND,
    GHOSTTY_TERMINAL_DATA_COLOR_PALETTE, GHOSTTY_TERMINAL_DATA_COLS,
    GHOSTTY_TERMINAL_DATA_CURSOR_PENDING_WRAP, GHOSTTY_TERMINAL_DATA_CURSOR_VISIBLE,
    GHOSTTY_TERMINAL_DATA_CURSOR_X, GHOSTTY_TERMINAL_DATA_CURSOR_Y,
    GHOSTTY_TERMINAL_DATA_MOUSE_TRACKING, GHOSTTY_TERMINAL_DATA_ROWS,
    GHOSTTY_TERMINAL_DATA_SCROLLBACK_ROWS, GHOSTTY_TERMINAL_DATA_TITLE,
    GHOSTTY_TERMINAL_DATA_TOTAL_ROWS,
};

pub fn query_cols(handle: NonNull<GhosttyTerminalImpl>) -> Result<u16, NativeTerminalError> {
    let mut cols: u16 = 0;
    // SAFETY: Category: Foreign Data Extraction. Output points to stack u16.
    let res = unsafe {
        ghostty_terminal_get(
            handle.as_ptr(),
            GHOSTTY_TERMINAL_DATA_COLS,
            &mut cols as *mut u16 as *mut c_void,
        )
    };
    NativeTerminalError::from_c_result(res, "ghostty_terminal_get(Cols)")?;
    Ok(cols)
}

pub fn query_rows(handle: NonNull<GhosttyTerminalImpl>) -> Result<u16, NativeTerminalError> {
    let mut rows: u16 = 0;
    // SAFETY: Category: Foreign Data Extraction. Output points to stack u16.
    let res = unsafe {
        ghostty_terminal_get(
            handle.as_ptr(),
            GHOSTTY_TERMINAL_DATA_ROWS,
            &mut rows as *mut u16 as *mut c_void,
        )
    };
    NativeTerminalError::from_c_result(res, "ghostty_terminal_get(Rows)")?;
    Ok(rows)
}

fn query_usize(
    handle: NonNull<GhosttyTerminalImpl>,
    data: i32,
    context: &'static str,
) -> Result<usize, NativeTerminalError> {
    let mut value = 0usize;
    // SAFETY: Category: Foreign Data Extraction. Output points to stack usize.
    let result = unsafe {
        ghostty_terminal_get(
            handle.as_ptr(),
            data,
            &mut value as *mut usize as *mut c_void,
        )
    };
    NativeTerminalError::from_c_result(result, context)?;
    Ok(value)
}

pub fn query_total_rows(
    handle: NonNull<GhosttyTerminalImpl>,
) -> Result<usize, NativeTerminalError> {
    query_usize(
        handle,
        GHOSTTY_TERMINAL_DATA_TOTAL_ROWS,
        "ghostty_terminal_get(TotalRows)",
    )
}

pub fn query_scrollback_rows(
    handle: NonNull<GhosttyTerminalImpl>,
) -> Result<usize, NativeTerminalError> {
    query_usize(
        handle,
        GHOSTTY_TERMINAL_DATA_SCROLLBACK_ROWS,
        "ghostty_terminal_get(ScrollbackRows)",
    )
}

pub fn query_mouse_tracking_enabled(
    handle: NonNull<GhosttyTerminalImpl>,
) -> Result<bool, NativeTerminalError> {
    let mut value = 0u8;
    // SAFETY: Category: Foreign Byte Extraction. Output points to stack u8.
    let result = unsafe {
        ghostty_terminal_get(
            handle.as_ptr(),
            GHOSTTY_TERMINAL_DATA_MOUSE_TRACKING,
            &mut value as *mut u8 as *mut c_void,
        )
    };
    NativeTerminalError::from_c_result(result, "ghostty_terminal_get(MouseTracking)")?;
    NativeTerminalError::decode_c_bool(value, "MouseTracking")
}

fn query_color(
    handle: NonNull<GhosttyTerminalImpl>,
    data: i32,
    context: &'static str,
) -> Result<ColorRgb, NativeTerminalError> {
    let mut color = GhosttyColorRgb::default();
    // SAFETY: Category: Foreign Data Extraction. Output points to stack GhosttyColorRgb.
    let result = unsafe {
        ghostty_terminal_get(
            handle.as_ptr(),
            data,
            &mut color as *mut GhosttyColorRgb as *mut c_void,
        )
    };
    NativeTerminalError::from_c_result(result, context)?;
    Ok(color.into())
}

pub fn query_default_foreground(
    handle: NonNull<GhosttyTerminalImpl>,
) -> Result<ColorRgb, NativeTerminalError> {
    query_color(
        handle,
        GHOSTTY_TERMINAL_DATA_COLOR_FOREGROUND,
        "ghostty_terminal_get(ColorForeground)",
    )
}

pub fn query_default_background(
    handle: NonNull<GhosttyTerminalImpl>,
) -> Result<ColorRgb, NativeTerminalError> {
    query_color(
        handle,
        GHOSTTY_TERMINAL_DATA_COLOR_BACKGROUND,
        "ghostty_terminal_get(ColorBackground)",
    )
}

pub fn query_default_cursor_color(
    handle: NonNull<GhosttyTerminalImpl>,
) -> Result<ColorRgb, NativeTerminalError> {
    query_color(
        handle,
        GHOSTTY_TERMINAL_DATA_COLOR_CURSOR,
        "ghostty_terminal_get(ColorCursor)",
    )
}

pub fn query_palette(
    handle: NonNull<GhosttyTerminalImpl>,
) -> Result<[ColorRgb; 256], NativeTerminalError> {
    let mut palette = [GhosttyColorRgb::default(); 256];
    // SAFETY: Category: Foreign Array Extraction.
    // Invariant: palette points to writable contiguous storage for exactly 256 GhosttyColorRgb values.
    let result = unsafe {
        ghostty_terminal_get(
            handle.as_ptr(),
            GHOSTTY_TERMINAL_DATA_COLOR_PALETTE,
            palette.as_mut_ptr() as *mut c_void,
        )
    };
    NativeTerminalError::from_c_result(result, "ghostty_terminal_get(ColorPalette)")?;
    Ok(palette.map(ColorRgb::from))
}

pub fn query_title(handle: NonNull<GhosttyTerminalImpl>) -> Result<String, NativeTerminalError> {
    let mut g_str = GhosttyString {
        ptr: std::ptr::null(),
        len: 0,
    };
    // SAFETY: Category: Foreign String Extraction.
    // Invariant: &mut g_str points to stack GhosttyString; returned ptr is valid for len bytes.
    let res = unsafe {
        ghostty_terminal_get(
            handle.as_ptr(),
            GHOSTTY_TERMINAL_DATA_TITLE,
            &mut g_str as *mut GhosttyString as *mut c_void,
        )
    };
    NativeTerminalError::from_c_result(res, "ghostty_terminal_get(Title)")?;

    if g_str.ptr.is_null() || g_str.len == 0 {
        return Ok(String::new());
    }

    // SAFETY: Category: Foreign String Extraction.
    // Invariant: successful query returned non-null g_str.ptr valid for exactly g_str.len bytes.
    let slice = unsafe { std::slice::from_raw_parts(g_str.ptr, g_str.len) };
    std::str::from_utf8(slice)
        .map(|s| s.to_string())
        .map_err(|e| NativeTerminalError::InvalidUtf8(e.to_string()))
}

pub fn query_cursor_position(
    handle: NonNull<GhosttyTerminalImpl>,
) -> Result<(u16, u16), NativeTerminalError> {
    let mut x: u16 = 0;
    let mut y: u16 = 0;
    // SAFETY: Category: Foreign Data Extraction. Output points to stack u16.
    let res_x = unsafe {
        ghostty_terminal_get(
            handle.as_ptr(),
            GHOSTTY_TERMINAL_DATA_CURSOR_X,
            &mut x as *mut u16 as *mut c_void,
        )
    };
    NativeTerminalError::from_c_result(res_x, "ghostty_terminal_get(CursorX)")?;

    // SAFETY: Category: Foreign Data Extraction. Output points to stack u16.
    let res_y = unsafe {
        ghostty_terminal_get(
            handle.as_ptr(),
            GHOSTTY_TERMINAL_DATA_CURSOR_Y,
            &mut y as *mut u16 as *mut c_void,
        )
    };
    NativeTerminalError::from_c_result(res_y, "ghostty_terminal_get(CursorY)")?;

    Ok((x, y))
}

pub fn query_cursor_state(
    handle: NonNull<GhosttyTerminalImpl>,
) -> Result<CursorState, NativeTerminalError> {
    let (x, y) = query_cursor_position(handle)?;

    let mut visible_u8: u8 = 1;
    // SAFETY: Category: Foreign Byte Extraction. Output points to stack u8.
    let res_vis = unsafe {
        ghostty_terminal_get(
            handle.as_ptr(),
            GHOSTTY_TERMINAL_DATA_CURSOR_VISIBLE,
            &mut visible_u8 as *mut u8 as *mut c_void,
        )
    };
    NativeTerminalError::from_c_result(res_vis, "ghostty_terminal_get(CursorVisible)")?;
    let visible = NativeTerminalError::decode_c_bool(visible_u8, "CursorVisible")?;

    let mut pending_wrap_u8: u8 = 0;
    // SAFETY: Category: Foreign Byte Extraction. Output points to stack u8.
    let res_wrap = unsafe {
        ghostty_terminal_get(
            handle.as_ptr(),
            GHOSTTY_TERMINAL_DATA_CURSOR_PENDING_WRAP,
            &mut pending_wrap_u8 as *mut u8 as *mut c_void,
        )
    };
    NativeTerminalError::from_c_result(res_wrap, "ghostty_terminal_get(CursorPendingWrap)")?;
    let pending_wrap = NativeTerminalError::decode_c_bool(pending_wrap_u8, "CursorPendingWrap")?;

    Ok(CursorState {
        x,
        y,
        visible,
        pending_wrap,
    })
}
