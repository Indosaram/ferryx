//! The production daemon rollback: the one path `ferryx-cli service rollback <version>` reaches.
//!
//! The plan's rule is one sentence: a rollback is refused while any keeper-owned session lives,
//! because a build that predates the keeper cannot discover or service one. That single refusal is
//! what keeps a user from trading a surviving shell for an "older but working" binary. It is
//! implemented here, not in the caller, so every surface - the CLI today and any later one - reaches
//! the same decision.
//!
//! Three inputs decide it, and each has exactly one honest answer:
//!
//! * **the floor** - the oldest version that may be installed, published in the shipped release
//!   manifest. The checked-in ledger in `packaging/keeper/releases.json` is a *build-time* input and
//!   is not on a user's host; borrowing it at runtime would silently claim a floor the build does
//!   not have. A missing, unreadable or malformed floor refuses the rollback
//!   ([`ROLLBACK_FLOOR_UNVERIFIED`]): an unknown minimum is not a permissive minimum.
//! * **the ownership** - read from the durable per-session table (`daemon::keeper_session`), because
//!   ownership has to outlive the daemon that wrote it. An unreadable table refuses
//!   ([`ROLLBACK_OWNERSHIP_UNKNOWN`]) instead of guessing it empty.
//! * **the target** - a rollback is only *possible* when this machine still has an immutable staged
//!   generation for that version. Nothing is downloaded, rebuilt or invented here; an absent
//!   generation is [`ROLLBACK_TARGET_UNAVAILABLE`].
//!
//! What a successful rollback changes is deliberately narrow: the launchd job's login registration is
//! moved to the target generation's staged executable and the superseded generations lose their
//! load-time launch, all by plist rewrite. Nothing is signalled, unloaded, booted out, killed or
//! restarted, so a running daemon keeps serving every session it already owns and the rollback takes
//! effect at the next daemon start. That is also why a refusal writes no file at all: the decision is
//! complete before the first byte is written.
//!
//! macOS delegates the decision to `daemon::keeper_launchd::rollback_gate`, the reviewed
//! implementation of the same two rules, so the shipping target has one gate rather than two. The
//! local implementation below is what the other targets use; it is written to the same rules and is
//! unit-tested on every target.

use std::path::{Path, PathBuf};

/// Refused: keeper-owned sessions are live and the target build cannot service them. The message
/// names the exact count, which is the number the operator has to see.
pub const ROLLBACK_REFUSED_KEEPER_SESSIONS: &str = "KEEPER_ROLLBACK_REFUSED";
/// Refused: the target is older than the manifest's oldest safe rollback version.
pub const ROLLBACK_BELOW_FLOOR: &str = "KEEPER_ROLLBACK_BELOW_FLOOR";
/// Refused: a live session reports an owner label this build does not recognize.
pub const ROLLBACK_OWNERSHIP_UNKNOWN: &str = "KEEPER_OWNERSHIP_UNKNOWN";
/// Refused: the shipped manifest carries no usable oldest safe rollback version.
pub const ROLLBACK_FLOOR_UNVERIFIED: &str = "KEEPER_ROLLBACK_FLOOR_UNVERIFIED";
/// Refused: this machine has no staged generation for the target version.
pub const ROLLBACK_TARGET_UNAVAILABLE: &str = "KEEPER_ROLLBACK_TARGET_UNAVAILABLE";
/// Refused: this runtime may not manage the user's launchd jobs (dev or isolated fixture).
pub const ROLLBACK_UNSUPPORTED_RUNTIME: &str = "ROLLBACK_UNSUPPORTED_RUNTIME";
/// The plist rewrite failed. The decision did not change any session.
pub const ROLLBACK_IO_FAILED: &str = "ROLLBACK_IO_FAILED";

/// Manifest environment override naming the shipped release manifest to read the floor from, so a QA
/// harness can drive the real CLI against a fixture without touching the user's install.
pub const MANIFEST_PATH_ENV: &str = "FERRYX_ROLLBACK_MANIFEST";

/// Who owns the live sessions, as counted from their persisted records.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LiveOwnership {
    pub keeper_owned: usize,
    pub direct_owned: usize,
    /// Sessions whose owner label this build does not recognize. Kept apart so the decision can fail
    /// closed instead of folding them into a category.
    pub unknown: usize,
}

impl LiveOwnership {
    pub fn total(&self) -> usize {
        self.keeper_owned + self.direct_owned + self.unknown
    }
}

/// The ownership answer. `Unverified` and "zero sessions" demand opposite actions, so they are
/// different values rather than an empty count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnershipAnswer {
    Known(LiveOwnership),
    /// The durable table exists but could not be read or parsed, so occupancy is unknown.
    Unverified(String),
}

impl OwnershipAnswer {
    /// The answer when no keeper exists on this target at all: no session can be keeper-owned, so
    /// zero is a fact rather than a guess.
    pub fn no_keeper_on_this_target() -> Self {
        Self::Known(LiveOwnership::default())
    }
}

/// What the shipped manifest says about the oldest version a rollback may reach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FloorAnswer {
    /// The manifest records a floor. Only a version at or above it may be installed.
    Recorded(String),
    /// The manifest carries no keeper block: this release predates the floor, so the ownership rule
    /// is the only rule that applies.
    NoFloorRecorded,
    /// The manifest is missing, unreadable, or carries a floor that is not a version.
    Unverified(String),
}

