//! Install directory: where keeper binaries live, and which generations may be deleted.
//!
//! Layout is `~/.ferryx/keeper/<version>/ferryx-ptykeeper`, deliberately **outside** the app
//! bundle, because replacing the bundle must not pull the binary out from under a running keeper or
//! the sessions it owns.
//!
//! Rules, straight from the plan:
//!
//! * A generation directory is **immutable** once planted. Installing a version that already exists
//!   is a no-op (same bytes) or a typed refusal (different bytes, since two different binaries must
//!   never answer to one version string).
//! * The last `RETAINED_GENERATIONS` versions are kept. Older ones are only removed when they hold
//!   **no sessions**; an occupied generation is never deleted, so retention can temporarily hold
//!   more than N.
//! * Meeting an unsupported keeper generation is reported as a typed
//!   [`IncompatibleGeneration`]; the caller preserves that keeper and its sessions, and never
//!   replaces it or spawns duplicate sessions alongside it.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KeeperError, KeeperErrorCode, KeeperResult};

/// How many generations are retained. Older generations still holding sessions are kept on top of
/// this, which is what makes retention safe rather than merely tidy.
pub const RETAINED_GENERATIONS: usize = 3;

/// Binary name inside each generation directory.
pub const BINARY_NAME: &str = "ferryx-ptykeeper";

/// Optional sidecar naming the published binary's SHA-256, written by the installer. Read to decide
/// whether an existing generation holds identical bytes; absent means "unknown", never "different".
pub const DIGEST_NAME: &str = "binary.sha256";

/// The lock file that serialises installations into one root. Never a generation name: the leading
/// dot keeps it out of [`looks_like_version`] and out of every generation listing.
const LOCK_NAME: &str = ".install.lock";

/// The root of the keeper install tree: `$FERRYX_KEEPER_DIR`, else `$HOME/.ferryx/keeper`.
pub fn default_install_root() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("FERRYX_KEEPER_DIR") {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir));
        }
    }
    let home = std::env::var_os("HOME")?;
    if home.is_empty() {
        return None;
    }
    Some(PathBuf::from(home).join(".ferryx").join("keeper"))
}

/// One generation directory on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generation {
    version: String,
    dir: PathBuf,
}

impl Generation {
    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn binary(&self) -> PathBuf {
        self.dir.join(BINARY_NAME)
    }

    pub fn digest(&self) -> Option<String> {
        let text = fs::read_to_string(self.dir.join(DIGEST_NAME)).ok()?;
        let trimmed = text.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_ascii_lowercase())
    }
}

/// Why a generation cannot be used by the asking daemon. Typed, and every variant names what the
/// caller must do: preserve the keeper and its sessions, and refuse rather than replace.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IncompatibleGeneration {
    #[error(
        "keeper generation {version} speaks protocol {major}.{minor}, outside the supported {supported_major}.x window; \
         its {sessions} session(s) are preserved and it is not replaced"
    )]
    ProtocolMajor {
        version: String,
        major: u16,
        minor: u16,
        supported_major: u16,
        sessions: usize,
    },
    #[error(
        "keeper generation {version} reports an unreadable protocol version ({detail}); it is preserved and not replaced"
    )]
    Unknown { version: String, detail: String },
    #[error(
        "keeper generation {version} is occupied by {sessions} session(s) and is inside the retained window, \
         so it is preserved; install again after those sessions end"
    )]
    Occupied { version: String, sessions: usize },
}

/// What an install did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallOutcome {
    /// New generation directory written.
    Installed(Generation),
    /// The generation already existed with identical bytes; nothing was written.
    AlreadyPresent(Generation),
}

impl InstallOutcome {
    pub fn generation(&self) -> &Generation {
        match self {
            Self::Installed(generation) | Self::AlreadyPresent(generation) => generation,
        }
    }
}

/// What a prune did. `retained` is what exists afterwards, newest first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PruneReport {
    pub removed: Vec<String>,
    pub retained: Vec<String>,
    /// Generations that would otherwise be pruned but still hold sessions.
    pub preserved_occupied: Vec<String>,
}

/// The install tree. Holds no open handles; every method re-reads the directory so a second keeper
/// process planting a generation is observed rather than assumed away.
pub struct InstallDir {
    root: PathBuf,
    retained: usize,
}

