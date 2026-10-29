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

/// This daemon's generation key: the same value owner.json carries
/// (owner_admission::current_generation_key).
fn generation_key() -> Option<String> {
    Some(crate::daemon::owner_admission::current_generation_key())
}

/// The command that starts the staged successor generation of an idle upgrade (todo 27):
/// `<staged exe> --daemon`, detached, with FERRYX_DAEMON_SUCCESSOR=1 so it waits for this
/// daemon's instance lock instead of failing fast, and announces that wait on stdout
/// (handover::SUCCESSOR_WAITING_TOKEN). It follows this daemon's process (PREDECESSOR_PID_ENV)
/// and starts its lock window only once this daemon has left, however long the in-flight drain
/// takes. It inherits the runtime and data dir overrides but not this daemon's watchdog identity:
/// that watchdog stops on the retirement marker this daemon leaves.
#[cfg(not(unix))]
fn idle_successor_command(executable: &Path) -> tokio::process::Command {
    use crate::daemon::owner_admission::{SUPERVISOR_PID_ENV, SUPERVISOR_START_ENV};
    let mut successor = tokio::process::Command::new(executable);
    successor
        .arg("--daemon")
        .env("FERRYX_DAEMON_SUCCESSOR", "1")
        .env(
            crate::daemon::handover::PREDECESSOR_PID_ENV,
            std::process::id().to_string(),
        )
        .env_remove(SUPERVISOR_PID_ENV)
        .env_remove(SUPERVISOR_START_ENV);
    crate::util::detach_launched_child(successor.as_std_mut());
    successor
}

/// Bound on the successor's readiness announcement. It is printed before the successor's first
/// lock attempt, so a working successor announces within its own startup time.
#[cfg(not(unix))]
const SUCCESSOR_READY_TIMEOUT: Duration = Duration::from_secs(15);

/// How long before the successor's own give-up time this daemon stops committing to leave (todo
/// 27, round 4). The successor stops following this daemon PREDECESSOR_EXIT_BOUND after it
/// announced; a retirement that starts later than this margin before that instant could have the
/// successor give up while the marker is written and the daemon shuts down, leaving none. The
/// margin covers the marker write, the graceful shutdown and the process exit.
#[cfg(any(not(unix), test))]
const SUCCESSOR_GIVE_UP_MARGIN: Duration = Duration::from_secs(10);

/// The running successor of an accepted idle upgrade.
#[cfg(any(not(unix), test))]
pub(super) struct IdleSuccessor {
    pub(super) child: tokio::process::Child,
    /// Taken right before the spawn, so it is never later than the successor's announcement,
    /// from which the successor's own PREDECESSOR_EXIT_BOUND runs.
    pub(super) spawned_at: std::time::Instant,
}

/// Whether an accepted successor still running at `now` gives up too soon for this daemon to
/// leave: it stops following this daemon no earlier than spawned_at + `bound`, and never takes
/// the lock after that.
#[cfg(any(not(unix), test))]
fn successor_gives_up_too_soon(
    spawned_at: std::time::Instant,
    now: std::time::Instant,
    bound: Duration,
    margin: Duration,
) -> bool {
    now + margin >= spawned_at + bound
}

