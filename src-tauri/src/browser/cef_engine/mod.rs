//! Chromium (CEF) engine for browser panes on Windows, macOS and Linux (X11/XWayland).
//!
//! Used once the engine is installed (Settings > Browser, see `engine_install`), or when
//! `FERRYX_CEF_ENGINE_DIR` names a local engine build. One engine process serves every tab of
//! one browser profile, so cookies, logins and extensions are shared by those tabs. Ferryx
//! drives it with the line protocol in `ferryx-browser-engine-protocol`: commands on the
//! engine's stdin, events on its stdout. Each tab is a Chrome-style window the engine keeps
//! over the tab's pane: a child window of the Ferryx window on Windows, a borderless window
//! ordered directly above it on macOS, a child X window of the Ferryx toplevel on Linux.
//! Closing stdin ends the engine.

use crate::browser::{BrowserError, LogicalRect};
pub use ferryx_browser_engine_protocol::Event;
use ferryx_browser_engine_protocol::{Command as EngineCommand, Rect};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows as platform;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as platform;

pub mod system;

pub use platform::ENGINE_EXE;
#[cfg(target_os = "windows")]
pub use windows::grant_sandbox_read;
#[cfg(target_os = "macos")]
pub use macos::window_number;
#[cfg(target_os = "linux")]
pub use linux::{parent_xid, WAYLAND_REASON};

/// A pane rectangle in the units the engine places windows in: pixels relative to the Ferryx
/// window on Windows, screen points from the top-left on macOS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicalRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl From<PhysicalRect> for Rect {
    fn from(rect: PhysicalRect) -> Self {
        Self { x: rect.x, y: rect.y, width: rect.width, height: rect.height }
    }
}

/// Receives an engine's events with the profile it serves. Runs on the engine's reader thread.
pub type EventSink = Arc<dyn Fn(&str, Event) + Send + Sync>;

struct Engine {
    child: Child,
    stdin: ChildStdin,
    /// The Ferryx window the engine's windows sit in (Windows) or above (macOS).
    parent: isize,
    sink: EventSink,
}

struct Tab {
    profile: String,
    /// The tab's native window once the engine reported it (Windows).
    handle: Option<isize>,
}

static ENGINES: LazyLock<Mutex<HashMap<String, Engine>>> = LazyLock::new(Default::default);
static TABS: LazyLock<Mutex<HashMap<String, Tab>>> = LazyLock::new(Default::default);

/// A DevTools call waiting for its answer: the profile whose engine runs it, and the waiter.
struct PendingDevtools {
    profile: String,
    reply: tokio::sync::oneshot::Sender<(bool, String)>,
}

static DEVTOOLS_CALLS: LazyLock<Mutex<HashMap<u32, PendingDevtools>>> = LazyLock::new(Default::default);
static NEXT_DEVTOOLS_ID: AtomicU32 = AtomicU32::new(1);

/// Ids at and above this belong to the engine's own vault DevTools calls on the same browser
/// (`engine/vault.rs`), so Ferryx's calls stay below it and never collide with them.
const ENGINE_OWN_DEVTOOLS_IDS: u32 = 1_000_000;

/// Next DevTools message id: positive and below [`ENGINE_OWN_DEVTOOLS_IDS`] (so inside `i32`,
/// as CEF's message ids are).
fn next_devtools_id() -> u32 {
    loop {
        let id = NEXT_DEVTOOLS_ID.fetch_add(1, Ordering::Relaxed) % ENGINE_OWN_DEVTOOLS_IDS;
        if id != 0 {
            return id;
        }
    }
}

/// The engine directory: a local build named by `FERRYX_CEF_ENGINE_DIR`, else the installed
/// engine under the app data dir. `None` keeps tabs on the system webview.
pub fn engine_dir(data_dir: &Path) -> Option<PathBuf> {
    let dir = match std::env::var_os("FERRYX_CEF_ENGINE_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => crate::browser::engine_install::install_dir(data_dir),
    };
    dir.join(ENGINE_EXE).is_file().then_some(dir)
}

pub fn is_cef_tab(browser_id: &str) -> bool {
    TABS.lock().contains_key(browser_id)
}

/// The ids of all open engine tabs.
pub fn tab_ids() -> Vec<String> {
    TABS.lock().keys().cloned().collect()
}

pub fn to_physical(bounds: &LogicalRect, scale: f64) -> PhysicalRect {
    PhysicalRect {
        x: (bounds.x * scale).round() as i32,
        y: (bounds.y * scale).round() as i32,
        width: (bounds.width * scale).round().max(0.0) as i32,
        height: (bounds.height * scale).round().max(0.0) as i32,
    }
}

