//! Opt-in OSC 7501 program status, OSC 133 prompt start and full reset callbacks.
//!
//! Nothing here is registered by default: a terminal without
//! `NativeTerminal::enable_program_status` drops every report and leaves the
//! `OSC 7501 ; ?` support query unanswered, so at most one engine per PTY claims support.

use std::ffi::{c_int, c_void};
use std::ptr::NonNull;
use std::sync::atomic::Ordering;

use super::bell::TerminalContext;
use super::error::NativeTerminalError;
use super::sys::constants::{
    GHOSTTY_PROGRAM_STATUS_KIND_AUTH, GHOSTTY_PROGRAM_STATUS_KIND_PERMISSION,
    GHOSTTY_PROGRAM_STATUS_KIND_QUESTION, GHOSTTY_PROGRAM_STATUS_STATE_BLOCKED,
    GHOSTTY_PROGRAM_STATUS_STATE_CLEAR, GHOSTTY_PROGRAM_STATUS_STATE_DONE,
    GHOSTTY_PROGRAM_STATUS_STATE_ERROR, GHOSTTY_PROGRAM_STATUS_STATE_IDLE,
    GHOSTTY_PROGRAM_STATUS_STATE_WORKING, GHOSTTY_SEMANTIC_PROMPT_PROMPT_CONTINUATION,
    GHOSTTY_SEMANTIC_PROMPT_PROMPT_RIGHT, GHOSTTY_SEMANTIC_PROMPT_PROMPT_SECONDARY,
    GHOSTTY_SEMANTIC_PROMPT_PROMPT_START, GHOSTTY_TERMINAL_OPT_PROGRAM_STATUS,
    GHOSTTY_TERMINAL_OPT_RESET, GHOSTTY_TERMINAL_OPT_SEMANTIC_PROMPT,
};
use super::sys::ffi::ghostty_terminal_set;
use super::sys::types::{
    GhosttyString, GhosttyTerminal, GhosttyTerminalImpl, GhosttyTerminalProgramStatus,
    GhosttyTerminalSemanticPrompt,
};

/// What a program says it is doing (`GhosttyProgramStatusState`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgramStatusState {
    Idle,
    Working,
    Done,
    Blocked,
    Error,
    /// Not a real state: remove the record with this id and every record beneath it; an empty
    /// id removes every record.
    Clear,
}

/// What a blocked program needs from the user (`GhosttyProgramStatusKind` without `NONE`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgramStatusKind {
    Permission,
    Question,
    Auth,
}

/// An owned copy of one OSC 7501 report. Text the program did not send is empty.
///
/// `title` and `message` are decoded and control-free but still untrusted program text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgramStatusReport {
    pub state: ProgramStatusState,
    /// Only set for `Blocked`, and only when the program sent a known kind.
    pub kind: Option<ProgramStatusKind>,
    /// 0 through 100; `None` when the program sent no (valid) percentage.
    pub progress: Option<u8>,
    pub id: String,
    pub app: String,
    pub title: String,
    pub message: String,
}

/// Which prompt an OSC 133 prompt start begins (`GhosttySemanticPromptPromptKind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptKind {
    Primary,
    Right,
    Continuation,
    Secondary,
}

/// One event delivered to the sink passed to `NativeTerminal::enable_program_status`.
///
/// A full reset (RIS, `ESC c`) delivers a `Report` with state `Clear` and an empty id, then
/// `Reset`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProgramStatusEvent {
    Report(ProgramStatusReport),
    PromptStart(PromptKind),
    Reset,
}

/// Receiver of program status events. Called synchronously from inside `feed`.
pub type ProgramStatusSink = Box<dyn FnMut(ProgramStatusEvent) + Send>;

const CALLBACK_OPTIONS: [c_int; 3] = [
    GHOSTTY_TERMINAL_OPT_PROGRAM_STATUS,
    GHOSTTY_TERMINAL_OPT_SEMANTIC_PROMPT,
    GHOSTTY_TERMINAL_OPT_RESET,
];

