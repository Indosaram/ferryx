//! Portable facade over the OS service manager that owns the headless daemon's lifetime.
//!
//! The daemon owns every PTY master descriptor, so it must outlive the GUI and come back when it
//! dies abnormally. Callers (the GUI at startup and the daemon client on demand) only talk to this
//! module; each target plugs in its own backend:
//!
//! * macOS - the `launchd` LaunchAgent in [`crate::daemon::launchd`] (same plist, same
//!   `KeepAlive { SuccessfulExit = false }`).
//! * Windows - a per-user scheduled task that runs the `--daemon-supervisor` watchdog
//!   ([`crate::daemon::supervisor_windows`]); Store (MSIX) installs are unsupported.
//! * Linux - the systemd user unit in [`crate::daemon::supervisor_linux`] (`Restart=on-failure`,
//!   `KillMode=process`); [`supported`] is `false` where no systemd user manager runs.
//! * Any other target - [`supported`] is always `false`.
//!
//! Where no backend is supported the daemon client keeps its detached-spawn fallback, which already
//! survives a GUI exit.
//!
//! Every backend shares one isolation policy: dev runtimes and any
//! `FERRYX_DATA_DIR`/`FERRYX_RUNTIME_DIR`/`FERRYX_SESSION_DIR` override never install a real unit,
//! because those mark a throwaway fixture. The QA-only [`SUPERVISOR_NAME_ENV`] override lifts that
//! policy and makes the backend use a distinct unit name, so QA can exercise the real service
//! manager without colliding with the user's own unit.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub type Result<T> = std::result::Result<T, String>;

/// QA-only override of the unit/task/label name. When set (non-empty) the backend is supported even
/// in an isolated runtime, and it manages a unit with this name instead of the real one.
pub const SUPERVISOR_NAME_ENV: &str = "FERRYX_DAEMON_SUPERVISOR_NAME";

/// Environment overrides that mark an isolated runtime.
pub const ISOLATED_RUNTIME_ENV: [&str; 3] =
    ["FERRYX_DATA_DIR", "FERRYX_RUNTIME_DIR", "FERRYX_SESSION_DIR"];

/// Environment lookup, injectable so the policy is testable without mutating the process env.
pub(crate) type Env<'a> = &'a dyn Fn(&str) -> Option<OsString>;

fn process_env(key: &str) -> Option<OsString> {
    std::env::var_os(key)
}

pub(crate) fn non_empty(env: Env<'_>, key: &str) -> Option<OsString> {
    env(key).filter(|value| !value.is_empty())
}

fn isolated_runtime(env: Env<'_>) -> bool {
    ISOLATED_RUNTIME_ENV
        .iter()
        .any(|key| non_empty(env, key).is_some())
}

/// Pure policy shared by every backend: a target without a backend is unsupported; otherwise the QA
/// name override wins, and an isolated or dev runtime never manages the real unit.
fn supported_for(
    target_has_backend: bool,
    dev_runtime: bool,
    isolated_runtime: bool,
    qa_name_override: bool,
) -> bool {
    target_has_backend
        && (qa_name_override
            || crate::daemon::launchd::agent_management_supported_for(
                dev_runtime,
                isolated_runtime,
            ))
}

fn supported_in(env: Env<'_>, dev_runtime: bool) -> bool {
    supported_for(
        backend::TARGET_HAS_BACKEND && backend::install_supported(),
        dev_runtime,
        isolated_runtime(env),
        non_empty(env, SUPERVISOR_NAME_ENV).is_some(),
    )
}

/// Whether this process may manage the daemon's supervisor unit on this target.
pub fn supported() -> bool {
    supported_in(&process_env, crate::daemon::server::is_dev_runtime()) && backend::available()
}

/// Name of the marker file that records the user's "do not autostart the daemon" choice.
const OPTOUT_FILE_NAME: &str = "daemon-supervisor.optout";

/// The opt-out marker inside a given data directory (test seam).
fn optout_path_in(dir: &Path) -> PathBuf {
    dir.join(OPTOUT_FILE_NAME)
}

