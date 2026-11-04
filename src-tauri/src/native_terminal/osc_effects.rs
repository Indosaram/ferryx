//! Opt-in daemon-only OSC side effects. All FFI payloads are copied during the callback.
use std::ffi::{c_int, c_void};
use std::ptr::NonNull;
use std::sync::atomic::Ordering;

use serde::{Deserialize, Serialize};

use super::bell::TerminalContext;
use super::sys::constants::*;
use super::sys::ffi::{ghostty_terminal_get, ghostty_terminal_set};
use super::sys::types::{GhosttyString, GhosttyTerminal, GhosttyTerminalImpl};
use super::NativeTerminalError;

pub const MAX_CLIPBOARD_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum OscEffect {
    Notify { title: String, body: String },
    Clipboard { text: String },
}

#[derive(Default)]
pub(super) struct Effects {
    pub enabled: bool,
    pub muted: bool,
    pub pending: Vec<OscEffect>,
    pub last_effect: Option<std::time::Instant>,
    pub cwd: Option<String>,
}

#[repr(C)]
struct Notification {
    size: usize,
    title: GhosttyString,
    body: GhosttyString,
}

#[repr(C)]
struct Content {
    mime: GhosttyString,
    data: GhosttyString,
}

#[repr(C)]
struct ClipboardWrite {
    size: usize,
    location: c_int,
    contents: *const Content,
    contents_len: usize,
    name: GhosttyString,
    granted: bool,
    can_remember: bool,
    ctx: *const c_void,
    reply: Option<unsafe extern "C" fn(*const ClipboardWrite, *const c_void)>,
}

pub fn sanitize(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() && !matches!(c,
            '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' |
            '\u{2066}'..='\u{2069}' | '\u{2028}' | '\u{2029}' | '\u{feff}'))
        .take(256)
        .collect()
}

// SAFETY: strings supplied by libghostty are borrowed for the callback duration.
unsafe fn copy_string(raw: GhosttyString) -> String {
    if raw.len == 0 || raw.ptr.is_null() { return String::new(); }
    String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(raw.ptr, raw.len) }).into_owned()
}

fn deliver(ctx: &TerminalContext, effect: OscEffect) {
    let mut effects = ctx.osc_effects.lock();
    if !effects.enabled || effects.muted { return; }
    let now = std::time::Instant::now();
    if effects.last_effect.is_some_and(|last| now.duration_since(last).as_secs() < 2) { return; }
    effects.last_effect = Some(now);
    effects.pending.push(effect);
}

unsafe extern "C" fn notification(_terminal: GhosttyTerminal, userdata: *mut c_void, raw: *const Notification) {
    if userdata.is_null() || raw.is_null() { return; }
    // SAFETY: userdata is the pinned context, and raw is borrowed for this call.
    let (ctx, raw) = unsafe { (&*(userdata as *const TerminalContext), &*raw) };
    if raw.size < std::mem::size_of::<Notification>() { return; }
    let (title, body) = unsafe { (copy_string(raw.title), copy_string(raw.body)) };
    deliver(ctx, OscEffect::Notify { title: sanitize(&title), body: sanitize(&body) });
}

unsafe extern "C" fn clipboard(_terminal: GhosttyTerminal, userdata: *mut c_void, raw: *const ClipboardWrite) {
    if userdata.is_null() || raw.is_null() { return; }
    // SAFETY: libghostty owns the request and its representations until this callback returns.
    let (ctx, raw) = unsafe { (&*(userdata as *const TerminalContext), &*raw) };
    if raw.size < std::mem::size_of::<ClipboardWrite>() || raw.location != 0 ||
        raw.contents_len != 1 || raw.contents.is_null() || !ctx.osc52_write.load(Ordering::Acquire) { return; }
    let content = unsafe { &*raw.contents };
    if content.data.len > MAX_CLIPBOARD_BYTES { return; }
    if unsafe { copy_string(content.mime) } != "text/plain" { return; }
    let text = unsafe { copy_string(content.data) };
    deliver(ctx, OscEffect::Clipboard { text: sanitize(&text) });
    // Returning without a reply denies protocol acknowledgements. OSC 52 has no reply;
    // never grant a remembered permission to OSC 5522 programs.
}

