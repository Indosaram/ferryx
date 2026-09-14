use ferryx_lib::daemon::protocol::{DaemonRequest, DaemonResponse, DAEMON_PROTOCOL_VERSION};
use std::error::Error;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::time::timeout;

type TestResult<T> = Result<T, Box<dyn Error + Send + Sync>>;
const DEADLINE: Duration = Duration::from_secs(15);
const READY: &str = "FERRYX_DAEMON_READY";

#[cfg(unix)]
mod platform {
    use super::*;
    pub type Stream = tokio::net::UnixStream;
    pub async fn connect(runtime: &Path) -> TestResult<Stream> {
        Ok(Stream::connect(runtime.join("daemon.sock")).await?)
    }
}

#[cfg(not(unix))]
mod platform {
    use super::*;
    pub type Stream = tokio::net::TcpStream;
    pub async fn connect(runtime: &Path) -> TestResult<Stream> {
        let path = runtime.join("daemon.port");
        let port = tokio::fs::read_to_string(path).await?.parse::<u16>()?;
        Ok(Stream::connect((std::net::Ipv4Addr::LOCALHOST, port)).await?)
    }
}

async fn handshake<S: AsyncRead + AsyncWrite + Unpin>(
    stream: S,
) -> TestResult<(BufReader<S>, u32)> {
    let mut connection = BufReader::new(stream);
    let request = DaemonRequest::Handshake {
        version: DAEMON_PROTOCOL_VERSION,
    };
    let wire = serde_json::to_string(&request)? + "\n";
    connection.get_mut().write_all(wire.as_bytes()).await?;
    let mut line = String::new();
    connection.read_line(&mut line).await?;
    match serde_json::from_str::<DaemonResponse>(&line)? {
        DaemonResponse::HandshakeOk { pid, .. } => Ok((connection, pid)),
        response => Err(format!("unexpected handshake variant: {response:?}").into()),
    }
}

struct TestDaemon {
    child: Child,
    pid: u32,
    stdout: tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
    // Keep this connection: a broken lock must not redirect owner cleanup to the contender.
    connection: Option<BufReader<platform::Stream>>,
}

impl TestDaemon {
    fn spawn(root: &Path, runtime: &Path, data: &Path) -> TestResult<Self> {
        let mut child = Command::new(env!("CARGO_BIN_EXE_ferryx"))
            .arg("--daemon")
            // Never inherit live daemon endpoints, remote configuration, or the user's home.
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("FERRYX_RUNTIME_DIR", runtime)
            .env("FERRYX_DATA_DIR", data)
            .env("FERRYX_SESSION_DIR", root.join("sessions"))
            .env("HOME", root.join("home"))
            .env("USERPROFILE", root.join("home"))
            .env("APPDATA", root.join("data"))
            .env("LOCALAPPDATA", root.join("data"))
            .env("TMPDIR", root.join("tmp"))
            .env("TMP", root.join("tmp"))
            .env("TEMP", root.join("tmp"))
            // Windows needs its OS directory, not any Ferryx configuration.
            .env(
                "SystemRoot",
                std::env::var_os("SystemRoot").unwrap_or_default(),
            )
            .current_dir(root.join("home"))
            .stdin(Stdio::null())
            // The readiness subscription exists before spawn; no polling or fixed sleeps.
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let pid = child.id().ok_or("spawned daemon has no PID")?;
        let stdout = BufReader::new(child.stdout.take().ok_or("missing stdout pipe")?).lines();
        eprintln!("spawned isolated daemon pid={pid}");
        Ok(Self {
            child,
            pid,
            stdout,
            connection: None,
        })
    }

    async fn connect_owned(&mut self, runtime: &Path) -> TestResult<()> {
        let (connection, pid) = handshake(platform::connect(runtime).await?).await?;
        if pid != self.pid {
            return Err(format!("isolated endpoint PID {pid} != child PID {}", self.pid).into());
        }
        self.connection = Some(connection);
        Ok(())
    }

