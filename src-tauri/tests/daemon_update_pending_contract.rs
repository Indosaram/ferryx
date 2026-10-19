//! Todo 9 integration: a genuinely incompatible old owner (a daemon built with
//! `--features qa-protocol-bump`, DAEMON_PROTOCOL_VERSION 6) against this normal build as the client.
//!
//! The bumped daemon cannot be produced by this test binary, so the verify node builds it first and
//! passes its path in FERRYX_QA_BUMPED_DAEMON:
//!   cargo build --manifest-path src-tauri/Cargo.toml --features qa-protocol-bump --target-dir <dir>
//!   FERRYX_QA_BUMPED_DAEMON=<dir>/debug/ferryx[.exe] cargo test --manifest-path src-tauri/Cargo.toml \
//!     --test daemon_update_pending_contract -- --ignored --test-threads=1
//! Without the variable the test is ignored, never silently passed.

use ferryx_lib::daemon::client::DaemonClient;
use ferryx_lib::daemon::update_pending::RestartOutcome;
use ferryx_lib::ipc::error::IpcErrorCode;
use serde_json::{json, Value};
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::time::timeout;

type TestResult<T> = Result<T, Box<dyn Error + Send + Sync>>;
const BUMPED_PROTOCOL: u32 = 6;
const DEADLINE: Duration = Duration::from_secs(20);

#[cfg(unix)]
type Stream = tokio::net::UnixStream;
#[cfg(not(unix))]
type Stream = tokio::net::TcpStream;

fn endpoint(runtime: &Path) -> PathBuf {
    if cfg!(unix) {
        runtime.join("daemon.sock")
    } else {
        runtime.join("daemon.port")
    }
}

async fn connect(runtime: &Path) -> TestResult<Stream> {
    #[cfg(unix)]
    {
        Ok(Stream::connect(endpoint(runtime)).await?)
    }
    #[cfg(not(unix))]
    {
        let port = tokio::fs::read_to_string(endpoint(runtime)).await?.trim().parse::<u16>()?;
        Ok(Stream::connect((std::net::Ipv4Addr::LOCALHOST, port)).await?)
    }
}

/// A raw protocol-6 data connection, speaking the old owner's own protocol.
struct OldWire {
    reader: BufReader<tokio::io::ReadHalf<Stream>>,
    writer: tokio::io::WriteHalf<Stream>,
}

impl OldWire {
    async fn open(runtime: &Path) -> TestResult<Self> {
        let (read, write) = tokio::io::split(connect(runtime).await?);
        let mut wire = Self { reader: BufReader::new(read), writer: write };
        let token = tokio::fs::read_to_string(runtime.join("daemon.token")).await.ok();
        let reply = wire
            .call(json!({"type":"handshake","version":BUMPED_PROTOCOL,"token":token.map(|t| t.trim().to_string())}))
            .await?;
        assert_eq!(reply["type"], "handshakeOk", "{reply}");
        assert_eq!(reply["version"], BUMPED_PROTOCOL, "the old owner really is protocol 6");
        Ok(wire)
    }

    async fn call(&mut self, request: Value) -> TestResult<Value> {
        self.writer.write_all(format!("{request}\n").as_bytes()).await?;
        self.writer.flush().await?;
        let mut line = String::new();
        timeout(Duration::from_secs(10), self.reader.read_line(&mut line)).await??;
        Ok(serde_json::from_str(line.trim())?)
    }
}

fn spawn_old_owner(binary: &Path, root: &Path, runtime: &Path) -> TestResult<Child> {
    for dir in ["home", "data", "tmp", "sessions"] {
        std::fs::create_dir_all(root.join(dir))?;
    }
    let child = Command::new(binary)
        .arg("--daemon")
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("FERRYX_RUNTIME_DIR", runtime)
        .env("FERRYX_DATA_DIR", root.join("data"))
        .env("FERRYX_SESSION_DIR", root.join("sessions"))
        .env("HOME", root.join("home"))
        .env("USERPROFILE", root.join("home"))
        .env("APPDATA", root.join("data"))
        .env("LOCALAPPDATA", root.join("data"))
        .env("TMPDIR", root.join("tmp"))
        .env("TMP", root.join("tmp"))
        .env("TEMP", root.join("tmp"))
        .env("SystemRoot", std::env::var_os("SystemRoot").unwrap_or_default())
        .current_dir(root.join("home"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()?;
    Ok(child)
}

async fn wait_ready(child: &mut Child) -> TestResult<()> {
    let stdout = child.stdout.take().ok_or("missing stdout")?;
    let mut lines = BufReader::new(stdout).lines();
    timeout(DEADLINE, async {
        while let Some(line) = lines.next_line().await? {
            if line.contains("FERRYX_DAEMON_READY") {
                // Keep draining so the daemon never blocks on a full pipe.
                tokio::spawn(async move { while let Ok(Some(_)) = lines.next_line().await {} });
                return Ok::<(), std::io::Error>(());
            }
        }
        Err(std::io::Error::other("daemon exited before READY"))
    })
    .await??;
    Ok(())
}

fn git_repo(root: &Path) -> TestResult<PathBuf> {
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo)?;
    let ok = std::process::Command::new("git").args(["init", "-b", "main"]).current_dir(&repo).output()?;
    assert!(ok.status.success());
    Ok(repo.canonicalize()?)
}

fn retirement_marker_names_pid(runtime: &Path, pid: u32) -> bool {
    std::fs::read_dir(runtime)
        .map(|entries| {
            entries.flatten().any(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                name.starts_with("retired-")
                    && std::fs::read_to_string(entry.path())
                        .ok()
                        .and_then(|body| serde_json::from_str::<Value>(&body).ok())
                        .is_some_and(|body| body["pid"] == pid)
            })
        })
        .unwrap_or(false)
}

