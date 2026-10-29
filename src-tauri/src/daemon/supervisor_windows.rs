//! Windows backend of [`crate::daemon::supervisor`]: one per-user scheduled task per daemon runtime
//! generation, each running that generation's watchdog.
//!
//! Every generation is a staged, immutable copy of the daemon runtime
//! ([`crate::daemon::runtime_stage`]) under `%LOCALAPPDATA%\Ferryx\daemon-runtime\<key>`, and gets
//! its own task `Ferryx\<name>-<key>` that runs `<staged ferryx.exe> --daemon-supervisor`. A task
//! never points at the GUI's executable, so replacing or removing the app leaves every running
//! daemon's binary untouched.
//!
//! Task Scheduler cannot restart the daemon by itself. Todo 21 measured that `RestartOnFailure`
//! relaunches neither after a non-zero exit nor after `taskkill /F`: it only covers a task that
//! fails to *start*. So the watchdog spawns `<its own staged exe> --daemon` outside the task's job,
//! waits on the process handle, and:
//!
//! * respawns after a non-zero exit (1 s backoff, at most [`MAX_CRASHES_PER_WINDOW`] per minute);
//! * after exit 0, stops at once when the daemon left a retirement marker; otherwise looks for a
//!   successor through the runtime dir's `daemon.port` and waits on that process instead;
//! * stops without spawning whenever a daemon of another protocol version serves the endpoint: that
//!   daemon belongs to another generation, which has its own task;
//! * exits once a daemon left with exit 0 and no successor took over.
//!
//! Registration never touches a running instance: an existing task is never recreated, deleted or
//! ended, and it is `/Run` only when no daemon owner exists. Only the newest generation keeps the
//! logon trigger; older tasks lose it through a definition re-import (`/Query /XML`, then
//! `/Create /XML /F`), which Task Scheduler applies to future runs only.
//!
//! The task is per user (`InteractiveToken`, `LeastPrivilege`), never a service and never elevated.
//! Store (MSIX) installs are unsupported: the Store owns their lifecycle.
#![cfg_attr(not(windows), allow(dead_code))]

use super::owner_admission::{Endpoint, LockState, OwnerProbe, OwnerRecord, StartingRecord};
use super::runtime_stage::{self, RuntimeUsage, StagedRuntime};
use super::supervisor::{non_empty, Env, Result, SUPERVISOR_NAME_ENV};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Base task name under the `Ferryx` task folder when no QA override is set. Generation tasks append
/// `-<generation key>`; the bare name is the legacy single task (todo 14 migrates it).
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

/// QA barrier channel of a `local-split-qa` build (todo 12), forwarded beside [`FORWARDED_ENV`]
/// only into a QA-named task. FERRYX_QA_DISABLE_CONTROL reaches the task's daemon only this way:
/// the GUI never starts the daemon itself (only3 Q09 part 3 got a daemon with control bound).
#[cfg(feature = "local-split-qa")]
pub const QA_FORWARDED_ENV: [&str; 4] = [
    "FERRYX_QA_BARRIER_DIR",
    "FERRYX_QA_RUN_ID",
    crate::ssh::direct::QA_SSH_KNOWN_HOSTS_ENV,
    "FERRYX_QA_DISABLE_CONTROL",
];
#[cfg(not(feature = "local-split-qa"))]
pub const QA_FORWARDED_ENV: [&str; 0] = [];

pub const RESPAWN_BACKOFF: Duration = Duration::from_secs(1);
pub const CRASH_WINDOW: Duration = Duration::from_secs(60);
pub const MAX_CRASHES_PER_WINDOW: usize = 5;
/// How long an exit 0 waits for a successor to publish its endpoint. A successor waits up to 15 s
/// for the predecessor's instance lock (`handover::successor_lock_wait`) before it binds.
pub const SUCCESSOR_WAIT: Duration = Duration::from_secs(20);

const LOGON_TRIGGER_OPEN: &str = "<LogonTrigger";
const LOGON_TRIGGER_CLOSE: &str = "</LogonTrigger>";
const RESTART_ON_FAILURE_OPEN: &str = "<RestartOnFailure";
const RESTART_ON_FAILURE_CLOSE: &str = "</RestartOnFailure>";

/// Store/MSIX installs never get a task: the Store owns their install and update lifecycle.
pub fn supported_for_install(managed_externally: bool) -> bool {
    !managed_externally
}

/// The base task name (`Ferryx\Daemon`, or the QA override). On its own it names the legacy task.
pub fn task_name(env: Env<'_>) -> String {
    let name = non_empty(env, SUPERVISOR_NAME_ENV)
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| DEFAULT_TASK_NAME.to_string());
    format!("Ferryx\\{name}")
}

/// The task of one runtime generation: `<task_name>-<generation key>`.
pub fn generation_task_name(env: Env<'_>, key: &str) -> String {
    format!("{}-{key}", task_name(env))
}

/// Where this user's task definitions are kept, under the user's own local app data.
fn supervisor_dir(env: Env<'_>) -> Result<PathBuf> {
    let base = non_empty(env, "LOCALAPPDATA").ok_or("LOCALAPPDATA is not set")?;
    Ok(PathBuf::from(base).join("Ferryx").join("supervisor"))
}

/// The definition file of one task: what `/Create /XML` imported, one file per task name.
fn task_xml_path(env: Env<'_>, name: &str) -> Result<PathBuf> {
    Ok(supervisor_dir(env)?.join(format!("{}.xml", name.replace('\\', "-"))))
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
    let qa = if non_empty(env, SUPERVISOR_NAME_ENV).is_some() { &QA_FORWARDED_ENV[..] } else { &[][..] };
    FORWARDED_ENV
        .iter()
        .chain(qa.iter())
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

/// The task definition without every `open`..`close` element (and the whitespace that led into
/// it), everything else byte-identical. An unterminated element is left as it is.
fn strip_element(xml: &str, open: &str, close: &str) -> String {
    let mut stripped = xml.to_string();
    while let Some(start) = stripped.find(open) {
        let Some(end_offset) = stripped[start..].find(close) else {
            break;
        };
        let end = start + end_offset + close.len();
        let line_start = stripped[..start].trim_end_matches([' ', '\t']).len();
        let line_start = stripped[..line_start]
            .strip_suffix("\r\n")
            .or_else(|| stripped[..line_start].strip_suffix('\n'))
            .map_or(line_start, str::len);
        stripped.replace_range(line_start..end, "");
    }
    stripped
}

/// The task definition without its `<LogonTrigger>` element (and the whitespace that led into it),
/// everything else byte-identical. Used to demote an older generation: a definition change applies
/// to future runs and never stops a running instance.
pub fn strip_logon_trigger(xml: &str) -> String {
    strip_element(xml, LOGON_TRIGGER_OPEN, LOGON_TRIGGER_CLOSE)
}

/// The task definition without any condition that starts it on its own: the logon trigger and
/// `RestartOnFailure` (which retries a start that failed). Used for the autostart opt-out, so a
/// task starts only on an explicit `/Run`. Like any definition change, it applies to future runs
/// and never stops a running instance.
pub fn strip_login_launch(xml: &str) -> String {
    strip_element(
        &strip_logon_trigger(xml),
        RESTART_ON_FAILURE_OPEN,
        RESTART_ON_FAILURE_CLOSE,
    )
}

pub fn has_logon_trigger(xml: &str) -> bool {
    xml.contains(LOGON_TRIGGER_OPEN)
}

/// Whether the definition still has a condition that starts the task without an explicit `/Run`.
pub fn has_login_launch(xml: &str) -> bool {
    has_logon_trigger(xml) || xml.contains(RESTART_ON_FAILURE_OPEN)
}

/// `schtasks /Query /XML` prints UTF-16 or the console code page depending on how it is piped.
fn decode_task_xml(bytes: &[u8]) -> String {
    if let Some(text) = decode_utf16_with_bom(bytes) {
        return text;
    }
    let looks_utf16 = bytes.len() >= 2 && bytes.len().is_multiple_of(2) && bytes[1] == 0;
    if looks_utf16 {
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        if let Ok(text) = String::from_utf16(&units) {
            return text;
        }
    }
    String::from_utf8_lossy(bytes).into_owned()
}

fn write_task_xml(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create the supervisor task directory: {e}"))?;
    }
    std::fs::write(path, encode_utf16_with_bom(content))
        .map_err(|e| format!("Failed to write the supervisor task XML: {e}"))
}

/// The Task Scheduler operations registration may issue, injected so tests need no real schtasks.
pub trait TaskScheduler {
    /// One `schtasks` invocation: `Ok(true)` when it exited 0, `Ok(false)` when it ran and failed.
    fn run(&mut self, args: &[&str]) -> Result<bool>;
    /// `schtasks /Query /TN <name> /XML`: the registered definition, `Ok(None)` when no such task.
    fn query_xml(&mut self, name: &str) -> Result<Option<String>>;
    /// The task's `MSFT_ScheduledTask` State enum name (`Ready`, `Running`, `Queued`, `Disabled`, ...),
    /// which is the same on every display language; `Err` when it cannot be read.
    fn task_state(&mut self, name: &str) -> Result<String>;
}

/// Registers one task definition from `content` (`/Create /XML /F`), never `/Run`.
fn import_definition(
    scheduler: &mut dyn TaskScheduler,
    name: &str,
    xml_path: &Path,
    content: &str,
) -> Result<()> {
    write_task_xml(xml_path, content)?;
    let xml_arg = xml_path.to_string_lossy();
    if !scheduler.run(&["/Create", "/TN", name, "/XML", &xml_arg, "/F"])? {
        // Leave no definition behind that claims to be registered.
        let _ = std::fs::remove_file(xml_path);
        return Err(format!("schtasks /Create failed for task {name}"));
    }
    Ok(())
}

/// The definition of `staged`'s generation task: its own staged executable, never the GUI's.
fn generation_definition(env: Env<'_>, staged: &StagedRuntime) -> Result<String> {
    Ok(render_task_xml(
        &task_user(env)?,
        &staged.executable.to_string_lossy(),
        &task_arguments(&forwarded_env(env)),
    ))
}

