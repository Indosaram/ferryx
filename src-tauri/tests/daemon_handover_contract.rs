#![cfg(unix)]
use ferryx_lib::daemon::protocol::{
    DaemonRequest, DaemonResponse, DaemonSessionDetails, DaemonStreamMessage,
    DAEMON_PROTOCOL_VERSION,
};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tempfile::{tempdir, TempDir};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::process::Command as TokioCommand;
use tokio::time::timeout;

struct TestDaemonClient {
    reader: BufReader<tokio::net::unix::OwnedReadHalf>,
    writer: tokio::net::unix::OwnedWriteHalf,
}

impl TestDaemonClient {
    async fn connect(socket_path: &Path) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
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
        let bytes = reader.read_line(&mut line).await?;
        if bytes == 0 {
            return Err("EOF during handshake".into());
        }
        let resp: DaemonResponse = serde_json::from_str(line.trim())?;
        match resp {
            DaemonResponse::HandshakeOk { .. } => {}
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
                shell: None,
                startup: None,
                cols,
                rows,
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

    async fn close(
        &mut self,
        session_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let resp = self
            .send_request(&DaemonRequest::Close {
                session_id: session_id.to_string(),
            })
            .await?;
        match resp {
            DaemonResponse::CloseOk => Ok(()),
            other => Err(format!("Close failed: {other:?}").into()),
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

    async fn describe_session(
        &mut self,
        session_id: &str,
    ) -> Result<DaemonSessionDetails, Box<dyn std::error::Error + Send + Sync>> {
        let resp = self
            .send_request(&DaemonRequest::DescribeSession {
                session_id: session_id.to_string(),
            })
            .await?;
        match resp {
            DaemonResponse::DescribeSessionOk { session } => Ok(session),
            other => Err(format!("DescribeSession failed: {other:?}").into()),
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
        reader.read_line(&mut line).await?;
        let hs_resp: DaemonResponse = serde_json::from_str(line.trim())?;
        assert!(matches!(hs_resp, DaemonResponse::HandshakeOk { .. }));

        let req = DaemonRequest::Attach {
            session_id: session_id.to_string(),
            after_sequence,
        };
        let mut req_json = serde_json::to_string(&req)?;
        req_json.push('\n');
        write_half.write_all(req_json.as_bytes()).await?;
        write_half.flush().await?;

        line.clear();
        reader.read_line(&mut line).await?;
        let attach_resp: DaemonResponse = serde_json::from_str(line.trim())?;

        Ok(Self {
            reader,
            attach_resp,
        })
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

fn create_test_git_repo() -> TempDir {
    let dir = tempdir().expect("Failed to create temporary git dir");
    let status = std::process::Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(dir.path())
        .output()
        .expect("git init failed");
    assert!(status.status.success(), "git init must succeed");

    let _ = std::fs::write(dir.path().join("README.md"), "# Test Repo\n");
    let _ = std::process::Command::new("git")
        .args(["config", "user.name", "Ferryx Test"])
        .current_dir(dir.path())
        .output();
    let _ = std::process::Command::new("git")
        .args(["config", "user.email", "test@ferryx.local"])
        .current_dir(dir.path())
        .output();
    let _ = std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(dir.path())
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "Initial commit"])
        .current_dir(dir.path())
        .output();

    dir
}

#[tokio::test]
async fn test_rolling_handover_two_daemon_lifecycle() {
    // Given: Old daemon (D1) running with an active shell session in an isolated runtime dir.
    let runtime_dir = tempdir().expect("Failed to create temporary runtime dir");
    let runtime_path = runtime_dir
        .path()
        .canonicalize()
        .unwrap_or_else(|_| runtime_dir.path().to_path_buf());
    let socket_path = runtime_path.join("daemon.sock");

    let bin_path = env!("CARGO_BIN_EXE_ferryx");

    let mut d1_child = TokioCommand::new(bin_path)
        .arg("--daemon")
        .env("FERRYX_RUNTIME_DIR", &runtime_path)
        .env("FERRYX_DATA_DIR", &runtime_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn D1 daemon");

    let d1_stdout = d1_child.stdout.take().expect("D1 stdout");
    let mut d1_reader = BufReader::new(d1_stdout).lines();
    let ready = timeout(Duration::from_secs(10), d1_reader.next_line())
        .await
        .expect("D1 readiness timeout")
        .expect("D1 read error")
        .expect("D1 EOF");
    assert_eq!(ready, "FERRYX_DAEMON_READY");

    let repo_dir = create_test_git_repo();
    let canonical_repo = repo_dir.path().canonicalize().unwrap();

    let mut client1 = TestDaemonClient::connect(&socket_path)
        .await
        .expect("Client connect to D1");
    let ws_id = "ws-handover-test";
    client1
        .register_workspace(ws_id, &canonical_repo)
        .await
        .expect("register ws on D1");

    let s1 = client1
        .spawn("req-handover-s1", ws_id, 80, 24)
        .await
        .expect("spawn s1 on D1");

    client1
        .write_input(&s1, b"echo HANDOVER_SESSION_1_READY\n")
        .await
        .expect("write to s1");

    let mut attach1 = TestAttachStream::attach(&socket_path, &s1, None)
        .await
        .expect("attach to s1");
    attach1
        .await_pattern_in_history_or_stream("HANDOVER_SESSION_1_READY", Duration::from_secs(5))
        .await
        .expect("await marker in s1");

    // When: D1 triggers handover or D2 starts with handover
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

    // Spawn D2 with handover-from in the same runtime dir
    let mut d2_child = TokioCommand::new(bin_path)
        .arg("--daemon")
        .arg("--handover-from")
        .arg(&legacy_socket_path)
        .env("FERRYX_RUNTIME_DIR", &runtime_path)
        .env("FERRYX_DATA_DIR", &runtime_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn D2 daemon");

    let d2_stdout = d2_child.stdout.take().expect("D2 stdout");
    let mut d2_reader = BufReader::new(d2_stdout).lines();
    let ready2 = timeout(Duration::from_secs(10), d2_reader.next_line())
        .await
        .expect("D2 readiness timeout")
        .expect("D2 read error")
        .expect("D2 EOF");
    assert_eq!(ready2, "FERRYX_DAEMON_READY");

    // Then: D2 is canonical on socket_path
    let mut client_d2 = TestDaemonClient::connect(&socket_path)
        .await
        .expect("Connect to canonical D2");

    // 1. ListSessions merges D1's active sessions
    let sessions_d2 = client_d2.list_sessions().await.expect("ListSessions on D2");
    assert!(
        sessions_d2.contains(&s1),
        "D2 list_sessions must contain D1's session s1: {sessions_d2:?}"
    );

    // 2. DescribeSession on s1 through D2 works transparently
    let desc_s1 = client_d2
        .describe_session(&s1)
        .await
        .expect("DescribeSession s1 via D2");
    assert_eq!(desc_s1.session_id, s1);
    assert!(desc_s1.running);

    // 3. Write and Attach to s1 through D2 proxies transparently to D1
    client_d2
        .write_input(&s1, b"echo PROXIED_WRITE_TO_OLD_SESSION_OK\n")
        .await
        .expect("write to s1 via D2");

    let mut attach_d2 = TestAttachStream::attach(&socket_path, &s1, None)
        .await
        .expect("attach to s1 via D2");
    let proxied_out = attach_d2
        .await_pattern_in_history_or_stream(
            "PROXIED_WRITE_TO_OLD_SESSION_OK",
            Duration::from_secs(5),
        )
        .await
        .expect("await proxied output on s1 via D2");
    assert!(proxied_out.contains("PROXIED_WRITE_TO_OLD_SESSION_OK"));

    // 4. Spawning a new session creates it locally in D2
    client_d2
        .register_workspace(ws_id, &canonical_repo)
        .await
        .expect("register ws on D2");
    let s2 = client_d2
        .spawn("req-handover-s2", ws_id, 80, 24)
        .await
        .expect("spawn s2 on D2");

    let sessions_d2_both = client_d2.list_sessions().await.expect("ListSessions on D2");
    assert!(sessions_d2_both.contains(&s1));
    assert!(sessions_d2_both.contains(&s2));

    // 5. Old daemon D1 refuses direct spawns on its legacy socket
    let mut legacy_client = TestDaemonClient::connect(Path::new(&legacy_socket_path))
        .await
        .expect("connect to D1 legacy socket");
    let spawn_refused = legacy_client.spawn("req-refused", ws_id, 80, 24).await;
    assert!(
        spawn_refused.is_err(),
        "D1 must refuse new spawns while draining"
    );

    // 6. Closing s1 closes old session; D1 exits automatically when its last session closes
    client_d2.close(&s1).await.expect("close s1 via D2");

    let d1_exit = timeout(Duration::from_secs(5), d1_child.wait())
        .await
        .expect("D1 must exit automatically after last session closed")
        .expect("D1 wait error");
    assert!(
        d1_exit.success() || d1_exit.code().unwrap_or(0) == 0,
        "D1 exited cleanly"
    );

    // 7. D2 remains healthy and serving s2
    let desc_s2 = client_d2
        .describe_session(&s2)
        .await
        .expect("DescribeSession s2 on D2");
    assert_eq!(desc_s2.session_id, s2);
    assert!(desc_s2.running);

    client_d2.close(&s2).await.expect("close s2");
    let _ = d2_child.kill().await;
    let _ = timeout(Duration::from_secs(3), d2_child.wait()).await;
}

#[tokio::test]
async fn test_rolling_handover_prepare_abort_rollback() {
    let runtime_dir = tempdir().expect("Failed to create temporary runtime dir");
    let runtime_path = runtime_dir
        .path()
        .canonicalize()
        .unwrap_or_else(|_| runtime_dir.path().to_path_buf());
    let socket_path = runtime_path.join("daemon.sock");

    let bin_path = env!("CARGO_BIN_EXE_ferryx");

    let mut d1_child = TokioCommand::new(bin_path)
        .arg("--daemon")
        .env("FERRYX_RUNTIME_DIR", &runtime_path)
        .env("FERRYX_DATA_DIR", &runtime_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn D1 daemon");

    let d1_stdout = d1_child.stdout.take().expect("D1 stdout");
    let mut d1_reader = BufReader::new(d1_stdout).lines();
    let ready = timeout(Duration::from_secs(10), d1_reader.next_line())
        .await
        .expect("D1 readiness timeout")
        .expect("D1 read error")
        .expect("D1 EOF");
    assert_eq!(ready, "FERRYX_DAEMON_READY");

    let mut client1 = TestDaemonClient::connect(&socket_path)
        .await
        .expect("Client connect to D1");

    // Request PrepareHandover
    let resp = client1
        .send_request(&DaemonRequest::PrepareHandover)
        .await
        .expect("PrepareHandover");
    let legacy_socket_path = match resp {
        DaemonResponse::PrepareHandoverOk {
            legacy_socket_path, ..
        } => legacy_socket_path,
        other => panic!("Expected PrepareHandoverOk, got {other:?}"),
    };

    assert!(Path::new(&legacy_socket_path).exists());

    // Abort handover
    let abort_resp = client1
        .send_request(&DaemonRequest::AbortHandover)
        .await
        .expect("AbortHandover");
    assert!(matches!(abort_resp, DaemonResponse::AbortHandoverOk));

    // Legacy socket should be cleaned up
    assert!(!Path::new(&legacy_socket_path).exists());

    // D1 remains canonical and accepts requests
    let ping_resp = client1
        .send_request(&DaemonRequest::Ping)
        .await
        .expect("Ping");
    assert!(matches!(ping_resp, DaemonResponse::Pong));

    let _ = d1_child.kill().await;
    let _ = timeout(Duration::from_secs(3), d1_child.wait()).await;
}