impl InstallDir {
    pub fn new(root: impl Into<PathBuf>, retained: usize) -> Self {
        Self {
            root: root.into(),
            retained: retained.max(1),
        }
    }

    /// The install tree for this user, or a typed error when `$HOME` is unset.
    pub fn for_user() -> KeeperResult<Self> {
        let root = default_install_root().ok_or_else(|| {
            KeeperError::new(
                KeeperErrorCode::InvalidSpawn,
                "cannot resolve the keeper install directory: neither FERRYX_KEEPER_DIR nor HOME is set",
            )
        })?;
        Ok(Self::new(root, RETAINED_GENERATIONS))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn retained_count(&self) -> usize {
        self.retained
    }

    /// Every generation present, newest first by descending version.
    pub fn generations(&self) -> KeeperResult<Vec<Generation>> {
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(KeeperError::new(
                    KeeperErrorCode::InvalidSpawn,
                    format!("cannot read keeper install root {}: {error}", self.root.display()),
                ))
            }
        };
        let mut found = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| {
                KeeperError::new(
                    KeeperErrorCode::InvalidSpawn,
                    format!("cannot read keeper install root {}: {error}", self.root.display()),
                )
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
            found.push(Generation {
                version,
                dir: entry.path(),
            });
        }
        found.sort_by(|left, right| compare_versions(&right.version, &left.version));
        Ok(found)
    }

    /// Resolves the newest generation that is not incompatible for the caller.
    pub fn newest_compatible(
        &self,
        compatible: impl Fn(&Generation) -> Result<(), IncompatibleGeneration>,
    ) -> KeeperResult<Option<Generation>> {
        for generation in self.generations()? {
            match compatible(&generation) {
                Ok(()) => return Ok(Some(generation)),
                Err(reason) => {
                    // Preserve and keep looking: an older compatible generation is a valid answer,
                    // and the incompatible one is never touched.
                    let _ = reason;
                    continue;
                }
            }
        }
        Ok(None)
    }

    /// Plants `binary` (and its digest, when given) at `<root>/<version>/`.
    ///
    /// Idempotent on identical bytes. A directory that exists with a *different* binary is a typed
    /// refusal: the version string is an identity, and two binaries behind one identity would make
    /// "which keeper owns this session" unanswerable.
    ///
    /// Two properties the previous version lacked, both of which matter because two app instances can
    /// launch at once:
    ///
    /// * the whole installation is **serialised** by a lock file in the root, so the read-decide-write
    ///   sequence is one critical section rather than a check-then-act;
    /// * the publish **never overwrites** an existing final binary. Staging is a unique
    ///   `create_new(true)` file (pid plus a process-wide counter), and it is published with
    ///   `fs::hard_link`, which fails with `EEXIST` instead of replacing. A loser therefore observes
    ///   the winner's bytes and answers `AlreadyPresent` or the immutable-generation refusal - it can
    ///   never mutate a binary another process has already started.
    pub fn install(
        &self,
        version: &str,
        binary: &Path,
        digest: Option<&str>,
    ) -> KeeperResult<InstallOutcome> {
        // The *whole* string is validated, not only its numeric prefix: `1.2.3-x/../../outside`
        // passed the old check and was then joined onto the root, walking out of the tree.
        validate_version(version)?;
        let incoming = fs::read(binary).map_err(|error| {
            KeeperError::new(
                KeeperErrorCode::InvalidSpawn,
                format!("cannot read keeper binary {}: {error}", binary.display()),
            )
        })?;
        let target = self.root.join(version);
        let target_binary = target.join(BINARY_NAME);
        let generation = Generation {
            version: version.to_owned(),
            dir: target.clone(),
        };

        // Cross-process exclusion for the whole read-decide-publish sequence. Held for the rest of
        // this call, so no second installer can observe "absent" while this one is publishing.
        let _lock = self.lock_root()?;

        if let Ok(existing) = fs::read(&target_binary) {
            if existing == incoming {
                return Ok(InstallOutcome::AlreadyPresent(generation));
            }
            return Err(KeeperError::new(
                KeeperErrorCode::InvalidSpawn,
                format!(
                    "keeper generation {version} already exists at {} with different bytes; \
                     a generation is immutable and is never replaced",
                    target.display()
                ),
            ));
        }

        create_dir_private(&target)?;
        // A unique staging name per installer: a shared `.partial` let one installer overwrite the
        // file another had open, so the bytes being published were not necessarily the bytes that
        // were verified. `create_new(true)` also refuses to reuse a leftover from a crashed run.
        let staging = target.join(format!(
            "{BINARY_NAME}.partial.{}.{}",
            std::process::id(),
            next_staging_counter()
        ));
        let mut staged = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staging)
            .map_err(|error| {
                KeeperError::new(
                    KeeperErrorCode::InvalidSpawn,
                    format!("cannot create the staging file {}: {error}", staging.display()),
                )
            })?;
        let write_result = io::Write::write_all(&mut staged, &incoming)
            .and_then(|()| io::Write::flush(&mut staged))
            .and_then(|()| staged.sync_all());
        drop(staged);
        if let Err(error) = write_result {
            let _ = fs::remove_file(&staging);
            return Err(KeeperError::new(
                KeeperErrorCode::InvalidSpawn,
                format!("cannot write keeper binary to {}: {error}", staging.display()),
            ));
        }
        if let Err(error) = set_executable(&staging) {
            let _ = fs::remove_file(&staging);
            return Err(error);
        }
        // `hard_link` publishes with no-replace semantics: it fails with `EEXIST` when the final name
        // already exists, where `rename` would silently replace it.
        match fs::hard_link(&staging, &target_binary) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                // Another installer won between the read above and this link. Its bytes are the
                // published ones; ours must match, or the generation is not the one we verified.
                let existing = fs::read(&target_binary).unwrap_or_default();
                let _ = fs::remove_file(&staging);
                if existing == incoming {
                    return Ok(InstallOutcome::AlreadyPresent(generation));
                }
                return Err(KeeperError::new(
                    KeeperErrorCode::InvalidSpawn,
                    format!(
                        "keeper generation {version} was published concurrently with different bytes at {}; \
                         a generation is immutable and is never replaced",
                        target.display()
                    ),
                ));
            }
            Err(error) => {
                let _ = fs::remove_file(&staging);
                return Err(KeeperError::new(
                    KeeperErrorCode::InvalidSpawn,
                    format!(
                        "cannot move keeper binary into place at {}: {error}",
                        target_binary.display()
                    ),
                ));
            }
        }
        let _ = fs::remove_file(&staging);
        if let Some(digest) = digest {
            fs::write(target.join(DIGEST_NAME), format!("{}\n", digest.trim().to_ascii_lowercase()))
                .map_err(|error| {
                    KeeperError::new(
                        KeeperErrorCode::InvalidSpawn,
                        format!("cannot record the keeper digest in {}: {error}", target.display()),
                    )
                })?;
        }
        Ok(InstallOutcome::Installed(generation))
    }

    /// Takes the root's installation lock, creating the root if needed.
    ///
    /// An advisory `flock` on a file in the root: advisory is enough because both sides are this
    /// crate, and a lock file in the root is the one path every installer of this tree shares. The
    /// lock is released by `Drop` when the returned file is dropped, including on an unwind.
    fn lock_root(&self) -> KeeperResult<fs::File> {
        create_dir_private(&self.root)?;
        let path = self.root.join(LOCK_NAME);
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|error| {
                KeeperError::new(
                    KeeperErrorCode::InvalidSpawn,
                    format!("cannot open the keeper install lock {}: {error}", path.display()),
                )
            })?;
        lock_exclusive(&file).map_err(|error| {
            KeeperError::new(
                KeeperErrorCode::InvalidSpawn,
                format!("cannot lock the keeper install root {}: {error}", path.display()),
            )
        })?;
        Ok(file)
    }

    /// Prunes generations beyond `retained`, newest first.
    ///
    /// `occupied` answers "does this generation still own sessions". The plan's rule is absolute:
    /// an occupied generation is **never** deleted, even when it is older than the retention
    /// window. Passing a closure rather than a set keeps the caller's live registry authoritative
    /// at the moment of pruning.
    pub fn prune(&self, occupied: impl Fn(&Generation) -> usize) -> KeeperResult<PruneReport> {
        let generations = self.generations()?;
        let mut report = PruneReport {
            removed: Vec::new(),
            retained: Vec::new(),
            preserved_occupied: Vec::new(),
        };
        for (index, generation) in generations.iter().enumerate() {
            let sessions = occupied(generation);
            let inside_window = index < self.retained;
            if inside_window || sessions > 0 {
                if sessions > 0 && !inside_window {
                    report.preserved_occupied.push(generation.version.clone());
                }
                report.retained.push(generation.version.clone());
                continue;
            }
            match fs::remove_dir_all(generation.dir()) {
                Ok(()) => report.removed.push(generation.version.clone()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    // Raced with another pruner: already gone, nothing to report.
                }
                Err(error) => {
                    return Err(KeeperError::new(
                        KeeperErrorCode::InvalidSpawn,
                        format!(
                            "cannot remove keeper generation {} at {}: {error}",
                            generation.version,
                            generation.dir().display()
                        ),
                    ))
                }
            }
        }
        Ok(report)
    }

    /// Removes a generation outright. Refuses while it is occupied, for callers that need the
    /// guarantee without going through `prune`.
    ///
    /// The version is validated exactly as [`Self::install`] validates it. Without that,
    /// `remove("../../etc", 0)` - or an absolute component, which `Path::join` splices in whole -
    /// turned this into a recursive delete outside the install tree.
    pub fn remove(&self, version: &str, sessions: usize) -> KeeperResult<()> {
        validate_version(version)?;
        if sessions > 0 {
            return Err(KeeperError::new(
                KeeperErrorCode::InvalidSpawn,
                format!(
                    "keeper generation {version} still owns {sessions} session(s); it is preserved"
                ),
            ));
        }
        let dir = self.root.join(version);
        match fs::remove_dir_all(&dir) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(KeeperError::new(
                KeeperErrorCode::InvalidSpawn,
                format!("cannot remove keeper generation {version} at {}: {error}", dir.display()),
            )),
        }
    }
}

