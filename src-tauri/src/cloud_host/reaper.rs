//! The scheduler entry that runs `ferryx-cli cloud reconcile` every five minutes.
//!
//! Provider-native auto-stop is the first layer of shutdown, but not every provider has one and a
//! provider TTL only covers the window the provider knows about. This is the second layer: a
//! user-level scheduler job, installed by the first `cloud create`, that sweeps the catalog whether
//! or not the operator ever runs a ferryx command again. It is removed as soon as no catalog row is
//! left in a state other than `deleted`.
//!
//! One job, one label, one interval, on three platforms:
//!
//! | platform | mechanism | artifact |
//! | --- | --- | --- |
//! | macOS | `launchd` LaunchAgent, `launchctl bootstrap gui/<uid>` | `~/Library/LaunchAgents/dev.ferryx.cloud-reaper.plist` |
//! | Linux | `systemd --user` service + timer, 300 s | `~/.config/systemd/user/dev.ferryx.cloud-reaper.{service,timer}` |
//! | Windows | Task Scheduler, every 5 minutes | the task `dev.ferryx.cloud-reaper` |
//!
//! Every platform answers the same three questions - is it loaded, install it, remove it - so the
//! lifecycle never branches on the target. Any other target gets a typed
//! [`ReaperError::Unsupported`] instead of a silent no-op: a `cloud create` on a platform with no
//! scheduler must be able to refuse a provider that has no native auto-stop, and it can only do
//! that if the absence is an error.
//!
//! Nothing here starts a long-running process: the job is `oneshot`/`StartInterval`-shaped, and the
//! reconcile it runs exits when the sweep is done.

use std::ffi::OsString;
use std::path::PathBuf;

/// The job's identity on every platform. Also the launchd `Label` and the systemd unit name.
pub const REAPER_LABEL: &str = "dev.ferryx.cloud-reaper";
/// How often the job runs, in seconds. Five minutes is the plan's window.
pub const REAPER_INTERVAL_SECONDS: u64 = 300;
/// The argv the job runs. `cloud reconcile` is the hidden CLI verb wired in the CLI todo.
pub const RECONCILE_ARGS: [&str; 2] = ["cloud", "reconcile"];
/// Overrides the resolved `ferryx-cli` path (QA, and an install whose CLI is not on `PATH`).
pub const EXECUTABLE_ENV: &str = "FERRYX_CLI_PATH";
/// Overrides the plist path on macOS, so a fixture run never touches the operator's LaunchAgents.
pub const PLIST_PATH_ENV: &str = "FERRYX_CLOUD_REAPER_PLIST_PATH";
/// Overrides the home directory the job's files are resolved under.
pub const HOME_ENV: &str = "FERRYX_CLOUD_REAPER_HOME";

/// A failure installing, querying or removing the scheduler entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReaperError {
    /// This build's target has no scheduler integration; the caller must supply its own shutdown
    /// guarantee (a provider with native auto-stop) or refuse the request.
    Unsupported { platform: &'static str, detail: String },
    /// The platform tool ran and refused, or could not be spawned.
    Failed { action: &'static str, message: String },
}

impl ReaperError {
    pub fn is_unsupported(&self) -> bool {
        matches!(self, ReaperError::Unsupported { .. })
    }

    /// A tool or spawn failure.
    pub fn failed(action: &'static str, message: impl Into<String>) -> Self {
        ReaperError::Failed { action, message: message.into() }
    }
}

impl std::fmt::Display for ReaperError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReaperError::Unsupported { platform, detail } => {
                write!(f, "the cloud reaper has no scheduler integration on {platform}: {detail}")
            }
            ReaperError::Failed { action, message } => write!(f, "cloud reaper {action} failed: {message}"),
        }
    }
}

impl std::error::Error for ReaperError {}

/// Where the job's files live and what it runs. A value, so a test never touches the real home.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReaperConfig {
    /// Absolute path of the `ferryx-cli` binary the job runs.
    pub executable: PathBuf,
    /// `$HOME` the artifacts are resolved under (`None` = resolve from the environment).
    pub home: Option<PathBuf>,
    /// Explicit launchd plist path; `None` = the standard LaunchAgents location.
    pub plist_path: Option<PathBuf>,
}

