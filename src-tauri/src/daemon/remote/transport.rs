use crate::daemon::protocol::{
    decode_daemon_stream_frame, encode_daemon_stream_frame, DaemonRequest, DaemonResponse,
    DaemonSessionDetails, DaemonStreamMessage, DAEMON_PROTOCOL_VERSION,
};
use crate::worktree::WorktreeIdentity;
use parking_lot::Mutex;
use std::collections::{HashMap, VecDeque};
use std::io;
use std::path::PathBuf;
#[cfg(test)]
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

const READY_SENTINEL: &str = "FERRYX_DAEMON_READY";
const RELAY_READY_SENTINEL: &str = "FERRYX-DAEMON RELAY v3 READY";
const READY_TIMEOUT: Duration = Duration::from_secs(10);
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(5);
const KEEPALIVE_TIMEOUT: Duration = Duration::from_secs(20);

static CLIENT_INSTANCE_ID: OnceLock<String> = OnceLock::new();
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteSessionState {
    Attached,
    Detached,
}

struct RegisteredSession {
    state: RemoteSessionState,
    frames: mpsc::Sender<DaemonStreamMessage<'static>>,
}

type DetachedCallback = Arc<dyn Fn(&str) + Send + Sync>;
type LadderCallback = Arc<dyn Fn() + Send + Sync>;

#[derive(Clone)]
pub struct SshRelayConfig {
    pub target: String,
    pub remote_bin: String,
    pub control_path: PathBuf,
}

pub struct SshRelayTransport {
    config: SshRelayConfig,
    ssh_program: PathBuf,
    sessions: Arc<Mutex<HashMap<String, RegisteredSession>>>,
    detached_callback: DetachedCallback,
    ladder_callback: LadderCallback,
    keepalive_interval: Duration,
    keepalive_timeout: Duration,
    outbound: Option<mpsc::Sender<String>>,
    pending: Arc<Mutex<VecDeque<oneshot::Sender<DaemonResponse>>>>,
    request_lock: tokio::sync::Mutex<()>,
    channel_task: Option<JoinHandle<()>>,
    generation: Option<u64>,
    epoch: Option<u64>,
}

pub struct StartSessionRequest {
    pub client_request_id: String,
    pub workspace_id: String,
    pub worktree: Option<WorktreeIdentity>,
    pub remote_path: Option<String>,
    pub cols: u16,
    pub rows: u16,
    pub shell: Option<String>,
}

impl SshRelayTransport {
    pub fn new(
        config: SshRelayConfig,
        detached_callback: impl Fn(&str) + Send + Sync + 'static,
        ladder_callback: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        Self::with_program_and_keepalive(
            config,
            PathBuf::from("ssh"),
            KEEPALIVE_INTERVAL,
            KEEPALIVE_TIMEOUT,
            Arc::new(detached_callback),
            Arc::new(ladder_callback),
        )
    }

    fn with_program_and_keepalive(
        config: SshRelayConfig,
        ssh_program: PathBuf,
        keepalive_interval: Duration,
        keepalive_timeout: Duration,
        detached_callback: DetachedCallback,
        ladder_callback: LadderCallback,
    ) -> Self {
        Self {
            config,
            ssh_program,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            detached_callback,
            ladder_callback,
            keepalive_interval,
            keepalive_timeout,
            outbound: None,
            pending: Arc::new(Mutex::new(VecDeque::new())),
            request_lock: tokio::sync::Mutex::new(()),
            channel_task: None,
            generation: None,
            epoch: None,
        }
    }

