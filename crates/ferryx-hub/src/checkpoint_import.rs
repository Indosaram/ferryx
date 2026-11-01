//! Import side of the portable supervisor checkpoint (plan todo 7, part B).
//!
//! Import reads an archive written by [`crate::checkpoint::export`] through the same manifest
//! types and archive names, checks the size budget and the target's supervisor identity, maps
//! host ids back to this machine's view, and stages the database as `hub.sqlite.incoming`. The
//! swap into `hub.sqlite` happens only at the next hub open, before any pool is created (see
//! [`apply_staged_swap`]); the previous file is kept as `hub.sqlite.pre-move-<ts>`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use serde::Serialize;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;

use crate::checkpoint::{
    env_key_exported, memory_agent_dir, read_archive, render_env, sha256_hex, CheckpointError,
    CheckpointManifest, MemorySource, PairedHostMapping, PrivateFileWriter, ARCHIVE_DATABASE,
    ARCHIVE_ENV, ARCHIVE_MANIFEST, ARCHIVE_MEMORY_BUNDLE, ARCHIVE_PROFILE,
    CHECKPOINT_FORMAT_VERSION, CHECKPOINT_MAX_COMPRESSED_BYTES, HOST_ID_TABLES, LOCAL_HOST_ID,
    MACHINE_HOST_PREFIX, REMOTE_HOST_UNMAPPED, SUPERVISOR_IDENTITY_MISMATCH,
};

/// The live hub database file name.
const LIVE_DB: &str = "hub.sqlite";
/// Staged database, swapped into `hub.sqlite` at the next hub open.
pub const INCOMING_DB: &str = "hub.sqlite.incoming";
const INCOMING_TMP: &str = "hub.sqlite.incoming.tmp";
/// Written next to the hub database: what the last import left unmapped.
pub const IMPORT_STATUS_FILE: &str = "checkpoint-import-status.json";
/// A `hub.sqlite.pre-lease-<ts>` file marks an archived (handed-over) hub: it counts as fresh.
const ARCHIVED_PREFIX: &str = "hub.sqlite.pre-lease-";
/// Run states a hold may interrupt (the `held_from` CHECK of migration 0024).
const HOLDABLE_STATES: &str = "'ready','dispatching','running','waiting_children',\
    'waiting_event','awaiting_owner','verifying','degraded','delivery_uncertain'";

/// Errors returned by checkpoint import.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    /// Archive budget, decompression and archive-format failures, with the export side's codes.
    #[error(transparent)]
    Checkpoint(#[from] CheckpointError),
    #[error("SUPERVISOR_IDENTITY_MISMATCH: target holds {target}, checkpoint carries {incoming}")]
    IdentityMismatch { target: String, incoming: String },
    #[error("checkpoint manifest: {0}")]
    Manifest(String),
    #[error("memory restore: {0}")]
    Memory(String),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Sql(#[from] sqlx::Error),
    #[error(transparent)]
    Hub(#[from] crate::error::HubError),
}

impl ImportError {
    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Checkpoint(error) => error.code(),
            Self::IdentityMismatch { .. } => SUPERVISOR_IDENTITY_MISMATCH,
            Self::Manifest(_) => "CHECKPOINT_MANIFEST_INVALID",
            Self::Memory(_) => "CHECKPOINT_MEMORY_RESTORE_FAILED",
            Self::Io(_) | Self::Sql(_) | Self::Hub(_) => "CHECKPOINT_IMPORT_FAILED",
        }
    }
}

/// Creates `path` (never overwriting) with mode 0600 on unix and writes `bytes`. ferryx-hub
/// cannot depend on src-tauri, so production callers pass their own private-file writer
/// (owner-only ACL on Windows); this is the unix-mode fallback.
pub fn write_private_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

pub fn default_private_writer() -> PrivateFileWriter {
    Arc::new(write_private_file)
}

pub struct ImportOptions {
    /// `~/.omo/memory/agents`: the repo lands in `<root>/<agent>-<h8>/repo`
    /// ([`memory_agent_dir`]) for the manifest's `memory_agent`.
    pub memory_root: PathBuf,
    /// Hub env file (`~/.ferryx/hub/env`) the exported keys are merged into, owner-only.
    pub env_file: PathBuf,
    /// The mhc profile `.omo/omo.jsonc` to restore from the checkpoint; `None` keeps the
    /// target's profile untouched.
    pub profile_omo_jsonc: Option<PathBuf>,
    /// Unix seconds used for `pre-move-<ts>` names and hold timestamps.
    pub now_ts: i64,
    pub write_private: PrivateFileWriter,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum MemoryRestore {
    None,
    /// Cloned from the recorded `memory.sync.remote`.
    Remote(String),
    /// Cloned from the travelling git bundle.
    Bundle,
}

/// Result of a staged import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportOutcome {
    /// sha256 (hex) of the compressed archive, the export side's `checkpoint_id`.
    pub checkpoint_id: String,
    pub supervisor_id: Option<String>,
    pub staged_db: PathBuf,
    pub unmapped_hosts: Vec<String>,
    pub held_goals: Vec<String>,
    pub released_dispatches: u64,
    pub memory: MemoryRestore,
    /// Owner-only files written: the merged hub env file when the checkpoint carried keys.
    pub secrets: Vec<PathBuf>,
    pub profile: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostMapping {
    pub unmapped_hosts: Vec<String>,
    pub held_goals: Vec<String>,
    /// Goals an earlier import held that this run released because their host is mapped now.
    pub restored_goals: Vec<String>,
    pub released_dispatches: u64,
}

/// The local host id for an exported `machine:<id>` host id: `local` for this machine, the
/// paired host id for a paired machine, `None` when that machine is not paired here.
pub fn local_host_id(
    host_id: &str,
    own_machine_id: &str,
    paired: &[PairedHostMapping],
) -> Option<String> {
    let machine = host_id.strip_prefix(MACHINE_HOST_PREFIX)?;
    if machine == own_machine_id {
        return Some(LOCAL_HOST_ID.to_owned());
    }
    paired
        .iter()
        .find(|host| host.machine_id == machine)
        .map(|host| host.host_id.clone())
}

fn timestamp(now_ts: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp(now_ts, 0)
        .unwrap_or_default()
        .to_rfc3339()
}

struct Unpacked {
    checkpoint_id: String,
    manifest: CheckpointManifest,
    hub_db: PathBuf,
    memory_bundle: Option<PathBuf>,
    /// Exported env entries; stay in memory until the private writer stores them.
    env: BTreeMap<String, String>,
    profile: Option<Vec<u8>>,
}

/// A single path component: safe to join under a directory.
fn safe_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', ':'])
}

fn invalid(message: impl Into<String>) -> ImportError {
    ImportError::Manifest(message.into())
}