impl ReaperConfig {
    /// Resolve the executable and home from the environment, with the environment overrides applied.
    pub fn from_env() -> Result<Self, ReaperError> {
        let home = match std::env::var_os(HOME_ENV) {
            Some(value) if !value.is_empty() => Some(PathBuf::from(value)),
            _ => std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(PathBuf::from),
        };
        let executable = match std::env::var_os(EXECUTABLE_ENV) {
            Some(value) if !value.is_empty() => PathBuf::from(value),
            _ => default_executable(home.as_deref())?,
        };
        let plist_path = std::env::var_os(PLIST_PATH_ENV)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
        Ok(Self { executable, home, plist_path })
    }

    /// An explicit configuration, for a fixture or an acceptance run.
    pub fn new(executable: impl Into<PathBuf>, home: impl Into<PathBuf>) -> Self {
        Self { executable: executable.into(), home: Some(home.into()), plist_path: None }
    }

    /// The home this configuration resolved, or an error naming the missing variable.
    fn require_home(&self) -> Result<PathBuf, ReaperError> {
        self.home.clone().ok_or_else(|| {
            ReaperError::failed("resolve", "cannot determine the home directory (HOME is not set)")
        })
    }
}

/// `ferryx-cli` next to the running binary, else the bare name for `PATH` lookup.
fn default_executable(home: Option<&std::path::Path>) -> Result<PathBuf, ReaperError> {
    if let Ok(current) = std::env::current_exe() {
        if let Some(directory) = current.parent() {
            for candidate in ["ferryx-cli", "ferryx-cli.exe"] {
                let path = directory.join(candidate);
                if path.is_file() {
                    return Ok(path);
                }
            }
        }
    }
    let _ = home;
    // The bare name keeps the job working when the CLI is installed on `PATH` later; the caller
    // that needs certainty passes `FERRYX_CLI_PATH`.
    Ok(PathBuf::from(if cfg!(windows) { "ferryx-cli.exe" } else { "ferryx-cli" }))
}

/// The three questions the lifecycle asks the scheduler, on whichever platform this build targets.
#[async_trait::async_trait]
pub trait ReaperScheduler: Send + Sync {
    /// `"macos"`, `"linux"`, `"windows"` or the target's own name when nothing is supported.
    fn platform(&self) -> &'static str;

    /// Whether the job is registered with the platform scheduler right now.
    async fn is_loaded(&self) -> Result<bool, ReaperError>;

    /// Register the job. Idempotent: an already-loaded job is a success, not a conflict.
    async fn install(&self) -> Result<(), ReaperError>;

    /// Remove the job and its artifact. Idempotent: an absent job is a success.
    async fn remove(&self) -> Result<(), ReaperError>;
}

/// The scheduler this build targets. Delegates to the platform module chosen at compile time.
#[derive(Debug, Clone)]
pub struct HostReaper {
    config: ReaperConfig,
}

impl HostReaper {
    pub fn new(config: ReaperConfig) -> Self {
        Self { config }
    }

    /// Resolve the configuration from the environment and build the host scheduler.
    pub fn from_env() -> Result<Self, ReaperError> {
        Ok(Self::new(ReaperConfig::from_env()?))
    }

    pub fn config(&self) -> &ReaperConfig {
        &self.config
    }
}

#[async_trait::async_trait]
impl ReaperScheduler for HostReaper {
    fn platform(&self) -> &'static str {
        backend::platform()
    }

    async fn is_loaded(&self) -> Result<bool, ReaperError> {
        let config = self.config.clone();
        off_runtime("query", move || backend::is_loaded(&config)).await
    }

    async fn install(&self) -> Result<(), ReaperError> {
        let config = self.config.clone();
        off_runtime("install", move || backend::install(&config)).await
    }

    async fn remove(&self) -> Result<(), ReaperError> {
        let config = self.config.clone();
        off_runtime("remove", move || backend::remove(&config)).await
    }
}

