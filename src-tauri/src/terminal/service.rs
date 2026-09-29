use crate::daemon::session_lifecycle::{SessionLifecycleRegistry, SessionProcessState};
use crate::terminal::output_hub::{HistoryRange, SessionAttachment, TerminalOutputHub};
use crate::terminal::{PtyError, PtyManager, PtySession, PtySessionState, TerminalSignal};
use crate::worktree::manager::WorktreeManager;
use parking_lot::Mutex;
use portable_pty::CommandBuilder;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{broadcast, watch};

/// Owner-neutral snapshot of one daemon-local session. Reading it never takes the
/// session's child handle, so it cannot wait behind a reap; the pid has its own accessor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
    pub id: String,
    pub state: PtySessionState,
    pub cols: u16,
    pub rows: u16,
    pub worktree_path: Option<PathBuf>,
    pub last_output_age_ms: Option<u64>,
}

impl SessionInfo {
    pub fn is_live(&self) -> bool {
        matches!(
            self.state,
            PtySessionState::Starting | PtySessionState::Running
        )
    }
}

/// The owner behind a held session handle. Phase 1 owners are daemon-local PTYs only.
#[derive(Clone)]
enum SessionOwner {
    Local(Arc<PtySession>),
}

/// Input path held for a connection's lifetime. It writes to the owner the session had
/// when the handle was resolved, exactly as the held PTY did.
#[derive(Clone)]
pub struct SessionInput(SessionOwner);

impl SessionInput {
    /// One frame of at most 64 KiB; dropping the future cancels the pending write.
    pub async fn write_cancellable(&self, data: &[u8]) -> Result<(), PtyError> {
        match &self.0 {
            SessionOwner::Local(session) => session.write_input_cancellable(data).await,
        }
    }

    /// Status of the same owner this input writes to.
    pub fn status(&self) -> SessionStatus {
        SessionStatus(self.0.clone())
    }
}

/// Lifecycle status that stays readable after the session leaves the registry, so an
/// exit code observed at close or natural exit is not lost to the removal.
#[derive(Clone)]
pub struct SessionStatus(SessionOwner);

impl SessionStatus {
    pub fn state(&self) -> PtySessionState {
        match &self.0 {
            SessionOwner::Local(session) => session.state(),
        }
    }

    pub fn is_reaped(&self) -> bool {
        match &self.0 {
            SessionOwner::Local(session) => session.is_reaped(),
        }
    }

    /// The shell's OS pid. This takes the session's child handle, which a reap holds.
    pub fn pid(&self) -> Option<u32> {
        match &self.0 {
            SessionOwner::Local(session) => session.pid(),
        }
    }

