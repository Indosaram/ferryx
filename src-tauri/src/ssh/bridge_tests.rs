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
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=yes",
            "-o",
            "UpdateHostKeys=no",
            "-o",
            "ConnectTimeout=2",
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
            .args(["bridge", "--stdio", "--root", self.root.to_str().unwrap()])
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
    let err =
        validate_target_handshake(&handshake, "host-live", Some(&bad_host_target)).unwrap_err();
    assert!(matches!(err, BridgeError::TargetExpired { .. }));

    // 4. Target with mismatched owner returns TargetExpired
    let bad_owner_target = TargetRef {
        host_id: "host-live".into(),
        owner_id: "owner-stale".into(),
        epoch: Epoch(12345),
        backend_session_id: "s1".into(),
    };
    let err =
        validate_target_handshake(&handshake, "host-live", Some(&bad_owner_target)).unwrap_err();
    assert!(matches!(err, BridgeError::TargetExpired { .. }));

    // 5. Target with mismatched epoch returns TargetExpired
    let bad_epoch_target = TargetRef {
        host_id: "host-live".into(),
        owner_id: "owner-live".into(),
        epoch: Epoch(99999),
        backend_session_id: "s1".into(),
    };
    let err =
        validate_target_handshake(&handshake, "host-live", Some(&bad_epoch_target)).unwrap_err();
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

    let read_res = read_handle
        .await
        .expect("join read task")
        .expect("pty_read");
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
        matches!(
            eof_err,
            BridgeError::ProcessExited { .. } | BridgeError::ConnectionClosed
        ),
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

    client
        .pty_stop(&spawn_res.target)
        .await
        .expect("pty_stop over SSH");
    client.close().await.expect("close client");

    // Dropping fixture reaps the owned daemon_child via RAII
}

#[tokio::test]
async fn test_ssh_bridge_dag_inventory_and_poll() {
    let fixture = TestFixture::new("dag");
    let client = fixture.create_client().await;

    assert!(client.supports_dag());

    client
        .project_register("proj-dag", fixture.project_dir.to_str().unwrap())
        .await
        .expect("register project");

    let runs_dir = fixture.project_dir.join(".omo/senpi-task/dag/runs");
    std::fs::create_dir_all(&runs_dir).expect("create runs dir");
    let run_file = runs_dir.join("run-test-1.json");
    let mut run_json: Value = serde_json::from_str(include_str!(
        "../dag/testdata/dag_081e597f-0aa8-4a20-a826-4e3d045aacef.json"
    ))
    .expect("valid DAG checkpoint fixture");
    run_json["runId"] = json!("run-dag-ssh-1");
    run_json["status"] = json!("running");
    std::fs::write(&run_file, serde_json::to_string(&run_json).unwrap()).expect("write run file");

    let runs = client
        .dag_inventory("proj-dag")
        .await
        .expect("query dag inventory");
    assert_eq!(runs.len(), 1);
    // The helper streams raw journal text; only the journal parser normalizes
    // optional fields (e.g. amendCount), so the desktop contract is
    // parse_run_checkpoint, never a bare serde decode.
    let snapshot = crate::dag::journal::parse_run_checkpoint(&runs[0].to_string())
        .expect("desktop parses raw helper checkpoint via parse_run_checkpoint");
    assert_eq!(snapshot.status, crate::dag::journal::DagRunStatus::Running);
    assert_eq!(snapshot.run_id, "run-dag-ssh-1");
    assert!(
        serde_json::from_value::<crate::dag::journal::DagRunSnapshot>(runs[0].clone()).is_err(),
        "raw checkpoints are not directly decodable; the parser contract is required"
    );

    let (mtime, new_runs) = client.dag_poll("proj-dag", 0, &[]).await.expect("dag poll");
    assert!(mtime > 0);
    assert_eq!(new_runs.len(), 1);

    let (mtime2, no_runs) = client
        .dag_poll("proj-dag", mtime, &new_runs)
        .await
        .expect("dag poll with known");
    assert_eq!(mtime2, mtime);
    assert!(no_runs.is_empty());

    let original_time = std::fs::metadata(&run_file).unwrap().modified().unwrap();
    run_json["status"] = json!("completed");
    std::fs::write(&run_file, serde_json::to_vec(&run_json).unwrap()).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&run_file)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(original_time))
        .unwrap();
    let (_, changed_runs) = client
        .dag_poll("proj-dag", mtime, &new_runs)
        .await
        .expect("same-timestamp changed checkpoint crosses real bridge");
    assert_eq!(changed_runs.len(), 1);
    let changed = crate::dag::journal::parse_run_checkpoint(&changed_runs[0].to_string())
        .expect("changed checkpoint parses through parse_run_checkpoint");
    assert_eq!(changed.status, crate::dag::journal::DagRunStatus::Completed);

    client.close().await.expect("close client");
}

