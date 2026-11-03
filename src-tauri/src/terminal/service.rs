use crate::daemon::session_lifecycle::{SessionLifecycleRegistry, SessionProcessState};
use crate::terminal::output_hub::{HistoryRange, SessionAttachment, TerminalOutputHub};
use crate::terminal::{PtyError, PtyManager, PtySession, TerminalSignal};
use crate::worktree::manager::WorktreeManager;
use parking_lot::Mutex;
use portable_pty::CommandBuilder;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::{broadcast, watch};

/// How long the output of a PTY that ended before its first attach is kept for that attach.
const UNATTACHED_EXIT_OUTPUT_RETENTION: std::time::Duration = std::time::Duration::from_secs(30);

/// Ends a local session's output stream at PTY EOF. A command that exited before anyone
/// attached keeps its output for the first attach (which a spawn's caller issues right after
/// the spawn returns), bounded by `UNATTACHED_EXIT_OUTPUT_RETENTION`.
fn end_output(output_hub: &Arc<TerminalOutputHub>, session_id: &str) {
    if output_hub.end_session(session_id) {
        return;
    }
    let output_hub = Arc::clone(output_hub);
    let session_id = session_id.to_string();
    tokio::spawn(async move {
        tokio::time::sleep(UNATTACHED_EXIT_OUTPUT_RETENTION).await;
        output_hub.remove_ended_session(&session_id);
    });
}

/// Writes OSC 7501 support query replies the session's screen mirror produced to its PTY, on a
/// blocking worker so the output pump never waits for the PTY input queue.
fn spawn_program_status_replies(
    pty_manager: &Arc<PtyManager>,
    session_id: &str,
    replies: Vec<Vec<u8>>,
) {
    if replies.is_empty() {
        return;
    }
    let pty_manager = Arc::clone(pty_manager);
    let session_id = session_id.to_string();
    tokio::task::spawn_blocking(move || {
        for reply in replies {
            if let Err(error) = pty_manager.write_input(&session_id, &reply) {
                tracing::debug!(session_id, %error, "program status query reply not written");
                return;
            }
        }
    });
}

#[derive(Clone)]
pub struct TerminalService {
    pty_manager: Arc<PtyManager>,
    output_hub: Arc<TerminalOutputHub>,
    remote: Arc<super::remote::RemoteRuntime>,
    paired: Arc<super::paired_runtime::Runtime>,
    lifecycle: Arc<Mutex<SessionLifecycleRegistry>>,
    pastes: Arc<PasteRegistry>,
}

/// How long `cancel_paste` waits for the chunk in flight to stop at its next await point.
const PASTE_CANCEL_SETTLE_BOUND: std::time::Duration = std::time::Duration::from_secs(2);
/// Idle paste records (completed, failed or cancelled) are kept this long so a late
/// `CancelPaste` still reports the exact count and a late chunk of a cancelled paste is refused.
const PASTE_RECORD_RETENTION: std::time::Duration = std::time::Duration::from_secs(600);
/// Upper bound on retained paste records across all sessions.
const MAX_PASTE_RECORDS: usize = 1024;

/// Outcome of `TerminalService::cancel_paste`. `bytes_written` is the exact count of the
/// paste's bytes the kernel accepted across all of its chunks when `settled`, else a lower bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PasteCancelOutcome {
    pub found: bool,
    pub bytes_written: u64,
    pub settled: bool,
}

/// Failure of one `PasteWrite` chunk.
#[derive(Debug)]
pub enum PasteChunkError {
    /// Refused (paste already cancelled) or stopped by `CancelPaste`; `bytes_written` is the
    /// exact count of this chunk's bytes the kernel accepted.
    Cancelled { bytes_written: usize },
    /// The daemon holds no record of this paste; nothing was written.
    Unknown,
    Failed(PtyError),
}

impl From<PasteChunkError> for crate::ipc::IpcError {
    fn from(error: PasteChunkError) -> Self {
        match error {
            PasteChunkError::Cancelled { bytes_written } => Self::new(
                crate::ipc::IpcErrorCode::Custom(crate::daemon::protocol::PASTE_CANCELLED_CODE.into()),
                "Paste cancelled",
            )
            .with_details(serde_json::json!({
                "pasteCancelled": true,
                "inputWritten": bytes_written > 0,
                "bytesWritten": bytes_written,
                "bytesWrittenExact": true,
                "partial": bytes_written > 0,
            })),
            PasteChunkError::Unknown => Self::new(
                crate::ipc::IpcErrorCode::Custom(crate::daemon::protocol::PASTE_UNKNOWN_CODE.into()),
                "Paste unknown to this daemon",
            )
            .with_details(serde_json::json!({
                "pasteUnknown": true,
                "inputWritten": false,
                "bytesWritten": 0,
                "bytesWrittenExact": true,
                "partial": false,
            })),
            PasteChunkError::Failed(error) => Self::from(error),
        }
    }
}

struct PasteRecord {
    cancel: tokio_util::sync::CancellationToken,
    /// Bytes of this paste the kernel accepted, advanced by the writer as each syscall lands,
    /// so it is exact even when a chunk's future is dropped mid-write.
    accepted: Arc<std::sync::atomic::AtomicU64>,
    completed: bool,
    /// true while no chunk of this paste is in flight.
    idle: watch::Sender<bool>,
    touched: std::time::Instant,
}

impl PasteRecord {
    fn new() -> Self {
        Self {
            cancel: tokio_util::sync::CancellationToken::new(),
            accepted: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            completed: false,
            idle: watch::channel(true).0,
            touched: std::time::Instant::now(),
        }
    }
}

/// Identified pastes, keyed by (session id, paste id). Only `PasteWrite`/`CancelPaste` consult
/// it; ordinary input never does, so no ordinary byte can be refused or swallowed by it.
#[derive(Default)]
struct PasteRegistry {
    records: Mutex<std::collections::HashMap<(String, String), PasteRecord>>,
}

impl PasteRegistry {
    fn begin(&self, session_id: &str) -> Result<String, PtyError> {
        let mut records = self.records.lock();
        Self::prune(&mut records, MAX_PASTE_RECORDS - 1);
        if records.len() >= MAX_PASTE_RECORDS {
            return Err(PtyError::InputError { bytes_written: 0, message: "PASTE_REGISTRY_FULL".into() });
        }
        let paste_id = uuid::Uuid::new_v4().to_string();
        records.insert((session_id.to_string(), paste_id.clone()), PasteRecord::new());
        Ok(paste_id)
    }

    /// Drops expired idle records, then the oldest idle records beyond the requested limit.
    /// Only registration reserves an extra slot. In-flight records are never dropped.
    fn prune(records: &mut std::collections::HashMap<(String, String), PasteRecord>, limit: usize) {
        records.retain(|_, record| !*record.idle.borrow() || record.touched.elapsed() < PASTE_RECORD_RETENTION);
        if records.len() <= limit {
            return;
        }
        let mut idle: Vec<_> = records
            .iter()
            .filter(|(_, record)| *record.idle.borrow())
            .map(|(key, record)| (record.touched, key.clone()))
            .collect();
        idle.sort_by_key(|(touched, _)| *touched);
        let excess = records.len() - limit;
        for (_, key) in idle.into_iter().take(excess) {
            records.remove(&key);
        }
    }

    /// Forgets every paste of a session that is going away; its in-flight chunk (if any) is
    /// cancelled so it stops at its next await point instead of holding the closing PTY.
    fn forget_session(&self, session_id: &str) {
        self.records.lock().retain(|(id, _), record| {
            if id == session_id {
                record.cancel.cancel();
                false
            } else {
                true
            }
        });
    }
}

impl Default for TerminalService {
    fn default() -> Self {
        Self::new(
            Arc::new(PtyManager::new()),
            Arc::new(TerminalOutputHub::default()),
        )
    }
}

impl TerminalService {
    #[cfg(all(test, unix))]
    pub(crate) fn with_remote_connector_for_test(connector: Arc<dyn super::remote::Connector>) -> Self {
        let mut service = Self::default();
        service.remote = Arc::new(super::remote::RemoteRuntime::with_connector(
            service.output_hub.clone(), connector,
        ));
        service
    }