unsafe extern "C" fn deny_read(_terminal: GhosttyTerminal, _userdata: *mut c_void, _read: *const c_void) {
    // The documented default on return without replying is denial. No clipboard data is read.
}

unsafe extern "C" fn pwd(terminal: GhosttyTerminal, userdata: *mut c_void) {
    if userdata.is_null() { return; }
    let Some(handle) = NonNull::new(terminal) else { return; };
    // SAFETY: pinned context outlives all registered callbacks.
    let ctx = unsafe { &*(userdata as *const TerminalContext) };
    ctx.osc_effects.lock().cwd = cwd(handle);
}

pub(super) fn register(handle: NonNull<GhosttyTerminalImpl>) -> Result<(), NativeTerminalError> {
    let limit = MAX_CLIPBOARD_BYTES;
    let result = unsafe { ghostty_terminal_set(handle.as_ptr(), GHOSTTY_TERMINAL_OPT_CLIPBOARD_WRITE_MAX_BYTES, &limit as *const _ as *const c_void) };
    NativeTerminalError::from_c_result(result, "set clipboard limit")?;
    for (option, callback) in [
        (GHOSTTY_TERMINAL_OPT_PWD_CHANGED, pwd as *const c_void),
        (GHOSTTY_TERMINAL_OPT_DESKTOP_NOTIFICATION, notification as *const c_void),
        (GHOSTTY_TERMINAL_OPT_CLIPBOARD_WRITE, clipboard as *const c_void),
        (GHOSTTY_TERMINAL_OPT_CLIPBOARD_READ, deny_read as *const c_void),
    ] {
        // SAFETY: handle is live and the callback matches the vendored header's ABI.
        let result = unsafe { ghostty_terminal_set(handle.as_ptr(), option, callback) };
        if let Err(error) = NativeTerminalError::from_c_result(result, "set OSC callback") {
            unregister(handle);
            return Err(error);
        }
    }
    Ok(())
}

pub(super) fn unregister(handle: NonNull<GhosttyTerminalImpl>) {
    for option in [GHOSTTY_TERMINAL_OPT_PWD_CHANGED, GHOSTTY_TERMINAL_OPT_DESKTOP_NOTIFICATION, GHOSTTY_TERMINAL_OPT_CLIPBOARD_WRITE, GHOSTTY_TERMINAL_OPT_CLIPBOARD_READ] {
        unsafe { ghostty_terminal_set(handle.as_ptr(), option, std::ptr::null()); }
    }
}

/// Read parser-owned pwd even after a muted seed replay. Ghostty's VT formatter includes
/// pwd in handover seeds, so cwd does not require a second daemon state ledger.
pub(super) fn cwd(handle: NonNull<GhosttyTerminalImpl>) -> Option<String> {
    let mut raw = GhosttyString { ptr: std::ptr::null(), len: 0 };
    let result = unsafe {
        ghostty_terminal_get(handle.as_ptr(), GHOSTTY_TERMINAL_DATA_PWD, &mut raw as *mut _ as *mut c_void)
    };
    if result != GHOSTTY_SUCCESS { return None; }
    let text = unsafe { copy_string(raw) };
    parse_cwd(&text)
}

