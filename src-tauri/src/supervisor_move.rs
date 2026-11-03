//! Production wiring of [`crate::supervisor_checkpoint::move_here`]: the holder reached over
//! the paired end-to-end encrypted channel only, todo 7's staged import, and this daemon's
//! lease client.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::hub_runtime::holder_link::{self, AccountGrants, Holder, HolderLink, PairedHosts};
use crate::paired_host::client::ClientError;
use crate::paired_host::inventory::HostView;
use crate::paired_host::secure_http::HttpResponse;
use crate::paired_host::service::PairedHostService;
use crate::remote::state::RemoteGatewayState;
use crate::supervisor_checkpoint::{self as cp, MoveError};

/// The daemon's paired-host inventory, installed with the lease client (the phone's move
/// route has only the gateway state).
static HOSTS: std::sync::RwLock<Option<PairedHostService>> = std::sync::RwLock::new(None);

pub(crate) fn install_hosts(hosts: PairedHostService) {
    *HOSTS.write().unwrap_or_else(|e| e.into_inner()) = Some(hosts);
}

/// The installed inventory, `None` before the hub opened.
pub(crate) fn installed_hosts() -> Option<PairedHostService> {
    HOSTS.read().unwrap_or_else(|e| e.into_inner()).clone()
}

/// The installed inventory's paired hosts; empty before the hub opened or when it is unreadable.
pub(crate) async fn paired_host_views() -> Vec<crate::paired_host::inventory::HostView> {
    let hosts = HOSTS.read().unwrap_or_else(|e| e.into_inner()).clone();
    match hosts {
        Some(hosts) => hosts.list().await.unwrap_or_default(),
        None => Vec::new(),
    }
}

/// [`move_supervisor_here`] with the installed inventory.
pub async fn move_here_installed(
    state: &RemoteGatewayState,
    force: bool,
    replace_local: bool,
    by: &str,
) -> Result<bool, MoveError> {
    let hosts = HOSTS.read().unwrap_or_else(|e| e.into_inner()).clone();
    let hosts = hosts.ok_or_else(|| MoveError::new(503, "SUPERVISOR_UNAVAILABLE", "the supervisor hub is not open in this daemon"))?;
    move_supervisor_here(state, &hosts, force, replace_local, by).await
}

/// One request to the paired host on a machine over the shared end-to-end encrypted session:
/// [`crate::paired_host::client::supervisor_secure_request`] in production.
#[async_trait]
trait SecureChannel: Send + Sync {
    async fn request(
        &self,
        machine_id: &str,
        method: &str,
        path: &str,
        body: Option<&[u8]>,
        limit: usize,
    ) -> Result<HttpResponse, ClientError>;
}

#[async_trait]
impl SecureChannel for PairedHostService {
    async fn request(
        &self,
        machine_id: &str,
        method: &str,
        path: &str,
        body: Option<&[u8]>,
        limit: usize,
    ) -> Result<HttpResponse, ClientError> {
        crate::paired_host::client::supervisor_secure_request(self, machine_id, method, path, body, limit).await
    }
}

/// The holder over the paired encrypted channel. Before each request this machine's pairing to
/// the holder is ensured through the holder link ([`HolderLink::holder_host`]): a standby of
/// the same account without a grant on the holder redeems one, a failed attempt is not
/// repeated within its backoff.
struct PairedMovePeer<'a> {
    link: &'a HolderLink,
    hosts: &'a dyn PairedHosts,
    account: &'a dyn AccountGrants,
    channel: &'a dyn SecureChannel,
}

impl PairedMovePeer<'_> {
    /// HOLDER_UNREACHABLE when this machine is not (and cannot get) paired to `holder`.
    async fn ensure_paired(&self, holder: &str) -> Result<HostView, MoveError> {
        let unpaired = || peer_error(ClientError::local("HOST_UNAVAILABLE"));
        if holder.is_empty() {
            return Err(unpaired());
        }
        let holder = Holder { machine_id: Some(holder.to_owned()), label: None };
        self.link.holder_host(&holder, self.hosts, self.account).await.ok_or_else(unpaired)
    }

    /// One request to the holder once this machine is paired to it; HOLDER_UNREACHABLE when it
    /// is not (and cannot get) paired. The agent-control refusal (403 marked
    /// `details.refusal = agentControl`) means the holder refused this machine's grant (one
    /// paired before the account vouched for machines, e.g. by the desktop): one
    /// fresh account grant is redeemed ([`HolderLink::repair_refused`], bounded by its pairing
    /// backoff) and the request is retried once.
    async fn request(
        &self,
        holder: &str,
        method: &str,
        path: &str,
        body: Option<&[u8]>,
        limit: usize,
    ) -> Result<HttpResponse, MoveError> {
        let host = self.ensure_paired(holder).await?;
        let response = self.channel.request(holder, method, path, body, limit).await.map_err(peer_error)?;
        if !agent_control_refusal(&response) {
            return Ok(response);
        }
        let link_holder = Holder { machine_id: Some(holder.to_owned()), label: None };
        match self.link.repair_refused(&link_holder, self.hosts, self.account, &host).await {
            Some(_) => self.channel.request(holder, method, path, body, limit).await.map_err(peer_error),
            None => Ok(response),
        }
    }
}

/// The holder's agent-control access refusal (403 FORBIDDEN with the fixed marker,
/// [`crate::agent_control::policy::is_agent_control_refusal`]); any other answer is the holder's.
fn agent_control_refusal(response: &HttpResponse) -> bool {
    if response.status != 403 {
        return false;
    }
    let value: Value = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
    value["error"]["code"] == "FORBIDDEN"
        && value["error"]["details"]
            .as_object()
            .is_some_and(crate::agent_control::policy::is_agent_control_refusal)
}