fn send(profile: &str, command: &EngineCommand) -> bool {
    let result = {
        let mut engines = ENGINES.lock();
        let Some(engine) = engines.get_mut(profile) else {
            return false;
        };
        writeln!(engine.stdin, "{}", command.to_line())
            .and_then(|()| engine.stdin.flush())
            .map_err(|error| (error, engine.child.id()))
    };
    match result {
        Ok(()) => true,
        Err((error, pid)) => {
            // The engine's helper processes can hold its stdout open after it died, so the
            // reader may never see the end of it: a refused command is the engine's end too.
            tracing::warn!(%profile, %error, "CEF engine stopped taking commands");
            engine_gone(profile, pid);
            false
        }
    }
}

/// Forgets a dead engine (only the one with `pid`, not a successor) and reports its tabs closed.
fn engine_gone(profile: &str, pid: u32) {
    let engine = {
        let mut engines = ENGINES.lock();
        match engines.get(profile) {
            Some(engine) if engine.child.id() == pid => engines.remove(profile),
            _ => None,
        }
    };
    let Some(mut engine) = engine else {
        return;
    };
    let _ = engine.child.kill();
    let _ = engine.child.wait();
    // Dropping the waiters tells every DevTools call in flight that the engine is gone.
    DEVTOOLS_CALLS.lock().retain(|_, pending| pending.profile != profile);
    let orphaned: Vec<String> = {
        let mut tabs = TABS.lock();
        let ids: Vec<String> =
            tabs.iter().filter(|(_, tab)| tab.profile == profile).map(|(id, _)| id.clone()).collect();
        for id in &ids {
            tabs.remove(id);
        }
        ids
    };
    for tab in orphaned {
        (engine.sink)(profile, Event::Closed { tab });
    }
}

fn send_to_tab(browser_id: &str, command: EngineCommand) -> Result<(), BrowserError> {
    let profile = TABS
        .lock()
        .get(browser_id)
        .map(|tab| tab.profile.clone())
        .ok_or_else(|| BrowserError::NotFound(browser_id.to_string()))?;
    if send(&profile, &command) {
        Ok(())
    } else {
        Err(BrowserError::NavigationFailed("the Chromium engine is not running".into()))
    }
}

/// What starting a profile's engine needs.
pub struct EngineLaunch<'a> {
    pub engine_dir: &'a Path,
    /// The profile's Chromium user data; `None` keeps it in memory (private tabs).
    pub cache_dir: Option<&'a Path>,
    /// The Ferryx window: an HWND on Windows, a window number on macOS.
    pub parent: isize,
}

/// Opens a tab in its profile's engine, starting the engine first when it is not running.
/// Blocking: run it through `run_blocking`.
pub fn open(
    launch: &EngineLaunch<'_>,
    profile: &str,
    sink: EventSink,
    browser_id: &str,
    rect: PhysicalRect,
    url: &str,
    visible: bool,
) -> Result<(), BrowserError> {
    // An engine that died since its last command is replaced, not written to.
    let dead = ENGINES.lock().get_mut(profile).and_then(|engine| {
        matches!(engine.child.try_wait(), Ok(Some(_))).then(|| engine.child.id())
    });
    if let Some(pid) = dead {
        engine_gone(profile, pid);
    }
    if !ENGINES.lock().contains_key(profile) {
        let failed = |error: std::io::Error| BrowserError::CreateFailed(format!("CEF engine: {error}"));
        let mut command = Command::new(launch.engine_dir.join(ENGINE_EXE));
        command.current_dir(launch.engine_dir);
        if let Some(cache_dir) = launch.cache_dir {
            std::fs::create_dir_all(cache_dir).map_err(failed)?;
            command.env("FERRYX_CEF_CACHE", cache_dir);
        }
        platform::configure(&mut command, launch.parent);
        start(command, profile, launch.parent, sink).map_err(failed)?;
    }
    TABS.lock().insert(
        browser_id.to_string(),
        Tab { profile: profile.to_string(), handle: None },
    );
    let command = EngineCommand::Open {
        tab: browser_id.to_string(),
        rect: rect.into(),
        visible,
        url: url.to_string(),
    };
    if send(profile, &command) {
        Ok(())
    } else {
        TABS.lock().remove(browser_id);
        Err(BrowserError::CreateFailed("the Chromium engine is not running".into()))
    }
}

