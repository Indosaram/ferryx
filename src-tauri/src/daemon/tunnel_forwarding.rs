//! Todo 9: end-to-end forwarding proof against a real loopback `sshd`.
//!
//! These tests drive the production `ssh::tunnel::registry` (argv builder, readiness
//! probe, child supervision, reconnect) over a private OpenSSH server and a
//! multi-accept echo destination. Nothing here fakes a forward: every assertion is
//! either bytes that reached the destination and came back, a listener that does or
//! does not exist, or a typed view/ring observation from the registry.
//!
//! Readiness is always awaited through the registry's own state/exit watches under a
//! bounded timeout. This module contains no sleep.

use crate::ipc::IpcError;
use crate::ssh::tunnel::argv::{self, ConfigSource};
use crate::ssh::tunnel::registry::{
    classify_ssh_stderr, ProcessSpawner, ProbeSocketFactory, Signal, SpawnedTunnel, TunnelChild,
    TunnelRegistry, TunnelSpawner, TunnelTransition,
};
use crate::ssh::tunnel::{
    TunnelFailureCode, TunnelRuntimeId, TunnelSpec, TunnelSpecId, TunnelState, TunnelView,
};
use crate::ssh::{SshAuthMethod, SshHost, SshHostSource};
use std::collections::HashMap;
use std::io;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::{ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::timeout;

const START_DEADLINE: Duration = Duration::from_secs(90);

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Allocation hint only, exactly like the registry's own allocator: the listener
/// is closed before the port is handed to `ssh`.
fn free_port() -> u16 {
    let listener =
        std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("reserve ephemeral port");
    let port = listener.local_addr().expect("reserved port address").port();
    drop(listener);
    port
}

fn ssh_program() -> PathBuf {
    let system = PathBuf::from("/usr/bin/ssh");
    if system.is_file() {
        return system;
    }
    PathBuf::from("ssh")
}

/// macOS ships `sshd` at `/usr/sbin`, Linux distributions use any of these.
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

fn kill_group(pid: u32, sig: libc::c_int) {
    if pid == 0 {
        return;
    }
    // Every fixture child leads its own process group, so a group kill takes the
    // monitor and its session children with it.
    let result = unsafe { libc::kill(-(pid as libc::pid_t), sig) };
    if result != 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ESRCH) {
            eprintln!("kill fixture process group {pid}: {error}");
        }
    }
}

/// Await a registry child-exit watch. The registry reaps every child it spawns, so
/// this is the event that proves a transport is gone rather than sampled.
async fn await_exit(mut rx: watch::Receiver<Option<ExitStatus>>) {
    let reached = timeout(Duration::from_secs(60), async {
        while rx.borrow().is_none() {
            if rx.changed().await.is_err() {
                break;
            }
        }
    })
    .await;
    assert!(reached.is_ok(), "tunnel child exit deadline");
    assert!(rx.borrow().is_some(), "tunnel child must be reaped");
}

/// Await a registry state watch until the live view satisfies `ready`, bounded, and
/// report the transition ring when it does not arrive: a silent green is never
/// acceptable here. Every wake comes from the registry; the bound turns a hang into
/// a failure, it is not a poll interval.
async fn await_view(
    mut rx: watch::Receiver<TunnelState>,
    registry: &TunnelRegistry,
    id: &TunnelRuntimeId,
    what: &str,
    mut ready: impl FnMut(&TunnelView) -> bool,
) {
    let reached = timeout(START_DEADLINE, async {
        loop {
            if ready(&view_of(registry, id)) {
                return true;
            }
            if rx.changed().await.is_err() {
                return false;
            }
        }
    })
    .await;
    assert_eq!(
        reached.ok(),
        Some(true),
        "{what} never arrived; ring {:?}",
        registry.transitions(id)
    );
}

fn view_of(registry: &TunnelRegistry, id: &TunnelRuntimeId) -> TunnelView {
    registry
        .list(&HashMap::new())
        .expect("list tunnels")
        .into_iter()
        .find(|view| view.runtime_id.as_ref() == Some(id))
        .expect("runtime view")
}

/// Write `payload` through the forwarded port and read the destination's echo back.
/// The payload only exists on the wire, so an identical echo proves the bytes
/// traversed the tunnel twice.
async fn round_trip(port: u16, payload: &[u8]) -> Vec<u8> {
    let outcome = timeout(Duration::from_secs(30), async {
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await?;
        stream.write_all(payload).await?;
        stream.flush().await?;
        let mut echoed = vec![0u8; payload.len()];
        stream.read_exact(&mut echoed).await?;
        Ok::<Vec<u8>, io::Error>(echoed)
    })
    .await
    .unwrap_or_else(|_| panic!("forwarded round trip on 127.0.0.1:{port} exceeded its deadline"));
    outcome.unwrap_or_else(|error| panic!("forwarded round trip on 127.0.0.1:{port} failed: {error}"))
}

/// 4 KiB including NUL bytes, so a text-only or truncated forward cannot pass.
fn payload() -> Vec<u8> {
    let bytes: Vec<u8> = (0..4096u32).map(|index| (index % 251) as u8).collect();
    assert_eq!(bytes.len(), 4096);
    assert!(bytes.contains(&0), "payload must carry a NUL byte");
    bytes
}