    pub fn new(pty_manager: Arc<PtyManager>, output_hub: Arc<TerminalOutputHub>) -> Self {
        pty_manager.set_output_hub(output_hub.clone());
        let service = Self {
            remote: Arc::new(super::remote::RemoteRuntime::new(output_hub.clone())),
            paired: Arc::new(super::paired_runtime::Runtime::default()),
            pty_manager,
            output_hub,
            lifecycle: Arc::new(Mutex::new(SessionLifecycleRegistry::default())),
            pastes: Arc::new(PasteRegistry::default()),
        };
        // The daemon owns the PTYs and their output hub, so the private QA
        // channel is installed here as well; without the runner env this is a
        // no-op and the daemon keeps no QA surface.
        #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
        super::qa_liveness::start(service.clone());
        #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
        super::qa_liveness::ensure_local_owner_control();
        service
    }

    pub fn paired(&self) -> &Arc<super::paired_runtime::Runtime> {
        &self.paired
    }

    pub(crate) fn set_paired_agent_sink(&self, sink: Arc<dyn super::remote::AgentStateSink>) {
        self.paired.set_agent_sink(sink);
    }

    pub fn remote(&self) -> &Arc<super::remote::RemoteRuntime> {
        &self.remote
    }

    /// Resource state of a daemon-owned process. Hibernated entries intentionally
    /// outlive PTY teardown so callers can distinguish suspension from never-spawned state.
    pub fn process_state(&self, session_id: &str) -> Option<SessionProcessState> {
        self.lifecycle.lock().state(session_id)
    }

    pub fn suspension_receipt(&self, session_id: &str) -> Option<super::ActuationReceipt> {
        self.get_session(session_id).and_then(|session| session.suspension_receipt())
    }

    /// Generation-aware admission. The returned future must be awaited for delivery status.
    pub fn write_input_operation(
        &self,
        id: &str,
        generation: u64,
        data: Vec<u8>,
    ) -> Result<futures_util::future::BoxFuture<'static, Result<(), PtyError>>, PtyError> {
        if super::paired_runtime::Runtime::owns(id) {
            let pending = self.paired.write(id, generation, data)?;
            return Ok(Box::pin(async move { pending.await.map_err(PtyError::RemoteInput) }));
        }
        if self.remote.contains(id) {
            let pending = self
                .remote
                .write(id, generation, data)
                .map_err(|e| PtyError::Other(e.to_string()))?;
            return Ok(Box::pin(async move { pending.await.map_err(PtyError::RemoteInput) }));
        }
        // Resolve session synchronously so missing IDs fail immediately with SessionNotFound.
        let session = self
            .pty_manager
            .get_session(id)
            .ok_or_else(|| PtyError::SessionNotFound(id.to_string()))?;

        // Perform the write asynchronously off the Tokio worker thread to avoid blocking
        // the reactor when the PTY input queue is full.
        #[cfg(any(unix, windows))]
        {
            const MAX_CHUNK_SIZE: usize = 65536;
            Ok(Box::pin(async move {
                // Chunks are written strictly in order, each under the session's input gate.
                // The gate is re-taken per chunk, so a second connection writing to the same
                // session can interleave only at a 64 KiB boundary.
                for (index, chunk) in data.chunks(MAX_CHUNK_SIZE).enumerate() {
                    session
                        .write_input_cancellable(chunk)
                        .await
                        .map_err(|error| match error {
                            PtyError::InputError { bytes_written, message } => PtyError::InputError {
                                bytes_written: index * MAX_CHUNK_SIZE + bytes_written,
                                message,
                            },
                            error => error,
                        })?;
                }
                Ok(())
            }))
        }

