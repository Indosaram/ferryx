//! Server side of the frozen control-v1 channel (see daemon/control.rs). Kept in its own file so
//! the data-protocol handler in server.rs stays untouched by control changes.

use super::*;
use crate::daemon::control::{
    self, ControlFailure, ControlReply, ControlRequest, HelloReply, InventoryReply,
    ShutdownDecision, CODE_UPGRADE_UNSUPPORTED,
};

#[cfg(test)]
#[path = "control_wire_tests.rs"]
mod control_wire_tests;

/// Under local-split-qa only: skip binding the control endpoint (todo 12, Q09).
#[cfg(feature = "local-split-qa")]
pub(crate) fn control_disabled_for_qa() -> bool {
    std::env::var("FERRYX_QA_DISABLE_CONTROL").as_deref() == Ok("1")
}

#[cfg(not(feature = "local-split-qa"))]
pub(crate) fn control_disabled_for_qa() -> bool {
    false
}

pub(crate) fn control_socket_path() -> PathBuf {
    get_runtime_dir().join(control::CONTROL_SOCKET_FILE)
}

/// Process start time (unix ms), captured the first time the control channel is bound.
fn daemon_start_time_ms() -> u64 {
    static START: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *START.get_or_init(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    })
}

/// This daemon's generation key.
// TODO(todo 3): return the per-install generation key once todo 3 lands it; until then the
// Hello answers null and the marker is keyed by pid.
fn generation_key() -> Option<String> {
    None
}

/// What the connection handler must do after its response is written and flushed.
enum AfterFlush {
    Nothing,
    /// Re-check the retirement predicate (RetireWhenIdle).
    CheckRetirement,
    /// Shutdown accepted: hand the exit to the single retirement task.
    Shutdown,
}

impl DaemonServer {
    /// Binds the control endpoint on an ordinary start. Called right after the data listener is
    /// bound, i.e. only while this process holds the daemon locks. A HandoverCandidate never calls
    /// this.
    pub(crate) async fn bind_control_endpoint(self: &Arc<Self>) -> Result<(), String> {
        if control_disabled_for_qa() {
            tracing::warn!("FERRYX_QA_DISABLE_CONTROL=1: control endpoint not bound");
            return Ok(());
        }
        daemon_start_time_ms();
        self.spawn_control_retirement_task_in(get_runtime_dir());

        #[cfg(unix)]
        {
            let path = control_socket_path();
            use crate::daemon::handover::AuthorityListenerRole;
            // A committed HandoverCandidate serves the control listener it inherited from its
            // predecessor (todo 8) instead of unlinking and re-binding the path.
            let listener = match self
                .handover_manager
                .take_inherited_listener(AuthorityListenerRole::Control)
            {
                Some(inherited) => UnixListener::from_std(inherited).map_err(|e| {
                    format!("Failed to serve the inherited control listener: {e}")
                })?,
                None => {
                    remove_stale_socket_after_lock(&path)?;
                    let listener = UnixListener::bind(&path).map_err(|e| {
                        format!("Failed to bind control socket at {}: {e}", path.display())
                    })?;
                    validate_safe_ownership_and_type(&path, RuntimeNodeKind::Socket)?;
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
                        .map_err(|error| format!("Failed to secure control socket: {error}"))?;
                    listener
                }
            };
            {
                use std::os::fd::AsRawFd;
                self.handover_manager
                    .register_authority_listener(AuthorityListenerRole::Control, listener.as_raw_fd())?;
            }
            let server = Arc::clone(self);
            tokio::spawn(async move {
                let manager = Arc::clone(&server.handover_manager);
                loop {
                    if !manager.accept_gate_open() {
                        manager.wait_accept_open().await;
                        continue;
                    }
                    let accepted = tokio::select! {
                        accepted = listener.accept() => accepted,
                        () = manager.wait_accept_closed() => continue,
                    };
                    match accepted {
                        Ok((stream, _)) => {
                            manager.note_accept(AuthorityListenerRole::Control);
                            let server = Arc::clone(&server);
                            tokio::spawn(async move {
                                server.handle_control_connection(stream, None).await;
                            });
                        }
                        Err(error) => {
                            tracing::error!("Control accept error: {error}");
                            break;
                        }
                    }
                }
            });
        }

        #[cfg(not(unix))]
        {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .map_err(|e| format!("Failed to bind control listener on localhost: {e}"))?;
            let port = listener
                .local_addr()
                .map_err(|e| format!("Failed to get control port: {e}"))?
                .port();
            let token = generate_transport_token();
            let runtime_dir = get_runtime_dir();
            let port_path = runtime_dir.join(control::CONTROL_PORT_FILE);
            let token_path = runtime_dir.join(control::CONTROL_TOKEN_FILE);
            let published_token = token.clone();
            // Same order as the data rendezvous: old token dropped, token written, port last.
            crate::ipc::run_blocking(move || {
                publish_transport_rendezvous(&port_path, &token_path, port, &published_token)
                    .map_err(|e| crate::ipc::IpcError::internal(e.to_string()))
            })
            .await
            .map_err(|e| format!("Failed to publish control port and token: {e}"))?;
            let token = Arc::new(token);
            let server = Arc::clone(self);
            tokio::spawn(async move {
                loop {
                    match listener.accept().await {
                        Ok((stream, _)) => {
                            let server = Arc::clone(&server);
                            let token = Arc::clone(&token);
                            tokio::spawn(async move {
                                server
                                    .handle_control_connection(stream, Some(token.as_str()))
                                    .await;
                            });
                        }
                        Err(error) => {
                            tracing::error!("Control accept error: {error}");
                            break;
                        }
                    }
                }
            });
        }

        tracing::info!("daemon control-v1 endpoint bound");
        Ok(())
    }