/// Ensures the task of `staged`'s generation and starts it only for an Absent owner.
///
/// `/Create` runs only when the task is missing. An existing task is never recreated, deleted or
/// ended, whatever executable or definition it has: the instance it runs may own every terminal.
/// `/Run` is issued only when `owner_absent` (admission's classify_owner said Absent); an
/// Alive or Unknown owner gets zero mutations of any running job. Returns whether a start
/// (`/Run`) was issued.
///
/// `login_autostart` false (the user opted out of autostart) creates a missing task without its
/// logon trigger and `RestartOnFailure`, so the start still goes through the task (never a GUI
/// child) but nothing starts it again at the next logon.
pub fn ensure_with(
    env: Env<'_>,
    staged: &StagedRuntime,
    scheduler: &mut dyn TaskScheduler,
    owner_absent: bool,
    login_autostart: bool,
) -> Result<bool> {
    let name = generation_task_name(env, &staged.key);
    if !scheduler.run(&["/Query", "/TN", &name])? {
        let xml_path = task_xml_path(env, &name)?;
        let definition = generation_definition(env, staged)?;
        let definition = if login_autostart {
            definition
        } else {
            strip_login_launch(&definition)
        };
        import_definition(scheduler, &name, &xml_path, &definition)?;
    }
    if !owner_absent {
        return Ok(false);
    }
    if !scheduler.run(&["/Run", "/TN", &name])? {
        return Err(format!("schtasks /Run failed for task {name}"));
    }
    Ok(true)
}

/// Registers `staged` as the newest generation for the next logon without starting anything, and
/// removes the logon trigger from every older generation task. Never `/Run`, `/End` or `/Delete`:
/// both changes are definition re-imports that apply to future runs only. Returns whether any
/// definition changed.
pub fn register_for_login_with(
    env: Env<'_>,
    staged: &StagedRuntime,
    scheduler: &mut dyn TaskScheduler,
) -> Result<bool> {
    let newest = generation_task_name(env, &staged.key);
    let newest_path = task_xml_path(env, &newest)?;
    let content = generation_definition(env, staged)?;
    let mut changed = false;
    let unchanged = read_task_xml(&newest_path).as_deref() == Some(content.as_str());
    if !unchanged || !scheduler.run(&["/Query", "/TN", &newest])? {
        import_definition(scheduler, &newest, &newest_path, &content)?;
        changed = true;
    }
    for key in recorded_generation_keys(env)? {
        if key == staged.key {
            continue;
        }
        changed |= remove_logon_trigger(env, scheduler, &generation_task_name(env, &key))?;
    }
    Ok(changed)
}

/// Deletes the tasks of superseded generations, after [`register_for_login_with`] removed their
/// logon trigger, together with their recorded definitions, so a task naming an old runtime no
/// longer keeps it from garbage collection. The candidates are
/// [`runtime_stage::superseded_job_keys`] over the recorded generations: `staged` and the next
/// newest generation keep their tasks, and so does every generation a running process executes
/// from. A registered candidate is deleted (`/Delete /F`) only when its State reads `Ready` or
/// `Disabled`: a `Running` or `Queued` task may host its watchdog and an unreadable State is no
/// proof of idleness, so such a task keeps its definition and its record. A candidate that is no
/// longer registered only loses its record. Never `/End` or `/Run`. Returns the removed keys.
pub fn remove_superseded_tasks_with(
    env: Env<'_>,
    staged: &StagedRuntime,
    runtime_root: &Path,
    scheduler: &mut dyn TaskScheduler,
    usage: &dyn RuntimeUsage,
) -> Result<Vec<String>> {
    let keys = recorded_generation_keys(env)?;
    let mut removed = Vec::new();
    for key in runtime_stage::superseded_job_keys(runtime_root, &keys, &staged.key, usage) {
        let name = generation_task_name(env, &key);
        if scheduler.run(&["/Query", "/TN", &name])? {
            let state = scheduler.task_state(&name);
            if !matches!(state.as_deref(), Ok("Ready" | "Disabled")) {
                tracing::info!(task = %name, ?state, "A superseded daemon task is not idle; it keeps its definition");
                continue;
            }
            if !scheduler.run(&["/Delete", "/TN", &name, "/F"])? {
                tracing::warn!(task = %name, "schtasks /Delete failed for a superseded daemon task");
                continue;
            }
        }
        let xml_path = task_xml_path(env, &name)?;
        match std::fs::remove_file(&xml_path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("Failed to remove {}: {error}", xml_path.display())),
        }
        removed.push(key);
    }
    Ok(removed)
}

/// Migrates the legacy single task (`Ferryx\Daemon`, or the QA base name) for the owner admission
/// classified under its lock (todo 14).
///
/// The legacy task is known by its recorded definition (`<task name>.xml`, written before every
/// `/Create` since the task was introduced); with no record there is nothing to migrate and no
/// `schtasks` call is made, so a second run after the migration issues nothing. With an Alive or
/// Unknown owner the task only loses its logon trigger through a definition re-import
/// (`/Query /XML`, then `/Create /XML /F`), which applies to future runs only: the running
/// watchdog and daemon keep going. With an Absent owner the task is deleted (`/Delete /F`, which
/// unregisters it) and its record removed, but only when its State reads `Ready` or `Disabled`: a
/// `Running` or `Queued` task may still host the legacy watchdog, and an unreadable State is not
/// proof of idleness, so such a task is kept and only loses its logon trigger. Never `/End` or
/// `/Run`. Returns whether anything changed.
pub fn migrate_legacy_task_with(
    env: Env<'_>,
    scheduler: &mut dyn TaskScheduler,
    owner_absent: bool,
) -> Result<bool> {
    let name = task_name(env);
    let xml_path = task_xml_path(env, &name)?;
    match std::fs::symlink_metadata(&xml_path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(format!("Failed to inspect {}: {error}", xml_path.display())),
    }
    if !owner_absent {
        return remove_logon_trigger(env, scheduler, &name);
    }
    let mut changed = false;
    if scheduler.run(&["/Query", "/TN", &name])? {
        let state = scheduler.task_state(&name);
        if !matches!(state.as_deref(), Ok("Ready" | "Disabled")) {
            tracing::info!(task = %name, ?state, "The legacy daemon task is not idle; it keeps running and only loses its logon trigger");
            return remove_logon_trigger(env, scheduler, &name);
        }
        if !scheduler.run(&["/Delete", "/TN", &name, "/F"])? {
            return Err(format!("schtasks /Delete failed for the legacy task {name}"));
        }
        changed = true;
    }
    match std::fs::remove_file(&xml_path) {
        Ok(()) => changed = true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Failed to remove {}: {error}", xml_path.display())),
    }
    Ok(changed)
}

/// Re-imports `name` without its logon trigger. The registered definition (`/Query /XML`) is
/// the source, so nothing else about the task changes. A task that is gone only loses its stale
/// definition file; a task already without the trigger is left alone.
fn remove_logon_trigger(
    env: Env<'_>,
    scheduler: &mut dyn TaskScheduler,
    name: &str,
) -> Result<bool> {
    let xml_path = task_xml_path(env, name)?;
    if read_task_xml(&xml_path).is_some_and(|recorded| !has_logon_trigger(&recorded)) {
        return Ok(false);
    }
    let Some(registered) = scheduler.query_xml(name)? else {
        let _ = std::fs::remove_file(&xml_path);
        return Ok(false);
    };
    if !has_logon_trigger(&registered) {
        write_task_xml(&xml_path, &registered)?;
        return Ok(false);
    }
    import_definition(scheduler, name, &xml_path, &strip_logon_trigger(&registered))?;
    Ok(true)
}

/// Autostart opt-out: every recorded generation task and the legacy task lose their logon trigger
/// and `RestartOnFailure` through a definition re-import (`/Query /XML`, then `/Create /XML /F`).
/// Never `/Run`, `/End` or `/Delete`: a running watchdog and its daemon keep going, and every
/// task stays registered so an opted-out GUI can still start the newest generation with `/Run`.
/// Every task is tried even when one fails; the failures are reported together. Returns whether
/// any definition changed.
pub fn remove_login_autostart_with(
    env: Env<'_>,
    scheduler: &mut dyn TaskScheduler,
) -> Result<bool> {
    let mut names: Vec<String> = recorded_generation_keys(env)?
        .iter()
        .map(|key| generation_task_name(env, key))
        .collect();
    names.push(task_name(env));
    let mut changed = false;
    let mut failures = Vec::new();
    for name in names {
        match remove_login_launch(env, scheduler, &name) {
            Ok(rewritten) => changed |= rewritten,
            Err(error) => failures.push(error),
        }
    }
    if failures.is_empty() {
        Ok(changed)
    } else {
        Err(failures.join("; "))
    }
}

/// Re-imports `name` without any login launch condition ([`strip_login_launch`]). The registered
/// definition is the source, so nothing else about the task changes. A task that is not
/// registered keeps its record (it may still protect a runtime from garbage collection); a task
/// that has no launch condition left is only recorded.
fn remove_login_launch(
    env: Env<'_>,
    scheduler: &mut dyn TaskScheduler,
    name: &str,
) -> Result<bool> {
    let xml_path = task_xml_path(env, name)?;
    if read_task_xml(&xml_path).is_some_and(|recorded| !has_login_launch(&recorded)) {
        return Ok(false);
    }
    let Some(registered) = scheduler.query_xml(name)? else {
        return Ok(false);
    };
    let stripped = strip_login_launch(&registered);
    if stripped == registered {
        write_task_xml(&xml_path, &registered)?;
        return Ok(false);
    }
    import_definition(scheduler, name, &xml_path, &stripped)?;
    Ok(true)
}

/// Generation keys of every task definition this user recorded (`<task name>-<key>.xml`). The
/// legacy single task (`<task name>.xml`) is not a generation and is left to the migration.
fn recorded_generation_keys(env: Env<'_>) -> Result<Vec<String>> {
    let dir = supervisor_dir(env)?;
    let prefix = format!("{}-", task_name(env).replace('\\', "-"));
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("Failed to read {}: {error}", dir.display())),
    };
    let mut keys: Vec<String> = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let file_name = entry.file_name().to_string_lossy().into_owned();
            let key = file_name.strip_prefix(&prefix)?.strip_suffix(".xml")?;
            (!key.is_empty()).then(|| key.to_string())
        })
        .collect();
    keys.sort();
    Ok(keys)
}

