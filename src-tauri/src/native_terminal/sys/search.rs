//! `ghostty/vt/search.h` bindings used by the `ghostty_search` evaluation path.

use std::ffi::{c_int, c_void};

use super::types::{GhosttyAllocator, GhosttySelection, GhosttyTerminal};

/// Opaque foreign search handle matching `include/ghostty/vt/types.h`.
#[repr(C)]
pub struct GhosttySearchImpl {
    _private: [u8; 0],
}
pub type GhosttySearch = *mut GhosttySearchImpl;

/// Caller-provided selection buffer matching `include/ghostty/vt/selection.h`.
#[repr(C)]
#[derive(Debug)]
pub struct GhosttySelectionBuffer {
    pub ptr: *mut GhosttySelection,
    pub cap: usize,
    pub len: usize,
}

// ptr(8) + cap(8) + len(8) on 64-bit targets.
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<GhosttySelectionBuffer>() == 24);

extern "C" {
    pub fn ghostty_search_new(
        allocator: *const GhosttyAllocator,
        out_search: *mut GhosttySearch,
        terminal: GhosttyTerminal,
    ) -> c_int;
    pub fn ghostty_search_free(search: GhosttySearch);
    pub fn ghostty_search_run(search: GhosttySearch) -> c_int;
    pub fn ghostty_search_set(search: GhosttySearch, option: c_int, value: *const c_void) -> c_int;
    pub fn ghostty_search_get(search: GhosttySearch, data: c_int, value: *mut c_void) -> c_int;
}
