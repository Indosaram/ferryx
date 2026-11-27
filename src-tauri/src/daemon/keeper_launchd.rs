//! macOS `launchd` LaunchAgent for the PTY keeper: a job independent of the daemon's own.
//!
//! The keeper owns every PTY master descriptor, so its lifetime must be independent of both the GUI
//! and the daemon. The daemon's agent ([`crate::daemon::launchd`]) cannot own it: that job's
//! `ProgramArguments` name the crash supervisor, and its `KeepAlive { SuccessfulExit = false }`
//! exists for the daemon's own deliberate `exit(0)` paths. A keeper parented by the daemon would be
//! torn down by exactly the crash it is meant to survive, so this module writes a second, separate
//! job that runs the keeper binary directly and keeps only *it* alive.
//!
//! `KeepAlive { SuccessfulExit = false }` is the right condition here too, for a different reason:
//! the keeper exits **0** on purpose when a second keeper already holds this runtime's instance
//! lock (`crates/ferryx-ptykeeper/src/main.rs`), and an unconditional `KeepAlive` would relaunch it
//! into that lock forever. An abnormal death (SIGSEGV, OOM kill, a failed bind) is the case that
//! must restart, but a restart does **not** recover what died with it: the keeper holds the PTY
//! masters, so its death (or a host reboot) ends those children and a relaunched keeper cannot
//! re-adopt them (plan, "Explicit exclusions"). The restart restores the *service* for sessions
//! created afterwards; it never restores the sessions the previous keeper owned.
//!
//! The job's `ProgramArguments` hold the keeper binary and nothing else. In particular they never
//! pass `--socket`: the keeper derives its endpoint from the runtime directory
//! (`crates/ferryx-ptykeeper/src/endpoint.rs::runtime_dir`, `$FERRYX_RUNTIME_DIR` else
//! `/tmp/ferryx-{uid}`), and the daemon's presence probe looks for
//! [`crate::daemon::crash_supervisor::KEEPER_SOCKET_NAME`] in that same directory. Freezing a path
//! into the plist would let the two disagree, and would point an isolated runtime's keeper at the
//! user's socket.
//!
//! Nothing here loads, unloads, bootstraps or kicks a job, and nothing here spawns `launchctl`: it
//! only renders and writes files, which launchd reads at the next login.
//!
//! The keeper binary is never the app bundle's executable. It lives in an immutable
//! `~/.ferryx/keeper/<version>/` generation outside the bundle (`crates/ferryx-ptykeeper/src/install.rs`),
//! the last [`RETAINED_GENERATIONS`] of them are kept, and a generation is deleted only while it
//! holds no sessions. The keeper's `InstallDir` cannot be borrowed from this crate (the keeper is a
//! separate binary crate with no dependency edge from `src-tauri`), so the resolution rules below
//! mirror that module's layout, ordering and version grammar; [`resolve_keeper_generation`] never
//! removes, replaces or prunes a generation, which is what makes an incompatible keeper safe to
//! meet.
//!
//! Three file-only operations complete that mirror, so the daemon never has to grow a second copy of
//! the install rules: [`install_keeper_generation`] plants one immutable generation,
//! [`prune_keeper_generations`] applies retention (never deleting an occupied or currently
//! programmed generation), and [`ensure_keeper_service`] runs both and points this job's plist at
//! the result. [`rollback_gate`] is the other half of the plan's ownership contract: a rollback to a
//! daemon build without keeper support is refused, with a typed error naming the live keeper-owned
//! session count, while any keeper owns a session.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::daemon::launchd::{self, generation};

/// Name of the install-tree lock file. A dotfile, so `generations_in` never mistakes it for a
/// generation (it is a file, not a directory, but the name rule keeps the tree unambiguous).
pub const INSTALL_LOCK_NAME: &str = ".keeper-install.lock";

/// Label of the production keeper job. Distinct from [`launchd::PLIST_LABEL`]
/// (`com.ferryx.daemon`): one launchd job, one lifecycle.
pub const PLIST_LABEL: &str = "com.ferryx.ptykeeper";

/// Label of the keeper job that isolated runtimes and QA builds install. It can never collide with
/// the production label, so a fixture run cannot repoint the user's login-time keeper.
pub const QA_PLIST_LABEL: &str = "com.ferryx.qa.ptykeeper";

/// Overrides the keeper plist path, so QA/fixture runs never touch the real LaunchAgent. The
/// daemon's own override (a different variable) never redirects this job.
pub const PLIST_PATH_ENV: &str = "FERRYX_PTY_KEEPER_PLIST_PATH";

/// Binary name inside each generation directory
/// (`crates/ferryx-ptykeeper/src/install.rs::BINARY_NAME`).
pub const BINARY_NAME: &str = "ferryx-ptykeeper";

/// Optional sidecar naming the installed binary's SHA-256
/// (`crates/ferryx-ptykeeper/src/install.rs::DIGEST_NAME`), so an existing generation can be proven
/// identical instead of merely believed.
pub const DIGEST_NAME: &str = "binary.sha256";

/// Owner label a keeper-owned session reports in `OWNERSHIP_LIST`
/// (`ferryx_ptykeeper::session::SessionOwner::as_str`). The protocol major stays where the client
/// that negotiates it lives ([`crate::daemon::keeper_client`]) rather than being restated here.
pub const KEEPER_OWNER_LABEL: &str = "keeper";

/// Owner label a daemon-owned (direct) session reports, from the same source.
pub const DAEMON_OWNER_LABEL: &str = "daemon";

/// The generation directory name this build installs under: its own app version.
///
/// A generation name is an identity, so it must change per release; a constant keeper-crate version
/// (`0.1.0`) would collide with the previous release's directory and trip the immutable-bytes
/// refusal on every upgrade. The app version has three numeric components and therefore satisfies
/// [`looks_like_version`], which also keeps retention ordering aligned with release order.
pub fn keeper_generation_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Overrides the keeper install root, else `$HOME/.ferryx/keeper`
/// (`crates/ferryx-ptykeeper/src/install.rs::default_install_root`).
pub const INSTALL_ROOT_ENV: &str = "FERRYX_KEEPER_DIR";

/// How many generations the installer retains
/// (`crates/ferryx-ptykeeper/src/install.rs::RETAINED_GENERATIONS`). Resolution reads it only to
/// report the window; it decides nothing about compatibility.
pub const RETAINED_GENERATIONS: usize = 3;

/// Why this process may not install the production keeper job here.
///
/// Typed rather than a panic or an ignored string so the daemon can report the refusal and leave
/// whatever keeper is already registered untouched.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeeperServiceError {
    /// The job's plist path or log directory could not be resolved: no override and no `$HOME`.
    #[error(
        "cannot resolve the keeper service paths: neither the plist override nor HOME names a location"
    )]
    NoServicePaths,
    /// The production label was requested from a dev or isolated runtime.
    #[error(
        "refusing to install the production keeper label {label}: this is a dev or isolated runtime, \
         so the job is installed under the QA label instead"
    )]
    ProductionLabelRefused { label: String },
    /// The plist could not be written. The job is left exactly as it was.
    #[error("cannot write the keeper service plist: {detail}")]
    Write { detail: String },
    /// No install tree is reachable: neither the keeper directory override nor `$HOME` is set.
    #[error("cannot resolve the keeper install root: neither the keeper directory override nor HOME is set")]
    NoInstallRoot,
    /// The install tree exists but cannot be read.
    #[error("cannot read the keeper install tree {root}: {detail}")]
    UnreadableInstallTree { root: String, detail: String },
    /// Installed generations exist, but none of them can serve this daemon. Nothing was removed:
    /// the incompatible keeper and the sessions it owns are preserved, as the install rules require.
    #[error(
        "no keeper generation under {root} can serve this daemon (retained window {retained} \
         generation(s), installed: {installed:?}, rejected: {rejected:?}); every generation is \
         preserved and none is replaced"
    )]
    NoCompatibleGeneration {
        root: String,
        retained: usize,
        installed: Vec<String>,
        rejected: Vec<KeeperIncompatibility>,
    },
    /// A generation name is not a version, so it would not sort or be retained like one.
    #[error(
        "refusing to install a keeper generation under the non-version directory name {version:?}"
    )]
    NotAVersion { version: String },
    /// The binary to install could not be read.
    #[error("cannot read the keeper binary to install at {path}: {detail}")]
    SourceUnreadable { path: String, detail: String },
    /// The generation exists with different bytes. It is never replaced: one version string is one
    /// identity, so two binaries behind it would make "which keeper owns this session" unanswerable.
    #[error(
        "keeper generation {version} already exists at {dir} with different bytes; a generation is \
         immutable and is never replaced"
    )]
    GenerationImmutable { version: String, dir: String },
    /// Writing the generation or removing a pruned one failed. Nothing in place was disturbed.
    #[error("cannot update the keeper install tree: {detail}")]
    Install { detail: String },
    /// The generation's binary exists but cannot be read, so its bytes cannot be shown to match.
    /// Refusing is the only safe answer: writing anyway would replace a generation this process
    /// cannot even inspect.
    #[error(
        "keeper generation {version} exists at {dir} but its binary is unreadable: {detail}; \
         a generation is immutable and is never replaced unseen"
    )]
    ExistingUnreadable {
        version: String,
        dir: String,
        detail: String,
    },
    /// The existing plist could not be read, so which generation it programs is unknown. Refused
    /// rather than treated as absent: repointing blind could replace an occupied keeper.
    #[error(
        "cannot read the keeper service plist at {path}: {detail}; the programmed generation is \
         unknown, so the job is left as it is"
    )]
    PlistUnreadable { path: String, detail: String },
    /// The job still points at a generation that owns sessions, so it is not repointed.
    #[error(
        "refusing to repoint the keeper job: generation {version} still owns {sessions} session(s), \
         and an occupied keeper is never replaced; the job is left as it was"
    )]
    KeeperOccupied { version: String, sessions: usize },
}

