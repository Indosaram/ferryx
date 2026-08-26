//! Safe viewport scrolling and scrollback state.

use std::ffi::c_void;
use std::ptr::NonNull;

use super::error::NativeTerminalError;
use super::sys::ffi::{ghostty_terminal_get, ghostty_terminal_scroll_viewport};
use super::sys::types::{
    GhosttyTerminalImpl, GhosttyTerminalScrollViewport, GhosttyTerminalScrollViewportValue,
    GhosttyTerminalScrollbar, GHOSTTY_SCROLL_VIEWPORT_BOTTOM, GHOSTTY_SCROLL_VIEWPORT_DELTA,
    GHOSTTY_SCROLL_VIEWPORT_ROW, GHOSTTY_SCROLL_VIEWPORT_TOP, GHOSTTY_TERMINAL_DATA_SCROLLBAR,
};

/// Requested viewport movement.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ScrollViewport {
    Top,
    Bottom,
    Delta(isize),
    Row(usize),
}

/// Scrollbar dimensions in full-screen row coordinates.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct ScrollbarState {
    pub total: u64,
    pub offset: u64,
    pub len: u64,
}

pub fn scroll_viewport(handle: NonNull<GhosttyTerminalImpl>, behavior: ScrollViewport) {
    let behavior = match behavior {
        ScrollViewport::Top => GhosttyTerminalScrollViewport {
            tag: GHOSTTY_SCROLL_VIEWPORT_TOP,
            value: GhosttyTerminalScrollViewportValue { _padding: [0; 2] },
        },
        ScrollViewport::Bottom => GhosttyTerminalScrollViewport {
            tag: GHOSTTY_SCROLL_VIEWPORT_BOTTOM,
            value: GhosttyTerminalScrollViewportValue { _padding: [0; 2] },
        },
        ScrollViewport::Delta(delta) => GhosttyTerminalScrollViewport {
            tag: GHOSTTY_SCROLL_VIEWPORT_DELTA,
            value: GhosttyTerminalScrollViewportValue { delta },
        },
        ScrollViewport::Row(row) => GhosttyTerminalScrollViewport {
            tag: GHOSTTY_SCROLL_VIEWPORT_ROW,
            value: GhosttyTerminalScrollViewportValue { row },
        },
    };

    // SAFETY: Category: Foreign State Mutation.
    // Invariant: handle is a live terminal and behavior exactly matches the tagged C union layout.
    unsafe { ghostty_terminal_scroll_viewport(handle.as_ptr(), behavior) };
}

pub fn query_scrollbar(
    handle: NonNull<GhosttyTerminalImpl>,
) -> Result<ScrollbarState, NativeTerminalError> {
    let mut raw = GhosttyTerminalScrollbar::default();
    // SAFETY: Category: Foreign Data Extraction.
    // Invariant: &mut raw points to writable stack storage matching GhosttyTerminalScrollbar.
    let result = unsafe {
        ghostty_terminal_get(
            handle.as_ptr(),
            GHOSTTY_TERMINAL_DATA_SCROLLBAR,
            &mut raw as *mut GhosttyTerminalScrollbar as *mut c_void,
        )
    };
    NativeTerminalError::from_c_result(result, "ghostty_terminal_get(Scrollbar)")?;
    Ok(ScrollbarState {
        total: raw.total,
        offset: raw.offset,
        len: raw.len,
    })
}