// ---------------------------------------------------------------------------
// Echo destination
// ---------------------------------------------------------------------------

/// Multi-accept, echo-on-every-connection TCP destination.
///
/// Every tunnel start consumes one connection here for the readiness probe, and a
/// healthy probe holds its channel until its 2 s deadline, so a single-accept
/// fixture would let the probe take the only accept. Byte counts are recorded per
/// connection, which lets a test assert on the connection that actually carried the
/// payload instead of "the first connection accepted".
struct EchoDestination {
    port: u16,
    observed: Arc<Mutex<Vec<usize>>>,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl EchoDestination {
    async fn start() -> Self {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind echo destination");
        let port = listener
            .local_addr()
            .expect("echo destination address")
            .port();
        let observed = Arc::new(Mutex::new(Vec::new()));
        let task = spawn_echo_accept(listener, observed.clone());
        Self {
            port,
            observed,
            task: Some(task),
        }
    }

    fn port(&self) -> u16 {
        self.port
    }

    /// Bytes each accepted connection has echoed so far.
    fn observed(&self) -> Vec<usize> {
        self.observed.lock().unwrap().clone()
    }

    /// Close the listening socket entirely: new forwarded connections are then
    /// refused by the destination kernel, which is the failure this fixture must
    /// produce. `accept`-then-close would be a successful connection followed by
    /// EOF, which does not fail the SSH channel.
    async fn close(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
            let _ = task.await;
        }
    }

    /// Rebind the same port. `SO_REUSEADDR` is required because the port carries no
    /// TIME_WAIT guarantee after the refused connections.
    async fn reopen(&mut self) -> io::Result<()> {
        assert!(self.task.is_none(), "reopen requires a closed destination");
        let socket = tokio::net::TcpSocket::new_v4()?;
        socket.set_reuseaddr(true)?;
        socket.bind((Ipv4Addr::LOCALHOST, self.port).into())?;
        let listener = socket.listen(128)?;
        self.task = Some(spawn_echo_accept(listener, self.observed.clone()));
        Ok(())
    }
}

impl Drop for EchoDestination {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

fn spawn_echo_accept(
    listener: TcpListener,
    observed: Arc<Mutex<Vec<usize>>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut clients = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                accepted = listener.accept() => {
                    let Ok((mut stream, _)) = accepted else { break };
                    let observed = observed.clone();
                    clients.spawn(async move {
                        let index = {
                            let mut counts = observed.lock().unwrap();
                            counts.push(0);
                            counts.len() - 1
                        };
                        let mut buffer = [0u8; 8192];
                        loop {
                            match stream.read(&mut buffer).await {
                                Ok(0) | Err(_) => break,
                                Ok(read) => {
                                    // Counted before the echo is written, so an observer
                                    // that received the echo has also seen the count.
                                    observed.lock().unwrap()[index] += read;
                                    if let Err(error) = stream.write_all(&buffer[..read]).await {
                                        eprintln!("echo destination write failed: {error}");
                                        break;
                                    }
                                }
                            }
                        }
                    });
                }
                Some(_) = clients.join_next(), if !clients.is_empty() => {}
            }
        }
    })
}

// ---------------------------------------------------------------------------
// Spawner seam
// ---------------------------------------------------------------------------

/// A spawned child that leaves the spawner's live-pid ledger when the registry's
/// reaper drops it, and notifies the ledger watch so a test can await the drain
/// instead of sampling it.
struct LedgerChild {
    inner: tokio::process::Child,
    pid: u32,
    live: Arc<Mutex<Vec<u32>>>,
    drained: watch::Sender<usize>,
}

impl TunnelChild for LedgerChild {
    fn id(&self) -> u32 {
        self.pid
    }

    fn wait(
        &mut self,
    ) -> Pin<Box<dyn std::future::Future<Output = io::Result<ExitStatus>> + Send + '_>> {
        Box::pin(self.inner.wait())
    }
}

impl Drop for LedgerChild {
    fn drop(&mut self) {
        let len = {
            let mut live = self.live.lock().unwrap();
            live.retain(|pid| *pid != self.pid);
            live.len()
        };
        self.drained.send_replace(len);
    }
}

/// Production-shaped spawner whose only difference is the program: the registry's
/// argv is executed by a launcher that adds the fixture's `known_hosts` and nothing
/// else, so the builder's `-F`, `-L` and every forced option reach real `ssh`
/// unchanged. The zero-byte trust-root property belongs to
/// `ssh::tunnel::argv`'s `empty_config_trust_root_is_enforced`, not to this harness.
struct FixtureSpawner {
    launcher: PathBuf,
    live: Arc<Mutex<Vec<u32>>>,
    history: Mutex<Vec<u32>>,
    drained: watch::Sender<usize>,
}

impl FixtureSpawner {
    fn new(launcher: PathBuf) -> Self {
        let (drained, _) = watch::channel(0);
        Self {
            launcher,
            live: Arc::new(Mutex::new(Vec::new())),
            history: Mutex::new(Vec::new()),
            drained,
        }
    }

    fn spawned(&self) -> Vec<u32> {
        self.history.lock().unwrap().clone()
    }

