//! Phase 2 acceptance harness for the abnormal-exit supervisor
//! (`.omo/plans/local-pty-crash-survival-20261116.md`, "Phase 2 - supervisor, keeper-backed only").
//!
//! Every case runs the real thing: the real `ferryx-cli --daemon-crash-supervisor` loop spawns the
//! real `ferryx-cli --daemon`, which owns a real runtime directory, a real data socket, a real
//! control-v1 endpoint and a real restart log. Nothing here re-implements the supervisor or fakes a
//! process exit. The one stand-in is the PTY keeper, and it stands in at the wire, not in the
//! decision: `crash_supervisor::KeeperSocketProbe` (production code) connects to a unix socket this
//! harness binds and answers its `HELLO` with the real `FXKP` framing built by
//! `daemon::keeper_client::KeeperFrame`, so the gate opens only because production validation ran.
//!
//! Cases (plan: "abnormal exit restarts once; exit 0 does not; handover does not; 6 rapid failures
//! stop at the limit"), plus the two refusals the plan's hard rule owes:
//!
//! 1. `abnormal_exit_restarts_exactly_once_then_exit_zero_stops_supervision`
//! 2. `exit_zero_never_restarts`
//! 3. `handover_predecessor_exits_zero_and_is_never_restarted`
//! 4. `six_rapid_failures_stop_at_the_restart_limit`
//! 5. `no_keeper_refuses_supervised_restart_and_starts_the_daemon_once`
//! 6. `a_keeper_that_dies_mid_run_refuses_the_restart` (the gate re-evaluated after the exit)
//! 7. `a_second_supervisor_on_one_runtime_starts_nothing_and_exits_clean`
//!
//! Isolation: `HOME`, `XDG_*`, `TMPDIR`, `FERRYX_RUNTIME_DIR`, `FERRYX_LEGACY_RUNTIME_DIR`,
//! `FERRYX_DATA_DIR`, `FERRYX_SESSION_DIR`, `FERRYX_KEEPER_DIR` and both launchd/keeper plist paths
//! point inside one `TempDir` per case, so no case can reach the user's daemon, keeper or
//! `/tmp/ferryx-<uid>` runtime. `FERRYX_DAEMON_SUPERVISOR_NAME` is deliberately *not* set:
//! `daemon::supervisor::supported_for` treats that name as permission to manage an OS job even for
//! an isolated runtime, and a handover commit calls `register_running_generation_for_login`. Leaving
//! it unset keeps `supported()` false (a `FERRYX_*_DIR` override is present), so no `launchctl` call
//! can happen. The daemon the supervisor spawns inherits the fixture environment by construction
//! (`ProcessHost::spawn_daemon` only overrides stdio), so one application covers all three roles.
//!
//! Every restart-log assertion is a **bounded 30 s wait** on the log's content ([`LOG_WAIT`]), never
//! a delay that "usually" suffices: the wait is woken by a watcher on the log directory and can only
//! succeed by finding the recorded decision. [`WAKE_NET`] exists solely because `FSEvents` may
//! coalesce or drop a notification for an append; it is a missed-wakeup net, never the criterion.
//! No assertion is made after a fixed sleep, and no process exit is waited for by polling.
//!
//! Known seams this harness works around and reports instead of hiding:
//!
//! * `ProcessHost::spawn_daemon` points the daemon's stdout at `/dev/null`, so
//!   `FERRYX_DAEMON_READY` is not observable; readiness is taken from a real control-v1 `Hello` pid
//!   instead, which is the same pid the supervisor's own restart log names.
//! * `cli::run_daemon_crash_supervisor` hardcodes `RestartPolicy::default()`, so the production
//!   backoff (1/2/4/8/16 s) cannot be shortened for a test: case 4 really waits ~31 s of backoff
//!   across six cold starts. Each individual log transition still has to land inside [`LOG_WAIT`];
//!   only the whole-case budget ([`CASE_WAIT`]) is larger, because the backoffs sum.
//! * A handover predecessor retires through `handover::HandoverManager::retirement_action()`, a bare
//!   `exit(0)` that writes no `retired-<generation>.json` marker. The durable evidence is the
//!   decision record the commit writes beside the legacy socket
//!   (`legacy-<pid>-<disambiguator>.transaction`, `state: "retired"`), which
//!   `ProcessHost::deliberate_exit` reads for the pid that just exited. Case 3 therefore asserts
//!   `kind=handover` / `reason=intentional:handover` **and** that the record exists for the
//!   predecessor, so the classification is pinned to evidence rather than to the exit status.
//! * A refused gate still starts exactly one daemon: `cli.rs` runs the supervisor as the launchd
//!   job's program, so a keeper-less install must still be served at login (`KeepAlive {
//!   SuccessfulExit = false }` would not relaunch a clean exit). Case 5 pins that shape: one
//!   `event=refusal ... decision=serve_once`, one `event=start supervised=off`, zero restarts.
//! * Single-supervisor ownership is asserted by case 7, which starts a second supervisor against a
//!   runtime a first one already owns: the second must exit 0 having started nothing and written
//!   no banner, because a healthy supervisor already serves that runtime.
//! * The keeper here is a wire-faithful responder, not the Phase 3 keeper binary:
//!   `crates/ferryx-ptykeeper` is a separate workspace with its own lock file and is not a
//!   dependency of this crate, so no keeper executable is built with these tests. What this proves
//!   is the gate: a socket at `<runtime>/keeper.sock` that completes the real `HELLO`/`HELLO_ACK`
//!   exchange with the daemon's own framing, including the kernel's peer-pid corroboration, since
//!   the responder thread lives in this harness process, which is the peer the supervisor dials.
//! * A live keeper is only one half of the gate. `supervised_restart_gate` also requires the runtime
//!   to route *new* sessions to that keeper, which it reads from the durable table at
//!   `OwnershipStore::for_runtime()`: `<FERRYX_DATA_DIR>/pty-ownership.json`, because that store is
//!   `logs_dir().parent()`. Every case that expects supervision therefore enters that state through
//!   the product's own enable path - `keeper_session::activate_ownership` against a
//!   `KeeperSocketProbe` bound to this case's socket and its table - never by writing the JSON here.
//!   A case that only bound the socket would exercise the gate's intended `no_keeper_ownership`
//!   refusal instead of supervision, which is a different case with its own coverage below.

