//! Portable facade over the OS service manager that owns the headless daemon's lifetime.
//!
//! The daemon owns every PTY master descriptor, so it must outlive the GUI and come back when it
//! dies abnormally. Callers (the GUI at startup and the daemon client on demand) only talk to this
//! module; each target plugs in its own backend:
//!
//! * macOS - one `launchd` LaunchAgent per daemon runtime generation
//!   ([`crate::daemon::launchd::generation`]), labelled `com.ferryx.daemon.<generation key>` and
//!   running the immutable staged runtime (`daemon::runtime_stage`), never the app bundle.
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

/// Whether the user turned daemon autostart off. It governs login registration only: an opted-out
/// user gets no login autostart on any generation, but [`ensure`] still starts an Absent owner
/// through the OS job (never as a GUI child).
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

/// Autostart opt-out: removes the login launch from every daemon generation's job and from the
/// legacy job, changing only future login registration (macOS `RunAtLoad`/`KeepAlive` false by
/// plist rewrite, Linux `disable` without `--now`, Windows logon trigger and `RestartOnFailure`
/// removal by definition re-import). Nothing is stopped, unloaded, ended or deleted, so a running
/// daemon keeps its PTYs. A missing job is success.
pub fn remove_autostart() -> Result<()> {
    if !supported() {
        return Ok(());
    }
    backend::remove_autostart(&process_env)
}

/// `login_autostart` false (the user opted out) still starts the job, but leaves it without any
/// login launch condition.
fn ensure_in(
    env: Env<'_>,
    dev_runtime: bool,
    executable: &Path,
    login_autostart: bool,
) -> Result<bool> {
    if !supported_in(env, dev_runtime) {
        return Ok(false);
    }
    backend::ensure(env, executable, login_autostart)
}

/// Starts the supervised daemon for an Absent owner. Callers MUST hold owner admission
/// (`owner_admission::admit` returned Start): on macOS this stages the runtime and bootstraps or
/// kickstarts this generation's job, and never touches a running or unverifiable job. Returns
/// `true` when a start was issued, `false` when nothing was started or the runtime is unsupported.
/// A user who turned autostart off ([`user_opted_out`]) still gets the start through the OS job
/// (macOS bootstrap or plain kickstart, Linux `start`, Windows `/Run`), never as a GUI child; the
/// job is left without any login launch condition.
pub fn ensure() -> Result<bool> {
    if !supported() {
        return Ok(false);
    }
    let executable =
        std::env::current_exe().map_err(|e| format!("Failed to get current exe path: {e}"))?;
    ensure_in(
        &process_env,
        crate::daemon::server::is_dev_runtime(),
        &executable,
        !user_opted_out(),
    )
}

/// Reloads the unit so the service manager starts the daemon again (macOS re-runs `RunAtLoad`).
pub fn reload() -> Result<()> {
    if !supported() {
        return Err("No daemon supervisor is supported in this runtime".to_string());
    }
    backend::reload(&process_env)
}

/// GUI setup entry: registers this build for the next login, and ensures/starts the supervised
/// daemon only when no daemon owner exists (owner admission says Absent). An alive or
/// unidentifiable owner is never reloaded, restarted, or stopped. Returns whether a start was
/// issued.
pub fn ensure_if_absent() -> Result<bool> {
    use crate::daemon::owner_admission as admission;

    let paths = admission::AdmissionPaths::for_runtime();
    admission::ensure_if_absent_with(
        &paths,
        &admission::RuntimeProbe { paths: &paths },
        &ProcessSupervisorOps,
        admission::ADMISSION_LOCK_DEADLINE,
        crate::daemon::client::DAEMON_READY_TIMEOUT,
    )
}

/// The real supervisor operations behind [`ensure_if_absent`].
pub(crate) struct ProcessSupervisorOps;

impl crate::daemon::owner_admission::SupervisorOps for ProcessSupervisorOps {
    fn start_absent_owner(&self) -> Result<bool> {
        // QA: a forced-fallback GUI leaves the start to its client's detached fallback.
        #[cfg(feature = "local-split-qa")]
        if crate::daemon::owner_admission::qa_probe_fault::force_fallback() {
            tracing::info!("FERRYX_QA_FORCE_FALLBACK=1: GUI setup issues no supervised start");
            return Ok(false);
        }
        ensure()
    }

    fn migrate_legacy_job(&self, owner: &crate::daemon::owner_admission::OwnerState) -> Result<()> {
        migrate_legacy_job(owner).map(|_| ())
    }

    fn reconcile_login_registration(&self) -> Result<()> {
        reconcile_login_registration().map(|_| ())
    }
}

