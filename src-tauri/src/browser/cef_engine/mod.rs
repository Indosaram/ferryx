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
use ferryx_browser_engine_protocol::{Command as EngineCommand, ExtensionCall, Rect};
use parking_lot::{Condvar, Mutex};
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
pub use linux::{parent_xid, take_x_focus, WAYLAND_FALLBACK_MESSAGE, WAYLAND_REASON};

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

struct EngineProcessInner {
    pid: u32,
    #[cfg(unix)]
    reaped: Mutex<bool>,
    #[cfg(windows)]
    dup_handle: Mutex<Option<std::os::windows::io::OwnedHandle>>,
    exit_status: Mutex<Option<Option<std::process::ExitStatus>>>,
    exited: Condvar,
}

/// Handle to a running or quitting CEF engine process, backed by one blocking waiter thread
/// that reaps the child with 0% idle CPU and wakes waiters immediately on exit.
#[derive(Clone)]
struct EngineProcess {
    inner: Arc<EngineProcessInner>,
}

#[cfg(windows)]
fn duplicate_process_handle(child: &Child) -> std::io::Result<std::os::windows::io::OwnedHandle> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
    use windows_sys::Win32::Foundation::{DuplicateHandle, DUPLICATE_SAME_ACCESS, HANDLE};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    let mut dup: HANDLE = std::ptr::null_mut();
    // SAFETY: `child.as_raw_handle()` is a live process handle owned by `child`;
    // both process handles are the current-process pseudo-handle.
    let ok = unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            child.as_raw_handle() as HANDLE,
            GetCurrentProcess(),
            &mut dup,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        )
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `DuplicateHandle` succeeded and returned a freshly owned handle.
    Ok(unsafe { OwnedHandle::from_raw_handle(dup as RawHandle) })
}

impl EngineProcess {
    fn prepare(child: &Child) -> std::io::Result<Self> {
        #[cfg(windows)]
        let dup_handle = duplicate_process_handle(child)?;
        Ok(Self {
            inner: Arc::new(EngineProcessInner {
                pid: child.id(),
                #[cfg(unix)]
                reaped: Mutex::new(false),
                #[cfg(windows)]
                dup_handle: Mutex::new(Some(dup_handle)),
                exit_status: Mutex::new(None),
                exited: Condvar::new(),
            }),
        })
    }

    fn spawn_waiter(
        &self,
        child: Child,
        thread_name: String,
        watch: Option<(String, std::sync::mpsc::Receiver<()>)>,
    ) -> std::io::Result<()> {
        let inner = Arc::clone(&self.inner);
        std::thread::Builder::new()
            .name(thread_name)
            .spawn(move || wait_and_reap(child, inner, watch))
            .map(|_| ())
    }