    /// One task per daemon and the only retirement path for both RetireWhenIdle and Shutdown:
    /// waits until a Retiring daemon is empty (nothing spawning or in flight, every response
    /// already flushed, and nothing live unless a Shutdown confirmed those sessions), then writes
    /// the marker into runtime_dir, shuts down gracefully and exits 0. The retiring_signalled
    /// swap in handover.rs makes the signal one-shot, so this runs at most once.
    pub(crate) fn spawn_control_retirement_task_in(self: &Arc<Self>, runtime_dir: PathBuf) {
        let server = Arc::clone(self);
        let ready = self.handover_manager.retiring_ready();
        tokio::spawn(async move {
            ready.notified().await;
            let reason = if server.handover_manager.control_shutdown_requested() {
                "control-v1 Shutdown"
            } else {
                "control-v1 RetireWhenIdle"
            };
            server.finish_control_retirement(&runtime_dir, reason).await;
        });
    }

    async fn finish_control_retirement(&self, runtime_dir: &Path, reason: &str) {
        let marker = runtime_dir.join(format!(
            "retired-{}.json",
            generation_key().unwrap_or_else(|| std::process::id().to_string())
        ));
        let body = serde_json::json!({
            "pid": std::process::id(),
            "reason": reason,
            "generation_key": generation_key(),
        })
        .to_string();
        if let Err(error) = crate::ipc::run_blocking(move || {
            fs::write(&marker, body).map_err(|e| crate::ipc::IpcError::internal(e.to_string()))
        })
        .await
        {
            tracing::error!(%error, "Failed to write retirement marker");
        }
        self.shutdown_gracefully(reason).await;
        self.handover_manager.run_retirement_action();
    }

    async fn handle_control_connection<S>(self: Arc<Self>, stream: S, expected_token: Option<&str>)
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    {
        let (read_half, mut write_half) = tokio::io::split(stream);
        let mut lines = BufReader::new(read_half).lines();
        let mut first_frame = true;
        while let Ok(Some(line)) = lines.next_line().await {
            if first_frame {
                first_frame = false;
                if let Some(expected) = expected_token {
                    let accepted = control::parse_object(&line)
                        .map(|frame| control::first_frame_token_matches(&frame, expected))
                        .unwrap_or(false);
                    if !accepted {
                        tracing::warn!("Control connection rejected: token mismatch");
                        return;
                    }
                }
            }
            // The guard is held until after write_all + flush, so a retirement never exits with
            // this response unwritten.
            let guard = match self
                .handover_manager
                .retain_request(Arc::clone(&self.terminal_service))
            {
                Ok(guard) => guard,
                Err(_) => return,
            };
            let (reply, after) = match control::decode_request(&line) {
                // RequestUpgrade needs the async handover path (todo 8); every other op is decided
                // synchronously below.
                Ok(ControlRequest::RequestUpgrade {
                    staged_executable,
                    generation_key,
                }) => (
                    self.control_request_upgrade(staged_executable, generation_key)
                        .await,
                    AfterFlush::Nothing,
                ),
                Ok(request) => self.dispatch_control(request),
                Err(error) => (error.reply(), AfterFlush::Nothing),
            };
            let frame = control::to_line(&control::encode_reply(&reply));
            if write_half.write_all(frame.as_bytes()).await.is_err() {
                return;
            }
            let _ = write_half.flush().await;
            match after {
                AfterFlush::Nothing => drop(guard),
                // Dropping the last request guard re-checks the predicate (handover.rs Drop).
                AfterFlush::CheckRetirement => {
                    drop(guard);
                    self.handover_manager
                        .check_retirement_if_empty(&self.terminal_service);
                }
                // The response is flushed; mark the Shutdown before this guard drops so the guard
                // drop (or the last other in-flight request) signals the retirement task.
                AfterFlush::Shutdown => {
                    self.handover_manager.mark_control_shutdown();
                    drop(guard);
                    return;
                }
            }
        }
    }