    fn live(&self) -> Vec<u32> {
        self.live.lock().unwrap().clone()
    }

    /// Await the reaper's own ledger drain, so "no child left behind" is observed
    /// rather than sampled.
    async fn await_drained(&self) {
        let mut rx = self.drained.subscribe();
        let drained = timeout(Duration::from_secs(30), async {
            while *rx.borrow() != 0 {
                if rx.changed().await.is_err() {
                    break;
                }
            }
        })
        .await;
        assert!(
            drained.is_ok(),
            "spawned tunnel children were never reaped, still live: {:?}",
            self.live()
        );
        assert_eq!(*self.drained.borrow(), 0, "still live: {:?}", self.live());
    }
}

impl TunnelSpawner for FixtureSpawner {
    fn spawn(&self, argv: &[String]) -> io::Result<SpawnedTunnel> {
        let mut command = tokio::process::Command::new(&self.launcher);
        command
            .args(&argv[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .process_group(0);
        let mut child = command.spawn()?;
        let pid = child.id().unwrap_or(0);
        let stderr = child.stderr.take().expect("piped stderr");
        let len = {
            let mut live = self.live.lock().unwrap();
            live.push(pid);
            live.len()
        };
        self.drained.send_replace(len);
        self.history.lock().unwrap().push(pid);
        Ok(SpawnedTunnel {
            child: Box::new(LedgerChild {
                inner: child,
                pid,
                live: self.live.clone(),
                drained: self.drained.clone(),
            }),
            stderr: Box::pin(stderr),
        })
    }

    fn kill_group(&self, pgid: i32, sig: Signal) {
        ProcessSpawner.kill_group(pgid, sig);
    }
}

impl Drop for FixtureSpawner {
    fn drop(&mut self) {
        let live = self.live.lock().unwrap().clone();
        for pid in live {
            kill_group(pid, libc::SIGKILL);
        }
    }
}

// ---------------------------------------------------------------------------
// Loopback sshd + destination fixture
// ---------------------------------------------------------------------------

/// A private OpenSSH server on loopback plus an echo destination and a tunnel
/// spawner pointed at it. Every sshd child is recorded at spawn and taken down by
/// `Drop`, on success and failure alike.
struct Fixture {
    dir: tempfile::TempDir,
    host: SshHost,
    sshd_port: u16,
    destination: EchoDestination,
    spawner: Arc<FixtureSpawner>,
    empty_config: PathBuf,
    config_a: PathBuf,
    config_a_text: String,
    sshd_pids: Arc<Mutex<Vec<u32>>>,
    server: tokio::task::JoinHandle<()>,
}

impl Fixture {
    async fn start() -> Self {
        use crate::ssh::tunnel_store::SystemClock;
        use std::os::fd::OwnedFd;
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::Builder::new()
            .prefix("fx-tunnel-forwarding")
            .tempdir()
            .expect("create forwarding fixture directory");
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))
            .expect("restrict forwarding fixture directory");
        for name in ["host_key", "user_key"] {
            let output = std::process::Command::new("ssh-keygen")
                .args(["-q", "-t", "ed25519", "-N", "", "-f"])
                .arg(dir.path().join(name))
                .output()
                .expect("run ssh-keygen for the forwarding fixture");
            assert!(
                output.status.success(),
                "ssh-keygen {name}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let sshd_config = dir.path().join("sshd_config");
        std::fs::write(
            &sshd_config,
            format!(
                "HostKey {}\nAuthorizedKeysFile {}\nStrictModes no\nUsePAM no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nLogLevel ERROR\n",
                dir.path().join("host_key").display(),
                dir.path().join("user_key.pub").display(),
            ),
        )
        .expect("write sshd fixture config");

        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind the loopback sshd fixture");
        let sshd_port = listener.local_addr().expect("fixture sshd address").port();
        let host_key =
            std::fs::read_to_string(dir.path().join("host_key.pub")).expect("read fixture host key");
        let known_hosts = dir.path().join("known_hosts");
        std::fs::write(&known_hosts, format!("[127.0.0.1]:{sshd_port} {host_key}"))
            .expect("write fixture known_hosts");

        // The spawner's program: it injects only fixture host-key trust.
        let launcher = dir.path().join("ssh-fixture");
        std::fs::write(
            &launcher,
            format!(
                "#!/bin/sh\nexec {} -o UserKnownHostsFile={} -o GlobalKnownHostsFile=/dev/null \"$@\"\n",
                ssh_program().display(),
                crate::ssh::direct::quote_posix(
                    known_hosts.to_str().expect("UTF-8 known_hosts path")
                ),
            ),
        )
        .expect("write fixture ssh launcher");
        std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o700))
            .expect("make fixture ssh launcher executable");

        // The trusted, zero-byte `-F` the registry validates. It lives in this 0700
        // fixture directory (mode 0600 file), the same shape as the daemon runtime
        // directory it replaces here.
        let empty_config = dir.path().join("ssh-tunnel-empty.conf");
        std::fs::write(&empty_config, b"").expect("write empty fixture config");
        std::fs::set_permissions(&empty_config, std::fs::Permissions::from_mode(0o600))
            .expect("restrict empty fixture config");

        let username = std::env::var("USER")
            .or_else(|_| std::env::var("LOGNAME"))
            .expect("fixture login user");
        let user_key = dir.path().join("user_key");
        // Todo 5-style fixture config `A`: the hop identity and trust a jump child
        // needs, so the dead-destination row can reach the destination dial at all.
        let config_a_text = format!(
            "Host bastion\n HostName 127.0.0.1\n User {username}\n Port {sshd_port}\n IdentityFile {}\n StrictHostKeyChecking no\n UserKnownHostsFile /dev/null\n",
            user_key.display()
        );
        let config_a = dir.path().join("config-a");
        std::fs::write(&config_a, &config_a_text).expect("write fixture config A");

        let sshd_pids = Arc::new(Mutex::new(Vec::new()));
        let log = dir.path().join("sshd.log");
        let accept_pids = sshd_pids.clone();
        let accept_config = sshd_config.clone();
        let server = tokio::spawn(async move {
            let mut children = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let Ok((stream, _)) = accepted else { break };
                        let stream = stream.into_std().expect("convert accepted sshd socket");
                        stream.set_nonblocking(false).expect("set accepted sshd socket blocking");
                        let log = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(&log)
                            .expect("open sshd fixture log");
                        let mut command = tokio::process::Command::new(sshd_binary());
                        command
                            .args(["-i", "-e", "-f"])
                            .arg(&accept_config)
                            .stdin(Stdio::from(OwnedFd::from(
                                stream.try_clone().expect("clone accepted sshd socket"),
                            )))
                            .stdout(Stdio::from(OwnedFd::from(stream)))
                            .stderr(log)
                            .process_group(0)
                            .kill_on_drop(true);
                        let mut child = command.spawn().expect("spawn loopback sshd");
                        let pid = child.id().expect("sshd fixture pid");
                        accept_pids.lock().unwrap().push(pid);
                        let reaped = accept_pids.clone();
                        children.spawn(async move {
                            let _ = child.wait().await;
                            reaped.lock().unwrap().retain(|recorded| *recorded != pid);
                        });
                    }
                    Some(_) = children.join_next(), if !children.is_empty() => {}
                }
            }
        });

        let destination = EchoDestination::start().await;
        let host = SshHost {
            id: "tunnel-forwarding-fixture".into(),
            label: "loopback sshd".into(),
            hostname: "127.0.0.1".into(),
            username: Some(username),
            port: Some(sshd_port),
            identity_file: Some(user_key.to_string_lossy().into_owned()),
            jump_host: None,
            source: SshHostSource::Manual,
            auth_method: SshAuthMethod::Key,
            disabled: None,
            shell: None,
        };
        let spawner = Arc::new(FixtureSpawner::new(launcher));
        Self {
            dir,
            host,
            sshd_port,
            destination,
            spawner,
            empty_config,
            config_a,
            config_a_text,
            sshd_pids,
            server,
        }
    }

    /// The empty `-F` lives in this fixture's 0700 directory instead of the operator's
    /// shared runtime directory. Everything else is the production registry.
    fn registry(&self) -> TunnelRegistry {
        TunnelRegistry::new(
            self.spawner.clone(),
            ConfigSource::Fixture {
                user: "",
                system: "",
            },
            Arc::new(ProbeSocketFactory::default()),
            Arc::new(SystemClock),
        )
        .with_runtime_dir(self.dir.path())
    }

    fn spec(&self, local_port: u16) -> TunnelSpec {
        TunnelSpec {
            id: TunnelSpecId("forwarding-fixture".into()),
            label: "forwarding fixture".into(),
            local_port: Some(local_port),
            remote_host: "127.0.0.1".into(),
            remote_port: self.destination.port(),
            enabled: true,
        }
    }

    /// Kill the transport of the established tunnel by taking down the `sshd -i`
    /// serving it. The local `-L` listener dies with it, so the registry's retry has
    /// to reconnect and restore it.
    fn kill_transport(&self) -> u32 {
        let pid = *self
            .sshd_pids
            .lock()
            .unwrap()
            .last()
            .expect("an sshd transport child must be serving the tunnel");
        kill_group(pid, libc::SIGKILL);
        pid
    }

    fn diagnostics(&self) -> String {
        let read = |name: &str| {
            std::fs::read_to_string(self.dir.path().join(name))
                .unwrap_or_else(|error| format!("<{name}: {error}>"))
        };
        format!("sshd={}; log:\n{}", self.sshd_port, read("sshd.log"))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
        let pids = self.sshd_pids.lock().unwrap().clone();
        for pid in pids {
            kill_group(pid, libc::SIGKILL);
        }
    }
}