    pub fn register_session(
        &self,
        session_id: impl Into<String>,
    ) -> mpsc::Receiver<DaemonStreamMessage<'static>> {
        let session_id = session_id.into();
        let (frames, receiver) = mpsc::channel(128);
        self.sessions.lock().insert(
            session_id,
            RegisteredSession {
                state: RemoteSessionState::Attached,
                frames,
            },
        );
        receiver
    }

    pub fn unregister_session(&self, session_id: &str) {
        self.sessions.lock().remove(session_id);
    }

    pub fn session_state(&self, session_id: &str) -> Option<RemoteSessionState> {
        self.sessions
            .lock()
            .get(session_id)
            .map(|entry| entry.state)
    }

    pub fn generation(&self) -> Option<u64> {
        self.generation
    }

    pub async fn connect(&mut self) -> io::Result<()> {
        if self.channel_task.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "SSH relay channel is already connected",
            ));
        }

        let generation = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
        let mut child = self.spawn_channel()?;
        let stdout = child.stdout.take().ok_or_else(|| {
            io::Error::new(io::ErrorKind::BrokenPipe, "SSH relay stdout was not piped")
        })?;
        let mut stdout = BufReader::new(stdout);
        wait_for_ready(&mut stdout, READY_TIMEOUT)
            .await
            .map_err(|error| {
                let _ = child.start_kill();
                error
            })?;
        let mut stdin = child.stdin.take().ok_or_else(|| {
            io::Error::new(io::ErrorKind::BrokenPipe, "SSH relay stdin was not piped")
        })?;

        let mut handshake = serde_json::to_string(&DaemonRequest::Handshake {
            version: DAEMON_PROTOCOL_VERSION,
        })
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        handshake.push('\n');
        stdin.write_all(handshake.as_bytes()).await?;
        stdin.flush().await?;
        let mut response = String::new();
        tokio::time::timeout(READY_TIMEOUT, stdout.read_line(&mut response))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "SSH relay handshake timed out"))??;
        let epoch = match serde_json::from_str::<DaemonResponse>(response.trim())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?
        {
            DaemonResponse::HandshakeOk { version, epoch, .. }
                if version == DAEMON_PROTOCOL_VERSION => epoch,
            DaemonResponse::HandshakeOk { version, .. } => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("remote daemon protocol {version}, expected {DAEMON_PROTOCOL_VERSION}"),
                ));
            }
            DaemonResponse::ProtocolMismatch { expected_version, received_version } => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("remote daemon expected protocol {expected_version}, received {received_version}"),
                ));
            }
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unexpected remote daemon handshake response: {other:?}"),
                ));
            }
        };

        let (outbound, outbound_rx) = mpsc::channel(128);
        let runtime = ChannelRuntime {
            sessions: Arc::clone(&self.sessions),
            pending: Arc::clone(&self.pending),
            detached_callback: Arc::clone(&self.detached_callback),
            ladder_callback: Arc::clone(&self.ladder_callback),
            keepalive_interval: self.keepalive_interval,
            keepalive_timeout: self.keepalive_timeout,
        };
        self.channel_task = Some(tokio::spawn(async move {
            run_channel(child, stdout, stdin, outbound_rx, runtime).await;
        }));
        self.outbound = Some(outbound);
        self.generation = Some(generation);
        self.epoch = Some(epoch);
        let _ = CLIENT_INSTANCE_ID.get_or_init(|| uuid::Uuid::new_v4().to_string());
        Ok(())
    }

    pub async fn request(&self, request: DaemonRequest) -> io::Result<DaemonResponse> {
        let _guard = self.request_lock.lock().await;
        let mut frame = serde_json::to_string(&request)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        frame.push('\n');
        let (response_tx, response_rx) = oneshot::channel();
        self.pending.lock().push_back(response_tx);
        if let Err(error) = self
            .outbound
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "SSH relay is not connected"))?
            .send(frame)
            .await
        {
            self.pending.lock().pop_back();
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, error.to_string()));
        }
        tokio::time::timeout(READY_TIMEOUT, response_rx)
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "remote daemon request timed out"))?
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "SSH relay channel closed before response"))
    }

    pub async fn start_session(
        &self,
        request: StartSessionRequest,
    ) -> io::Result<DaemonSessionDetails> {
        let StartSessionRequest {
            client_request_id,
            workspace_id,
            worktree,
            remote_path,
            cols,
            rows,
            shell,
        } = request;
        match self
            .request(DaemonRequest::Spawn {
                client_request_id,
                workspace_id,
                worktree,
                cwd: remote_path,
                cols,
                rows,
                shell,
                // This request executes on the remote resident daemon. Retaining
                // the local SSH startup would recursively SSH from the host.
                startup: None,
            })
            .await?
        {
            DaemonResponse::SpawnOk { session, .. } => Ok(session),
            DaemonResponse::Error { message } => Err(io::Error::other(message)),
            other => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unexpected remote spawn response: {other:?}"),
            )),
        }
    }

    pub async fn attach_session(
        &self,
        session_id: &str,
        after_sequence: Option<u64>,
    ) -> io::Result<DaemonResponse> {
        self.request(DaemonRequest::Attach {
            session_id: session_id.to_string(),
            after_sequence,
        })
        .await
    }

    pub async fn send(&self, message: &DaemonStreamMessage<'_>) -> io::Result<()> {
        let frame = encode_daemon_stream_frame(message)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        self.outbound
            .as_ref()
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotConnected, "SSH relay is not connected")
            })?
            .send(frame)
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "SSH relay channel is closed"))
    }

    pub async fn shutdown(&mut self) {
        self.outbound.take();
        if let Some(task) = self.channel_task.take() {
            task.abort();
            let _ = task.await;
        }
    }

    fn spawn_channel(&self) -> io::Result<Child> {
        let mut command = Command::new(&self.ssh_program);
        #[cfg(unix)]
        command.args([
            "-o",
            "ControlMaster=auto",
            "-o",
            &format!("ControlPath={}", self.config.control_path.display()),
            "-o",
            "ControlPersist=600",
            "-o",
            "ServerAliveInterval=15",
            "-o",
            "ServerAliveCountMax=3",
        ]);
        command
            .arg(&self.config.target)
            .arg(&self.config.remote_bin)
            .arg("--relay-bridge")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
    }
}