/// Starts the successor and waits, bounded, for the event that proves it will take over: its
/// SUCCESSOR_WAITING_TOKEN line, printed once it is alive and blocked on this daemon's instance
/// lock. Owning the endpoint cannot be the signal, because the successor only gets the lock after
/// this daemon exits. A successor that ends (stdout EOF) or stays silent until the deadline is a
/// failure; it holds no lock then, and a silent one is stopped so it cannot take over later.
/// Returns the running successor, whose handle the daemon keeps until it retires, or the typed
/// refusal reason with a detail for the log.
#[cfg(any(not(unix), test))]
async fn start_announced_successor(
    mut command: tokio::process::Command,
    ready_timeout: Duration,
) -> Result<IdleSuccessor, (&'static str, String)> {
    use crate::daemon::handover::SUCCESSOR_WAITING_TOKEN;
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let spawned_at = std::time::Instant::now();
    let mut child = command.spawn().map_err(|error| {
        (
            control::REASON_SUCCESSOR_NOT_STARTED,
            format!("spawn failed: {error}"),
        )
    })?;
    let pid = child.id().unwrap_or_default();
    let Some(stdout) = child.stdout.take() else {
        let _ = child.start_kill();
        return Err((
            control::REASON_SUCCESSOR_NOT_STARTED,
            "no stdout pipe".into(),
        ));
    };
    let mut lines = BufReader::new(stdout).lines();
    let announced = tokio::time::timeout(ready_timeout, async {
        while let Ok(Some(line)) = lines.next_line().await {
            if line.trim() == SUCCESSOR_WAITING_TOKEN {
                return true;
            }
        }
        false
    })
    .await;
    match announced {
        Ok(true) => Ok(IdleSuccessor { child, spawned_at }),
        Ok(false) => {
            // stdout closed without the announcement: the successor ended (or gave up its
            // stdout); it never reached the lock wait.
            let _ = child.start_kill();
            let status = child.wait().await;
            Err((
                control::REASON_SUCCESSOR_EXITED,
                format!("successor {pid} ended before announcing readiness: {status:?}"),
            ))
        }
        Err(_) => {
            let _ = child.kill().await;
            Err((
                control::REASON_SUCCESSOR_NOT_READY,
                format!("successor {pid} did not announce readiness within {ready_timeout:?}"),
            ))
        }
    }
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
        // Resolve (and cache) the generation key off the workers; Hello reads it synchronously.
        crate::ipc::run_blocking(|| Ok(crate::daemon::owner_admission::current_generation_key()))
            .await
            .map_err(|error| format!("Failed to resolve the generation key: {error}"))?;
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
            self.note_published_endpoint(&path);
            {
                use std::os::fd::AsRawFd;
                self.handover_manager
                    .register_authority_listener(AuthorityListenerRole::Control, listener.as_raw_fd())?;
            }
            let server = Arc::clone(self);
            tokio::spawn(async move {
                let manager = Arc::clone(&server.handover_manager);
                loop {
                    // None: the successor serves this listener now; close this copy.
                    let Some(accepted) = manager.gated_accept(|| listener.accept()).await else {
                        drop(listener);
                        break;
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
            self.note_published_endpoint(&runtime_dir.join(control::CONTROL_PORT_FILE));
            self.note_published_endpoint(&runtime_dir.join(control::CONTROL_TOKEN_FILE));
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
    /// swap in handover.rs makes the signal one-shot, so this exits at most once; a retirement
    /// cancelled because the idle-upgrade successor ended re-arms the signal and waits again.
    pub(crate) fn spawn_control_retirement_task_in(self: &Arc<Self>, runtime_dir: PathBuf) {
        let server = Arc::clone(self);
        let ready = self.handover_manager.retiring_ready();
        tokio::spawn(async move {
            loop {
                ready.notified().await;
                let reason = if server.handover_manager.control_shutdown_requested() {
                    "control-v1 Shutdown"
                } else {
                    "control-v1 RetireWhenIdle"
                };
                if server.finish_control_retirement(&runtime_dir, reason).await {
                    break;
                }
            }
        });
    }

    /// Writes the marker, shuts down gracefully and runs the retirement action; true once it did.
    /// After an accepted idle upgrade the successor must still be running at this point: it is
    /// the daemon that takes over. One that ended (it gave up on the lock, or crashed) cancels the
    /// retirement before anything is torn down, so this daemon reopens session creation and keeps
    /// serving instead of leaving the machine without a daemon (todo 27, round 3). So does one
    /// that is about to give up on its own (round 4): leaving then could end both daemons.
    async fn finish_control_retirement(&self, runtime_dir: &Path, reason: &str) -> bool {
        #[cfg(any(not(unix), test))]
        if let Some(detail) = self.idle_successor_gone(std::time::Instant::now()) {
            tracing::warn!(
                %detail,
                "Retirement cancelled: the idle-upgrade successor ended before this daemon left; \
                 session creation reopens and this daemon keeps serving"
            );
            self.handover_manager.reopen_after_successor_exit();
            return false;
        }
        let runtime_dir = runtime_dir.to_path_buf();
        let marker_reason = reason.to_string();
        if let Err(error) = crate::ipc::run_blocking(move || {
            // The watchdog reads retired-<generation_key>.json and checks the pid in its body.
            let key = crate::daemon::owner_admission::current_generation_key();
            let marker = runtime_dir.join(format!("retired-{key}.json"));
            let body = serde_json::json!({
                "pid": std::process::id(),
                "reason": marker_reason,
                "generation_key": key,
            })
            .to_string();
            fs::write(&marker, body).map_err(|e| crate::ipc::IpcError::internal(e.to_string()))
        })
        .await
        {
            tracing::error!(%error, "Failed to write retirement marker");
        }
        self.shutdown_gracefully(reason).await;
        self.handover_manager.run_retirement_action();
        true
    }

    /// Why an accepted idle upgrade's successor cannot take over from a daemon leaving at `now`:
    /// it is no longer running, or it gives up following this daemon within
    /// SUCCESSOR_GIVE_UP_MARGIN. None while it can, or when no idle upgrade was accepted. An
    /// unreadable state counts as gone: staying keeps a daemon, leaving might not. The record is
    /// cleared whenever a reason is returned.
    #[cfg(any(not(unix), test))]
    fn idle_successor_gone(&self, now: std::time::Instant) -> Option<String> {
        use crate::daemon::handover::PREDECESSOR_EXIT_BOUND;
        let mut slot = self.idle_successor.lock();
        let successor = slot.as_mut()?;
        let pid = successor.child.id();
        let detail = match successor.child.try_wait() {
            Ok(None)
                if successor_gives_up_too_soon(
                    successor.spawned_at,
                    now,
                    PREDECESSOR_EXIT_BOUND,
                    SUCCESSOR_GIVE_UP_MARGIN,
                ) =>
            {
                // It is still waiting for this daemon's exit and holds no lock; stopped so it
                // cannot take over later, alongside the successor of a later upgrade.
                let _ = successor.child.start_kill();
                format!(
                    "successor {pid:?} stops following this daemon {PREDECESSOR_EXIT_BOUND:?} \
                     after it started, within {SUCCESSOR_GIVE_UP_MARGIN:?} from now"
                )
            }
            Ok(None) => return None,
            Ok(Some(status)) => format!("successor {pid:?} exited: {status}"),
            Err(error) => format!("successor {pid:?} state unreadable: {error}"),
        };
        *slot = None;
        Some(detail)
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
                // RequestUpgrade needs the async handover path (todo 8) or, on Windows, the idle
                // upgrade (todo 27); every other op is decided synchronously below.
                Ok(ControlRequest::RequestUpgrade {
                    staged_executable,
                    generation_key,
                }) => {
                    self.control_request_upgrade(staged_executable, generation_key)
                        .await
                }
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
                        reason: None,
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

    /// Windows has no handover candidate (todo 27): a daemon with live sessions is never
    /// replaced, and an idle one hands its endpoint to the staged successor generation
    /// (idle_request_upgrade). FERRYX_DAEMON_IDLE_UPGRADE=0 keeps the refusal-only behavior.
    #[cfg(not(unix))]
    async fn control_request_upgrade(
        self: &Arc<Self>,
        staged_executable: String,
        generation_key: Option<String>,
    ) -> (ControlReply, AfterFlush) {
        if !crate::daemon::handover::idle_upgrade_requested() {
            tracing::warn!(
                "RequestUpgrade refused: FERRYX_DAEMON_IDLE_UPGRADE disables the idle upgrade; \
                 restart Ferryx to finish updating"
            );
            return (
                ControlReply::Failed(ControlFailure::code(CODE_UPGRADE_UNSUPPORTED)),
                AfterFlush::Nothing,
            );
        }
        self.idle_request_upgrade(
            staged_executable,
            generation_key,
            idle_successor_command,
            SUCCESSOR_READY_TIMEOUT,
        )
        .await
    }

    /// RequestUpgrade on a daemon without session transfer (todo 27). The staged executable must
    /// lie in the generation dir named by generation_key under daemon_runtime_root(), as on Unix.
    /// With a live session the upgrade is refused with UPGRADE_REFUSED carrying the live count,
    /// which the GUI turns into "restart to finish updating". With none, the zero-count Shutdown
    /// gate closes session creation atomically, the successor built by `successor` is started and
    /// must announce, within `ready_timeout`, that it is waiting for this daemon's instance lock
    /// (start_announced_successor). Only then does the reply ask the connection handler to
    /// retire: the single retirement task exits once the reply is flushed and nothing is in
    /// flight, instead of after a fixed sleep. A successor that cannot be started, ends at once or
    /// stays silent reopens the gate and the daemon keeps serving; the refusal carries the typed
    /// reason.
    #[cfg(any(not(unix), test))]
    async fn idle_request_upgrade<F>(
        self: &Arc<Self>,
        staged_executable: String,
        generation_key: Option<String>,
        successor: F,
        ready_timeout: Duration,
    ) -> (ControlReply, AfterFlush)
    where
        F: FnOnce(&Path) -> tokio::process::Command,
    {
        use crate::daemon::handover::{upgrade_action, HandoverStatus, UpgradeAction};
        let refuse = |code: &str, live: Option<u32>| {
            let failure = ControlFailure {
                code: code.to_string(),
                live,
                reason: None,
            };
            (ControlReply::Failed(failure), AfterFlush::Nothing)
        };
        let Some(key) = generation_key else {
            tracing::warn!("RequestUpgrade refused: no generation_key");
            return refuse(control::CODE_BAD_REQUEST, None);
        };
        let Some(root) = daemon_runtime_root() else {
            return refuse(CODE_UPGRADE_UNSUPPORTED, None);
        };
        let executable = PathBuf::from(&staged_executable);
        let checked = {
            let (executable, key) = (executable.clone(), key.clone());
            crate::ipc::run_blocking(move || {
                crate::daemon::handover_request::validate_staged_executable(
                    &root,
                    &executable,
                    &key,
                )
                .map_err(crate::ipc::IpcError::internal)?;
                Ok(crate::daemon::handover_request::own_generation_key())
            })
            .await
        };
        let own_key = match checked {
            Ok(own_key) => own_key,
            Err(error) => {
                tracing::warn!(%error, staged_executable, "RequestUpgrade refused: staged executable rejected");
                return refuse(control::CODE_BAD_REQUEST, None);
            }
        };
        if own_key.as_deref() == Some(key.as_str()) {
            tracing::info!(generation_key = %key, "RequestUpgrade refused: already running this generation");
            return refuse(control::CODE_UPGRADE_REFUSED, None);
        }
        let live = crate::daemon::handover::live_session_count(&self.terminal_service);
        if let UpgradeAction::Refuse { live_sessions } = upgrade_action(live as usize, false) {
            tracing::warn!(
                live_sessions,
                "RequestUpgrade refused: this platform cannot transfer live sessions; \
                 restart Ferryx to finish updating"
            );
            return refuse(control::CODE_UPGRADE_REFUSED, Some(live));
        }
        if self.handover_manager.status() != HandoverStatus::Active {
            return refuse(control::CODE_UPGRADE_REFUSED, None);
        }
        // Same atomic gate as Shutdown { confirm_live_sessions: 0 }: no session can appear after
        // this count, and a session that appeared since the check above refuses the upgrade.
        match self
            .handover_manager
            .control_shutdown(&self.terminal_service, 0)
        {
            ShutdownDecision::Proceed { .. } => {}
            ShutdownDecision::CountChanged { live, .. } => {
                return refuse(control::CODE_UPGRADE_REFUSED, Some(live));
            }
            ShutdownDecision::Refuse(code) => {
                tracing::info!(code, "RequestUpgrade refused by the session gate");
                return refuse(control::CODE_UPGRADE_REFUSED, None);
            }
        }
        match start_announced_successor(successor(&executable), ready_timeout).await {
            Ok(successor) => {
                let pid = successor.child.id();
                // Checked by the retirement task right before this daemon leaves.
                *self.idle_successor.lock() = Some(successor);
                tracing::info!(
                    successor_pid = ?pid,
                    generation_key = %key,
                    "RequestUpgrade accepted with no live session: successor generation is waiting \
                     for the instance lock; exiting once this reply is flushed"
                );
                (ControlReply::UpgradeAccepted, AfterFlush::Shutdown)
            }
            Err((reason, detail)) => {
                tracing::warn!(
                    reason,
                    %detail,
                    "RequestUpgrade refused: successor not ready; the daemon keeps serving"
                );
                self.handover_manager.reopen_after_idle_upgrade_failure();
                let failure = ControlFailure {
                    code: control::CODE_UPGRADE_REFUSED.to_string(),
                    live: None,
                    reason: Some(reason.to_string()),
                };
                (ControlReply::Failed(failure), AfterFlush::Nothing)
            }
        }
    }

    #[cfg(unix)]
    async fn control_request_upgrade(
        self: &Arc<Self>,
        staged_executable: String,
        generation_key: Option<String>,
    ) -> (ControlReply, AfterFlush) {
        let reply = self
            .control_request_handover(staged_executable, generation_key)
            .await;
        (reply, AfterFlush::Nothing)
    }

    /// RequestUpgrade { staged_executable, generation_key } (todo 8). (a) the staged executable
    /// must lie under daemon_runtime_root() inside the generation dir named by generation_key;
    /// (b) prepare_handover, then handover-request.json (nonce, 30 s expiry, mode 0600);
    /// (c) supervisor::start_handover_candidate starts the successor generation's own OS job. The
    /// predecessor keeps serving everything until the candidate commits; any refusal leaves it
    /// Active. No in-place re-exec and no child process of this daemon is ever used here.
    #[cfg(unix)]
    async fn control_request_handover(
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
        // The shared prepare path (server.rs): QA fault arm, prepare_handover off the workers,
        // predecessor-prepared receipt; the same one PrepareHandover uses.
        let (legacy_path, _, legacy_listener) = match self.prepare_handover_shared().await {
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
            #[cfg(test)]
            let test_starter = self.handover_manager.candidate_starter();
            crate::ipc::run_blocking(move || {
                let start = || -> Result<CandidateStart, String> {
                    request_file::write_request(&runtime_dir, &request)?;
                    #[cfg(test)]
                    if let Some(starter) = test_starter {
                        return request_file::start_handover_candidate_with(
                            &runtime_dir,
                            &crate::daemon::owner_admission::AdmissionPaths::for_runtime().admission_lock,
                            &request,
                            std::process::id(),
                            starter.as_ref(),
                            crate::daemon::owner_admission::ADMISSION_LOCK_DEADLINE,
                        );
                    }
                    crate::daemon::supervisor::start_handover_candidate(&request)
                };
                start().map_err(crate::ipc::IpcError::internal)
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
