//! The relay slot pool: the finite set of account machine records a T2 box can claim.
//!
//! A T2 provider (a fresh box in someone else's cloud) cannot enroll as a machine of its own. The
//! account is at its record limit, no route deletes a record, and the relay pins each machineId's
//! public key at that machineId's first successful connection forever - so a box has to present a
//! record and a keypair that already exist. Those records are the pool, one directory per record:
//!
//! ```text
//! ~/.ferryx/cloud-slots/<record-id>/identity.json            0600, machineId + its Ed25519 keypair
//! ~/.ferryx/cloud-slots/<record-id>/attach-identity.json     0600, the relay attach keypair
//! ~/.ferryx/cloud-slots/<record-id>/account-enrollment.json  0600, account + record provenance
//! ~/.ferryx/cloud-slots/<record-id>/lease.json               0600, who holds the record right now
//! ```
//!
//! Four rules hold here:
//!
//! 1. **A slot is leased to one machine at a time.** [`SlotRegistry::lease`] takes the first free
//!    record and writes the holder's name into it; a second box is refused with
//!    [`SlotError::AlreadyLeased`], which names the holder, or gets
//!    [`SlotError::NoFreeSlot`] - `NO_RELAY_SLOT` - when every record is held. The whole pool is
//!    read and written under one `flock`, so two `cloud create` calls cannot both see a record free.
//! 2. **A slot is never re-keyed or re-enrolled here.** This module only reads `identity.json`; it
//!    never writes it, and it never asks the account for another machineId. Re-enrolling a slot
//!    would burn a record the pool cannot replace, and a new keypair under a pinned machineId would
//!    stop the record from ever connecting again.
//! 3. **The lease file is 0600 and published by rename.** As in [`super::catalog`], the temp file is
//!    created restricted rather than chmodded afterwards, fsynced, then renamed over the lease.
//! 4. **No key material leaves this module.** The identity's private key is parsed, checked for
//!    presence and dropped inside one function; [`SlotLease`] holds the record id, the holder name
//!    and the machineId, so its `Debug`, `Display` and every log line built from it are safe.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ipc::IpcError;

use super::catalog::STATE_DIR;

/// The pool's directory inside [`STATE_DIR`].
pub const SLOTS_DIR: &str = "cloud-slots";
/// The record's identity: machineId, display name and the pinned keypair.
pub const IDENTITY_FILE: &str = "identity.json";
/// The lease record this module writes.
pub const LEASE_FILE: &str = "lease.json";
/// The lease record's schema version.
pub const LEASE_VERSION: u32 = 1;
/// The `flock` file inside the pool root, held for the length of one lease or release.
const LOCK_FILE: &str = ".lease.lock";

/// The typed failure of a slot call. `code()` is the stable token a CLI or a test asserts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotError {
    /// Every slot in the pool is held by another machine. The CLI's exit-4 token.
    NoFreeSlot,
    /// The slot is held by a different machine. `holder` is the name in its lease record.
    AlreadyLeased { slot: String, holder: String },
    /// No slot directory carries that name.
    UnknownSlot(String),
    /// A slot's files are unreadable or unparsable. Reported, never treated as a free slot: a
    /// corrupt lease that read as "free" would hand one record to two machines.
    Corrupt { slot: String, detail: String },
    /// The release names a different holder, so it would steal a live slot.
    NotHolder { slot: String, holder: String },
    /// The pool could not be read or written.
    Io { action: &'static str, message: String },
}

impl SlotError {
    /// `NO_RELAY_SLOT`, `SLOT_ALREADY_LEASED`, ... - the token a CLI maps onto an exit code.
    pub fn code(&self) -> &'static str {
        match self {
            SlotError::NoFreeSlot => "NO_RELAY_SLOT",
            SlotError::AlreadyLeased { .. } => "SLOT_ALREADY_LEASED",
            SlotError::UnknownSlot(_) => "SLOT_UNKNOWN",
            SlotError::Corrupt { .. } => "SLOT_CORRUPT",
            SlotError::NotHolder { .. } => "SLOT_NOT_HOLDER",
            SlotError::Io { .. } => "SLOT_IO_ERROR",
        }
    }

    fn io(action: &'static str, path: &Path, error: &std::io::Error) -> Self {
        SlotError::Io { action, message: format!("{}: {error}", path.display()) }
    }
}