    fn dispatch_control(&self, request: ControlRequest) -> (ControlReply, AfterFlush) {
        match request {
            ControlRequest::Hello => {
                let mut ops: Vec<String> =
                    control::FROZEN_OPS.iter().map(|op| op.to_string()).collect();
                // The handover wire is frozen as handover-v1 (tests/fixtures/handover-v1.json);
                // only unix daemons hand over.
                if cfg!(unix) {
                    ops.push(control::HANDOVER_V1_OP.to_string());
                }
                (
                    ControlReply::Hello(HelloReply {
                        daemon_version: env!("CARGO_PKG_VERSION").to_string(),
                        protocol_version: DAEMON_PROTOCOL_VERSION,
                        pid: std::process::id(),
                        start_time: daemon_start_time_ms(),
                        generation_key: generation_key(),
                        epoch: self.epoch,
                        ops,
                    }),
                    AfterFlush::Nothing,
                )
            }
            ControlRequest::Inventory => {
                let (live_local, live_remote, live_paired) =
                    crate::daemon::handover::live_session_counts(&self.terminal_service);
                (
                    ControlReply::Inventory(InventoryReply {
                        live_local,
                        live_remote,
                        live_paired,
                    }),
                    AfterFlush::Nothing,
                )
            }
            // Routed to control_request_upgrade by handle_control_connection; kept as the
            // Windows-equivalent answer for any other caller.
            ControlRequest::RequestUpgrade { .. } => (
                ControlReply::Failed(ControlFailure::code(CODE_UPGRADE_UNSUPPORTED)),
                AfterFlush::Nothing,
            ),
            ControlRequest::RetireWhenIdle => {
                let live = self
                    .handover_manager
                    .control_retire_when_idle(&self.terminal_service);
                (ControlReply::Live { live }, AfterFlush::CheckRetirement)
            }
            ControlRequest::Shutdown {
                confirm_live_sessions,
            } => match self
                .handover_manager
                .control_shutdown(&self.terminal_service, confirm_live_sessions)
            {
                ShutdownDecision::Refuse(code) => (
                    ControlReply::Failed(ControlFailure::code(code)),
                    AfterFlush::Nothing,
                ),
                ShutdownDecision::CountChanged { live, .. } => (
                    ControlReply::Failed(ControlFailure {
                        code: control::CODE_COUNT_CHANGED.to_string(),
                        live: Some(live),
                    }),
                    AfterFlush::Nothing,
                ),
                ShutdownDecision::Proceed { live } => {
                    (ControlReply::Live { live }, AfterFlush::Shutdown)
                }
            },
            ControlRequest::Unsupported { .. } => (
                ControlReply::Failed(ControlFailure::code(control::CODE_UNSUPPORTED_OP)),
                AfterFlush::Nothing,
            ),
        }
    }

    /// Windows has no handover candidate: the live owner is never replaced by an upgrade.
    #[cfg(not(unix))]
    async fn control_request_upgrade(
        self: &Arc<Self>,
        _staged_executable: String,
        _generation_key: Option<String>,
    ) -> ControlReply {
        ControlReply::Failed(ControlFailure::code(CODE_UPGRADE_UNSUPPORTED))
    }