/// Why a rollback is refused. Every variant carries what the caller must report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RollbackRefusal {
    KeeperSessionsExist {
        target_version: String,
        keeper_owned: usize,
    },
    BelowFloor {
        target_version: String,
        oldest_safe_rollback: String,
    },
    OwnershipUnknown {
        target_version: String,
        detail: String,
    },
    FloorUnverified {
        target_version: String,
        detail: String,
    },
    TargetUnavailable {
        target_version: String,
        detail: String,
    },
    UnsupportedRuntime {
        detail: String,
    },
    Io {
        detail: String,
    },
}

impl RollbackRefusal {
    /// The stable code a caller prints and a script greps for. Branches on the variant, never on text.
    pub fn code(&self) -> &'static str {
        match self {
            Self::KeeperSessionsExist { .. } => ROLLBACK_REFUSED_KEEPER_SESSIONS,
            Self::BelowFloor { .. } => ROLLBACK_BELOW_FLOOR,
            Self::OwnershipUnknown { .. } => ROLLBACK_OWNERSHIP_UNKNOWN,
            Self::FloorUnverified { .. } => ROLLBACK_FLOOR_UNVERIFIED,
            Self::TargetUnavailable { .. } => ROLLBACK_TARGET_UNAVAILABLE,
            Self::UnsupportedRuntime { .. } => ROLLBACK_UNSUPPORTED_RUNTIME,
            Self::Io { .. } => ROLLBACK_IO_FAILED,
        }
    }

    /// The live keeper-owned count the refusal names, when it names one.
    pub fn keeper_session_count(&self) -> Option<usize> {
        match self {
            Self::KeeperSessionsExist { keeper_owned, .. } => Some(*keeper_owned),
            _ => None,
        }
    }
}

impl std::fmt::Display for RollbackRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::KeeperSessionsExist { target_version, keeper_owned } => write!(
                f,
                "refusing to roll the daemon back to {target_version}: {keeper_owned} session(s) are \
                 owned by the PTY keeper, and that build cannot discover or service them, so the \
                 rollback is refused until those sessions end"
            ),
            Self::BelowFloor { target_version, oldest_safe_rollback } => write!(
                f,
                "refusing to roll the daemon back to {target_version}: the oldest safe rollback \
                 version is {oldest_safe_rollback}"
            ),
            Self::OwnershipUnknown { target_version, detail } => write!(
                f,
                "refusing to roll the daemon back to {target_version}: the session ownership table \
                 is unreadable ({detail}), so keeper-owned sessions cannot be ruled out"
            ),
            Self::FloorUnverified { target_version, detail } => write!(
                f,
                "refusing to roll the daemon back to {target_version}: the shipped release manifest \
                 does not state a usable oldest safe rollback version ({detail}), so the minimum \
                 safe build is unknown"
            ),
            Self::TargetUnavailable { target_version, detail } => write!(
                f,
                "cannot roll the daemon back to {target_version}: {detail}, and nothing was changed"
            ),
            Self::UnsupportedRuntime { detail } => write!(f, "rollback unavailable: {detail}"),
            Self::Io { detail } => write!(f, "rollback failed: {detail}"),
        }
    }
}

impl std::error::Error for RollbackRefusal {}

/// Three numeric components, optional `-`/`+` suffix: the grammar a generation directory name and a
/// published version share.
fn looks_like_version(text: &str) -> bool {
    let core = text.split(['-', '+']).next().unwrap_or("");
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3 && parts.iter().all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

/// Numeric comparison of `<major>.<minor>.<patch>`, with a lexical tiebreak on the suffix, matching
/// the release pipeline's ordering so a version cannot be judged differently here than there.
fn compare_versions(left: &str, right: &str) -> std::cmp::Ordering {
    fn split(text: &str) -> ([u64; 3], String) {
        let mut parts = text.split(['-', '+']);
        let core = parts.next().unwrap_or("");
        let suffix: String = parts.map(|part| format!("-{part}")).collect();
        let mut numbers = [0u64; 3];
        for (index, part) in core.split('.').take(3).enumerate() {
            numbers[index] = part.parse().unwrap_or(0);
        }
        (numbers, suffix)
    }
    let (left_numbers, left_suffix) = split(left);
    let (right_numbers, right_suffix) = split(right);
    for index in 0..3 {
        if left_numbers[index] != right_numbers[index] {
            return left_numbers[index].cmp(&right_numbers[index]);
        }
    }
    left_suffix.cmp(&right_suffix)
}

/// The decision, and the facts it rests on, so a caller can report more than "ok".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RollbackDecision {
    pub target_version: String,
    /// Whether the target build can discover and service keeper-owned sessions.
    pub supports_keeper: bool,
    pub keeper_owned: usize,
    pub direct_owned: usize,
}