#[tokio::test]
#[ignore = "needs FERRYX_QA_BUMPED_DAEMON: a daemon built with --features qa-protocol-bump"]
async fn incompatible_old_owner_keeps_its_terminals_and_retires_when_idle() -> TestResult<()> {
    let binary = PathBuf::from(
        std::env::var_os("FERRYX_QA_BUMPED_DAEMON").ok_or("FERRYX_QA_BUMPED_DAEMON is not set")?,
    );
    let root = tempfile::tempdir()?;
    // The client below stages its own runtime for RequestUpgrade through
    // daemon_runtime_root(), which honours FERRYX_DATA_DIR. Without this the test process
    // stages its harness executable into the host's real data dir. This is the only test in
    // this integration binary (its own process), and the variable is set before any client or
    // child exists, so no other thread reads the environment concurrently.
    let client_data = root.path().join("client-data");
    std::env::set_var("FERRYX_DATA_DIR", &client_data);
    let runtime = root.path().join("runtime");
    std::fs::create_dir_all(&runtime)?;
    let mut old = spawn_old_owner(&binary, root.path(), &runtime)?;
    let old_pid = old.id().ok_or("old owner has no pid")?;
    wait_ready(&mut old).await?;

    // One live terminal on the old owner, opened over its own protocol.
    let repo = git_repo(root.path())?;
    let mut wire = OldWire::open(&runtime).await?;
    let registered = wire
        .call(json!({"type":"registerWorkspace","workspaceId":"ws-t9","repoRoot":repo}))
        .await?;
    assert_eq!(registered["type"], "registerWorkspaceOk", "{registered}");
    let spawned = wire
        .call(json!({"type":"spawn","clientRequestId":"t9-live","workspaceId":"ws-t9",
            "worktree":null,"cwd":null,"cols":80,"rows":24}))
        .await?;
    assert_eq!(spawned["type"], "spawnOk", "{spawned}");
    let session_id = spawned["sessionId"].as_str().ok_or("no session id")?.to_string();

    // This normal (protocol 5) build connects: pending, never a Shutdown, terminate or spawn. On
    // Unix the client first asks for a control-channel upgrade with a runtime it staged under
    // its private FERRYX_DATA_DIR; the old owner refuses it (the staged executable is outside
    // the old owner's own runtime root), so it is the refused path that enters pending. On
    // Windows nothing is staged and pending is entered directly.
    let client = DaemonClient::new_with_socket(endpoint(&runtime));
    let error = timeout(DEADLINE, client.list_sessions()).await?.unwrap_err();
    assert_eq!(error.code, IpcErrorCode::DaemonUpdatePending, "{error:?}");
    // The client stages only under its private data dir. Linux stages the test executable there;
    // macOS refuses to stage (a test binary has no daemon helper bundle) and Windows never stages,
    // so neither creates a generation anywhere.
    let staged_root = ferryx_lib::daemon::server::daemon_runtime_root().ok_or("no runtime root")?;
    assert_eq!(staged_root, client_data.join("daemon-runtime"), "staging root is the private one");
    let staged: Vec<_> = match std::fs::read_dir(&staged_root) {
        Ok(entries) => entries.flatten().map(|entry| entry.path()).filter(|path| path.is_dir()).collect(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error.into()),
    };
    let expected_generations = if cfg!(target_os = "linux") { 1 } else { 0 };
    assert_eq!(staged.len(), expected_generations, "staged generations: {staged:?}");
    assert!(staged.iter().all(|path| path.starts_with(root.path())), "staged outside the temp dir: {staged:?}");
    let details = error.details.clone().ok_or("no details")?;
    assert_eq!(details["liveSessions"], 1, "{details}");
    assert_eq!(details["retireRequested"], true, "RetireWhenIdle was sent on entering pending");
    assert!(old.try_wait()?.is_none(), "zero Shutdown / terminate: the old owner is alive");
    assert!(
        !runtime.join("starting.json").exists(),
        "zero spawn: no start was admitted while the old owner lives"
    );

    // A stale confirmation is refused by the daemon; nothing exits.
    let outcome = client
        .restart_previous_version_terminals(2)
        .await
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(outcome, RestartOutcome::CountChanged { live: Some(1) });
    assert!(old.try_wait()?.is_none(), "count changed: no exit");
    let probe = wire.call(json!({"type":"ping"})).await?;
    assert_eq!(probe["type"], "pong", "the live terminal's owner still serves: {probe}");

    // The last terminal closes: the old owner retires by itself with a marker and exit 0.
    let closed = wire.call(json!({"type":"close","sessionId":session_id})).await?;
    assert_eq!(closed["type"], "closeOk", "{closed}");
    drop(wire);
    let status = timeout(DEADLINE, old.wait()).await??;
    assert!(status.success(), "RetireWhenIdle exits 0 so the watchdog stops: {status}");
    assert!(
        retirement_marker_names_pid(&runtime, old_pid),
        "the retirement marker names the exited old owner"
    );
    Ok(())
}
