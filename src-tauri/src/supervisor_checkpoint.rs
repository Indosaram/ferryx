//! Supervisor checkpoints between machines (plan todo 8): the local checkpoint store, the
//! promotion state a standby's lease client presents and status reads, and the move.
//!
//! A checkpoint is the archive [`ferryx_hub::checkpoint::export`] writes; its id is the sha256
//! of the compressed bytes. Standbys keep the newest [`KEEP_NEWEST`] under
//! `<data>/supervisor-checkpoint/<id>.tar.zst` (owner-only). The relay is the single writer of
//! the lease: a standby only ever asks, presenting its newest checkpoint id, and the relay's
//! checkpoint rule decides; a forced takeover is only ever an explicit person's request.

use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::daemon::supervisor_lease_role::LEASE_CHECKPOINT_MISMATCH;
use crate::remote::protocol::{SupervisorLeaseAck, SupervisorLeaseOutcome};

/// Directory under the remote data dir holding the checkpoints.
pub const CHECKPOINT_DIR: &str = "supervisor-checkpoint";
/// Archive suffix of a stored checkpoint.
pub const CHECKPOINT_SUFFIX: &str = ".tar.zst";
/// Checkpoints a machine keeps.
pub const KEEP_NEWEST: usize = 2;
/// Holder publish cadence.
pub const PUBLISH_EVERY: std::time::Duration = std::time::Duration::from_secs(300);
/// Response header naming the id of the checkpoint `GET /api/v1/supervisor/checkpoint` serves.
pub const CHECKPOINT_ID_HEADER: &str = "x-ferryx-checkpoint-id";
/// A move would lose changes: only an explicit `force` may take over.
pub const SUPERVISOR_FORCE_REQUIRED: &str = "SUPERVISOR_FORCE_REQUIRED";
const INDEX_FILE: &str = "index.json";

/// `<data>/supervisor-checkpoint`, `None` when the data dir cannot be resolved.
pub fn checkpoint_dir() -> Option<PathBuf> {
    crate::remote::auth::canonical_remote_dir().map(|dir| dir.join(CHECKPOINT_DIR))
}

/// The checkpoint id of an archive: lowercase sha256 hex of its bytes.
pub fn checkpoint_id_of(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn valid_id(id: &str) -> bool {
    id.len() == 64 && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// One stored checkpoint; `at` is when this machine stored it (unix seconds). `pulled` marks
/// a copy of another machine's checkpoint (as opposed to this machine's own export).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredCheckpoint {
    pub id: String,
    pub at: u64,
    #[serde(default)]
    pub pulled: bool,
}

/// The owner-only checkpoint directory, newest first in its index. Blocking.
#[derive(Debug, Clone)]
pub struct CheckpointStore {
    dir: PathBuf,
}

impl CheckpointStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn path_of(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}{CHECKPOINT_SUFFIX}"))
    }

    fn index(&self) -> Vec<StoredCheckpoint> {
        std::fs::read(self.dir.join(INDEX_FILE))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn newest(&self) -> Option<StoredCheckpoint> {
        self.index().into_iter().find(|entry| self.path_of(&entry.id).exists())
    }

    pub fn find(&self, id: &str) -> Option<StoredCheckpoint> {
        self.index().into_iter().find(|entry| entry.id == id && self.path_of(id).exists())
    }

    /// Stores `bytes` as checkpoint `id` (refused unless `id` is their sha256), records it as
    /// the newest at `at` and deletes all but the newest [`KEEP_NEWEST`].
    pub fn save(&self, id: &str, bytes: &[u8], at: u64, pulled: bool) -> std::io::Result<StoredCheckpoint> {
        if !valid_id(id) || checkpoint_id_of(bytes) != id {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "the checkpoint does not match its id",
            ));
        }
        std::fs::create_dir_all(&self.dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.dir, std::fs::Permissions::from_mode(0o700))?;
        }
        let path = self.path_of(id);
        if !path.exists() {
            let tmp = self.dir.join(format!(".{id}.{}.partial", uuid::Uuid::new_v4().simple()));
            ferryx_hub::checkpoint_import::write_private_file(&tmp, bytes)?;
            if let Err(error) = std::fs::rename(&tmp, &path) {
                let _ = std::fs::remove_file(&tmp);
                return Err(error);
            }
        }
        let entry = StoredCheckpoint { id: id.to_owned(), at, pulled };
        let mut index: Vec<StoredCheckpoint> = self.index().into_iter().filter(|e| e.id != id).collect();
        index.insert(0, entry.clone());
        for old in index.split_off(KEEP_NEWEST.min(index.len())) {
            let _ = std::fs::remove_file(self.path_of(&old.id));
        }
        crate::account::store::write_private_json(&self.dir.join(INDEX_FILE), &index)?;
        Ok(entry)
    }
}

