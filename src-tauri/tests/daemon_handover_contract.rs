#![cfg(unix)]
#[path = "support/machine_retirement.rs"]
mod machine_retirement;
use ferryx_lib::daemon::protocol::{
    DaemonRequest, DaemonResponse, DaemonSessionDetails, DaemonStreamMessage,
    DAEMON_PROTOCOL_VERSION,
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
                shell: Some("/bin/sh".to_string()),
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
    _writer: tokio::net::unix::OwnedWriteHalf,
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
        assert!(
            matches!(hs_resp, DaemonResponse::HandshakeOk { pid, version, .. } if pid == expected_pid && version == DAEMON_PROTOCOL_VERSION)
        );

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
        assert!(
            matches!(&attach_resp, DaemonResponse::AttachOk { session_id: id, .. } if id == session_id)
        );

        Ok(Self {
            reader,
            _writer: write_half,
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

// Own children before any fallible readiness/protocol work. Drop runs cleanup on a
// separate runtime so unwinding a current-thread Tokio test cannot strand PTYs.
struct PrivateDaemons {
    root: TempDir,
    children: Vec<Child>,
    drains: Vec<std::thread::JoinHandle<()>>,
    cleanup_errors: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    cleanup_visits: std::sync::Arc<std::sync::Mutex<Vec<u32>>>,
    canonical_first_mutation: bool,
}

impl PrivateDaemons {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("fx-v02-")
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
            drains: Vec::new(),
            cleanup_errors: Default::default(),
            cleanup_visits: Default::default(),
            canonical_first_mutation: false,
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
            // This contract asserts the legacy v4 routing path (a durable route in
            // `handover_routes.json` that keeps the predecessor draining), so opt out of the
            // default-on v5 ownership transfer explicitly.
            .env("FERRYX_HANDOVER_V5", "0")
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

    fn private_socket(&self, path: &Path) {
        assert_eq!(
            path.parent(),
            self.socket().parent(),
            "socket must stay in private runtime"
        );
        assert!(!std::fs::symlink_metadata(path)
            .unwrap()
            .file_type()
            .is_symlink());
    }

    async fn launch(&mut self, legacy: Option<&Path>) -> usize {
        // Handover CLI launches intentionally do not print the ready token.
        // Subscribe before spawning, then validate the replacement's exact PID
        // after private runtime changes; no sleep/retry polling of user sockets.
        use notify::Watcher;
        let (event_tx, mut events) = tokio::sync::mpsc::unbounded_channel();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                // Receiver closure is normal once this launch has completed.
                drop(event_tx.send(event));
            })
            .expect("private runtime watcher");
        watcher
            .watch(
                self.socket().parent().unwrap(),
                notify::RecursiveMode::NonRecursive,
            )
            .expect("watch private runtime before spawn");
        let mut command = self.command();
        if let Some(path) = legacy {
            self.private_socket(path);
            command.arg("--handover-from").arg(path);
        }
        let child = command.spawn().expect("spawn isolated daemon");
        eprintln!(
            "V02 launched owned PID {} handover={} root {}",
            child.id(),
            legacy.is_some(),
            self.root.path().display()
        );
        let index = self.children.len();
        self.children.push(child);
        let child = &mut self.children[index];
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.drains.push(std::thread::spawn(move || {
            use std::io::BufRead;
            let mut reader = std::io::BufReader::new(stdout);
            let mut line = String::new();
            let result = reader
                .read_line(&mut line)
                .map(|_| line.trim_end().to_owned());
            if tx.send(result).is_err() {
                eprintln!("V02 readiness receiver dropped");
            }
            if let Err(error) = std::io::copy(&mut reader, &mut std::io::sink()) {
                eprintln!("V02 stdout drain: {error}");
            }
        }));
        self.drains.push(std::thread::spawn(move || {
            if let Err(error) =
                std::io::copy(&mut std::io::BufReader::new(stderr), &mut std::io::stderr())
            {
                eprintln!("V02 stderr drain: {error}");
            }
        }));
        if legacy.is_some() {
            let mut observed_events = Vec::new();
            timeout(DAEMON_READY_BUDGET, async {
                loop {
                    let event = events
                        .recv()
                        .await
                        .expect("watcher ended")
                        .expect("runtime watch error");
                    observed_events.push(format!("{event:?}"));
                    if !event
                        .paths
                        .iter()
                        .any(|path| path.file_name() == Some(std::ffi::OsStr::new("daemon.sock")))
                    {
                        continue;
                    }
                    match TestDaemonClient::connect(&self.socket(), self.pid(index)).await {
                        Ok(_) => break,
                        Err(error) => eprintln!("V02 replacement not serving yet: {error}"),
                    }
                }
            })
            .await
            .unwrap_or_else(|error| {
                panic!(
                    "replacement owned-PID readiness timeout: {error}; events: {observed_events:?}"
                )
            });
        } else {
            assert_eq!(
                timeout(DAEMON_READY_BUDGET, rx).await.unwrap_or_else(|error| {
                    while let Ok(event) = events.try_recv() {
                        eprintln!("V02 initial timeout filesystem event: {event:?}");
                    }
                    let entries = std::fs::read_dir(self.socket().parent().unwrap())
                        .map(|entries| entries.map(|entry| entry.map(|entry| entry.path())).collect::<Vec<_>>());
                    panic!("V02 initial readiness PID {} root {}: {error}; runtime entries: {entries:?}", self.pid(index), self.root.path().display());
                }).unwrap().unwrap(),
                "FERRYX_DAEMON_READY"
            );
        }
        TestDaemonClient::connect(&self.socket(), self.pid(index))
            .await
            .expect("exact owned PID");
        index
    }

    fn pid(&self, index: usize) -> u32 {
        self.children[index].id()
    }

    fn repo(&self) -> PathBuf {
        let repo = self.root.path().join("repo");
        std::fs::write(repo.join("README.md"), "# Test Repo\n").unwrap();
        for args in [
            vec!["init", "-b", "main"],
            vec!["add", "."],
            vec![
                "-c",
                "user.name=Ferryx Test",
                "-c",
                "user.email=test@ferryx.local",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-m",
                "Initial commit",
            ],
        ] {
            let output = Command::new("/usr/bin/git")
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("HOME", self.root.path().join("home"))
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .current_dir(&repo)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        repo.canonicalize().unwrap()
    }

    fn cleanup(&mut self) -> Vec<String> {
        if self.children.is_empty() {
            return Vec::new();
        }
        let root = self.root.path().to_path_buf();
        let mut children = std::mem::take(&mut self.children);
        let drains = std::mem::take(&mut self.drains);
        let cleanup_visits = std::sync::Arc::clone(&self.cleanup_visits);
        let canonical_first_mutation = self.canonical_first_mutation;
        std::thread::spawn(move || {
            let mut errors = Vec::new();
            let mut cleaned_pids = std::collections::BTreeSet::new();
            let mut connection_failures = Vec::new();
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            match runtime {
                Ok(runtime) => runtime.block_on(async {
                    // Only enumerate sockets inside our fresh mode-0700 TempDir. Never
                    // consult discovery, manifests, or global daemon locations.
                    let entries = match std::fs::read_dir(root.join("runtime")) {
                        Ok(entries) => entries,
                        Err(error) => {
                            errors.push(error.to_string());
                            return;
                        }
                    };
                    let mut sockets = Vec::new();
                    for entry in entries {
                        let entry = match entry {
                            Ok(entry) => entry,
                            Err(error) => {
                                errors.push(error.to_string());
                                continue;
                            }
                        };
                        use std::os::unix::fs::FileTypeExt;
                        let path = entry.path();
                        let name = entry.file_name().to_string_lossy().into_owned();
                        if !(name == "daemon.sock"
                            || (name.starts_with("legacy-") && name.ends_with(".sock")))
                        {
                            continue;
                        }
                        if !entry
                            .file_type()
                            .map(|kind| kind.is_socket())
                            .unwrap_or(false)
                        {
                            continue;
                        }
                        sockets.push(path);
                    }
                    // D2 lists D1's sessions too. Closing through D2 first can retire
                    // D1 and unlink its endpoint before we account for D1's cleanup.
                    // Drain the legacy owner directly before visiting the proxy.
                    sockets.sort_by_key(|path| {
                        (path.file_name() == Some(std::ffi::OsStr::new("daemon.sock")), path.clone())
                    });
                    if canonical_first_mutation {
                        sockets.reverse();
                    }
                    for path in sockets {
                        for child in &mut children {
                            match child.try_wait() {
                                Ok(Some(_)) => continue,
                                Err(error) => {
                                    errors.push(error.to_string());
                                    continue;
                                }
                                Ok(None) => {}
                            }
                            let result = timeout(Duration::from_secs(5), async {
                                let mut client =
                                    TestDaemonClient::connect(&path, child.id()).await?;
                                // Record the exact handshake event before any destructive
                                // Close, not filesystem order or eventual process exit.
                                cleanup_visits.lock().unwrap().push(child.id());
                                let sessions = match client.list_sessions().await {
                                    Ok(sessions) => sessions,
                                    Err(error) => {
                                        errors.push(format!("owned list: {error}"));
                                        return Ok(());
                                    }
                                };
                                let mut closed_all = true;
                                for session in sessions {
                                    match client.close(&session).await {
                                        Ok(()) => eprintln!(
                                            "V02 closed PTY {session} via owned PID {} at {}",
                                            child.id(),
                                            path.display()
                                        ),
                                        Err(error) => {
                                            closed_all = false;
                                            errors.push(format!("owned close {session}: {error}"));
                                        }
                                    }
                                }
                                if closed_all { cleaned_pids.insert(child.id()); }
                                Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
                            })
                            .await;
                            match result {
                                Ok(Ok(())) => break,
                                // A mismatch is expected while trying the other owned child.
                                // Preserve all attempts; an uncovered live child below is a failure.
                                Ok(Err(error)) => connection_failures.push(format!(
                                    "{} PID {}: {error}", path.display(), child.id()
                                )),
                                Err(error) => {
                                    errors.push(format!("cleanup {}: {error}", path.display()))
                                }
                            }
                        }
                    }
                }),
                Err(error) => errors.push(format!("cleanup runtime: {error}")),
            }
            for child in &mut children {
                match child.try_wait() {
                    Ok(Some(_)) => {}
                    Ok(None) => {
                        if !cleaned_pids.contains(&child.id()) {
                            errors.push(format!("owned PID {} had no successful session cleanup; attempts: {connection_failures:?}", child.id()));
                        }
                        if let Err(error) = child.kill() {
                            errors.push(format!("kill owned {}: {error}", child.id()));
                        }
                    }
                    Err(error) => errors.push(error.to_string()),
                }
                match child.wait() {
                    Ok(status) => eprintln!(
                        "V02 reaped owned PID {} ({status}) root {}",
                        child.id(),
                        root.display()
                    ),
                    Err(error) => errors.push(format!("reap owned {}: {error}", child.id())),
                }
            }
            for drain in drains {
                if drain.join().is_err() {
                    errors.push("output drain panicked".into());
                }
            }
            errors
        })
        .join()
        .expect("cleanup thread must not panic")
    }
}

impl Drop for PrivateDaemons {
    fn drop(&mut self) {
        let errors = self.cleanup();
        if !errors.is_empty() {
            self.cleanup_errors.lock().unwrap().extend(errors.clone());
            if std::thread::panicking() {
                eprintln!("V02 cleanup errors: {errors:?}");
            } else {
                panic!("V02 cleanup errors: {errors:?}");
            }
        }
    }
}

fn output_command(marker: &str) -> Vec<u8> {
    let encoded: String = marker.bytes().map(|byte| format!("\\{byte:03o}")).collect();
    let command = format!("printf '{encoded}\\n'\n");
    assert!(
        !command.contains(marker),
        "input echo cannot satisfy output assertion"
    );
    command.into_bytes()
}

#[test]
fn test_private_daemon_command_safety() {
    let daemons = PrivateDaemons::new();
    let mut command = daemons.command();
    // Safe RED: remove the override only from an inspected, never-spawned command.
    if std::env::var_os("FERRYX_V02_SAFETY_MUTATION").is_some() {
        command.env_remove("FERRYX_RUNTIME_DIR");
    }
    let env: std::collections::BTreeMap<_, _> = command.get_envs().collect();
    for (key, name) in [
        ("FERRYX_RUNTIME_DIR", "runtime"),
        ("HOME", "home"),
        ("FERRYX_DATA_DIR", "data"),
        ("FERRYX_SESSION_DIR", "session"),
    ] {
        assert_eq!(
            env.get(std::ffi::OsStr::new(key)).copied().flatten(),
            Some(daemons.root.path().join(name).as_os_str()),
            "private child override required: {key}"
        );
    }
    // Command's Debug representation exposes env_clear; inspect without executing.
    assert!(
        format!("{command:?}").contains("env -i"),
        "child environment must be cleared"
    );
    assert_eq!(command.get_program(), env!("CARGO_BIN_EXE_ferryx"));
    assert_eq!(command.get_args().collect::<Vec<_>>(), ["--daemon"]);
    output_command("V02_GENERATED_OUTPUT");
}

// The local DescribeSession DTO deliberately has no PID field. Obtain $$ only
// from a shell we spawned, after a subscribed output marker confirms its write.
async fn capture_shell_pid(
    daemons: &PrivateDaemons,
    index: usize,
    client: &mut TestDaemonClient,
    session: &str,
    label: &str,
) -> u32 {
    let details = client.describe_session(session).await.unwrap();
    assert_eq!(details.session_id, session);
    assert!(details.running);
    let path = daemons.root.path().join(format!("{label}.pid"));
    let marker = format!("V02_{label}_PID_RECORDED");
    let mut stream = TestAttachStream::attach(&daemons.socket(), daemons.pid(index), session, None)
        .await
        .unwrap();
    let mut command = format!("printf '%s\\n' \"$$\" > '{}'; ", path.display()).into_bytes();
    command.extend(output_command(&marker));
    client.write_input(session, &command).await.unwrap();
    stream
        .await_pattern_in_history_or_stream(&marker, Duration::from_secs(5))
        .await
        .unwrap();
    let pid: u32 = std::fs::read_to_string(&path)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(pid > 1);
    assert_ne!(pid, daemons.pid(index));
    eprintln!(
        "V02 owned shell PID {pid} session {session} daemon PID {} root {}",
        daemons.pid(index),
        daemons.root.path().display()
    );
    pid
}

#[test]
fn test_owned_children_cleanup_on_error_and_panic() {
    for panic_in_body in [false, true] {
        let mut owned_pids = Vec::new();
        let mut shell_pids = Vec::new();
        let mut owned_root = PathBuf::new();
        let mut cleanup_errors = None;
        let mut cleanup_visits = None;
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async {
                let mut daemons = PrivateDaemons::new();
                cleanup_errors = Some(std::sync::Arc::clone(&daemons.cleanup_errors));
                cleanup_visits = Some(std::sync::Arc::clone(&daemons.cleanup_visits));
                // Fault injection changes only traversal of this owner's private sockets.
                daemons.canonical_first_mutation =
                    std::env::var_os("FERRYX_V02_CANONICAL_FIRST_MUTATION").is_some();
                owned_root = daemons.root.path().to_path_buf();
                let d1 = daemons.launch(None).await;
                owned_pids.push(daemons.pid(d1));
                let repo = daemons.repo();
                let mut client = TestDaemonClient::connect(&daemons.socket(), daemons.pid(d1))
                    .await
                    .unwrap();
                client.register_workspace("cleanup", &repo).await.unwrap();
                let session = client
                    .spawn("cleanup-old", "cleanup", 80, 24)
                    .await
                    .unwrap();
                shell_pids
                    .push(capture_shell_pid(&daemons, d1, &mut client, &session, "old").await);
                let legacy = match client
                    .send_request(&DaemonRequest::PrepareHandover)
                    .await
                    .unwrap()
                {
                    DaemonResponse::PrepareHandoverOk {
                        legacy_socket_path, ..
                    } => legacy_socket_path,
                    other => panic!("prepare: {other:?}"),
                };
                let d2 = daemons.launch(Some(Path::new(&legacy))).await;
                owned_pids.push(daemons.pid(d2));
                let mut client = TestDaemonClient::connect(&daemons.socket(), daemons.pid(d2))
                    .await
                    .unwrap();
                client.register_workspace("cleanup", &repo).await.unwrap();
                let session = client
                    .spawn("cleanup-new", "cleanup", 80, 24)
                    .await
                    .unwrap();
                shell_pids
                    .push(capture_shell_pid(&daemons, d2, &mut client, &session, "new").await);
                if panic_in_body {
                    panic!("V02 intentional owned-child cleanup proof");
                }
                Err::<(), _>("V02 intentional error cleanup proof")
            })
        }));
        assert_eq!(
            cleanup_visits.unwrap().lock().unwrap().as_slice(),
            owned_pids.as_slice(),
            "cleanup must handshake legacy owner before proxy can close its sessions"
        );
        if panic_in_body {
            let panic = outcome.expect_err("intentional panic must unwind");
            assert_eq!(
                panic.downcast_ref::<&str>(),
                Some(&"V02 intentional owned-child cleanup proof")
            );
        } else {
            assert_eq!(outcome.unwrap(), Err("V02 intentional error cleanup proof"));
        }
        assert_eq!(owned_pids.len(), 2, "proof must reach both owned daemons");
        let cleanup_errors = cleanup_errors.unwrap();
        let cleanup_errors = cleanup_errors.lock().unwrap();
        assert!(
            cleanup_errors.is_empty(),
            "unwinding cleanup must not conceal errors: {cleanup_errors:?}"
        );
        assert!(!owned_root.exists(), "private root removed after cleanup");
        assert_eq!(shell_pids.len(), 2, "proof must capture both owned shells");
        for pid in owned_pids.into_iter().chain(shell_pids) {
            // Signal zero only observes exact PIDs obtained from our children.
            assert_eq!(
                unsafe { libc::kill(pid as i32, 0) },
                -1,
                "owned PID {pid} survived"
            );
            assert_eq!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::ESRCH)
            );
            eprintln!(
                "V02 confirmed absent owned PID {pid} root {}",
                owned_root.display()
            );
        }
    }
}

