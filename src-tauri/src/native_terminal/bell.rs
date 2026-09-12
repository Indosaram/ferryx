//! Safe bell, title, and pty write event observation and callback management.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use parking_lot::Mutex;

use super::sys::types::GhosttyTerminal;

/// A pending or in-flight PTY write record holding its observed generation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PtyWriteRecord {
    pub generation: Option<u64>,
    pub data: Vec<u8>,
}

/// Shared thread-safe event context attached via GHOSTTY_TERMINAL_OPT_USERDATA.
pub struct TerminalContext {
    pub bell_counter: AtomicU64,
    pub title_updated: AtomicBool,
    pub pty_writes_suppressed: AtomicBool,
    pub remote_generation: Mutex<Option<u64>>,
    pub write_pty_buffer: Mutex<Vec<PtyWriteRecord>>,
    pub pty_write_tx: Mutex<Option<tokio::sync::mpsc::UnboundedSender<PtyWriteRecord>>>,
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

/// Safe C callback for VT query and mode reports written back to the PTY (DSR CPR, DA, etc.).
///
/// Prevents unwinding across the foreign ABI.
pub unsafe extern "C" fn terminal_write_pty_callback(
    _terminal: GhosttyTerminal,
    userdata: *mut c_void,
    data: *const u8,
    len: usize,
) {
    if userdata.is_null() || data.is_null() || len == 0 {
        return;
    }
    // SAFETY: FFI pointer dereference invariant.
    // userdata is guaranteed by NativeTerminal to point to a valid pinned heap TerminalContext.
    let ctx = unsafe { &*(userdata as *const TerminalContext) };
    if ctx.pty_writes_suppressed.load(Ordering::Acquire) {
        return;
    }
    let generation = *ctx.remote_generation.lock();
    let bytes = unsafe { std::slice::from_raw_parts(data, len) }.to_vec();
    let record = PtyWriteRecord {
        generation,
        data: bytes,
    };
    if let Some(guard) = ctx.pty_write_tx.try_lock() {
        if let Some(tx) = guard.as_ref() {
            let _ = tx.send(record);
            return;
        }
    }
    if let Some(mut guard) = ctx.write_pty_buffer.try_lock() {
        // Bound the buffer: observer terminals with no PTY writer registered (e.g.
        // RemoteTerminalMirror) must not accumulate VT responses without limit.
        // Cap by whole records to avoid truncating escape sequences.
        const MAX_WRITE_PTY_BUFFER: usize = 16 * 1024;
        let current_bytes: usize = guard.iter().map(|r| r.data.len()).sum();
        if current_bytes + record.data.len() <= MAX_WRITE_PTY_BUFFER {
            guard.push(record);
        }
    }
}