/// Checks the size budget, reads the archive, verifies the manifest and unpacks the database
/// and memory bundle into `staging`. Blocking.
fn unpack(archive: &Path, staging: &Path) -> Result<Unpacked, ImportError> {
    let bytes = fs::metadata(archive)?.len();
    if bytes > CHECKPOINT_MAX_COMPRESSED_BYTES {
        return Err(CheckpointError::TooLarge {
            bytes,
            budget: CHECKPOINT_MAX_COMPRESSED_BYTES,
        }
        .into());
    }
    let compressed = fs::read(archive)?;
    let checkpoint_id = sha256_hex(&compressed);
    // Unknown entries (a sessions json in particular) are ignored: sessions start fresh.
    let mut files = read_archive(&compressed)?;
    drop(compressed);

    let manifest = files
        .remove(ARCHIVE_MANIFEST)
        .ok_or_else(|| invalid(format!("missing {ARCHIVE_MANIFEST}")))?;
    let manifest: CheckpointManifest =
        serde_json::from_slice(&manifest).map_err(|error| invalid(error.to_string()))?;
    if manifest.format_version != CHECKPOINT_FORMAT_VERSION {
        return Err(invalid(format!(
            "unsupported format_version {}",
            manifest.format_version
        )));
    }
    if manifest
        .supervisor_id
        .as_ref()
        .is_some_and(|id| id.len() != 36)
    {
        return Err(invalid("supervisor_id is not a uuid"));
    }
    if !safe_name(&manifest.memory_agent) {
        return Err(invalid(format!(
            "unsafe memory agent {:?}",
            manifest.memory_agent
        )));
    }

    let db = files
        .remove(ARCHIVE_DATABASE)
        .ok_or_else(|| invalid(format!("missing {ARCHIVE_DATABASE}")))?;
    let hub_db = staging.join(ARCHIVE_DATABASE);
    fs::write(&hub_db, db)?;

    let memory_bundle = match &manifest.memory {
        MemorySource::Bundle { .. } => {
            let bundle = files
                .remove(ARCHIVE_MEMORY_BUNDLE)
                .ok_or_else(|| invalid(format!("missing {ARCHIVE_MEMORY_BUNDLE}")))?;
            let path = staging.join("supervisor.bundle");
            fs::write(&path, bundle)?;
            Some(path)
        }
        MemorySource::Absent | MemorySource::Remote { .. } => None,
    };

    // Only keys the manifest lists and the export allowlist admits: a machine-local key such
    // as FERRYX_HUB_DB never lands in the target env, even from a hand-made archive.
    let env = files
        .remove(ARCHIVE_ENV)
        .map(|body| crate::hub_env::parse_env_file(&String::from_utf8_lossy(&body)))
        .unwrap_or_default()
        .into_iter()
        .filter(|(key, _)| manifest.env_keys.contains(key) && env_key_exported(key))
        .collect();

    let profile = if manifest.has_profile {
        let profile = files
            .remove(ARCHIVE_PROFILE)
            .ok_or_else(|| invalid(format!("missing {ARCHIVE_PROFILE}")))?;
        Some(profile)
    } else {
        None
    };
    Ok(Unpacked {
        checkpoint_id,
        manifest,
        hub_db,
        memory_bundle,
        env,
        profile,
    })
}

/// Refuses unless the target is fresh, archived, or holds the same supervisor.
async fn check_identity(target_hub_dir: &Path, incoming: Option<&str>) -> Result<(), ImportError> {
    let dir = target_hub_dir.to_owned();
    let archived = tokio::task::spawn_blocking(move || -> io::Result<bool> {
        match fs::read_dir(&dir) {
            Ok(entries) => {
                for entry in entries {
                    if entry?.file_name().to_string_lossy().starts_with(ARCHIVED_PREFIX) {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    })
    .await
    .map_err(io::Error::other)??;
    let db = target_hub_dir.join(LIVE_DB);
    if archived || !tokio::fs::try_exists(&db).await? {
        return Ok(());
    }
    let pool = crate::storage::connect_readonly(&db).await?;
    let held = async {
        let has_table: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'supervisor_identity'",
        )
        .fetch_one(&pool)
        .await?;
        if has_table == 0 {
            return Ok::<_, sqlx::Error>(Vec::new());
        }
        sqlx::query_scalar::<_, String>("SELECT supervisor_id FROM supervisor_identity")
            .fetch_all(&pool)
            .await
    }
    .await;
    pool.close().await;
    match held?
        .into_iter()
        .find(|target| Some(target.as_str()) != incoming)
    {
        Some(target) => Err(ImportError::IdentityMismatch {
            target,
            incoming: incoming.unwrap_or("<none>").to_owned(),
        }),
        None => Ok(()),
    }
}

/// Opens a staged database file as one connection in rollback-journal mode, so the result
/// is a single file that can be renamed without WAL sidecars. No migration runs here: the
/// real open migrates after the swap.
async fn open_staged(path: &Path) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .foreign_keys(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Delete)
        .busy_timeout(crate::storage::BUSY_TIMEOUT);
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
}

/// Maps exported `machine:<id>` host ids to this machine's view, in one transaction.
///
/// `machine:<own>` becomes `local`, a paired machine becomes its paired host id, and any
/// other id stays as it is. Every goal tied to an unmapped host (through its execution
/// binding or its lane) has its ready dispatch released and is held `paused` with
/// `held_detail = REMOTE_HOST_UNMAPPED`; it is never redirected to another pane. Run again
/// (a later import, or after pairing), it maps hosts that became paired and restores the
/// goals it held whose hosts are all mapped now.
pub async fn map_host_ids(
    pool: &SqlitePool,
    own_machine_id: &str,
    paired: &[PairedHostMapping],
    now_ts: i64,
) -> Result<HostMapping, sqlx::Error> {
    let now = timestamp(now_ts);
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    // Export rewrites `host_id` only in tables that have the column; so does import.
    let mut tables = Vec::new();
    for &table in HOST_ID_TABLES {
        let columns: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pragma_table_info(?) WHERE name = 'host_id'",
        )
        .bind(table)
        .fetch_one(&mut *tx)
        .await?;
        if columns > 0 {
            tables.push(table);
        }
    }
    let mut hosts = BTreeSet::new();
    for &table in &tables {
        let rows: Vec<String> = sqlx::query_scalar(&format!(
            "SELECT DISTINCT host_id FROM {table} WHERE substr(host_id, 1, ?) = ?"
        ))
        .bind(MACHINE_HOST_PREFIX.len() as i64)
        .bind(MACHINE_HOST_PREFIX)
        .fetch_all(&mut *tx)
        .await?;
        hosts.extend(rows);
    }
    let mut mapping = HostMapping::default();
    let mut blocked = BTreeSet::new();
    for host in hosts {
        if let Some(local) = local_host_id(&host, own_machine_id, paired) {
            for &table in &tables {
                // The fleet cache is keyed by host: a stale local row yields to the import.
                let verb = if table == "fleet_inventory_cache" {
                    "UPDATE OR REPLACE"
                } else {
                    "UPDATE"
                };
                sqlx::query(&format!("{verb} {table} SET host_id = ? WHERE host_id = ?"))
                    .bind(&local)
                    .bind(&host)
                    .execute(&mut *tx)
                    .await?;
            }
            continue;
        }
        let goals: Vec<String> = sqlx::query_scalar(
            "SELECT goal_id FROM supervisor_goals
             WHERE goal_id IN (SELECT goal_id FROM supervisor_execution_bindings WHERE host_id = ?)
                OR lane_key IN (SELECT key FROM lanes WHERE host_id = ?)",
        )
        .bind(&host)
        .bind(&host)
        .fetch_all(&mut *tx)
        .await?;
        blocked.extend(goals);
        mapping.unmapped_hosts.push(host);
    }
    for goal_id in &blocked {
        let released = sqlx::query(
            "UPDATE supervisor_dispatches
             SET state = 'released', claimed_at = COALESCE(claimed_at, ?), updated_at = ?
             WHERE goal_id = ? AND state = 'ready'",
        )
        .bind(&now)
        .bind(&now)
        .bind(goal_id)
        .execute(&mut *tx)
        .await?;
        mapping.released_dispatches += released.rows_affected();
        // SQLite evaluates every right-hand side against the old row: held_from keeps the
        // state the hold interrupted.
        sqlx::query(&format!(
            "UPDATE supervisor_goals
             SET held_from = run_state, held_detail = ?, run_state = 'paused', state_detail = ?,
                 updated_at = ?
             WHERE goal_id = ? AND run_state IN ({HOLDABLE_STATES})"
        ))
        .bind(REMOTE_HOST_UNMAPPED)
        .bind(REMOTE_HOST_UNMAPPED)
        .bind(&now)
        .bind(goal_id)
        .execute(&mut *tx)
        .await?;
    }
    let held: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT goal_id, held_from FROM supervisor_goals
         WHERE run_state = 'paused' AND held_detail = ? ORDER BY goal_id",
    )
    .bind(REMOTE_HOST_UNMAPPED)
    .fetch_all(&mut *tx)
    .await?;
    for (goal_id, held_from) in held {
        if blocked.contains(&goal_id) {
            mapping.held_goals.push(goal_id);
            continue;
        }
        // Its host is mapped now. A dispatch released by the hold was never sent, so a goal
        // held while ready or dispatching goes back to ready for a fresh dispatch.
        let state = match held_from.as_deref() {
            None | Some("ready") | Some("dispatching") => "ready",
            Some(other) => other,
        };
        sqlx::query(
            "UPDATE supervisor_goals
             SET run_state = ?, state_detail = NULL, held_from = NULL, held_detail = NULL,
                 updated_at = ?
             WHERE goal_id = ?",
        )
        .bind(state)
        .bind(&now)
        .bind(&goal_id)
        .execute(&mut *tx)
        .await?;
        mapping.restored_goals.push(goal_id);
    }
    tx.commit().await?;
    Ok(mapping)
}

