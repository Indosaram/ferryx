#![cfg(unix)]

use ferryx_lib::daemon::protocol::{
    DaemonRequest, DaemonResponse, DaemonStreamMessage, DAEMON_PROTOCOL_VERSION,
};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::time::timeout;

struct TestDaemonClient {
    reader: BufReader<tokio::net::unix::OwnedReadHalf>,
    writer: tokio::net::unix::OwnedWriteHalf,
}

impl TestDaemonClient {
    async fn connect(
        socket_path: &Path,
        expected_pid: u32,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let stream = UnixStream::connect(socket_path).await?;
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);

        let hs = DaemonRequest::Handshake {
            version: DAEMON_PROTOCOL_VERSION,
        };
        let mut hs_json = serde_json::to_string(&hs)?;
        hs_json.push('\n');
        write_half.write_all(hs_json.as_bytes()).await?;
        write_half.flush().await?;

        let mut line = String::new();
        let bytes = timeout(Duration::from_secs(5), reader.read_line(&mut line)).await??;
        if bytes == 0 {
            return Err("EOF during handshake".into());
        }
        let resp: DaemonResponse = serde_json::from_str(line.trim())?;
        match resp {
            DaemonResponse::HandshakeOk { pid, version, .. }
                if pid == expected_pid && version == DAEMON_PROTOCOL_VERSION => {}
            other => return Err(format!("Expected HandshakeOk, got {other:?}").into()),
        };

        Ok(Self {
            reader,
            writer: write_half,
        })
    }

    async fn send_request(
        &mut self,
        req: &DaemonRequest,
    ) -> Result<DaemonResponse, Box<dyn std::error::Error + Send + Sync>> {
        let mut json = serde_json::to_string(req)?;
        json.push('\n');
        self.writer.write_all(json.as_bytes()).await?;
        self.writer.flush().await?;

        let mut line = String::new();
        let bytes = timeout(Duration::from_secs(5), self.reader.read_line(&mut line))
            .await
            .map_err(|_| "Request timed out waiting for response")??;
        if bytes == 0 {
            return Err("EOF from daemon".into());
        }
        let resp: DaemonResponse = serde_json::from_str(line.trim())?;
        Ok(resp)
    }

    async fn register_workspace(
        &mut self,
        workspace_id: &str,
        repo_root: &Path,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let resp = self
            .send_request(&DaemonRequest::RegisterWorkspace {
                workspace_id: workspace_id.to_string(),
                repo_root: repo_root.to_string_lossy().to_string(),
            })
            .await?;
        match resp {
            DaemonResponse::RegisterWorkspaceOk => Ok(()),
            other => Err(format!("RegisterWorkspace failed: {other:?}").into()),
        }
    }

    async fn spawn(
        &mut self,
        client_request_id: &str,
        workspace_id: &str,
        cols: u16,
        rows: u16,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let resp = self
            .send_request(&DaemonRequest::Spawn {
                client_request_id: client_request_id.to_string(),
                workspace_id: workspace_id.to_string(),
                worktree: None,
                cwd: None,
                cols,
                rows,
                shell: None,
                startup: None,
            })
            .await?;
        match resp {
            DaemonResponse::SpawnOk { session_id, .. } => Ok(session_id),
            other => Err(format!("Spawn failed: {other:?}").into()),
        }
    }

    async fn write_input(
        &mut self,
        session_id: &str,
        data: &[u8],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let resp = self
            .send_request(&DaemonRequest::Write {
                session_id: session_id.to_string(),
                data: data.to_vec(),
            })
            .await?;
        match resp {
            DaemonResponse::WriteOk => Ok(()),
            other => Err(format!("Write failed: {other:?}").into()),
        }
    }

    async fn list_sessions(
        &mut self,
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
        let resp = self.send_request(&DaemonRequest::ListSessions).await?;
        match resp {
            DaemonResponse::ListSessionsOk { sessions, .. } => Ok(sessions),
            other => Err(format!("ListSessions failed: {other:?}").into()),
        }
    }
}

struct TestAttachStream {
    reader: BufReader<tokio::net::unix::OwnedReadHalf>,
    pub attach_resp: DaemonResponse,
}

impl TestAttachStream {
    async fn attach(
        socket_path: &Path,
        expected_pid: u32,
        session_id: &str,
        after_sequence: Option<u64>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let stream = UnixStream::connect(socket_path).await?;
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);

        let hs = DaemonRequest::Handshake {
            version: DAEMON_PROTOCOL_VERSION,
        };
        let mut hs_json = serde_json::to_string(&hs)?;
        hs_json.push('\n');
        write_half.write_all(hs_json.as_bytes()).await?;
        write_half.flush().await?;