        #[cfg(not(any(unix, windows)))]
        {
            Ok(Box::pin(async move {
                tokio::task::spawn_blocking(move || session.write_input(&data))
                    .await
                    .map_err(|e| PtyError::Other(format!("PTY write task panicked: {e}")))?
            }))
        }
    }

    /// Allocates a server-owned paste identity. An absent identity is never recreated by a
    /// chunk or cancellation, including after eviction, expiry, or daemon handover.
    pub fn begin_paste(&self, id: &str) -> Result<String, PtyError> {
        if self.pty_manager.get_session(id).is_none() {
            return Err(PtyError::SessionNotFound(id.to_string()));
        }
        self.pastes.begin(id)
    }

    /// Writes one chunk of the identified paste `paste_id`. Local PTY chunks run under the
    /// paste's cancellation token, so `cancel_paste` stops the chunk in flight and every later
    /// chunk is refused. Remote and paired sessions keep ordinary write semantics.
    pub async fn write_paste_chunk(
        &self,
        id: &str,
        generation: u64,
        paste_id: &str,
        data: Vec<u8>,
        final_chunk: bool,
    ) -> Result<(), PasteChunkError> {
        #[cfg(any(unix, windows))]
        {
            if !super::paired_runtime::Runtime::owns(id) && !self.remote.contains(id) {
                let session = self
                    .pty_manager
                    .get_session(id)
                    .ok_or_else(|| PasteChunkError::Failed(PtyError::SessionNotFound(id.to_string())))?;
                return self.write_local_paste_chunk(&session, id, paste_id, &data, final_chunk).await;
            }
        }
        let _ = (paste_id, final_chunk);
        self.write_input_operation(id, generation, data)
            .map_err(PasteChunkError::Failed)?
            .await
            .map_err(PasteChunkError::Failed)
    }

    #[cfg(any(unix, windows))]
    async fn write_local_paste_chunk(
        &self,
        session: &PtySession,
        id: &str,
        paste_id: &str,
        data: &[u8],
        final_chunk: bool,
    ) -> Result<(), PasteChunkError> {
        const MAX_CHUNK_SIZE: usize = 65536;
        let key = (id.to_string(), paste_id.to_string());
        let (token, progress, idle) = {
            let mut records = self.pastes.records.lock();
            PasteRegistry::prune(&mut records, MAX_PASTE_RECORDS);
            let record = records.get_mut(&key).ok_or(PasteChunkError::Unknown)?;
            record.touched = std::time::Instant::now();
            if record.cancel.is_cancelled() {
                return Err(PasteChunkError::Cancelled { bytes_written: 0 });
            }
            if record.completed || !*record.idle.borrow() {
                return Err(PasteChunkError::Failed(PtyError::InputError {
                    bytes_written: 0,
                    message: "PASTE_CHUNK_OUT_OF_ORDER".into(),
                }));
            }
            record.idle.send_replace(false);
            (record.cancel.clone(), Arc::clone(&record.accepted), record.idle.clone())
        };
        // Restores the idle flag even if this future is dropped, so a waiting cancel never
        // outlives its bound because of a vanished chunk. Byte accounting lives in `progress`.
        struct Settle<'a> { registry: &'a PasteRegistry, key: (String, String), done: bool, idle: watch::Sender<bool> }
        impl Drop for Settle<'_> {
            fn drop(&mut self) {
                if let Some(record) = self.registry.records.lock().get_mut(&self.key) {
                    record.completed |= self.done;
                    record.touched = std::time::Instant::now();
                }
                self.idle.send_replace(true);
            }
        }
        let mut settle = Settle { registry: &self.pastes, key, done: false, idle };
        let start = progress.load(std::sync::atomic::Ordering::Acquire);
        let in_request = || (progress.load(std::sync::atomic::Ordering::Acquire) - start) as usize;
        for chunk in data.chunks(MAX_CHUNK_SIZE) {
            match session.write_input_until(chunk, &token, &progress).await {
                Ok(()) => {}
                Err(super::session::InputStop::Cancelled { .. }) => {
                    return Err(PasteChunkError::Cancelled { bytes_written: in_request() });
                }
                Err(super::session::InputStop::Failed(PtyError::InputError { message, .. })) => {
                    return Err(PasteChunkError::Failed(PtyError::InputError { bytes_written: in_request(), message }));
                }
                Err(super::session::InputStop::Failed(error)) => return Err(PasteChunkError::Failed(error)),
            }
        }
        settle.done = final_chunk;
        Ok(())
    }

    /// Cancels the paste `paste_id` on `id`: the chunk in flight stops at its next await point
    /// and every later chunk is refused. An unknown paste remains unknown and cannot be
    /// written; its previous accepted count is unavailable. Never signals or flushes the tty.
    pub async fn cancel_paste(&self, id: &str, paste_id: &str) -> PasteCancelOutcome {
        let key = (id.to_string(), paste_id.to_string());
        let (found, mut idle, progress) = {
            let mut records = self.pastes.records.lock();
            let found = records.contains_key(&key);
            let Some(record) = records.get_mut(&key) else {
                return PasteCancelOutcome { found: false, bytes_written: 0, settled: false };
            };
            record.touched = std::time::Instant::now();
            record.cancel.cancel();
            (found, record.idle.subscribe(), Arc::clone(&record.accepted))
        };
        let settled = tokio::time::timeout(PASTE_CANCEL_SETTLE_BOUND, idle.wait_for(|idle| *idle))
            .await
            .is_ok_and(|result| result.is_ok());
        let bytes_written = progress.load(std::sync::atomic::Ordering::Acquire);
        PasteCancelOutcome { found, bytes_written, settled }
    }

    pub fn resize_operation(
        &self,
        id: &str,
        generation: u64,
        cols: u16,
        rows: u16,
    ) -> Result<super::remote::RemoteOperation, PtyError> {
        if super::paired_runtime::Runtime::owns(id) {
            return self.paired.resize(id, generation, cols, rows);
        }
        if self.remote.contains(id) {
            return self
                .remote
                .resize(id, generation, cols, rows)
                .map_err(|e| PtyError::Other(e.to_string()));
        }
        self.resize(id, cols, rows)?;
        Ok(Box::pin(async { Ok(()) }))
    }

    pub fn pty_manager(&self) -> &Arc<PtyManager> {
        &self.pty_manager
    }

    #[cfg(test)]
    pub(crate) fn try_acquire_cwd_probe(
        &self,
        session_id: &str,
    ) -> Option<tokio::sync::OwnedSemaphorePermit> {
        self.pty_manager.try_acquire_cwd_probe(session_id)
    }

    pub fn output_hub(&self) -> &Arc<TerminalOutputHub> {
        &self.output_hub
    }

    pub fn spawn_shell(
        &self,
        cols: u16,
        rows: u16,
    ) -> Result<(String, watch::Receiver<()>), PtyError> {
        let (session_id, pty_rx) = self.pty_manager.spawn_shell(cols, rows)?;
        Ok(self.register_output(session_id, pty_rx, cols, rows))
    }

    pub fn spawn_in_worktree(
        &self,
        cmd: CommandBuilder,
        cols: u16,
        rows: u16,
        worktree_manager: &WorktreeManager,
        worktree_path: &Path,
    ) -> Result<(String, watch::Receiver<()>), PtyError> {
        let (session_id, pty_rx) =
            self.pty_manager
                .spawn_in_worktree(cmd, cols, rows, worktree_manager, worktree_path)?;
        Ok(self.register_output(session_id, pty_rx, cols, rows))
    }

    pub(crate) fn spawn_in_worktree_with_id(
        &self,
        session_id: String,
        cmd: CommandBuilder,
        cols: u16,
        rows: u16,
        worktree_manager: &WorktreeManager,
        worktree_path: &Path,
    ) -> Result<(String, watch::Receiver<()>), PtyError> {
        let (session_id, pty_rx) = self.pty_manager.spawn_in_worktree_with_id(
            session_id,
            cmd,
            cols,
            rows,
            worktree_manager,
            worktree_path,
        )?;
        Ok(self.register_output(session_id, pty_rx, cols, rows))
    }

    #[cfg(test)]
    pub(crate) fn spawn_resolved_with_id(
        &self,
        session_id: String,
        cmd: CommandBuilder,
        cols: u16,
        rows: u16,
        context: super::pty::ResolvedSpawnContext,
    ) -> Result<(String, watch::Receiver<()>), PtyError> {
        let (session_id, pty_rx) = self
            .pty_manager
            .spawn_resolved_with_id(session_id, cmd, cols, rows, context)?;
        Ok(self.register_output(session_id, pty_rx, cols, rows))
    }

    pub fn spawn_ssh(
        &self,
        host: &crate::ssh::SshHost,
        environment: &crate::ssh::runtime::RemoteEnvironment,
        remote_root: &str,
        cols: u16,
        rows: u16,
        state_endpoint: Option<&crate::ssh::state_bridge::StateEndpoint>,
    ) -> Result<(String, watch::Receiver<()>), PtyError> {
        let session_id = uuid::Uuid::new_v4().to_string();
        // With the integration turned off the pane gets no socket forward and no state endpoint.
        let agent_sock = crate::daemon::server::agent_state_socket_path();
        let has_sock = std::path::Path::new(&agent_sock).exists();
        let (agent_socket, state_endpoint) = crate::ssh::operations::integration_forwarding(
            self.pty_manager.agent_integration_enabled(),
            if has_sock { Some(&agent_sock) } else { None },
            state_endpoint,
        );
        let plan = crate::ssh::operations::shell_plan(
            host,
            environment,
            remote_root,
            &session_id,
            agent_socket,
            state_endpoint,
        )
        .map_err(|e| PtyError::Other(e.to_string()))?;
        let mut cmd = CommandBuilder::new(&plan.program);
        cmd.args(&plan.args);
        for (key, value) in crate::ssh::password::environment(&plan.args)
            .map_err(|e| PtyError::Other(e.to_string()))?
        {
            cmd.env(key, value);
        }
        // No remote path is ever used as the local SSH process working directory.
        let pty_rx = self
            .pty_manager
            .spawn_with_id(session_id.clone(), cmd, cols, rows)?;

        Ok(self.register_output(session_id, pty_rx, cols, rows))
    }

    fn register_output(
        &self,
        session_id: String,
        mut pty_rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
        cols: u16,
        rows: u16,
    ) -> (String, watch::Receiver<()>) {
        self.lifecycle.lock().mark_running(session_id.clone());
        let _raw_rx = self.output_hub.register_session(&session_id);
        drop(_raw_rx);
        self.output_hub.record_initial_size(&session_id, cols, rows);
        // A local PTY: this daemon is the one terminal that answers its OSC 7501 query.
        self.output_hub.enable_program_status(&session_id);

        let (lifecycle_tx, lifecycle_rx) = watch::channel(());
        // Spawn output pump task from PTY reader to OutputHub
        let output_hub = Arc::clone(&self.output_hub);
        let pty_manager = Arc::clone(&self.pty_manager);
        let lifecycle = Arc::clone(&self.lifecycle);
        let session_id_clone = session_id.clone();
        tokio::spawn(async move {
            let _lifecycle_tx = lifecycle_tx;
            while let Some(chunk) = pty_rx.recv().await {
                let read_unix_micros = crate::terminal::metrics::take_pty_read_timestamp(
                    &session_id_clone,
                    chunk.len(),
                );
                if let Some((_chunk, replies)) =
                    output_hub.publish_with_replies(&session_id_clone, chunk, read_unix_micros)
                {
                    spawn_program_status_replies(&pty_manager, &session_id_clone, replies);
                }
            }
            crate::terminal::metrics::clear_pty_read_timestamps(&session_id_clone);
            end_output(&output_hub, &session_id_clone);
            let mut registry = lifecycle.lock();
            if registry.state(&session_id_clone) != Some(SessionProcessState::Hibernated) {
                registry.remove(&session_id_clone);
            }
        });

        (session_id, lifecycle_rx)
    }

    /// Drains an ADOPTED session's PTY output into the hub.
    ///
    /// A handover transfers the PTY master to the successor, which adopts the session and gets a
    /// fresh output receiver. That receiver must be held and pumped for as long as the child runs:
    /// the lifecycle watcher treats a closed receiver as "the owner went away" and closes the
    /// session, which kills the very child the handover was supposed to preserve. Unlike
    /// `register_output`, the hub state is already present (the predecessor's snapshot was
    /// imported during adoption), so this must not register the session again or record a new
    /// initial size.
    #[cfg(unix)]
    pub(crate) fn pump_adopted_output(
        &self,
        session_id: String,
        mut pty_rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
    ) {
        // The imported mirror was seeded muted; from here on it answers like a fresh session.
        self.output_hub.enable_program_status(&session_id);
        let output_hub = Arc::clone(&self.output_hub);
        let pty_manager = Arc::clone(&self.pty_manager);
        let lifecycle = Arc::clone(&self.lifecycle);
        tokio::spawn(async move {
            while let Some(chunk) = pty_rx.recv().await {
                let read_unix_micros = crate::terminal::metrics::take_pty_read_timestamp(
                    &session_id,
                    chunk.len(),
                );
                if let Some((_chunk, replies)) =
                    output_hub.publish_with_replies(&session_id, chunk, read_unix_micros)
                {
                    spawn_program_status_replies(&pty_manager, &session_id, replies);
                }
            }
            crate::terminal::metrics::clear_pty_read_timestamps(&session_id);
            end_output(&output_hub, &session_id);
            let mut registry = lifecycle.lock();
            if registry.state(&session_id) != Some(SessionProcessState::Hibernated) {
                registry.remove(&session_id);
            }
        });
    }

    pub fn attach(
        &self,
        session_id: &str,
    ) -> Result<(Vec<u8>, broadcast::Receiver<Vec<u8>>), PtyError> {
        if !self.list_sessions().contains(&session_id.to_string()) && !self.output_hub.has_session(session_id) {
            return Err(PtyError::SessionNotFound(session_id.to_string()));
        }

        self.output_hub
            .subscribe(session_id)
            .ok_or_else(|| PtyError::SessionNotFound(session_id.to_string()))
    }

    pub fn attach_with_sequence(
        &self,
        session_id: &str,
        after_sequence: Option<u64>,
    ) -> Result<SessionAttachment, PtyError> {
        if !self.list_sessions().contains(&session_id.to_string()) && !self.output_hub.has_session(session_id) {
            return Err(PtyError::SessionNotFound(session_id.to_string()));
        }

        self.output_hub
            .subscribe_with_sequence(session_id, after_sequence)
            .ok_or_else(|| PtyError::SessionNotFound(session_id.to_string()))
    }

    /// Ranges variant of [`Self::attach_with_sequence`]: history segments are described as
    /// byte offsets into the snapshot's flat history so callers can build wire segments
    /// without an intermediate full materialization of segment bytes.
    pub fn attach_with_sequence_ranges(
        &self,
        session_id: &str,
        after_sequence: Option<u64>,
    ) -> Result<(SessionAttachment, Vec<HistoryRange>), PtyError> {
        if !self.list_sessions().contains(&session_id.to_string()) && !self.output_hub.has_session(session_id) {
            return Err(PtyError::SessionNotFound(session_id.to_string()));
        }

        self.output_hub
            .subscribe_with_screen_mirror(session_id, after_sequence)
            .ok_or_else(|| PtyError::SessionNotFound(session_id.to_string()))
    }

    /// Attaches to a session while snapshotting remote generation under the remote state lock.
    ///
    /// For remote sessions, this holds the entry's state lock while taking the hub snapshot,
    /// guaranteeing that the generation and history snapshot are atomically consistent and
    /// cannot race a concurrent reconnect/retry advancing generation.
    pub fn attach_remote_with_sequence(
        &self,
        session_id: &str,
        after_sequence: Option<u64>,
    ) -> Result<(SessionAttachment, Option<u64>), PtyError> {
        if self.remote.contains(session_id) {
            let (attachment, generation) = self
                .remote
                .attach_snapshot_with_generation(session_id, after_sequence)
                .map_err(|_| PtyError::SessionNotFound(session_id.to_string()))?;
            Ok((attachment, Some(generation)))
        } else {
            let attachment = self.attach_with_sequence(session_id, after_sequence)?;
            Ok((attachment, None))
        }
    }

    /// Ranges variant of [`Self::attach_remote_with_sequence`]: returns history segment
    /// byte offsets alongside the attachment so the daemon wire build can slice the flat
    /// snapshot directly instead of cloning materialized segment buffers.
    pub fn attach_remote_with_sequence_ranges(
        &self,
        session_id: &str,
        after_sequence: Option<u64>,
    ) -> Result<(SessionAttachment, Vec<HistoryRange>, Option<u64>), PtyError> {
        if self.remote.contains(session_id) {
            let (attachment, ranges, generation) = self
                .remote
                .attach_snapshot_with_generation_ranges(session_id, after_sequence)
                .map_err(|_| PtyError::SessionNotFound(session_id.to_string()))?;
            Ok((attachment, ranges, Some(generation)))
        } else {
            let (attachment, ranges) = self.attach_with_sequence_ranges(session_id, after_sequence)?;
            Ok((attachment, ranges, None))
        }
    }

    pub fn write_input(&self, session_id: &str, data: &[u8]) -> Result<(), PtyError> {
        if self.remote.contains(session_id) || super::paired_runtime::Runtime::owns(session_id) {
            return Err(PtyError::Other(
                "Remote input requires write_input_operation and a generation".into(),
            ));
        }
        self.pty_manager.write_input(session_id, data)
    }

    pub fn resize(&self, session_id: &str, cols: u16, rows: u16) -> Result<(), PtyError> {
        if super::paired_runtime::Runtime::owns(session_id) {
            return Err(PtyError::Other(
                "Paired resize requires a controller generation".into(),
            ));
        }
        self.pty_manager.resize(session_id, cols, rows)?;
        // Single choke point for ALL resize callers (daemon request arm, remote gateway):
        // every PTY resize must leave a ledger marker or segmented replay misattributes
        // post-resize bytes to the previous width.
        self.output_hub.record_resize(session_id, cols, rows);
        Ok(())
    }

    pub fn signal(&self, session_id: &str, signal: TerminalSignal) -> Result<(), PtyError> {
        if super::paired_runtime::Runtime::owns(session_id) {
            return Err(PtyError::Other(
                "Paired signal requires a controller generation".into(),
            ));
        }
        self.pty_manager.signal(session_id, signal)
    }

    pub async fn detach_session(&self, session_id: &str) -> Result<(), PtyError> {
        if super::paired_runtime::Runtime::owns(session_id) {
            self.paired
                .detach(session_id)
                .await
                .map_err(PtyError::Other)?;
            self.output_hub.remove_session(session_id);
            self.lifecycle.lock().remove(session_id);
            return Ok(());
        }
        Err(PtyError::Other(
            "Session detach is supported only for paired sessions".into(),
        ))
    }

    pub async fn close_session(&self, session_id: &str) -> Result<(), PtyError> {
        self.pastes.forget_session(session_id);
        if super::paired_runtime::Runtime::owns(session_id) {
            self.paired
                .detach(session_id)
                .await
                .map_err(PtyError::Other)?;
            self.output_hub.remove_session(session_id);
            self.lifecycle.lock().remove(session_id);
            return Ok(());
        }
        if self.remote.contains(session_id) {
            return self
                .remote
                .close(session_id)
                .await
                .map_err(|e| PtyError::Other(e.to_string()));
        }
        let result = self.pty_manager.close_session(session_id).await;
        if result.is_ok() {
            self.output_hub.remove_session(session_id);
            self.lifecycle.lock().remove(session_id);
        }
        result
    }

    pub async fn hibernate_session(&self, session_id: &str) -> Result<(), PtyError> {
        if self.remote.contains(session_id) || super::paired_runtime::Runtime::owns(session_id) {
            return Err(PtyError::Other(
                "Session hibernation is supported only for local PTYs".into(),
            ));
        }
        self.pastes.forget_session(session_id);
        self.lifecycle
            .lock()
            .mark_hibernated(session_id.to_string());
        self.output_hub.remove_session(session_id);
        match self.pty_manager.close_session(session_id).await {
            Ok(()) => Ok(()),
            Err(error) => {
                if self.pty_manager.get_session(session_id).is_some() {
                    self.lifecycle.lock().mark_running(session_id.to_string());
                } else {
                    self.lifecycle.lock().remove(session_id);
                }
                Err(error)
            }
        }
    }

    pub async fn suspend_session(&self, session_id: &str) -> Result<(), PtyError> {
        if self.remote.contains(session_id) || super::paired_runtime::Runtime::owns(session_id) {
            return Err(PtyError::Other(
                "Session suspend is supported only for local PTYs".into(),
            ));
        }
        let session = self.get_session(session_id)
            .ok_or_else(|| PtyError::SessionNotFound(session_id.into()))?;
        let target = crate::ipc::run_blocking(move || {
            session.suspension_target().map_err(crate::ipc::IpcError::internal)
        }).await.map_err(|error| PtyError::Other(error.to_string()))?;
        self.suspend_verified_session(session_id, target).await.map(|_| ())
    }

    pub async fn suspend_verified_session(&self, session_id: &str, target: super::SuspensionTarget)
        -> Result<super::ActuationReceipt, PtyError>
    {
        let session = self.get_session(session_id)
            .ok_or_else(|| PtyError::SessionNotFound(session_id.into()))?;
        let receipt = crate::ipc::run_blocking(move || {
            let actual = session.suspension_target().map_err(crate::ipc::IpcError::internal)?;
            let receipt = actuate_suspension(&actual, &target, super::stop_for_owned_suspension)
                .map_err(crate::ipc::IpcError::internal)?;
            session.set_suspension_receipt(Some(receipt.clone()));
            Ok(receipt)
        }).await.map_err(|error| PtyError::Other(error.to_string()))?;
        self.lifecycle.lock().mark_suspended(session_id.to_string());
        Ok(receipt)
    }

    pub async fn resume_owned_session(&self, session_id: &str, target: super::SuspensionTarget) -> Result<(), PtyError> {
        let session = self.get_session(session_id)
            .ok_or_else(|| PtyError::SessionNotFound(session_id.into()))?;
        crate::ipc::run_blocking(move || {
            let actual = session.suspension_target().map_err(crate::ipc::IpcError::internal)?;
            if actual != target {
                return Err(crate::ipc::IpcError::internal(super::SuspensionError::IdentityMismatch { pid: target.pid }));
            }
            auto_resume_suspension(&target, super::classify_stop_source, super::resume_owned)
                .map_err(crate::ipc::IpcError::internal)?;
            session.set_suspension_receipt(None);
            Ok(())
        }).await.map_err(|error| PtyError::Other(error.to_string()))?;
        self.lifecycle.lock().mark_running(session_id);
        Ok(())
    }

    pub async fn resume_session(&self, session_id: &str) -> Result<(), PtyError> {
        if self.remote.contains(session_id) || super::paired_runtime::Runtime::owns(session_id) {
            return Err(PtyError::Other(
                "Session resume is supported only for local PTYs".into(),
            ));
        }
        let manager = self.pty_manager.clone();
        let id = session_id.to_owned();
        crate::ipc::run_blocking(move || manager.signal(&id, TerminalSignal::Continue)
            .map_err(crate::ipc::IpcError::internal)).await
            .map_err(|error| PtyError::Other(error.to_string()))?;
        if let Some(session) = self.get_session(session_id) { session.set_suspension_receipt(None); }
        self.lifecycle.lock().mark_running(session_id.to_string());
        Ok(())
    }

    pub(crate) async fn close_machine_session(
        &self,
        session_id: &str,
        authorize: Arc<dyn Fn() -> Result<(), String> + Send + Sync>,
    ) -> Result<(), PtyError> {
        self.pastes.forget_session(session_id);
        self.pty_manager
            .close_authorized(session_id, std::time::Duration::from_secs(5), authorize)
            .await?;
        self.output_hub.remove_session(session_id);
        self.lifecycle.lock().remove(session_id);
        Ok(())
    }

    pub fn list_sessions(&self) -> Vec<String> {
        let mut sessions = self.pty_manager.list_sessions();
        sessions.extend(self.remote.list());
        sessions.extend(self.paired.list());
        sessions
    }

    pub fn get_session(&self, session_id: &str) -> Option<Arc<PtySession>> {
        self.pty_manager.get_session(session_id)
    }

    /// Live identity of every session, kept separate from list_sessions registration IDs.
    /// Blocking (reads kernel process start time); callers must use run_blocking.
    pub fn describe_live_sessions(&self) -> Vec<crate::daemon::protocol::LiveSessionInfo> {
        let mut out = Vec::new();
        for id in self.pty_manager.list_sessions() {
            let Some(session) = self.pty_manager.get_session(&id) else { continue };
            let start = session.suspension_target().ok().and_then(|t| t.started_at_unix_ms);
            out.push(local_live_info(id, session.pid(), start, session.is_reader_finished()));
        }
        for id in self.remote.list() {
            let Some(details) = self.remote.details(&id) else { continue };
            out.push(remote_live_info(id, details.state, details.pid));
        }
        for id in self.paired.list() {
            // The host-reported pid and the actor's transport/exit state; never a local pid.
            let Some(live) = self.paired.live_state(&id) else { continue };
            out.push(crate::daemon::protocol::LiveSessionInfo::remote(
                crate::daemon::protocol::LiveSessionKind::Paired,
                id,
                live.remote_pid,
                live.reader_open,
                live.connected,
            ));
        }
        out
    }
}

