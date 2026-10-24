//! The runtime dir moved from `/tmp/rorca-{uid}` to `/tmp/ferryx-{uid}`. For one release the
//! daemon also serves `daemon.sock` in the legacy dir, so a GUI from before the rename still
//! connects. `FERRYX_LEGACY_RUNTIME_DIR` points the legacy dir into this fixture.
#![cfg(unix)]

use ferryx_lib::daemon::protocol::{DaemonRequest, DaemonResponse, DAEMON_PROTOCOL_VERSION};
use std::error::Error;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::process::Command;
use tokio::time::timeout;

type TestResult<T> = Result<T, Box<dyn Error + Send + Sync>>;
const DEADLINE: Duration = Duration::from_secs(15);

async fn send(connection: &mut BufReader<UnixStream>, request: &DaemonRequest) -> TestResult<()> {
    let wire = serde_json::to_string(request)? + "\n";
    connection.get_mut().write_all(wire.as_bytes()).await?;
    Ok(())
}

/// Connects and handshakes on `socket`; returns the open connection and the daemon pid.
async fn handshake(socket: &Path) -> TestResult<(BufReader<UnixStream>, u32)> {
    let mut connection = BufReader::new(UnixStream::connect(socket).await?);
    send(
        &mut connection,
        &DaemonRequest::Handshake { version: DAEMON_PROTOCOL_VERSION, token: None },
    )
    .await?;
    let mut line = String::new();
    connection.read_line(&mut line).await?;
    match serde_json::from_str::<DaemonResponse>(&line)? {
        DaemonResponse::HandshakeOk { pid, .. } => Ok((connection, pid)),
        response => Err(format!("unexpected handshake on {}: {response:?}", socket.display()).into()),
    }
}

#[test]
fn daemon_listens_on_new_and_legacy_socket_paths() -> TestResult<()> {
    let fixture = tempfile::Builder::new().prefix("fx-legacy-sock-").tempdir_in("/tmp")?;
    let root = fixture.path().to_path_buf();
    let runtime = root.join("ferryx-run");
    let legacy = root.join("rorca-run");
    for dir in ["home", "data", "sessions", "tmp"] {
        std::fs::create_dir_all(root.join(dir))?;
    }
    let executor = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let mut child = Command::new(env!("CARGO_BIN_EXE_ferryx"))
        .arg("--daemon")
        // Never inherit live daemon endpoints, remote configuration, or the user's home.
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("FERRYX_RUNTIME_DIR", &runtime)
        .env("FERRYX_LEGACY_RUNTIME_DIR", &legacy)
        .env("FERRYX_DATA_DIR", root.join("data"))
        .env("FERRYX_SESSION_DIR", root.join("sessions"))
        .env("HOME", root.join("home"))
        .env("TMPDIR", root.join("tmp"))
        .current_dir(root.join("home"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()?;
    let child_pid = child.id().ok_or("spawned daemon has no pid")?;
    let mut stdout = BufReader::new(child.stdout.take().ok_or("missing stdout")?).lines();

    let outcome: TestResult<()> = executor.block_on(async {
        // The readiness subscription exists before the daemon starts; no polling.
        let ready = timeout(DEADLINE, stdout.next_line()).await??;
        if ready.as_deref() != Some("FERRYX_DAEMON_READY") {
            return Err(format!("daemon did not become ready: {ready:?}").into());
        }
        let new_socket = runtime.join("daemon.sock");
        let legacy_socket = legacy.join("daemon.sock");

        let (_new, new_pid) = timeout(DEADLINE, handshake(&new_socket)).await??;
        let (_old, legacy_pid) = timeout(DEADLINE, handshake(&legacy_socket)).await??;
        assert_eq!(new_pid, child_pid, "the new socket is served by the spawned daemon");
        assert_eq!(legacy_pid, child_pid, "the legacy socket is served by the same daemon");

        let meta = std::fs::symlink_metadata(&legacy_socket)?;
        assert!(meta.file_type().is_socket(), "the legacy path is a real socket, not a symlink");
        assert_eq!(meta.permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::metadata(&legacy)?.permissions().mode() & 0o777, 0o700);

        // A legacy path removed by hand never affects clients of the new path.
        std::fs::remove_file(&legacy_socket)?;
        let (mut owner, pid) = timeout(DEADLINE, handshake(&new_socket)).await??;
        assert_eq!(pid, child_pid);
        send(&mut owner, &DaemonRequest::Shutdown).await?;
        Ok(())
    });

    let status = executor.block_on(async { timeout(DEADLINE, child.wait()).await });
    if !matches!(status, Ok(Ok(_))) {
        executor.block_on(async {
            let _ = child.kill().await;
        });
    }
    outcome?;
    fixture.close()?;
    Ok(())
}
