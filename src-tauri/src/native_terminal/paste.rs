//! Safe clipboard paste encoding.

use std::ffi::c_void;
use std::ptr::NonNull;

use super::error::NativeTerminalError;
use super::sys::ffi::{ghostty_paste_encode, ghostty_paste_is_safe, ghostty_terminal_get};
use super::sys::types::{
    GhosttyTerminalImpl, GhosttyTerminalModeConfig, GHOSTTY_MODE_BRACKETED_PASTE,
    GHOSTTY_TERMINAL_DATA_MODE,
};

pub fn paste_is_safe(text: &str) -> bool {
    // SAFETY: Category: Buffer Borrowing across FFI.
    // Invariant: text.as_ptr() is valid for text.len() bytes and borrowed synchronously.
    unsafe { ghostty_paste_is_safe(text.as_ptr(), text.len()) }
}

pub fn bracketed_paste_enabled(
    handle: NonNull<GhosttyTerminalImpl>,
) -> Result<bool, NativeTerminalError> {
    let mut config = GhosttyTerminalModeConfig {
        mode: GHOSTTY_MODE_BRACKETED_PASTE,
        value: false,
    };
    // SAFETY: Category: Foreign Sized Data Extraction.
    // Invariant: config.mode is initialized and &mut config matches GhosttyTerminalModeConfig.
    let result = unsafe {
        ghostty_terminal_get(
            handle.as_ptr(),
            GHOSTTY_TERMINAL_DATA_MODE,
            &mut config as *mut GhosttyTerminalModeConfig as *mut c_void,
        )
    };
    NativeTerminalError::from_c_result(result, "ghostty_terminal_get(ModeBracketedPaste)")?;
    Ok(config.value)
}

pub fn encode_paste(
    handle: NonNull<GhosttyTerminalImpl>,
    text: &str,
) -> Result<Vec<u8>, NativeTerminalError> {
    encode_paste_with_override(handle, text, None)
}

pub fn encode_paste_with_override(
    handle: NonNull<GhosttyTerminalImpl>,
    text: &str,
    bracketed_override: Option<bool>,
) -> Result<Vec<u8>, NativeTerminalError> {
    let normalized = if text.contains('\r') {
        text.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        text.to_string()
    };

    let is_multiline = normalized.contains('\n');
    let bracketed = match bracketed_override {
        Some(val) => val,
        None => bracketed_paste_enabled(handle)? || is_multiline,
    };

    let mut input = normalized.as_bytes().to_vec();
    let input_ptr = if input.is_empty() {
        std::ptr::null_mut()
    } else {
        input.as_mut_ptr()
    };
    let mut required = 0usize;

    // SAFETY: Category: Foreign Buffer Size Query.
    // Invariant: input_ptr is null for empty input or writable for input.len() bytes; out buffer is intentionally null.
    let query = unsafe {
        ghostty_paste_encode(
            input_ptr,
            input.len(),
            bracketed,
            std::ptr::null_mut(),
            0,
            &mut required,
        )
    };
    match NativeTerminalError::from_c_result(query, "ghostty_paste_encode(size)") {
        Err(NativeTerminalError::OutOfSpace) => {}
        other => other?,
    }

    let mut output = vec![0u8; required];
    let output_ptr = if output.is_empty() {
        std::ptr::null_mut()
    } else {
        output.as_mut_ptr()
    };
    let mut written = 0usize;
    // SAFETY: Category: Foreign Buffer Encoding.
    // Invariant: input is writable for input.len(); output is writable for output.len(); lengths remain stable synchronously.
    let result = unsafe {
        ghostty_paste_encode(
            input_ptr,
            input.len(),
            bracketed,
            output_ptr,
            output.len(),
            &mut written,
        )
    };
    NativeTerminalError::from_c_result(result, "ghostty_paste_encode")?;
    output.truncate(written);
    Ok(output)
}
