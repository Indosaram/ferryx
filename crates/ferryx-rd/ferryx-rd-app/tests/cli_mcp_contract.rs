use ferryx_rd_net::{PskIdentity, TlsPskServer};
use ferryx_rd_proto::{
    Capabilities, ControlMessage, Handshake, InputEvent, InputEventType, PacketHeader, PacketType,
    WireCodec, PROTOCOL_VERSION,
};
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    sync::{mpsc, oneshot},
};

const DEADLINE: Duration = Duration::from_secs(5);

#[path = "support/client_exe.rs"]
mod client_exe;

enum Shutdown {
    Eof,
    Http,
    OccupiedHttp,
    StartupFailure,
}

#[tokio::test]
async fn eof_releases_then_disconnects_with_http() {
    owned_session(Shutdown::Eof).await;
}

#[tokio::test]
async fn http_stop_joins_mcp_with_stdin_open() {
    owned_session(Shutdown::Http).await;
}

#[tokio::test]
async fn occupied_http_port_exits_without_a_default_deadline() {
    owned_session(Shutdown::OccupiedHttp).await;
}

#[tokio::test]
async fn startup_failure_cancels_fixture_admission() {
    owned_session(Shutdown::StartupFailure).await;
}

async fn owned_session(shutdown: Shutdown) {
    // Given: private sockets, cancellable admission, and deadline-bound TLS I/O.
    let key = [0x42; 32];
    let server = TlsPskServer::new([PskIdentity::pairing("fixture", &key).unwrap()]).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let udp = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let (cancel_tx, cancel_rx) = oneshot::channel::<()>();
    let host = tokio::spawn(async move {
        let accepted = tokio::select! {
            _ = cancel_rx => return None,
            result = tokio::time::timeout(DEADLINE, listener.accept()) => result.ok()?.ok()?,
        };
        let socket = accepted.0.into_std().unwrap();
        tokio::task::spawn_blocking(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(15);
            let mut stream = server.accept_stream_until(socket, deadline).unwrap();
            stream
                .ssl_stream()
                .get_ref()
                .set_write_timeout(Some(DEADLINE))
                .unwrap();
            stream.read_frame_until(deadline).unwrap().unwrap();
            let ack = Handshake {
                name: "fixture".into(),
                width: 64,
                height: 64,
                scale: 1.0,
                version: PROTOCOL_VERSION,
                capabilities: Capabilities::AUTHENTICATED_UDP_REGISTRATION,
                pairing_id: String::new(),
                session_salt: [0; 16],
            };
            let mut packet = PacketHeader::new(PacketType::HandshakeAck, 0, 0, 0)
                .encode()
                .unwrap();
            packet.extend(ack.encode().unwrap());
            stream.write_frame(&packet).unwrap();
            let mut events = Vec::new();
            let mut disconnected = false;
            while let Ok(Some(frame)) = stream.read_frame_until(deadline) {
                let header = PacketHeader::decode(&frame[..PacketHeader::SIZE]).unwrap();
                let body = &frame[PacketHeader::SIZE..];
                match header.packet_type {
                    PacketType::InputEvent => {
                        events.push(InputEvent::decode(body).unwrap().event_type)
                    }
                    PacketType::Control
                        if ControlMessage::decode(body).unwrap() == ControlMessage::Disconnect =>
                    {
                        disconnected = true;
                        break;
                    }
                    _ => {}
                }
            }
            Some((events, disconnected))
        })
        .await
        .unwrap()
    });
    let directory = tempfile::tempdir().unwrap();
    let store = directory.path().join("store.json");
    if matches!(shutdown, Shutdown::StartupFailure) {
        std::fs::write(&store, b"not json").unwrap();
    }
    let mut command = tokio::process::Command::new(client_exe::client_exe());
    command
        .args([
            "--host",
            "127.0.0.1",
            "--tcp-port",
            &addr.port().to_string(),
            "--udp-port",
            &udp.local_addr().unwrap().port().to_string(),
            "--pairing-id",
            "fixture",
            "--pairing-store",
        ])
        .arg(&store);
    if !matches!(shutdown, Shutdown::StartupFailure) {
        command.args(["--psk-hex", &"42".repeat(32)]);
    }
    if !matches!(shutdown, Shutdown::OccupiedHttp) {
        command.arg("--mcp");
    }
    command.args([
        "--allow-unauthenticated-agent",
        "--agent-server",
        &if matches!(shutdown, Shutdown::OccupiedHttp) {
            occupied.local_addr().unwrap().port().to_string()
        } else {
            "0".to_string()
        },
    ]);
    let mut child = command
        .env("RUST_LOG", "info")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let stderr = child.stderr.take().unwrap();
    let (ready_tx, mut ready_rx) = mpsc::unbounded_channel();
    let logs = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        let mut captured = String::new();
        while let Some(line) = lines.next_line().await.unwrap() {
            if let Some(suffix) = line
                .split("Headless agent server listening on http://")
                .nth(1)
            {
                let _ = ready_tx.send(suffix.trim().to_string());
            }
            captured.push_str(&line);
            captured.push('\n');
        }
        captured
    });
    // When: trigger the chosen terminal event. Assertions follow owned cleanup.
    let operation = tokio::time::timeout(DEADLINE, async {
        match shutdown {
            Shutdown::Eof | Shutdown::Http => {
                stdin.as_mut().unwrap().write_all(
                    b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n"
                ).await?;
                let mut line = String::new();
                stdout.read_line(&mut line).await?;
                let response: serde_json::Value = serde_json::from_str(&line)?;
                let address = ready_rx.recv().await.ok_or_else(|| std::io::Error::other("no listener"))?;
                match shutdown {
                    Shutdown::Eof => drop(stdin.take()),
                    Shutdown::Http => {
                        let mut http = tokio::net::TcpStream::connect(address).await?;
                        http.write_all(b"POST /api/v1/session/disconnect HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n").await?;
                    }
                    Shutdown::OccupiedHttp | Shutdown::StartupFailure => unreachable!(),
                }
                anyhow::ensure!(response["id"] == 1, "initialize response ID mismatch");
            }
            Shutdown::OccupiedHttp | Shutdown::StartupFailure => {}
        }
        Ok::<(), anyhow::Error>(())
    }).await;
    let exited = tokio::time::timeout(DEADLINE, child.wait()).await;
    let timely_exit = exited.is_ok();
    if !timely_exit {
        child.kill().await.unwrap();
    }
    let status = child.wait().await.unwrap();
    drop(stdin);
    let _ = cancel_tx.send(());
    let peer = tokio::time::timeout(Duration::from_secs(20), host)
        .await
        .unwrap()
        .unwrap();
    let logs = tokio::time::timeout(DEADLINE, logs).await.unwrap().unwrap();
    let mut tail = String::new();
    tokio::time::timeout(DEADLINE, stdout.read_to_string(&mut tail))
        .await
        .unwrap()
        .unwrap();
    // Then: no failure can strand admission, child exit, or output reader jobs.
    operation.unwrap().unwrap();
    assert!(
        timely_exit,
        "agent worker completion did not terminate the CLI"
    );
    match shutdown {
        Shutdown::Eof | Shutdown::Http => {
            assert!(status.success(), "{logs}");
            let (events, disconnected) = peer.unwrap();
            assert!(disconnected);
            assert!(events.contains(&InputEventType::Reset));
        }
        Shutdown::OccupiedHttp => {
            assert!(!status.success());
            assert_eq!(status.code(), Some(1), "binding failure must propagate");
            assert!(
                peer.unwrap().1,
                "failed API must disconnect the remote session"
            );
        }
        Shutdown::StartupFailure => {
            assert!(!status.success());
            assert!(peer.is_none(), "startup failure unexpectedly connected");
            assert!(tail.is_empty(), "startup diagnostics contaminated stdout");
            assert!(!logs.is_empty());
        }
    }
}

#[tokio::test]
async fn corrupt_store_keeps_stdout_protocol_only() {
    // Given: an explicit corrupt private store, without contacting a host.
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("pairings.json");
    std::fs::write(&store, b"not json").unwrap();
    // When: the real MCP binary fails during startup.
    let output = tokio::time::timeout(
        DEADLINE,
        tokio::process::Command::new(client_exe::client_exe())
            .args([
                "--host",
                "127.0.0.1",
                "--pairing-id",
                "fixture",
                "--mcp",
                "--pairing-store",
            ])
            .arg(store)
            .env("RUST_LOG", "info")
            .kill_on_drop(true)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    // Then: diagnostics never enter the protocol stream.
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
}
