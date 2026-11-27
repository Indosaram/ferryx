//! Production keeper startup: install this build's generation from the app bundle, apply retention,
//! and report whether a keeper is live.
//!
//! This is the only place that calls `keeper_launchd::ensure_keeper_service`, and it does so on every
//! daemon start. It also owns the one narrow `launchctl` interaction in the keeper path: a
//! first-install activation that bootstraps (or kickstarts) *this* label, and only after the plist
//! on disk provably programs this generation's binary. It never runs `bootout`, never passes `-k`,
//! and never disturbs a job launchd reports as running. A dev or isolated runtime never reaches
//! `launchctl` at all, not even when the probe finds no keeper. The presence of the keeper is
//! decided by probing its socket, never by assuming the install or the activation worked.
//!
//! The whole pass runs under [`StartupLock`]: occupancy is decided and then acted on inside one
//! exclusive section, so no concurrent install can make an "empty tree" answer stale before the
//! prune it authorizes.
//!
//! A runtime that may not manage the user's keeper (`is_dev_runtime`, an isolated
//! `FERRYX_RUNTIME_DIR`/`FERRYX_DATA_DIR`/`FERRYX_SESSION_DIR`) is refused by
//! `keeper_launchd::keeper_service_for` itself, so an isolated runtime can never repoint the
//! production job. A bundle with no keeper payload is a typed refusal, not a silent no-op: shipping
//! no keeper means supervised restart must stay off, and that decision is reported to the caller.

use std::path::{Path, PathBuf};

use crate::daemon::keeper_launchd::{self, KeeperGeneration, KeeperServiceError};
use crate::daemon::launchd::generation;

use super::keeper_session::{
    KeeperSessionError, KeeperSessionHost, KeeperSessionView, StartupDiscovery,
};

/// The keeper payload's place inside a macOS app bundle, as `scripts/lib/keeper-bundle.mjs` plants
/// it: `Contents/Resources/keeper/{ferryx-ptykeeper,keeper.json}`.
pub const BUNDLE_KEEPER_DIR: &str = "keeper";

/// Why the startup pass could not install or discover the keeper.
#[derive(Debug)]
pub enum KeeperStartupError {
    /// This build's bundle carries no keeper payload, so no generation could be installed.
    PayloadAbsent { checked: PathBuf },
    /// The bundle payload is present but its manifest disagrees with the binary.
    PayloadUnusable { detail: String },
    /// The keeper's own registry could not be read, so which generations still own sessions is
    /// unknown. A pass that does not know occupancy must not prune or repoint anything.
    OccupancyUnknown { detail: String },
    /// The keeper answered but its sessions could not be reconciled into the durable table.
    Discovery { detail: String },
    /// The durable table could not be written while this pass was pinning the ownership flag.
    OwnershipWriteRefused { detail: String },
    /// The keeper-ownership override named a mode this build does not implement.
    OwnershipPolicyUnknown { detail: String },
    /// `keeper_launchd` refused the install, prune or plist write.
    Service(KeeperServiceError),
}

impl std::fmt::Display for KeeperStartupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PayloadAbsent { checked } => write!(
                f,
                "KEEPER_PAYLOAD_ABSENT: no keeper payload at {} so supervised restart stays off",
                checked.display()
            ),
            Self::PayloadUnusable { detail } => {
                write!(f, "KEEPER_PAYLOAD_UNUSABLE: {detail}")
            }
            Self::OccupancyUnknown { detail } => write!(
                f,
                "KEEPER_OCCUPANCY_UNKNOWN: {detail}; nothing was installed, pruned or repointed, \
                 because a generation with unknown occupancy may still be serving a session"
            ),
            Self::Discovery { detail } => write!(
                f,
                "KEEPER_DISCOVERY_FAILED: {detail}; the durable table was left exactly as it was"
            ),
            Self::OwnershipWriteRefused { detail } => write!(
                f,
                "KEEPER_OWNERSHIP_WRITE_REFUSED: {detail}; the persisted ownership flag was left \
                 exactly as it was, so no session was moved between owners"
            ),
            Self::OwnershipPolicyUnknown { detail } => write!(
                f,
                "KEEPER_OWNERSHIP_POLICY_UNKNOWN: {detail}; the flag was not changed, because an \
                 unrecognised value must never be read as either enable or disable"
            ),
            Self::Service(error) => write!(f, "KEEPER_SERVICE_REFUSED: {error}"),
        }
    }
}

impl std::error::Error for KeeperStartupError {}

impl From<KeeperServiceError> for KeeperStartupError {
    fn from(error: KeeperServiceError) -> Self {
        Self::Service(error)
    }
}

/// What the activation step did to the keeper job.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum KeeperActivation {
    /// The pass never reached activation.
    #[default]
    NotAttempted,
    /// A keeper already answered this runtime's socket: no `launchctl` call was made.
    AlreadyLive,
    /// The job was not registered, so it was bootstrapped from this runtime's plist.
    Bootstrapped,
    /// The job was registered and idle, so it was kickstarted without `-k`.
    Kickstarted,
    /// launchd reports a state this daemon will not disturb.
    LeftAlone { state: String },
    /// This runtime may not manage a launchd job at all, so no start was attempted.
    Skipped { reason: String },
    /// The plist does not program this generation's binary, so nothing was started.
    PlistMismatch {
        expected: PathBuf,
        found: Option<PathBuf>,
    },
    /// The activation itself failed. The install and the runtime still stand; only the start did not.
    Failed { detail: String },
}

impl KeeperActivation {
    /// Whether this pass issued a start, as opposed to finding the job already healthy.
    pub fn start_issued(&self) -> bool {
        matches!(self, Self::Bootstrapped | Self::Kickstarted)
    }
}

/// What one startup pass did, for the daemon log and for the readiness report.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeeperStartupReport {
    /// The generation the job now names, when a payload was installed or was already present.
    pub generation: Option<String>,
    /// Whether this pass planted new bytes (false when the generation already existed).
    pub installed: bool,
    pub plist_rewritten: bool,
    pub pruned: Vec<String>,
    pub preserved_occupied: Vec<String>,
    /// What the activation step did to the job, so a log reader can tell "already serving" from
    /// "this pass started it" from "it was left alone".
    pub activation: KeeperActivation,
    /// Whether a keeper answered on this runtime's socket after the install pass.
    pub keeper_live: bool,
    /// `None` when the pass stopped before activation. `Some(outcome)` is the runtime-bound
    /// activation: `outcome.is_active()` holds only when this runtime's keeper proved live *and*
    /// the durable flag that routes new sessions to it is set, which are exactly the two
    /// conditions supervised restart needs.
    pub ownership: Option<crate::daemon::keeper_session::ActivationOutcome>,
    /// Keeper-held sessions discovered from the keeper itself, reconciled into the durable table.
    pub sessions: Vec<String>,
    /// Sessions this pass actually attached, in discovery order.
    ///
    /// Separate from `sessions` because discovery and attachment are different facts: a session can
    /// be discovered and then fail to attach (a keeper that answered the registry lookup and then
    /// refused the attach). Reporting only `sessions` would claim continuity for a session with no
    /// output pump here, which is exactly the silent loss this pass exists to prevent.
    pub attached: Vec<String>,
    /// Sessions the durable table expected but the keeper no longer holds.
    pub vanished: Vec<String>,
}

/// The bundle directory that holds the keeper payload, derived from the running executable.
///
/// A macOS app bundle lays the executable out at `Contents/MacOS/<exe>`, so the payload sits at
/// `Contents/Resources/keeper`. The installed daemon runs from a staged generation's helper bundle
/// (`<key>/Ferryx Daemon.app/Contents/MacOS/ferryx`), which has no Resources; its copy of the payload
/// is `<key>/keeper`, written when the generation was staged. A non-bundle binary (a `cargo run`
/// daemon) has neither; that is not an error, it is the `PayloadAbsent` case the caller logs.
pub fn bundle_keeper_dir(executable: &Path) -> Option<PathBuf> {
    let macos_dir = executable.parent()?;
    let contents = macos_dir.parent()?;
    if macos_dir.file_name()? != "MacOS" {
        return None;
    }
    let resources = contents.join("Resources").join(BUNDLE_KEEPER_DIR);
    if resources.is_dir() {
        return Some(resources);
    }
    let generation = contents.parent()?.parent()?;
    let staged = generation.join(crate::daemon::runtime_stage::KEEPER_PAYLOAD_DIR);
    Some(if staged.is_dir() { staged } else { resources })
}

