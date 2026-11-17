//! Todo 15: the daemon integration harness for SSH tunnels.
//!
//! Three cases that no `--lib` filter can honestly run, because each one needs a real
//! `ferryx --daemon` process, a real `ssh` child it owns, and (for the first two) a real
//! loopback `sshd`:
//!
//! - `sigkill_daemon_reaps_orphan_child_and_releases_port`
//! - `host_record_change_tears_down_live_tunnel`
//! - `paired_peer_cannot_write_definitions`
//!
//! Run with `cargo test --manifest-path src-tauri/Cargo.toml --test ssh_tunnel_daemon --
//! --test-threads=1`. Not under `--lib`.
//!
//! This file owns its fixtures on purpose: `bridge_tests.rs`'s `sshd_binary()` and
//! `LOOPBACK_SSH_FIXTURE` live behind `#[cfg(test)]` in the lib crate and are not visible
//! from a separate `[[test]]` target, so the loopback server is spawned here in the same
//! shape `daemon/tunnel_forwarding.rs` uses (a bound listener in inetd mode, one `sshd -i`
//! per accepted connection, every pid recorded and taken down on drop).
//!
//! Isolation: every case runs the daemon with `FERRYX_DATA_DIR` and `FERRYX_RUNTIME_DIR`
//! pointed at its own tempdir, so it can neither collide with nor sweep a real `-dev`
//! daemon on a shared host. `--daemon` stays in the foreground (it prints
//! `FERRYX_DAEMON_READY` on stdout and serves until Shutdown), so `Command::child.id()` is
//! the daemon itself and a SIGKILL of that pid proves what these cases claim.

#![cfg(unix)]

use ferryx_lib::daemon::protocol::{
    DaemonRequest, DaemonResponse, TunnelForgetHostRequest, TunnelListResponse,
    TunnelSpecsSaveRequest, TunnelStartRequest, TunnelStopRequest, DAEMON_PROTOCOL_VERSION,
};
use ferryx_lib::daemon::DaemonServer;
use ferryx_lib::ssh::tunnel::{
    TunnelFailureCode, TunnelSpec, TunnelSpecId, TunnelState, TunnelView,
};
use ferryx_lib::ssh::{SshAuthMethod, SshHost, SshHostSource};
use serde_json::Value;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, UnixListener, UnixStream};
use tokio::process::{Child, Command};

const READY: &str = "FERRYX_DAEMON_READY";
/// Boot, handshake and one full observer tick must fit comfortably inside this.
const START_BUDGET: Duration = Duration::from_secs(60);
/// A teardown the host observer owes us, measured from the mutation.
const TEARDOWN_BUDGET: Duration = Duration::from_secs(15);
/// The observer's interval is five seconds; an idle window has to outlast one tick for the
/// observer to cancel itself. Here elapsed time IS the behavior under test (the re-arm), so
/// waiting is the assertion, not a synchronization shortcut.
const ONE_TICK: Duration = Duration::from_secs(6);

type TestResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

// ---------------------------------------------------------------------------
// Bounded waits
//
// Each of these waits for an effect produced by ANOTHER process (a child reaped, a port
// released, a row moved). There is no watch to subscribe to across the process boundary, so
// the loop re-reads the effect and yields; the deadline turns a hang into a failure. No
// assertion depends on the loop having run a particular number of times.
// ---------------------------------------------------------------------------