        let mut line = String::new();
        timeout(Duration::from_secs(5), reader.read_line(&mut line)).await??;
        let hs_resp: DaemonResponse = serde_json::from_str(line.trim())?;
        if !matches!(hs_resp, DaemonResponse::HandshakeOk { pid, version, .. } if pid == expected_pid && version == DAEMON_PROTOCOL_VERSION) {
            return Err(format!("Expected HandshakeOk, got {hs_resp:?}").into());
        }

        let req = DaemonRequest::Attach {
            session_id: session_id.to_string(),
            after_sequence,
        };
        let mut req_json = serde_json::to_string(&req)?;
        req_json.push('\n');
        write_half.write_all(req_json.as_bytes()).await?;
        write_half.flush().await?;

        line.clear();
        timeout(Duration::from_secs(5), reader.read_line(&mut line)).await??;
        let attach_resp: DaemonResponse = serde_json::from_str(line.trim())?;
        match &attach_resp {
            DaemonResponse::AttachOk { .. } => Ok(Self { reader, attach_resp }),
            other => Err(format!("Attach failed: {other:?}").into()),
        }
    }

    async fn await_pattern_in_history_or_stream(
        &mut self,
        pattern: &str,
        max_duration: Duration,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let mut accumulated = match &self.attach_resp {
            DaemonResponse::AttachOk { history, .. } => {
                String::from_utf8_lossy(history).to_string()
            }
            _ => String::new(),
        };

        if accumulated.contains(pattern) {
            return Ok(accumulated);
        }

        let start = std::time::Instant::now();
        while start.elapsed() < max_duration {
            let rem = max_duration.saturating_sub(start.elapsed());
            let mut line = String::new();
            let bytes = timeout(rem, self.reader.read_line(&mut line))
                .await
                .map_err(|_| "Timed out waiting for pattern in stream")??;
            if bytes == 0 {
                break;
            }
            if let Ok(DaemonStreamMessage::Output { data, .. }) =
                serde_json::from_str::<DaemonStreamMessage<'static>>(line.trim())
            {
                let text = String::from_utf8_lossy(&data);
                accumulated.push_str(&text);
                if accumulated.contains(pattern) {
                    return Ok(accumulated);
                }
            }
        }

        if accumulated.contains(pattern) {
            Ok(accumulated)
        } else {
            Err(format!(
                "Pattern '{pattern}' not found in stream output. Accumulated: '{accumulated}'"
            )
            .into())
        }
    }
}

struct PrivateDaemons {
    root: TempDir,
    children: Vec<Child>,
}

impl PrivateDaemons {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("fx-v05-")
            .tempdir_in("/tmp")
            .unwrap();
        for name in [
            "runtime", "home", "data", "session", "config", "cache", "tmp", "repo",
        ] {
            std::fs::create_dir(root.path().join(name)).unwrap();
        }
        Self {
            root,
            children: Vec::new(),
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ferryx"));
        command
            .env_clear()
            .arg("--daemon")
            .current_dir(self.root.path());
        for (key, name) in [
            ("HOME", "home"),
            ("FERRYX_RUNTIME_DIR", "runtime"),
            ("FERRYX_DATA_DIR", "data"),
            ("FERRYX_SESSION_DIR", "session"),
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_DATA_HOME", "data"),
            ("XDG_CACHE_HOME", "cache"),
            ("XDG_RUNTIME_DIR", "runtime"),
            ("TMPDIR", "tmp"),
            ("ZDOTDIR", "home"),
        ] {
            command.env(key, self.root.path().join(name));
        }
        command
            .env("FERRYX_HANDOVER_V5", "1")
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .env("SHELL", "/bin/sh")
            .env("LANG", "C")
            .env("TERM", "xterm-256color")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    fn socket(&self) -> PathBuf {
        self.root.path().join("runtime/daemon.sock")
    }

    fn repo(&self) -> PathBuf {
        self.root.path().join("repo")
    }

    fn pid(&self, idx: usize) -> u32 {
        self.children[idx].id()
    }

    async fn launch(&mut self, legacy: Option<&Path>) -> usize {
        use notify::Watcher;
        let (event_tx, mut events) = tokio::sync::mpsc::unbounded_channel();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                drop(event_tx.send(event));
            })
            .expect("runtime watcher");
        watcher
            .watch(
                self.socket().parent().unwrap(),
                notify::RecursiveMode::NonRecursive,
            )
            .expect("watch runtime");

        let mut cmd = self.command();
        if let Some(leg) = legacy {
            cmd.arg("--handover-from").arg(leg);
        }
        let mut child = cmd.spawn().expect("spawn daemon");
        let stdout = child.stdout.take().expect("stdout");
        let stderr = child.stderr.take().expect("stderr");