/// Where the opt-out marker lives. It must survive restarts, so it sits in the same canonical
/// `<data>/remote` directory as the agent-integration setting: `FERRYX_DATA_DIR` relocates it, and
/// the cross-platform resolver falls back to the per-user data dir on every target.
fn optout_path() -> Option<PathBuf> {
    crate::remote::auth::resolve_canonical_remote_dir(|key| std::env::var_os(key))
        .map(|dir| optout_path_in(&dir))
}

fn user_opted_out_at(path: &Path) -> bool {
    path.exists()
}

fn set_user_opt_out_at(path: &Path, on: bool) -> Result<()> {
    if on {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create the data directory: {e}"))?;
        }
        std::fs::write(path, b"").map_err(|e| format!("Failed to write the opt-out marker: {e}"))
    } else {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("Failed to remove the opt-out marker: {e}")),
        }
    }
}

/// Whether the user turned daemon autostart off; [`ensure`] then never (re)installs the unit.
pub fn user_opted_out() -> bool {
    optout_path().is_some_and(|path| user_opted_out_at(&path))
}

/// Records (or clears) the user's autostart opt-out so it survives app and daemon restarts.
pub fn set_user_opt_out(on: bool) -> Result<()> {
    let path = optout_path().ok_or("Cannot determine the Ferryx data directory")?;
    set_user_opt_out_at(&path, on)
}

/// Whether the supervisor unit/task is currently installed for this user.
pub fn installed() -> bool {
    supported() && backend::installed(&process_env)
}

/// Removes the autostart unit without stopping the running daemon (its PTYs stay alive); the
/// daemon simply is not started again at the next login. A missing unit is success.
pub fn remove_autostart() -> Result<()> {
    if !supported() {
        return Ok(());
    }
    backend::remove_autostart(&process_env)
}

/// `systemctl` arguments that drop the unit from login autostart. Never `--now` or `stop`: the
/// running daemon owns every PTY and must keep running.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn linux_remove_autostart_args(unit: &str) -> Vec<&str> {
    vec!["--user", "disable", unit]
}

/// [`ensure_in`] behind the user's opt-out: an opted-out user never gets the unit back.
fn ensure_unless_opted_out(
    opted_out: bool,
    env: Env<'_>,
    dev_runtime: bool,
    executable: &Path,
) -> Result<bool> {
    if opted_out {
        return Ok(false);
    }
    ensure_in(env, dev_runtime, executable)
}

fn ensure_in(env: Env<'_>, dev_runtime: bool, executable: &Path) -> Result<bool> {
    if !supported_in(env, dev_runtime) {
        return Ok(false);
    }
    backend::ensure(env, executable)
}

/// Makes the supervisor own the daemon: removes a stale unit whose executable no longer exists,
/// writes the unit when it changed, and (re)loads it when needed. Returns `true` when it
/// (re)installed or reloaded the unit, `false` when nothing changed or the runtime is unsupported.
/// A user who turned autostart off ([`user_opted_out`]) is a no-op that returns `false`.
pub fn ensure() -> Result<bool> {
    let opted_out = user_opted_out();
    if opted_out || !supported() {
        return Ok(false);
    }
    let executable =
        std::env::current_exe().map_err(|e| format!("Failed to get current exe path: {e}"))?;
    ensure_unless_opted_out(
        opted_out,
        &process_env,
        crate::daemon::server::is_dev_runtime(),
        &executable,
    )
}

/// Reloads the unit so the service manager starts the daemon again (macOS re-runs `RunAtLoad`).
pub fn reload() -> Result<()> {
    if !supported() {
        return Err("No daemon supervisor is supported in this runtime".to_string());
    }
    backend::reload(&process_env)
}

/// Stops and removes the unit; a missing unit is success.
pub fn uninstall() -> Result<()> {
    if !supported() {
        return Err("No daemon supervisor is supported in this runtime".to_string());
    }
    backend::uninstall(&process_env)
}