/// Legacy single-job migration (macOS `com.ferryx.daemon` and the pre-rebrand
/// `com.rorca.daemon`, Linux `ferryx-daemon.service`, Windows `Ferryx\Daemon`), run by
/// [`ensure_if_absent`] under admission.lock with the owner it classified. An Alive or Unknown
/// legacy owner keeps its job loaded and running; the job only loses login autostart. An Absent
/// owner's legacy job is removed. Nothing is ever stopped, booted out, ended or unloaded, and a
/// second run is a no-op. The newest generation is registered for login by
/// [`reconcile_login_registration`]. Runs whether or not the user opted out of autostart, since
/// it only ever removes login autostart. Returns whether anything changed.
pub(crate) fn migrate_legacy_job(owner: &crate::daemon::owner_admission::OwnerState) -> Result<bool> {
    if !supported() {
        return Ok(false);
    }
    backend::migrate_legacy_job(&process_env, owner)
}

/// Registers this build for the next login without starting it or touching any running job
/// (no bootstrap/load, no `--now`, no `/Run`). Returns whether a definition changed. A user who
/// turned autostart off, or an unsupported runtime, is a no-op.
///
/// Each backend registers the current generation's own job (launchd plist, systemd unit,
/// scheduled task) and removes login autostart from older generations without stopping them;
/// the legacy single-name job is left to its separate migration.
pub fn reconcile_login_registration() -> Result<bool> {
    if user_opted_out() || !supported() {
        return Ok(false);
    }
    let executable =
        std::env::current_exe().map_err(|e| format!("Failed to get current exe path: {e}"))?;
    backend::register_for_login(&process_env, &executable)
}

/// Registers the generation this daemon itself runs from for the next login and demotes every
/// older generation, exactly as [`reconcile_login_registration`] does for the GUI's build. A
/// handover successor calls it once it owns the sessions: its job was started with no login
/// autostart (`start_candidate`), and a GUI-free upgrade (control-channel `RequestUpgrade`) has
/// no new GUI that would register it, so without this the next login starts the retired
/// predecessor. A daemon that is not a staged runtime copy (dev build), a user who opted out, or
/// an unsupported runtime is a no-op. Never starts, stops or reloads a job.
pub(crate) fn register_running_generation_for_login() -> Result<bool> {
    if user_opted_out() || !supported() {
        return Ok(false);
    }
    let Some(key) = crate::daemon::handover_request::own_generation_key() else {
        return Ok(false);
    };
    let executable =
        std::env::current_exe().map_err(|e| format!("Failed to get current exe path: {e}"))?;
    backend::register_staged_for_login(
        &process_env,
        &crate::daemon::runtime_stage::StagedRuntime { key, executable },
    )
}

/// HandoverCandidate admission (todo 8): the only start allowed while an owner is Alive. Called
/// only by the control-channel RequestUpgrade handler, after it verified the staged executable and
/// wrote handover-request.json. Takes the admission lock, re-reads the request and starts the
/// successor generation's job only when the request is exactly the caller's own (same nonce,
/// generation key and predecessor identity, predecessor pid == this process). Owner state is
/// ignored on purpose: the owner is the caller. The predecessor's own job is never touched.
#[cfg(unix)]
pub(crate) fn start_handover_candidate(
    request: &crate::daemon::handover_request::HandoverRequest,
) -> Result<crate::daemon::handover_request::CandidateStart> {
    use crate::daemon::handover_request as handover_request;
    if !supported() {
        return Ok(handover_request::CandidateStart::Refused(
            "No daemon supervisor is supported in this runtime".to_string(),
        ));
    }
    let paths = crate::daemon::owner_admission::AdmissionPaths::for_runtime();
    handover_request::start_handover_candidate_with(
        &crate::daemon::server::get_runtime_dir(),
        &paths.admission_lock,
        request,
        std::process::id(),
        &ProcessCandidateStarter,
        crate::daemon::owner_admission::ADMISSION_LOCK_DEADLINE,
    )
}

#[cfg(unix)]
struct ProcessCandidateStarter;

#[cfg(unix)]
impl crate::daemon::handover_request::CandidateStarter for ProcessCandidateStarter {
    fn start_candidate(
        &self,
        request: &crate::daemon::handover_request::HandoverRequest,
    ) -> Result<crate::daemon::handover_request::CandidateStart> {
        backend::start_candidate(&process_env, &request.generation_key, &request.staged_executable)
    }
}

/// Stops and removes the unit; a missing unit is success.
pub fn uninstall() -> Result<()> {
    if !supported() {
        return Err("No daemon supervisor is supported in this runtime".to_string());
    }
    backend::uninstall(&process_env)
}

#[cfg(target_os = "macos")]
mod backend {
    use super::{non_empty, Env, Result, SUPERVISOR_NAME_ENV};
    use crate::daemon::launchd::{self, generation};
    use crate::daemon::owner_admission::OwnerState;
    use crate::daemon::runtime_stage;
    use std::path::{Path, PathBuf};