/// Why one installed generation cannot serve this daemon.
///
/// Mirrors the shape - not the type - of
/// `ferryx_ptykeeper::install::IncompatibleGeneration`, which this crate cannot link. The variants
/// are the ones a resolver can decide without the keeper handshake; the handshake itself is the
/// Phase 3 seam named in [`crate::daemon::crash_supervisor::KeeperSocketProbe`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeeperIncompatibility {
    /// The generation speaks a protocol major outside the daemon's window.
    ProtocolMajor {
        version: String,
        major: u16,
        minor: u16,
        supported_major: u16,
    },
    /// The generation is unreadable or unusable, and says why.
    Unknown { version: String, detail: String },
}

/// One generation directory on disk: `<root>/<version>/ferryx-ptykeeper`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeeperGeneration {
    version: String,
    dir: PathBuf,
}

impl KeeperGeneration {
    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn binary(&self) -> PathBuf {
        self.dir.join(BINARY_NAME)
    }
}

/// Everything the keeper's job needs that is not a side effect: its label, where its plist lives,
/// where its raw stdout/stderr go, and the isolation variables a QA job forwards.
///
/// Shaped like [`generation::GenerationJob`] on purpose, so a reader moving between the daemon's job
/// and the keeper's sees the same fields and the same `render` step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeeperService {
    pub label: String,
    pub plist_path: PathBuf,
    pub log_dir: PathBuf,
    /// `EnvironmentVariables` of the job: empty for the production label, the isolation variables
    /// for the QA label, so a QA keeper binds the QA runtime's socket and never the user's.
    pub environment: Vec<(String, String)>,
}

impl KeeperService {
    /// Renders this job's plist for `executable_path`, the immutable generation's binary.
    pub fn render(&self, executable_path: &Path, run_at_load: bool) -> String {
        generation::with_environment(
            generate_keeper_launchd_plist(
                &self.label,
                &executable_path.to_string_lossy(),
                &self.log_dir,
                run_at_load,
            ),
            &self.environment,
        )
    }
}

/// Which label this runtime must use: the production one, or the QA one that cannot collide with it.
///
/// Same three-signal policy as the daemon's (`launchd::agent_management_supported_for`): a debug
/// build and any `FERRYX_DATA_DIR`/`FERRYX_RUNTIME_DIR`/`FERRYX_SESSION_DIR` override mark a
/// throwaway fixture, so they get the QA label instead of the user's login-time job.
pub fn label_for(dev_runtime: bool, isolated_runtime: bool) -> &'static str {
    if launchd::agent_management_supported_for(dev_runtime, isolated_runtime) {
        PLIST_LABEL
    } else {
        QA_PLIST_LABEL
    }
}

/// Pure plist-path resolution, the same shape as [`launchd::launchd_plist_path_for`]: the override
/// is the plist path itself, otherwise `$HOME/Library/LaunchAgents/<label>.plist`. Different labels
/// therefore produce different files, so a QA install never rewrites the production one.
pub fn keeper_plist_path_for(
    label: &str,
    override_path: Option<&OsStr>,
    home: Option<&OsStr>,
) -> Option<PathBuf> {
    if let Some(override_path) = override_path {
        let override_path = PathBuf::from(override_path);
        return (!override_path.as_os_str().is_empty()).then_some(override_path);
    }
    home.map(|home| {
        PathBuf::from(home)
            .join("Library/LaunchAgents")
            .join(format!("{label}.plist"))
    })
}

/// [`keeper_plist_path_for`] for this process. `None` on every non-macOS target, like the daemon's.
#[cfg(target_os = "macos")]
pub fn get_keeper_plist_path(label: &str) -> Option<PathBuf> {
    keeper_plist_path_for(
        label,
        std::env::var_os(PLIST_PATH_ENV).as_deref(),
        std::env::var_os("HOME").as_deref(),
    )
}

#[cfg(not(target_os = "macos"))]
pub fn get_keeper_plist_path(_label: &str) -> Option<PathBuf> {
    None
}

/// The keeper's raw stdout/stderr directory. The same private directory as the daemon's (never
/// `/tmp`, which another user can pre-create as a symlink) but with its own file names: two
/// processes must not interleave into one launchd capture file.
pub fn keeper_log_dir_for(home: Option<&OsStr>) -> Option<PathBuf> {
    launchd::launchd_log_dir_for(home)
}

/// Builds the job for `label` in this runtime, and refuses the production label where it would
/// repoint the user's own keeper.
///
/// `env` is injected rather than read so the policy is testable without mutating the process
/// environment, exactly as the daemon's supervisor facade does it.
pub fn keeper_service_with_label(
    label: &str,
    dev_runtime: bool,
    isolated_runtime: bool,
    env: &dyn Fn(&str) -> Option<OsString>,
) -> Result<KeeperService, KeeperServiceError> {
    let supported = launchd::agent_management_supported_for(dev_runtime, isolated_runtime);
    if label == PLIST_LABEL && !supported {
        return Err(KeeperServiceError::ProductionLabelRefused {
            label: label.to_string(),
        });
    }
    let plist_path = keeper_plist_path_for(
        label,
        env(PLIST_PATH_ENV).as_deref(),
        env("HOME").as_deref(),
    )
    .ok_or(KeeperServiceError::NoServicePaths)?;
    let log_dir = keeper_log_dir_for(env("HOME").as_deref())
        .ok_or(KeeperServiceError::NoServicePaths)?;
    Ok(KeeperService {
        label: label.to_string(),
        plist_path,
        log_dir,
        environment: keeper_job_environment(label, env),
    })
}

/// The `EnvironmentVariables` of a keeper job.
///
/// A LaunchAgent inherits no shell environment, and the keeper finds its socket through
/// `$FERRYX_RUNTIME_DIR`; without it an isolated keeper would bind `/tmp/ferryx-<uid>/keeper.sock`
/// and fight the user's own keeper for the very socket the isolated daemon probes. So **every**
/// label but the production one forwards the isolation variables, not only [`QA_PLIST_LABEL`]: an
/// acceptance run that mints its own per-run private label must get the same protection as the QA
/// label, and a label this module does not recognise is still not the user's. The production label
/// carries no `EnvironmentVariables` at all and lets the keeper derive the endpoint.
///
/// The key list is the daemon's [`generation::QA_JOB_ENV`], so the two jobs can never forward
/// different sets.
fn keeper_job_environment(
    label: &str,
    env: &dyn Fn(&str) -> Option<OsString>,
) -> Vec<(String, String)> {
    if label == PLIST_LABEL {
        return Vec::new();
    }
    generation::QA_JOB_ENV
        .iter()
        .copied()
        .filter_map(|key| {
            env(key)
                .and_then(|value| value.into_string().ok())
                .filter(|value| !value.is_empty() && !value.contains(['\n', '\r']))
                .map(|value| (key.to_string(), value))
        })
        .collect()
}

/// Whether `label` names one of the shipped jobs, which no private run may take over.
///
/// The production labels are the user's own login-time jobs: `com.ferryx.daemon`
/// ([`launchd::PLIST_LABEL`]), the pre-rebrand `com.rorca.daemon`
/// ([`launchd::RORCA_PLIST_LABEL`]), and this module's [`PLIST_LABEL`]. Nothing outside a user's
/// explicit install may write, bootstrap or kick one of them; an acceptance run must mint its own
/// label instead, and this function is how it proves that before touching `launchctl`.
pub fn is_private_label(label: &str) -> bool {
    !matches!(
        label,
        PLIST_LABEL | QA_PLIST_LABEL | launchd::PLIST_LABEL | launchd::RORCA_PLIST_LABEL
    )
}

/// Which one of this runtime's labels a job may be installed under.
pub fn keeper_service_for(
    dev_runtime: bool,
    isolated_runtime: bool,
    env: &dyn Fn(&str) -> Option<OsString>,
) -> Result<KeeperService, KeeperServiceError> {
    keeper_service_with_label(
        label_for(dev_runtime, isolated_runtime),
        dev_runtime,
        isolated_runtime,
        env,
    )
}

