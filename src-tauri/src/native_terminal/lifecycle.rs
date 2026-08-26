//! Terminal handle allocation, callback registration, and teardown lifecycle.

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicU64};

use super::bell::{terminal_bell_callback, terminal_title_changed_callback, TerminalContext};
use super::error::NativeTerminalError;
use super::sys::ffi::{ghostty_terminal_free, ghostty_terminal_new, ghostty_terminal_set};
use super::sys::types::{
    GhosttyTerminal, GhosttyTerminalImpl, GHOSTTY_TERMINAL_OPT_BELL,
    GHOSTTY_TERMINAL_OPT_TITLE_CHANGED, GHOSTTY_TERMINAL_OPT_USERDATA,
};

/// Allocates a new native terminal and registers callbacks transactionally.
pub fn create_native_terminal(
    cols: u16,
    rows: u16,
) -> Result<(NonNull<GhosttyTerminalImpl>, Box<TerminalContext>), NativeTerminalError> {
    if cols == 0 || rows == 0 {
        return Err(NativeTerminalError::InvalidDimensions(cols, rows));
    }

    let mut raw_term: GhosttyTerminal = std::ptr::null_mut();

    // SAFETY: Category: Foreign Handle Allocation.
    // Invariant: Null allocator selects libghostty-vt default. &mut raw_term is a valid stack pointer.
    let result = unsafe { ghostty_terminal_new(std::ptr::null(), &mut raw_term, cols, rows) };
    NativeTerminalError::from_c_result(result, "ghostty_terminal_new")?;

    let non_null = NonNull::new(raw_term).ok_or_else(|| {
        NativeTerminalError::InvalidValue(
            "ghostty_terminal_new returned null pointer despite success status".to_string(),
        )
    })?;

    let context = Box::new(TerminalContext {
        bell_counter: AtomicU64::new(0),
        title_updated: AtomicBool::new(false),
    });
    let userdata_ptr = (&*context) as *const TerminalContext as *const c_void;

    // SAFETY: Category: Foreign Option Configuration.
    // Invariant: non_null is valid initialized terminal handle; userdata_ptr points to heap-pinned context.
    let reg_userdata = unsafe {
        ghostty_terminal_set(
            non_null.as_ptr(),
            GHOSTTY_TERMINAL_OPT_USERDATA,
            userdata_ptr,
        )
    };
    if let Err(e) = NativeTerminalError::from_c_result(reg_userdata, "set(OPT_USERDATA)") {
        // SAFETY: Clean up allocated foreign handle on registration failure.
        unsafe { ghostty_terminal_free(non_null.as_ptr()) };
        return Err(e);
    }

    let reg_bell = unsafe {
        ghostty_terminal_set(
            non_null.as_ptr(),
            GHOSTTY_TERMINAL_OPT_BELL,
            terminal_bell_callback as *const c_void,
        )
    };
    if let Err(e) = NativeTerminalError::from_c_result(reg_bell, "set(OPT_BELL)") {
        unsafe { ghostty_terminal_free(non_null.as_ptr()) };
        return Err(e);
    }

    let reg_title = unsafe {
        ghostty_terminal_set(
            non_null.as_ptr(),
            GHOSTTY_TERMINAL_OPT_TITLE_CHANGED,
            terminal_title_changed_callback as *const c_void,
        )
    };
    if let Err(e) = NativeTerminalError::from_c_result(reg_title, "set(OPT_TITLE_CHANGED)") {
        unsafe { ghostty_terminal_free(non_null.as_ptr()) };
        return Err(e);
    }

    Ok((non_null, context))
}

/// Detaches callbacks and deallocates the foreign terminal handle.
pub fn teardown_native_terminal(handle: NonNull<GhosttyTerminalImpl>) {
    // SAFETY: Category: Foreign Resource Deallocation & Callback Lifetime.
    // Invariant: Unset foreign callbacks so terminal never dereferences context after it is deallocated.
    unsafe {
        let null_ptr: *const c_void = std::ptr::null();
        let _ = ghostty_terminal_set(handle.as_ptr(), GHOSTTY_TERMINAL_OPT_BELL, null_ptr);
        let _ = ghostty_terminal_set(
            handle.as_ptr(),
            GHOSTTY_TERMINAL_OPT_TITLE_CHANGED,
            null_ptr,
        );
        let _ = ghostty_terminal_set(handle.as_ptr(), GHOSTTY_TERMINAL_OPT_USERDATA, null_ptr);
        ghostty_terminal_free(handle.as_ptr());
    }
}
