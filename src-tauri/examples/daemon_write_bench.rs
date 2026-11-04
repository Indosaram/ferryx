//! Daemon Write latency benchmark. Not product code.
//!
//! Starts an isolated daemon in this process (private FERRYX_RUNTIME_DIR and FERRYX_DATA_DIR in
//! a fresh temp dir, never the per-user daemon), spawns one `cat >/dev/null` pane, then sends
//! 10,000 one-byte `Write` requests over the real `daemon.sock`, one at a time, each timed from
//! the request line being written to its response line being read. Prints one JSON object:
//! `{"requests":N,"p50_us":..,"p95_us":..,"p99_us":..}`. Exit status 1 when a Write is not
//! answered `WriteOk`.
//!
//! Usage: cargo run --manifest-path src-tauri/Cargo.toml --release --example daemon_write_bench

#[cfg(unix)]
#[tokio::main]
async fn main() -> std::process::ExitCode {
    use ferryx_lib::daemon::{DaemonRequest, DaemonResponse, DaemonServer};
    use std::sync::Arc;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    const REQUESTS: usize = 10_000;
    const WARMUP: usize = 200;

    let root = tempfile::tempdir().expect("temp root");
    std::env::set_var("FERRYX_RUNTIME_DIR", root.path().join("runtime"));
    std::env::set_var("FERRYX_DATA_DIR", root.path().join("data"));
    std::env::set_var("HOME", root.path().join("home"));
    tokio::fs::create_dir_all(root.path().join("home")).await.expect("home");

    let daemon = Arc::new(DaemonServer::new());
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let serving = tokio::spawn(Arc::clone(&daemon).run_server_with_handover_and_readiness(None, Some(ready_tx)));
    tokio::select! {
        result = serving => panic!("daemon ended before ready: {result:?}"),
        result = ready_rx => result.expect("daemon ready"),
    }

    let mut cmd = portable_pty::CommandBuilder::new("/bin/sh");
    cmd.arg("-c");
    cmd.arg("stty -echo; exec cat >/dev/null");
    let (session_id, mut output) = daemon
        .terminal_service()
        .pty_manager()
        .spawn(cmd, 80, 24)
        .expect("spawn the cat pane");
    tokio::spawn(async move { while output.recv().await.is_some() {} });

    let stream = tokio::net::UnixStream::connect(root.path().join("runtime").join("daemon.sock"))
        .await
        .expect("connect daemon.sock");
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut frame = serde_json::to_string(&DaemonRequest::Write {
        session_id: session_id.clone(),
        data: b"x".to_vec(),
    })
    .expect("encode Write");
    frame.push('\n');

    let mut samples = Vec::with_capacity(REQUESTS);
    let mut line = String::new();
    for index in 0..WARMUP + REQUESTS {
        line.clear();
        let started = std::time::Instant::now();
        write_half.write_all(frame.as_bytes()).await.expect("send Write");
        let read = reader.read_line(&mut line).await.expect("read response");
        let elapsed = started.elapsed();
        match serde_json::from_str::<DaemonResponse>(line.trim()) {
            Ok(DaemonResponse::WriteOk) if read > 0 => {}
            other => {
                eprintln!("write {index} answered {other:?} ({line:?})");
                return std::process::ExitCode::FAILURE;
            }
        }
        if index >= WARMUP {
            samples.push(elapsed.as_micros() as u64);
        }
    }
    samples.sort_unstable();
    let percentile = |p: usize| samples[(samples.len() * p / 100).min(samples.len() - 1)];
    println!(
        "{}",
        serde_json::json!({
            "requests": samples.len(),
            "p50_us": percentile(50),
            "p95_us": percentile(95),
            "p99_us": percentile(99),
        })
    );
    let _ = daemon.terminal_service().pty_manager().close_session(&session_id).await;
    std::process::ExitCode::SUCCESS
}

#[cfg(not(unix))]
fn main() {
    eprintln!("daemon_write_bench drives daemon.sock and runs on unix only");
}