pub(crate) fn local_live_info(
    id: String,
    pid: Option<u32>,
    start_time: Option<u64>,
    reader_finished: bool,
) -> crate::daemon::protocol::LiveSessionInfo {
    crate::daemon::protocol::LiveSessionInfo::local(id, pid, start_time, !reader_finished)
}

pub(crate) fn remote_live_info(
    id: String,
    state: super::remote::RemoteConnectionState,
    pid: Option<crate::ssh::bridge::RemotePid>,
) -> crate::daemon::protocol::LiveSessionInfo {
    use super::remote::RemoteConnectionState as S;
    let connected = matches!(state, S::Connected);
    let reader_open = matches!(state, S::Connected | S::Reconnecting);
    crate::daemon::protocol::LiveSessionInfo::remote(
        crate::daemon::protocol::LiveSessionKind::Ssh, id, pid, reader_open, connected,
    )
}

#[cfg(test)]
mod paste_cancel_tests {
    use super::*;

    #[tokio::test]
    async fn cancellation_keeps_receipt_when_session_record_is_removed() {
        let service = TerminalService::default();
        let record = PasteRecord::new();
        record.accepted.store(17, std::sync::atomic::Ordering::Release);
        record.idle.send_replace(false);
        let idle = record.idle.clone();
        service.pastes.records.lock().insert(("s".into(), "p".into()), record);
        let mut cancel = Box::pin(service.cancel_paste("s", "p"));
        assert!(matches!(futures_util::poll!(cancel.as_mut()), std::task::Poll::Pending));
        service.pastes.forget_session("s");
        idle.send_replace(true);
        assert_eq!(cancel.await, PasteCancelOutcome { found: true, bytes_written: 17, settled: true });
    }

