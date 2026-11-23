//! The machine catalog: `~/.ferryx/cloud-hosts.json`.
//!
//! One row per machine ever created from this Mac. A row lives until the provider confirms the
//! machine is gone, and then stays as a `deleted` tombstone: the row is the only local record that
//! we created that machine, and a future sweep must be able to tell "we already deleted this" from
//! "we never knew about this".
//!
//! Three rules hold for every byte this module touches.
//!
//! 1. **0600, always, published by rename.** The file is written to a sibling temp file opened with
//!    mode 0600, fsynced, then renamed over the live name. A crash mid-write leaves the old catalog
//!    intact, and an existing 0644 file is replaced by a 0600 one on the next write.
//! 2. **Blocking I/O stays off the runtime threads.** Every read and write goes through
//!    [`crate::ipc::run_blocking`].
//! 3. **A parse failure is not an empty catalog.** A file this build cannot read is reported, never
//!    overwritten: starting from zero would forget live machines and their hard deadlines, which is
//!    exactly the state that lets free credit burn.
//!
//! No token, key or enrollment code is ever a field here. A row names a machine; the credential
//! that reaches it lives in the provider's own store, in a slot identity file, or nowhere.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ipc::{run_blocking, IpcError, IpcErrorCode};

use super::{SandboxState, Transport};

/// Directory under `$HOME` shared by every ferryx state file.
pub const STATE_DIR: &str = ".ferryx";
/// The catalog's file name inside [`STATE_DIR`].
pub const CATALOG_FILE: &str = "cloud-hosts.json";
/// Schema version this build writes. A file carrying a newer one is refused, never rewritten.
pub const CATALOG_VERSION: u32 = 1;

/// One machine we created, from `create` until the provider confirms it is gone.
///
/// Times are unix milliseconds so they line up with [`super::ListedSandbox::created_at`] and a
/// deadline comparison never has to convert units.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogRow {
    /// The `--name` the operator used; unique in the catalog and the CLI's handle for the machine.
    pub name: String,
    /// Registry key of the provider that hosts it (`e2b`, `daytona`, ...).
    pub provider: String,
    /// The provider's own id, the argument every other provider call takes.
    pub sandbox_id: String,
    /// How agent traffic reaches it once it is ready.
    pub transport: Transport,
    pub created_at: i64,
    /// Last time a verb used the machine; the idle clock is measured from here.
    pub last_used_at: i64,
    /// Idle window in seconds. The machine is stopped when `now > last_used_at + ttl_seconds`.
    pub ttl_seconds: u64,
    /// `created_at + min(provider max, 6h)`: the machine is deleted at this instant whatever the
    /// idle clock says.
    pub hard_deadline: i64,
    pub state: SandboxState,
    /// The local dispatch socket, once one is listening. `None` in relay argv mode or before the
    /// first `cloud socket`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub socket_path: Option<String>,
    /// The relay slot (account machine record id) a T2 machine claimed from the pool. `None` for an
    /// SSH machine, which has its own route in. Only the record id is stored: the machineId the
    /// relay keys control channels by is re-read from the slot's own `identity.json`, so the two
    /// cannot drift, and the keypair never enters the catalog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<String>,
}

impl CatalogRow {
    /// A confirmed-deleted row. Kept forever, never acted on again.
    pub fn is_tombstone(&self) -> bool {
        self.state == SandboxState::Deleted
    }

    /// The instant this machine goes idle (stops earning its keep without being used).
    pub fn idle_deadline(&self) -> i64 {
        self.last_used_at.saturating_add(seconds_to_ms(self.ttl_seconds))
    }

    /// Idle past its window: the sweep stops it.
    pub fn is_idle(&self, now_ms: i64) -> bool {
        now_ms > self.idle_deadline()
    }

    /// Past the hard ceiling: the sweep deletes it, idle or not.
    pub fn is_past_hard_deadline(&self, now_ms: i64) -> bool {
        now_ms > self.hard_deadline
    }
}

/// The whole file: a version and the rows, ordered by name so a diff of two saves is readable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Catalog {
    #[serde(default = "current_version")]
    pub version: u32,
    #[serde(default)]
    pub rows: Vec<CatalogRow>,
}

fn current_version() -> u32 {
    CATALOG_VERSION
}

impl Default for Catalog {
    fn default() -> Self {
        Self { version: CATALOG_VERSION, rows: Vec::new() }
    }
}

