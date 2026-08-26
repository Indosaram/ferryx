//! Safe terminal selection helpers.

use std::ffi::c_void;
use std::ptr::NonNull;

use super::error::NativeTerminalError;
use super::sys::ffi::{
    ghostty_free, ghostty_terminal_get, ghostty_terminal_grid_ref,
    ghostty_terminal_point_from_grid_ref, ghostty_terminal_select_all,
    ghostty_terminal_select_line, ghostty_terminal_select_word,
    ghostty_terminal_selection_format_alloc, ghostty_terminal_selection_ordered,
    ghostty_terminal_set,
};
use super::sys::types::{
    GhosttyGridRef, GhosttyPoint, GhosttyPointCoordinate, GhosttyPointValue, GhosttySelection,
    GhosttyTerminalImpl, GhosttyTerminalSelectLineOptions, GhosttyTerminalSelectWordOptions,
    GhosttyTerminalSelectionFormatOptions, GHOSTTY_FORMATTER_FORMAT_PLAIN, GHOSTTY_NO_VALUE,
    GHOSTTY_POINT_TAG_SCREEN, GHOSTTY_POINT_TAG_VIEWPORT, GHOSTTY_SELECTION_ORDER_FORWARD,
    GHOSTTY_TERMINAL_DATA_SELECTION, GHOSTTY_TERMINAL_OPT_SELECTION,
};

fn viewport_ref(
    handle: NonNull<GhosttyTerminalImpl>,
    col: u16,
    row: u16,
) -> Result<GhosttyGridRef, NativeTerminalError> {
    let point = GhosttyPoint {
        tag: GHOSTTY_POINT_TAG_VIEWPORT,
        value: GhosttyPointValue {
            coordinate: GhosttyPointCoordinate {
                x: col,
                y: u32::from(row),
            },
        },
    };
    let mut grid_ref = GhosttyGridRef::default();
    // SAFETY: Category: Foreign Grid Reference Extraction.
    // Invariant: handle is live; point is initialized; out grid_ref is valid writable stack storage.
    let result = unsafe { ghostty_terminal_grid_ref(handle.as_ptr(), point, &mut grid_ref) };
    NativeTerminalError::from_c_result(result, "ghostty_terminal_grid_ref(Viewport)")?;
    Ok(grid_ref)
}

fn install_selection(
    handle: NonNull<GhosttyTerminalImpl>,
    selection: &GhosttySelection,
) -> Result<(), NativeTerminalError> {
    // SAFETY: Category: Foreign Selection Installation.
    // Invariant: selection refs were produced by this terminal and remain valid until this synchronous mutation.
    let result = unsafe {
        ghostty_terminal_set(
            handle.as_ptr(),
            GHOSTTY_TERMINAL_OPT_SELECTION,
            selection as *const GhosttySelection as *const c_void,
        )
    };
    NativeTerminalError::from_c_result(result, "ghostty_terminal_set(Selection)")
}

pub fn select_all(handle: NonNull<GhosttyTerminalImpl>) -> Result<(), NativeTerminalError> {
    let mut selection = GhosttySelection::default();
    // SAFETY: Category: Foreign Selection Extraction.
    // Invariant: handle is live and out selection is initialized writable stack storage.
    let result = unsafe { ghostty_terminal_select_all(handle.as_ptr(), &mut selection) };
    NativeTerminalError::from_c_result(result, "ghostty_terminal_select_all")?;
    install_selection(handle, &selection)
}

pub fn select_word_at(
    handle: NonNull<GhosttyTerminalImpl>,
    col: u16,
    row: u16,
) -> Result<(), NativeTerminalError> {
    let options = GhosttyTerminalSelectWordOptions {
        size: std::mem::size_of::<GhosttyTerminalSelectWordOptions>(),
        grid_ref: viewport_ref(handle, col, row)?,
        boundary_codepoints: std::ptr::null(),
        boundary_codepoints_len: 0,
    };
    let mut selection = GhosttySelection::default();
    // SAFETY: Category: Foreign Selection Extraction.
    // Invariant: options contains a fresh ref from this terminal and default boundary pointers are null with zero length.
    let result = unsafe { ghostty_terminal_select_word(handle.as_ptr(), &options, &mut selection) };
    NativeTerminalError::from_c_result(result, "ghostty_terminal_select_word")?;
    install_selection(handle, &selection)
}

pub fn select_line_at(
    handle: NonNull<GhosttyTerminalImpl>,
    row: u16,
) -> Result<(), NativeTerminalError> {
    let options = GhosttyTerminalSelectLineOptions {
        size: std::mem::size_of::<GhosttyTerminalSelectLineOptions>(),
        grid_ref: viewport_ref(handle, 0, row)?,
        whitespace: std::ptr::null(),
        whitespace_len: 0,
        semantic_prompt_boundary: false,
    };
    let mut selection = GhosttySelection::default();
    // SAFETY: Category: Foreign Selection Extraction.
    // Invariant: options contains a fresh ref from this terminal and default whitespace pointers are null with zero length.
    let result = unsafe { ghostty_terminal_select_line(handle.as_ptr(), &options, &mut selection) };
    NativeTerminalError::from_c_result(result, "ghostty_terminal_select_line")?;
    install_selection(handle, &selection)
}