async fn wait_for_ready<R>(reader: &mut R, duration: Duration) -> io::Result<()>
where
    R: tokio::io::AsyncBufRead + Unpin,
{
    tokio::time::timeout(duration, async {
        let mut line = String::new();
        loop {
            line.clear();
            if reader.read_line(&mut line).await? == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "SSH relay closed before readiness sentinel",
                ));
            }
            let line = line.trim_end();
            if line == READY_SENTINEL || line == RELAY_READY_SENTINEL {
                return Ok(());
            }
        }
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "SSH relay readiness timed out"))?
}

struct ChannelRuntime {
    sessions: Arc<Mutex<HashMap<String, RegisteredSession>>>,
    pending: Arc<Mutex<VecDeque<oneshot::Sender<DaemonResponse>>>>,
    detached_callback: DetachedCallback,
    ladder_callback: LadderCallback,
    keepalive_interval: Duration,
    keepalive_timeout: Duration,
}

async fn run_channel<R>(
    mut child: Child,
    mut stdout: R,
    mut stdin: tokio::process::ChildStdin,
    mut outbound: mpsc::Receiver<String>,
    runtime: ChannelRuntime,
) where
    R: tokio::io::AsyncBufRead + Unpin,
{
    let ChannelRuntime {
        sessions,
        pending,
        detached_callback,
        ladder_callback,
        keepalive_interval,
        keepalive_timeout,
    } = runtime;
    if let Some(mut stderr) = child.stderr.take() {
        tokio::spawn(async move {
            let mut sink = Vec::new();
            let _ = stderr.read_to_end(&mut sink).await;
        });
    }

    let mut tick = tokio::time::interval(keepalive_interval);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_received = Instant::now();
    let mut line = String::new();
    loop {
        tokio::select! {
            frame = outbound.recv() => {
                let Some(frame) = frame else { break; };
                if stdin.write_all(frame.as_bytes()).await.is_err() || stdin.flush().await.is_err() {
                    break;
                }
            }
            read = stdout.read_line(&mut line) => {
                match read {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        last_received = Instant::now();
                        let frame = std::mem::take(&mut line);
                        if route_frame(&sessions, &pending, &frame).await.is_err() {
                            break;
                        }
                    }
                }
            }
            _ = tick.tick() => {
                if last_received.elapsed() >= keepalive_timeout {
                    break;
                }
                if stdin.write_all(b"{\"type\":\"ping\"}\n").await.is_err()
                    || stdin.flush().await.is_err()
                {
                    break;
                }
            }
        }
    }

    let _ = child.start_kill();
    let _ = child.wait().await;
    pending.lock().clear();
    mark_all_detached(&sessions, &detached_callback);
    ladder_callback();
}

async fn route_frame(
    sessions: &Arc<Mutex<HashMap<String, RegisteredSession>>>,
    pending: &Arc<Mutex<VecDeque<oneshot::Sender<DaemonResponse>>>>,
    frame: &str,
) -> io::Result<()> {
    let value: serde_json::Value = serde_json::from_str(frame.trim())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if let Ok(response) = serde_json::from_value::<DaemonResponse>(value.clone()) {
        if let Some(sender) = pending.lock().pop_front() {
            let _ = sender.send(response);
        }
        return Ok(());
    }
    let Some(session_id) = value.get("sessionId").and_then(|value| value.as_str()) else {
        return Ok(());
    };
    let message = decode_daemon_stream_frame(frame)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let sender = sessions
        .lock()
        .get(session_id)
        .map(|entry| entry.frames.clone());
    if let Some(sender) = sender {
        let _ = sender.send(message).await;
    }
    Ok(())
}