/// Renders the keeper's LaunchAgent.
///
/// `ProgramArguments` name the keeper binary of an immutable generation and nothing else: no
/// `--daemon`, no `--daemon-crash-supervisor`, and no `--socket`, because the endpoint must stay
/// derived from the runtime (`crash_supervisor::KEEPER_SOCKET_NAME`).
///
/// `RunAtLoad` starts the keeper at login. `KeepAlive { SuccessfulExit = false }` restarts it after
/// an abnormal death - the case that loses sessions - and leaves its deliberate `exit(0)` (a second
/// keeper that found the instance lock held) alone. `AbandonProcessGroup` stops launchd from
/// signalling the PTY children when the job's main process exits, and `ProcessType Interactive`
/// keeps a terminal host from being throttled like a background job. The soft descriptor limit is
/// raised because a keeper holds several descriptors per session and dying of EMFILE would strand
/// them; the process raises it again at startup against the hard ceiling.
pub fn generate_keeper_launchd_plist(
    label: &str,
    executable_path: &str,
    log_dir: &Path,
    run_at_load: bool,
) -> String {
    let stdout_path = log_dir.join("keeper.stdout.log");
    let stderr_path = log_dir.join("keeper.stderr.log");
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

/// Writes this job's plist only when its content differs, and reports whether it was rewritten.
///
/// Delegates to [`generation::write_plist_if_changed`]: the same rule and the same reason (a GUI
/// start must not rewrite the file on every run) plus the same directory creation, because launchd
/// does not create a missing `StandardOutPath` parent and a job whose output cannot be opened does
/// not run.
pub fn write_keeper_launchd_plist(
    service: &KeeperService,
    executable_path: &Path,
    run_at_load: bool,
) -> Result<bool, KeeperServiceError> {
    let content = service.render(executable_path, run_at_load);
    generation::write_plist_if_changed(&content, &service.log_dir, &service.plist_path).map_err(
        |detail| KeeperServiceError::Write { detail },
    )
}

/// `$FERRYX_KEEPER_DIR`, else `$HOME/.ferryx/keeper`: the same rule and the same precedence as
/// `ferryx_ptykeeper::install::default_install_root`, so the daemon and a keeper never disagree
/// about where generations live.
pub fn default_keeper_install_root_for(
    keeper_dir: Option<&OsStr>,
    home: Option<&OsStr>,
) -> Option<PathBuf> {
    if let Some(dir) = keeper_dir {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir));
        }
    }
    let home = home.filter(|home| !home.is_empty())?;
    Some(PathBuf::from(home).join(".ferryx").join("keeper"))
}

/// [`default_keeper_install_root_for`] for this process.
pub fn get_keeper_install_root() -> Option<PathBuf> {
    default_keeper_install_root_for(
        std::env::var_os(INSTALL_ROOT_ENV).as_deref(),
        std::env::var_os("HOME").as_deref(),
    )
}

/// A version directory is `<major>.<minor>.<patch>` with optional `-pre`/`+build` suffixes;
/// anything else in the install root (logs, `.partial` leftovers) is not a generation. Mirrors
/// `ferryx_ptykeeper::install::looks_like_version`.
fn looks_like_version(name: &str) -> bool {
    let core = name.split(['-', '+']).next().unwrap_or("");
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

/// Whether `version` is a version string that can be joined onto an install root **safely**.
///
/// [`looks_like_version`] alone is not enough for that: it validates only the numeric prefix, so
/// `1.2.3/../../etc` and `1.2.3-../../x` both pass it and then escape the root when joined. The
/// version reaches the installer from `CARGO_PKG_VERSION` today, which makes the escape unreachable
/// in practice -- but both functions are `pub` and take a caller's string, so the boundary is
/// validated here rather than trusted from the caller.
///
/// The rule is the whole string, not a prefix: exactly one normal path component, with a numeric
/// `major.minor.patch` core and a suffix made only of the characters a semver pre-release/build may
/// contain. Deliberately separate from [`looks_like_version`], which stays the traversal rule for
/// names already on disk.
fn is_safe_version(version: &str) -> bool {
    if version.is_empty() || version == "." || version == ".." {
        return false;
    }
    // A separator of any kind would make the join escape or descend, and a leading dot could name
    // a hidden directory the traversal rules skip.
    if version.starts_with('.')
        || version.contains('/')
        || version.contains('\\')
        || version.contains('\0')
    {
        return false;
    }
    if version.contains("..") {
        return false;
    }
    looks_like_version(version) && version_suffix_is_safe(version)
}

/// Whether everything after the numeric core is a semver pre-release/build suffix.
fn version_suffix_is_safe(version: &str) -> bool {
    let mut seen = 0usize;
    for part in version.split(['-', '+']) {
        seen += 1;
        if seen == 1 {
            continue;
        }
        if part.is_empty() {
            return false;
        }
        if !part
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-')
        {
            return false;
        }
    }
    true
}

/// Orders two versions by numeric components, then lexically for stability across pre-release
/// suffixes. Mirrors `ferryx_ptykeeper::install::compare_versions`, which retention depends on.
fn compare_versions(left: &str, right: &str) -> std::cmp::Ordering {
    let split = |text: &str| -> (Vec<u64>, String) {
        let mut suffix = String::new();
        let mut numbers = Vec::new();
        for (index, part) in text.split(['-', '+']).enumerate() {
            if index == 0 {
                for component in part.split('.') {
                    numbers.push(component.parse::<u64>().unwrap_or(0));
                }
            } else {
                suffix.push('-');
                suffix.push_str(part);
            }
        }
        (numbers, suffix)
    };
    let (left_numbers, left_suffix) = split(left);
    let (right_numbers, right_suffix) = split(right);
    left_numbers
        .cmp(&right_numbers)
        .then_with(|| left_suffix.cmp(&right_suffix))
}

/// Every generation under `root`, newest first, as `ferryx_ptykeeper::install::InstallDir::generations`
/// lists them: directories (and symlinks to them) whose name is a version. A missing root is an
/// empty tree, not an error - a user who has never installed a keeper has no keeper job yet.
fn generations_in(root: &Path) -> Result<Vec<KeeperGeneration>, KeeperServiceError> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(KeeperServiceError::UnreadableInstallTree {
                root: root.display().to_string(),
                detail: error.to_string(),
            })
        }
    };
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| KeeperServiceError::UnreadableInstallTree {
            root: root.display().to_string(),
            detail: error.to_string(),
        })?;
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(_) => continue,
        };
        if !file_type.is_dir() && !file_type.is_symlink() {
            continue;
        }
        let version = entry.file_name().to_string_lossy().into_owned();
        if !looks_like_version(&version) {
            continue;
        }
        found.push(KeeperGeneration {
            version,
            dir: entry.path(),
        });
    }
    found.sort_by(|left, right| compare_versions(&right.version, &left.version));
    Ok(found)
}

/// The newest generation under `root` that `compatible` accepts, in the order
/// `ferryx_ptykeeper::install::InstallDir::newest_compatible` walks them: an incompatible generation
/// is skipped **and preserved**, and an older compatible one is a valid answer.
///
/// When nothing is acceptable the error is typed and lists what is installed and why each candidate
/// was rejected, so the daemon can report the incompatibility. Nothing is removed, replaced or
/// pruned: the keeper holding sessions keeps them.
pub fn resolve_keeper_generation(
    root: &Path,
    compatible: impl Fn(&KeeperGeneration) -> Result<(), KeeperIncompatibility>,
) -> Result<KeeperGeneration, KeeperServiceError> {
    let generations = generations_in(root)?;
    let installed: Vec<String> = generations.iter().map(|g| g.version().to_string()).collect();
    let mut rejected = Vec::new();
    for candidate in generations {
        match compatible(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(reason) => rejected.push(reason),
        }
    }
    Err(KeeperServiceError::NoCompatibleGeneration {
        root: root.display().to_string(),
        retained: RETAINED_GENERATIONS,
        installed,
        rejected,
    })
}

/// Accepts a generation whose binary is present and executable.
///
/// This is the strongest claim decidable from the install tree alone. The keeper protocol
/// handshake (major/minor, attach epoch) is the Phase 3 seam named in
/// [`crate::daemon::crash_supervisor::KeeperSocketProbe`]; until it exists this predicate neither
/// asserts nor fakes a version, which is why [`KeeperIncompatibility::ProtocolMajor`] is a caller's
/// decision and not invented here.
pub fn accept_installed_generation(
    generation: &KeeperGeneration,
) -> Result<(), KeeperIncompatibility> {
    let binary = generation.binary();
    let metadata = fs::metadata(&binary).map_err(|error| KeeperIncompatibility::Unknown {
        version: generation.version.clone(),
        detail: format!("cannot read {}: {error}", binary.display()),
    })?;
    if !metadata.is_file() {
        return Err(KeeperIncompatibility::Unknown {
            version: generation.version.clone(),
            detail: format!("{} is not a file", binary.display()),
        });
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(KeeperIncompatibility::Unknown {
                version: generation.version.clone(),
                detail: format!("{} is not executable", binary.display()),
            });
        }
    }
    Ok(())
}

/// The newest installed generation with a runnable binary, for the install root of this user.
///
/// Separate from [`resolve_keeper_generation`] so a caller that does know the keeper's protocol can
/// pass its own predicate instead.
pub fn resolve_installed_keeper() -> Result<KeeperGeneration, KeeperServiceError> {
    let root = get_keeper_install_root().ok_or(KeeperServiceError::NoInstallRoot)?;
    resolve_keeper_generation(&root, accept_installed_generation)
}

/// Manifest key that records the oldest keeper-capable version a user may roll back to. The release
/// assembly writes it and the runtime reads it, so both lanes name it once.
pub const OLDEST_SAFE_ROLLBACK_KEY: &str = "oldestSafeRollbackVersion";

/// What [`install_keeper_generation`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeeperInstallOutcome {
    Installed(KeeperGeneration),
    AlreadyPresent(KeeperGeneration),
}

impl KeeperInstallOutcome {
    pub fn generation(&self) -> &KeeperGeneration {
        match self {
            Self::Installed(generation) | Self::AlreadyPresent(generation) => generation,
        }
    }
}