/// Stores `sink` and registers the program status, semantic prompt and reset callbacks.
///
/// On failure every callback is unregistered again and the sink is dropped.
pub(super) fn register(
    handle: NonNull<GhosttyTerminalImpl>,
    ctx: &TerminalContext,
    sink: ProgramStatusSink,
) -> Result<(), NativeTerminalError> {
    *ctx.program_status_sink.lock() = Some(sink);
    let callbacks: [(c_int, *const c_void, &'static str); 3] = [
        (
            GHOSTTY_TERMINAL_OPT_SEMANTIC_PROMPT,
            terminal_semantic_prompt_callback as *const c_void,
            "set(OPT_SEMANTIC_PROMPT)",
        ),
        (
            GHOSTTY_TERMINAL_OPT_RESET,
            terminal_reset_callback as *const c_void,
            "set(OPT_RESET)",
        ),
        (
            GHOSTTY_TERMINAL_OPT_PROGRAM_STATUS,
            terminal_program_status_callback as *const c_void,
            "set(OPT_PROGRAM_STATUS)",
        ),
    ];
    for (option, callback, context) in callbacks {
        // SAFETY: Category: Foreign Option Configuration.
        // Invariant: handle is a live terminal; callback is an extern "C" fn matching the
        // option's documented function type; userdata already points to the pinned context.
        let result = unsafe { ghostty_terminal_set(handle.as_ptr(), option, callback) };
        if let Err(error) = NativeTerminalError::from_c_result(result, context) {
            unregister(handle);
            *ctx.program_status_sink.lock() = None;
            return Err(error);
        }
    }
    Ok(())
}

/// Unregisters every callback this module installs. Harmless when none were installed.
pub(super) fn unregister(handle: NonNull<GhosttyTerminalImpl>) {
    for option in CALLBACK_OPTIONS {
        // SAFETY: Category: Foreign Option Configuration.
        // Invariant: handle is a live terminal; NULL disables the callback.
        let _ = unsafe { ghostty_terminal_set(handle.as_ptr(), option, std::ptr::null()) };
    }
}

/// Copies a borrowed libghostty string into an owned `String`.
///
/// # Safety
/// `s.ptr` must be valid for `s.len` bytes for the duration of this call.
unsafe fn owned_string(s: GhosttyString) -> String {
    if s.ptr.is_null() || s.len == 0 {
        return String::new();
    }
    // SAFETY: Guaranteed by the caller: the callback's strings are valid during the call.
    let bytes = unsafe { std::slice::from_raw_parts(s.ptr, s.len) };
    String::from_utf8_lossy(bytes).into_owned()
}

fn state_from_c(state: c_int) -> Option<ProgramStatusState> {
    match state {
        GHOSTTY_PROGRAM_STATUS_STATE_IDLE => Some(ProgramStatusState::Idle),
        GHOSTTY_PROGRAM_STATUS_STATE_WORKING => Some(ProgramStatusState::Working),
        GHOSTTY_PROGRAM_STATUS_STATE_DONE => Some(ProgramStatusState::Done),
        GHOSTTY_PROGRAM_STATUS_STATE_BLOCKED => Some(ProgramStatusState::Blocked),
        GHOSTTY_PROGRAM_STATUS_STATE_ERROR => Some(ProgramStatusState::Error),
        GHOSTTY_PROGRAM_STATUS_STATE_CLEAR => Some(ProgramStatusState::Clear),
        _ => None,
    }
}

fn kind_from_c(kind: c_int) -> Option<ProgramStatusKind> {
    match kind {
        GHOSTTY_PROGRAM_STATUS_KIND_PERMISSION => Some(ProgramStatusKind::Permission),
        GHOSTTY_PROGRAM_STATUS_KIND_QUESTION => Some(ProgramStatusKind::Question),
        GHOSTTY_PROGRAM_STATUS_KIND_AUTH => Some(ProgramStatusKind::Auth),
        _ => None,
    }
}

fn prompt_kind_from_c(kind: c_int) -> PromptKind {
    match kind {
        GHOSTTY_SEMANTIC_PROMPT_PROMPT_RIGHT => PromptKind::Right,
        GHOSTTY_SEMANTIC_PROMPT_PROMPT_CONTINUATION => PromptKind::Continuation,
        GHOSTTY_SEMANTIC_PROMPT_PROMPT_SECONDARY => PromptKind::Secondary,
        _ => PromptKind::Primary,
    }
}

/// Returns the context unless userdata is missing or delivery is muted.
///
/// # Safety
/// `userdata` must be null or point to the live `TerminalContext` set as OPT_USERDATA.
unsafe fn live_context<'a>(userdata: *mut c_void) -> Option<&'a TerminalContext> {
    if userdata.is_null() {
        return None;
    }
    // SAFETY: Guaranteed by the caller; the context outlives every registered callback
    // because teardown unregisters them before clearing userdata.
    let ctx = unsafe { &*(userdata as *const TerminalContext) };
    if ctx.program_status_muted.load(Ordering::Acquire) {
        return None;
    }
    Some(ctx)
}

/// Hands `event` to the sink without letting a panic unwind across the C ABI.
fn deliver(ctx: &TerminalContext, event: ProgramStatusEvent) {
    let mut guard = ctx.program_status_sink.lock();
    let Some(sink) = guard.as_mut() else {
        return;
    };
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sink(event)));
    if outcome.is_err() {
        tracing::warn!("program status sink panicked; event dropped");
    }
}