fn peer_error(error: crate::paired_host::client::ClientError) -> MoveError {
    match error.machine_error {
        Some(machine) => MoveError::new(502, &machine.code, machine.message.clone()),
        None => MoveError::new(503, cp::HOLDER_UNREACHABLE, format!("the holder cannot be reached ({})", error.code)),
    }
}

fn peer_refusal(status: u16, body: &[u8]) -> MoveError {
    let value: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let code = value["error"]["code"].as_str().unwrap_or("HOLDER_REFUSED");
    let message = value["error"]["message"].as_str().unwrap_or("the holder refused the move");
    MoveError::new(status, code, message)
}

#[async_trait]
impl cp::MovePeer for PairedMovePeer<'_> {
    async fn release_to(&self, holder: &str, target: &str) -> Result<Option<String>, MoveError> {
        let body = serde_json::to_vec(&json!({ "targetMachineId": target })).unwrap_or_default();
        let response = self
            .request(holder, "POST", "/api/v1/supervisor/release-to", Some(body.as_slice()), 64 * 1024)
            .await?;
        if response.status != 200 {
            return Err(peer_refusal(response.status, &response.body));
        }
        let value: Value = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
        Ok(value["checkpointId"].as_str().map(str::to_owned))
    }

    async fn fetch(&self, holder: &str) -> Result<(String, Vec<u8>), MoveError> {
        let limit = usize::try_from(ferryx_hub::checkpoint::CHECKPOINT_MAX_COMPRESSED_BYTES).unwrap_or(usize::MAX);
        let response = self.request(holder, "GET", "/api/v1/supervisor/checkpoint", None, limit).await?;
        if response.status != 200 {
            return Err(peer_refusal(response.status, &response.body));
        }
        let id = response.header(cp::CHECKPOINT_ID_HEADER).map(str::to_owned).unwrap_or_default();
        Ok((id, response.body))
    }
}

/// Todo 7's staged import into this machine's hub directory. `replace_local` is the owner's
/// explicit `--replace-local`: only then is a hub holding another supervisor archived and
/// replaced; otherwise its `SUPERVISOR_IDENTITY_MISMATCH` refusal stands.
struct HubMoveImport {
    hub_dir: std::path::PathBuf,
    machine_id: String,
    paired: Vec<ferryx_hub::checkpoint::PairedHostMapping>,
    replace_local: bool,
}

impl HubMoveImport {
    /// Auto-promotion's importer: never `replace_local`, so a hub holding another supervisor
    /// keeps refusing the import (only an owner's `--replace-local` move replaces it).
    fn for_promotion(
        hub_dir: std::path::PathBuf,
        machine_id: String,
        paired: Vec<ferryx_hub::checkpoint::PairedHostMapping>,
    ) -> Self {
        Self { hub_dir, machine_id, paired, replace_local: false }
    }
}

#[async_trait]
impl cp::MoveImport for HubMoveImport {
    async fn import(&self, archive: &std::path::Path) -> Result<(), MoveError> {
        let home = ferryx_hub::hub_env::home_dir()
            .ok_or_else(|| MoveError::new(500, "HOME_UNRESOLVED", "neither HOME nor USERPROFILE names a home directory"))?;
        let options = || ferryx_hub::checkpoint_import::ImportOptions {
            memory_root: home.join(".omo").join("memory").join("agents"),
            env_file: ferryx_hub::hub_env::env_file_path().unwrap_or_else(|| home.join(".ferryx/hub/env")),
            profile_omo_jsonc: None,
            now_ts: crate::hub_runtime::unix_now() as i64,
            write_private: ferryx_hub::checkpoint_import::default_private_writer(),
        };
        stage_import(archive, &self.hub_dir, &self.machine_id, &self.paired, self.replace_local, options)
            .await
            .map_err(|error| MoveError::new(409, &error.code, error.message))
    }

    async fn discard(&self) {
        let incoming = self.hub_dir.join(ferryx_hub::checkpoint_import::INCOMING_DB);
        if let Err(error) = tokio::fs::remove_file(&incoming).await {
            tracing::warn!(%error, "the staged checkpoint import was not discarded");
        }
    }
}

/// A refused or failed import or archive: its stable code and message.
#[derive(Debug)]
struct ReplaceError {
    code: String,
    message: String,
}

/// Todo 7's staged import into `hub_dir`. When this hub holds another supervisor (the import
/// refuses `SUPERVISOR_IDENTITY_MISMATCH`), that refusal stands unless `replace_local` (the
/// owner's explicit `--replace-local`, plan todo 13): then the local database is first
/// archived as `hub.sqlite.pre-lease-<ts>` (its open goals stay adoptable) and the import
/// runs once more. A matching identity imports without an archive.
async fn stage_import(
    archive: &std::path::Path,
    hub_dir: &std::path::Path,
    machine_id: &str,
    paired: &[ferryx_hub::checkpoint::PairedHostMapping],
    replace_local: bool,
    options: impl Fn() -> ferryx_hub::checkpoint_import::ImportOptions,
) -> Result<(), ReplaceError> {
    use ferryx_hub::checkpoint_import::import;
    let failed = |error: ferryx_hub::checkpoint_import::ImportError| ReplaceError {
        code: error.code().to_owned(),
        message: error.to_string(),
    };
    let refused = match import(archive, hub_dir, machine_id, paired, options()).await {
        Ok(_) => return Ok(()),
        Err(error) if replace_local && error.code() == ferryx_hub::checkpoint::SUPERVISOR_IDENTITY_MISMATCH => error,
        Err(error) => return Err(failed(error)),
    };
    let dir = hub_dir.to_owned();
    let now = options().now_ts;
    let archived = tokio::task::spawn_blocking(move || ferryx_hub::adoption::archive_before_replace(&dir, now))
        .await
        .map_err(|error| ReplaceError { code: "INTERNAL_ERROR".into(), message: error.to_string() })?
        .map_err(|error| ReplaceError { code: "STORAGE".into(), message: error.to_string() })?;
    match archived {
        Some(path) => tracing::info!(archive = %path.display(), "the local supervisor was archived before its replacement"),
        // Already archived (or no database): the import still refuses only for another reason.
        None => tracing::debug!(error = %refused, "the local supervisor was archived before"),
    }
    import(archive, hub_dir, machine_id, paired, options()).await.map(|_| ()).map_err(failed)
}

