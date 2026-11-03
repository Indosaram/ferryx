//! Safe bell, title, and pty write event observation and callback management.

use parking_lot::Mutex;
use std::ffi::{c_int, c_void};
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, Ordering};

use super::error::NativeTerminalError;
use super::sys::constants::{GHOSTTY_COLOR_SCHEME_DARK, GHOSTTY_COLOR_SCHEME_LIGHT};
use super::sys::ffi::ghostty_color_scheme_report_encode;
use super::sys::types::GhosttyTerminal;
use crate::terminal::TerminalAppearance;

/// Start of the OSC 7501 support query reply (`OSC 7501 ; ? : states=...`).
pub(crate) const PROGRAM_STATUS_REPLY_PREFIX: &[u8] = b"\x1b]7501;?";

#[cfg(test)]
#[path = "color_scheme_tests.rs"]
mod color_scheme_tests;

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
    pub cell_width: AtomicU32,
    pub cell_height: AtomicU32,
    pub rows: AtomicU16,
    pub cols: AtomicU16,
    /// Receiver for OSC 7501 / prompt-start / reset events; `None` until opted in.
    pub program_status_sink: Mutex<Option<super::program_status::ProgramStatusSink>>,
    /// When set, the program status callbacks drop every event (seed or gap replay).
    pub program_status_muted: AtomicBool,
    /// While PTY writes are suppressed, still keep OSC 7501 support query replies, in
    /// `program_status_replies`, for the owner to write itself. Every other write stays dropped.
    pub program_status_reply_capture: AtomicBool,
    pub program_status_replies: Mutex<Vec<Vec<u8>>>,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct GhosttySizeReportSize {
    pub rows: u16,
    pub columns: u16,
    pub cell_width: u32,
    pub cell_height: u32,
}

/// Safe C callback for terminal size query events (CSI 14/16/18 t and mode 2048).
pub unsafe extern "C" fn terminal_size_callback(
    _terminal: GhosttyTerminal,
    userdata: *mut c_void,
    out_size: *mut GhosttySizeReportSize,
) -> bool {
    if userdata.is_null() || out_size.is_null() {
        return false;
    }
    let ctx = unsafe { &*(userdata as *const TerminalContext) };
    let cell_width = ctx.cell_width.load(Ordering::Acquire);
    let cell_height = ctx.cell_height.load(Ordering::Acquire);
    let rows = ctx.rows.load(Ordering::Acquire);
    let columns = ctx.cols.load(Ordering::Acquire);
    if cell_width == 0 || cell_height == 0 || rows == 0 || columns == 0 {
        return false;
    }
    unsafe {
        *out_size = GhosttySizeReportSize {
            rows,
            columns,
            cell_width,
            cell_height,
        };
    }
    true
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
    let bytes = unsafe { std::slice::from_raw_parts(data, len) }.to_vec();
    queue_pty_write(ctx, bytes);
}

/// Queues bytes for the PTY on the path VT query replies use, tagged with the remote generation.
///
/// Returns false when nothing was queued: writes are suppressed (history replay, Lagged
/// recovery) or the bounded fallback buffer is full or contended.
pub(crate) fn queue_pty_write(ctx: &TerminalContext, bytes: Vec<u8>) -> bool {
    if ctx.pty_writes_suppressed.load(Ordering::Acquire) {
        if ctx.program_status_reply_capture.load(Ordering::Acquire)
            && bytes.starts_with(PROGRAM_STATUS_REPLY_PREFIX)
        {
            ctx.program_status_replies.lock().push(bytes);
            return true;
        }
        return false;
    }
    let generation = *ctx.remote_generation.lock();
    let record = PtyWriteRecord {
        generation,
        data: bytes,
    };
    if let Some(guard) = ctx.pty_write_tx.try_lock() {
        if let Some(tx) = guard.as_ref() {
            return tx.send(record).is_ok();
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
            return true;
        }
    }
    false
}

/// Maps the app appearance onto libghostty-vt's `GhosttyColorScheme`.
pub(crate) fn color_scheme_for(appearance: TerminalAppearance) -> c_int {
    match appearance {
        TerminalAppearance::Light => GHOSTTY_COLOR_SCHEME_LIGHT,
        TerminalAppearance::Dark => GHOSTTY_COLOR_SCHEME_DARK,
    }
}

/// Safe C callback answering color scheme queries (CSI ? 996 n) with the live app appearance.
///
/// Prevents unwinding across the foreign ABI and makes no re-entrant terminal calls.
pub unsafe extern "C" fn terminal_color_scheme_callback(
    _terminal: GhosttyTerminal,
    _userdata: *mut c_void,
    out_scheme: *mut c_int,
) -> bool {
    if out_scheme.is_null() {
        return false;
    }
    let scheme = color_scheme_for(crate::terminal::terminal_appearance());
    // SAFETY: libghostty-vt passes a valid, writable GhosttyColorScheme (C enum, int-sized).
    unsafe { *out_scheme = scheme };
    true
}

/// Encodes the DEC 2031 color scheme report (`ESC [ ? 997 ; 1|2 n`) with libghostty-vt.
pub(crate) fn encode_color_scheme_report(
    appearance: TerminalAppearance,
) -> Result<Vec<u8>, NativeTerminalError> {
    let mut buf = [0u8; 16];
    let mut written = 0usize;
    // SAFETY: buf is writable for buf.len() bytes and written points to a stack usize; the
    // library writes at most buf_len bytes and reports the count through out_written.
    let result = unsafe {
        ghostty_color_scheme_report_encode(
            color_scheme_for(appearance),
            buf.as_mut_ptr().cast(),
            buf.len(),
            &mut written,
        )
    };
    NativeTerminalError::from_c_result(result, "ghostty_color_scheme_report_encode")?;
    Ok(buf[..written.min(buf.len())].to_vec())
}