// ---------------------------------------------------------------------------
// Daemon-ownership harness for the detach case
// ---------------------------------------------------------------------------

enum Command {
    Start {
        host: SshHost,
        spec: TunnelSpec,
        reply: oneshot::Sender<Result<TunnelView, IpcError>>,
    },
    Stop {
        id: TunnelRuntimeId,
        reply: oneshot::Sender<Result<(), IpcError>>,
    },
    List {
        reply: oneshot::Sender<Result<Vec<TunnelView>, IpcError>>,
    },
}

/// The registry lives inside a task that owns it, as the daemon does, and is reached
/// only through this handle. [`Daemon`] holds one sender of its own, so a client
/// dropping its socket cannot end the actor or tear a tunnel down; only dropping the
/// daemon closes the channel.
struct Daemon {
    tx: Option<mpsc::Sender<Command>>,
    done: oneshot::Receiver<()>,
}

impl Daemon {
    fn start(spawner: Arc<dyn TunnelSpawner>, runtime_dir: &Path) -> Self {
        let (tx, mut rx) = mpsc::channel::<Command>(8);
        let (done_tx, done) = oneshot::channel();
        let runtime_dir = runtime_dir.to_path_buf();
        tokio::spawn(async move {
            let registry = TunnelRegistry::new(
                spawner,
                ConfigSource::Fixture {
                    user: "",
                    system: "",
                },
                Arc::new(ProbeSocketFactory::default()),
                Arc::new(SystemClock),
            )
            .with_runtime_dir(&runtime_dir);
            while let Some(command) = rx.recv().await {
                match command {
                    Command::Start { host, spec, reply } => {
                        let _ = reply.send(registry.start(&host, spec, false).await);
                    }
                    Command::Stop { id, reply } => {
                        let _ = reply.send(registry.stop(&id).await);
                    }
                    Command::List { reply } => {
                        let _ = reply.send(registry.list(&HashMap::new()));
                    }
                }
            }
            let _ = registry.shutdown().await;
            let _ = done_tx.send(());
        });
        Self {
            tx: Some(tx),
            done,
        }
    }

