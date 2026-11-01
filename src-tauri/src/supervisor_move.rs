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
        let options = ferryx_hub::checkpoint_import::ImportOptions {
            memory_root: home.join(".omo").join("memory").join("agents"),
            env_file: ferryx_hub::hub_env::env_file_path().unwrap_or_else(|| home.join(".ferryx/hub/env")),
            profile_omo_jsonc: None,
            now_ts: crate::hub_runtime::unix_now() as i64,
            write_private: ferryx_hub::checkpoint_import::default_private_writer(),
        };
        ferryx_hub::checkpoint_import::import(archive, &self.hub_dir, &self.machine_id, &self.paired, options)
            .await
            .map(|_| ())
            .map_err(|error| MoveError::new(409, error.code(), error.to_string()))
    }

    async fn discard(&self) {
        let incoming = self.hub_dir.join(ferryx_hub::checkpoint_import::INCOMING_DB);
        if let Err(error) = tokio::fs::remove_file(&incoming).await {
            tracing::warn!(%error, "the staged checkpoint import was not discarded");
        }
    }
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
    let archive = cp::CheckpointStore::new(dir).path_of(&id);
    match cp::MoveImport::import(&importer, &archive).await {
        Ok(()) => {
            state.set_own(&id);
            tracing::info!(checkpoint = %id, "promotion imported the synced checkpoint");
        }
        Err(error) => tracing::warn!(code = %error.code, "promotion did not import the synced checkpoint: {}", error.message),
    }
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