/// Executables named by every recorded task definition (generations and the legacy task), so
/// runtime garbage collection never removes a runtime a registered task still launches.
pub fn registered_executables(env: Env<'_>) -> Vec<PathBuf> {
    let Ok(dir) = supervisor_dir(env) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "xml"))
        .filter_map(|entry| read_task_xml(&entry.path()))
        .filter_map(|xml| task_command(&xml))
        .map(PathBuf::from)
        .collect()
}

/// The daemon executable a watchdog spawns: its own staged runtime (`<root>/<key>/ferryx.exe`),
/// never the GUI's executable. `current_exe` and `runtime_root` are already resolved.
pub fn staged_daemon_executable(current_exe: &Path, runtime_root: &Path) -> Result<PathBuf> {
    let inside = current_exe.strip_prefix(runtime_root).map_err(|_| {
        format!(
            "the watchdog runs from {}, outside the staged daemon runtime {}",
            current_exe.display(),
            runtime_root.display()
        )
    })?;
    if inside.components().count() != 2 {
        return Err(format!(
            "{} is not a generation executable under {}",
            current_exe.display(),
            runtime_root.display()
        ));
    }
    Ok(current_exe.to_path_buf())
}

/// The generation key of a staged executable: the name of its generation directory.
pub fn generation_key_of(staged_executable: &Path) -> Option<String> {
    Some(
        staged_executable
            .parent()?
            .file_name()?
            .to_string_lossy()
            .into_owned(),
    )
}

/// File names a retiring daemon may use for its marker (`control_server::finish_control_retirement`
/// names it by generation key, or by pid while the daemon does not know its key).
pub fn retirement_marker_names(generation_key: Option<&str>, exited_pid: u32) -> Vec<String> {
    let mut names: Vec<String> = generation_key
        .map(|key| format!("retired-{key}.json"))
        .into_iter()
        .collect();
    names.push(format!("retired-{exited_pid}.json"));
    names
}

/// Whether a retirement marker was written by the daemon that just exited. A marker left by an
/// earlier daemon of the same generation names another pid and is ignored.
pub fn marker_names_pid(body: &str, exited_pid: u32) -> bool {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| value.get("pid")?.as_u64())
        == Some(u64::from(exited_pid))
}

/// Delegates to an owner probe but drops a pending-start record this very process wrote: admission
/// records its own start before it asks the backend to issue it, and that record must not make the
/// same start look pending (row 5b) to itself.
pub(crate) struct IgnoreOwnPendingStart<'a>(pub &'a dyn OwnerProbe);

impl OwnerProbe for IgnoreOwnPendingStart<'_> {
    fn lock_state(&self) -> LockState {
        self.0.lock_state()
    }
    fn read_record(&self) -> std::result::Result<Option<OwnerRecord>, String> {
        self.0.read_record()
    }
    fn process_start_time(&self, pid: u32) -> Option<u64> {
        self.0.process_start_time(pid)
    }
    fn endpoint(&self) -> std::result::Result<Endpoint, String> {
        self.0.endpoint()
    }
    fn read_starting(&self) -> std::result::Result<Option<StartingRecord>, String> {
        Ok(self
            .0
            .read_starting()?
            .filter(|starting| starting.launcher_pid != std::process::id()))
    }
    fn now_unix_ms(&self) -> u64 {
        self.0.now_unix_ms()
    }
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
        if !FORWARDED_ENV.contains(&key) && !QA_FORWARDED_ENV.contains(&key) {
            return Some(Err(format!(
                "{key} may not be forwarded to the daemon supervisor"
            )));
        }
        forwarded.push((key.to_string(), value.to_string()));
    }
    Some(Ok(forwarded))
}

/// What the endpoint showed the watchdog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seen {
    /// A daemon of this protocol version, with the pid its handshake reported.
    Owner(u32),
    /// A daemon of another protocol version: it belongs to another generation, whose own task
    /// supervises it.
    OtherVersion,
    /// Nothing answered.
    Nothing,
}

/// The side effects of the watchdog loop, injected so the loop's decisions are testable anywhere.
pub trait WatchdogHost {
    fn probe(&mut self) -> Seen;
    /// After an exit 0: probe until a successor other than `exited_pid` answers, another protocol
    /// version answers, or [`SUCCESSOR_WAIT`] passes (`Seen::Nothing`).
    fn wait_for_successor(&mut self, exited_pid: u32) -> Seen;
    /// Spawns this generation's daemon and returns its pid.
    fn spawn(&mut self) -> Result<u32>;
    /// Waits for `pid` to exit; `Ok(None)` when its exit code cannot be read.
    fn wait(&mut self, pid: u32) -> Result<Option<u32>>;
    /// Whether `exited_pid` left a retirement marker before its deliberate exit.
    fn retired(&mut self, exited_pid: u32) -> bool;
    fn sleep(&mut self, duration: Duration);
    fn log(&mut self, message: &str);
}

/// The first process to wait on: a live daemon of this protocol version is followed, nothing
/// answering spawns one, and a daemon of another version means stop (`None`) without spawning.
fn begin(host: &mut dyn WatchdogHost) -> Option<Result<u32>> {
    match host.probe() {
        Seen::Owner(owner) => {
            host.log(&format!("following live daemon {owner}"));
            Some(Ok(owner))
        }
        Seen::OtherVersion => {
            host.log("a daemon of another protocol version serves the endpoint; stopping");
            None
        }
        Seen::Nothing => Some(host.spawn()),
    }
}

/// The `--daemon-supervisor` main loop over `host`. Returns the process exit code.
pub fn run_watchdog_with(host: &mut dyn WatchdogHost) -> i32 {
    let mut watchdog = Watchdog::new();
    let mut pid = match begin(host) {
        Some(Ok(pid)) => pid,
        Some(Err(error)) => {
            host.log(&format!("cannot start: {error}"));
            return 1;
        }
        None => return 0,
    };
    loop {
        let code = match host.wait(pid) {
            Ok(code) => code,
            Err(error) => {
                host.log(&format!("cannot supervise: {error}"));
                return 1;
            }
        };
        let seen = if code == Some(0) {
            if host.retired(pid) {
                host.log(&format!("daemon {pid} retired deliberately; stopping"));
                return 0;
            }
            host.wait_for_successor(pid)
        } else {
            host.probe()
        };
        let owner = match seen {
            Seen::Owner(owner) => Some(owner),
            Seen::Nothing => None,
            Seen::OtherVersion => {
                host.log(&format!(
                    "daemon {pid} exited code={code:?}; another protocol version serves the endpoint; stopping"
                ));
                return 0;
            }
        };
        let action = watchdog.on_exit(pid, code, owner, Instant::now());
        host.log(&format!(
            "daemon {pid} exited code={code:?} owner={owner:?} -> {action:?}"
        ));
        pid = match action {
            WatchdogAction::Stop => return 0,
            WatchdogAction::Follow(owner) => owner,
            WatchdogAction::Respawn { after } => {
                host.sleep(after);
                match begin(host) {
                    Some(Ok(pid)) => pid,
                    Some(Err(error)) => {
                        host.log(&format!("respawn failed: {error}"));
                        return 1;
                    }
                    None => return 0,
                }
            }
        };
    }
}

#[cfg(windows)]
pub use platform::{
    ensure, installed, register_for_login, register_staged_for_login, reload, remove_autostart,
    run_watchdog, uninstall,
};

#[cfg(windows)]
mod platform {
    use super::*;
    use crate::daemon::owner_admission::{
        classify_owner, AdmissionPaths, OwnerState, RuntimeProbe, SUPERVISOR_PID_ENV,
        SUPERVISOR_START_ENV,
    };
    use crate::daemon::protocol::{DaemonRequest, DaemonResponse, DAEMON_PROTOCOL_VERSION};
    use crate::daemon::runtime_stage::{self, SystemRuntimeUsage};
    use std::collections::HashMap;
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

    /// The real Task Scheduler.
    pub(crate) struct SystemScheduler;

    impl TaskScheduler for SystemScheduler {
        fn run(&mut self, args: &[&str]) -> Result<bool> {
            crate::util::no_window_command("schtasks")
                .args(args)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map(|status| status.success())
                .map_err(|e| format!("Failed to run schtasks: {e}"))
        }