impl std::fmt::Display for SlotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SlotError::NoFreeSlot => f.write_str(
                "NO_RELAY_SLOT: every relay slot is leased; delete a cloud machine to return its \
                 slot to the pool",
            ),
            SlotError::AlreadyLeased { slot, holder } => {
                write!(f, "SLOT_ALREADY_LEASED: relay slot {slot} is leased to {holder}")
            }
            SlotError::UnknownSlot(slot) => write!(f, "SLOT_UNKNOWN: no relay slot named {slot} exists"),
            SlotError::Corrupt { slot, detail } => {
                write!(f, "SLOT_CORRUPT: relay slot {slot} cannot be read: {detail}")
            }
            SlotError::NotHolder { slot, holder } => {
                write!(f, "SLOT_NOT_HOLDER: relay slot {slot} is leased to {holder}, not to this machine")
            }
            SlotError::Io { action, message } => write!(f, "SLOT_IO_ERROR: {action}: {message}"),
        }
    }
}

impl std::error::Error for SlotError {}

/// One claimed slot: the record a box may now use, and never its key material.
///
/// `Debug` is derived on purpose: every field here is safe to log, which is why the keypair is not
/// a field. [`Display`](std::fmt::Display) prints the same facts for an operator line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotLease {
    slot: String,
    name: String,
    machine_id: String,
    display_name: Option<String>,
    dir: PathBuf,
}

impl SlotLease {
    /// The record id, which is also the slot directory's name.
    pub fn slot(&self) -> &str {
        &self.slot
    }

    /// The cloud machine holding the lease (`fx-<something>`).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The account machineId this record's keypair is pinned to.
    pub fn machine_id(&self) -> &str {
        &self.machine_id
    }

    /// The account's display name for the record, when the enrollment recorded one.
    pub fn display_name(&self) -> Option<&str> {
        self.display_name.as_deref()
    }

    /// The slot directory a provisioning step copies from.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// `identity.json` - the machineId and the keypair the relay already trusts.
    pub fn identity_path(&self) -> PathBuf {
        self.dir.join(IDENTITY_FILE)
    }

    /// `attach-identity.json` - the separate keypair the relay attach handshake signs with.
    pub fn attach_identity_path(&self) -> PathBuf {
        self.dir.join("attach-identity.json")
    }
}

impl std::fmt::Display for SlotLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "relay slot {} (machineId {}) leased to {}", self.slot, self.machine_id, self.name)
    }
}

/// The pool at `~/.ferryx/cloud-slots/`.
///
/// A value, not a global: a test (and a QA run) points it at a temp directory, so nothing here can
/// reach the operator's real pool by accident.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotRegistry {
    root: PathBuf,
}

impl SlotRegistry {
    /// A registry over an explicit pool root.
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The real location: `$HOME/.ferryx/cloud-slots`.
    pub fn default_location() -> Result<Self, SlotError> {
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).ok_or_else(|| {
            SlotError::Io {
                action: "resolve the home directory",
                message: "cannot resolve the home directory for the relay slot pool".to_string(),
            }
        })?;
        Ok(Self::at(location_in(Path::new(&home))))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Claim a slot for `name`, preferring the one it already holds.
    ///
    /// A retried `cloud create` for the same machine gets its own slot back instead of a second
    /// record, because the provider side of that create is idempotent and the box may already be
    /// half-provisioned with this machineId.
    pub async fn lease(&self, name: &str) -> Result<SlotLease, SlotError> {
        let root = self.root.clone();
        let name = name.to_string();
        off_runtime(move || acquire(&root, None, &name)).await
    }

    /// Claim one named slot, for a provisioning step that must use a record the operator picked.
    pub async fn lease_specific(&self, slot: &str, name: &str) -> Result<SlotLease, SlotError> {
        let root = self.root.clone();
        let (slot, name) = (slot.to_string(), name.to_string());
        off_runtime(move || acquire(&root, Some(&slot), &name)).await
    }

    /// The slot `name` currently holds, if any.
    pub async fn lease_of(&self, name: &str) -> Result<Option<SlotLease>, SlotError> {
        let root = self.root.clone();
        let name = name.to_string();
        off_runtime(move || holder_of(&root, &name)).await
    }

    /// Return a slot to the pool. Releasing a slot nobody holds is already true.
    pub async fn release(&self, lease: &SlotLease) -> Result<(), SlotError> {
        let root = self.root.clone();
        let (slot, name) = (lease.slot.clone(), lease.name.clone());
        off_runtime(move || release_at(&root, &slot, &name)).await
    }
}

/// `$HOME/.ferryx/cloud-slots` for a given home directory. Pure, so the join rule is testable.
pub fn location_in(home: &Path) -> PathBuf {
    home.join(STATE_DIR).join(SLOTS_DIR)
}

