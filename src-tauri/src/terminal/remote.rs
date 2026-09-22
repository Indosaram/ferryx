//! Same-target remote terminal runtime. SSH child ownership remains in `ssh::bridge`.
use super::output_hub::TerminalOutputHub;
use crate::scoped_contracts::TargetRef;
use crate::ssh::{bridge::*, helper_setup::HelperLocation, runtime::RemoteEnvironment, SshHost};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::{Arc, Weak},
    time::Duration,
};
use tokio::sync::{mpsc, oneshot, watch, Mutex as AsyncMutex};

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
                let stage = details
                    .and_then(|d| d.get("stage"))
                    .and_then(|v| v.as_str());
                if stage == Some("helper_missing")
                    || error.code == crate::ipc::IpcErrorCode::CliExecutableNotFound
                {
                    RemoteFailureKind::Missing
                } else if error.code != crate::ipc::IpcErrorCode::IoError {
                    RemoteFailureKind::Protocol
                } else if stage == Some("transport") {
                    RemoteFailureKind::Transport
                } else if stage == Some("execution")
                    && details
                        .and_then(|d| d.get("exitCode"))
                        .and_then(|v| v.as_i64())
                        == Some(255)
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
            | BridgeError::TargetMismatch { .. } => RemoteFailureKind::Protocol,
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

// Internal seam keeps deterministic tests at the actual controller boundary.
pub(crate) trait Transport: Send + Sync {
    fn describe<'a>(&'a self, target: &'a TargetRef) -> Rpc<'a, DescribeResult>;
    fn read<'a>(&'a self, target: &'a TargetRef, cursor: RemoteCursor) -> Rpc<'a, ReadResult>;
    fn write<'a>(&'a self, target: &'a TargetRef, bytes: &'a [u8]) -> Rpc<'a, ()>;
    fn resize<'a>(&'a self, target: &'a TargetRef, cols: u16, rows: u16) -> Rpc<'a, ()>;
    fn stop<'a>(&'a self, target: &'a TargetRef) -> Rpc<'a, ()>;
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
    pending_size: Option<(u16, u16, u64)>,
}
const MAX_PENDING_OPERATIONS: usize = 17;
const MAX_PENDING_BYTES: usize = 256 * 1024;
pub(crate) const INPUT_QUEUE_TIMEOUT: Duration = Duration::from_secs(5);

struct QueuePermits {
    allocated_entries: usize,
    allocated_bytes: usize,
}

enum RemoteCommandKind {
    Write(Vec<u8>),
    Resize(u16, u16),
}

struct RemoteCommand {
    generation: u64,
    dispatch_deadline: tokio::time::Instant,
    kind: RemoteCommandKind,
    bytes: usize,
    reply: oneshot::Sender<Result<(), RemoteFailure>>,
}