/// Real child helper process, real SSH-shaped bridge connections: the desktop
/// opens a helper-owned subscription and receives inventory plus asynchronous
/// updates without ever sending a known-run set or driving a poll loop.
#[tokio::test]
async fn ssh_bridge_dag_subscription_streams_inventory_updates_and_cancels() {
    let fixture = TestFixture::new("dag-stream");
    let client = fixture.create_client().await;
    assert!(
        client.supports_dag_stream(),
        "helper must advertise dagSubscribeV1"
    );

    client
        .project_register("proj-stream", fixture.project_dir.to_str().unwrap())
        .await
        .expect("register project");

    let runs_dir = fixture.project_dir.join(".omo/senpi-task/dag/runs");
    std::fs::create_dir_all(&runs_dir).expect("create runs dir");
    let run_file = runs_dir.join("run-stream.json");
    let mut run_json: Value = serde_json::from_str(include_str!(
        "../dag/testdata/dag_081e597f-0aa8-4a20-a826-4e3d045aacef.json"
    ))
    .expect("valid DAG checkpoint fixture");
    run_json["runId"] = json!("run-stream-1");
    run_json["status"] = json!("running");
    std::fs::write(&run_file, serde_json::to_vec(&run_json).unwrap()).expect("write checkpoint");

    // 1. Inventory arrives on the subscribe frame itself.
    let (mut subscription, inventory) = client
        .dag_subscribe_on(fixture.spawn_bridge_connection(), "proj-stream")
        .await
        .expect("open dag subscription");
    assert!(inventory.resync, "first frame must be a full inventory");
    assert_eq!(inventory.runs.len(), 1);
    assert!(inventory.dropped.is_empty());
    let hydrated = crate::dag::journal::parse_run_checkpoint(&inventory.runs[0].to_string())
        .expect("stream frames carry raw checkpoints for parse_run_checkpoint");
    assert_eq!(hydrated.run_id, "run-stream-1");
    assert_eq!(hydrated.status, crate::dag::journal::DagRunStatus::Running);

    // 2. A same-mtime content change is delivered asynchronously, with no
    //    knownRuns payload from the desktop.
    let original_time = std::fs::metadata(&run_file).unwrap().modified().unwrap();
    run_json["status"] = json!("completed");
    std::fs::write(&run_file, serde_json::to_vec(&run_json).unwrap()).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&run_file)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(original_time))
        .unwrap();
    assert_eq!(
        std::fs::metadata(&run_file).unwrap().modified().unwrap(),
        original_time,
        "fixture must preserve the original mtime"
    );

    let update = subscription
        .next_frame(Duration::from_secs(5))
        .await
        .expect("same-mtime update crosses the real bridge");
    assert!(!update.resync);
    assert_eq!(update.runs.len(), 1, "expected exactly the changed run");
    assert!(
        update.sequence > inventory.sequence,
        "sequence must advance"
    );
    let changed = crate::dag::journal::parse_run_checkpoint(&update.runs[0].to_string())
        .expect("update frame parses through parse_run_checkpoint");
    assert_eq!(changed.status, crate::dag::journal::DagRunStatus::Completed);

    // 3. An unchanged rewrite is deduplicated by the helper.
    std::fs::write(&run_file, serde_json::to_vec(&run_json).unwrap()).unwrap();
    let idle = subscription
        .next_frame(Duration::from_millis(600))
        .await
        .expect("idle frame");
    assert!(
        idle.runs.is_empty(),
        "byte-identical rewrite must not be redelivered: {idle:?}"
    );
    assert!(!idle.closed);

    // 4. Explicit cancellation releases the subscription and its connection.
    subscription.unsubscribe().await.expect("unsubscribe");

    client.close().await.expect("close client");
}