/// What [`prune_keeper_generations`] did. `retained` is what exists afterwards, newest first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeeperPruneReport {
    pub removed: Vec<String>,
    pub retained: Vec<String>,
    pub preserved_occupied: Vec<String>,
    pub preserved_referenced: Vec<String>,
}

/// Plants `source_binary` as `<root>/<version>/ferryx-ptykeeper`, immutably.
///
/// Identical bytes at that version are a no-op; different bytes are a typed refusal, because a
/// generation name is an identity and two binaries behind one identity make "which keeper owns this
/// session" unanswerable.
///
/// Two independent app instances reach this at startup, so the write is serialized and the publish
/// is atomic *without replacement*:
///
/// * a lock file under the root serializes the whole check-then-write sequence across processes;
/// * the staging file is unique per installer (pid plus a process-wide counter) and created with
///   `create_new`, so two installers can never share -- or mutate -- one staging inode;
/// * the publish is a `hard_link`, which fails with `EEXIST` instead of overwriting. A plain
///   `rename` replaced whatever was there, so a loser of the race silently destroyed the winner's
///   freshly published binary. The winner is then compared byte-for-byte, so a genuine content
///   conflict still surfaces as [`KeeperServiceError::GenerationImmutable`].
pub fn install_keeper_generation(
    root: &Path,
    version: &str,
    source_binary: &Path,
    digest: Option<&str>,
) -> Result<KeeperInstallOutcome, KeeperServiceError> {
    if !is_safe_version(version) {
        return Err(KeeperServiceError::NotAVersion {
            version: version.to_string(),
        });
    }
    let incoming = fs::read(source_binary).map_err(|error| KeeperServiceError::SourceUnreadable {
        path: source_binary.display().to_string(),
        detail: error.to_string(),
    })?;
    // Taken before the existence check: the check and the publish are one decision, and an
    // unlocked pair lets two installers both observe "absent" and both try to publish.
    let _lock = InstallLock::acquire(root)?;
    let dir = root.join(version);
    let binary = dir.join(BINARY_NAME);
    let generation = KeeperGeneration {
        version: version.to_string(),
        dir: dir.clone(),
    };

    match fs::read(&binary) {
        Ok(existing) if existing == incoming => {
            return Ok(KeeperInstallOutcome::AlreadyPresent(generation))
        }
        Ok(_) => {
            return Err(KeeperServiceError::GenerationImmutable {
                version: version.to_string(),
                dir: dir.display().to_string(),
            })
        }
        // Only NotFound means absent: any other read error must not fall through to the write.
        Err(error) if error.kind() != io::ErrorKind::NotFound => {
            return Err(KeeperServiceError::ExistingUnreadable {
                version: version.to_string(),
                dir: dir.display().to_string(),
                detail: error.to_string(),
            })
        }
        Err(_) => {}
    }

    create_private_dir(&dir)?;
    let staging = staging_path(&dir);
    let write = (|| -> io::Result<()> {
        use std::io::Write as _;
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o755);
        }
        let mut file = options.open(&staging)?;
        file.write_all(&incoming)?;
        file.sync_all()
    })();
    if let Err(error) = write {
        let _ = fs::remove_file(&staging);
        return Err(KeeperServiceError::Install {
            detail: format!("cannot stage {}: {error}", binary.display()),
        });
    }
    let _ = set_mode(&staging, 0o755);
    let published = fs::hard_link(&staging, &binary);
    let _ = fs::remove_file(&staging);
    match published {
        Ok(()) => {}
        // `EEXIST`: another installer published between the read above and this link. The lock
        // makes that unreachable inside this process family, but the publish is still written to
        // be safe against a writer that does not take it. Identical bytes are the same no-op the
        // read would have returned; different bytes are the immutable-generation refusal.
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            match fs::read(&binary) {
                Ok(existing) if existing == incoming => {
                    return Ok(KeeperInstallOutcome::AlreadyPresent(generation))
                }
                _ => {
                    return Err(KeeperServiceError::GenerationImmutable {
                        version: version.to_string(),
                        dir: dir.display().to_string(),
                    })
                }
            }
        }
        Err(error) => {
            return Err(KeeperServiceError::Install {
                detail: format!("cannot plant {}: {error}", binary.display()),
            })
        }
    }
    if let Some(digest) = digest {
        fs::write(dir.join(DIGEST_NAME), format!("{}\n", digest.trim().to_ascii_lowercase())).map_err(
            |error| KeeperServiceError::Install {
                detail: format!("cannot record the digest in {}: {error}", dir.display()),
            },
        )?;
    }
    Ok(KeeperInstallOutcome::Installed(generation))
}

/// A staging name no other installer can produce: pid plus a process-wide counter.
fn staging_path(dir: &Path) -> PathBuf {
    static STAGING_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let ticket = STAGING_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    dir.join(format!(
        "{BINARY_NAME}.partial-{}-{ticket}",
        std::process::id()
    ))
}

/// The install-tree lock, held for one `install_keeper_generation` call.
///
/// A separate flock from the startup pass's own lock (which lives in `keeper_startup`): this one
/// guards the generation tree itself, so any writer of `install_keeper_generation` serializes even
/// when it is not a startup pass. A writer that never takes it is not fenced -- the keeper crate's
/// own installer is such a writer today -- but every path in this process is.
struct InstallLock {
    file: fs::File,
}

impl InstallLock {
    fn acquire(root: &Path) -> Result<Self, KeeperServiceError> {
        let fail = |detail: String| KeeperServiceError::Install { detail };
        fs::create_dir_all(root).map_err(|error| {
            fail(format!("cannot create {}: {error}", root.display()))
        })?;
        let path = root.join(INSTALL_LOCK_NAME);
        let file = fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .map_err(|error| fail(format!("cannot open {}: {error}", path.display())))?;
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            // SAFETY: `as_raw_fd` borrows a descriptor from `file`, which stays open and valid for
            // the call; `flock` only sets a lock flag on that descriptor and touches no memory.
            //
            // `EINTR` is retried rather than reported: a signal delivered while this thread is
            // parked in `flock` is not another installer holding the tree, and treating it as one
            // would refuse a healthy install.
            loop {
                let locked = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
                if locked == 0 {
                    break;
                }
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(fail(format!(
                    "cannot lock {}: another installer holds it ({error})",
                    path.display()
                )));
            }
        }
        Ok(Self { file })
    }
}

impl Drop for InstallLock {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            // SAFETY: same descriptor invariant as acquisition; releasing a lock this process holds.
            unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
        }
        let _ = &self.file;
    }
}

/// Applies retention: the newest [`RETAINED_GENERATIONS`] stay, and anything older is removed only
/// when it owns no sessions and is not one of `keep_programs`.
///
/// `occupied` answers "how many sessions does this generation still own". A non-zero answer always
/// wins over the window, so retention can temporarily hold more than N: deleting an occupied
/// generation would strand the sessions whose masters live inside it. `keep_programs` names
/// binaries a launchd plist points at, which must outlive this call even when unoccupied - a job
/// whose program disappeared cannot start the keeper at the next login.
pub fn prune_keeper_generations(
    root: &Path,
    keep_programs: &[PathBuf],
    occupied: impl Fn(&KeeperGeneration) -> usize,
) -> Result<KeeperPruneReport, KeeperServiceError> {
    let generations = generations_in(root)?;
    let mut report = KeeperPruneReport::default();
    for (index, generation) in generations.iter().enumerate() {
        let inside_window = index < RETAINED_GENERATIONS;
        let sessions = occupied(generation);
        let referenced = keep_programs.iter().any(|program| same_path(program, &generation.binary()));
        if inside_window || sessions > 0 || referenced {
            if sessions > 0 && !inside_window {
                report.preserved_occupied.push(generation.version.clone());
            }
            if referenced && !inside_window && sessions == 0 {
                report.preserved_referenced.push(generation.version.clone());
            }
            report.retained.push(generation.version.clone());
            continue;
        }
        match fs::remove_dir_all(generation.dir()) {
            Ok(()) => report.removed.push(generation.version.clone()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(KeeperServiceError::Install {
                    detail: format!(
                        "cannot remove keeper generation {} at {}: {error}",
                        generation.version,
                        generation.dir().display()
                    ),
                })
            }
        }
    }
    Ok(report)
}

/// What [`ensure_keeper_service`] produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeeperServiceOutcome {
    pub generation: KeeperGeneration,
    pub installed: bool,
    pub plist_rewritten: bool,
    pub pruned: KeeperPruneReport,
}