impl Catalog {
    /// The row a `--name` names, if any.
    pub fn row(&self, name: &str) -> Option<&CatalogRow> {
        self.rows.iter().find(|row| row.name == name)
    }

    /// The row a provider id names, if any. Includes tombstones, so an orphan scan can tell "ours
    /// and already handled" from "ours and forgotten".
    pub fn row_for_sandbox<'a>(&'a self, sandbox_id: &str) -> Option<&'a CatalogRow> {
        self.rows.iter().find(|row| row.sandbox_id == sandbox_id)
    }

    /// Insert or replace by name. Returns the row as stored.
    pub fn upsert(&mut self, row: CatalogRow) -> CatalogRow {
        match self.rows.iter_mut().find(|existing| existing.name == row.name) {
            Some(existing) => *existing = row.clone(),
            None => self.rows.push(row.clone()),
        }
        self.rows.sort_by(|left, right| left.name.cmp(&right.name));
        row
    }

    /// Rows that are not tombstones: the machines that still cost money.
    pub fn live(&self) -> impl Iterator<Item = &CatalogRow> {
        self.rows.iter().filter(|row| !row.is_tombstone())
    }

    /// Whether any row still needs the reaper. `false` is what uninstalls the scheduler entry.
    pub fn has_live(&self) -> bool {
        self.live().next().is_some()
    }

    /// Record a use of `name`, so the idle clock restarts. `false` when no such row exists.
    pub fn touch(&mut self, name: &str, now_ms: i64) -> bool {
        match self.rows.iter_mut().find(|row| row.name == name) {
            Some(row) => {
                row.last_used_at = now_ms;
                true
            }
            None => false,
        }
    }
}

/// Where the catalog lives and how to read and write it.
///
/// A value, not a global: a test (and a QA run) points it at a temp directory, so nothing here can
/// reach the operator's real `~/.ferryx/cloud-hosts.json` by accident.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogStore {
    path: PathBuf,
}

impl CatalogStore {
    /// A store over an explicit path.
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The real location: `$HOME/.ferryx/cloud-hosts.json`.
    pub fn default_location() -> Result<Self, IpcError> {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .ok_or_else(|| {
                IpcError::new(IpcErrorCode::IoError, "cannot resolve the home directory for the cloud catalog")
            })?;
        Ok(Self::at(location_in(Path::new(&home))))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Read the catalog. A missing file is an empty catalog, not an error: the first `cloud create`
    /// writes it. An unreadable or newer-versioned file is an error and is left alone.
    pub async fn load(&self) -> Result<Catalog, IpcError> {
        let path = self.path.clone();
        run_blocking(move || read_catalog(&path)).await
    }

    /// Replace the catalog atomically at mode 0600.
    pub async fn save(&self, catalog: &Catalog) -> Result<(), IpcError> {
        let path = self.path.clone();
        let catalog = catalog.clone();
        run_blocking(move || write_catalog(&path, &catalog)).await
    }
}

/// `$HOME/.ferryx/cloud-hosts.json` for a given home directory. Pure, so the join rule is testable.
pub fn location_in(home: &Path) -> PathBuf {
    home.join(STATE_DIR).join(CATALOG_FILE)
}

/// Seconds as milliseconds, saturating so a nonsense TTL cannot wrap into the past.
pub fn seconds_to_ms(seconds: u64) -> i64 {
    i64::try_from(seconds.saturating_mul(1_000)).unwrap_or(i64::MAX)
}

fn read_catalog(path: &Path) -> Result<Catalog, IpcError> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Catalog::default()),
        Err(error) => {
            return Err(IpcError::new(
                IpcErrorCode::IoError,
                format!("cannot read the cloud catalog {}: {error}", path.display()),
            ))
        }
    };
    let catalog: Catalog = serde_json::from_slice(&bytes).map_err(|error| {
        IpcError::new(
            IpcErrorCode::ParseError,
            format!("cannot parse the cloud catalog {}: {error}", path.display()),
        )
    })?;
    if catalog.version > CATALOG_VERSION {
        return Err(IpcError::new(
            IpcErrorCode::ParseError,
            format!(
                "the cloud catalog {} was written by a newer build (version {}, this build writes {CATALOG_VERSION})",
                path.display(),
                catalog.version
            ),
        ));
    }
    Ok(catalog)
}

