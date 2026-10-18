//! Windows backend of [`crate::daemon::supervisor`]: a per-user scheduled task that runs a watchdog.
//!
//! Task Scheduler cannot restart the daemon by itself. Todo 21 measured that `RestartOnFailure`
//! relaunches neither after a non-zero exit nor after `taskkill /F`: it only covers a task that
//! fails to *start*. So the task `Ferryx\<name>` runs `<exe> --daemon-supervisor`. That watchdog
//! spawns `<exe> --daemon` outside the task's job, waits on the process handle, and:
//!
//! * respawns after a non-zero exit (1 s backoff, at most [`MAX_CRASHES_PER_WINDOW`] per minute);
//! * after exit 0, looks for the handover/upgrade successor through the runtime dir's `daemon.port`
//!   and the owner pid its handshake reports, and waits on that process instead;
//! * exits once a daemon left with exit 0 and no successor took over.
//!
//! The task is per user (`InteractiveToken`, `LeastPrivilege`), never a service and never elevated.
//! Store (MSIX) installs are unsupported: the Store owns their lifecycle.
#![cfg_attr(not(windows), allow(dead_code))]

use super::supervisor::{non_empty, Env, Result, SUPERVISOR_NAME_ENV};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Task name under the `Ferryx` task folder when no QA override is set.
pub const DEFAULT_TASK_NAME: &str = "Daemon";
/// The argument that runs the watchdog instead of the GUI or the daemon.
pub const SUPERVISOR_ARG: &str = "--daemon-supervisor";
/// Carries one isolation variable from the task definition into the watchdog (`--env KEY=VALUE`).
/// A task action has no environment block, so without this a QA task would start a watchdog that
/// manages the user's real runtime directory.
pub const ENV_ARG: &str = "--env";
/// The only variables the task may forward; anything else on the command line is refused.
pub const FORWARDED_ENV: [&str; 4] = [
    "FERRYX_DATA_DIR",
    "FERRYX_RUNTIME_DIR",
    "FERRYX_SESSION_DIR",
    SUPERVISOR_NAME_ENV,
];

pub const RESPAWN_BACKOFF: Duration = Duration::from_secs(1);
pub const CRASH_WINDOW: Duration = Duration::from_secs(60);
pub const MAX_CRASHES_PER_WINDOW: usize = 5;
/// How long an exit 0 waits for a successor to publish its endpoint. A successor waits up to 15 s
/// for the predecessor's instance lock (`handover::successor_lock_wait`) before it binds.
pub const SUCCESSOR_WAIT: Duration = Duration::from_secs(20);

/// Store/MSIX installs never get a task: the Store owns their install and update lifecycle.
pub fn supported_for_install(managed_externally: bool) -> bool {
    !managed_externally
}

pub fn task_name(env: Env<'_>) -> String {
    let name = non_empty(env, SUPERVISOR_NAME_ENV)
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| DEFAULT_TASK_NAME.to_string());
    format!("Ferryx\\{name}")
}

/// Where the rendered task XML is kept; comparing against it is what makes the write change-only.
/// One file per task name, under the user's own local app data.
fn task_xml_path(env: Env<'_>) -> Result<PathBuf> {
    let base = non_empty(env, "LOCALAPPDATA").ok_or("LOCALAPPDATA is not set")?;
    let name = task_name(env).replace('\\', "-");
    Ok(PathBuf::from(base)
        .join("Ferryx")
        .join("supervisor")
        .join(format!("{name}.xml")))
}

fn task_user(env: Env<'_>) -> Result<String> {
    let user = non_empty(env, "USERNAME").ok_or("USERNAME is not set")?;
    let user = user.to_string_lossy();
    Ok(match non_empty(env, "USERDOMAIN") {
        Some(domain) => format!("{}\\{user}", domain.to_string_lossy()),
        None => user.into_owned(),
    })
}

fn forwarded_env(env: Env<'_>) -> Vec<(String, String)> {
    FORWARDED_ENV
        .iter()
        .filter_map(|key| {
            non_empty(env, key).map(|value| (key.to_string(), value.to_string_lossy().into_owned()))
        })
        .collect()
}