/// Installs this build's generation, applies retention, and points the keeper job at it - all by
/// writing files, never by talking to `launchctl`.
///
/// Safe to call on every start: a generation with identical bytes is a no-op and an unchanged plist
/// is not rewritten. It refuses to repoint the job while the generation the plist currently names
/// still owns sessions, which is the plan's "an occupied keeper is never replaced" rule; the refusal
/// names the count and leaves both the generation and the plist exactly as they were.
pub fn ensure_keeper_service(
    service: &KeeperService,
    root: &Path,
    version: &str,
    source_binary: &Path,
    digest: Option<&str>,
    occupied: &dyn Fn(&KeeperGeneration) -> usize,
    run_at_load: bool,
) -> Result<KeeperServiceOutcome, KeeperServiceError> {
    let current_program = match fs::read_to_string(&service.plist_path) {
        Ok(content) => generation::plist_program(&content).map(PathBuf::from),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(KeeperServiceError::PlistUnreadable {
                path: service.plist_path.display().to_string(),
                detail: error.to_string(),
            })
        }
    };
    // The same boundary check `install_keeper_generation` applies, done here too because this
    // function joins the version onto the root itself (for the plist program and the prune list)
    // before the installer ever sees it.
    if !is_safe_version(version) {
        return Err(KeeperServiceError::NotAVersion {
            version: version.to_string(),
        });
    }
    let target = KeeperGeneration {
        version: version.to_string(),
        dir: root.join(version),
    };

    if let Some(program) = current_program.as_deref() {
        if !same_path(program, &target.binary()) {
            let current = generations_in(root)?
                .into_iter()
                .find(|candidate| same_path(&candidate.binary(), program));
            if let Some(current) = current {
                let sessions = occupied(&current);
                if sessions > 0 {
                    return Err(KeeperServiceError::KeeperOccupied {
                        version: current.version,
                        sessions,
                    });
                }
            }
        }
    }

    let outcome = install_keeper_generation(root, version, source_binary, digest)?;
    let mut keep_programs: Vec<PathBuf> = vec![target.binary()];
    keep_programs.extend(current_program);
    let pruned = prune_keeper_generations(root, &keep_programs, occupied)?;
    let plist_rewritten = write_keeper_launchd_plist(service, &target.binary(), run_at_load)?;

    Ok(KeeperServiceOutcome {
        generation: outcome.generation().clone(),
        installed: matches!(outcome, KeeperInstallOutcome::Installed(_)),
        plist_rewritten,
        pruned,
    })
}

/// Who owns the live sessions, counted from their persisted owner labels.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionOwnership {
    pub keeper_owned: usize,
    pub direct_owned: usize,
    /// Sessions whose owner label this build does not recognize. Kept apart so the rollback gate
    /// can fail closed instead of guessing them into a category.
    pub unknown: usize,
}

impl SessionOwnership {
    pub fn total(&self) -> usize {
        self.keeper_owned + self.direct_owned + self.unknown
    }

    /// Counts `OWNERSHIP_LIST` owner labels (`"keeper"`, `"daemon"`).
    pub fn from_owner_labels<I, S>(labels: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut ownership = Self::default();
        for label in labels {
            match label.as_ref() {
                KEEPER_OWNER_LABEL => ownership.keeper_owned += 1,
                DAEMON_OWNER_LABEL => ownership.direct_owned += 1,
                _ => ownership.unknown += 1,
            }
        }
        ownership
    }
}

/// The build a rollback would install.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RollbackTarget<'a> {
    pub version: &'a str,
    /// Whether that build can discover and service keeper-owned sessions at all.
    pub supports_keeper: bool,
}

/// Why a rollback is refused. Typed, and every variant names what the caller must report.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeeperRollbackRefusal {
    #[error(
        "refusing to roll back to {target_version}: {keeper_owned} session(s) are owned by the \
         keeper, and that build cannot service them; end those sessions or install a \
         keeper-capable build"
    )]
    KeeperSessionsExist {
        target_version: String,
        keeper_owned: usize,
    },
    #[error(
        "refusing to roll back to {target_version}: {unknown} live session(s) report an owner label \
         this build does not recognize, so keeper ownership cannot be ruled out"
    )]
    OwnershipUnknown { target_version: String, unknown: usize },
    #[error(
        "refusing to roll back to {target_version}: the oldest safe rollback version is \
         {oldest_safe_rollback}"
    )]
    OlderThanSafeRollback {
        target_version: String,
        oldest_safe_rollback: String,
    },
    /// The manifest names a floor this build cannot parse, so the minimum safe version is unknown.
    /// Refused rather than ignored: an unreadable floor would otherwise let any older build in.
    #[error(
        "refusing to roll back to {target_version}: the recorded oldest safe rollback version \
         {oldest_safe_rollback:?} is not a version, so the minimum safe build is unknown"
    )]
    MalformedSafeRollback {
        target_version: String,
        oldest_safe_rollback: String,
    },
}

/// The plan's rollback rule: a build without keeper support cannot become the running daemon while
/// any keeper-owned session lives, and no build older than the recorded floor may be installed.
///
/// Direct-owned sessions never block a rollback - a pre-keeper build services them exactly as this
/// one does - which is what lets an upgrade path stay open for a user who never had a keeper
/// session. `oldest_safe_rollback` is the manifest's [`OLDEST_SAFE_ROLLBACK_KEY`]; `None` means the
/// manifest records no floor and only the ownership rule applies.
pub fn rollback_gate(
    target: RollbackTarget<'_>,
    ownership: SessionOwnership,
    oldest_safe_rollback: Option<&str>,
) -> Result<(), KeeperRollbackRefusal> {
    if let Some(floor) = oldest_safe_rollback {
        if !looks_like_version(floor) {
            return Err(KeeperRollbackRefusal::MalformedSafeRollback {
                target_version: target.version.to_string(),
                oldest_safe_rollback: floor.to_string(),
            });
        }
        if compare_versions(target.version, floor).is_lt() {
            return Err(KeeperRollbackRefusal::OlderThanSafeRollback {
                target_version: target.version.to_string(),
                oldest_safe_rollback: floor.to_string(),
            });
        }
    }
    if !target.supports_keeper && ownership.keeper_owned > 0 {
        return Err(KeeperRollbackRefusal::KeeperSessionsExist {
            target_version: target.version.to_string(),
            keeper_owned: ownership.keeper_owned,
        });
    }
    if !target.supports_keeper && ownership.unknown > 0 {
        return Err(KeeperRollbackRefusal::OwnershipUnknown {
            target_version: target.version.to_string(),
            unknown: ownership.unknown,
        });
    }
    Ok(())
}