/// The relay's last refusal of this machine's acquire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaseRefusal {
    pub code: String,
    /// The checkpoint the relay recorded for the lease.
    pub expected_checkpoint: Option<String>,
    pub holder_machine_id: Option<String>,
    pub holder_label: Option<String>,
}

/// Why this machine is not promoted; `code` is stable for status consumers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromotionBlock {
    /// `hub_stopped` is set here: the owner stopped the hub, so it never promotes itself.
    HubStopped,
    /// The lease records a checkpoint and this machine has none.
    CheckpointMissing { expected: Option<String> },
    /// This machine's newest checkpoint is not the one the lease records.
    CheckpointStale { have: String, expected: Option<String> },
    /// The holder's lease is still live (or in grace).
    HolderLive { holder: Option<String> },
    Refused { code: String },
}

impl PromotionBlock {
    pub fn code(&self) -> &str {
        match self {
            Self::HubStopped => "HUB_STOPPED",
            Self::CheckpointMissing { .. } => "CHECKPOINT_MISSING",
            Self::CheckpointStale { .. } => "CHECKPOINT_STALE",
            Self::HolderLive { .. } => "LEASE_HELD",
            Self::Refused { code } => code.as_str(),
        }
    }
}

impl std::fmt::Display for PromotionBlock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let none = || "none".to_owned();
        match self {
            Self::HubStopped => f.write_str("the hub is stopped on this machine, so it does not take over"),
            Self::CheckpointMissing { expected } => write!(
                f,
                "this machine has no checkpoint; the lease records {}",
                expected.clone().unwrap_or_else(none)
            ),
            Self::CheckpointStale { have, expected } => write!(
                f,
                "this machine's checkpoint {have} is stale; the lease records {}",
                expected.clone().unwrap_or_else(none)
            ),
            Self::HolderLive { holder } => {
                write!(f, "{} still holds the supervisor lease", holder.clone().unwrap_or_else(|| "another machine".into()))
            }
            Self::Refused { code } => write!(f, "the relay refused the takeover ({code})"),
        }
    }
}

/// The block reason for the given inputs; `None` when nothing blocks a promotion.
pub fn block_reason_for(hub_stopped: bool, local: Option<&str>, refusal: Option<&LeaseRefusal>) -> Option<PromotionBlock> {
    if hub_stopped {
        return Some(PromotionBlock::HubStopped);
    }
    let refusal = refusal?;
    let expected = refusal.expected_checkpoint.clone();
    Some(match refusal.code.as_str() {
        LEASE_CHECKPOINT_MISMATCH => match local {
            None => PromotionBlock::CheckpointMissing { expected },
            Some(have) => PromotionBlock::CheckpointStale { have: have.to_owned(), expected },
        },
        "LEASE_HELD" => PromotionBlock::HolderLive {
            holder: refusal.holder_label.clone().or_else(|| refusal.holder_machine_id.clone()),
        },
        code => PromotionBlock::Refused { code: code.to_owned() },
    })
}

/// What status shows about promotion and replication.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromotionStatus {
    pub block_code: Option<String>,
    pub block_reason: Option<String>,
    pub checkpoint_id: Option<String>,
    pub checkpoint_age_secs: Option<u64>,
}

#[derive(Debug, Default)]
struct Inner {
    local: Option<StoredCheckpoint>,
    published: Option<String>,
    reported: Option<String>,
    staged: Option<String>,
    pending_import: Option<String>,
    hub_stopped: bool,
    refusal: Option<LeaseRefusal>,
}

/// Promotion inputs shared by the lease client, the hub runtime and status.
#[derive(Debug, Default)]
pub struct CheckpointState {
    inner: parking_lot::Mutex<Inner>,
}

static SHARED: LazyLock<Arc<CheckpointState>> = LazyLock::new(Arc::default);

/// This daemon's state.
pub fn shared() -> Arc<CheckpointState> {
    Arc::clone(&*SHARED)
}

/// [`CheckpointState::block_reason`] of this daemon (what `hub status` shows).
pub fn promotion_block_reason() -> Option<PromotionBlock> {
    SHARED.block_reason()
}

/// [`CheckpointState::status`] of this daemon at `now` (unix seconds).
pub fn promotion_status(now: u64) -> PromotionStatus {
    SHARED.status(now)
}