    /// Snapshot of this same held owner, read at the moment of the call. A caller that
    /// resolved the handle before entering a future keeps HEAD's timing: the registry
    /// lookup happens at resolution, the field reads happen here, on the same object.
    pub fn info(&self) -> SessionInfo {
        match &self.0 {
            SessionOwner::Local(session) => {
                let (cols, rows) = session.get_size();
                SessionInfo {
                    id: session.id().to_string(),
                    state: session.state(),
                    cols,
                    rows,
                    worktree_path: session.worktree_path(),
                    last_output_age_ms: session.last_output_age_ms(),
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct TerminalService {
    pty_manager: Arc<PtyManager>,
    output_hub: Arc<TerminalOutputHub>,
    remote: Arc<super::remote::RemoteRuntime>,
    paired: Arc<super::paired_runtime::Runtime>,
    lifecycle: Arc<Mutex<SessionLifecycleRegistry>>,
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
    pub fn new(pty_manager: Arc<PtyManager>, output_hub: Arc<TerminalOutputHub>) -> Self {
        pty_manager.set_output_hub(output_hub.clone());
        Self {
            remote: Arc::new(super::remote::RemoteRuntime::new(output_hub.clone())),
            paired: Arc::new(super::paired_runtime::Runtime::default()),
            pty_manager,
            output_hub,
            lifecycle: Arc::new(Mutex::new(SessionLifecycleRegistry::default())),
        }
    }

    pub fn paired(&self) -> &Arc<super::paired_runtime::Runtime> {
        &self.paired
    }

    pub fn remote(&self) -> &Arc<super::remote::RemoteRuntime> {
        &self.remote
    }

    /// Resource state of a daemon-owned process. Hibernated entries intentionally
    /// outlive PTY teardown so callers can distinguish suspension from never-spawned state.
    pub fn process_state(&self, session_id: &str) -> Option<SessionProcessState> {
        self.lifecycle.lock().state(session_id)
    }

    /// Generation-aware admission. The returned future must be awaited for delivery status.
    pub fn write_input_operation(
        &self,
        id: &str,
        generation: u64,
        data: Vec<u8>,
    ) -> Result<super::remote::RemoteOperation, PtyError> {
        if super::paired_runtime::Runtime::owns(id) {
            return self.paired.write(id, generation, data);
        }
        if self.remote.contains(id) {
            return self
                .remote
                .write(id, generation, data)
                .map_err(|e| PtyError::Other(e.to_string()));
        }
        // Resolve session synchronously so missing IDs fail immediately with SessionNotFound.
        let session = self
            .pty_manager
            .get_session(id)
            .ok_or_else(|| PtyError::SessionNotFound(id.to_string()))?;

        // Perform the write asynchronously off the Tokio worker thread to avoid blocking
        // the reactor when the PTY input queue is full.
        #[cfg(unix)]
        {
            const MAX_CHUNK_SIZE: usize = 65536;
            Ok(Box::pin(async move {
                // Chunks are written strictly in order, each under the session's input gate.
                // The gate is re-taken per chunk, so a second connection writing to the same
                // session can interleave only at a 64 KiB boundary.
                for chunk in data.chunks(MAX_CHUNK_SIZE) {
                    session
                        .write_input_cancellable(chunk)
                        .await
                        .map_err(|e| super::remote::RemoteFailure {
                            kind: super::remote::RemoteFailureKind::Transport,
                            message: e.to_string(),
                        })?;
                }
                Ok(())
            }))
        }

        #[cfg(not(unix))]
        {
            Ok(Box::pin(async move {
                tokio::task::spawn_blocking(move || session.write_input(&data))
                    .await
                    .map_err(|e| super::remote::RemoteFailure {
                        kind: super::remote::RemoteFailureKind::Transport,
                        message: format!("PTY write task panicked: {e}"),
                    })?
                    .map_err(|e| super::remote::RemoteFailure {
                        kind: super::remote::RemoteFailureKind::Transport,
                        message: e.to_string(),
                    })
            }))
        }
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
        let agent_sock = crate::daemon::server::agent_state_socket_path();
        let has_sock = std::path::Path::new(&agent_sock).exists();
        let plan = crate::ssh::operations::shell_plan(
            host,
            environment,
            remote_root,
            &session_id,
            if has_sock { Some(&agent_sock) } else { None },
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

        let (lifecycle_tx, lifecycle_rx) = watch::channel(());
        // Spawn output pump task from PTY reader to OutputHub
        let output_hub = Arc::clone(&self.output_hub);
        let lifecycle = Arc::clone(&self.lifecycle);
        let session_id_clone = session_id.clone();
        tokio::spawn(async move {
            let _lifecycle_tx = lifecycle_tx;
            while let Some(chunk) = pty_rx.recv().await {
                let read_unix_micros = crate::terminal::metrics::take_pty_read_timestamp(
                    &session_id_clone,
                    chunk.len(),
                );
                output_hub.publish_with_read_timestamp(&session_id_clone, chunk, read_unix_micros);
            }
            crate::terminal::metrics::clear_pty_read_timestamps(&session_id_clone);
            output_hub.remove_session(&session_id_clone);
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
    pub(crate) fn pump_adopted_output(
        &self,
        session_id: String,
        mut pty_rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
    ) {
        let output_hub = Arc::clone(&self.output_hub);
        let lifecycle = Arc::clone(&self.lifecycle);
        tokio::spawn(async move {
            while let Some(chunk) = pty_rx.recv().await {
                let read_unix_micros = crate::terminal::metrics::take_pty_read_timestamp(
                    &session_id,
                    chunk.len(),
                );
                output_hub.publish_with_read_timestamp(&session_id, chunk, read_unix_micros);
            }
            crate::terminal::metrics::clear_pty_read_timestamps(&session_id);
            output_hub.remove_session(&session_id);
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
        if !self.list_sessions().contains(&session_id.to_string()) {
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
        if !self.list_sessions().contains(&session_id.to_string()) {
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
        if !self.list_sessions().contains(&session_id.to_string()) {
            return Err(PtyError::SessionNotFound(session_id.to_string()));
        }

        self.output_hub
            .subscribe_with_sequence_ranges(session_id, after_sequence)
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
        self.output_hub.remove_session(session_id);
        let result = self.pty_manager.close_session(session_id).await;
        if result.is_ok() {
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
        self.pty_manager.signal(session_id, TerminalSignal::Stop)?;
        self.lifecycle.lock().mark_suspended(session_id.to_string());
        Ok(())
    }

    pub async fn resume_session(&self, session_id: &str) -> Result<(), PtyError> {
        if self.remote.contains(session_id) || super::paired_runtime::Runtime::owns(session_id) {
            return Err(PtyError::Other(
                "Session resume is supported only for local PTYs".into(),
            ));
        }
        self.pty_manager
            .signal(session_id, TerminalSignal::Continue)?;
        self.lifecycle.lock().mark_running(session_id.to_string());
        Ok(())
    }

    pub(crate) async fn close_machine_session(
        &self,
        session_id: &str,
        authorize: Arc<dyn Fn() -> Result<(), String> + Send + Sync>,
    ) -> Result<(), PtyError> {
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

    /// Whether a native PTY this daemon owns is registered under this id. It is false
    /// for every other owner, so a consumer asking "does this daemon serve the session"
    /// must OR in those owners itself, as `SessionRouter::is_local_session` does.
    pub fn has_native_pty_session(&self, session_id: &str) -> bool {
        self.pty_manager.get_session(session_id).is_some()
    }

    pub fn session_info(&self, session_id: &str) -> Option<SessionInfo> {
        self.session_status(session_id).map(|status| status.info())
    }

    /// The shell's OS pid. This takes the session's child handle, which a reap holds.
    pub fn session_pid(&self, session_id: &str) -> Option<u32> {
        self.pty_manager.get_session(session_id)?.pid()
    }

    pub fn session_input(&self, session_id: &str) -> Option<SessionInput> {
        self.pty_manager
            .get_session(session_id)
            .map(|session| SessionInput(SessionOwner::Local(session)))
    }

    pub fn session_status(&self, session_id: &str) -> Option<SessionStatus> {
        self.pty_manager
            .get_session(session_id)
            .map(|session| SessionStatus(SessionOwner::Local(session)))
    }

    pub(crate) fn foreground_source(
        &self,
        session_id: &str,
    ) -> Option<super::foreground::ForegroundSource> {
        self.pty_manager
            .get_session(session_id)
            .map(super::foreground::ForegroundSource::Local)
    }
}
