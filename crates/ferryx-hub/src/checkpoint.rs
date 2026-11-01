//! Portable supervisor checkpoint (export side).
//!
//! A checkpoint is a zstd-compressed tar holding a consistent `VACUUM INTO`
//! copy of `hub.sqlite` (bulk regenerable tables dropped, host ids rewritten
//! to machine-scoped ids), the allowlisted machine-independent hub env with
//! inlined secrets, the mhc profile `omo.jsonc`, and the supervisor memory
//! (git bundle, or the configured sync remote). `checkpoint_id` is the
//! sha256 of the compressed archive.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::SqlitePool;

/// Maximum compressed checkpoint size.
pub const CHECKPOINT_MAX_COMPRESSED_BYTES: u64 = 64 * 1024 * 1024;
/// Upper bound for a decompressed checkpoint, so a small archive cannot expand without limit.
/// 512 MiB is eight times the compressed budget: a compacted hub database with its bulk
/// tables emptied compresses roughly 3-6x with zstd level 3. Import streams the database and
/// the memory bundle to disk, so this bounds disk use, not memory.
pub const CHECKPOINT_MAX_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;
/// Upper bound for one archive entry held in memory (manifest, env, profile).
pub const CHECKPOINT_MAX_BUFFERED_ENTRY_BYTES: u64 = 16 * 1024 * 1024;
/// Error code surfaced in status when the archive exceeds the budget.
pub const CHECKPOINT_TOO_LARGE: &str = "CHECKPOINT_TOO_LARGE";
/// Import refused: the target hub holds a different supervisor.
pub const SUPERVISOR_IDENTITY_MISMATCH: &str = "SUPERVISOR_IDENTITY_MISMATCH";
/// `held_detail` of a goal whose host has no mapping on the importing machine.
pub const REMOTE_HOST_UNMAPPED: &str = "REMOTE_HOST_UNMAPPED";
/// Manifest format version.
pub const CHECKPOINT_FORMAT_VERSION: u32 = 1;
/// omo memory agent used by the supervisor.
pub const SUPERVISOR_MEMORY_AGENT: &str = crate::supervisor::MEMORY_NAME;
/// Host id of this machine in a hub database.
pub const LOCAL_HOST_ID: &str = "local";
/// Prefix of a machine-scoped host id inside a checkpoint.
pub const MACHINE_HOST_PREFIX: &str = "machine:";
/// Prefix of a host id the exporting machine could not scope (neither local nor paired). The
/// importer cannot map it to any machine: its goals stay held `REMOTE_HOST_UNMAPPED`.
pub const UNMAPPED_HOST_PREFIX: &str = "unmapped:";

pub const ARCHIVE_MANIFEST: &str = "manifest.json";
pub const ARCHIVE_DATABASE: &str = "hub.sqlite";
pub const ARCHIVE_ENV: &str = "env/hub.env";
pub const ARCHIVE_PROFILE: &str = "profile/omo.jsonc";
pub const ARCHIVE_MEMORY_BUNDLE: &str = "memory/supervisor.bundle";

/// Regenerable bulk tables emptied in the copy (schema kept). Measured with dbstat on the
/// owner's hub DB: inbox_digests ~30 MiB, messages ~20 MiB of 80 MB.
pub const EXCLUDED_TABLES: &[&str] = &["inbox_digests", "messages"];
/// Tables whose `host_id` column is rewritten to machine-scoped ids (when the column exists).
pub const HOST_ID_TABLES: &[&str] = &[
    "supervisor_project_hosts",
    "supervisor_execution_bindings",
    "supervisor_dispatches",
    "lanes",
    "fleet_inventory_cache",
];

const ZSTD_LEVEL: i32 = 3;

/// Machine-local or test-only keys never exported even when a prefix allows them.
const ENV_DENY: &[&str] = &[
    "FERRYX_HUB_DB",
    "FERRYX_HUB_ENV_FILE",
    "FERRYX_HUB_CONFIG",
    "FERRYX_HUB_MHC_BIN",
    "FERRYX_HUB_EXTRA_PATH",
    "FERRYX_HUB_WEB_ROOT",
    "FERRYX_HUB_IMPORT",
    "FERRYX_HUB_IMPORT_FROM",
    "FERRYX_HUB_CDP_PORT",
    "FERRYX_HUB_SLACK_API",
    "FERRYX_HUB_TRACKER_ADDR",
    "FERRYX_HUB_SUPERVISOR_PANE",
    "FERRYX_HUB_OMO_SESSIONS",
];
const ENV_DENY_SUFFIXES: &[&str] = &["_FILE", "_PATH", "_DIR", "_ROOT", "_BIN", "_DB"];
const ENV_ALLOW_PREFIXES: &[&str] = &["SLACK_", "DISCORD_", "FERRYX_HUB_MHC_"];
const ENV_ALLOW: &[&str] = &[
    "OPENAI_API_KEY",
    "OPENAI_API_BASE",
    "ANTHROPIC_API_KEY",
    "GEMINI_API_KEY",
    "GOOGLE_API_KEY",
    "OPENROUTER_API_KEY",
    "XAI_API_KEY",
    "ELEVENLABS_API_KEY",
    "FERRYX_HUB_SLACK_OWNER",
    "FERRYX_HUB_DISCORD",
    "FERRYX_HUB_NOTIFY",
    "FERRYX_HUB_NOTIFY_AFTER_SECS",
    "FERRYX_HUB_QUIET_HOURS",
    "FERRYX_HUB_TRACKER_TOKEN",
    "FERRYX_CRON_MODEL",
];