/// Run blocking filesystem work off the async runtime, keeping [`SlotError`].
async fn off_runtime<T, F>(operation: F) -> Result<T, SlotError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, SlotError> + Send + 'static,
{
    crate::ipc::run_blocking(move || Ok::<_, IpcError>(operation()))
        .await
        .map_err(|error| SlotError::Io { action: "run the slot operation", message: error.message })?
}

/// The lease record as it is written to disk. No key material, by construction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct LeaseRecord {
    version: u32,
    /// The cloud machine name holding the slot.
    name: String,
    #[serde(rename = "leasedAt")]
    leased_at: i64,
}

/// The fields of `identity.json` this module needs. Deliberately not `Debug`: a stray `{:?}` of
/// this struct would print the private key.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IdentityFile {
    machine_id: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    public_key: String,
    #[serde(default)]
    private_key: String,
}

/// The non-secret part of an identity, after the keypair was checked and dropped.
struct Identity {
    machine_id: String,
    display_name: Option<String>,
}

fn acquire(root: &Path, requested: Option<&str>, name: &str) -> Result<SlotLease, SlotError> {
    let _lock = RegistryLock::acquire(root)?;
    let candidates: Vec<String> = match requested {
        Some(slot) => {
            if !plain_name(slot) || !root.join(slot).join(IDENTITY_FILE).is_file() {
                return Err(SlotError::UnknownSlot(slot.to_string()));
            }
            vec![slot.to_string()]
        }
        None => slot_names(root)?,
    };

    // 1. A machine that already holds a slot keeps it: a retried create must not take a second
    //    record, and the box it is creating may already carry that machineId.
    for slot in &candidates {
        let dir = root.join(slot);
        if let Some(record) = read_lease(&dir, slot)? {
            if record.name == name {
                return build_lease(root, slot, &record);
            }
        }
    }

    // 2. The first free slot in name order, so two runs pick the same record first.
    for slot in &candidates {
        let dir = root.join(slot);
        match read_lease(&dir, slot)? {
            Some(record) => {
                if requested.is_some() {
                    return Err(SlotError::AlreadyLeased { slot: slot.clone(), holder: record.name });
                }
            }
            None => {
                // Read the identity before writing the lease: a slot whose keypair is gone must
                // never be handed to a box that would then fail to connect.
                let identity = read_identity(root, slot)?;
                // A slot directory holds a private key, so it is owner-only like the pool root -
                // best effort, as in the catalog, because a mode we cannot set must not fail a
                // lease the operator asked for.
                let _ = restrict(&dir, 0o700);
                let record = LeaseRecord { version: LEASE_VERSION, name: name.to_string(), leased_at: now_ms() };
                write_lease(&dir, &record)?;
                return Ok(SlotLease {
                    slot: slot.clone(),
                    name: record.name,
                    machine_id: identity.machine_id,
                    display_name: identity.display_name,
                    dir,
                });
            }
        }
    }
    Err(SlotError::NoFreeSlot)
}

fn holder_of(root: &Path, name: &str) -> Result<Option<SlotLease>, SlotError> {
    for slot in slot_names(root)? {
        let dir = root.join(slot.as_str());
        if let Some(record) = read_lease(&dir, &slot)? {
            if record.name == name {
                return build_lease(root, &slot, &record).map(Some);
            }
        }
    }
    Ok(None)
}

fn release_at(root: &Path, slot: &str, name: &str) -> Result<(), SlotError> {
    let _lock = RegistryLock::acquire(root)?;
    let dir = root.join(slot);
    let Some(record) = read_lease(&dir, slot)? else {
        // Already released (or the slot directory is gone): the caller's intent holds.
        return Ok(());
    };
    if record.name != name {
        return Err(SlotError::NotHolder { slot: slot.to_string(), holder: record.name });
    }
    let path = dir.join(LEASE_FILE);
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(SlotError::io("remove", &path, &error)),
    }
    // Best effort, as in the catalog: the unlink has happened, and a filesystem that refuses a
    // directory fsync must not fail the release.
    if let Ok(directory) = std::fs::File::open(&dir) {
        let _ = directory.sync_all();
    }
    Ok(())
}

/// Slot directory names, sorted. A directory without an `identity.json` is not a slot (a stray
/// directory must not turn into a lease), and neither is a dotfile or a plain file.
fn slot_names(root: &Path) -> Result<Vec<String>, SlotError> {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(SlotError::io("list", root, &error)),
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| SlotError::io("list", root, &error))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        if !path.is_dir() || !path.join(IDENTITY_FILE).is_file() {
            continue;
        }
        names.push(name);
    }
    names.sort();
    Ok(names)
}