    fn client(&self) -> TunnelClient {
        TunnelClient {
            tx: self.tx.clone().expect("daemon handle"),
        }
    }

    /// Drop the daemon's own sender and await its teardown. Every client must be
    /// dropped first, which is what makes this a detach rather than a shutdown.
    async fn finish(self) {
        let Self { tx, done } = self;
        drop(tx);
        timeout(Duration::from_secs(60), done)
            .await
            .expect("daemon teardown deadline")
            .expect("daemon teardown");
    }
}

/// What a client connection holds: a channel to the daemon, never the registry.
struct TunnelClient {
    tx: mpsc::Sender<Command>,
}

impl TunnelClient {
    async fn start(&self, host: SshHost, spec: TunnelSpec) -> TunnelView {
        let (reply, answer) = oneshot::channel();
        self.tx
            .send(Command::Start { host, spec, reply })
            .await
            .expect("daemon command channel");
        answer
            .await
            .expect("daemon start reply")
            .expect("tunnel start accepted")
    }

    async fn stop(&self, id: TunnelRuntimeId) {
        let (reply, answer) = oneshot::channel();
        self.tx
            .send(Command::Stop { id, reply })
            .await
            .expect("daemon command channel");
        answer.await.expect("daemon stop reply").expect("tunnel stop");
    }

    async fn list(&self) -> Vec<TunnelView> {
        let (reply, answer) = oneshot::channel();
        self.tx
            .send(Command::List { reply })
            .await
            .expect("daemon command channel");
        answer.await.expect("daemon list reply").expect("tunnel list")
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn forwards_exact_bytes_to_destination() {
    let fixture = Fixture::start().await;
    let registry = fixture.registry();
    let port = free_port();
    let started = registry
        .start(&fixture.host, fixture.spec(port), false)
        .await
        .expect("tunnel start");
    assert_eq!(
        started.state,
        TunnelState::Listening,
        "tunnel must reach listening; {}",
        fixture.diagnostics()
    );
    assert_eq!(started.local_port, port);
    let id = started.runtime_id.clone().expect("active runtime id");
    eprintln!("forwarding proof: local port {port}, runtime {}", id.0);

    let bytes = payload();
    let echoed = round_trip(port, &bytes).await;
    assert_eq!(
        echoed, bytes,
        "bytes must come back from the destination identical"
    );
    assert!(
        fixture.destination.observed().contains(&bytes.len()),
        "one destination connection must have carried exactly {} bytes, saw {:?}",
        bytes.len(),
        fixture.destination.observed()
    );
    assert_eq!(view_of(&registry, &id).state, TunnelState::Listening);

    registry.shutdown().await.expect("registry shutdown");
    fixture.spawner.await_drained().await;
}

#[tokio::test]
async fn occupied_port_is_typed_and_leaves_no_child() {
    let fixture = Fixture::start().await;
    let registry = fixture.registry();
    // The occupant is never accepted from, so `connect` succeeds and the port is
    // observably occupied rather than refused.
    let occupant = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind occupant");
    let port = occupant.local_addr().expect("occupant address").port();
    let started = registry
        .start(&fixture.host, fixture.spec(port), true)
        .await
        .expect("tunnel start request");
    assert_eq!(started.state, TunnelState::Failed);
    assert_eq!(started.failure, Some(TunnelFailureCode::PortInUse));
    assert_eq!(
        started.attempt, 0,
        "no attempt may be spawned for a taken port"
    );

    let id = started.runtime_id.clone().expect("failed runtime id");
    let ring = registry.transitions(&id);
    assert_eq!(ring.len(), 1, "one typed transition: {ring:?}");
    assert_eq!(ring[0].to, TunnelState::Failed);
    assert_eq!(ring[0].code, Some(TunnelFailureCode::PortInUse));

    assert!(
        fixture.spawner.spawned().is_empty(),
        "zero children may be spawned, saw {:?}",
        fixture.spawner.spawned()
    );
    fixture.spawner.await_drained().await;
    assert!(
        std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port)).is_err(),
        "the occupant must still own its port"
    );
    drop(occupant);
    registry.shutdown().await.expect("registry shutdown");
}