        fn query_xml(&mut self, name: &str) -> Result<Option<String>> {
            let output = crate::util::no_window_command("schtasks")
                .args(["/Query", "/TN", name, "/XML"])
                .stdin(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .output()
                .map_err(|e| format!("Failed to run schtasks: {e}"))?;
            if !output.status.success() {
                return Ok(None);
            }
            Ok(Some(decode_task_xml(&output.stdout)))
        }

        fn task_state(&mut self, name: &str) -> Result<String> {
            // schtasks' Status column is localized; the MSFT_ScheduledTask State enum prints the
            // invariant name on every display language.
            let (folder, leaf) = name.rsplit_once('\\').unwrap_or(("", name));
            let task_path = if folder.is_empty() {
                "\\".to_string()
            } else {
                format!("\\{folder}\\")
            };
            let quote = |value: &str| value.replace('\'', "''");
            let output = crate::util::no_window_command("powershell")
                .args([
                    "-NoProfile",
                    "-Command",
                    &format!(
                        "(Get-ScheduledTask -TaskPath '{}' -TaskName '{}').State",
                        quote(&task_path),
                        quote(leaf)
                    ),
                ])
                .stdin(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .output()
                .map_err(|e| format!("Failed to run powershell: {e}"))?;
            if !output.status.success() {
                return Err(format!("Get-ScheduledTask failed for task {name}"));
            }
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        }
    }

    /// Stages this executable as an immutable runtime generation; tasks only ever run that copy.
    fn stage(executable: &Path) -> Result<StagedRuntime> {
        let root = crate::daemon::server::daemon_runtime_root()
            .ok_or("Cannot determine the daemon runtime directory")?;
        runtime_stage::stage_runtime(&root, executable).map_err(|e| e.to_string())
    }

    /// Owner admission's verdict right before a `/Run`. Callers reach this only through admission
    /// (which already wrote this process's starting.json), so that record is not a pending start.
    fn owner_absent() -> bool {
        let paths = AdmissionPaths::for_runtime();
        let probe = RuntimeProbe { paths: &paths };
        let state = classify_owner(&IgnoreOwnPendingStart(&probe));
        if state != OwnerState::Absent {
            tracing::info!(?state, "A daemon owner exists; the generation task is not run");
        }
        state == OwnerState::Absent
    }

    pub fn ensure(env: Env<'_>, executable: &Path, login_autostart: bool) -> Result<bool> {
        let staged = stage(executable)?;
        ensure_with(
            env,
            &staged,
            &mut SystemScheduler,
            owner_absent(),
            login_autostart,
        )
    }

    /// Autostart opt-out on every generation task and the legacy task; never `/End` or `/Delete`.
    pub fn remove_autostart(env: Env<'_>) -> Result<()> {
        remove_login_autostart_with(env, &mut SystemScheduler).map(|_| ())
    }

    /// Registers this build's generation for the next logon and demotes older generations, deletes
    /// the idle tasks of superseded generations, then collects runtime generations that neither a
    /// process nor a recorded task uses.
    pub fn register_for_login(env: Env<'_>, executable: &Path) -> Result<bool> {
        let staged = stage(executable)?;
        register_staged_for_login(env, &staged)
    }

    /// [`register_for_login`] for an already staged generation (the running daemon's own).
    pub fn register_staged_for_login(env: Env<'_>, staged: &StagedRuntime) -> Result<bool> {
        let changed = register_for_login_with(env, staged, &mut SystemScheduler)?;
        if let Some(root) = crate::daemon::server::daemon_runtime_root() {
            let processes = SystemRuntimeUsage {
                registered_job_executables: Vec::new(),
            };
            match remove_superseded_tasks_with(env, staged, &root, &mut SystemScheduler, &processes)
            {
                Ok(removed) if !removed.is_empty() => {
                    tracing::info!(?removed, "Removed superseded daemon generation tasks")
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(%error, "Could not remove superseded daemon generation tasks")
                }
            }
            let usage = SystemRuntimeUsage {
                registered_job_executables: registered_executables(env),
            };
            match runtime_stage::collect_garbage(&root, &usage) {
                Ok(removed) if !removed.is_empty() => {
                    tracing::info!(?removed, "Removed unused daemon runtime generations")
                }
                Ok(_) => {}
                Err(error) => tracing::warn!(%error, "Daemon runtime garbage collection failed"),
            }
        }
        Ok(changed)
    }

    /// [`migrate_legacy_task_with`] against the real Task Scheduler.
    pub fn migrate_legacy_task(env: Env<'_>, owner_absent: bool) -> Result<bool> {
        migrate_legacy_task_with(env, &mut SystemScheduler, owner_absent)
    }

    /// Starts this build's generation only when no owner exists; never restarts a running one.
    pub fn reload(env: Env<'_>) -> Result<()> {
        let executable =
            std::env::current_exe().map_err(|e| format!("Failed to get current exe path: {e}"))?;
        ensure(
            env,
            &executable,
            !crate::daemon::supervisor::user_opted_out(),
        )
        .map(|_| ())
    }

    pub fn installed(env: Env<'_>) -> bool {
        let Ok(keys) = recorded_generation_keys(env) else {
            return false;
        };
        keys.iter().any(|key| {
            SystemScheduler
                .run(&["/Query", "/TN", &generation_task_name(env, key)])
                .unwrap_or(false)
        })
    }

    /// Deletes every recorded generation task, only while no daemon owner exists: a task whose
    /// owner is Alive or Unknown is never deleted.
    pub fn uninstall(env: Env<'_>) -> Result<()> {
        if !owner_absent() {
            return Err(
                "A daemon is running or cannot be identified; its tasks are left in place"
                    .to_string(),
            );
        }
        for key in recorded_generation_keys(env)? {
            let name = generation_task_name(env, &key);
            if SystemScheduler.run(&["/Query", "/TN", &name])?
                && !SystemScheduler.run(&["/Delete", "/TN", &name, "/F"])?
            {
                return Err(format!("schtasks /Delete failed for task {name}"));
            }
            if let Ok(path) = task_xml_path(env, &name) {
                let _ = std::fs::remove_file(path);
            }
        }
        Ok(())
    }

    fn probe_endpoint() -> Seen {
        probe_endpoint_once().unwrap_or(Seen::Nothing)
    }

    fn probe_endpoint_once() -> Option<Seen> {
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
            Ok(DaemonResponse::HandshakeOk { pid, .. }) => Some(Seen::Owner(pid)),
            Ok(DaemonResponse::ProtocolMismatch { .. }) => Some(Seen::OtherVersion),
            _ => None,
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

    /// This watchdog's own staged executable, which is also the daemon it spawns. Never the GUI's
    /// executable: a watchdog that does not run from the staged runtime refuses to spawn.
    fn daemon_executable() -> Result<PathBuf> {
        let current =
            std::env::current_exe().map_err(|e| format!("Failed to get current exe path: {e}"))?;
        let root = crate::daemon::server::daemon_runtime_root()
            .ok_or("Cannot determine the daemon runtime directory")?;
        let canonical = |path: &Path| std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        staged_daemon_executable(&canonical(&current), &canonical(&root))
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

    /// The real watchdog side effects. Spawned children are waited on through their own handle.
    struct SystemHost {
        children: HashMap<u32, std::process::Child>,
    }

    impl WatchdogHost for SystemHost {
        fn probe(&mut self) -> Seen {
            probe_endpoint()
        }

        fn wait_for_successor(&mut self, exited_pid: u32) -> Seen {
            let deadline = Instant::now() + SUCCESSOR_WAIT;
            loop {
                match probe_endpoint() {
                    Seen::Owner(owner) if owner != exited_pid => return Seen::Owner(owner),
                    Seen::OtherVersion => return Seen::OtherVersion,
                    _ => {}
                }
                if Instant::now() >= deadline {
                    return Seen::Nothing;
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        }

        fn spawn(&mut self) -> Result<u32> {
            let exe = daemon_executable()?;
            let mut command = std::process::Command::new(&exe);
            command
                .arg("--daemon")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            // The daemon records its watchdog in owner.json (admission row 5c).
            let own_pid = std::process::id();
            if let Some(start) = crate::daemon::owner_admission::process_start_time(own_pid) {
                command
                    .env(SUPERVISOR_PID_ENV, own_pid.to_string())
                    .env(SUPERVISOR_START_ENV, start.to_string());
            }
            crate::util::detach_launched_child(&mut command);
            let child = command
                .spawn()
                .map_err(|e| format!("Failed to spawn {} --daemon: {e}", exe.display()))?;
            let pid = child.id();
            log(&format!(
                "spawned daemon {pid} from {} (creation flags {:#x})",
                exe.display(),
                crate::util::windows_detach_flags()
            ));
            self.children.insert(pid, child);
            Ok(pid)
        }

        fn wait(&mut self, pid: u32) -> Result<Option<u32>> {
            match self.children.remove(&pid) {
                Some(mut child) => Ok(child
                    .wait()
                    .ok()
                    .and_then(|status| status.code())
                    .map(|code| code as u32)),
                None => wait_for_pid(pid),
            }
        }

        fn retired(&mut self, exited_pid: u32) -> bool {
            let key = daemon_executable()
                .ok()
                .and_then(|exe| generation_key_of(&exe));
            let runtime_dir = crate::daemon::server::get_runtime_dir();
            retirement_marker_names(key.as_deref(), exited_pid)
                .iter()
                .filter_map(|name| std::fs::read_to_string(runtime_dir.join(name)).ok())
                .any(|body| marker_names_pid(&body, exited_pid))
        }

        fn sleep(&mut self, duration: Duration) {
            std::thread::sleep(duration);
        }

        fn log(&mut self, message: &str) {
            log(message);
        }
    }

    /// The `--daemon-supervisor` main loop. Returns the process exit code.
    pub fn run_watchdog() -> i32 {
        log("watchdog started");
        run_watchdog_with(&mut SystemHost {
            children: HashMap::new(),
        })
    }
}
/// Kept apart from the re-export list above so the legacy migration (todo 14) merges cleanly with
/// lanes that edit that list.
#[cfg(windows)]
pub use platform::migrate_legacy_task;

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
        // QA barrier variables are forwarded only for a QA-named task, and accepted by the parser.
        let plain = env_of(&[("FERRYX_QA_BARRIER_DIR", r"C:\qa\b")]);
        assert!(forwarded_env(&plain).is_empty());
        let qa = env_of(&[(SUPERVISOR_NAME_ENV, "DaemonQa"), ("FERRYX_QA_BARRIER_DIR", r"C:\qa\b"), ("FERRYX_QA_RUN_ID", "r1")]);
        let keys: Vec<String> = forwarded_env(&qa).into_iter().map(|(key, _)| key).collect();
        if cfg!(feature = "local-split-qa") {
            assert_eq!(keys, [SUPERVISOR_NAME_ENV, "FERRYX_QA_BARRIER_DIR", "FERRYX_QA_RUN_ID"]);
            assert!(matches!(parse_supervisor_args(&[SUPERVISOR_ARG, ENV_ARG, "FERRYX_QA_RUN_ID=r1"]), Some(Ok(_))));
            // Q09 part 3: the no-control switch reaches the task's daemon only through the task.
            let off = env_of(&[(SUPERVISOR_NAME_ENV, "DaemonQa"), ("FERRYX_QA_DISABLE_CONTROL", "1")]);
            assert_eq!(forwarded_env(&off), [(SUPERVISOR_NAME_ENV.to_string(), "DaemonQa".to_string()), ("FERRYX_QA_DISABLE_CONTROL".to_string(), "1".to_string())]);
            assert!(forwarded_env(&env_of(&[("FERRYX_QA_DISABLE_CONTROL", "1")])).is_empty(), "never into a user task");
            assert!(matches!(parse_supervisor_args(&[SUPERVISOR_ARG, ENV_ARG, "FERRYX_QA_DISABLE_CONTROL=1"]), Some(Ok(_))));
        } else {
            assert_eq!(keys, [SUPERVISOR_NAME_ENV]);
            assert!(matches!(parse_supervisor_args(&[SUPERVISOR_ARG, ENV_ARG, "FERRYX_QA_RUN_ID=r1"]), Some(Err(_))));
        }
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

    #[test]
    fn task_name_is_per_generation() {
        assert_eq!(
            generation_task_name(&env_of(&[]), "2026.1018.1-ab12cd34"),
            "Ferryx\\Daemon-2026.1018.1-ab12cd34"
        );
        assert_eq!(
            generation_task_name(&env_of(&[(SUPERVISOR_NAME_ENV, "DaemonQa")]), "k"),
            "Ferryx\\DaemonQa-k"
        );
    }

    /// A fake Task Scheduler: records every call and answers `/Query` from a set of registered
    /// task names with the definitions it was given. `state` is the invariant State every task
    /// reports; `None` makes the State unreadable.
    #[derive(Default)]
    struct FakeScheduler {
        calls: Vec<String>,
        registered: HashMap<String, String>,
        state: Option<String>,
    }

    impl FakeScheduler {
        fn with_task(mut self, name: &str, xml: &str) -> Self {
            self.registered.insert(name.to_string(), xml.to_string());
            self
        }

        fn with_state(mut self, state: &str) -> Self {
            self.state = Some(state.to_string());
            self
        }

        fn mutations(&self) -> Vec<&String> {
            self.calls
                .iter()
                .filter(|call| !call.starts_with("/Query"))
                .collect()
        }
    }

    impl TaskScheduler for FakeScheduler {
        fn run(&mut self, args: &[&str]) -> Result<bool> {
            self.calls.push(args.join(" "));
            Ok(match args[0] {
                "/Query" => self.registered.contains_key(args[2]),
                "/Create" => {
                    let xml = read_task_xml(Path::new(args[4])).expect("utf-16 task xml");
                    self.registered.insert(args[2].to_string(), xml);
                    true
                }
                _ => true,
            })
        }

        fn query_xml(&mut self, name: &str) -> Result<Option<String>> {
            self.calls.push(format!("/Query /TN {name} /XML"));
            Ok(self.registered.get(name).cloned())
        }

        fn task_state(&mut self, name: &str) -> Result<String> {
            self.calls.push(format!("/Query /TN {name} State"));
            self.state
                .clone()
                .ok_or_else(|| format!("Get-ScheduledTask failed for task {name}"))
        }
    }

    struct Fixture {
        dir: tempfile::TempDir,
        local: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let local = dir.path().join("LocalAppData");
            Self { dir, local }
        }

        fn env(&self) -> impl Fn(&str) -> Option<std::ffi::OsString> {
            env_of(&[
                ("LOCALAPPDATA", self.local.to_str().unwrap()),
                ("USERNAME", "sook"),
                ("USERDOMAIN", "DESKTOP-1"),
            ])
        }

        /// A staged generation under the runtime root; the GUI executable lives elsewhere.
        fn staged(&self, key: &str) -> StagedRuntime {
            let executable = self
                .local
                .join("Ferryx")
                .join("daemon-runtime")
                .join(key)
                .join("ferryx.exe");
            std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
            std::fs::write(&executable, key.as_bytes()).unwrap();
            StagedRuntime {
                key: key.to_string(),
                executable,
            }
        }

        fn gui_exe(&self) -> PathBuf {
            self.dir.path().join("Program Files").join("Ferryx").join("ferryx.exe")
        }

        fn xml_path(&self, name: &str) -> PathBuf {
            self.local
                .join("Ferryx")
                .join("supervisor")
                .join(format!("{}.xml", name.replace('\\', "-")))
        }

        fn definition(&self, staged: &StagedRuntime) -> String {
            generation_definition(&self.env(), staged).unwrap()
        }
    }

    /// The task XML is the only channel to the watchdog (a scheduled task has no environment
    /// block): the barrier env is rendered into its Arguments only for a QA-named task, and the
    /// watchdog's own parser gives it back so run_daemon_supervisor sets it before spawning the
    /// daemon, which inherits it.
    #[test]
    fn qa_task_arguments_carry_the_barrier_env_to_the_watchdog() {
        let f = Fixture::new();
        let staged = f.staged("2026.1013.2-0a1b2c3d");
        let base = [("LOCALAPPDATA", f.local.to_str().unwrap()), ("USERNAME", "sook"), ("USERDOMAIN", "DESKTOP-1")];
        let barrier = [("FERRYX_QA_BARRIER_DIR", r"C:\qa\barrier"), ("FERRYX_QA_RUN_ID", "run-1")];
        let user: Vec<(&str, &str)> = base.into_iter().chain(barrier).collect();
        assert!(!generation_definition(&env_of(&user), &staged).unwrap().contains("FERRYX_QA_"));

        let qa: Vec<(&str, &str)> = base.into_iter().chain([(SUPERVISOR_NAME_ENV, "DaemonQa")]).chain(barrier).collect();
        let forwarded = forwarded_env(&env_of(&qa));
        let xml = generation_definition(&env_of(&qa), &staged).unwrap();
        assert!(xml.contains(&xml_escape(&task_arguments(&forwarded))), "{xml}");
        let args: Vec<String> = std::iter::once(SUPERVISOR_ARG.to_string())
            .chain(forwarded.iter().flat_map(|(k, v)| [ENV_ARG.to_string(), format!("{k}={v}")]))
            .collect();
        let parsed = parse_supervisor_args(&args).unwrap().unwrap();
        let keys: Vec<&str> = parsed.iter().map(|(k, _)| k.as_str()).collect();
        if cfg!(feature = "local-split-qa") {
            assert_eq!(keys, [SUPERVISOR_NAME_ENV, "FERRYX_QA_BARRIER_DIR", "FERRYX_QA_RUN_ID"]);
        } else {
            assert_eq!(keys, [SUPERVISOR_NAME_ENV]);
        }
    }

    #[test]
    fn missing_task_is_created_from_the_staged_runtime_and_run_when_absent() {
        let f = Fixture::new();
        let staged = f.staged("v2-00000002");
        let mut scheduler = FakeScheduler::default();
        assert_eq!(ensure_with(&f.env(), &staged, &mut scheduler, true, true), Ok(true));
        let name = "Ferryx\\Daemon-v2-00000002";
        assert_eq!(scheduler.calls[0], format!("/Query /TN {name}"));
        assert!(
            scheduler.calls[1].starts_with(&format!("/Create /TN {name} /XML ")),
            "{:?}",
            scheduler.calls
        );
        assert!(scheduler.calls[1].ends_with(" /F"));
        assert_eq!(scheduler.calls[2], format!("/Run /TN {name}"));
        assert_eq!(scheduler.calls.len(), 3);
        let registered = &scheduler.registered[name];
        assert_eq!(task_command(registered).as_deref(), staged.executable.to_str());
        assert_ne!(task_command(registered), f.gui_exe().to_str().map(str::to_owned));
        assert!(registered.contains("<Arguments>--daemon-supervisor"));
        assert!(has_logon_trigger(registered));
    }

    #[test]
    fn existing_task_with_absent_owner_is_only_run() {
        let f = Fixture::new();
        let staged = f.staged("v2-00000002");
        let name = "Ferryx\\Daemon-v2-00000002";
        let mut scheduler = FakeScheduler::default().with_task(name, "<Task/>");
        assert_eq!(ensure_with(&f.env(), &staged, &mut scheduler, true, true), Ok(true));
        assert_eq!(
            scheduler.calls,
            vec![format!("/Query /TN {name}"), format!("/Run /TN {name}")]
        );
    }

    #[test]
    fn existing_task_with_alive_or_unknown_owner_gets_zero_mutations() {
        let f = Fixture::new();
        let staged = f.staged("v2-00000002");
        let name = "Ferryx\\Daemon-v2-00000002";
        // The registered definition names an executable that no longer exists: HEAD deleted and
        // recreated such a task. Now it is left alone, because its instance may own every terminal.
        let stale = render_task_xml("u", r"C:\gone\ferryx.exe", SUPERVISOR_ARG);
        let mut scheduler = FakeScheduler::default().with_task(name, &stale);
        assert_eq!(ensure_with(&f.env(), &staged, &mut scheduler, false, true), Ok(false));
        assert_eq!(scheduler.calls, vec![format!("/Query /TN {name}")]);
        assert!(scheduler.mutations().is_empty(), "{:?}", scheduler.calls);
        assert_eq!(scheduler.registered[name], stale, "the definition is untouched");
    }

    #[test]
    fn missing_task_with_alive_owner_is_created_but_never_run() {
        let f = Fixture::new();
        let staged = f.staged("v2-00000002");
        let mut scheduler = FakeScheduler::default();
        assert_eq!(ensure_with(&f.env(), &staged, &mut scheduler, false, true), Ok(false));
        assert!(scheduler.calls.iter().any(|call| call.starts_with("/Create")));
        assert!(
            !scheduler.calls.iter().any(|call| {
                call.starts_with("/Run") || call.starts_with("/End") || call.starts_with("/Delete")
            }),
            "{:?}",
            scheduler.calls
        );
    }

    #[test]
    fn strip_logon_trigger_removes_only_the_trigger() {
        let xml = render_task_xml("DESKTOP-1\\sook", r"C:\rt\k\ferryx.exe", SUPERVISOR_ARG);
        let stripped = strip_logon_trigger(&xml);
        assert!(!has_logon_trigger(&stripped), "{stripped}");
        assert!(stripped.contains("<Triggers>\n  </Triggers>"), "{stripped}");
        assert_eq!(task_command(&stripped), task_command(&xml));
        for kept in [
            "<LogonType>InteractiveToken</LogonType>",
            "<MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>",
            "<Arguments>--daemon-supervisor</Arguments>",
        ] {
            assert!(stripped.contains(kept), "lost {kept}");
        }
        assert_eq!(strip_logon_trigger(&stripped), stripped, "idempotent");
        let crlf = xml.replace('\n', "\r\n");
        assert!(!has_logon_trigger(&strip_logon_trigger(&crlf)));
        assert_eq!(
            strip_logon_trigger("<a><LogonTrigger>unterminated"),
            "<a><LogonTrigger>unterminated",
            "a malformed definition is never truncated"
        );
    }

    #[test]
    fn older_generation_with_alive_owner_loses_only_its_logon_trigger() {
        let f = Fixture::new();
        let old = f.staged("v1-00000001");
        let new = f.staged("v2-00000002");
        let old_name = "Ferryx\\Daemon-v1-00000001";
        let new_name = "Ferryx\\Daemon-v2-00000002";
        // The old generation was registered (and recorded) earlier and its instance is running.
        let old_definition = f.definition(&old);
        write_task_xml(&f.xml_path(old_name), &old_definition).unwrap();
        let mut scheduler = FakeScheduler::default().with_task(old_name, &old_definition);

        assert_eq!(register_for_login_with(&f.env(), &new, &mut scheduler), Ok(true));

        let old_calls: Vec<&String> = scheduler
            .calls
            .iter()
            .filter(|call| call.contains(old_name))
            .collect();
        assert_eq!(old_calls.len(), 2, "{:?}", scheduler.calls);
        assert_eq!(*old_calls[0], format!("/Query /TN {old_name} /XML"));
        assert!(old_calls[1].starts_with(&format!("/Create /TN {old_name} /XML ")));
        assert!(old_calls[1].ends_with(" /F"));
        let demoted = &scheduler.registered[old_name];
        assert!(!has_logon_trigger(demoted), "{demoted}");
        assert_eq!(task_command(demoted).as_deref(), old.executable.to_str());
        assert!(has_logon_trigger(&scheduler.registered[new_name]), "the newest keeps it");
        assert!(
            !scheduler.calls.iter().any(|call| {
                call.starts_with("/Run") || call.starts_with("/End") || call.starts_with("/Delete")
            }),
            "registration never runs, ends or deletes a task: {:?}",
            scheduler.calls
        );

        // A second reconcile changes nothing and re-imports nothing.
        scheduler.calls.clear();
        assert_eq!(register_for_login_with(&f.env(), &new, &mut scheduler), Ok(false));
        assert!(scheduler.mutations().is_empty(), "{:?}", scheduler.calls);
    }

    #[test]
    fn a_vanished_older_task_only_loses_its_record() {
        let f = Fixture::new();
        let old = f.staged("v1-00000001");
        let new = f.staged("v2-00000002");
        let old_name = "Ferryx\\Daemon-v1-00000001";
        write_task_xml(&f.xml_path(old_name), &f.definition(&old)).unwrap();
        let mut scheduler = FakeScheduler::default();
        register_for_login_with(&f.env(), &new, &mut scheduler).unwrap();
        assert!(!f.xml_path(old_name).exists());
        assert!(!scheduler
            .calls
            .iter()
            .any(|call| call.contains(old_name) && !call.starts_with("/Query")));
    }

    /// Four generations, each with a recorded and registered task, the newest just registered.
    fn four_generations(
        f: &Fixture,
        scheduler: &mut FakeScheduler,
    ) -> (PathBuf, Vec<StagedRuntime>) {
        let root = f.local.join("Ferryx").join("daemon-runtime");
        let generations: Vec<StagedRuntime> = (1..=4)
            .map(|index| {
                let staged = f.staged(&format!("v{index}-0000000{index}"));
                let marker = root.join(&staged.key).join(runtime_stage::STAGED_AT_FILE);
                std::fs::write(marker, index.to_string()).unwrap();
                let name = generation_task_name(&f.env(), &staged.key);
                let definition = f.definition(&staged);
                write_task_xml(&f.xml_path(&name), &definition).unwrap();
                scheduler.registered.insert(name, definition);
                staged
            })
            .collect();
        register_for_login_with(&f.env(), &generations[3], scheduler).unwrap();
        (root, generations)
    }

    fn no_process() -> runtime_stage::SystemRuntimeUsage {
        runtime_stage::SystemRuntimeUsage {
            registered_job_executables: Vec::new(),
        }
    }

    #[test]
    fn superseded_idle_tasks_are_deleted_and_their_runtimes_then_collected() {
        let f = Fixture::new();
        let mut scheduler = FakeScheduler::default().with_state("Ready");
        let (root, generations) = four_generations(&f, &mut scheduler);
        let collect = || {
            let usage = runtime_stage::SystemRuntimeUsage {
                registered_job_executables: registered_executables(&f.env()),
            };
            let mut removed = runtime_stage::collect_garbage(&root, &usage).unwrap();
            removed.sort();
            removed
        };
        assert!(
            collect().is_empty(),
            "every recorded task protects its runtime"
        );
        scheduler.calls.clear();

        let mut removed = remove_superseded_tasks_with(
            &f.env(),
            &generations[3],
            &root,
            &mut scheduler,
            &no_process(),
        )
        .unwrap();

        removed.sort();
        assert_eq!(removed, ["v1-00000001", "v2-00000002"]);
        let mut mutations: Vec<String> = scheduler.mutations().into_iter().cloned().collect();
        mutations.sort();
        assert_eq!(
            mutations,
            [
                "/Delete /TN Ferryx\\Daemon-v1-00000001 /F",
                "/Delete /TN Ferryx\\Daemon-v2-00000002 /F"
            ]
        );
        assert!(!f.xml_path("Ferryx\\Daemon-v1-00000001").exists());
        assert!(!f.xml_path("Ferryx\\Daemon-v2-00000002").exists());
        assert!(f.xml_path("Ferryx\\Daemon-v3-00000003").exists());
        assert!(f.xml_path("Ferryx\\Daemon-v4-00000004").exists());
        assert_eq!(collect(), ["v1-00000001", "v2-00000002"]);
        assert!(!generations[0].executable.exists() && generations[2].executable.exists());
    }

    #[test]
    fn a_superseded_task_that_runs_or_cannot_be_read_is_kept() {
        for state in [Some("Running"), Some("Queued"), None] {
            let f = Fixture::new();
            let mut scheduler = FakeScheduler {
                state: state.map(str::to_string),
                ..FakeScheduler::default()
            };
            let (root, generations) = four_generations(&f, &mut scheduler);
            scheduler.calls.clear();

            let removed = remove_superseded_tasks_with(
                &f.env(),
                &generations[3],
                &root,
                &mut scheduler,
                &no_process(),
            );

            assert_eq!(removed, Ok(Vec::new()), "{state:?}");
            assert!(
                scheduler.mutations().is_empty(),
                "{state:?}: {:?}",
                scheduler.calls
            );
            assert!(
                f.xml_path("Ferryx\\Daemon-v1-00000001").exists(),
                "{state:?}"
            );
        }
    }

    /// No `/Run`, `/End` or `/Delete`: opting out of autostart must never start, end or remove a
    /// task whose watchdog may own every terminal.
    fn assert_no_run_end_or_delete(calls: &[String]) {
        assert!(
            !calls.iter().any(|call| {
                call.starts_with("/Run") || call.starts_with("/End") || call.starts_with("/Delete")
            }),
            "{calls:?}"
        );
    }

    #[test]
    fn strip_login_launch_removes_the_trigger_and_restart_on_failure_only() {
        let xml = render_task_xml("DESKTOP-1\\sook", r"C:\rt\k\ferryx.exe", SUPERVISOR_ARG);
        assert!(has_login_launch(&xml));
        let stripped = strip_login_launch(&xml);
        assert!(!has_login_launch(&stripped), "{stripped}");
        assert!(!stripped.contains("RestartOnFailure"), "{stripped}");
        assert!(!has_logon_trigger(&stripped), "{stripped}");
        assert_eq!(task_command(&stripped), task_command(&xml));
        for kept in [
            "<AllowStartOnDemand>true</AllowStartOnDemand>",
            "<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>",
            "<Arguments>--daemon-supervisor</Arguments>",
            "<LogonType>InteractiveToken</LogonType>",
        ] {
            assert!(stripped.contains(kept), "lost {kept}");
        }
        assert_eq!(strip_login_launch(&stripped), stripped, "idempotent");
        assert!(!has_login_launch(&strip_login_launch(&xml.replace('\n', "\r\n"))));
    }

    #[test]
    fn opt_out_strips_login_launch_from_every_generation_and_the_legacy_task_without_ending_any() {
        let f = Fixture::new();
        let old = f.staged("v1-00000001");
        let new = f.staged("v2-00000002");
        let old_name = "Ferryx\\Daemon-v1-00000001";
        let new_name = "Ferryx\\Daemon-v2-00000002";
        let legacy_name = "Ferryx\\Daemon";
        // The older generation was already demoted (no logon trigger, RestartOnFailure kept, as todo
        // 6 leaves it); the newest has its trigger; the legacy task has both. All three may be running.
        let old_definition = strip_logon_trigger(&f.definition(&old));
        let new_definition = f.definition(&new);
        let legacy_definition = render_task_xml("DESKTOP-1\\sook", r"C:\Program Files\Ferryx\ferryx.exe", SUPERVISOR_ARG);
        write_task_xml(&f.xml_path(old_name), &old_definition).unwrap();
        write_task_xml(&f.xml_path(new_name), &new_definition).unwrap();
        let mut scheduler = FakeScheduler::default()
            .with_task(old_name, &old_definition)
            .with_task(new_name, &new_definition)
            .with_task(legacy_name, &legacy_definition);

        assert_eq!(remove_login_autostart_with(&f.env(), &mut scheduler), Ok(true));

        assert_no_run_end_or_delete(&scheduler.calls);
        for name in [old_name, new_name, legacy_name] {
            let calls: Vec<&String> = scheduler
                .calls
                .iter()
                .filter(|call| call.contains(&format!("{name} ")))
                .collect();
            assert_eq!(calls.len(), 2, "{name}: {:?}", scheduler.calls);
            assert_eq!(*calls[0], format!("/Query /TN {name} /XML"));
            assert!(calls[1].starts_with(&format!("/Create /TN {name} /XML ")), "{calls:?}");
            assert!(calls[1].ends_with(" /F"), "{calls:?}");
            let registered = &scheduler.registered[name];
            assert!(!has_login_launch(registered), "{name}: {registered}");
            assert!(registered.contains("<AllowStartOnDemand>true</AllowStartOnDemand>"), "{name} must stay runnable on demand");
        }
        assert_eq!(task_command(&scheduler.registered[old_name]).as_deref(), old.executable.to_str());
        assert_eq!(task_command(&scheduler.registered[new_name]).as_deref(), new.executable.to_str());
        for name in [old_name, new_name, legacy_name] {
            assert!(f.xml_path(name).exists(), "{name} stays recorded, so its runtime is kept");
        }

        // A second opt-out reads only the records: no schtasks call at all.
        scheduler.calls.clear();
        assert_eq!(remove_login_autostart_with(&f.env(), &mut scheduler), Ok(false));
        assert!(scheduler.calls.is_empty(), "{:?}", scheduler.calls);

        // Re-enabling registers the newest with its trigger again; the older generation stays demoted.
        assert_eq!(register_for_login_with(&f.env(), &new, &mut scheduler), Ok(true));
        assert!(has_logon_trigger(&scheduler.registered[new_name]));
        assert!(!has_logon_trigger(&scheduler.registered[old_name]));
        assert_no_run_end_or_delete(&scheduler.calls);
    }

    #[test]
    fn opt_out_leaves_an_unregistered_task_and_its_record_alone() {
        let f = Fixture::new();
        let old = f.staged("v1-00000001");
        let old_name = "Ferryx\\Daemon-v1-00000001";
        let definition = f.definition(&old);
        write_task_xml(&f.xml_path(old_name), &definition).unwrap();
        let mut scheduler = FakeScheduler::default();
        assert_eq!(remove_login_autostart_with(&f.env(), &mut scheduler), Ok(false));
        assert!(scheduler.mutations().is_empty(), "{:?}", scheduler.calls);
        assert_eq!(read_task_xml(&f.xml_path(old_name)).as_deref(), Some(definition.as_str()));
    }

    #[test]
    fn opted_out_launch_with_absent_owner_runs_the_newest_task_without_a_logon_trigger() {
        let f = Fixture::new();
        let staged = f.staged("v2-00000002");
        let name = "Ferryx\\Daemon-v2-00000002";
        let mut scheduler = FakeScheduler::default();
        assert_eq!(ensure_with(&f.env(), &staged, &mut scheduler, true, false), Ok(true));
        assert_eq!(scheduler.calls.len(), 3, "{:?}", scheduler.calls);
        assert!(scheduler.calls[1].starts_with(&format!("/Create /TN {name} /XML ")));
        assert_eq!(scheduler.calls[2], format!("/Run /TN {name}"), "the start goes through the task, not a GUI child");
        let registered = scheduler.registered[name].clone();
        assert!(!has_login_launch(&registered), "{registered}");
        assert_eq!(task_command(&registered).as_deref(), staged.executable.to_str());
        assert_eq!(read_task_xml(&f.xml_path(name)).as_deref(), Some(registered.as_str()));

        // An existing task is only run, whatever its definition, and a second opt-out finds nothing to do.
        scheduler.calls.clear();
        assert_eq!(ensure_with(&f.env(), &staged, &mut scheduler, true, false), Ok(true));
        assert_eq!(scheduler.calls, vec![format!("/Query /TN {name}"), format!("/Run /TN {name}")]);
        scheduler.calls.clear();
        assert_eq!(remove_login_autostart_with(&f.env(), &mut scheduler), Ok(false));
        assert!(scheduler.mutations().is_empty(), "{:?}", scheduler.calls);

        // An Alive or Unknown owner is never run, opted out or not.
        let mut scheduler = FakeScheduler::default().with_task(name, &registered);
        assert_eq!(ensure_with(&f.env(), &staged, &mut scheduler, false, false), Ok(false));
        assert!(scheduler.mutations().is_empty(), "{:?}", scheduler.calls);
    }

    #[test]
    fn the_legacy_task_is_not_a_generation() {
        let f = Fixture::new();
        let new = f.staged("v2-00000002");
        write_task_xml(&f.xml_path("Ferryx\\Daemon"), "<Task><LogonTrigger/></Task>").unwrap();
        let mut scheduler = FakeScheduler::default().with_task("Ferryx\\Daemon", "x");
        register_for_login_with(&f.env(), &new, &mut scheduler).unwrap();
        assert!(
            !scheduler.calls.iter().any(|call| call.contains("Ferryx\\Daemon ")
                || call.ends_with("Ferryx\\Daemon")),
            "the legacy task is the migration's (todo 14): {:?}",
            scheduler.calls
        );
    }

    /// The legacy single task as baseline `ensure_with` registered it: the GUI's own executable,
    /// with a logon trigger, recorded as `Ferryx-Daemon.xml`.
    fn install_legacy_task(f: &Fixture) -> (String, String) {
        let name = "Ferryx\\Daemon".to_string();
        let xml = render_task_xml(
            "DESKTOP-1\\sook",
            &f.gui_exe().to_string_lossy(),
            &task_arguments(&[]),
        );
        assert!(has_logon_trigger(&xml));
        write_task_xml(&f.xml_path(&name), &xml).unwrap();
        (name, xml)
    }

    fn assert_never_ends(calls: &[String]) {
        for call in calls {
            assert!(!call.starts_with("/End"), "forbidden schtasks call {call}");
            assert!(!call.starts_with("/Run"), "the migration never starts a task: {call}");
        }
    }

    /// Todo 14: an Alive or Unknown legacy owner keeps its running task; the task only loses its
    /// logon trigger through exactly one `/Query /XML` and one `/Create /XML /F`, with zero
    /// `/End` or `/Delete`, and a second run issues no call.
    #[test]
    fn legacy_task_with_alive_or_unknown_owner_only_loses_its_logon_trigger() {
        let f = Fixture::new();
        let (name, xml) = install_legacy_task(&f);
        let mut scheduler = FakeScheduler::default().with_task(&name, &xml);
        assert_eq!(migrate_legacy_task_with(&f.env(), &mut scheduler, false), Ok(true));
        assert_eq!(scheduler.calls.len(), 2, "{:?}", scheduler.calls);
        assert_eq!(scheduler.calls[0], format!("/Query /TN {name} /XML"));
        assert!(scheduler.calls[1].starts_with(&format!("/Create /TN {name} /XML ")));
        assert!(scheduler.calls[1].ends_with(" /F"));
        assert!(!scheduler.calls.iter().any(|call| call.starts_with("/Delete")), "{:?}", scheduler.calls);
        assert_never_ends(&scheduler.calls);
        let registered = scheduler.registered.get(&name).unwrap().clone();
        assert!(!has_logon_trigger(&registered), "{registered}");
        assert_eq!(task_command(&registered), task_command(&xml), "only the trigger changes");
        assert!(f.xml_path(&name).exists(), "the legacy task stays registered");

        scheduler.calls.clear();
        assert_eq!(migrate_legacy_task_with(&f.env(), &mut scheduler, false), Ok(false));
        assert!(scheduler.calls.is_empty(), "the second run issues nothing: {:?}", scheduler.calls);
    }

    /// Todo 14: an Absent legacy owner's `Ready` task is deleted once and its record removed; the
    /// second run issues no call. A task that is already gone only loses its record.
    #[test]
    fn legacy_task_with_absent_owner_is_removed_once() {
        let f = Fixture::new();
        let (name, xml) = install_legacy_task(&f);
        let mut scheduler = FakeScheduler::default()
            .with_task(&name, &xml)
            .with_state("Ready");
        assert_eq!(migrate_legacy_task_with(&f.env(), &mut scheduler, true), Ok(true));
        assert_eq!(
            scheduler.calls,
            vec![
                format!("/Query /TN {name}"),
                format!("/Query /TN {name} State"),
                format!("/Delete /TN {name} /F"),
            ]
        );
        assert_never_ends(&scheduler.calls);
        assert!(!f.xml_path(&name).exists());

        scheduler.calls.clear();
        assert_eq!(migrate_legacy_task_with(&f.env(), &mut scheduler, true), Ok(false));
        assert!(scheduler.calls.is_empty(), "the second run issues nothing: {:?}", scheduler.calls);

        let (_, _) = install_legacy_task(&f);
        let mut gone = FakeScheduler::default();
        assert_eq!(migrate_legacy_task_with(&f.env(), &mut gone, true), Ok(true));
        assert_eq!(gone.calls, vec![format!("/Query /TN {name}")]);
        assert!(!f.xml_path(&name).exists());
    }

    /// Todo 14: an Absent owner's legacy task whose State is not idle (`Running`, so its watchdog may
    /// still respawn the daemon) or cannot be read is kept: zero `/Delete`, it only loses its logon
    /// trigger, and its record stays. A second run issues no mutation.
    #[test]
    fn legacy_task_with_absent_owner_is_kept_while_running_or_unreadable() {
        for state in [Some("Running"), None] {
            let f = Fixture::new();
            let (name, xml) = install_legacy_task(&f);
            let mut scheduler = FakeScheduler::default().with_task(&name, &xml);
            scheduler.state = state.map(str::to_string);
            assert_eq!(migrate_legacy_task_with(&f.env(), &mut scheduler, true), Ok(true), "{state:?}");
            assert_eq!(scheduler.calls.len(), 4, "{state:?}: {:?}", scheduler.calls);
            assert_eq!(scheduler.calls[0], format!("/Query /TN {name}"));
            assert_eq!(scheduler.calls[1], format!("/Query /TN {name} State"));
            assert_eq!(scheduler.calls[2], format!("/Query /TN {name} /XML"));
            assert!(scheduler.calls[3].starts_with(&format!("/Create /TN {name} /XML ")));
            assert!(
                !scheduler.calls.iter().any(|call| call.starts_with("/Delete")),
                "{state:?}: {:?}",
                scheduler.calls
            );
            assert_never_ends(&scheduler.calls);
            let registered = scheduler.registered.get(&name).unwrap().clone();
            assert!(!has_logon_trigger(&registered), "{registered}");
            assert!(f.xml_path(&name).exists(), "the legacy task stays registered");

            scheduler.calls.clear();
            assert_eq!(migrate_legacy_task_with(&f.env(), &mut scheduler, true), Ok(false));
            assert!(scheduler.mutations().is_empty(), "{state:?}: {:?}", scheduler.calls);
        }
    }

    #[test]
    fn registered_executables_cover_every_recorded_task() {
        let f = Fixture::new();
        let old = f.staged("v1-00000001");
        let new = f.staged("v2-00000002");
        write_task_xml(&f.xml_path("Ferryx\\Daemon-v1-00000001"), &f.definition(&old)).unwrap();
        write_task_xml(&f.xml_path("Ferryx\\Daemon-v2-00000002"), &f.definition(&new)).unwrap();
        let mut found = registered_executables(&f.env());
        found.sort();
        assert_eq!(found, vec![old.executable.clone(), new.executable.clone()]);
    }

    #[test]
    fn watchdog_spawns_only_its_own_staged_runtime() {
        let root = Path::new(r"C:\Users\u\AppData\Local\Ferryx\daemon-runtime");
        let staged = root.join("v1-00000001").join("ferryx.exe");
        assert_eq!(staged_daemon_executable(&staged, root), Ok(staged.clone()));
        assert_eq!(generation_key_of(&staged).as_deref(), Some("v1-00000001"));
        assert!(staged_daemon_executable(
            Path::new(r"C:\Program Files\Ferryx\ferryx.exe"),
            root
        )
        .is_err());
        assert!(staged_daemon_executable(&root.join("ferryx.exe"), root).is_err());
    }

    #[test]
    fn retirement_marker_must_name_the_exited_pid() {
        assert_eq!(
            retirement_marker_names(Some("v1-00000001"), 7),
            vec!["retired-v1-00000001.json".to_string(), "retired-7.json".to_string()]
        );
        assert_eq!(retirement_marker_names(None, 7), vec!["retired-7.json".to_string()]);
        assert!(marker_names_pid(r#"{"pid":7,"reason":"control-v1 Shutdown"}"#, 7));
        assert!(!marker_names_pid(r#"{"pid":8}"#, 7), "an earlier daemon's marker");
        assert!(!marker_names_pid("not json", 7));
    }

    #[test]
    fn utf16_task_query_output_decodes() {
        let xml = "<Task><LogonTrigger/></Task>";
        assert_eq!(decode_task_xml(&encode_utf16_with_bom(xml)), xml);
        assert_eq!(decode_task_xml(&encode_utf16_with_bom(xml)[2..]), xml);
        assert_eq!(decode_task_xml(xml.as_bytes()), xml);
    }

    /// A scripted watchdog host: probes, successor waits and exit codes come from queues; every
    /// spawn and retirement check is counted.
    #[derive(Default)]
    struct FakeHost {
        probes: VecDeque<Seen>,
        successors: VecDeque<Seen>,
        exits: VecDeque<Option<u32>>,
        retired_pids: Vec<u32>,
        next_pid: u32,
        spawns: u32,
        log: Vec<String>,
    }

    impl WatchdogHost for FakeHost {
        fn probe(&mut self) -> Seen {
            self.probes.pop_front().unwrap_or(Seen::Nothing)
        }
        fn wait_for_successor(&mut self, _exited_pid: u32) -> Seen {
            self.successors.pop_front().unwrap_or(Seen::Nothing)
        }
        fn spawn(&mut self) -> Result<u32> {
            self.spawns += 1;
            self.next_pid += 1;
            Ok(100 + self.next_pid)
        }
        fn wait(&mut self, _pid: u32) -> Result<Option<u32>> {
            Ok(self.exits.pop_front().expect("the script ran out of exits"))
        }
        fn retired(&mut self, exited_pid: u32) -> bool {
            self.retired_pids.contains(&exited_pid)
        }
        fn sleep(&mut self, _duration: Duration) {}
        fn log(&mut self, message: &str) {
            self.log.push(message.to_string());
        }
    }

    #[test]
    fn other_version_at_start_stops_with_zero_spawns() {
        let mut host = FakeHost {
            probes: VecDeque::from([Seen::OtherVersion]),
            ..FakeHost::default()
        };
        assert_eq!(run_watchdog_with(&mut host), 0);
        assert_eq!(host.spawns, 0);
    }

    #[test]
    fn other_version_after_a_crash_stops_with_zero_respawns() {
        let mut host = FakeHost {
            probes: VecDeque::from([Seen::Nothing, Seen::OtherVersion]),
            exits: VecDeque::from([Some(1)]),
            ..FakeHost::default()
        };
        assert_eq!(run_watchdog_with(&mut host), 0);
        assert_eq!(host.spawns, 1, "only the first spawn");
    }

    #[test]
    fn other_version_after_exit_zero_stops_with_zero_spawns() {
        let mut host = FakeHost {
            successors: VecDeque::from([Seen::OtherVersion]),
            exits: VecDeque::from([Some(0)]),
            ..FakeHost::default()
        };
        assert_eq!(run_watchdog_with(&mut host), 0);
        assert_eq!(host.spawns, 1);
    }

    #[test]
    fn exit_zero_with_retirement_marker_stops_without_waiting() {
        let mut host = FakeHost {
            exits: VecDeque::from([Some(0)]),
            retired_pids: vec![101],
            // A successor would be followed; the marker must win before any successor wait.
            successors: VecDeque::from([Seen::Owner(555)]),
            ..FakeHost::default()
        };
        assert_eq!(run_watchdog_with(&mut host), 0);
        assert_eq!(host.spawns, 1);
        assert_eq!(host.successors.len(), 1, "no successor wait after a retirement");
    }

    #[test]
    fn exit_zero_without_marker_keeps_following_a_successor() {
        let mut host = FakeHost {
            exits: VecDeque::from([Some(0), Some(0)]),
            successors: VecDeque::from([Seen::Owner(555), Seen::Nothing]),
            ..FakeHost::default()
        };
        assert_eq!(run_watchdog_with(&mut host), 0);
        assert_eq!(host.spawns, 1, "the successor is followed, not respawned");
        assert!(host.log.iter().any(|line| line.contains("Follow(555)")), "{:?}", host.log);
    }

    #[test]
    fn nonzero_exit_respawns_this_generation() {
        let mut host = FakeHost {
            exits: VecDeque::from([Some(3), Some(0)]),
            ..FakeHost::default()
        };
        assert_eq!(run_watchdog_with(&mut host), 0);
        assert_eq!(host.spawns, 2);
    }

    #[test]
    fn ignore_own_pending_start_hides_only_this_process() {
        struct Probe(u32);
        impl OwnerProbe for Probe {
            fn lock_state(&self) -> LockState {
                LockState::Free
            }
            fn read_record(&self) -> std::result::Result<Option<OwnerRecord>, String> {
                Ok(None)
            }
            fn process_start_time(&self, _pid: u32) -> Option<u64> {
                None
            }
            fn endpoint(&self) -> std::result::Result<Endpoint, String> {
                Ok(Endpoint::Missing)
            }
            fn read_starting(&self) -> std::result::Result<Option<StartingRecord>, String> {
                Ok(Some(StartingRecord {
                    launcher_pid: self.0,
                    launcher_start_time: 1,
                    generation_key: "g".to_string(),
                    started_at_unix_ms: 0,
                }))
            }
            fn now_unix_ms(&self) -> u64 {
                0
            }
        }
        let own = Probe(std::process::id());
        assert_eq!(IgnoreOwnPendingStart(&own).read_starting(), Ok(None));
        let other = Probe(std::process::id().wrapping_add(1));
        assert!(IgnoreOwnPendingStart(&other).read_starting().unwrap().is_some());
    }

    /// maho-win only (run with --ignored): the running instance of an older generation's task
    /// survives the logon-trigger re-import, and the task still shows Running. The test creates,
    /// runs, ends and deletes only its own QA-named task.
    #[cfg(windows)]
    #[test]
    #[ignore = "drives the real Task Scheduler; run on maho-win with --ignored"]
    fn real_task_instance_survives_logon_trigger_removal() {
        use super::platform::SystemScheduler;

        struct QaTask(String);
        impl Drop for QaTask {
            fn drop(&mut self) {
                let _ = SystemScheduler.run(&["/End", "/TN", &self.0]);
                let _ = SystemScheduler.run(&["/Delete", "/TN", &self.0, "/F"]);
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let qa = format!("FerryxQaT6-{}", std::process::id());
        let user = std::env::var("USERNAME").unwrap();
        let domain = std::env::var("USERDOMAIN").unwrap_or_default();
        let env = env_of(&[
            ("LOCALAPPDATA", dir.path().to_str().unwrap()),
            ("USERNAME", user.as_str()),
            ("USERDOMAIN", domain.as_str()),
            (SUPERVISOR_NAME_ENV, qa.as_str()),
        ]);
        let name = generation_task_name(&env, "qa-old");
        // The instance reports its pid over loopback: the test waits on that signal, never polls.
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let script = format!(
            "$c = New-Object System.Net.Sockets.TcpClient('127.0.0.1', {port}); \
             $b = [Text.Encoding]::ASCII.GetBytes([string]$PID); \
             $c.GetStream().Write($b, 0, $b.Length); $c.Close(); Start-Sleep -Seconds 300"
        );
        let xml = render_task_xml(
            &task_user(&env).unwrap(),
            "powershell.exe",
            &format!("-NoProfile -WindowStyle Hidden -Command \"{script}\""),
        );
        let mut scheduler = SystemScheduler;
        import_definition(&mut scheduler, &name, &task_xml_path(&env, &name).unwrap(), &xml)
            .unwrap();
        let _task = QaTask(name.clone());
        assert_eq!(scheduler.run(&["/Run", "/TN", &name]), Ok(true));

        let (sender, accepted) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(listener.accept().map(|(stream, _)| stream));
        });
        let mut stream = accepted
            .recv_timeout(Duration::from_secs(30))
            .expect("the QA task never connected")
            .expect("accepting the QA task's connection failed");
        stream.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
        let mut text = String::new();
        std::io::Read::read_to_string(&mut stream, &mut text)
            .expect("the QA task never sent its pid");
        let pid: u32 = text.trim().parse().expect("the QA task sent a pid");
        let started = crate::daemon::owner_admission::process_start_time(pid)
            .expect("the task instance is running");

        assert_eq!(remove_logon_trigger(&env, &mut scheduler, &name), Ok(true));

        let registered = scheduler.query_xml(&name).unwrap().expect("task still registered");
        assert!(!has_logon_trigger(&registered), "{registered}");
        assert_eq!(
            crate::daemon::owner_admission::process_start_time(pid),
            Some(started),
            "the running instance must survive the re-import"
        );
        // schtasks' Status column is localized; the MSFT_ScheduledTask State enum prints the
        // invariant name on every display language.
        let (folder, leaf) = name.rsplit_once('\\').expect("the task name has a folder");
        let state = crate::util::no_window_command("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!("(Get-ScheduledTask -TaskPath '\\{folder}\\' -TaskName '{leaf}').State"),
            ])
            .output()
            .unwrap();
        assert!(state.status.success(), "{state:?}");
        let state = String::from_utf8_lossy(&state.stdout);
        assert_eq!(state.trim(), "Running", "{state}");
    }
}