#![cfg(unix)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ferryx_lib::daemon::control::{connect, ControlConnection, ControlReply, ControlRequest};
use ferryx_lib::daemon::crash_supervisor::{
    KeeperSocketProbe, KEEPER_SOCKET_NAME, MAX_RESTARTS_PER_WINDOW, RESTART_LOG_NAME,
    SUPERVISOR_ARG, SUPERVISOR_LOCK_NAME,
};
use ferryx_lib::daemon::keeper_client::{
    KeeperFrame, KeeperKind, KEEPER_HEADER_BYTES, KEEPER_PROTOCOL_MAJOR,
};
use ferryx_lib::daemon::keeper_session::{
    activate_ownership, ActivationOutcome, OwnershipStore, SessionOwner, OWNERSHIP_FILE_NAME,
};
use ferryx_lib::daemon::protocol::{DaemonRequest, DaemonResponse, DAEMON_PROTOCOL_VERSION};
use notify::Watcher as _;
use tempfile::TempDir;

/// The binary under test. `ferryx-cli` carries `required-features = []`, so the supervisor, the
/// daemon and this harness all exist in a headless build.
const BIN: &str = env!("CARGO_BIN_EXE_ferryx-cli");

/// The plan's bound: every restart-log assertion gets this long and no longer.
const LOG_WAIT: Duration = Duration::from_secs(30);

/// Cold-start budget for a daemon start. The daemon hashes its own executable to build its
/// generation key before it binds the control endpoint, which is the slow part on a loaded machine;
/// the existing handover contract uses the same 90 s for the same reason.
const READY_WAIT: Duration = Duration::from_secs(90);

/// Whole-case guard, so a wedged supervisor fails its case instead of hanging the suite.
const CASE_WAIT: Duration = Duration::from_secs(240);

/// Missed-wakeup net for coalesced `FSEvents`/`kqueue` notifications. Never the pass criterion: the
/// waits below return as soon as the event, the log content or the process exit is observed.
const WAKE_NET: Duration = Duration::from_millis(200);

const BANNER_LINE: &str = "event=banner";
const STOP_LINE: &str = "event=stop";

struct Fixture {
    root: TempDir,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = tempfile::Builder::new()
            .prefix(&format!("fx-p2-{label}-"))
            .tempdir_in("/tmp")
            .expect("private case root under /tmp");
        for name in [
            "home",
            "runtime",
            "legacy-run",
            "data",
            "data/logs",
            "sessions",
            "config",
            "cache",
            "tmp",
            "launchd",
            "keeper",
            "logs",
        ] {
            std::fs::create_dir_all(root.path().join(name)).expect("case directory");
        }
        // The daemon refuses a runtime directory that is not 0700, and `--handover-from` validates
        // the same on the predecessor's socket parent.
        set_mode(&root.path().join("runtime"), 0o700);
        set_mode(&root.path().join("legacy-run"), 0o700);
        let fixture = Self { root };
        fixture.refuse_production_paths();
        fixture
    }

    /// A case that reached the user's real runtime would be a safety incident, not a flake.
    fn refuse_production_paths(&self) {
        // SAFETY: `getuid` takes no arguments, dereferences no pointers and has no side effects.
        let uid = unsafe { libc::getuid() };
        for production in [
            format!("/tmp/ferryx-{uid}"),
            format!("/tmp/ferryx-{uid}-dev"),
            format!("/tmp/rorca-{uid}"),
            format!("/tmp/rorca-{uid}-dev"),
        ] {
            let production = PathBuf::from(production);
            for isolated in [self.runtime(), self.legacy_runtime()] {
                assert!(
                    isolated.starts_with(self.root.path()),
                    "case path {} must stay inside {}",
                    isolated.display(),
                    self.root.path().display()
                );
                assert_ne!(
                    isolated, production,
                    "an isolated case must never run on the production runtime directory"
                );
            }
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }

    fn runtime(&self) -> PathBuf {
        self.path("runtime")
    }

    fn legacy_runtime(&self) -> PathBuf {
        self.path("legacy-run")
    }

    fn socket(&self) -> PathBuf {
        self.path("runtime/daemon.sock")
    }

    fn restart_log(&self) -> PathBuf {
        self.path("data/logs").join(RESTART_LOG_NAME)
    }

    fn keeper_socket(&self) -> PathBuf {
        self.runtime().join(KEEPER_SOCKET_NAME)
    }

    /// The durable ownership table the supervisor's gate reads, at the path the supervisor itself
    /// resolves: `OwnershipStore::for_runtime()` is `<FERRYX_DATA_DIR>/pty-ownership.json`.
    fn ownership_table(&self) -> PathBuf {
        self.path("data").join(OWNERSHIP_FILE_NAME)
    }

    /// Puts this case into the plan's "keeper ownership became default for new sessions" state,
    /// using the product's own enable path.
    ///
    /// `activate_ownership` is exactly what `KeeperOwnership::activate_new_sessions` calls: it
    /// probes this runtime's keeper with the real `HELLO`/`HELLO_ACK` handshake and only then sets
    /// the flag, through `OwnershipStore::save` - the same serializer and atomic replace the daemon
    /// uses. It answers `NotProven` and writes nothing when no keeper answers, so it cannot
    /// manufacture this state for the keeper-less case.
    ///
    /// The table path is passed explicitly rather than left to `OwnershipStore::for_runtime`, which
    /// would resolve through *this* process's `FERRYX_DATA_DIR` - the harness applies its isolation
    /// to the children it spawns, not to itself.
    fn activate_keeper_ownership(&self) -> ActivationOutcome {
        let store = OwnershipStore::at(self.ownership_table());
        let probe = KeeperSocketProbe::new(self.runtime())
            .with_ownership_table(self.ownership_table());
        let outcome =
            activate_ownership(&store, &probe).expect("the case ownership table is writable");
        assert_eq!(
            store
                .load()
                .expect("the table activation just wrote")
                .new_session_owner(),
            SessionOwner::Keeper,
            "activation must leave new sessions routed to this case's keeper"
        );
        outcome
    }

    fn apply_env(&self, command: &mut Command) {
        command
            .env_clear()
            .current_dir(self.root.path())
            .env("HOME", self.path("home"))
            .env("USERPROFILE", self.path("home"))
            .env("ZDOTDIR", self.path("home"))
            .env("XDG_CONFIG_HOME", self.path("config"))
            .env("XDG_DATA_HOME", self.path("data"))
            .env("XDG_CACHE_HOME", self.path("cache"))
            .env("XDG_RUNTIME_DIR", self.runtime())
            .env("TMPDIR", self.path("tmp"))
            .env("FERRYX_RUNTIME_DIR", self.runtime())
            .env("FERRYX_LEGACY_RUNTIME_DIR", self.legacy_runtime())
            .env("FERRYX_DATA_DIR", self.path("data"))
            .env("FERRYX_SESSION_DIR", self.path("sessions"))
            .env("FERRYX_KEEPER_DIR", self.path("keeper"))
            .env(
                "FERRYX_PTY_KEEPER_PLIST_PATH",
                self.path("launchd/com.ferryx.qa.ptykeeper.plist"),
            )
            .env(
                "FERRYX_DAEMON_PLIST_PATH",
                self.path("launchd/com.ferryx.qa.daemon.plist"),
            )
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .env("SHELL", "/bin/sh")
            .env("TERM", "xterm-256color")
            .env("LANG", "C");
    }

    fn supervisor_command(&self) -> Command {
        let mut command = Command::new(BIN);
        command.arg(SUPERVISOR_ARG);
        self.apply_env(&mut command);
        command
    }

    fn daemon_command(&self, handover_from: Option<&Path>) -> Command {
        let mut command = Command::new(BIN);
        command.arg("--daemon");
        if let Some(legacy) = handover_from {
            command.arg("--handover-from").arg(legacy);
        }
        self.apply_env(&mut command);
        command
    }
}

fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("chmod");
}

struct Keeper {
    socket: PathBuf,
    closing: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<usize>>,
}

impl Keeper {
    /// Binds `<runtime>/keeper.sock` before the supervisor starts, so its startup gate probe finds a
    /// live keeper. The listener is not a "accepts a connection" stand-in: every connection is
    /// answered with a `HELLO_ACK` built by the daemon's own wire code, and the probe refuses it
    /// unless request id, frame sequence, protocol major, keeper version and peer pid all agree.
    fn start(runtime: &Path) -> Self {
        let socket = runtime.join(KEEPER_SOCKET_NAME);
        let listener = std::os::unix::net::UnixListener::bind(&socket).expect("keeper socket");
        set_mode(&socket, 0o600);
        let closing = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&closing);
        let thread = std::thread::Builder::new()
            .name("qa-keeper".to_string())
            .spawn(move || serve_hello(listener, flag))
            .expect("keeper thread");
        Self {
            socket,
            closing,
            thread: Some(thread),
        }
    }

    /// Ends the keeper the way its own process would: nothing answers on the socket again and the
    /// socket file is gone. Returns how many handshakes the daemon's probe completed.
    fn stop(mut self) -> usize {
        let answered = self.shutdown();
        let _ = std::fs::remove_file(&self.socket);
        answered
    }

    fn shutdown(&mut self) -> usize {
        self.closing.store(true, Ordering::SeqCst);
        // The listener belongs to the thread, so its accept loop is woken by a connection rather
        // than by a sleep or a poll.
        let _ = std::os::unix::net::UnixStream::connect(&self.socket);
        self.thread
            .take()
            .map_or(0, |thread| thread.join().unwrap_or(0))
    }
}

impl Drop for Keeper {
    fn drop(&mut self) {
        let _ = self.shutdown();
        let _ = std::fs::remove_file(&self.socket);
    }
}

fn serve_hello(listener: std::os::unix::net::UnixListener, closing: Arc<AtomicBool>) -> usize {
    let mut answered = 0;
    for stream in listener.incoming() {
        if closing.load(Ordering::SeqCst) {
            break;
        }
        let Ok(mut stream) = stream else { break };
        if answer_with_real_framing(&mut stream).is_ok() {
            answered += 1;
        }
    }
    answered
}

/// One `HELLO` in, one `HELLO_ACK` out, through `daemon::keeper_client`'s own encoder and decoder,
/// so a framing drift fails here instead of passing on a hand-written byte layout.
fn answer_with_real_framing(stream: &mut std::os::unix::net::UnixStream) -> std::io::Result<()> {
    use std::io::{Read as _, Write as _};

    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut header = [0u8; KEEPER_HEADER_BYTES];
    stream.read_exact(&mut header)?;
    let payload_len = u32::from_be_bytes([header[8], header[9], header[10], header[11]]) as usize;
    let mut bytes = vec![0u8; KEEPER_HEADER_BYTES + payload_len];
    bytes[..KEEPER_HEADER_BYTES].copy_from_slice(&header);
    stream.read_exact(&mut bytes[KEEPER_HEADER_BYTES..])?;
    let request = KeeperFrame::decode(&bytes).expect("the probe frames with the daemon's own code");
    if request.kind != KeeperKind::Hello {
        return Ok(());
    }
    let reply = KeeperFrame::new(
        KeeperKind::HelloAck,
        request.request_id.clone(),
        request.frame_sequence,
        serde_json::json!({
            "protocolMajor": KEEPER_PROTOCOL_MAJOR,
            "protocolMinor": 0,
            "keeperVersion": env!("CARGO_PKG_VERSION"),
            // The probe corroborates this against the kernel's peer pid, and this thread lives in
            // the harness process, which is exactly the peer the supervisor connects to.
            "pid": std::process::id(),
        }),
    );
    stream.write_all(&reply.encode())?;
    stream.flush()
}

struct DirWatch {
    _watcher: notify::RecommendedWatcher,
    rx: tokio::sync::mpsc::UnboundedReceiver<()>,
}

