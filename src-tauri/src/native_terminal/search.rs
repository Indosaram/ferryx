//! Scrollback-aware terminal grid text search.

use std::ptr::NonNull;

use super::error::NativeTerminalError;
use super::sys::ffi::{ghostty_grid_ref_graphemes, ghostty_terminal_grid_ref};
use super::sys::types::{
    GhosttyGridRef, GhosttyPoint, GhosttyPointCoordinate, GhosttyPointValue, GhosttyTerminalImpl,
    GHOSTTY_POINT_TAG_SCREEN,
};

fn cell_text(
    handle: NonNull<GhosttyTerminalImpl>,
    col: u16,
    row: u32,
) -> Result<String, NativeTerminalError> {
    let point = GhosttyPoint {
        tag: GHOSTTY_POINT_TAG_SCREEN,
        value: GhosttyPointValue {
            coordinate: GhosttyPointCoordinate { x: col, y: row },
        },
    };
    let mut grid_ref = GhosttyGridRef::default();
    // SAFETY: Category: Foreign Grid Reference Extraction.
    // Invariant: handle is live; screen point is caller-bounded; output is writable stack storage.
    let result = unsafe { ghostty_terminal_grid_ref(handle.as_ptr(), point, &mut grid_ref) };
    NativeTerminalError::from_c_result(result, "ghostty_terminal_grid_ref(Search)")?;

    let mut required = 0usize;
    // SAFETY: Category: Foreign Buffer Size Query.
    // Invariant: grid_ref is fresh and valid until the next mutation; null buffer intentionally queries codepoint count.
    let query =
        unsafe { ghostty_grid_ref_graphemes(&grid_ref, std::ptr::null_mut(), 0, &mut required) };
    match NativeTerminalError::from_c_result(query, "ghostty_grid_ref_graphemes(size)") {
        Err(NativeTerminalError::OutOfSpace) => {}
        other => other?,
    }
    if required == 0 {
        return Ok(" ".to_string());
    }

    let mut codepoints = vec![0u32; required];
    let mut written = 0usize;
    // SAFETY: Category: Foreign Buffer Extraction.
    // Invariant: grid_ref remains fresh and codepoints is writable for its declared element count.
    let result = unsafe {
        ghostty_grid_ref_graphemes(
            &grid_ref,
            codepoints.as_mut_ptr(),
            codepoints.len(),
            &mut written,
        )
    };
    NativeTerminalError::from_c_result(result, "ghostty_grid_ref_graphemes")?;
    codepoints.truncate(written);
    codepoints
        .into_iter()
        .map(|codepoint| {
            char::from_u32(codepoint).ok_or_else(|| {
                NativeTerminalError::InvalidValue(format!(
                    "invalid Unicode scalar returned for grid cell: {codepoint:#x}"
                ))
            })
        })
        .collect()
}

pub fn search_grid(
    handle: NonNull<GhosttyTerminalImpl>,
    cols: u16,
    total_rows: usize,
    query: &str,
    case_sensitive: bool,
) -> Result<Vec<(u16, u16, u16)>, NativeTerminalError> {
    if query.is_empty() {
        return Ok(Vec::new());
    }

    let normalized_query = if case_sensitive {
        query.to_owned()
    } else {
        query.to_lowercase()
    };

    let row_count = total_rows.min(usize::from(u16::MAX) + 1);
    let mut matches = Vec::new();
    for row in 0..row_count {
        let mut cells = Vec::with_capacity(cols as usize);
        for col in 0..cols {
            cells.push(cell_text(handle, col, row as u32)?);
        }

        for start in 0..cols {
            let mut candidate = String::new();
            for end in start..cols {
                candidate.push_str(&cells[end as usize]);
                let normalized_candidate = if case_sensitive {
                    candidate.as_str().into()
                } else {
                    candidate.to_lowercase()
                };
                if normalized_candidate == normalized_query {
                    matches.push((row as u16, start, end));
                    break;
                }
                if normalized_candidate.len() >= normalized_query.len() {
                    break;
                }
            }
        }
    }
    Ok(matches)
}