/// Points the scheduled task at this executable without starting it. The protocol-mismatch path
/// calls this before it replaces an older daemon, so the watchdog's next respawn runs this build.
/// A user who turned autostart off ([`user_opted_out`]) is a no-op that returns `false`, so the
/// scheduled task is never re-registered behind their back.
#[cfg(windows)]
pub fn retarget() -> Result<bool> {
    retarget_unless_opted_out(user_opted_out(), || {
        if !supported() {
            return Ok(false);
        }
        let executable =
            std::env::current_exe().map_err(|e| format!("Failed to get current exe path: {e}"))?;
        crate::daemon::supervisor_windows::retarget(&process_env, &executable)
    })
}

/// [`retarget`] behind the user's opt-out: the backend is never touched for an opted-out user.
#[cfg(any(windows, test))]
fn retarget_unless_opted_out(
    opted_out: bool,
    retarget: impl FnOnce() -> Result<bool>,
) -> Result<bool> {
    if opted_out {
        return Ok(false);
    }
    retarget()
}

#[cfg(target_os = "macos")]
mod backend {
    use super::{non_empty, Env, Result, SUPERVISOR_NAME_ENV};
    use crate::daemon::launchd;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};

    pub const TARGET_HAS_BACKEND: bool = true;

    pub fn install_supported() -> bool {
        true
    }

    pub fn available() -> bool {
        true
    }

    fn label(env: Env<'_>) -> String {
        non_empty(env, SUPERVISOR_NAME_ENV)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| launchd::PLIST_LABEL.to_string())
    }

    /// `launchd::get_launchd_plist_path` rules, except that a QA label gets its own file.
    fn plist_path(env: Env<'_>, label: &str) -> Result<PathBuf> {
        let override_path = env(launchd::PLIST_PATH_ENV);
        let home = env("HOME");
        let path = if label == launchd::PLIST_LABEL || override_path.is_some() {
            launchd::launchd_plist_path_for(override_path.as_deref(), home.as_deref())
        } else {
            home.map(|home| {
                PathBuf::from(home)
                    .join("Library/LaunchAgents")
                    .join(format!("{label}.plist"))
            })
        };
        path.ok_or_else(|| "Cannot determine LaunchAgent path".to_string())
    }

    /// The plist `launchd::generate_launchd_plist` renders; byte-identical for the real label.
    pub(super) fn render_plist(label: &str, executable: &Path, log_dir: &Path) -> String {
        let plist = launchd::generate_launchd_plist(&executable.to_string_lossy(), log_dir);
        if label == launchd::PLIST_LABEL {
            return plist;
        }
        plist.replacen(
            &format!("<string>{}</string>", launchd::PLIST_LABEL),
            &format!("<string>{label}</string>"),
            1,
        )
    }

    /// First `ProgramArguments` entry of an existing plist: the executable launchd would run.
    pub(super) fn plist_program(content: &str) -> Option<&str> {
        let after_key = &content[content.find("<key>ProgramArguments</key>")?..];
        let start = after_key.find("<string>")? + "<string>".len();
        let end = after_key[start..].find("</string>")?;
        Some(&after_key[start..start + end])
    }

    /// Mirrors `launchd::write_launchd_plist`: write only on change, create the log directory.
    fn write_plist(content: &str, log_dir: &Path, plist_path: &Path) -> Result<bool> {
        if fs::read_to_string(plist_path).is_ok_and(|existing| existing == content) {
            return Ok(false);
        }
        if let Some(parent) = plist_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create launch agent directory: {e}"))?;
        }
        fs::create_dir_all(log_dir)
            .map_err(|e| format!("Failed to create agent log directory: {e}"))?;
        fs::write(plist_path, content).map_err(|e| format!("Failed to write plist: {e}"))?;
        Ok(true)
    }

    /// The domain comes from the calling uid, not `$HOME`, like `launchd`'s own loaded check.
    fn job_is_loaded(label: &str) -> bool {
        // SAFETY: `libc::getuid` takes no arguments, dereferences no pointers, and has no side effects.
        let uid = unsafe { libc::getuid() };
        Command::new("launchctl")
            .args(["print", &format!("gui/{uid}/{label}")])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    pub fn ensure(env: Env<'_>, executable: &Path) -> Result<bool> {
        let label = label(env);
        let plist_path = plist_path(env, &label)?;
        let stale_program = fs::read_to_string(&plist_path)
            .ok()
            .and_then(|content| plist_program(&content).map(str::to_owned))
            .filter(|program| !Path::new(program).exists());
        if let Some(program) = stale_program {
            // The app this job points at was removed. Drop the job; the write below installs it for
            // the current executable. A failed unload is not fatal: the load unloads again first.
            if let Err(error) = launchd::uninstall_launchd_agent_from_path(&plist_path) {
                tracing::warn!(%error, %program, "Could not remove the stale launchd agent");
            }
        }
        let log_dir = launchd::launchd_log_dir_for(env("HOME").as_deref())
            .ok_or("Cannot determine the agent log directory")?;
        let content = render_plist(&label, executable, &log_dir);
        let rewritten = write_plist(&content, &log_dir, &plist_path)?;
        if rewritten || !job_is_loaded(&label) {
            launchd::load_launchd_agent(&plist_path)?;
            return Ok(true);
        }
        Ok(false)
    }

    pub fn reload(env: Env<'_>) -> Result<()> {
        launchd::load_launchd_agent(&plist_path(env, &label(env))?)
    }

    pub fn uninstall(env: Env<'_>) -> Result<()> {
        launchd::uninstall_launchd_agent_from_path(&plist_path(env, &label(env))?)
    }

    pub fn installed(env: Env<'_>) -> bool {
        plist_path(env, &label(env)).is_ok_and(|path| path.exists())
    }

    /// Deletes the plist only. No `launchctl unload`/`bootout`: that would stop the running daemon.
    pub fn remove_autostart(env: Env<'_>) -> Result<()> {
        let path = plist_path(env, &label(env))?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("Failed to remove the LaunchAgent plist: {e}")),
        }
    }
}

