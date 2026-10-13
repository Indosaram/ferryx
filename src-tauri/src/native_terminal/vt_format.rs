//! Serializes a terminal's active screen back into VT bytes that rebuild it in a fresh parser.

use std::ptr::NonNull;

use super::error::NativeTerminalError;
use super::queries::{query_rows, query_scrollback_rows};
use super::sys::constants::GHOSTTY_FORMATTER_FORMAT_VT;
use super::sys::ffi::{
    ghostty_formatter_format_alloc, ghostty_formatter_free, ghostty_formatter_terminal_new,
    ghostty_free,
};
use super::sys::types::{
    GhosttyFormatter, GhosttyFormatterScreenExtra, GhosttyFormatterTerminalExtra,
    GhosttyFormatterTerminalOptions, GhosttyTerminalImpl,
};

#[derive(Clone, Copy, Default)]
struct Extras {
    leading: bool,
    trailing: bool,
}

fn format(
    handle: NonNull<GhosttyTerminalImpl>,
    extras: Extras,
) -> Result<Vec<u8>, NativeTerminalError> {
    let options = GhosttyFormatterTerminalOptions {
        size: std::mem::size_of::<GhosttyFormatterTerminalOptions>(),
        emit: GHOSTTY_FORMATTER_FORMAT_VT,
        // One emitted line per grid row keeps the replayed row count exact.
        unwrap: false,
        trim: false,
        extra: GhosttyFormatterTerminalExtra {
            size: std::mem::size_of::<GhosttyFormatterTerminalExtra>(),
            palette: extras.leading,
            modes: extras.leading,
            scrolling_region: extras.trailing,
            tabstops: extras.leading,
            pwd: extras.trailing,
            keyboard: extras.trailing,
            screen: GhosttyFormatterScreenExtra {
                size: std::mem::size_of::<GhosttyFormatterScreenExtra>(),
                cursor: extras.trailing,
                style: extras.trailing,
                hyperlink: extras.trailing,
                protection: extras.trailing,
                kitty_keyboard: extras.trailing,
                charsets: extras.trailing,
            },
        },
        selection: std::ptr::null(),
    };
    let mut formatter: GhosttyFormatter = std::ptr::null_mut();
    // SAFETY: Category: Foreign Handle Allocation.
    // Invariant: handle is a live terminal that outlives the formatter freed below; options is
    // fully initialized with matching struct sizes; the out pointer is valid stack storage.
    let result = unsafe {
        ghostty_formatter_terminal_new(std::ptr::null(), &mut formatter, handle.as_ptr(), options)
    };
    NativeTerminalError::from_c_result(result, "ghostty_formatter_terminal_new")?;
    struct Guard(GhosttyFormatter);
    impl Drop for Guard {
        fn drop(&mut self) {
            // SAFETY: the formatter came from ghostty_formatter_terminal_new and is freed once.
            unsafe { ghostty_formatter_free(self.0) };
        }
    }
    let formatter = Guard(formatter);

    let mut ptr = std::ptr::null_mut();
    let mut len = 0usize;
    // SAFETY: Category: Foreign Allocated Buffer Extraction.
    // Invariant: formatter is live; null allocator selects the default; outputs are stack storage.
    let result = unsafe {
        ghostty_formatter_format_alloc(formatter.0, std::ptr::null(), &mut ptr, &mut len)
    };
    NativeTerminalError::from_c_result(result, "ghostty_formatter_format_alloc")?;
    if len == 0 {
        return Ok(Vec::new());
    }
    if ptr.is_null() {
        return Err(NativeTerminalError::InvalidValue(
            "formatter returned null pointer with non-zero length".to_string(),
        ));
    }
    // SAFETY: Category: Foreign Buffer Extraction. ptr is valid for len bytes until freed below.
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec();
    // SAFETY: Category: Foreign Resource Deallocation. Exact allocation from the default allocator.
    unsafe { ghostty_free(std::ptr::null(), ptr, len) };
    Ok(bytes)
}

/// VT bytes that rebuild this terminal's active screen, modes, and cursor in a fresh parser of
/// the same size, with the replayed grid aligned to the original row for row.
///
/// The formatter drops trailing blank rows, so its content alone would end up bottom-aligned
/// and every later cursor movement would land on the wrong row. The gap is restored with
/// newlines between the content and the trailing state (scroll region, cursor, style), which is
/// why the output is assembled from three passes over the same state.
pub fn reconstruction_bytes(
    handle: NonNull<GhosttyTerminalImpl>,
) -> Result<Vec<u8>, NativeTerminalError> {
    let leading_and_content = format(handle, Extras { leading: true, trailing: false })?;
    let content = format(handle, Extras::default())?;
    let content_and_trailing = format(handle, Extras { leading: false, trailing: true })?;
    let trailing = content_and_trailing
        .strip_prefix(content.as_slice())
        .ok_or_else(|| {
            NativeTerminalError::InvalidValue("formatter content differs between passes".to_string())
        })?;

    // Each emitted CRLF ends one grid row; the last non-blank row has none.
    let emitted_rows = content.windows(2).filter(|pair| pair == b"\r\n").count() + 1;
    let grid_rows = query_scrollback_rows(handle)? + usize::from(query_rows(handle)?);
    let padding = grid_rows.saturating_sub(emitted_rows);

    let mut out = Vec::with_capacity(leading_and_content.len() + padding * 2 + trailing.len());
    out.extend_from_slice(&leading_and_content);
    for _ in 0..padding {
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(trailing);
    Ok(out)
}