fn write_catalog(path: &Path, catalog: &Catalog) -> Result<(), IpcError> {
    let bytes = serde_json::to_vec_pretty(catalog).map_err(|error| {
        IpcError::new(IpcErrorCode::ParseError, format!("cannot serialize the cloud catalog: {error}"))
    })?;
    write_private_atomically(path, &bytes).map_err(|error| {
        IpcError::new(
            IpcErrorCode::IoError,
            format!("cannot write the cloud catalog {}: {error}", path.display()),
        )
    })
}

/// Write `bytes` to `path` through a 0600 temp file and one rename.
///
/// The temp file is created restricted rather than chmodded afterwards, so the contents are never
/// briefly world-readable; the rename is what publishes them, so a reader sees either the whole old
/// file or the whole new one.
fn write_private_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;

    let parent = path.parent().filter(|parent| !parent.as_os_str().is_empty()).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "the catalog path has no parent directory")
    })?;
    std::fs::create_dir_all(parent)?;
    // `~/.ferryx` holds the daemon's secrets and this catalog; only this user may traverse it.
    let _ = restrict(parent, 0o700);

    let temp = path.with_extension(format!("json.tmp.{}", std::process::id()));
    {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    // A umask can only clear bits, and some filesystems ignore the open mode, so state 0600
    // explicitly before the name changes hands.
    restrict(&temp, 0o600)?;
    std::fs::rename(&temp, path)?;
    // Best effort: a directory fsync makes the rename durable, but a filesystem that refuses it
    // (some network mounts) must not fail a catalog write that already happened.
    if let Ok(directory) = std::fs::File::open(parent) {
        let _ = directory.sync_all();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, state: SandboxState) -> CatalogRow {
        CatalogRow {
            name: name.to_string(),
            provider: "fake".to_string(),
            sandbox_id: format!("fake-{name}"),
            transport: Transport::Relay,
            created_at: 1_735_689_600_000,
            last_used_at: 1_735_689_600_000,
            ttl_seconds: 1_800,
            hard_deadline: 1_735_711_200_000,
            state,
            socket_path: None,
            slot: None,
        }
    }

    fn store_in(dir: &tempfile::TempDir) -> CatalogStore {
        CatalogStore::at(dir.path().join("state").join(CATALOG_FILE))
    }

    #[test]
    fn default_location_is_home_dot_ferryx_cloud_hosts_json() {
        assert_eq!(
            location_in(Path::new("/Users/someone")),
            PathBuf::from("/Users/someone/.ferryx/cloud-hosts.json")
        );
    }

    #[tokio::test]
    async fn a_missing_file_loads_as_an_empty_catalog() {
        let dir = tempfile::tempdir().unwrap();
        let catalog = store_in(&dir).load().await.expect("a missing catalog is empty");
        assert_eq!(catalog.version, CATALOG_VERSION);
        assert!(catalog.rows.is_empty());
        assert!(!catalog.has_live());
        assert!(!store_in(&dir).path().exists(), "loading must not create the file");
    }

    #[tokio::test]
    async fn round_trip_keeps_every_field() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(&dir);
        let mut catalog = Catalog::default();
        let mut first = row("fx-a", SandboxState::Ready);
        first.socket_path = Some("/tmp/ferryx-cloud-501/fx-a.sock".to_string());
        first.transport = Transport::Ssh;
        catalog.upsert(first.clone());
        catalog.upsert(row("fx-b", SandboxState::Deleted));
        store.save(&catalog).await.expect("save");

        let loaded = store.load().await.expect("load");
        assert_eq!(loaded, catalog);
        assert_eq!(loaded.row("fx-a"), Some(&first));
        assert_eq!(loaded.row_for_sandbox("fake-fx-b").map(|row| row.name.as_str()), Some("fx-b"));
    }

    #[tokio::test]
    async fn a_world_readable_catalog_is_rewritten_0600_on_the_next_write() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(&dir);
        std::fs::create_dir_all(store.path().parent().unwrap()).unwrap();

        // The state a careless tool or a `cp` leaves behind.
        std::fs::write(store.path(), b"{\"version\":1,\"rows\":[]}").unwrap();
        set_mode(store.path(), 0o644);

        let mut catalog = store.load().await.expect("load a 0644 catalog");
        catalog.upsert(row("fx-a", SandboxState::Ready));
        store.save(&catalog).await.expect("save");

        assert_eq!(mode_of(store.path()), 0o600, "the catalog must be owner-only after a write");
        assert_eq!(store.load().await.expect("reload").row("fx-a").map(|row| row.name.clone()), Some("fx-a".to_string()));
    }

    #[tokio::test]
    async fn a_corrupt_catalog_is_reported_and_left_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(&dir);
        std::fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        std::fs::write(store.path(), b"{not json").unwrap();

        let error = store.load().await.expect_err("a corrupt catalog must not read as empty");
        assert_eq!(error.code, IpcErrorCode::ParseError);
        assert_eq!(std::fs::read(store.path()).unwrap(), b"{not json", "the file is never clobbered");
    }

    #[tokio::test]
    async fn a_catalog_from_a_newer_build_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(&dir);
        std::fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        std::fs::write(store.path(), format!("{{\"version\":{},\"rows\":[]}}", CATALOG_VERSION + 1)).unwrap();

        let error = store.load().await.expect_err("a newer schema is refused");
        assert_eq!(error.code, IpcErrorCode::ParseError);
        assert!(error.message.contains("newer build"));
    }

    #[tokio::test]
    async fn tombstones_stay_in_the_file_and_do_not_count_as_live() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(&dir);
        let mut catalog = Catalog::default();
        catalog.upsert(row("fx-gone", SandboxState::Deleted));
        store.save(&catalog).await.expect("save");

        let mut loaded = store.load().await.expect("load");
        assert_eq!(loaded.rows.len(), 1);
        assert!(!loaded.has_live());
        assert_eq!(loaded.live().count(), 0);
        // A tombstone is not resurrected by an upsert of the same name either.
        loaded.upsert(row("fx-gone", SandboxState::Deleted));
        assert_eq!(loaded.rows.len(), 1);
    }

    #[tokio::test]
    async fn rows_are_sorted_by_name_and_upsert_replaces_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(&dir);
        let mut catalog = Catalog::default();
        catalog.upsert(row("fx-z", SandboxState::Ready));
        catalog.upsert(row("fx-a", SandboxState::Ready));
        assert_eq!(catalog.rows.iter().map(|row| row.name.as_str()).collect::<Vec<_>>(), vec!["fx-a", "fx-z"]);

        let mut replacement = row("fx-a", SandboxState::Stopped);
        replacement.last_used_at += 1_000;
        catalog.upsert(replacement.clone());
        assert_eq!(catalog.rows.len(), 2);
        assert_eq!(catalog.row("fx-a"), Some(&replacement));

        store.save(&catalog).await.expect("save");
        assert_eq!(store.load().await.expect("load").rows.len(), 2);
    }

    #[tokio::test]
    async fn no_row_carries_a_credential() {
        let mut catalog = Catalog::default();
        catalog.upsert(row("fx-a", SandboxState::Ready));
        let json = serde_json::to_string_pretty(&catalog).unwrap().to_lowercase();
        for forbidden in ["token", "secret", "api_key", "apikey", "authorization", "password", "private_key"] {
            assert!(!json.contains(forbidden), "the catalog must never carry {forbidden}: {json}");
        }
    }

    #[test]
    fn idle_and_hard_deadline_are_computed_from_the_row() {
        let mut entry = row("fx-a", SandboxState::Ready);
        entry.last_used_at = 1_000_000;
        entry.ttl_seconds = 1_800;
        entry.hard_deadline = 2_000_000;

        assert_eq!(entry.idle_deadline(), 2_800_000);
        assert!(!entry.is_idle(2_800_000), "the boundary itself is not idle");
        assert!(entry.is_idle(2_800_001));
        assert!(!entry.is_past_hard_deadline(2_000_000));
        assert!(entry.is_past_hard_deadline(2_000_001));
    }

    #[test]
    fn touch_moves_the_idle_clock_and_reports_an_unknown_name() {
        let mut catalog = Catalog::default();
        catalog.upsert(row("fx-a", SandboxState::Ready));
        assert!(catalog.touch("fx-a", 1_735_699_600_000));
        assert_eq!(catalog.row("fx-a").unwrap().last_used_at, 1_735_699_600_000);
        assert!(!catalog.touch("fx-missing", 1));
    }

    #[test]
    fn seconds_to_ms_saturates_instead_of_wrapping() {
        assert_eq!(seconds_to_ms(1_800), 1_800_000);
        assert_eq!(seconds_to_ms(u64::MAX), i64::MAX);
    }

    fn mode_of(path: &Path) -> u32 {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            return std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        }
        #[cfg(not(unix))]
        {
            let _ = path;
            0o600
        }
    }

    fn set_mode(path: &Path, mode: u32) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
        }
        #[cfg(not(unix))]
        {
            let _ = (path, mode);
        }
    }
}