/// The plan's rollback rule over already-resolved inputs.
///
/// `supports_keeper` is not guessed from a version string: it is true exactly when the manifest
/// records a floor the target is at or above, because the floor *is* the first build that understood
/// keeper-owned sessions. A target below the floor is refused as too old; a target whose support the
/// manifest cannot state (no floor recorded) is treated as unable to service keeper sessions, so live
/// keeper sessions refuse it and a machine with none may proceed. Both readings are fail-closed: the
/// only way to lose is to be wrong about a session that does not exist, never about one that does.
pub fn decide_rollback(
    target_version: &str,
    ownership: &OwnershipAnswer,
    floor: &FloorAnswer,
) -> Result<RollbackDecision, RollbackRefusal> {
    if !looks_like_version(target_version) {
        return Err(RollbackRefusal::TargetUnavailable {
            target_version: target_version.to_string(),
            detail: "that is not a three-component app version".to_string(),
        });
    }

    let (supports_keeper, oldest_safe_rollback) = match floor {
        FloorAnswer::Unverified(detail) => {
            return Err(RollbackRefusal::FloorUnverified {
                target_version: target_version.to_string(),
                detail: detail.clone(),
            })
        }
        FloorAnswer::NoFloorRecorded => (false, None),
        FloorAnswer::Recorded(floor) => {
            if !looks_like_version(floor) {
                return Err(RollbackRefusal::FloorUnverified {
                    target_version: target_version.to_string(),
                    detail: format!("oldestSafeRollbackVersion {floor:?} is not a version"),
                });
            }
            if compare_versions(target_version, floor).is_lt() {
                return Err(RollbackRefusal::BelowFloor {
                    target_version: target_version.to_string(),
                    oldest_safe_rollback: floor.clone(),
                });
            }
            (true, Some(floor.as_str()))
        }
    };

    let live = match ownership {
        OwnershipAnswer::Unverified(detail) => {
            return Err(RollbackRefusal::OwnershipUnknown {
                target_version: target_version.to_string(),
                detail: detail.clone(),
            })
        }
        OwnershipAnswer::Known(live) => *live,
    };

    // macOS runs the reviewed gate so the shipping target has exactly one implementation of these two
    // rules. Every other target runs the same rules written out above.
    #[cfg(target_os = "macos")]
    {
        use crate::daemon::keeper_launchd::{self, RollbackTarget, SessionOwnership};
        let target = RollbackTarget { version: target_version, supports_keeper };
        let ownership = SessionOwnership {
            keeper_owned: live.keeper_owned,
            direct_owned: live.direct_owned,
            unknown: live.unknown,
        };
        match keeper_launchd::rollback_gate(target, ownership, oldest_safe_rollback) {
            Ok(()) => {}
            Err(keeper_launchd::KeeperRollbackRefusal::KeeperSessionsExist { keeper_owned, .. }) => {
                return Err(RollbackRefusal::KeeperSessionsExist {
                    target_version: target_version.to_string(),
                    keeper_owned,
                })
            }
            Err(keeper_launchd::KeeperRollbackRefusal::OwnershipUnknown { unknown, .. }) => {
                return Err(RollbackRefusal::OwnershipUnknown {
                    target_version: target_version.to_string(),
                    detail: format!("{unknown} session(s) report an unrecognized owner"),
                })
            }
            Err(keeper_launchd::KeeperRollbackRefusal::OlderThanSafeRollback {
                oldest_safe_rollback,
                ..
            }) => {
                return Err(RollbackRefusal::BelowFloor {
                    target_version: target_version.to_string(),
                    oldest_safe_rollback,
                })
            }
            Err(keeper_launchd::KeeperRollbackRefusal::MalformedSafeRollback {
                oldest_safe_rollback,
                ..
            }) => {
                return Err(RollbackRefusal::FloorUnverified {
                    target_version: target_version.to_string(),
                    detail: format!("oldestSafeRollbackVersion {oldest_safe_rollback:?} is not a version"),
                })
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        if !supports_keeper && live.keeper_owned > 0 {
            return Err(RollbackRefusal::KeeperSessionsExist {
                target_version: target_version.to_string(),
                keeper_owned: live.keeper_owned,
            });
        }
        if !supports_keeper && live.unknown > 0 {
            return Err(RollbackRefusal::OwnershipUnknown {
                target_version: target_version.to_string(),
                detail: format!("{} session(s) report an unrecognized owner", live.unknown),
            });
        }
        let _ = oldest_safe_rollback;
    }

    Ok(RollbackDecision {
        target_version: target_version.to_string(),
        supports_keeper,
        keeper_owned: live.keeper_owned,
        direct_owned: live.direct_owned,
    })
}

/// Reads the floor out of a shipped release manifest (`latest.json`'s `keeper` block).
///
/// Only the shipped document is consulted. A build whose manifest cannot be found refuses rather than
/// falling back to a checked-in ledger that exists only in a source checkout: the floor a user's
/// rollback is judged against must be the floor their build actually published.
pub fn floor_from_manifest(manifest: &Path) -> FloorAnswer {
    let bytes = match std::fs::read(manifest) {
        Ok(bytes) => bytes,
        Err(error) => {
            return FloorAnswer::Unverified(format!("cannot read {}: {error}", manifest.display()))
        }
    };
    let value: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(error) => {
            return FloorAnswer::Unverified(format!("{} is not JSON: {error}", manifest.display()))
        }
    };
    let Some(keeper) = value.get("keeper") else {
        // No keeper block at all: the manifest is from a release that shipped no keeper, so it states
        // no floor and only the ownership rule applies.
        return FloorAnswer::NoFloorRecorded;
    };
    match keeper.get("oldestSafeRollbackVersion").and_then(serde_json::Value::as_str) {
        Some(floor) => FloorAnswer::Recorded(floor.to_string()),
        None => FloorAnswer::Unverified(format!(
            "{} has a keeper block without an oldestSafeRollbackVersion string",
            manifest.display()
        )),
    }
}

/// The manifest a running build publishes its floor in, as shipped beside the executable.
///
/// A release lays the updater manifest at `Contents/Resources/latest.json`; QA may name one through
/// [`MANIFEST_PATH_ENV`]. `None` means this executable has no adjacent manifest, which the caller
/// reports as [`FloorAnswer::Unverified`] rather than inventing a floor.
pub fn shipped_manifest_path(executable: &Path) -> Option<PathBuf> {
    if let Some(override_path) = std::env::var_os(MANIFEST_PATH_ENV).filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(override_path));
    }
    let resources = executable.parent()?.parent()?.join("Resources");
    [resources.join("latest.json"), resources.join("keeper").join("latest.json")]
        .into_iter()
        .find(|candidate| candidate.is_file())
}