/// Starts an engine process and the thread that reads its events.
fn start(mut command: Command, profile: &str, parent: isize, sink: EventSink) -> std::io::Result<()> {
    let mut child = command.stdin(Stdio::piped()).stdout(Stdio::piped()).spawn()?;
    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(std::io::Error::other("engine pipes unavailable"));
    };
    let pid = child.id();
    ENGINES.lock().insert(profile.to_string(), Engine { child, stdin, parent, sink: Arc::clone(&sink) });
    let profile = profile.to_string();
    std::thread::Builder::new()
        .name(format!("cef-engine-{profile}"))
        .spawn(move || read_events(stdout, &profile, pid, &sink))?;
    Ok(())
}

fn read_events(stdout: std::process::ChildStdout, profile: &str, pid: u32, sink: &EventSink) {
    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        let Some(event) = Event::parse(&line) else { continue };
        match &event {
            Event::Created { tab, handle } => {
                if let Some(entry) = TABS.lock().get_mut(tab) {
                    entry.handle = Some(*handle as isize);
                }
            }
            Event::Closed { tab } => {
                TABS.lock().remove(tab);
            }
            Event::Devtools { id, ok, result, .. } => {
                // An answer goes to the call that asked for it, not to the tab's event sink.
                if let Some(pending) = DEVTOOLS_CALLS.lock().remove(id) {
                    let _ = pending.reply.send((*ok, result.clone()));
                }
                continue;
            }
            _ => {}
        }
        sink(profile, event);
    }
    // The engine is gone (it crashed, or Ferryx closed its stdin): its tabs went with it.
    engine_gone(profile, pid);
}

/// Makes a window the engine opened on its own the window of the Ferryx tab `browser_id`.
pub fn adopt(profile: &str, browser_id: &str, popup: u32) -> bool {
    TABS.lock().insert(
        browser_id.to_string(),
        Tab { profile: profile.to_string(), handle: None },
    );
    let sent = send(profile, &EngineCommand::Adopt { tab: browser_id.to_string(), popup });
    if !sent {
        TABS.lock().remove(browser_id);
    }
    sent
}

pub fn set_bounds(browser_id: &str, rect: PhysicalRect) -> bool {
    let sent = send_to_tab(browser_id, EngineCommand::Bounds { tab: browser_id.to_string(), rect: rect.into() });
    // The app's own windows must stop painting over the window's new area.
    repaint(browser_id);
    sent.is_ok()
}

pub fn set_visible(browser_id: &str, visible: bool) -> bool {
    let tab = browser_id.to_string();
    let command = if visible { EngineCommand::Show { tab } } else { EngineCommand::Hide { tab } };
    send_to_tab(browser_id, command).is_ok()
}

/// Closes the tab's window in its engine, and ends the engine when that was its last tab.
/// Returns false when the tab is not an engine tab.
pub fn close(browser_id: &str) -> bool {
    let Some(tab) = TABS.lock().remove(browser_id) else {
        return false;
    };
    send(&tab.profile, &EngineCommand::Close { tab: browser_id.to_string() });
    let last = !TABS.lock().values().any(|other| other.profile == tab.profile);
    if last {
        // Dropping stdin tells the engine to quit; a thread waits for it so no zombie is left.
        if let Some(Engine { mut child, stdin, .. }) = ENGINES.lock().remove(&tab.profile) {
            drop(stdin);
            let _ = std::thread::Builder::new()
                .name("cef-engine-exit".into())
                .spawn(move || child.wait());
        }
    }
    true
}

pub fn navigate(browser_id: &str, url: &str) -> Result<(), BrowserError> {
    send_to_tab(browser_id, EngineCommand::Navigate { tab: browser_id.to_string(), url: url.to_string() })
}

pub fn history(browser_id: &str, forward: bool) -> Result<(), BrowserError> {
    let tab = browser_id.to_string();
    send_to_tab(browser_id, if forward { EngineCommand::Forward { tab } } else { EngineCommand::Back { tab } })
}

pub fn reload(browser_id: &str, ignore_cache: bool) -> Result<(), BrowserError> {
    send_to_tab(browser_id, EngineCommand::Reload { tab: browser_id.to_string(), ignore_cache })
}

/// Gives the tab keyboard focus: Chrome's address bar when `address`, else the page.
pub fn focus(browser_id: &str, address: bool) -> Result<(), BrowserError> {
    send_to_tab(browser_id, EngineCommand::Focus { tab: browser_id.to_string(), address })
}