/// C callback for OSC 7501 reports. Copies every string before returning.
unsafe extern "C" fn terminal_program_status_callback(
    _terminal: GhosttyTerminal,
    userdata: *mut c_void,
    report: *const GhosttyTerminalProgramStatus,
) {
    if report.is_null() {
        return;
    }
    // SAFETY: userdata is the pinned TerminalContext registered by create_native_terminal.
    let Some(ctx) = (unsafe { live_context(userdata) }) else {
        return;
    };
    // SAFETY: libghostty passes a valid report that lives for the duration of this call.
    let raw = unsafe { &*report };
    if raw.size < std::mem::size_of::<GhosttyTerminalProgramStatus>() {
        return;
    }
    let Some(state) = state_from_c(raw.state) else {
        return;
    };
    // SAFETY: The report's strings are valid for the duration of this call.
    let (id, app, title, message) = unsafe {
        (
            owned_string(raw.id),
            owned_string(raw.app),
            owned_string(raw.title),
            owned_string(raw.message),
        )
    };
    let progress = u8::try_from(raw.progress).ok().filter(|value| *value <= 100);
    deliver(
        ctx,
        ProgramStatusEvent::Report(ProgramStatusReport {
            state,
            kind: kind_from_c(raw.kind),
            progress,
            id,
            app,
            title,
            message,
        }),
    );
}

/// C callback for OSC 133 shell integration events; only prompt starts are delivered.
unsafe extern "C" fn terminal_semantic_prompt_callback(
    _terminal: GhosttyTerminal,
    userdata: *mut c_void,
    event: *const GhosttyTerminalSemanticPrompt,
) {
    if event.is_null() {
        return;
    }
    // SAFETY: userdata is the pinned TerminalContext registered by create_native_terminal.
    let Some(ctx) = (unsafe { live_context(userdata) }) else {
        return;
    };
    // SAFETY: libghostty passes a valid event that lives for the duration of this call.
    let raw = unsafe { &*event };
    if raw.size < std::mem::size_of::<GhosttyTerminalSemanticPrompt>()
        || raw.kind != GHOSTTY_SEMANTIC_PROMPT_PROMPT_START
    {
        return;
    }
    deliver(
        ctx,
        ProgramStatusEvent::PromptStart(prompt_kind_from_c(raw.prompt_kind)),
    );
}

