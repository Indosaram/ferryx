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
//! critically - does **not** kill it when the GUI quits. Callers never use this module directly:
//! the GUI and the daemon client go through the portable facade in `daemon::supervisor`, whose
//! macOS backend is this module.
//!
//! `KeepAlive` is deliberately `{ SuccessfulExit = false }` rather than `true`. The daemon exits 0
//! on purpose in three legitimate cases - a committed handover, an idle binary swap, and a
//! duplicate-endpoint startup - and an unconditional `KeepAlive` would fight all three by
//! relaunching instantly into the instance lock.
//!
//! Each daemon runtime generation (`daemon::runtime_stage`) gets its own job, labelled
//! `com.ferryx.daemon.<generation key>` (QA: `<FERRYX_DAEMON_SUPERVISOR_NAME>.<key>`), whose
//! `ProgramArguments` name the immutable staged copy of that generation, never the GUI's executable
//! inside the app bundle. Only the newest generation has `RunAtLoad` true. The [`generation`]
//! module holds the job-state rules: a running or unverifiable job is never touched, an Absent
//! owner is started only by `bootstrap` (job not registered) or a plain `kickstart` (registered and
//! idle; never `-k`), and older generations only lose their load-time launch by a plist rewrite
//! (`RunAtLoad` false and `KeepAlive` false, since `KeepAlive { SuccessfulExit }` implies
//! `RunAtLoad`). Nothing here unloads or boots out a job; only the explicit user uninstall in
//! [`uninstall_launchd_agent_from_path`] does.
//!
//! Other targets plug their own backend into `daemon::supervisor`; where none is supported the
//! client keeps the detached-spawn path in `daemon::client`, which already survives a GUI exit.
//!
//! Every path that spawns the `launchctl` binary is gated to `#[cfg(target_os = "macos")]`, so a
//! caller cannot shell out to a macOS-only tool on Windows or Linux.

use std::fs;
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::process::Command;

pub const PLIST_LABEL: &str = "com.ferryx.daemon";

/// Label of the pre-rebrand single job. Installs that predate the Ferryx name may still carry
/// `~/Library/LaunchAgents/com.rorca.daemon.plist`; the legacy migration treats it like
/// [`PLIST_LABEL`]'s single job.
pub const RORCA_PLIST_LABEL: &str = "com.rorca.daemon";

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