/// The durable ownership table, read into the answer the decision needs.
///
/// A missing table is an empty one: a user who never enabled the keeper has no records, which is not
/// an error. Anything else - unreadable, unparsable, an owner label this build does not know - is
/// `Unverified`, because the count it would produce is a guess and this decision must not guess.
#[cfg(unix)]
pub fn durable_ownership(runtime_dir: Option<&Path>) -> OwnershipAnswer {
    use crate::daemon::keeper_session::{OwnershipStore, SessionOwner};
    let store = match runtime_dir {
        Some(dir) => OwnershipStore::in_runtime_dir(dir),
        None => match OwnershipStore::for_runtime() {
            Ok(store) => store,
            Err(error) => return OwnershipAnswer::Unverified(error.to_string()),
        },
    };
    match store.load() {
        Ok(table) => OwnershipAnswer::Known(LiveOwnership {
            keeper_owned: table
                .sessions
                .values()
                .filter(|record| record.owner == SessionOwner::Keeper)
                .count(),
            direct_owned: table
                .sessions
                .values()
                .filter(|record| record.owner == SessionOwner::Daemon)
                .count(),
            unknown: 0,
        }),
        Err(error) => OwnershipAnswer::Unverified(error.to_string()),
    }
}

/// No keeper ships on this target, so no session can be keeper-owned.
#[cfg(not(unix))]
pub fn durable_ownership(_runtime_dir: Option<&Path>) -> OwnershipAnswer {
    OwnershipAnswer::no_keeper_on_this_target()
}

/// Where a rollback finds the machine's staged generations and where it writes the job plists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RollbackPaths {
    pub agents_dir: PathBuf,
    pub log_dir: PathBuf,
    pub runtime_root: PathBuf,
}

/// What a successful decision will change, resolved before anything is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RollbackPlan {
    pub label: String,
    pub target_executable: PathBuf,
    pub target_plist: PathBuf,
    /// Plists of other generations that still carry a load-time launch and must lose it.
    pub superseded: Vec<PathBuf>,
}

/// The staged daemon executable inside a generation directory: a helper bundle on macOS, a bare file
/// elsewhere.
fn staged_executable(generation_dir: &Path) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let bundle = generation_dir
            .join(crate::daemon::runtime_stage::HELPER_BUNDLE_NAME)
            .join("Contents/MacOS")
            .join(crate::daemon::runtime_stage::HELPER_EXECUTABLE);
        if bundle.is_file() {
            return Some(bundle);
        }
    }
    let bare = generation_dir.join(if cfg!(windows) { "ferryx.exe" } else { "ferryx" });
    bare.is_file().then_some(bare)
}

/// Resolves the target generation and the plists a rollback touches, without writing anything.
///
/// A generation key is `<version>-<8 hex>` (`daemon::runtime_stage::generation_key`), so a target is
/// present exactly when a directory with that prefix holds a runnable daemon. Nothing is staged here:
/// a version this machine never installed is a typed refusal, not a download.
pub fn plan_rollback(version: &str, paths: &RollbackPaths) -> Result<RollbackPlan, RollbackRefusal> {
    let entries = std::fs::read_dir(&paths.runtime_root).map_err(|error| {
        RollbackRefusal::TargetUnavailable {
            target_version: version.to_string(),
            detail: format!(
                "no staged daemon runtime at {} ({error})",
                paths.runtime_root.display()
            ),
        }
    })?;
    let mut target: Option<(String, PathBuf)> = None;
    for entry in entries.flatten() {
        let key = entry.file_name().to_string_lossy().into_owned();
        if !entry.path().is_dir() || !key.starts_with(&format!("{version}-")) {
            continue;
        }
        if let Some(executable) = staged_executable(&entry.path()) {
            // Two generations can share a version (different builds). The newest name wins so the
            // choice is deterministic instead of filesystem-order dependent.
            if target.as_ref().is_none_or(|(existing, _)| key > *existing) {
                target = Some((key, executable));
            }
        }
    }
    let Some((key, target_executable)) = target else {
        return Err(RollbackRefusal::TargetUnavailable {
            target_version: version.to_string(),
            detail: format!(
                "no staged daemon runtime for {version} under {}",
                paths.runtime_root.display()
            ),
        });
    };

    let base = label_base();
    let label = crate::daemon::launchd::generation::generation_label(&base, &key);
    let target_plist = paths.agents_dir.join(format!("{label}.plist"));
    let mut superseded = Vec::new();
    // `generation_plists` returns plist labels, not generation keys: the label already carries the
    // base prefix, so the target is excluded by label and every other generation becomes superseded.
    if let Ok(plists) = crate::daemon::launchd::generation::generation_plists(&paths.agents_dir, &base) {
        for (other_label, plist_path) in plists {
            if other_label == label || plist_path == target_plist {
                continue;
            }
            superseded.push(plist_path);
        }
    }

    Ok(RollbackPlan {
        label,
        target_executable,
        target_plist,
        superseded,
    })
}