impl DirWatch {
    fn new(dir: &Path) -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            if event.is_ok() {
                let _ = tx.send(());
            }
        })
        .expect("a directory watcher");
        watcher
            .watch(dir, notify::RecursiveMode::NonRecursive)
            .expect("watch the case directory");
        Self {
            _watcher: watcher,
            rx,
        }
    }

    /// Waits for the next change, but never longer than [`WAKE_NET`] and never past `deadline`.
    /// Returns whether the caller still has budget; a `false` is the caller's verdict to give.
    async fn settled(&mut self, deadline: Instant) -> bool {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return false;
        }
        let _ = tokio::time::timeout(remaining.min(WAKE_NET), self.rx.recv()).await;
        Instant::now() < deadline
    }
}

struct Procs {
    /// Daemon pids learned from this case's own control endpoint, plus the processes this harness
    /// spawned. Never a pid from anywhere else.
    pids: Vec<u32>,
    sender: tokio::sync::mpsc::UnboundedSender<(u32, ExitStatus)>,
    exited: tokio::sync::mpsc::UnboundedReceiver<(u32, ExitStatus)>,
    statuses: HashMap<u32, ExitStatus>,
    waiters: Vec<std::thread::JoinHandle<()>>,
}

impl Procs {
    fn new() -> Self {
        let (sender, exited) = tokio::sync::mpsc::unbounded_channel();
        Self {
            pids: Vec::new(),
            sender,
            exited,
            statuses: HashMap::new(),
            waiters: Vec::new(),
        }
    }

    fn track(&mut self, pid: u32) {
        assert!(pid > 1, "refusing to track pid {pid}");
        assert_ne!(
            pid,
            std::process::id(),
            "refusing to track the harness process itself"
        );
        if !self.pids.contains(&pid) {
            self.pids.push(pid);
        }
    }

    /// Starts a reaping thread that owns `child` and reports its exit exactly once.
    fn spawn(&mut self, label: &'static str, mut child: Child) -> u32 {
        let pid = child.id();
        self.track(pid);
        let sender = self.sender.clone();
        let waiter = std::thread::Builder::new()
            .name(format!("fx-{label}"))
            .spawn(move || {
                if let Ok(status) = child.wait() {
                    let _ = sender.send((pid, status));
                }
            })
            .expect("process waiter thread");
        self.waiters.push(waiter);
        pid
    }

    fn forget(&mut self, pid: u32) {
        self.pids.retain(|tracked| *tracked != pid);
    }
}

impl Drop for Procs {
    fn drop(&mut self) {
        for pid in &self.pids {
            terminate(*pid);
        }
        for waiter in self.waiters.drain(..) {
            let _ = waiter.join();
        }
    }
}

fn terminate(pid: u32) {
    if pid <= 1 || pid == std::process::id() {
        return;
    }
    // SAFETY: `kill(2)` with SIGKILL on a pid this case learned from its own isolated runtime's
    // control endpoint or spawned itself. A pid that already exited answers ESRCH, which is the
    // expected case here.
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGKILL);
    }
}

fn process_is_alive(pid: u32) -> bool {
    // SAFETY: signal 0 is a pure existence probe and sends nothing.
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

async fn control_pid(runtime: &Path) -> Result<Option<u32>, String> {
    match connect(runtime).await.map_err(|e| e.to_string())? {
        ControlConnection::Missing => Ok(None),
        ControlConnection::Connected(mut client) => {
            match client
                .call(&ControlRequest::Hello)
                .await
                .map_err(|e| e.to_string())?
            {
                ControlReply::Hello(hello) => Ok(Some(hello.pid)),
                other => Err(format!("Hello answered {other:?}")),
            }
        }
    }
}

/// Asks the daemon serving this runtime to shut down with the exact live count (0), which is the
/// deliberate exit cases 1 and 2 are about. The pid check keeps the harness from ever shutting down
/// a daemon this case does not own.
async fn shutdown_daemon(runtime: &Path, expected_pid: u32) -> Result<u32, String> {
    let ControlConnection::Connected(mut client) =
        connect(runtime).await.map_err(|e| e.to_string())?
    else {
        return Err("no control endpoint to shut down".to_string());
    };
    let pid = match client
        .call(&ControlRequest::Hello)
        .await
        .map_err(|e| e.to_string())?
    {
        ControlReply::Hello(hello) => hello.pid,
        other => return Err(format!("Hello answered {other:?}")),
    };
    if pid != expected_pid {
        return Err(format!(
            "refusing to shut down pid {pid}: this case owns pid {expected_pid}"
        ));
    }
    match client
        .call(&ControlRequest::Shutdown {
            confirm_live_sessions: 0,
        })
        .await
        .map_err(|e| e.to_string())?
    {
        ControlReply::Live { live } => Ok(live),
        ControlReply::Failed(failure) => Err(format!("Shutdown refused: {failure:?}")),
        other => Err(format!("Shutdown answered {other:?}")),
    }
}

struct DataClient {
    reader: tokio::io::BufReader<tokio::net::unix::OwnedReadHalf>,
    writer: tokio::net::unix::OwnedWriteHalf,
}

impl DataClient {
    async fn connect(socket: &Path, expected_pid: u32) -> Result<Self, String> {
        use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _};

        let stream = tokio::net::UnixStream::connect(socket)
            .await
            .map_err(|e| format!("connect {}: {e}", socket.display()))?;
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = tokio::io::BufReader::new(read_half);

        let handshake = DaemonRequest::Handshake {
            version: DAEMON_PROTOCOL_VERSION,
            token: None,
        };
        let mut frame = serde_json::to_string(&handshake).map_err(|e| e.to_string())?;
        frame.push('\n');
        write_half
            .write_all(frame.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        write_half.flush().await.map_err(|e| e.to_string())?;

        let mut line = String::new();
        let read = tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line))
            .await
            .map_err(|_| "handshake timed out".to_string())?
            .map_err(|e| e.to_string())?;
        if read == 0 {
            return Err("EOF during handshake".to_string());
        }
        match serde_json::from_str::<DaemonResponse>(line.trim()).map_err(|e| e.to_string())? {
            DaemonResponse::HandshakeOk {
                pid, version, ..
            } if pid == expected_pid && version == DAEMON_PROTOCOL_VERSION => {}
            other => {
                return Err(format!(
                    "expected HandshakeOk for pid {expected_pid}, got {other:?}"
                ))
            }
        }
        Ok(Self {
            reader,
            writer: write_half,
        })
    }

    async fn request(&mut self, request: &DaemonRequest) -> Result<DaemonResponse, String> {
        use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _};

        let mut frame = serde_json::to_string(request).map_err(|e| e.to_string())?;
        frame.push('\n');
        self.writer
            .write_all(frame.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        self.writer.flush().await.map_err(|e| e.to_string())?;

        let mut line = String::new();
        let read = tokio::time::timeout(Duration::from_secs(10), self.reader.read_line(&mut line))
            .await
            .map_err(|_| "request timed out".to_string())?
            .map_err(|e| e.to_string())?;
        if read == 0 {
            return Err("EOF from the daemon".to_string());
        }
        serde_json::from_str(line.trim()).map_err(|e| e.to_string())
    }
}

