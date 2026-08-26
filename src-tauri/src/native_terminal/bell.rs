//! Safe bell and title event observation and callback management.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::sys::types::GhosttyTerminal;

/// Shared thread-safe event context attached via GHOSTTY_TERMINAL_OPT_USERDATA.
pub struct TerminalContext {
    pub bell_counter: AtomicU64,
    pub title_updated: AtomicBool,
}

/// Safe C callback for terminal BEL character (0x07) events.
///
/// Prevents unwinding across the foreign ABI. `AtomicU64::fetch_add` contains
/// no branching or panics and makes no re-entrant terminal calls.
pub unsafe extern "C" fn terminal_bell_callback(_terminal: GhosttyTerminal, userdata: *mut c_void) {
    if userdata.is_null() {
        return;
    }
    // SAFETY: FFI pointer dereference invariant.
    // userdata is guaranteed by NativeTerminal to point to a valid pinned heap TerminalContext.
    let ctx = unsafe { &*(userdata as *const TerminalContext) };
    ctx.bell_counter.fetch_add(1, Ordering::Relaxed);
}

/// Safe C callback for OSC title changed events.
///
/// Prevents unwinding across the foreign ABI. Marks pending title update without
/// re-entrant terminal calls per terminal.h specifications.
pub unsafe extern "C" fn terminal_title_changed_callback(
    _terminal: GhosttyTerminal,
    userdata: *mut c_void,
) {
    if userdata.is_null() {
        return;
    }
    // SAFETY: FFI pointer dereference invariant.
    // userdata is guaranteed by NativeTerminal to point to a valid pinned heap TerminalContext.
    let ctx = unsafe { &*(userdata as *const TerminalContext) };
    ctx.title_updated.store(true, Ordering::Release);
}