#[cfg(windows)]
mod backend {
    use super::{Env, Result};
    use crate::daemon::supervisor_windows as task;
    use std::path::Path;

    pub const TARGET_HAS_BACKEND: bool = true;

    /// The Store owns an MSIX install's lifecycle, so it never gets a task.
    pub fn install_supported() -> bool {
        task::supported_for_install(crate::ipc::updater::updater_managed_externally())
    }

    /// The Task Scheduler is part of every supported Windows install.
    pub fn available() -> bool {
        true
    }

    pub fn ensure(env: Env<'_>, executable: &Path) -> Result<bool> {
        task::ensure(env, executable)
    }

    pub fn reload(env: Env<'_>) -> Result<()> {
        task::reload(env)
    }

    pub fn uninstall(env: Env<'_>) -> Result<()> {
        task::uninstall(env)
    }

    fn schtasks(args: &[&str]) -> Result<bool> {
        crate::util::no_window_command("schtasks")
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .map_err(|e| format!("Failed to run schtasks: {e}"))
    }

    pub fn installed(env: Env<'_>) -> bool {
        schtasks(&["/Query", "/TN", &task::task_name(env)]).unwrap_or(false)
    }

    /// Deletes the task definition only (no `/End`), so the running watchdog and daemon stay up.
    pub fn remove_autostart(env: Env<'_>) -> Result<()> {
        let name = task::task_name(env);
        if schtasks(&["/Query", "/TN", &name])? && !schtasks(&["/Delete", "/TN", &name, "/F"])? {
            return Err(format!("schtasks /Delete failed for task {name}"));
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
mod backend {
    use super::{Env, Result, SUPERVISOR_NAME_ENV};
    use crate::daemon::supervisor_linux;
    use std::path::{Path, PathBuf};

    pub const TARGET_HAS_BACKEND: bool = true;

    pub fn install_supported() -> bool {
        true
    }

    /// No systemd user manager (WSL, containers, another init) means no supervisor; the daemon logs
    /// the reason once at startup.
    pub fn available() -> bool {
        supervisor_linux::availability().is_none()
    }

    fn unit(env: Env<'_>) -> Result<(String, PathBuf)> {
        let name = supervisor_linux::unit_name(env(SUPERVISOR_NAME_ENV).as_deref())?;
        let path = supervisor_linux::unit_path(
            env("XDG_CONFIG_HOME").as_deref(),
            env("HOME").as_deref(),
            &name,
        )
        .ok_or("Cannot determine the systemd user unit path")?;
        Ok((name, path))
    }

    pub fn ensure(env: Env<'_>, executable: &Path) -> Result<bool> {
        let (name, path) = unit(env)?;
        let executable =
            supervisor_linux::daemon_executable(env("APPIMAGE").as_deref(), executable);
        supervisor_linux::ensure(&name, &path, &executable)
    }

    pub fn reload(env: Env<'_>) -> Result<()> {
        supervisor_linux::start(&unit(env)?.0)
    }

    pub fn uninstall(env: Env<'_>) -> Result<()> {
        let (name, path) = unit(env)?;
        supervisor_linux::uninstall(&name, &path)
    }

    pub fn installed(env: Env<'_>) -> bool {
        unit(env).is_ok_and(|(_, path)| path.exists())
    }

    fn systemctl(args: &[&str]) -> Result<bool> {
        std::process::Command::new("systemctl")
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .map_err(|e| format!("Failed to run systemctl: {e}"))
    }

    /// Disables the unit (no `--now`, so the running daemon keeps its PTYs), deletes the unit file,
    /// then reloads the user manager. Disable runs first because systemd cannot resolve the unit's
    /// install links once its file is gone.
    pub fn remove_autostart(env: Env<'_>) -> Result<()> {
        let (name, path) = unit(env)?;
        if !systemctl(&super::linux_remove_autostart_args(&name))? {
            tracing::warn!(unit = %name, "systemctl --user disable failed; removing the unit file anyway");
        }
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("Failed to remove the systemd user unit: {e}")),
        }
        if !systemctl(&["--user", "daemon-reload"])? {
            return Err("systemctl --user daemon-reload failed".to_string());
        }
        Ok(())
    }
}

/// Other targets never get a backend: callers fall back to the detached spawn.
#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
mod backend {
    use super::{Env, Result};
    use std::path::Path;