    pub const TARGET_HAS_BACKEND: bool = true;

    pub fn install_supported() -> bool {
        true
    }

    pub fn available() -> bool {
        true
    }

    /// Label base: `com.ferryx.daemon`, or the QA override. Each generation's label is
    /// `<base>.<generation key>`.
    fn label_base(env: Env<'_>) -> String {
        non_empty(env, SUPERVISOR_NAME_ENV)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| launchd::PLIST_LABEL.to_string())
    }

    fn agents_dir(env: Env<'_>) -> Result<PathBuf> {
        generation::launch_agents_dir(env(launchd::PLIST_PATH_ENV).as_deref(), env("HOME").as_deref())
            .ok_or_else(|| "Cannot determine the LaunchAgents directory".to_string())
    }

    fn uid() -> u32 {
        // SAFETY: `libc::getuid` takes no arguments, dereferences no pointers, and has no side effects.
        unsafe { libc::getuid() }
    }

    /// Stages this build's signed helper bundle as an immutable runtime generation and describes
    /// its job. The job always runs the staged copy, never the GUI executable in the app bundle.
    fn current_job(env: Env<'_>, gui_executable: &Path) -> Result<generation::GenerationJob> {
        let root = crate::daemon::server::daemon_runtime_root()
            .ok_or("Cannot determine the daemon runtime directory")?;
        let staged = runtime_stage::stage_runtime(&root, gui_executable).map_err(|e| e.to_string())?;
        job_for(env, &staged)
    }

    pub(super) fn job_for(
        env: Env<'_>,
        staged: &runtime_stage::StagedRuntime,
    ) -> Result<generation::GenerationJob> {
        let label = generation::generation_label(&label_base(env), &staged.key);
        let plist_path = agents_dir(env)?.join(format!("{label}.plist"));
        let log_dir = launchd::launchd_log_dir_for(env("HOME").as_deref())
            .ok_or("Cannot determine the agent log directory")?;
        Ok(generation::GenerationJob {
            label,
            executable: staged.executable.clone(),
            plist_path,
            log_dir,
            environment: generation::qa_job_environment(env),
        })
    }

    /// Starts this build's generation job. Callers hold owner admission and saw an Absent owner
    /// (`owner_admission::admit` returned Start), so the owner passed to the job-state switch is
    /// Absent; a Running or Unverifiable job is still left alone. Returns whether a start was issued.
    /// With `login_autostart` false the job's plist is left without a load-time launch condition.
    pub fn ensure(env: Env<'_>, executable: &Path, login_autostart: bool) -> Result<bool> {
        let job = current_job(env, executable)?;
        let outcome = generation::ensure_generation_job(
            &generation::SystemLaunchctl,
            uid(),
            &job,
            &OwnerState::Absent,
            login_autostart,
        )?;
        tracing::info!(label = %job.label, ?outcome, "Daemon generation job ensured");
        Ok(matches!(
            outcome,
            generation::EnsureOutcome::Bootstrapped | generation::EnsureOutcome::Kickstarted
        ))
    }

    /// Starts the successor generation's job for a handover (todo 8): NotRegistered -> plist +
    /// `bootstrap`; RegisteredIdle -> plain `kickstart` (no `-k`); Running or Unverifiable ->
    /// refused, so the predecessor keeps serving. The owner (the caller) is deliberately passed as
    /// Absent to the job-state switch; the successor label is never the predecessor's own.
    /// `login_autostart` is false: starting the candidate never registers it for login. Login
    /// registration stays with [`super::reconcile_login_registration`] (newest generation only,
    /// no launchctl), so a candidate that never commits leaves no `RunAtLoad` plist behind.
    pub fn start_candidate(
        env: Env<'_>,
        key: &str,
        executable: &Path,
    ) -> Result<crate::daemon::handover_request::CandidateStart> {
        use crate::daemon::handover_request::CandidateStart;
        let staged = runtime_stage::StagedRuntime {
            key: key.to_string(),
            executable: executable.to_path_buf(),
        };
        let job = job_for(env, &staged)?;
        let outcome = generation::ensure_generation_job(
            &generation::SystemLaunchctl,
            uid(),
            &job,
            &OwnerState::Absent,
            false,
        )?;
        tracing::info!(label = %job.label, ?outcome, "Handover candidate job start");
        Ok(match outcome {
            generation::EnsureOutcome::Bootstrapped | generation::EnsureOutcome::Kickstarted => {
                CandidateStart::Started
            }
            other => CandidateStart::Refused(format!(
                "successor job {} was not started: {other:?}",
                job.label
            )),
        })
    }

    /// Registers this build's generation for the next login (plist with `RunAtLoad` true, no
    /// launchctl), removes login autostart from older generations by plist rewrite, then lets the
    /// runtime garbage collector drop generations no process runs and no job references. The
    /// legacy `com.ferryx.daemon` job is left as it is (its migration is separate).
    pub fn register_for_login(env: Env<'_>, executable: &Path) -> Result<bool> {
        let job = current_job(env, executable)?;
        register_job_for_login(env, &job)
    }

    /// [`register_for_login`] for an already staged generation (the running daemon's own).
    pub fn register_staged_for_login(env: Env<'_>, staged: &runtime_stage::StagedRuntime) -> Result<bool> {
        let job = job_for(env, staged)?;
        register_job_for_login(env, &job)
    }

    fn register_job_for_login(env: Env<'_>, job: &generation::GenerationJob) -> Result<bool> {
        let (changed, dir, base) = login_registration_with(&generation::SystemLaunchctl, uid(), env, job)?;
        collect_unused_runtimes(env, &dir, &base);
        Ok(changed)
    }

    /// The plist half of login registration: `job` gets `RunAtLoad` true, every other generation
    /// in the agents dir loses its load-time launch. No runtime garbage collection.
    pub(super) fn login_registration_with(
        launchctl: &dyn generation::Launchctl,
        uid: u32,
        env: Env<'_>,
        job: &generation::GenerationJob,
    ) -> Result<(bool, PathBuf, String)> {
        let dir = agents_dir(env)?;
        let base = label_base(env);
        let mut changed = generation::register_newest_for_login(job)?;
        changed |= generation::retire_older_generations(launchctl, uid, &dir, &base, &job.label)?;
        Ok((changed, dir, base))
    }

    /// Every executable a registered job references is kept, including the legacy job's. When any
    /// plist cannot be read nothing is collected: a runtime that may be referenced stays.
    fn collect_unused_runtimes(env: Env<'_>, dir: &Path, base: &str) {
        let Some(root) = crate::daemon::server::daemon_runtime_root() else {
            return;
        };
        let mut registered = match generation::job_executables(dir, base) {
            Ok(executables) => executables,
            Err(error) => {
                tracing::warn!(%error, "Cannot read the daemon job plists; keeping every runtime generation");
                return;
            }
        };
        if let Some(legacy) = legacy_plist_path(env) {
            match std::fs::read_to_string(&legacy) {
                Ok(content) => registered.extend(generation::plist_program(&content).map(PathBuf::from)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    tracing::warn!(%error, "Cannot read the legacy daemon plist; keeping every runtime generation");
                    return;
                }
            }
        }
        let usage = runtime_stage::SystemRuntimeUsage {
            registered_job_executables: registered,
        };
        match runtime_stage::collect_garbage(&root, &usage) {
            Ok(removed) if !removed.is_empty() => {
                tracing::info!(?removed, "Removed unused daemon runtime generations")
            }
            Ok(_) => {}
            Err(error) => tracing::warn!(%error, "Daemon runtime garbage collection failed"),
        }
    }

    /// The legacy single-job plist (`com.ferryx.daemon.plist`, or the QA `<base>.plist`).
    fn legacy_plist_path(env: Env<'_>) -> Option<PathBuf> {
        agents_dir(env)
            .ok()
            .map(|dir| dir.join(format!("{}.plist", label_base(env))))
    }

    /// Reloading would unload the job that holds the live daemon; per-generation jobs are only ever
    /// bootstrapped or kickstarted for an Absent owner (`ensure`).
    pub fn reload(_env: Env<'_>) -> Result<()> {
        Err("Reloading a launchd daemon job is not supported; it would stop the live daemon".to_string())
    }

    /// Explicit user uninstall: unloads and removes every generation job and the legacy job.
    pub fn uninstall(env: Env<'_>) -> Result<()> {
        let dir = agents_dir(env)?;
        for (_, path) in generation::generation_plists(&dir, &label_base(env))? {
            launchd::uninstall_launchd_agent_from_path(&path)?;
        }
        match legacy_plist_path(env) {
            Some(legacy) => launchd::uninstall_launchd_agent_from_path(&legacy),
            None => Ok(()),
        }
    }

    pub fn installed(env: Env<'_>) -> bool {
        let Ok(dir) = agents_dir(env) else {
            return false;
        };
        generation::generation_plists(&dir, &label_base(env)).is_ok_and(|plists| !plists.is_empty())
            || legacy_plist_path(env).is_some_and(|path| path.exists())
    }

    /// Sets `RunAtLoad` false on every generation plist and the legacy plist by file rewrite only.
    /// No `launchctl` call: that could stop the running daemon.
    pub fn remove_autostart(env: Env<'_>) -> Result<()> {
        let dir = agents_dir(env)?;
        for (_, path) in generation::generation_plists(&dir, &label_base(env))? {
            generation::disable_login_autostart(&path)?;
        }
        if let Some(legacy) = legacy_plist_path(env) {
            generation::disable_login_autostart(&legacy)?;
        }
        Ok(())
    }

    /// The legacy single jobs: `<base>` (`com.ferryx.daemon`, or the QA base), and, outside QA,
    /// the pre-rebrand `com.rorca.daemon`.
    pub(super) fn legacy_jobs(env: Env<'_>) -> Result<Vec<(String, PathBuf)>> {
        let dir = agents_dir(env)?;
        let mut labels = vec![label_base(env)];
        if non_empty(env, SUPERVISOR_NAME_ENV).is_none() {
            labels.push(launchd::RORCA_PLIST_LABEL.to_string());
        }
        Ok(labels
            .into_iter()
            .map(|label| {
                let path = dir.join(format!("{label}.plist"));
                (label, path)
            })
            .collect())
    }

    /// Migrates every legacy single job by plist rewrite (Alive/Unknown owner) or removal of a job
    /// with no process (Absent owner); only `launchctl print` ever runs. Every job is attempted;
    /// the first error is returned after the rest ran.
    pub fn migrate_legacy_job(env: Env<'_>, owner: &OwnerState) -> Result<bool> {
        let mut changed = false;
        let mut first_error = None;
        for (label, path) in legacy_jobs(env)? {
            match generation::migrate_legacy_job(&generation::SystemLaunchctl, uid(), &label, &path, owner) {
                Ok(job_changed) => changed |= job_changed,
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(changed),
        }
    }
}