/// Answers the profile engine's held permission request (see `site_permissions`). Returns
/// false when that engine is not running; its requests went with it.
pub fn answer_permission(profile: &str, request: u64, allow: bool) -> bool {
    send(profile, &EngineCommand::Permission { request, allow })
}

/// Runs one Chrome DevTools Protocol call on the tab's browser through the engine's
/// in-process DevTools message API (the engine opens no DevTools port). Returns the CDP
/// result object, or the CDP error as `AutomationFailed`.
pub async fn devtools(
    browser_id: &str,
    method: &str,
    params: serde_json::Value,
    timeout: Duration,
) -> Result<serde_json::Value, BrowserError> {
    let profile = TABS
        .lock()
        .get(browser_id)
        .map(|tab| tab.profile.clone())
        .ok_or_else(|| BrowserError::NotFound(browser_id.to_string()))?;
    let id = next_devtools_id();
    let (reply, answer) = tokio::sync::oneshot::channel();
    DEVTOOLS_CALLS.lock().insert(id, PendingDevtools { profile: profile.clone(), reply });
    let command = EngineCommand::Devtools {
        tab: browser_id.to_string(),
        id,
        method: method.to_string(),
        params: params.to_string(),
    };
    if !send(&profile, &command) {
        DEVTOOLS_CALLS.lock().remove(&id);
        return Err(BrowserError::AutomationFailed("the Chromium engine is not running".into()));
    }
    let outcome = tokio::time::timeout(timeout, answer).await;
    DEVTOOLS_CALLS.lock().remove(&id);
    let (ok, raw) = outcome
        .map_err(|_| BrowserError::AutomationFailed(format!("{method} timed out in the Chromium engine")))?
        .map_err(|_| BrowserError::AutomationFailed("the Chromium engine stopped".into()))?;
    devtools_answer(method, ok, &raw)
}

/// Turns an engine DevTools answer into the CDP result or a typed failure.
fn devtools_answer(method: &str, ok: bool, raw: &str) -> Result<serde_json::Value, BrowserError> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|error| {
        BrowserError::AutomationFailed(format!("{method} returned malformed JSON: {error}"))
    })?;
    if ok {
        return Ok(value);
    }
    let message = value
        .get("message")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("the DevTools call failed");
    Err(BrowserError::AutomationFailed(format!("{method}: {message}")))
}

/// Tells a running engine of `profile` to apply the cookie file Ferryx staged at `path`.
/// Returns false when that engine is not running; it applies the file when it next starts.
pub fn import_cookies(profile: &str, path: &Path) -> bool {
    send(profile, &EngineCommand::ImportCookies { path: path.display().to_string() })
}

/// Runs a password fill script in the tab's isolated vault world inside the engine.
pub fn fill(browser_id: &str, script: String) -> Result<(), BrowserError> {
    send_to_tab(browser_id, EngineCommand::Fill { tab: browser_id.to_string(), script })
}