    #[tokio::test]
    async fn cancel_before_first_chunk_refuses_the_whole_paste_without_touching_ordinary_input() {
        let service = TerminalService::default();
        let outcome = service.cancel_paste("no-such-session", "p-early").await;
        assert_eq!(outcome, PasteCancelOutcome { found: false, bytes_written: 0, settled: false });
        // Unknown IDs cannot create records, so a later chunk cannot restart them.
        let key = ("no-such-session".to_string(), "p-early".to_string());
        assert!(!service.pastes.records.lock().contains_key(&key));
        service.pastes.forget_session("no-such-session");
        assert!(service.pastes.records.lock().is_empty(), "close must drop the session's records");
    }

    #[test]
    fn registry_is_bounded_and_never_drops_an_in_flight_paste() {
        let mut records = std::collections::HashMap::new();
        let busy = PasteRecord::new();
        busy.idle.send_replace(false);
        records.insert(("s".to_string(), "busy".to_string()), busy);
        for index in 0..MAX_PASTE_RECORDS + 50 {
            records.insert(("s".to_string(), format!("idle-{index}")), PasteRecord::new());
            PasteRegistry::prune(&mut records, MAX_PASTE_RECORDS);
        }
        assert!(records.len() <= MAX_PASTE_RECORDS);
        assert!(records.contains_key(&("s".to_string(), "busy".to_string())));
    }