/// The refusal text for a rollback, or `None` when the rollback is allowed. Kept separate so a
/// surface that shows a message cannot accidentally treat `Allowed` as a message.
pub fn rollback_refusal_message(
    target: RollbackTarget<'_>,
    ownership: SessionOwnership,
    oldest_safe_rollback: Option<&str>,
) -> Option<String> {
    rollback_gate(target, ownership, oldest_safe_rollback)
        .err()
        .map(|refusal| refusal.to_string())
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> io::Result<()> {
    Ok(())
}

fn same_path(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn create_private_dir(dir: &Path) -> Result<(), KeeperServiceError> {
    match fs::create_dir_all(dir) {
        Ok(()) => {}
        Err(error) => {
            return Err(KeeperServiceError::Install {
                detail: format!("cannot create keeper directory {}: {error}", dir.display()),
            })
        }
    }
    set_mode(dir, 0o700).map_err(|error| KeeperServiceError::Install {
        detail: format!("cannot set mode 700 on {}: {error}", dir.display()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let owned: Vec<(String, OsString)> = pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), OsString::from(*value)))
            .collect();
        move |key| {
            owned
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value.clone())
        }
    }

    /// The strings launchd would hand the job as argv[0..], read out of the rendered plist.
    fn program_arguments(plist: &str) -> Vec<String> {
        let start = plist
            .find("<key>ProgramArguments</key>")
            .expect("a plist always has ProgramArguments");
        let rest = &plist[start..];
        let open = rest.find("<array>").expect("an array") + "<array>".len();
        let close = rest.find("</array>").expect("a closed array");
        rest[open..close]
            .lines()
            .filter_map(|line| {
                line.trim()
                    .strip_prefix("<string>")
                    .and_then(|line| line.strip_suffix("</string>"))
                    .map(str::to_owned)
            })
            .collect()
    }

    /// The `StandardOutPath`/`StandardErrorPath` values of a plist.
    fn standard_paths(plist: &str) -> Vec<String> {
        ["StandardOutPath", "StandardErrorPath"]
            .iter()
            .map(|key| {
                let marker = format!("<key>{key}</key>\n    <string>");
                let start = plist.find(&marker).unwrap_or_else(|| panic!("{key}")) + marker.len();
                let end = start + plist[start..].find("</string>").expect("a closed string");
                plist[start..end].to_string()
            })
            .collect()
    }

    fn plant(root: &Path, version: &str, body: &[u8]) -> PathBuf {
        let dir = root.join(version);
        fs::create_dir_all(&dir).expect("create generation");
        let binary = dir.join(BINARY_NAME);
        fs::write(&binary, body).expect("write binary");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).expect("executable");
        }
        binary
    }

    #[test]
    fn the_keeper_job_runs_the_keeper_binary_and_never_the_supervisor() {
        let log_dir = Path::new("/Users/someone/Library/Logs/Ferryx");
        let executable = "/Users/someone/.ferryx/keeper/0.4.0/ferryx-ptykeeper";
        let plist = generate_keeper_launchd_plist(PLIST_LABEL, executable, log_dir, true);

        assert_eq!(program_arguments(&plist), vec![executable.to_string()]);
        assert!(plist.contains(executable), "the generation's binary is the program");
        assert!(!plist.contains("--daemon-crash-supervisor"));
        assert!(!plist.contains("--daemon"), "the daemon is never the keeper's program");
        assert!(
            !plist.contains("--socket"),
            "the endpoint stays derived from the runtime, never frozen into argv"
        );
        assert!(plist.contains("<key>RunAtLoad</key>\n    <true/>"));
        assert!(plist.contains(
            "<key>KeepAlive</key>\n    <dict>\n        <key>SuccessfulExit</key>\n        <false/>\n    </dict>"
        ));
        assert!(plist.contains("<key>AbandonProcessGroup</key>\n    <true/>"));
        assert!(plist.contains("<key>ProcessType</key>\n    <string>Interactive</string>"));
        assert!(plist.contains("<key>NumberOfFiles</key>\n        <integer>4096</integer>"));
        assert!(plist.contains(&format!("<string>{PLIST_LABEL}</string>")));

        let disabled = generate_keeper_launchd_plist(PLIST_LABEL, executable, log_dir, false);
        assert!(disabled.contains("<key>RunAtLoad</key>\n    <false/>"));
        assert_eq!(program_arguments(&disabled), program_arguments(&plist));
    }

    #[test]
    fn the_qa_label_and_plist_path_differ_from_the_production_ones() {
        assert_eq!(label_for(false, false), PLIST_LABEL);
        assert_eq!(label_for(true, false), QA_PLIST_LABEL);
        assert_eq!(label_for(false, true), QA_PLIST_LABEL);
        assert_ne!(QA_PLIST_LABEL, PLIST_LABEL);
        assert_ne!(QA_PLIST_LABEL, launchd::PLIST_LABEL);
        assert_ne!(PLIST_LABEL, launchd::PLIST_LABEL, "one job, one label");
        assert_ne!(PLIST_PATH_ENV, launchd::PLIST_PATH_ENV, "one override, one plist");

        let home = OsStr::new("/Users/someone");
        let production = keeper_plist_path_for(PLIST_LABEL, None, Some(home)).expect("production");
        let qa = keeper_plist_path_for(QA_PLIST_LABEL, None, Some(home)).expect("qa");
        assert_eq!(
            production,
            PathBuf::from("/Users/someone/Library/LaunchAgents/com.ferryx.ptykeeper.plist")
        );
        assert_eq!(
            qa,
            PathBuf::from("/Users/someone/Library/LaunchAgents/com.ferryx.qa.ptykeeper.plist")
        );
        assert_ne!(production, qa);
        assert_eq!(
            keeper_plist_path_for(
                PLIST_LABEL,
                Some(OsStr::new("/tmp/fixture/keeper.plist")),
                Some(home)
            ),
            Some(PathBuf::from("/tmp/fixture/keeper.plist"))
        );
        assert_eq!(keeper_plist_path_for(PLIST_LABEL, None, None), None);
    }

    #[test]
    fn the_keeper_logs_are_distinct_from_the_daemons() {
        let log_dir = Path::new("/Users/someone/Library/Logs/Ferryx");
        let daemon = launchd::generate_launchd_plist(
            launchd::PLIST_LABEL,
            "/Applications/Ferryx.app/Contents/MacOS/ferryx",
            log_dir,
            true,
        );
        let keeper = generate_keeper_launchd_plist(
            PLIST_LABEL,
            "/Users/someone/.ferryx/keeper/0.4.0/ferryx-ptykeeper",
            log_dir,
            true,
        );
        let daemon_paths = standard_paths(&daemon);
        let keeper_paths = standard_paths(&keeper);

        assert!(keeper_paths.iter().all(|path| path.contains("keeper.")), "{keeper_paths:?}");
        assert!(
            keeper_paths.iter().all(|path| !path.contains("daemon.")),
            "a keeper must never write into the daemon's capture file: {keeper_paths:?}"
        );
        for path in &keeper_paths {
            assert!(!daemon_paths.contains(path), "{path} is shared with the daemon");
        }
        assert_ne!(keeper_paths, daemon_paths);
    }

    #[test]
    fn rewriting_identical_content_reports_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let service = KeeperService {
            label: PLIST_LABEL.to_string(),
            plist_path: dir.path().join(format!("{PLIST_LABEL}.plist")),
            log_dir: dir.path().join("logs"),
            environment: Vec::new(),
        };
        let executable = dir
            .path()
            .join(".ferryx/keeper/0.4.0")
            .join(BINARY_NAME);

        assert!(write_keeper_launchd_plist(&service, &executable, true).unwrap());
        assert!(
            service.log_dir.is_dir(),
            "launchd does not create a missing StandardOutPath parent, so the writer must"
        );
        assert!(
            !write_keeper_launchd_plist(&service, &executable, true).unwrap(),
            "an unchanged plist must not be rewritten on every start"
        );

        let next = dir.path().join(".ferryx/keeper/0.5.0").join(BINARY_NAME);
        assert!(
            write_keeper_launchd_plist(&service, &next, true).unwrap(),
            "a new generation rewrites the job"
        );
        assert!(fs::read_to_string(&service.plist_path).unwrap().contains("0.5.0"));
    }

    #[test]
    fn an_isolated_runtime_refuses_to_install_the_production_label() {
        let env = env_of(&[("HOME", "/Users/someone")]);

        for (dev, isolated) in [(true, false), (false, true), (true, true)] {
            let refused = keeper_service_with_label(PLIST_LABEL, dev, isolated, &env)
                .expect_err("an isolated runtime must not install the production label");
            assert_eq!(
                refused,
                KeeperServiceError::ProductionLabelRefused {
                    label: PLIST_LABEL.to_string()
                }
            );

            let service =
                keeper_service_for(dev, isolated, &env).expect("the QA job is installable");
            assert_eq!(service.label, QA_PLIST_LABEL);
            assert_eq!(
                service.plist_path.file_name().unwrap().to_str().unwrap(),
                format!("{QA_PLIST_LABEL}.plist")
            );
        }

        let production = keeper_service_with_label(PLIST_LABEL, false, false, &env)
            .expect("a real runtime installs the production label");
        assert_eq!(production.label, PLIST_LABEL);
        assert!(production.environment.is_empty(), "a production job forwards no environment");
    }

    #[test]
    fn a_qa_job_forwards_the_isolation_environment_and_a_production_job_does_not() {
        let env = env_of(&[
            ("HOME", "/Users/someone"),
            ("FERRYX_RUNTIME_DIR", "/tmp/ferryx-qa"),
            ("FERRYX_DAEMON_SUPERVISOR_NAME", "com.ferryx.qa"),
        ]);
        let qa = keeper_service_for(false, true, &env).expect("qa job");
        assert_eq!(
            qa.environment,
            vec![
                (
                    "FERRYX_RUNTIME_DIR".to_string(),
                    "/tmp/ferryx-qa".to_string()
                ),
                (
                    crate::daemon::supervisor::SUPERVISOR_NAME_ENV.to_string(),
                    "com.ferryx.qa".to_string()
                ),
            ],
            "a QA job forwards its isolation variables in fixed order"
        );
        let plist = qa.render(
            Path::new("/Users/someone/.ferryx/keeper/0.4.0/ferryx-ptykeeper"),
            true,
        );
        assert!(plist.contains("<key>EnvironmentVariables</key>"));
        assert!(plist.contains("<string>/tmp/ferryx-qa</string>"));
        assert_eq!(
            program_arguments(&plist),
            vec!["/Users/someone/.ferryx/keeper/0.4.0/ferryx-ptykeeper".to_string()],
            "the environment never becomes an argument"
        );
        assert!(!plist.contains("--daemon-crash-supervisor"));
    }

    /// A label this module has never seen - an acceptance run's own `com.ferryx.qa.<run>` - must
    /// forward the isolation environment exactly as the QA label does. Without it the job's keeper
    /// binds the user's `/tmp/ferryx-<uid>/keeper.sock`, which is the real incident this rule
    /// prevents; a per-run label must therefore not fall into the production branch.
    #[test]
    fn a_private_label_forwards_the_isolation_environment_like_the_qa_label() {
        let env = env_of(&[
            ("HOME", "/Users/someone"),
            ("FERRYX_RUNTIME_DIR", "/tmp/fx-run/runtime"),
            ("FERRYX_DATA_DIR", "/tmp/fx-run/data"),
        ]);
        let private = keeper_service_with_label("com.ferryx.qa.supervisor-abc123", true, true, &env)
            .expect("a private label is usable in an isolated runtime");
        assert!(is_private_label(&private.label), "{} must be private", private.label);
        assert_eq!(
            private.environment,
            vec![
                (
                    "FERRYX_DATA_DIR".to_string(),
                    "/tmp/fx-run/data".to_string()
                ),
                (
                    "FERRYX_RUNTIME_DIR".to_string(),
                    "/tmp/fx-run/runtime".to_string()
                ),
            ],
            "a private label forwards the same keys, in the same fixed order"
        );

        for shipped in [
            PLIST_LABEL,
            QA_PLIST_LABEL,
            launchd::PLIST_LABEL,
            launchd::RORCA_PLIST_LABEL,
        ] {
            assert!(
                !is_private_label(shipped),
                "{shipped} is a shipped job, never a private run's"
            );
        }
    }

    #[test]
    fn resolution_picks_the_newest_compatible_generation_and_preserves_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("keeper");
        plant(&root, "0.1.0", b"old");
        plant(&root, "0.2.0", b"incompatible");
        plant(&root, "0.10.0", b"newest");
        fs::create_dir_all(root.join("logs")).unwrap();

        let newest = resolve_keeper_generation(&root, accept_installed_generation)
            .expect("a compatible generation exists");
        assert_eq!(newest.version(), "0.10.0", "versions sort numerically, not lexically");
        assert_eq!(newest.binary(), root.join("0.10.0").join(BINARY_NAME));

        let older = resolve_keeper_generation(&root, |generation| {
            if generation.version() == "0.10.0" {
                Err(KeeperIncompatibility::ProtocolMajor {
                    version: generation.version().to_string(),
                    major: 2,
                    minor: 0,
                    supported_major: 1,
                })
            } else {
                Ok(())
            }
        })
        .expect("an older compatible generation is a valid answer");
        assert_eq!(older.version(), "0.2.0");
        assert!(root.join("0.10.0").exists(), "the incompatible keeper is preserved");
        assert!(root.join("0.1.0").exists() && root.join("0.2.0").exists());
    }

    #[test]
    fn no_compatible_generation_is_a_typed_error_naming_what_is_installed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("keeper");
        plant(&root, "0.1.0", b"binary");
        fs::remove_file(root.join("0.1.0").join(BINARY_NAME)).unwrap();

        let error = resolve_keeper_generation(&root, accept_installed_generation)
            .expect_err("a generation without a binary cannot serve");
        match &error {
            KeeperServiceError::NoCompatibleGeneration {
                installed,
                rejected,
                retained,
                ..
            } => {
                assert_eq!(installed, &vec!["0.1.0".to_string()]);
                assert_eq!(*retained, RETAINED_GENERATIONS);
                match rejected.as_slice() {
                    [KeeperIncompatibility::Unknown { version, .. }] => {
                        assert_eq!(version.as_str(), "0.1.0")
                    }
                    other => panic!("expected one unreadable generation, got {other:?}"),
                }
            }
            other => panic!("expected a typed incompatibility, got {other:?}"),
        }
        assert!(root.join("0.1.0").is_dir(), "nothing is deleted by resolution");

        let empty = dir.path().join("absent-tree");
        assert!(matches!(
            resolve_keeper_generation(&empty, accept_installed_generation),
            Err(KeeperServiceError::NoCompatibleGeneration { .. })
        ));
    }

    #[test]
    fn the_install_root_matches_the_installers_rule() {
        assert_eq!(
            default_keeper_install_root_for(None, Some(OsStr::new("/Users/someone"))),
            Some(PathBuf::from("/Users/someone/.ferryx/keeper"))
        );
        assert_eq!(
            default_keeper_install_root_for(
                Some(OsStr::new("/qa/keeper")),
                Some(OsStr::new("/Users/someone"))
            ),
            Some(PathBuf::from("/qa/keeper")),
            "the explicit root wins"
        );
        assert_eq!(
            default_keeper_install_root_for(Some(OsStr::new("")), None),
            None,
            "an empty root is unset, never the filesystem root"
        );
    }

    fn source(dir: &Path, body: &[u8]) -> PathBuf {
        let path = dir.join("built-keeper");
        fs::write(&path, body).expect("write source binary");
        path
    }

    fn temp_service(dir: &Path) -> KeeperService {
        KeeperService {
            label: PLIST_LABEL.to_string(),
            plist_path: dir.join(format!("{PLIST_LABEL}.plist")),
            log_dir: dir.join("logs"),
            environment: Vec::new(),
        }
    }

    #[test]
    fn planting_a_generation_is_idempotent_and_never_replaces_different_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("keeper");
        let built = source(dir.path(), b"binary-one");

        let first = install_keeper_generation(&root, "2026.1115.1", &built, Some("abc123")).unwrap();
        assert!(matches!(first, KeeperInstallOutcome::Installed(_)));
        assert_eq!(
            fs::read(root.join("2026.1115.1").join(BINARY_NAME)).unwrap(),
            b"binary-one"
        );
        assert_eq!(
            fs::read_to_string(root.join("2026.1115.1").join(DIGEST_NAME)).unwrap(),
            "abc123\n"
        );

        let again = install_keeper_generation(&root, "2026.1115.1", &built, None).unwrap();
        assert!(matches!(again, KeeperInstallOutcome::AlreadyPresent(_)));

        fs::write(&built, b"binary-two").unwrap();
        let refused = install_keeper_generation(&root, "2026.1115.1", &built, None)
            .expect_err("a generation is immutable");
        assert!(matches!(refused, KeeperServiceError::GenerationImmutable { .. }));
        assert_eq!(
            fs::read(root.join("2026.1115.1").join(BINARY_NAME)).unwrap(),
            b"binary-one",
            "the existing generation is untouched"
        );

        assert!(matches!(
            install_keeper_generation(&root, "keeper", &built, None),
            Err(KeeperServiceError::NotAVersion { .. })
        ));
    }

    #[test]
    fn retention_keeps_three_and_preserves_occupied_and_referenced_generations() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("keeper");
        for version in ["0.1.0", "0.2.0", "0.3.0", "0.4.0", "0.5.0"] {
            plant(&root, version, b"binary");
        }

        let report = prune_keeper_generations(&root, &[], |generation| {
            usize::from(generation.version() == "0.1.0") * 2
        })
        .unwrap();

        assert_eq!(report.removed, vec!["0.2.0".to_string()]);
        assert_eq!(report.preserved_occupied, vec!["0.1.0".to_string()]);
        assert_eq!(
            report.retained,
            vec![
                "0.5.0".to_string(),
                "0.4.0".to_string(),
                "0.3.0".to_string(),
                "0.1.0".to_string()
            ]
        );
        assert!(root.join("0.1.0").exists(), "an occupied generation is never deleted");
        assert!(!root.join("0.2.0").exists());

        let free_now = prune_keeper_generations(&root, &[], |_| 0).unwrap();
        assert_eq!(
            free_now.removed,
            vec!["0.1.0".to_string()],
            "the former occupant goes once its sessions end"
        );

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("keeper");
        for version in ["0.1.0", "0.2.0", "0.3.0", "0.4.0"] {
            plant(&root, version, b"binary");
        }
        let referenced = root.join("0.1.0").join(BINARY_NAME);
        let report = prune_keeper_generations(&root, &[referenced], |_| 0).unwrap();
        assert_eq!(report.preserved_referenced, vec!["0.1.0".to_string()]);
        assert!(
            root.join("0.1.0").exists(),
            "a generation a plist still points at is not deleted"
        );
    }

    #[test]
    fn installing_the_service_plants_retains_and_points_the_job_at_one_generation() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("keeper");
        for version in ["2026.1114.1", "2026.1114.2", "2026.1115.1", "2026.1115.2"] {
            plant(&root, version, b"old-binary");
        }
        let service = temp_service(dir.path());
        let built = source(dir.path(), b"new-binary");

        let outcome = ensure_keeper_service(
            &service,
            &root,
            "2026.1116.1",
            &built,
            Some("digest"),
            &|_| 0,
            true,
        )
        .expect("an unoccupied tree installs cleanly");

        assert!(outcome.installed);
        assert!(outcome.plist_rewritten);
        assert_eq!(outcome.generation.version(), "2026.1116.1");
        let plist = fs::read_to_string(&service.plist_path).unwrap();
        let program = generation::plist_program(&plist).expect("a program");
        assert_eq!(
            PathBuf::from(program),
            root.join("2026.1116.1").join(BINARY_NAME),
            "the job runs the new immutable generation"
        );
        assert!(!root.join("2026.1114.1").exists(), "retention prunes past the window");
        assert!(!root.join("2026.1114.2").exists());
        assert!(root.join("2026.1115.1").exists());
        assert!(root.join("2026.1115.2").exists());

        let repeat = ensure_keeper_service(
            &service,
            &root,
            "2026.1116.1",
            &built,
            Some("digest"),
            &|_| 0,
            true,
        )
        .expect("a second start is a no-op");
        assert!(!repeat.installed);
        assert!(!repeat.plist_rewritten, "an unchanged plist is not rewritten");
    }

    #[test]
    fn the_service_is_not_repointed_while_the_programmed_generation_owns_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("keeper");
        let service = temp_service(dir.path());
        plant(&root, "2026.1115.1", b"current");
        assert!(write_keeper_launchd_plist(
            &service,
            &root.join("2026.1115.1").join(BINARY_NAME),
            true
        )
        .unwrap());
        let built = source(dir.path(), b"next-binary");

        let refused = ensure_keeper_service(
            &service,
            &root,
            "2026.1116.1",
            &built,
            None,
            &|generation| usize::from(generation.version() == "2026.1115.1") * 3,
            true,
        )
        .expect_err("an occupied keeper is never replaced");
        match refused {
            KeeperServiceError::KeeperOccupied { version, sessions } => {
                assert_eq!(version, "2026.1115.1");
                assert_eq!(sessions, 3);
            }
            other => panic!("expected an occupancy refusal, got {other:?}"),
        }

        let expected = root.join("2026.1115.1").join(BINARY_NAME).to_string_lossy().into_owned();
        let plist = fs::read_to_string(&service.plist_path).unwrap();
        assert_eq!(
            generation::plist_program(&plist),
            Some(expected.as_str()),
            "the job is left exactly as it was"
        );
        assert!(
            !root.join("2026.1116.1").exists(),
            "the refused generation was not planted"
        );
    }

    #[test]
    fn ownership_counts_keeper_and_direct_sessions_apart() {
        let mixed = SessionOwnership::from_owner_labels(["keeper", "keeper", "daemon"]);
        assert_eq!(
            mixed,
            SessionOwnership {
                keeper_owned: 2,
                direct_owned: 1,
                unknown: 0
            }
        );
        assert_eq!(mixed.total(), 3);
        assert_eq!(
            SessionOwnership::from_owner_labels(Vec::<String>::new()),
            SessionOwnership::default()
        );
        assert_eq!(SessionOwnership::from_owner_labels(["keeper", "???"]).unknown, 1);
    }

    #[test]
    fn a_rollback_is_refused_only_while_a_keeper_owned_session_lives() {
        let pre_keeper = RollbackTarget {
            version: "2026.1115.1",
            supports_keeper: false,
        };
        let mixed = SessionOwnership::from_owner_labels(["keeper", "daemon"]);

        let refusal = rollback_gate(pre_keeper, mixed, None)
            .expect_err("keeper sessions block a pre-keeper build");
        match &refusal {
            KeeperRollbackRefusal::KeeperSessionsExist {
                target_version,
                keeper_owned,
            } => {
                assert_eq!(target_version, "2026.1115.1");
                assert_eq!(*keeper_owned, 1, "the count names keeper-owned sessions only");
            }
            other => panic!("expected a keeper-ownership refusal, got {other:?}"),
        }
        let message = rollback_refusal_message(pre_keeper, mixed, None).expect("a refusal message");
        assert!(message.contains("1 session(s)"), "{message}");

        let direct_only = SessionOwnership::from_owner_labels(["daemon", "daemon"]);
        assert_eq!(
            rollback_gate(pre_keeper, direct_only, None),
            Ok(()),
            "direct-owned sessions never block a rollback"
        );
        assert_eq!(rollback_refusal_message(pre_keeper, direct_only, None), None);

        let after_the_keeper_session_ends = SessionOwnership::from_owner_labels(["daemon"]);
        assert_eq!(
            rollback_gate(pre_keeper, after_the_keeper_session_ends, None),
            Ok(()),
            "the rollback succeeds once the keeper session is gone"
        );
    }

    #[test]
    fn a_rollback_below_the_manifest_floor_is_refused_and_an_unknown_owner_fails_closed() {
        let keeper_capable = RollbackTarget {
            version: "2026.1115.1",
            supports_keeper: true,
        };
        assert_eq!(
            rollback_gate(keeper_capable, SessionOwnership::default(), Some("2026.1116.1")),
            Err(KeeperRollbackRefusal::OlderThanSafeRollback {
                target_version: "2026.1115.1".to_string(),
                oldest_safe_rollback: "2026.1116.1".to_string(),
            }),
            "a build older than the recorded floor is refused"
        );

        let at_the_floor = RollbackTarget {
            version: "2026.1116.1",
            supports_keeper: true,
        };
        assert_eq!(
            rollback_gate(at_the_floor, SessionOwnership::default(), Some("2026.1116.1")),
            Ok(())
        );

        let unknown_owner = SessionOwnership::from_owner_labels(["keeper-ish"]);
        let pre_keeper = RollbackTarget {
            version: "2026.1116.1",
            supports_keeper: false,
        };
        assert_eq!(
            rollback_gate(pre_keeper, unknown_owner, None),
            Err(KeeperRollbackRefusal::OwnershipUnknown {
                target_version: "2026.1116.1".to_string(),
                unknown: 1,
            })
        );
    }

    #[test]
    fn the_generation_version_is_this_builds_app_version() {
        assert_eq!(keeper_generation_version(), env!("CARGO_PKG_VERSION"));
        assert!(
            looks_like_version(keeper_generation_version()),
            "the app version must be usable as a generation name"
        );
    }

    #[test]
    fn a_floor_this_build_cannot_parse_refuses_every_rollback() {
        let target = RollbackTarget {
            version: "2026.1116.1",
            supports_keeper: true,
        };
        assert_eq!(
            rollback_gate(target, SessionOwnership::default(), Some("keeper")),
            Err(KeeperRollbackRefusal::MalformedSafeRollback {
                target_version: "2026.1116.1".to_string(),
                oldest_safe_rollback: "keeper".to_string(),
            }),
            "an unreadable floor must not be skipped: skipping it admits every older build"
        );
        let message =
            rollback_refusal_message(target, SessionOwnership::default(), Some("keeper")).expect("a message");
        assert!(message.contains("\"keeper\""), "{message}");
    }

    #[test]
    fn a_generation_with_an_unreadable_binary_is_refused_rather_than_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("keeper");
        let generation = root.join("2026.1116.1");
        fs::create_dir_all(generation.join(BINARY_NAME)).unwrap();
        let built = source(dir.path(), b"binary");

        let refused = install_keeper_generation(&root, "2026.1116.1", &built, None)
            .expect_err("an uninspectable generation is never overwritten");
        match refused {
            KeeperServiceError::ExistingUnreadable { version, .. } => assert_eq!(version, "2026.1116.1"),
            other => panic!("expected an unreadable-generation refusal, got {other:?}"),
        }
    }

    #[test]
    fn an_unreadable_plist_refuses_to_repoint_the_job() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("keeper");
        let service = temp_service(dir.path());
        fs::create_dir_all(service.plist_path.clone()).unwrap();
        let built = source(dir.path(), b"binary");

        let refused = ensure_keeper_service(&service, &root, "2026.1116.1", &built, None, &|_| 0, true)
            .expect_err("an unreadable plist must not be treated as absent");
        match refused {
            KeeperServiceError::PlistUnreadable { path, .. } => assert_eq!(path, service.plist_path.display().to_string()),
            other => panic!("expected an unreadable-plist refusal, got {other:?}"),
        }
        assert!(
            !root.join("2026.1116.1").exists(),
            "nothing is planted when the job cannot be described"
        );
    }

    #[test]
    fn only_one_normal_numeric_version_component_may_be_joined_onto_the_root() {
        // F-R1-missed-1: `looks_like_version` validated only the numeric prefix, so these all
        // passed it and then escaped the install root when joined.
        for escaped in [
            "1.2.3/../../etc",
            "1.2.3-../../x",
            "1.2.3/..",
            "../1.2.3",
            "..",
            ".",
            "",
            ".1.2.3",
            "1.2.3\\..\\x",
            "1.2.3/0",
        ] {
            assert!(!is_safe_version(escaped), "{escaped:?} must be refused");
        }
        // A suffix made of semver characters is still a version.
        for accepted in ["1.2.3", "0.4.0", "1.2.3-pre", "1.2.3-pre.1", "1.2.3+build.5", "2026.1115.1"] {
            assert!(is_safe_version(accepted), "{accepted:?} must be accepted");
        }
        // A non-numeric core is not a version, whatever else is true of it.
        for rejected in ["v1.2.3", "1.2", "1.2.3.4", "1.x.3", "1.2.3_"] {
            assert!(!is_safe_version(rejected), "{rejected:?} must be refused");
        }
    }

    #[test]
    fn an_unsafe_version_is_refused_before_anything_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("keeper");
        let built = source(dir.path(), b"binary");
        for escaped in ["1.2.3/../../etc", "..", "1.2.3-../../x"] {
            match install_keeper_generation(&root, escaped, &built, None) {
                Err(KeeperServiceError::NotAVersion { version }) => {
                    assert_eq!(version, escaped)
                }
                other => panic!("{escaped:?} must be refused as not a version, got {other:?}"),
            }
        }
        // The refusal is before the read of the source and before the root is created, so nothing
        // escaped and nothing was planted.
        assert!(!root.exists() || fs::read_dir(&root).unwrap().next().is_none());
    }

    #[test]
    fn a_repeated_install_publishes_once_and_leaves_no_staging_file() {
        // The publish is a no-replace link from a unique `create_new` staging file, so an identical
        // second install is `AlreadyPresent` rather than a second overwrite of the published inode.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("keeper");
        let built = source(dir.path(), b"binary-bytes");
        let first = install_keeper_generation(&root, "1.0.0", &built, None).expect("install");
        assert!(matches!(first, KeeperInstallOutcome::Installed(_)));
        let second = install_keeper_generation(&root, "1.0.0", &built, None).expect("reinstall");
        assert!(matches!(second, KeeperInstallOutcome::AlreadyPresent(_)));

        let leftovers: Vec<String> = fs::read_dir(root.join("1.0.0"))
            .expect("generation dir")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".partial"))
            .collect();
        assert!(leftovers.is_empty(), "no staging file survives a publish: {leftovers:?}");
        assert_eq!(
            fs::read(root.join("1.0.0").join(BINARY_NAME)).unwrap(),
            b"binary-bytes".to_vec()
        );
    }

    #[test]
    fn a_concurrent_install_never_overwrites_a_published_generation() {
        // Two installers that both observe "absent" must not both publish: the no-replace link
        // makes the loser fail and then compare bytes, which is the immutable-generation rule.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("keeper");
        let first_dir = dir.path().join("src-first");
        let second_dir = dir.path().join("src-second");
        fs::create_dir_all(&first_dir).unwrap();
        fs::create_dir_all(&second_dir).unwrap();
        let first = source(&first_dir, b"first-writer");
        let second = source(&second_dir, b"second-writer");
        install_keeper_generation(&root, "1.0.0", &first, None).expect("first install");
        match install_keeper_generation(&root, "1.0.0", &second, None) {
            Err(KeeperServiceError::GenerationImmutable { version, .. }) => {
                assert_eq!(version, "1.0.0")
            }
            other => panic!("different bytes at a published version must be refused, got {other:?}"),
        }
        assert_eq!(
            fs::read(root.join("1.0.0").join(BINARY_NAME)).unwrap(),
            b"first-writer".to_vec(),
            "the published bytes are never replaced"
        );
    }
}