#[tokio::test]
async fn test_rolling_handover_two_daemon_lifecycle() {
    let mut daemons = PrivateDaemons::new();
    let socket_path = daemons.socket();
    let d1 = daemons.launch(None).await;
    let canonical_repo = daemons.repo();

    let mut client1 = TestDaemonClient::connect(&socket_path, daemons.pid(d1))
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
        .write_input(&s1, &output_command("HANDOVER_SESSION_1_READY"))
        .await
        .expect("write to s1");

    let mut attach1 = TestAttachStream::attach(&socket_path, daemons.pid(d1), &s1, None)
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

    daemons.private_socket(Path::new(&legacy_socket_path));
    let d2 = daemons.launch(Some(Path::new(&legacy_socket_path))).await;

    // Then: D2 is canonical on socket_path
    let mut client_d2 = TestDaemonClient::connect(&socket_path, daemons.pid(d2))
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
        .write_input(&s1, &output_command("PROXIED_WRITE_TO_OLD_SESSION_OK"))
        .await
        .expect("write to s1 via D2");

    let mut attach_d2 = TestAttachStream::attach(&socket_path, daemons.pid(d2), &s1, None)
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
    let mut legacy_client =
        TestDaemonClient::connect(Path::new(&legacy_socket_path), daemons.pid(d1))
            .await
            .expect("connect to D1 legacy socket");
    let spawn_refused = legacy_client.spawn("req-refused", ws_id, 80, 24).await;
    assert!(
        spawn_refused.is_err(),
        "D1 must refuse new spawns while draining"
    );

    // 6. Closing s1 closes old session; D1 exits automatically when its last session closes
    let mut exits = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::child()).unwrap();
    client_d2.close(&s1).await.expect("close s1 via D2");

    let d1_exit = timeout(Duration::from_secs(5), async {
        loop {
            if let Some(status) = daemons.children[d1].try_wait().expect("D1 wait") {
                break status;
            }
            exits.recv().await.expect("SIGCHLD stream");
        }
    })
    .await
    .expect("D1 must retire when its final session closes");
    assert!(d1_exit.success(), "D1 exited cleanly: {d1_exit}");

    // 7. D2 remains healthy and serving s2
    let desc_s2 = client_d2
        .describe_session(&s2)
        .await
        .expect("DescribeSession s2 on D2");
    assert_eq!(desc_s2.session_id, s2);
    assert!(desc_s2.running);

    client_d2.close(&s2).await.expect("close s2");
    assert!(daemons.cleanup().is_empty());
}

#[tokio::test]
async fn test_rolling_handover_prepare_abort_rollback() {
    let mut daemons = PrivateDaemons::new();
    let socket_path = daemons.socket();
    let d1 = daemons.launch(None).await;

    let mut client1 = TestDaemonClient::connect(&socket_path, daemons.pid(d1))
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

    daemons.private_socket(Path::new(&legacy_socket_path));
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

    assert!(daemons.cleanup().is_empty());
}