    #[test]
    fn registry_lookup_at_capacity_preserves_registered_pastes() {
        let registry = PasteRegistry::default();
        let ids: Vec<_> = (0..MAX_PASTE_RECORDS).map(|_| registry.begin("s").unwrap()).collect();
        let mut records = registry.records.lock();
        PasteRegistry::prune(&mut records, MAX_PASTE_RECORDS);
        assert_eq!(records.len(), MAX_PASTE_RECORDS);
        for id in ids {
            assert!(records.contains_key(&("s".into(), id)));
        }
    }

    #[test]
    fn registry_refuses_registration_when_every_slot_is_in_flight() {
        let registry = PasteRegistry::default();
        for _ in 0..MAX_PASTE_RECORDS {
            let id = registry.begin("s").unwrap();
            registry.records.lock().get(&("s".into(), id)).unwrap().idle.send_replace(false);
        }
        assert!(matches!(registry.begin("s"), Err(PtyError::InputError { bytes_written: 0, .. })));
        assert_eq!(registry.records.lock().len(), MAX_PASTE_RECORDS);
    }

    #[cfg(target_os = "linux")]
    async fn await_marker(rx: &mut tokio::sync::mpsc::Receiver<Vec<u8>>, marker: &str) -> Option<String> {
        tokio::time::timeout(std::time::Duration::from_secs(30), async {
            let mut seen = Vec::new();
            while let Some(chunk) = rx.recv().await {
                seen.extend_from_slice(&chunk);
                let text = String::from_utf8_lossy(&seen).into_owned();
                if text.contains(marker) {
                    return Some(text);
                }
            }
            None
        })
        .await
        .ok()
        .flatten()
    }

    /// Drives `paste` only on its own wakeups until it is parked on a master the kernel reports
    /// unwritable (the tty input queue is full): `Ok(None)`, or until it finishes: `Ok(Some(out))`.
    #[cfg(target_os = "linux")]
    async fn drive_until_parked<F>(paste: &mut std::pin::Pin<Box<F>>, master_fd: i32) -> Result<Option<F::Output>, ()>
    where
        F: std::future::Future,
    {
        use std::task::Poll;
        let writable = || {
            let mut probe = libc::pollfd { fd: master_fd, events: libc::POLLOUT, revents: 0 };
            // SAFETY: one initialized pollfd; the session keeps the fd alive for the call.
            (unsafe { libc::poll(&mut probe, 1, 0) }) > 0 && probe.revents & libc::POLLOUT != 0
        };
        tokio::time::timeout(std::time::Duration::from_secs(30), std::future::poll_fn(|cx| {
            match paste.as_mut().poll(cx) {
                Poll::Ready(out) => Poll::Ready(Some(out)),
                Poll::Pending if !writable() => Poll::Ready(None),
                Poll::Pending => Poll::Pending,
            }
        }))
        .await
        .map_err(|_| ())
    }

    /// Raw (non-canonical, ISIG off) program that does not read until released. The pending
    /// paste is cancelled server-side with an exact count, a late chunk of it is refused, and an
    /// ordinary 0x03 byte then reaches the program as data, unchanged.
    #[cfg(target_os = "linux")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn raw_mode_paste_cancel_reports_exact_bytes_and_never_resumes_later_chunks() {
        let fixture = tempfile::tempdir().unwrap();
        let release = fixture.path().join("release");
        let path = std::ffi::CString::new(release.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: path is a valid NUL-terminated string for the duration of the call.
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        let service = TerminalService::default();
        let mut cmd = portable_pty::CommandBuilder::new("python3");
        cmd.args(["-c", "import os,sys,termios\na=termios.tcgetattr(0)\na[3]&=~(termios.ICANON|termios.ECHO|termios.ISIG|termios.IEXTEN)\na[0]&=~(termios.IXON|termios.ICRNL)\ntermios.tcsetattr(0,termios.TCSANOW,a)\nf=os.open(sys.argv[1],os.O_RDWR)\nprint('READY',flush=True)\nos.read(f,1)\nx=y=o=0\nwhile True:\n b=os.read(0,65536)\n if not b: break\n i=b.find(b'\\x03')\n p=b if i<0 else b[:i]\n x+=p.count(b'x'); y+=p.count(b'y'); o+=len(p)-p.count(b'x')-p.count(b'y')\n if i>=0:\n  print('COUNT=%d Y=%d OTHER=%d CTRL=1' % (x,y,o),flush=True)\n  break\n"]);
        cmd.arg(release.as_os_str());
        let (id, mut output) = service.pty_manager().spawn(cmd, 80, 24).unwrap();
        let session = service.get_session(&id).unwrap();
        let ready = await_marker(&mut output, "READY").await.is_some();
        let master_fd = session.raw_master_fd().expect("master fd");
        const TOTAL: usize = 300 * 1024;

        let paste_service = service.clone();
        let paste_id = id.clone();
        let registered = service.begin_paste(&id).unwrap();
        let writing = registered.clone();
        let mut paste = Box::pin(async move {
            paste_service.write_paste_chunk(&paste_id, 0, &writing, vec![b'x'; TOTAL], true).await
        });
        let parked = if ready { drive_until_parked(&mut paste, master_fd).await } else { Err(()) };
        let parked_ok = matches!(parked, Ok(None));
        let (result, outcome) = if parked_ok {
            tokio::join!(paste.as_mut(), service.cancel_paste(&id, &registered))
        } else {
            drop(paste);
            (Err(PasteChunkError::Failed(PtyError::Other("paste was not parked".into()))),
                service.cancel_paste(&id, &registered).await)
        };
        let late = service.write_paste_chunk(&id, 0, &registered, vec![b'y'; 16], false).await;
        service.pastes.forget_session(&id);
        let evicted = service.write_paste_chunk(&id, 0, &registered, vec![b'y'; 16], false).await;
        let released = tokio::task::spawn_blocking(move || {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new().write(true).custom_flags(libc::O_NONBLOCK).open(release)?.write_all(b"g")
        }).await.unwrap();
        let control = session.write_input_cancellable(&[0x03]).await;
        let report = await_marker(&mut output, "CTRL=1").await;
        service.close_session(&id).await.unwrap();

        assert!(ready, "fixture never configured raw input");
        assert!(parked_ok, "paste must be parked on the full PTY input queue");
        let outcome_bytes = outcome.bytes_written as usize;
        assert!(outcome.found && outcome.settled, "{outcome:?}");
        assert!(outcome_bytes > 0 && outcome_bytes < TOTAL, "{outcome:?}");
        match result {
            Err(PasteChunkError::Cancelled { bytes_written }) => assert_eq!(bytes_written, outcome_bytes),
            other => panic!("pending chunk must stop as cancelled: {other:?}"),
        }
        assert!(matches!(late, Err(PasteChunkError::Cancelled { bytes_written: 0 })), "{late:?}");
        assert!(matches!(evicted, Err(PasteChunkError::Unknown)), "{evicted:?}");
        released.unwrap();
        assert!(control.is_ok(), "ordinary control byte after cancellation: {control:?}");
        let report = report.expect("program reports what it received");
        let line = report.lines().find(|line| line.contains("COUNT=")).unwrap().trim().to_string();
        assert_eq!(line, format!("COUNT={outcome_bytes} Y=0 OTHER=0 CTRL=1"));
    }