/// Version directories are `<major>.<minor>.<patch>` with optional `-pre`/`+build` suffixes.
/// Anything else in the install root (logs, `.partial` leftovers, editor droppings) is ignored
/// rather than parsed as a generation.
pub fn looks_like_version(name: &str) -> bool {
    let core = name.split(['-', '+']).next().unwrap_or("");
    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() != 3 {
        return false;
    }
    parts.iter().all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

/// Refuses anything that is not exactly one normal path component holding a version string.
///
/// [`looks_like_version`] only inspects the numeric prefix, which is enough to *list* a directory
/// but not to *join* one onto the install root: `1.2.3-x/../../outside` passed it and then escaped
/// the tree, and an absolute component would have discarded the root entirely. This is the check
/// every path-building call site uses, so the whole string - suffix included - has to be one normal
/// component built from `[A-Za-z0-9._+-]`, with no separator, no `..`, and no leading dot.
pub fn validate_version(version: &str) -> KeeperResult<()> {
    if !looks_like_version(version) {
        return Err(KeeperError::new(
            KeeperErrorCode::InvalidSpawn,
            format!("refusing to use the non-version directory name {version:?}"),
        ));
    }
    // `..` and `.` are made of allowed characters, so they are rejected explicitly rather than by
    // the character set. A leading dot is rejected for the same reason: it would name a hidden file
    // and could shadow the lock or a staging name.
    let allowed = !version.is_empty()
        && version != "."
        && version != ".."
        && !version.starts_with('.')
        && version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'+' | b'-'));
    // One normal component: a separator, a root, or any parent-dir element makes this a path, not a
    // name, and `Path::join` would honour it.
    let mut components = Path::new(version).components();
    let single_normal = matches!(components.next(), Some(std::path::Component::Normal(_)))
        && components.next().is_none();
    if !allowed || !single_normal {
        return Err(KeeperError::new(
            KeeperErrorCode::InvalidSpawn,
            format!(
                "refusing to use {version:?} as a keeper generation: it must be a single normal path \
                 component of [A-Za-z0-9._+-] with no separator and no parent reference"
            ),
        ));
    }
    Ok(())
}