/// The job label base: the QA override when a harness set one, else the production label.
fn label_base() -> String {
    std::env::var(crate::daemon::supervisor::SUPERVISOR_NAME_ENV)
        .ok()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| crate::daemon::launchd::PLIST_LABEL.to_string())
}

/// Applies a resolved plan: the target generation gets the login launch, the others lose it.
///
/// Every step is a plist rewrite. No `launchctl` call is made, so no job is started, stopped or
/// kicked, and a running daemon keeps its PTYs. Returns whether any file changed.
///
/// The rewritten plist comes from [`crate::daemon::launchd::generate_launchd_plist`], so a rolled
/// back generation keeps the crash supervisor as its `ProgramArguments` and is never downgraded into
/// a bare `--daemon` job: a rollback changes which binary supervises, not whether supervision
/// happens.
pub fn apply_rollback(plan: &RollbackPlan, paths: &RollbackPaths) -> Result<bool, RollbackRefusal> {
    let content = crate::daemon::launchd::generate_launchd_plist(
        &plan.label,
        &plan.target_executable.to_string_lossy(),
        &paths.log_dir,
        true,
    );
    let mut changed = crate::daemon::launchd::generation::write_plist_if_changed(
        &content,
        &paths.log_dir,
        &plan.target_plist,
    )
    .map_err(|detail| RollbackRefusal::Io { detail })?;
    for plist in &plan.superseded {
        changed |= crate::daemon::launchd::generation::disable_login_autostart(plist)
            .map_err(|detail| RollbackRefusal::Io { detail })?;
    }
    Ok(changed)
}

/// Whether this process may rewrite the user's launchd job plists for a rollback.
///
/// A dev build, or any `FERRYX_DATA_DIR`/`FERRYX_RUNTIME_DIR`/`FERRYX_SESSION_DIR` fixture, must not
/// touch the production LaunchAgents. The QA supervisor-name override lifts that, exactly as it does
/// for the supervisor itself, because it also moves the job to a QA label and the caller's paths to a
/// QA directory - so a harness can drive the real decision without reaching the user's install.
pub fn rollback_runtime_supported() -> bool {
    let isolated = ["FERRYX_DATA_DIR", "FERRYX_RUNTIME_DIR", "FERRYX_SESSION_DIR"]
        .iter()
        .any(|key| std::env::var_os(key).is_some_and(|value| !value.is_empty()));
    let qa_override = std::env::var_os(crate::daemon::supervisor::SUPERVISOR_NAME_ENV)
        .is_some_and(|value| !value.is_empty());
    qa_override
        || crate::daemon::launchd::agent_management_supported_for(
            crate::daemon::server::is_dev_runtime(),
            isolated,
        )
}

/// Everything `service rollback <version>` needs, resolved from the real machine.
///
/// `Ok(decision)` means the rollback is allowed and applied; `Err` is a typed refusal that changed
/// nothing. The dev/isolated-runtime refusal comes first because such a process must never rewrite
/// the user's LaunchAgents, whatever the ownership answer says.
pub fn run_rollback(
    version: &str,
    runtime_dir: Option<&Path>,
    executable: &Path,
) -> Result<(RollbackDecision, bool), RollbackRefusal> {
    if !rollback_runtime_supported() {
        return Err(RollbackRefusal::UnsupportedRuntime {
            detail: "this is a dev or isolated runtime, so the user's launchd jobs are not managed here"
                .to_string(),
        });
    }
    let ownership = durable_ownership(runtime_dir);
    let floor = match shipped_manifest_path(executable) {
        Some(manifest) => floor_from_manifest(&manifest),
        None => FloorAnswer::Unverified(format!(
            "no release manifest beside {}",
            executable.display()
        )),
    };
    let decision = decide_rollback(version, &ownership, &floor)?;

    let paths = real_paths()?;
    let plan = plan_rollback(version, &paths)?;
    let changed = apply_rollback(&plan, &paths)?;
    Ok((decision, changed))
}