/// Paired host as seen from the exporting machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairedHostMapping {
    pub host_id: String,
    pub machine_id: String,
}

/// Writes `bytes` to `path` creating the file owner-only (0600 / private ACL).
pub type PrivateFileWriter = Arc<dyn Fn(&Path, &[u8]) -> std::io::Result<()> + Send + Sync>;

/// Where the supervisor memory comes from on import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MemorySource {
    /// No committed memory on the exporting machine.
    Absent,
    /// `git bundle --all` of the agent repo at [`ARCHIVE_MEMORY_BUNDLE`].
    Bundle { agent_dir: String },
    /// `memory.sync.remote` is configured: the importer pulls it.
    Remote { remote: String },
}

/// `manifest.json` of a checkpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointManifest {
    pub format_version: u32,
    pub created_at: String,
    pub source_machine_id: String,
    pub supervisor_id: Option<String>,
    pub memory_agent: String,
    pub memory: MemorySource,
    /// Names only; values live in [`ARCHIVE_ENV`].
    pub env_keys: Vec<String>,
    pub excluded_tables: Vec<String>,
    /// Source host id -> machine-scoped id written into the database copy.
    pub host_map: BTreeMap<String, String>,
    pub has_profile: bool,
}

/// Inputs for [`export`].
#[derive(Clone)]
pub struct ExportRequest {
    pub hub_dir: PathBuf,
    pub own_machine_id: String,
    pub paired: Vec<PairedHostMapping>,
    /// Hub env file; allowlisted keys are exported and `*_FILE` targets inlined.
    pub env_file: Option<PathBuf>,
    /// The mhc profile `.omo/omo.jsonc` (never the provider sessions json).
    pub profile_omo_jsonc: Option<PathBuf>,
    /// `~/.omo/memory/agents`.
    pub memory_root: Option<PathBuf>,
    pub output_path: PathBuf,
    /// Compressed size limit; [`CHECKPOINT_MAX_COMPRESSED_BYTES`] in production.
    pub budget_bytes: u64,
    pub write_private: PrivateFileWriter,
}

impl ExportRequest {
    pub fn new(
        hub_dir: PathBuf,
        own_machine_id: String,
        output_path: PathBuf,
        write_private: PrivateFileWriter,
    ) -> Self {
        Self {
            hub_dir,
            own_machine_id,
            paired: Vec::new(),
            env_file: None,
            profile_omo_jsonc: None,
            memory_root: None,
            output_path,
            budget_bytes: CHECKPOINT_MAX_COMPRESSED_BYTES,
            write_private,
        }
    }
}

/// Result of a successful export.
#[derive(Debug, Clone)]
pub struct ExportedCheckpoint {
    pub checkpoint_id: String,
    pub path: PathBuf,
    pub compressed_bytes: u64,
    pub manifest: CheckpointManifest,
}

/// Export failure.
#[derive(Debug, thiserror::Error)]
pub enum CheckpointError {
    /// Over the compressed budget, or over a decompressed bound while reading; then `bytes`
    /// is `budget + 1`, the archive is not read to its end.
    #[error("CHECKPOINT_TOO_LARGE: checkpoint is {bytes} bytes, budget {budget}")]
    TooLarge { bytes: u64, budget: u64 },
    #[error("checkpoint io: {0}")]
    Io(#[from] std::io::Error),
    #[error("checkpoint sqlite: {0}")]
    Sql(#[from] sqlx::Error),
    #[error("checkpoint integrity_check: {0}")]
    Integrity(String),
    #[error("checkpoint: {0}")]
    Other(String),
}

impl CheckpointError {
    /// Stable code shown in status.
    pub fn code(&self) -> &'static str {
        match self {
            Self::TooLarge { .. } => CHECKPOINT_TOO_LARGE,
            Self::Io(_) => "CHECKPOINT_IO",
            Self::Sql(_) => "CHECKPOINT_SQL",
            Self::Integrity(_) => "CHECKPOINT_INTEGRITY",
            Self::Other(_) => "CHECKPOINT_FAILED",
        }
    }
}

/// `<memory_root>/<agent>-<sha256(agent)[..8]>`: how omo names a memory agent dir.
pub fn memory_agent_dir(memory_root: &Path, agent: &str) -> PathBuf {
    memory_root.join(format!("{agent}-{}", &sha256_hex(agent.as_bytes())[..8]))
}

/// `local`/NULL -> `machine:<own>`, each paired host id -> `machine:<its machine id>`.
pub fn host_map(own_machine_id: &str, paired: &[PairedHostMapping]) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    map.insert(
        LOCAL_HOST_ID.to_owned(),
        format!("{MACHINE_HOST_PREFIX}{own_machine_id}"),
    );
    for host in paired {
        if host.host_id == LOCAL_HOST_ID || host.host_id.starts_with(MACHINE_HOST_PREFIX) {
            continue;
        }
        map.insert(
            host.host_id.clone(),
            format!("{MACHINE_HOST_PREFIX}{}", host.machine_id),
        );
    }
    map
}