impl CheckpointState {
    /// The newest checkpoint stored on this machine.
    pub fn local(&self) -> Option<StoredCheckpoint> {
        self.inner.lock().local.clone()
    }

    pub fn set_local(&self, local: Option<StoredCheckpoint>) {
        self.inner.lock().local = local;
    }

    pub fn hub_stopped(&self) -> bool {
        self.inner.lock().hub_stopped
    }

    /// The owner's `hub_stopped` here: a stopped machine never promotes itself.
    pub fn set_hub_stopped(&self, stopped: bool) {
        self.inner.lock().hub_stopped = stopped;
    }

    /// This machine's hub database now reflects checkpoint `id` (it exported or imported it).
    pub fn set_own(&self, id: &str) {
        let mut inner = self.inner.lock();
        inner.staged = Some(id.to_owned());
        if inner.published.as_deref() == Some(id) {
            return;
        }
        inner.published = None;
    }

    /// The holder exported `entry`: newest local, reported on the next renew.
    pub fn publish(&self, entry: StoredCheckpoint) {
        let mut inner = self.inner.lock();
        inner.staged = Some(entry.id.clone());
        inner.published = Some(entry.id.clone());
        inner.local = Some(entry);
    }

    /// The published checkpoint the relay has not acknowledged yet (sent with a renew).
    pub fn unreported(&self) -> Option<String> {
        let inner = self.inner.lock();
        inner.published.clone().filter(|id| inner.reported.as_deref() != Some(id.as_str()))
    }

    /// The newest checkpoint this holder published (sent with its release).
    pub fn published(&self) -> Option<String> {
        self.inner.lock().published.clone()
    }

    pub fn mark_reported(&self, id: &str) {
        self.inner.lock().reported = Some(id.to_owned());
    }

    /// The relay refused this machine's acquire (`None` clears the last refusal).
    pub fn note_refusal(&self, refusal: Option<LeaseRefusal>) {
        self.inner.lock().refusal = refusal;
    }

    /// A standby's lease query: held stays a refusal; a free lease is promotable only when it
    /// records no checkpoint or this machine's newest one.
    pub fn note_query(&self, ack: &SupervisorLeaseAck) {
        let mut inner = self.inner.lock();
        let local = inner.local.as_ref().map(|local| local.id.clone());
        let refusal = match ack.outcome {
            SupervisorLeaseOutcome::Refused => refusal_of(ack),
            SupervisorLeaseOutcome::Released => match ack.checkpoint_id.as_deref() {
                Some(expected) if Some(expected) != local.as_deref() => Some(LeaseRefusal {
                    code: LEASE_CHECKPOINT_MISMATCH.to_owned(),
                    expected_checkpoint: Some(expected.to_owned()),
                    holder_machine_id: ack.holder_machine_id.clone(),
                    holder_label: ack.holder_label.clone(),
                }),
                _ => None,
            },
            _ => None,
        };
        inner.refusal = refusal;
    }

    /// Auto-promotion: a standby asks for the lease only when the owner has not stopped the hub
    /// here and the last query found the lease free with a matching (or no) checkpoint.
    pub fn may_auto_acquire(&self) -> bool {
        let inner = self.inner.lock();
        !inner.hub_stopped && inner.refusal.is_none()
    }

    /// The relay granted an acquire recording `checkpoint`. A takeover (`was_holder` false)
    /// whose recorded checkpoint is this machine's newest, but not what its database already
    /// reflects, leaves that checkpoint to import before the hub runs
    /// ([`Self::take_pending_import`]).
    pub fn note_granted(&self, checkpoint: Option<&str>, was_holder: bool) {
        let mut inner = self.inner.lock();
        inner.refusal = None;
        let Some(checkpoint) = checkpoint else { return };
        let have = inner.local.as_ref().is_some_and(|local| local.id == checkpoint && local.pulled);
        if !was_holder && have && inner.staged.as_deref() != Some(checkpoint) {
            inner.pending_import = Some(checkpoint.to_owned());
        }
    }

    /// The checkpoint a promotion must import first, once.
    pub fn take_pending_import(&self) -> Option<String> {
        self.inner.lock().pending_import.take()
    }

    /// Whether this machine's hub database already reflects checkpoint `id`.
    pub fn reflects(&self, id: &str) -> bool {
        self.inner.lock().staged.as_deref() == Some(id)
    }

    pub fn block_reason(&self) -> Option<PromotionBlock> {
        let inner = self.inner.lock();
        block_reason_for(
            inner.hub_stopped,
            inner.local.as_ref().map(|local| local.id.as_str()),
            inner.refusal.as_ref(),
        )
    }