/// A process-wide counter for staging names, so two installs in one process never collide.
fn next_staging_counter() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Takes an exclusive advisory lock on `file`.
#[cfg(unix)]
fn lock_exclusive(file: &fs::File) -> io::Result<()> {
    use std::os::unix::io::AsRawFd as _;
    loop {
        let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
        if result == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::Interrupted {
            continue;
        }
        return Err(error);
    }
}

/// Takes an exclusive lock on `file`. No `flock` outside Unix, so the in-process staging counter and
/// the no-replace publish are the whole guarantee there.
#[cfg(not(unix))]
fn lock_exclusive(_file: &fs::File) -> io::Result<()> {
    Ok(())
}

/// Orders two version strings by numeric components, then lexically for stability across
/// pre-release suffixes. Total and deterministic: retention depends on it.
pub fn compare_versions(left: &str, right: &str) -> std::cmp::Ordering {
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

/// Every generation directory must be traversable only by its owner: it holds an executable that
/// will own the user's PTYs, so a writable-by-others path is a privilege problem, not untidiness.
fn create_dir_private(dir: &Path) -> KeeperResult<()> {
    if let Some(parent) = dir.parent() {
        if !parent.exists() {
            create_dir_private(parent)?;
        }
    }
    match fs::create_dir(dir) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => {
            return Err(KeeperError::new(
                KeeperErrorCode::InvalidSpawn,
                format!("cannot create keeper directory {}: {error}", dir.display()),
            ))
        }
    }
    set_mode(dir, 0o700)
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> KeeperResult<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|error| {
        KeeperError::new(
            KeeperErrorCode::InvalidSpawn,
            format!("cannot set mode {mode:o} on {}: {error}", path.display()),
        )
    })
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> KeeperResult<()> {
    Ok(())
}