/// Whether a hub env key is machine-independent and may leave this machine.
pub fn env_key_exported(key: &str) -> bool {
    if ENV_DENY.contains(&key) || ENV_DENY_SUFFIXES.iter().any(|s| key.ends_with(s)) {
        return false;
    }
    ENV_ALLOW.contains(&key) || ENV_ALLOW_PREFIXES.iter().any(|p| key.starts_with(p))
}

/// Writes a checkpoint of the hub to `request.output_path` through `write_private`.
///
/// The database is copied with `VACUUM INTO` (a consistent snapshot of a live WAL database),
/// then bulk tables are emptied and host ids rewritten in the copy only.
pub async fn export(
    pool: &SqlitePool,
    request: &ExportRequest,
) -> Result<ExportedCheckpoint, CheckpointError> {
    if request.own_machine_id.trim().is_empty() {
        return Err(CheckpointError::Other("own machine id is empty".to_owned()));
    }
    let staging = request
        .hub_dir
        .join(format!(".checkpoint-staging-{}", uuid::Uuid::new_v4()));
    let dir = staging.clone();
    blocking(move || Ok(create_private_dir(&dir)?)).await?;
    let result = export_staged(pool, request, &staging).await;
    let dir = staging.clone();
    if let Err(error) = blocking(move || Ok(std::fs::remove_dir_all(dir)?)).await {
        tracing::warn!(%error, staging = %staging.display(), "checkpoint staging not removed");
    }
    result
}

async fn export_staged(
    pool: &SqlitePool,
    request: &ExportRequest,
    staging: &Path,
) -> Result<ExportedCheckpoint, CheckpointError> {
    let db_path = staging.join(ARCHIVE_DATABASE);
    snapshot_into(pool, &db_path).await?;
    let map = host_map(&request.own_machine_id, &request.paired);
    let supervisor_id = prepare_copy(&db_path, &map).await?;
    let request = request.clone();
    let bundle_path = staging.join("supervisor.bundle");
    blocking(move || {
        let db = std::fs::read(db_path)?;
        let env = export_env(request.env_file.as_deref())?;
        let profile = match &request.profile_omo_jsonc {
            Some(path) if path.is_file() => Some(std::fs::read(path)?),
            _ => None,
        };
        let (agent, remote) = profile_memory(profile.as_deref());
        let (memory, bundle) = match (remote, &request.memory_root) {
            (Some(remote), _) => (MemorySource::Remote { remote }, None),
            (None, Some(root)) => {
                let agent_dir = memory_agent_dir(root, &agent);
                match bundle_memory(&agent_dir.join("repo"), &bundle_path)? {
                    Some(bytes) => {
                        let name = agent_dir
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        (MemorySource::Bundle { agent_dir: name }, Some(bytes))
                    }
                    None => (MemorySource::Absent, None),
                }
            }
            (None, None) => (MemorySource::Absent, None),
        };
        let manifest = CheckpointManifest {
            format_version: CHECKPOINT_FORMAT_VERSION,
            created_at: chrono::Utc::now().to_rfc3339(),
            source_machine_id: request.own_machine_id.clone(),
            supervisor_id,
            memory_agent: agent,
            memory,
            env_keys: env.keys().cloned().collect(),
            excluded_tables: EXCLUDED_TABLES.iter().map(|t| (*t).to_owned()).collect(),
            host_map: map,
            has_profile: profile.is_some(),
        };
        let manifest_json = serde_json::to_vec_pretty(&manifest)
            .map_err(|error| CheckpointError::Other(format!("manifest: {error}")))?;
        let env_text = render_env(&env);
        let mut entries: Vec<(&str, &[u8])> = vec![
            (ARCHIVE_MANIFEST, manifest_json.as_slice()),
            (ARCHIVE_DATABASE, db.as_slice()),
            (ARCHIVE_ENV, env_text.as_bytes()),
        ];
        if let Some(profile) = &profile {
            entries.push((ARCHIVE_PROFILE, profile.as_slice()));
        }
        if let Some(bundle) = &bundle {
            entries.push((ARCHIVE_MEMORY_BUNDLE, bundle.as_slice()));
        }
        let bytes = build_archive(&entries)?;
        let size = bytes.len() as u64;
        if size > request.budget_bytes {
            return Err(CheckpointError::TooLarge {
                bytes: size,
                budget: request.budget_bytes,
            });
        }
        (request.write_private)(&request.output_path, &bytes)?;
        Ok(ExportedCheckpoint {
            checkpoint_id: sha256_hex(&bytes),
            path: request.output_path.clone(),
            compressed_bytes: size,
            manifest,
        })
    })
    .await
}