fn suffixed(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

fn run_git(command: &mut Command) -> Result<(), ImportError> {
    let output = command
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|error| ImportError::Memory(format!("git: {error}")))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(ImportError::Memory(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ))
    }
}

/// What import changed outside the staged database, so a failure can put it back.
#[derive(Default)]
struct Undo {
    memory_repo: Option<PathBuf>,
    memory_aside: Option<PathBuf>,
    /// (installed path, previous file moved aside)
    files: Vec<(PathBuf, Option<PathBuf>)>,
    staged_tmp: Option<PathBuf>,
}

impl Undo {
    fn rollback(self) {
        if let Some(tmp) = self.staged_tmp {
            let _ = fs::remove_file(tmp);
        }
        for (installed, aside) in self.files.into_iter().rev() {
            let _ = fs::remove_file(&installed);
            if let Some(aside) = aside {
                if let Err(error) = fs::rename(&aside, &installed) {
                    let path = installed.display();
                    tracing::warn!(%error, %path, "checkpoint rollback: file not restored");
                }
            }
        }
        if let Some(repo) = self.memory_repo {
            let _ = fs::remove_dir_all(&repo);
            if let Some(aside) = self.memory_aside {
                if let Err(error) = fs::rename(&aside, &repo) {
                    let path = repo.display();
                    tracing::warn!(%error, %path, "checkpoint rollback: memory repo not restored");
                }
            }
        }
    }
}

/// Clones the memory repo from the recorded remote or the travelling bundle into
/// `<memory_root>/<agent>-<h8>/repo`. An existing repo is moved aside to `repo.pre-move-<ts>`,
/// never merged.
fn restore_memory(
    manifest: &CheckpointManifest,
    bundle: Option<&Path>,
    memory_root: &Path,
    now_ts: i64,
    undo: &mut Undo,
) -> Result<MemoryRestore, ImportError> {
    let (source, restore) = match (&manifest.memory, bundle) {
        (MemorySource::Remote { remote }, _) => {
            (PathBuf::from(remote), MemoryRestore::Remote(remote.clone()))
        }
        (MemorySource::Bundle { .. }, Some(bundle)) => (bundle.to_owned(), MemoryRestore::Bundle),
        (MemorySource::Bundle { .. }, None) => {
            return Err(invalid(format!("missing {ARCHIVE_MEMORY_BUNDLE}")))
        }
        (MemorySource::Absent, _) => return Ok(MemoryRestore::None),
    };
    let agent_dir = memory_agent_dir(memory_root, &manifest.memory_agent);
    let repo = agent_dir.join("repo");
    fs::create_dir_all(&agent_dir)?;
    if repo.exists() {
        let aside = suffixed(&repo, &format!(".pre-move-{now_ts}"));
        fs::rename(&repo, &aside)?;
        undo.memory_aside = Some(aside);
    }
    undo.memory_repo = Some(repo.clone());
    run_git(
        Command::new("git")
            .arg("clone")
            .arg("--quiet")
            .arg(&source)
            .arg(&repo),
    )?;
    let agent_id = agent_dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| manifest.memory_agent.clone());
    let email = format!("{agent_id}@omo.local");
    let config = [
        ("omo.agentid", agent_id.as_str()),
        ("user.name", "OmO Agent"),
        ("user.email", email.as_str()),
        ("commit.gpgsign", "false"),
        ("gc.auto", "0"),
    ];
    for (key, value) in config {
        run_git(
            Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(["config", "--local", key, value]),
        )?;
    }
    Ok(restore)
}

/// Installs `body` at `dest` owner-only through `write_private`: written as
/// `<dest>.incoming`, the previous file kept as `<dest>.pre-move-<ts>`, then renamed into place.
fn install_private(
    dest: &Path,
    body: &[u8],
    write_private: &PrivateFileWriter,
    now_ts: i64,
    undo: &mut Undo,
) -> Result<(), ImportError> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let incoming = suffixed(dest, ".incoming");
    let _ = fs::remove_file(&incoming);
    (**write_private)(&incoming, body)?;
    let aside = if dest.exists() {
        let aside = suffixed(dest, &format!(".pre-move-{now_ts}"));
        fs::rename(dest, &aside)?;
        Some(aside)
    } else {
        None
    };
    undo.files.push((dest.to_owned(), aside));
    fs::rename(&incoming, dest)?;
    Ok(())
}