/// Renders the LaunchAgent that owns one daemon generation's lifetime.
///
/// `RunAtLoad` (true only for the newest generation) starts it at login;
/// `KeepAlive { SuccessfulExit = false }` restarts only abnormal deaths, leaving the daemon's
/// deliberate `exit(0)` paths (handover commit, idle binary swap, duplicate endpoint) alone.
/// `AbandonProcessGroup` keeps launchd from killing the daemon's PTY children when the job's main
/// process exits, and `ProcessType Interactive` keeps a terminal host from being throttled like a
/// background job. The soft descriptor limit is raised well above the 256 that launchd
/// hands out by default, because a terminal daemon spends roughly three descriptors per session and
/// a daemon that runs out mid-write dies with every session it owns; the daemon raises the soft
/// limit again at startup, which is what lifts it to the hard ceiling.
pub fn generate_launchd_plist(
    label: &str,
    executable_path: &str,
    log_dir: &Path,
    run_at_load: bool,
) -> String {
    let stdout_path = log_dir.join("daemon.stdout.log");
    let stderr_path = log_dir.join("daemon.stderr.log");
    let run_at_load = if run_at_load { "<true/>" } else { "<false/>" };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{executable_path}</string>
        <string>--daemon</string>
    </array>
    <key>RunAtLoad</key>
    {run_at_load}
    <key>KeepAlive</key>
    <dict>
        <key>SuccessfulExit</key>
        <false/>
    </dict>
    <key>AbandonProcessGroup</key>
    <true/>
    <key>ProcessType</key>
    <string>Interactive</string>
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

/// Writes the legacy single-job plist only when its content differs, and reports whether it was
/// rewritten. Nothing loads it: launchd reads it at the next login.
pub fn write_launchd_plist(
    executable_path: &Path,
    log_dir: &Path,
    plist_path: &Path,
) -> Result<bool, String> {
    let content = generate_launchd_plist(
        PLIST_LABEL,
        &executable_path.to_string_lossy(),
        log_dir,
        true,
    );
    generation::write_plist_if_changed(&content, log_dir, plist_path)
}

/// Per-generation launchd jobs, one per staged daemon runtime generation.
///
/// Every start decision here is made for a caller that holds owner admission
/// (`daemon::owner_admission`): only an Absent owner may start a job, and then only through
/// `bootstrap` (job not registered) or a plain `kickstart` (registered and idle). A running or
/// unverifiable job is never touched, and nothing here unloads, boots out or `kickstart -k`s a
/// job. Older generations only lose their load-time launch (`RunAtLoad` and `KeepAlive`) by a
/// plist rewrite, which launchd reads at the next login; their running process keeps going.
/// `launchctl` is injected through [`Launchctl`],
/// so these rules are testable on every target without the real launchd.
pub(crate) mod generation {
    #![cfg_attr(not(target_os = "macos"), allow(dead_code))]

    use super::generate_launchd_plist;
    use crate::daemon::owner_admission::OwnerState;
    use std::ffi::OsStr;
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};

    const RUN_AT_LOAD_TRUE: &str = "<key>RunAtLoad</key>\n    <true/>";
    const RUN_AT_LOAD_FALSE: &str = "<key>RunAtLoad</key>\n    <false/>";
    /// The `KeepAlive` dict [`generate_launchd_plist`] renders. launchd.plist(5): `SuccessfulExit`
    /// implies `RunAtLoad`, so a plist keeping it is still launched when launchd loads it at login.
    const KEEP_ALIVE_ABNORMAL_EXIT: &str = "<key>KeepAlive</key>\n    <dict>\n        <key>SuccessfulExit</key>\n        <false/>\n    </dict>";
    /// The unconditional `KeepAlive` of the pre-rebrand `com.rorca.daemon` plist.
    const KEEP_ALIVE_TRUE: &str = "<key>KeepAlive</key>\n    <true/>";
    const KEEP_ALIVE_FALSE: &str = "<key>KeepAlive</key>\n    <false/>";

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct LaunchctlOutput {
        pub success: bool,
        pub stdout: String,
        pub stderr: String,
    }

    /// Runs `launchctl <args>`.
    pub(crate) trait Launchctl {
        fn run(&self, args: &[&str]) -> io::Result<LaunchctlOutput>;
    }

    /// The real `launchctl` binary.
    #[cfg(target_os = "macos")]
    pub(crate) struct SystemLaunchctl;

    #[cfg(target_os = "macos")]
    impl Launchctl for SystemLaunchctl {
        fn run(&self, args: &[&str]) -> io::Result<LaunchctlOutput> {
            let output = std::process::Command::new("launchctl")
                .args(args)
                .stdin(std::process::Stdio::null())
                .output()?;
            Ok(LaunchctlOutput {
                success: output.status.success(),
                stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            })
        }
    }

    /// What `launchctl print gui/<uid>/<label>` says about a job.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) enum JobState {
        /// Exit non-zero with "Could not find service".
        NotRegistered,
        /// Registered with a `pid = N` line.
        Running { pid: u32 },
        /// Registered, no pid line.
        RegisteredIdle,
        /// Anything else, including a non-zero exit with other text.
        Unverifiable { detail: String },
    }

    pub(crate) fn job_state_from_print(result: io::Result<LaunchctlOutput>) -> JobState {
        let output = match result {
            Ok(output) => output,
            Err(error) => {
                return JobState::Unverifiable {
                    detail: format!("launchctl print could not run: {error}"),
                }
            }
        };
        if !output.success {
            if output.stderr.contains("Could not find service")
                || output.stdout.contains("Could not find service")
            {
                return JobState::NotRegistered;
            }
            return JobState::Unverifiable {
                detail: format!("launchctl print failed: {}", output.stderr.trim()),
            };
        }
        let pid_line = output
            .stdout
            .lines()
            .map(str::trim)
            .find_map(|line| line.strip_prefix("pid = "));
        match pid_line {
            None => JobState::RegisteredIdle,
            Some(pid) => match pid.trim().parse() {
                Ok(pid) => JobState::Running { pid },
                Err(_) => JobState::Unverifiable {
                    detail: format!("launchctl print reported an unreadable pid: {pid}"),
                },
            },
        }
    }

    /// Read-only; the domain comes from the calling uid, not `$HOME`.
    pub(crate) fn print_job(launchctl: &dyn Launchctl, uid: u32, label: &str) -> JobState {
        let target = format!("gui/{uid}/{label}");
        job_state_from_print(launchctl.run(&["print", target.as_str()]))
    }

    /// `<base>.<generation key>`, e.g. `com.ferryx.daemon.2026.1013.2-0a1b2c3d`.
    pub(crate) fn generation_label(base: &str, key: &str) -> String {
        format!("{base}.{key}")
    }

    /// Directory of the generation plists: `~/Library/LaunchAgents`, or for QA the directory named
    /// by `FERRYX_DAEMON_PLIST_PATH` (a directory, or the directory of a `.plist` path). An empty
    /// override resolves to nothing, like [`super::launchd_plist_path_for`].
    pub(crate) fn launch_agents_dir(
        override_path: Option<&OsStr>,
        home: Option<&OsStr>,
    ) -> Option<PathBuf> {
        if let Some(override_path) = override_path {
            if override_path.is_empty() {
                return None;
            }
            let path = PathBuf::from(override_path);
            if path.extension().is_some_and(|extension| extension == "plist") {
                return path.parent().map(Path::to_path_buf);
            }
            return Some(path);
        }
        home.map(|home| PathBuf::from(home).join("Library/LaunchAgents"))
    }

    /// One generation's job: its label, its staged executable and where its plist lives.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct GenerationJob {
        pub label: String,
        pub executable: PathBuf,
        pub plist_path: PathBuf,
        pub log_dir: PathBuf,
    }

    impl GenerationJob {
        pub(crate) fn render(&self, run_at_load: bool) -> String {
            generate_launchd_plist(
                &self.label,
                &self.executable.to_string_lossy(),
                &self.log_dir,
                run_at_load,
            )
        }
    }

    /// Writes `content` only when it differs and reports whether it did. Creates the log dir,
    /// because launchd does not create a missing `StandardOutPath` parent and a job whose output
    /// cannot be opened does not run.
    pub(crate) fn write_plist_if_changed(
        content: &str,
        log_dir: &Path,
        plist_path: &Path,
    ) -> Result<bool, String> {
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

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) enum EnsureOutcome {
        /// The job was not registered: its plist was written and bootstrapped.
        Bootstrapped,
        /// The job was registered and idle: one plain kickstart.
        Kickstarted,
        /// An owner exists; launchd was not asked anything.
        OwnerPresent,
        /// The job is running or its state is unverifiable; it was left alone.
        Untouched(JobState),
    }

    /// The job-state switch for the current generation, evaluated under owner admission.
    ///
    /// Owner Absent + NotRegistered -> write plist, `launchctl bootstrap gui/<uid> <plist>`;
    /// owner Absent + RegisteredIdle -> `launchctl kickstart gui/<uid>/<label>` (never `-k`, no
    /// plist rewrite); owner Alive or Unknown, or job Running or Unverifiable -> zero mutations.
    ///
    /// With `login_autostart` false (autostart opt-out) a NotRegistered job gets a plist with no load-time launch
    /// condition (`RunAtLoad` and `KeepAlive` false), so launchd never starts it at login; because
    /// such a plist is not started by `bootstrap` either, the bootstrap is followed by one plain
    /// `kickstart` (never `-k`). The start still goes through the job, never as a GUI child. A
    /// RegisteredIdle job gets the same single kickstart and no plist rewrite either way.
    pub(crate) fn ensure_generation_job(
        launchctl: &dyn Launchctl,
        uid: u32,
        job: &GenerationJob,
        owner: &OwnerState,
        login_autostart: bool,
    ) -> Result<EnsureOutcome, String> {
        if *owner != OwnerState::Absent {
            return Ok(EnsureOutcome::OwnerPresent);
        }
        match print_job(launchctl, uid, &job.label) {
            JobState::NotRegistered => {
                let content = if login_autostart {
                    job.render(true)
                } else {
                    without_load_time_launch(&job.render(false))
                };
                write_plist_if_changed(&content, &job.log_dir, &job.plist_path)?;
                let domain = format!("gui/{uid}");
                let plist = job.plist_path.to_string_lossy().into_owned();
                mutate(launchctl, &["bootstrap", domain.as_str(), plist.as_str()])?;
                if !login_autostart {
                    let target = format!("gui/{uid}/{}", job.label);
                    mutate(launchctl, &["kickstart", target.as_str()])?;
                }
                Ok(EnsureOutcome::Bootstrapped)
            }
            JobState::RegisteredIdle => {
                let target = format!("gui/{uid}/{}", job.label);
                mutate(launchctl, &["kickstart", target.as_str()])?;
                Ok(EnsureOutcome::Kickstarted)
            }
            state => Ok(EnsureOutcome::Untouched(state)),
        }
    }

    fn mutate(launchctl: &dyn Launchctl, args: &[&str]) -> Result<(), String> {
        let output = launchctl
            .run(args)
            .map_err(|e| format!("Failed to run launchctl {}: {e}", args[0]))?;
        if output.success {
            return Ok(());
        }
        let detail = if output.stderr.trim().is_empty() {
            output.stdout.trim()
        } else {
            output.stderr.trim()
        };
        Err(format!("launchctl {} failed: {detail}", args[0]))
    }

    /// Registers the newest generation for the next login: writes its plist with `RunAtLoad` true
    /// and calls no `launchctl` at all, so launchd loads it only at the next login.
    pub(crate) fn register_newest_for_login(job: &GenerationJob) -> Result<bool, String> {
        write_plist_if_changed(&job.render(true), &job.log_dir, &job.plist_path)
    }

    /// Generation plists `<base>.<key>.plist` in `dir`, sorted by label. The legacy single-job
    /// `<base>.plist` is not a generation.
    pub(crate) fn generation_plists(dir: &Path, base: &str) -> Result<Vec<(String, PathBuf)>, String> {
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(format!("Failed to read {}: {e}", dir.display())),
        };
        let prefix = format!("{base}.");
        let mut plists = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| format!("Failed to read {}: {e}", dir.display()))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(label) = name.strip_suffix(".plist") else {
                continue;
            };
            if label.len() > prefix.len() && label.starts_with(&prefix) {
                plists.push((label.to_string(), entry.path()));
            }
        }
        plists.sort();
        Ok(plists)
    }

    /// The plist without any load-time launch condition: `RunAtLoad` false and `KeepAlive` false
    /// (`KeepAlive { SuccessfulExit }` implies `RunAtLoad`). Everything else is unchanged.
    fn without_load_time_launch(content: &str) -> String {
        content
            .replacen(RUN_AT_LOAD_TRUE, RUN_AT_LOAD_FALSE, 1)
            .replacen(KEEP_ALIVE_ABNORMAL_EXIT, KEEP_ALIVE_FALSE, 1)
    }

    /// Removes every load-time launch condition ([`without_load_time_launch`]) by rewriting the
    /// plist file only. launchd reads the file at the next login and a running job keeps its current
    /// definition. A missing or already-disabled plist is left unchanged. Returns whether the file
    /// changed.
    pub(crate) fn disable_login_autostart(plist_path: &Path) -> Result<bool, String> {
        let content = match fs::read_to_string(plist_path) {
            Ok(content) => content,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(format!("Failed to read {}: {e}", plist_path.display())),
        };
        let disabled = without_load_time_launch(&content);
        if disabled == content {
            return Ok(false);
        }
        fs::write(plist_path, disabled)
            .map_err(|e| format!("Failed to rewrite {}: {e}", plist_path.display()))?;
        Ok(true)
    }

    /// Every generation except `newest_label` loses login autostart by plist rewrite; its plist is
    /// deleted only when launchd reports no process for it (not registered, or registered and
    /// idle). Only `launchctl print` runs: an older generation is never booted out, unloaded or
    /// kickstarted. Returns whether any file changed.
    pub(crate) fn retire_older_generations(
        launchctl: &dyn Launchctl,
        uid: u32,
        dir: &Path,
        base: &str,
        newest_label: &str,
    ) -> Result<bool, String> {
        let mut changed = false;
        for (label, path) in generation_plists(dir, base)? {
            if label == newest_label {
                continue;
            }
            changed |= disable_login_autostart(&path)?;
            match print_job(launchctl, uid, &label) {
                JobState::NotRegistered | JobState::RegisteredIdle => match fs::remove_file(&path) {
                    Ok(()) => changed = true,
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(format!("Failed to remove {}: {e}", path.display())),
                },
                JobState::Running { .. } | JobState::Unverifiable { .. } => {}
            }
        }
        Ok(changed)
    }

    /// Migrates one legacy single job (`com.ferryx.daemon`, `com.rorca.daemon`, or the QA
    /// `<base>`) for the owner that admission classified under its lock.
    ///
    /// A legacy job never gets a `launchctl` mutation. Its plist always loses its load-time launch
    /// by file rewrite, so the next login starts only the newest generation. With an Alive or
    /// Unknown owner that is all: the job stays loaded and its daemon keeps running. With an
    /// Absent owner the plist is also deleted, but only when `launchctl print` reports no process
    /// for the label (not registered, or registered and idle); a running or unverifiable legacy
    /// job keeps its (disabled) plist. A missing plist is a no-op with no `launchctl` call, so a
    /// second run after the migration issues nothing. Returns whether any file changed.
    pub(crate) fn migrate_legacy_job(
        launchctl: &dyn Launchctl,
        uid: u32,
        label: &str,
        plist_path: &Path,
        owner: &OwnerState,
    ) -> Result<bool, String> {
        match fs::symlink_metadata(plist_path) {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(format!("Failed to inspect {}: {e}", plist_path.display())),
        }
        let mut changed = disable_login_autostart(plist_path)?;
        changed |= drop_unconditional_keep_alive(plist_path)?;
        if *owner != OwnerState::Absent {
            return Ok(changed);
        }
        match print_job(launchctl, uid, label) {
            JobState::NotRegistered | JobState::RegisteredIdle => match fs::remove_file(plist_path) {
                Ok(()) => changed = true,
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("Failed to remove {}: {e}", plist_path.display())),
            },
            state @ (JobState::Running { .. } | JobState::Unverifiable { .. }) => {
                tracing::info!(%label, ?state, "Legacy daemon job still has a process; it keeps running and only lost login autostart");
            }
        }
        Ok(changed)
    }

    /// The pre-rebrand `com.rorca.daemon` plist kept an unconditional `KeepAlive`, which launchd
    /// also treats as a load-time launch; it becomes `false` by file rewrite only. Kept out of
    /// [`disable_login_autostart`] because only legacy plists were ever rendered that way.
    fn drop_unconditional_keep_alive(plist_path: &Path) -> Result<bool, String> {
        let content = match fs::read_to_string(plist_path) {
            Ok(content) => content,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(format!("Failed to read {}: {e}", plist_path.display())),
        };
        let disabled = content.replacen(KEEP_ALIVE_TRUE, KEEP_ALIVE_FALSE, 1);
        if disabled == content {
            return Ok(false);
        }
        fs::write(plist_path, disabled)
            .map_err(|e| format!("Failed to rewrite {}: {e}", plist_path.display()))?;
        Ok(true)
    }

    /// First `ProgramArguments` entry of a plist: the executable launchd runs.
    pub(crate) fn plist_program(content: &str) -> Option<&str> {
        let after_key = &content[content.find("<key>ProgramArguments</key>")?..];
        let start = after_key.find("<string>")? + "<string>".len();
        let end = after_key[start..].find("</string>")?;
        Some(&after_key[start..start + end])
    }

    /// Executables referenced by the generation plists in `dir`, for the runtime garbage collector
    /// (`runtime_stage::RuntimeUsage::registered_job_executables`). An unreadable plist is an
    /// error, so the collector never removes a runtime a job may still reference.
    pub(crate) fn job_executables(dir: &Path, base: &str) -> Result<Vec<PathBuf>, String> {
        let mut executables = Vec::new();
        for (_, path) in generation_plists(dir, base)? {
            let content = match fs::read_to_string(&path) {
                Ok(content) => content,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => return Err(format!("Failed to read {}: {e}", path.display())),
            };
            let program = plist_program(&content)
                .ok_or_else(|| format!("{} names no program", path.display()))?;
            executables.push(PathBuf::from(program));
        }
        Ok(executables)
    }
}