#[cfg(unix)]
fn set_executable(path: &Path) -> KeeperResult<()> {
    set_mode(path, 0o755)
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> KeeperResult<()> {
    Ok(())
}

/// Versions present under `root`, newest first. Convenience for callers that only need names.
pub fn installed_versions(root: &Path) -> KeeperResult<Vec<String>> {
    let installed = InstallDir::new(root, RETAINED_GENERATIONS);
    Ok(installed
        .generations()?
        .into_iter()
        .map(|generation| generation.version)
        .collect())
}

/// Refuses when a requested version is not one of `available`. Typed so the failure path always
/// names the versions that exist instead of silently substituting one.
pub fn require_generation(
    available: &[String],
    requested: &str,
) -> KeeperResult<String> {
    if available.iter().any(|version| version == requested) {
        Ok(requested.to_owned())
    } else {
        let mut known: BTreeSet<&str> = available.iter().map(String::as_str).collect();
        let joined = if known.is_empty() {
            "<none>".to_owned()
        } else {
            known
                .iter()
                .copied()
                .collect::<Vec<_>>()
                .join(", ")
        };
        known.clear();
        Err(KeeperError::new(
            KeeperErrorCode::UnknownSession,
            format!("keeper generation {requested} is not installed; installed: {joined}"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plant(root: &Path, version: &str, body: &[u8]) -> PathBuf {
        let dir = root.join(version);
        fs::create_dir_all(&dir).expect("create generation");
        let binary = dir.join(BINARY_NAME);
        fs::write(&binary, body).expect("write binary");
        binary
    }

    #[test]
    fn installing_a_new_generation_creates_an_owner_only_directory() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("built-keeper");
        fs::write(&source, b"binary-one").expect("source binary");

        let install = InstallDir::new(temp.path().join("keeper"), RETAINED_GENERATIONS);
        let outcome = install.install("0.1.0", &source, Some("abc123")).expect("install");
        let generation = outcome.generation();
        assert_eq!(generation.version(), "0.1.0");
        assert_eq!(fs::read(generation.binary()).expect("read back"), b"binary-one");
        assert_eq!(generation.digest().as_deref(), Some("abc123"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = fs::metadata(generation.dir()).expect("stat").permissions().mode() & 0o777;
            assert_eq!(mode, 0o700, "a generation directory is owner-only");
            let binary_mode =
                fs::metadata(generation.binary()).expect("stat").permissions().mode() & 0o777;
            assert_eq!(binary_mode, 0o755);
        }
    }

    #[test]
    fn installing_the_same_version_twice_is_idempotent_and_never_duplicates() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("built-keeper");
        fs::write(&source, b"binary-one").expect("source binary");
        let install = InstallDir::new(temp.path().join("keeper"), RETAINED_GENERATIONS);

        let first = install.install("0.1.0", &source, None).expect("first install");
        assert!(matches!(first, InstallOutcome::Installed(_)));
        let second = install.install("0.1.0", &source, None).expect("second install");
        assert!(matches!(second, InstallOutcome::AlreadyPresent(_)));
        assert_eq!(install.generations().expect("generations").len(), 1);
    }

    #[test]
    fn a_different_binary_under_an_existing_version_is_refused_not_replaced() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("built-keeper");
        fs::write(&source, b"binary-one").expect("source binary");
        let install = InstallDir::new(temp.path().join("keeper"), RETAINED_GENERATIONS);
        install.install("0.1.0", &source, None).expect("install");

        fs::write(&source, b"binary-two").expect("rewrite source");
        let error = install
            .install("0.1.0", &source, None)
            .expect_err("a generation is immutable");
        assert_eq!(error.code, KeeperErrorCode::InvalidSpawn);
        let binary = install.root().join("0.1.0").join(BINARY_NAME);
        assert_eq!(fs::read(binary).expect("read back"), b"binary-one");
    }

    #[test]
    fn retention_keeps_three_and_never_deletes_an_occupied_generation() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("keeper");
        for version in ["0.1.0", "0.2.0", "0.3.0", "0.4.0", "0.5.0"] {
            plant(&root, version, b"binary");
        }
        // The oldest generation still owns a session: it must survive pruning beyond the window.
        let install = InstallDir::new(&root, RETAINED_GENERATIONS);
        let report = install
            .prune(|generation| if generation.version() == "0.1.0" { 2 } else { 0 })
            .expect("prune");

        assert_eq!(report.removed, vec!["0.2.0".to_owned()]);
        assert_eq!(report.preserved_occupied, vec!["0.1.0".to_owned()]);
        assert_eq!(
            report.retained,
            vec![
                "0.5.0".to_owned(),
                "0.4.0".to_owned(),
                "0.3.0".to_owned(),
                "0.1.0".to_owned()
            ]
        );
        assert!(root.join("0.1.0").exists(), "an occupied generation is preserved");
        assert!(!root.join("0.2.0").exists());
        assert!(root.join("0.3.0").exists());
    }

    #[test]
    fn a_second_prune_removes_the_former_occupied_generation_once_it_is_free() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("keeper");
        for version in ["0.1.0", "0.2.0", "0.3.0", "0.4.0"] {
            plant(&root, version, b"binary");
        }
        let install = InstallDir::new(&root, RETAINED_GENERATIONS);
        install.prune(|generation| usize::from(generation.version() == "0.1.0")).expect("first prune");
        assert!(root.join("0.1.0").exists());

        let report = install.prune(|_| 0).expect("second prune");
        assert_eq!(report.removed, vec!["0.1.0".to_owned()]);
        assert!(!root.join("0.1.0").exists());
    }

    #[test]
    fn removal_refuses_while_the_generation_is_occupied() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("keeper");
        plant(&root, "0.1.0", b"binary");
        let install = InstallDir::new(&root, RETAINED_GENERATIONS);
        let error = install.remove("0.1.0", 3).expect_err("occupied generations are preserved");
        assert_eq!(error.code, KeeperErrorCode::InvalidSpawn);
        assert!(root.join("0.1.0").exists());
        install.remove("0.1.0", 0).expect("free generations are removable");
    }

    #[test]
    fn non_version_entries_are_ignored_by_generation_listing() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("keeper");
        plant(&root, "0.1.0", b"binary");
        fs::create_dir_all(root.join("logs")).expect("logs dir");
        fs::create_dir_all(root.join("keeper.sock.d")).expect("junk dir");
        fs::write(root.join("README"), b"notes").expect("junk file");

        let versions = installed_versions(&root).expect("versions");
        assert_eq!(versions, vec!["0.1.0".to_owned()]);
    }

    #[test]
    fn versions_sort_numerically_not_lexically() {
        assert!(compare_versions("0.10.0", "0.9.0") == std::cmp::Ordering::Greater);
        assert!(compare_versions("1.0.0", "1.0.0") == std::cmp::Ordering::Equal);
        assert!(looks_like_version("1.2.3"));
        assert!(looks_like_version("1.2.3-beta.1"));
        assert!(!looks_like_version("keeper"));
        assert!(!looks_like_version("1.2"));
        assert!(!looks_like_version(""));
    }

    #[test]
    fn a_version_that_could_leave_the_install_root_is_refused() {
        // `looks_like_version` only inspects the numeric prefix, so every one of these passed it and
        // was then joined onto the root: the suffix walked out of the tree, and an absolute path
        // discarded the root entirely.
        for hostile in [
            "1.2.3/../../outside",
            "1.2.3-x/../../outside",
            "/tmp/outside",
            "1.2.3/0.9.9",
            "..",
            ".",
            ".hidden",
            "1.2.3-x\\..\\outside",
        ] {
            let error = validate_version(hostile).expect_err(hostile);
            assert_eq!(error.code, KeeperErrorCode::InvalidSpawn, "{hostile}");
        }
        for accepted in ["0.1.0", "1.2.3-beta.1", "1.2.3+build.7", "10.20.30"] {
            validate_version(accepted).expect(accepted);
        }
    }

    #[test]
    fn install_and_remove_refuse_a_version_that_escapes_the_root() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("built-keeper");
        fs::write(&source, b"binary-one").expect("source binary");
        let root = temp.path().join("keeper");
        let install = InstallDir::new(root.clone(), RETAINED_GENERATIONS);

        let error = install
            .install("1.2.3/../../../escaped", &source, None)
            .expect_err("a path-shaped version must be refused before any write");
        assert_eq!(error.code, KeeperErrorCode::InvalidSpawn);
        assert!(!temp.path().join("escaped").exists(), "nothing was written outside the root");

        let outside = temp.path().join("outside");
        fs::create_dir_all(&outside).expect("outside dir");
        let error = install
            .remove("../outside", 0)
            .expect_err("remove must not delete outside the root");
        assert_eq!(error.code, KeeperErrorCode::InvalidSpawn);
        assert!(outside.exists(), "a refused remove deletes nothing");
    }

    #[test]
    fn an_install_leaves_no_staging_file_and_serialises_on_the_root_lock() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("built-keeper");
        fs::write(&source, b"binary-one").expect("source binary");
        let root = temp.path().join("keeper");
        let install = InstallDir::new(root.clone(), RETAINED_GENERATIONS);
        install.install("0.1.0", &source, None).expect("install");

        let leftovers: Vec<_> = fs::read_dir(root.join("0.1.0"))
            .expect("read generation")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".partial"))
            .collect();
        assert!(leftovers.is_empty(), "staging files must not survive a publish: {leftovers:?}");
        assert!(root.join(".install.lock").exists(), "the root lock is created and kept");
        assert_eq!(
            installed_versions(&root).expect("versions"),
            vec!["0.1.0".to_owned()],
            "the lock file is not a generation"
        );
    }

    #[test]
    fn an_unknown_generation_request_names_what_is_installed() {
        let available = vec!["0.1.0".to_owned(), "0.2.0".to_owned()];
        assert_eq!(require_generation(&available, "0.2.0").expect("present"), "0.2.0");
        let error = require_generation(&available, "0.9.0").expect_err("absent");
        assert!(error.message.contains("0.1.0") && error.message.contains("0.2.0"));
    }

    #[test]
    fn an_incompatible_generation_is_reported_typed_and_skipped_not_replaced() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("keeper");
        plant(&root, "0.1.0", b"old");
        plant(&root, "0.2.0", b"new");
        let install = InstallDir::new(&root, RETAINED_GENERATIONS);

        let newest = install
            .newest_compatible(|generation| {
                if generation.version() == "0.2.0" {
                    Err(IncompatibleGeneration::ProtocolMajor {
                        version: generation.version().to_owned(),
                        major: 2,
                        minor: 0,
                        supported_major: 1,
                        sessions: 4,
                    })
                } else {
                    Ok(())
                }
            })
            .expect("resolve")
            .expect("an older compatible generation exists");
        assert_eq!(newest.version(), "0.1.0");
        assert!(root.join("0.2.0").exists(), "the incompatible generation is preserved");
    }
}