/// Merges the exported env entries over the target env file (machine-local keys there stay)
/// and installs the result owner-only. Nothing is written when the checkpoint has no keys.
fn install_env(
    env: &BTreeMap<String, String>,
    env_file: &Path,
    write_private: &PrivateFileWriter,
    now_ts: i64,
    undo: &mut Undo,
) -> Result<Vec<PathBuf>, ImportError> {
    if env.is_empty() {
        return Ok(Vec::new());
    }
    let mut merged = match fs::read_to_string(env_file) {
        Ok(text) => crate::hub_env::parse_env_file(&text),
        Err(error) if error.kind() == io::ErrorKind::NotFound => BTreeMap::new(),
        Err(error) => return Err(error.into()),
    };
    for (key, value) in env {
        // The imported value replaces a local `<KEY>_FILE` pointer to the same secret.
        merged.remove(&format!("{key}_FILE"));
        merged.insert(key.clone(), value.clone());
    }
    install_private(
        env_file,
        render_env(&merged).as_bytes(),
        write_private,
        now_ts,
        undo,
    )?;
    Ok(vec![env_file.to_owned()])
}

#[derive(Serialize)]
struct ImportStatus<'a> {
    checkpoint_id: &'a str,
    supervisor_id: Option<&'a str>,
    imported_at: String,
    unmapped_hosts: &'a [String],
    held_goals: &'a [String],
    memory: &'a MemoryRestore,
}

/// Import a checkpoint archive written by [`crate::checkpoint::export`] into `target_hub_dir`,
/// staging the database.
///
/// `paired` lists every machine paired with this one (its local host id and machine id).
/// Nothing is live until the next hub open calls [`apply_staged_swap`]; any failure here
/// rolls back the memory repo, the env file, the profile and the staged file.
pub async fn import(
    archive: &Path,
    target_hub_dir: &Path,
    own_machine_id: &str,
    paired: &[PairedHostMapping],
    options: ImportOptions,
) -> Result<ImportOutcome, ImportError> {
    let staging = target_hub_dir.join(format!(".checkpoint-staging-{}", uuid::Uuid::new_v4()));
    let result =
        import_staged(archive, target_hub_dir, own_machine_id, paired, options, &staging).await;
    let cleanup = staging.clone();
    let _ = tokio::task::spawn_blocking(move || fs::remove_dir_all(cleanup)).await;
    result
}

async fn import_staged(
    archive: &Path,
    target_hub_dir: &Path,
    own_machine_id: &str,
    paired: &[PairedHostMapping],
    options: ImportOptions,
    staging: &Path,
) -> Result<ImportOutcome, ImportError> {
    let unpacked = {
        let (archive, staging) = (archive.to_owned(), staging.to_owned());
        tokio::task::spawn_blocking(move || -> Result<Unpacked, ImportError> {
            fs::create_dir_all(&staging)?;
            unpack(&archive, &staging)
        })
        .await
        .map_err(io::Error::other)??
    };
    check_identity(target_hub_dir, unpacked.manifest.supervisor_id.as_deref()).await?;
    let ImportOptions {
        memory_root,
        env_file,
        profile_omo_jsonc,
        now_ts,
        write_private,
    } = options;

    let tmp = target_hub_dir.join(INCOMING_TMP);
    let staged = stage_database(&unpacked.hub_db, &tmp, own_machine_id, paired, now_ts).await;
    let mapping = match staged {
        Ok(mapping) => mapping,
        Err(error) => {
            let tmp = tmp.clone();
            let _ = tokio::task::spawn_blocking(move || fs::remove_file(tmp)).await;
            return Err(error);
        }
    };

    let manifest = unpacked.manifest.clone();
    let bundle = unpacked.memory_bundle.clone();
    let env = unpacked.env.clone();
    let profile = unpacked.profile.clone();
    let target = target_hub_dir.to_owned();
    let (memory, secrets, profile) = tokio::task::spawn_blocking(move || {
        let mut undo = Undo {
            staged_tmp: Some(tmp.clone()),
            ..Undo::default()
        };
        let restored = restore_memory(&manifest, bundle.as_deref(), &memory_root, now_ts, &mut undo)
            .and_then(|memory| {
                let secrets = install_env(&env, &env_file, &write_private, now_ts, &mut undo)?;
                let profile = match (profile, profile_omo_jsonc) {
                    (Some(body), Some(dest)) => {
                        install_private(&dest, &body, &write_private, now_ts, &mut undo)?;
                        Some(dest)
                    }
                    _ => None,
                };
                // Commit point: the staged file becomes visible to the next open.
                fs::rename(&tmp, target.join(INCOMING_DB))?;
                Ok((memory, secrets, profile))
            });
        if restored.is_err() {
            undo.rollback();
        }
        restored
    })
    .await
    .map_err(io::Error::other)??;

    let outcome = ImportOutcome {
        checkpoint_id: unpacked.checkpoint_id,
        supervisor_id: unpacked.manifest.supervisor_id,
        staged_db: target_hub_dir.join(INCOMING_DB),
        unmapped_hosts: mapping.unmapped_hosts,
        held_goals: mapping.held_goals,
        released_dispatches: mapping.released_dispatches,
        memory,
        secrets,
        profile,
    };
    let status = ImportStatus {
        checkpoint_id: &outcome.checkpoint_id,
        supervisor_id: outcome.supervisor_id.as_deref(),
        imported_at: timestamp(now_ts),
        unmapped_hosts: &outcome.unmapped_hosts,
        held_goals: &outcome.held_goals,
        memory: &outcome.memory,
    };
    let body = serde_json::to_vec_pretty(&status).map_err(io::Error::other)?;
    tokio::fs::write(target_hub_dir.join(IMPORT_STATUS_FILE), body).await?;
    Ok(outcome)
}

/// Swaps a staged `hub.sqlite.incoming` into place. Call at hub open, before any pool is
/// created: nothing may hold the old file open (Windows refuses to rename an open file).
///
/// The old database and its `-wal`/`-shm` sidecars are kept as `hub.sqlite.pre-move-<ts>`
/// (plus the same suffixes); a stale WAL must never meet the incoming file. Returns the path
/// of the kept database, `None` when nothing was staged or no database existed before. A
/// failed swap puts the old files back.
pub fn apply_staged_swap(hub_dir: &Path, now_ts: i64) -> io::Result<Option<PathBuf>> {
    let incoming = hub_dir.join(INCOMING_DB);
    if !incoming.exists() {
        return Ok(None);
    }
    let live = hub_dir.join(LIVE_DB);
    let backup = hub_dir.join(format!("{LIVE_DB}.pre-move-{now_ts}"));
    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
    for suffix in ["", "-wal", "-shm"] {
        let from = suffixed(&live, suffix);
        if !from.exists() {
            continue;
        }
        let to = suffixed(&backup, suffix);
        if let Err(error) = fs::rename(&from, &to) {
            undo_moves(&moved);
            return Err(error);
        }
        moved.push((from, to));
    }
    if let Err(error) = fs::rename(&incoming, &live) {
        undo_moves(&moved);
        return Err(error);
    }
    Ok(moved.first().filter(|(from, _)| *from == live).map(|(_, to)| to.clone()))
}

fn undo_moves(moved: &[(PathBuf, PathBuf)]) {
    for (from, to) in moved.iter().rev() {
        if let Err(error) = fs::rename(to, from) {
            tracing::warn!(%error, path = %from.display(), "staged swap rollback failed");
        }
    }
}