    pub fn status(&self, now: u64) -> PromotionStatus {
        let block = self.block_reason();
        let local = self.local();
        PromotionStatus {
            block_code: block.as_ref().map(|block| block.code().to_owned()),
            block_reason: block.as_ref().map(ToString::to_string),
            checkpoint_id: local.as_ref().map(|local| local.id.clone()),
            checkpoint_age_secs: local.map(|local| now.saturating_sub(local.at)),
        }
    }
}

/// The refusal an acquire ack carries, `None` for any other outcome.
pub fn refusal_of(ack: &SupervisorLeaseAck) -> Option<LeaseRefusal> {
    (ack.outcome == SupervisorLeaseOutcome::Refused).then(|| LeaseRefusal {
        code: ack.reason_code.clone().unwrap_or_else(|| "LEASE_REFUSED".to_owned()),
        expected_checkpoint: ack.checkpoint_id.clone(),
        holder_machine_id: ack.holder_machine_id.clone(),
        holder_label: ack.holder_label.clone(),
    })
}

/// A move refused or failed: the machine error envelope's status, code, message and details.
#[derive(Debug, Clone, PartialEq)]
pub struct MoveError {
    pub status: u16,
    pub code: String,
    pub message: String,
    pub details: serde_json::Map<String, serde_json::Value>,
}

/// The wire code of a move the current holder keeps (its lease is live).
pub const SUPERVISOR_STANDBY: &str = "SUPERVISOR_STANDBY";
/// [`MovePeer`] could not reach the holder over the paired channel.
pub const HOLDER_UNREACHABLE: &str = "HOLDER_UNREACHABLE";

impl MoveError {
    pub fn new(status: u16, code: &str, message: impl Into<String>) -> Self {
        Self { status, code: code.to_owned(), message: message.into(), details: serde_json::Map::new() }
    }

    fn standby(holder_machine_id: Option<String>, holder_label: Option<String>) -> Self {
        let mut error = Self::new(409, SUPERVISOR_STANDBY, "another machine runs the supervisor");
        error.details.insert("holderMachineId".into(), serde_json::json!(holder_machine_id));
        error.details.insert("holderLabel".into(), serde_json::json!(holder_label));
        error
    }

    fn force_required(holder_label: Option<String>, holder: &str, checkpoint_at: Option<u64>) -> Self {
        let since = checkpoint_at.map_or_else(|| "the last sync".to_owned(), |at| at.to_string());
        let mut error = Self::new(
            409,
            SUPERVISOR_FORCE_REQUIRED,
            format!(
                "changes since {since} on {} will be lost; confirm with force",
                holder_label.as_deref().unwrap_or(holder)
            ),
        );
        error.details.insert("holderLabel".into(), serde_json::json!(holder_label));
        error.details.insert("checkpointAt".into(), serde_json::json!(checkpoint_at));
        error
    }
}

/// The holder as the move target reaches it: over the paired end-to-end encrypted channel only.
#[async_trait::async_trait]
pub trait MovePeer: Send + Sync {
    /// `POST /api/v1/supervisor/release-to` on `holder`: it stops, publishes a final checkpoint
    /// and releases the lease reserved for `target`. Returns that checkpoint's id;
    /// [`HOLDER_UNREACHABLE`] when the holder cannot be reached.
    async fn release_to(&self, holder: &str, target: &str) -> Result<Option<String>, MoveError>;
    /// `GET /api/v1/supervisor/checkpoint` on `holder`: `(id, archive)` of its newest.
    async fn fetch(&self, holder: &str) -> Result<(String, Vec<u8>), MoveError>;
}

/// This machine's lease client as a move drives it.
#[async_trait::async_trait]
pub trait MoveLease: Send + Sync {
    /// The id the relay knows this machine by (`reserved_for` of a move).
    fn machine_id(&self) -> String;
    /// The relay's view of the lease; `None` when the relay cannot be asked.
    async fn query(&self) -> Option<SupervisorLeaseAck>;
    /// One acquire; true when this machine holds the lease afterwards.
    async fn acquire(&self, force: bool) -> bool;
}

/// Checkpoint import into this machine's hub (todo 7's staged import).
#[async_trait::async_trait]
pub trait MoveImport: Send + Sync {
    /// Stages `archive` for the next hub open. Refusals keep their code
    /// (`SUPERVISOR_IDENTITY_MISMATCH` when this hub has its own different supervisor).
    async fn import(&self, archive: &Path) -> Result<(), MoveError>;
    /// Drops a staged import that will not be used.
    async fn discard(&self);
}