/// Brings the tab's window back into view after the page changed: Windows repaints the app's
/// own windows around it, macOS puts the engine's windows back above the Ferryx window.
pub fn repaint(browser_id: &str) {
    let Some((profile, handle)) = TABS.lock().get(browser_id).map(|tab| (tab.profile.clone(), tab.handle)) else {
        return;
    };
    let parent = ENGINES.lock().get(&profile).map(|engine| engine.parent);
    if let Some(parent) = parent {
        if platform::repaint(parent, handle) {
            send(&profile, &EngineCommand::Raise);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn logical_bounds_scale_to_physical_pixels() {
        let bounds = LogicalRect { x: 10.5, y: 20.0, width: 300.25, height: 200.0 };
        assert_eq!(
            to_physical(&bounds, 1.5),
            PhysicalRect { x: 16, y: 30, width: 450, height: 300 }
        );
    }

    #[test]
    fn unknown_tab_is_not_a_cef_tab() {
        assert!(!is_cef_tab("no-such-tab"));
        assert!(!set_visible("no-such-tab", true));
        assert!(!close("no-such-tab"));
        assert!(matches!(navigate("no-such-tab", "https://a.test/"), Err(BrowserError::NotFound(_))));
    }

    #[test]
    fn devtools_answers_map_to_results_and_typed_failures() {
        assert_eq!(
            devtools_answer("Runtime.evaluate", true, r#"{"result":{"value":1}}"#).unwrap(),
            serde_json::json!({"result": {"value": 1}})
        );
        assert_eq!(
            devtools_answer("Page.navigate", false, r#"{"code":-32000,"message":"Cannot navigate"}"#),
            Err(BrowserError::AutomationFailed("Page.navigate: Cannot navigate".into()))
        );
        assert!(matches!(
            devtools_answer("Page.enable", true, "not json"),
            Err(BrowserError::AutomationFailed(_))
        ));
        assert!(next_devtools_id() > 0 && next_devtools_id() <= i32::MAX as u32);
    }

    #[tokio::test]
    async fn a_devtools_call_on_an_unknown_tab_is_not_found() {
        let error = devtools("no-such-tab", "Page.enable", serde_json::json!({}), Duration::from_secs(1))
            .await
            .expect_err("unknown tab");
        assert!(matches!(error, BrowserError::NotFound(_)));
    }

    /// A stand-in engine answers a DevTools call: the answer reaches the caller and not the
    /// sink, and the engine exiting afterwards still closes its tab.
    #[tokio::test]
    async fn a_devtools_answer_reaches_its_caller() {
        let id = next_devtools_id();
        // No quotes in the line: `cmd /C echo` on Windows would print them escaped.
        let line = format!("@ferryx devtools dt-tab {id} 1 [7]");
        #[cfg(target_os = "windows")]
        let command = {
            let mut command = Command::new("cmd");
            command.args(["/C", &format!("echo {line}")]);
            command
        };
        #[cfg(not(target_os = "windows"))]
        let command = {
            let mut command = Command::new("sh");
            command.args(["-c", &format!("echo '{line}'")]);
            command
        };
        let (reply, answer) = tokio::sync::oneshot::channel();
        DEVTOOLS_CALLS.lock().insert(id, PendingDevtools { profile: "dt-profile".into(), reply });
        let (sender, receiver) = std::sync::mpsc::channel();
        let sender = Mutex::new(sender);
        let sink: EventSink = Arc::new(move |_: &str, event: Event| {
            let _ = sender.lock().send(event);
        });
        TABS.lock().insert("dt-tab".into(), Tab { profile: "dt-profile".into(), handle: None });
        start(command, "dt-profile", 0, sink).expect("stand-in engine starts");
        let (ok, raw) = tokio::time::timeout(Duration::from_secs(10), answer)
            .await
            .expect("answer in time")
            .expect("answer delivered");
        assert!(ok);
        assert_eq!(devtools_answer("Runtime.evaluate", ok, &raw).unwrap(), serde_json::json!([7]));
        let closed = receiver.recv_timeout(Duration::from_secs(10)).expect("closed event");
        assert_eq!(closed, Event::Closed { tab: "dt-tab".into() });
    }

    /// A stand-in engine prints two events and exits: the events reach the sink in order, and
    /// the tab it served is reported closed and forgotten.
    #[test]
    fn events_reach_the_sink_and_an_exited_engine_closes_its_tabs() {
        #[cfg(target_os = "windows")]
        let command = {
            let mut command = Command::new("cmd");
            command.args(["/C", "echo @ferryx created stand-in-tab 7& echo @ferryx title stand-in-tab Hello"]);
            command
        };
        #[cfg(not(target_os = "windows"))]
        let command = {
            let mut command = Command::new("sh");
            command.args(["-c", "echo '@ferryx created stand-in-tab 7'; echo '@ferryx title stand-in-tab Hello'"]);
            command
        };
        let (sender, receiver) = mpsc::channel();
        let sender = Mutex::new(sender);
        let sink: EventSink = Arc::new(move |profile: &str, event: Event| {
            let _ = sender.lock().send((profile.to_string(), event));
        });
        let profile = "stand-in-profile";
        TABS.lock().insert(
            "stand-in-tab".to_string(),
            Tab { profile: profile.to_string(), handle: None },
        );
        start(command, profile, 0, sink).expect("stand-in engine starts");

        let mut events = Vec::new();
        while events.len() < 3 {
            let (from, event) = receiver.recv_timeout(Duration::from_secs(10)).expect("engine event");
            assert_eq!(from, profile);
            events.push(event);
        }
        assert_eq!(
            events,
            vec![
                Event::Created { tab: "stand-in-tab".into(), handle: 7 },
                Event::Title { tab: "stand-in-tab".into(), title: "Hello".into() },
                Event::Closed { tab: "stand-in-tab".into() },
            ]
        );
        assert!(!is_cef_tab("stand-in-tab"));
        assert!(!ENGINES.lock().contains_key(profile));
    }
}
