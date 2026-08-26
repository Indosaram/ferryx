//! Cell decoding and attribute extraction for render snapshot traversal.

use std::ffi::{c_int, c_void};

use super::color::ColorRgb;
use super::error::NativeTerminalError;
use super::snapshot::{CellSnapshot, CellWide};
use super::sys::ffi::{ghostty_cell_get, ghostty_render_state_row_cells_get};
use super::sys::types::{
    GhosttyBuffer, GhosttyCell, GhosttyColorRgb, GhosttyRenderStateRowCells, GhosttyStyle,
    GHOSTTY_CELL_DATA_WIDE, GHOSTTY_INVALID_VALUE, GHOSTTY_NO_VALUE, GHOSTTY_OUT_OF_SPACE,
    GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_BG_COLOR, GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_FG_COLOR,
    GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_UTF8, GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_RAW,
    GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_STYLE, GHOSTTY_SUCCESS,
};

/// Maximum bounded byte limit for a single grapheme cluster to prevent allocation exhaustion.
const MAX_GRAPHEME_CLUSTER_BYTES: usize = 1024;

fn query_optional_color(
    cells: GhosttyRenderStateRowCells,
    data_kind: c_int,
    ctx: &'static str,
) -> Result<Option<ColorRgb>, NativeTerminalError> {
    let mut raw = GhosttyColorRgb::default();
    // SAFETY: Category: Foreign Output Pointer Validity.
    // Invariant: cells is a valid foreign row cells handle; &mut raw points to initialized stack GhosttyColorRgb.
    let res = unsafe {
        ghostty_render_state_row_cells_get(cells, data_kind, &mut raw as *mut _ as *mut c_void)
    };
    if res == GHOSTTY_SUCCESS {
        Ok(Some(ColorRgb::from(raw)))
    } else if res == GHOSTTY_INVALID_VALUE || res == GHOSTTY_NO_VALUE {
        Ok(None)
    } else {
        NativeTerminalError::from_c_result(res, ctx)?;
        Ok(None)
    }
}

fn decode_underline_mode(val: c_int) -> Result<bool, NativeTerminalError> {
    match val {
        0 => Ok(false),
        1..=5 => Ok(true),
        other => Err(NativeTerminalError::ForeignErrorCode(other)),
    }
}

pub fn extract_cell_snapshot(
    cells: GhosttyRenderStateRowCells,
) -> Result<CellSnapshot, NativeTerminalError> {
    let mut stack_buf = [0u8; 64];
    let mut buf_desc = GhosttyBuffer {
        ptr: stack_buf.as_mut_ptr(),
        cap: stack_buf.len(),
        len: 0,
    };
    // SAFETY: Category: Foreign Buffer Pointer and Capacity Invariant.
    // Invariant: buf_desc.ptr points to 64 contiguous writable stack bytes matching buf_desc.cap.
    let g_res = unsafe {
        ghostty_render_state_row_cells_get(
            cells,
            GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_UTF8,
            &mut buf_desc as *mut _ as *mut c_void,
        )
    };

    let text = if g_res == GHOSTTY_SUCCESS {
        if buf_desc.len == 0 {
            String::new()
        } else {
            if buf_desc.len > stack_buf.len() {
                return Err(NativeTerminalError::InvalidValue(
                    "foreign grapheme UTF-8 length exceeds stack buffer capacity".to_string(),
                ));
            }
            let slice = &stack_buf[..buf_desc.len];
            std::str::from_utf8(slice)
                .map(|s| s.to_string())
                .map_err(|e| NativeTerminalError::InvalidUtf8(e.to_string()))?
        }
    } else if g_res == GHOSTTY_OUT_OF_SPACE {
        if buf_desc.len == 0 || buf_desc.len > MAX_GRAPHEME_CLUSTER_BYTES {
            return Err(NativeTerminalError::LimitExceeded);
        }
        let mut heap_buf = vec![0u8; buf_desc.len];
        let mut heap_desc = GhosttyBuffer {
            ptr: heap_buf.as_mut_ptr(),
            cap: heap_buf.len(),
            len: 0,
        };
        // SAFETY: Category: Foreign Dynamic Buffer Retry Invariant.
        // Invariant: heap_desc.ptr points to heap_buf of bounded capacity buf_desc.len.
        let retry_res = unsafe {
            ghostty_render_state_row_cells_get(
                cells,
                GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_UTF8,
                &mut heap_desc as *mut _ as *mut c_void,
            )
        };
        NativeTerminalError::from_c_result(retry_res, "row_cells_get(GraphemesUtf8 heap)")?;
        if heap_desc.len > heap_buf.len() {
            return Err(NativeTerminalError::InvalidValue(
                "foreign grapheme UTF-8 length exceeds heap buffer capacity".to_string(),
            ));
        }
        std::str::from_utf8(&heap_buf[..heap_desc.len])
            .map(|s| s.to_string())
            .map_err(|e| NativeTerminalError::InvalidUtf8(e.to_string()))?
    } else {
        NativeTerminalError::from_c_result(g_res, "row_cells_get(GraphemesUtf8)")?;
        String::new()
    };

    let mut raw_cell: GhosttyCell = 0;
    // SAFETY: Category: Foreign Data Extraction.
    // Invariant: &mut raw_cell points to stack GhosttyCell (u64).
    let cell_res = unsafe {
        ghostty_render_state_row_cells_get(
            cells,
            GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_RAW,
            &mut raw_cell as *mut _ as *mut c_void,
        )
    };
    NativeTerminalError::from_c_result(cell_res, "row_cells_get(Raw)")?;

    let mut wide_c_int: c_int = 0;
    // SAFETY: Category: Foreign Cell Data Extraction.
    // Invariant: raw_cell passed by value; &mut wide_c_int points to stack c_int.
    let wide_res = unsafe {
        ghostty_cell_get(
            raw_cell,
            GHOSTTY_CELL_DATA_WIDE,
            &mut wide_c_int as *mut _ as *mut c_void,
        )
    };
    NativeTerminalError::from_c_result(wide_res, "ghostty_cell_get(Wide)")?;
    let wide = CellWide::try_from(wide_c_int)?;

    let fg = query_optional_color(
        cells,
        GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_FG_COLOR,
        "row_cells_get(FgColor)",
    )?;
    let bg = query_optional_color(
        cells,
        GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_BG_COLOR,
        "row_cells_get(BgColor)",
    )?;

    let mut style = GhosttyStyle::default();
    // SAFETY: Category: Foreign Sized Struct Extraction.
    // Invariant: style.size is initialized to sizeof(GhosttyStyle); fields received as raw integers.
    let style_res = unsafe {
        ghostty_render_state_row_cells_get(
            cells,
            GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_STYLE,
            &mut style as *mut _ as *mut c_void,
        )
    };
    NativeTerminalError::from_c_result(style_res, "row_cells_get(Style)")?;

    let bold = NativeTerminalError::decode_c_bool(style.bold, "style.bold")?;
    let italic = NativeTerminalError::decode_c_bool(style.italic, "style.italic")?;
    let inverse = NativeTerminalError::decode_c_bool(style.inverse, "style.inverse")?;
    let underline = decode_underline_mode(style.underline)?;

    Ok(CellSnapshot {
        text,
        wide,
        fg,
        bg,
        bold,
        italic,
        underline,
        inverse,
    })
}
