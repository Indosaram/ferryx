use crate::daemon::session_lifecycle::{SessionLifecycleRegistry, SessionProcessState};
use crate::terminal::output_hub::{HistoryRange, SessionAttachment, TerminalOutputHub};
use crate::terminal::{PtyError, PtyManager, PtySession, TerminalSignal};
use crate::worktree::manager::WorktreeManager;
use parking_lot::Mutex;
use portable_pty::CommandBuilder;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::{broadcast, watch};

#[derive(Clone)]
pub struct TerminalService {
    pty_manager: Arc<PtyManager>,
    output_hub: Arc<TerminalOutputHub>,
    remote: Arc<super::remote::RemoteRuntime>,
    paired: Arc<super::paired_runtime::Runtime>,
    lifecycle: Arc<Mutex<SessionLifecycleRegistry>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PumpToken(u64);

#[derive(Default)]
pub(crate) struct PumpTokenFence {
    next: std::sync::atomic::AtomicU64,
    active: Mutex<Option<PumpToken>>,
}

impl PumpTokenFence {
    pub(crate) fn install(&self) -> PumpToken {
        let token = PumpToken(self.next.fetch_add(1, std::sync::atomic::Ordering::AcqRel).wrapping_add(1));
        *self.active.lock() = Some(token);
        token
    }

    pub(crate) fn is_active(&self, token: PumpToken) -> bool { *self.active.lock() == Some(token) }

    pub(crate) fn remove(&self, token: PumpToken) -> bool {
        let mut active = self.active.lock();
        if *active == Some(token) { *active = None; true } else { false }
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
    pub fn new(pty_manager: Arc<PtyManager>, output_hub: Arc<TerminalOutputHub>) -> Self {
        pty_manager.set_output_hub(output_hub.clone());
        let service = Self {
            remote: Arc::new(super::remote::RemoteRuntime::new(output_hub.clone())),
            paired: Arc::new(super::paired_runtime::Runtime::default()),
            pty_manager,
            output_hub,
            lifecycle: Arc::new(Mutex::new(SessionLifecycleRegistry::default())),
        };
        // The daemon owns the PTYs and their output hub, so the private QA
        // channel is installed here as well; without the runner env this is a
        // no-op and the daemon keeps no QA surface.
        #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
        super::qa_liveness::start(service.clone());
        service
    }

    pub fn paired(&self) -> &Arc<super::paired_runtime::Runtime> {
        &self.paired
    }

    pub fn set_paired_agent_sink(&self, sink: Arc<dyn super::remote::AgentStateSink>) {
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
}