/// The machine's real rollback paths.
fn real_paths() -> Result<RollbackPaths, RollbackRefusal> {
    use crate::daemon::launchd::{self, generation};
    let home = std::env::var_os("HOME");
    let agents_dir = generation::launch_agents_dir(
        std::env::var_os(launchd::PLIST_PATH_ENV).as_deref(),
        home.as_deref(),
    )
    .ok_or_else(|| RollbackRefusal::UnsupportedRuntime {
        detail: "cannot determine the LaunchAgents directory".to_string(),
    })?;
    let log_dir = launchd::launchd_log_dir_for(home.as_deref()).ok_or_else(|| {
        RollbackRefusal::UnsupportedRuntime {
            detail: "cannot determine the agent log directory".to_string(),
        }
    })?;
    let runtime_root =
        crate::daemon::server::daemon_runtime_root().ok_or_else(|| RollbackRefusal::UnsupportedRuntime {
            detail: "cannot determine the daemon runtime directory".to_string(),
        })?;
    Ok(RollbackPaths { agents_dir, log_dir, runtime_root })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::crash_supervisor::SUPERVISOR_ARG;

    fn keeper_sessions(count: usize) -> OwnershipAnswer {
        OwnershipAnswer::Known(LiveOwnership { keeper_owned: count, direct_owned: 0, unknown: 0 })
    }

    fn mixed() -> OwnershipAnswer {
        OwnershipAnswer::Known(LiveOwnership {
            keeper_owned: 2,
            direct_owned: 3,
            unknown: 0,
        })
    }

    /// The plan's acceptance: a live keeper session refuses the rollback with its count in the
    /// message, and the same rollback passes once that session is gone.
    #[test]
    fn a_keeper_session_refuses_and_its_absence_allows() {
        let refusal = decide_rollback("2026.1013.2", &keeper_sessions(1), &FloorAnswer::NoFloorRecorded)
            .expect_err("one keeper session must refuse");
        assert_eq!(refusal.code(), ROLLBACK_REFUSED_KEEPER_SESSIONS);
        assert_eq!(refusal.keeper_session_count(), Some(1));
        assert!(refusal.to_string().contains('1'), "{refusal}");

        let decision = decide_rollback("2026.1013.2", &keeper_sessions(0), &FloorAnswer::NoFloorRecorded)
            .expect("no keeper session means the rollback is allowed");
        assert!(!decision.supports_keeper);
        assert_eq!(decision.keeper_owned, 0);
    }

    /// Direct-owned sessions never block: a pre-keeper build services them exactly as this one does,
    /// and blocking on them would close the upgrade path for a user who never had a keeper session.
    #[test]
    fn direct_owned_sessions_alone_never_refuse() {
        let only_direct = OwnershipAnswer::Known(LiveOwnership {
            keeper_owned: 0,
            direct_owned: 5,
            unknown: 0,
        });
        let decision =
            decide_rollback("2026.1013.2", &only_direct, &FloorAnswer::NoFloorRecorded).expect("allowed");
        assert_eq!(decision.direct_owned, 5);
    }

    /// Both ownership types at once: the refusal names the keeper count, not the total.
    #[test]
    fn mixed_ownership_names_the_keeper_count_not_the_total() {
        let refusal = decide_rollback("2026.1013.2", &mixed(), &FloorAnswer::NoFloorRecorded)
            .expect_err("refused while a keeper session lives");
        assert_eq!(refusal.keeper_session_count(), Some(2));
        assert!(refusal.to_string().contains("2 session(s)"), "{refusal}");
    }

    /// An unknown owner label fails closed: a build that cannot read the label cannot know whether the
    /// keeper owns that session.
    #[test]
    fn an_unrecognized_owner_fails_closed() {
        let unknown = OwnershipAnswer::Known(LiveOwnership { keeper_owned: 0, direct_owned: 0, unknown: 1 });
        let refusal = decide_rollback("2026.1013.2", &unknown, &FloorAnswer::NoFloorRecorded)
            .expect_err("unknown ownership must refuse");
        assert_eq!(refusal.code(), ROLLBACK_OWNERSHIP_UNKNOWN);
    }

    /// An unreadable table is not an empty one. This is the fail-closed case the plan calls out.
    #[test]
    fn an_unreadable_ownership_table_refuses() {
        let refusal = decide_rollback(
            "2026.1013.2",
            &OwnershipAnswer::Unverified("pty-ownership.json is not JSON".to_string()),
            &FloorAnswer::NoFloorRecorded,
        )
        .expect_err("unverified ownership must refuse");
        assert_eq!(refusal.code(), ROLLBACK_OWNERSHIP_UNKNOWN);
        assert!(refusal.to_string().contains("not JSON"), "{refusal}");
    }

    /// A manifest that states no floor is not a permissive manifest: the minimum safe build is
    /// unknown, so nothing may be installed.
    #[test]
    fn an_unverified_floor_refuses_even_with_no_sessions() {
        let refusal = decide_rollback(
            "2026.1013.2",
            &keeper_sessions(0),
            &FloorAnswer::Unverified("no release manifest beside the executable".to_string()),
        )
        .expect_err("an unknown floor must refuse");
        assert_eq!(refusal.code(), ROLLBACK_FLOOR_UNVERIFIED);
    }

    /// A recorded floor is a hard minimum, and it is what makes a target keeper-capable.
    #[test]
    fn the_floor_bounds_the_target_and_licenses_keeper_support() {
        let floor = FloorAnswer::Recorded("2026.1116.1".to_string());
        let below = decide_rollback("2026.1013.2", &keeper_sessions(0), &floor)
            .expect_err("below the floor");
        assert_eq!(below.code(), ROLLBACK_BELOW_FLOOR);

        let at = decide_rollback("2026.1116.1", &keeper_sessions(3), &floor)
            .expect("a keeper-capable target may be installed with keeper sessions live");
        assert!(at.supports_keeper);

        let above = decide_rollback("2026.1117.0", &keeper_sessions(3), &floor).expect("above the floor");
        assert!(above.supports_keeper);
    }

    /// A malformed floor is unreadable, not zero: an unparsable minimum must never admit any build.
    #[test]
    fn a_malformed_floor_refuses() {
        let refusal = decide_rollback(
            "2026.1116.1",
            &keeper_sessions(0),
            &FloorAnswer::Recorded("keeper".to_string()),
        )
        .expect_err("a non-version floor must refuse");
        assert_eq!(refusal.code(), ROLLBACK_FLOOR_UNVERIFIED);
    }

    #[test]
    fn a_target_that_is_not_a_version_is_refused() {
        for bad in ["", "latest", "2026.11", "-1.2.3"] {
            let refusal = decide_rollback(bad, &keeper_sessions(0), &FloorAnswer::NoFloorRecorded)
                .expect_err("not a version");
            assert_eq!(refusal.code(), ROLLBACK_TARGET_UNAVAILABLE, "{bad}");
        }
    }

    #[test]
    fn manifest_floor_is_read_from_the_keeper_block_only() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = dir.path().join("latest.json");
        std::fs::write(
            &manifest,
            r#"{"version":"2026.1117.1","keeper":{"oldestSafeRollbackVersion":"2026.1116.1"}}"#,
        )
        .unwrap();
        assert_eq!(
            floor_from_manifest(&manifest),
            FloorAnswer::Recorded("2026.1116.1".to_string())
        );

        std::fs::write(&manifest, r#"{"version":"2026.1116.1"}"#).unwrap();
        assert_eq!(floor_from_manifest(&manifest), FloorAnswer::NoFloorRecorded);

        std::fs::write(&manifest, r#"{"keeper":{}}"#).unwrap();
        assert!(matches!(floor_from_manifest(&manifest), FloorAnswer::Unverified(_)));

        std::fs::write(&manifest, "not json").unwrap();
        assert!(matches!(floor_from_manifest(&manifest), FloorAnswer::Unverified(_)));

        assert!(matches!(
            floor_from_manifest(&dir.path().join("absent.json")),
            FloorAnswer::Unverified(_)
        ));
    }

    fn stage_generation(root: &Path, key: &str) -> PathBuf {
        let dir = root.join(key);
        std::fs::create_dir_all(&dir).unwrap();
        let executable = if cfg!(windows) { dir.join("ferryx.exe") } else { dir.join("ferryx") };
        std::fs::write(&executable, b"#!/bin/sh\n").unwrap();
        executable
    }

    /// A rollback resolves the newest staged generation for the version and names the plists it will
    /// supersede; it writes nothing until `apply_rollback` runs.
    #[test]
    fn a_plan_resolves_the_newest_generation_and_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("daemon-runtime");
        let agents = dir.path().join("LaunchAgents");
        let logs = dir.path().join("Logs");
        std::fs::create_dir_all(&agents).unwrap();
        std::fs::create_dir_all(&logs).unwrap();
        let older = stage_generation(&root, "2026.1013.2-aaaaaaaa");
        let newer = stage_generation(&root, "2026.1013.2-bbbbbbbb");
        stage_generation(&root, "2026.1116.1-cccccccc");

        let paths = RollbackPaths {
            agents_dir: agents.clone(),
            log_dir: logs.clone(),
            runtime_root: root.clone(),
        };
        let plan = plan_rollback("2026.1013.2", &paths).expect("a staged generation exists");
        assert_eq!(plan.target_executable, newer);
        assert_ne!(plan.target_executable, older);
        assert!(plan.label.ends_with("2026.1013.2-bbbbbbbb"), "{}", plan.label);
        assert!(plan.superseded.is_empty());
        assert!(
            std::fs::read_dir(&agents).unwrap().next().is_none(),
            "planning must not write a plist"
        );
    }

    /// Applying the plan gives the target the login launch and takes it away from the others, and it
    /// never touches a running daemon: only plists change.
    #[test]
    fn applying_a_plan_moves_the_login_launch_by_plist_rewrite_only() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("daemon-runtime");
        let agents = dir.path().join("LaunchAgents");
        let logs = dir.path().join("Logs");
        std::fs::create_dir_all(&agents).unwrap();
        std::fs::create_dir_all(&logs).unwrap();
        let old_executable = stage_generation(&root, "2026.1013.2-aaaaaaaa");
        let new_executable = stage_generation(&root, "2026.1013.2-bbbbbbbb");
        let paths = RollbackPaths {
            agents_dir: agents.clone(),
            log_dir: logs.clone(),
            runtime_root: root.clone(),
        };

        // The installed (old) generation currently owns the login launch.
        let old_label =
            crate::daemon::launchd::generation::generation_label(&label_base(), "2026.1013.2-aaaaaaaa");
        let old_plist = agents.join(format!("{old_label}.plist"));
        let old_content = crate::daemon::launchd::generate_launchd_plist(
            &old_label,
            &old_executable.to_string_lossy(),
            &logs,
            true,
        );
        std::fs::write(&old_plist, &old_content).unwrap();

        let plan = plan_rollback("2026.1013.2", &paths).expect("a staged generation exists");
        assert_eq!(plan.target_executable, new_executable);
        assert_eq!(plan.superseded, vec![old_plist.clone()]);

        assert!(apply_rollback(&plan, &paths).expect("plists are writable"));
        let installed = std::fs::read_to_string(&plan.target_plist).unwrap();
        assert!(installed.contains(&new_executable.to_string_lossy().into_owned()));
        assert!(installed.contains("<key>RunAtLoad</key>\n    <true/>"), "{installed}");
        let superseded = std::fs::read_to_string(&old_plist).unwrap();
        assert!(
            !superseded.contains("<key>RunAtLoad</key>\n    <true/>"),
            "the superseded generation loses the login launch:\n{superseded}"
        );
        assert!(
            superseded.contains("<key>KeepAlive</key>\n    <false/>"),
            "it keeps no abnormal-exit keep-alive either:\n{superseded}"
        );
        assert_eq!(
            crate::daemon::launchd::supervisor_chain_from_plist(&superseded),
            Some(vec![SUPERVISOR_ARG.to_string()]),
            "losing the login launch must not drop the supervisor chain:\n{superseded}"
        );

        // Idempotent: a second apply changes nothing.
        assert!(!apply_rollback(&plan, &paths).expect("second apply"));
    }

    /// A rollback rewrites the job in place, so it is the one path that can silently change what
    /// launchd runs. It must land on the same supervisor chain the forward install writes: a
    /// downgrade to `<exe> --daemon` would drop crash supervision for every rolled-back user, and it
    /// would do so invisibly, because the plist still names a plausible executable.
    #[test]
    fn a_rolled_back_job_still_runs_the_supervisor_and_never_a_bare_daemon() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("daemon-runtime");
        let agents = dir.path().join("LaunchAgents");
        let logs = dir.path().join("Logs");
        std::fs::create_dir_all(&agents).unwrap();
        std::fs::create_dir_all(&logs).unwrap();
        let old_executable = stage_generation(&root, "2026.1013.2-aaaaaaaa");
        stage_generation(&root, "2026.1116.1-cccccccc");
        let paths = RollbackPaths {
            agents_dir: agents.clone(),
            log_dir: logs.clone(),
            runtime_root: root.clone(),
        };
        let old_label =
            crate::daemon::launchd::generation::generation_label(&label_base(), "2026.1013.2-aaaaaaaa");
        let old_plist = agents.join(format!("{old_label}.plist"));
        std::fs::write(
            &old_plist,
            crate::daemon::launchd::generate_launchd_plist(
                &old_label,
                &old_executable.to_string_lossy(),
                &logs,
                true,
            ),
        )
        .unwrap();

        let plan = plan_rollback("2026.1116.1", &paths).expect("a newer generation is staged");
        apply_rollback(&plan, &paths).expect("plists are writable");

        let written = std::fs::read_to_string(&plan.target_plist).unwrap();
        let arguments = crate::daemon::launchd::plist_program_arguments(&written)
            .unwrap_or_else(|| panic!("a rolled-back job still names its program:\n{written}"));
        assert_eq!(
            arguments.len(),
            2,
            "the chain is the executable plus exactly one argument:\n{written}"
        );
        assert_eq!(
            arguments[0],
            plan.target_executable.to_string_lossy(),
            "the rolled-back executable is the one just installed:\n{written}"
        );
        assert_eq!(
            crate::daemon::launchd::supervisor_chain_from_plist(&written),
            Some(vec![SUPERVISOR_ARG.to_string()]),
            "a rollback must keep crash supervision, not downgrade the job:\n{written}"
        );
        assert!(
            !written.contains("<string>--daemon</string>"),
            "a bare daemon chain is the shape this test exists to forbid:\n{written}"
        );
        for plist in &plan.superseded {
            let superseded = std::fs::read_to_string(plist).unwrap();
            assert!(
                !superseded.contains("<key>RunAtLoad</key>\n    <true/>"),
                "the superseded generation loses the login launch, not its supervisor chain:\n{superseded}"
            );
            assert_eq!(
                crate::daemon::launchd::supervisor_chain_from_plist(&superseded),
                Some(vec![SUPERVISOR_ARG.to_string()]),
                "a disabled job keeps the chain it would run again if re-enabled:\n{superseded}"
            );
        }
    }

    /// A version this machine never staged is a refusal, and the refusal is the one an operator can
    /// act on: install that build first.
    #[test]
    fn an_unstaged_target_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("daemon-runtime");
        std::fs::create_dir_all(root.join("2026.1116.1-aaaaaaaa")).unwrap();
        let paths = RollbackPaths {
            agents_dir: dir.path().join("LaunchAgents"),
            log_dir: dir.path().join("Logs"),
            runtime_root: root,
        };
        let refusal =
            plan_rollback("2026.1013.2", &paths).expect_err("that version was never staged");
        assert_eq!(refusal.code(), ROLLBACK_TARGET_UNAVAILABLE);
        assert!(!paths.agents_dir.exists(), "a refusal writes nothing");
    }

    #[test]
    fn version_grammar_matches_three_numeric_components() {
        assert!(looks_like_version("2026.1116.1"));
        assert!(looks_like_version("2026.1116.1-rc.1"));
        for bad in ["", "2026", "2026.11", "v2026.1.1", "2026.11.x"] {
            assert!(!looks_like_version(bad), "{bad}");
        }
        assert!(compare_versions("2026.1116.1", "2026.1117.0").is_lt());
        assert!(compare_versions("2026.1116.10", "2026.1116.9").is_gt());
        assert!(compare_versions("2026.1116.1", "2026.1116.1").is_eq());
    }
}