struct Case {
    // Field order is drop order: processes are killed and reaped, the keeper goes, the watchers
    // stop, then the tree is removed.
    procs: Procs,
    keeper: Option<Keeper>,
    logs: DirWatch,
    runtime_events: DirWatch,
    fixture: Fixture,
}

impl Case {
    fn new(label: &str, with_keeper: bool) -> Self {
        let fixture = Fixture::new(label);
        let keeper = with_keeper.then(|| {
            let keeper = Keeper::start(&fixture.runtime());
            // Both halves of the gate's requirement, in the order the plan's intra-release
            // activation names them: the keeper is live, and then it owns new sessions.
            assert!(
                matches!(
                    fixture.activate_keeper_ownership(),
                    ActivationOutcome::Activated { keeper_pid: Some(_) }
                ),
                "a case with a keeper must enter the plan's \"keeper owns new sessions\" state; a \
                 bound socket alone is the gate's `no_keeper_ownership` refusal"
            );
            keeper
        });
        let logs = DirWatch::new(&fixture.path("data/logs"));
        let runtime_events = DirWatch::new(&fixture.runtime());
        Self {
            procs: Procs::new(),
            keeper,
            logs,
            runtime_events,
            fixture,
        }
    }

    fn spawn_supervisor(&mut self) -> u32 {
        self.spawn_supervisor_as("supervisor", "logs/supervisor.err")
    }

    /// A supervisor with its own role and stderr file, so a case can run two of them and still
    /// tell their streams apart.
    fn spawn_supervisor_as(&mut self, role: &'static str, stderr: &str) -> u32 {
        let mut command = self.fixture.supervisor_command();
        let stderr = std::fs::File::create(self.fixture.path(stderr))
            .expect("capture the supervisor's stderr");
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr));
        let child = command.spawn().expect("spawn the crash supervisor");
        self.procs.spawn(role, child)
    }

    fn spawn_successor(&mut self, legacy_socket: &Path) -> u32 {
        let mut command = self.fixture.daemon_command(Some(legacy_socket));
        let stderr = std::fs::File::create(self.fixture.path("logs/successor.err"))
            .expect("capture the successor's stderr");
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr));
        let child = command.spawn().expect("spawn the handover successor");
        self.procs.spawn("successor", child)
    }

    fn log_text(&self) -> String {
        std::fs::read_to_string(self.fixture.restart_log()).unwrap_or_default()
    }

    fn diagnostics(&self) -> String {
        let mut out = String::new();
        for name in [
            "logs/supervisor.err",
            "logs/second-supervisor.err",
            "logs/successor.err",
        ] {
            let path = self.fixture.path(name);
            if let Ok(text) = std::fs::read_to_string(&path) {
                out.push_str(&format!("--- {name} ---\n{text}\n"));
            }
        }
        if out.is_empty() {
            out.push_str("(no stderr was captured)\n");
        }
        out
    }

    /// Waits until the daemon serving this runtime answers control-v1 with a pid other than
    /// `exclude`, and returns that pid. Readiness is protocol-derived because the supervisor nulls
    /// the daemon's stdout, and the pid is the one the supervisor's own restart log names.
    async fn await_daemon_ready(&mut self, exclude: Option<u32>) -> u32 {
        let deadline = Instant::now() + READY_WAIT;
        let mut last;
        loop {
            match control_pid(&self.fixture.runtime()).await {
                Ok(Some(pid)) if Some(pid) != exclude => {
                    self.procs.track(pid);
                    return pid;
                }
                Ok(Some(pid)) => last = format!("control endpoint still answers with pid {pid}"),
                Ok(None) => last = "no control endpoint yet".to_string(),
                Err(error) => last = format!("control endpoint error: {error}"),
            }
            if !self.runtime_events.settled(deadline).await {
                panic!(
                    "no daemon answered control-v1 within {READY_WAIT:?} ({last}); restart log:\n{}\n{}",
                    self.log_text(),
                    self.diagnostics()
                );
            }
        }
    }

    /// The bounded wait the plan asks for: returns the restart log once it records `want`.
    async fn await_log(&mut self, want: &str) -> String {
        let path = self.fixture.restart_log();
        let deadline = Instant::now() + LOG_WAIT;
        loop {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            if text.contains(want) {
                return text;
            }
            if !self.logs.settled(deadline).await {
                panic!(
                    "the restart log never recorded {want:?} within {LOG_WAIT:?}; it holds:\n{text}\n{}",
                    self.diagnostics()
                );
            }
        }
    }

    /// Waits for a process this case spawned, on the signal its reaping thread sends.
    async fn await_exit(&mut self, pid: u32, what: &str) -> ExitStatus {
        if let Some(status) = self.procs.statuses.remove(&pid) {
            self.procs.forget(pid);
            return status;
        }
        let deadline = Instant::now() + CASE_WAIT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                panic!(
                    "{what} (pid {pid}) did not exit within {CASE_WAIT:?}; restart log:\n{}\n{}",
                    self.log_text(),
                    self.diagnostics()
                );
            }
            let next = tokio::time::timeout(remaining, self.procs.exited.recv()).await;
            match next {
                Ok(Some((exited, status))) if exited == pid => {
                    self.procs.forget(pid);
                    return status;
                }
                Ok(Some((exited, status))) => {
                    self.procs.statuses.insert(exited, status);
                }
                Ok(None) => panic!(
                    "every tracked process exited without reporting {what} (pid {pid}); restart log:\n{}\n{}",
                    self.log_text(),
                    self.diagnostics()
                ),
                Err(_) => panic!(
                    "{what} (pid {pid}) did not exit within {CASE_WAIT:?}; restart log:\n{}\n{}",
                    self.log_text(),
                    self.diagnostics()
                ),
            }
        }
    }

    fn crash(&mut self, pid: u32) {
        terminate(pid);
        self.procs.forget(pid);
    }

    fn released(&mut self, pid: u32) {
        self.procs.forget(pid);
    }

    fn stop_keeper(&mut self) -> usize {
        self.keeper.take().map_or(0, Keeper::stop)
    }
}