/// A subscription must never be opened for a project the helper has not
/// registered, even when the caller passes a real remote directory path.
#[tokio::test]
async fn ssh_bridge_dag_subscription_rejects_unregistered_project() {
    let fixture = TestFixture::new("dag-stream-unregistered");
    let client = fixture.create_client().await;

    let err = client
        .dag_subscribe_on(
            fixture.spawn_bridge_connection(),
            fixture.project_dir.to_str().unwrap(),
        )
        .await
        .expect_err("unregistered project must be rejected");
    assert!(
        matches!(err, BridgeError::TargetNotFound),
        "expected NOT_FOUND, got {err:?}"
    );

    client.close().await.expect("close client");
}

/// Dropping the subscription (the cancellation path used while connecting,
/// registering, or waiting) must release the helper-side subscription without
/// touching PTY lifecycle.
#[tokio::test]
async fn ssh_bridge_dag_subscription_drop_releases_helper_and_keeps_pty_alive() {
    let fixture = TestFixture::new("dag-stream-drop");
    let client = fixture.create_client().await;

    client
        .project_register("proj-drop", fixture.project_dir.to_str().unwrap())
        .await
        .expect("register project");
    std::fs::create_dir_all(fixture.project_dir.join(".omo/senpi-task/dag/runs"))
        .expect("create runs dir");

    let spawn_res = client
        .pty_spawn(&SpawnParams {
            project_id: "proj-drop".into(),
            worktree: Some(".".into()),
            cols: Some(80),
            rows: Some(24),
            program: None,
            args: None,
            env: None,
            client_request_id: Some("req-dag-drop".into()),
        })
        .await
        .expect("pty_spawn");

    let (subscription, first) = client
        .dag_subscribe_on(fixture.spawn_bridge_connection(), "proj-drop")
        .await
        .expect("open dag subscription");
    let subscription_id = subscription.id().to_string();
    drop(subscription);

    // The helper released the subscription: a later reference is unknown.
    let mut probe = fixture.spawn_bridge_connection();
    let mut released = false;
    for _ in 0..50 {
        match probe
            .request(
                "dag.next",
                json!({ "subscriptionId": subscription_id, "waitMs": 100 }),
                Duration::from_secs(5),
            )
            .await
        {
            Err(BridgeError::TargetNotFound) => {
                released = true;
                break;
            }
            Ok(_) => continue,
            Err(e) => panic!("unexpected probe error: {e:?}"),
        }
    }
    probe.close().await.expect("close probe");
    assert!(
        released,
        "dropping the subscription must release it on the helper (id {subscription_id}, first seq {})",
        first.sequence
    );

    // The PTY is untouched by DAG subscription teardown.
    let describe = client
        .reattach(&spawn_res.target)
        .await
        .expect("PTY survives DAG subscription teardown");
    assert_eq!(describe.pid, spawn_res.pid);
    assert!(!describe.exited, "DAG teardown must not kill the PTY");

    client.pty_stop(&spawn_res.target).await.expect("pty_stop");
    client.close().await.expect("close client");
}

