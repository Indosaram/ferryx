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
fn control_disabled_for_qa() -> bool {
    std::env::var("FERRYX_QA_DISABLE_CONTROL").as_deref() == Ok("1")
}

#[cfg(not(feature = "local-split-qa"))]
fn control_disabled_for_qa() -> bool {
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
    // TODO(todo 8): a HandoverCandidate inherits the predecessor's control listener fd through
    // the authority transfer and starts accept_control_connections on it after CommitHandoverOk.
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
            remove_stale_socket_after_lock(&path)?;
            let listener = UnixListener::bind(&path).map_err(|e| {
                format!("Failed to bind control socket at {}: {e}", path.display())
            })?;
            validate_safe_ownership_and_type(&path, RuntimeNodeKind::Socket)?;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
                .map_err(|error| format!("Failed to secure control socket: {error}"))?;
            let server = Arc::clone(self);
            tokio::spawn(async move {
                loop {
                    match listener.accept().await {
                        Ok((stream, _)) => {
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
                // TODO(todo 8): the candidate sends Hello over control first and refuses with
                // CODE_HANDOVER_WIRE_UNSUPPORTED unless these ops list handover-v1; todo 8 owns
                // that path: "sends the upgrade through the control channel (todo 15) as
                // RequestUpgrade { staged_executable, generation_key }".
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
            // TODO(todo 8): unix routes to the RequestUpgrade handover path; Windows stays
            // UPGRADE_UNSUPPORTED permanently.
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
}