pub fn parse_cwd(text: &str) -> Option<String> {
    let path = if text.starts_with("file://") {
        let uri = url::Url::parse(text).ok()?;
        if uri.query().is_some() || uri.fragment().is_some() { return None; }
        let mut local = uri;
        local.set_host(None).ok()?;
        local.to_file_path().ok()?.to_string_lossy().into_owned()
    } else {
        text.to_string()
    };
    if path.is_empty() || path.chars().any(char::is_control) { return None; }
    // file://host is a shell assertion, not a network resource to open. Preserve the
    // local path; on Windows file:///C:/... is reported without the URI's leading slash.
    #[cfg(windows)]
    let path = if path.as_bytes().get(2) == Some(&b':') { path[1..].to_string() } else { path };
    std::path::Path::new(&path).is_absolute().then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_terminal::{NativeTerminal, TerminalEngine};
    use crate::terminal::screen_mirror::ScreenMirror;

    #[derive(Deserialize)]
    struct Case {
        name: String,
        input: String,
        #[serde(default)]
        enabled: bool,
        #[serde(default)]
        remote: bool,
        #[serde(default)]
        replay: bool,
        cwd: Option<String>,
        events: Vec<OscEffect>,
    }

    #[test]
    fn osc_expectation_table() {
        let cases: Vec<Case> = serde_json::from_str(include_str!("../../tests/fixtures/osc_expectations.json")).unwrap();
        for case in cases {
            let mut mirror = ScreenMirror::new(80, 24).unwrap();
            assert!(mirror.enable_program_status());
            mirror.set_osc52_write(case.enabled && !case.remote);
            if case.replay { mirror.feed_replay(case.input.as_bytes()); }
            else { mirror.feed(case.input.as_bytes()); }
            assert_eq!(mirror.take_osc_effects(), case.events, "{}", case.name);
            if let Some(cwd) = case.cwd {
                #[cfg(not(windows))]
                assert_eq!(mirror.osc_cwd().as_deref(), Some(cwd.as_str()), "{}", case.name);
                #[cfg(windows)]
                let _ = cwd;
            }
        }
    }

    #[test]
    fn desktop_engine_does_not_handle_effects() {
        let mut terminal = NativeTerminal::new(80, 24).unwrap();
        terminal.feed(b"\x1b]777;notify;Build;done\x07").unwrap();
        assert!(terminal.take_osc_effects().is_empty());
    }

    #[test]
    fn output_hub_checks_local_ownership_before_clipboard_delivery() {
        use std::sync::Arc;
        let hub = crate::terminal::TerminalOutputHub::default();
        let effects = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let sink = Arc::clone(&effects);
        hub.set_osc_effect_sink(Arc::new(move |id, effect| sink.lock().push((id.to_string(), effect))));
        hub.set_osc_local_session(Arc::new(|id| id == "local"));
        hub.set_osc52_write(true);
        let _local = hub.register_session("local");
        let _remote = hub.register_session("remote");
        assert!(hub.enable_program_status("local"));
        assert!(hub.enable_program_status("remote"));
        hub.publish("remote", b"\x1b]52;c;aGk=\x07".to_vec());
        assert!(effects.lock().is_empty());
        hub.publish("local", b"\x1b]52;c;aGk=\x07".to_vec());
        assert_eq!(*effects.lock(), vec![("local".into(), OscEffect::Clipboard { text: "hi".into() })]);
    }

    #[test]
    fn notification_rate_limit_and_owned_copy() {
        let mut terminal = NativeTerminal::new(80, 24).unwrap();
        terminal.enable_osc_effects().unwrap();
        let mut input = b"\x1b]777;notify;Build;done\x07".to_vec();
        terminal.feed(&input).unwrap();
        input.fill(b'x');
        terminal.feed(b"\x1b]9;second\x07").unwrap();
        assert_eq!(terminal.take_osc_effects(), vec![OscEffect::Notify { title: "Build".into(), body: "done".into() }]);
    }

    #[test]
    fn sanitizes_controls_bidi_and_character_bound() {
        assert_eq!(sanitize("a\u{202e}b\n\u{2066}c"), "abc");
        assert_eq!(sanitize(&"\u{4e2d}".repeat(300)).chars().count(), 256);
    }

    #[test]
    fn seed_replay_never_delivers_effects_but_preserves_pwd() {
        let mut mirror = ScreenMirror::new(80, 24).unwrap();
        mirror.feed_replay(b"\x1b]7;file:///tmp\x07");
        let seed = mirror.seed().unwrap();
        let mut restored = ScreenMirror::from_seed(&seed).unwrap();
        assert!(restored.enable_program_status());
        assert!(restored.take_osc_effects().is_empty());
        assert_eq!(restored.osc_cwd(), mirror.osc_cwd());
    }
}