    /// RequestUpgrade { staged_executable, generation_key } (todo 8). (a) the staged executable
    /// must lie under daemon_runtime_root() inside the generation dir named by generation_key;
    /// (b) prepare_handover, then handover-request.json (nonce, 30 s expiry, mode 0600);
    /// (c) supervisor::start_handover_candidate starts the successor generation's own OS job. The
    /// predecessor keeps serving everything until the candidate commits; any refusal leaves it
    /// Active. No in-place re-exec and no child process of this daemon is ever used here.
    #[cfg(unix)]
    async fn control_request_upgrade(
        self: &Arc<Self>,
        staged_executable: String,
        generation_key: Option<String>,
    ) -> ControlReply {
        use crate::daemon::handover_request::{self as request_file, CandidateStart, HandoverRequest};
        let failed = |code: &str| ControlReply::Failed(ControlFailure::code(code));
        let Some(key) = generation_key else {
            tracing::warn!("RequestUpgrade refused: no generation_key");
            return failed(control::CODE_BAD_REQUEST);
        };
        let Some(root) = daemon_runtime_root() else {
            return failed(CODE_UPGRADE_UNSUPPORTED);
        };
        let executable = PathBuf::from(&staged_executable);
        // Everything that touches the filesystem or /proc runs off the Tokio workers: staged
        // executable validation, this daemon's own generation key (current_exe + canonicalize)
        // and its start time (/proc on Linux).
        let pid = std::process::id();
        let checked = {
            let (root, executable, key) = (root.clone(), executable.clone(), key.clone());
            crate::ipc::run_blocking(move || {
                request_file::validate_staged_executable(&root, &executable, &key)
                    .map_err(crate::ipc::IpcError::internal)?;
                Ok((
                    request_file::own_generation_key(),
                    crate::daemon::owner_admission::process_start_time(pid),
                ))
            })
            .await
        };
        let (own_key, start_time) = match checked {
            Ok(checked) => checked,
            Err(error) => {
                tracing::warn!(%error, staged_executable, "RequestUpgrade refused: staged executable rejected");
                return failed(control::CODE_BAD_REQUEST);
            }
        };
        if own_key.as_deref() == Some(key.as_str()) {
            tracing::info!(generation_key = %key, "RequestUpgrade refused: already running this generation");
            return failed(control::CODE_UPGRADE_REFUSED);
        }
        let Some(start_time) = start_time else {
            return failed(control::CODE_UPGRADE_REFUSED);
        };
        // prepare_handover removes, binds and chmods the legacy socket.
        let prepared = {
            let (manager, terminals) = (Arc::clone(&self.handover_manager), Arc::clone(&self.terminal_service));
            crate::ipc::run_blocking(move || {
                manager.prepare_handover(&terminals).map_err(crate::ipc::IpcError::internal)
            })
            .await
        };
        let (legacy_path, _, legacy_listener) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                tracing::warn!(%error, "RequestUpgrade refused: handover could not be prepared");
                return failed(control::CODE_UPGRADE_REFUSED);
            }
        };
        let request = HandoverRequest::new(
            legacy_path,
            pid,
            start_time,
            self.epoch,
            key,
            executable,
            crate::daemon::owner_admission::unix_now_ms(),
        );
        let runtime_dir = get_runtime_dir();
        let transfers_before = request_file::transfers_started();
        let outcome = {
            let (runtime_dir, request) = (runtime_dir.clone(), request.clone());
            crate::ipc::run_blocking(move || {
                request_file::write_request(&runtime_dir, &request)
                    .and_then(|()| crate::daemon::supervisor::start_handover_candidate(&request))
                    .map_err(crate::ipc::IpcError::internal)
            })
            .await
        };
        match outcome {
            Ok(CandidateStart::Started) => {
                tracing::info!(
                    generation_key = %request.generation_key,
                    "RequestUpgrade accepted: successor generation job started as handover candidate"
                );
                Arc::clone(self).serve_legacy_handover_socket(legacy_listener);
                Arc::clone(self).abandon_handover_if_never_claimed(runtime_dir, transfers_before);
                ControlReply::UpgradeAccepted
            }
            refused => {
                tracing::warn!(?refused, "RequestUpgrade refused: successor job not started; predecessor keeps serving");
                drop(legacy_listener);
                self.abandon_prepared_handover(runtime_dir).await;
                failed(control::CODE_UPGRADE_REFUSED)
            }
        }
    }

    /// Pre-commit rollback for a request no candidate acted on: removes the request, records the
    /// abort decision and returns to Active. Only valid while still Prepared.
    #[cfg(unix)]
    async fn abandon_prepared_handover(&self, runtime_dir: PathBuf) {
        let manager = Arc::clone(&self.handover_manager);
        let result = crate::ipc::run_blocking(move || {
            crate::daemon::handover_request::remove_request(&runtime_dir);
            manager.abort_handover().map_err(crate::ipc::IpcError::internal)
        })
        .await;
        if let Err(error) = result {
            tracing::warn!(%error, "Could not abandon the prepared handover");
        }
    }

    /// When the request expires and no candidate ever asked for a session transfer, the
    /// predecessor leaves Prepared on its own (the candidate exits 0 on an expired request).
    #[cfg(unix)]
    fn abandon_handover_if_never_claimed(self: Arc<Self>, runtime_dir: PathBuf, transfers_before: u64) {
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(
                crate::daemon::handover_request::REQUEST_TTL_MS,
            ))
            .await;
            if crate::daemon::handover_request::transfers_started() == transfers_before
                && self.handover_manager.status() == crate::daemon::handover::HandoverStatus::Prepared
            {
                tracing::warn!("Handover request expired unclaimed; predecessor stays the owner");
                self.abandon_prepared_handover(runtime_dir).await;
            }
        });
    }
}