/// Unloads the LaunchAgent with `launchctl unload -w` and removes the plist; macOS-only. Reachable
/// only from an explicit user uninstall.
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
        let plist = generate_launchd_plist(PLIST_LABEL, "/opt/ferryx/bin/ferryx", log_dir, true);
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
        let plist = generate_launchd_plist(PLIST_LABEL, "/opt/ferryx/bin/ferryx", log_dir, true);
        let stdout_path = log_dir.join("daemon.stdout.log");
        let stderr_path = log_dir.join("daemon.stderr.log");
        assert!(plist.contains(&format!(
            "<key>StandardOutPath</key>\n    <string>{}</string>",
            stdout_path.display()
        )));
        assert!(plist.contains(&format!(
            "<key>StandardErrorPath</key>\n    <string>{}</string>",
            stderr_path.display()
        )));
        assert!(!plist.contains("/tmp/"), "agent output must not live in /tmp: {plist}");
    }

    #[test]
    fn keep_alive_restarts_only_abnormal_deaths() {
        let log_dir = Path::new("/Users/someone/Library/Logs/Ferryx");
        let plist = generate_launchd_plist(PLIST_LABEL, "/opt/ferryx/bin/ferryx", log_dir, true);
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
        let plist = generate_launchd_plist(PLIST_LABEL, "/opt/ferryx/bin/ferryx", log_dir, true);
        assert!(
            plist.contains("<key>NumberOfFiles</key>\n        <integer>4096</integer>"),
            "the agent must raise the soft descriptor limit: {plist}"
        );
    }

    mod generation_jobs {
        use super::super::generation::*;
        use crate::daemon::owner_admission::OwnerState;
        use std::cell::RefCell;
        use std::collections::HashMap;
        use std::fs;
        use std::io;
        use std::path::{Path, PathBuf};

        const UID: u32 = 501;
        const BASE: &str = "com.ferryx.daemon";
        const KEY: &str = "2026.1013.2-0a1b2c3d";

        /// Fake launchctl: answers `print` from a per-label script and records every call.
        #[derive(Default)]
        struct FakeLaunchctl {
            prints: HashMap<String, LaunchctlOutput>,
            calls: RefCell<Vec<Vec<String>>>,
        }

        impl FakeLaunchctl {
            fn with_print(mut self, label: &str, success: bool, stdout: &str, stderr: &str) -> Self {
                self.prints.insert(
                    format!("gui/{UID}/{label}"),
                    LaunchctlOutput {
                        success,
                        stdout: stdout.to_string(),
                        stderr: stderr.to_string(),
                    },
                );
                self
            }
            fn not_registered(self, label: &str) -> Self {
                self.with_print(label, false, "", "Bad request.\nCould not find service \"x\" in domain for user gui: 501\n")
            }
            fn running(self, label: &str, pid: u32) -> Self {
                self.with_print(label, true, &format!("{label} = {{\n\tstate = running\n\tpid = {pid}\n}}\n"), "")
            }
            fn idle(self, label: &str) -> Self {
                self.with_print(label, true, &format!("{label} = {{\n\tstate = not running\n\tlast exit code = 0\n}}\n"), "")
            }
            fn calls(&self) -> Vec<Vec<String>> {
                self.calls.borrow().clone()
            }
            /// Every call that is not a read-only `print`.
            fn mutations(&self) -> Vec<Vec<String>> {
                self.calls().into_iter().filter(|call| call[0] != "print").collect()
            }
        }

        impl Launchctl for FakeLaunchctl {
            fn run(&self, args: &[&str]) -> io::Result<LaunchctlOutput> {
                self.calls.borrow_mut().push(args.iter().map(|arg| arg.to_string()).collect());
                if args[0] == "print" {
                    return self.prints.get(args[1]).cloned().ok_or_else(|| io::Error::other("unscripted print"));
                }
                Ok(LaunchctlOutput { success: true, stdout: String::new(), stderr: String::new() })
            }
        }

        fn job(dir: &Path) -> GenerationJob {
            let label = generation_label(BASE, KEY);
            GenerationJob {
                plist_path: dir.join(format!("{label}.plist")),
                label,
                executable: dir.join(format!("daemon-runtime/{KEY}/Ferryx Daemon.app/Contents/MacOS/ferryx")),
                log_dir: dir.join("logs"),
            }
        }

        /// Only `print`, `bootstrap` and a plain `kickstart` are ever allowed; anything else
        /// (bootout, unload, stop, remove, kickstart -k, ...) could end a live daemon.
        fn assert_never_stops(calls: &[Vec<String>]) {
            for call in calls {
                assert!(
                    matches!(call[0].as_str(), "print" | "bootstrap" | "kickstart"),
                    "forbidden launchctl call {call:?}"
                );
                assert!(!call.iter().any(|arg| arg == "-k"), "kickstart -k kills the running job: {call:?}");
            }
        }

        #[test]
        fn generation_plist_names_staged_runtime_label_and_abandons_process_group() {
            let dir = tempfile::tempdir().unwrap();
            let job = job(dir.path());
            let plist = job.render(true);
            assert!(plist.contains(&format!("<string>{BASE}.{KEY}</string>")));
            assert!(plist.contains(&format!("<string>{}</string>", job.executable.display())));
            assert_eq!(plist_program(&plist), Some(job.executable.to_str().unwrap()));
            assert!(plist.contains("<key>AbandonProcessGroup</key>\n    <true/>"));
            assert!(plist.contains("<key>ProcessType</key>\n    <string>Interactive</string>"));
            assert!(plist.contains("<key>SuccessfulExit</key>\n        <false/>"));
            assert!(plist.contains("<key>RunAtLoad</key>\n    <true/>"));
            assert!(job.render(false).contains("<key>RunAtLoad</key>\n    <false/>"));
        }

        #[test]
        fn launchctl_print_maps_to_job_states() {
            let ok = |stdout: &str| Ok(LaunchctlOutput { success: true, stdout: stdout.into(), stderr: String::new() });
            let failed = |stderr: &str| Ok(LaunchctlOutput { success: false, stdout: String::new(), stderr: stderr.into() });
            assert_eq!(job_state_from_print(failed("Could not find service \"x\" in domain")), JobState::NotRegistered);
            assert_eq!(job_state_from_print(ok("x = {\n\tpid = 4242\n}")), JobState::Running { pid: 4242 });
            assert_eq!(job_state_from_print(ok("x = {\n\tstate = not running\n}")), JobState::RegisteredIdle);
            assert!(matches!(job_state_from_print(failed("Operation not permitted")), JobState::Unverifiable { .. }));
            assert!(matches!(job_state_from_print(ok("\tpid = nope")), JobState::Unverifiable { .. }));
            assert!(matches!(job_state_from_print(Err(io::Error::other("spawn failed"))), JobState::Unverifiable { .. }));
        }

        #[test]
        fn absent_owner_and_unregistered_job_writes_plist_and_bootstraps_once() {
            let dir = tempfile::tempdir().unwrap();
            let job = job(dir.path());
            let launchctl = FakeLaunchctl::default().not_registered(&job.label);
            let outcome = ensure_generation_job(&launchctl, UID, &job, &OwnerState::Absent, true).unwrap();
            assert_eq!(outcome, EnsureOutcome::Bootstrapped);
            assert_eq!(
                launchctl.mutations(),
                vec![vec!["bootstrap".to_string(), format!("gui/{UID}"), job.plist_path.to_string_lossy().into_owned()]]
            );
            assert_eq!(fs::read_to_string(&job.plist_path).unwrap(), job.render(true));
        }

        #[test]
        fn registered_idle_job_with_absent_owner_gets_exactly_one_kickstart_without_k() {
            let dir = tempfile::tempdir().unwrap();
            let job = job(dir.path());
            let launchctl = FakeLaunchctl::default().idle(&job.label);
            let outcome = ensure_generation_job(&launchctl, UID, &job, &OwnerState::Absent, true).unwrap();
            assert_eq!(outcome, EnsureOutcome::Kickstarted);
            assert_eq!(launchctl.mutations(), vec![vec!["kickstart".to_string(), format!("gui/{UID}/{}", job.label)]]);
            assert!(!job.plist_path.exists(), "a kickstart must not rewrite the plist");
            assert_never_stops(&launchctl.calls());
        }

        #[test]
        fn running_job_gets_zero_launchctl_mutations() {
            let dir = tempfile::tempdir().unwrap();
            let job = job(dir.path());
            let launchctl = FakeLaunchctl::default().running(&job.label, 4242);
            let outcome = ensure_generation_job(&launchctl, UID, &job, &OwnerState::Absent, true).unwrap();
            assert_eq!(outcome, EnsureOutcome::Untouched(JobState::Running { pid: 4242 }));
            assert!(launchctl.mutations().is_empty());
            assert!(!job.plist_path.exists());
        }

        #[test]
        fn unverifiable_job_gets_zero_launchctl_mutations() {
            let dir = tempfile::tempdir().unwrap();
            let job = job(dir.path());
            let launchctl = FakeLaunchctl::default().with_print(&job.label, false, "", "Operation not permitted");
            let outcome = ensure_generation_job(&launchctl, UID, &job, &OwnerState::Absent, true).unwrap();
            assert!(matches!(outcome, EnsureOutcome::Untouched(JobState::Unverifiable { .. })));
            assert!(launchctl.mutations().is_empty());
            // A print that cannot even run is unverifiable too.
            let silent = FakeLaunchctl::default();
            let outcome = ensure_generation_job(&silent, UID, &job, &OwnerState::Absent, true).unwrap();
            assert!(matches!(outcome, EnsureOutcome::Untouched(JobState::Unverifiable { .. })));
            assert!(silent.mutations().is_empty());
        }

        #[test]
        fn alive_or_unknown_owner_gets_zero_launchctl_calls() {
            let dir = tempfile::tempdir().unwrap();
            let job = job(dir.path());
            for owner in [
                OwnerState::Alive { pid: 7, start_time: 9 },
                OwnerState::Unknown { reason: "lock held, holder unreadable".into() },
            ] {
                let launchctl = FakeLaunchctl::default().idle(&job.label);
                let outcome = ensure_generation_job(&launchctl, UID, &job, &owner, true).unwrap();
                assert_eq!(outcome, EnsureOutcome::OwnerPresent);
                assert!(launchctl.calls().is_empty(), "{owner:?} must not reach launchctl");
                assert!(!job.plist_path.exists());
            }
        }

        #[test]
        fn older_generation_is_never_booted_out_or_unloaded() {
            let dir = tempfile::tempdir().unwrap();
            let newest = job(dir.path());
            let old_running = generation_label(BASE, "2026.1001.1-11111111");
            let old_idle = generation_label(BASE, "2026.1005.1-22222222");
            let old_unverifiable = generation_label(BASE, "2026.1007.1-33333333");
            for label in [&old_running, &old_idle, &old_unverifiable] {
                let older = GenerationJob {
                    label: label.clone(),
                    executable: dir.path().join(format!("daemon-runtime/{label}/ferryx")),
                    plist_path: dir.path().join(format!("{label}.plist")),
                    log_dir: newest.log_dir.clone(),
                };
                fs::write(&older.plist_path, older.render(true)).unwrap();
            }
            // The legacy single job is not a generation and is left alone here.
            let legacy = dir.path().join(format!("{BASE}.plist"));
            fs::write(&legacy, "legacy").unwrap();
            register_newest_for_login(&newest).unwrap();

            let launchctl = FakeLaunchctl::default()
                .running(&old_running, 100)
                .idle(&old_idle)
                .with_print(&old_unverifiable, false, "", "Operation not permitted");
            assert!(retire_older_generations(&launchctl, UID, dir.path(), BASE, &newest.label).unwrap());

            assert!(launchctl.mutations().is_empty(), "only launchctl print may run: {:?}", launchctl.calls());
            assert_never_stops(&launchctl.calls());
            let running_plist = fs::read_to_string(dir.path().join(format!("{old_running}.plist"))).unwrap();
            assert!(running_plist.contains("<key>RunAtLoad</key>\n    <false/>"), "a running older job loses login autostart only");
            assert!(!running_plist.contains("SuccessfulExit"), "KeepAlive SuccessfulExit implies RunAtLoad: {running_plist}");
            assert!(running_plist.contains("<key>KeepAlive</key>\n    <false/>"));
            let unverifiable_plist = fs::read_to_string(dir.path().join(format!("{old_unverifiable}.plist"))).unwrap();
            assert!(unverifiable_plist.contains("<key>RunAtLoad</key>\n    <false/>"));
            assert!(!unverifiable_plist.contains("SuccessfulExit"), "{unverifiable_plist}");
            assert!(!dir.path().join(format!("{old_idle}.plist")).exists(), "an idle older job's plist is deleted");
            assert_eq!(fs::read_to_string(&newest.plist_path).unwrap(), newest.render(true), "the newest generation keeps RunAtLoad and KeepAlive");
            assert_eq!(fs::read_to_string(&legacy).unwrap(), "legacy");

            // A second run changes nothing.
            assert!(!retire_older_generations(&launchctl, UID, dir.path(), BASE, &newest.label).unwrap());
            assert_eq!(fs::read_to_string(dir.path().join(format!("{old_running}.plist"))).unwrap(), running_plist);
        }

        /// Backs `supervisor`'s macOS `remove_autostart` (opt-out) as well as retirement: a disabled
        /// plist has no load-time launch condition left, whatever `RunAtLoad` it was rendered with.
        #[test]
        fn disabled_plist_has_no_load_time_launch_condition_and_rewrite_is_idempotent() {
            let dir = tempfile::tempdir().unwrap();
            let job = job(dir.path());
            for run_at_load in [true, false] {
                fs::write(&job.plist_path, job.render(run_at_load)).unwrap();
                assert!(disable_login_autostart(&job.plist_path).unwrap());
                let disabled = fs::read_to_string(&job.plist_path).unwrap();
                assert!(!disabled.contains("SuccessfulExit"), "{disabled}");
                assert!(!disabled.contains("<true/>\n    <key>KeepAlive</key>"), "{disabled}");
                assert!(disabled.contains("<key>RunAtLoad</key>\n    <false/>"), "{disabled}");
                assert!(disabled.contains("<key>KeepAlive</key>\n    <false/>"), "{disabled}");
                assert_eq!(plist_program(&disabled), Some(job.executable.to_str().unwrap()));
                assert!(disabled.contains("<key>AbandonProcessGroup</key>\n    <true/>"));

                assert!(!disable_login_autostart(&job.plist_path).unwrap());
                assert_eq!(fs::read_to_string(&job.plist_path).unwrap(), disabled, "a second rewrite keeps the same bytes");
            }
            let missing = dir.path().join("missing.plist");
            assert!(!disable_login_autostart(&missing).unwrap());
            assert!(!missing.exists());
        }

        #[test]
        fn newest_generation_login_registration_calls_no_launchctl_and_is_idempotent() {
            let dir = tempfile::tempdir().unwrap();
            let job = job(dir.path());
            assert!(register_newest_for_login(&job).unwrap());
            assert!(!register_newest_for_login(&job).unwrap());
            assert!(job.log_dir.is_dir());
        }

        /// Opted out, an Absent owner still starts through the job (bootstrap, then one plain
        /// kickstart, since a plist without load-time launch is not started by bootstrap), and the
        /// plist it leaves has no login launch condition.
        #[test]
        fn opted_out_unregistered_job_is_bootstrapped_without_login_launch_and_kickstarted_once() {
            let dir = tempfile::tempdir().unwrap();
            let job = job(dir.path());
            let launchctl = FakeLaunchctl::default().not_registered(&job.label);
            let outcome =
                ensure_generation_job(&launchctl, UID, &job, &OwnerState::Absent, false).unwrap();
            assert_eq!(outcome, EnsureOutcome::Bootstrapped);
            assert_eq!(
                launchctl.mutations(),
                vec![
                    vec!["bootstrap".to_string(), format!("gui/{UID}"), job.plist_path.to_string_lossy().into_owned()],
                    vec!["kickstart".to_string(), format!("gui/{UID}/{}", job.label)],
                ]
            );
            assert_never_stops(&launchctl.calls());
            let plist = fs::read_to_string(&job.plist_path).unwrap();
            assert!(plist.contains("<key>RunAtLoad</key>\n    <false/>"), "{plist}");
            assert!(plist.contains("<key>KeepAlive</key>\n    <false/>"), "{plist}");
            assert!(!plist.contains("SuccessfulExit"), "KeepAlive SuccessfulExit implies RunAtLoad: {plist}");
            assert_eq!(plist_program(&plist), Some(job.executable.to_str().unwrap()));
            assert!(!disable_login_autostart(&job.plist_path).unwrap(), "already without login launch");

            // Opting back in registers the newest generation for login again.
            assert!(register_newest_for_login(&job).unwrap());
            assert_eq!(fs::read_to_string(&job.plist_path).unwrap(), job.render(true));
        }

        #[test]
        fn opted_out_launch_never_touches_a_running_unverifiable_or_owned_job() {
            let dir = tempfile::tempdir().unwrap();
            let job = job(dir.path());
            let idle = FakeLaunchctl::default().idle(&job.label);
            assert_eq!(
                ensure_generation_job(&idle, UID, &job, &OwnerState::Absent, false).unwrap(),
                EnsureOutcome::Kickstarted
            );
            assert_eq!(idle.mutations(), vec![vec!["kickstart".to_string(), format!("gui/{UID}/{}", job.label)]]);
            assert!(!job.plist_path.exists(), "a kickstart must not rewrite the plist");

            let running = FakeLaunchctl::default().running(&job.label, 4242);
            assert_eq!(
                ensure_generation_job(&running, UID, &job, &OwnerState::Absent, false).unwrap(),
                EnsureOutcome::Untouched(JobState::Running { pid: 4242 })
            );
            assert!(running.mutations().is_empty());
            let unverifiable = FakeLaunchctl::default().with_print(&job.label, false, "", "Operation not permitted");
            assert!(matches!(
                ensure_generation_job(&unverifiable, UID, &job, &OwnerState::Absent, false).unwrap(),
                EnsureOutcome::Untouched(JobState::Unverifiable { .. })
            ));
            assert!(unverifiable.mutations().is_empty());
            let owned = FakeLaunchctl::default().not_registered(&job.label);
            assert_eq!(
                ensure_generation_job(&owned, UID, &job, &OwnerState::Alive { pid: 7, start_time: 9 }, false).unwrap(),
                EnsureOutcome::OwnerPresent
            );
            assert!(owned.calls().is_empty());
            assert!(!job.plist_path.exists());
        }

        /// Opt-out over every generation plist (and the legacy one) is a file rewrite per plist: no
        /// launchctl runs at all, so no job is unloaded, booted out or stopped.
        #[test]
        fn opt_out_disables_every_generation_and_the_legacy_plist_by_file_rewrite_only() {
            let dir = tempfile::tempdir().unwrap();
            let newest = job(dir.path());
            let older_label = generation_label(BASE, "2026.1001.1-11111111");
            let older = GenerationJob {
                label: older_label.clone(),
                executable: dir.path().join("daemon-runtime/2026.1001.1-11111111/ferryx"),
                plist_path: dir.path().join(format!("{older_label}.plist")),
                log_dir: newest.log_dir.clone(),
            };
            register_newest_for_login(&newest).unwrap();
            fs::write(&older.plist_path, older.render(false)).unwrap();
            let legacy = dir.path().join(format!("{BASE}.plist"));
            fs::write(&legacy, super::super::generate_launchd_plist(BASE, "/Applications/Ferryx.app/Contents/MacOS/ferryx", &newest.log_dir, true)).unwrap();

            let mut changed = Vec::new();
            for (_, path) in generation_plists(dir.path(), BASE).unwrap() {
                changed.push(disable_login_autostart(&path).unwrap());
            }
            changed.push(disable_login_autostart(&legacy).unwrap());
            assert_eq!(changed, vec![true, true, true], "older, newest and legacy each lose their login launch");
            for path in [&older.plist_path, &newest.plist_path, &legacy] {
                let plist = fs::read_to_string(path).unwrap();
                assert!(plist.contains("<key>RunAtLoad</key>\n    <false/>"), "{plist}");
                assert!(!plist.contains("SuccessfulExit"), "{plist}");
                assert!(plist.contains("<key>AbandonProcessGroup</key>\n    <true/>"));
            }
            for (_, path) in generation_plists(dir.path(), BASE).unwrap() {
                assert!(!disable_login_autostart(&path).unwrap(), "idempotent");
            }
        }

        #[test]
        fn job_executables_report_every_generation_program_for_runtime_gc() {
            let dir = tempfile::tempdir().unwrap();
            let job = job(dir.path());
            register_newest_for_login(&job).unwrap();
            fs::write(dir.path().join("unrelated.plist"), "x").unwrap();
            assert_eq!(job_executables(dir.path(), BASE).unwrap(), vec![job.executable.clone()]);
            fs::write(dir.path().join(format!("{BASE}.broken.plist")), "no program").unwrap();
            assert!(job_executables(dir.path(), BASE).is_err(), "an unreadable job must block garbage collection");
        }

        #[test]
        fn launch_agents_dir_honors_the_qa_override() {
            let home = std::ffi::OsStr::new("/Users/someone");
            assert_eq!(launch_agents_dir(None, Some(home)), Some(PathBuf::from("/Users/someone/Library/LaunchAgents")));
            assert_eq!(launch_agents_dir(Some("/qa/agents".as_ref()), Some(home)), Some(PathBuf::from("/qa/agents")));
            assert_eq!(launch_agents_dir(Some("/qa/agents/x.plist".as_ref()), Some(home)), Some(PathBuf::from("/qa/agents")));
            assert_eq!(launch_agents_dir(Some("".as_ref()), Some(home)), None);
        }

        /// The pre-rebrand `com.rorca.daemon` plist: `RunAtLoad` and an unconditional `KeepAlive`.
        fn rorca_plist(executable: &str) -> String {
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\">\n<dict>\n    <key>Label</key>\n    <string>com.rorca.daemon</string>\n    <key>ProgramArguments</key>\n    <array>\n        <string>{executable}</string>\n        <string>--daemon</string>\n    </array>\n    <key>RunAtLoad</key>\n    <true/>\n    <key>KeepAlive</key>\n    <true/>\n</dict>\n</plist>\n"
            )
        }

        /// Both legacy labels as they were written on disk: the Ferryx single job (rendered by
        /// the same template, label `com.ferryx.daemon`) and the pre-rebrand rorca job.
        fn legacy_plists(dir: &Path) -> Vec<(String, PathBuf, String)> {
            let ferryx = super::super::generate_launchd_plist(
                super::super::PLIST_LABEL,
                "/Applications/Ferryx.app/Contents/MacOS/ferryx",
                &dir.join("logs"),
                true,
            );
            let rorca = rorca_plist("/Applications/Rorca.app/Contents/MacOS/rorca");
            vec![
                (BASE.to_string(), dir.join(format!("{BASE}.plist")), ferryx),
                (
                    super::super::RORCA_PLIST_LABEL.to_string(),
                    dir.join(format!("{}.plist", super::super::RORCA_PLIST_LABEL)),
                    rorca,
                ),
            ]
        }

        /// Todo 14: an Alive or Unknown legacy owner keeps its job loaded and running. The plist
        /// only loses every load-time launch condition by file rewrite, with zero launchctl calls,
        /// and a second run changes nothing and calls nothing.
        #[test]
        fn legacy_job_with_alive_or_unknown_owner_only_loses_login_autostart() {
            for owner in [
                OwnerState::Alive { pid: 7, start_time: 9 },
                OwnerState::Unknown { reason: "lock held, holder unreadable".into() },
            ] {
                let dir = tempfile::tempdir().unwrap();
                for (label, path, content) in legacy_plists(dir.path()) {
                    fs::write(&path, &content).unwrap();
                    let launchctl = FakeLaunchctl::default().running(&label, 4242);
                    assert!(migrate_legacy_job(&launchctl, UID, &label, &path, &owner).unwrap(), "{label}");
                    assert!(launchctl.calls().is_empty(), "{owner:?} {label}: {:?}", launchctl.calls());
                    let disabled = fs::read_to_string(&path).expect("the legacy plist stays in place");
                    assert!(disabled.contains("<key>RunAtLoad</key>\n    <false/>"), "{disabled}");
                    assert!(disabled.contains("<key>KeepAlive</key>\n    <false/>"), "{disabled}");
                    assert!(!disabled.contains("<true/>\n    <key>KeepAlive</key>"), "{disabled}");
                    assert!(!disabled.contains("SuccessfulExit"), "{disabled}");
                    assert!(!disabled.contains("<key>KeepAlive</key>\n    <true/>"), "{disabled}");
                    assert_eq!(plist_program(&disabled), plist_program(&content), "{label}");

                    let again = FakeLaunchctl::default();
                    assert!(!migrate_legacy_job(&again, UID, &label, &path, &owner).unwrap(), "{label}");
                    assert!(again.calls().is_empty(), "the second run issues nothing: {:?}", again.calls());
                    assert_eq!(fs::read_to_string(&path).unwrap(), disabled, "{label}");
                }
            }
        }

        /// Todo 14: an Absent legacy owner's job is removed once (only `launchctl print` runs,
        /// never bootout/unload/kickstart), and the second run issues no call at all.
        #[test]
        fn legacy_job_with_absent_owner_is_removed_once() {
            for idle in [false, true] {
                let dir = tempfile::tempdir().unwrap();
                for (label, path, content) in legacy_plists(dir.path()) {
                    fs::write(&path, &content).unwrap();
                    let launchctl = if idle {
                        FakeLaunchctl::default().idle(&label)
                    } else {
                        FakeLaunchctl::default().not_registered(&label)
                    };
                    assert!(migrate_legacy_job(&launchctl, UID, &label, &path, &OwnerState::Absent).unwrap());
                    assert!(!path.exists(), "{label} must be removed");
                    assert!(launchctl.mutations().is_empty(), "only print may run: {:?}", launchctl.calls());
                    assert_eq!(launchctl.calls().len(), 1, "{:?}", launchctl.calls());

                    let again = FakeLaunchctl::default();
                    assert!(!migrate_legacy_job(&again, UID, &label, &path, &OwnerState::Absent).unwrap());
                    assert!(again.calls().is_empty(), "the second run issues nothing: {:?}", again.calls());
                }
            }
        }

        /// An Absent owner classification with a legacy job that still reports a process (or an
        /// unreadable state) never removes or stops that job; it only loses login autostart.
        #[test]
        fn legacy_job_with_a_process_is_kept_even_for_an_absent_owner() {
            let dir = tempfile::tempdir().unwrap();
            for (label, path, content) in legacy_plists(dir.path()) {
                for running in [true, false] {
                    fs::write(&path, &content).unwrap();
                    let launchctl = if running {
                        FakeLaunchctl::default().running(&label, 77)
                    } else {
                        FakeLaunchctl::default().with_print(&label, false, "", "Operation not permitted")
                    };
                    assert!(migrate_legacy_job(&launchctl, UID, &label, &path, &OwnerState::Absent).unwrap());
                    assert!(path.exists(), "{label} with a process keeps its plist");
                    assert!(launchctl.mutations().is_empty(), "{:?}", launchctl.calls());
                    assert!(fs::read_to_string(&path).unwrap().contains("<key>RunAtLoad</key>\n    <false/>"));
                }
            }
        }
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