/// The lease client as a move drives it.
struct HandleMoveLease {
    handle: crate::daemon::supervisor_lease_client::SupervisorLeaseHandle,
    machine_id: String,
}

#[async_trait]
impl cp::MoveLease for HandleMoveLease {
    fn machine_id(&self) -> String {
        self.machine_id.clone()
    }

    async fn query(&self) -> Option<crate::remote::protocol::SupervisorLeaseAck> {
        self.handle.query().await
    }

    async fn acquire(&self, force: bool) -> bool {
        self.handle.acquire(force).await;
        self.handle.is_holder_now()
    }
}

/// Moves the supervisor to this machine: `ferryx hub move-here`, the phone's
/// `POST /api/v1/supervisor/move` and the desktop's move button. `by` names who asked.
/// `replace_local` (the owner's explicit `--replace-local`) archives and replaces a local
/// supervisor with another identity; without it that import is refused.
pub async fn move_supervisor_here(
    state: &RemoteGatewayState,
    hosts: &PairedHostService,
    force: bool,
    replace_local: bool,
    by: &str,
) -> Result<bool, MoveError> {
    let unavailable = || MoveError::new(503, "SUPERVISOR_UNAVAILABLE", "the supervisor hub is not open in this daemon");
    let handle = crate::hub_runtime::lease_handle().ok_or_else(unavailable)?;
    let hub_dir = crate::hub_runtime::open_hub_dir().ok_or_else(unavailable)?;
    let dir = cp::checkpoint_dir()
        .ok_or_else(|| MoveError::new(503, "SUPERVISOR_UNAVAILABLE", "the Ferryx data directory cannot be resolved"))?;
    let machine_id = crate::hub_runtime::lease_machine_id().await;
    let paired = hosts
        .list()
        .await
        .map(|hosts| {
            hosts
                .into_iter()
                .map(|host| ferryx_hub::checkpoint::PairedHostMapping { host_id: host.host_id, machine_id: host.machine_id })
                .collect()
        })
        .unwrap_or_default();
    let lease = HandleMoveLease { handle, machine_id: machine_id.clone() };
    let importer = HubMoveImport { hub_dir, machine_id, paired, replace_local };
    let paired_hosts = holder_link::ServicePairedHosts(hosts.clone());
    let peer = PairedMovePeer {
        link: &holder_link::LINK,
        hosts: &paired_hosts,
        account: &holder_link::AccountService,
        channel: hosts,
    };
    let store = cp::CheckpointStore::new(dir);
    let moved = cp::move_here(&lease, &peer, &importer, &store, &cp::shared(), force, crate::hub_runtime::unix_now()).await?;
    tracing::info!(by, moved, force, replace_local, "supervisor move requested");
    crate::remote::server::agent_control_service::publish_supervisor_changed(state, json!({ "hub": "moved" }));
    Ok(moved)
}

/// Auto-promotion's import: the takeover the relay granted against this machine's newest
/// pulled checkpoint is staged before the hub reopens. A failed import is logged; the hub then
/// reopens on its own database, as the import never touched it. Promotion never replaces a
/// local supervisor with another identity (only an owner's `--replace-local` move does).
pub async fn import_pending(hub_dir: &std::path::Path) {
    let state = cp::shared();
    let Some(id) = state.take_pending_import() else { return };
    let Some(dir) = cp::checkpoint_dir() else { return };
    let hosts = HOSTS.read().unwrap_or_else(|e| e.into_inner()).clone();
    let paired = match hosts {
        Some(hosts) => hosts
            .list()
            .await
            .map(|hosts| {
                hosts
                    .into_iter()
                    .map(|host| ferryx_hub::checkpoint::PairedHostMapping { host_id: host.host_id, machine_id: host.machine_id })
                    .collect()
            })
            .unwrap_or_default(),
        None => Vec::new(),
    };
    let importer =
        HubMoveImport::for_promotion(hub_dir.to_owned(), crate::hub_runtime::lease_machine_id().await, paired);
    if let Err(error) = import_checkpoint(&state, &cp::CheckpointStore::new(dir), &importer, &id).await {
        tracing::warn!(code = %error.code, "promotion did not import the synced checkpoint: {}", error.message);
    }
}

/// Stages stored checkpoint `id` through `importer`; on success the hub reflects it.
async fn import_checkpoint(
    state: &cp::CheckpointState,
    store: &cp::CheckpointStore,
    importer: &dyn cp::MoveImport,
    id: &str,
) -> Result<(), MoveError> {
    cp::MoveImport::import(importer, &store.path_of(id)).await?;
    state.set_own(id);
    tracing::info!(checkpoint = %id, "promotion imported the synced checkpoint");
    Ok(())
}

/// Pairs this machine to `holder` through the holder link (an account grant when it has none,
/// at most one attempt per [`holder_link::PAIRING_RETRY`]); a no-op before the hub opened.
pub(crate) async fn pair_to_holder(holder: Holder) {
    let hosts = HOSTS.read().unwrap_or_else(|e| e.into_inner()).clone();
    let Some(hosts) = hosts else { return };
    let paired_hosts = holder_link::ServicePairedHosts(hosts);
    let _ = holder_link::LINK.holder_host(&holder, &paired_hosts, &holder_link::AccountService).await;
}