async fn wait_until<T>(what: &str, budget: Duration, mut probe: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + budget;
    loop {
        if let Some(value) = probe() {
            return value;
        }
        assert!(Instant::now() < deadline, "{what} did not happen within {budget:?}");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

/// Await a row by `specId`. A stopped definition has no runtime id, so this is the only way
/// to observe a stop.
async fn await_row(connection: &mut Connection, spec_id: &str, want: TunnelState) -> TestResult<()> {
    let deadline = Instant::now() + TEARDOWN_BUDGET;
    let mut last;
    loop {
        let list = connection.list().await?;
        match list.views.iter().find(|view| view.spec_id.0 == spec_id) {
            Some(view) if view.state == want => return Ok(()),
            Some(view) => last = format!("state={:?} failure={:?}", view.state, view.failure),
            None => last = "no row for that spec id".into(),
        }
        assert!(
            Instant::now() < deadline,
            "{spec_id} never reached {want:?} within {TEARDOWN_BUDGET:?} ({last})"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Await a row by runtime id, and hand back the view that satisfied the wait.
async fn await_runtime(
    connection: &mut Connection,
    runtime_id: &str,
    want: TunnelState,
) -> TestResult<TunnelView> {
    let deadline = Instant::now() + TEARDOWN_BUDGET;
    let mut last;
    loop {
        let list = connection.list().await?;
        match list
            .views
            .iter()
            .find(|view| view.runtime_id.as_ref().map(|id| id.0.as_str()) == Some(runtime_id))
        {
            Some(view) if view.state == want => return Ok(view.clone()),
            Some(view) => last = format!("state={:?} failure={:?}", view.state, view.failure),
            None => last = "the runtime is absent from the list".into(),
        }
        assert!(
            Instant::now() < deadline,
            "runtime {runtime_id} never reached {want:?} within {TEARDOWN_BUDGET:?} ({last})"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn await_port_free(port: u16, what: &str) {
    wait_until(what, TEARDOWN_BUDGET, || {
        std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).ok()
    })
    .await;
}

/// Exactly one `list`, for use after a wait that deliberately avoided one: a second poll
/// loop here would let `list`'s own inline adoption stand in for the observer.
async fn single_view(connection: &mut Connection, runtime_id: &str) -> TestResult<TunnelView> {
    connection
        .list()
        .await?
        .views
        .into_iter()
        .find(|view| view.runtime_id.as_ref().map(|id| id.0.as_str()) == Some(runtime_id))
        .ok_or_else(|| format!("runtime {runtime_id} is absent from the list").into())
}

// ---------------------------------------------------------------------------
// Loopback sshd fixture
// ---------------------------------------------------------------------------

fn sshd_binary() -> PathBuf {
    let candidates = ["/usr/sbin/sshd", "/usr/local/sbin/sshd", "/sbin/sshd"];
    for candidate in candidates {
        let candidate = PathBuf::from(candidate);
        if candidate.is_file() {
            return candidate;
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path) {
            let candidate = directory.join("sshd");
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    PathBuf::from(candidates[0])
}

fn login_user() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| {
            let output = std::process::Command::new("id")
                .arg("-un")
                .output()
                .expect("read the login user");
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        })
}

fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .unwrap_or_else(|error| panic!("chmod {mode:o} {}: {error}", path.display()));
}

/// A private OpenSSH server on loopback, its key material, and an echo destination the
/// forward can reach. Host-key trust is written into the DAEMON's home by [`Fixture::trust`],
/// because `ssh` looks in the `HOME` the daemon was spawned with, not the fixture's.
struct Fixture {
    host: SshHost,
    destination_port: u16,
    /// The `known_hosts` row for this fixture's `sshd`, pinned to its ephemeral port.
    known_hosts_line: String,
    sshd_pids: Arc<Mutex<Vec<u32>>>,
    server: tokio::task::JoinHandle<()>,
    destination: tokio::task::JoinHandle<()>,
    /// Kept last so the directories outlive every task and file they describe.
    _directory: tempfile::TempDir,
}

impl Fixture {
    async fn start() -> Self {
        let directory = tempfile::Builder::new()
            .prefix("fx-tunnel-daemon")
            .tempdir()
            .expect("create the tunnel daemon fixture directory");
        set_mode(directory.path(), 0o700);
        for name in ["host_key", "user_key"] {
            let output = std::process::Command::new("ssh-keygen")
                .args(["-q", "-t", "ed25519", "-N", "", "-f"])
                .arg(directory.path().join(name))
                .output()
                .expect("run ssh-keygen for the tunnel fixture");
            assert!(
                output.status.success(),
                "ssh-keygen {name}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let sshd_config = directory.path().join("sshd_config");
        std::fs::write(
            &sshd_config,
            format!(
                "HostKey {}\nAuthorizedKeysFile {}\nStrictModes no\nUsePAM no\n\
                 PasswordAuthentication no\nKbdInteractiveAuthentication no\n\
                 LogLevel ERROR\nUseDNS no\n",
                directory.path().join("host_key").display(),
                directory.path().join("user_key.pub").display(),
            ),
        )
        .expect("write the sshd fixture config");

        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind the loopback sshd fixture");
        let sshd_port = listener.local_addr().expect("fixture sshd address").port();

        // The daemon's `ssh` child inherits the daemon's HOME, so the pin is written there
        // by `trust` once the isolation directories exist.
        let host_key = std::fs::read_to_string(directory.path().join("host_key.pub"))
            .expect("read the fixture host key");
        let known_hosts_line = format!("[127.0.0.1]:{sshd_port} {host_key}");

        let sshd_pids: Arc<Mutex<Vec<u32>>> = Arc::default();
        let accept_pids = Arc::clone(&sshd_pids);
        let server = tokio::spawn(async move {
            let mut children = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let Ok((stream, _)) = accepted else { break };
                        let std_stream = stream.into_std().expect("convert the accepted sshd socket");
                        std_stream.set_nonblocking(false).expect("make the sshd socket blocking");
                        use std::os::fd::OwnedFd;
                        let mut command = Command::new(sshd_binary());
                        command
                            .args(["-i", "-e", "-f"])
                            .arg(&sshd_config)
                            .stdin(Stdio::from(OwnedFd::from(
                                std_stream.try_clone().expect("clone the accepted sshd socket"),
                            )))
                            .stdout(Stdio::from(OwnedFd::from(std_stream)))
                            .stderr(Stdio::null())
                            .process_group(0)
                            .kill_on_drop(true);
                        let mut child = command.spawn().expect("spawn the loopback sshd");
                        let pid = child.id().expect("sshd fixture pid");
                        accept_pids.lock().unwrap().push(pid);
                        let reaped = Arc::clone(&accept_pids);
                        children.spawn(async move {
                            let _ = child.wait().await;
                            reaped.lock().unwrap().retain(|recorded| *recorded != pid);
                        });
                    }
                    Some(_) = children.join_next(), if !children.is_empty() => {}
                }
            }
        });

        // The forward's destination: accept and echo, so the registry's readiness probe
        // really traversed the tunnel instead of landing on the local listener.
        let destination_listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind the echo destination");
        let destination_port = destination_listener
            .local_addr()
            .expect("destination address")
            .port();
        let destination = tokio::spawn(async move {
            let mut clients = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted = destination_listener.accept() => {
                        let Ok((mut stream, _)) = accepted else { break };
                        clients.spawn(async move {
                            let mut buffer = [0u8; 8192];
                            while let Ok(read) = stream.read(&mut buffer).await {
                                if read == 0 || stream.write_all(&buffer[..read]).await.is_err() {
                                    break;
                                }
                            }
                        });
                    }
                    Some(_) = clients.join_next(), if !clients.is_empty() => {}
                }
            }
        });

        let host = SshHost {
            id: "t15-loopback".into(),
            label: "t15 loopback sshd".into(),
            hostname: "127.0.0.1".into(),
            username: Some(login_user()),
            port: Some(sshd_port),
            identity_file: Some(
                directory
                    .path()
                    .join("user_key")
                    .to_string_lossy()
                    .into_owned(),
            ),
            jump_host: None,
            source: SshHostSource::Manual,
            auth_method: SshAuthMethod::Key,
            disabled: None,
            shell: None,
        };
        Self {
            host,
            destination_port,
            known_hosts_line,
            sshd_pids,
            server,
            destination,
            _directory: directory,
        }
    }

    fn host(&self) -> &SshHost {
        &self.host
    }

    /// Pin this fixture's host key in `home`'s `.ssh/known_hosts`, where the daemon's `ssh`
    /// child will look. `StrictHostKeyChecking=yes` is forced on the tunnel argv, so this is
    /// the only thing that lets the child get past host-key verification.
    fn trust(&self, home: &Path) -> io::Result<()> {
        let ssh_dir = home.join(".ssh");
        std::fs::create_dir_all(&ssh_dir)?;
        set_mode(home, 0o700);
        set_mode(&ssh_dir, 0o700);
        let known_hosts = ssh_dir.join("known_hosts");
        std::fs::write(&known_hosts, &self.known_hosts_line)?;
        set_mode(&known_hosts, 0o600);
        Ok(())
    }

    /// The authoritative host store the daemon reads (`FERRYX_DATA_DIR/ssh_hosts.json`).
    fn write_host_record(&self, data: &Path, host: &SshHost) -> io::Result<()> {
        std::fs::write(
            data.join("ssh_hosts.json"),
            serde_json::json!({ "version": 1, "hosts": [host] }).to_string(),
        )
    }

    /// Every `intent: running` record in the sidecar, read as raw JSON so this harness can
    /// never drift from the record shape the daemon actually writes.
    fn running_records(&self, data: &Path) -> Vec<Value> {
        let Ok(bytes) = std::fs::read(data.join("ssh_tunnel_runtimes.json")) else {
            return Vec::new();
        };
        let Ok(envelope) = serde_json::from_slice::<Value>(&bytes) else {
            return Vec::new();
        };
        envelope
            .get("records")
            .and_then(Value::as_object)
            .map(|records| {
                records
                    .values()
                    .filter(|record| record.get("intent").and_then(Value::as_str) == Some("running"))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
        self.destination.abort();
        // Every sshd child was spawned into its own group; take the group, not the pid.
        for pid in self.sshd_pids.lock().unwrap().iter().copied() {
            unsafe { libc::kill(-(pid as i32), libc::SIGKILL) };
        }
    }
}

// ---------------------------------------------------------------------------
// Process identity, so a teardown can never signal a recycled pid
// ---------------------------------------------------------------------------

/// `Some("")` when the lookup ran and no such process exists, `None` when it could not be
/// determined — the same read `runtime_records::PsProcessIdentity` performs.
fn process_start_time(pid: u32) -> Option<String> {
    let output = std::process::Command::new("ps")
        .args(["-o", "lstart=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    if !output.status.success() {
        return Some(String::new());
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn process_alive(pid: u32) -> bool {
    process_start_time(pid).is_some_and(|start| !start.is_empty())
}

/// The safety net Todo 7's sweep test arms, repeated here: if the successor daemon never
/// reclaims the orphan, this test must still not leave a stray `ssh` holding a loopback port
/// on a shared build host. Verification is by pid AND start time, never by pattern.
struct OrphanGuard {
    pid: u32,
    pgid: i32,
    start_time: String,
    armed: bool,
}

impl OrphanGuard {
    fn arm(record: &Value) -> Option<Self> {
        Some(Self {
            pid: u32::try_from(record.get("pid")?.as_u64()?).ok()?,
            pgid: i32::try_from(record.get("pgid")?.as_i64()?).ok()?,
            start_time: record.get("processStartTime")?.as_str()?.to_string(),
            armed: true,
        })
    }

    fn disarm(&mut self, why: &str) {
        eprintln!("orphan guard disarmed: {why}");
        self.armed = false;
    }
}

impl Drop for OrphanGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        if process_start_time(self.pid).as_deref() != Some(self.start_time.as_str()) {
            // Gone, or the pid was recycled: either way there is nothing of ours to signal.
            return;
        }
        eprintln!(
            "orphan guard: killing the tunnel child group pgid={} pid={}",
            self.pgid, self.pid
        );
        unsafe { libc::kill(-self.pgid, libc::SIGKILL) };
    }
}

// ---------------------------------------------------------------------------
// The daemon under test
// ---------------------------------------------------------------------------

/// Everything the daemon resolves through the environment, pointed at one tempdir.
struct Isolation {
    root: PathBuf,
    runtime: PathBuf,
    data: PathBuf,
    home: PathBuf,
}

impl Isolation {
    fn create(directory: tempfile::TempDir) -> Self {
        let root = directory.keep();
        let runtime = root.join("run");
        let data = root.join("data");
        let home = root.join("home");
        for path in [&runtime, &data, &home, &root.join("sessions"), &root.join("tmp")] {
            std::fs::create_dir_all(path).expect("create an isolated daemon directory");
        }
        set_mode(&runtime, 0o700);
        set_mode(&home, 0o700);
        Self {
            root,
            runtime,
            data,
            home,
        }
    }
}

impl Drop for Isolation {
    /// `TempDir::keep` hands the directory over so the daemon's own paths outlive the
    /// fixture, which leaves the cleanup to us. Test cases declare the isolation before any
    /// daemon, so the daemon's own `Drop` has already taken its children down by the time
    /// this runs; without it every run would litter the temp dir with sockets and sidecars.
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

struct Daemon {
    child: Child,
    pid: u32,
    /// The daemon's own data directory, so an early-returning case can still find the
    /// children this daemon recorded and take them down.
    data: PathBuf,
    /// Held for the life of the process: dropping it would close the readiness pipe.
    _stdout: tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
}

impl Drop for Daemon {
    /// Safety net for a case that returns early through `?`: the normal paths call
    /// `shutdown`, which reaps every child through the daemon's own clean exit. A leaked
    /// daemon would keep its UDS and its `ssh` children holding loopback ports for the
    /// rest of the run, so anything still alive here is killed by pid, and each child it
    /// recorded is killed by group only after its pid AND start time still match.
    fn drop(&mut self) {
        // An exited daemon already reaped its children; only a still-running one can be
        // holding the socket. `start_kill` is a no-op once the child has exited, and the
        // kill releases the port immediately, so there is nothing to wait for here (a
        // blocking sleep inside a Drop on a runtime worker would stall the test).
        let running = matches!(self.child.try_wait(), Ok(None));
        if running {
            let _ = self.child.start_kill();
            eprintln!(
                "dropped a live daemon pid={}; reaping the children it recorded",
                self.pid
            );
        }
        let Ok(bytes) = std::fs::read(self.data.join("ssh_tunnel_runtimes.json")) else {
            return;
        };
        let Ok(envelope) = serde_json::from_slice::<Value>(&bytes) else {
            return;
        };
        let records = envelope.get("records").and_then(Value::as_object);
        for record in records.map(|map| map.values().cloned()).into_iter().flatten() {
            if record.get("owner").and_then(|owner| owner.get("ownerPid")).and_then(Value::as_u64)
                != Some(u64::from(self.pid))
            {
                continue;
            }
            let Some(pid) = record.get("pid").and_then(Value::as_u64).map(|pid| pid as u32) else {
                continue;
            };
            let Some(pgid) = record.get("pgid").and_then(Value::as_i64) else {
                continue;
            };
            let start = record.get("processStartTime").and_then(Value::as_str);
            if start.is_some() && process_start_time(pid).as_deref() == start {
                unsafe { libc::kill(-(pgid as i32), libc::SIGKILL) };
                eprintln!("dropped daemon: killed recorded tunnel child pid={pid} pgid={pgid}");
            }
        }
    }
}

impl Daemon {
    /// Spawn `ferryx --daemon` in the foreground and wait for its readiness line.
    async fn spawn(isolation: &Isolation) -> TestResult<Self> {
        let root = isolation.root.clone();
        let mut child = Command::new(env!("CARGO_BIN_EXE_ferryx"))
            .arg("--daemon")
            // Never inherit a real daemon's endpoints, remote configuration, or the user's HOME.
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("FERRYX_RUNTIME_DIR", &isolation.runtime)
            .env("FERRYX_DATA_DIR", &isolation.data)
            .env("FERRYX_SESSION_DIR", root.join("sessions"))
            .env("HOME", &isolation.home)
            .env("USERPROFILE", &isolation.home)
            .env("TMPDIR", root.join("tmp"))
            .env("TMP", root.join("tmp"))
            .env("TEMP", root.join("tmp"))
            .current_dir(&isolation.home)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let pid = child.id().ok_or("the spawned daemon has no pid")?;
        let mut stdout = BufReader::new(child.stdout.take().ok_or("missing daemon stdout")?).lines();
        let line = tokio::time::timeout(START_BUDGET, stdout.next_line())
            .await
            .map_err(|_| format!("daemon {pid} never announced readiness"))??;
        if line.as_deref() != Some(READY) {
            return Err(format!("daemon {pid} printed {line:?}, not {READY}").into());
        }
        Ok(Self {
            child,
            pid,
            data: isolation.data.clone(),
            _stdout: stdout,
        })
    }

    async fn connection(&self, isolation: &Isolation) -> TestResult<Connection> {
        Connection::open(&isolation.runtime.join("daemon.sock"), self.pid).await
    }

    /// SIGKILL: no shutdown path runs, so the `ssh` child is orphaned exactly as a crash
    /// leaves it. That is the case the successor's sweep exists for.
    async fn sigkill(&mut self) -> TestResult<()> {
        unsafe { libc::kill(self.pid as i32, libc::SIGKILL) };
        let status = tokio::time::timeout(START_BUDGET, self.child.wait()).await??;
        eprintln!("SIGKILLed daemon pid={} status={status}", self.pid);
        Ok(())
    }

    /// Orderly stop, so a normally-ending case leaves no daemon behind either.
    async fn shutdown(&mut self, isolation: &Isolation) {
        if self.child.try_wait().ok().flatten().is_some() {
            return;
        }
        if let Ok(mut connection) = Connection::open(&isolation.runtime.join("daemon.sock"), self.pid).await
        {
            let _ = connection.send(DaemonRequest::Shutdown).await;
        }
        match tokio::time::timeout(START_BUDGET, self.child.wait()).await {
            Ok(Ok(status)) => eprintln!("stopped daemon pid={} status={status}", self.pid),
            _ => {
                let _ = self.child.kill().await;
                let _ = self.child.wait().await;
                eprintln!("force-killed daemon pid={}", self.pid);
            }
        }
    }
}

struct Connection {
    reader: BufReader<tokio::net::unix::OwnedReadHalf>,
    writer: tokio::net::unix::OwnedWriteHalf,
}

impl Connection {
    async fn open(socket: &Path, expect_pid: u32) -> TestResult<Self> {
        let stream = tokio::time::timeout(START_BUDGET, UnixStream::connect(socket)).await??;
        let (reader, writer) = stream.into_split();
        let mut connection = Self {
            reader: BufReader::new(reader),
            writer,
        };
        let answer = connection
            .send(DaemonRequest::Handshake {
                version: DAEMON_PROTOCOL_VERSION,
                token: None,
            })
            .await?;
        match answer {
            DaemonResponse::HandshakeOk { pid, .. } if pid == expect_pid => Ok(connection),
            DaemonResponse::HandshakeOk { pid, .. } => Err(format!(
                "{} answered with pid {pid}, not the expected {expect_pid}",
                socket.display()
            )
            .into()),
            other => Err(format!("unexpected handshake answer: {other:?}").into()),
        }
    }

    async fn send(&mut self, request: DaemonRequest) -> TestResult<DaemonResponse> {
        let mut wire = serde_json::to_vec(&request)?;
        wire.push(b'\n');
        self.writer.write_all(&wire).await?;
        self.writer.flush().await?;
        let mut line = String::new();
        let read = tokio::time::timeout(START_BUDGET, self.reader.read_line(&mut line)).await??;
        if read == 0 {
            return Err("the daemon closed the connection".into());
        }
        Ok(serde_json::from_str(&line)?)
    }

    async fn list(&mut self) -> TestResult<TunnelListResponse> {
        match self.send(DaemonRequest::SshTunnelList).await? {
            DaemonResponse::SshTunnelList { response } => Ok(response),
            other => Err(format!("unexpected list answer: {other:?}").into()),
        }
    }

    async fn host_revision(&mut self, host_id: &str) -> TestResult<u64> {
        Ok(self
            .list()
            .await?
            .host_revisions
            .iter()
            .find(|entry| entry.host_id == host_id)
            .map(|entry| entry.revision)
            .unwrap_or(0))
    }

    /// Save at the revision the daemon actually holds, so the revision barrier is never the
    /// reason a case fails.
    async fn save_specs(&mut self, host_id: &str, specs: Vec<TunnelSpec>) -> TestResult<u64> {
        let definitions_revision = self.list().await?.definitions_revision;
        match self
            .send(DaemonRequest::SshTunnelSpecsSave {
                request: TunnelSpecsSaveRequest {
                    host_id: host_id.into(),
                    specs,
                    expected_revision: Some(definitions_revision),
                    force: false,
                    discard_invalid: false,
                },
            })
            .await?
        {
            DaemonResponse::SshTunnelSpecsSave { response } => Ok(response.definitions_revision),
            other => Err(format!("unexpected save answer: {other:?}").into()),
        }
    }

    async fn start_saved(&mut self, host_id: &str, spec_id: &str) -> TestResult<TunnelView> {
        let host_revision = self.host_revision(host_id).await?;
        match self
            .send(DaemonRequest::SshTunnelStart {
                request: TunnelStartRequest {
                    host_id: host_id.into(),
                    spec: None,
                    spec_id: Some(spec_id.into()),
                    host_revision: Some(host_revision),
                },
            })
            .await?
        {
            DaemonResponse::SshTunnelStart { response } => Ok(response.view),
            other => Err(format!("unexpected start answer: {other:?}").into()),
        }
    }

    async fn stop(&mut self, runtime_id: &str) -> TestResult<bool> {
        match self
            .send(DaemonRequest::SshTunnelStop {
                request: TunnelStopRequest {
                    runtime_id: runtime_id.into(),
                },
            })
            .await?
        {
            DaemonResponse::SshTunnelStop { response } => Ok(response.stopped),
            other => Err(format!("unexpected stop answer: {other:?}").into()),
        }
    }
}

fn spec(id: &str, label: &str, local_port: u16, destination_port: u16) -> TunnelSpec {
    TunnelSpec {
        id: TunnelSpecId(id.into()),
        label: label.into(),
        local_port: Some(local_port),
        remote_host: "127.0.0.1".into(),
        remote_port: destination_port,
        enabled: true,
    }
}

fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .expect("reserve a loopback port");
    let port = listener.local_addr().expect("reserved address").port();
    drop(listener);
    port
}

fn runtime_id_of(view: &TunnelView) -> String {
    view.runtime_id
        .as_ref()
        .expect("an accepted start has a runtime id")
        .0
        .clone()
}

/// The spawned daemon builds its argv with `ConfigSource::Real`, which scans the host's
/// system `ssh_config` and refuses any directive `argv::INERT` does not cover. macOS ships
/// `Include /etc/ssh/ssh_config.d/*` -> `100-macos.conf` -> `crypto.conf`, whose `Ciphers`
/// line is outside that list, so a start there answers `Failed { unsupported_config }`
/// before any child exists. That is a property of the platform's config, not of the tunnel
/// runtime, and a caller must not read it as "the forward did not work": say so here instead
/// of failing later with a bare state mismatch.
fn assert_startable(view: &TunnelView, what: &str) -> TestResult<()> {
    if view.state == TunnelState::Failed
        && view.failure == Some(TunnelFailureCode::UnsupportedConfig)
    {
        return Err(format!(
            "{what}: the daemon refused the host before spawning (failure_piece={:?}). This \
             harness runs the real daemon, which scans the host's system ssh_config \
             (/etc/ssh/ssh_config and its includes); a directive outside argv::INERT there \
             refuses every start, so this row never reaches the tunnel runtime.",
            view.failure_piece
        )
        .into());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Case 1
// ---------------------------------------------------------------------------

/// A SIGKILLed daemon leaves its `ssh -L` child alive: the child does not die with its
/// parent and keeps `127.0.0.1:<port>`. The successor must reclaim it from the runtimes
/// sidecar — proving the reap by the port becoming bindable AND the record disappearing,
/// never by `waitpid`, which only the dead parent could call — and the saved definition must
/// then list as `stopped`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sigkill_daemon_reaps_orphan_child_and_releases_port() -> TestResult<()> {
    let fixture = Fixture::start().await;
    let isolation = Isolation::create(tempfile::tempdir()?);
    fixture.trust(&isolation.home)?;
    fixture.write_host_record(&isolation.data, fixture.host())?;
    let host_id = fixture.host().id.clone();
    let local_port = free_port();

    let mut first = Daemon::spawn(&isolation).await?;
    let mut connection = first.connection(&isolation).await?;
    connection
        .save_specs(
            &host_id,
            vec![spec("saved-one", "postgres", local_port, fixture.destination_port)],
        )
        .await?;
    let started = connection.start_saved(&host_id, "saved-one").await?;
    assert_startable(&started, "case 1")?;
    let runtime_id = runtime_id_of(&started);
    await_runtime(&mut connection, &runtime_id, TunnelState::Listening).await?;

    // Read the orphan's identity BEFORE the kill: afterwards the only process that could
    // report the child's pid and start time is gone.
    let record = wait_until("a running runtime record", TEARDOWN_BUDGET, || {
        fixture
            .running_records(&isolation.data)
            .into_iter()
            .find(|record| record.get("runtimeId").and_then(Value::as_str) == Some(runtime_id.as_str()))
    })
    .await;
    let orphan_pid = u32::try_from(record["pid"].as_u64().expect("a pid record carries pid"))?;
    let orphan_start = record["processStartTime"]
        .as_str()
        .expect("a pid record carries the start time")
        .to_string();
    assert!(
        process_alive(orphan_pid),
        "the tunnel child must be live before the kill (pid {orphan_pid})"
    );
    let mut guard = OrphanGuard::arm(&record).expect("the record carries pid, pgid and start time");
    eprintln!(
        "recorded orphan pid={orphan_pid} start={orphan_start:?} port={local_port} runtime={runtime_id}"
    );

    first.sigkill().await?;
    drop(connection);
    assert!(
        process_alive(orphan_pid),
        "the ssh child outlives a SIGKILLed daemon; that is what the sidecar exists for"
    );

    // The successor reclaims on its boot sweep: the first tick of its never-cancelled timer.
    let mut second = Daemon::spawn(&isolation).await?;
    wait_until("the successor's sweep to retire the dead instance's record", TEARDOWN_BUDGET, || {
        fixture.running_records(&isolation.data).is_empty().then_some(())
    })
    .await;
    await_port_free(local_port, "the reclaimed port must be bindable").await;
    // The spec's observable is the port and the record, not a process liveness probe: a
    // SIGKILLed child can linger as a zombie until its adopter reaps it, so `ps` would be a
    // flaky oracle here while a bindable port is a definitive one. The guard stays armed until
    // this point, so a sweep that did nothing would still be cleaned up below.
    guard.disarm("the successor daemon released the port, so the orphan is gone");

    let mut connection = second.connection(&isolation).await?;
    let list = connection.list().await?;
    let view = list
        .views
        .iter()
        .find(|view| view.spec_id.0 == "saved-one")
        .expect("a saved definition is still listed after its runtime is gone");
    assert_eq!(
        view.state,
        TunnelState::Stopped,
        "a reclaimed definition lists as stopped: {view:?}"
    );
    assert!(view.runtime_id.is_none(), "stopped means no runtime id");
    assert!(
        list.definitions
            .iter()
            .any(|entry| entry.host_id == host_id && entry.specs.len() == 1),
        "the definition itself survived the crash: {list:?}"
    );

    second.shutdown(&isolation).await;
    Ok(())
}

// ---------------------------------------------------------------------------
// Case 2
// ---------------------------------------------------------------------------

/// The host observer is the only thing that notices a host record edited behind the daemon's
/// back, and it is cancelled when nothing is live. Both halves live in one fn because the
/// second is what proves the cancellation re-arms: an observer that is cancelled once and
/// never re-armed passes the first half and every lib test.
///
/// 1. A live tunnel is torn down within 15 s of the host record changing, and the port is
///    released.
/// 2. Stop it, sit past one observer tick with no live runtime (the observer cancels), start
///    a second tunnel, change the record again, and require the same teardown within 15 s.
///
/// Both halves wait on the PORT, never on `list`: `list` and `start` run the same adoption
/// inline, so a poll loop would let a broken observer pass. A freed port can only come after
/// the bump and the kill, so the single `list` that follows reads a decision already made.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn host_record_change_tears_down_live_tunnel() -> TestResult<()> {
    let fixture = Fixture::start().await;
    let isolation = Isolation::create(tempfile::tempdir()?);
    fixture.trust(&isolation.home)?;
    fixture.write_host_record(&isolation.data, fixture.host())?;
    let host_id = fixture.host().id.clone();

    let mut daemon = Daemon::spawn(&isolation).await?;
    let mut connection = daemon.connection(&isolation).await?;
    let first_port = free_port();
    let second_port = free_port();
    connection
        .save_specs(
            &host_id,
            vec![
                spec("saved-one", "cache", first_port, fixture.destination_port),
                spec("saved-two", "database", second_port, fixture.destination_port),
            ],
        )
        .await?;

    let first_started = connection.start_saved(&host_id, "saved-one").await?;
    assert_startable(&first_started, "case 2, first tunnel")?;
    let first_runtime = runtime_id_of(&first_started);
    await_runtime(&mut connection, &first_runtime, TunnelState::Listening).await?;

    // Rewrite the host record through the plain store writer, behind the daemon's back.
    // The mutated field is `disabled`, which is part of the compared identity tuple and
    // still leaves the next start allowed: `Some(false)` is not the disabled refusal, and
    // it does not change who authenticates, so the re-arm row can start again.
    let mut edited = fixture.host().clone();
    edited.disabled = Some(false);
    fixture.write_host_record(&isolation.data, &edited)?;

    await_port_free(first_port, "the observer must release the torn-down port").await;
    let torn = single_view(&mut connection, &first_runtime).await?;
    assert_eq!(
        torn.failure,
        Some(TunnelFailureCode::HostChanged),
        "the teardown reason is typed, not prose: {torn:?}"
    );
    assert_eq!(torn.state, TunnelState::Failed, "a torn-down runtime is failed: {torn:?}");

    // -- the re-arm row -----------------------------------------------------
    assert!(
        connection.stop(&first_runtime).await?,
        "stopping a live runtime answers stopped=true"
    );
    await_row(&mut connection, "saved-one", TunnelState::Stopped).await?;
    // Past one observer tick with nothing live: this is the window in which the observer
    // cancels itself. Nothing is asserted about the cancellation (it is internal); the
    // second half is what proves it re-armed. This sleep is the assertion, not a polling
    // shortcut: the window has to elapse for the cancellation to happen at all.
    tokio::time::sleep(ONE_TICK).await;

    let second_started = connection.start_saved(&host_id, "saved-two").await?;
    assert_startable(&second_started, "case 2, re-armed tunnel")?;
    let second_runtime = runtime_id_of(&second_started);
    await_runtime(&mut connection, &second_runtime, TunnelState::Listening).await?;

    // Back to the shape the fixture wrote: a second real identity change, still startable.
    let mut edited_again = edited.clone();
    edited_again.disabled = None;
    fixture.write_host_record(&isolation.data, &edited_again)?;

    await_port_free(second_port, "the re-armed observer must release the second port").await;
    let torn = single_view(&mut connection, &second_runtime).await?;
    assert_eq!(
        torn.failure,
        Some(TunnelFailureCode::HostChanged),
        "the re-armed observer owed the same teardown: {torn:?}"
    );

    daemon.shutdown(&isolation).await;
    Ok(())
}

// ---------------------------------------------------------------------------
// Case 3
// ---------------------------------------------------------------------------

/// A paired peer can read the tunnel list but may never write definitions: `SpecsSave`,
/// `SpecsReset` and `ForgetHost` all answer `details.reason = "local_only"`, and the
/// definitions file is byte-identical afterwards. `SpecsReset` could quarantine the
/// operator's file and `ForgetHost` could delete a host's rows, which is why the boundary is
/// checked per variant rather than once for the family.
///
/// This case uses an in-process owner rather than a spawned daemon because the paired
/// transport is a dispatch-loop entry (`handle_paired_client`), not something a client can
/// ask for over the socket; the owner path is exercised on the same server so "refused"
/// cannot be confused with "this server never serves tunnels".
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn paired_peer_cannot_write_definitions() -> TestResult<()> {
    let fixture = Fixture::start().await;
    let root = tempfile::tempdir()?;
    let data = root.path().join("data");
    std::fs::create_dir_all(&data)?;
    fixture.write_host_record(&data, fixture.host())?;
    let host_id = fixture.host().id.clone();

    // Isolated catalog paths, so no canonical identity directory is consulted.
    let server = Arc::new(DaemonServer::new_with_paths(
        Some(data.join("config.json")),
        Some(data.join("auth.json")),
    ));
    let definitions = data.join("ssh_tunnels.json");
    assert!(
        !definitions.exists(),
        "the fixture starts with no definitions file"
    );

    // The owner connection saves one definition first: "byte-identical" needs bytes, and the
    // local path must be proven to work on this same server.
    let owner_socket = root.path().join("owner.sock");
    let owner_listener = UnixListener::bind(&owner_socket)?;
    let saving = Arc::clone(&server);
    let owner = tokio::spawn(async move {
        let (stream, _) = owner_listener.accept().await.expect("owner accept");
        saving.handle_unix_client(stream).await;
    });
    let owner_bytes = {
        let mut connection = Connection::open(&owner_socket, std::process::id()).await?;
        let saved = connection
            .send(DaemonRequest::SshTunnelSpecsSave {
                request: TunnelSpecsSaveRequest {
                    host_id: host_id.clone(),
                    specs: vec![spec("saved-one", "cache", free_port(), fixture.destination_port)],
                    expected_revision: Some(0),
                    force: false,
                    discard_invalid: false,
                },
            })
            .await?;
        match saved {
            DaemonResponse::SshTunnelSpecsSave { .. } => {}
            other => return Err(format!("the owner's save must succeed: {other:?}").into()),
        }
        let bytes = std::fs::read(&definitions)?;
        assert!(!bytes.is_empty(), "the owner's save created the definitions file");
        bytes
    };
    owner.abort();

    // Now the same server over the paired transport.
    let paired_socket = root.path().join("paired.sock");
    let paired_listener = UnixListener::bind(&paired_socket)?;
    let serving = Arc::clone(&server);
    let paired = tokio::spawn(async move {
        let (stream, _) = paired_listener.accept().await.expect("paired accept");
        serving.handle_paired_client(stream).await;
    });
    let mut peer = Connection::open(&paired_socket, std::process::id()).await?;

    let requests = vec![
        (
            "SpecsSave",
            DaemonRequest::SshTunnelSpecsSave {
                request: TunnelSpecsSaveRequest {
                    host_id: host_id.clone(),
                    specs: vec![],
                    expected_revision: Some(1),
                    force: true,
                    discard_invalid: true,
                },
            },
        ),
        ("SpecsReset", DaemonRequest::SshTunnelSpecsReset),
        (
            "ForgetHost",
            DaemonRequest::SshTunnelForgetHost {
                request: TunnelForgetHostRequest {
                    host_id: host_id.clone(),
                },
            },
        ),
    ];
    for (name, request) in requests {
        match peer.send(request).await? {
            DaemonResponse::Error { code, details, .. } => {
                assert_eq!(
                    code.as_deref(),
                    Some("UNSUPPORTED"),
                    "{name} must be refused as UNSUPPORTED"
                );
                let details = details.expect("a typed refusal carries details");
                assert_eq!(
                    details["reason"], "local_only",
                    "{name} must name the local-owner boundary"
                );
            }
            other => return Err(format!("{name} must be refused, got {other:?}").into()),
        }
    }
    drop(peer);
    paired.abort();

    assert_eq!(
        std::fs::read(&definitions)?,
        owner_bytes,
        "a paired peer's refusals must leave the definitions file byte-identical"
    );
    let persisted: Value = serde_json::from_slice(&std::fs::read(&definitions)?)?;
    assert_eq!(
        persisted["hosts"].get(&host_id).and_then(Value::as_array).map(Vec::len),
        Some(1),
        "the paired peer removed nothing: {persisted}"
    );
    Ok(())
}