fn assert_log(text: &str, needle: &str, why: &str) {
    assert!(text.contains(needle), "{why}\nrestart log:\n{text}");
}

fn assert_no_log(text: &str, needle: &str, why: &str) {
    assert!(!text.contains(needle), "{why}\nrestart log:\n{text}");
}

fn count_log(text: &str, needle: &str) -> usize {
    text.matches(needle).count()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn abnormal_exit_restarts_exactly_once_then_exit_zero_stops_supervision() {
    let mut case = Case::new("restart", true);
    let supervisor = case.spawn_supervisor();

    let crashed = case.await_daemon_ready(None).await;
    assert_ne!(crashed, supervisor, "the daemon is not the supervisor");

    // A signal death is the shape of a real crash: the supervisor observes the exit the way it
    // would observe a SIGSEGV, and decides exactly as it would then.
    case.crash(crashed);
    let text = case.await_log("event=exit").await;
    assert_log(
        &text,
        "exit=signal=9 kind=abnormal",
        "a SIGKILLed daemon must be recorded as an abnormal exit",
    );
    assert_log(
        &text,
        "event=gate supervised_restart=on",
        "a live keeper must open the gate before supervision starts",
    );
    assert_log(
        &text,
        &format!("(pid {})", std::process::id()),
        "the gate must name the keeper pid the kernel corroborated, not the daemon's",
    );

    let text = case.await_log("event=restart").await;
    assert_eq!(
        count_log(&text, "event=restart"),
        1,
        "one crash is one restart, no more: {text}"
    );
    assert_log(&text, "attempt=1", "the first restart is attempt 1: {text}");
    assert_log(
        &text,
        "decision=restart",
        "the decision must be recorded, not only acted on: {text}",
    );

    // The replacement really serves this runtime: a different pid answers control-v1.
    let restarted = case.await_daemon_ready(Some(crashed)).await;
    assert_ne!(
        restarted, crashed,
        "the restart must be a new process, not the crashed one"
    );

    shutdown_daemon(&case.fixture.runtime(), restarted)
        .await
        .expect("control-v1 Shutdown on the restarted daemon");
    case.released(restarted);
    let text = case.await_log(STOP_LINE).await;
    assert_log(
        &text,
        "kind=clean_exit",
        "an exit 0 is a clean exit, never a crash: {text}",
    );
    assert_log(
        &text,
        "reason=intentional:clean_exit",
        "supervision must end on the deliberate exit: {text}",
    );
    assert_eq!(
        count_log(&text, "event=restart"),
        1,
        "the clean exit must not add a restart: {text}"
    );

    let status = case.await_exit(supervisor, "the supervisor").await;
    assert!(
        status.success(),
        "the supervisor exits 0 after an intentional stop, got {status:?}; restart log:\n{text}"
    );
    let answered = case.stop_keeper();
    assert!(
        answered >= 1,
        "the gate must have been opened by a completed keeper handshake"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exit_zero_never_restarts() {
    let mut case = Case::new("exit-zero", true);
    let supervisor = case.spawn_supervisor();
    let daemon = case.await_daemon_ready(None).await;

    shutdown_daemon(&case.fixture.runtime(), daemon)
        .await
        .expect("control-v1 Shutdown");
    case.released(daemon);
    let text = case.await_log(STOP_LINE).await;
    assert_log(&text, "kind=clean_exit", "exit 0 is intentional: {text}");
    assert_log(
        &text,
        "reason=intentional:clean_exit",
        "the stop reason must name the clean exit: {text}",
    );
    assert_no_log(
        &text,
        "event=restart",
        "an exit 0 must never write a restart line",
    );
    assert_no_log(&text, "kind=abnormal", "no crash happened: {text}");

    let status = case.await_exit(supervisor, "the supervisor").await;
    assert!(
        status.success(),
        "the supervisor exits 0 on an intentional stop, got {status:?}; restart log:\n{text}"
    );
    // The retirement marker the daemon wrote is what made this a deliberate exit; without it the
    // same status 0 would still be a clean exit, so this is evidence, not the mechanism.
    let markers: Vec<PathBuf> = std::fs::read_dir(case.fixture.runtime())
        .expect("the runtime directory")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("retired-"))
        })
        .collect();
    assert!(
        markers.iter().any(|marker| {
            std::fs::read_to_string(marker)
                .unwrap_or_default()
                .contains("control-v1 Shutdown")
        }),
        "a control-v1 Shutdown must leave the retirement marker the supervisor reads: {markers:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn handover_predecessor_exits_zero_and_is_never_restarted() {
    let mut case = Case::new("handover", true);
    let supervisor = case.spawn_supervisor();
    let predecessor = case.await_daemon_ready(None).await;

    // Prepare on the real data protocol, on the socket the supervisor's daemon really serves.
    let mut client = DataClient::connect(&case.fixture.socket(), predecessor)
        .await
        .expect("data-protocol handshake with the supervised daemon");
    let legacy_socket = match client.request(&DaemonRequest::PrepareHandover).await {
        Ok(DaemonResponse::PrepareHandoverOk {
            legacy_socket_path,
            active_sessions,
        }) => {
            assert!(
                active_sessions.is_empty(),
                "a zero-session handover is legal and is what this case exercises: {active_sessions:?}"
            );
            PathBuf::from(legacy_socket_path)
        }
        other => panic!("PrepareHandover failed: {other:?}"),
    };
    assert!(
        legacy_socket.starts_with(case.fixture.runtime()),
        "the legacy socket must live in this case's runtime: {}",
        legacy_socket.display()
    );
    drop(client);

    // The supervisor only ever spawns a plain `--daemon`, so the successor is started here, with
    // the same isolated environment, as the same generation, over the same real handover path.
    let successor = case.spawn_successor(&legacy_socket);
    assert_ne!(successor, predecessor);

    // The successor adopts the predecessor's listener and locks at commit, so `daemon.sock` keeps
    // serving and now answers with the successor's own pid. That is the ownership proof.
    let adopted = case.await_daemon_ready(Some(predecessor)).await;
    assert_eq!(
        adopted, successor,
        "the daemon serving the endpoint after commit must be the successor"
    );
    case.released(predecessor);
    DataClient::connect(&case.fixture.socket(), successor)
        .await
        .expect("the successor serves the canonical data endpoint with its own pid");

    let text = case.await_log(STOP_LINE).await;
    assert_log(
        &text,
        &format!("pid={predecessor}"),
        "the predecessor's exit is on record: {text}",
    );
    assert_log(
        &text,
        "kind=handover",
        "a committed handover is classified from its durable decision record, not from its exit \
         status: {text}",
    );
    assert_log(
        &text,
        "reason=intentional:handover",
        "a handover must never be treated as a crash: {text}",
    );
    assert_no_log(
        &text,
        "event=restart",
        "a handover must never be answered by a restart",
    );

    let status = case.await_exit(supervisor, "the supervisor").await;
    assert!(
        status.success(),
        "the supervisor exits 0 after the handover, got {status:?}; restart log:\n{text}"
    );
    assert!(
        process_is_alive(successor),
        "the successor must still serve this runtime"
    );

    // The classification must be evidence, not luck: the durable decision record for the pid that
    // retired exists in this runtime, names that pid, and records the committed state.
    let records: Vec<(PathBuf, serde_json::Value)> = std::fs::read_dir(case.fixture.runtime())
        .expect("the runtime directory")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with(&format!("legacy-{predecessor}-"))
                        && name.ends_with(".transaction")
                })
        })
        .filter_map(|path| {
            let bytes = std::fs::read(&path).ok()?;
            Some((path, serde_json::from_slice(&bytes).ok()?))
        })
        .collect();
    assert_eq!(
        records.len(),
        1,
        "the commit writes exactly one decision record for the retiring predecessor: {:?}",
        records.iter().map(|(path, _)| path).collect::<Vec<_>>()
    );
    let (path, record) = &records[0];
    assert_eq!(
        record.get("predecessorPid").and_then(|pid| pid.as_u64()),
        Some(u64::from(predecessor)),
        "{} must name the pid that retired",
        path.display()
    );
    assert_eq!(
        record.get("state").and_then(|state| state.as_str()),
        Some("retired"),
        "{} must record the committed state the supervisor reads",
        path.display()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn six_rapid_failures_stop_at_the_restart_limit() {
    let mut case = Case::new("limit", true);
    let supervisor = case.spawn_supervisor();

    let mut previous = None;
    let mut crashed = Vec::new();
    for _ in 0..=MAX_RESTARTS_PER_WINDOW {
        let daemon = case.await_daemon_ready(previous).await;
        case.crash(daemon);
        crashed.push(daemon);
        previous = Some(daemon);
    }
    assert_eq!(
        crashed.len(),
        MAX_RESTARTS_PER_WINDOW + 1,
        "the initial start plus one start per allowed restart"
    );

    let text = case.await_log(BANNER_LINE).await;
    assert_log(
        &text,
        "reason=restart_limit",
        "the stop reason names the exhausted budget: {text}",
    );
    assert_eq!(
        count_log(&text, "event=restart"),
        MAX_RESTARTS_PER_WINDOW,
        "the cap is {MAX_RESTARTS_PER_WINDOW} restarts per window: {text}"
    );
    assert_eq!(
        count_log(&text, "kind=abnormal"),
        MAX_RESTARTS_PER_WINDOW + 1,
        "every one of the six deaths is recorded as a crash: {text}"
    );
    for attempt in 1..=MAX_RESTARTS_PER_WINDOW {
        assert_log(
            &text,
            &format!("attempt={attempt}"),
            "each restart is numbered: {text}",
        );
    }
    assert_log(
        &text,
        "decision=stop",
        "the stop itself is a decision on record: {text}",
    );
    assert_log(
        &text,
        &format!("crashed {MAX_RESTARTS_PER_WINDOW} times in 10 minutes"),
        "the banner states the limit it hit: {text}",
    );

    let status = case.await_exit(supervisor, "the supervisor").await;
    assert!(
        status.success(),
        "hitting the limit is a normal stop, got {status:?}; restart log:\n{text}"
    );
    // Nothing restarts a seventh daemon: the loop is over and no new pid may appear.
    assert_eq!(
        control_pid(&case.fixture.runtime()).await,
        Ok(None),
        "no daemon may be serving this runtime after the limit stop"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_keeper_refuses_supervised_restart_and_starts_the_daemon_once() {
    let mut case = Case::new("no-keeper", false);
    assert!(
        !case.fixture.keeper_socket().exists(),
        "this case must run without any keeper socket"
    );
    let supervisor = case.spawn_supervisor();

    let text = case.await_log(BANNER_LINE).await;
    assert_log(
        &text,
        "event=gate supervised_restart=off",
        "the refusal is a gate decision and must be on record: {text}",
    );
    assert_log(
        &text,
        "event=refusal code=no_live_keeper decision=serve_once",
        "the refusal is a typed decision, and it is the decision that still serves this login: \
         {text}",
    );
    assert_log(
        &text,
        "no live PTY keeper",
        "the banner must say why restart is off: {text}",
    );
    // The typed token lives on the stop line, not on the banner: a banner states the cause in prose,
    // the decision record carries `reason=`. Waiting for that line is what keeps this assertion
    // about the recorded decision instead of about the banner's position in the file.
    let text = case.await_log(STOP_LINE).await;
    assert_log(
        &text,
        "reason=no_live_keeper",
        "the stop reason must name the refusal: {text}",
    );
    assert_eq!(
        count_log(&text, "event=restart"),
        0,
        "no restart may exist without a keeper: {text}"
    );

    let status = case.await_exit(supervisor, "the supervisor").await;
    assert!(
        status.success(),
        "the refusal is not an error exit, got {status:?}; restart log:\n{text}"
    );

    // The login still gets a daemon: it is simply unsupervised, so one start and never a second.
    let daemon = case.await_daemon_ready(None).await;
    assert_ne!(daemon, supervisor);
    assert!(
        process_is_alive(daemon),
        "the one allowed start must be a live daemon"
    );
    let text = case.log_text();
    assert_log(
        &text,
        &format!("event=start pid={daemon} supervised=off decision=serve_once"),
        "the single start must be on record as unsupervised and tied to the pid that serves this \
         runtime, so \"one start\" is not inferred from process state: {text}",
    );
    assert_eq!(
        count_log(&text, "event=start"),
        1,
        "a refused gate may start exactly one daemon, ever: {text}"
    );
    assert_eq!(
        count_log(&text, "event=gate"),
        1,
        "the gate was decided exactly once: {text}"
    );
    assert_eq!(
        count_log(&text, "event=restart"),
        0,
        "an unsupervised daemon is never restarted: {text}"
    );
}

/// Two supervisors on one runtime is the failure the plan's ownership rule exists for: two daemons
/// racing one instance lock and two restart budgets for one crash. The second one must lose the
/// claim before it decides anything, start nothing, and say nothing to the user - a banner claiming
/// the daemon was not started would be false while the first supervisor serves it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_second_supervisor_on_one_runtime_starts_nothing_and_exits_clean() {
    let mut case = Case::new("second-supervisor", true);
    let supervisor = case.spawn_supervisor();
    let daemon = case.await_daemon_ready(None).await;
    let first_gate = case.log_text();
    assert_log(
        &first_gate,
        "event=gate supervised_restart=on",
        "the first supervisor must own this runtime before the second is started: {first_gate}",
    );
    assert!(
        case.fixture.runtime().join(SUPERVISOR_LOCK_NAME).exists(),
        "the claim is a file in the runtime directory, so a second supervisor can find it"
    );

    let second = case.spawn_supervisor_as("supervisor-2", "logs/second-supervisor.err");
    assert_ne!(second, supervisor);
    let status = case.await_exit(second, "the second supervisor").await;
    assert!(
        status.success(),
        "a refused claim is a clean exit, got {status:?}; restart log:\n{}",
        case.log_text()
    );

    let text = case.log_text();
    assert_log(
        &text,
        "event=refusal code=supervisor_already_running decision=stop",
        "the second supervisor's refusal must be on record: {text}",
    );
    assert_log(
        &text,
        "reason=already_supervised",
        "the stop reason names what happened: {text}",
    );
    assert_eq!(
        count_log(&text, "event=gate"),
        1,
        "the second supervisor decided no gate, because it supervised nothing: {text}"
    );
    assert_eq!(
        count_log(&text, "event=start"),
        1,
        "a second supervisor must never start a daemon of its own: {text}"
    );
    assert_eq!(
        count_log(&text, BANNER_LINE),
        0,
        "a refused claim must stay silent in the UI: the first supervisor is serving this runtime: \
         {text}"
    );
    assert_eq!(
        control_pid(&case.fixture.runtime()).await,
        Ok(Some(daemon)),
        "the daemon the first supervisor started must be untouched"
    );
    assert!(
        process_is_alive(daemon),
        "the first supervisor's daemon must still be serving"
    );
    assert!(
        process_is_alive(supervisor),
        "the first supervisor must still be supervising, not have been displaced"
    );

    // And it is still a working supervisor: a crash is still answered by one restart.
    case.crash(daemon);
    let after = case.await_log("decision=restart").await;
    assert_eq!(
        count_log(&after, "event=restart"),
        1,
        "the surviving supervisor keeps its restart budget: {after}"
    );
    let restarted = case.await_daemon_ready(Some(daemon)).await;
    assert_ne!(restarted, daemon, "the restart is a new process");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_keeper_that_dies_mid_run_refuses_the_restart() {
    let mut case = Case::new("keeper-lost", true);
    let supervisor = case.spawn_supervisor();
    let daemon = case.await_daemon_ready(None).await;

    let gate_on = case.log_text();
    assert_log(
        &gate_on,
        "event=gate supervised_restart=on",
        "supervision must have started with a live keeper: {gate_on}",
    );

    // The keeper goes away while the daemon is still serving, then the daemon crashes: the gate is
    // re-checked after the exit and must refuse, never decide from the startup state.
    let answered = case.stop_keeper();
    assert!(
        answered >= 1,
        "the keeper must have completed at least the startup handshake"
    );
    assert!(
        !case.fixture.keeper_socket().exists(),
        "the keeper socket must be gone before the crash"
    );

    case.crash(daemon);
    let text = case.await_log(BANNER_LINE).await;
    assert_log(
        &text,
        "exit=signal=9 kind=abnormal",
        "the crash itself is still on record: {text}",
    );
    assert_log(
        &text,
        "reason=no_live_keeper",
        "the stop reason names the lost keeper: {text}",
    );
    assert_eq!(
        count_log(&text, "event=gate supervised_restart=off"),
        1,
        "the mid-run death must close the gate on record: {text}"
    );
    assert_eq!(
        count_log(&text, "event=restart"),
        0,
        "a crash with no keeper left must never be restarted: {text}"
    );
    let status = case.await_exit(supervisor, "the supervisor").await;
    assert!(
        status.success(),
        "the refusal is a normal stop, got {status:?}; restart log:\n{text}"
    );
}