/// Imports checkpoint `id` from the local store unless the hub already reflects it.
async fn import_local(
    importer: &dyn MoveImport,
    store: &CheckpointStore,
    state: &CheckpointState,
    id: &str,
) -> Result<bool, MoveError> {
    if state.reflects(id) {
        return Ok(false);
    }
    importer.import(&store.path_of(id)).await?;
    state.set_own(id);
    Ok(true)
}

async fn acquire_or_discard(
    lease: &dyn MoveLease,
    importer: &dyn MoveImport,
    state: &CheckpointState,
    imported: bool,
    force: bool,
) -> Result<bool, MoveError> {
    if lease.acquire(force).await {
        return Ok(true);
    }
    if imported {
        // The old holder may re-acquire its own row; this machine keeps its own database.
        importer.discard().await;
        state.inner.lock().staged = None;
    }
    let refusal = state.inner.lock().refusal.clone();
    Err(match refusal {
        Some(refusal) => MoveError::standby(refusal.holder_machine_id, refusal.holder_label),
        None => MoveError::new(503, "SUPERVISOR_UNAVAILABLE", "the relay did not grant the supervisor lease"),
    })
}

/// Moves the supervisor to this machine. Returns whether it moved (false: already holder).
///
/// Live holder: it releases to this machine with a final checkpoint, which is pulled,
/// imported, then acquired (the reservation grants `fence + 1` here). Holder unreachable:
/// only the relay's rule decides (expired + grace + this machine's checkpoint matching the
/// recorded one); otherwise only an explicit `force` takes over.
pub async fn move_here(
    lease: &dyn MoveLease,
    peer: &dyn MovePeer,
    importer: &dyn MoveImport,
    store: &CheckpointStore,
    state: &CheckpointState,
    force: bool,
    now: u64,
) -> Result<bool, MoveError> {
    let me = lease.machine_id();
    let Some(ack) = lease.query().await else {
        return Err(MoveError::new(503, "SUPERVISOR_UNAVAILABLE", "the relay cannot be asked for the supervisor lease"));
    };
    let holder = ack.holder_machine_id.clone().unwrap_or_default();
    match ack.outcome {
        SupervisorLeaseOutcome::Granted => Ok(false),
        SupervisorLeaseOutcome::Refused => {
            let final_checkpoint = match peer.release_to(&holder, &me).await {
                Ok(id) => id,
                // A live lease on an unreachable holder: the relay keeps it; force cannot help.
                Err(error) if error.code == HOLDER_UNREACHABLE => {
                    return Err(MoveError::standby(ack.holder_machine_id, ack.holder_label));
                }
                Err(error) => return Err(error),
            };
            let mut imported = false;
            if let Some(id) = final_checkpoint {
                if store.find(&id).is_none() {
                    let (served, bytes) = peer.fetch(&holder).await?;
                    if served != id {
                        return Err(MoveError::new(502, "CHECKPOINT_MISMATCH", "the holder served another checkpoint"));
                    }
                    let saved = save_blocking(store, &id, bytes, now).await?;
                    state.set_local(Some(saved));
                }
                imported = import_local(importer, store, state, &id).await?;
            }
            acquire_or_discard(lease, importer, state, imported, false).await
        }
        _ => {
            let local = state.local();
            let matching = match (&ack.checkpoint_id, &local) {
                (None, _) => true,
                (Some(recorded), Some(local)) => *recorded == local.id,
                (Some(_), None) => false,
            };
            if !matching {
                if !force {
                    return Err(MoveError::force_required(ack.holder_label, &holder, local.map(|l| l.at)));
                }
                return acquire_or_discard(lease, importer, state, false, true).await;
            }
            let mut imported = false;
            if let (Some(id), Some(local)) = (&ack.checkpoint_id, &local) {
                if local.pulled {
                    imported = import_local(importer, store, state, id).await?;
                }
            }
            acquire_or_discard(lease, importer, state, imported, false).await
        }
    }
}

/// [`CheckpointStore::save`] of a pulled checkpoint, on the blocking pool.
pub async fn save_blocking(
    store: &CheckpointStore,
    id: &str,
    bytes: Vec<u8>,
    now: u64,
) -> Result<StoredCheckpoint, MoveError> {
    let (store, id) = (store.clone(), id.to_owned());
    tokio::task::spawn_blocking(move || store.save(&id, &bytes, now, true))
        .await
        .map_err(|error| MoveError::new(500, "INTERNAL_ERROR", error.to_string()))?
        .map_err(|error| MoveError::new(502, "CHECKPOINT_INVALID", error.to_string()))
}

#[cfg(test)]
#[path = "supervisor_checkpoint_tests.rs"]
mod tests;