    /// Canonical program with ISIG on that never reads stdin: an ordinary Ctrl+C byte after the
    /// paste still raises SIGINT. Measured on Linux (pty-steady-full.log), a canonical tty never
    /// fills: the line discipline accepts and drops input past its line limit, so the paste
    /// normally completes and this checks plain kernel ISIG. The full-queue signal path is
    /// covered by the raw-mode native paste test; if a kernel does park the paste, the accepted
    /// count must match between the chunk result and the cancel reply.
    #[cfg(target_os = "linux")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn canonical_mode_ctrl_c_reaches_program_after_a_paste() {
        let service = TerminalService::default();
        let mut cmd = portable_pty::CommandBuilder::new("python3");
        cmd.args(["-c", "import signal,sys,termios\na=termios.tcgetattr(0)\na[3]=(a[3]|termios.ICANON|termios.ISIG)&~termios.ECHO\ntermios.tcsetattr(0,termios.TCSANOW,a)\nsignal.signal(signal.SIGINT,lambda *_:(print('GOT_INT',flush=True),sys.exit(0)))\nprint('READY',flush=True)\nsignal.pause()\n"]);
        let (id, mut output) = service.pty_manager().spawn(cmd, 80, 24).unwrap();
        let session = service.get_session(&id).unwrap();
        let ready = await_marker(&mut output, "READY").await.is_some();
        let master_fd = session.raw_master_fd().expect("master fd");
        const TOTAL: usize = 300 * 1024;

        let paste_service = service.clone();
        let paste_id = id.clone();
        let registered = service.begin_paste(&id).unwrap();
        let writing = registered.clone();
        let mut paste = Box::pin(async move {
            paste_service.write_paste_chunk(&paste_id, 0, &writing, vec![b'x'; TOTAL], true).await
        });
        let parked = if ready { drive_until_parked(&mut paste, master_fd).await } else { Err(()) };
        let parked_ok = parked.is_ok();
        let (result, outcome) = match parked {
            Ok(None) => tokio::join!(paste.as_mut(), service.cancel_paste(&id, &registered)),
            Ok(Some(done)) => (done, service.cancel_paste(&id, &registered).await),
            Err(()) => {
                drop(paste);
                (Err(PasteChunkError::Failed(PtyError::Other("paste stalled".into()))),
                    service.cancel_paste(&id, &registered).await)
            }
        };
        let control = session.write_input_cancellable(&[0x03]).await;
        let interrupted = if control.is_ok() { await_marker(&mut output, "GOT_INT").await } else { None };
        service.close_session(&id).await.unwrap();

        assert!(ready, "fixture never announced READY");
        assert!(parked_ok, "paste neither parked nor finished within the bound");
        assert!(outcome.found && outcome.settled, "{outcome:?}");
        match result {
            Ok(()) => assert_eq!(outcome.bytes_written as usize, TOTAL),
            Err(PasteChunkError::Cancelled { bytes_written }) => assert_eq!(bytes_written as u64, outcome.bytes_written),
            Err(other) => panic!("unexpected paste failure: {other:?}"),
        }
        assert!(control.is_ok(), "Ctrl+C byte write failed: {control:?}");
        assert!(interrupted.is_some(), "canonical program never received SIGINT");
    }
}

#[cfg(test)]
mod live_session_tests {
    use super::*;
    use crate::daemon::protocol::LiveSessionKind;
    use crate::ssh::bridge::RemotePid;
    use crate::terminal::remote::RemoteConnectionState;

    #[test]
    fn exited_local_pty_reports_reader_closed() {
        let info = local_live_info("l".into(), Some(10), Some(5), true);
        assert!(!info.reader_open);
        assert!(!info.connected);
        assert_eq!(info.kind, LiveSessionKind::Local);
        assert_eq!(info.local_child_pid, Some(10));
        let live = local_live_info("l".into(), Some(10), Some(5), false);
        assert!(live.reader_open);
    }

    #[test]
    fn disconnected_remote_entry_reports_not_connected() {
        for state in [RemoteConnectionState::Disconnected, RemoteConnectionState::Expired, RemoteConnectionState::Reconnecting] {
            let info = remote_live_info("r".into(), state, Some(RemotePid(77)));
            assert!(!info.connected, "{state:?}");
            assert_eq!(info.remote_pid, Some(77));
            assert_eq!(info.local_child_pid, None);
        }
        let connected = remote_live_info("r".into(), RemoteConnectionState::Connected, Some(RemotePid(77)));
        assert!(connected.connected && connected.reader_open);
        assert_eq!(connected.kind, LiveSessionKind::Ssh);
    }
}

fn actuate_suspension(
    actual: &super::SuspensionTarget,
    requested: &super::SuspensionTarget,
    stop: impl FnOnce(&super::SuspensionTarget) -> Result<super::ActuationReceipt, super::SuspensionError>,
) -> Result<super::ActuationReceipt, super::SuspensionError> {
    if actual != requested {
        return Err(super::SuspensionError::IdentityMismatch { pid: requested.pid });
    }
    let receipt = stop(requested)?;
    if receipt.pid != actual.pid || receipt.incarnation != actual.incarnation
        || receipt.source != super::SuspensionSource::FerryxOwned
    {
        return Err(super::SuspensionError::IdentityMismatch { pid: requested.pid });
    }
    Ok(receipt)
}

pub(crate) fn auto_resume_suspension(
    target: &super::SuspensionTarget,
    classify: impl FnOnce(&super::SuspensionTarget) -> Result<super::SuspensionSource, super::SuspensionError>,
    resume: impl FnOnce(&super::SuspensionTarget) -> Result<(), super::SuspensionError>,
) -> Result<(), super::SuspensionError> {
    match classify(target)? {
        super::SuspensionSource::FerryxOwned => resume(target),
        super::SuspensionSource::External | super::SuspensionSource::Unknown =>
            Err(super::SuspensionError::NotOwned { pid: target.pid }),
    }
}
#[cfg(test)]
mod preparation_tests {
    use super::*;

    fn suspension_target() -> super::super::SuspensionTarget {
        super::super::SuspensionTarget { pid: 42, incarnation: "spawn-incarnation".into(), started_at_unix_ms: Some(100) }
    }

    #[test]
    fn pane_liveness_suspension_receipt_only_on_successful_actuation() {
        let target = suspension_target();
        let failure = actuate_suspension(&target, &target, |_| Err(super::super::SuspensionError::NotOwned { pid: 42 }));
        assert!(failure.is_err());
        let receipt = super::super::ActuationReceipt { pid: 42, incarnation: target.incarnation.clone(),
            source: super::super::SuspensionSource::FerryxOwned, actuated_at_unix_ms: 200,
            stop_observed: true, guarantee: super::super::StopGuarantee::IdentityBoundObservedStop };
        assert_eq!(actuate_suspension(&target, &target, |_| Ok(receipt.clone())).unwrap(), receipt);
    }

    #[test]
    fn pane_liveness_suspension_external_stop_never_auto_resumed() {
        let target = suspension_target();
        let resumed = std::cell::Cell::new(false);
        let result = auto_resume_suspension(&target,
            |_| Ok(super::super::SuspensionSource::External),
            |_| { resumed.set(true); Ok(()) });
        assert!(matches!(result, Err(super::super::SuspensionError::NotOwned { .. })));
        assert!(!resumed.get());
    }