#[cfg(windows)]
mod backend {
    use super::{Env, Result};
    use crate::daemon::owner_admission::OwnerState;
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

    pub fn ensure(env: Env<'_>, executable: &Path, login_autostart: bool) -> Result<bool> {
        task::ensure(env, executable, login_autostart)
    }

    /// Registers this build's generation task for logon and demotes older generations, without
    /// `/Run`, `/End` or `/Delete`.
    pub fn register_for_login(env: Env<'_>, executable: &Path) -> Result<bool> {
        task::register_for_login(env, executable)
    }

    pub fn register_staged_for_login(
        env: Env<'_>,
        staged: &crate::daemon::runtime_stage::StagedRuntime,
    ) -> Result<bool> {
        task::register_staged_for_login(env, staged)
    }

    pub fn reload(env: Env<'_>) -> Result<()> {
        task::reload(env)
    }

    /// The legacy `Ferryx\Daemon` task: logon trigger removed for an Alive/Unknown owner,
    /// deleted for an Absent one; never `/End`.
    pub fn migrate_legacy_job(env: Env<'_>, owner: &OwnerState) -> Result<bool> {
        task::migrate_legacy_task(env, *owner == OwnerState::Absent)
    }

    pub fn uninstall(env: Env<'_>) -> Result<()> {
        task::uninstall(env)
    }