#[tokio::test]
async fn client_detach_keeps_runtime_and_traffic() {
    // The registry is owned by the daemon-side actor, never by a client. A client
    // that drops its socket must not take the tunnel with it; if production tore
    // tunnels down with the initiating connection, this test could not pass.
    let fixture = Fixture::start().await;
    let daemon = Daemon::start(fixture.spawner.clone(), fixture.dir.path());
    let port = free_port();

    let first = daemon.client();
    let started = first
        .start(fixture.host.clone(), fixture.spec(port))
        .await;
    assert_eq!(
        started.state,
        TunnelState::Listening,
        "tunnel must reach listening; {}",
        fixture.diagnostics()
    );
    let id = started.runtime_id.clone().expect("active runtime id");
    eprintln!("detach proof: local port {port}, runtime {}", id.0);
    drop(first);

    let second = daemon.client();
    let listed = second.list().await;
    let after = listed
        .iter()
        .find(|view| view.runtime_id.as_ref() == Some(&id))
        .unwrap_or_else(|| panic!("runtime {} must survive the detach: {listed:?}", id.0));
    assert_eq!(
        after.runtime_id,
        Some(id.clone()),
        "same runtime id, not a new one"
    );
    assert_eq!(
        after.state,
        TunnelState::Listening,
        "still listening, not stopped"
    );

    let echoed = round_trip(port, b"after-detach").await;
    assert_eq!(
        echoed,
        b"after-detach".to_vec(),
        "traffic must still flow after the detach"
    );
    second.stop(id).await;
    drop(second);
    daemon.finish().await;
    fixture.spawner.await_drained().await;
}

#[tokio::test]
async fn stop_releases_listener() {
    let fixture = Fixture::start().await;
    let registry = fixture.registry();

    // Cycle one: the client closes first, so the listener side carries no TIME_WAIT
    // and a plain bind must succeed once the child is reaped.
    let clean = free_port();
    let started = registry
        .start(&fixture.host, fixture.spec(clean), false)
        .await
        .expect("tunnel start");
    assert_eq!(
        started.state,
        TunnelState::Listening,
        "{}",
        fixture.diagnostics()
    );
    let id = started.runtime_id.clone().expect("active runtime id");
    let exited = registry.subscribe_exit(&id).expect("child exit watch");
    let echoed = round_trip(clean, b"stop-cycle").await;
    assert_eq!(echoed, b"stop-cycle".to_vec());
    registry.stop(&id).await.expect("tunnel stop");
    await_exit(exited).await;
    assert!(
        std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, clean)).is_ok(),
        "port {clean} must be bindable again after stop"
    );

    // Cycle two is the oracle that separates the `connect()` readiness probe from a
    // withdrawn bind probe: the traffic byte is consumed and the client stays open
    // across `stop`, leaving the killed child's socket in FIN_WAIT_2 on the listener
    // port. A bind-based availability check would refuse the restart; the connect
    // probe must not.
    let reused = free_port();
    let started = registry
        .start(&fixture.host, fixture.spec(reused), false)
        .await
        .expect("tunnel start");
    assert_eq!(
        started.state,
        TunnelState::Listening,
        "{}",
        fixture.diagnostics()
    );
    let id = started.runtime_id.clone().expect("active runtime id");
    let exited = registry.subscribe_exit(&id).expect("child exit watch");
    let mut held = TcpStream::connect((Ipv4Addr::LOCALHOST, reused))
        .await
        .expect("client connection to the tunnel");
    held.write_all(b"\0").await.expect("write the held byte");
    let mut echoed = [1u8; 1];
    held.read_exact(&mut echoed)
        .await
        .expect("await the echoed byte from the destination");
    assert_eq!(echoed, [0u8], "the destination must echo the NUL byte");
    registry.stop(&id).await.expect("tunnel stop");
    await_exit(exited).await;

    let restarted = registry
        .start(&fixture.host, fixture.spec(reused), false)
        .await
        .expect("tunnel restart on the same fixed port");
    assert_eq!(
        restarted.state,
        TunnelState::Listening,
        "the same fixed port must rebind immediately after stop; {}",
        fixture.diagnostics()
    );
    let bytes = payload();
    let echoed = round_trip(reused, &bytes).await;
    assert_eq!(echoed, bytes, "the restarted tunnel must forward");
    drop(held);
    registry.shutdown().await.expect("registry shutdown");
    fixture.spawner.await_drained().await;
}