/// C callback for a full reset (RIS, `ESC c`), called after the clear-all report.
unsafe extern "C" fn terminal_reset_callback(_terminal: GhosttyTerminal, userdata: *mut c_void) {
    // SAFETY: userdata is the pinned TerminalContext registered by create_native_terminal.
    let Some(ctx) = (unsafe { live_context(userdata) }) else {
        return;
    };
    deliver(ctx, ProgramStatusEvent::Reset);
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::{
        ProgramStatusEvent, ProgramStatusKind, ProgramStatusReport, ProgramStatusState,
        PromptKind,
    };
    use crate::native_terminal::engine::TerminalEngine;
    use crate::native_terminal::NativeTerminal;

    const QUERY: &[u8] = b"\x1b]7501;?\x1b\\";
    const QUERY_REPLY: &[u8] = b"\x1b]7501;?:states=idle,working,done,blocked,error\
:kinds=permission,question,auth\x1b\\";
    /// `msg` is base64 for "Running tests".
    const WORKING_REPORT: &[u8] =
        b"\x1b]7501;state=working:app=claude:msg=UnVubmluZyB0ZXN0cw==\x1b\\";

    fn enabled_terminal() -> (NativeTerminal, Arc<Mutex<Vec<ProgramStatusEvent>>>) {
        let mut terminal = NativeTerminal::new(80, 24).unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink_events = Arc::clone(&events);
        terminal
            .enable_program_status(move |event| sink_events.lock().unwrap().push(event))
            .unwrap();
        (terminal, events)
    }

    fn take(events: &Mutex<Vec<ProgramStatusEvent>>) -> Vec<ProgramStatusEvent> {
        std::mem::take(&mut *events.lock().unwrap())
    }

    fn working_report() -> ProgramStatusEvent {
        ProgramStatusEvent::Report(ProgramStatusReport {
            state: ProgramStatusState::Working,
            kind: None,
            progress: None,
            id: String::new(),
            app: "claude".to_string(),
            title: String::new(),
            message: "Running tests".to_string(),
        })
    }

    #[test]
    fn report_is_delivered_as_owned_decoded_values() {
        let (mut terminal, events) = enabled_terminal();
        terminal.feed(WORKING_REPORT).unwrap();
        assert_eq!(take(&events), vec![working_report()]);

        terminal
            .feed(
                b"\x1b]7501;state=blocked:kind=permission:progress=42:id=tf/plan:app=terraform\x1b\\",
            )
            .unwrap();
        assert_eq!(
            take(&events),
            vec![ProgramStatusEvent::Report(ProgramStatusReport {
                state: ProgramStatusState::Blocked,
                kind: Some(ProgramStatusKind::Permission),
                progress: Some(42),
                id: "tf/plan".to_string(),
                app: "terraform".to_string(),
                title: String::new(),
                message: String::new(),
            })]
        );
    }

    #[test]
    fn disabled_by_default_query_gets_no_reply() {
        let mut terminal = NativeTerminal::new(80, 24).unwrap();
        terminal.feed(QUERY).unwrap();
        terminal.feed(WORKING_REPORT).unwrap();
        assert!(
            terminal.buffered_pty_writes().is_empty(),
            "a terminal that did not opt in must not claim OSC 7501 support"
        );
    }

    #[test]
    fn enabled_query_gets_exactly_one_reply() {
        let (mut terminal, events) = enabled_terminal();
        terminal.feed(QUERY).unwrap();
        let writes = terminal.buffered_pty_writes();
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].data, QUERY_REPLY);
        assert!(take(&events).is_empty(), "the query itself is not a report");
    }

    #[test]
    fn muted_terminal_drops_reports_until_unmuted() {
        let (mut terminal, events) = enabled_terminal();
        terminal.set_program_status_muted(true);
        terminal.feed(WORKING_REPORT).unwrap();
        terminal.feed(b"\x1b]133;A\x07\x1bc").unwrap();
        assert!(take(&events).is_empty());

        terminal.set_program_status_muted(false);
        terminal.feed(WORKING_REPORT).unwrap();
        assert_eq!(take(&events), vec![working_report()]);
    }

    #[test]
    fn full_reset_delivers_clear_all_then_reset() {
        let (mut terminal, events) = enabled_terminal();
        terminal.feed(b"\x1bc").unwrap();
        assert_eq!(
            take(&events),
            vec![
                ProgramStatusEvent::Report(ProgramStatusReport {
                    state: ProgramStatusState::Clear,
                    kind: None,
                    progress: None,
                    id: String::new(),
                    app: String::new(),
                    title: String::new(),
                    message: String::new(),
                }),
                ProgramStatusEvent::Reset,
            ]
        );
    }

    #[test]
    fn osc_133_prompt_start_is_delivered() {
        let (mut terminal, events) = enabled_terminal();
        terminal.feed(b"\x1b]133;A\x07").unwrap();
        assert_eq!(
            take(&events),
            vec![ProgramStatusEvent::PromptStart(PromptKind::Primary)]
        );
    }
}