    #[test]
    fn pane_liveness_suspension_identity_mismatch_refused_before_actuation() {
        let actual = suspension_target();
        let mut requested = actual.clone();
        requested.incarnation = "replacement".into();
        let actuated = std::cell::Cell::new(false);
        let result = actuate_suspension(&actual, &requested, |_| {
            actuated.set(true);
            Err(super::super::SuspensionError::NotOwned { pid: 42 })
        });
        assert!(matches!(result, Err(super::super::SuspensionError::IdentityMismatch { .. })));
        assert!(!actuated.get());
    }
    #[tokio::test]
    async fn preparation_resolved_spawn_context_and_probe_permit_lifetime() {
        let service = TerminalService::default();
        let spawning = service.clone();
        let (root, spawned) = crate::ipc::run_blocking(move || {
            let root = tempfile::tempdir().unwrap();
            let cwd = root.path().join("nested");
            std::fs::create_dir(&cwd).unwrap();
            let mut command = if cfg!(windows) {
                let mut command = CommandBuilder::new("cmd.exe");
                command.args(["/D", "/Q", "/K"]);
                command
            } else {
                CommandBuilder::new("/bin/sh")
            };
            command.env("FERRYX_WORKSPACE_ID", "foreign-daemon-identity");
            let context = super::super::pty::ResolvedSpawnContext {
                root: root.path().to_owned(),
                cwd,
                managed_workspace_id: None,
            };
            let spawned = spawning.spawn_resolved_with_id(
                uuid::Uuid::new_v4().to_string(),
                command,
                80,
                24,
                context,
            );
            Ok((root, spawned))
        })
        .await
        .unwrap();
        let (id, _lifecycle) = spawned.unwrap();
        let permit = service.try_acquire_cwd_probe(&id);
        let acquired = permit.is_some();
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let worker = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(std::time::Duration::from_secs(5))
        });
        let entered = tokio::time::timeout(std::time::Duration::from_secs(5), entered_rx).await;
        let busy = service.try_acquire_cwd_probe(&id).is_none();
        let ownership = service
            .get_session(&id)
            .and_then(|session| session.worktree_path());
        let close = service.close_session(&id).await;
        let removed = service.try_acquire_cwd_probe(&id).is_none();
        let released = release_tx.send(());
        let joined = worker.await;
        assert!(close.is_ok(), "{close:?}");
        assert!(matches!(entered, Ok(Ok(()))));
        assert!(released.is_ok());
        assert!(matches!(joined, Ok(Ok(()))));
        assert!(acquired && busy && removed);
        assert_eq!(ownership.as_deref(), Some(root.path()));
        assert!(service.try_acquire_cwd_probe("missing").is_none());
    }

    #[tokio::test]
    async fn preparation_cold_path_does_not_discover_login_path() {
        let service = TerminalService::default();
        let (legacy_entered_tx, legacy_entered_rx) = tokio::sync::oneshot::channel();
        let (legacy_release_tx, legacy_release_rx) = std::sync::mpsc::channel();
        let legacy = tokio::task::spawn_blocking(move || {
            let entered = std::sync::Mutex::new(Some(legacy_entered_tx));
            let release = std::sync::Mutex::new(legacy_release_rx);
            super::super::shell::with_path_discovery(
                Arc::new(move || {
                    entered.lock().unwrap().take().unwrap().send(()).unwrap();
                    release
                        .lock()
                        .unwrap()
                        .recv_timeout(std::time::Duration::from_secs(5))
                        .unwrap();
                    Vec::new()
                }),
                super::super::shell::legacy_search_paths,
            )
        });
        let entered = tokio::time::timeout(std::time::Duration::from_secs(5), legacy_entered_rx).await;
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = calls.clone();
        let git_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let git_counted = git_calls.clone();
        let spawning = service.clone();
        let setup = crate::ipc::run_blocking(move || {
            let root = tempfile::tempdir().unwrap();
            let result = super::super::shell::with_path_discovery(
                Arc::new(move || {
                    counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Vec::new()
                }),
                || {
                    crate::worktree::git::with_git_observer(
                        Arc::new(move |_, _| {
                            git_counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        }),
                        || {
                            let command = super::super::shell::resolve_ordinary_shell_command(
                                Some(if cfg!(windows) { "cmd.exe" } else { "/bin/sh" }),
                            )?;
                            spawning.spawn_resolved_with_id(
                                uuid::Uuid::new_v4().to_string(),
                                command,
                                80,
                                24,
                                super::super::pty::ResolvedSpawnContext {
                                    root: root.path().to_owned(),
                                    cwd: root.path().to_owned(),
                                    managed_workspace_id: None,
                                },
                            )
                        },
                    )
                },
            );
            Ok((root, result))
        })
        .await;
        let close = match &setup {
            Ok((_, Ok((id, _)))) => service.close_session(id).await,
            _ => Ok(()),
        };
        let released = legacy_release_tx.send(());
        let joined = legacy.await;
        assert!(matches!(entered, Ok(Ok(()))));
        assert!(released.is_ok() && joined.is_ok());
        assert!(close.is_ok(), "{close:?}");
        assert!(matches!(setup, Ok((_, Ok(_)))), "{setup:?}");
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(git_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test]
    #[cfg(windows)]
    async fn windows_input_operation_uses_cancellable_native_writer() {
        struct RejectedBlockingWriter;
        impl std::io::Write for RejectedBlockingWriter {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("blocking input writer was used"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let service = TerminalService::default();
        let root = tempfile::tempdir().unwrap();
        let mut command = portable_pty::CommandBuilder::new("cmd.exe");
        command.args(["/D", "/Q", "/K"]);
        let (id, _output) = service.spawn_resolved_with_id(
            uuid::Uuid::new_v4().to_string(),
            command,
            80,
            24,
            super::super::pty::ResolvedSpawnContext {
                root: root.path().to_owned(),
                cwd: root.path().to_owned(),
                managed_workspace_id: None,
            },
        ).expect("spawn isolated input fixture");
        service.get_session(&id).unwrap()
            .set_writer_for_test(Box::new(RejectedBlockingWriter));

        let result = service.write_input_operation(&id, 0, b"\r".to_vec())
            .expect("admit input").await;
        let closed = service.close_session(&id).await;
        assert!(closed.is_ok(), "{closed:?}");
        assert!(result.is_ok(), "{result:?}");
    }

    #[tokio::test]
    async fn regression_local_pty_create_6_fast_exit_keeps_output_for_the_first_attach() {
        let service = TerminalService::default();
        let root = tempfile::tempdir().unwrap();
        let mut command = portable_pty::CommandBuilder::new(if cfg!(windows) {
            std::env::var_os("ComSpec").unwrap_or_else(|| "cmd.exe".into())
        } else {
            "/bin/sh".into()
        });
        if cfg!(windows) {
            command.args(["/D", "/Q", "/K"]);
        } else {
            command.arg("-c");
            command.arg("echo quick_exit_output; exit 0");
        }
        let context = super::super::pty::ResolvedSpawnContext {
            root: root.path().to_owned(),
            cwd: root.path().to_owned(),
            managed_workspace_id: None,
        };
        let (id, mut output_pump) = service
            .spawn_resolved_with_id(uuid::Uuid::new_v4().to_string(), command, 80, 24, context)
            .expect("spawn fast exiting command");
        #[cfg(windows)]
        {
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
            let (initial_snapshot, mut output) = service
                .output_hub()
                .observe_for_test(&id)
                .expect("observe spawned session output without attaching");
            let mut accumulated = initial_snapshot;
            let mut dsr_answers = 0usize;
            let mut command_sent = false;
            tokio::time::timeout_at(deadline, async {
                loop {
                    while let Some(query_start) = accumulated
                        .windows(4)
                        .position(|w| w == b"\x1b[6n")
                    {
                        accumulated.drain(..query_start + 4);
                        dsr_answers += 1;
                        service.write_input(&id, b"\x1b[1;1R").expect("reply to ConPTY DSR");
                        if !command_sent {
                            command_sent = true;
                            service
                                .write_input(&id, b"echo quick_exit_ou^tput&exit 0\r")
                                .expect("run and exit fixture command");
                        }
                    }
                    if accumulated.windows(b"quick_exit_output".len())
                        .any(|w| w == b"quick_exit_output")
                    {
                        break;
                    }
                    let chunk = output.recv().await.expect("PTY output broadcast remains open");
                    accumulated.extend_from_slice(&chunk);
                }
                assert!(dsr_answers > 0, "ConPTY did not issue a DSR query");
                let _ = tokio::time::timeout_at(deadline, output_pump.changed())
                    .await
                    .expect("the command's output ended within the shared drain bound");
            })
            .await
            .expect("ConPTY handshake, command output, and pump completion within one drain bound");
        }
        #[cfg(not(windows))]
        let _ = tokio::time::timeout(std::time::Duration::from_secs(10), output_pump.changed())
            .await
            .expect("the command's output ended");

        let (snapshot, mut stream) = service
            .attach(&id)
            .expect("the first attach after a fast exit still sees the output");
        let text = String::from_utf8_lossy(&snapshot);
        assert!(text.contains("quick_exit_output"), "snapshot lost the exited command's output: {text}");
        // The stream of an ended session is already over, and the session is gone afterwards.
        assert!(matches!(
            tokio::time::timeout(std::time::Duration::from_secs(5), stream.recv()).await,
            Ok(Err(broadcast::error::RecvError::Closed))
        ));
        assert!(!service.output_hub.has_session(&id));
        assert!(service.attach(&id).is_err());
    }
}