/// The occupancy answer retention needs: how many live sessions each generation owns, read from the
/// keeper's own registry rather than from the durable table.
///
/// The keeper is authoritative, so a generation whose sessions survived a daemon restart is still
/// counted; the durable table alone would miss any session this daemon has not yet adopted.
///
/// The answer is a type, not a list, because "the keeper did not answer" and "the keeper answered
/// with no sessions" demand opposite actions and an empty `Vec` cannot tell them apart. There is no
/// variant for "the keeper did not answer": that is [`KeeperStartupError::OccupancyUnknown`], so an
/// unproven count is unrepresentable here and cannot reach retention by accident.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OccupancyState {
    /// The keeper answered: the live session count for every generation that owns at least one. A
    /// generation missing from this list genuinely owns none.
    Known(Vec<(String, usize)>),
    /// Nothing has ever been installed for this runtime and no keeper-owned session was ever
    /// recorded, so no keeper process can exist to own one. This is proof from durable artifacts,
    /// never from how a socket answered.
    NothingInstalled,
}

impl OccupancyState {
    /// How many keeper sessions `generation` owns, as far as this answer knows.
    pub fn sessions_for(&self, generation: &KeeperGeneration) -> usize {
        match self {
            Self::NothingInstalled => 0,
            Self::Known(counts) => counts
                .iter()
                .find(|(version, _)| version == generation.version())
                .map(|(_, count)| *count)
                .unwrap_or(0),
        }
    }
}

/// The durable artifacts that decide whether a keeper *can* exist here, independent of any socket.
pub struct InstallEvidence<'a> {
    pub plist_path: PathBuf,
    pub install_root: PathBuf,
    pub store: &'a crate::daemon::keeper_session::OwnershipStore,
}

/// The occupancy answer retention needs, or a typed refusal when occupancy was not proven.
///
/// A socket that refuses a connection, or is missing, proves nothing: a keeper can be wedged before
/// `listen`, its socket can be unlinked while the process holds live PTYs, or a keeper from another
/// install root can own them. So a failed discovery is not "no sessions" - it is a failure to learn,
/// and the only way out is independent evidence that nothing was ever installed.
pub fn occupied_by_generation(
    host: &KeeperSessionHost,
    evidence: &InstallEvidence<'_>,
) -> Result<OccupancyState, KeeperStartupError> {
    classify_occupancy(host.discover(), evidence)
}

fn classify_occupancy(
    discovered: Result<Vec<KeeperSessionView>, KeeperSessionError>,
    evidence: &InstallEvidence<'_>,
) -> Result<OccupancyState, KeeperStartupError> {
    let views = match discovered {
        Ok(views) => views,
        Err(error) => {
            // An access denial is the one discovery failure no durable artifact can answer. A socket
            // this process may not even use can belong to a keeper whose tree this process also may
            // not inspect, so an apparently empty install tree beside it proves nothing at all.
            // Failing closed is the whole point of the typed refusal: `nothing_installed` can
            // license a first install, but it can never license reading a refused probe as zero.
            if denies_access(&error) {
                return Err(KeeperStartupError::OccupancyUnknown {
                    detail: format!(
                        "the keeper socket denied this process access ({error}), so the sessions it \
                         may be serving cannot be counted from this install tree"
                    ),
                });
            }
            return if nothing_installed(evidence)? {
                Ok(OccupancyState::NothingInstalled)
            } else {
                Err(KeeperStartupError::OccupancyUnknown {
                    detail: format!(
                        "the keeper did not answer ({error}) while this install tree still holds \
                         state a keeper may be serving"
                    ),
                })
            };
        }
    };
    Ok(OccupancyState::Known(group_occupancy(views)))
}

/// Whether discovery failed because this process is not allowed to use the keeper socket, rather
/// than because nothing is listening there.
///
/// The distinction is load-bearing: `NotFound` and `ConnectionRefused` say no keeper is home, which
/// durable artifacts may corroborate into a first install. A denial says a keeper may well be home
/// and this process is blind to it, so no artifact read from the same tree can outvote it.
fn denies_access(error: &KeeperSessionError) -> bool {
    matches!(
        error,
        KeeperSessionError::Keeper(crate::daemon::keeper_client::KeeperClientError::Io(io))
            if io.kind() == std::io::ErrorKind::PermissionDenied
    )
}

fn group_occupancy(views: Vec<KeeperSessionView>) -> Vec<(String, usize)> {
    let mut counts: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for view in views {
        *counts.entry(view.generation).or_default() += 1;
    }
    counts.into_iter().collect()
}

/// Whether three durable facts together prove no keeper can be running for this runtime: no job
/// plist, no installed generation, and no keeper-owned session ever recorded.
///
/// Any observable artifact, and any read that fails, answers `false` or an error, so this can only
/// ever license a first install - it can never license a prune or a repoint over existing state.
fn nothing_installed(evidence: &InstallEvidence<'_>) -> Result<bool, KeeperStartupError> {
    let unproven = |detail: String| KeeperStartupError::OccupancyUnknown { detail };
    match std::fs::symlink_metadata(&evidence.plist_path) {
        Ok(_) => return Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(unproven(format!(
                "cannot inspect {}: {error}",
                evidence.plist_path.display()
            )))
        }
    }
    match std::fs::read_dir(&evidence.install_root) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry.map_err(|error| {
                    unproven(format!(
                        "cannot read {}: {error}",
                        evidence.install_root.display()
                    ))
                })?;
                if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                    return Ok(false);
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(unproven(format!(
                "cannot read {}: {error}",
                evidence.install_root.display()
            )))
        }
    }
    let table = evidence.store.load().map_err(|error| {
        unproven(format!(
            "cannot read the durable ownership table at {}: {error}",
            evidence.store.path().display()
        ))
    })?;
    Ok(table.keeper_owned_count() == 0)
}

/// The inputs of one install pass, spelled out so the pass runs without the process environment, the
/// real `launchctl`, or a live keeper.
pub(crate) struct KeeperInstallPass<'a> {
    pub service: &'a keeper_launchd::KeeperService,
    pub root: &'a Path,
    pub payload: &'a Path,
    pub version: &'a str,
    pub occupancy: &'a OccupancyState,
    pub launchctl: &'a dyn generation::Launchctl,
    pub uid: u32,
    pub run_at_load: bool,
    /// Whether this runtime is allowed to mutate a launchd job. False for a dev or isolated runtime,
    /// which must never register, bootstrap or kick a job - not even when the probe finds nothing.
    pub may_activate: bool,
    /// The exclusive section this pass runs inside. Carrying the token in the pass is what makes the
    /// lock unskippable: an install pass cannot be constructed without proof that the tree it reads
    /// occupancy from is the tree it then mutates.
    pub lock: &'a StartupLock,
}

/// Installs this build's keeper generation, prunes, points the job at it, and activates it.
///
/// `occupancy` is passed through to retention so a generation that still owns a session is never
/// removed and the job is never repointed away from it. An unknown occupancy never reaches this
/// function at all: [`occupied_by_generation`] refuses before the pass runs.
pub fn ensure_keeper(
    executable: &Path,
    occupancy: &OccupancyState,
    lock: &StartupLock,
    probe: &dyn Fn() -> bool,
) -> Result<KeeperStartupReport, KeeperStartupError> {
    let payload = bundle_keeper_dir(executable).ok_or_else(|| KeeperStartupError::PayloadAbsent {
        checked: executable.to_path_buf(),
    })?;
    let service = keeper_launchd::keeper_service_for(
        crate::daemon::server::is_dev_runtime(),
        runtime_is_isolated(),
        &|key| std::env::var_os(key),
    )?;
    let root = keeper_launchd::get_keeper_install_root().ok_or(KeeperServiceError::NoInstallRoot)?;
    let pass = KeeperInstallPass {
        service: &service,
        root: &root,
        payload: &payload,
        version: keeper_launchd::keeper_generation_version(),
        occupancy,
        launchctl: &generation::SystemLaunchctl,
        uid: current_uid(),
        run_at_load: true,
        may_activate: !crate::daemon::server::is_dev_runtime() && !runtime_is_isolated(),
        lock,
    };
    run_keeper_install_pass(&pass, probe)
}