/// Quotes one argument for `CommandLineToArgvW`: backslashes are literal except before a quote,
/// so a run of them before a quote (or before the closing quote) is doubled.
pub fn quote_windows_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return arg.to_string();
    }
    let mut quoted = String::from("\"");
    let mut backslashes = 0usize;
    for ch in arg.chars() {
        match ch {
            '\\' => backslashes += 1,
            '"' => {
                quoted.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                quoted.push('"');
                backslashes = 0;
            }
            other => {
                quoted.extend(std::iter::repeat_n('\\', backslashes));
                quoted.push(other);
                backslashes = 0;
            }
        }
    }
    quoted.extend(std::iter::repeat_n('\\', backslashes * 2));
    quoted.push('"');
    quoted
}

pub fn task_arguments(forwarded: &[(String, String)]) -> String {
    let mut args = SUPERVISOR_ARG.to_string();
    for (key, value) in forwarded {
        args.push(' ');
        args.push_str(ENV_ARG);
        args.push(' ');
        args.push_str(&quote_windows_arg(&format!("{key}={value}")));
    }
    args
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn xml_unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// The task definition. It starts at logon, never overlaps itself, never times out, ignores battery
/// state, and runs as the logged-on user without elevation. `RestartOnFailure` only covers a
/// watchdog that fails to start; restarting the daemon itself is the watchdog's job.
pub fn render_task_xml(user: &str, executable: &str, arguments: &str) -> String {
    let user = xml_escape(user);
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.4" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Keeps the Ferryx terminal daemon running for this user and restarts it after a crash.</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{user}</UserId>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>false</StartWhenAvailable>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <RestartOnFailure>
      <Interval>PT1M</Interval>
      <Count>3</Count>
    </RestartOnFailure>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{command}</Command>
      <Arguments>{arguments}</Arguments>
    </Exec>
  </Actions>
</Task>
"#,
        command = xml_escape(executable),
        arguments = xml_escape(arguments),
    )
}

/// The executable an existing task definition launches.
pub fn task_command(xml: &str) -> Option<String> {
    let start = xml.find("<Command>")? + "<Command>".len();
    let end = xml[start..].find("</Command>")?;
    Some(xml_unescape(&xml[start..start + end]))
}

/// `schtasks /XML` expects the UTF-16 file its declaration names.
fn encode_utf16_with_bom(text: &str) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xFE];
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

fn decode_utf16_with_bom(bytes: &[u8]) -> Option<String> {
    let body = bytes.strip_prefix(&[0xFF, 0xFE])?;
    if body.len() % 2 != 0 {
        return None;
    }
    let units: Vec<u16> = body
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    String::from_utf16(&units).ok()
}

fn read_task_xml(path: &Path) -> Option<String> {
    decode_utf16_with_bom(&std::fs::read(path).ok()?)
}

/// One `schtasks` invocation: `Ok(true)` when it exited 0, `Ok(false)` when it ran and failed.
pub type Schtasks<'a> = &'a mut dyn FnMut(&[&str]) -> Result<bool>;

/// What [`ensure_with`] does once the definition is registered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AfterRegister {
    /// `/Run` the task when no daemon answers (startup and on-demand connect).
    RunWhenIdle,
    /// Only rewrite the definition (protocol-mismatch replacement, where the old daemon still runs).
    RegisterOnly,
}