    pub fn installed(env: Env<'_>) -> bool {
        task::installed(env)
    }

    /// Strips the logon trigger and `RestartOnFailure` from every generation task and the legacy
    /// task by definition re-import; never `/End` or `/Delete`, so a running watchdog and its
    /// daemon stay up and an opted-out GUI can still `/Run` the newest task.
    pub fn remove_autostart(env: Env<'_>) -> Result<()> {
        task::remove_autostart(env)
    }
}

#[cfg(target_os = "linux")]
mod backend {
    use super::{Env, Result, SUPERVISOR_NAME_ENV};
    use crate::daemon::owner_admission::OwnerState;
    use crate::daemon::{runtime_stage, supervisor_linux};
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

    /// Stages this build's daemon runtime and names its generation unit
    /// `<base>-<generation key>` next to the base unit. The unit runs the staged executable, never
    /// the app install or AppImage path, so replacing the app leaves a running generation intact.
    fn generation_unit(env: Env<'_>, executable: &Path) -> Result<(String, String, PathBuf, PathBuf)> {
        let (base, base_path) = unit(env)?;
        let source = supervisor_linux::daemon_executable(env("APPIMAGE").as_deref(), executable);
        let root = crate::daemon::server::daemon_runtime_root()
            .ok_or("Cannot determine the daemon runtime directory")?;
        let staged = runtime_stage::stage_runtime(&root, &source).map_err(|e| e.to_string())?;
        let name = supervisor_linux::generation_unit_name(&base, &staged.key)?;
        let path = base_path.with_file_name(format!("{name}.service"));
        Ok((base, name, path, staged.executable))
    }