    async fn stop(&mut self, runtime: &Path) -> TestResult<()> {
        if self.child.try_wait()?.is_none() {
            if self.connection.is_none() {
                self.connect_owned(runtime).await?;
            }
            let wire = serde_json::to_string(&DaemonRequest::Shutdown)? + "\n";
            self.connection
                .as_mut()
                .ok_or("missing owned connection")?
                .get_mut()
                .write_all(wire.as_bytes())
                .await?;
        }
        let status = self.child.wait().await?;
        eprintln!("reaped isolated daemon pid={} status={status}", self.pid);
        Ok(())
    }
}

#[test]
fn second_daemon_refuses_already_locked_socket_directory() -> TestResult<()> {
    // All paths are children of a fresh fixture; no default socket or data path is consulted.
    let fixture = tempfile::Builder::new().prefix("fx-dup-").tempdir()?;
    let root = fixture.path().to_path_buf();
    let runtime_dir = root.join("run");
    let data_dir = root.join("data");
    for path in [
        &runtime_dir,
        &data_dir,
        &root.join("home"),
        &root.join("sessions"),
        &root.join("tmp"),
    ] {
        std::fs::create_dir_all(path)?;
    }
    eprintln!(
        "isolated runtime={} data={}",
        runtime_dir.display(),
        data_dir.display()
    );
    let executor = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let mut children = Vec::new();
    let outcome: TestResult<()> = executor.block_on(async {
        children.push(TestDaemon::spawn(&root, &runtime_dir, &data_dir)?);
        let ready = timeout(DEADLINE, children[0].stdout.next_line()).await??;
        if ready.as_deref() != Some(READY) {
            return Err(format!("first daemon did not become ready: {ready:?}").into());
        }
        timeout(DEADLINE, children[0].connect_owned(&runtime_dir)).await??;

        // The contender uses exactly the owner's socket directory AND persistent lock directory.
        children.push(TestDaemon::spawn(&root, &runtime_dir, &data_dir)?);
        let contender_line = timeout(DEADLINE, children[1].stdout.next_line()).await??;
        // Capture an owned cleanup connection even when a mutation lets the duplicate start.
        if contender_line.as_deref() == Some(READY) {
            timeout(DEADLINE, children[1].connect_owned(&runtime_dir)).await??;
            return Err("duplicate daemon reached readiness while owner held both locks".into());
        }
        let status = timeout(DEADLINE, children[1].child.wait()).await??;
        if status.code() != Some(1) {
            return Err(
                format!("duplicate must refuse startup with exit code 1, got {status}").into(),
            );
        }
        eprintln!("duplicate refused startup: {status}; no readiness token");
        if children[0].child.try_wait()?.is_some() {
            return Err("owner exited when duplicate attempted startup".into());
        }
        // A fresh connection proves the losing process did not unlink/replace the owner's socket.
        let (_, pid) = timeout(DEADLINE, async {
            handshake(platform::connect(&runtime_dir).await?).await
        })
        .await??;
        if pid != children[0].pid {
            return Err("duplicate replaced the owner's endpoint".into());
        }
        Ok(())
    });

    // Cleanup precedes the assertion, including the mutation-failure path. Only protocol shutdown
    // over PID-checked, fixture-local connections is used; never send OS signals to any daemon.
    let mut cleanup_errors = Vec::new();
    for child in children.iter_mut().rev() {
        if let Err(error) =
            executor.block_on(async { timeout(DEADLINE, child.stop(&runtime_dir)).await? })
        {
            cleanup_errors.push(error.to_string());
        }
    }
    drop(children);
    fixture.close()?;
    assert!(!root.exists(), "temporary daemon fixture survived cleanup");
    assert!(
        cleanup_errors.is_empty(),
        "daemon cleanup failed: {cleanup_errors:?}"
    );
    eprintln!(
        "cleanup verified: children reaped and {} removed",
        root.display()
    );
    assert!(outcome.is_ok(), "{}", outcome.unwrap_err());
    Ok(())
}
