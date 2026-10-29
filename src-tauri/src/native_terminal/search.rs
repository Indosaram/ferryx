//! Scrollback-aware terminal grid text search.

use std::ptr::NonNull;

#[path = "search_matching.rs"]
mod matching;

use super::error::NativeTerminalError;
use super::sys::ffi::{
    ghostty_grid_ref_graphemes, ghostty_terminal_grid_ref, ghostty_terminal_point_from_grid_ref,
};
use super::sys::search::{
    ghostty_search_free, ghostty_search_get, ghostty_search_new, ghostty_search_run,
    ghostty_search_set, GhosttySearch, GhosttySearchImpl, GhosttySelectionBuffer,
};
use super::sys::types::{
    GhosttyGridRef, GhosttyPoint, GhosttyPointCoordinate, GhosttyPointValue, GhosttyTerminalImpl,
    GhosttySelection, GhosttyString, GHOSTTY_POINT_TAG_SCREEN, GHOSTTY_SEARCH_DATA_MATCHES,
    GHOSTTY_SEARCH_OPT_NEEDLE,
};

/// Read the grapheme cluster rendered by a single screen-coordinate cell.
/// Empty cells read as a single space so a row reads as contiguous text.
pub fn screen_cell_text(
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
            cells.push(screen_cell_text(handle, col, row as u32)?);
        }

        matching::append_row_matches(
            &cells,
            &normalized_query,
            case_sensitive,
            row as u16,
            &mut matches,
        );
    }
    Ok(matches)
}

/// Whether `search_with_ghostty` returns exactly what `search_grid` returns for this query.
///
/// `ghostty_search` matches bytes of the plain-formatted screen and folds only ASCII letters,
/// with no case-sensitive mode (search.h). It is therefore only exact for case-insensitive
/// needles of printable ASCII that:
/// - do not start or end with a space (the formatter trims trailing blanks per row, while
///   `search_grid` reads every blank cell as a space), and
/// - contain no `k`/`K` (`search_grid` lowercases with Unicode rules, where U+212A KELVIN SIGN
///   becomes ASCII `k`; ghostty_search does not fold it).
///
/// Every other query must stay on `search_grid`.
pub fn ghostty_search_eligible(query: &str, case_sensitive: bool) -> bool {
    let bytes = query.as_bytes();
    !case_sensitive
        && bytes.first().is_some_and(u8::is_ascii_graphic)
        && bytes.last().is_some_and(u8::is_ascii_graphic)
        && bytes.iter().all(|&b| b == b' ' || b.is_ascii_graphic())
        && !bytes.iter().any(|b| b.eq_ignore_ascii_case(&b'k'))
}

/// Owns one `GhosttySearch` bound to a terminal and frees it on drop.
struct SearchHandle(NonNull<GhosttySearchImpl>);

impl SearchHandle {
    fn new(handle: NonNull<GhosttyTerminalImpl>) -> Result<Self, NativeTerminalError> {
        let mut raw: GhosttySearch = std::ptr::null_mut();
        // SAFETY: Category: Foreign Handle Creation.
        // Invariant: handle is a live terminal serialized by the caller's borrow; null allocator
        // selects the library default; raw is writable stack storage.
        let result = unsafe { ghostty_search_new(std::ptr::null(), &mut raw, handle.as_ptr()) };
        NativeTerminalError::from_c_result(result, "ghostty_search_new")?;
        NonNull::new(raw).map(Self).ok_or_else(|| {
            NativeTerminalError::InvalidValue("ghostty_search_new returned a null search".into())
        })
    }

    fn run(&self, needle: &str) -> Result<(), NativeTerminalError> {
        let needle = GhosttyString {
            ptr: needle.as_ptr(),
            len: needle.len(),
        };
        // SAFETY: Category: Foreign Option Write.
        // Invariant: the search is live; the needle bytes are copied by the library during the call.
        let result = unsafe {
            ghostty_search_set(
                self.0.as_ptr(),
                GHOSTTY_SEARCH_OPT_NEEDLE,
                (&needle as *const GhosttyString).cast(),
            )
        };
        NativeTerminalError::from_c_result(result, "ghostty_search_set(Needle)")?;
        // SAFETY: Category: Foreign Blocking Search.
        // Invariant: the search is live and the caller holds exclusive access to its terminal.
        let result = unsafe { ghostty_search_run(self.0.as_ptr()) };
        NativeTerminalError::from_c_result(result, "ghostty_search_run")
    }

