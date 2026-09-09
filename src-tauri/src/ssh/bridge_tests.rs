use super::*;
use crate::ssh::runtime::{RemoteEnvironment, RemoteExecutor, RemotePlatform};
use crate::ssh::{SshAuthMethod, SshHost, SshHostSource};
use std::io::BufRead;
use std::path::PathBuf;
use std::process::Stdio;
use tempfile::TempDir;

#[test]
fn ssh_reconnect_safety_setup_error_preserves_structured_fields() {
    // Given: setup failed with a machine-readable code and diagnostic details.
    let expected = IpcError::new(crate::ipc::IpcErrorCode::IoError, "opaque diagnostic")
        .with_details(serde_json::json!({"stage": "transport", "exitCode": 255}));
    // When: the error crosses the bridge boundary.
    let converted = BridgeError::from(expected.clone());
    // Then: the runtime can classify the original fields without parsing prose.
    match converted {
        BridgeError::SshSetup(actual) => assert_eq!(actual, expected),
        other => panic!("Setup error lost its structured fields: {other:?}"),
    }
}

fn helper_binary_path() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let debug_path = manifest_dir.join("../remote-helper/target/debug/ferryx-remote-helper");
    assert!(
        debug_path.is_file(),
        "ferryx-remote-helper debug binary must exist at {}",
        debug_path.display()
    );
    debug_path
}

fn sample_host(id: &str) -> SshHost {
    SshHost {
        id: id.to_string(),
        label: "test-host".to_string(),
        hostname: "127.0.0.1".to_string(),
        username: None,
        port: None,
        identity_file: None,
        jump_host: None,
        source: SshHostSource::Config,
        auth_method: SshAuthMethod::Agent,
        disabled: None,
    }
}

fn sample_env() -> RemoteEnvironment {
    RemoteEnvironment {
        platform: RemotePlatform::Posix,
        executor: RemoteExecutor::Sh,
        version: "Darwin 25.6".to_string(),
        home: "/tmp".to_string(),
        temp: "/tmp".to_string(),
        git: true,
    }
}