/// `VACUUM INTO` a new owner-only file (SQLite keeps the mode of an existing empty file): the
/// copy carries the same data as the live hub database.
async fn snapshot_into(pool: &SqlitePool, db_path: &Path) -> Result<(), CheckpointError> {
    let target = db_path.to_owned();
    blocking(move || Ok(create_private(&target).map(drop)?)).await?;
    sqlx::query("VACUUM INTO ?")
        .bind(db_path.to_string_lossy().into_owned())
        .execute(pool)
        .await?;
    Ok(())
}

/// Creates `path` as a new file, mode 0600 on unix. Blocking.
pub(crate) fn create_private(path: &Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

/// Creates the new directory `path` (and its missing parents), mode 0700 on unix. Blocking.
pub(crate) fn create_private_dir(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}

/// Empties excluded tables, rewrites host ids, compacts and checks the copy.
/// Returns the supervisor id held by the copy.
async fn prepare_copy(
    db_path: &Path,
    map: &BTreeMap<String, String>,
) -> Result<Option<String>, CheckpointError> {
    let options = SqliteConnectOptions::new()
        .filename(db_path)
        .journal_mode(SqliteJournalMode::Delete)
        .foreign_keys(false);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;
    let result = rewrite_copy(&pool, map).await;
    pool.close().await;
    result
}

async fn rewrite_copy(
    pool: &SqlitePool,
    map: &BTreeMap<String, String>,
) -> Result<Option<String>, CheckpointError> {
    let mut conn = pool.acquire().await?;
    for table in EXCLUDED_TABLES {
        if table_exists(&mut conn, table).await? {
            sqlx::query(&format!("DELETE FROM {table}"))
                .execute(&mut *conn)
                .await?;
        }
    }
    let own = map.get(LOCAL_HOST_ID).cloned().unwrap_or_default();
    for table in HOST_ID_TABLES {
        let columns: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pragma_table_info(?) WHERE name = 'host_id'",
        )
        .bind(*table)
        .fetch_one(&mut *conn)
        .await?;
        if columns == 0 {
            continue;
        }
        sqlx::query(&format!("UPDATE {table} SET host_id = ? WHERE host_id IS NULL"))
            .bind(&own)
            .execute(&mut *conn)
            .await?;
        for (from, to) in map {
            sqlx::query(&format!("UPDATE {table} SET host_id = ? WHERE host_id = ?"))
                .bind(to)
                .bind(from)
                .execute(&mut *conn)
                .await?;
        }
        // Neither local nor paired: no machine id to scope it with. Marked so the importer
        // lists it as unmapped instead of taking it for one of its own host ids.
        sqlx::query(&format!(
            "UPDATE {table} SET host_id = ? || host_id
             WHERE substr(host_id, 1, ?) != ? AND substr(host_id, 1, ?) != ?"
        ))
        .bind(UNMAPPED_HOST_PREFIX)
        .bind(MACHINE_HOST_PREFIX.len() as i64)
        .bind(MACHINE_HOST_PREFIX)
        .bind(UNMAPPED_HOST_PREFIX.len() as i64)
        .bind(UNMAPPED_HOST_PREFIX)
        .execute(&mut *conn)
        .await?;
    }
    let supervisor_id = if table_exists(&mut conn, "supervisor_identity").await? {
        sqlx::query_scalar::<_, String>(
            "SELECT supervisor_id FROM supervisor_identity ORDER BY created_at LIMIT 1",
        )
        .fetch_optional(&mut *conn)
        .await?
    } else {
        None
    };
    sqlx::query("VACUUM").execute(&mut *conn).await?;
    let check: Vec<String> = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_all(&mut *conn)
        .await?;
    if check != ["ok"] {
        return Err(CheckpointError::Integrity(check.join("; ")));
    }
    Ok(supervisor_id)
}

async fn table_exists(
    conn: &mut sqlx::SqliteConnection,
    table: &str,
) -> Result<bool, CheckpointError> {
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?")
            .bind(table)
            .fetch_one(&mut *conn)
            .await?;
    Ok(count > 0)
}

/// Allowlisted env entries; a `<KEY>_FILE` pointer is replaced by `<KEY>` with the file's value.
fn export_env(path: Option<&Path>) -> Result<BTreeMap<String, String>, CheckpointError> {
    let Some(path) = path.filter(|path| path.is_file()) else {
        return Ok(BTreeMap::new());
    };
    let entries = crate::hub_env::parse_env_file(&std::fs::read_to_string(path)?);
    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut out = BTreeMap::new();
    for (key, value) in &entries {
        if let Some(base) = key.strip_suffix("_FILE") {
            if !env_key_exported(base) {
                continue;
            }
            let target = base_dir.join(value.trim());
            match std::fs::read_to_string(&target) {
                Ok(secret) if !secret.trim().is_empty() => {
                    out.insert(base.to_owned(), secret.trim().to_owned());
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(%error, key = %key, "checkpoint secret file not readable");
                }
            }
        } else if env_key_exported(key) {
            out.entry(key.clone()).or_insert_with(|| value.clone());
        }
    }
    Ok(out)
}