    /// Removes staged generations no process runs from and no unit file in `unit_dir` references.
    fn collect_runtime_garbage(unit_dir: &Path) {
        let Some(root) = crate::daemon::server::daemon_runtime_root() else {
            return;
        };
        let usage = runtime_stage::SystemRuntimeUsage {
            registered_job_executables: supervisor_linux::registered_job_executables(unit_dir),
        };
        if let Err(error) = runtime_stage::collect_garbage(&root, &usage) {
            tracing::warn!(%error, "Could not collect unused daemon runtime generations");
        }
    }

    /// Starts this build's generation unit when it is idle; a busy unit is left alone. The start
    /// never enables the unit, so an opted-out user (`login_autostart` false) gets no login start.
    pub fn ensure(env: Env<'_>, executable: &Path, _login_autostart: bool) -> Result<bool> {
        let (_, name, path, staged) = generation_unit(env, executable)?;
        supervisor_linux::ensure(&name, &path, &staged)
    }

    /// Starts the successor generation's unit for a handover (todo 8): writes the unit if needed,
    /// `daemon-reload` when it changed, `start --no-block`; an already active unit is refused.
    /// The predecessor's own unit is never touched.
    pub fn start_candidate(
        env: Env<'_>,
        key: &str,
        executable: &Path,
    ) -> Result<crate::daemon::handover_request::CandidateStart> {
        use crate::daemon::handover_request::CandidateStart;
        let (base, base_path) = unit(env)?;
        let name = supervisor_linux::generation_unit_name(&base, key)?;
        let path = base_path.with_file_name(format!("{name}.service"));
        Ok(if supervisor_linux::ensure(&name, &path, executable)? {
            CandidateStart::Started
        } else {
            CandidateStart::Refused(format!("successor unit {name} is already active"))
        })
    }

    /// Writes this build's generation unit and enables it for login without `--now`; older
    /// generations are disabled without `--now`. A running unit is untouched. Garbage collection
    /// runs after registration, so the unit just written already protects its runtime.
    pub fn register_for_login(env: Env<'_>, executable: &Path) -> Result<bool> {
        let (base, name, path, staged) = generation_unit(env, executable)?;
        register_unit_for_login(&base, &name, &path, &staged)
    }

    /// [`register_for_login`] for an already staged generation (the running daemon's own).
    pub fn register_staged_for_login(env: Env<'_>, staged: &runtime_stage::StagedRuntime) -> Result<bool> {
        let (base, base_path) = unit(env)?;
        let name = supervisor_linux::generation_unit_name(&base, &staged.key)?;
        let path = base_path.with_file_name(format!("{name}.service"));
        register_unit_for_login(&base, &name, &path, &staged.executable)
    }

    fn register_unit_for_login(base: &str, name: &str, path: &Path, staged: &Path) -> Result<bool> {
        let rewritten = supervisor_linux::register_for_login(base, name, path, staged)?;
        if let Some(unit_dir) = path.parent() {
            collect_runtime_garbage(unit_dir);
        }
        Ok(rewritten)
    }

    /// The legacy single unit `<base>.service`: disabled without `--now` for an Alive/Unknown
    /// owner, removed when idle for an Absent one; never stopped.
    pub fn migrate_legacy_job(env: Env<'_>, owner: &OwnerState) -> Result<bool> {
        let (name, path) = unit(env)?;
        supervisor_linux::migrate_legacy_unit(&name, &path, *owner == OwnerState::Absent)
    }

    pub fn reload(env: Env<'_>) -> Result<()> {
        supervisor_linux::start(&unit(env)?.0)
    }

    pub fn uninstall(env: Env<'_>) -> Result<()> {
        let (name, path) = unit(env)?;
        supervisor_linux::uninstall(&name, &path)
    }

    pub fn installed(env: Env<'_>) -> bool {
        unit(env).is_ok_and(|(base, path)| {
            path.exists()
                || path.parent().is_some_and(|dir| {
                    !supervisor_linux::generation_units_in(dir, &base).is_empty()
                })
        })
    }

    /// Disables every generation unit and the legacy base unit without `--now`; no unit file is
    /// removed and nothing is stopped, so running daemons keep their PTYs and their definitions.
    pub fn remove_autostart(env: Env<'_>) -> Result<()> {
        let (base, path) = unit(env)?;
        let unit_dir = path
            .parent()
            .ok_or("Cannot determine the systemd user unit directory")?;
        supervisor_linux::remove_login_autostart_with(&supervisor_linux::UserSystemctl, unit_dir, &base)
    }
}

/// Other targets never get a backend: callers fall back to the detached spawn.
#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
mod backend {
    use super::{Env, Result};
    use crate::daemon::owner_admission::OwnerState;
    use std::path::Path;

    pub const TARGET_HAS_BACKEND: bool = false;

    pub fn install_supported() -> bool {
        true
    }

    pub fn available() -> bool {
        false
    }

    pub fn ensure(_env: Env<'_>, _executable: &Path, _login_autostart: bool) -> Result<bool> {
        Ok(false)
    }