/// Copies the unpacked database to `tmp` and maps its host ids there.
async fn stage_database(
    unpacked_db: &Path,
    tmp: &Path,
    own_machine_id: &str,
    paired: &[PairedHostMapping],
    now_ts: i64,
) -> Result<HostMapping, ImportError> {
    let (from, to) = (unpacked_db.to_owned(), tmp.to_owned());
    tokio::task::spawn_blocking(move || {
        let _ = fs::remove_file(&to);
        fs::copy(from, to)
    })
    .await
    .map_err(io::Error::other)??;
    let pool = open_staged(tmp).await?;
    let mapped = map_host_ids(&pool, own_machine_id, paired, now_ts).await;
    pool.close().await;
    Ok(mapped?)
}

/// Hub open with the staged swap: applies a pending `hub.sqlite.incoming` next to `db_path`
/// (on a blocking thread), then opens and migrates the database. The swap runs before the
/// pool exists, so no connection holds the old file.
pub async fn open_with_staged_swap(
    db_path: &Path,
    now_ts: i64,
) -> crate::error::Result<SqlitePool> {
    if let Some(dir) = db_path.parent() {
        let dir = dir.to_owned();
        tokio::task::spawn_blocking(move || apply_staged_swap(&dir, now_ts))
            .await
            .map_err(|error| crate::error::HubError::message(format!("staged swap: {error}")))??;
    }
    crate::storage::connect(db_path).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checkpoint::{
        build_archive, export, ExportRequest, CHECKPOINT_TOO_LARGE, SUPERVISOR_MEMORY_AGENT,
    };

    const SUP: &str = "aaaaaaaa-0000-4000-8000-000000000001";
    const OTHER_SUP: &str = "bbbbbbbb-0000-4000-8000-000000000002";
    const PROJECT: &str = "cccccccc-0000-4000-8000-000000000003";
    const G1: &str = "dddddddd-0000-4000-8000-000000000001";
    const G2: &str = "dddddddd-0000-4000-8000-000000000002";
    const NOW: &str = "2026-11-01T00:00:00Z";
    const SEEN: [&str; 3] = ["1700000000.000100", "1700000000.000200", "1700000000.000300"];

    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir()
            .join(format!("ferryx-checkpoint-{name}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn git_ok(args: &[&str]) -> String {
        let output = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    /// A memory repo with one commit; returns its HEAD.
    fn memory_source(dir: &Path) -> String {
        fs::create_dir_all(dir).unwrap();
        let repo = dir.to_str().unwrap();
        git_ok(&["init", "-q", repo]);
        fs::write(dir.join("facts.md"), "owner prefers small diffs\n").unwrap();
        git_ok(&["-C", repo, "add", "."]);
        git_ok(&["-C", repo, "commit", "-q", "-m", "chore(facts): extract 1 fact"]);
        git_ok(&["-C", repo, "rev-parse", "HEAD"])
    }

    async fn exec(pool: &SqlitePool, sql: &str, binds: &[&str]) {
        let mut query = sqlx::query(sql);
        for bind in binds {
            query = query.bind(*bind);
        }
        query.execute(pool).await.unwrap();
    }

    async fn seed_identity(pool: &SqlitePool, supervisor_id: &str) {
        exec(
            pool,
            "INSERT INTO supervisor_identity(supervisor_id, owner_principal, created_at) VALUES(?, 'owner:u1', ?)",
            &[supervisor_id, NOW],
        )
        .await;
    }

    fn paired(host_id: &str, machine_id: &str) -> PairedHostMapping {
        PairedHostMapping {
            host_id: host_id.to_owned(),
            machine_id: machine_id.to_owned(),
        }
    }

    /// The hub of machine-a. Host ids are as machine-a sees them: `local`, or its paired host
    /// ids `mac-b` (machine-b) and `mac-c` (machine-c).
    async fn source_hub(dir: &Path, goals: &[(&str, &str, &str)]) -> SqlitePool {
        let pool = crate::storage::connect(&dir.join(LIVE_DB)).await.unwrap();
        seed_identity(&pool, SUP).await;
        exec(
            &pool,
            "INSERT INTO supervisor_projects(project_id, supervisor_id, name, repo_identity, created_at)
             VALUES(?, ?, 'ferryx', 'github.com/o/ferryx', ?)",
            &[PROJECT, SUP, NOW],
        )
        .await;
        for &(lane, goal, host) in goals {
            let request = format!("req-{lane}");
            let git_dir = format!("/repo/{lane}");
            exec(
                &pool,
                "INSERT INTO lanes(key, title, lead_session_key, cwd, session_key, status, done_signals,
                     last_activity_at, host_id)
                 VALUES(?, 'Lane', 'lead', '/tmp', 'lead', 'working', 'file a', ?, ?)",
                &[lane, NOW, host],
            )
            .await;
            exec(&pool, "INSERT INTO lane_goal(lane_key, updated_at) VALUES(?, ?)", &[lane, NOW]).await;
            exec(
                &pool,
                "INSERT INTO supervisor_goals(goal_id, supervisor_id, project_id, lane_key, owner_principal,
                     run_state, created_at, updated_at)
                 VALUES(?, ?, ?, ?, 'owner:u1', 'ready', ?, ?)",
                &[goal, SUP, PROJECT, lane, NOW, NOW],
            )
            .await;
            exec(
                &pool,
                "INSERT INTO supervisor_execution_bindings(goal_id, kind, host_id, updated_at)
                 VALUES(?, 'pty', ?, ?)",
                &[goal, host, NOW],
            )
            .await;
            exec(
                &pool,
                "INSERT INTO supervisor_dispatches(goal_id, goal_revision, ordinal, request_id, generation,
                     instruction, state, created_at, updated_at)
                 VALUES(?, 1, 1, ?, 1, 'continue', 'ready', ?, ?)",
                &[goal, &request, NOW, NOW],
            )
            .await;
            exec(
                &pool,
                "INSERT OR IGNORE INTO supervisor_project_hosts(project_id, host_id, git_common_dir, updated_at)
                 VALUES(?, ?, ?, ?)",
                &[PROJECT, host, &git_dir, NOW],
            )
            .await;
            exec(
                &pool,
                "INSERT OR IGNORE INTO fleet_inventory_cache(host_id, sessions, updated_at) VALUES(?, '[]', ?)",
                &[host, NOW],
            )
            .await;
        }
        exec(
            &pool,
            "INSERT INTO slack_cursors(bot_id, channel_id, thread_ts, last_ts, updated_at)
             VALUES('B1', 'C1', '', ?, ?)",
            &[SEEN[2], NOW],
        )
        .await;
        exec(
            &pool,
            "CREATE TABLE IF NOT EXISTS slack_inbound_seen (id TEXT PRIMARY KEY NOT NULL, seen_at TEXT NOT NULL)",
            &[],
        )
        .await;
        for ts in SEEN {
            exec(
                &pool,
                "INSERT INTO slack_inbound_seen(id, seen_at) VALUES(?, ?)",
                &[&format!("C1/{ts}"), NOW],
            )
            .await;
        }
        exec(
            &pool,
            "INSERT INTO supervisor_event_cursors(consumer, last_seq, updated_at) VALUES('wakes', 7, ?)",
            &[NOW],
        )
        .await;
        pool
    }

    struct Fixture {
        root: PathBuf,
        archive: PathBuf,
        checkpoint_id: String,
        memory_head: String,
        profile: Vec<u8>,
    }

    /// Machine-a exports its hub through [`crate::checkpoint::export`]. `remote` configures
    /// `memory.sync.remote` in the profile, so the memory is recorded instead of bundled.
    async fn fixture(name: &str, goals: &[(&str, &str, &str)], remote: bool) -> Fixture {
        let root = temp_root(name);
        let source = root.join("source");
        fs::create_dir_all(&source).unwrap();
        let pool = source_hub(&source, goals).await;
        let memory_root = source.join("memory-agents");
        let (memory_head, profile) = if remote {
            let remote = source.join("memory-remote");
            let head = memory_source(&remote);
            let profile = serde_json::json!({
                "memory": {"agent": SUPERVISOR_MEMORY_AGENT, "sync": {"remote": remote}}
            });
            (head, format!("{profile}\n"))
        } else {
            let repo = memory_agent_dir(&memory_root, SUPERVISOR_MEMORY_AGENT).join("repo");
            let head = memory_source(&repo);
            (head, crate::meta::mhc_profile::memory_identity_config())
        };
        let profile_path = source.join(".omo").join("omo.jsonc");
        fs::create_dir_all(profile_path.parent().unwrap()).unwrap();
        fs::write(&profile_path, &profile).unwrap();
        let env = source.join("env");
        fs::write(&env, b"FERRYX_HUB_DB=/Users/a/.ferryx/hub/hub.sqlite\nSLACK_BOT_TOKEN=xoxb-secret\n")
            .unwrap();
        // Machine-local provider sessions: never part of a checkpoint.
        fs::write(
            source.join("mhc-supervisor-sessions.json"),
            br#"{"lead":"stale-session"}"#,
        )
        .unwrap();

        let archive = root.join("checkpoint.tar.zst");
        let mut request = ExportRequest::new(
            source.clone(),
            "machine-a".to_owned(),
            archive.clone(),
            default_private_writer(),
        );
        request.paired = vec![paired("mac-b", "machine-b"), paired("mac-c", "machine-c")];
        request.env_file = Some(env);
        request.profile_omo_jsonc = Some(profile_path);
        request.memory_root = Some(memory_root);
        let exported = export(&pool, &request).await.unwrap();
        pool.close().await;
        Fixture {
            root,
            archive,
            checkpoint_id: exported.checkpoint_id,
            memory_head,
            profile: profile.into_bytes(),
        }
    }

    /// Import options for a target machine rooted at `root` with its hub in `hub`.
    fn options(root: &Path, hub: &Path, now_ts: i64) -> ImportOptions {
        ImportOptions {
            memory_root: root.join("memory-agents"),
            env_file: hub.join("env"),
            profile_omo_jsonc: Some(profile_path(root)),
            now_ts,
            write_private: default_private_writer(),
        }
    }

    fn profile_path(root: &Path) -> PathBuf {
        root.join("profile").join(".omo").join("omo.jsonc")
    }

    fn memory_repo(root: &Path) -> PathBuf {
        memory_agent_dir(&root.join("memory-agents"), SUPERVISOR_MEMORY_AGENT).join("repo")
    }

    async fn open_target(hub: &Path) -> SqlitePool {
        open_with_staged_swap(&hub.join(LIVE_DB), 1_800_000_000).await.unwrap()
    }

    async fn host_of(pool: &SqlitePool, table: &str, key_sql: &str, key: &str) -> String {
        sqlx::query_scalar(&format!("SELECT host_id FROM {table} WHERE {key_sql} = ?"))
            .bind(key)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn round_trip_preserves_goal_identity_memory_and_starts_fresh_sessions() {
        let fx = fixture("round-trip", &[("lane-a", G1, "local")], false).await;
        let files = read_archive(&fs::read(&fx.archive).unwrap()).unwrap();
        assert!(files.keys().all(|name| !name.contains("sessions")));
        let hub = fx.root.join("hub");
        let outcome = import(&fx.archive, &hub, "machine-a", &[], options(&fx.root, &hub, 1_800_000_000))
            .await
            .unwrap();

        assert_eq!(outcome.checkpoint_id, fx.checkpoint_id);
        assert_eq!(outcome.supervisor_id.as_deref(), Some(SUP));
        assert_eq!(outcome.memory, MemoryRestore::Bundle);
        assert!(outcome.unmapped_hosts.is_empty());
        assert_eq!(outcome.profile, Some(profile_path(&fx.root)));
        assert_eq!(fs::read(profile_path(&fx.root)).unwrap(), fx.profile);
        assert!(!hub.join("mhc-supervisor-sessions.json").exists(), "sessions start fresh");
        let staging: Vec<_> = fs::read_dir(&hub)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(".checkpoint-staging-"))
            .collect();
        assert!(staging.is_empty(), "staging dir is removed");

        let pool = open_target(&hub).await;
        let identity: (String, String, String) =
            sqlx::query_as("SELECT supervisor_id, owner_principal, memory_name FROM supervisor_identity")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(identity, (SUP.to_owned(), "owner:u1".to_owned(), "ferryx-supervisor".to_owned()));
        let goal: (String, String) = sqlx::query_as("SELECT lane_key, run_state FROM supervisor_goals WHERE goal_id = ?")
            .bind(G1)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(goal, ("lane-a".to_owned(), "ready".to_owned()));
        assert_eq!(host_of(&pool, "lanes", "key", "lane-a").await, LOCAL_HOST_ID);
        pool.close().await;

        let repo = memory_repo(&fx.root);
        assert!(repo.ends_with("ferryx-supervisor-d08240e6/repo"));
        let repo = repo.to_str().unwrap().to_owned();
        assert_eq!(git_ok(&["-C", &repo, "rev-parse", "HEAD"]), fx.memory_head);
        assert_eq!(git_ok(&["-C", &repo, "config", "omo.agentid"]), "ferryx-supervisor-d08240e6");
        fs::remove_dir_all(&fx.root).unwrap();
    }

    #[tokio::test]
    async fn a_recorded_sync_remote_is_pulled_instead_of_a_bundle() {
        let fx = fixture("remote", &[("lane-a", G1, "local")], true).await;
        let files = read_archive(&fs::read(&fx.archive).unwrap()).unwrap();
        assert!(!files.contains_key(ARCHIVE_MEMORY_BUNDLE));
        let hub = fx.root.join("hub");
        let outcome = import(&fx.archive, &hub, "machine-a", &[], options(&fx.root, &hub, 1_800_000_000))
            .await
            .unwrap();
        let remote = fx.root.join("source").join("memory-remote");
        assert_eq!(outcome.memory, MemoryRestore::Remote(remote.to_str().unwrap().to_owned()));
        let repo = memory_repo(&fx.root);
        assert_eq!(git_ok(&["-C", repo.to_str().unwrap(), "rev-parse", "HEAD"]), fx.memory_head);
        fs::remove_dir_all(&fx.root).unwrap();
    }

    #[tokio::test]
    async fn local_and_paired_host_ids_map_in_both_directions() {
        let goals = [("lane-a", G1, "local"), ("lane-b", G2, "mac-b")];
        let fx = fixture("map", &goals, false).await;

        // Back on machine-a: its own rows become local, machine-b's become machine-a's pairing.
        let (root_a, hub_a) = (fx.root.join("a"), fx.root.join("hub-a"));
        let paired_a = [paired("paired-b", "machine-b")];
        let outcome = import(&fx.archive, &hub_a, "machine-a", &paired_a, options(&root_a, &hub_a, 1_800_000_000))
            .await
            .unwrap();
        assert!(outcome.unmapped_hosts.is_empty() && outcome.held_goals.is_empty());
        let pool = open_target(&hub_a).await;
        for (table, key_sql, key, expected) in [
            ("lanes", "key", "lane-a", "local"),
            ("lanes", "key", "lane-b", "paired-b"),
            ("supervisor_execution_bindings", "goal_id", G1, "local"),
            ("supervisor_execution_bindings", "goal_id", G2, "paired-b"),
            ("supervisor_project_hosts", "git_common_dir", "/repo/lane-a", "local"),
            ("supervisor_project_hosts", "git_common_dir", "/repo/lane-b", "paired-b"),
        ] {
            assert_eq!(host_of(&pool, table, key_sql, key).await, expected, "{table} {key}");
        }
        let cached: Vec<String> = sqlx::query_scalar("SELECT host_id FROM fleet_inventory_cache ORDER BY host_id")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(cached, ["local", "paired-b"]);
        pool.close().await;

        // On machine-b: 'local' of machine-a becomes b's pairing of a, and b's own rows local.
        let (root_b, hub_b) = (fx.root.join("b"), fx.root.join("hub-b"));
        let paired_b = [paired("paired-a", "machine-a")];
        import(&fx.archive, &hub_b, "machine-b", &paired_b, options(&root_b, &hub_b, 1_800_000_000))
            .await
            .unwrap();
        let pool = open_target(&hub_b).await;
        assert_eq!(host_of(&pool, "lanes", "key", "lane-a").await, "paired-a");
        assert_eq!(host_of(&pool, "lanes", "key", "lane-b").await, "local");
        assert_eq!(host_of(&pool, "supervisor_execution_bindings", "goal_id", G1).await, "paired-a");
        assert_eq!(host_of(&pool, "supervisor_execution_bindings", "goal_id", G2).await, "local");
        pool.close().await;
        fs::remove_dir_all(&fx.root).unwrap();
    }

    #[tokio::test]
    async fn an_unmapped_host_holds_its_goal_visibly_until_that_machine_is_paired() {
        let goals = [("lane-a", G1, "local"), ("lane-c", G2, "mac-c")];
        let fx = fixture("unmapped", &goals, false).await;
        let hub = fx.root.join("hub");
        let outcome = import(&fx.archive, &hub, "machine-a", &[], options(&fx.root, &hub, 1_800_000_000))
            .await
            .unwrap();
        assert_eq!(outcome.unmapped_hosts, ["machine:machine-c"]);
        assert_eq!(outcome.held_goals, [G2]);
        assert_eq!(outcome.released_dispatches, 1);
        let status: serde_json::Value =
            serde_json::from_slice(&fs::read(hub.join(IMPORT_STATUS_FILE)).unwrap()).unwrap();
        assert_eq!(status["unmapped_hosts"], serde_json::json!(["machine:machine-c"]));

        let pool = open_target(&hub).await;
        type Held = (String, Option<String>, Option<String>, Option<String>);
        let held: Held = sqlx::query_as(
            "SELECT run_state, state_detail, held_from, held_detail FROM supervisor_goals WHERE goal_id = ?",
        )
        .bind(G2)
        .fetch_one(&pool)
        .await
        .unwrap();
        let code = Some(REMOTE_HOST_UNMAPPED.to_owned());
        assert_eq!(held, ("paused".to_owned(), code.clone(), Some("ready".to_owned()), code));
        let dispatch: String = sqlx::query_scalar("SELECT state FROM supervisor_dispatches WHERE goal_id = ?")
            .bind(G2)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(dispatch, "released");
        // Never redirected: the binding still names the remote machine, not another pane.
        assert_eq!(host_of(&pool, "supervisor_execution_bindings", "goal_id", G2).await, "machine:machine-c");
        let mapped_goal: String = sqlx::query_scalar("SELECT run_state FROM supervisor_goals WHERE goal_id = ?")
            .bind(G1)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(mapped_goal, "ready");

        // Once machine-c is paired, the next mapping pass maps it and lifts the hold.
        let later = map_host_ids(&pool, "machine-a", &[paired("paired-c", "machine-c")], 1_800_000_100)
            .await
            .unwrap();
        assert!(later.unmapped_hosts.is_empty());
        assert_eq!(later.restored_goals, [G2]);
        let restored: (String, Option<String>) =
            sqlx::query_as("SELECT run_state, held_detail FROM supervisor_goals WHERE goal_id = ?")
                .bind(G2)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(restored, ("ready".to_owned(), None));
        assert_eq!(host_of(&pool, "lanes", "key", "lane-c").await, "paired-c");
        pool.close().await;
        fs::remove_dir_all(&fx.root).unwrap();
    }

    #[tokio::test]
    async fn a_target_holding_another_supervisor_is_refused_and_an_archived_one_is_fresh() {
        let fx = fixture("identity", &[("lane-a", G1, "local")], false).await;
        let hub = fx.root.join("hub");
        fs::create_dir_all(&hub).unwrap();
        let pool = crate::storage::connect(&hub.join(LIVE_DB)).await.unwrap();
        seed_identity(&pool, OTHER_SUP).await;
        pool.close().await;

        let error = import(&fx.archive, &hub, "machine-a", &[], options(&fx.root, &hub, 1_800_000_000))
            .await
            .unwrap_err();
        assert_eq!(error.code(), SUPERVISOR_IDENTITY_MISMATCH);
        assert!(!hub.join(INCOMING_DB).exists(), "nothing staged");
        assert!(!hub.join("env").exists(), "no secret installed");
        assert!(!profile_path(&fx.root).exists(), "no profile installed");
        assert!(!memory_repo(&fx.root).exists(), "no memory restored");

        // The same supervisor is accepted.
        let same = fx.root.join("hub-same");
        fs::create_dir_all(&same).unwrap();
        let pool = crate::storage::connect(&same.join(LIVE_DB)).await.unwrap();
        seed_identity(&pool, SUP).await;
        pool.close().await;
        import(&fx.archive, &same, "machine-a", &[], options(&fx.root.join("same"), &same, 1_800_000_000))
            .await
            .unwrap();

        // An archived target (a pre-lease copy exists) counts as fresh.
        fs::write(hub.join("hub.sqlite.pre-lease-1799999999"), b"").unwrap();
        import(&fx.archive, &hub, "machine-a", &[], options(&fx.root, &hub, 1_800_000_000))
            .await
            .unwrap();
        assert!(hub.join(INCOMING_DB).exists());
        fs::remove_dir_all(&fx.root).unwrap();
    }

    #[tokio::test]
    async fn an_archive_over_the_budget_is_refused() {
        let root = temp_root("too-large");
        let archive = root.join("big.tar.zst");
        let file = fs::File::create(&archive).unwrap();
        file.set_len(CHECKPOINT_MAX_COMPRESSED_BYTES + 1).unwrap();
        drop(file);
        let hub = root.join("hub");
        let error = import(&archive, &hub, "machine-a", &[], options(&root, &hub, 1_800_000_000))
            .await
            .unwrap_err();
        assert_eq!(error.code(), CHECKPOINT_TOO_LARGE);
        fs::remove_dir_all(&root).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn secrets_are_owner_only_after_import() {
        use std::os::unix::fs::PermissionsExt;
        let fx = fixture("secrets", &[("lane-a", G1, "local")], false).await;
        let hub = fx.root.join("hub");
        fs::create_dir_all(&hub).unwrap();
        fs::write(hub.join("env"), b"OLD=1\n").unwrap();
        let outcome = import(&fx.archive, &hub, "machine-a", &[], options(&fx.root, &hub, 1_800_000_000))
            .await
            .unwrap();
        assert_eq!(outcome.secrets, [hub.join("env")]);
        for path in [hub.join("env"), profile_path(&fx.root)] {
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "{}", path.display());
        }
        let env = crate::hub_env::parse_env_file(&fs::read_to_string(hub.join("env")).unwrap());
        assert_eq!(env.get("SLACK_BOT_TOKEN").map(String::as_str), Some("xoxb-secret"));
        assert_eq!(env.get("OLD").map(String::as_str), Some("1"), "local keys stay");
        assert!(!env.contains_key("FERRYX_HUB_DB"), "machine-local keys never travel");
        assert_eq!(fs::read(hub.join("env.pre-move-1800000000")).unwrap(), b"OLD=1\n");
        fs::remove_dir_all(&fx.root).unwrap();
    }

    /// Windows has no mode bits: the caller's writer applies the owner-only ACL. Import must
    /// create every secret through that callback, as a new file, and nowhere else.
    #[cfg(windows)]
    #[tokio::test]
    async fn secrets_are_created_through_the_private_writer_on_windows() {
        let fx = fixture("secrets-win", &[("lane-a", G1, "local")], false).await;
        let hub = fx.root.join("hub");
        let created = Arc::new(std::sync::Mutex::new(Vec::<PathBuf>::new()));
        let seen = Arc::clone(&created);
        let writer: PrivateFileWriter = Arc::new(move |path: &Path, bytes: &[u8]| {
            seen.lock().unwrap().push(path.to_owned());
            write_private_file(path, bytes)
        });
        let mut opts = options(&fx.root, &hub, 1_800_000_000);
        opts.write_private = writer;
        import(&fx.archive, &hub, "machine-a", &[], opts).await.unwrap();
        let profile_incoming = profile_path(&fx.root).with_file_name("omo.jsonc.incoming");
        assert_eq!(*created.lock().unwrap(), [hub.join("env.incoming"), profile_incoming]);
        let env = crate::hub_env::parse_env_file(&fs::read_to_string(hub.join("env")).unwrap());
        assert_eq!(env.get("SLACK_BOT_TOKEN").map(String::as_str), Some("xoxb-secret"));
        // The private creation path never overwrites: an existing file is an error.
        let error = write_private_file(&hub.join("env"), b"x").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        fs::remove_dir_all(&fx.root).unwrap();
    }

    #[tokio::test]
    async fn a_staged_import_swaps_only_at_the_next_open() {
        let fx = fixture("swap", &[("lane-a", G1, "local")], false).await;
        let hub = fx.root.join("hub");
        let live = hub.join(LIVE_DB);
        fs::create_dir_all(&hub).unwrap();
        let pool = crate::storage::connect(&live).await.unwrap();
        seed_identity(&pool, SUP).await;
        pool.close().await;
        let before = fs::read(&live).unwrap();

        import(&fx.archive, &hub, "machine-a", &[], options(&fx.root, &hub, 1_800_000_000))
            .await
            .unwrap();
        assert_eq!(fs::read(&live).unwrap(), before, "the live database is untouched by import");
        assert!(hub.join(INCOMING_DB).exists());
        assert!(!hub.join(INCOMING_TMP).exists());

        let pool = open_with_staged_swap(&live, 1_800_000_050).await.unwrap();
        let goals: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM supervisor_goals")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(goals, 1, "the imported database is live after the open");
        pool.close().await;
        assert!(!hub.join(INCOMING_DB).exists());
        let kept = hub.join("hub.sqlite.pre-move-1800000050");
        assert!(kept.exists(), "the old database is kept");
        assert_eq!(apply_staged_swap(&hub, 1_800_000_060).unwrap(), None, "nothing staged twice");
        fs::remove_dir_all(&fx.root).unwrap();
    }

    #[tokio::test]
    async fn a_backfill_sweep_after_the_round_trip_posts_no_reply() {
        let fx = fixture("dedupe", &[("lane-a", G1, "local")], false).await;
        let hub = fx.root.join("hub");
        import(&fx.archive, &hub, "machine-a", &[], options(&fx.root, &hub, 1_800_000_000))
            .await
            .unwrap();
        let pool = open_target(&hub).await;
        let cursor: String = sqlx::query_scalar(
            "SELECT last_ts FROM slack_cursors WHERE bot_id = 'B1' AND channel_id = 'C1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let wakes: i64 = sqlx::query_scalar("SELECT last_seq FROM supervisor_event_cursors WHERE consumer = 'wakes'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(wakes, 7);
        // The sweep replays every message the old machine answered. A reply goes out only for
        // a message past the cursor whose first-delivery insert wins (slack_turn's dedupe).
        let mut replies = 0;
        for ts in SEEN {
            if ts.parse::<f64>().unwrap() <= cursor.parse::<f64>().unwrap() {
                let first = sqlx::query("INSERT OR IGNORE INTO slack_inbound_seen(id, seen_at) VALUES(?, ?)")
                    .bind(format!("C1/{ts}"))
                    .bind(NOW)
                    .execute(&pool)
                    .await
                    .unwrap();
                replies += first.rows_affected();
            } else {
                replies += 1;
            }
        }
        assert_eq!(replies, 0);
        pool.close().await;
        fs::remove_dir_all(&fx.root).unwrap();
    }

    #[tokio::test]
    async fn an_archive_without_its_database_is_refused_and_nothing_is_staged() {
        let root = temp_root("no-db");
        let manifest = CheckpointManifest {
            format_version: CHECKPOINT_FORMAT_VERSION,
            created_at: NOW.to_owned(),
            source_machine_id: "machine-a".to_owned(),
            supervisor_id: Some(SUP.to_owned()),
            memory_agent: SUPERVISOR_MEMORY_AGENT.to_owned(),
            memory: MemorySource::Absent,
            env_keys: Vec::new(),
            excluded_tables: Vec::new(),
            host_map: BTreeMap::new(),
            has_profile: false,
        };
        let manifest = serde_json::to_vec(&manifest).unwrap();
        let archive = root.join("checkpoint.tar.zst");
        fs::write(&archive, build_archive(&[(ARCHIVE_MANIFEST, manifest.as_slice())]).unwrap()).unwrap();
        let hub = root.join("hub");
        let error = import(&archive, &hub, "machine-a", &[], options(&root, &hub, 1_800_000_000))
            .await
            .unwrap_err();
        assert_eq!(error.code(), "CHECKPOINT_MANIFEST_INVALID");
        assert!(!hub.join(INCOMING_DB).exists());
        fs::remove_dir_all(&root).unwrap();
    }
}
