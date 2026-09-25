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

/// Cold-start readiness budget for a privately spawned daemon. The previous 15s is not enough when
/// this machine is loaded (a concurrent Rust/Chromium build pushes a first daemon start past it), so
/// the WAIT expired before any assertion ran. Only this unrelated cold-start budget changes.
const DAEMON_READY_BUDGET: Duration = Duration::from_secs(90);

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
            token: None,
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

    /// Explicit shell/cwd keep this contract deterministic: a non-interactive shell has no rc
    /// startup work, so the first write is executed instead of racing shell initialization.
    async fn spawn_with_shell(
        &mut self,
        client_request_id: &str,
        workspace_id: &str,
        cols: u16,
        rows: u16,
        shell: Option<&str>,
        cwd: Option<&Path>,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let resp = self
            .send_request(&DaemonRequest::Spawn {
                client_request_id: client_request_id.to_string(),
                workspace_id: workspace_id.to_string(),
                worktree: None,
                cwd: cwd.map(|p| p.to_string_lossy().into_owned()),
                cols,
                rows,
                shell: shell.map(str::to_string),
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

    async fn describe_session(
        &mut self,
        session_id: &str,
    ) -> Result<ferryx_lib::daemon::protocol::DaemonSessionDetails, Box<dyn std::error::Error + Send + Sync>>
    {
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

/// Extracts the shell's own pid from `printf 'V5_CHILD_PID=%s\n' "$$"` output.
fn parse_child_pid(accumulated: &str) -> Option<u32> {
    let start = accumulated.rfind("V5_CHILD_PID=")? + "V5_CHILD_PID=".len();
    let rest = &accumulated[start..];
    let end = rest.find(|c: char| !c.is_ascii_digit())?;
    rest[..end].parse().ok()
}

/// True while the process exists (signal 0 is a pure existence probe).
fn process_is_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

struct TestAttachStream {
    reader: BufReader<tokio::net::unix::OwnedReadHalf>,
    /// Held for the stream's lifetime: dropping tokio's `OwnedWriteHalf` shuts down the write half
    /// of the socket, which makes the daemon see EOF and close the connection. Without this the
    /// read half can never deliver live output, and any assertion would silently fall back to the
    /// attach snapshot's history.
    _write_half: tokio::net::unix::OwnedWriteHalf,
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
            token: None,
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
            DaemonResponse::AttachOk { .. } => Ok(Self {
                reader,
                _write_half: write_half,
                attach_resp,
            }),
            other => Err(format!("Attach failed: {other:?}").into()),
        }
    }

    /// Reads history and stream until the shell reports its own pid. The tty echo of the input
    /// line contains the literal `%s` form, so only executed output (`V5_CHILD_PID=<digits>`)
    /// satisfies the parse.
    async fn await_child_pid(
        &mut self,
        max_duration: Duration,
    ) -> Result<u32, Box<dyn std::error::Error + Send + Sync>> {
        let mut accumulated = match &self.attach_resp {
            DaemonResponse::AttachOk { history, .. } => String::from_utf8_lossy(history).to_string(),
            _ => String::new(),
        };
        let start = std::time::Instant::now();
        loop {
            if let Some(pid) = parse_child_pid(&accumulated) {
                return Ok(pid);
            }
            if start.elapsed() >= max_duration {
                return Err(
                    format!("shell never reported its pid; accumulated: {accumulated:?}").into()
                );
            }
            let rem = max_duration.saturating_sub(start.elapsed());
            let mut line = String::new();
            let bytes = timeout(rem, self.reader.read_line(&mut line))
                .await
                .map_err(|_| "Timed out waiting for the shell pid")??;
            if bytes == 0 {
                return Err(format!(
                    "stream closed before the pid arrived; accumulated: {accumulated:?}"
                )
                .into());
            }
            if let Ok(DaemonStreamMessage::Output { data, .. }) =
                serde_json::from_str::<DaemonStreamMessage<'static>>(line.trim())
            {
                accumulated.push_str(&String::from_utf8_lossy(&data));
            }
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
    /// Value applied to `FERRYX_HANDOVER_V5` for every spawned daemon. `None` leaves the variable
    /// absent so the compiled-in default decides the handover path.
    v5_flag: Option<&'static str>,
}

impl PrivateDaemons {
    fn with_v5_flag(v5_flag: Option<&'static str>) -> Self {
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
            v5_flag,
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
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .env("SHELL", "/bin/sh")
            .env("LANG", "C")
            .env("TERM", "xterm-256color")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(flag) = self.v5_flag {
            command.env("FERRYX_HANDOVER_V5", flag);
        }
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
            timeout(DAEMON_READY_BUDGET, async {
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
            let ready_line = timeout(DAEMON_READY_BUDGET, rx)
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
    run_v5_handover_case(Some("1")).await;
}

/// The same v5 contract must hold when `FERRYX_HANDOVER_V5` is absent from the daemon's
/// environment: ownership transfer is the compiled-in default, so a predecessor still retires
/// immediately instead of draining behind a legacy route.
#[tokio::test]
async fn test_v5_ownership_transfer_is_the_default_without_the_flag() {
    run_v5_handover_case(None).await;
}

async fn run_v5_handover_case(v5_flag: Option<&'static str>) {
    let mut daemons = PrivateDaemons::with_v5_flag(v5_flag);
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
        .spawn_with_shell("req-v5-s1", ws_id, 80, 24, Some("/bin/sh"), Some(&canonical_repo))
        .await
        .expect("spawn s1 on D1");

    // Attach BEFORE writing so no early output can be missed, then write and wait for the pid: a
    // write that lands before the shell reads its tty is only echoed by the line discipline, which
    // is why an assertion that accepts echoed text proves nothing.
    let mut attach1 = TestAttachStream::attach(&socket_path, pid_d1, &s1, None)
        .await
        .expect("attach to s1 on D1");

    // Echo off, so every marker observed in the stream proves the CHILD executed it rather than
    // the tty echoing our input back.
    client1
        .write_input(&s1, b"stty -echo\n")
        .await
        .expect("disable echo on s1");

    // Ask the shell for its own pid: the transferred child is the process whose survival this
    // contract is about, and `ps` cannot identify it unambiguously.
    client1
        .write_input(&s1, b"printf 'V5_CHILD_PID=%s\\n' \"$$\"\n")
        .await
        .expect("report child pid");

    let child_pid = attach1
        .await_child_pid(Duration::from_secs(10))
        .await
        .expect("shell must report its pid before the handover");
    assert!(
        process_is_alive(child_pid),
        "the session shell {child_pid} must be alive before the handover"
    );

    // Split the sentinel across concatenated literals so the echoed command line never contains
    // the assembled marker: seeing it in the stream can only mean the shell ran it.
    client1
        .write_input(&s1, b"printf 'V5_%s_%s\\n' 'STREAM' 'TEST_MARKER'\n")
        .await
        .expect("write to s1");

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

    // A session that only LOOKS preserved is not preserved. Retained history is served from the
    // hub snapshot even when the child is gone, so liveness and a fresh round-trip are the real
    // contract: the transferred child must outlive the predecessor, the daemon must still report
    // it running, and input written after the handover must be EXECUTED by that child.
    let post_handover = client_d2
        .describe_session(&s1)
        .await
        .expect("describe transferred session on D2");
    assert!(
        post_handover.running,
        "the transferred session must still be running after the predecessor exits: {post_handover:?}"
    );
    assert!(
        process_is_alive(child_pid),
        "the transferred child {child_pid} must survive the predecessor's exit"
    );

    // A nonce generated after the handover cannot appear in retained history, so observing it in
    // the stream proves the child executed the input we just wrote.
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let split = nonce / 2;
    let rest = nonce - split;
    let post_marker = format!("V5_POST_{split}_{rest}");
    let end_marker = format!("V5_END_{split}_{rest}");

    let mut attach3 = TestAttachStream::attach(&socket_path, pid_d2, &s1, None)
        .await
        .expect("attach to transferred s1 on D2 for the post-handover round-trip");

    // The nonce is split across two printf arguments, so the echoed input line never contains the
    // assembled marker; observing it proves execution, not echo.
    client_d2
        .write_input(
            &s1,
            format!(
                "printf 'V5_%s_%s\\n' 'POST_{split}' '{rest}'; printf 'V5_%s_%s\\n' 'END_{split}' '{rest}'\n"
            )
            .as_bytes(),
        )
        .await
        .expect("post-handover write must be accepted");

    let round_trip = attach3
        .await_pattern_in_history_or_stream(&end_marker, Duration::from_secs(10))
        .await
        .expect("post-handover input must be executed by the transferred child");
    assert_eq!(
        round_trip.matches(&post_marker).count(),
        1,
        "post-handover marker must arrive exactly once (echo is off, so this proves execution): {round_trip}"
    );

    // The round-trip must not have cost the session either.
    assert!(
        process_is_alive(child_pid),
        "the transferred child {child_pid} must survive the post-handover round-trip"
    );
    let settled = client_d2
        .describe_session(&s1)
        .await
        .expect("describe transferred session after the round-trip");
    assert!(
        settled.running,
        "the transferred session must still be running after the round-trip: {settled:?}"
    );

    let routes_file = daemons.root.path().join("runtime/handover_routes.json");
    assert!(!routes_file.exists(), "handover_routes.json must NOT exist on v5 path");
}