    #[cfg(unix)]
    pub fn start_candidate(
        _env: Env<'_>,
        _key: &str,
        _executable: &Path,
    ) -> Result<crate::daemon::handover_request::CandidateStart> {
        Ok(crate::daemon::handover_request::CandidateStart::Refused(
            "No daemon supervisor backend on this target".to_string(),
        ))
    }

    pub fn register_for_login(_env: Env<'_>, _executable: &Path) -> Result<bool> {
        Ok(false)
    }

    pub fn register_staged_for_login(
        _env: Env<'_>,
        _staged: &crate::daemon::runtime_stage::StagedRuntime,
    ) -> Result<bool> {
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

    pub fn migrate_legacy_job(_env: Env<'_>, _owner: &OwnerState) -> Result<bool> {
        Ok(false)
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
            ensure_in(&env, false, Path::new("/opt/ferryx/bin/ferryx"), true),
            Ok(false)
        );
        assert_eq!(
            ensure_in(&env, false, Path::new("/opt/ferryx/bin/ferryx"), false),
            Ok(false),
            "an isolated runtime starts nothing, opted out or not"
        );
        assert_eq!(
            std::fs::read_dir(home.path()).unwrap().count(),
            0,
            "an isolated runtime must not create any unit or log file"
        );
    }

    /// The opt-out is a persisted marker. It changes only login registration: the per-backend
    /// tests (`launchd`, `supervisor_linux`, `supervisor_windows`) prove an opted-out start
    /// still goes through the OS job and that opting out never stops a job.
    #[test]
    fn opt_out_marker_persists_and_clears() {
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

    /// The macOS job of a generation runs the staged runtime copy, never the GUI's executable in
    /// the app bundle, under the per-generation label (QA override included).
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_generation_job_runs_the_staged_runtime() {
        let home = tempfile::tempdir().unwrap();
        let staged = crate::daemon::runtime_stage::StagedRuntime {
            key: "2026.1013.2-0a1b2c3d".to_string(),
            executable: home
                .path()
                .join("daemon-runtime/2026.1013.2-0a1b2c3d/Ferryx Daemon.app/Contents/MacOS/ferryx"),
        };
        let env = env_of(&[("HOME", home.path().to_str().unwrap())]);
        let job = backend::job_for(&env, &staged).unwrap();
        assert_eq!(job.label, "com.ferryx.daemon.2026.1013.2-0a1b2c3d");
        assert_eq!(
            job.plist_path,
            home.path().join("Library/LaunchAgents/com.ferryx.daemon.2026.1013.2-0a1b2c3d.plist")
        );
        let rendered = job.render(true);
        assert_eq!(
            crate::daemon::launchd::generation::plist_program(&rendered),
            Some(staged.executable.to_str().unwrap())
        );
        assert!(!rendered.contains("/Applications/"), "the job must not run the app bundle");

        let agents = home.path().join("qa-agents");
        let qa = env_of(&[
            ("HOME", home.path().to_str().unwrap()),
            (SUPERVISOR_NAME_ENV, "ferryx-qa-1"),
            (crate::daemon::launchd::PLIST_PATH_ENV, agents.to_str().unwrap()),
        ]);
        let job = backend::job_for(&qa, &staged).unwrap();
        assert_eq!(job.label, "ferryx-qa-1.2026.1013.2-0a1b2c3d");
        assert_eq!(job.plist_path, agents.join("ferryx-qa-1.2026.1013.2-0a1b2c3d.plist"));
    }

    /// The barrier env reaches the job daemon only through a QA-named generation plist: job_for
    /// renders it into EnvironmentVariables, and the user's job (no QA name) carries none.
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_qa_job_plist_carries_the_barrier_env_to_its_daemon() {
        let home = tempfile::tempdir().unwrap();
        let staged = crate::daemon::runtime_stage::StagedRuntime {
            key: "2026.1013.2-0a1b2c3d".to_string(),
            executable: home.path().join("daemon-runtime/2026.1013.2-0a1b2c3d/Ferryx Daemon.app/Contents/MacOS/ferryx"),
        };
        let home_str = home.path().to_str().unwrap();
        let barrier = [("FERRYX_QA_BARRIER_DIR", "/qa/barrier"), ("FERRYX_QA_RUN_ID", "run-1")];
        let user: Vec<(&str, &str)> = [("HOME", home_str)].into_iter().chain(barrier).collect();
        let plist = backend::job_for(&env_of(&user), &staged).unwrap().render(true);
        assert!(!plist.contains("EnvironmentVariables") && !plist.contains("/qa/barrier"));

        let qa: Vec<(&str, &str)> = [("HOME", home_str), (SUPERVISOR_NAME_ENV, "ferryx-qa-run-1"), ("FERRYX_RUNTIME_DIR", "/qa/run")]
            .into_iter()
            .chain(barrier)
            .collect();
        let plist = backend::job_for(&env_of(&qa), &staged).unwrap().render(true);
        let forwarded = cfg!(feature = "local-split-qa");
        for (key, value) in barrier.iter().chain([&("FERRYX_RUNTIME_DIR", "/qa/run")]) {
            let pair = format!("<key>{key}</key>\n        <string>{value}</string>");
            let expected = forwarded || !key.starts_with("FERRYX_QA_");
            assert_eq!(plist.contains(&pair), expected, "{key} in\n{plist}");
        }
    }

    /// The macOS legacy migration covers `com.ferryx.daemon` and the pre-rebrand
    /// `com.rorca.daemon`; a QA run (name override) migrates only its own QA-named legacy job and
    /// never the user's real ones.
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_legacy_jobs_cover_both_legacy_labels_and_qa_only_its_own() {
        let home = tempfile::tempdir().unwrap();
        let agents = home.path().join("Library/LaunchAgents");
        let env = env_of(&[("HOME", home.path().to_str().unwrap())]);
        assert_eq!(
            backend::legacy_jobs(&env).unwrap(),
            vec![
                ("com.ferryx.daemon".to_string(), agents.join("com.ferryx.daemon.plist")),
                ("com.rorca.daemon".to_string(), agents.join("com.rorca.daemon.plist")),
            ]
        );
        let qa_agents = home.path().join("qa-agents");
        let qa = env_of(&[
            ("HOME", home.path().to_str().unwrap()),
            (SUPERVISOR_NAME_ENV, "ferryx-qa-1"),
            (crate::daemon::launchd::PLIST_PATH_ENV, qa_agents.to_str().unwrap()),
        ]);
        assert_eq!(
            backend::legacy_jobs(&qa).unwrap(),
            vec![("ferryx-qa-1".to_string(), qa_agents.join("ferryx-qa-1.plist"))]
        );
    }

    /// A handover successor registers its own staged generation for login. Its job was started
    /// with no load-time launch and the predecessor's plist still has `RunAtLoad` true; afterwards
    /// the successor autostarts at login and the predecessor does not, while the predecessor's
    /// plist is kept because its daemon is still running (draining). Only `print` runs.
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_successor_registers_its_staged_generation_and_demotes_the_predecessor() {
        use crate::daemon::launchd::generation::{Launchctl, LaunchctlOutput};
        struct Printer(std::cell::RefCell<Vec<Vec<String>>>);
        impl Launchctl for Printer {
            fn run(&self, args: &[&str]) -> std::io::Result<LaunchctlOutput> {
                self.0.borrow_mut().push(args.iter().map(|a| a.to_string()).collect());
                Ok(LaunchctlOutput {
                    success: true,
                    stdout: "state = running\n\tpid = 4242\n".to_string(),
                    stderr: String::new(),
                })
            }
        }
        let home = tempfile::tempdir().unwrap();
        let env = env_of(&[("HOME", home.path().to_str().unwrap())]);
        let staged = |key: &str| crate::daemon::runtime_stage::StagedRuntime {
            key: key.to_string(),
            executable: home
                .path()
                .join(format!("daemon-runtime/{key}/Ferryx Daemon.app/Contents/MacOS/ferryx")),
        };
        let old = backend::job_for(&env, &staged("2026.1023.3-8f33b601")).unwrap();
        let new = backend::job_for(&env, &staged("2026.1024.1-f83e7ee2")).unwrap();
        std::fs::create_dir_all(old.plist_path.parent().unwrap()).unwrap();
        std::fs::write(&old.plist_path, old.render(true)).unwrap();
        std::fs::write(&new.plist_path, new.render(false)).unwrap();
        let run_at_load = |path: &std::path::Path| {
            let content = std::fs::read_to_string(path).unwrap();
            content.contains("<key>RunAtLoad</key>\n    <true/>")
        };
        assert!(run_at_load(&old.plist_path) && !run_at_load(&new.plist_path));

        let launchctl = Printer(Default::default());
        let (changed, _, _) = backend::login_registration_with(&launchctl, 501, &env, &new).unwrap();
        assert!(changed);
        assert!(run_at_load(&new.plist_path), "the successor must autostart at the next login");
        assert!(old.plist_path.exists(), "a running predecessor keeps its plist");
        assert!(!run_at_load(&old.plist_path), "the predecessor must lose login autostart");
        assert!(
            launchctl.0.borrow().iter().all(|call| call[0] == "print"),
            "registration never starts, stops or reloads a job"
        );
        let (again, _, _) = backend::login_registration_with(&launchctl, 501, &env, &new).unwrap();
        assert!(!again, "a second registration changes nothing");
    }
}