    pub const TARGET_HAS_BACKEND: bool = false;

    pub fn install_supported() -> bool {
        true
    }

    pub fn available() -> bool {
        false
    }

    pub fn ensure(_env: Env<'_>, _executable: &Path) -> Result<bool> {
        Ok(false)
    }

    pub fn reload(_env: Env<'_>) -> Result<()> {
        Err("No daemon supervisor backend on this target".to_string())
    }

    pub fn uninstall(_env: Env<'_>) -> Result<()> {
        Ok(())
    }

    pub fn installed(_env: Env<'_>) -> bool {
        false
    }

    pub fn remove_autostart(_env: Env<'_>) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let map: HashMap<String, OsString> = pairs
            .iter()
            .map(|(key, value)| (key.to_string(), OsString::from(value)))
            .collect();
        move |key| map.get(key).cloned()
    }

    #[test]
    fn supervisor_is_unsupported_for_isolated_runtimes() {
        for key in ISOLATED_RUNTIME_ENV {
            let isolated = env_of(&[(key, "/tmp/ferryx-fixture")]);
            assert!(!supported_in(&isolated, false), "{key} must disable the supervisor");
            assert!(
                !supported_for(true, false, isolated_runtime(&isolated), false),
                "{key} must disable the supervisor even where a backend exists"
            );
            let qa = env_of(&[(key, "/tmp/ferryx-fixture"), (SUPERVISOR_NAME_ENV, "com.ferryx.qa")]);
            assert_eq!(
                supported_in(&qa, false),
                backend::TARGET_HAS_BACKEND,
                "the QA name override re-enables the backend under {key}"
            );
        }
        let empty = env_of(&[("FERRYX_DATA_DIR", "")]);
        assert!(!isolated_runtime(&empty), "an empty override is not an isolated runtime");
        assert!(!supported_for(true, true, false, false), "a dev runtime is unsupported");
        assert!(supported_for(true, false, false, false));
    }

    #[test]
    fn supervisor_is_unsupported_on_unknown_targets() {
        assert!(!supported_for(false, false, false, false));
        assert!(
            !supported_for(false, false, false, true),
            "the QA override cannot conjure a backend on a target without one"
        );
        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        assert!(!supported());
    }