fn loopback_ssh_available() -> bool {
    std::process::Command::new("ssh")
        .args([
            "-o", "BatchMode=yes",
            "-o", "StrictHostKeyChecking=yes",
            "-o", "UpdateHostKeys=no",
            "-o", "ConnectTimeout=2",
            "127.0.0.1",
            "true",
        ])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

struct TestFixture {
    _dir: TempDir,
    root: PathBuf,
    project_dir: PathBuf,
    host_id: String,
    helper_bin: PathBuf,
    daemon_child: Option<std::process::Child>,
}

impl TestFixture {
    /// Spawns an owned Child daemon with immediate RAII guard construction.
    /// Readiness is observed via a bounded channel from a background reader thread.
    /// Any timeout or panic immediately kills the child to unblock the thread and prevent leaks.
    fn new(name: &str) -> Self {
        let helper_bin = helper_binary_path();
        let dir = tempfile::tempdir().expect("create tempdir");
        let root = dir.path().join("state");
        let project_dir = dir.path().join("project");
        std::fs::create_dir_all(&root).expect("create state dir");
        std::fs::create_dir_all(&project_dir).expect("create project dir");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700));
            let _ = std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700));
            let _ = std::fs::set_permissions(&project_dir, std::fs::Permissions::from_mode(0o700));
        }

        let host_id = format!("test-{name}-{}", uuid::Uuid::new_v4());

        // Spawn owned child daemon directly
        let mut child = std::process::Command::new(&helper_bin)
            .args([
                "daemon",
                "--root",
                root.to_str().unwrap(),
                "--host-id",
                &host_id,
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn owned helper daemon child");

        let stdout = child.stdout.take().expect("child stdout");

        // Construct RAII fixture immediately so any failure drops and reaps the child
        let fixture = Self {
            _dir: dir,
            root,
            project_dir,
            host_id,
            helper_bin,
            daemon_child: Some(child),
        };

        // Bounded channel to receive readiness from reader thread
        let (tx, rx) = std::sync::mpsc::channel();
        let reader_thread = std::thread::spawn(move || {
            let mut reader = std::io::BufReader::new(stdout);
            let mut line = String::new();
            let res = reader.read_line(&mut line).map(|_| line);
            let _ = tx.send(res);
        });

        // Bounded wait on readiness event (5 seconds)
        let ready_res = rx.recv_timeout(Duration::from_secs(5));
        match ready_res {
            Ok(Ok(line)) if line.contains("\"event\":\"ready\"") => {
                let _ = reader_thread.join();
                fixture
            }
            Ok(Ok(other_line)) => {
                drop(fixture);
                let _ = reader_thread.join();
                panic!("daemon output did not contain ready event: {other_line}");
            }
            Ok(Err(e)) => {
                drop(fixture);
                let _ = reader_thread.join();
                panic!("failed to read daemon readiness: {e}");
            }
            Err(e) => {
                // Timeout: dropping fixture kills the child, unblocking the reader thread
                drop(fixture);
                let _ = reader_thread.join();
                panic!("timed out waiting for daemon ready event: {e}");
            }
        }
    }

    fn spawn_bridge_connection(&self) -> BridgeConnection {
        let child = tokio::process::Command::new(&self.helper_bin)
            .args([
                "bridge",
                "--stdio",
                "--root",
                self.root.to_str().unwrap(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .expect("spawn test bridge process");
        BridgeConnection::from_child(child).expect("create BridgeConnection")
    }

    async fn create_client(&self) -> SshBridgeClient {
        let control = self.spawn_bridge_connection();
        let reader = self.spawn_bridge_connection();
        SshBridgeClient::from_connections(control, reader, &self.host_id)
            .await
            .expect("create SshBridgeClient")
    }
}

impl Drop for TestFixture {
    fn drop(&mut self) {
        if let Some(mut child) = self.daemon_child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[tokio::test]
async fn ssh_bridge_handshake_verifies_host_and_epoch() {
    let fixture = TestFixture::new("handshake");
    let mut conn = fixture.spawn_bridge_connection();
    let handshake = conn.handshake().await.expect("handshake");

    assert_eq!(handshake.protocol, 1);
    assert_eq!(handshake.host_id, fixture.host_id);
    assert!(handshake.capabilities.iter().any(|c| c == "sshHelperV1"));
    assert!(!handshake.owner_id.is_empty());
    assert!(handshake.epoch.0 > 0);

    conn.close().await.expect("close connection");
}

#[tokio::test]
async fn ssh_bridge_frame_framing_roundtrip_and_size_bounds() {
    // 1. Verify valid frame under 1 MiB round-trips
    let mut buffer = Vec::new();
    let test_val = json!({ "key": "value", "number": 42 });
    write_frame_async(&mut buffer, &test_val, Duration::from_secs(2))
        .await
        .expect("write_frame");

    assert!(buffer.len() > 4);
    let mut slice = &buffer[..];
    let decoded = read_frame_async(&mut slice, Duration::from_secs(2))
        .await
        .expect("read_frame");
    assert_eq!(decoded, Some(test_val));

    // 2. Verify frame > 1 MiB is rejected on write
    let huge_str = "x".repeat(MAX_FRAME + 1);
    let huge_val = json!({ "payload": huge_str });
    let mut sink = Vec::new();
    let write_err = write_frame_async(&mut sink, &huge_val, Duration::from_secs(2)).await;
    assert!(matches!(write_err, Err(BridgeError::FrameTooLarge(_))));

    // 3. Verify oversized length header is rejected on read
    let oversized_len: u32 = (MAX_FRAME as u32) + 10;
    let mut bad_header = oversized_len.to_be_bytes().to_vec();
    bad_header.extend_from_slice(&vec![0u8; 100]);
    let mut bad_slice = &bad_header[..];
    let read_err = read_frame_async(&mut bad_slice, Duration::from_secs(2)).await;
    assert!(matches!(read_err, Err(BridgeError::FrameTooLarge(_))));
}

#[tokio::test]
async fn ssh_bridge_target_expired_fails_explicitly_never_spawns() {
    let fixture = TestFixture::new("target-expired");
    let client = fixture.create_client().await;

    // 1. Host mismatch fails validation
    let wrong_host_target = TargetRef {
        host_id: "other-host".to_string(),
        owner_id: client.owner_id().to_string(),
        epoch: client.epoch(),
        backend_session_id: "session-1".to_string(),
    };
    let err = client.reattach(&wrong_host_target).await.unwrap_err();
    assert!(matches!(err, BridgeError::TargetExpired { .. }));

    // 2. Owner mismatch fails validation
    let wrong_owner_target = TargetRef {
        host_id: client.host_id().to_string(),
        owner_id: "wrong-owner-uuid".to_string(),
        epoch: client.epoch(),
        backend_session_id: "session-1".to_string(),
    };
    let err = client.reattach(&wrong_owner_target).await.unwrap_err();
    assert!(matches!(err, BridgeError::TargetExpired { .. }));

    // 3. Epoch mismatch fails validation
    let wrong_epoch_target = TargetRef {
        host_id: client.host_id().to_string(),
        owner_id: client.owner_id().to_string(),
        epoch: Epoch(client.epoch().0 + 999),
        backend_session_id: "session-1".to_string(),
    };
    let err = client.reattach(&wrong_epoch_target).await.unwrap_err();
    assert!(matches!(err, BridgeError::TargetExpired { .. }));

    // 4. Correct identity but non-existent backendSessionId returns TargetNotFound
    let missing_session_target = TargetRef {
        host_id: client.host_id().to_string(),
        owner_id: client.owner_id().to_string(),
        epoch: client.epoch(),
        backend_session_id: "nonexistent-session".to_string(),
    };
    let err = client.reattach(&missing_session_target).await.unwrap_err();
    assert!(matches!(err, BridgeError::TargetNotFound));

    client.close().await.expect("close client");
}

#[test]
fn ssh_bridge_handshake_independently_validates_configured_host_and_target_identity() {
    let handshake = HandshakeResult {
        protocol: 1,
        capabilities: vec!["sshHelperV1".into()],
        host_id: "host-live".into(),
        owner_id: "owner-live".into(),
        epoch: Epoch(12345),
        os: "linux".into(),
        arch: "x86_64".into(),
    };

    // 1. Configured host mismatch returns HostMismatch
    let err = validate_target_handshake(&handshake, "host-configured-different", None).unwrap_err();
    assert!(matches!(err, BridgeError::HostMismatch { .. }));

    // 2. Matching host without target passes
    assert!(validate_target_handshake(&handshake, "host-live", None).is_ok());

    // 3. Target with mismatched host returns TargetExpired
    let bad_host_target = TargetRef {
        host_id: "host-stale".into(),
        owner_id: "owner-live".into(),
        epoch: Epoch(12345),
        backend_session_id: "s1".into(),
    };
    let err = validate_target_handshake(&handshake, "host-live", Some(&bad_host_target)).unwrap_err();
    assert!(matches!(err, BridgeError::TargetExpired { .. }));

    // 4. Target with mismatched owner returns TargetExpired
    let bad_owner_target = TargetRef {
        host_id: "host-live".into(),
        owner_id: "owner-stale".into(),
        epoch: Epoch(12345),
        backend_session_id: "s1".into(),
    };
    let err = validate_target_handshake(&handshake, "host-live", Some(&bad_owner_target)).unwrap_err();
    assert!(matches!(err, BridgeError::TargetExpired { .. }));

    // 5. Target with mismatched epoch returns TargetExpired
    let bad_epoch_target = TargetRef {
        host_id: "host-live".into(),
        owner_id: "owner-live".into(),
        epoch: Epoch(99999),
        backend_session_id: "s1".into(),
    };
    let err = validate_target_handshake(&handshake, "host-live", Some(&bad_epoch_target)).unwrap_err();
    assert!(matches!(err, BridgeError::TargetExpired { .. }));

    // 6. Matching target passes
    let good_target = TargetRef {
        host_id: "host-live".into(),
        owner_id: "owner-live".into(),
        epoch: Epoch(12345),
        backend_session_id: "s1".into(),
    };
    assert!(validate_target_handshake(&handshake, "host-live", Some(&good_target)).is_ok());
}

#[tokio::test]
async fn ssh_bridge_target_mismatch_on_describe_or_read_is_rejected() {
    let fixture = TestFixture::new("target-mismatch");
    let client = fixture.create_client().await;

    client
        .project_register("proj-mismatch", fixture.project_dir.to_str().unwrap())
        .await
        .expect("register project");

    let spawned = client
        .pty_spawn(&SpawnParams {
            project_id: "proj-mismatch".into(),
            worktree: Some(".".into()),
            cols: Some(80),
            rows: Some(24),
            program: None,
            args: None,
            env: None,
            client_request_id: Some("req-mismatch-test".into()),
        })
        .await
        .expect("pty_spawn");

    let mut conn = fixture.spawn_bridge_connection();
    let _ = conn.handshake().await.expect("handshake");

    // Attempt describe with a target whose backendSessionId is mismatched
    let mismatched_target = TargetRef {
        host_id: spawned.target.host_id.clone(),
        owner_id: spawned.target.owner_id.clone(),
        epoch: spawned.target.epoch,
        backend_session_id: "different-id".into(),
    };

    let desc_err = conn.pty_describe(&mismatched_target).await.unwrap_err();
    assert!(matches!(desc_err, BridgeError::TargetNotFound));

    conn.close().await.expect("close connection");
    client.pty_stop(&spawned.target).await.expect("pty_stop");
    client.close().await.expect("close client");
}

#[tokio::test]
async fn ssh_bridge_independent_read_does_not_block_control() {
    let fixture = TestFixture::new("independent-streams");
    let client = fixture.create_client().await;

    client
        .project_register("proj-1", fixture.project_dir.to_str().unwrap())
        .await
        .expect("register project");

    #[cfg(unix)]
    let program = Some("/bin/cat".to_string());
    #[cfg(windows)]
    let program = None;

    let spawned = client
        .pty_spawn(&SpawnParams {
            project_id: "proj-1".into(),
            worktree: Some(".".into()),
            cols: Some(80),
            rows: Some(24),
            program,
            args: Some(vec![]),
            env: None,
            client_request_id: Some("req-stream-test".into()),
        })
        .await
        .expect("pty_spawn");

    let target = spawned.target;

    // In a background task, issue a long-poll pty_read with 4000ms wait
    let client_for_read = client.clone();
    let target_for_read = target.clone();
    let read_handle = tokio::spawn(async move {
        client_for_read
            .pty_read(&target_for_read, RemoteCursor(0), 4000)
            .await
    });

    tokio::task::yield_now().await;

    let start = std::time::Instant::now();
    let write_res = client
        .pty_write(&target, b"hello-independent-channel\n")
        .await;
    let elapsed = start.elapsed();

    assert!(write_res.is_ok(), "write must succeed: {write_res:?}");
    assert!(
        elapsed < Duration::from_millis(500),
        "write took {elapsed:?}, should have completed without waiting for read"
    );

    let read_res = read_handle.await.expect("join read task").expect("pty_read");
    let decoded = read_res.decoded_bytes();
    let text = String::from_utf8_lossy(&decoded);
    assert!(
        text.contains("hello-independent-channel"),
        "expected echo in read chunks, got: {text}"
    );

    // Cursor must be canonical u64
    assert!(read_res.cursor.as_u64() > 0);

    client.pty_stop(&target).await.expect("pty_stop");
    client.close().await.expect("close client");
}

#[tokio::test]
async fn ssh_bridge_remote_pid_does_not_expose_signal_target() {
    let pid = RemotePid(99999);
    assert_eq!(pid.as_u32(), 99999);
    assert_eq!(format!("{pid}"), "99999");

    let json_val = serde_json::to_value(&pid).expect("serialize");
    assert_eq!(json_val, json!(99999));

    let deserialized: RemotePid = serde_json::from_value(json_val).expect("deserialize");
    assert_eq!(deserialized, pid);
}

#[tokio::test]
async fn ssh_bridge_cursor_canonical_decimal_u64_parsing() {
    let c = RemoteCursor(12345);
    assert_eq!(c.as_u64(), 12345);
    assert_eq!(*c, 12345);
    assert_eq!(format!("{c}"), "12345");

    // Deserialization accepts canonical string
    let parsed: RemoteCursor = serde_json::from_str("\"12345\"").expect("parse canonical str");
    assert_eq!(parsed, c);

    // Deserialization accepts non-negative integer
    let parsed_int: RemoteCursor = serde_json::from_str("12345").expect("parse int");
    assert_eq!(parsed_int, c);

    // Rejects non-canonical or negative strings
    assert!(serde_json::from_str::<RemoteCursor>("\"0123\"").is_err());
    assert!(serde_json::from_str::<RemoteCursor>("\"-1\"").is_err());
    assert!(serde_json::from_str::<RemoteCursor>("\"abc\"").is_err());
    assert!(serde_json::from_str::<RemoteCursor>("-5").is_err());
}

#[tokio::test]
async fn ssh_bridge_spawn_retry_requires_matching_client_request_id() {
    let fixture = TestFixture::new("spawn-retry");
    let client = fixture.create_client().await;

    client
        .project_register("proj-retry", fixture.project_dir.to_str().unwrap())
        .await
        .expect("register project");

    let params = SpawnParams {
        project_id: "proj-retry".into(),
        worktree: Some(".".into()),
        cols: Some(80),
        rows: Some(24),
        program: None,
        args: None,
        env: None,
        client_request_id: Some("req-stable-123".into()),
    };

    let first = client.pty_spawn(&params).await.expect("first spawn");

    let second = client
        .pty_spawn_retry(&params, "req-stable-123")
        .await
        .expect("retry spawn");

    assert_eq!(first.target, second.target);
    assert_eq!(first.pid, second.pid);

    let mismatch_err = client
        .pty_spawn_retry(&params, "wrong-req-id")
        .await
        .unwrap_err();
    assert!(matches!(mismatch_err, BridgeError::Protocol(_)));

    let mut conflict_params = params.clone();
    conflict_params.cols = Some(120);
    let conflict_err = client.pty_spawn(&conflict_params).await.unwrap_err();
    assert!(
        matches!(conflict_err, BridgeError::Remote(ref msg) if msg.contains("REQUEST_CONFLICT")),
        "expected REQUEST_CONFLICT, got {conflict_err:?}"
    );

    client.pty_stop(&first.target).await.expect("pty_stop");
    client.close().await.expect("close client");
}

#[tokio::test]
async fn ssh_bridge_single_attempt_write_does_not_queue() {
    let fixture = TestFixture::new("single-attempt");
    let client = fixture.create_client().await;

    let fake_target = TargetRef {
        host_id: client.host_id().to_string(),
        owner_id: client.owner_id().to_string(),
        epoch: client.epoch(),
        backend_session_id: "fake-session".to_string(),
    };

    client.close().await.expect("close client");

    let write_err = client.pty_write(&fake_target, b"fail").await.unwrap_err();
    assert!(matches!(write_err, BridgeError::ConnectionClosed));

    let stop_err = client.pty_stop(&fake_target).await.unwrap_err();
    assert!(matches!(stop_err, BridgeError::ConnectionClosed));
}

#[tokio::test]
async fn ssh_bridge_lifecycle_poison_on_timeout_cancel_and_eof_reaping() {
    let fixture = TestFixture::new("lifecycle-reaping");

    // Spawn a real session so pty.read legitimately blocks on the remote helper
    let client = fixture.create_client().await;
    client
        .project_register("proj-life", fixture.project_dir.to_str().unwrap())
        .await
        .expect("register project");

    #[cfg(unix)]
    let program = Some("/bin/cat".to_string());
    #[cfg(windows)]
    let program = None;

    let spawned = client
        .pty_spawn(&SpawnParams {
            project_id: "proj-life".into(),
            worktree: Some(".".into()),
            cols: Some(80),
            rows: Some(24),
            program,
            args: Some(vec![]),
            env: None,
            client_request_id: Some("req-life-test".into()),
        })
        .await
        .expect("pty_spawn");

    // Part A: Timeout poisons connection, preventing subsequent writes/misassociations
    let mut conn = fixture.spawn_bridge_connection();
    let _ = conn.handshake().await.expect("handshake");

    // Trigger a timeout on a request that blocks waiting for cat output (waitMs = 5000, client timeout = 20ms)
    let timeout_err = conn
        .request(
            "pty.read",
            json!({
                "target": spawned.target,
                "cursor": "1",
                "waitMs": 5000
            }),
            Duration::from_millis(20),
        )
        .await
        .unwrap_err();

    assert!(
        matches!(timeout_err, BridgeError::Timeout(_)),
        "expected Timeout, got: {timeout_err:?}"
    );

    // Next request must fail immediately with ConnectionClosed without writing/reading
    let next_err = conn
        .request("handshake", json!({}), Duration::from_secs(2))
        .await
        .unwrap_err();
    assert!(matches!(next_err, BridgeError::ConnectionClosed));

    // Calling close on a poisoned connection is idempotent and reaps child
    conn.close().await.expect("close poisoned connection");

    // Part B: Cancelled in-flight request then drop reaps child process
    let mut conn_cancel = fixture.spawn_bridge_connection();
    let _ = conn_cancel.handshake().await.expect("handshake");
    let child_pid = conn_cancel.child_id().expect("child PID must be known");

    // Start an in-flight request and cancel it mid-flight by dropping the future
    {
        let cancel_future = conn_cancel.request(
            "pty.read",
            json!({ "target": spawned.target, "cursor": "1", "waitMs": 5000 }),
            Duration::from_secs(10),
        );
        tokio::select! {
            _ = cancel_future => panic!("request should not complete"),
            _ = tokio::time::sleep(Duration::from_millis(20)) => {},
        }
    }

    // Next request must fail immediately because connection was poisoned before I/O
    let next_err = conn_cancel
        .request("handshake", json!({}), Duration::from_secs(1))
        .await
        .unwrap_err();
    assert!(matches!(next_err, BridgeError::ConnectionClosed));

    // Dropping conn_cancel must kill and reap the child process
    drop(conn_cancel);

    // Verify child process was reaped and is not lingering
    #[cfg(unix)]
    {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let mut reaped = false;
        while std::time::Instant::now() < deadline {
            let res = std::process::Command::new("ps")
                .args(["-p", &child_pid.to_string(), "-o", "pid="])
                .output();
            if let Ok(output) = res {
                if !output.status.success() || output.stdout.trim_ascii().is_empty() {
                    reaped = true;
                    break;
                }
            }
            tokio::task::yield_now().await;
        }
        assert!(reaped, "child PID {child_pid} must be reaped after drop");
    }

    // Part C: Closed-on-EOF then close cleans up idempotently without hanging
    let mut conn_eof = fixture.spawn_bridge_connection();
    let _ = conn_eof.handshake().await.expect("handshake");
    let eof_pid = conn_eof.child_id().expect("eof child PID");

    // Close stdin of the child to trigger EOF on stdout
    drop(conn_eof.writer.take());

    // Next request will hit EOF on read
    let eof_err = conn_eof
        .request("handshake", json!({}), Duration::from_secs(2))
        .await
        .unwrap_err();
    assert!(
        matches!(eof_err, BridgeError::ProcessExited { .. } | BridgeError::ConnectionClosed),
        "expected ProcessExited or ConnectionClosed, got: {eof_err:?}"
    );

    // close() after EOF must be idempotent and cleanly reap child without hanging
    conn_eof.close().await.expect("close after EOF");

    #[cfg(unix)]
    {
        let res = std::process::Command::new("ps")
            .args(["-p", &eof_pid.to_string(), "-o", "pid="])
            .output();
        if let Ok(output) = res {
            assert!(
                !output.status.success() || output.stdout.trim_ascii().is_empty(),
                "child PID {eof_pid} must be reaped after close"
            );
        }
    }

    client.pty_stop(&spawned.target).await.expect("pty_stop");
    client.close().await.expect("close client");
}

#[tokio::test]
async fn ssh_bridge_live_loopback_openssh_connection() {
    assert!(
        loopback_ssh_available(),
        "Loopback SSH must be available for live OpenSSH bridge test"
    );

    // Use owned daemon fixture; ensure_started preserves the already-live owned daemon,
    // avoiding detached process spawning or unowned daemon lifetimes.
    let fixture = TestFixture::new("live-loopback");
    let host = sample_host(&fixture.host_id);
    let env = sample_env();
    let location = HelperLocation {
        executable: fixture.helper_bin.to_str().unwrap().to_string(),
        root: fixture.root.to_str().unwrap().to_string(),
    };

    let client = SshBridgeClient::connect(&host, &env, &location)
        .await
        .expect("connect SshBridgeClient over OpenSSH");

    assert_eq!(client.host_id(), fixture.host_id);
    let handshake = client.handshake_info();
    assert_eq!(handshake.protocol, 1);
    assert!(handshake.capabilities.iter().any(|c| c == "sshHelperV1"));

    // Register project over SSH bridge
    client
        .project_register("proj-ssh", fixture.project_dir.to_str().unwrap())
        .await
        .expect("register project over SSH");

    // Spawn PTY over SSH bridge
    let spawn_res = client
        .pty_spawn(&SpawnParams {
            project_id: "proj-ssh".into(),
            worktree: Some(".".into()),
            cols: Some(80),
            rows: Some(24),
            program: None,
            args: None,
            env: None,
            client_request_id: Some("req-ssh-live".into()),
        })
        .await
        .expect("pty_spawn over SSH");

    let describe = client
        .reattach(&spawn_res.target)
        .await
        .expect("reattach over SSH");
    assert_eq!(describe.pid, spawn_res.pid);

    client.pty_stop(&spawn_res.target).await.expect("pty_stop over SSH");
    client.close().await.expect("close client");

    // Dropping fixture reaps the owned daemon_child via RAII
}
