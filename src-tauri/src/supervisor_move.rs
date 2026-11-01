//! Production wiring of [`crate::supervisor_checkpoint::move_here`]: the holder reached over
//! the paired end-to-end encrypted channel only, todo 7's staged import, and this daemon's
//! lease client.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::paired_host::service::PairedHostService;
use crate::remote::state::RemoteGatewayState;
use crate::supervisor_checkpoint::{self as cp, MoveError};

/// The daemon's paired-host inventory, installed with the lease client (the phone's move
/// route has only the gateway state).
static HOSTS: std::sync::RwLock<Option<PairedHostService>> = std::sync::RwLock::new(None);

pub(crate) fn install_hosts(hosts: PairedHostService) {
    *HOSTS.write().unwrap_or_else(|e| e.into_inner()) = Some(hosts);
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
pub async fn move_here_installed(state: &RemoteGatewayState, force: bool, by: &str) -> Result<bool, MoveError> {
    let hosts = HOSTS.read().unwrap_or_else(|e| e.into_inner()).clone();
    let hosts = hosts.ok_or_else(|| MoveError::new(503, "SUPERVISOR_UNAVAILABLE", "the supervisor hub is not open in this daemon"))?;
    move_supervisor_here(state, &hosts, force, by).await
}

/// The holder over [`crate::paired_host::client::supervisor_secure_request`].
struct PairedMovePeer<'a> {
    hosts: &'a PairedHostService,
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
        let response = crate::paired_host::client::supervisor_secure_request(
            self.hosts,
            holder,
            "POST",
            "/api/v1/supervisor/release-to",
            Some(body.as_slice()),
            64 * 1024,
        )
        .await
        .map_err(peer_error)?;
        if response.status != 200 {
            return Err(peer_refusal(response.status, &response.body));
        }
        let value: Value = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
        Ok(value["checkpointId"].as_str().map(str::to_owned))
    }

    async fn fetch(&self, holder: &str) -> Result<(String, Vec<u8>), MoveError> {
        let limit = usize::try_from(ferryx_hub::checkpoint::CHECKPOINT_MAX_COMPRESSED_BYTES).unwrap_or(usize::MAX);
        let response = crate::paired_host::client::supervisor_secure_request(
            self.hosts,
            holder,
            "GET",
            "/api/v1/supervisor/checkpoint",
            None,
            limit,
        )
        .await
        .map_err(peer_error)?;
        if response.status != 200 {
            return Err(peer_refusal(response.status, &response.body));
        }
        let id = response.header(cp::CHECKPOINT_ID_HEADER).map(str::to_owned).unwrap_or_default();
        Ok((id, response.body))
    }
}

/// Todo 7's staged import into this machine's hub directory.
struct HubMoveImport {
    hub_dir: std::path::PathBuf,
    machine_id: String,
    paired: Vec<ferryx_hub::checkpoint::PairedHostMapping>,
}

