//! Render state snapshot capture and grid traversal logic.

use std::ffi::c_void;
use std::ptr::NonNull;

use super::cell_extractor::extract_cell_snapshot;
use super::cursor::{CursorSnapshot, CursorVisualStyle};
use super::error::NativeTerminalError;
use super::guards::{RenderStateGuard, RowCellsGuard, RowIteratorGuard};
use super::snapshot::RenderSnapshot;
use super::sys::ffi::{
    ghostty_render_state_get, ghostty_render_state_new, ghostty_render_state_row_cells_new,
    ghostty_render_state_row_cells_next, ghostty_render_state_row_get,
    ghostty_render_state_row_iterator_new, ghostty_render_state_row_iterator_next,
    ghostty_render_state_update,
};
use super::sys::types::{
    GhosttyRenderState, GhosttyRenderStateCursor, GhosttyRenderStateRowCells,
    GhosttyRenderStateRowIterator, GhosttyTerminal, GHOSTTY_RENDER_STATE_DATA_COLS,
    GHOSTTY_RENDER_STATE_DATA_CURSOR, GHOSTTY_RENDER_STATE_DATA_ROWS,
    GHOSTTY_RENDER_STATE_DATA_ROW_ITERATOR, GHOSTTY_RENDER_STATE_ROW_DATA_CELLS,
};

