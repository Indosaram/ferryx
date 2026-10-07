//! macOS `launchd` LaunchAgent ownership for the headless daemon.
//!
//! The daemon owns every PTY master descriptor. While it is spawned as a child of the GUI it shares
//! that process's lifetime and process group, so any teardown that reaches the app - a force quit, a
//! crash, a logout, an app swap - takes the daemon with it and every terminal dies at the same
//! instant. That is the 2026-10-07 incident: 54 of 74 panes were lost with no lifecycle record at
//! all in `~/.ferryx/logs/daemon.log`, because no exit path ever ran.
//!
//! On macOS the fix is to stop treating the daemon as a GUI child and hand its lifetime to
//! `launchd`: the agent starts the daemon at login, restarts it when it dies unexpectedly, and -
//! critically - does **not** kill it when the GUI quits. The GUI only ensures the agent exists and
//! then connects to whatever daemon is already there.
//!
//! `KeepAlive` is deliberately `{ SuccessfulExit = false }` rather than `true`. The daemon exits 0
//! on purpose in three legitimate cases - a committed handover, an idle binary swap, and a
//! duplicate-endpoint startup - and an unconditional `KeepAlive` would fight all three by
//! relaunching instantly into the instance lock.
//!
//! Windows and Linux have no equivalent wired here yet (no service registration, no
//! `systemd --user` unit, no XDG autostart file). They keep the detached-spawn path in
//! `daemon::client`, which already survives a GUI exit; only crash-restart is macOS-only today.
//!
//! Every path that spawns the `launchctl` binary is gated to `#[cfg(target_os = "macos")]`, so a
//! caller cannot shell out to a macOS-only tool on Windows or Linux.

use std::fs;
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::process::Command;

pub const PLIST_LABEL: &str = "com.ferryx.daemon";

/// Overrides the managed plist path so QA/fixture runs never touch the real LaunchAgent.
pub const PLIST_PATH_ENV: &str = "FERRYX_DAEMON_PLIST_PATH";

#[cfg(target_os = "macos")]
pub fn get_launchd_plist_path() -> Option<PathBuf> {
    launchd_plist_path_for(
        std::env::var_os(PLIST_PATH_ENV).as_deref(),
        std::env::var_os("HOME").as_deref(),
    )
}

/// Pure plist-path resolution, so the override and `$HOME` rules are testable without mutating the
/// process environment.
pub fn launchd_plist_path_for(
    override_path: Option<&std::ffi::OsStr>,
    home: Option<&std::ffi::OsStr>,
) -> Option<PathBuf> {
    if let Some(override_path) = override_path {
        let override_path = PathBuf::from(override_path);
        return (!override_path.as_os_str().is_empty()).then_some(override_path);
    }
    home.map(|h| {
        PathBuf::from(h)
            .join("Library/LaunchAgents")
            .join(format!("{PLIST_LABEL}.plist"))
    })
}

#[cfg(not(target_os = "macos"))]
pub fn get_launchd_plist_path() -> Option<PathBuf> {
    None
}

/// Private directory for the agent's raw stdout/stderr. Never `/tmp`: this daemon now always runs,
/// and a world-writable path lets another user pre-create the file as a symlink and redirect the
/// daemon's output. The daemon's own durable log lives in `~/.ferryx/logs`; this only captures what
/// it writes before that sink is installed, plus the readiness token.
pub fn launchd_log_dir_for(home: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    home.map(|home| PathBuf::from(home).join("Library/Logs/Ferryx"))
}

