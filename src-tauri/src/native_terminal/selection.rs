//! Safe terminal selection helpers.

use std::ffi::c_void;
use std::ptr::NonNull;

use super::error::NativeTerminalError;
use super::guards::{SelectionGestureEventGuard, SelectionGestureGuard};
use super::mouse::{MouseAction, MouseButton, MouseEvent};
use super::sys::ffi::{
    ghostty_free, ghostty_selection_gesture_event, ghostty_selection_gesture_event_new,
    ghostty_selection_gesture_event_set, ghostty_selection_gesture_new,
    ghostty_selection_gesture_reset, ghostty_terminal_get, ghostty_terminal_grid_ref,
    ghostty_terminal_point_from_grid_ref, ghostty_terminal_select_all,
    ghostty_terminal_select_line, ghostty_terminal_select_word,
    ghostty_terminal_selection_format_alloc, ghostty_terminal_selection_ordered,
    ghostty_terminal_set,
};
use super::sys::types::{
    GhosttyGridRef, GhosttyPoint, GhosttyPointCoordinate, GhosttyPointValue, GhosttySelection,
    GhosttySelectionGesture, GhosttySelectionGestureEvent, GhosttySelectionGestureGeometry,
    GhosttySurfacePosition, GhosttyTerminalImpl, GhosttyTerminalSelectLineOptions,
    GhosttyTerminalSelectWordOptions, GhosttyTerminalSelectionFormatOptions,
    GHOSTTY_FORMATTER_FORMAT_PLAIN, GHOSTTY_NO_VALUE, GHOSTTY_POINT_TAG_SCREEN,
    GHOSTTY_POINT_TAG_VIEWPORT, GHOSTTY_SELECTION_GESTURE_EVENT_OPT_GEOMETRY,
    GHOSTTY_SELECTION_GESTURE_EVENT_OPT_POSITION, GHOSTTY_SELECTION_GESTURE_EVENT_OPT_REF,
    GHOSTTY_SELECTION_GESTURE_EVENT_TYPE_DRAG, GHOSTTY_SELECTION_GESTURE_EVENT_TYPE_PRESS,
    GHOSTTY_SELECTION_GESTURE_EVENT_TYPE_RELEASE, GHOSTTY_SELECTION_ORDER_FORWARD, GHOSTTY_SUCCESS,
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

pub fn create_selection_gesture() -> Result<SelectionGestureGuard, NativeTerminalError> {
    let mut gesture: GhosttySelectionGesture = std::ptr::null_mut();
    // SAFETY: Category: Foreign Handle Allocation. Null allocator selects default.
    let result = unsafe { ghostty_selection_gesture_new(std::ptr::null(), &mut gesture) };
    NativeTerminalError::from_c_result(result, "ghostty_selection_gesture_new")?;
    let non_null = NonNull::new(gesture).ok_or_else(|| {
        NativeTerminalError::InvalidValue(
            "ghostty_selection_gesture_new returned null pointer".to_string(),
        )
    })?;
    Ok(SelectionGestureGuard::new(non_null))
}

pub fn reset_selection_gesture(
    gesture: &SelectionGestureGuard,
    terminal: NonNull<GhosttyTerminalImpl>,
) {
    // SAFETY: Category: Foreign State Mutation. Both handles are valid.
    unsafe {
        ghostty_selection_gesture_reset(gesture.as_ptr(), terminal.as_ptr());
    }
}

pub fn apply_mouse_gesture(
    terminal: NonNull<GhosttyTerminalImpl>,
    gesture: &SelectionGestureGuard,
    event: &MouseEvent,
) -> Result<(), NativeTerminalError> {
    if !event.position.x.is_finite() || !event.position.y.is_finite() {
        return Err(NativeTerminalError::InvalidValue(
            "MousePosition x and y coordinates must be finite".to_string(),
        ));
    }

    if event.action == MouseAction::Press
        && event.button.is_some()
        && event.button != Some(MouseButton::Left)
    {
        return Ok(());
    }

    let cols = super::queries::query_cols(terminal)?;
    let rows = super::queries::query_rows(terminal)?;

    let (cell_width, cell_height, padding_left, padding_top, screen_height) = match event.size {
        Some(sz) => (
            sz.cell_width.max(1),
            sz.cell_height.max(1),
            sz.padding_left,
            sz.padding_top,
            sz.screen_height.max(1),
        ),
        None => (10, 16, 0, 0, (u32::from(rows) * 16).max(1)),
    };

    let rel_x = (event.position.x.max(0.0) - padding_left as f32).max(0.0);
    let rel_y = (event.position.y.max(0.0) - padding_top as f32).max(0.0);
    let col = ((rel_x / cell_width as f32) as u16).min(cols.saturating_sub(1));
    let row = ((rel_y / cell_height as f32) as u16).min(rows.saturating_sub(1));

    let grid_ref = viewport_ref(terminal, col, row)?;

    match event.action {
        MouseAction::Press => {
            let mut event_raw: GhosttySelectionGestureEvent = std::ptr::null_mut();
            // SAFETY: Category: Foreign Resource Allocation. Creates selection gesture event.
            let res = unsafe {
                ghostty_selection_gesture_event_new(
                    std::ptr::null(),
                    &mut event_raw,
                    GHOSTTY_SELECTION_GESTURE_EVENT_TYPE_PRESS,
                )
            };
            NativeTerminalError::from_c_result(res, "ghostty_selection_gesture_event_new(Press)")?;
            let non_null = NonNull::new(event_raw).ok_or_else(|| {
                NativeTerminalError::InvalidValue(
                    "ghostty_selection_gesture_event_new returned null pointer".to_string(),
                )
            })?;
            let event_guard = SelectionGestureEventGuard(non_null);

            // SAFETY: Category: Foreign Option Configuration. grid_ref is valid stack storage.
            let res = unsafe {
                ghostty_selection_gesture_event_set(
                    event_guard.0.as_ptr(),
                    GHOSTTY_SELECTION_GESTURE_EVENT_OPT_REF,
                    &grid_ref as *const GhosttyGridRef as *const c_void,
                )
            };
            NativeTerminalError::from_c_result(res, "ghostty_selection_gesture_event_set(Ref)")?;

            let pos = GhosttySurfacePosition {
                x: event.position.x as f64,
                y: event.position.y as f64,
            };
            // SAFETY: Category: Foreign Option Configuration. pos is valid stack storage.
            let res = unsafe {
                ghostty_selection_gesture_event_set(
                    event_guard.0.as_ptr(),
                    GHOSTTY_SELECTION_GESTURE_EVENT_OPT_POSITION,
                    &pos as *const GhosttySurfacePosition as *const c_void,
                )
            };
            NativeTerminalError::from_c_result(
                res,
                "ghostty_selection_gesture_event_set(Position)",
            )?;

            let mut selection = GhosttySelection::default();
            // SAFETY: Category: Foreign Selection Gesture Event Application.
            // Invariant: gesture and terminal are live handles; selection is stack allocated.
            let event_res = unsafe {
                ghostty_selection_gesture_event(
                    gesture.as_ptr(),
                    terminal.as_ptr(),
                    event_guard.0.as_ptr(),
                    &mut selection,
                )
            };
            if event_res == GHOSTTY_SUCCESS {
                install_selection(terminal, &selection)?;
            } else if event_res == GHOSTTY_NO_VALUE {
                clear_selection(terminal)?;
            } else {
                NativeTerminalError::from_c_result(
                    event_res,
                    "ghostty_selection_gesture_event(Press)",
                )?;
            }
        }
        MouseAction::Motion => {
            let mut event_raw: GhosttySelectionGestureEvent = std::ptr::null_mut();
            // SAFETY: Category: Foreign Resource Allocation. Creates selection gesture event.
            let res = unsafe {
                ghostty_selection_gesture_event_new(
                    std::ptr::null(),
                    &mut event_raw,
                    GHOSTTY_SELECTION_GESTURE_EVENT_TYPE_DRAG,
                )
            };
            NativeTerminalError::from_c_result(res, "ghostty_selection_gesture_event_new(Drag)")?;
            let non_null = NonNull::new(event_raw).ok_or_else(|| {
                NativeTerminalError::InvalidValue(
                    "ghostty_selection_gesture_event_new returned null pointer".to_string(),
                )
            })?;
            let event_guard = SelectionGestureEventGuard(non_null);

            // SAFETY: Category: Foreign Option Configuration. grid_ref is valid stack storage.
            let res = unsafe {
                ghostty_selection_gesture_event_set(
                    event_guard.0.as_ptr(),
                    GHOSTTY_SELECTION_GESTURE_EVENT_OPT_REF,
                    &grid_ref as *const GhosttyGridRef as *const c_void,
                )
            };
            NativeTerminalError::from_c_result(res, "ghostty_selection_gesture_event_set(Ref)")?;

            let pos = GhosttySurfacePosition {
                x: event.position.x as f64,
                y: event.position.y as f64,
            };
            // SAFETY: Category: Foreign Option Configuration. pos is valid stack storage.
            let res = unsafe {
                ghostty_selection_gesture_event_set(
                    event_guard.0.as_ptr(),
                    GHOSTTY_SELECTION_GESTURE_EVENT_OPT_POSITION,
                    &pos as *const GhosttySurfacePosition as *const c_void,
                )
            };
            NativeTerminalError::from_c_result(
                res,
                "ghostty_selection_gesture_event_set(Position)",
            )?;

            let geometry = GhosttySelectionGestureGeometry {
                columns: u32::from(cols).max(1),
                cell_width,
                padding_left,
                screen_height,
            };
            // SAFETY: Category: Foreign Option Configuration. geometry is valid stack storage.
            let res = unsafe {
                ghostty_selection_gesture_event_set(
                    event_guard.0.as_ptr(),
                    GHOSTTY_SELECTION_GESTURE_EVENT_OPT_GEOMETRY,
                    &geometry as *const GhosttySelectionGestureGeometry as *const c_void,
                )
            };
            NativeTerminalError::from_c_result(
                res,
                "ghostty_selection_gesture_event_set(Geometry)",
            )?;

            let mut selection = GhosttySelection::default();
            // SAFETY: Category: Foreign Selection Gesture Event Application.
            // Invariant: gesture and terminal are live handles; selection is stack allocated.
            let event_res = unsafe {
                ghostty_selection_gesture_event(
                    gesture.as_ptr(),
                    terminal.as_ptr(),
                    event_guard.0.as_ptr(),
                    &mut selection,
                )
            };
            if event_res == GHOSTTY_SUCCESS {
                install_selection(terminal, &selection)?;
            } else if event_res != GHOSTTY_NO_VALUE {
                NativeTerminalError::from_c_result(
                    event_res,
                    "ghostty_selection_gesture_event(Drag)",
                )?;
            }
        }
        MouseAction::Release => {
            let mut event_raw: GhosttySelectionGestureEvent = std::ptr::null_mut();
            // SAFETY: Category: Foreign Resource Allocation. Creates selection gesture event.
            let res = unsafe {
                ghostty_selection_gesture_event_new(
                    std::ptr::null(),
                    &mut event_raw,
                    GHOSTTY_SELECTION_GESTURE_EVENT_TYPE_RELEASE,
                )
            };
            NativeTerminalError::from_c_result(
                res,
                "ghostty_selection_gesture_event_new(Release)",
            )?;
            let non_null = NonNull::new(event_raw).ok_or_else(|| {
                NativeTerminalError::InvalidValue(
                    "ghostty_selection_gesture_event_new returned null pointer".to_string(),
                )
            })?;
            let event_guard = SelectionGestureEventGuard(non_null);

            // SAFETY: Category: Foreign Option Configuration. grid_ref is valid stack storage.
            let res = unsafe {
                ghostty_selection_gesture_event_set(
                    event_guard.0.as_ptr(),
                    GHOSTTY_SELECTION_GESTURE_EVENT_OPT_REF,
                    &grid_ref as *const GhosttyGridRef as *const c_void,
                )
            };
            NativeTerminalError::from_c_result(res, "ghostty_selection_gesture_event_set(Ref)")?;

            // SAFETY: Category: Foreign Selection Gesture Event Application.
            // Invariant: Release event takes NULL out_selection and returns GHOSTTY_NO_VALUE.
            let event_res = unsafe {
                ghostty_selection_gesture_event(
                    gesture.as_ptr(),
                    terminal.as_ptr(),
                    event_guard.0.as_ptr(),
                    std::ptr::null_mut(),
                )
            };
            if event_res != GHOSTTY_SUCCESS && event_res != GHOSTTY_NO_VALUE {
                NativeTerminalError::from_c_result(
                    event_res,
                    "ghostty_selection_gesture_event(Release)",
                )?;
            }
        }
    }

    Ok(())
}