/// Run one scheduler operation on a blocking thread.
///
/// `crate::ipc::run_blocking` is typed to `IpcError`, so the scheduler's own error travels as the
/// success value and only a failed join (a panic in the worker) becomes a [`ReaperError`].
async fn off_runtime<T, F>(action: &'static str, operation: F) -> Result<T, ReaperError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, ReaperError> + Send + 'static,
{
    crate::ipc::run_blocking(move || Ok::<_, crate::ipc::IpcError>(operation()))
        .await
        .map_err(|error| ReaperError::failed(action, error.message))?
}

#[cfg(target_os = "macos")]
use launchd as backend;
#[cfg(target_os = "linux")]
use systemd as backend;
#[cfg(target_os = "windows")]
use schtasks as backend;
#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
use unsupported as backend;

/// Run a platform tool and return its exit status with both streams captured.
fn run_tool(program: &str, args: &[OsString]) -> Result<ToolOutput, ReaperError> {
    use std::process::Stdio;
    let output = std::process::Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                ReaperError::failed("run", format!("{program} is not installed"))
            } else {
                ReaperError::failed("run", format!("cannot run {program}: {error}"))
            }
        })?;
    Ok(ToolOutput {
        success: output.status.success(),
        #[cfg(target_os = "macos")]
        code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

struct ToolOutput {
    success: bool,
    #[cfg(target_os = "macos")]
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

impl ToolOutput {
    fn combined(&self) -> String {
        format!("{}\n{}", self.stdout, self.stderr)
    }
}

// ---------------------------------------------------------------------------------------------
// macOS: launchd LaunchAgent
// ---------------------------------------------------------------------------------------------

#[cfg(target_os = "macos")]
mod launchd {
    use super::*;

    fn uid() -> u32 {
        // SAFETY: `getuid` is a pure libc lookup with no preconditions.
        unsafe { libc::getuid() }
    }

    fn plist_path(config: &ReaperConfig) -> Result<PathBuf, ReaperError> {
        if let Some(path) = &config.plist_path {
            return Ok(path.clone());
        }
        Ok(config
            .require_home()?
            .join("Library/LaunchAgents")
            .join(format!("{REAPER_LABEL}.plist")))
    }

    fn domain() -> String {
        format!("gui/{}", uid())
    }

    fn service_target() -> String {
        format!("{}/{REAPER_LABEL}", domain())
    }

    /// The job's stdout/stderr; a launchd job with no log path loses the reconcile output.
    fn log_dir(config: &ReaperConfig) -> Result<PathBuf, ReaperError> {
        Ok(config.require_home()?.join(".ferryx/logs"))
    }

    /// Render the LaunchAgent. Pure, so the interval and argv are assertable without launchd.
    pub(super) fn render_plist(config: &ReaperConfig) -> Result<String, ReaperError> {
        let logs = log_dir(config)?;
        let stdout_path = logs.join("cloud-reaper.stdout.log");
        let stderr_path = logs.join("cloud-reaper.stderr.log");
        Ok(format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{REAPER_LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
        <string>{}</string>
        <string>{}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>StartInterval</key>
    <integer>{REAPER_INTERVAL_SECONDS}</integer>
    <key>ProcessType</key>
    <string>Background</string>
    <key>LowPriorityIO</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{}</string>
    <key>StandardErrorPath</key>
    <string>{}</string>
</dict>
</plist>
"#,
            config.executable.display(),
            RECONCILE_ARGS[0],
            RECONCILE_ARGS[1],
            stdout_path.display(),
            stderr_path.display()
        ))
    }

    pub(super) fn platform() -> &'static str {
        "macos"
    }

    pub(super) fn is_loaded(_config: &ReaperConfig) -> Result<bool, ReaperError> {
        let output = run_tool("launchctl", &[OsString::from("print"), OsString::from(service_target())])?;
        if output.success {
            return Ok(true);
        }
        // `launchctl print` answers "Could not find service" (113) for an unregistered job; that is
        // the only non-zero answer that means "absent" rather than "launchd is unhappy".
        if output.code == Some(113) || output.combined().contains("Could not find service") {
            return Ok(false);
        }
        Err(ReaperError::failed(
            "query",
            format!("launchctl print {} exited {:?}: {}", service_target(), output.code, output.combined().trim()),
        ))
    }

    pub(super) fn install(config: &ReaperConfig) -> Result<(), ReaperError> {
        if is_loaded(config)? {
            return Ok(());
        }
        let path = plist_path(config)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                ReaperError::failed("install", format!("cannot create {}: {error}", parent.display()))
            })?;
        }
        std::fs::create_dir_all(log_dir(config)?).map_err(|error| {
            ReaperError::failed("install", format!("cannot create the reaper log directory: {error}"))
        })?;
        std::fs::write(&path, render_plist(config)?).map_err(|error| {
            ReaperError::failed("install", format!("cannot write {}: {error}", path.display()))
        })?;

        let output = run_tool(
            "launchctl",
            &[OsString::from("bootstrap"), OsString::from(domain()), path.clone().into_os_string()],
        )?;
        if output.success {
            return Ok(());
        }
        // Another process may have bootstrapped the same job between the query and the write.
        if output.combined().contains("already bootstrapped") || output.combined().contains("Bootstrap failed: 5") {
            return if is_loaded(config)? {
                Ok(())
            } else {
                Err(ReaperError::failed("install", output.combined().trim().to_string()))
            };
        }
        Err(ReaperError::failed(
            "install",
            format!("launchctl bootstrap {} exited {:?}: {}", domain(), output.code, output.combined().trim()),
        ))
    }

    pub(super) fn remove(config: &ReaperConfig) -> Result<(), ReaperError> {
        let path = plist_path(config)?;
        let output = run_tool("launchctl", &[OsString::from("bootout"), OsString::from(service_target())])?;
        let absent = output.code == Some(113)
            || output.combined().contains("Could not find service")
            || output.combined().contains("No such process");
        if !output.success && !absent {
            return Err(ReaperError::failed(
                "remove",
                format!("launchctl bootout {} exited {:?}: {}", service_target(), output.code, output.combined().trim()),
            ));
        }
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(ReaperError::failed("remove", format!("cannot remove {}: {error}", path.display()))),
        }
    }

    #[cfg(test)]
    pub(super) fn plist_location(config: &ReaperConfig) -> PathBuf {
        plist_path(config).unwrap()
    }
}