        let (tx, rx) = tokio::sync::oneshot::channel();
        std::thread::spawn(move || {
            use std::io::BufRead;
            let mut reader = std::io::BufReader::new(stdout);
            let mut line = String::new();
            let result = reader
                .read_line(&mut line)
                .map(|_| line.trim_end().to_owned());
            let _ = tx.send(result);
            let _ = std::io::copy(&mut reader, &mut std::io::sink());
        });
        std::thread::spawn(move || {
            let _ = std::io::copy(&mut std::io::BufReader::new(stderr), &mut std::io::stderr());
        });

        let index = self.children.len();
        self.children.push(child);

        if legacy.is_some() {
            timeout(Duration::from_secs(15), async {
                loop {
                    let event = events
                        .recv()
                        .await
                        .expect("watcher ended")
                        .expect("watch error");
                    if !event
                        .paths
                        .iter()
                        .any(|path| path.file_name() == Some(std::ffi::OsStr::new("daemon.sock")))
                    {
                        continue;
                    }
                    match TestDaemonClient::connect(&self.socket(), self.pid(index)).await {
                        Ok(_) => break,
                        Err(_) => {}
                    }
                }
            })
            .await
            .expect("replacement readiness timeout");
        } else {
            let ready_line = timeout(Duration::from_secs(15), rx)
                .await
                .expect("timeout waiting for FERRYX_DAEMON_READY")
                .expect("rx error")
                .expect("read error");
            assert_eq!(ready_line, "FERRYX_DAEMON_READY");
        }

        TestDaemonClient::connect(&self.socket(), self.pid(index))
            .await
            .expect("connect to exact owned PID");
        index
    }
}

impl Drop for PrivateDaemons {
    fn drop(&mut self) {
        for child in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[tokio::test]
async fn test_v5_zero_session_loss_and_immediate_predecessor_exit() {
    let mut daemons = PrivateDaemons::new();
    let socket_path = daemons.socket();
    let d1 = daemons.launch(None).await;
    let canonical_repo = daemons.repo();
    let pid_d1 = daemons.pid(d1);

    let mut client1 = TestDaemonClient::connect(&socket_path, pid_d1)
        .await
        .expect("Client connect to D1");
    let ws_id = "ws-handover-v5";
    client1
        .register_workspace(ws_id, &canonical_repo)
        .await
        .expect("register ws on D1");

    let s1 = client1
        .spawn("req-v5-s1", ws_id, 80, 24)
        .await
        .expect("spawn s1 on D1");

    client1
        .write_input(&s1, b"echo V5_STREAM_TEST_MARKER\n")
        .await
        .expect("write to s1");

    let mut attach1 = TestAttachStream::attach(&socket_path, pid_d1, &s1, None)
        .await
        .expect("attach to s1 on D1");

    attach1
        .await_pattern_in_history_or_stream("V5_STREAM_TEST_MARKER", Duration::from_secs(5))
        .await
        .expect("marker on D1");

    let resp = client1
        .send_request(&DaemonRequest::PrepareHandover)
        .await
        .expect("PrepareHandover on D1");

    let (legacy_socket_path, active_sessions) = match resp {
        DaemonResponse::PrepareHandoverOk {
            legacy_socket_path,
            active_sessions,
        } => (legacy_socket_path, active_sessions),
        other => panic!("Expected PrepareHandoverOk, got {other:?}"),
    };

    assert!(active_sessions.contains(&s1));

    let d2 = daemons.launch(Some(Path::new(&legacy_socket_path))).await;
    let pid_d2 = daemons.pid(d2);

    let mut client_d2 = TestDaemonClient::connect(&socket_path, pid_d2)
        .await
        .expect("Connect to canonical D2");

    let sessions_d2 = client_d2.list_sessions().await.expect("ListSessions on D2");
    assert!(
        sessions_d2.contains(&s1),
        "D2 list_sessions must contain D1's transferred session s1: {sessions_d2:?}"
    );

    let mut attach2 = TestAttachStream::attach(&socket_path, pid_d2, &s1, None)
        .await
        .expect("attach to transferred s1 on D2");

    attach2
        .await_pattern_in_history_or_stream("V5_STREAM_TEST_MARKER", Duration::from_secs(5))
        .await
        .expect("marker preserved on D2");

    let d1_child = &mut daemons.children[d1];
    let exited = timeout(Duration::from_secs(5), async {
        loop {
            match d1_child.try_wait() {
                Ok(Some(_status)) => return true,
                Ok(None) => tokio::time::sleep(Duration::from_millis(50)).await,
                Err(_) => return false,
            }
        }
    })
    .await
    .unwrap_or(false);

    assert!(exited, "Predecessor daemon D1 must exit immediately after v5 commit");

    let routes_file = daemons.root.path().join("runtime/handover_routes.json");
    assert!(!routes_file.exists(), "handover_routes.json must NOT exist on v5 path");
}