/// A standby's replication step: pairs to the holder its lease names (an account grant when
/// this machine has none), then pulls the holder's newest checkpoint over the paired encrypted
/// channel when it is not stored here yet. True when a checkpoint of the holder is stored here
/// after it. One pull at a time ([`one_pull_at_a_time`]): the first-standby retry and the
/// publisher's periodic pull never archive this hub concurrently.
pub async fn pull_from_holder() -> bool {
    one_pull_at_a_time(pull_from_holder_now()).await
}

/// Serializes the standby pulls of this process.
static PULL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Runs `pull` once no other standby pull of this process runs.
async fn one_pull_at_a_time<F: std::future::Future>(pull: F) -> F::Output {
    let _serial = PULL.lock().await;
    pull.await
}

async fn pull_from_holder_now() -> bool {
    let Some(handle) = crate::hub_runtime::lease_handle() else { return false };
    let hosts = HOSTS.read().unwrap_or_else(|e| e.into_inner()).clone();
    let (Some(hosts), Some(dir)) = (hosts, cp::checkpoint_dir()) else { return false };
    let Some(ack) = handle.query().await else { return false };
    let store = cp::CheckpointStore::new(dir);
    let paired_hosts = holder_link::ServicePairedHosts(hosts.clone());
    let peer = PairedMovePeer {
        link: &holder_link::LINK,
        hosts: &paired_hosts,
        account: &holder_link::AccountService,
        channel: &hosts,
    };
    let Some(stored) = pull(&ack, &store, &peer, &cp::shared(), crate::hub_runtime::unix_now()).await else {
        return false;
    };
    learn_holder_identity(&store, &stored).await;
    true
}

/// The checkpoint a held lease (`ack`) records, stored here: found, or fetched from the holder
/// through `peer` and saved (then this machine's newest in `state`). A lease held by another
/// machine first pairs this machine to that holder, whether or not it records a checkpoint
/// yet, so a standby pairs as soon as it learns the holder. `None` when the lease is not held,
/// pairing failed, the lease records no checkpoint, or the pull failed.
async fn pull(
    ack: &crate::remote::protocol::SupervisorLeaseAck,
    store: &cp::CheckpointStore,
    peer: &PairedMovePeer<'_>,
    state: &cp::CheckpointState,
    now: u64,
) -> Option<String> {
    if ack.outcome != crate::remote::protocol::SupervisorLeaseOutcome::Refused {
        return None;
    }
    let holder = ack.holder_machine_id.as_deref()?;
    if let Err(error) = peer.ensure_paired(holder).await {
        tracing::debug!(code = %error.code, "not paired to the supervisor holder: {}", error.message);
        return None;
    }
    let recorded = ack.checkpoint_id.clone()?;
    let (lookup, id) = (store.clone(), recorded.clone());
    if tokio::task::spawn_blocking(move || lookup.find(&id).is_some()).await.unwrap_or(false) {
        return Some(recorded);
    }
    match cp::MovePeer::fetch(peer, holder).await {
        Ok((id, bytes)) => match cp::save_blocking(store, &id, bytes, now).await {
            Ok(saved) => {
                state.set_local(Some(saved));
                Some(id)
            }
            Err(error) => {
                tracing::warn!(code = %error.code, "pulled supervisor checkpoint not stored: {}", error.message);
                None
            }
        },
        Err(error) => {
            tracing::debug!(code = %error.code, "supervisor checkpoint not pulled: {}", error.message);
            None
        }
    }
}

/// The supervisor id of the holder, from the newest holder checkpoint this standby stored;
/// `None` until one was read.
static HOLDER_SUPERVISOR_ID: std::sync::RwLock<Option<String>> = std::sync::RwLock::new(None);

/// The holder's supervisor id as this standby last learned it from the holder's checkpoint;
/// before any pull since this daemon started, the one the newest pulled checkpoint stored here
/// names. `None` when no holder checkpoint was read.
pub(crate) async fn known_holder_supervisor_id() -> Option<String> {
    if let Some(known) = HOLDER_SUPERVISOR_ID.read().unwrap_or_else(|e| e.into_inner()).clone() {
        return Some(known);
    }
    let dir = cp::checkpoint_dir()?;
    tokio::task::spawn_blocking(move || {
        let store = cp::CheckpointStore::new(dir);
        let newest = store.newest().filter(|entry| entry.pulled)?;
        ferryx_hub::adoption::checkpoint_supervisor_id(&store.path_of(&newest.id)).ok().flatten()
    })
    .await
    .ok()
    .flatten()
}