/// Registers `Ferryx\<name>` for `executable`.
///
/// A definition whose executable no longer exists is deleted first. The XML is written only when it
/// changed, and `/Create` runs only when it changed or the task is missing. Returns `true` when the
/// task was (re)created or started.
pub fn ensure_with(
    env: Env<'_>,
    executable: &Path,
    schtasks: Schtasks<'_>,
    daemon_is_live: &dyn Fn() -> bool,
    after: AfterRegister,
) -> Result<bool> {
    let name = task_name(env);
    let xml_path = task_xml_path(env)?;
    let stale = read_task_xml(&xml_path)
        .as_deref()
        .and_then(task_command)
        .filter(|command| !Path::new(command).exists());
    if let Some(command) = stale {
        tracing::warn!(%command, task = %name, "Removing a daemon supervisor task whose executable is gone");
        if !schtasks(&["/Delete", "/TN", &name, "/F"])? {
            tracing::warn!(task = %name, "schtasks /Delete of the stale task failed; /Create /F replaces it");
        }
        let _ = std::fs::remove_file(&xml_path);
    }

    let content = render_task_xml(
        &task_user(env)?,
        &executable.to_string_lossy(),
        &task_arguments(&forwarded_env(env)),
    );
    let rewritten = read_task_xml(&xml_path).as_deref() != Some(content.as_str());
    if rewritten {
        if let Some(parent) = xml_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create the supervisor task directory: {e}"))?;
        }
        std::fs::write(&xml_path, encode_utf16_with_bom(&content))
            .map_err(|e| format!("Failed to write the supervisor task XML: {e}"))?;
    }
    let created = rewritten || !schtasks(&["/Query", "/TN", &name])?;
    if created {
        let xml_arg = xml_path.to_string_lossy();
        if !schtasks(&["/Create", "/TN", &name, "/XML", &xml_arg, "/F"])? {
            // Leave no definition behind that claims to be registered.
            let _ = std::fs::remove_file(&xml_path);
            return Err(format!("schtasks /Create failed for task {name}"));
        }
    }
    let mut started = false;
    if after == AfterRegister::RunWhenIdle && !daemon_is_live() {
        if !schtasks(&["/Run", "/TN", &name])? {
            return Err(format!("schtasks /Run failed for task {name}"));
        }
        started = true;
    }
    Ok(created || started)
}

/// What the watchdog does after the process it waits on exits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchdogAction {
    /// Start a new daemon after this delay.
    Respawn { after: Duration },
    /// Another live daemon owns the endpoint (a handover/upgrade successor, or the winner of an
    /// instance-lock race); wait on it instead.
    Follow(u32),
    /// The daemon left deliberately and nothing replaced it: stop supervising.
    Stop,
}

/// The watchdog's decision state: the scheduled restart times that drive the crash cap.
#[derive(Debug, Default)]
pub struct Watchdog {
    restarts: VecDeque<Instant>,
}

impl Watchdog {
    pub fn new() -> Self {
        Self::default()
    }

    /// `exit_code` is `None` when the exit status could not be read, which counts as abnormal.
    /// `live_owner` is the pid of the daemon that answers on the endpoint now, if any.
    pub fn on_exit(
        &mut self,
        exited_pid: u32,
        exit_code: Option<u32>,
        live_owner: Option<u32>,
        now: Instant,
    ) -> WatchdogAction {
        if let Some(owner) = live_owner.filter(|owner| *owner != exited_pid) {
            return WatchdogAction::Follow(owner);
        }
        if exit_code == Some(0) {
            return WatchdogAction::Stop;
        }
        while self
            .restarts
            .front()
            .is_some_and(|restart| now.saturating_duration_since(*restart) >= CRASH_WINDOW)
        {
            self.restarts.pop_front();
        }
        let after = if self.restarts.len() >= MAX_CRASHES_PER_WINDOW {
            let oldest = self.restarts[self.restarts.len() - MAX_CRASHES_PER_WINDOW];
            (oldest + CRASH_WINDOW)
                .saturating_duration_since(now)
                .max(RESPAWN_BACKOFF)
        } else {
            RESPAWN_BACKOFF
        };
        self.restarts.push_back(now + after);
        WatchdogAction::Respawn { after }
    }
}

/// Parses the watchdog's command line (everything after the executable). `None` when the first
/// argument is not [`SUPERVISOR_ARG`].
pub fn parse_supervisor_args<S: AsRef<str>>(args: &[S]) -> Option<Result<Vec<(String, String)>>> {
    let (first, rest) = args.split_first()?;
    if first.as_ref() != SUPERVISOR_ARG {
        return None;
    }
    let mut forwarded = Vec::new();
    let mut rest = rest.iter();
    while let Some(arg) = rest.next() {
        if arg.as_ref() != ENV_ARG {
            return Some(Err(format!(
                "unexpected daemon supervisor argument: {}",
                arg.as_ref()
            )));
        }
        let Some(pair) = rest.next() else {
            return Some(Err(format!("missing value for {ENV_ARG}")));
        };
        let Some((key, value)) = pair.as_ref().split_once('=') else {
            return Some(Err(format!("{ENV_ARG} expects KEY=VALUE")));
        };
        if !FORWARDED_ENV.contains(&key) {
            return Some(Err(format!(
                "{key} may not be forwarded to the daemon supervisor"
            )));
        }
        forwarded.push((key.to_string(), value.to_string()));
    }
    Some(Ok(forwarded))
}