/// Runs one install pass against explicit inputs. Everything is file-only until the activation step,
/// and the activation step is the only thing that may run `launchctl`.
pub(crate) fn run_keeper_install_pass(
    pass: &KeeperInstallPass<'_>,
    probe: &dyn Fn() -> bool,
) -> Result<KeeperStartupReport, KeeperStartupError> {
    let _exclusive = pass.lock;
    let source_binary = pass.payload.join(keeper_launchd::BINARY_NAME);
    if !source_binary.is_file() {
        return Err(KeeperStartupError::PayloadAbsent {
            checked: source_binary,
        });
    }
    let digest = payload_digest(pass.payload)?;
    let occupied = |generation: &KeeperGeneration| pass.occupancy.sessions_for(generation);
    let outcome = keeper_launchd::ensure_keeper_service(
        pass.service,
        pass.root,
        pass.version,
        &source_binary,
        digest.as_deref(),
        &occupied,
        pass.run_at_load,
    )?;
    let target_binary = pass
        .root
        .join(pass.version)
        .join(keeper_launchd::BINARY_NAME);
    let activation = if pass.may_activate {
        activate_keeper(pass.service, &target_binary, pass.uid, pass.launchctl, probe)
    } else {
        KeeperActivation::Skipped {
            reason: "a dev or isolated runtime never mutates a launchd job".to_string(),
        }
    };
    Ok(KeeperStartupReport {
        generation: Some(outcome.generation.version().to_string()),
        installed: outcome.installed,
        plist_rewritten: outcome.plist_rewritten,
        pruned: outcome.pruned.removed,
        preserved_occupied: outcome.pruned.preserved_occupied,
        activation,
        keeper_live: probe(),
        ownership: None,
        sessions: Vec::new(),
        attached: Vec::new(),
        vanished: Vec::new(),
    })
}

/// Starts the keeper job this pass just wrote, so a first install does not have to wait for the next
/// login. The probe is asked first: a keeper that already answers is never disturbed, and no
/// `launchctl` call is made at all. The plist is then read back and the job is started only when it
/// programs exactly `target_binary`, because a plist naming another program belongs to some other
/// generation's job and this pass must not bootstrap it. `-k` is never passed and `bootout` is never
/// called, so a job launchd reports as running or as a state this daemon cannot read is left alone
/// rather than restarted under a live keeper's sessions.
fn activate_keeper(
    service: &keeper_launchd::KeeperService,
    target_binary: &Path,
    uid: u32,
    launchctl: &dyn generation::Launchctl,
    probe: &dyn Fn() -> bool,
) -> KeeperActivation {
    if probe() {
        return KeeperActivation::AlreadyLive;
    }
    let content = match std::fs::read_to_string(&service.plist_path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return KeeperActivation::Failed {
                detail: format!(
                    "the keeper plist {} is absent after the install pass",
                    service.plist_path.display()
                ),
            }
        }
        Err(error) => {
            return KeeperActivation::Failed {
                detail: format!("cannot read {}: {error}", service.plist_path.display()),
            }
        }
    };
    let programs_target = generation::plist_program(&content)
        .is_some_and(|program| Path::new(program) == target_binary);
    if !programs_target {
        return KeeperActivation::PlistMismatch {
            expected: target_binary.to_path_buf(),
            found: generation::plist_program(&content).map(PathBuf::from),
        };
    }
    match generation::print_job(launchctl, uid, &service.label) {
        generation::JobState::NotRegistered => {
            let domain = format!("gui/{uid}");
            let plist = service.plist_path.to_string_lossy().into_owned();
            match mutate(launchctl, &["bootstrap", domain.as_str(), plist.as_str()]) {
                Ok(()) => KeeperActivation::Bootstrapped,
                Err(detail) => KeeperActivation::Failed { detail },
            }
        }
        generation::JobState::RegisteredIdle => {
            let target = format!("gui/{uid}/{}", service.label);
            match mutate(launchctl, &["kickstart", target.as_str()]) {
                Ok(()) => KeeperActivation::Kickstarted,
                Err(detail) => KeeperActivation::Failed { detail },
            }
        }
        generation::JobState::Running { pid } => KeeperActivation::LeftAlone {
            state: format!("running (pid {pid})"),
        },
        generation::JobState::Unverifiable { detail } => KeeperActivation::LeftAlone {
            state: format!("unverifiable: {detail}"),
        },
    }
}

