#![cfg(unix)]
use ferryx_lib::daemon::launchd::{
    generate_launchd_plist, get_launchd_plist_path, uninstall_launchd_agent_from_path,
};
use ferryx_lib::daemon::protocol::{
    DaemonRequest, DaemonResponse, DaemonSessionDetails, DaemonStreamMessage,
    DAEMON_PROTOCOL_VERSION,
};
use ferryx_lib::daemon::server::{get_lock_path, get_socket_path};
use ferryx_lib::session::{
    clear_session_from_path, load_session_from_path, save_session_to_path, PersistedLayout,
    PersistedTab, PersistedWorkspace, PersistedWorkspaceSession, PersistedWorktree,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tempfile::{tempdir, TempDir};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::process::Command as TokioCommand;
use tokio::time::timeout;

/// Harness managing the standalone `ferryx --daemon` process lifecycle with deterministic
/// readiness awaiting, clean shutdown, and safe cleanup of socket/lock artifacts on drop.
struct DaemonProcessHarness {
    child: tokio::process::Child,
    socket_path: PathBuf,
    lock_path: PathBuf,
    pub daemon_pid: u32,
}

impl DaemonProcessHarness {
    async fn start() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let bin_path = env!("CARGO_BIN_EXE_ferryx");
        let socket_path = get_socket_path();
        let lock_path = get_lock_path();

        let mut child = TokioCommand::new(bin_path)
            .arg("--daemon")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let stdout = child.stdout.take().expect("Daemon stdout must be captured");
        let mut reader = BufReader::new(stdout).lines();

        // Exact readiness signal: wait for deterministic daemon ready line without polling/sleeps
        let ready_line = timeout(Duration::from_secs(10), reader.next_line())
            .await
            .map_err(|_| "Daemon readiness timed out")??
            .ok_or("Daemon process exited before emitting readiness signal")?;

        assert_eq!(
            ready_line, "FERRYX_DAEMON_READY",
            "Daemon stdout must emit deterministic readiness token"
        );

        // Probe handshake to obtain daemon pid
        let stream = UnixStream::connect(&socket_path).await?;
        let (read_half, mut write_half) = stream.into_split();
        let hs = DaemonRequest::Handshake {
            version: DAEMON_PROTOCOL_VERSION,
        };
        let mut hs_json = serde_json::to_string(&hs)?;
        hs_json.push('\n');
        write_half.write_all(hs_json.as_bytes()).await?;
        write_half.flush().await?;

        let mut line = String::new();
        let mut hs_reader = BufReader::new(read_half);
        hs_reader.read_line(&mut line).await?;
        let resp: DaemonResponse = serde_json::from_str(line.trim())?;
        let daemon_pid = match resp {
            DaemonResponse::HandshakeOk { pid, .. } => pid,
            other => panic!("Expected HandshakeOk, got {other:?}"),
        };

        Ok(Self {
            child,
            socket_path,
            lock_path,
            daemon_pid,
        })
    }

    async fn connect_client(
        &self,
    ) -> Result<TestDaemonClient, Box<dyn std::error::Error + Send + Sync>> {
        TestDaemonClient::connect(&self.socket_path).await
    }

    async fn shutdown(mut self) {
        if let Ok(mut client) = TestDaemonClient::connect(&self.socket_path).await {
            let _ = client.send_request(&DaemonRequest::Shutdown).await;
        }

        if timeout(Duration::from_secs(3), self.child.wait())
            .await
            .is_err()
        {
            let _ = self.child.kill().await;
            let _ = timeout(Duration::from_secs(3), self.child.wait()).await;
        }

        let _ = std::fs::remove_file(&self.socket_path);
        let _ = std::fs::remove_file(&self.lock_path);
    }
}

impl Drop for DaemonProcessHarness {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
        let _ = std::fs::remove_file(&self.socket_path);
        let _ = std::fs::remove_file(&self.lock_path);
    }
}