/// Reads the supervisor id stored checkpoint `id` names (the holder's identity), records it
/// and archives this hub when it holds another identity (plan todo 13). A checkpoint without
/// an identity, or one that cannot be read, leaves the holder's identity unknown: nothing is
/// archived.
async fn learn_holder_identity(store: &cp::CheckpointStore, id: &str) {
    let path = store.path_of(id);
    let read = tokio::task::spawn_blocking(move || ferryx_hub::adoption::checkpoint_supervisor_id(&path)).await;
    let supervisor_id = match read {
        Ok(Ok(Some(supervisor_id))) => supervisor_id,
        Ok(Ok(None)) => return,
        Ok(Err(error)) => {
            tracing::warn!(%error, "the holder's supervisor id could not be read from its checkpoint");
            return;
        }
        Err(error) => {
            tracing::warn!(%error, "the holder checkpoint read task failed");
            return;
        }
    };
    *HOLDER_SUPERVISOR_ID.write().unwrap_or_else(|e| e.into_inner()) = Some(supervisor_id.clone());
    crate::hub_runtime::archive_for_holder(&supervisor_id).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paired_host::client::ClientError;
    use crate::supervisor_checkpoint::{CheckpointState, CheckpointStore, MoveImport};
    use std::path::{Path, PathBuf};

    const NOW: i64 = 1_800_000_000;

    /// A hub database in `dir` whose supervisor belongs to `owner` (a fresh supervisor id).
    async fn hub_with_owner(dir: &Path, owner: &str) -> ferryx_hub::goals::SqlitePool {
        std::fs::create_dir_all(dir).unwrap();
        let pool = ferryx_hub::storage::connect(&dir.join("hub.sqlite")).await.unwrap();
        ferryx_hub::supervisor::provision_owner(&pool, owner, "ferryx", "github.com/example/ferryx", "host-a", "/repo/.git")
            .await
            .unwrap();
        pool
    }

    /// Machine A's hub (same owner, its own supervisor id) exported to a checkpoint archive.
    async fn exported_checkpoint(root: &Path) -> PathBuf {
        let source = root.join("source");
        let pool = hub_with_owner(&source, "owner:u1").await;
        let exported = ferryx_hub::checkpoint::export(
            &pool,
            &ferryx_hub::checkpoint::ExportRequest::new(
                source.clone(),
                "machine-a".into(),
                root.join("checkpoint.tar.zst"),
                ferryx_hub::checkpoint_import::default_private_writer(),
            ),
        )
        .await
        .unwrap();
        pool.close().await;
        exported.path
    }

    fn import_options(root: &Path, target: &Path) -> impl Fn() -> ferryx_hub::checkpoint_import::ImportOptions {
        let (memory_root, env_file) = (root.join("memory-agents"), target.join("env"));
        move || ferryx_hub::checkpoint_import::ImportOptions {
            memory_root: memory_root.clone(),
            env_file: env_file.clone(),
            profile_omo_jsonc: None,
            now_ts: NOW,
            write_private: ferryx_hub::checkpoint_import::default_private_writer(),
        }
    }

    /// The main archive files in `dir` (an archive's `-wal` copy is not another archive).
    fn archives(dir: &Path) -> usize {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with(ferryx_hub::adoption::ARCHIVE_PREFIX) && !name.ends_with("-wal"))
            .count()
    }

    /// The owner's `--replace-local`: a local supervisor that has another identity is archived
    /// before the import (todo 13's `archive_before_replace`), then the checkpoint is staged.
    #[tokio::test]
    async fn a_replace_local_move_onto_another_local_supervisor_archives_it_before_the_import() {
        let root = tempfile::tempdir().unwrap();
        let exported = exported_checkpoint(root.path()).await;
        let target = root.path().join("target");
        hub_with_owner(&target, "owner:u1").await.close().await;

        stage_import(&exported, &target, "machine-b", &[], true, import_options(root.path(), &target)).await.unwrap();

        assert!(target.join(format!("{}{NOW}", ferryx_hub::adoption::ARCHIVE_PREFIX)).is_file(), "archived first");
        assert!(target.join(ferryx_hub::checkpoint_import::INCOMING_DB).exists(), "the checkpoint is staged");
        assert_eq!(archives(&target), 1);
    }

    /// Without `--replace-local` todo 7's identity refusal stands: nothing archived or staged.
    #[tokio::test]
    async fn a_move_onto_another_local_supervisor_without_replace_local_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let exported = exported_checkpoint(root.path()).await;
        let target = root.path().join("target");
        hub_with_owner(&target, "owner:u1").await.close().await;

        let error = stage_import(&exported, &target, "machine-b", &[], false, import_options(root.path(), &target))
            .await
            .unwrap_err();

        assert_eq!(error.code, ferryx_hub::checkpoint::SUPERVISOR_IDENTITY_MISMATCH);
        assert_eq!(archives(&target), 0, "nothing archived");
        assert!(!target.join(ferryx_hub::checkpoint_import::INCOMING_DB).exists(), "nothing staged");
    }

    /// Machine A's hub exported and stored as a pulled checkpoint; returns its id.
    async fn stored_checkpoint(root: &Path, store: &CheckpointStore) -> String {
        let source = root.join("source");
        let pool = hub_with_owner(&source, "slack:T1:U-owner").await;
        let request = ferryx_hub::checkpoint::ExportRequest::new(
            source,
            "mach-a".to_owned(),
            root.join("export.tar.zst"),
            ferryx_hub::checkpoint_import::default_private_writer(),
        );
        let exported = ferryx_hub::checkpoint::export(&pool, &request).await.unwrap();
        pool.close().await;
        let bytes = std::fs::read(&exported.path).unwrap();
        store.save(&exported.checkpoint_id, &bytes, 1_000, true).unwrap().id
    }

    fn importer(hub_dir: PathBuf) -> HubMoveImport {
        HubMoveImport { hub_dir, machine_id: "mach-b".to_owned(), paired: Vec::new(), replace_local: false }
    }

    #[tokio::test]
    async fn a_promotion_import_stages_the_synced_checkpoint_in_the_hub_dir() {
        let root = tempfile::tempdir().unwrap();
        let store = CheckpointStore::new(root.path().join(cp::CHECKPOINT_DIR));
        let id = stored_checkpoint(root.path(), &store).await;
        let hub_dir = root.path().join("hub");
        std::fs::create_dir_all(&hub_dir).unwrap();
        let (state, importer) = (CheckpointState::default(), importer(hub_dir.clone()));

        assert_eq!(import_checkpoint(&state, &store, &importer, &id).await, Ok(()));

        let incoming = hub_dir.join(ferryx_hub::checkpoint_import::INCOMING_DB);
        assert!(incoming.exists(), "the checkpoint is staged for the next hub open");
        assert!(state.reflects(&id));
        importer.discard().await;
        assert!(!incoming.exists(), "a discarded import leaves nothing staged");
    }

    #[tokio::test]
    async fn a_promotion_import_into_a_hub_with_another_supervisor_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let store = CheckpointStore::new(root.path().join(cp::CHECKPOINT_DIR));
        let id = stored_checkpoint(root.path(), &store).await;
        let hub_dir = root.path().join("hub");
        hub_with_owner(&hub_dir, "slack:T2:U-other").await.close().await;
        let (state, importer) = (CheckpointState::default(), importer(hub_dir.clone()));

        let error = import_checkpoint(&state, &store, &importer, &id).await.unwrap_err();

        assert_eq!((error.status, error.code.as_str()), (409, ferryx_hub::checkpoint::SUPERVISOR_IDENTITY_MISMATCH));
        assert!(!hub_dir.join(ferryx_hub::checkpoint_import::INCOMING_DB).exists(), "nothing staged");
        assert!(!state.reflects(&id));
    }

    /// Promotion's own importer never replaces the local supervisor: a hub with another
    /// identity is neither archived nor staged over.
    #[tokio::test]
    async fn the_promotion_importer_never_replaces_a_local_supervisor() {
        let root = tempfile::tempdir().unwrap();
        let store = CheckpointStore::new(root.path().join(cp::CHECKPOINT_DIR));
        let id = stored_checkpoint(root.path(), &store).await;
        let hub_dir = root.path().join("hub");
        hub_with_owner(&hub_dir, "slack:T2:U-other").await.close().await;
        let importer = HubMoveImport::for_promotion(hub_dir.clone(), "mach-b".to_owned(), Vec::new());
        assert!(!importer.replace_local);

        let error = import_checkpoint(&CheckpointState::default(), &store, &importer, &id).await.unwrap_err();

        assert_eq!(error.code, ferryx_hub::checkpoint::SUPERVISOR_IDENTITY_MISMATCH);
        assert_eq!(archives(&hub_dir), 0, "the local supervisor was not archived");
        assert!(!hub_dir.join(ferryx_hub::checkpoint_import::INCOMING_DB).exists(), "nothing staged");
    }

    #[test]
    fn the_paired_channel_maps_an_unreachable_holder_and_keeps_refusal_codes() {
        let unreachable = ClientError { code: "HOST_UNAVAILABLE".into(), machine_error: None, request_id: None, ambiguous: false };
        assert_eq!(peer_error(unreachable).code, cp::HOLDER_UNREACHABLE);
        let refused = ClientError {
            code: "SUPERVISOR_STANDBY".into(),
            machine_error: Some(Box::new(crate::remote::machine_protocol::MachineError {
                code: cp::SUPERVISOR_STANDBY.into(),
                message: "another machine runs the supervisor".into(),
                retryable: false,
                request_id: "r1".into(),
                details: Default::default(),
            })),
            request_id: None,
            ambiguous: false,
        };
        let error = peer_error(refused);
        assert_eq!((error.status, error.code.as_str()), (502, cp::SUPERVISOR_STANDBY));

        let body = br#"{"error":{"code":"SUPERVISOR_STANDBY","message":"held"}}"#;
        let error = peer_refusal(409, body);
        assert_eq!((error.status, error.code.as_str(), error.message.as_str()), (409, cp::SUPERVISOR_STANDBY, "held"));
        assert_eq!(peer_refusal(500, b"not json").code, "HOLDER_REFUSED");
    }

    use crate::hub_runtime::holder_link::tests::{FakeAccount, FakeHosts};
    use crate::remote::protocol::{SupervisorLeaseAck, SupervisorLeaseOutcome};

    /// The holder's encrypted channel as `supervisor_secure_request` behaves: only a machine
    /// with a machine-scope paired host in `hosts` is reached (else HOST_UNAVAILABLE); it
    /// serves `archive` as its newest checkpoint. Requests are recorded. While `forbid` is
    /// above zero the holder answers 403 FORBIDDEN (a grant without the account's marker).
    struct FakeChannel<'a> {
        hosts: &'a FakeHosts,
        archive: Vec<u8>,
        requests: parking_lot::Mutex<Vec<String>>,
        forbid: std::sync::atomic::AtomicUsize,
    }

    #[async_trait]
    impl SecureChannel for FakeChannel<'_> {
        async fn request(
            &self,
            machine_id: &str,
            method: &str,
            path: &str,
            _body: Option<&[u8]>,
            _limit: usize,
        ) -> Result<HttpResponse, ClientError> {
            let paired = self.hosts.hosts.lock().iter().any(|host| {
                host.machine_id == machine_id
                    && host.grant_scope == crate::paired_host::inventory::GrantScope::Machine
            });
            if !paired {
                return Err(ClientError::local("HOST_UNAVAILABLE"));
            }
            self.requests.lock().push(format!("{method} {path}"));
            if self.forbid.load(std::sync::atomic::Ordering::SeqCst) > 0 {
                self.forbid.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                return Ok(HttpResponse {
                    status: 403,
                    headers: Vec::new(),
                    body: br#"{"error":{"code":"FORBIDDEN","message":"agent control is off","details":{"refusal":"agentControl"}}}"#.to_vec(),
                });
            }
            Ok(HttpResponse {
                status: 200,
                headers: vec![(cp::CHECKPOINT_ID_HEADER.to_owned(), cp::checkpoint_id_of(&self.archive))],
                body: self.archive.clone(),
            })
        }
    }

    /// The relay's answer to a standby's query: held by `mach-b`, recording `checkpoint`.
    fn held_by_b(checkpoint: &str) -> SupervisorLeaseAck {
        SupervisorLeaseAck {
            request_id: "q".into(),
            outcome: SupervisorLeaseOutcome::Refused,
            fence: Some(3),
            holder_machine_id: Some("mach-b".into()),
            holder_label: Some("Laptop B".into()),
            expires_in_secs: None,
            previous_expires_in_secs: None,
            checkpoint_id: Some(checkpoint.to_owned()),
            account_machine_count: Some(2),
            reason_code: Some("LEASE_HELD".into()),
            lease_capable_machines: None,
        }
    }

    /// Plan task 5: a standby with no paired host for the holder its lease ack names redeems
    /// an account grant on it, then pulls and stores the holder's checkpoint.
    #[tokio::test]
    async fn a_standby_unpaired_to_the_holder_redeems_a_grant_and_then_pulls_its_checkpoint() {
        let root = tempfile::tempdir().unwrap();
        let store = CheckpointStore::new(root.path().join(cp::CHECKPOINT_DIR));
        let (link, hosts, state) = (HolderLink::new(), FakeHosts::default(), CheckpointState::default());
        let account = FakeAccount { session: true, grants: parking_lot::Mutex::new(Vec::new()) };
        let channel = FakeChannel { hosts: &hosts, archive: b"holder checkpoint".to_vec(), requests: Default::default(), forbid: Default::default() };
        let id = cp::checkpoint_id_of(&channel.archive);
        let peer = PairedMovePeer { link: &link, hosts: &hosts, account: &account, channel: &channel };

        let pulled = pull(&held_by_b(&id), &store, &peer, &state, 1_000).await;

        assert_eq!(pulled.as_deref(), Some(id.as_str()));
        assert_eq!(*account.grants.lock(), vec!["mach-b".to_owned()], "one account grant on the holder");
        assert_eq!(*hosts.redeemed.lock(), vec!["mach-b".to_owned()]);
        assert_eq!(*channel.requests.lock(), vec!["GET /api/v1/supervisor/checkpoint".to_owned()]);
        assert!(store.find(&id).is_some_and(|stored| stored.pulled), "the holder's checkpoint is stored here");
        assert_eq!(state.local().map(|local| local.id), Some(id));
        assert_eq!(link.pairing_state(), "paired");
    }

    /// A refused redeem leaves "not paired to holder" and is not attempted again inside the
    /// pairing backoff; once it has passed, the next pull tries again.
    #[tokio::test(start_paused = true)]
    async fn a_failed_redeem_is_not_retried_inside_the_pairing_backoff() {
        let root = tempfile::tempdir().unwrap();
        let store = CheckpointStore::new(root.path().join(cp::CHECKPOINT_DIR));
        let (link, state) = (HolderLink::new(), CheckpointState::default());
        let hosts = FakeHosts { refuse_redeem: true, ..FakeHosts::default() };
        let account = FakeAccount { session: true, grants: parking_lot::Mutex::new(Vec::new()) };
        let channel = FakeChannel { hosts: &hosts, archive: b"holder checkpoint".to_vec(), requests: Default::default(), forbid: Default::default() };
        let ack = held_by_b(&cp::checkpoint_id_of(&channel.archive));
        let peer = PairedMovePeer { link: &link, hosts: &hosts, account: &account, channel: &channel };

        assert_eq!(pull(&ack, &store, &peer, &state, 1_000).await, None);
        assert_eq!(hosts.redeemed.lock().len(), 1);
        assert_eq!(link.pairing_state(), "not paired to holder");

        tokio::time::advance(holder_link::PAIRING_RETRY - std::time::Duration::from_secs(1)).await;
        assert_eq!(pull(&ack, &store, &peer, &state, 1_001).await, None);
        assert_eq!(account.grants.lock().len(), 1, "no second grant inside the backoff");
        assert_eq!(hosts.redeemed.lock().len(), 1, "no second redeem inside the backoff");
        assert_eq!(link.pairing_state(), "not paired to holder");

        tokio::time::advance(std::time::Duration::from_secs(1)).await;
        assert_eq!(pull(&ack, &store, &peer, &state, 1_002).await, None);
        assert_eq!(hosts.redeemed.lock().len(), 2, "tried again once the backoff passed");
        assert!(channel.requests.lock().is_empty(), "nothing was requested from an unpaired holder");
        assert!(state.local().is_none());
    }

    /// Q6: a holder that has not published a checkpoint yet is still paired to at once; only
    /// the pull waits for a recorded checkpoint.
    #[tokio::test]
    async fn a_lease_held_without_a_checkpoint_still_pairs_the_standby_to_the_holder() {
        let root = tempfile::tempdir().unwrap();
        let store = CheckpointStore::new(root.path().join(cp::CHECKPOINT_DIR));
        let (link, hosts, state) = (HolderLink::new(), FakeHosts::default(), CheckpointState::default());
        let account = FakeAccount { session: true, grants: parking_lot::Mutex::new(Vec::new()) };
        let channel = FakeChannel { hosts: &hosts, archive: b"holder checkpoint".to_vec(), requests: Default::default(), forbid: Default::default() };
        let peer = PairedMovePeer { link: &link, hosts: &hosts, account: &account, channel: &channel };
        let ack = SupervisorLeaseAck { checkpoint_id: None, ..held_by_b("unused") };

        assert_eq!(pull(&ack, &store, &peer, &state, 1_000).await, None, "nothing to pull yet");

        assert_eq!(*account.grants.lock(), vec!["mach-b".to_owned()], "one account grant on the holder");
        assert_eq!(*hosts.redeemed.lock(), vec!["mach-b".to_owned()]);
        assert_eq!(link.pairing_state(), "paired");
        assert!(channel.requests.lock().is_empty(), "no checkpoint was requested");
        assert!(state.local().is_none());
    }

    /// MAJOR-A: a standby already paired to the holder by the desktop (a grant without the
    /// account's machine marker) gets 403 on the checkpoint pull while agent control is off.
    /// It redeems exactly one fresh account grant and the pull succeeds on the retry.
    #[tokio::test]
    async fn a_forbidden_checkpoint_pull_redeems_one_fresh_grant_and_succeeds_on_the_retry() {
        let root = tempfile::tempdir().unwrap();
        let store = CheckpointStore::new(root.path().join(cp::CHECKPOINT_DIR));
        let (link, hosts, state) = (HolderLink::new(), FakeHosts::default(), CheckpointState::default());
        hosts.hosts.lock().push(crate::hub_runtime::holder_link::tests::host("mach-b", crate::paired_host::inventory::AuthStatus::Paired));
        let account = FakeAccount { session: true, grants: parking_lot::Mutex::new(Vec::new()) };
        let channel = FakeChannel {
            hosts: &hosts,
            archive: b"holder checkpoint".to_vec(),
            requests: Default::default(),
            forbid: std::sync::atomic::AtomicUsize::new(1),
        };
        let id = cp::checkpoint_id_of(&channel.archive);
        let peer = PairedMovePeer { link: &link, hosts: &hosts, account: &account, channel: &channel };

        let pulled = pull(&held_by_b(&id), &store, &peer, &state, 1_000).await;

        assert_eq!(pulled.as_deref(), Some(id.as_str()));
        assert_eq!(*account.grants.lock(), vec!["mach-b".to_owned()], "exactly one account grant");
        assert_eq!(*hosts.redeemed.lock(), vec!["mach-b".to_owned()], "exactly one redeem");
        assert_eq!(
            *channel.requests.lock(),
            vec!["GET /api/v1/supervisor/checkpoint".to_owned(), "GET /api/v1/supervisor/checkpoint".to_owned()],
            "the refused pull and its one retry"
        );
        assert_eq!(link.pairing_state(), "paired");
    }

    /// F4 MINOR-1: a 403 FORBIDDEN without the agent-control marker (e.g. a view-only or
    /// excluded device) is the holder's answer: reported, never repaired.
    #[test]
    fn only_the_marked_agent_control_refusal_counts_as_one() {
        let response = |status: u16, body: &[u8]| HttpResponse { status, headers: Vec::new(), body: body.to_vec() };
        let marked = br#"{"error":{"code":"FORBIDDEN","message":"m","details":{"refusal":"agentControl"}}}"#;
        assert!(agent_control_refusal(&response(403, marked)));
        assert!(!agent_control_refusal(&response(403, br#"{"error":{"code":"FORBIDDEN","message":"m"}}"#)));
        assert!(!agent_control_refusal(&response(
            403,
            br#"{"error":{"code":"FORBIDDEN","message":"m","details":{"refusal":"other"}}}"#
        )));
        assert!(!agent_control_refusal(&response(409, marked)));
    }

    /// A release-to the holder keeps refusing with 403 is reported, not looped on: one fresh
    /// grant, one retry, and a second call inside the backoff redeems nothing more.
    #[tokio::test(start_paused = true)]
    async fn a_release_to_that_stays_forbidden_redeems_once_and_does_not_loop() {
        let (link, hosts) = (HolderLink::new(), FakeHosts::default());
        hosts.hosts.lock().push(crate::hub_runtime::holder_link::tests::host("mach-b", crate::paired_host::inventory::AuthStatus::Paired));
        let account = FakeAccount { session: true, grants: parking_lot::Mutex::new(Vec::new()) };
        let channel = FakeChannel {
            hosts: &hosts,
            archive: Vec::new(),
            requests: Default::default(),
            forbid: std::sync::atomic::AtomicUsize::new(usize::MAX),
        };
        let peer = PairedMovePeer { link: &link, hosts: &hosts, account: &account, channel: &channel };

        let error = cp::MovePeer::release_to(&peer, "mach-b", "mach-a").await.unwrap_err();
        assert_eq!((error.status, error.code.as_str()), (403, "FORBIDDEN"));
        assert_eq!(hosts.redeemed.lock().len(), 1);
        assert_eq!(channel.requests.lock().len(), 2, "the refused call and one retry");

        let error = cp::MovePeer::release_to(&peer, "mach-b", "mach-a").await.unwrap_err();
        assert_eq!(error.code, "FORBIDDEN");
        assert_eq!(account.grants.lock().len(), 1, "no second grant inside the backoff");
        assert_eq!(hosts.redeemed.lock().len(), 1, "no second redeem inside the backoff");
        assert_eq!(channel.requests.lock().len(), 3, "one more call on the fresh grant, no retry");

        let error = cp::MovePeer::release_to(&peer, "mach-b", "mach-a").await.unwrap_err();
        assert_eq!(error.code, cp::HOLDER_UNREACHABLE, "the refused grant is set aside inside the backoff");
        assert_eq!(hosts.redeemed.lock().len(), 1);
        assert_eq!(channel.requests.lock().len(), 3, "nothing is sent on a refused grant");
    }

    /// F3: two standby pulls of this process never overlap.
    #[tokio::test]
    async fn standby_pulls_run_one_at_a_time() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        let running = std::sync::Arc::new(AtomicUsize::new(0));
        let overlapped = std::sync::Arc::new(AtomicBool::new(false));
        let pull = |running: std::sync::Arc<AtomicUsize>, overlapped: std::sync::Arc<AtomicBool>| async move {
            if running.fetch_add(1, Ordering::SeqCst) > 0 {
                overlapped.store(true, Ordering::SeqCst);
            }
            // Gives the other pull every chance to enter while this one runs.
            for _ in 0..16 {
                tokio::task::yield_now().await;
            }
            running.fetch_sub(1, Ordering::SeqCst);
        };
        let first = one_pull_at_a_time(pull(running.clone(), overlapped.clone()));
        let second = one_pull_at_a_time(pull(running.clone(), overlapped.clone()));
        tokio::time::timeout(std::time::Duration::from_secs(10), async { tokio::join!(first, second) })
            .await
            .expect("bounded");
        assert!(!overlapped.load(Ordering::SeqCst), "the pulls overlapped");
    }
}