/// One `launchctl` mutation, with the tool's own text in the error.
fn mutate(launchctl: &dyn generation::Launchctl, args: &[&str]) -> Result<(), String> {
    let output = launchctl
        .run(args)
        .map_err(|error| format!("launchctl {} could not run: {error}", args[0]))?;
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

fn current_uid() -> u32 {
    // SAFETY: `libc::getuid` takes no arguments, dereferences no pointers and has no side effects.
    unsafe { libc::getuid() }
}

/// Name of the startup lock file inside the keeper install root.
pub(crate) const STARTUP_LOCK_NAME: &str = ".keeper-startup.lock";

/// Exclusive ownership of one startup pass, held from discovery until the last mutation.
///
/// The invariant this enforces: the install tree an occupancy answer was read from is the same tree
/// the mutations then act on. A fresh-install answer means "empty tree, no job, no recorded keeper
/// session", and none of those three facts is stable under a concurrent install: another process
/// could plant a generation or start a keeper between the answer and the prune, and the prune would
/// then delete a generation that had just become live. Serializing the two steps removes that
/// window. A pass that cannot take the lock refuses rather than guessing.
///
/// The boundary, stated precisely so no caller reads more into it: this flock serializes passes that
/// go through [`ensure_keeper`], which today is the daemon's startup pass. It does not fence a
/// writer that never takes it, and the keeper crate's own
/// `ferryx_ptykeeper::install::InstallDir::{install, prune}` writes the same
/// `~/.ferryx/keeper/<version>` tree with no lock at all. While that stays true, exclusive
/// ownership covers daemon startup passes only and is not proof against an external installer.
pub struct StartupLock {
    file: std::fs::File,
}

impl StartupLock {
    pub(crate) fn acquire(root: &Path) -> Result<Self, KeeperStartupError> {
        let unproven = |detail: String| KeeperStartupError::OccupancyUnknown { detail };
        std::fs::create_dir_all(root)
            .map_err(|error| unproven(format!("cannot create {}: {error}", root.display())))?;
        let path = root.join(STARTUP_LOCK_NAME);
        let file = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .map_err(|error| unproven(format!("cannot open {}: {error}", path.display())))?;
        use std::os::unix::io::AsRawFd;
        // SAFETY: `as_raw_fd` borrows a descriptor from `file`, which stays open and valid for the
        // call; `flock` only sets a lock flag on that descriptor and touches no memory.
        let locked = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if locked != 0 {
            return Err(unproven(format!(
                "another keeper startup pass holds {}; this pass will not decide occupancy from a \
                 tree that pass may be mutating",
                path.display()
            )));
        }
        Ok(Self { file })
    }
}

impl Drop for StartupLock {
    fn drop(&mut self) {
        use std::os::unix::io::AsRawFd;
        // SAFETY: same descriptor invariant as acquisition; releasing a lock this process holds.
        unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
    }
}

/// Discovers the keeper's sessions and reconciles them into the durable ownership table.
///
/// A discovery failure is returned rather than flattened into an empty answer: "no sessions" and
/// "the keeper never told us" lead to opposite decisions, and a caller handed an empty
/// [`StartupDiscovery`] would write off every session the durable table still expects.
pub fn reconcile(
    store: &crate::daemon::keeper_session::OwnershipStore,
) -> Result<StartupDiscovery, KeeperStartupError> {
    super::keeper_session::startup_discovery(store).map_err(|error| {
        tracing::warn!(
            %error,
            "keeper discovery failed; no keeper session was adopted or written off"
        );
        KeeperStartupError::Discovery {
            detail: error.to_string(),
        }
    })
}

/// The whole production startup pass: install this build's generation, check whether a keeper is
/// live, reconcile its sessions into the durable table, and wire the runtime into the terminal
/// service so every later spawn and every keeper-owned session's I/O is routed to it.
///
/// The runtime is always installed, even with no live keeper: routing decisions then read the
/// durable flag, and a spawn into an absent keeper fails with a typed error instead of silently
/// falling back to a direct PTY and losing the durability the flag promised.
pub fn start_production(
    terminal_service: std::sync::Arc<crate::terminal::TerminalService>,
) -> Result<KeeperStartupReport, KeeperStartupError> {
    let store = crate::daemon::keeper_session::OwnershipStore::for_runtime()
        .map_err(|error| KeeperStartupError::PayloadUnusable {
            detail: error.to_string(),
        })?;
    let host = KeeperSessionHost::for_runtime();
    let root = keeper_launchd::get_keeper_install_root().ok_or(KeeperServiceError::NoInstallRoot)?;
    let service = keeper_launchd::keeper_service_for(
        crate::daemon::server::is_dev_runtime(),
        runtime_is_isolated(),
        &|key| std::env::var_os(key),
    )?;
    // The lock covers discovery and every mutation that follows it. Without it a "nothing is
    // installed" answer could be stale by the time the prune runs, and the prune would then delete a
    // generation a concurrent install had just made live. Refusing is the only safe answer when the
    // lock is unavailable, because occupancy cannot be trusted outside it.
    let _startup_lock = StartupLock::acquire(&root)?;
    // Occupancy is decided before anything touches the install tree or the plist: an answer of
    // "unknown" returns here, so no generation can be pruned and the job is never repointed away
    // from a keeper that might still own a session.
    let occupancy = occupied_by_generation(
        &host,
        &InstallEvidence {
            plist_path: service.plist_path.clone(),
            install_root: root.clone(),
            store: &store,
        },
    )?;
    let probe = || {
        use crate::daemon::crash_supervisor::KeeperPresence as _;
        crate::daemon::crash_supervisor::KeeperSocketProbe::for_runtime()
            .live_keeper()
            .is_live()
    };
    let executable = std::env::current_exe().unwrap_or_default();
    let mut report = ensure_keeper(&executable, &occupancy, &_startup_lock, &probe)?;
    let discovery = reconcile(&store)?;
    report.sessions = discovery
        .sessions
        .iter()
        .map(|view| view.session_id.clone())
        .collect();
    report.vanished = discovery.vanished;

    let ownership = std::sync::Arc::new(crate::daemon::keeper_session::KeeperOwnership::new(
        store,
        crate::daemon::keeper_session::CheckpointCadence::default(),
    ));
    let mut runtime = crate::daemon::keeper_session::KeeperRuntime::new(host, ownership);
    let service_for_checkpoints = std::sync::Arc::clone(&terminal_service);
    runtime.set_checkpoint_source(std::sync::Arc::new(move |session_id: &str| {
        paired_checkpoint_snapshot(&service_for_checkpoints, session_id)
    }));
    // The gap sink is wired here, before the runtime is wrapped in an `Arc` and handed to the
    // terminal service, because `set_gap_sink` needs `&mut` and `install_keeper` takes an `Arc`.
    // Without this the pump's `REPLAY_GAP` arm calls the default no-op and the receiver keeps
    // parsing a stream with a hole in it: bytes after the seam would be applied to pre-gap parser
    // state, which is the discontinuous feed the recovery contract forbids. `mark_replay_gap`
    // publishes the `ReplayGap` seam on the hub's own sequence axis, and the pump only forwards
    // post-gap bytes after it returns, so the reset is always observed first.
    let service_for_gaps = std::sync::Arc::clone(&terminal_service);
    runtime.set_gap_sink(std::sync::Arc::new(move |session_id: &str| {
        if !service_for_gaps.mark_replay_gap(session_id) {
            // A session the hub never saw has no parser to reset and nothing to publish to.
            tracing::warn!(
                session_id,
                "keeper reported a replay gap for a session this hub does not hold"
            );
        }
    }));
    runtime.set_ring_capacity_bytes(terminal_service.output_hub().session_ring_capacity());
    let service_for_bytes = std::sync::Arc::clone(&terminal_service);
    runtime.set_hub_published(std::sync::Arc::new(move |session_id: &str| {
        service_for_bytes
            .output_hub()
            .session_published_bytes(session_id)
    }));
    let service_for_restore = std::sync::Arc::clone(&terminal_service);
    runtime.set_restore(std::sync::Arc::new(move |session_id: &str, bytes: &[u8]| {
        service_for_restore.restore_checkpoint(session_id, bytes)
    }));
    let runtime = std::sync::Arc::new(runtime);
    terminal_service.install_keeper(std::sync::Arc::clone(&runtime));

    let (attachment, outcome) = reconcile_and_activate(
        &runtime,
        &terminal_service,
        &discovery.sessions,
        ownership_policy_from_env(),
    );
    let KeeperAttachment { attached, unserved } = attachment;
    report.attached = attached;
    // A session that was discovered but not attached has no pump here, so reporting it under
    // `sessions` alone would claim continuity this pass did not achieve. It stays keeper-owned and
    // recorded, which is what lets the next pass retry it.
    if !unserved.is_empty() {
        tracing::warn!(
            sessions = ?unserved,
            "keeper-held sessions were discovered but could not be attached; they stay \
             keeper-owned and are retried on the next startup pass"
        );
    }

    // Activation comes last, after the recovered sessions have a pump here and the runtime is
    // installed: new sessions are routed to the keeper only once this runtime can demonstrably
    // serve the ones the keeper already holds. The outcome is reported, never assumed, and a
    // failure leaves the flag exactly as it was rather than promising durability it cannot keep.
    match outcome {
        Ok(outcome) => {
            tracing::info!(
                generation = ?report.generation,
                keeper_live = report.keeper_live,
                attached = report.attached.len(),
                ?outcome,
                "keeper ownership activation"
            );
            report.ownership = Some(outcome);
        }
        // The flag write is the one step whose failure leaves routing undecided rather than merely
        // unproven: a spawn after this point reads whatever the table still says. Reported, never
        // swallowed, and the sessions above are already attached either way.
        Err(error) => {
            tracing::warn!(%error, "keeper ownership flag was not written; routing is unchanged")
        }
    }
    Ok(report)
}

fn runtime_is_isolated() -> bool {
    crate::daemon::supervisor::ISOLATED_RUNTIME_ENV
        .iter()
        .any(|key| std::env::var_os(key).is_some_and(|value| !value.is_empty()))
}

/// What this pass will do to the new-session routing flag.
///
/// The flag is a daemon-start decision, never a live toggle: `Enabled` routes new spawns to the
/// keeper once one proves live, `Disabled` keeps them direct-owned, and `Unchanged` leaves the
/// durable value alone. `Disabled` is the production disable surface, and it is a *routing* answer
/// only -- it never stops this pass from attaching the sessions the keeper already holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnershipPolicy {
    /// Route new sessions to the keeper once this runtime's keeper proves live.
    Enabled,
    /// Keep new sessions direct-owned. Existing keeper-owned sessions still attach.
    Disabled,
    /// Leave the persisted flag exactly as it is.
    Unchanged,
}

/// The environment override that pins the new-session routing flag at daemon start.
///
/// The name is deliberately not one of [`crate::daemon::supervisor::ISOLATED_RUNTIME_ENV`]: setting
/// it must not by itself make a runtime look isolated, or a launch that disabled the keeper would
/// stop being able to reach its own keeper install tree.
pub const KEEPER_OWNERSHIP_POLICY_ENV: &str = "FERRYX_KEEPER_OWNERSHIP";

impl OwnershipPolicy {
    /// Parses the override. An unrecognised value is an error, never a default: reading
    /// `FERRYX_KEEPER_OWNERSHIP=enabeld` as `Unchanged` would let an operator believe a disable
    /// happened when nothing was written.
    pub fn parse(raw: &str) -> Result<Self, KeeperStartupError> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "enabled" | "enable" | "on" | "true" | "1" => Ok(Self::Enabled),
            "disabled" | "disable" | "off" | "false" | "0" => Ok(Self::Disabled),
            "unchanged" | "auto" => Ok(Self::Unchanged),
            other => Err(KeeperStartupError::OwnershipPolicyUnknown {
                detail: format!(
                    "{KEEPER_OWNERSHIP_POLICY_ENV}={other:?} is not one of \
                     enabled|disabled|unchanged"
                ),
            }),
        }
    }

    /// The policy the process environment asks for. An absent or empty value means `Unchanged`, so
    /// a launch that does not care keeps the persisted flag byte for byte.
    pub fn from_env(
        env: &dyn Fn(&str) -> Option<std::ffi::OsString>,
    ) -> Result<Self, KeeperStartupError> {
        match env(KEEPER_OWNERSHIP_POLICY_ENV).filter(|value| !value.is_empty()) {
            // A non-UTF-8 value cannot be any accepted spelling, and guessing would turn a typo into
            // a silent enable.
            Some(value) => match value.to_str() {
                Some(text) => Self::parse(text),
                None => Err(KeeperStartupError::OwnershipPolicyUnknown {
                    detail: format!(
                        "{KEEPER_OWNERSHIP_POLICY_ENV} is not valid UTF-8, so it cannot name a \
                         routing policy"
                    ),
                }),
            },
            None => Ok(Self::Unchanged),
        }
    }

    /// What this policy says about new sessions, or `None` when it does not say.
    pub fn routes_new_sessions_to_keeper(&self) -> Option<bool> {
        match self {
            Self::Enabled => Some(true),
            Self::Disabled => Some(false),
            Self::Unchanged => None,
        }
    }
}