/// Frame-ordering contract at the desktop parser seam: contiguous frames are
/// accepted, duplicates and regressions are rejected, and a forward gap is only
/// legal when the frame re-establishes the current inventory (`resync`).
#[tokio::test]
async fn ssh_bridge_dag_frame_ordering_requires_contiguity_or_resync() {
    let fixture = TestFixture::new("dag-stream-order");
    let client = fixture.create_client().await;
    client
        .project_register("proj-order", fixture.project_dir.to_str().unwrap())
        .await
        .expect("register project");
    std::fs::create_dir_all(fixture.project_dir.join(".omo/senpi-task/dag/runs"))
        .expect("create runs dir");

    let (mut subscription, first) = client
        .dag_subscribe_on(fixture.spawn_bridge_connection(), "proj-order")
        .await
        .expect("open dag subscription");
    let id = subscription.id().to_string();
    let base = first.sequence;

    let frame = |sequence: u64, resync: bool| DagFrame {
        subscription_id: id.clone(),
        sequence,
        runs: Vec::new(),
        dropped: Vec::new(),
        resync,
        more: false,
        closed: false,
    };

    // Contiguous: accepted, cursor advances.
    subscription
        .accept_frame_for_test(frame(base + 1, false))
        .expect("contiguous frame is accepted");
    assert_eq!(subscription.last_sequence(), base + 1);

    // Duplicate and regression: rejected, cursor unchanged.
    for stale in [base + 1, base] {
        let err = subscription
            .accept_frame_for_test(frame(stale, false))
            .expect_err("stale frame must be rejected");
        assert!(
            matches!(&err, BridgeError::Protocol(m) if m.contains("regressed")),
            "expected regression rejection, got {err:?}"
        );
    }
    assert_eq!(subscription.last_sequence(), base + 1);

    // Forward gap without resync: rejected (design 4.5 forbids silently
    // applying frames after a loss).
    let err = subscription
        .accept_frame_for_test(frame(base + 5, false))
        .expect_err("gap without resync must be rejected");
    assert!(
        matches!(&err, BridgeError::Protocol(m) if m.contains("gap")),
        "expected gap rejection, got {err:?}"
    );
    assert_eq!(subscription.last_sequence(), base + 1);

    // Same gap carrying a resync inventory: accepted.
    subscription
        .accept_frame_for_test(frame(base + 5, true))
        .expect("resync frame may close a gap");
    assert_eq!(subscription.last_sequence(), base + 5);

    // A frame for a different subscription is never accepted.
    let mut foreign = frame(base + 6, false);
    foreign.subscription_id = format!("{id}-other");
    let err = subscription
        .accept_frame_for_test(foreign)
        .expect_err("foreign subscription frame must be rejected");
    assert!(
        matches!(&err, BridgeError::Protocol(m) if m.contains("foreign subscription")),
        "expected foreign-subscription rejection, got {err:?}"
    );

    subscription.unsubscribe().await.expect("unsubscribe");
    client.close().await.expect("close client");
}

/// A consumer that dies while parked inside a blocking `dag.next` must still be
/// cleaned up. The helper observes EOF only when the blocked call returns, so
/// release is bounded by the requested wait, not unbounded.
#[tokio::test]
async fn ssh_bridge_dag_subscription_released_after_eof_during_blocked_next() {
    let fixture = TestFixture::new("dag-stream-eof");
    let client = fixture.create_client().await;

    client
        .project_register("proj-eof", fixture.project_dir.to_str().unwrap())
        .await
        .expect("register project");
    std::fs::create_dir_all(fixture.project_dir.join(".omo/senpi-task/dag/runs"))
        .expect("create runs dir");

    let (mut subscription, _first) = client
        .dag_subscribe_on(fixture.spawn_bridge_connection(), "proj-eof")
        .await
        .expect("open dag subscription");
    let subscription_id = subscription.id().to_string();

    // Park a real blocking dag.next on the helper, then kill the consumer.
    let (parked_tx, parked_rx) = tokio::sync::oneshot::channel::<()>();
    let waiter = tokio::spawn(async move {
        let _ = parked_tx.send(());
        let _ = subscription.next_frame(Duration::from_secs(2)).await;
        subscription
    });
    parked_rx.await.expect("waiter started");
    waiter.abort();
    let _ = waiter.await;

    let mut probe = fixture.spawn_bridge_connection();
    let mut released = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while tokio::time::Instant::now() < deadline {
        match probe
            .request(
                "dag.next",
                json!({ "subscriptionId": subscription_id, "waitMs": 200 }),
                Duration::from_secs(10),
            )
            .await
        {
            Err(BridgeError::TargetNotFound) => {
                released = true;
                break;
            }
            Ok(_) => continue,
            Err(e) => panic!("unexpected probe error: {e:?}"),
        }
    }
    probe.close().await.expect("close probe");
    assert!(
        released,
        "helper must release subscription {subscription_id} after consumer EOF"
    );

    client.close().await.expect("close client");
}