fn mark_all_detached(
    sessions: &Arc<Mutex<HashMap<String, RegisteredSession>>>,
    detached_callback: &DetachedCallback,
) {
    let session_ids = {
        let mut sessions = sessions.lock();
        sessions
            .iter_mut()
            .filter_map(|(session_id, entry)| {
                if entry.state == RemoteSessionState::Detached {
                    None
                } else {
                    entry.state = RemoteSessionState::Detached;
                    Some(session_id.clone())
                }
            })
            .collect::<Vec<_>>()
    };
    for session_id in session_ids {
        detached_callback(&session_id);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::borrow::Cow;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::tempdir;
    use tokio::sync::oneshot;
    use tokio::time::timeout;

    const TEST_TIMEOUT: Duration = Duration::from_secs(3);

    fn script(contents: &str) -> (tempfile::TempDir, PathBuf) {
        let directory = tempdir().expect("temp script directory");
        let path = directory.path().join("fake-ssh");
        std::fs::write(&path, format!("#!/bin/sh\n{contents}\n")).expect("write script");
        let mut permissions = std::fs::metadata(&path)
            .expect("script metadata")
            .permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&path, permissions).expect("make script executable");
        (directory, path)
    }

    fn config(directory: &Path) -> SshRelayConfig {
        SshRelayConfig {
            target: "fake-target".into(),
            remote_bin: "/remote/ferryx".into(),
            control_path: directory.join("cm.sock"),
        }
    }

    fn transport(
        config: SshRelayConfig,
        program: PathBuf,
        interval: Duration,
        keepalive_timeout: Duration,
        detached_callback: DetachedCallback,
        ladder_callback: LadderCallback,
    ) -> SshRelayTransport {
        SshRelayTransport::with_program_and_keepalive(
            config,
            program,
            interval,
            keepalive_timeout,
            detached_callback,
            ladder_callback,
        )
    }

    #[tokio::test]
    async fn sentinel_handshake_and_session_frame_routing() {
        let (_directory, program) = script(
            r#"printf 'FERRYX_DAEMON_READY\n'
IFS= read -r handshake
printf '{"type":"handshakeOk","version":3,"pid":42,"epoch":7}\n'
printf '{"type":"output","sessionId":"s1","sequence":7,"data":"aGk="}\n'
while IFS= read -r line; do
  case "$line" in *'"type":"ping"'*) printf '{"type":"pong"}\n';; esac
done"#,
        );
        let (detached_tx, _detached_rx) = mpsc::unbounded_channel();
        let (ladder_tx, _ladder_rx) = mpsc::unbounded_channel();
        let mut transport = transport(
            config(_directory.path()),
            program,
            Duration::from_secs(1),
            Duration::from_secs(2),
            Arc::new(move |id| {
                let _ = detached_tx.send(id.to_string());
            }),
            Arc::new(move || {
                let _ = ladder_tx.send(());
            }),
        );
        let mut frames = transport.register_session("s1");
        transport.connect().await.expect("connect fake relay");
        assert!(transport.generation().is_some());

        let frame = timeout(TEST_TIMEOUT, frames.recv())
            .await
            .expect("routed frame timeout")
            .expect("routed frame");
        assert!(
            matches!(frame, DaemonStreamMessage::Output { session_id, sequence: 7, ref data, .. }
            if session_id == "s1" && data.as_ref() == b"hi")
        );

        transport
            .send(&DaemonStreamMessage::Output {
                session_id: Cow::Borrowed("s1"),
                sequence: 8,
                data: Cow::Borrowed(b"outbound"),
                metrics_read_unix_micros: None,
            })
            .await
            .expect("send outbound frame");
        transport.shutdown().await;
    }

    #[tokio::test]
    async fn request_response_correlation_and_start_session_use_daemon_protocol() {
        let (directory, program) = script(
            r#"printf 'FERRYX-DAEMON RELAY v3 READY\n'
IFS= read -r handshake
printf '{"type":"handshakeOk","version":3,"pid":42,"epoch":7}\n'
IFS= read -r ping
printf '{"type":"pong"}\n'
IFS= read -r spawn
case "$spawn" in *'"type":"spawn"'*'"cwd":"/srv/repo"'*'"startup"'*) exit 2;; esac
printf '{"type":"spawnOk","sessionId":"remote-s1","epoch":7,"session":{"sessionId":"remote-s1","workspaceId":"ws","worktree":null,"cwd":"/srv/repo","cols":100,"rows":30,"running":true,"startSequence":null,"endSequence":null}}\n'
while IFS= read -r line; do :; done"#,
        );
        let mut transport = transport(
            config(directory.path()),
            program,
            Duration::from_secs(1),
            Duration::from_secs(2),
            Arc::new(|_| {}),
            Arc::new(|| {}),
        );
        transport.connect().await.expect("connect fake relay");
        assert!(matches!(
            transport.request(DaemonRequest::Ping).await.expect("correlated pong"),
            DaemonResponse::Pong
        ));
        let session = transport
            .start_session(StartSessionRequest {
                client_request_id: "req-remote".into(),
                workspace_id: "ws".into(),
                worktree: None,
                remote_path: Some("/srv/repo".into()),
                cols: 100,
                rows: 30,
                shell: None,
            })
            .await
            .expect("start remote session");
        assert_eq!(session.session_id, "remote-s1");
        assert_eq!(session.cwd.as_deref(), Some("/srv/repo"));
        transport.shutdown().await;
    }

    #[tokio::test]
    async fn child_death_detaches_registered_sessions_and_triggers_ladder() {
        let (directory, program) = script(
            "printf 'FERRYX_DAEMON_READY\\n'\nIFS= read -r handshake\ncase \"$handshake\" in *'\"type\":\"handshake\"'*'\"version\":3'*) printf '{\"type\":\"handshakeOk\",\"version\":3,\"pid\":42,\"epoch\":7}\\n'; exit 0;; *) exit 2;; esac",
        );
        let (detached_tx, mut detached_rx) = mpsc::unbounded_channel();
        let (ladder_tx, ladder_rx) = oneshot::channel();
        let ladder_tx = Arc::new(Mutex::new(Some(ladder_tx)));
        let mut transport = transport(
            config(directory.path()),
            program,
            Duration::from_secs(1),
            Duration::from_secs(2),
            Arc::new(move |id| {
                let _ = detached_tx.send(id.to_string());
            }),
            Arc::new(move || {
                if let Some(tx) = ladder_tx.lock().take() {
                    let _ = tx.send(());
                }
            }),
        );
        let _frames = transport.register_session("surviving-pty");
        transport.connect().await.expect("connect fake relay");

        let detached = timeout(TEST_TIMEOUT, detached_rx.recv())
            .await
            .expect("detachment timeout")
            .expect("detachment callback");
        assert_eq!(detached, "surviving-pty");
        timeout(TEST_TIMEOUT, ladder_rx)
            .await
            .expect("ladder timeout")
            .expect("ladder callback");
        assert_eq!(
            transport.session_state("surviving-pty"),
            Some(RemoteSessionState::Detached)
        );
        transport.shutdown().await;
    }

    #[tokio::test]
    async fn keepalive_timeout_detaches_and_triggers_ladder() {
        let (directory, program) =
            script("printf 'FERRYX_DAEMON_READY\\n'\nIFS= read -r handshake\nprintf '{\"type\":\"handshakeOk\",\"version\":3,\"pid\":42,\"epoch\":7}\\n'\nwhile IFS= read -r line; do :; done");
        let (detached_tx, detached_rx) = oneshot::channel();
        let detached_tx = Arc::new(Mutex::new(Some(detached_tx)));
        let (ladder_tx, ladder_rx) = oneshot::channel();
        let ladder_tx = Arc::new(Mutex::new(Some(ladder_tx)));
        let mut transport = transport(
            config(directory.path()),
            program,
            Duration::from_millis(20),
            Duration::from_millis(80),
            Arc::new(move |_| {
                if let Some(tx) = detached_tx.lock().take() {
                    let _ = tx.send(());
                }
            }),
            Arc::new(move || {
                if let Some(tx) = ladder_tx.lock().take() {
                    let _ = tx.send(());
                }
            }),
        );
        let _frames = transport.register_session("s-timeout");
        transport.connect().await.expect("connect fake relay");

        timeout(TEST_TIMEOUT, detached_rx)
            .await
            .expect("keepalive detachment timeout")
            .expect("detachment callback");
        timeout(TEST_TIMEOUT, ladder_rx)
            .await
            .expect("keepalive ladder timeout")
            .expect("ladder callback");
        assert_eq!(
            transport.session_state("s-timeout"),
            Some(RemoteSessionState::Detached)
        );
        transport.shutdown().await;
    }
}