#[cfg(windows)]
pub use platform::{ensure, reload, retarget, run_watchdog, uninstall};

#[cfg(windows)]
mod platform {
    use super::*;
    use crate::daemon::protocol::{DaemonRequest, DaemonResponse, DAEMON_PROTOCOL_VERSION};
    use std::ffi::c_void;
    use std::io::{BufRead as _, BufReader, Write as _};

    type Handle = *mut c_void;
    const SYNCHRONIZE: u32 = 0x0010_0000;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const INFINITE: u32 = 0xFFFF_FFFF;
    const WAIT_OBJECT_0: u32 = 0;
    const ERROR_INVALID_PARAMETER: i32 = 87;

    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
        fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
        fn GetExitCodeProcess(handle: Handle, code: *mut u32) -> i32;
        fn CloseHandle(handle: Handle) -> i32;
    }

    fn process_env(key: &str) -> Option<std::ffi::OsString> {
        std::env::var_os(key)
    }

    fn run_schtasks(args: &[&str]) -> Result<bool> {
        crate::util::no_window_command("schtasks")
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .map_err(|e| format!("Failed to run schtasks: {e}"))
    }

    pub fn ensure(env: Env<'_>, executable: &Path) -> Result<bool> {
        ensure_with(
            env,
            executable,
            &mut run_schtasks,
            &|| live_owner_pid().is_some(),
            AfterRegister::RunWhenIdle,
        )
    }

    /// Points the task at `executable` without starting it, so the watchdog's next respawn
    /// launches the new build.
    pub fn retarget(env: Env<'_>, executable: &Path) -> Result<bool> {
        ensure_with(
            env,
            executable,
            &mut run_schtasks,
            &|| true,
            AfterRegister::RegisterOnly,
        )
    }

    pub fn reload(env: Env<'_>) -> Result<()> {
        let name = task_name(env);
        if run_schtasks(&["/Run", "/TN", &name])? {
            Ok(())
        } else {
            Err(format!("schtasks /Run failed for task {name}"))
        }
    }

    pub fn uninstall(env: Env<'_>) -> Result<()> {
        let name = task_name(env);
        if run_schtasks(&["/Query", "/TN", &name])?
            && !run_schtasks(&["/Delete", "/TN", &name, "/F"])?
        {
            return Err(format!("schtasks /Delete failed for task {name}"));
        }
        if let Ok(path) = task_xml_path(env) {
            let _ = std::fs::remove_file(path);
        }
        Ok(())
    }

    /// Who serves the runtime dir's `daemon.port` now.
    enum Endpoint {
        /// A daemon of this protocol version, with the pid its handshake reported.
        Owner(u32),
        /// A daemon of another protocol version: the build was replaced under this watchdog.
        OtherVersion,
    }

    fn live_owner_pid() -> Option<u32> {
        match probe_endpoint()? {
            Endpoint::Owner(pid) => Some(pid),
            Endpoint::OtherVersion => None,
        }
    }

    fn probe_endpoint() -> Option<Endpoint> {
        let port: u16 = std::fs::read_to_string(crate::daemon::server::get_socket_path())
            .ok()?
            .trim()
            .parse()
            .ok()?;
        let token = std::fs::read_to_string(crate::daemon::server::get_transport_token_path())
            .ok()
            .map(|token| token.trim().to_string())
            .filter(|token| !token.is_empty())?;
        let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        let stream = std::net::TcpStream::connect_timeout(&address, Duration::from_secs(1)).ok()?;
        stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
        let mut line = serde_json::to_string(&DaemonRequest::Handshake {
            version: DAEMON_PROTOCOL_VERSION,
            token: Some(token),
        })
        .ok()?;
        line.push('\n');
        (&stream).write_all(line.as_bytes()).ok()?;
        let mut reply = String::new();
        BufReader::new(&stream).read_line(&mut reply).ok()?;
        match serde_json::from_str::<DaemonResponse>(&reply) {
            Ok(DaemonResponse::HandshakeOk { pid, .. }) => Some(Endpoint::Owner(pid)),
            Ok(DaemonResponse::ProtocolMismatch { .. }) => Some(Endpoint::OtherVersion),
            _ => None,
        }
    }

    /// After exit 0: the successor's pid, or `Err(())` when the successor speaks another protocol
    /// version (this watchdog's build was replaced and must hand over to the new one).
    fn wait_for_successor(exited_pid: u32) -> std::result::Result<Option<u32>, ()> {
        let deadline = Instant::now() + SUCCESSOR_WAIT;
        loop {
            match probe_endpoint() {
                Some(Endpoint::Owner(owner)) if owner != exited_pid => return Ok(Some(owner)),
                Some(Endpoint::OtherVersion) => return Err(()),
                _ => {}
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    /// Starts the watchdog of the build the task now names and exits this one, so a successor of
    /// another protocol version (the protocol-mismatch replacement) stays supervised. The forwarded
    /// isolation variables are already in this process's environment and are inherited.
    fn hand_over_to_new_build() -> i32 {
        let exe = match daemon_executable() {
            Ok(exe) => exe,
            Err(error) => {
                log(&format!("cannot hand over to the new build: {error}"));
                return 1;
            }
        };
        let mut command = std::process::Command::new(&exe);
        command
            .arg(SUPERVISOR_ARG)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        crate::util::detach_launched_child(&mut command);
        match command.spawn() {
            Ok(child) => {
                log(&format!(
                    "handed over to the watchdog of {} (pid {})",
                    exe.display(),
                    child.id()
                ));
                0
            }
            Err(error) => {
                log(&format!("cannot hand over to {}: {error}", exe.display()));
                1
            }
        }
    }

    /// Waits for `pid` to exit. `Ok(None)` when it is already gone or its code is unreadable.
    fn wait_for_pid(pid: u32) -> Result<Option<u32>> {
        // SAFETY: OpenProcess takes plain integers and returns a handle or null; no pointer is read.
        let handle =
            unsafe { OpenProcess(SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            let error = std::io::Error::last_os_error();
            return match error.raw_os_error() {
                Some(ERROR_INVALID_PARAMETER) => Ok(None),
                _ => Err(format!("Cannot open daemon process {pid}: {error}")),
            };
        }
        let mut code = 0u32;
        // SAFETY: `handle` is the process handle opened above with SYNCHRONIZE and
        // PROCESS_QUERY_LIMITED_INFORMATION, `code` is a valid out pointer, and the handle is closed
        // exactly once, after its last use.
        let exit = unsafe {
            let waited = WaitForSingleObject(handle, INFINITE);
            let read = waited == WAIT_OBJECT_0 && GetExitCodeProcess(handle, &mut code) != 0;
            CloseHandle(handle);
            read.then_some(code)
        };
        Ok(exit)
    }

    /// The daemon executable: the one the task currently names (so a retargeted task respawns the
    /// new build), else this executable.
    fn daemon_executable() -> Result<PathBuf> {
        let registered = task_xml_path(&process_env)
            .ok()
            .and_then(|path| read_task_xml(&path))
            .as_deref()
            .and_then(task_command)
            .map(PathBuf::from)
            .filter(|path| path.exists());
        match registered {
            Some(path) => Ok(path),
            None => {
                std::env::current_exe().map_err(|e| format!("Failed to get current exe path: {e}"))
            }
        }
    }

    fn spawn_daemon() -> Result<std::process::Child> {
        let exe = daemon_executable()?;
        let mut command = std::process::Command::new(&exe);
        command
            .arg("--daemon")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        crate::util::detach_launched_child(&mut command);
        command
            .spawn()
            .map_err(|e| format!("Failed to spawn {} --daemon: {e}", exe.display()))
    }

    fn log(message: &str) {
        let line = format!(
            "{} pid={} {message}\n",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_millis())
                .unwrap_or_default(),
            std::process::id()
        );
        let path = crate::daemon::server::get_runtime_dir().join("supervisor.log");
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = file.write_all(line.as_bytes());
        }
    }

    enum Tracked {
        Child(std::process::Child),
        Pid(u32),
    }

    /// The process to wait on next. `Ok(None)` when a daemon of another protocol version serves the
    /// endpoint: this build was replaced, so spawning one of its own daemons would only race the new
    /// build for the instance lock; the caller hands over to the new build's watchdog instead.
    fn start() -> Result<Option<Tracked>> {
        match probe_endpoint() {
            Some(Endpoint::Owner(owner)) => {
                log(&format!("following live daemon {owner}"));
                return Ok(Some(Tracked::Pid(owner)));
            }
            Some(Endpoint::OtherVersion) => return Ok(None),
            None => {}
        }
        let child = spawn_daemon()?;
        log(&format!(
            "spawned daemon {} (creation flags {:#x})",
            child.id(),
            crate::util::windows_detach_flags()
        ));
        Ok(Some(Tracked::Child(child)))
    }

    /// The `--daemon-supervisor` main loop. Returns the process exit code.
    pub fn run_watchdog() -> i32 {
        log("watchdog started");
        let mut watchdog = Watchdog::new();
        let mut tracked = match start() {
            Ok(Some(tracked)) => tracked,
            Ok(None) => return hand_over_to_new_build(),
            Err(error) => {
                log(&format!("cannot start: {error}"));
                return 1;
            }
        };
        loop {
            let (pid, code) = match &mut tracked {
                Tracked::Child(child) => (
                    child.id(),
                    child
                        .wait()
                        .ok()
                        .and_then(|status| status.code())
                        .map(|code| code as u32),
                ),
                Tracked::Pid(pid) => match wait_for_pid(*pid) {
                    Ok(code) => (*pid, code),
                    Err(error) => {
                        log(&format!("cannot supervise: {error}"));
                        return 1;
                    }
                },
            };
            let owner = if code == Some(0) {
                match wait_for_successor(pid) {
                    Ok(owner) => owner,
                    Err(()) => return hand_over_to_new_build(),
                }
            } else {
                match probe_endpoint() {
                    Some(Endpoint::Owner(owner)) => Some(owner),
                    Some(Endpoint::OtherVersion) => return hand_over_to_new_build(),
                    None => None,
                }
            };
            let action = watchdog.on_exit(pid, code, owner, Instant::now());
            log(&format!(
                "daemon {pid} exited code={code:?} owner={owner:?} -> {action:?}"
            ));
            tracked = match action {
                WatchdogAction::Stop => return 0,
                WatchdogAction::Follow(owner) => Tracked::Pid(owner),
                WatchdogAction::Respawn { after } => {
                    std::thread::sleep(after);
                    match start() {
                        Ok(Some(tracked)) => tracked,
                        Ok(None) => return hand_over_to_new_build(),
                        Err(error) => {
                            log(&format!("respawn failed: {error}"));
                            return 1;
                        }
                    }
                }
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::ffi::OsString;

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let map: HashMap<String, OsString> = pairs
            .iter()
            .map(|(key, value)| (key.to_string(), OsString::from(value)))
            .collect();
        move |key| map.get(key).cloned()
    }

    #[test]
    fn task_xml_runs_the_watchdog_per_user_without_elevation() {
        let arguments = task_arguments(&[
            ("FERRYX_RUNTIME_DIR".into(), r"C:\qa dir\".into()),
            (SUPERVISOR_NAME_ENV.into(), "DaemonQa".into()),
        ]);
        let xml = render_task_xml(
            "DESKTOP-1\\sook",
            r"C:\Program Files\Ferryx\ferryx.exe",
            &arguments,
        );
        for fragment in [
            "<LogonTrigger>",
            "<UserId>DESKTOP-1\\sook</UserId>",
            "<LogonType>InteractiveToken</LogonType>",
            "<RunLevel>LeastPrivilege</RunLevel>",
            "<MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>",
            "<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>",
            "<DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>",
            "<StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>",
            r"<Command>C:\Program Files\Ferryx\ferryx.exe</Command>",
            r#"<Arguments>--daemon-supervisor --env &quot;FERRYX_RUNTIME_DIR=C:\qa dir\\&quot; --env FERRYX_DAEMON_SUPERVISOR_NAME=DaemonQa</Arguments>"#,
        ] {
            assert!(xml.contains(fragment), "missing {fragment} in\n{xml}");
        }
        assert!(
            !xml.contains("HighestAvailable"),
            "the task must never request elevation"
        );
        assert_eq!(
            task_command(&xml).as_deref(),
            Some(r"C:\Program Files\Ferryx\ferryx.exe")
        );
        let escaped = render_task_xml("u", r"C:\a&b\<x>.exe", SUPERVISOR_ARG);
        assert_eq!(task_command(&escaped).as_deref(), Some(r"C:\a&b\<x>.exe"));
        assert_eq!(
            decode_utf16_with_bom(&encode_utf16_with_bom(&xml)).as_deref(),
            Some(xml.as_str())
        );

        // The arguments must come back unchanged through the watchdog's own parser.
        assert_eq!(quote_windows_arg(r#"say "hi""#), r#""say \"hi\"""#);
        assert_eq!(
            parse_supervisor_args(&[SUPERVISOR_ARG, ENV_ARG, r"FERRYX_RUNTIME_DIR=C:\qa dir\"]),
            Some(Ok(vec![(
                "FERRYX_RUNTIME_DIR".to_string(),
                r"C:\qa dir\".to_string()
            )]))
        );
        assert_eq!(parse_supervisor_args(&["--daemon"]), None);
        assert!(matches!(
            parse_supervisor_args(&[SUPERVISOR_ARG, ENV_ARG, "PATH=x"]),
            Some(Err(_))
        ));
        assert_eq!(
            task_name(&env_of(&[(SUPERVISOR_NAME_ENV, "DaemonQa")])),
            "Ferryx\\DaemonQa"
        );
        assert_eq!(task_name(&env_of(&[])), "Ferryx\\Daemon");
    }

    #[test]
    fn respawns_only_on_nonzero_exit() {
        let now = Instant::now();
        let mut watchdog = Watchdog::new();
        assert_eq!(
            watchdog.on_exit(10, Some(0), None, now),
            WatchdogAction::Stop
        );
        assert_eq!(
            watchdog.on_exit(10, Some(1), None, now),
            WatchdogAction::Respawn {
                after: RESPAWN_BACKOFF
            }
        );
        assert_eq!(
            watchdog.on_exit(11, None, None, now + Duration::from_secs(5)),
            WatchdogAction::Respawn {
                after: RESPAWN_BACKOFF
            },
            "an unreadable exit status is not a deliberate exit"
        );
        assert_eq!(
            watchdog.on_exit(12, Some(0), Some(12), now),
            WatchdogAction::Stop,
            "the exited process is never its own successor"
        );
    }

    #[test]
    fn follows_successor_after_exit_zero() {
        let mut watchdog = Watchdog::new();
        let now = Instant::now();
        assert_eq!(
            watchdog.on_exit(10, Some(0), Some(20), now),
            WatchdogAction::Follow(20)
        );
        assert_eq!(
            watchdog.on_exit(20, Some(1), Some(30), now),
            WatchdogAction::Follow(30),
            "a lock-race loser exits while the winner serves; follow the winner"
        );
    }

    #[test]
    fn backs_off_after_repeated_crashes() {
        let mut watchdog = Watchdog::new();
        let start = Instant::now();
        let mut now = start;
        for _ in 0..MAX_CRASHES_PER_WINDOW {
            assert_eq!(
                watchdog.on_exit(1, Some(1), None, now),
                WatchdogAction::Respawn {
                    after: RESPAWN_BACKOFF
                }
            );
            now += RESPAWN_BACKOFF;
        }
        let WatchdogAction::Respawn { after } = watchdog.on_exit(1, Some(1), None, now) else {
            panic!("a crash is always respawned");
        };
        assert!(
            after > Duration::from_secs(50),
            "the sixth crash in a minute must wait, got {after:?}"
        );
        let restart_at = now + after;
        assert!(
            restart_at.duration_since(start) >= CRASH_WINDOW,
            "at most 5 restarts per minute"
        );
        assert_eq!(
            watchdog.on_exit(1, Some(1), None, restart_at + Duration::from_secs(120)),
            WatchdogAction::Respawn {
                after: RESPAWN_BACKOFF
            },
            "the cap resets once the window has passed"
        );
    }

    #[test]
    fn stops_when_no_daemon_remains() {
        let mut watchdog = Watchdog::new();
        let now = Instant::now();
        watchdog.on_exit(1, Some(3), None, now);
        assert_eq!(
            watchdog.on_exit(2, Some(0), None, now),
            WatchdogAction::Stop
        );
    }

    #[test]
    fn store_installs_are_unsupported() {
        assert!(!supported_for_install(true));
        assert!(supported_for_install(false));
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        local: PathBuf,
        exe: PathBuf,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let local = dir.path().join("LocalAppData");
        let exe = dir.path().join("ferryx.exe");
        std::fs::write(&exe, b"").unwrap();
        Fixture {
            local,
            exe,
            _dir: dir,
        }
    }

    fn run_ensure(
        f: &Fixture,
        present: bool,
        live: bool,
        after: AfterRegister,
    ) -> (Result<bool>, Vec<String>) {
        let env = env_of(&[
            ("LOCALAPPDATA", f.local.to_str().unwrap()),
            ("USERNAME", "sook"),
            ("USERDOMAIN", "DESKTOP-1"),
        ]);
        let mut calls = Vec::new();
        let mut schtasks = |args: &[&str]| {
            calls.push(args.join(" "));
            Ok(args[0] != "/Query" || present)
        };
        let result = ensure_with(&env, &f.exe, &mut schtasks, &|| live, after);
        (result, calls)
    }

    #[test]
    fn task_xml_is_written_and_created_only_when_changed() {
        let f = fixture();
        let (result, calls) = run_ensure(&f, false, false, AfterRegister::RunWhenIdle);
        assert_eq!(result, Ok(true));
        // A new definition is created with /F straight away; querying first would add nothing.
        assert!(
            calls[0].starts_with("/Create /TN Ferryx\\Daemon /XML "),
            "{calls:?}"
        );
        assert_eq!(calls[1], "/Run /TN Ferryx\\Daemon");
        assert_eq!(calls.len(), 2, "{calls:?}");
        let xml_path = f
            .local
            .join("Ferryx")
            .join("supervisor")
            .join("Ferryx-Daemon.xml");
        let written = read_task_xml(&xml_path).expect("utf-16 task xml");
        assert_eq!(task_command(&written).as_deref(), f.exe.to_str());

        let (result, calls) = run_ensure(&f, true, true, AfterRegister::RunWhenIdle);
        assert_eq!(
            result,
            Ok(false),
            "an unchanged, registered task with a live daemon is left alone"
        );
        assert_eq!(calls, vec!["/Query /TN Ferryx\\Daemon".to_string()]);

        let (_, calls) = run_ensure(&f, true, false, AfterRegister::RegisterOnly);
        assert!(
            !calls.iter().any(|call| call.starts_with("/Run")),
            "{calls:?}"
        );
    }

    #[test]
    fn stale_task_with_missing_exe_is_removed() {
        let f = fixture();
        let xml_path = f
            .local
            .join("Ferryx")
            .join("supervisor")
            .join("Ferryx-Daemon.xml");
        std::fs::create_dir_all(xml_path.parent().unwrap()).unwrap();
        let gone = f.local.join("uninstalled").join("ferryx.exe");
        std::fs::write(
            &xml_path,
            encode_utf16_with_bom(&render_task_xml(
                "u",
                gone.to_str().unwrap(),
                SUPERVISOR_ARG,
            )),
        )
        .unwrap();
        let (result, calls) = run_ensure(&f, true, true, AfterRegister::RunWhenIdle);
        assert_eq!(result, Ok(true));
        assert_eq!(calls[0], "/Delete /TN Ferryx\\Daemon /F");
        assert!(
            calls
                .iter()
                .any(|call| call.starts_with("/Create /TN Ferryx\\Daemon")),
            "{calls:?}"
        );
        let written = read_task_xml(&xml_path).unwrap();
        assert_eq!(task_command(&written).as_deref(), f.exe.to_str());
    }
}
