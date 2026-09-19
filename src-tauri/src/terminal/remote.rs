//! Same-target remote terminal runtime. SSH child ownership remains in `ssh::bridge`.
use super::output_hub::TerminalOutputHub;
use crate::scoped_contracts::TargetRef;
use crate::ssh::{bridge::*, helper_setup::HelperLocation, runtime::RemoteEnvironment, SshHost};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, future::Future, path::PathBuf, pin::Pin, sync::Arc, time::Duration};
use tokio::sync::{watch, Mutex as AsyncMutex};

type Rpc<'a, T> = Pin<Box<dyn Future<Output = Result<T, BridgeError>> + Send + 'a>>;
pub type RemoteOperation = Pin<Box<dyn Future<Output = Result<(), RemoteFailure>> + Send>>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSessionConfig {
    pub host: SshHost,
    pub environment: RemoteEnvironment,
    pub helper: HelperLocation,
    pub project_id: String,
    pub project_path: String,
    pub worktree: Option<String>,
    /// Opaque launcher identity; never interpreted as an agent resume command.
    pub agent_identity: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSessionDescriptor {
    pub backend_session_id: String,
    pub target: TargetRef,
    pub config: RemoteSessionConfig,
    pub client_request_id: String,
    pub remote_cursor: RemoteCursor,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoteFailureKind {
    Transport,
    Authentication,
    Missing,
    Expired,
    Protocol,
    Busy,
    StaleGeneration,
    Disconnected,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{kind:?}: {message}")]
pub struct RemoteFailure {
    pub kind: RemoteFailureKind,
    pub message: String,
}
impl RemoteFailure {
    fn new(kind: RemoteFailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
    pub fn from_bridge(error: &BridgeError) -> Self {
        let text = error.to_string();
        let kind = match error {
            BridgeError::TargetNotFound => RemoteFailureKind::Missing,
            BridgeError::RemoteTargetExpired | BridgeError::TargetExpired { .. } => {
                RemoteFailureKind::Expired
            }
            BridgeError::ProcessExited { stderr, .. } => ssh_stderr_failure_kind(stderr),
            BridgeError::SshSetup(error) => {
                let details = error.details.as_ref();
                let stage = details.and_then(|d| d.get("stage")).and_then(|v| v.as_str());
                if stage == Some("helper_missing")
                    || error.code == crate::ipc::IpcErrorCode::CliExecutableNotFound
                {
                    RemoteFailureKind::Missing
                } else if error.code != crate::ipc::IpcErrorCode::IoError {
                    RemoteFailureKind::Protocol
                } else if stage == Some("transport") {
                    RemoteFailureKind::Transport
                } else if stage == Some("execution")
                    && details.and_then(|d| d.get("exitCode")).and_then(|v| v.as_i64()) == Some(255)
                {
                    details
                        .and_then(|d| d.get("stderr"))
                        .and_then(|v| v.as_str())
                        .map(ssh_stderr_failure_kind)
                        .unwrap_or(RemoteFailureKind::Protocol)
                } else {
                    RemoteFailureKind::Protocol
                }
            }
            BridgeError::Io(_) | BridgeError::ConnectionClosed | BridgeError::Timeout(_) => {
                RemoteFailureKind::Transport
            }
            BridgeError::SshPlan(_)
            | BridgeError::ProcessSpawn(_)
            | BridgeError::FrameTooLarge(_)
            | BridgeError::Protocol(_)
            | BridgeError::Remote(_)
            | BridgeError::HostMismatch { .. }
            | BridgeError::TargetMismatch { .. }
            | BridgeError::NotTransferable(_) => RemoteFailureKind::Protocol,
        };
        Self::new(kind, text)
    }
}
fn ssh_stderr_failure_kind(stderr: &str) -> RemoteFailureKind {
    let lower = stderr.to_lowercase();
    if lower.contains("permission denied")
        || lower.contains("authentication")
        || lower.contains("host key verification failed")
    {
        RemoteFailureKind::Authentication
    } else {
        RemoteFailureKind::Transport
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoteConnectionState {
    Connected,
    Reconnecting,
    Disconnected,
    Expired,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteReplayGap {
    pub requested_after_cursor: RemoteCursor,
    pub available_from_cursor: RemoteCursor,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSessionDetails {
    pub descriptor: RemoteSessionDescriptor,
    pub state: RemoteConnectionState,
    pub generation: u64,
    pub attempts: u32,
    pub failure: Option<RemoteFailure>,
    pub replay_gap: Option<RemoteReplayGap>,
    pub pid: Option<RemotePid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteExportState {
    pub descriptor: RemoteSessionDescriptor,
    pub generation: u64,
    pub pending_size: Option<(u16, u16)>,
    pub pid: Option<RemotePid>,
    pub bridge_transfer: Option<SshBridgeTransferState>,
}

// Internal seam keeps deterministic tests at the actual controller boundary.
pub(crate) trait Transport: Send + Sync {
    fn describe<'a>(&'a self, target: &'a TargetRef) -> Rpc<'a, DescribeResult>;
    fn read<'a>(&'a self, target: &'a TargetRef, cursor: RemoteCursor) -> Rpc<'a, ReadResult>;
    fn write<'a>(&'a self, target: &'a TargetRef, bytes: &'a [u8]) -> Rpc<'a, ()>;
    fn resize<'a>(&'a self, target: &'a TargetRef, cols: u16, rows: u16) -> Rpc<'a, ()>;
    fn stop<'a>(&'a self, target: &'a TargetRef) -> Rpc<'a, ()>;
    fn as_ssh_bridge(&self) -> Option<&SshBridgeClient> {
        None
    }
}
impl Transport for SshBridgeClient {
    fn describe<'a>(&'a self, t: &'a TargetRef) -> Rpc<'a, DescribeResult> {
        Box::pin(self.reattach(t))
    }
    fn read<'a>(&'a self, t: &'a TargetRef, c: RemoteCursor) -> Rpc<'a, ReadResult> {
        Box::pin(self.pty_read(t, c, 1000))
    }
    fn write<'a>(&'a self, t: &'a TargetRef, b: &'a [u8]) -> Rpc<'a, ()> {
        Box::pin(async move {
            if self.pty_write(t, b).await?.accepted {
                Ok(())
            } else {
                Err(BridgeError::Remote("Input rejected".into()))
            }
        })
    }
    fn resize<'a>(&'a self, t: &'a TargetRef, c: u16, r: u16) -> Rpc<'a, ()> {
        Box::pin(async move {
            self.pty_resize(t, c, r).await?;
            Ok(())
        })
    }
    fn stop<'a>(&'a self, t: &'a TargetRef) -> Rpc<'a, ()> {
        Box::pin(async move {
            self.pty_stop(t).await?;
            Ok(())
        })
    }
    fn as_ssh_bridge(&self) -> Option<&SshBridgeClient> {
        Some(self)
    }
}
pub(crate) trait Connector: Send + Sync {
    fn connect<'a>(&'a self, d: &'a RemoteSessionDescriptor) -> Rpc<'a, Arc<dyn Transport>>;
    fn delay(&self, attempt: u32) -> Pin<Box<dyn Future<Output = ()> + Send>>;
}
struct SshConnector;
impl Connector for SshConnector {
    fn connect<'a>(&'a self, d: &'a RemoteSessionDescriptor) -> Rpc<'a, Arc<dyn Transport>> {
        Box::pin(async move {
            Ok(Arc::new(
                SshBridgeClient::connect_with_target(
                    &d.config.host,
                    &d.config.environment,
                    &d.config.helper,
                    Some(&d.target),
                )
                .await?,
            ) as Arc<dyn Transport>)
        })
    }
    fn delay(&self, attempt: u32) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        Box::pin(tokio::time::sleep(Duration::from_millis(
            250 * (1u64 << attempt.min(5)),
        )))
    }
}
pub(crate) struct Session {
    pub(crate) details: RemoteSessionDetails,
    transport: Option<Arc<dyn Transport>>,
    task: Option<tokio::task::JoinHandle<()>>,
    updates: watch::Sender<RemoteSessionDetails>,
    /// Latest client-requested size that never reached the remote PTY because the
    /// session was disconnected or the in-flight RPC failed. Re-applied on the next
    /// successful connect so pane geometry survives the connection race instead of
    /// leaving the remote PTY at its spawn defaults.
    pending_size: Option<(u16, u16)>,
}
pub(crate) struct Entry {
    pub(crate) state: Mutex<Session>,
    control: Arc<AsyncMutex<()>>,
}
impl Entry {
    fn notify(s: &Session) {
        s.updates.send_replace(s.details.clone());
    }
}
pub struct RemoteRuntime {
    sessions: Mutex<HashMap<String, Arc<Entry>>>,
    hub: Arc<TerminalOutputHub>,
    connector: Arc<dyn Connector>,
}
const MAX_ATTEMPTS: u32 = 5;
impl RemoteRuntime {
    pub fn new(hub: Arc<TerminalOutputHub>) -> Self {
        Self::with_connector(hub, Arc::new(SshConnector))
    }
    pub(crate) fn with_connector(
        hub: Arc<TerminalOutputHub>,
        connector: Arc<dyn Connector>,
    ) -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            hub,
            connector,
        }
    }
    /// Only this method invokes pty.spawn. Caller supplies an immutable, durable request ID.
    pub async fn create(
        &self,
        config: RemoteSessionConfig,
        mut params: SpawnParams,
        client_request_id: String,
    ) -> Result<RemoteSessionDescriptor, RemoteFailure> {
        if client_request_id.is_empty()
            || params
                .client_request_id
                .as_ref()
                .is_some_and(|id| id != &client_request_id)
        {
            return Err(RemoteFailure::new(
                RemoteFailureKind::Protocol,
                "A stable matching clientRequestId is required",
            ));
        }
        params.client_request_id = Some(client_request_id.clone());
        params.project_id = config.project_id.clone();
        params.worktree = config.worktree.clone();
        let cols = params.cols.unwrap_or(80);
        let rows = params.rows.unwrap_or(24);
        let client = SshBridgeClient::connect(&config.host, &config.environment, &config.helper)
            .await
            .map_err(|e| RemoteFailure::from_bridge(&e))?;
        let result = async {
            client
                .project_register(&config.project_id, &config.project_path)
                .await?;
            client.pty_spawn_retry(&params, &client_request_id).await
        }
        .await;
        // No reconnect path is allowed to call create, including uncertain spawn results.
        let spawned = result.map_err(|e| RemoteFailure::from_bridge(&e))?;
        client
            .validate_target(&spawned.target)
            .map_err(|e| RemoteFailure::from_bridge(&e))?;
        let d = RemoteSessionDescriptor {
            backend_session_id: uuid::Uuid::new_v4().to_string(),
            target: spawned.target,
            config,
            client_request_id,
            remote_cursor: RemoteCursor(0),
            cols,
            rows,
        };
        self.insert(d.clone(), Some(Arc::new(client)))?;
        Ok(d)
    }
    /// Registers persisted identity and reattaches; never spawns a PTY.
    pub fn restore(&self, descriptor: RemoteSessionDescriptor) -> Result<(), RemoteFailure> {
        self.insert(descriptor, None)
    }
    fn insert(
        &self,
        d: RemoteSessionDescriptor,
        transport: Option<Arc<dyn Transport>>,
    ) -> Result<(), RemoteFailure> {
        if d.target.host_id != d.config.host.id
            || d.backend_session_id.is_empty()
            || d.client_request_id.is_empty()
        {
            return Err(RemoteFailure::new(
                RemoteFailureKind::Protocol,
                "Invalid persisted remote identity",
            ));
        }
        let mut map = self.sessions.lock();
        if map.contains_key(&d.backend_session_id) || self.hub.has_session(&d.backend_session_id) {
            return Err(RemoteFailure::new(
                RemoteFailureKind::Protocol,
                "Backend session ID already registered",
            ));
        }
        if map.values().any(|entry| entry.state.lock().details.descriptor.target == d.target) {
            return Err(RemoteFailure::new(
                RemoteFailureKind::Protocol,
                "Remote target already has a session controller",
            ));
        }
        let id = d.backend_session_id.clone();
        self.hub.register_session(&id);
        self.hub.record_initial_size(&id, d.cols, d.rows);
        let details = RemoteSessionDetails {
            descriptor: d,
            state: RemoteConnectionState::Reconnecting,
            generation: 1,
            attempts: 0,
            failure: None,
            replay_gap: None,
            pid: None,
        };
        let (updates, _) = watch::channel(details.clone());
        let entry = Arc::new(Entry {
            state: Mutex::new(Session {
                details,
                updates,
                transport: None,
                task: None,
                pending_size: None,
            }),
            control: Arc::new(AsyncMutex::new(())),
        });
        map.insert(id, entry.clone());
        self.launch(&entry, transport);
        Ok(())
    }
    fn entry(&self, id: &str) -> Result<Arc<Entry>, RemoteFailure> {
        self.sessions.lock().get(id).cloned().ok_or_else(|| {
            RemoteFailure::new(RemoteFailureKind::Missing, "Remote session not registered")
        })
    }
    pub fn contains(&self, id: &str) -> bool {
        self.sessions.lock().contains_key(id)
    }
    pub fn list(&self) -> Vec<String> {
        self.sessions.lock().keys().cloned().collect()
    }
    pub fn details(&self, id: &str) -> Option<RemoteSessionDetails> {
        self.entry(id).ok().map(|e| e.state.lock().details.clone())
    }
    #[cfg(test)]
    pub(crate) fn entry_for_test(&self, id: &str) -> Result<Arc<Entry>, RemoteFailure> {
        self.entry(id)
    }
    pub fn subscribe(
        &self,
        id: &str,
    ) -> Result<watch::Receiver<RemoteSessionDetails>, RemoteFailure> {
        Ok(self.entry(id)?.state.lock().updates.subscribe())
    }
    /// Atomically captures the history snapshot from the hub while holding the remote session lock,
    /// guaranteeing the snapshot and remote generation cannot desync across reconnects.
    pub fn attach_snapshot_with_generation(
        &self,
        id: &str,
        after_sequence: Option<u64>,
    ) -> Result<(crate::terminal::output_hub::SessionAttachment, u64), RemoteFailure> {
        let e = self.entry(id)?;
        let s = e.state.lock();
        let generation = s.details.generation;
        let attachment = self
            .hub
            .subscribe_with_sequence(id, after_sequence)
            .ok_or_else(|| RemoteFailure::new(RemoteFailureKind::Missing, "Session not found in hub"))?;
        Ok((attachment, generation))
    }
    fn launch(&self, entry: &Arc<Entry>, initial: Option<Arc<dyn Transport>>) {
        let mut state = entry.state.lock();
        let generation = state.details.generation;
        let e = entry.clone();
        let hub = self.hub.clone();
        let connector = self.connector.clone();
        state.task = Some(tokio::spawn(async move {
            run(e, hub, connector, generation, initial).await;
        }));
    }
    /// Explicit retry is deduplicated while reconnecting and only describes the stored target.
    pub fn retry(&self, id: &str) -> Result<(), RemoteFailure> {
        let e = self.entry(id)?;
        {
            let mut s = e.state.lock();
            if matches!(
                s.details.state,
                RemoteConnectionState::Connected | RemoteConnectionState::Reconnecting
            ) {
                return Ok(());
            }
            if let Some(task) = s.task.take() {
                task.abort();
            }
            s.details.generation += 1;
            s.details.state = RemoteConnectionState::Reconnecting;
            s.details.attempts = 0;
            s.transport = None;
            Entry::notify(&s);
        }
        self.launch(&e, None);
        Ok(())
    }
    /// Admission is synchronous. No queued control lock; future rechecks generation at dispatch.
    pub fn write(
        &self,
        id: &str,
        generation: u64,
        bytes: Vec<u8>,
    ) -> Result<RemoteOperation, RemoteFailure> {
        self.operation(id, generation, Some(bytes), None)
    }
    pub fn resize(
        &self,
        id: &str,
        generation: u64,
        cols: u16,
        rows: u16,
    ) -> Result<RemoteOperation, RemoteFailure> {
        self.operation(id, generation, None, Some((cols, rows)))
    }
    fn operation(
        &self,
        id: &str,
        generation: u64,
        bytes: Option<Vec<u8>>,
        size: Option<(u16, u16)>,
    ) -> Result<RemoteOperation, RemoteFailure> {
        let e = self.entry(id)?;
        let guard = e.control.clone().try_lock_owned().map_err(|_| {
            RemoteFailure::new(
                RemoteFailureKind::Busy,
                "Remote control operation in flight; input was not queued",
            )
        })?;
        {
            let mut s = e.state.lock();
            if let Err(error) = check_connected(&s, generation) {
                // A size request that could not be dispatched is still the pane's
                // intent; remember it so the next connect re-applies it.
                if size.is_some() {
                    s.pending_size = size;
                }
                return Err(error);
            }
        }
        drop(guard);
        let hub = self.hub.clone();
        Ok(Box::pin(async move {
            let _guard = e.control.clone().try_lock_owned().map_err(|_| {
                RemoteFailure::new(
                    RemoteFailureKind::Busy,
                    "Remote control is busy; input was not queued",
                )
            })?;
            let (client, target) = {
                let mut s = e.state.lock();
                if let Err(error) = check_connected(&s, generation) {
                    // The reconnect raced between admission and dispatch; the size
                    // request must survive it exactly like the admission failure.
                    if size.is_some() {
                        s.pending_size = size;
                    }
                    return Err(error);
                }
                (
                    s.transport.clone().unwrap(),
                    s.details.descriptor.target.clone(),
                )
            };
            // run() takes the same gate before replacing/invalidation of this generation.
            let result = if let Some(bytes) = bytes {
                client.write(&target, &bytes).await
            } else {
                let (c, r) = size.unwrap();
                client.resize(&target, c, r).await
            };
            let mut s = e.state.lock();
            match result {
                Ok(()) => {
                    if let Some((cols, rows)) = size {
                        s.details.descriptor.cols = cols;
                        s.details.descriptor.rows = rows;
                        s.pending_size = None;
                        hub.record_resize(&s.details.descriptor.backend_session_id, cols, rows);
                        Entry::notify(&s);
                    }
                    Ok(())
                }
                Err(error) => {
                    let failure = RemoteFailure::from_bridge(&error);
                    // The request never reached the remote PTY; keep it for the
                    // reconnect path so the size is not silently lost.
                    if size.is_some() {
                        s.pending_size = size;
                    }
                    fail(&mut s, failure.clone());
                    Err(failure)
                }
            }
        }))
    }
    /// User intent only. Outage close establishes a connection solely to stop the explicit target.
    pub async fn close(&self, id: &str) -> Result<(), RemoteFailure> {
        let e = self.entry(id)?;
        let _gate = e.control.lock().await;
        let (d, client) = {
            let mut s = e.state.lock();
            if let Some(t) = s.task.take() {
                t.abort();
            }
            s.details.generation += 1;
            s.details.state = RemoteConnectionState::Disconnected;
            let c = s.transport.take();
            Entry::notify(&s);
            (s.details.descriptor.clone(), c)
        };
        let result = async {
            let client = match client {
                Some(c) => c,
                None => self.connector.connect(&d).await?,
            };
            client.stop(&d.target).await
        }
        .await;
        if let Err(error) = result {
            let f = RemoteFailure::from_bridge(&error);
            let mut s = e.state.lock();
            fail(&mut s, f.clone());
            s.details.state = RemoteConnectionState::Disconnected;
            Entry::notify(&s);
            return Err(f);
        }
        self.sessions.lock().remove(id);
        self.hub.remove_session(id);
        Ok(())
    }
    /// Exports the full session state for handover ownership transfer.
    /// Produces full RemoteSessionDescriptor, exact remote_cursor, exact generation,
    /// pending size, pid, and captured SSH bridge transfer state.
    pub async fn export_entry(&self, id: &str) -> Result<RemoteExportState, RemoteFailure> {
        let e = self.entry(id)?;
        let _gate = e.control.lock().await;

        let bridge_client = {
            let s = e.state.lock();
            s.transport
                .as_ref()
                .and_then(|t| t.as_ssh_bridge().cloned())
        };

        let bridge_transfer = if let Some(ref client) = bridge_client {
            client
                .pause_for_transfer()
                .await
                .map_err(|e| RemoteFailure::from_bridge(&e))?;
            let state = match client.export_transfer_state().await {
                Ok(s) => s,
                Err(e) => {
                    let _ = client.unpause_after_rollback().await;
                    return Err(RemoteFailure::from_bridge(&e));
                }
            };
            Some(state)
        } else {
            None
        };

        let (descriptor, generation, pending_size, pid) = {
            let mut s = e.state.lock();
            if let Some(task) = s.task.take() {
                task.abort();
            }
            (
                s.details.descriptor.clone(),
                s.details.generation,
                s.pending_size,
                s.details.pid,
            )
        };

        Ok(RemoteExportState {
            descriptor,
            generation,
            pending_size,
            pid,
            bridge_transfer,
        })
    }

    /// Alias for `export_entry`.
    pub async fn export_for_transfer(&self, id: &str) -> Result<RemoteExportState, RemoteFailure> {
        self.export_entry(id).await
    }

    /// Detaches the session's SSH bridge without killing the local SSH child process.
    pub async fn detach_without_kill(&self, id: &str) -> Result<(), RemoteFailure> {
        let e = self.entry(id)?;
        let _gate = e.control.lock().await;
        let bridge = {
            let mut s = e.state.lock();
            if let Some(t) = s.task.take() {
                t.abort();
            }
            s.transport.take().and_then(|t| t.as_ssh_bridge().cloned())
        };
        if let Some(bridge) = bridge {
            bridge.detach_without_kill().await;
        }
        Ok(())
    }

    /// Live-imports a transferred session into this runtime.
    ///
    /// Distinct from `RemoteRuntime::restore`:
    /// - Preserves `RemoteCursor` and `generation` exactly (never reset to 0 or 1).
    /// - Never reconnects.
    /// - Never spawns a new remote PTY.
    /// - Attaches directly to the existing `TerminalOutputHub` session without failing.
    pub fn live_import(
        &self,
        state: RemoteExportState,
        transport: Option<Arc<dyn Transport>>,
    ) -> Result<(), RemoteFailure> {
        let transport = match transport {
            Some(t) => Some(t),
            None => {
                if let Some(bts) = state.bridge_transfer.clone() {
                    let client = SshBridgeClient::from_transfer_state(bts)
                        .map_err(|e| RemoteFailure::from_bridge(&e))?;
                    Some(Arc::new(client) as Arc<dyn Transport>)
                } else {
                    None
                }
            }
        };
        self.insert_live(state, transport)
    }

    /// Alias for `live_import`.
    pub fn import_transferred(
        &self,
        state: RemoteExportState,
        transport: Option<Arc<dyn Transport>>,
    ) -> Result<(), RemoteFailure> {
        self.live_import(state, transport)
    }

    fn insert_live(
        &self,
        state: RemoteExportState,
        transport: Option<Arc<dyn Transport>>,
    ) -> Result<(), RemoteFailure> {
        let d = state.descriptor;
        if d.target.host_id != d.config.host.id
            || d.backend_session_id.is_empty()
            || d.client_request_id.is_empty()
        {
            return Err(RemoteFailure::new(
                RemoteFailureKind::Protocol,
                "Invalid persisted remote identity",
            ));
        }
        let mut map = self.sessions.lock();
        if map.contains_key(&d.backend_session_id) {
            return Err(RemoteFailure::new(
                RemoteFailureKind::Protocol,
                "Backend session ID already registered",
            ));
        }
        if map.values().any(|entry| entry.state.lock().details.descriptor.target == d.target) {
            return Err(RemoteFailure::new(
                RemoteFailureKind::Protocol,
                "Remote target already has a session controller",
            ));
        }
        let id = d.backend_session_id.clone();
        if !self.hub.has_session(&id) {
            self.hub.register_session(&id);
            self.hub.record_initial_size(&id, d.cols, d.rows);
        }
        let details = RemoteSessionDetails {
            descriptor: d,
            state: RemoteConnectionState::Connected,
            generation: state.generation,
            attempts: 0,
            failure: None,
            replay_gap: None,
            pid: state.pid,
        };
        let (updates, _) = watch::channel(details.clone());
        let entry = Arc::new(Entry {
            state: Mutex::new(Session {
                details,
                updates,
                transport: transport.clone(),
                task: None,
                pending_size: state.pending_size,
            }),
            control: Arc::new(AsyncMutex::new(())),
        });
        map.insert(id, entry.clone());
        if let Some(client) = transport {
            self.launch_live(&entry, client);
        }
        Ok(())
    }

    fn launch_live(&self, entry: &Arc<Entry>, client: Arc<dyn Transport>) {
        let mut state = entry.state.lock();
        let generation = state.details.generation;
        let e = entry.clone();
        let hub = self.hub.clone();
        let connector = self.connector.clone();
        state.task = Some(tokio::spawn(async move {
            run_live(e, hub, connector, generation, client).await;
        }));
    }
}
impl Drop for RemoteRuntime {
    fn drop(&mut self) {
        for e in self.sessions.get_mut().values() {
            let mut s = e.state.lock();
            if let Some(t) = s.task.take() {
                t.abort();
            }
            s.transport = None;
            s.details.generation += 1;
            s.details.state = RemoteConnectionState::Disconnected;
            Entry::notify(&s);
        }
    }
}
fn check_connected(s: &Session, generation: u64) -> Result<(), RemoteFailure> {
    if s.details.generation != generation {
        return Err(RemoteFailure::new(
            RemoteFailureKind::StaleGeneration,
            "Input belongs to an old connection generation",
        ));
    }
    if s.details.state != RemoteConnectionState::Connected {
        return Err(RemoteFailure::new(
            RemoteFailureKind::Disconnected,
            "Remote terminal is not connected; input was not sent",
        ));
    }
    Ok(())
}
fn fail(s: &mut Session, failure: RemoteFailure) {
    s.transport = None;
    s.details.state = match failure.kind {
        RemoteFailureKind::Expired | RemoteFailureKind::Missing => RemoteConnectionState::Expired,
        RemoteFailureKind::Transport => RemoteConnectionState::Reconnecting,
        _ => RemoteConnectionState::Disconnected,
    };
    s.details.failure = Some(failure);
    Entry::notify(s);
}
async fn run(
    e: Arc<Entry>,
    hub: Arc<TerminalOutputHub>,
    connector: Arc<dyn Connector>,
    mut generation: u64,
    mut initial: Option<Arc<dyn Transport>>,
) {
    let mut attempts = 0;
    loop {
        let d = {
            let mut s = e.state.lock();
            if s.details.generation != generation {
                return;
            }
            // A previous transport failure must not mask authentication/expiry on this dial.
            s.details.failure = None;
            s.details.descriptor.clone()
        };
        let connection = match initial.take() {
            Some(c) => Ok(c),
            None => connector.connect(&d).await,
        };
        let outcome = async {
            let client = connection?;
            let info = client.describe(&d.target).await?;
            if info.target != d.target {
                return Err(BridgeError::TargetMismatch {
                    expected: d.target.clone(),
                    actual: info.target,
                });
            }
            if info.exited {
                return Err(BridgeError::TargetNotFound);
            }
            let mut desired_size: Option<(u16, u16)> = None;
            {
                let _gate = e.control.lock().await;
                let mut s = e.state.lock();
                if s.details.generation != generation {
                    return Ok(());
                }
                s.transport = Some(client.clone());
                s.details.pid = Some(info.pid);
                s.details.state = RemoteConnectionState::Connected;
                s.details.failure = None;
                desired_size = Some(s.pending_size.take().unwrap_or((
                    s.details.descriptor.cols,
                    s.details.descriptor.rows,
                )));
                Entry::notify(&s);
            }
            // Converge the remote PTY onto the last size the daemon knows about
            // before streaming resumes: a reconnect must not leave the remote shell
            // at stale spawn defaults while the pane renders a different grid.
            if let Some((cols, rows)) = desired_size {
                if client.resize(&d.target, cols, rows).await.is_ok() {
                    let mut s = e.state.lock();
                    if s.details.generation != generation {
                        return Ok(());
                    }
                    s.details.descriptor.cols = cols;
                    s.details.descriptor.rows = rows;
                    hub.record_resize(&d.backend_session_id, cols, rows);
                    Entry::notify(&s);
                }
            }
            // Subscribe before observing state so control-side failures cannot be lost
            // while the independent reader is in a long poll.
            let mut updates = e.state.lock().updates.subscribe();
            loop {
                let cursor = {
                    let s = e.state.lock();
                    if s.details.generation != generation {
                        return Ok(());
                    }
                    if s.details.state != RemoteConnectionState::Connected {
                        return Err(BridgeError::ConnectionClosed);
                    }
                    s.details.descriptor.remote_cursor
                };
                let read = tokio::select! {
                    biased;
                    _ = updates.wait_for(|details| {
                        details.generation != generation
                            || details.state != RemoteConnectionState::Connected
                    }) => return Err(BridgeError::ConnectionClosed),
                    result = client.read(&d.target, cursor) => result?,
                };
                let mut s = e.state.lock();
                if s.details.generation != generation {
                    return Ok(());
                }
                if read.target != d.target
                    || read.cursor < cursor
                    || read.chunks.windows(2).any(|w| w[0].cursor >= w[1].cursor)
                    || read.chunks.iter().any(|c| c.cursor > read.cursor)
                {
                    return Err(BridgeError::Protocol(
                        "Invalid remote output ordering or identity".into(),
                    ));
                }
                if read.gap {
                    hub.publish_gap(&d.backend_session_id);
                    s.details.replay_gap = Some(RemoteReplayGap {
                        requested_after_cursor: cursor,
                        available_from_cursor: read
                            .chunks
                            .first()
                            .map(|c| c.cursor)
                            .unwrap_or(read.cursor),
                    });
                }
                for chunk in read.chunks {
                    if chunk.cursor > cursor {
                        hub.publish(&d.backend_session_id, chunk.bytes);
                    }
                }
                s.details.descriptor.remote_cursor = read.cursor;
                Entry::notify(&s);
                if read.exited {
                    return Err(BridgeError::TargetNotFound);
                }
            }
        }
        .await;
        let Err(error) = outcome else {
            return;
        };
        let _gate = e.control.lock().await;
        {
            let mut s = e.state.lock();
            if s.details.generation != generation {
                return;
            }
            // Preserve a control-side terminal failure rather than replacing it with EOF.
            let failure = s
                .details
                .failure
                .clone()
                .unwrap_or_else(|| RemoteFailure::from_bridge(&error));
            fail(&mut s, failure.clone());
            if failure.kind != RemoteFailureKind::Transport {
                return;
            }
            if attempts >= MAX_ATTEMPTS {
                s.details.state = RemoteConnectionState::Disconnected;
                Entry::notify(&s);
                return;
            }
            attempts += 1;
            s.details.attempts = attempts;
            s.details.generation += 1;
            generation = s.details.generation;
            Entry::notify(&s);
        }
        drop(_gate);
        connector.delay(attempts - 1).await;
    }
}

async fn run_live(
    e: Arc<Entry>,
    hub: Arc<TerminalOutputHub>,
    _connector: Arc<dyn Connector>,
    generation: u64,
    client: Arc<dyn Transport>,
) {
    let d = {
        let mut s = e.state.lock();
        if s.details.generation != generation {
            return;
        }
        s.transport = Some(client.clone());
        s.details.state = RemoteConnectionState::Connected;
        s.details.failure = None;
        Entry::notify(&s);
        s.details.descriptor.clone()
    };

    let pending_size = {
        let mut s = e.state.lock();
        s.pending_size.take()
    };
    if let Some((cols, rows)) = pending_size {
        if client.resize(&d.target, cols, rows).await.is_ok() {
            let mut s = e.state.lock();
            if s.details.generation == generation {
                s.details.descriptor.cols = cols;
                s.details.descriptor.rows = rows;
                hub.record_resize(&d.backend_session_id, cols, rows);
                Entry::notify(&s);
            }
        }
    }

    let mut updates = e.state.lock().updates.subscribe();
    loop {
        let cursor = {
            let s = e.state.lock();
            if s.details.generation != generation {
                return;
            }
            if s.details.state != RemoteConnectionState::Connected {
                return;
            }
            s.details.descriptor.remote_cursor
        };
        let read = tokio::select! {
            biased;
            _ = async {
                let _ = updates.wait_for(|details| {
                    details.generation != generation
                        || details.state != RemoteConnectionState::Connected
                }).await;
            } => return,
            result = client.read(&d.target, cursor) => match result {
                Ok(r) => r,
                Err(error) => {
                    let _gate = e.control.lock().await;
                    let mut s = e.state.lock();
                    if s.details.generation == generation {
                        let failure = RemoteFailure::from_bridge(&error);
                        fail(&mut s, failure);
                    }
                    return;
                }
            },
        };

        let mut s = e.state.lock();
        if s.details.generation != generation {
            return;
        }
        if read.target != d.target
            || read.cursor < cursor
            || read.chunks.windows(2).any(|w| w[0].cursor >= w[1].cursor)
            || read.chunks.iter().any(|c| c.cursor > read.cursor)
        {
            let failure = RemoteFailure::new(
                RemoteFailureKind::Protocol,
                "Invalid remote output ordering or identity",
            );
            fail(&mut s, failure);
            return;
        }
        if read.gap {
            hub.publish_gap(&d.backend_session_id);
            s.details.replay_gap = Some(RemoteReplayGap {
                requested_after_cursor: cursor,
                available_from_cursor: read
                    .chunks
                    .first()
                    .map(|c| c.cursor)
                    .unwrap_or(read.cursor),
            });
        }
        for chunk in read.chunks {
            if chunk.cursor > cursor {
                hub.publish(&d.backend_session_id, chunk.bytes);
            }
        }
        s.details.descriptor.remote_cursor = read.cursor;
        Entry::notify(&s);
        if read.exited {
            let failure = RemoteFailure::new(RemoteFailureKind::Missing, "Remote target exited");
            fail(&mut s, failure);
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scoped_contracts::Epoch;
    use std::process::Stdio;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::mpsc;

    struct TestTransport {
        reads: tokio::sync::Mutex<mpsc::UnboundedReceiver<Result<ReadResult, BridgeError>>>,
        describes: AtomicUsize,
        writes: AtomicUsize,
        resizes: AtomicUsize,
        stops: AtomicUsize,
        bridge_client: Option<SshBridgeClient>,
    }

    impl Transport for TestTransport {
        fn describe<'a>(&'a self, t: &'a TargetRef) -> Rpc<'a, DescribeResult> {
            self.describes.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                Ok(DescribeResult {
                    target: t.clone(),
                    pid: RemotePid(11111),
                    cwd: "/project".into(),
                    cols: 80,
                    rows: 24,
                    cursor: RemoteCursor(0),
                    exited: false,
                })
            })
        }

        fn read<'a>(&'a self, _: &'a TargetRef, _: RemoteCursor) -> Rpc<'a, ReadResult> {
            Box::pin(async move {
                self.reads
                    .lock()
                    .await
                    .recv()
                    .await
                    .expect("channel retained")
            })
        }

        fn write<'a>(&'a self, _: &'a TargetRef, _: &'a [u8]) -> Rpc<'a, ()> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move { Ok(()) })
        }

        fn resize<'a>(&'a self, _: &'a TargetRef, _: u16, _: u16) -> Rpc<'a, ()> {
            self.resizes.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move { Ok(()) })
        }

        fn stop<'a>(&'a self, _: &'a TargetRef) -> Rpc<'a, ()> {
            self.stops.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move { Ok(()) })
        }

        fn as_ssh_bridge(&self) -> Option<&SshBridgeClient> {
            self.bridge_client.as_ref()
        }
    }

    struct NonConnectingDialer {
        calls: AtomicUsize,
    }

    impl Connector for NonConnectingDialer {
        fn connect<'a>(&'a self, _: &'a RemoteSessionDescriptor) -> Rpc<'a, Arc<dyn Transport>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                Err(BridgeError::ConnectionClosed)
            })
        }

        fn delay(&self, _: u32) -> Pin<Box<dyn Future<Output = ()> + Send>> {
            Box::pin(async {})
        }
    }

    fn sample_descriptor(session_id: &str, cursor: u64) -> RemoteSessionDescriptor {
        RemoteSessionDescriptor {
            backend_session_id: session_id.to_string(),
            target: TargetRef {
                host_id: "test-host".to_string(),
                owner_id: "test-owner".to_string(),
                epoch: Epoch(42),
                backend_session_id: "remote-target-id".to_string(),
            },
            config: RemoteSessionConfig {
                host: SshHost {
                    id: "test-host".to_string(),
                    label: "test-host".to_string(),
                    hostname: "127.0.0.1".to_string(),
                    username: None,
                    port: None,
                    identity_file: None,
                    jump_host: None,
                    source: crate::ssh::SshHostSource::Config,
                    auth_method: crate::ssh::SshAuthMethod::Agent,
                    disabled: None,
                },
                environment: RemoteEnvironment {
                    platform: crate::ssh::runtime::RemotePlatform::Posix,
                    executor: crate::ssh::runtime::RemoteExecutor::Sh,
                    version: "test".to_string(),
                    home: "/home".to_string(),
                    temp: "/tmp".to_string(),
                    git: true,
                },
                helper: HelperLocation {
                    executable: "/bin/helper".to_string(),
                    root: "/tmp".to_string(),
                },
                project_id: "proj".to_string(),
                project_path: "/proj".to_string(),
                worktree: None,
                agent_identity: None,
            },
            client_request_id: "req-123".to_string(),
            remote_cursor: RemoteCursor(cursor),
            cols: 100,
            rows: 30,
        }
    }

    #[tokio::test]
    async fn test_remote_runtime_export_and_live_import_roundtrip() {
        let hub = Arc::new(TerminalOutputHub::default());
        let dialer = Arc::new(NonConnectingDialer {
            calls: AtomicUsize::new(0),
        });
        let runtime = RemoteRuntime::with_connector(hub.clone(), dialer.clone());

        let (_tx, rx) = mpsc::unbounded_channel();
        let transport = Arc::new(TestTransport {
            reads: tokio::sync::Mutex::new(rx),
            describes: AtomicUsize::new(0),
            writes: AtomicUsize::new(0),
            resizes: AtomicUsize::new(0),
            stops: AtomicUsize::new(0),
            bridge_client: None,
        });

        // Seed session directly into runtime
        let d = sample_descriptor("sess-roundtrip", 55);
        runtime.insert(d.clone(), Some(transport.clone())).expect("insert");

        // Set specific generation and pending_size
        let entry = runtime.entry("sess-roundtrip").unwrap();
        {
            let mut s = entry.state.lock();
            s.details.generation = 9;
            s.pending_size = Some((110, 35));
            s.details.pid = Some(RemotePid(777));
        }

        // Export session
        let exported = runtime.export_for_transfer("sess-roundtrip").await.expect("export");
        assert_eq!(exported.descriptor.backend_session_id, "sess-roundtrip");
        assert_eq!(exported.descriptor.remote_cursor, RemoteCursor(55));
        assert_eq!(exported.descriptor.cols, 100);
        assert_eq!(exported.descriptor.rows, 30);
        assert_eq!(exported.generation, 9);
        assert_eq!(exported.pending_size, Some((110, 35)));
        assert_eq!(exported.pid, Some(RemotePid(777)));
        assert_eq!(exported.descriptor.target.epoch, Epoch(42));

        // Create successor runtime sharing the same output hub
        let successor_dialer = Arc::new(NonConnectingDialer {
            calls: AtomicUsize::new(0),
        });
        let successor = RemoteRuntime::with_connector(hub.clone(), successor_dialer.clone());

        // Setup imported transport
        let (tx2, rx2) = mpsc::unbounded_channel();
        let imported_transport = Arc::new(TestTransport {
            reads: tokio::sync::Mutex::new(rx2),
            describes: AtomicUsize::new(0),
            writes: AtomicUsize::new(0),
            resizes: AtomicUsize::new(0),
            stops: AtomicUsize::new(0),
            bridge_client: None,
        });

        // Live import into successor
        successor.live_import(exported, Some(imported_transport.clone())).expect("live_import");

        // Verify live import invariants:
        // 1. Never reconnected
        assert_eq!(successor_dialer.calls.load(Ordering::SeqCst), 0);
        // 2. Preserved generation and cursor exactly
        let details = successor.details("sess-roundtrip").expect("details");
        assert_eq!(details.generation, 9);
        assert_eq!(details.descriptor.remote_cursor, RemoteCursor(55));
        assert_eq!(details.state, RemoteConnectionState::Connected);
        assert_eq!(details.pid, Some(RemotePid(777)));

        // 3. Output stream resumes from exact cursor
        tx2.send(Ok(ReadResult {
            target: details.descriptor.target.clone(),
            pid: RemotePid(777),
            cwd: "/proj".into(),
            cursor: RemoteCursor(56),
            after_sequence: 0,
            gap: false,
            exited: false,
            chunks: vec![ReadChunk {
                cursor: RemoteCursor(56),
                sequence: 1,
                data_base64: "".into(),
                bytes: b"hello live import".to_vec(),
            }],
        })).unwrap();

        // Wait for update
        let mut sub = successor.subscribe("sess-roundtrip").unwrap();
        let updated = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let d = sub.borrow_and_update().clone();
                if d.descriptor.remote_cursor == RemoteCursor(56) {
                    return d;
                }
                sub.changed().await.unwrap();
            }
        }).await.expect("cursor updated");

        assert_eq!(updated.descriptor.remote_cursor, RemoteCursor(56));

        // 4. Output appeared in existing output hub
        let attachment = hub.subscribe_with_sequence("sess-roundtrip", None).expect("attachment");
        assert!(attachment.snapshot.history.windows(b"hello live import".len()).any(|w| w == b"hello live import"));
    }

    #[tokio::test]
    async fn test_remote_runtime_poisoned_bridge_export_rejected() {
        let hub = Arc::new(TerminalOutputHub::default());
        let dialer = Arc::new(NonConnectingDialer {
            calls: AtomicUsize::new(0),
        });
        let runtime = RemoteRuntime::with_connector(hub.clone(), dialer.clone());

        // Spawn a real cat child and create an SshBridgeClient
        let child1 = tokio::process::Command::new("cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let child2 = tokio::process::Command::new("cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();

        let mut ctrl = BridgeConnection::from_child(child1).unwrap();
        let rdr = BridgeConnection::from_child(child2).unwrap();

        // Poison control connection
        ctrl.poison_for_test();

        let bridge = SshBridgeClient {
            control: Arc::new(tokio::sync::Mutex::new(ctrl)),
            reader: Arc::new(tokio::sync::Mutex::new(rdr)),
            host_id: "test-host".into(),
            owner_id: "test-owner".into(),
            epoch: Epoch(1),
            handshake: HandshakeResult {
                protocol: PROTOCOL_VERSION,
                capabilities: vec!["sshHelperV1".into()],
                host_id: "test-host".into(),
                owner_id: "test-owner".into(),
                epoch: Epoch(1),
                os: "posix".into(),
                arch: "arm64".into(),
            },
        };

        let d = sample_descriptor("sess-poisoned", 0);
        let bridge_arc = Arc::new(bridge);
        runtime.insert(d, Some(bridge_arc.clone())).expect("insert");
        {
            let e = runtime.entry("sess-poisoned").unwrap();
            let mut s = e.state.lock();
            s.transport = Some(bridge_arc);
        }

        // Export must fail because connection is poisoned
        let err = runtime.export_for_transfer("sess-poisoned").await.unwrap_err();
        assert_eq!(err.kind, RemoteFailureKind::Protocol);
    }

    #[tokio::test]
    async fn test_remote_runtime_drop_after_detach_does_not_kill_child() {
        let hub = Arc::new(TerminalOutputHub::default());
        let dialer = Arc::new(NonConnectingDialer {
            calls: AtomicUsize::new(0),
        });
        let runtime = RemoteRuntime::with_connector(hub.clone(), dialer.clone());

        let child1 = tokio::process::Command::new("cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let child2 = tokio::process::Command::new("cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();

        let pid1 = child1.id().unwrap();
        let pid2 = child2.id().unwrap();

        let ctrl = BridgeConnection::from_child(child1).unwrap();
        let rdr = BridgeConnection::from_child(child2).unwrap();

        let bridge = SshBridgeClient {
            control: Arc::new(tokio::sync::Mutex::new(ctrl)),
            reader: Arc::new(tokio::sync::Mutex::new(rdr)),
            host_id: "test-host".into(),
            owner_id: "test-owner".into(),
            epoch: Epoch(1),
            handshake: HandshakeResult {
                protocol: PROTOCOL_VERSION,
                capabilities: vec!["sshHelperV1".into()],
                host_id: "test-host".into(),
                owner_id: "test-owner".into(),
                epoch: Epoch(1),
                os: "posix".into(),
                arch: "arm64".into(),
            },
        };

        let d = sample_descriptor("sess-detach", 0);
        let bridge_arc = Arc::new(bridge);
        runtime.insert(d, Some(bridge_arc.clone())).expect("insert");
        {
            let e = runtime.entry("sess-detach").unwrap();
            let mut s = e.state.lock();
            s.transport = Some(bridge_arc);
        }

        // Detach without kill
        runtime.detach_without_kill("sess-detach").await.expect("detach");

        // Drop runtime
        drop(runtime);

        // Wait a short duration
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Both children must STILL be alive
        #[cfg(unix)]
        {
            assert_eq!(unsafe { libc::kill(pid1 as i32, 0) }, 0);
            assert_eq!(unsafe { libc::kill(pid2 as i32, 0) }, 0);
            unsafe {
                libc::kill(pid1 as i32, libc::SIGKILL);
                libc::kill(pid2 as i32, libc::SIGKILL);
            }
        }
    }
}