// ---------------------------------------------------------------------------------------------
// Linux: systemd --user service + timer
// ---------------------------------------------------------------------------------------------

#[cfg(target_os = "linux")]
mod systemd {
    use super::*;

    fn unit_dir(config: &ReaperConfig) -> Result<PathBuf, ReaperError> {
        Ok(config.require_home()?.join(".config/systemd/user"))
    }

    fn timer_unit() -> String {
        format!("{REAPER_LABEL}.timer")
    }

    fn service_unit() -> String {
        format!("{REAPER_LABEL}.service")
    }

    /// A `Type=oneshot` service: reconcile exits when the sweep is done, so nothing lingers.
    pub(super) fn render_service(config: &ReaperConfig) -> String {
        format!(
            "[Unit]\nDescription=Ferryx cloud host reaper\n\n[Service]\nType=oneshot\nExecStart={} {}\n",
            config.executable.display(),
            RECONCILE_ARGS.join(" ")
        )
    }

    /// `OnUnitActiveSec` keeps the five-minute cadence; the first run happens at install time.
    pub(super) fn render_timer() -> String {
        format!(
            "[Unit]\nDescription=Ferryx cloud host reaper timer\n\n[Timer]\nOnActiveSec={REAPER_INTERVAL_SECONDS}\nOnUnitActiveSec={REAPER_INTERVAL_SECONDS}\nAccuracySec=30s\nUnit={}\n\n[Install]\nWantedBy=timers.target\n",
            service_unit()
        )
    }