fn ownership_policy_from_env() -> OwnershipPolicy {
    match OwnershipPolicy::from_env(&|key: &str| std::env::var_os(key)) {
        Ok(policy) => policy,
        Err(error) => {
            // Refusing here would abort the whole startup pass, including attaching keeper-held
            // sessions, over a typo in an override. The safer reading is "change nothing": the
            // durable flag stays as the user last set it, and the refusal is logged.
            tracing::warn!(%error, "keeper ownership override ignored");
            OwnershipPolicy::Unchanged
        }
    }
}

/// What one reconcile-and-activate step attached.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeeperAttachment {
    /// Sessions that now have an output pump on this runtime, in discovery order.
    pub attached: Vec<String>,
    /// Sessions the keeper holds that this pass could not attach, with the typed refusal.
    pub unserved: Vec<(String, String)>,
}

/// Attaches every keeper-held session, then applies the routing policy to new sessions.
///
/// The ordering is the whole contract: attachment happens **first and unconditionally**, before the
/// flag is touched. Disabling keeper ownership is a statement about new sessions, so a pass that
/// disabled first and attached second would strand every session the keeper still holds for the
/// rest of their lives. Attaching first makes the disable harmless by construction rather than by
/// the order a caller happens to use.
///
/// A failed attach never removes the session's durable record and never closes its keeper-side PTY:
/// the keeper is still serving it, so that record is the only thing that lets the next pass retry.
pub fn reconcile_and_activate(
    runtime: &crate::daemon::keeper_session::KeeperRuntime,
    terminal_service: &crate::terminal::TerminalService,
    discovered: &[KeeperSessionView],
    policy: OwnershipPolicy,
) -> (
    KeeperAttachment,
    Result<crate::daemon::keeper_session::ActivationOutcome, KeeperStartupError>,
) {
    // Both seams are closures so the *contract* below is testable without a keeper socket: attaching
    // needs a live keeper, and the ordering it must obey is exactly what a test has to observe.
    let store = runtime.ownership().store();
    attach_then_apply_policy(
        discovered,
        policy,
        &|session_id: &str| {
            let (recovered, rx) = runtime.attach(session_id)?;
            // Either condition means this hub cannot serve the session's true first byte: the
            // prefix was beyond what one recovery reads, or the keeper dropped bytes at a seam.
            // Reporting only `prefix_skipped` left a gapped recovery registered as a complete
            // history, so a reader was never told its first byte was not the session's first.
            terminal_service.pump_keeper_output(
                session_id,
                rx,
                recovered.prefix_skipped || recovered.replay_gap.is_some(),
            );
            Ok(())
        },
        &|| {
            runtime
                .activate_new_sessions()
                .map_err(|error| error.to_string())
        },
        // The disable half of the flag. This is the one production call site that turns keeper
        // ownership off, and it is deliberately not behind a helper: a source-level audit looks for
        // this exact call, and an indirection would report a disable path that no reader can find.
        &|| {
            store
                .set_keeper_enabled(false)
                .map(|_| ())
                .map_err(|error| error.to_string())
        },
    )
}

/// The ordered composition one startup pass performs: attach every keeper-held session, then apply
/// the new-session routing policy.
///
/// Taking both halves as closures is what makes the *order* testable without a keeper socket. The
/// order is the contract: a disable that ran before attachment would leave every session the keeper
/// still holds without a pump for the rest of its life, so the sequence cannot be left to whichever
/// call site happens to call the pieces in the right order.
pub fn attach_then_apply_policy(
    discovered: &[KeeperSessionView],
    policy: OwnershipPolicy,
    attach: &dyn Fn(&str) -> Result<(), crate::daemon::keeper_session::KeeperSessionError>,
    activate: &dyn Fn() -> Result<crate::daemon::keeper_session::ActivationOutcome, String>,
    suppress_new_sessions: &dyn Fn() -> Result<(), String>,
) -> (
    KeeperAttachment,
    Result<crate::daemon::keeper_session::ActivationOutcome, KeeperStartupError>,
) {
    let attachment = attach_all(discovered, attach);
    let outcome = apply_policy(
        policy,
        attachment.attached.len(),
        activate,
        suppress_new_sessions,
    );
    (attachment, outcome)
}

/// Attaches every discovered session, collecting the ones that could not be attached.
///
/// `attach` returns once a pump exists for the session; its typed `Err` text is carried into
/// [`KeeperAttachment::unserved`] so the report names *why* a session has no pump instead of only
/// that it has none.
fn attach_all(
    discovered: &[KeeperSessionView],
    attach: &dyn Fn(&str) -> Result<(), crate::daemon::keeper_session::KeeperSessionError>,
) -> KeeperAttachment {
    let mut attachment = KeeperAttachment::default();
    for view in discovered {
        match attach(&view.session_id) {
            Ok(()) => attachment.attached.push(view.session_id.clone()),
            Err(error) => {
                tracing::warn!(
                    session_id = %view.session_id,
                    %error,
                    "keeper session could not be reattached; it stays keeper-owned and is retried \
                     next start"
                );
                attachment
                    .unserved
                    .push((view.session_id.clone(), error.to_string()));
            }
        }
    }
    attachment
}

/// Applies the routing policy to new sessions. Attachment has already happened by the time this
/// runs, which is what makes a disable unable to strand a keeper-held session.
///
/// `attached` is the count reported by [`KeeperAttachment`], so the suppression outcome names how
/// much was still being served when new sessions stopped going to the keeper.
fn apply_policy(
    policy: OwnershipPolicy,
    attached: usize,
    activate: &dyn Fn() -> Result<crate::daemon::keeper_session::ActivationOutcome, String>,
    suppress_new_sessions: &dyn Fn() -> Result<(), String>,
) -> Result<crate::daemon::keeper_session::ActivationOutcome, KeeperStartupError> {
    // `Enabled` and `Unchanged` both reach activation: activation is itself a no-op unless a keeper
    // proves live and the flag is off, so an unchanged launch is never asked to decide.
    match policy.routes_new_sessions_to_keeper() {
        Some(false) => suppress_new_sessions()
            .map(|()| {
                crate::daemon::keeper_session::ActivationOutcome::Suppressed {
                    keeper_sessions: attached,
                }
            })
            .map_err(|error| KeeperStartupError::OwnershipWriteRefused { detail: error }),
        _ => activate().map_err(|error| KeeperStartupError::OwnershipWriteRefused { detail: error }),
    }
}

/// Serialized terminal state paired with the output offset that state covers.
///
/// The pair must be one observation, and it now is one: `TerminalService::checkpoint_snapshot`
/// serializes and reads the published-byte counter inside a single `SessionHub` read lock. The
/// previous implementation read the counter on both sides of a serialization and retried up to four
/// times, which is correct but can *never* succeed for a session that publishes continuously -- and
/// a session producing output is precisely the one whose checkpoint matters. Missing the pair meant
/// committing nothing at all, so the whole session would recover from the replay log alone.
#[cfg(unix)]
fn paired_checkpoint_snapshot(
    service: &crate::terminal::TerminalService,
    session_id: &str,
) -> Option<crate::daemon::keeper_session::CheckpointSnapshot> {
    service
        .checkpoint_snapshot(session_id)
        .map(
            |(bytes, applied_offset)| crate::daemon::keeper_session::CheckpointSnapshot {
                bytes,
                applied_offset,
            },
        )
}

/// The sha256 recorded beside the bundle payload, so the installed generation's digest describes the
/// bytes that shipped. An absent manifest is not fatal: the generation still installs, without a
/// recorded digest.
fn payload_digest(payload: &Path) -> Result<Option<String>, KeeperStartupError> {
    let manifest = payload.join("keeper.json");
    let text = match std::fs::read_to_string(&manifest) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(KeeperStartupError::PayloadUnusable {
                detail: format!("cannot read {}: {error}", manifest.display()),
            })
        }
    };
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|error| KeeperStartupError::PayloadUnusable {
            detail: format!("{} is not valid JSON: {error}", manifest.display()),
        })?;
    let recorded = value
        .get("sha256")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let actual = sha256_file(&payload.join(keeper_launchd::BINARY_NAME))?;
    match recorded {
        Some(recorded) if recorded.eq_ignore_ascii_case(&actual) => Ok(Some(actual)),
        Some(recorded) => Err(KeeperStartupError::PayloadUnusable {
            detail: format!(
                "{} records sha256 {recorded} but the binary hashes to {actual}",
                manifest.display()
            ),
        }),
        None => Ok(Some(actual)),
    }
}