pub(crate) struct Entry {
    pub(crate) state: Mutex<Session>,
    control: Arc<AsyncMutex<()>>,
    queue_tx: mpsc::UnboundedSender<RemoteCommand>,
    queue_permits: Arc<parking_lot::Mutex<QueuePermits>>,
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
        if map
            .values()
            .any(|entry| entry.state.lock().details.descriptor.target == d.target)
        {
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
        let (queue_tx, queue_rx) = mpsc::unbounded_channel();
        let permits = Arc::new(parking_lot::Mutex::new(QueuePermits {
            allocated_entries: 0,
            allocated_bytes: 0,
        }));
        let entry = Arc::new(Entry {
            state: Mutex::new(Session {
                details,
                updates,
                transport: None,
                task: None,
                pending_size: None,
            }),
            control: Arc::new(AsyncMutex::new(())),
            queue_tx,
            queue_permits: permits,
        });
        tokio::spawn(queue_worker(
            queue_rx,
            Arc::downgrade(&entry),
            self.hub.clone(),
        ));
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
    #[cfg(test)]
    pub(crate) fn hold_dispatch_gate_for_test(
        &self,
        id: &str,
    ) -> Result<tokio::sync::OwnedMutexGuard<()>, RemoteFailure> {
        self.entry(id)?
            .control
            .clone()
            .try_lock_owned()
            .map_err(|_| {
                RemoteFailure::new(
                    RemoteFailureKind::Busy,
                    "test dispatch gate is already held",
                )
            })
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
            .ok_or_else(|| {
                RemoteFailure::new(RemoteFailureKind::Missing, "Session not found in hub")
            })?;
        Ok((attachment, generation))
    }

    /// Ranges variant of [`Self::attach_snapshot_with_generation`]: returns history segment
    /// byte offsets into the snapshot's flat history instead of materialized segment buffers.
    pub fn attach_snapshot_with_generation_ranges(
        &self,
        id: &str,
        after_sequence: Option<u64>,
    ) -> Result<
        (
            crate::terminal::output_hub::SessionAttachment,
            Vec<crate::terminal::output_hub::HistoryRange>,
            u64,
        ),
        RemoteFailure,
    > {
        let e = self.entry(id)?;
        let s = e.state.lock();
        let generation = s.details.generation;
        let (attachment, ranges) = self
            .hub
            .subscribe_with_sequence_ranges(id, after_sequence)
            .ok_or_else(|| {
                RemoteFailure::new(RemoteFailureKind::Missing, "Session not found in hub")
            })?;
        Ok((attachment, ranges, generation))
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

        {
            let mut s = e.state.lock();
            if let Err(error) = check_connected(&s, generation) {
                if let Some((cols, rows)) = size {
                    // Only a same-generation outage may retain the desired geometry:
                    // a stale-generation op must never seed pending_size, otherwise
                    // its old geometry is replayed onto the next connection. The
                    // queue-worker dispatch path applies the same rule.
                    if error.kind == RemoteFailureKind::Disconnected {
                        let overwrite = s
                            .pending_size
                            .as_ref()
                            .map_or(true, |&(_, _, gen)| generation >= gen);
                        if overwrite {
                            s.pending_size = Some((cols, rows, generation));
                        }
                    }
                }
                return Err(error);
            }
        }

        let payload_bytes = bytes.as_ref().map(|b| b.len()).unwrap_or(32);
        let (reply_tx, reply_rx) = oneshot::channel();
        let kind = match (bytes, size) {
            (Some(b), _) => RemoteCommandKind::Write(b),
            (_, Some((c, r))) => RemoteCommandKind::Resize(c, r),
            _ => unreachable!(),
        };

        let cmd = RemoteCommand {
            generation,
            dispatch_deadline: tokio::time::Instant::now() + INPUT_QUEUE_TIMEOUT,
            kind,
            bytes: payload_bytes,
            reply: reply_tx,
        };

        {
            let mut permits = e.queue_permits.lock();
            if permits.allocated_entries >= MAX_PENDING_OPERATIONS
                || permits.allocated_bytes + payload_bytes > MAX_PENDING_BYTES
            {
                return Err(RemoteFailure::new(
                    RemoteFailureKind::Busy,
                    "Remote control queue is full",
                ));
            }
            if e.queue_tx.send(cmd).is_err() {
                return Err(RemoteFailure::new(
                    RemoteFailureKind::Disconnected,
                    "Remote queue worker dropped",
                ));
            }
            permits.allocated_entries += 1;
            permits.allocated_bytes += payload_bytes;
        }

        Ok(Box::pin(async move {
            match reply_rx.await {
                Ok(res) => res,
                Err(_) => Err(RemoteFailure::new(
                    RemoteFailureKind::Disconnected,
                    "Remote terminal operation cancelled",
                )),
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
                {
                    let mut s = e.state.lock();
                    if s.details.generation != generation {
                        return Ok(());
                    }
                    s.transport = Some(client.clone());
                    s.details.pid = Some(info.pid);
                    s.details.state = RemoteConnectionState::Connected;
                    s.details.failure = None;
                    desired_size = Some(
                        s.pending_size
                            .take()
                            .map(|(c, r, _)| (c, r))
                            .unwrap_or((s.details.descriptor.cols, s.details.descriptor.rows)),
                    );
                    Entry::notify(&s);
                }
                // Converge the remote PTY onto the last size the daemon knows about
                // before streaming resumes: a reconnect must not leave the remote shell
                // at stale spawn defaults while the pane renders a different grid.
                // This runs while the control gate is still held (state lock released)
                // so a concurrently dispatched newer resize cannot be overwritten by
                // this older desired size, and a failed convergence restores the
                // desired size instead of dropping it.
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
                    } else {
                        let mut s = e.state.lock();
                        if s.details.generation == generation {
                            let restore = s
                                .pending_size
                                .as_ref()
                                .map_or(true, |&(_, _, gen)| generation >= gen);
                            if restore {
                                s.pending_size = Some((cols, rows, generation));
                            }
                        }
                    }
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
        // The outage transition must not wait behind the dispatch gate: an in-flight
        // transport write on a dying connection may hold it for the full inner budget,
        // which would stall session state, reconnect scheduling, and queued-input
        // rejection. Correctness is preserved by the state lock plus the dispatch
        // re-check (`check_connected`) every queued op performs under that lock.
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
        connector.delay(attempts - 1).await;
    }
}

async fn queue_worker(
    mut rx: mpsc::UnboundedReceiver<RemoteCommand>,
    entry_weak: Weak<Entry>,
    hub: Arc<TerminalOutputHub>,
) {
    while let Some(cmd) = rx.recv().await {
        let Some(e) = entry_weak.upgrade() else {
            break;
        };

        struct PermitGuard {
            permits: Arc<parking_lot::Mutex<QueuePermits>>,
            bytes: usize,
        }
        impl Drop for PermitGuard {
            fn drop(&mut self) {
                let mut p = self.permits.lock();
                p.allocated_entries = p.allocated_entries.saturating_sub(1);
                p.allocated_bytes = p.allocated_bytes.saturating_sub(self.bytes);
            }
        }
        let _guard = PermitGuard {
            permits: e.queue_permits.clone(),
            bytes: cmd.bytes,
        };

        if cmd.reply.is_closed() {
            continue;
        }

        let gate = tokio::time::timeout_at(cmd.dispatch_deadline, e.control.lock()).await;
        let Ok(_gate) = gate else {
            let _ = cmd.reply.send(Err(RemoteFailure::new(
                RemoteFailureKind::Busy,
                "Remote input queue deadline exceeded; input was not sent",
            )));
            continue;
        };
        if tokio::time::Instant::now() >= cmd.dispatch_deadline {
            let _ = cmd.reply.send(Err(RemoteFailure::new(
                RemoteFailureKind::Busy,
                "Remote input queue deadline exceeded; input was not sent",
            )));
            continue;
        }
        if cmd.reply.is_closed() {
            continue;
        }

        let (client, target) = {
            let mut s = e.state.lock();
            if let Err(error) = check_connected(&s, cmd.generation) {
                if let RemoteCommandKind::Resize(cols, rows) = cmd.kind {
                    // Only a same-generation outage may retain the desired geometry:
                    // a stale-generation op must never seed pending_size, otherwise
                    // its old geometry is replayed onto the next connection and can
                    // regress geometry the newer generation already applied.
                    if error.kind == RemoteFailureKind::Disconnected {
                        let overwrite = s
                            .pending_size
                            .as_ref()
                            .map_or(true, |&(_, _, gen)| cmd.generation >= gen);
                        if overwrite {
                            s.pending_size = Some((cols, rows, cmd.generation));
                        }
                    }
                }
                let _ = cmd.reply.send(Err(error));
                continue;
            }
            (
                s.transport.clone().unwrap(),
                s.details.descriptor.target.clone(),
            )
        };

        let result = match cmd.kind {
            RemoteCommandKind::Write(ref bytes) => client.write(&target, bytes).await,
            RemoteCommandKind::Resize(cols, rows) => client.resize(&target, cols, rows).await,
        };

        let mut s = e.state.lock();
        let outcome = match result {
            Ok(()) => {
                if let RemoteCommandKind::Resize(cols, rows) = cmd.kind {
                    s.details.descriptor.cols = cols;
                    s.details.descriptor.rows = rows;
                    let clear = s
                        .pending_size
                        .as_ref()
                        .map_or(true, |&(_, _, gen)| cmd.generation >= gen);
                    if clear {
                        s.pending_size = None;
                    }
                    hub.record_resize(&s.details.descriptor.backend_session_id, cols, rows);
                    Entry::notify(&s);
                }
                Ok(())
            }
            Err(error) => {
                let failure = RemoteFailure::from_bridge(&error);
                if let RemoteCommandKind::Resize(cols, rows) = cmd.kind {
                    let overwrite = s
                        .pending_size
                        .as_ref()
                        .map_or(true, |&(_, _, gen)| cmd.generation >= gen);
                    if overwrite {
                        s.pending_size = Some((cols, rows, cmd.generation));
                    }
                }
                fail(&mut s, failure.clone());
                Err(failure)
            }
        };

        let _ = cmd.reply.send(outcome);
    }
}