pub fn selection_text(
    handle: NonNull<GhosttyTerminalImpl>,
) -> Result<Option<String>, NativeTerminalError> {
    let options = GhosttyTerminalSelectionFormatOptions {
        size: std::mem::size_of::<GhosttyTerminalSelectionFormatOptions>(),
        emit: GHOSTTY_FORMATTER_FORMAT_PLAIN,
        unwrap: true,
        trim: true,
        selection: std::ptr::null(),
    };
    let mut ptr = std::ptr::null_mut();
    let mut len = 0usize;
    // SAFETY: Category: Foreign Allocated Buffer Extraction.
    // Invariant: null allocator selects default; out pointer and length are valid stack storage.
    let result = unsafe {
        ghostty_terminal_selection_format_alloc(
            handle.as_ptr(),
            std::ptr::null(),
            options,
            &mut ptr,
            &mut len,
        )
    };
    if result == GHOSTTY_NO_VALUE {
        return Ok(None);
    }
    NativeTerminalError::from_c_result(result, "ghostty_terminal_selection_format_alloc")?;

    let bytes = if len == 0 {
        Vec::new()
    } else {
        if ptr.is_null() {
            return Err(NativeTerminalError::InvalidValue(
                "selection formatter returned null pointer with non-zero length".to_string(),
            ));
        }
        // SAFETY: Category: Foreign Buffer Extraction.
        // Invariant: successful allocation returned ptr valid for len bytes until ghostty_free below.
        unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec()
    };

    // SAFETY: Category: Foreign Resource Deallocation.
    // Invariant: ptr and len are exactly the allocation returned by the default Ghostty allocator.
    unsafe { ghostty_free(std::ptr::null(), ptr, len) };
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|error| NativeTerminalError::InvalidUtf8(error.to_string()))
}

pub fn clear_selection(handle: NonNull<GhosttyTerminalImpl>) -> Result<(), NativeTerminalError> {
    // SAFETY: Category: Foreign Selection Mutation.
    // Invariant: null value is the documented way to clear the active selection.
    let result = unsafe {
        ghostty_terminal_set(
            handle.as_ptr(),
            GHOSTTY_TERMINAL_OPT_SELECTION,
            std::ptr::null(),
        )
    };
    NativeTerminalError::from_c_result(result, "ghostty_terminal_set(ClearSelection)")
}

pub fn selection_range(
    handle: NonNull<GhosttyTerminalImpl>,
) -> Result<Option<(u16, u16, u16, u16)>, NativeTerminalError> {
    let mut selection = GhosttySelection::default();
    // SAFETY: Category: Foreign Selection Extraction.
    // Invariant: output points to initialized writable GhosttySelection stack storage.
    let result = unsafe {
        ghostty_terminal_get(
            handle.as_ptr(),
            GHOSTTY_TERMINAL_DATA_SELECTION,
            &mut selection as *mut GhosttySelection as *mut c_void,
        )
    };
    if result == GHOSTTY_NO_VALUE {
        return Ok(None);
    }
    NativeTerminalError::from_c_result(result, "ghostty_terminal_get(Selection)")?;

    let mut ordered = GhosttySelection::default();
    // SAFETY: Category: Foreign Selection Ordering.
    // Invariant: selection is a fresh snapshot from this terminal and no mutation has occurred.
    let result = unsafe {
        ghostty_terminal_selection_ordered(
            handle.as_ptr(),
            &selection,
            GHOSTTY_SELECTION_ORDER_FORWARD,
            &mut ordered,
        )
    };
    NativeTerminalError::from_c_result(result, "ghostty_terminal_selection_ordered")?;

    let mut start = GhosttyPointCoordinate::default();
    let mut end = GhosttyPointCoordinate::default();
    // SAFETY: Category: Foreign Grid Coordinate Conversion.
    // Invariant: ordered endpoints are fresh refs from this terminal; outputs are writable stack storage.
    let start_result = unsafe {
        ghostty_terminal_point_from_grid_ref(
            handle.as_ptr(),
            &ordered.start,
            GHOSTTY_POINT_TAG_SCREEN,
            &mut start,
        )
    };
    NativeTerminalError::from_c_result(
        start_result,
        "ghostty_terminal_point_from_grid_ref(Start)",
    )?;
    // SAFETY: Category: Foreign Grid Coordinate Conversion.
    // Invariant: ordered endpoints are fresh refs from this terminal; outputs are writable stack storage.
    let end_result = unsafe {
        ghostty_terminal_point_from_grid_ref(
            handle.as_ptr(),
            &ordered.end,
            GHOSTTY_POINT_TAG_SCREEN,
            &mut end,
        )
    };
    NativeTerminalError::from_c_result(end_result, "ghostty_terminal_point_from_grid_ref(End)")?;

    let start_row = u16::try_from(start.y).map_err(|_| {
        NativeTerminalError::InvalidValue("selection start row exceeds u16 API range".to_string())
    })?;
    let end_row = u16::try_from(end.y).map_err(|_| {
        NativeTerminalError::InvalidValue("selection end row exceeds u16 API range".to_string())
    })?;
    Ok(Some((start.x, start_row, end.x, end_row)))
}