#[tokio::test]
async fn destination_close_keeps_transport_listening() {
    let mut fixture = Fixture::start().await;
    let registry = fixture.registry();
    let port = free_port();
    let started = registry
        .start(&fixture.host, fixture.spec(port), false)
        .await
        .expect("tunnel start");
    assert_eq!(
        started.state,
        TunnelState::Listening,
        "{}",
        fixture.diagnostics()
    );
    let id = started.runtime_id.clone().expect("active runtime id");
    let echoed = round_trip(port, b"before-close").await;
    assert_eq!(echoed, b"before-close".to_vec());
    let ring_before = registry.transitions(&id).len();
    // Subscribe before triggering the failure; the transition is Listening ->
    // Listening, so only the watch notification can observe it.
    let mut states = registry.subscribe_state(&id).expect("state watch");

    fixture.destination.close().await;
    let outcome = timeout(Duration::from_secs(30), async {
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await?;
        stream.write_all(b"x").await?;
        stream.flush().await?;
        let mut byte = [0u8; 1];
        // EOF and reset are both a refused channel; a payload is not.
        Ok::<Option<usize>, io::Error>(stream.read(&mut byte).await.ok())
    })
    .await
    .expect("refused forwarded connection deadline")
    .expect("refused forwarded connection");
    assert!(
        matches!(outcome, Some(0) | None),
        "a refused destination channel must not deliver payload, got {outcome:?}"
    );

    timeout(Duration::from_secs(30), states.changed())
        .await
        .expect("channel failure must notify the state watch")
        .expect("tunnel state watch");
    let observed = view_of(&registry, &id);
    assert_eq!(
        observed.state,
        TunnelState::Listening,
        "a per-channel failure must not fail the tunnel"
    );
    assert_eq!(observed.failure, Some(TunnelFailureCode::ChannelFailed));
    let ring = registry.transitions(&id);
    assert_eq!(
        ring.len(),
        ring_before + 1,
        "exactly one transition entry must be recorded: {ring:?}"
    );
    let last = ring.last().expect("transition ring");
    assert_eq!(last.from, TunnelState::Listening);
    assert_eq!(last.to, TunnelState::Listening);
    assert_eq!(last.code, Some(TunnelFailureCode::ChannelFailed));

    // The transport's own `-L` listener is still accepting.
    TcpStream::connect((Ipv4Addr::LOCALHOST, port))
        .await
        .expect("the tunnel listener must still accept connections");

    fixture
        .destination
        .reopen()
        .await
        .expect("reopen destination");
    let echoed = round_trip(port, b"reopened").await;
    assert_eq!(
        echoed,
        b"reopened".to_vec(),
        "a forwarded byte must round-trip again once the destination is back"
    );
    registry.shutdown().await.expect("registry shutdown");
    fixture.spawner.await_drained().await;
}

/// Live bastion, dead destination sshd. This case must not run through the real
/// registry: the product spawns every hop child with the zero-byte `-F`, so a hop
/// with no `IdentityFile` and no known host key dies at `Host key verification
/// failed` (rule 2 -> `host_key_rejected`, a different code) before the dead
/// destination is ever reached. It also cannot be redirected with `HOME`, because
/// OpenSSH reads `known_hosts` from the password-entry home. So this row runs
/// against a Todo 5-style fixture argv carrying config `A` (hop `IdentityFile`,
/// `StrictHostKeyChecking no`, `UserKnownHostsFile /dev/null`) spawned through the
/// `TunnelSpawner` seam, asserting code, no listener and exit 255 there. The
/// zero-byte trust-root property stays in `ssh::tunnel::argv`'s
/// `empty_config_trust_root_is_enforced`.
#[tokio::test]
async fn dead_destination_behind_live_bastion_shows_no_listener() {
    let fixture = Fixture::start().await;
    let local = free_port();
    // Nothing listens here: the destination sshd is dead while the bastion is live.
    let dead = free_port();
    let host = SshHost {
        id: "dead-destination".into(),
        label: "dead destination".into(),
        hostname: "127.0.0.1".into(),
        username: fixture.host.username.clone(),
        port: Some(dead),
        identity_file: fixture.host.identity_file.clone(),
        jump_host: None,
        source: SshHostSource::Manual,
        auth_method: SshAuthMethod::Key,
        disabled: None,
        shell: None,
    };
    let spec = TunnelSpec {
        id: TunnelSpecId("dead-destination".into()),
        label: "dead destination".into(),
        local_port: Some(local),
        remote_host: "127.0.0.1".into(),
        remote_port: fixture.destination.port(),
        enabled: true,
    };
    // Built with the empty config the registry passes, then pointed at config `A`
    // and given the hop alias, exactly as Todo 5's jump row does: the hop child
    // inherits `-F`, so it carries the bastion identity and no host-key checking.
    let mut args = argv::tunnel_argv(
        &host,
        &spec,
        local,
        &fixture.empty_config,
        &ConfigSource::Fixture {
            user: "",
            system: "",
        },
    )
    .expect("fixture argv");
    let empty_index = args
        .iter()
        .position(|arg| arg == "-F")
        .expect("-F in fixture argv");
    args[empty_index + 1] = fixture.config_a.to_string_lossy().into_owned();
    args.splice(3..3, ["-J".to_string(), "bastion".to_string()]);
    assert_eq!(args[3], "-J");
    assert_eq!(args[4], "bastion");
    let config_index = args
        .iter()
        .position(|arg| arg == "-F")
        .expect("-F in fixture argv");
    assert_eq!(
        Path::new(&args[config_index + 1]),
        fixture.config_a.as_path(),
        "the hop child must read config A"
    );
    assert!(config_index < args.len() - 2, "-F must precede the target");
    assert!(fixture.config_a_text.contains("Host bastion\n"));

    let SpawnedTunnel {
        mut child,
        mut stderr,
    } = fixture.spawner.spawn(&args).expect("spawn fixture ssh");
    // The guard is armed only while the leader is unreaped: after `wait` the pid can
    // be recycled and a group kill would hit an unrelated process group.
    let mut guard = GroupKill {
        pid: child.id(),
        armed: true,
    };

    let mut diagnostics = String::new();
    let drained = timeout(Duration::from_secs(60), async {
        let mut buffer = [0u8; 4096];
        loop {
            match stderr.read(&mut buffer).await {
                Ok(0) => break,
                Ok(read) => diagnostics.push_str(&String::from_utf8_lossy(&buffer[..read])),
                Err(error) => {
                    eprintln!("fixture ssh stderr read failed: {error}");
                    break;
                }
            }
        }
    })
    .await;
    assert!(
        drained.is_ok(),
        "fixture ssh must terminate on its own; stderr:\n{diagnostics}"
    );
    let status = timeout(Duration::from_secs(60), child.wait())
        .await
        .expect("fixture ssh exit deadline")
        .expect("fixture ssh wait");
    guard.armed = false;
    drop(child);
    assert_eq!(
        status.code(),
        Some(255),
        "a dead destination must fail the transport; stderr:\n{diagnostics}"
    );
    assert_eq!(
        classify_ssh_stderr(&diagnostics, false),
        Some(TunnelFailureCode::HostUnreachable),
        "typed transport failure; stderr:\n{diagnostics}\n{}",
        fixture.diagnostics()
    );
    let refused = timeout(
        Duration::from_secs(10),
        TcpStream::connect((Ipv4Addr::LOCALHOST, local)),
    )
    .await
    .expect("connect attempt deadline");
    assert!(
        refused.is_err(),
        "a dead destination must not leave a listener on 127.0.0.1:{local}"
    );
    assert!(
        std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, local)).is_ok(),
        "port {local} must be free: no listener may survive a failed transport"
    );
    drop(guard);
    fixture.spawner.await_drained().await;
}