fn read_identity(root: &Path, slot: &str) -> Result<Identity, SlotError> {
    let path = root.join(slot).join(IDENTITY_FILE);
    let bytes = std::fs::read(&path).map_err(|error| SlotError::io("read", &path, &error))?;
    let parsed: IdentityFile = serde_json::from_slice(&bytes).map_err(|error| SlotError::Corrupt {
        slot: slot.to_string(),
        detail: format!("{IDENTITY_FILE}: {error}"),
    })?;
    // From here on the keypair is only checked and dropped: nothing outside this function can
    // print it, serialize it, or copy it into a log line.
    if parsed.machine_id.trim().is_empty() {
        return Err(SlotError::Corrupt {
            slot: slot.to_string(),
            detail: format!("{IDENTITY_FILE} has no machineId"),
        });
    }
    if parsed.public_key.trim().is_empty() || parsed.private_key.trim().is_empty() {
        return Err(SlotError::Corrupt {
            slot: slot.to_string(),
            detail: format!("{IDENTITY_FILE} has no keypair; the record could not connect"),
        });
    }
    Ok(Identity { machine_id: parsed.machine_id, display_name: parsed.display_name })
}

fn read_lease(dir: &Path, slot: &str) -> Result<Option<LeaseRecord>, SlotError> {
    let path = dir.join(LEASE_FILE);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(SlotError::io("read", &path, &error)),
    };
    let record: LeaseRecord = serde_json::from_slice(&bytes).map_err(|error| SlotError::Corrupt {
        slot: slot.to_string(),
        detail: format!("{LEASE_FILE}: {error}"),
    })?;
    if record.name.trim().is_empty() {
        return Err(SlotError::Corrupt {
            slot: slot.to_string(),
            detail: format!("{LEASE_FILE} names no holder"),
        });
    }
    if record.version > LEASE_VERSION {
        return Err(SlotError::Corrupt {
            slot: slot.to_string(),
            detail: format!(
                "{LEASE_FILE} was written by a newer build (version {}, this build writes {LEASE_VERSION})",
                record.version
            ),
        });
    }
    Ok(Some(record))
}

fn build_lease(root: &Path, slot: &str, record: &LeaseRecord) -> Result<SlotLease, SlotError> {
    let identity = read_identity(root, slot)?;
    Ok(SlotLease {
        slot: slot.to_string(),
        name: record.name.clone(),
        machine_id: identity.machine_id,
        display_name: identity.display_name,
        dir: root.join(slot),
    })
}

/// A record id is one directory name: no separators, no `.`/`..`, so a caller cannot point the
/// pool at a path outside its root.
fn plain_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\')
        && !name.starts_with('.')
}

/// Write the lease through a 0600 temp file and one rename, exactly as the catalog does: the temp
/// file is created restricted rather than chmodded afterwards, so the contents are never briefly
/// group-readable, and a reader sees either the old lease or the new one.
fn write_lease(dir: &Path, record: &LeaseRecord) -> Result<(), SlotError> {
    let bytes = serde_json::to_vec_pretty(record).map_err(|error| SlotError::Io {
        action: "serialize",
        message: format!("cannot serialize the slot lease: {error}"),
    })?;
    let path = dir.join(LEASE_FILE);
    let temp = dir.join(format!("{LEASE_FILE}.tmp.{}", std::process::id()));
    write_private_atomically(&path, &temp, &bytes).map_err(|error| SlotError::io("write", &path, &error))
}

fn write_private_atomically(path: &Path, temp: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;

    {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let mut file = options.open(temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    // A umask can only clear bits, and some filesystems ignore the open mode, so state 0600
    // explicitly before the name changes hands.
    restrict(temp, 0o600)?;
    std::fs::rename(temp, path)?;
    if let Some(parent) = path.parent() {
        if let Ok(directory) = std::fs::File::open(parent) {
            let _ = directory.sync_all();
        }
    }
    Ok(())
}

#[cfg(unix)]
fn restrict(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn restrict(_path: &Path, _mode: u32) -> std::io::Result<()> {
    Ok(())
}

/// One cross-process guard per pool, held for the length of a lease or a release.
///
/// A lease is a read-modify-write over the directory, so the pool needs one lock rather than a lock
/// per slot: two creates scanning at once would otherwise both see the same record free and both
/// write a lease. `flock` is advisory, which is enough here because every writer in this process
/// and in a second `ferryx` process takes it.
struct RegistryLock {
    #[cfg(unix)]
    file: std::fs::File,
}

impl RegistryLock {
    fn acquire(root: &Path) -> Result<Self, SlotError> {
        std::fs::create_dir_all(root).map_err(|error| SlotError::io("create", root, &error))?;
        // The pool holds keypairs: only this user may traverse it.
        let _ = restrict(root, 0o700);
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd as _;

            let path = root.join(LOCK_FILE);
            let file = open_private(&path)?;
            loop {
                // SAFETY: `flock` only reads the descriptor this process owns.
                if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } == 0 {
                    break;
                }
                let error = std::io::Error::last_os_error();
                if error.kind() != std::io::ErrorKind::Interrupted {
                    return Err(SlotError::io("lock", &path, &error));
                }
            }
            Ok(Self { file })
        }
        #[cfg(not(unix))]
        {
            Ok(Self {})
        }
    }
}

/// Open (or create) `path` at mode 0600 for the lock file.
#[cfg(unix)]
fn open_private(path: &Path) -> Result<std::fs::File, SlotError> {
    use std::os::unix::fs::OpenOptionsExt as _;

    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)
        .map_err(|error| SlotError::io("open", path, &error))?;
    Ok(file)
}