    #[test]
    fn ensure_is_a_no_op_for_an_isolated_runtime() {
        let home = tempfile::tempdir().unwrap();
        let fixture = home.path().join("data");
        let env = env_of(&[
            ("HOME", home.path().to_str().unwrap()),
            ("FERRYX_DATA_DIR", fixture.to_str().unwrap()),
        ]);
        assert_eq!(
            ensure_in(&env, false, Path::new("/opt/ferryx/bin/ferryx")),
            Ok(false)
        );
        assert_eq!(
            std::fs::read_dir(home.path()).unwrap().count(),
            0,
            "an isolated runtime must not create any unit or log file"
        );
    }

    #[test]
    fn ensure_is_noop_when_user_opted_out() {
        let home = tempfile::tempdir().unwrap();
        // The QA name override makes the backend supported, so only the opt-out stops the install.
        let env = env_of(&[
            ("HOME", home.path().to_str().unwrap()),
            ("XDG_CONFIG_HOME", home.path().join("config").to_str().unwrap()),
            ("LOCALAPPDATA", home.path().join("local").to_str().unwrap()),
            (SUPERVISOR_NAME_ENV, "com.ferryx.optout-test"),
        ]);
        assert_eq!(
            ensure_unless_opted_out(true, &env, false, Path::new("/opt/ferryx/bin/ferryx")),
            Ok(false)
        );
        assert_eq!(
            std::fs::read_dir(home.path()).unwrap().count(),
            0,
            "an opted-out user must not get any unit or log file"
        );

        let data = tempfile::tempdir().unwrap();
        let marker = optout_path_in(data.path());
        assert!(!user_opted_out_at(&marker));
        set_user_opt_out_at(&marker, true).unwrap();
        assert!(user_opted_out_at(&marker), "the opt-out persists as a file");
        set_user_opt_out_at(&marker, false).unwrap();
        assert!(!user_opted_out_at(&marker));
        assert_eq!(
            set_user_opt_out_at(&marker, false),
            Ok(()),
            "clearing a missing opt-out is success"
        );
    }

    #[test]
    fn retarget_is_noop_when_user_opted_out() {
        let mut called = false;
        assert_eq!(
            retarget_unless_opted_out(true, || {
                called = true;
                Ok(true)
            }),
            Ok(false)
        );
        assert!(!called, "an opted-out user must never reach the retarget backend");

        let mut called = false;
        assert_eq!(
            retarget_unless_opted_out(false, || {
                called = true;
                Ok(true)
            }),
            Ok(true)
        );
        assert!(called, "without the opt-out the backend still runs");
    }

    #[test]
    fn linux_remove_autostart_args_never_stop_the_daemon() {
        let args = linux_remove_autostart_args("ferryx-daemon.service");
        assert!(args.contains(&"disable"));
        assert!(args.contains(&"--user"));
        assert!(!args.contains(&"--now"), "--now would stop the running daemon");
        assert!(!args.contains(&"stop"));
        assert_eq!(args.last(), Some(&"ferryx-daemon.service"));
    }

    /// The facade must hand launchd exactly the plist the launchd module renders, and point the
    /// stale-unit check at its program.
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_backend_renders_the_unchanged_launchd_plist() {
        let exe = Path::new("/opt/ferryx/bin/ferryx");
        let log_dir = Path::new("/Users/someone/Library/Logs/Ferryx");
        let expected = crate::daemon::launchd::generate_launchd_plist("/opt/ferryx/bin/ferryx", log_dir);
        let rendered =
            backend::render_plist(crate::daemon::launchd::PLIST_LABEL, exe, log_dir);
        assert_eq!(rendered, expected);
        assert_eq!(backend::plist_program(&rendered), Some("/opt/ferryx/bin/ferryx"));
        let qa = backend::render_plist("com.ferryx.qa", exe, log_dir);
        assert!(qa.contains("<string>com.ferryx.qa</string>"));
        assert!(!qa.contains("<string>com.ferryx.daemon</string>"));
    }
}