/// Whether this process may manage the developer's real LaunchAgent.
///
/// `false` for the dev runtime (`/tmp/rorca-<uid>-dev`, i.e. debug builds) and for any
/// `FERRYX_DATA_DIR`/`FERRYX_RUNTIME_DIR`/`FERRYX_SESSION_DIR` override, because those mark an
/// isolated fixture: installing an agent there would point the user's login-time autostart at a
/// throwaway test build. Also `false` on every non-macOS target, where no agent exists.
pub fn agent_management_supported() -> bool {
    #[cfg(target_os = "macos")]
    {
        agent_management_supported_for(
            crate::daemon::server::is_dev_runtime(),
            ["FERRYX_DATA_DIR", "FERRYX_RUNTIME_DIR", "FERRYX_SESSION_DIR"]
                .iter()
                .any(|key| std::env::var_os(key).is_some_and(|value| !value.is_empty())),
        )
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// Pure form of the policy above: an isolated runtime must never install the user's real agent.
pub fn agent_management_supported_for(dev_runtime: bool, isolated_runtime: bool) -> bool {
    !dev_runtime && !isolated_runtime
}

/// Renders the LaunchAgent that owns the daemon's lifetime.
///
/// `RunAtLoad` starts it at login; `KeepAlive { SuccessfulExit = false }` restarts only abnormal
/// deaths, leaving the daemon's deliberate `exit(0)` paths (handover commit, idle binary swap,
/// duplicate endpoint) alone. The soft descriptor limit is raised well above the 256 that launchd
/// hands out by default, because a terminal daemon spends roughly three descriptors per session and
/// a daemon that runs out mid-write dies with every session it owns; the daemon raises the soft
/// limit again at startup, which is what lifts it to the hard ceiling.
pub fn generate_launchd_plist(executable_path: &str, log_dir: &Path) -> String {
    let stdout_path = log_dir.join("daemon.stdout.log");
    let stderr_path = log_dir.join("daemon.stderr.log");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{PLIST_LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{executable_path}</string>
        <string>--daemon</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <dict>
        <key>SuccessfulExit</key>
        <false/>
    </dict>
    <key>SoftResourceLimits</key>
    <dict>
        <key>NumberOfFiles</key>
        <integer>4096</integer>
    </dict>
    <key>StandardOutPath</key>
    <string>{}</string>
    <key>StandardErrorPath</key>
    <string>{}</string>
</dict>
</plist>
"#,
        stdout_path.display(),
        stderr_path.display()
    )
}

/// Writes the plist only when its content differs, and reports whether it was rewritten.
///
/// The GUI calls this on every start, and rewriting an unchanged file would bump its mtime for no
/// reason. A caller that gets `true` must reload the agent so launchd re-reads it. The log directory
/// is created here because launchd does not create a missing `StandardOutPath` parent, and a job
/// whose output cannot be opened is a job that does not run.
pub fn write_launchd_plist(
    executable_path: &Path,
    log_dir: &Path,
    plist_path: &Path,
) -> Result<bool, String> {
    let content = generate_launchd_plist(&executable_path.to_string_lossy(), log_dir);
    if fs::read_to_string(plist_path).is_ok_and(|existing| existing == content) {
        return Ok(false);
    }
    if let Some(parent) = plist_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create launch agent directory: {e}"))?;
    }
    fs::create_dir_all(log_dir).map_err(|e| format!("Failed to create agent log directory: {e}"))?;
    fs::write(plist_path, content).map_err(|e| format!("Failed to write plist: {e}"))?;
    Ok(true)
}

/// `launchctl` reports the job in this process's GUI domain, or exits non-zero when it is absent.
///
/// The domain comes from the calling uid, not `$HOME`, so an overridden `HOME` cannot read the
/// wrong domain.
#[cfg(target_os = "macos")]
fn launchd_job_is_loaded() -> bool {
    // SAFETY: `libc::getuid` takes no arguments, dereferences no pointers, and has no side effects.
    let uid = unsafe { libc::getuid() };
    Command::new("launchctl")
        .args(["print", &format!("gui/{uid}/{PLIST_LABEL}")])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Loads (or reloads) the agent so launchd reads the plist at `plist_path`.
///
/// `unload` runs first and its failure is ignored: a rewrite only takes effect after the old job is
/// dropped, because `load -w` on a loaded job keeps the previous definition.
#[cfg(target_os = "macos")]
pub fn load_launchd_agent(plist_path: &Path) -> Result<(), String> {
    let path = plist_path.to_string_lossy().to_string();
    let _ = Command::new("launchctl")
        .args(["unload", "-w", &path])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    let output = Command::new("launchctl")
        .args(["load", "-w", &path])
        .output()
        .map_err(|e| format!("Failed to execute launchctl load: {e}"))?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let has_error = !output.status.success()
        || stderr.contains("failed")
        || stderr.contains("Failed")
        || stderr.contains("error")
        || stderr.contains("Error");

    if has_error {
        let msg = if !stderr.trim().is_empty() {
            stderr.trim()
        } else if !stdout.trim().is_empty() {
            stdout.trim()
        } else {
            "launchctl load failed with unknown error"
        };
        return Err(format!("launchctl load failed: {msg}"));
    }

    Ok(())
}

/// Non-macOS stub: the caller falls back to the detached spawn in `daemon::client`.
#[cfg(not(target_os = "macos"))]
pub fn load_launchd_agent(_plist_path: &Path) -> Result<(), String> {
    Err("LaunchAgent is macOS-only".to_string())
}

/// Makes the daemon's lifetime launchd's responsibility instead of the GUI's: writes the plist when
/// it changed, loads it when it is new or unloaded, and returns its path.
#[cfg(target_os = "macos")]
pub fn ensure_launchd_agent(executable_path: &Path) -> Result<PathBuf, String> {
    let plist_path = get_launchd_plist_path().ok_or("Cannot determine LaunchAgent path")?;
    let log_dir = launchd_log_dir_for(std::env::var_os("HOME").as_deref())
        .ok_or("Cannot determine the agent log directory")?;
    let rewritten = write_launchd_plist(executable_path, &log_dir, &plist_path)?;
    if rewritten || !launchd_job_is_loaded() {
        load_launchd_agent(&plist_path)?;
    }
    Ok(plist_path)
}

#[cfg(not(target_os = "macos"))]
pub fn ensure_launchd_agent(_executable_path: &Path) -> Result<PathBuf, String> {
    Err("LaunchAgent is macOS-only".to_string())
}

/// Writes the plist and loads it with `launchctl load -w`; macOS-only.
#[cfg(target_os = "macos")]
pub fn install_launchd_agent_for_path(
    executable_path: &Path,
    plist_path: &Path,
) -> Result<(), String> {
    let log_dir = launchd_log_dir_for(std::env::var_os("HOME").as_deref())
        .ok_or("Cannot determine the agent log directory")?;
    write_launchd_plist(executable_path, &log_dir, plist_path)?;
    load_launchd_agent(plist_path)
}

/// Non-macOS stub: no LaunchAgent exists, and the only caller already failed on the missing plist
/// path, so this cannot report a false success.
#[cfg(not(target_os = "macos"))]
pub fn install_launchd_agent_for_path(
    _executable_path: &Path,
    _plist_path: &Path,
) -> Result<(), String> {
    Ok(())
}

pub fn install_launchd_agent() -> Result<PathBuf, String> {
    let plist_path = get_launchd_plist_path().ok_or("Cannot determine HOME directory")?;
    let current_exe =
        std::env::current_exe().map_err(|e| format!("Failed to get current exe path: {e}"))?;

    install_launchd_agent_for_path(&current_exe, &plist_path)?;
    Ok(plist_path)
}

/// Unloads the LaunchAgent with `launchctl unload -w` and removes the plist; macOS-only.
#[cfg(target_os = "macos")]
pub fn uninstall_launchd_agent_from_path(plist_path: &Path) -> Result<(), String> {
    if plist_path.exists() {
        let output = Command::new("launchctl")
            .args(["unload", "-w", &plist_path.to_string_lossy()])
            .output()
            .map_err(|e| format!("Failed to execute launchctl unload: {e}"))?;

        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let has_error = !output.status.success()
            || stderr.contains("failed")
            || stderr.contains("Failed")
            || stderr.contains("error")
            || stderr.contains("Error");

        if has_error {
            let msg = if !stderr.trim().is_empty() {
                stderr.trim()
            } else if !stdout.trim().is_empty() {
                stdout.trim()
            } else {
                "launchctl unload failed with unknown error"
            };
            return Err(format!("launchctl unload failed: {msg}"));
        }

        fs::remove_file(plist_path).map_err(|e| format!("Failed to remove plist file: {e}"))?;
    }
    Ok(())
}

/// Non-macOS no-op, matching the idempotent success the macOS path returns for an absent plist.
#[cfg(not(target_os = "macos"))]
pub fn uninstall_launchd_agent_from_path(_plist_path: &Path) -> Result<(), String> {
    Ok(())
}

pub fn uninstall_launchd_agent() -> Result<(), String> {
    let plist_path = get_launchd_plist_path().ok_or("Cannot determine HOME directory")?;
    uninstall_launchd_agent_from_path(&plist_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_generate_plist_contains_required_fields() {
        let log_dir = Path::new("/Users/someone/Library/Logs/Ferryx");
        let plist = generate_launchd_plist("/opt/ferryx/bin/ferryx", log_dir);
        assert!(plist.contains(&format!("<string>{PLIST_LABEL}</string>")));
        assert!(plist.contains("<string>/opt/ferryx/bin/ferryx</string>"));
        assert!(plist.contains("<string>--daemon</string>"));
        assert!(plist.contains("<key>KeepAlive</key>"));
    }

    /// A world-writable `/tmp` output path lets another local user pre-create the file as a symlink
    /// and redirect an always-running daemon's output; the agent's logs belong in the user's own
    /// log directory.
    #[test]
    fn plist_never_writes_agent_output_to_a_shared_directory() {
        let log_dir = Path::new("/Users/someone/Library/Logs/Ferryx");
        let plist = generate_launchd_plist("/opt/ferryx/bin/ferryx", log_dir);
        assert!(plist.contains(&format!(
            "<key>StandardOutPath</key>\n    <string>{}/daemon.stdout.log</string>",
            log_dir.display()
        )));
        assert!(plist.contains(&format!(
            "<key>StandardErrorPath</key>\n    <string>{}/daemon.stderr.log</string>",
            log_dir.display()
        )));
        assert!(!plist.contains("/tmp/"), "agent output must not live in /tmp: {plist}");
    }

    #[test]
    fn keep_alive_restarts_only_abnormal_deaths() {
        let log_dir = Path::new("/Users/someone/Library/Logs/Ferryx");
        let plist = generate_launchd_plist("/opt/ferryx/bin/ferryx", log_dir);
        assert!(
            plist.contains("<key>RunAtLoad</key>"),
            "the agent must start the daemon at login: {plist}"
        );
        assert!(
            plist.contains("<key>SuccessfulExit</key>\n        <false/>"),
            "KeepAlive must be conditional on an abnormal exit, otherwise a committed handover or an \
             idle binary swap is relaunched straight back into the instance lock: {plist}"
        );
        assert!(
            !plist.contains("<key>KeepAlive</key>\n    <true/>"),
            "an unconditional KeepAlive is the failure this guards against: {plist}"
        );
    }

    #[test]
    fn plist_raises_the_descriptor_limit_above_the_launchd_default() {
        let log_dir = Path::new("/Users/someone/Library/Logs/Ferryx");
        let plist = generate_launchd_plist("/opt/ferryx/bin/ferryx", log_dir);
        assert!(
            plist.contains("<key>NumberOfFiles</key>\n        <integer>4096</integer>"),
            "the agent must raise the soft descriptor limit: {plist}"
        );
    }

    #[test]
    fn write_launchd_plist_reports_only_real_rewrites() {
        let dir = tempdir().unwrap();
        let plist_path = dir.path().join(format!("{PLIST_LABEL}.plist"));
        let log_dir = dir.path().join("logs");
        let exe = Path::new("/opt/ferryx/bin/ferryx");

        assert!(write_launchd_plist(exe, &log_dir, &plist_path).unwrap());
        assert!(
            log_dir.is_dir(),
            "launchd does not create a missing StandardOutPath parent, so the writer must"
        );
        assert!(
            !write_launchd_plist(exe, &log_dir, &plist_path).unwrap(),
            "an unchanged plist must not be rewritten on every GUI start"
        );
        assert!(write_launchd_plist(Path::new("/opt/ferryx/bin/ferryx-next"), &log_dir, &plist_path)
            .unwrap());
        assert!(fs::read_to_string(&plist_path)
            .unwrap()
            .contains("/opt/ferryx/bin/ferryx-next"));
    }

    #[test]
    fn plist_path_override_wins_over_the_real_launch_agents_directory() {
        let override_path = std::ffi::OsStr::new("/tmp/fixture/com.ferryx.daemon.plist");
        let home = std::ffi::OsStr::new("/Users/someone");
        assert_eq!(
            launchd_plist_path_for(Some(override_path), Some(home)),
            Some(PathBuf::from(override_path))
        );
        assert_eq!(
            launchd_plist_path_for(None, Some(home)),
            Some(PathBuf::from(format!(
                "/Users/someone/Library/LaunchAgents/{PLIST_LABEL}.plist"
            )))
        );
        assert_eq!(launchd_plist_path_for(None, None), None);
    }

    #[test]
    fn agent_log_directory_is_inside_the_users_own_home() {
        assert_eq!(
            launchd_log_dir_for(Some(std::ffi::OsStr::new("/Users/someone"))),
            Some(PathBuf::from("/Users/someone/Library/Logs/Ferryx"))
        );
        assert_eq!(launchd_log_dir_for(None), None);
    }

    #[test]
    fn an_isolated_runtime_never_manages_the_real_agent() {
        assert!(agent_management_supported_for(false, false));
        assert!(!agent_management_supported_for(true, false));
        assert!(!agent_management_supported_for(false, true));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_uninstall_surfaces_launchctl_failure() {
        let dir = tempdir().unwrap();
        let fake_plist = dir.path().join(format!("{PLIST_LABEL}.plist"));
        fs::write(&fake_plist, "invalid plist").unwrap();

        let result = uninstall_launchd_agent_from_path(&fake_plist);
        assert!(
            result.is_err(),
            "Expected error when unloading non-loaded/invalid plist via launchctl"
        );
        let err = result.unwrap_err();
        assert!(
            err.contains("launchctl unload failed"),
            "Error message should indicate launchctl unload failed: {err}"
        );
    }
}