    #[cfg(test)]
    fn spawn_detached(mut child: Child) -> std::io::Result<Self> {
        let process = match Self::prepare(&child) {
            Ok(process) => process,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        process.spawn_waiter(child, "cef-engine-exit-test".into(), None)?;
        Ok(process)
    }

    fn pid(&self) -> u32 {
        self.inner.pid
    }

    fn has_exited(&self) -> bool {
        self.inner.exit_status.lock().is_some()
    }

    fn wait(&self) -> Option<std::process::ExitStatus> {
        let mut status = self.inner.exit_status.lock();
        while status.is_none() {
            self.inner.exited.wait(&mut status);
        }
        (*status).flatten()
    }

    fn wait_timeout(&self, timeout: Duration) -> bool {
        let mut status = self.inner.exit_status.lock();
        if status.is_some() {
            return true;
        }
        if timeout.is_zero() {
            return false;
        }
        let deadline = std::time::Instant::now() + timeout;
        while status.is_none() {
            if self.inner.exited.wait_until(&mut status, deadline).timed_out() {
                return status.is_some();
            }
        }
        true
    }

    fn kill(&self) {
        #[cfg(unix)]
        {
            let reaped = self.inner.reaped.lock();
            if !*reaped {
                // SAFETY: `reaped` is false and the waiter only sets it to true and reaps
                // the child after acquiring this lock, so `pid` cannot have been recycled.
                unsafe {
                    libc::kill(self.inner.pid as libc::pid_t, libc::SIGKILL);
                }
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::Foundation::HANDLE;
            use windows_sys::Win32::System::Threading::TerminateProcess;

            let handle = self.inner.dup_handle.lock();
            if let Some(handle) = handle.as_ref() {
                // SAFETY: `handle` is an owned duplicated process handle kept alive under the lock.
                unsafe {
                    let _ = TerminateProcess(handle.as_raw_handle() as HANDLE, 1);
                }
            }
        }
    }
}

/// Blocks on OS process exit with zero polling, reaps the child safely without PID-reuse
/// races, notifies any exit waiters, and tears down orphaned tabs if the engine died while
/// still registered in [`ENGINES`].
fn wait_and_reap(
    mut child: Child,
    inner: Arc<EngineProcessInner>,
    watch: Option<(String, std::sync::mpsc::Receiver<()>)>,
) {
    let status = {
        #[cfg(unix)]
        {
            loop {
                // SAFETY: zero-initialized `siginfo_t` is valid as an out-param to `waitid`.
                let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
                // Peek exit without reaping (`WNOWAIT`) so the PID stays reserved until we
                // synchronize with any concurrent `kill()` under `inner.reaped`.
                let rc = unsafe {
                    libc::waitid(
                        libc::P_PID,
                        inner.pid as libc::id_t,
                        &mut info,
                        libc::WEXITED | libc::WNOWAIT,
                    )
                };
                if rc == 0 {
                    break;
                }
                if std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                    continue;
                }
                break;
            }
            {
                let mut reaped = inner.reaped.lock();
                *reaped = true;
            }
            child.wait().ok()
        }
        #[cfg(windows)]
        {
            let status = child.wait().ok();
            let handle = inner.dup_handle.lock().take();
            drop(handle);
            status
        }
        #[cfg(not(any(unix, windows)))]
        {
            child.wait().ok()
        }
    };
    {
        let mut exit_status = inner.exit_status.lock();
        *exit_status = Some(status);
        inner.exited.notify_all();
    }
    if let Some((profile, reader_finished)) = watch {
        finish_engine_watch(&profile, inner.pid, &reader_finished);
    }
}

struct Engine {
    process: EngineProcess,
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
/// Held from the "is it running" check to the engine's registration: two tabs of one profile
/// opening at once (a restored tab and an inbound link on a cold launch) would otherwise start
/// two engines on one profile dir, and the second fails CEF initialization.
static LAUNCHING: Mutex<()> = Mutex::new(());
static TABS: LazyLock<Mutex<HashMap<String, Tab>>> = LazyLock::new(Default::default);
/// An engine told to quit (its last tab closed) whose process may still run: Chromium refuses
/// a profile dir another live engine holds, so the next engine of that profile waits on the
/// single [`EngineProcess`] waiter until it exits.
static EXITING: LazyLock<Mutex<HashMap<String, EngineProcess>>> = LazyLock::new(Default::default);
/// How long a quitting engine gets before the next engine of its profile ends it.
const PREVIOUS_ENGINE_EXIT_TIMEOUT: Duration = Duration::from_secs(10);

/// How the profile's previous engine was out of the way of a new one.
#[derive(Debug, PartialEq, Eq)]
enum PreviousEngine {
    None,
    Exited,
    Killed,
}

/// Makes sure the profile's previous engine is gone: waits up to `timeout` for it to quit, then
/// kills it. Blocking; runs under [`LAUNCHING`].
fn end_previous_engine(profile: &str, timeout: Duration) -> PreviousEngine {
    let Some(previous) = EXITING.lock().remove(profile) else {
        return PreviousEngine::None;
    };
    if previous.wait_timeout(timeout) {
        return PreviousEngine::Exited;
    }
    tracing::warn!(%profile, "the previous Chromium engine did not quit in time; ending it");
    previous.kill();
    let _ = previous.wait();
    PreviousEngine::Killed
}

/// A DevTools call waiting for its answer: the profile whose engine runs it, and the waiter.
struct PendingDevtools {
    profile: String,
    reply: tokio::sync::oneshot::Sender<(bool, String)>,
}

static DEVTOOLS_CALLS: LazyLock<Mutex<HashMap<u32, PendingDevtools>>> = LazyLock::new(Default::default);
static NEXT_DEVTOOLS_ID: AtomicU32 = AtomicU32::new(1);
/// Extension calls waiting for their answer, like [`DEVTOOLS_CALLS`] (ids of their own).
static EXTENSION_CALLS: LazyLock<Mutex<HashMap<u32, PendingDevtools>>> = LazyLock::new(Default::default);
static NEXT_EXTENSION_ID: AtomicU32 = AtomicU32::new(1);

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
    #[cfg(test)]
    if let Some(sent) = fake::send(profile, command) {
        return sent;
    }
    let result = {
        let mut engines = ENGINES.lock();
        let Some(engine) = engines.get_mut(profile) else {
            return false;
        };
        writeln!(engine.stdin, "{}", command.to_line())
            .and_then(|()| engine.stdin.flush())
            .map_err(|error| (error, engine.process.pid()))
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
            Some(engine) if engine.process.pid() == pid => engines.remove(profile),
            _ => None,
        }
    };
    let Some(engine) = engine else {
        return;
    };
    drop(engine.stdin);
    engine.process.kill();
    let _ = engine.process.wait();
    // Dropping the waiters tells every DevTools call in flight that the engine is gone.
    DEVTOOLS_CALLS.lock().retain(|_, pending| pending.profile != profile);
    EXTENSION_CALLS.lock().retain(|_, pending| pending.profile != profile);
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

/// Whether the profile's engine process is running.
pub fn is_running(profile: &str) -> bool {
    ENGINES.lock().contains_key(profile)
}

/// A debug build appends these whitespace-separated Chromium switches to the engine command
/// line (QA: `--remote-debugging-port` for a CDP virtual authenticator,
/// `--use-fake-device-for-media-stream`); release builds ignore it.
pub const DEBUG_ENGINE_ARGS_ENV: &str = "FERRYX_ENGINE_EXTRA_ARGS";

/// The Chromium switches in a [`DEBUG_ENGINE_ARGS_ENV`] value: its whitespace-separated
/// tokens that start with `--`; anything else is dropped.
fn debug_engine_args(value: Option<&str>) -> Vec<String> {
    value
        .unwrap_or_default()
        .split_whitespace()
        .filter(|token| token.starts_with("--"))
        .map(str::to_owned)
        .collect()
}

/// Starts the profile's engine when it is not running, without opening a tab.
/// Blocking: run it through `run_blocking`.
pub fn ensure_running(launch: &EngineLaunch<'_>, profile: &str, sink: EventSink) -> Result<(), BrowserError> {
    // An engine that died since its last command is replaced, not written to.
    let dead = ENGINES.lock().get(profile).and_then(|engine| {
        engine.process.has_exited().then(|| engine.process.pid())
    });
    if let Some(pid) = dead {
        engine_gone(profile, pid);
    }
    start_once(
        || is_running(profile),
        || end_previous_engine(profile, PREVIOUS_ENGINE_EXIT_TIMEOUT),
        || {
            let failed = |error: std::io::Error| BrowserError::CreateFailed(format!("CEF engine: {error}"));
            let mut command = Command::new(launch.engine_dir.join(ENGINE_EXE));
            command.current_dir(launch.engine_dir);
            if let Some(cache_dir) = launch.cache_dir {
                std::fs::create_dir_all(cache_dir).map_err(failed)?;
                command.env("FERRYX_CEF_CACHE", cache_dir);
            }
            platform::configure(&mut command, launch.parent);
            if cfg!(debug_assertions) {
                command.args(debug_engine_args(std::env::var(DEBUG_ENGINE_ARGS_ENV).ok().as_deref()));
            }
            start(command, profile, launch.parent, sink).map_err(failed)
        },
    )
}

/// Runs `start` unless `running`, with no other engine launch in between (see [`LAUNCHING`]),
/// after `end_previous` has the profile's quitting engine (if any) out of the way.
fn start_once<P>(
    running: impl Fn() -> bool,
    end_previous: impl FnOnce() -> P,
    start: impl FnOnce() -> Result<(), BrowserError>,
) -> Result<(), BrowserError> {
    let _launching = LAUNCHING.lock();
    if running() {
        return Ok(());
    }
    end_previous();
    start()
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
    ensure_running(launch, profile, sink)?;
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

/// Starts an engine process, its stdout reader thread, and its single blocking exit waiter.
fn start(mut command: Command, profile: &str, parent: isize, sink: EventSink) -> std::io::Result<()> {
    let mut child = command.stdin(Stdio::piped()).stdout(Stdio::piped()).spawn()?;
    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(std::io::Error::other("engine pipes unavailable"));
    };
    let process = match EngineProcess::prepare(&child) {
        Ok(process) => process,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    let pid = process.pid();
    ENGINES.lock().insert(
        profile.to_string(),
        Engine { process: process.clone(), stdin, parent, sink: Arc::clone(&sink) },
    );
    // The reader drops `reader_done` when it returns, which tells the exit waiter the engine's
    // last events were delivered.
    let (reader_done, reader_finished) = std::sync::mpsc::channel::<()>();
    let reader_profile = profile.to_string();
    if let Err(error) = std::thread::Builder::new()
        .name(format!("cef-engine-{reader_profile}"))
        .spawn(move || {
            let _reader_done = reader_done;
            read_events(stdout, &reader_profile, pid, &sink);
        })
    {
        let mut engines = ENGINES.lock();
        if engines.get(profile).is_some_and(|engine| engine.process.pid() == pid) {
            engines.remove(profile);
        }
        drop(engines);
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    let watched_profile = profile.to_string();
    if let Err(error) = process.spawn_waiter(
        child,
        format!("cef-engine-exit-{watched_profile}"),
        Some((watched_profile, reader_finished)),
    ) {
        let mut engines = ENGINES.lock();
        if engines.get(profile).is_some_and(|engine| engine.process.pid() == pid) {
            engines.remove(profile);
        }
        drop(engines);
        process.kill();
        return Err(error);
    }
    Ok(())
}

/// How long the exit waiter lets the reader deliver an exited engine's last events before it
/// closes the tabs itself (the reader never finishes while a helper process holds stdout).
const EXIT_DRAIN: Duration = Duration::from_secs(2);

/// Whether [`ENGINES`] still holds the engine of `profile` started as `pid`. A successor engine
/// of the same profile (another pid) is not this waiter's business.
fn is_owned_engine(profile: &str, pid: u32) -> bool {
    ENGINES
        .lock()
        .get(profile)
        .is_some_and(|engine| engine.process.pid() == pid)
}

/// Closes tabs when the engine main process exits while still registered in [`ENGINES`]. The
/// reader thread only sees the end of stdout, and a Chromium helper process (GPU, zygote,
/// renderer) that outlives a killed engine keeps that pipe open (linux CI run 41801115761:
/// four engine tabs stayed listed after SIGKILL), so process exit itself has to close the tabs.
fn finish_engine_watch(
    profile: &str,
    pid: u32,
    reader_finished: &std::sync::mpsc::Receiver<()>,
) {
    if !is_owned_engine(profile, pid) {
        return;
    }
    // Normally the reader reaches the end of stdout right away and reports the
    // tabs closed after the engine's last events; then engine_gone below is a no-op.
    let _ = reader_finished.recv_timeout(EXIT_DRAIN);
    if !is_owned_engine(profile, pid) {
        return;
    }
    tracing::warn!(%profile, pid, "CEF engine process exited");
    engine_gone(profile, pid);
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
                answer_devtools(*id, *ok, result.clone());
                continue;
            }
            Event::Extensions { id, ok, result } => {
                if let Some(pending) = EXTENSION_CALLS.lock().remove(id) {
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

/// Hands an engine's DevTools answer to the call waiting for it (none after a timeout).
fn answer_devtools(id: u32, ok: bool, result: String) {
    if let Some(pending) = DEVTOOLS_CALLS.lock().remove(&id) {
        let _ = pending.reply.send((ok, result));
    }
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
        // Dropping stdin tells the engine to quit; its single waiter thread reaps it.
        if let Some(Engine { process, stdin, .. }) = ENGINES.lock().remove(&tab.profile) {
            drop(stdin);
            EXITING.lock().insert(tab.profile, process);
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
    let result = send_to_tab(browser_id, EngineCommand::Focus { tab: browser_id.to_string(), address });
    tracing::info!(%browser_id, address, sent = result.is_ok(), "CEF engine focus command");
    result
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

/// Drops local files on the tab's page at viewport point (x, y) (CSS pixels) through CDP
/// Input.dispatchDragEvent (dragEnter, dragOver, drop), the way an OS drop would; Chromium grants
/// the page access to the files. Returns each call's outcome, in order.
pub async fn drop_files(
    browser_id: &str,
    paths: &[String],
    x: f64,
    y: f64,
) -> Vec<(String, Result<serde_json::Value, String>)> {
    let mut calls = Vec::new();
    for kind in ["dragEnter", "dragOver", "drop"] {
        let params = serde_json::json!({
            "type": kind,
            "x": x,
            "y": y,
            "data": {"items": [], "files": paths, "dragOperationsMask": 1},
        });
        let result = devtools(
            browser_id,
            "Input.dispatchDragEvent",
            params,
            Duration::from_secs(5),
        )
        .await
        .map_err(|error| error.to_string());
        let failed = result.is_err();
        calls.push((kind.to_string(), result));
        if failed {
            break;
        }
    }
    calls
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

/// What an extension call failing with `message` returns: `"cancelled"` (the user cancelled
/// Chrome's remove dialog) is its own message.
pub const EXTENSION_CALL_CANCELLED: &str = "cancelled";

/// Runs one extension call in the profile's running engine (its hidden `chrome://extensions`
/// page) and returns the call's JSON value; for [`ExtensionCall::List`] the extension array.
pub async fn extensions(
    profile: &str,
    call: ExtensionCall,
    timeout: Duration,
) -> Result<serde_json::Value, BrowserError> {
    let id = loop {
        let id = NEXT_EXTENSION_ID.fetch_add(1, Ordering::Relaxed);
        if id != 0 {
            break id;
        }
    };
    let (reply, answer) = tokio::sync::oneshot::channel();
    EXTENSION_CALLS.lock().insert(id, PendingDevtools { profile: profile.to_string(), reply });
    if !send(profile, &EngineCommand::Extensions { id, call }) {
        EXTENSION_CALLS.lock().remove(&id);
        return Err(BrowserError::AutomationFailed("the Chromium engine is not running".into()));
    }
    let outcome = tokio::time::timeout(timeout, answer).await;
    EXTENSION_CALLS.lock().remove(&id);
    let (ok, raw) = outcome
        .map_err(|_| BrowserError::AutomationFailed("the extension change timed out in the Chromium engine".into()))?
        .map_err(|_| BrowserError::AutomationFailed("the Chromium engine stopped".into()))?;
    extensions_answer(ok, &raw)
}

/// Turns an engine extensions answer into its JSON value or a typed failure.
fn extensions_answer(ok: bool, raw: &str) -> Result<serde_json::Value, BrowserError> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|error| {
        BrowserError::AutomationFailed(format!("the extensions page returned malformed JSON: {error}"))
    })?;
    if ok {
        return Ok(value);
    }
    let message = value
        .get("message")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("the extension change failed");
    Err(BrowserError::AutomationFailed(message.to_string()))
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
pub(crate) mod fake {
    //! In-process stand-in for engine processes, so the DevTools path (tab lookup, call ids,
    //! command and event lines, answers, timeouts, a dying engine) runs in unit tests on every
    //! OS without a Chromium engine.

    use super::*;

    /// Gets each command line Ferryx writes to the engine's stdin and returns the event lines
    /// the engine prints in reply; `None` means the engine exits on that command.
    pub(crate) type Handler = Arc<dyn Fn(&str) -> Option<Vec<String>> + Send + Sync>;

    static HANDLERS: LazyLock<Mutex<HashMap<String, Handler>>> = LazyLock::new(Default::default);

    /// Runs `handler` as the engine of `profile`, serving `tabs`.
    pub(crate) fn install(profile: &str, tabs: &[&str], handler: Handler) {
        HANDLERS.lock().insert(profile.to_string(), handler);
        let mut known = TABS.lock();
        for tab in tabs {
            known.insert((*tab).to_string(), Tab { profile: profile.to_string(), handle: None });
        }
    }

    /// Registers `tab` under `profile` while no engine runs for that profile.
    pub(crate) fn orphan_tab(profile: &str, tab: &str) {
        TABS.lock().insert(tab.to_string(), Tab { profile: profile.to_string(), handle: None });
    }

    /// Stops the stand-in of `profile`: its tabs and the calls waiting on it go with it.
    pub(crate) fn remove(profile: &str) {
        HANDLERS.lock().remove(profile);
        exited(profile);
    }

    fn exited(profile: &str) {
        DEVTOOLS_CALLS.lock().retain(|_, pending| pending.profile != profile);
        TABS.lock().retain(|_, tab| tab.profile != profile);
    }

    /// `None` when no stand-in serves `profile` (the real engine path then runs).
    pub(super) fn send(profile: &str, command: &EngineCommand) -> Option<bool> {
        let handler = HANDLERS.lock().get(profile).cloned()?;
        let Some(lines) = handler(&command.to_line()) else {
            // The command was written, then the engine died before answering.
            HANDLERS.lock().remove(profile);
            exited(profile);
            return Some(true);
        };
        for line in lines {
            if let Some(Event::Devtools { id, ok, result, .. }) = Event::parse(&line) {
                answer_devtools(id, ok, result);
            }
        }
        Some(true)
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
    fn debug_engine_args_keep_only_switches() {
        let value = " --remote-debugging-port=9222  stray\t--use-fake-device-for-media-stream -x ";
        assert_eq!(
            debug_engine_args(Some(value)),
            ["--remote-debugging-port=9222", "--use-fake-device-for-media-stream"]
        );
        assert!(debug_engine_args(None).is_empty());
        assert!(debug_engine_args(Some("  \t ")).is_empty());
    }

    /// A stand-in process for an engine told to quit: one that exits at once, or one that
    /// ignores the quit and keeps running.
    fn quitting_engine(exits: bool) -> Child {
        #[cfg(target_os = "windows")]
        let mut command = {
            let mut command = Command::new("cmd");
            command.args(["/C", if exits { "exit 0" } else { "ping -n 60 127.0.0.1 >NUL" }]);
            command
        };
        #[cfg(not(target_os = "windows"))]
        let mut command = {
            let mut command = Command::new("sh");
            command.args(["-c", if exits { "exit 0" } else { "exec sleep 60" }]);
            command
        };
        command.stdout(Stdio::null()).stderr(Stdio::null()).spawn().expect("stand-in engine starts")
    }

    /// A new engine of a profile goes after that profile's quitting engine: none, one that
    /// exited, and one that ignored the quit (killed once the wait is over).
    #[test]
    fn a_new_engine_ends_the_previous_one_of_its_profile_first() {
        assert_eq!(end_previous_engine("exit-none", Duration::ZERO), PreviousEngine::None);

        let done = EngineProcess::spawn_detached(quitting_engine(true)).expect("watch done");
        EXITING.lock().insert("exit-done".to_string(), done);
        assert_eq!(end_previous_engine("exit-done", Duration::from_secs(30)), PreviousEngine::Exited);
        assert!(!EXITING.lock().contains_key("exit-done"));

        let stuck = EngineProcess::spawn_detached(quitting_engine(false)).expect("watch stuck");
        EXITING.lock().insert("exit-stuck".to_string(), stuck.clone());
        assert_eq!(end_previous_engine("exit-stuck", Duration::ZERO), PreviousEngine::Killed);
        assert!(stuck.has_exited());
        assert_eq!(end_previous_engine("exit-stuck", Duration::ZERO), PreviousEngine::None);
    }

    /// The previous engine is ended only when a new one is about to start, under the launch
    /// lock: a running engine is reused and its profile's quitting engine left alone.
    #[test]
    fn a_running_engine_skips_ending_the_previous_one() {
        let ended = std::sync::atomic::AtomicUsize::new(0);
        start_once(|| true, || ended.fetch_add(1, Ordering::SeqCst), || Ok(())).expect("reuse");
        assert_eq!(ended.load(Ordering::SeqCst), 0);
        start_once(|| false, || ended.fetch_add(1, Ordering::SeqCst), || Ok(())).expect("start");
        assert_eq!(ended.load(Ordering::SeqCst), 1);
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

    #[test]
    fn extensions_answers_map_to_values_and_typed_failures() {
        assert_eq!(extensions_answer(true, "[]").unwrap(), serde_json::json!([]));
        assert_eq!(extensions_answer(true, "null").unwrap(), serde_json::Value::Null);
        assert_eq!(
            extensions_answer(false, r#"{"message":"cancelled"}"#),
            Err(BrowserError::AutomationFailed(EXTENSION_CALL_CANCELLED.into()))
        );
        assert!(matches!(extensions_answer(true, "not json"), Err(BrowserError::AutomationFailed(_))));
    }

    #[tokio::test]
    async fn an_extension_call_without_a_running_engine_fails_at_once() {
        assert!(!is_running("no-such-profile"));
        let error = extensions("no-such-profile", ExtensionCall::List, Duration::from_secs(1))
            .await
            .expect_err("no engine");
        assert!(matches!(error, BrowserError::AutomationFailed(_)));
        assert!(!EXTENSION_CALLS.lock().values().any(|pending| pending.profile == "no-such-profile"));
    }

    /// A stand-in engine answers an extension call: the answer reaches the caller, and the
    /// first event the sink sees is the tab closing when the engine exits, not the answer.
    #[tokio::test]
    async fn an_extensions_answer_reaches_its_caller() {
        let id = NEXT_EXTENSION_ID.fetch_add(1, Ordering::Relaxed);
        let line = format!("@ferryx extensions {id} 1 [7]");
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
        EXTENSION_CALLS.lock().insert(id, PendingDevtools { profile: "ext-profile".into(), reply });
        let (sender, receiver) = std::sync::mpsc::channel();
        let sender = Mutex::new(sender);
        let sink: EventSink = Arc::new(move |_: &str, event: Event| {
            let _ = sender.lock().send(event);
        });
        TABS.lock().insert("ext-tab".into(), Tab { profile: "ext-profile".into(), handle: None });
        start(command, "ext-profile", 0, sink).expect("stand-in engine starts");
        let (ok, raw) = tokio::time::timeout(Duration::from_secs(10), answer)
            .await
            .expect("answer in time")
            .expect("answer delivered");
        assert_eq!(extensions_answer(ok, &raw).unwrap(), serde_json::json!([7]));
        let first = receiver.recv_timeout(Duration::from_secs(10)).expect("closed event");
        assert_eq!(first, Event::Closed { tab: "ext-tab".into() });
        assert!(!EXTENSION_CALLS.lock().contains_key(&id));
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

    /// A killed engine whose helper process still holds its stdout (linux CI run 41801115761):
    /// the reader never sees the end of the pipe, so the exit watcher closes the tab once the
    /// engine main process is gone. The stand-in engine starts a background `sleep` that
    /// inherits stdout and then exits on its own.
    #[cfg(not(target_os = "windows"))]
    #[test]
    fn an_exited_engine_closes_its_tabs_while_a_helper_holds_stdout() {
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 30 & echo '@ferryx created helper-tab 9'; exit 0"]);
        let (sender, receiver) = mpsc::channel();
        let sender = Mutex::new(sender);
        let sink: EventSink = Arc::new(move |profile: &str, event: Event| {
            let _ = sender.lock().send((profile.to_string(), event));
        });
        let profile = "helper-holds-stdout-profile";
        TABS.lock().insert("helper-tab".to_string(), Tab { profile: profile.to_string(), handle: None });
        start(command, profile, 0, sink).expect("stand-in engine starts");

        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        let mut events = Vec::new();
        while !events.contains(&Event::Closed { tab: "helper-tab".into() }) {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            let (from, event) = receiver.recv_timeout(left).expect("tab closed after the engine exited");
            assert_eq!(from, profile);
            events.push(event);
        }
        assert_eq!(events.first(), Some(&Event::Created { tab: "helper-tab".into(), handle: 9 }));
        assert!(!is_cef_tab("helper-tab"));
        assert!(!ENGINES.lock().contains_key(profile));
    }

    /// The exit watcher leaves an engine Ferryx no longer holds, or a successor with another
    /// pid, alone.
    #[test]
    fn the_exit_watcher_ignores_engines_it_does_not_own() {
        assert!(!is_owned_engine("never-started-profile", 1));
        let (_tx, rx) = mpsc::channel();
        // Returns immediately without waiting EXIT_DRAIN when not owned.
        finish_engine_watch("never-started-profile", 1, &rx);
    }

    /// Two tabs of one profile opening at once (a restored tab and a cold launch's inbound
    /// link, seen on maho-win) start one engine: the second waits for the first launch and then
    /// finds it running. A barrier lines both threads up at the call.
    #[test]
    fn concurrent_opens_of_one_profile_start_one_engine() {
        let started = std::sync::atomic::AtomicUsize::new(0);
        let running = std::sync::atomic::AtomicBool::new(false);
        let barrier = std::sync::Barrier::new(2);
        std::thread::scope(|scope| {
            for _ in 0..2 {
                scope.spawn(|| {
                    barrier.wait();
                    start_once(
                        || running.load(Ordering::SeqCst),
                        || (),
                        || {
                            started.fetch_add(1, Ordering::SeqCst);
                            // Widen the window between the check and the registration.
                            std::thread::yield_now();
                            running.store(true, Ordering::SeqCst);
                            Ok(())
                        },
                    )
                    .expect("start");
                });
            }
        });
        assert_eq!(started.load(Ordering::SeqCst), 1);
    }

    /// Print and Save as PDF on a Chromium tab: `cmd_browser_print` routes to the tab's engine
    /// and `print_pdf` sends the chosen path, as the line protocol commands the engine reads. The
    /// stand-in engine (`sort`, on every OS) writes what it received to a file when stdin closes.
    #[tokio::test]
    async fn print_and_pdf_commands_reach_the_tabs_engine() {
        use tauri::Manager;
        let dir = tempfile::tempdir().unwrap();
        let received = dir.path().join("stdin.txt");
        let mut child = Command::new("sort")
            .stdin(Stdio::piped())
            .stdout(std::fs::File::create(&received).unwrap())
            .spawn()
            .expect("stand-in engine starts");
        let stdin = child.stdin.take().expect("stdin");
        let process = EngineProcess::spawn_detached(child).expect("watch print engine");
        let profile = "print-profile";
        let sink: EventSink = Arc::new(|_: &str, _: Event| {});
        ENGINES.lock().insert(profile.to_string(), Engine { process, stdin, parent: 0, sink });
        TABS.lock().insert("print-tab".into(), Tab { profile: profile.into(), handle: None });

        let app = tauri::test::mock_app();
        app.manage(Arc::new(crate::browser::BrowserManager::new()));
        crate::ipc::browser_system::cmd_browser_print(app.handle().clone(), app.state(), "print-tab".into())
            .await
            .expect("print reaches the engine");
        let pdf = dir.path().join("My Report.pdf").to_string_lossy().to_string();
        system::print_pdf("print-tab", &pdf).expect("pdf reaches the engine");

        TABS.lock().remove("print-tab");
        let Engine { process, stdin, .. } = ENGINES.lock().remove(profile).expect("engine");
        drop(stdin);
        assert!(process.wait().expect("stand-in exits").success());
        let mut lines: Vec<String> = std::fs::read_to_string(&received)
            .unwrap()
            .lines()
            .map(|line| line.trim_end().to_string())
            .filter(|line| !line.is_empty())
            .collect();
        lines.sort();
        let pdf_line = EngineCommand::PrintPdf { tab: "print-tab".into(), path: pdf.clone() }.to_line();
        assert_eq!(EngineCommand::parse(&pdf_line), Some(EngineCommand::PrintPdf { tab: "print-tab".into(), path: pdf }));
        let mut expected = vec![EngineCommand::Print { tab: "print-tab".into() }.to_line(), pdf_line];
        expected.sort();
        assert_eq!(lines, expected);
    }

    /// A Chromium tab whose engine is gone: print fails with a code instead of opening anything.
    #[tokio::test]
    async fn print_on_a_tab_without_a_running_engine_fails() {
        use tauri::Manager;
        TABS.lock().insert("orphan-print-tab".into(), Tab { profile: "no-engine-profile".into(), handle: None });
        let app = tauri::test::mock_app();
        app.manage(Arc::new(crate::browser::BrowserManager::new()));
        let error = crate::ipc::browser_system::cmd_browser_print(app.handle().clone(), app.state(), "orphan-print-tab".into())
            .await
            .expect_err("no engine");
        assert_eq!(error.code.to_string(), "BROWSER_NAVIGATION_FAILED");
        assert!(matches!(system::print_pdf("orphan-print-tab", "/tmp/x.pdf"), Err(BrowserError::NavigationFailed(_))));
        TABS.lock().remove("orphan-print-tab");
    }
}