fn sha256_file(path: &Path) -> Result<String, KeeperStartupError> {
    use sha2::Digest as _;
    let bytes = std::fs::read(path).map_err(|error| KeeperStartupError::PayloadUnusable {
        detail: format!("cannot read {}: {error}", path.display()),
    })?;
    let mut hasher = sha2::Sha256::new();
    hasher.update(&bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::keeper_client::KeeperClientError;

    #[test]
    fn a_staged_daemon_resolves_its_generations_keeper_payload() {
        let temp = tempfile::tempdir().unwrap();
        // Given: an installed app with its payload, and a staged generation with its own copy.
        let app = temp.path().join("Ferryx.app/Contents");
        std::fs::create_dir_all(app.join("Resources").join(BUNDLE_KEEPER_DIR)).unwrap();
        let generation = temp.path().join("daemon-runtime/1.0.0-00000001");
        let staged = generation.join(crate::daemon::runtime_stage::KEEPER_PAYLOAD_DIR);
        std::fs::create_dir_all(&staged).unwrap();
        // When / Then: the app executable reads Resources; the staged helper reads its generation.
        assert_eq!(
            bundle_keeper_dir(&app.join("MacOS/ferryx")),
            Some(app.join("Resources").join(BUNDLE_KEEPER_DIR))
        );
        assert_eq!(
            bundle_keeper_dir(&generation.join("Ferryx Daemon.app/Contents/MacOS/ferryx")),
            Some(staged)
        );
        // A binary outside any bundle has no payload directory.
        assert_eq!(bundle_keeper_dir(&temp.path().join("bin/ferryx")), None);
    }
    use crate::daemon::keeper_session::{
        OwnershipRecord, OwnershipStore, SessionOwner, KeeperSessionView,
    };
    use crate::daemon::launchd::generation::{Launchctl, LaunchctlOutput};
    use std::cell::RefCell;
    use std::io;

    /// Deterministic stand-in for `launchctl`: every call is recorded and nothing is executed.
    struct FakeLaunchctl {
        print: (bool, String, String),
        fail_mutations: bool,
        calls: RefCell<Vec<Vec<String>>>,
    }

    impl FakeLaunchctl {
        fn new(print: (bool, String, String)) -> Self {
            Self {
                print,
                fail_mutations: false,
                calls: RefCell::new(Vec::new()),
            }
        }

        fn not_registered() -> Self {
            Self::new((false, String::new(), "Could not find service".to_string()))
        }

        fn idle() -> Self {
            Self::new((true, "state = waiting\n".to_string(), String::new()))
        }

        fn running(pid: u32) -> Self {
            Self::new((true, format!("pid = {pid}\n"), String::new()))
        }

        fn unverifiable() -> Self {
            Self::new((false, String::new(), "Input/output error".to_string()))
        }

        fn calls(&self) -> Vec<Vec<String>> {
            self.calls.borrow().clone()
        }

        fn mutations(&self) -> Vec<Vec<String>> {
            self.calls()
                .into_iter()
                .filter(|args| args.first().is_some_and(|verb| verb != "print"))
                .collect()
        }
    }

    impl Launchctl for FakeLaunchctl {
        fn run(&self, args: &[&str]) -> io::Result<LaunchctlOutput> {
            self.calls
                .borrow_mut()
                .push(args.iter().map(|arg| (*arg).to_string()).collect());
            if args.first() == Some(&"print") {
                let (success, stdout, stderr) = self.print.clone();
                return Ok(LaunchctlOutput {
                    success,
                    stdout,
                    stderr,
                });
            }
            if self.fail_mutations {
                return Err(io::Error::new(io::ErrorKind::NotFound, "no launchctl"));
            }
            Ok(LaunchctlOutput {
                success: true,
                stdout: String::new(),
                stderr: String::new(),
            })
        }
    }

    struct Fixture {
        dir: tempfile::TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                dir: tempfile::tempdir().expect("temp dir"),
            }
        }

        fn root(&self) -> PathBuf {
            self.dir.path().join("keeper")
        }

        fn plist(&self) -> PathBuf {
            self.dir.path().join("com.ferryx.qa.ptykeeper.plist")
        }

        fn store(&self) -> OwnershipStore {
            OwnershipStore::at(self.dir.path().join("pty-ownership.json"))
        }

        fn payload(&self) -> PathBuf {
            let payload = self.dir.path().join("payload");
            std::fs::create_dir_all(&payload).expect("payload dir");
            std::fs::write(payload.join(keeper_launchd::BINARY_NAME), b"keeper-bytes")
                .expect("payload binary");
            payload
        }

        fn service(&self) -> keeper_launchd::KeeperService {
            keeper_launchd::KeeperService {
                label: "com.ferryx.qa.ptykeeper".to_string(),
                plist_path: self.plist(),
                log_dir: self.dir.path().join("logs"),
                environment: Vec::new(),
            }
        }

        fn target(&self, version: &str) -> PathBuf {
            self.root().join(version).join(keeper_launchd::BINARY_NAME)
        }

        fn plant_generation(&self, version: &str) {
            let dir = self.root().join(version);
            std::fs::create_dir_all(&dir).expect("generation dir");
            std::fs::write(dir.join(keeper_launchd::BINARY_NAME), version.as_bytes())
                .expect("generation binary");
        }

        fn evidence<'a>(&self, store: &'a OwnershipStore) -> InstallEvidence<'a> {
            InstallEvidence {
                plist_path: self.plist(),
                install_root: self.root(),
                store,
            }
        }
    }

    fn view(session_id: &str, generation: &str) -> KeeperSessionView {
        KeeperSessionView {
            session_id: session_id.to_string(),
            pid: 1,
            epoch: 1,
            sequence_offset: 0,
            cols: 80,
            rows: 24,
            owner: "keeper".to_string(),
            generation: generation.to_string(),
        }
    }

    fn io_error(kind: io::ErrorKind) -> KeeperSessionError {
        KeeperSessionError::Keeper(KeeperClientError::Io(io::Error::new(kind, "socket")))
    }

    fn protocol() -> KeeperSessionError {
        KeeperSessionError::Keeper(KeeperClientError::Protocol("unexpected frame".to_string()))
    }

    fn pass<'a>(
        service: &'a keeper_launchd::KeeperService,
        root: &'a Path,
        payload: &'a Path,
        occupancy: &'a OccupancyState,
        launchctl: &'a dyn Launchctl,
        may_activate: bool,
        lock: &'a StartupLock,
    ) -> KeeperInstallPass<'a> {
        KeeperInstallPass {
            service,
            root,
            payload,
            version: "9.9.9",
            occupancy,
            launchctl,
            uid: 501,
            run_at_load: true,
            may_activate,
            lock,
        }
    }

    #[test]
    fn a_missing_socket_never_proves_zero_occupancy_over_an_installed_tree() {
        let fixture = Fixture::new();
        fixture.plant_generation("9.9.8");
        let store = fixture.store();
        let evidence = fixture.evidence(&store);
        assert!(matches!(
            classify_occupancy(Err(io_error(io::ErrorKind::NotFound)), &evidence),
            Err(KeeperStartupError::OccupancyUnknown { .. })
        ));
    }

    #[test]
    fn a_refused_connection_never_proves_zero_occupancy() {
        let fixture = Fixture::new();
        fixture.plant_generation("9.9.8");
        let store = fixture.store();
        let evidence = fixture.evidence(&store);
        assert!(matches!(
            classify_occupancy(Err(io_error(io::ErrorKind::ConnectionRefused)), &evidence),
            Err(KeeperStartupError::OccupancyUnknown { .. })
        ));
    }

    #[test]
    fn a_refused_connection_with_a_job_plist_never_proves_zero_occupancy() {
        let fixture = Fixture::new();
        std::fs::write(fixture.plist(), "<plist/>").expect("plist");
        let store = fixture.store();
        let evidence = fixture.evidence(&store);
        assert!(matches!(
            classify_occupancy(Err(io_error(io::ErrorKind::ConnectionRefused)), &evidence),
            Err(KeeperStartupError::OccupancyUnknown { .. })
        ));
    }

    #[test]
    fn a_recorded_keeper_session_blocks_the_fresh_install_answer() {
        let fixture = Fixture::new();
        let store = fixture.store();
        let mut table = crate::daemon::keeper_session::OwnershipTable::default();
        table.record(OwnershipRecord {
            session_id: "s1".to_string(),
            owner: SessionOwner::Keeper,
            epoch: 0,
            cols: 0,
            rows: 0,
            pid: 0,
            process_start_time: None,
            generation: None,
            created_at: 0,
            checkpoint_offset: 0,
        });
        store.save(&table).expect("save table");
        let evidence = fixture.evidence(&store);
        assert!(matches!(
            classify_occupancy(Err(protocol()), &evidence),
            Err(KeeperStartupError::OccupancyUnknown { .. })
        ));
    }

    #[test]
    fn a_permission_denied_discovery_never_proves_zero_occupancy() {
        let fixture = Fixture::new();
        let store = fixture.store();
        let evidence = fixture.evidence(&store);
        assert!(matches!(
            classify_occupancy(Err(io_error(io::ErrorKind::PermissionDenied)), &evidence),
            Err(KeeperStartupError::OccupancyUnknown { .. })
        ));
    }

    #[test]
    fn an_absent_policy_means_unchanged_and_a_typo_is_refused_by_name() {
        let empty = OwnershipPolicy::from_env(&|_key: &str| -> Option<std::ffi::OsString> { None })
            .expect("absent override");
        assert_eq!(empty, OwnershipPolicy::Unchanged);
        assert_eq!(empty.routes_new_sessions_to_keeper(), None);
        // An empty value is absent, not "false": an unset-but-present variable is how a shell
        // forwards an unset knob, and reading it as a disable would stop routing silently.
        let blank = OwnershipPolicy::from_env(&|_key: &str| Some(std::ffi::OsString::from("")))
            .expect("empty override");
        assert_eq!(blank, OwnershipPolicy::Unchanged);

        assert_eq!(
            OwnershipPolicy::parse("disabled").expect("disable"),
            OwnershipPolicy::Disabled
        );
        assert_eq!(
            OwnershipPolicy::parse("  OFF ").expect("case and space insensitive"),
            OwnershipPolicy::Disabled
        );
        assert_eq!(
            OwnershipPolicy::parse("enabled").expect("enable"),
            OwnershipPolicy::Enabled
        );
        let refusal = OwnershipPolicy::parse("enabeld").expect_err("typo must be refused");
        assert!(matches!(
            refusal,
            KeeperStartupError::OwnershipPolicyUnknown { .. }
        ));
        // The refusal names the variable and the accepted values, so an operator can fix it.
        let text = refusal.to_string();
        assert!(text.contains("KEEPER_OWNERSHIP_POLICY_UNKNOWN"), "{text}");
        assert!(text.contains(KEEPER_OWNERSHIP_POLICY_ENV), "{text}");
        assert!(text.contains("enabeld"), "{text}");
    }

    #[test]
    fn disabling_attaches_every_keeper_session_first_and_then_stops_routing_new_ones() {
        let discovered = vec![view("a", "9.9.9"), view("b", "9.9.9"), view("c", "9.9.8")];
        let order = RefCell::new(Vec::new());
        let (attachment, outcome) = attach_then_apply_policy(
            &discovered,
            OwnershipPolicy::Disabled,
            &|session_id| {
                order.borrow_mut().push(format!("attach:{session_id}"));
                Ok(())
            },
            &|| {
                order.borrow_mut().push("activate".to_string());
                Ok(crate::daemon::keeper_session::ActivationOutcome::Activated {
                    keeper_pid: Some(1),
                })
            },
            &|| {
                order.borrow_mut().push("disable".to_string());
                Ok(())
            },
        );

        // The whole contract is here: every attach precedes the flag write, and activation is never
        // reached, because a disable does not ask whether a keeper could serve a *new* session.
        assert_eq!(
            order.borrow().to_vec(),
            vec![
                "attach:a".to_string(),
                "attach:b".to_string(),
                "attach:c".to_string(),
                "disable".to_string(),
            ]
        );
        assert_eq!(
            attachment.attached,
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
        assert!(attachment.unserved.is_empty());
        assert_eq!(
            outcome.expect("disable is a success"),
            crate::daemon::keeper_session::ActivationOutcome::Suppressed {
                keeper_sessions: 3
            }
        );
    }

    #[test]
    fn a_session_that_cannot_attach_is_reported_and_never_counted_as_served() {
        let discovered = vec![view("live", "9.9.9"), view("gone", "9.9.9")];
        let (attachment, outcome) = attach_then_apply_policy(
            &discovered,
            OwnershipPolicy::Disabled,
            &|session_id| {
                if session_id == "gone" {
                    return Err(KeeperSessionError::NotKeeperOwned {
                        session_id: session_id.to_string(),
                    });
                }
                Ok(())
            },
            &|| unreachable!("a disable never activates"),
            &|| Ok(()),
        );
        assert_eq!(attachment.attached, vec!["live".to_string()]);
        assert_eq!(attachment.unserved.len(), 1);
        assert_eq!(attachment.unserved[0].0, "gone");
        // The suppression count is what is actually served, not what was discovered.
        assert_eq!(
            outcome.expect("still a success"),
            crate::daemon::keeper_session::ActivationOutcome::Suppressed {
                keeper_sessions: 1
            }
        );
    }

    #[test]
    fn an_enabled_policy_attaches_first_and_then_asks_for_activation() {
        let discovered = vec![view("a", "9.9.9")];
        let order = RefCell::new(Vec::new());
        let (attachment, outcome) = attach_then_apply_policy(
            &discovered,
            OwnershipPolicy::Enabled,
            &|_session_id: &str| {
                order.borrow_mut().push("attach");
                Ok(())
            },
            &|| {
                order.borrow_mut().push("activate");
                Ok(crate::daemon::keeper_session::ActivationOutcome::AlreadyActive)
            },
            &|| unreachable!("an enable never suppresses"),
        );
        assert_eq!(
            order.borrow().to_vec(),
            vec!["attach".to_string(), "activate".to_string()]
        );
        assert_eq!(attachment.attached, vec!["a".to_string()]);
        assert_eq!(
            outcome.expect("activation outcome"),
            crate::daemon::keeper_session::ActivationOutcome::AlreadyActive
        );
    }

    #[test]
    fn a_refused_flag_write_is_typed_and_leaves_the_attached_sessions_in_place() {
        let discovered = vec![view("a", "9.9.9")];
        let (attachment, outcome) = attach_then_apply_policy(
            &discovered,
            OwnershipPolicy::Disabled,
            &|_session_id: &str| Ok(()),
            &|| unreachable!("a disable never activates"),
            &|| Err("permission denied".to_string()),
        );
        assert_eq!(attachment.attached, vec!["a".to_string()]);
        let refusal = outcome.expect_err("the write failed");
        assert!(matches!(
            refusal,
            KeeperStartupError::OwnershipWriteRefused { .. }
        ));
        let text = refusal.to_string();
        assert!(text.contains("KEEPER_OWNERSHIP_WRITE_REFUSED"), "{text}");
        assert!(text.contains("permission denied"), "{text}");
    }

    #[test]
    fn a_truly_fresh_runtime_is_authorized_to_install() {
        let fixture = Fixture::new();
        let store = fixture.store();
        let evidence = fixture.evidence(&store);
        assert_eq!(
            classify_occupancy(Err(io_error(io::ErrorKind::NotFound)), &evidence)
                .expect("fresh install"),
            OccupancyState::NothingInstalled
        );
        assert_eq!(
            classify_occupancy(Err(io_error(io::ErrorKind::ConnectionRefused)), &evidence)
                .expect("fresh install"),
            OccupancyState::NothingInstalled
        );
    }

    #[test]
    fn discovered_sessions_are_counted_per_generation() {
        let fixture = Fixture::new();
        let store = fixture.store();
        let evidence = fixture.evidence(&store);
        let discovered = Ok(vec![
            view("a", "9.9.9"),
            view("b", "9.9.9"),
            view("c", "9.9.8"),
        ]);
        assert_eq!(
            classify_occupancy(discovered, &evidence).expect("known occupancy"),
            OccupancyState::Known(vec![
                ("9.9.8".to_string(), 1),
                ("9.9.9".to_string(), 2),
            ])
        );
    }

    #[test]
    fn a_first_install_bootstraps_once_and_name_the_installed_binary() {
        let fixture = Fixture::new();
        let store = fixture.store();
        let _lock = StartupLock::acquire(&fixture.root()).expect("startup lock");
        let occupancy = classify_occupancy(Err(io_error(io::ErrorKind::NotFound)), &fixture.evidence(&store))
            .expect("fresh install");
        let service = fixture.service();
        let payload = fixture.payload();
        let launchctl = FakeLaunchctl::not_registered();
        let root = fixture.root();
        let install = pass(&service, &root, &payload, &occupancy, &launchctl, true, &_lock);
        let report = run_keeper_install_pass(&install, &|| false).expect("install pass");

        assert_eq!(report.generation.as_deref(), Some("9.9.9"));
        assert_eq!(report.activation, KeeperActivation::Bootstrapped);
        assert!(report.pruned.is_empty(), "{:?}", report.pruned);
        let mutations = launchctl.mutations();
        assert_eq!(mutations.len(), 1, "{mutations:?}");
        assert_eq!(mutations[0][0], "bootstrap");
        assert_eq!(mutations[0][1], "gui/501");
        assert_eq!(mutations[0][2], fixture.plist().to_string_lossy());

        let plist = std::fs::read_to_string(fixture.plist()).expect("plist");
        assert!(plist.contains(&fixture.target("9.9.9").to_string_lossy().into_owned()), "{plist}");
    }

    #[test]
    fn a_live_keeper_is_never_disturbed() {
        let fixture = Fixture::new();
        let occupancy = OccupancyState::Known(Vec::new());
        let service = fixture.service();
        let payload = fixture.payload();
        let launchctl = FakeLaunchctl::not_registered();
        let root = fixture.root();
        let lock = StartupLock::acquire(&root).expect("startup lock");
        let install = pass(&service, &root, &payload, &occupancy, &launchctl, true, &lock);
        let report = run_keeper_install_pass(&install, &|| true).expect("install pass");
        assert_eq!(report.activation, KeeperActivation::AlreadyLive);
        assert!(report.keeper_live);
        assert!(launchctl.calls().is_empty(), "{:?}", launchctl.calls());
    }

    #[test]
    fn an_isolated_runtime_never_reaches_launchctl_even_with_no_live_keeper() {
        let fixture = Fixture::new();
        let occupancy = OccupancyState::Known(Vec::new());
        let service = fixture.service();
        let payload = fixture.payload();
        let launchctl = FakeLaunchctl::not_registered();
        let root = fixture.root();
        let lock = StartupLock::acquire(&root).expect("startup lock");
        let install = pass(&service, &root, &payload, &occupancy, &launchctl, false, &lock);
        let report = run_keeper_install_pass(&install, &|| false).expect("install pass");
        assert!(matches!(report.activation, KeeperActivation::Skipped { .. }));
        assert!(!report.activation.start_issued());
        assert!(launchctl.calls().is_empty(), "{:?}", launchctl.calls());
    }

    #[test]
    fn a_running_job_is_left_alone() {
        let fixture = Fixture::new();
        fixture.plant_generation("9.9.9");
        std::fs::write(
            fixture.plist(),
            fixture.service().render(&fixture.target("9.9.9"), true),
        )
        .expect("plist");
        let launchctl = FakeLaunchctl::running(4242);
        let activation = activate_keeper(
            &fixture.service(),
            &fixture.target("9.9.9"),
            501,
            &launchctl,
            &|| false,
        );
        assert_eq!(
            activation,
            KeeperActivation::LeftAlone {
                state: "running (pid 4242)".to_string()
            }
        );
        assert_eq!(launchctl.mutations().len(), 0);
    }

    #[test]
    fn an_unverifiable_job_is_left_alone() {
        let fixture = Fixture::new();
        fixture.plant_generation("9.9.9");
        std::fs::write(
            fixture.plist(),
            fixture.service().render(&fixture.target("9.9.9"), true),
        )
        .expect("plist");
        let launchctl = FakeLaunchctl::unverifiable();
        let activation = activate_keeper(
            &fixture.service(),
            &fixture.target("9.9.9"),
            501,
            &launchctl,
            &|| false,
        );
        assert!(matches!(activation, KeeperActivation::LeftAlone { .. }));
        assert_eq!(launchctl.mutations().len(), 0);
    }

    #[test]
    fn an_idle_job_is_kickstarted_without_a_kill_flag() {
        let fixture = Fixture::new();
        fixture.plant_generation("9.9.9");
        std::fs::write(
            fixture.plist(),
            fixture.service().render(&fixture.target("9.9.9"), true),
        )
        .expect("plist");
        let launchctl = FakeLaunchctl::idle();
        let activation = activate_keeper(
            &fixture.service(),
            &fixture.target("9.9.9"),
            501,
            &launchctl,
            &|| false,
        );
        assert_eq!(activation, KeeperActivation::Kickstarted);
        let mutations = launchctl.mutations();
        assert_eq!(mutations.len(), 1, "{mutations:?}");
        assert_eq!(
            mutations[0],
            vec!["kickstart", "gui/501/com.ferryx.qa.ptykeeper"]
        );
        assert!(!mutations[0].iter().any(|arg| arg == "-k"));
    }

    #[test]
    fn a_plist_naming_another_binary_is_never_bootstrapped() {
        let fixture = Fixture::new();
        std::fs::write(
            fixture.plist(),
            fixture.service().render(Path::new("/opt/other/keeper"), true),
        )
        .expect("plist");
        let launchctl = FakeLaunchctl::not_registered();
        let activation = activate_keeper(
            &fixture.service(),
            &fixture.target("9.9.9"),
            501,
            &launchctl,
            &|| false,
        );
        assert_eq!(
            activation,
            KeeperActivation::PlistMismatch {
                expected: fixture.target("9.9.9"),
                found: Some(PathBuf::from("/opt/other/keeper")),
            }
        );
        assert!(launchctl.calls().is_empty(), "{:?}", launchctl.calls());
    }

    #[test]
    fn an_occupied_generation_is_neither_pruned_nor_repointed() {
        let fixture = Fixture::new();
        fixture.plant_generation("9.9.8");
        let plist_before = fixture.service().render(&fixture.target("9.9.8"), true);
        std::fs::write(fixture.plist(), &plist_before).expect("plist");
        let occupancy = OccupancyState::Known(vec![("9.9.8".to_string(), 3)]);
        let payload = fixture.payload();
        let service = fixture.service();
        let launchctl = FakeLaunchctl::not_registered();
        let root = fixture.root();
        let lock = StartupLock::acquire(&root).expect("startup lock");
        let install = pass(&service, &root, &payload, &occupancy, &launchctl, true, &lock);
        let error = run_keeper_install_pass(&install, &|| false)
            .expect_err("an occupied generation must refuse the repoint");
        assert!(
            matches!(
                error,
                KeeperStartupError::Service(KeeperServiceError::KeeperOccupied { ref version, sessions: 3 })
                    if version == "9.9.8"
            ),
            "{error:?}"
        );
        assert_eq!(
            std::fs::read_to_string(fixture.plist()).expect("plist"),
            plist_before
        );
        assert!(fixture.root().join("9.9.8").is_dir());
        assert!(launchctl.calls().is_empty(), "{:?}", launchctl.calls());
    }

    #[test]
    fn retention_keeps_an_occupied_generation_outside_the_window() {
        let fixture = Fixture::new();
        for version in ["9.9.9", "9.9.8", "9.9.7", "9.9.6", "9.9.5"] {
            fixture.plant_generation(version);
        }
        let occupancy = OccupancyState::Known(vec![("9.9.5".to_string(), 2)]);
        let report = keeper_launchd::prune_keeper_generations(&fixture.root(), &[], |generation| {
            occupancy.sessions_for(generation)
        })
        .expect("prune");
        assert_eq!(report.removed, vec!["9.9.6".to_string()]);
        assert_eq!(report.preserved_occupied, vec!["9.9.5".to_string()]);
        assert!(fixture.root().join("9.9.5").is_dir());
    }

    #[test]
    fn the_startup_lock_refuses_a_second_pass_and_leaves_the_tree_untouched() {
        let fixture = Fixture::new();
        fixture.plant_generation("9.9.8");
        let first = StartupLock::acquire(&fixture.root()).expect("first lock");
        let second = StartupLock::acquire(&fixture.root());
        assert!(
            matches!(second, Err(KeeperStartupError::OccupancyUnknown { .. })),
            "a concurrent pass must refuse rather than decide occupancy"
        );
        assert!(fixture.root().join("9.9.8").is_dir());
        assert!(!fixture.plist().exists());
        drop(first);
        assert!(StartupLock::acquire(&fixture.root()).is_ok());
    }
}