#[cfg(unix)]
impl Drop for RegistryLock {
    fn drop(&mut self) {
        use std::os::unix::io::AsRawFd as _;

        // SAFETY: the descriptor is open for the whole life of `self`; unlocking it is explicit so
        // a later reader of this code does not have to know that closing releases the lock.
        unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A key that is obviously not a key, and must never appear in a lease, a log line or a file
    /// other than the identity it was planted in.
    const PLANTED_PRIVATE_KEY: &str = "planted-private-key-must-never-be-printed";

    fn pool(records: &[&str]) -> (tempfile::TempDir, SlotRegistry) {
        let dir = tempfile::tempdir().unwrap();
        let registry = SlotRegistry::at(location_in(dir.path()));
        for record in records {
            seed_slot(registry.root(), record);
        }
        (dir, registry)
    }

    fn seed_slot(root: &Path, record: &str) {
        let slot = root.join(record);
        std::fs::create_dir_all(&slot).unwrap();
        std::fs::write(slot.join(IDENTITY_FILE), identity_json(record)).unwrap();
        restrict(&slot.join(IDENTITY_FILE), 0o600).unwrap();
    }

    fn identity_json(record: &str) -> String {
        format!(
            "{{\"machineId\":\"{record}-machine\",\"displayName\":\"{record}-display\",\
             \"publicKey\":\"cHVibGljLWtleQ==\",\"privateKey\":\"{PLANTED_PRIVATE_KEY}\"}}"
        )
    }

    #[cfg(unix)]
    fn mode_of(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn default_location_is_home_dot_ferryx_cloud_slots() {
        assert_eq!(
            location_in(Path::new("/Users/someone")),
            PathBuf::from("/Users/someone/.ferryx/cloud-slots")
        );
    }

    #[tokio::test]
    async fn a_lease_hands_out_a_slot_and_records_the_holder() {
        let (_dir, registry) = pool(&["record-1"]);
        let lease = registry.lease("fx-a").await.expect("lease");

        assert_eq!(lease.slot(), "record-1");
        assert_eq!(lease.name(), "fx-a");
        assert_eq!(lease.machine_id(), "record-1-machine");
        assert_eq!(lease.display_name(), Some("record-1-display"));
        assert_eq!(lease.dir(), registry.root().join("record-1"));
        assert!(lease.identity_path().ends_with("record-1/identity.json"));

        let record = read_lease(lease.dir(), "record-1").expect("read").expect("a lease");
        assert_eq!(record.name, "fx-a");
        assert_eq!(record.version, LEASE_VERSION);
        assert!(record.leased_at > 0);
    }

    #[tokio::test]
    async fn two_machines_get_two_different_slots() {
        let (_dir, registry) = pool(&["record-1", "record-2"]);
        let first = registry.lease("fx-a").await.expect("first");
        let second = registry.lease("fx-b").await.expect("second");

        assert_eq!(first.slot(), "record-1", "the pool hands out the first record in name order");
        assert_eq!(second.slot(), "record-2");
        assert_ne!(first.machine_id(), second.machine_id());
    }

    #[tokio::test]
    async fn leasing_a_slot_someone_else_holds_is_refused_with_the_holder_name() {
        let (_dir, registry) = pool(&["record-1"]);
        registry.lease_specific("record-1", "fx-a").await.expect("first holder");

        let error = registry.lease_specific("record-1", "fx-b").await.expect_err("double lease");
        assert_eq!(error.code(), "SLOT_ALREADY_LEASED");
        assert_eq!(
            error,
            SlotError::AlreadyLeased { slot: "record-1".into(), holder: "fx-a".into() }
        );
        assert!(error.to_string().contains("fx-a"), "the refusal names the holder: {error}");
    }

    #[tokio::test]
    async fn no_relay_slot_when_every_slot_is_leased() {
        let (_dir, registry) = pool(&["record-1", "record-2"]);
        registry.lease("fx-a").await.expect("first");
        registry.lease("fx-b").await.expect("second");

        let error = registry.lease("fx-c").await.expect_err("the pool is empty");
        assert_eq!(error.code(), "NO_RELAY_SLOT");
        assert!(error.to_string().starts_with("NO_RELAY_SLOT:"));
    }

    #[tokio::test]
    async fn no_relay_slot_when_the_pool_holds_no_slot() {
        let (dir, registry) = pool(&[]);

        let error = registry.lease("fx-a").await.expect_err("an empty pool");
        assert_eq!(error.code(), "NO_RELAY_SLOT");
        // The pool root gains only its lock file: no slot, and above all no lease.
        let entries: Vec<String> = std::fs::read_dir(registry.root())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(entries, vec![LOCK_FILE.to_string()], "nothing but the lock file was created");
        assert!(!location_in(dir.path()).join(LEASE_FILE).exists());
    }

    #[tokio::test]
    async fn release_returns_the_slot_to_the_pool() {
        let (_dir, registry) = pool(&["record-1"]);
        let lease = registry.lease("fx-a").await.expect("lease");
        assert!(registry.lease_of("fx-a").await.unwrap().is_some());

        registry.release(&lease).await.expect("release");

        assert!(registry.lease_of("fx-a").await.unwrap().is_none());
        assert!(!lease.dir().join(LEASE_FILE).exists());
        assert!(lease.identity_path().exists(), "the record itself stays, keypair and all");
        let next = registry.lease("fx-b").await.expect("the slot is free again");
        assert_eq!(next.slot(), "record-1");
    }

    #[tokio::test]
    async fn release_by_another_machine_is_refused() {
        let (_dir, registry) = pool(&["record-1"]);
        let lease = registry.lease("fx-a").await.expect("lease");
        let mut stolen = lease.clone();
        stolen.name = "fx-b".to_string();

        let error = registry.release(&stolen).await.expect_err("not the holder");
        assert_eq!(error.code(), "SLOT_NOT_HOLDER");
        assert!(error.to_string().contains("fx-a"), "{error}");
        assert!(registry.lease_of("fx-a").await.unwrap().is_some(), "the live lease survives");
    }

    #[tokio::test]
    async fn release_twice_is_idempotent() {
        let (_dir, registry) = pool(&["record-1"]);
        let lease = registry.lease("fx-a").await.expect("lease");

        registry.release(&lease).await.expect("first release");
        registry.release(&lease).await.expect("a second release is already true");
    }

    #[tokio::test]
    async fn a_retried_lease_for_the_same_machine_returns_its_own_slot() {
        let (_dir, registry) = pool(&["record-1", "record-2"]);
        let first = registry.lease("fx-a").await.expect("lease");
        let retried = registry.lease("fx-a").await.expect("a retried create keeps its slot");

        assert_eq!(retried.slot(), first.slot(), "the same machine never takes a second record");
        assert_eq!(registry.lease("fx-b").await.unwrap().slot(), "record-2");

        // The named form is idempotent the same way.
        let named = registry.lease_specific("record-1", "fx-a").await.expect("its own slot");
        assert_eq!(named.slot(), "record-1");
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn the_lease_file_is_0600_and_the_identity_file_is_never_rewritten() {
        let (_dir, registry) = pool(&["record-1"]);
        let identity = registry.root().join("record-1").join(IDENTITY_FILE);
        let before = std::fs::read(&identity).unwrap();
        let mode_before = mode_of(&identity);
        let leased_at_before = std::fs::metadata(&identity).unwrap().modified().unwrap();

        let lease = registry.lease("fx-a").await.expect("lease");
        let lease_path = lease.dir().join(LEASE_FILE);
        assert_eq!(mode_of(&lease_path), 0o600, "the lease is owner-only");
        assert_eq!(mode_of(lease.dir()), 0o700, "the pool stays owner-only");
        assert_eq!(std::fs::read(&identity).unwrap(), before, "the keypair is never rewritten");
        assert_eq!(mode_of(&identity), mode_before);
        assert_eq!(std::fs::metadata(&identity).unwrap().modified().unwrap(), leased_at_before);

        registry.release(&lease).await.expect("release");
        assert_eq!(std::fs::read(&identity).unwrap(), before, "release never touches the keypair");
        // The temp file the rename published is gone: no partial lease is left behind.
        let leftovers: Vec<String> = std::fs::read_dir(lease.dir())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".tmp."))
            .collect();
        assert!(leftovers.is_empty(), "temp files are renamed, not left: {leftovers:?}");
    }

    #[tokio::test]
    async fn the_lease_file_carries_no_key_material() {
        let (_dir, registry) = pool(&["record-1"]);
        let lease = registry.lease("fx-a").await.expect("lease");

        let written = std::fs::read_to_string(lease.dir().join(LEASE_FILE)).unwrap();
        assert!(!written.contains(PLANTED_PRIVATE_KEY), "the lease must never hold a key: {written}");
        assert!(!written.contains("cHVibGljLWtleQ=="), "nor a public key: {written}");
        assert!(written.contains("fx-a"));
    }

    #[tokio::test]
    async fn a_directory_without_an_identity_is_not_part_of_the_pool() {
        let (dir, registry) = pool(&[]);
        std::fs::create_dir_all(registry.root().join("not-a-slot")).unwrap();
        std::fs::write(registry.root().join("stray-file"), b"x").unwrap();
        seed_slot(registry.root(), "record-1");

        let lease = registry.lease("fx-a").await.expect("the real slot");
        assert_eq!(lease.slot(), "record-1", "a directory with no identity is not a slot");
        assert!(!registry.root().join("not-a-slot").join(LEASE_FILE).exists());
        assert!(!dir.path().join("stray-file").join(LEASE_FILE).exists());
    }

    #[tokio::test]
    async fn a_corrupt_identity_is_reported_and_never_leased() {
        let (_dir, registry) = pool(&["record-1"]);
        std::fs::write(registry.root().join("record-1").join(IDENTITY_FILE), b"{not json").unwrap();

        let error = registry.lease("fx-a").await.expect_err("a broken record is not a slot");
        assert_eq!(error.code(), "SLOT_CORRUPT");
        assert!(!registry.root().join("record-1").join(LEASE_FILE).exists(), "no lease on a broken slot");
    }

    #[tokio::test]
    async fn an_identity_without_a_keypair_is_refused() {
        let (_dir, registry) = pool(&["record-1"]);
        std::fs::write(
            registry.root().join("record-1").join(IDENTITY_FILE),
            b"{\"machineId\":\"m\",\"publicKey\":\"\",\"privateKey\":\"\"}",
        )
        .unwrap();

        let error = registry.lease("fx-a").await.expect_err("a slot that cannot connect");
        assert_eq!(error.code(), "SLOT_CORRUPT");
        assert!(error.to_string().contains("no keypair"), "{error}");
    }

    #[tokio::test]
    async fn a_corrupt_lease_is_reported_and_never_read_as_free() {
        let (_dir, registry) = pool(&["record-1"]);
        let lease = registry.lease("fx-a").await.expect("lease");
        std::fs::write(lease.dir().join(LEASE_FILE), b"{not json").unwrap();

        // Reading it as free would hand one record to two machines at once.
        let error = registry.lease("fx-b").await.expect_err("a corrupt lease is not free");
        assert_eq!(error.code(), "SLOT_CORRUPT");
        assert_eq!(std::fs::read(lease.dir().join(LEASE_FILE)).unwrap(), b"{not json");
    }

    #[tokio::test]
    async fn a_lease_from_a_newer_build_is_refused() {
        let (_dir, registry) = pool(&["record-1"]);
        let lease = registry.lease("fx-a").await.expect("lease");
        std::fs::write(
            lease.dir().join(LEASE_FILE),
            format!("{{\"version\":{},\"name\":\"fx-z\",\"leasedAt\":1}}", LEASE_VERSION + 1),
        )
        .unwrap();

        let error = registry.lease("fx-b").await.expect_err("a newer schema");
        assert_eq!(error.code(), "SLOT_CORRUPT");
        assert!(error.to_string().contains("newer build"), "{error}");
    }

    #[tokio::test]
    async fn debug_and_display_never_show_key_material() {
        let (_dir, registry) = pool(&["record-1"]);
        let lease = registry.lease("fx-a").await.expect("lease");

        let debug = format!("{lease:?}");
        let display = format!("{lease}");
        for rendered in [&debug, &display] {
            assert!(!rendered.contains(PLANTED_PRIVATE_KEY), "key leak: {rendered}");
            assert!(!rendered.contains("cHVibGljLWtleQ=="), "key leak: {rendered}");
            assert!(!rendered.to_lowercase().contains("privatekey"), "key leak: {rendered}");
        }
        assert!(display.contains("record-1"));
        assert!(display.contains("record-1-machine"));
        assert!(display.contains("fx-a"));

        // The error paths are just as quiet: no identity bytes, no key.
        let error = SlotError::AlreadyLeased { slot: "record-1".into(), holder: "fx-b".into() };
        assert!(!format!("{error:?}{error}").contains(PLANTED_PRIVATE_KEY));
    }

    #[tokio::test]
    async fn a_slot_name_with_a_path_separator_is_unknown() {
        let (_dir, registry) = pool(&["record-1"]);

        for name in ["../record-1", "a/b", "..", ".hidden"] {
            let error = registry.lease_specific(name, "fx-a").await.expect_err("not a slot");
            assert_eq!(error.code(), "SLOT_UNKNOWN", "{name}");
        }
        assert!(registry.lease_of("fx-a").await.unwrap().is_none());
        assert!(!registry.root().join("record-1").join(LEASE_FILE).exists());
    }

    // ---- the lock, contended by a second process ----------------------------------------------

    /// The child half of [`the_pool_lock_is_held_across_processes`], selected by environment.
    ///
    /// Run without `FX_FLOCK_MODE` (as an ordinary suite run does) it asserts nothing: the parent
    /// test is the only caller that gives it a job. It reports on stdout rather than through its exit
    /// status, so "the lock was free" is a value the parent reads rather than a passing test.
    #[test]
    fn flock_probe_child() {
        let Ok(mode) = std::env::var("FX_FLOCK_MODE") else { return };
        let root = PathBuf::from(std::env::var("FX_FLOCK_ROOT").expect("FX_FLOCK_ROOT"));
        match mode.as_str() {
            // Take the lock without waiting. A second open file description in a second process
            // contends with the parent's exactly as a second `ferryx` would.
            "probe" => {
                use std::os::unix::io::AsRawFd as _;

                let file = open_private(&root.join(LOCK_FILE)).expect("open the pool lock");
                // SAFETY: `flock` only reads the descriptor this process owns.
                let taken = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
                if taken == 0 {
                    // SAFETY: as above; releasing a lock this process holds.
                    unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
                    println!("FX_PROBE=LOCK_TAKEN");
                } else {
                    let error = std::io::Error::last_os_error();
                    let blocked = matches!(error.raw_os_error(), Some(code) if code == libc::EWOULDBLOCK || code == libc::EAGAIN);
                    assert!(blocked, "the lock failed for another reason: {error}");
                    println!("FX_PROBE=LOCK_HELD");
                }
            }
            // A real lease from a second process, through the public API, after the holder let go.
            "lease" => {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("runtime");
                let lease = runtime
                    .block_on(SlotRegistry::at(&root).lease("fx-child"))
                    .expect("the child leases the freed pool");
                println!("FX_PROBE=LEASED {}", lease.slot());
            }
            other => panic!("unknown FX_FLOCK_MODE {other}"),
        }
    }

    /// Spawn this test binary's child probe and return its stdout. `--exact` keeps the child to the
    /// one test, the count is asserted so a renamed child cannot pass vacuously, and `--nocapture`
    /// is what lets a passing child report the lock state it observed.
    #[cfg(unix)]
    fn run_child(mode: &str, root: &Path) -> String {
        let output = std::process::Command::new(std::env::current_exe().expect("the test binary"))
            .args([
                "--exact",
                "cloud_host::slots::tests::flock_probe_child",
                "--test-threads=1",
                "--nocapture",
            ])
            .env("FX_FLOCK_MODE", mode)
            .env("FX_FLOCK_ROOT", root)
            .output()
            .expect("spawn the child");
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        assert!(
            output.status.success() && stdout.contains("running 1 test"),
            "the child probe did not run: {stdout}{}",
            String::from_utf8_lossy(&output.stderr)
        );
        stdout
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn the_pool_lock_is_held_across_processes() {
        let (_dir, registry) = pool(&["record-1"]);

        // The control: with nobody holding it, a second process takes the lock. Without this, a
        // blocked result below would prove nothing about the lock.
        let free = run_child("probe", registry.root());
        assert!(free.contains("FX_PROBE=LOCK_TAKEN"), "an unheld lock is free: {free}");

        let held = RegistryLock::acquire(registry.root()).expect("hold the pool lock");
        let blocked = run_child("probe", registry.root());
        assert!(
            blocked.contains("FX_PROBE=LOCK_HELD"),
            "a second process took a lock this one holds: {blocked}"
        );

        // The lock is what makes a lease a read-modify-write instead of a race: once released, a
        // second process leases the same pool from its own descriptor, and the parent can read the
        // lease the child wrote.
        drop(held);
        let leased = run_child("lease", registry.root());
        assert!(leased.contains("FX_PROBE=LEASED record-1"), "{leased}");
        assert_eq!(
            registry.lease_of("fx-child").await.unwrap().map(|lease| lease.slot().to_string()),
            Some("record-1".to_string()),
            "the child's lease is the one the parent reads"
        );
    }
}