pub(crate) fn render_env(entries: &BTreeMap<String, String>) -> String {
    let mut text = String::new();
    for (key, value) in entries {
        if value.contains('\n') || value.contains('\r') {
            continue;
        }
        text.push_str(&format!("{key}={value}\n"));
    }
    text
}

/// `(memory.agent, memory.sync.remote)` from the profile omo.jsonc.
fn profile_memory(profile: Option<&[u8]>) -> (String, Option<String>) {
    let value = profile.and_then(|bytes| {
        let text = String::from_utf8_lossy(bytes);
        let stripped: String = text
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        serde_json::from_str::<serde_json::Value>(&stripped).ok()
    });
    let text_at = |pointer: &str| {
        value
            .as_ref()
            .and_then(|value| value.pointer(pointer))
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    };
    let agent = text_at("/memory/agent")
        .filter(|agent| agent != "auto")
        .unwrap_or_else(|| SUPERVISOR_MEMORY_AGENT.to_owned());
    let enabled = value
        .as_ref()
        .and_then(|value| value.pointer("/memory/sync/enabled"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);
    let remote = text_at("/memory/sync/remote").filter(|_| enabled);
    (agent, remote)
}

/// `git bundle create --all` of the memory repo, or `None` when it has no commits.
fn bundle_memory(repo: &Path, out: &Path) -> Result<Option<Vec<u8>>, CheckpointError> {
    if !repo.is_dir() {
        return Ok(None);
    }
    let refs = git(repo, &["for-each-ref", "--count=1", "--format=%(refname)"])?;
    if refs.trim().is_empty() {
        return Ok(None);
    }
    let out_arg = out.to_string_lossy().into_owned();
    git(repo, &["bundle", "create", &out_arg, "--all"])?;
    Ok(Some(std::fs::read(out)?))
}

fn git(repo: &Path, args: &[&str]) -> Result<String, CheckpointError> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()?;
    if !output.status.success() {
        return Err(CheckpointError::Other(format!(
            "git {}: {}",
            args.first().copied().unwrap_or_default(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub(crate) fn build_archive(entries: &[(&str, &[u8])]) -> Result<Vec<u8>, CheckpointError> {
    let encoder = zstd::stream::write::Encoder::new(Vec::new(), ZSTD_LEVEL)?;
    let mut builder = tar::Builder::new(encoder);
    for (name, data) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_size(data.len() as u64);
        header.set_mode(0o600);
        header.set_mtime(0);
        builder.append_data(&mut header, name, *data)?;
    }
    Ok(builder.into_inner()?.finish()?)
}

/// Every file of a checkpoint archive by its archive path, within
/// [`CHECKPOINT_MAX_ARCHIVE_BYTES`] decompressed. Blocking.
pub fn read_archive(bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, CheckpointError> {
    let mut files = BTreeMap::new();
    visit_archive(bytes, CHECKPOINT_MAX_ARCHIVE_BYTES, |name, entry| {
        let mut data = Vec::new();
        entry.read_to_end(&mut data)?;
        files.insert(name.to_owned(), data);
        Ok(())
    })?;
    Ok(files)
}

/// Streams each archive entry to `visit` with its archive path. Every byte, including the
/// entries `visit` leaves unread, comes through one reader bounded at `limit` decompressed
/// bytes; past it the whole read fails with [`CHECKPOINT_TOO_LARGE`]. Blocking.
pub(crate) fn visit_archive(
    bytes: &[u8],
    limit: u64,
    mut visit: impl FnMut(&str, &mut dyn Read) -> Result<(), CheckpointError>,
) -> Result<(), CheckpointError> {
    let exceeded = Rc::new(Cell::new(false));
    let result = visit_entries(bytes, limit, &exceeded, &mut visit);
    if exceeded.get() {
        return Err(CheckpointError::TooLarge {
            bytes: limit.saturating_add(1),
            budget: limit,
        });
    }
    result
}

fn visit_entries(
    bytes: &[u8],
    limit: u64,
    exceeded: &Rc<Cell<bool>>,
    visit: &mut dyn FnMut(&str, &mut dyn Read) -> Result<(), CheckpointError>,
) -> Result<(), CheckpointError> {
    let decoder = zstd::stream::read::Decoder::new(bytes)?;
    let mut archive = tar::Archive::new(BoundedReader {
        inner: decoder,
        left: limit,
        exceeded: Rc::clone(exceeded),
    });
    for entry in archive.entries()? {
        let mut entry = entry?;
        let name = entry.path()?.to_string_lossy().into_owned();
        visit(&name, &mut entry)?;
    }
    Ok(())
}

/// Fails a read once more than `left` bytes came through, and records that it did: the
/// tar reader may wrap the error, the flag stays exact.
struct BoundedReader<R> {
    inner: R,
    left: u64,
    exceeded: Rc<Cell<bool>>,
}

impl<R: Read> Read for BoundedReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        match self.left.checked_sub(n as u64) {
            Some(left) => {
                self.left = left;
                Ok(n)
            }
            None => {
                self.exceeded.set(true);
                Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "decompressed checkpoint exceeds its bound",
                ))
            }
        }
    }
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, CheckpointError> + Send + 'static,
) -> Result<T, CheckpointError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| CheckpointError::Other(format!("blocking task: {error}")))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const SUPERVISOR_ID: &str = "11111111-2222-3333-4444-555555555555";

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let dir = std::env::temp_dir()
                .join(format!("ferryx-checkpoint-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Owner-only creation, as the src-tauri callback does.
    fn private_writer() -> PrivateFileWriter {
        Arc::new(|path: &Path, bytes: &[u8]| {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            options.open(path)?.write_all(bytes)
        })
    }

    async fn hub(dir: &Path) -> SqlitePool {
        crate::storage::connect(&dir.join("hub.sqlite")).await.unwrap()
    }

    async fn seed(pool: &SqlitePool) {
        sqlx::query(
            "INSERT INTO supervisor_identity(supervisor_id, owner_principal, created_at)
             VALUES(?, 'owner@example.com', '2026-11-01T00:00:00Z')",
        )
        .bind(SUPERVISOR_ID)
        .execute(pool)
        .await
        .unwrap();
        for (host, sessions) in [("local", "[1]"), ("mac-b", "[2]"), ("stranger", "[3]")] {
            sqlx::query(
                "INSERT INTO fleet_inventory_cache(host_id, sessions, updated_at)
                 VALUES(?, ?, 'now')",
            )
            .bind(host)
            .bind(sessions)
            .execute(pool)
            .await
            .unwrap();
        }
        for (key, host) in [("lane-a", None), ("lane-b", Some("mac-b"))] {
            sqlx::query(
                "INSERT INTO lanes(key, title, lead_session_key, cwd, session_key, status,
                                   done_signals, last_activity_at, host_id)
                 VALUES(?, 't', 'lead', '/tmp', 's', 'queued', '[]', 'now', ?)",
            )
            .bind(key)
            .bind(host)
            .execute(pool)
            .await
            .unwrap();
        }
        sqlx::query("INSERT INTO inbox_digests(body, created_at) VALUES('digest', 'now')")
            .execute(pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO messages(id, session_key, role, content) VALUES('m1', 's', 'user', 'hi')",
        )
        .execute(pool)
        .await
        .unwrap();
    }

    fn request(dir: &Path) -> ExportRequest {
        let mut request = ExportRequest::new(
            dir.to_path_buf(),
            "machine-a".to_owned(),
            dir.join("out.ckpt"),
            private_writer(),
        );
        request.paired = vec![PairedHostMapping {
            host_id: "mac-b".to_owned(),
            machine_id: "machine-b".to_owned(),
        }];
        request
    }

    /// Writes the archived database to disk and opens it.
    async fn archived_db(dir: &Path, files: &BTreeMap<String, Vec<u8>>) -> SqlitePool {
        let path = dir.join("restored.sqlite");
        std::fs::write(&path, &files[ARCHIVE_DATABASE]).unwrap();
        crate::storage::connect_readonly(&path).await.unwrap()
    }

    #[tokio::test]
    async fn export_rewrites_host_ids_and_empties_bulk_tables() {
        let dir = TempDir::new();
        let pool = hub(dir.path()).await;
        seed(&pool).await;
        let request = request(dir.path());

        let exported = export(&pool, &request).await.unwrap();

        let bytes = std::fs::read(&exported.path).unwrap();
        assert_eq!(exported.compressed_bytes, bytes.len() as u64);
        assert_eq!(exported.checkpoint_id, sha256_hex(&bytes));
        assert_eq!(exported.checkpoint_id.len(), 64);
        assert_eq!(exported.manifest.supervisor_id.as_deref(), Some(SUPERVISOR_ID));
        assert_eq!(
            exported.manifest.host_map.get("mac-b").map(String::as_str),
            Some("machine:machine-b")
        );
        let files = read_archive(&bytes).unwrap();
        let manifest: CheckpointManifest =
            serde_json::from_slice(&files[ARCHIVE_MANIFEST]).unwrap();
        assert_eq!(manifest, exported.manifest);

        let copy = archived_db(dir.path(), &files).await;
        let hosts: Vec<String> =
            sqlx::query_scalar("SELECT host_id FROM fleet_inventory_cache ORDER BY sessions")
                .fetch_all(&copy)
                .await
                .unwrap();
        assert_eq!(hosts, ["machine:machine-a", "machine:machine-b", "unmapped:stranger"]);
        let lanes: Vec<String> = sqlx::query_scalar("SELECT host_id FROM lanes ORDER BY key")
            .fetch_all(&copy)
            .await
            .unwrap();
        assert_eq!(lanes, ["machine:machine-a", "machine:machine-b"]);
        for table in EXCLUDED_TABLES {
            let rows: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
                .fetch_one(&copy)
                .await
                .unwrap();
            assert_eq!(rows, 0, "{table}");
        }
        let identity: String = sqlx::query_scalar("SELECT supervisor_id FROM supervisor_identity")
            .fetch_one(&copy)
            .await
            .unwrap();
        assert_eq!(identity, SUPERVISOR_ID);
        let check: String = sqlx::query_scalar("PRAGMA integrity_check")
            .fetch_one(&copy)
            .await
            .unwrap();
        assert_eq!(check, "ok");

        // The live database is untouched.
        let live: String =
            sqlx::query_scalar("SELECT host_id FROM fleet_inventory_cache WHERE sessions = '[1]'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(live, "local");
        let digests: i64 = sqlx::query_scalar("SELECT count(*) FROM inbox_digests")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(digests, 1);
        let staging_left = std::fs::read_dir(dir.path()).unwrap().any(|entry| {
            let name = entry.unwrap().file_name();
            name.to_string_lossy().starts_with(".checkpoint-staging")
        });
        assert!(!staging_left);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn checkpoint_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new();
        let pool = hub(dir.path()).await;
        let exported = export(&pool, &request(dir.path())).await.unwrap();
        let mode = std::fs::metadata(&exported.path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[tokio::test]
    async fn env_allowlist_inlines_secret_files_and_drops_machine_keys() {
        let dir = TempDir::new();
        let pool = hub(dir.path()).await;
        std::fs::write(dir.path().join("slack-bot"), "xoxb-secret\n").unwrap();
        let env = dir.path().join("hub.env");
        std::fs::write(
            &env,
            "FERRYX_HUB_DB=/Users/a/.ferryx/hub/hub.sqlite\n\
             SLACK_BOT_TOKEN_FILE=slack-bot\n\
             SLACK_APP_TOKEN=xapp-1\n\
             FERRYX_HUB_MHC_BIN=/opt/mhc\n\
             FERRYX_HUB_MHC_MODEL=gpt\n\
             HOME=/Users/a\n\
             OPENAI_API_KEY=sk-1\n",
        )
        .unwrap();
        let profile_dir = dir.path().join("mhc-supervisor").join(".omo");
        std::fs::create_dir_all(&profile_dir).unwrap();
        let profile = profile_dir.join("omo.jsonc");
        std::fs::write(&profile, crate::meta::mhc_profile::memory_identity_config()).unwrap();
        std::fs::write(
            dir.path().join("mhc-supervisor-sessions.json"),
            r#"{"session":"provider-local"}"#,
        )
        .unwrap();
        let mut request = request(dir.path());
        request.env_file = Some(env);
        request.profile_omo_jsonc = Some(profile.clone());

        let exported = export(&pool, &request).await.unwrap();

        let files = read_archive(&std::fs::read(&exported.path).unwrap()).unwrap();
        let env = crate::hub_env::parse_env_file(&String::from_utf8_lossy(&files[ARCHIVE_ENV]));
        let expected: BTreeMap<String, String> = [
            ("FERRYX_HUB_MHC_MODEL", "gpt"),
            ("OPENAI_API_KEY", "sk-1"),
            ("SLACK_APP_TOKEN", "xapp-1"),
            ("SLACK_BOT_TOKEN", "xoxb-secret"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
        assert_eq!(env, expected);
        assert_eq!(files[ARCHIVE_PROFILE], std::fs::read(&profile).unwrap());
        assert!(files.keys().all(|name| !name.contains("sessions")));
        assert!(files
            .values()
            .all(|data| !String::from_utf8_lossy(data).contains("provider-local")));
        assert_eq!(exported.manifest.memory_agent, SUPERVISOR_MEMORY_AGENT);
        assert!(exported.manifest.has_profile);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_database_snapshot_is_created_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new();
        let pool = hub(dir.path()).await;
        seed(&pool).await;
        let path = dir.path().join("snapshot.sqlite");

        snapshot_into(&pool, &path).await.unwrap();

        let metadata = std::fs::metadata(&path).unwrap();
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        assert!(metadata.len() > 0, "VACUUM INTO filled the private file");
    }

    #[tokio::test]
    async fn export_over_budget_fails_too_large_and_writes_nothing() {
        let dir = TempDir::new();
        let pool = hub(dir.path()).await;
        let mut request = request(dir.path());
        request.budget_bytes = 1;

        let error = export(&pool, &request).await.unwrap_err();

        assert_eq!(error.code(), CHECKPOINT_TOO_LARGE);
        assert!(matches!(error, CheckpointError::TooLarge { budget: 1, .. }));
        assert!(!request.output_path.exists());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn export_during_live_writes_is_consistent() {
        let dir = TempDir::new();
        let pool = hub(dir.path()).await;
        let writer_pool = pool.clone();
        let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel::<()>();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel::<()>();
        let writer = tokio::spawn(async move {
            let mut started = Some(started_tx);
            let mut n = 0_u64;
            loop {
                if stop_rx.try_recv().is_ok() {
                    return n;
                }
                sqlx::query("INSERT INTO hub_meta(key, value) VALUES(?, hex(randomblob(256)))")
                    .bind(format!("k{n}"))
                    .execute(&writer_pool)
                    .await
                    .unwrap();
                n += 1;
                if let Some(started) = started.take() {
                    started.send(()).unwrap();
                }
            }
        });
        started_rx.await.unwrap();

        let exported = export(&pool, &request(dir.path())).await.unwrap();
        stop_tx.send(()).unwrap();
        assert!(writer.await.unwrap() > 0);

        let files = read_archive(&std::fs::read(&exported.path).unwrap()).unwrap();
        let copy = archived_db(dir.path(), &files).await;
        let check: String = sqlx::query_scalar("PRAGMA integrity_check")
            .fetch_one(&copy)
            .await
            .unwrap();
        assert_eq!(check, "ok");
    }

    #[tokio::test]
    async fn large_hub_with_bulk_tables_exports_under_budget() {
        let dir = TempDir::new();
        let pool = hub(dir.path()).await;
        // ~26 MB each of incompressible text in the two excluded tables, ~28 MB kept.
        for (table, columns, values) in [
            ("inbox_digests", "body, created_at", "hex(randomblob(4096)), 'now'"),
            (
                "messages",
                "id, session_key, role, content",
                "'m' || i, 's', 'user', hex(randomblob(4096))",
            ),
            ("hub_meta", "key, value", "'big' || i, hex(randomblob(4096))"),
        ] {
            sqlx::query(&format!(
                "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 3300)
                 INSERT INTO {table}({columns}) SELECT {values} FROM n"
            ))
            .execute(&pool)
            .await
            .unwrap();
        }
        let live_bytes: i64 = sqlx::query_scalar(
            "SELECT page_count * page_size FROM pragma_page_count(), pragma_page_size()",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(live_bytes > 80_000_000, "fixture is {live_bytes} bytes");

        let exported = export(&pool, &request(dir.path())).await.unwrap();

        assert!(exported.compressed_bytes <= CHECKPOINT_MAX_COMPRESSED_BYTES);
    }

    fn git_in(repo: &Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }

    #[tokio::test]
    async fn memory_repo_is_bundled_with_its_commits() {
        let dir = TempDir::new();
        let pool = hub(dir.path()).await;
        let memory_root = dir.path().join("memory-agents");
        let agent_dir = memory_agent_dir(&memory_root, SUPERVISOR_MEMORY_AGENT);
        assert!(agent_dir.ends_with("ferryx-supervisor-d08240e6"));
        let repo = agent_dir.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        git_in(&repo, &["init", "-q"]);
        std::fs::write(repo.join("facts.md"), "fact\n").unwrap();
        git_in(&repo, &["add", "facts.md"]);
        git_in(
            &repo,
            &[
                "-c",
                "user.name=OmO Agent",
                "-c",
                "user.email=agent@omo.local",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-q",
                "-m",
                "chore(facts): extract 1 fact",
            ],
        );
        let mut request = request(dir.path());
        request.memory_root = Some(memory_root);

        let exported = export(&pool, &request).await.unwrap();

        assert_eq!(
            exported.manifest.memory,
            MemorySource::Bundle {
                agent_dir: "ferryx-supervisor-d08240e6".to_owned()
            }
        );
        let files = read_archive(&std::fs::read(&exported.path).unwrap()).unwrap();
        let bundle = dir.path().join("check.bundle");
        std::fs::write(&bundle, &files[ARCHIVE_MEMORY_BUNDLE]).unwrap();
        git_in(&repo, &["bundle", "verify", &bundle.to_string_lossy()]);
    }

    #[tokio::test]
    async fn configured_sync_remote_is_recorded_instead_of_bundled() {
        let dir = TempDir::new();
        let pool = hub(dir.path()).await;
        let profile = dir.path().join("omo.jsonc");
        std::fs::write(
            &profile,
            "// pinned by ferryx\n{\"memory\":{\"agent\":\"ferryx-supervisor\",\"sync\":{\"remote\":\"git@example.com:me/memory.git\"}}}\n",
        )
        .unwrap();
        let mut request = request(dir.path());
        request.profile_omo_jsonc = Some(profile);
        request.memory_root = Some(dir.path().join("memory-agents"));

        let exported = export(&pool, &request).await.unwrap();

        assert_eq!(
            exported.manifest.memory,
            MemorySource::Remote {
                remote: "git@example.com:me/memory.git".to_owned()
            }
        );
        let files = read_archive(&std::fs::read(&exported.path).unwrap()).unwrap();
        assert!(!files.contains_key(ARCHIVE_MEMORY_BUNDLE));
    }

    #[test]
    fn env_keys_exported_only_when_machine_independent() {
        for key in [
            "SLACK_BOT_TOKEN",
            "DISCORD_BOT_TOKEN",
            "OPENAI_API_KEY",
            "FERRYX_HUB_MHC_MODEL",
        ] {
            assert!(env_key_exported(key), "{key}");
        }
        for key in [
            "FERRYX_HUB_DB",
            "FERRYX_HUB_ENV_FILE",
            "SLACK_BOT_TOKEN_FILE",
            "FERRYX_HUB_MHC_BIN",
            "HOME",
            "PATH",
        ] {
            assert!(!env_key_exported(key), "{key}");
        }
    }
}