#[async_trait]
impl cp::MoveImport for HubMoveImport {
    async fn import(&self, archive: &std::path::Path) -> Result<(), MoveError> {
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default();
        let options = || ferryx_hub::checkpoint_import::ImportOptions {
            memory_root: home.join(".omo").join("memory").join("agents"),
            env_file: ferryx_hub::hub_env::env_file_path().unwrap_or_else(|| home.join(".ferryx/hub/env")),
            profile_omo_jsonc: None,
            now_ts: crate::hub_runtime::unix_now() as i64,
            write_private: ferryx_hub::checkpoint_import::default_private_writer(),
        };
        import_replacing_local(archive, &self.hub_dir, &self.machine_id, &self.paired, options)
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
/// refuses `SUPERVISOR_IDENTITY_MISMATCH`), the local one is replaced: its database is first
/// archived as `hub.sqlite.pre-lease-<ts>` (plan todo 13, its open goals stay adoptable),
/// then the import runs once more. A matching identity imports without an archive.
async fn import_replacing_local(
    archive: &std::path::Path,
    hub_dir: &std::path::Path,
    machine_id: &str,
    paired: &[ferryx_hub::checkpoint::PairedHostMapping],
    options: impl Fn() -> ferryx_hub::checkpoint_import::ImportOptions,
) -> Result<(), ReplaceError> {
    use ferryx_hub::checkpoint_import::import;
    let failed = |error: ferryx_hub::checkpoint_import::ImportError| ReplaceError {
        code: error.code().to_owned(),
        message: error.to_string(),
    };
    let refused = match import(archive, hub_dir, machine_id, paired, options()).await {
        Ok(_) => return Ok(()),
        Err(error) if error.code() == ferryx_hub::checkpoint::SUPERVISOR_IDENTITY_MISMATCH => error,
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
pub async fn move_supervisor_here(
    state: &RemoteGatewayState,
    hosts: &PairedHostService,
    force: bool,
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
    let importer = HubMoveImport { hub_dir, machine_id, paired };
    let peer = PairedMovePeer { hosts };
    let store = cp::CheckpointStore::new(dir);
    let moved = cp::move_here(&lease, &peer, &importer, &store, &cp::shared(), force, crate::hub_runtime::unix_now()).await?;
    tracing::info!(by, moved, force, "supervisor move requested");
    crate::remote::server::agent_control_service::publish_supervisor_changed(state, json!({ "hub": "moved" }));
    Ok(moved)
}

/// Auto-promotion's import: the takeover the relay granted against this machine's newest
/// pulled checkpoint is staged before the hub reopens. A failed import is logged; the hub then
/// reopens on its own database, as the import never touched it.
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
    let importer = HubMoveImport { hub_dir: hub_dir.to_owned(), machine_id: crate::hub_runtime::lease_machine_id().await, paired };
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

/// A standby's replication step: pulls the holder's newest checkpoint over the paired
/// encrypted channel when it is not stored here yet.
pub async fn pull_from_holder() {
    let Some(handle) = crate::hub_runtime::lease_handle() else { return };
    let hosts = HOSTS.read().unwrap_or_else(|e| e.into_inner()).clone();
    let (Some(hosts), Some(dir)) = (hosts, cp::checkpoint_dir()) else { return };
    let Some(ack) = handle.query().await else { return };
    if ack.outcome != crate::remote::protocol::SupervisorLeaseOutcome::Refused {
        return;
    }
    let (Some(holder), Some(recorded)) = (ack.holder_machine_id, ack.checkpoint_id) else { return };
    let store = cp::CheckpointStore::new(dir);
    if store.find(&recorded).is_some() {
        return;
    }
    let peer = PairedMovePeer { hosts: &hosts };
    match cp::MovePeer::fetch(&peer, &holder).await {
        Ok((id, bytes)) => match cp::save_blocking(&store, &id, bytes, crate::hub_runtime::unix_now()).await {
            Ok(saved) => cp::shared().set_local(Some(saved)),
            Err(error) => tracing::warn!(code = %error.code, "pulled supervisor checkpoint not stored: {}", error.message),
        },
        Err(error) => tracing::debug!(code = %error.code, "supervisor checkpoint not pulled: {}", error.message),
    }
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

    /// Replacing a local supervisor that has another identity archives it before the
    /// import (todo 13's `archive_before_replace`), then stages the checkpoint.
    #[tokio::test]
    async fn a_move_onto_another_local_supervisor_archives_it_before_the_import() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let pool = hub_with_owner(&source, "owner:u1").await;
        let exported = ferryx_hub::checkpoint::export(
            &pool,
            &ferryx_hub::checkpoint::ExportRequest::new(
                source.clone(),
                "machine-a".into(),
                root.path().join("checkpoint.tar.zst"),
                ferryx_hub::checkpoint_import::default_private_writer(),
            ),
        )
        .await
        .unwrap();
        pool.close().await;
        let target = root.path().join("target");
        hub_with_owner(&target, "owner:u1").await.close().await;
        let options = || ferryx_hub::checkpoint_import::ImportOptions {
            memory_root: root.path().join("memory-agents"),
            env_file: target.join("env"),
            profile_omo_jsonc: None,
            now_ts: NOW,
            write_private: ferryx_hub::checkpoint_import::default_private_writer(),
        };
        let archives = || {
            std::fs::read_dir(&target)
                .unwrap()
                .filter(|entry| {
                    entry.as_ref().unwrap().file_name().to_string_lossy().starts_with(ferryx_hub::adoption::ARCHIVE_PREFIX)
                })
                .count()
        };

        import_replacing_local(&exported.path, &target, "machine-b", &[], options).await.unwrap();

        assert!(target.join(format!("{}{NOW}", ferryx_hub::adoption::ARCHIVE_PREFIX)).is_file(), "archived first");
        assert!(target.join(ferryx_hub::checkpoint_import::INCOMING_DB).exists(), "the checkpoint is staged");
        assert_eq!(archives(), 1);
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
        HubMoveImport { hub_dir, machine_id: "mach-b".to_owned(), paired: Vec::new() }
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
}