#[cfg(unix)]
#[tokio::test]
async fn ssh_bridge_real_pty_large_session_writes_do_not_block_other_session() {
    let fixture = TestFixture::new("backpressure-iso");
    let client = fixture.create_client().await;
    client
        .project_register("proj-iso", fixture.project_dir.to_str().unwrap())
        .await
        .expect("register project");

    let program = Some("/bin/cat".to_string());
    async fn spawn_one(
        client: &SshBridgeClient,
        program: &Option<String>,
        rid: &'static str,
    ) -> SpawnResult {
        client
            .pty_spawn(&SpawnParams {
                project_id: "proj-iso".into(),
                worktree: Some(".".into()),
                cols: Some(80),
                rows: Some(24),
                program: program.clone(),
                args: Some(vec![]),
                env: None,
                client_request_id: Some(rid.into()),
            })
            .await
            .expect("pty_spawn")
    }
    let session_a = spawn_one(&client, &program, "req-iso-a").await;
    let session_b = spawn_one(&client, &program, "req-iso-b").await;

    let mut conn_a = fixture.spawn_bridge_connection();
    let _ = conn_a.handshake().await.expect("handshake a");
    let mut conn_b = fixture.spawn_bridge_connection();
    let _ = conn_b.handshake().await.expect("handshake b");

    // Queue a burst of large line-terminated writes on session A's own connection
    // (no reader drains A yet). The claim under test is cross-session non-blockage:
    // session B's write and stream must complete while A's burst is being serviced.
    // The burst handle is awaited before close so no write outlives the test body.
    let (first_write_in_flight, first_write_in_flight_rx) = tokio::sync::oneshot::channel();
    let saturate = {
        let target = session_a.target.clone();
        tokio::spawn(async move {
            let mut first = vec![b'x'; 3900];
            first.extend_from_slice(b"-L0\n");
            let first_ok = conn_a.pty_write(&target, &first).await.is_ok();
            let _ = first_write_in_flight.send(first_ok);
            for k in 1..8 {
                let mut chunk = vec![b'x'; 3900];
                chunk.extend_from_slice(format!("-L{k}\n").as_bytes());
                if conn_a.pty_write(&target, &chunk).await.is_err() {
                    break;
                }
            }
            conn_a
        })
    };
    // Deterministic contention: session A's first large write must actually be in
    // flight (or have failed) before session B proceeds, so the cross-session
    // claim below can no longer pass by scheduling luck.
    let first_ok = first_write_in_flight_rx.await.unwrap_or(false);
    assert!(first_ok, "session A's first large write must succeed");

    // Session B must stay fully interactive while A's burst is in flight or queued.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    conn_b
        .pty_write(&session_b.target, b"b-ping\n")
        .await
        .expect("write b while a burst is in flight");
    let mut transcript_b: Vec<u8> = Vec::new();
    let mut cursor_b = RemoteCursor(0);
    while tokio::time::Instant::now() < deadline {
        if transcript_b.windows(6).any(|w| w == b"b-ping") {
            break;
        }
        let read = conn_b
            .pty_read(&session_b.target, cursor_b, 400)
            .await
            .expect("read b while a burst is in flight");
        cursor_b = read.cursor;
        transcript_b.extend(read.decoded_bytes());
    }
    assert!(
        transcript_b.windows(6).any(|w| w == b"b-ping"),
        "session B output must stream while session A services large writes"
    );

    // Join the burst task before closing so every A write is settled.
    let _conn_a = saturate.await.expect("burst task");

    client.close().await.expect("close client");
}