/// Kills one spawned fixture child's process group when it goes out of scope.
/// Disarm after the leader has been reaped.
struct GroupKill {
    pid: u32,
    armed: bool,
}

impl Drop for GroupKill {
    fn drop(&mut self) {
        if self.armed {
            kill_group(self.pid, libc::SIGKILL);
        }
    }
}

#[tokio::test]
async fn network_kill_reconnects_and_restores_listener() {
    let fixture = Fixture::start().await;
    let registry = fixture.registry();
    let port = free_port();
    let started = registry
        .start(&fixture.host, fixture.spec(port), false)
        .await
        .expect("tunnel start");
    assert_eq!(
        started.state,
        TunnelState::Listening,
        "{}",
        fixture.diagnostics()
    );
    let id = started.runtime_id.clone().expect("active runtime id");
    let echoed = round_trip(port, b"before-kill").await;
    assert_eq!(echoed, b"before-kill".to_vec());
    // Both watches are subscribed before the kill, so nothing is missed. The wait
    // below is driven by the state watch and requires the NEW transport (attempt 2)
    // to be listening, because the watch still reads the old `Listening` value.
    let states = registry.subscribe_state(&id).expect("state watch");
    let exited = registry.subscribe_exit(&id).expect("child exit watch");

    let victim = fixture.kill_transport();
    eprintln!(
        "network kill proof: local port {port}, runtime {}, killed sshd {victim}",
        id.0
    );
    await_exit(exited).await;
    await_view(states, &registry, &id, "the restored listener", |view| {
        view.state == TunnelState::Listening && view.attempt >= 2
    })
    .await;

    let restored = view_of(&registry, &id);
    assert_eq!(
        restored.runtime_id,
        Some(id.clone()),
        "the same runtime must reconnect"
    );
    assert_eq!(
        restored.state,
        TunnelState::Listening,
        "the listener must be restored; {}",
        fixture.diagnostics()
    );
    assert!(
        restored.attempt >= 2,
        "a NEW transport must serve the restored listener, attempt={}",
        restored.attempt
    );
    assert_eq!(
        restored.failure, None,
        "a restored transport clears the attempt failure"
    );
    let ring = registry.transitions(&id);
    assert!(
        ring.iter().any(|entry: &TunnelTransition| entry.from == TunnelState::Listening
            && entry.to == TunnelState::Reconnecting
            && entry.code == Some(TunnelFailureCode::HostUnreachable)),
        "the transport loss must be recorded as host_unreachable: {ring:?}"
    );
    let bytes = payload();
    let echoed = round_trip(port, &bytes).await;
    assert_eq!(
        echoed, bytes,
        "the restored listener must forward the exact payload again"
    );
    registry.shutdown().await.expect("registry shutdown");
    fixture.spawner.await_drained().await;
}