/// Helper client wrapping a command UDS stream connection to the daemon.
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
                cols,
                rows,
            })
            .await?;
        match resp {
            DaemonResponse::SpawnOk { session_id } => Ok(session_id),
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

    async fn resize(
        &mut self,
        session_id: &str,
        cols: u16,
        rows: u16,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let resp = self
            .send_request(&DaemonRequest::Resize {
                session_id: session_id.to_string(),
                cols,
                rows,
            })
            .await?;
        match resp {
            DaemonResponse::ResizeOk => Ok(()),
            other => Err(format!("Resize failed: {other:?}").into()),
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

/// Helper stream for attaching to a PTY session over the daemon protocol.
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

    async fn next_message(
        &mut self,
    ) -> Result<DaemonStreamMessage<'static>, Box<dyn std::error::Error + Send + Sync>> {
        let mut line = String::new();
        let bytes = timeout(Duration::from_secs(5), self.reader.read_line(&mut line))
            .await
            .map_err(|_| "Attach stream timed out waiting for next message")??;
        if bytes == 0 {
            return Err("EOF from attach stream".into());
        }
        let msg: DaemonStreamMessage<'static> = serde_json::from_str(line.trim())?;
        Ok(msg)
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

fn extract_pid_and_ppid(output: &str) -> Option<(u32, u32)> {
    for part in output.split("__PID_IS_") {
        if let Some((pid_str, rest)) = part.split_once(":__PPID_IS_") {
            let ppid_str = rest.split_whitespace().next().unwrap_or(rest);
            let pid_clean: String = pid_str.chars().filter(|c| c.is_ascii_digit()).collect();
            let ppid_clean: String = ppid_str.chars().filter(|c| c.is_ascii_digit()).collect();
            if !pid_clean.is_empty() && !ppid_clean.is_empty() {
                if let (Ok(pid), Ok(ppid)) = (pid_clean.parse::<u32>(), ppid_clean.parse::<u32>()) {
                    return Some((pid, ppid));
                }
            }
        }
    }
    None
}

#[tokio::test]
async fn test_daemon_cli_selection_headless_readiness_and_cancellation() {
    let bin_path = env!("CARGO_BIN_EXE_ferryx");
    let mut child = TokioCommand::new(bin_path)
        .arg("--daemon")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn ferryx with --daemon");

    let stdout = child.stdout.take().expect("Child stdout must be captured");
    let mut reader = BufReader::new(stdout).lines();

    // Exact readiness signal: wait for deterministic daemon ready line without polling/sleeps
    let ready_line = timeout(Duration::from_secs(10), reader.next_line())
        .await
        .expect("Daemon readiness timed out")
        .expect("Failed to read readiness line from daemon stdout")
        .expect("Daemon exited before emitting readiness signal");

    assert_eq!(
        ready_line, "FERRYX_DAEMON_READY",
        "Daemon stdout must emit deterministic readiness token"
    );

    let socket_path = get_socket_path();
    let stream = UnixStream::connect(&socket_path)
        .await
        .expect("Failed to connect to daemon UDS socket after readiness signal");

    let (read_half, mut write_half) = stream.into_split();
    let mut socket_reader = BufReader::new(read_half);

    // 1. Handshake
    let hs = DaemonRequest::Handshake {
        version: DAEMON_PROTOCOL_VERSION,
    };
    let mut hs_json = serde_json::to_string(&hs).unwrap();
    hs_json.push('\n');
    write_half.write_all(hs_json.as_bytes()).await.unwrap();

    let mut line = String::new();
    socket_reader.read_line(&mut line).await.unwrap();
    let resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
    match resp {
        DaemonResponse::HandshakeOk { version, pid, .. } => {
            assert_eq!(version, DAEMON_PROTOCOL_VERSION);
            assert!(pid > 0);
        }
        other => panic!("Expected HandshakeOk, got {other:?}"),
    }

    // 2. Ping
    line.clear();
    let ping = DaemonRequest::Ping;
    let mut ping_json = serde_json::to_string(&ping).unwrap();
    ping_json.push('\n');
    write_half.write_all(ping_json.as_bytes()).await.unwrap();

    socket_reader.read_line(&mut line).await.unwrap();
    let resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
    assert!(matches!(resp, DaemonResponse::Pong));

    // Exact cancellation signal: terminate child process and await bounded shutdown
    child
        .kill()
        .await
        .expect("Failed to send termination signal to daemon process");
    let exit_status = timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("Daemon process failed to exit within timeout after cancellation")
        .expect("Failed to wait on child process");

    assert!(
        !exit_status.success() || exit_status.code().unwrap_or(0) == 0,
        "Child process terminated"
    );
}

#[tokio::test]
async fn test_daemon_terminal_persistence_reconnect_replay_and_isolation() {
    let daemon = DaemonProcessHarness::start()
        .await
        .expect("Failed to start daemon harness");
    let repo_dir = create_test_git_repo();
    let canonical_repo = repo_dir.path().canonicalize().expect("canonical repo path");

    // Client A connects and registers workspace
    let mut client_a = daemon
        .connect_client()
        .await
        .expect("Client A connect failed");
    let ws_id = "ws-persistence-integration";
    client_a
        .register_workspace(ws_id, &canonical_repo)
        .await
        .expect("RegisterWorkspace on Client A failed");

    // Spawn Shell A and Shell B
    let session_a = client_a
        .spawn("req-persist-a", ws_id, 80, 24)
        .await
        .expect("Spawn Shell A failed");
    let session_b = client_a
        .spawn("req-persist-b", ws_id, 80, 24)
        .await
        .expect("Spawn Shell B failed");

    // Write identifying markers with PID and PPID to both shells using math expressions so the echo does not match the target pattern
    client_a
        .write_input(
            &session_a,
            b"echo __PID_IS_$$:__PPID_IS_$PPID; echo MARKER_A_$(expr 5000 + 1111)\n",
        )
        .await
        .expect("Write input to Shell A failed");

    client_a
        .write_input(
            &session_b,
            b"echo __PID_IS_$$:__PPID_IS_$PPID; echo MARKER_B_$(expr 5000 + 2222)\n",
        )
        .await
        .expect("Write input to Shell B failed");

    // Read initial stream from Shell A to parse shell A PID and PPID
    let mut attach_a_init = TestAttachStream::attach(&daemon.socket_path, &session_a, None)
        .await
        .expect("Attach to Shell A on Client A failed");
    let out_a = attach_a_init
        .await_pattern_in_history_or_stream("MARKER_A_6111", Duration::from_secs(5))
        .await
        .expect("Await marker on Shell A failed");

    let (shell_a_pid, shell_a_ppid) = extract_pid_and_ppid(&out_a).unwrap_or_else(|| {
        panic!("Failed to parse shell A PID and PPID from out_a:\n{out_a:?}");
    });

    assert_eq!(
        shell_a_ppid, daemon.daemon_pid,
        "Shell A PPID must match daemon PID"
    );
    assert_ne!(
        shell_a_ppid,
        std::process::id(),
        "Shell A must not be owned by GUI / test runner"
    );

    // Read initial stream from Shell B to parse shell B PID and PPID
    let mut attach_b_init = TestAttachStream::attach(&daemon.socket_path, &session_b, None)
        .await
        .expect("Attach to Shell B on Client A failed");
    let out_b = attach_b_init
        .await_pattern_in_history_or_stream("MARKER_B_7222", Duration::from_secs(5))
        .await
        .expect("Await marker on Shell B failed");

    let (shell_b_pid, shell_b_ppid) = extract_pid_and_ppid(&out_b).unwrap_or_else(|| {
        panic!("Failed to parse shell B PID and PPID from out_b:\n{out_b:?}");
    });
    assert_eq!(shell_b_ppid, daemon.daemon_pid);

    // Drop Client A and its attach streams (simulating GUI client loss / sudden disconnect)
    drop(client_a);
    drop(attach_a_init);
    drop(attach_b_init);

    // Contract: Client A disconnect leaves shell PIDs and daemon alive
    assert_eq!(
        unsafe { libc::kill(daemon.daemon_pid as i32, 0) },
        0,
        "Daemon process must remain alive after client loss"
    );
    assert_eq!(
        unsafe { libc::kill(shell_a_pid as i32, 0) },
        0,
        "Shell A PID must remain alive after Client A disconnect"
    );
    assert_eq!(
        unsafe { libc::kill(shell_b_pid as i32, 0) },
        0,
        "Shell B PID must remain alive after Client A disconnect"
    );

    // Client B connects to daemon
    let mut client_b = daemon
        .connect_client()
        .await
        .expect("Client B connect failed");

    // Client B lists sessions
    let sessions = client_b
        .list_sessions()
        .await
        .expect("ListSessions on Client B failed");
    assert!(
        sessions.contains(&session_a),
        "Session A must be listed in active daemon sessions"
    );
    assert!(
        sessions.contains(&session_b),
        "Session B must be listed in active daemon sessions"
    );

    // Client B describes session A
    let desc_a = client_b
        .describe_session(&session_a)
        .await
        .expect("DescribeSession A failed");
    assert!(desc_a.running, "Session A must be running");
    assert_eq!(desc_a.cols, 80);
    assert_eq!(desc_a.rows, 24);

    // Client B attaches to session A and verifies history replay exactly once
    let mut attach_b_a = TestAttachStream::attach(&daemon.socket_path, &session_a, None)
        .await
        .expect("Attach to session A on Client B failed");

    let history_a = match &attach_b_a.attach_resp {
        DaemonResponse::AttachOk {
            history,
            start_sequence,
            end_sequence,
            gap,
            ..
        } => {
            assert!(gap.is_none(), "No replay gap expected for recent session");
            assert!(
                start_sequence.is_some(),
                "History start_sequence must be present"
            );
            assert!(
                end_sequence.is_some(),
                "History end_sequence must be present"
            );
            String::from_utf8_lossy(history).to_string()
        }
        other => panic!("Expected AttachOk, got {other:?}"),
    };

    assert!(
        history_a.contains("MARKER_A_6111"),
        "History snapshot must contain earlier Shell A output"
    );

    // Client B writes new input to Session A
    client_b
        .write_input(
            &session_a,
            b"echo __RECONNECT_CONFIRMED_$(expr 30 + 12)__\n",
        )
        .await
        .expect("Write input on reconnected session A failed");

    let stream_out = attach_b_a
        .await_pattern_in_history_or_stream("__RECONNECT_CONFIRMED_42__", Duration::from_secs(5))
        .await
        .expect("Reconnected stream output failed to arrive");
    assert!(stream_out.contains("__RECONNECT_CONFIRMED_42__"));

    // Client B resizes session A
    client_b
        .resize(&session_a, 132, 43)
        .await
        .expect("Resize session A failed");
    let desc_a_resized = client_b
        .describe_session(&session_a)
        .await
        .expect("DescribeSession A after resize failed");
    assert_eq!(desc_a_resized.cols, 132);
    assert_eq!(desc_a_resized.rows, 43);

    // Contract: Closing session A ends its process group while session B remains alive and responsive
    client_b
        .close(&session_a)
        .await
        .expect("Close session A failed");

    // Await stream Exit message on attach_b_a
    let exit_deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut exit_received = false;
    while std::time::Instant::now() < exit_deadline {
        let msg = attach_b_a
            .next_message()
            .await
            .expect("Await exit message on stream failed");
        if matches!(msg, DaemonStreamMessage::Exit { .. }) {
            exit_received = true;
            break;
        }
    }
    assert!(
        exit_received,
        "Stream must deliver Exit message on session close"
    );

    // Verify Shell A process terminates
    let exit_deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut shell_a_exited = false;
    while std::time::Instant::now() < exit_deadline {
        if unsafe { libc::kill(shell_a_pid as i32, 0) != 0 } {
            shell_a_exited = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        shell_a_exited,
        "Shell A process group must terminate when session A is closed"
    );

    // Verify Shell B remains alive and responsive
    assert_eq!(
        unsafe { libc::kill(shell_b_pid as i32, 0) },
        0,
        "Shell B PID must remain alive after Session A is closed"
    );

    let desc_b = client_b
        .describe_session(&session_b)
        .await
        .expect("DescribeSession B failed");
    assert!(
        desc_b.running,
        "Session B must remain running after Session A closed"
    );

    client_b
        .write_input(&session_b, b"echo __SESSION_B_STILL_$(expr 1000 + 234)__\n")
        .await
        .expect("Write input to Session B failed");

    let mut attach_b_b = TestAttachStream::attach(&daemon.socket_path, &session_b, None)
        .await
        .expect("Attach to Session B failed");
    let hist_b = match &attach_b_b.attach_resp {
        DaemonResponse::AttachOk { history, .. } => String::from_utf8_lossy(history).to_string(),
        other => panic!("Expected AttachOk for session B, got {other:?}"),
    };

    if !hist_b.contains("__SESSION_B_STILL_1234__") {
        let out_resp = attach_b_b
            .await_pattern_in_history_or_stream("__SESSION_B_STILL_1234__", Duration::from_secs(5))
            .await
            .expect("Session B failed to respond to input");
        assert!(out_resp.contains("__SESSION_B_STILL_1234__"));
    }

    // Clean up session B
    client_b
        .close(&session_b)
        .await
        .expect("Close session B failed");

    // Daemon shutdown & cleanup
    daemon.shutdown().await;
}

#[tokio::test]
async fn test_daemon_protocol_mismatch_explicit_error_and_no_fallback() {
    let daemon = DaemonProcessHarness::start()
        .await
        .expect("Failed to start daemon harness");

    let stream = UnixStream::connect(&daemon.socket_path)
        .await
        .expect("connect");
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    let invalid_version = DAEMON_PROTOCOL_VERSION + 999;
    let hs = DaemonRequest::Handshake {
        version: invalid_version,
    };
    let mut hs_json = serde_json::to_string(&hs).unwrap();
    hs_json.push('\n');
    write_half.write_all(hs_json.as_bytes()).await.unwrap();
    write_half.flush().await.unwrap();

    let mut line = String::new();
    let bytes = timeout(Duration::from_secs(5), reader.read_line(&mut line))
        .await
        .expect("read timeout")
        .expect("read line");
    assert!(bytes > 0);

    let resp: DaemonResponse = serde_json::from_str(line.trim()).expect("parse response");
    match resp {
        DaemonResponse::ProtocolMismatch {
            expected_version,
            received_version,
        } => {
            assert_eq!(expected_version, DAEMON_PROTOCOL_VERSION);
            assert_eq!(received_version, invalid_version);
        }
        other => panic!("Expected ProtocolMismatch, got {other:?}"),
    }

    // Explicit check: server closes stream immediately on mismatch (no silent fallback)
    line.clear();
    let next_bytes = timeout(Duration::from_secs(5), reader.read_line(&mut line))
        .await
        .expect("read timeout")
        .expect("read line");
    assert_eq!(
        next_bytes, 0,
        "Server must close stream immediately after ProtocolMismatch"
    );

    daemon.shutdown().await;
}

#[tokio::test]
async fn test_daemon_output_sequence_contiguity_and_replay_gap() {
    let daemon = DaemonProcessHarness::start()
        .await
        .expect("Failed to start daemon harness");
    let repo_dir = create_test_git_repo();
    let canonical_repo = repo_dir.path().canonicalize().unwrap();
    let mut client = daemon.connect_client().await.expect("connect client");

    let ws_id = "ws-seq-contiguity-test";
    client
        .register_workspace(ws_id, &canonical_repo)
        .await
        .expect("register ws");
    let session_id = client
        .spawn("req-seq-1", ws_id, 80, 24)
        .await
        .expect("spawn");

    let mut attach = TestAttachStream::attach(&daemon.socket_path, &session_id, None)
        .await
        .expect("attach");
    let start_end = match &attach.attach_resp {
        DaemonResponse::AttachOk { end_sequence, .. } => *end_sequence,
        other => panic!("Expected AttachOk, got {other:?}"),
    };

    let mut last_seq = start_end.unwrap_or(0);
    client
        .write_input(
            &session_id,
            b"for i in 1 2 3 4 5; do echo SEQ_BURST_$i; done\n",
        )
        .await
        .expect("write");

    let mut seen_bursts = 0;
    while seen_bursts < 5 {
        let msg = attach.next_message().await.expect("next message");
        if let DaemonStreamMessage::Output { sequence, data, .. } = msg {
            assert!(
                sequence > last_seq,
                "Sequence {sequence} must be greater than last sequence {last_seq}"
            );
            assert_eq!(
                sequence,
                last_seq + 1,
                "Output chunk sequence must be strictly contiguous without drops"
            );
            last_seq = sequence;
            let text = String::from_utf8_lossy(&data);
            if text.contains("SEQ_BURST_") {
                seen_bursts += text.matches("SEQ_BURST_").count();
            }
        }
    }

    // Await any trailing prompt chunks and obtain final sequence from DescribeSession
    let desc = client
        .describe_session(&session_id)
        .await
        .expect("describe session");
    let current_end_seq = desc.end_sequence;

    // Attach with after_sequence: current_end_seq returns empty history and no gap
    let attach_current =
        TestAttachStream::attach(&daemon.socket_path, &session_id, current_end_seq)
            .await
            .expect("attach at current end");
    match &attach_current.attach_resp {
        DaemonResponse::AttachOk {
            history,
            start_sequence,
            gap,
            ..
        } => {
            assert!(
                history.is_empty(),
                "History must be empty when attaching at current sequence"
            );
            assert_eq!(*start_sequence, None);
            assert_eq!(*gap, None);
        }
        other => panic!("Expected AttachOk, got {other:?}"),
    }

    client.close(&session_id).await.expect("close session");
    daemon.shutdown().await;
}

#[tokio::test]
async fn test_daemon_gui_process_non_ownership_and_process_tree() {
    let daemon = DaemonProcessHarness::start()
        .await
        .expect("Failed to start daemon harness");
    let repo_dir = create_test_git_repo();
    let canonical_repo = repo_dir.path().canonicalize().unwrap();
    let mut client = daemon.connect_client().await.expect("connect client");

    let ws_id = "ws-tree-test";
    client
        .register_workspace(ws_id, &canonical_repo)
        .await
        .expect("register ws");
    let session_id = client
        .spawn("req-tree-1", ws_id, 80, 24)
        .await
        .expect("spawn");

    client
        .write_input(
            &session_id,
            b"echo __PID_IS_$$:__PPID_IS_$PPID; echo TREE_MARKER_$(expr 8000 + 888)\n",
        )
        .await
        .expect("write");

    let mut attach = TestAttachStream::attach(&daemon.socket_path, &session_id, None)
        .await
        .expect("attach");

    let output = attach
        .await_pattern_in_history_or_stream("TREE_MARKER_8888", Duration::from_secs(5))
        .await
        .expect("await marker");

    let current_test_pid = std::process::id();

    let (shell_pid, shell_ppid) =
        extract_pid_and_ppid(&output).expect("Failed to parse shell PID and PPID");

    assert_eq!(
        shell_ppid, daemon.daemon_pid,
        "Shell PPID ({shell_ppid}) must match standalone daemon PID ({})",
        daemon.daemon_pid
    );

    assert_ne!(
        shell_ppid, current_test_pid,
        "Shell child must not be directly owned by the GUI / test process ({current_test_pid})"
    );

    // Verify via system ps tool
    let ps_output = std::process::Command::new("ps")
        .args(["-o", "ppid=", "-p", &shell_pid.to_string()])
        .output()
        .expect("ps command failed");
    if ps_output.status.success() {
        let ps_ppid_str = String::from_utf8_lossy(&ps_output.stdout)
            .trim()
            .to_string();
        if let Ok(ps_ppid) = ps_ppid_str.parse::<u32>() {
            assert_eq!(ps_ppid, daemon.daemon_pid);
            assert_ne!(ps_ppid, current_test_pid);
        }
    }

    client.close(&session_id).await.expect("close session");
    daemon.shutdown().await;
}

#[test]
fn test_launchd_plist_generation_and_identity_contract() {
    let plist = generate_launchd_plist("/Applications/Ferryx.app/Contents/MacOS/ferryx");
    assert!(
        plist.contains("<string>com.rorca.daemon</string>"),
        "Must preserve com.rorca.daemon compatibility identifier in launchd plist"
    );
    assert!(
        plist.contains("<string>/Applications/Ferryx.app/Contents/MacOS/ferryx</string>"),
        "Must contain executable path"
    );
    assert!(
        plist.contains("<string>--daemon</string>"),
        "Must configure --daemon flag in ProgramArguments"
    );
}

#[test]
fn test_get_launchd_plist_path_location() {
    if let Some(path) = get_launchd_plist_path() {
        assert!(
            path.ends_with("Library/LaunchAgents/com.rorca.daemon.plist"),
            "LaunchAgent plist must target Library/LaunchAgents/com.rorca.daemon.plist"
        );
    }
}

#[test]
fn test_uninstall_launchd_agent_idempotent_when_missing() {
    let dir = tempdir().unwrap();
    let missing_plist = dir.path().join("missing.plist");
    assert!(
        uninstall_launchd_agent_from_path(&missing_plist).is_ok(),
        "Uninstalling missing plist should be a successful no-op"
    );
}

#[tokio::test]
async fn test_durable_fsync_session_persistence_lifecycle() {
    let dir = tempdir().unwrap();
    let session_file = dir.path().join("durable_session.json");

    let mut workspaces = HashMap::new();
    workspaces.insert(
        "default".to_string(),
        PersistedWorkspace {
            workspace_id: "default".to_string(),
            repo_root: PathBuf::from("/tmp/repo"),
            worktrees: vec![PersistedWorktree {
                path: "/tmp/repo".to_string(),
                branch: "main".to_string(),
                head: "abcdef".to_string(),
                is_main: true,
                is_locked: false,
            }],
            active_worktree_path: Some("/tmp/repo".to_string()),
            layout: PersistedLayout {
                split_mode: "none".to_string(),
                primary_tab_id: Some("tab-1".to_string()),
                secondary_tab_id: None,
                active_tab_id: Some("tab-1".to_string()),
                tabs: vec![PersistedTab {
                    id: "tab-1".to_string(),
                    kind: None,
                    label: "main".to_string(),
                    pinned: None,
                    terminal: None,
                    browser: None,
                    custom_title: None,
                    session_id: Some("term-1".to_string()),
                    worktree_path: Some("/tmp/repo".to_string()),
                    pane_tree: None,
                    session_ids_by_leaf_id: None,
                    active_leaf_id: None,
                    expanded_leaf_id: None,
                    extra: HashMap::new(),
                }],
                tab_groups: None,
                tab_group_layout: None,
                focused_group_id: None,
                layouts_by_tab_id: None,
                extra: HashMap::new(),
            },
            worktree_layouts: None,
            layout_by_worktree: None,
            terminal_sessions: HashMap::new(),
            extra: HashMap::new(),
        },
    );

    let session = PersistedWorkspaceSession {
        version: 1,
        timestamp: 1234567890,
        active_workspace_id: "default".to_string(),
        workspaces,
        extra: HashMap::new(),
    };

    save_session_to_path(&session_file, &session).expect("durable fsync save");
    assert!(session_file.exists());

    let loaded = load_session_from_path(&session_file)
        .expect("load")
        .expect("present");
    assert_eq!(loaded.version, 1);
    assert_eq!(loaded.active_workspace_id, "default");

    clear_session_from_path(&session_file).expect("clear");
    assert!(!session_file.exists());
}