    pub(super) fn platform() -> &'static str {
        "linux"
    }

    pub(super) fn is_loaded(_config: &ReaperConfig) -> Result<bool, ReaperError> {
        let output = run_tool(
            "systemctl",
            &[
                OsString::from("--user"),
                OsString::from("is-active"),
                OsString::from("--quiet"),
                OsString::from(timer_unit()),
            ],
        )?;
        Ok(output.success)
    }

    pub(super) fn install(config: &ReaperConfig) -> Result<(), ReaperError> {
        let dir = unit_dir(config)?;
        std::fs::create_dir_all(&dir).map_err(|error| {
            ReaperError::failed("install", format!("cannot create {}: {error}", dir.display()))
        })?;
        for (name, body) in [(service_unit(), render_service(config)), (timer_unit(), render_timer())] {
            let path = dir.join(&name);
            std::fs::write(&path, body).map_err(|error| {
                ReaperError::failed("install", format!("cannot write {}: {error}", path.display()))
            })?;
        }
        let reload = run_tool("systemctl", &[OsString::from("--user"), OsString::from("daemon-reload")])?;
        if !reload.success {
            return Err(ReaperError::failed("install", format!("systemctl --user daemon-reload: {}", reload.combined().trim())));
        }
        let enable = run_tool(
            "systemctl",
            &[
                OsString::from("--user"),
                OsString::from("enable"),
                OsString::from("--now"),
                OsString::from(timer_unit()),
            ],
        )?;
        if !enable.success {
            return Err(ReaperError::failed("install", format!("systemctl --user enable: {}", enable.combined().trim())));
        }
        Ok(())
    }

    pub(super) fn remove(config: &ReaperConfig) -> Result<(), ReaperError> {
        let _ = run_tool(
            "systemctl",
            &[
                OsString::from("--user"),
                OsString::from("disable"),
                OsString::from("--now"),
                OsString::from(timer_unit()),
            ],
        )?;
        let dir = unit_dir(config)?;
        for name in [timer_unit(), service_unit()] {
            match std::fs::remove_file(dir.join(&name)) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(ReaperError::failed("remove", format!("cannot remove {name}: {error}")))
                }
            }
        }
        let _ = run_tool("systemctl", &[OsString::from("--user"), OsString::from("daemon-reload")])?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// Windows: Task Scheduler
// ---------------------------------------------------------------------------------------------

#[cfg(target_os = "windows")]
mod schtasks {
    use super::*;