pub fn capture_render_snapshot(
    term_ptr: GhosttyTerminal,
) -> Result<RenderSnapshot, NativeTerminalError> {
    let mut state_raw: GhosttyRenderState = std::ptr::null_mut();

    // SAFETY: Category: Foreign Handle Allocation.
    // Invariant: Null allocator selects libghostty-vt default. &mut state_raw is a valid stack pointer.
    let res = unsafe { ghostty_render_state_new(std::ptr::null(), &mut state_raw) };
    NativeTerminalError::from_c_result(res, "ghostty_render_state_new")?;
    let non_null_state = NonNull::new(state_raw).ok_or_else(|| {
        NativeTerminalError::InvalidValue(
            "ghostty_render_state_new returned null pointer despite success".to_string(),
        )
    })?;
    let state_guard = RenderStateGuard(non_null_state);

    // SAFETY: Category: Foreign State Synchronous Update.
    // Invariant: state_guard.0 and term_ptr are validated non-null handles.
    let res = unsafe { ghostty_render_state_update(state_guard.0.as_ptr(), term_ptr) };
    NativeTerminalError::from_c_result(res, "ghostty_render_state_update")?;

    let mut cols: u16 = 0;
    let mut rows: u16 = 0;
    // SAFETY: Category: Foreign Data Extraction. Output points to stack u16.
    let res_c = unsafe {
        ghostty_render_state_get(
            state_guard.0.as_ptr(),
            GHOSTTY_RENDER_STATE_DATA_COLS,
            &mut cols as *mut _ as *mut c_void,
        )
    };
    NativeTerminalError::from_c_result(res_c, "ghostty_render_state_get(Cols)")?;

    // SAFETY: Category: Foreign Data Extraction. Output points to stack u16.
    let res_r = unsafe {
        ghostty_render_state_get(
            state_guard.0.as_ptr(),
            GHOSTTY_RENDER_STATE_DATA_ROWS,
            &mut rows as *mut _ as *mut c_void,
        )
    };
    NativeTerminalError::from_c_result(res_r, "ghostty_render_state_get(Rows)")?;

    let mut raw_cursor = GhosttyRenderStateCursor::default();
    // SAFETY: Category: Foreign Sized Struct Extraction.
    // Invariant: raw_cursor.size is initialized to sizeof(GhosttyRenderStateCursor).
    let res_cur = unsafe {
        ghostty_render_state_get(
            state_guard.0.as_ptr(),
            GHOSTTY_RENDER_STATE_DATA_CURSOR,
            &mut raw_cursor as *mut _ as *mut c_void,
        )
    };
    NativeTerminalError::from_c_result(res_cur, "ghostty_render_state_get(Cursor)")?;

    let viewport_has_value = NativeTerminalError::decode_c_bool(
        raw_cursor.viewport_has_value,
        "cursor.viewport_has_value",
    )?;
    let visible = NativeTerminalError::decode_c_bool(raw_cursor.visible, "cursor.visible")?;
    let blinking = NativeTerminalError::decode_c_bool(raw_cursor.blinking, "cursor.blinking")?;
    let wide_tail = if viewport_has_value {
        NativeTerminalError::decode_c_bool(raw_cursor.wide_tail, "cursor.wide_tail")?
    } else {
        false
    };

    let cursor = CursorSnapshot {
        x: if viewport_has_value {
            raw_cursor.viewport_x
        } else {
            0
        },
        y: if viewport_has_value {
            raw_cursor.viewport_y
        } else {
            0
        },
        wide_tail,
        visible,
        blinking,
        visual_style: CursorVisualStyle::try_from(raw_cursor.visual_style)?,
    };

    let mut row_iter_raw: GhosttyRenderStateRowIterator = std::ptr::null_mut();
    // SAFETY: Category: Foreign Handle Allocation. Null allocator selects default.
    let res = unsafe { ghostty_render_state_row_iterator_new(std::ptr::null(), &mut row_iter_raw) };
    NativeTerminalError::from_c_result(res, "ghostty_render_state_row_iterator_new")?;
    let non_null_iter = NonNull::new(row_iter_raw).ok_or_else(|| {
        NativeTerminalError::InvalidValue(
            "ghostty_render_state_row_iterator_new returned null pointer".to_string(),
        )
    })?;
    let mut row_iter_guard = RowIteratorGuard(non_null_iter);

    let mut row_iter_slot = row_iter_guard.0.as_ptr();
    // SAFETY: Category: Foreign Handle Population Invariant.
    // Invariant: &mut row_iter_slot receives populated row iterator reference.
    let res = unsafe {
        ghostty_render_state_get(
            state_guard.0.as_ptr(),
            GHOSTTY_RENDER_STATE_DATA_ROW_ITERATOR,
            &mut row_iter_slot as *mut _ as *mut c_void,
        )
    };
    NativeTerminalError::from_c_result(res, "ghostty_render_state_get(RowIterator)")?;
    let populated_iter = NonNull::new(row_iter_slot).ok_or_else(|| {
        NativeTerminalError::InvalidValue(
            "ghostty_render_state_get(RowIterator) returned null pointer".to_string(),
        )
    })?;
    row_iter_guard.0 = populated_iter;

    let mut row_cells_raw: GhosttyRenderStateRowCells = std::ptr::null_mut();
    // SAFETY: Category: Foreign Handle Allocation. Null allocator selects default.
    let res = unsafe { ghostty_render_state_row_cells_new(std::ptr::null(), &mut row_cells_raw) };
    NativeTerminalError::from_c_result(res, "ghostty_render_state_row_cells_new")?;
    let non_null_cells = NonNull::new(row_cells_raw).ok_or_else(|| {
        NativeTerminalError::InvalidValue(
            "ghostty_render_state_row_cells_new returned null pointer".to_string(),
        )
    })?;
    let mut row_cells_guard = RowCellsGuard(non_null_cells);

    let mut grid = Vec::with_capacity(rows as usize);

    // SAFETY: Category: Foreign Iterator Traversal Invariant. row_iter_guard.0 is non-null and valid.
    loop {
        let has_next_u8 =
            unsafe { ghostty_render_state_row_iterator_next(row_iter_guard.0.as_ptr()) };
        let has_next = NativeTerminalError::decode_c_bool(has_next_u8, "row_iterator_next")?;
        if !has_next {
            break;
        }

        let mut row_cells_slot = row_cells_guard.0.as_ptr();
        // SAFETY: Category: Foreign Row Data Extraction.
        // Invariant: row_iter_guard.0 and &mut row_cells_slot are valid non-null handle pointers.
        let res = unsafe {
            ghostty_render_state_row_get(
                row_iter_guard.0.as_ptr(),
                GHOSTTY_RENDER_STATE_ROW_DATA_CELLS,
                &mut row_cells_slot as *mut _ as *mut c_void,
            )
        };
        NativeTerminalError::from_c_result(res, "ghostty_render_state_row_get(Cells)")?;
        let populated_cells = NonNull::new(row_cells_slot).ok_or_else(|| {
            NativeTerminalError::InvalidValue(
                "ghostty_render_state_row_get(Cells) returned null pointer".to_string(),
            )
        })?;
        row_cells_guard.0 = populated_cells;

        let mut row_vec = Vec::with_capacity(cols as usize);
        // SAFETY: Category: Foreign Cell Traversal Invariant. row_cells_guard.0 is valid for row.
        loop {
            let cell_next_u8 =
                unsafe { ghostty_render_state_row_cells_next(row_cells_guard.0.as_ptr()) };
            let cell_next = NativeTerminalError::decode_c_bool(cell_next_u8, "row_cells_next")?;
            if !cell_next {
                break;
            }
            row_vec.push(extract_cell_snapshot(row_cells_guard.0.as_ptr())?);
        }
        grid.push(row_vec);
    }

    Ok(RenderSnapshot {
        cols,
        rows,
        cursor,
        grid,
    })
}