    /// All matches on the active screen; valid until the terminal is next modified.
    fn matches(&self) -> Result<Vec<GhosttySelection>, NativeTerminalError> {
        let mut buffer = GhosttySelectionBuffer {
            ptr: std::ptr::null_mut(),
            cap: 0,
            len: 0,
        };
        // SAFETY: Category: Foreign Buffer Size Query.
        // Invariant: the search is live; a null ptr with cap 0 asks only for the required length.
        let result = unsafe {
            ghostty_search_get(
                self.0.as_ptr(),
                GHOSTTY_SEARCH_DATA_MATCHES,
                (&mut buffer as *mut GhosttySelectionBuffer).cast(),
            )
        };
        match NativeTerminalError::from_c_result(result, "ghostty_search_get(Matches size)") {
            Ok(()) => return Ok(Vec::new()),
            Err(NativeTerminalError::OutOfSpace) => {}
            Err(other) => return Err(other),
        }
        let mut selections = vec![GhosttySelection::default(); buffer.len];
        buffer = GhosttySelectionBuffer {
            ptr: selections.as_mut_ptr(),
            cap: selections.len(),
            len: 0,
        };
        // SAFETY: Category: Foreign Buffer Extraction.
        // Invariant: ptr addresses cap initialized, writable GhosttySelection entries.
        let result = unsafe {
            ghostty_search_get(
                self.0.as_ptr(),
                GHOSTTY_SEARCH_DATA_MATCHES,
                (&mut buffer as *mut GhosttySelectionBuffer).cast(),
            )
        };
        NativeTerminalError::from_c_result(result, "ghostty_search_get(Matches)")?;
        selections.truncate(buffer.len);
        Ok(selections)
    }
}

impl Drop for SearchHandle {
    fn drop(&mut self) {
        // SAFETY: Category: Foreign Handle Release.
        // Invariant: the search was created by ghostty_search_new and is freed exactly once here.
        unsafe { ghostty_search_free(self.0.as_ptr()) }
    }
}

fn match_screen_point(
    handle: NonNull<GhosttyTerminalImpl>,
    grid_ref: &GhosttyGridRef,
) -> Result<GhosttyPointCoordinate, NativeTerminalError> {
    let mut point = GhosttyPointCoordinate::default();
    // SAFETY: Category: Foreign Grid Coordinate Conversion.
    // Invariant: grid_ref is a match snapshot from this terminal, read before any mutation.
    let result = unsafe {
        ghostty_terminal_point_from_grid_ref(
            handle.as_ptr(),
            grid_ref,
            GHOSTTY_POINT_TAG_SCREEN,
            &mut point,
        )
    };
    NativeTerminalError::from_c_result(result, "ghostty_terminal_point_from_grid_ref(Search)")?;
    Ok(point)
}

/// True when the cell holds exactly one codepoint. An ASCII needle can end on the base of a
/// grapheme that carries combining marks (`e` in `e\u{301}`); `search_grid` compares whole
/// cells, so such a match does not exist there.
fn holds_one_codepoint(grid_ref: &GhosttyGridRef) -> Result<bool, NativeTerminalError> {
    let mut codepoints = [0u32; 2];
    let mut written = 0usize;
    // SAFETY: Category: Foreign Buffer Extraction.
    // Invariant: grid_ref is a fresh match snapshot; codepoints is writable for its length.
    let result = unsafe {
        ghostty_grid_ref_graphemes(
            grid_ref,
            codepoints.as_mut_ptr(),
            codepoints.len(),
            &mut written,
        )
    };
    match NativeTerminalError::from_c_result(result, "ghostty_grid_ref_graphemes(SearchMatch)") {
        Ok(()) => Ok(written == 1),
        Err(NativeTerminalError::OutOfSpace) => Ok(false),
        Err(other) => Err(other),
    }
}

/// Search with libghostty-vt's `ghostty_search` and report matches the way `search_grid` does:
/// `(screen row, first column, last column)`, sorted, single-row only, rows capped at
/// `u16::MAX`. Rejects any query for which `ghostty_search_eligible` is false.
pub fn search_with_ghostty(
    handle: NonNull<GhosttyTerminalImpl>,
    query: &str,
    case_sensitive: bool,
) -> Result<Vec<(u16, u16, u16)>, NativeTerminalError> {
    if !ghostty_search_eligible(query, case_sensitive) {
        return Err(NativeTerminalError::InvalidValue(format!(
            "query {query:?} (case_sensitive={case_sensitive}) must use search_grid"
        )));
    }
    let search = SearchHandle::new(handle)?;
    search.run(query)?;
    let selections = search.matches()?;
    let mut matches = Vec::with_capacity(selections.len());
    for selection in &selections {
        let start = match_screen_point(handle, &selection.start)?;
        let end = match_screen_point(handle, &selection.end)?;
        // A match across a soft wrap spans rows; search_grid scans one row at a time.
        if start.y != end.y {
            continue;
        }
        let Ok(row) = u16::try_from(start.y) else {
            continue;
        };
        if !holds_one_codepoint(&selection.start)? || !holds_one_codepoint(&selection.end)? {
            continue;
        }
        matches.push((row, start.x.min(end.x), start.x.max(end.x)));
    }
    matches.sort_unstable();
    Ok(matches)
}