    pub(super) fn platform() -> &'static str {
        "windows"
    }

    pub(super) fn is_loaded(_config: &ReaperConfig) -> Result<bool, ReaperError> {
        let output = run_tool(
            "schtasks",
            &[OsString::from("/Query"), OsString::from("/TN"), OsString::from(REAPER_LABEL)],
        )?;
        Ok(output.success)
    }

    pub(super) fn install(config: &ReaperConfig) -> Result<(), ReaperError> {
        let task_run = format!("{} {}", config.executable.display(), RECONCILE_ARGS.join(" "));
        let output = run_tool(
            "schtasks",
            &[
                OsString::from("/Create"),
                OsString::from("/SC"),
                OsString::from("MINUTE"),
                OsString::from("/MO"),
                OsString::from("5"),
                OsString::from("/TN"),
                OsString::from(REAPER_LABEL),
                OsString::from("/TR"),
                OsString::from(task_run),
                OsString::from("/F"),
            ],
        )?;
        if output.success {
            Ok(())
        } else {
            Err(ReaperError::failed("install", format!("schtasks /Create: {}", output.combined().trim())))
        }
    }

    pub(super) fn remove(_config: &ReaperConfig) -> Result<(), ReaperError> {
        let output = run_tool(
            "schtasks",
            &[
                OsString::from("/Delete"),
                OsString::from("/TN"),
                OsString::from(REAPER_LABEL),
                OsString::from("/F"),
            ],
        )?;
        if output.success || output.combined().contains("cannot find") {
            Ok(())
        } else {
            Err(ReaperError::failed("remove", format!("schtasks /Delete: {}", output.combined().trim())))
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Everything else: a typed refusal, never a silent no-op
// ---------------------------------------------------------------------------------------------

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
mod unsupported {
    use super::*;

    pub(super) fn platform() -> &'static str {
        std::env::consts::OS
    }

    fn refuse<T>() -> Result<T, ReaperError> {
        Err(ReaperError::Unsupported {
            platform: std::env::consts::OS,
            detail: "no launchd, systemd --user or Task Scheduler integration is compiled for this target"
                .to_string(),
        })
    }

    pub(super) fn is_loaded(_config: &ReaperConfig) -> Result<bool, ReaperError> {
        refuse()
    }

    pub(super) fn install(_config: &ReaperConfig) -> Result<(), ReaperError> {
        refuse()
    }

    pub(super) fn remove(_config: &ReaperConfig) -> Result<(), ReaperError> {
        refuse()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> ReaperConfig {
        ReaperConfig::new("/opt/ferryx/bin/ferryx-cli", "/Users/someone")
    }

    #[test]
    fn the_interval_is_five_minutes_on_every_platform() {
        assert_eq!(REAPER_INTERVAL_SECONDS, 300);
        assert_eq!(RECONCILE_ARGS, ["cloud", "reconcile"]);
    }

    #[test]
    fn the_label_is_the_one_the_plan_names() {
        assert_eq!(REAPER_LABEL, "dev.ferryx.cloud-reaper");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_launch_agent_carries_the_interval_the_argv_and_a_log_path() {
        let plist = launchd::render_plist(&config()).expect("render");
        assert!(plist.contains("<key>Label</key>\n    <string>dev.ferryx.cloud-reaper</string>"), "{plist}");
        assert!(plist.contains("<key>StartInterval</key>\n    <integer>300</integer>"), "{plist}");
        assert!(plist.contains("<string>/opt/ferryx/bin/ferryx-cli</string>"), "{plist}");
        assert!(plist.contains("<string>cloud</string>"), "{plist}");
        assert!(plist.contains("<string>reconcile</string>"), "{plist}");
        assert!(plist.contains("cloud-reaper.stderr.log"), "{plist}");
        // A shell string here would let a path with a space become two arguments.
        assert!(!plist.contains("ferryx-cli cloud reconcile"), "{plist}");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_launch_agent_lands_in_launch_agents_unless_overridden() {
        assert_eq!(
            launchd::plist_location(&config()),
            PathBuf::from("/Users/someone/Library/LaunchAgents/dev.ferryx.cloud-reaper.plist")
        );
        let mut override_config = config();
        override_config.plist_path = Some(PathBuf::from("/tmp/qa/dev.ferryx.cloud-reaper.plist"));
        assert_eq!(
            launchd::plist_location(&override_config),
            PathBuf::from("/tmp/qa/dev.ferryx.cloud-reaper.plist")
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_systemd_units_carry_the_interval_and_the_argv() {
        assert!(systemd::render_timer().contains("OnUnitActiveSec=300"));
        assert!(systemd::render_service(&config()).contains("/opt/ferryx/bin/ferryx-cli cloud reconcile"));
    }

    #[test]
    fn an_unsupported_target_is_a_typed_error_not_a_no_op() {
        let error = ReaperError::Unsupported { platform: "plan9", detail: "none".to_string() };
        assert!(error.is_unsupported());
        assert!(error.to_string().contains("no scheduler integration on plan9"));
        assert!(!ReaperError::failed("install", "boom").is_unsupported());
    }

    #[test]
    fn a_missing_executable_override_falls_back_to_the_environment_or_path() {
        // `from_env` must never fail only because `FERRYX_CLI_PATH` is unset: the sibling lookup or
        // the bare name covers a normal install.
        let executable = default_executable(Some(std::path::Path::new("/Users/someone")))
            .expect("a default executable is always resolvable");
        assert!(!executable.as_os_str().is_empty());
    }

    #[test]
    fn require_home_names_the_missing_variable() {
        let mut without_home = config();
        without_home.home = None;
        let error = without_home.require_home().expect_err("no home");
        assert_eq!(error.to_string(), "cloud reaper resolve failed: cannot determine the home directory (HOME is not set)");
    }
}
