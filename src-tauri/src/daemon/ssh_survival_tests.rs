use super::*;

#[tokio::test]
async fn ssh_daemon_restart_restores_identity_without_local_pty() {
    let dir = tempfile::tempdir().unwrap();
    let server = DaemonServer::new_with_paths(
        Some(dir.path().join("config")),
        Some(dir.path().join("auth")),
    );
    let descriptor: crate::terminal::remote::RemoteSessionDescriptor = serde_json::from_value(serde_json::json!({
        "backendSessionId":"original-pane-backend", "target":{"hostId":"host","ownerId":"owner","epoch":"1","backendSessionId":"original-target"},
        "config":{"host":{"id":"host","label":"host","hostname":"127.0.0.1","port":1,"source":"manual","authMethod":"agent"},"environment":{"platform":"posix","executor":"sh","version":"test","home":"/tmp","temp":"/tmp","git":true},"helper":{"executable":"/helper","root":"/root"},"projectId":"ssh:abcd","projectPath":"/project","worktree":null,"agentIdentity":null},
        "clientRequestId":"original-request","remoteCursor":"73","cols":80,"rows":24
    })).unwrap();
    let path = dir.path().join("identities.json");
    server
        .terminal_service
        .remote()
        .restore(descriptor.clone())
        .unwrap();
    server
        .persist_remote_sessions_at(path.clone())
        .await
        .unwrap();
    drop(server);
    let restored = DaemonServer::new_with_paths(
        Some(dir.path().join("config2")),
        Some(dir.path().join("auth2")),
    );
    restored.restore_remote_sessions_at(path).await.unwrap();
    assert!(restored
        .session_router
        .is_local_session("original-pane-backend"));
    assert!(restored
        .terminal_service
        .get_session("original-pane-backend")
        .is_none());
    let mut replay_descriptor = descriptor.clone();
    replay_descriptor.remote_cursor = crate::ssh::bridge::RemoteCursor(0);
    assert_eq!(
        restored
            .terminal_service
            .remote()
            .details("original-pane-backend")
            .unwrap()
            .descriptor,
        replay_descriptor
    );
    restored
        .terminal_service
        .output_hub()
        .publish("original-pane-backend", b"retained replay".to_vec());
    assert!(matches!(
        restored.handle_describe_session("original-pane-backend"),
        DaemonResponse::DescribeSessionOk { .. }
    ));
    let restored = Arc::new(restored);
    let (client, stream) = tokio::io::duplex(32 * 1024);
    let serving = tokio::spawn(restored.clone().handle_client(stream));
    let (read, mut write) = tokio::io::split(client);
    let mut lines = BufReader::new(read).lines();
    write
        .write_all(b"{\"type\":\"remoteSessionDetails\",\"sessionId\":\"original-pane-backend\"}\n")
        .await
        .unwrap();
    let line = tokio::time::timeout(Duration::from_secs(3), lines.next_line())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let reply: DaemonResponse = serde_json::from_str(&line).unwrap();
    assert!(matches!(
        reply,
        DaemonResponse::RemoteSessionDetailsOk {
            details: Some(_),
            legacy_direct_ssh: false
        }
    ));
    write.write_all(b"{\"type\":\"attach\",\"sessionId\":\"original-pane-backend\",\"afterSequence\":999}\n").await.unwrap();
    let line = tokio::time::timeout(Duration::from_secs(3), lines.next_line())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(
        matches!(serde_json::from_str::<DaemonResponse>(&line).unwrap(), DaemonResponse::AttachOk { gap: Some(_), history, .. } if history.as_ref() == b"retained replay")
    );
    let line = tokio::time::timeout(Duration::from_secs(3), lines.next_line())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(matches!(
        serde_json::from_str::<DaemonStreamMessage>(&line).unwrap(),
        DaemonStreamMessage::RemoteStatus { .. }
    ));
    serving.abort();
    assert!(serving.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn ssh_reconnect_safety_gap_precedes_recovered_output() {
    let hub = Arc::new(TerminalOutputHub::default());
    let (_, rx) = hub.register_session_channels("gap-session");
    let (client, server) = tokio::io::duplex(32 * 1024);
    let pump = tokio::spawn(DaemonServer::pump_sequenced_stream(
        "gap-session".into(),
        rx,
        hub.clone(),
        server,
    ));
    hub.publish("gap-session", b"before".to_vec());
    hub.publish_gap("gap-session");
    hub.publish("gap-session", b"after".to_vec());
    let mut lines = BufReader::new(client).lines();
    let messages = tokio::time::timeout(Duration::from_secs(3), async {
        let mut result = Vec::new();
        for _ in 0..3 {
            result.push(
                serde_json::from_str::<DaemonStreamMessage>(
                    &lines.next_line().await.unwrap().unwrap(),
                )
                .unwrap(),
            );
        }
        result
    })
    .await
    .unwrap();
    assert!(
        matches!(&messages[0], DaemonStreamMessage::Output { data, .. } if data.as_ref() == b"before")
    );
    assert!(matches!(&messages[1], DaemonStreamMessage::Gap { .. }));
    assert!(
        matches!(&messages[2], DaemonStreamMessage::Output { data, .. } if data.as_ref() == b"after")
    );
    pump.abort();
    assert!(pump.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn ssh_reconnect_safety_proxy_preserves_gap_marker() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::Builder::new()
        .prefix("fx")
        .tempdir_in("/tmp")
        .unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = dir.path().join("daemon.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let (ready, release) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (read, mut write) = stream.into_split();
        let mut lines = BufReader::new(read).lines();
        assert!(lines
            .next_line()
            .await
            .unwrap()
            .unwrap()
            .contains("handshake"));
        write
            .write_all(b"{\"type\":\"handshakeOk\",\"version\":3,\"pid\":1,\"epoch\":1}\n")
            .await
            .unwrap();
        assert!(lines.next_line().await.unwrap().unwrap().contains("attach"));
        write.write_all(b"{\"type\":\"attachOk\",\"epoch\":1,\"sessionId\":\"s\",\"startSequence\":null,\"endSequence\":null,\"gap\":null,\"history\":\"\"}\n").await.unwrap();
        release.await.unwrap();
        write.write_all(b"{\"type\":\"gap\",\"sessionId\":\"s\",\"requestedAfterSequence\":4,\"availableFromSequence\":6}\n{\"type\":\"output\",\"sessionId\":\"s\",\"sequence\":6,\"data\":\"eA==\"}\n").await.unwrap();
    });
    let peer = crate::daemon::proxy::LegacyPeer::new(path, vec!["s".into()]);
    let mut attachment = peer.attach_session("s", None).await.unwrap();
    ready.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        let boundary = attachment.receiver.recv().await.unwrap();
        assert_eq!(boundary.sequence, 5);
        assert_eq!(boundary.replay_gap.unwrap().available_from_sequence, 6);
        let output = attachment.receiver.recv().await.unwrap();
        assert_eq!(output.sequence, 6);
        assert_eq!(output.bytes.as_ref(), b"x");
    })
    .await
    .unwrap();
    server.await.unwrap();
}

#[test]
fn ssh_reconnect_safety_retry_is_additive_protocol() {
    let request: DaemonRequest = serde_json::from_value(
        serde_json::json!({"type":"retryRemoteSession","sessionId":"original"}),
    )
    .unwrap();
    assert!(
        matches!(request, DaemonRequest::RetryRemoteSession { session_id } if session_id == "original")
    );
    assert_eq!(DAEMON_PROTOCOL_VERSION, 5);
}

/// In-process stand-in for a draining predecessor daemon: binds a real UDS socket in a
/// 0700 /tmp directory (the socket path validation `LegacyPeer` performs demands it) and
/// answers handshake plus one scripted reply per connection.
struct FakeLegacyDaemon {
    _dir: tempfile::TempDir,
    socket_path: PathBuf,
    task: tokio::task::JoinHandle<()>,
    requests: Arc<parking_lot::Mutex<Vec<DaemonRequest>>>,
}

impl Drop for FakeLegacyDaemon {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl FakeLegacyDaemon {
    fn start(reply: serde_json::Value) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::Builder::new()
            .prefix("fx")
            .tempdir_in("/tmp")
            .unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let socket_path = dir.path().join("daemon.sock");
        let listener = UnixListener::bind(&socket_path).unwrap();
        let requests = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let recorded = Arc::clone(&requests);
        let task = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let recorded = Arc::clone(&recorded);
                let reply = reply.clone();
                tokio::spawn(async move {
                    let (read, mut write) = tokio::io::split(stream);
                    let mut lines = BufReader::new(read).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        let request: DaemonRequest = match serde_json::from_str(&line) {
                            Ok(request) => request,
                            Err(_) => return,
                        };
                        let response = match request {
                            DaemonRequest::Handshake { version, .. } => serde_json::json!({
                                "type": "handshakeOk", "version": version, "pid": 1, "epoch": 1
                            }),
                            other => {
                                recorded.lock().push(other);
                                reply.clone()
                            }
                        };
                        let mut payload = serde_json::to_vec(&response).unwrap();
                        payload.push(b'\n');
                        if write.write_all(&payload).await.is_err() {
                            return;
                        }
                    }
                });
            }
        });
        Self {
            _dir: dir,
            socket_path,
            task,
            requests,
        }
    }

    fn peer(&self, sessions: Vec<String>) -> Arc<crate::daemon::proxy::LegacyPeer> {
        Arc::new(crate::daemon::proxy::LegacyPeer::new(
            self.socket_path.clone(),
            sessions,
        ))
    }

    fn recorded(&self) -> Vec<DaemonRequest> {
        self.requests.lock().clone()
    }
}

fn peer_remote_details_reply(session_id: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "remoteSessionDetailsOk",
        "legacyDirectSsh": false,
        "details": {
            "descriptor": {
                "backendSessionId": session_id,
                "target": {"hostId":"host","ownerId":"owner","epoch":"1","backendSessionId":"peer-target"},
                "config": {"host":{"id":"host","label":"host","hostname":"127.0.0.1","port":1,"source":"manual","authMethod":"agent"},"environment":{"platform":"posix","executor":"sh","version":"test","home":"/tmp","temp":"/tmp","git":true},"helper":{"executable":"/helper","root":"/root"},"projectId":"ssh:abcd","projectPath":"/project","worktree":null,"agentIdentity":null},
                "clientRequestId": "peer-request",
                "remoteCursor": "7",
                "cols": 80,
                "rows": 24
            },
            "state": "connected",
            "generation": 3,
            "attempts": 0,
            "failure": null,
            "replayGap": null,
            "pid": 4242
        }
    })
}

async fn request_response(server: &Arc<DaemonServer>, request: serde_json::Value) -> DaemonResponse {
    let (client, stream) = tokio::io::duplex(32 * 1024);
    let serving = tokio::spawn(Arc::clone(server).handle_client(stream));
    let (read, mut write) = tokio::io::split(client);
    let mut lines = BufReader::new(read).lines();
    let mut payload = serde_json::to_vec(&request).unwrap();
    payload.push(b'\n');
    write.write_all(&payload).await.unwrap();
    let line = tokio::time::timeout(Duration::from_secs(5), lines.next_line())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    serving.abort();
    serde_json::from_str(&line).unwrap()
}

/// Rolling handover keeps live remote sessions on the draining predecessor, so the new
/// daemon's local runtime has no entry. Answering `details: null` makes the GUI treat the
/// session as dead and respawn it; the query must be routed to the owning peer instead.
#[tokio::test]
async fn ssh_handover_remote_details_route_to_legacy_peer_instead_of_null() {
    let dir = tempfile::tempdir().unwrap();
    let server = Arc::new(DaemonServer::new_with_paths(
        Some(dir.path().join("config")),
        Some(dir.path().join("auth")),
    ));
    let legacy = FakeLegacyDaemon::start(peer_remote_details_reply("peer-owned-session"));
    server
        .session_router
        .add_legacy_peer(legacy.peer(vec!["peer-owned-session".into()]));
    assert!(server
        .terminal_service
        .remote()
        .details("peer-owned-session")
        .is_none());

    let reply = request_response(
        &server,
        serde_json::json!({"type":"remoteSessionDetails","sessionId":"peer-owned-session"}),
    )
    .await;

    let DaemonResponse::RemoteSessionDetailsOk { details, .. } = reply else {
        panic!("expected remoteSessionDetailsOk, got {reply:?}");
    };
    let details = details.expect("peer-routed session must not answer details:null");
    assert_eq!(details.descriptor.backend_session_id, "peer-owned-session");
    assert_eq!(details.pid.map(|pid| pid.0), Some(4242));
    assert!(matches!(
        legacy.recorded().as_slice(),
        [DaemonRequest::RemoteSessionDetails { session_id }] if session_id == "peer-owned-session"
    ));
}

/// A session neither runtime owns keeps the existing local answer, and a peer transport
/// failure must surface as an error rather than an authoritative "session is gone".
#[tokio::test]
async fn ssh_handover_remote_details_fall_back_when_peer_lacks_session() {
    let dir = tempfile::tempdir().unwrap();
    let server = Arc::new(DaemonServer::new_with_paths(
        Some(dir.path().join("config")),
        Some(dir.path().join("auth")),
    ));
    let legacy = FakeLegacyDaemon::start(serde_json::json!({
        "type": "error",
        "message": "Session 'ghost-session' not found",
        "code": "SESSION_NOT_FOUND"
    }));
    server
        .session_router
        .add_legacy_peer(legacy.peer(vec!["ghost-session".into()]));

    let reply = request_response(
        &server,
        serde_json::json!({"type":"remoteSessionDetails","sessionId":"ghost-session"}),
    )
    .await;
    assert!(matches!(
        reply,
        DaemonResponse::RemoteSessionDetailsOk {
            details: None,
            legacy_direct_ssh: false
        }
    ));

    // An unreachable peer is not evidence of absence.
    let unreachable = Arc::new(crate::daemon::proxy::LegacyPeer::new(
        dir.path().join("missing.sock"),
        vec!["unreachable-session".into()],
    ));
    server.session_router.add_legacy_peer(unreachable);
    let reply = request_response(
        &server,
        serde_json::json!({"type":"remoteSessionDetails","sessionId":"unreachable-session"}),
    )
    .await;
    assert!(
        matches!(reply, DaemonResponse::Error { .. }),
        "transport failure must not degrade into details:null, got {reply:?}"
    );
}

/// Retry has to reach the daemon that owns the controller, and must stay local when this
/// runtime owns the session.
#[tokio::test]
async fn ssh_handover_retry_routes_to_owner_runtime() {
    let dir = tempfile::tempdir().unwrap();
    let server = Arc::new(DaemonServer::new_with_paths(
        Some(dir.path().join("config")),
        Some(dir.path().join("auth")),
    ));
    let legacy = FakeLegacyDaemon::start(serde_json::json!({"type":"retryRemoteSessionOk"}));
    server.session_router.add_legacy_peer(legacy.peer(vec![
        "peer-retry-session".into(),
        "local-retry-session".into(),
    ]));

    let reply = request_response(
        &server,
        serde_json::json!({"type":"retryRemoteSession","sessionId":"peer-retry-session"}),
    )
    .await;
    assert!(
        matches!(reply, DaemonResponse::RetryRemoteSessionOk),
        "expected peer retry acknowledgement, got {reply:?}"
    );
    assert!(matches!(
        legacy.recorded().as_slice(),
        [DaemonRequest::RetryRemoteSession { session_id }] if session_id == "peer-retry-session"
    ));

    // The same session id now lives in the local runtime: retry must not leave this daemon.
    let descriptor: crate::terminal::remote::RemoteSessionDescriptor = serde_json::from_value(serde_json::json!({
        "backendSessionId":"local-retry-session", "target":{"hostId":"host","ownerId":"owner","epoch":"1","backendSessionId":"local-target"},
        "config":{"host":{"id":"host","label":"host","hostname":"127.0.0.1","port":1,"source":"manual","authMethod":"agent"},"environment":{"platform":"posix","executor":"sh","version":"test","home":"/tmp","temp":"/tmp","git":true},"helper":{"executable":"/helper","root":"/root"},"projectId":"ssh:abcd","projectPath":"/project","worktree":null,"agentIdentity":null},
        "clientRequestId":"local-request","remoteCursor":"0","cols":80,"rows":24
    })).unwrap();
    server
        .terminal_service
        .remote()
        .restore(descriptor)
        .unwrap();
    let reply = request_response(
        &server,
        serde_json::json!({"type":"retryRemoteSession","sessionId":"local-retry-session"}),
    )
    .await;
    assert!(
        matches!(
            reply,
            DaemonResponse::RetryRemoteSessionOk | DaemonResponse::RemoteSessionError { .. }
        ),
        "locally owned retry must be answered locally, got {reply:?}"
    );
    assert_eq!(
        legacy.recorded().len(),
        1,
        "locally owned session must not be forwarded to the legacy peer"
    );
}

/// An unrelated peer error whose message merely mentions "not found" must be
/// forwarded verbatim; only the structured SESSION_NOT_FOUND discriminator may
/// classify a peer response as "the session does not exist there either".
#[tokio::test]
async fn ssh_handover_remote_details_forward_unrelated_errors_verbatim() {
    let dir = tempfile::tempdir().unwrap();
    let server = Arc::new(DaemonServer::new_with_paths(
        Some(dir.path().join("config")),
        Some(dir.path().join("auth")),
    ));
    let legacy = FakeLegacyDaemon::start(serde_json::json!({
        "type": "error",
        "message": "remote gateway channel not found while dialing",
        "code": "REMOTE_GATEWAY_DIAL_FAILED"
    }));
    server
        .session_router
        .add_legacy_peer(legacy.peer(vec!["dial-failure-session".into()]));

    let reply = request_response(
        &server,
        serde_json::json!({"type":"remoteSessionDetails","sessionId":"dial-failure-session"}),
    )
    .await;
    assert!(
        matches!(
            &reply,
            DaemonResponse::Error { code, .. }
                if code.as_deref() == Some("REMOTE_GATEWAY_DIAL_FAILED")
        ),
        "unrelated peer errors must be forwarded verbatim, got {reply:?}"
    );
    assert!(matches!(
        legacy.recorded().as_slice(),
        [DaemonRequest::RemoteSessionDetails { .. }]
    ));
}

fn handover_persistence_record(id: &str, target: &str) -> serde_json::Value {
    serde_json::json!({
        "descriptor": {
            "backendSessionId": id,
            "target": {"hostId":"host","ownerId":"owner","epoch":"1","backendSessionId":target},
            "config": {"host":{"id":"host","label":"host","hostname":"127.0.0.1","port":1,"source":"manual","authMethod":"agent"},"environment":{"platform":"posix","executor":"sh","version":"test","home":"/tmp","temp":"/tmp","git":true},"helper":{"executable":"/helper","root":"/root"},"projectId":"ssh:abcd","projectPath":"/project","worktree":null,"agentIdentity":null},
            "clientRequestId": format!("{id}-request"),
            "remoteCursor": "0",
            "cols": 80,
            "rows": 24
        },
        "metadata": null
    })
}

fn persisted_durable_ids(path: &std::path::Path) -> Vec<String> {
    let raw = std::fs::read_to_string(path).expect("durable snapshot must exist");
    let value: serde_json::Value = serde_json::from_str(&raw).expect("durable snapshot must parse");
    value["remoteSessions"]
        .as_array()
        .expect("remoteSessions array")
        .iter()
        .map(|record| {
            record["descriptor"]["backendSessionId"]
                .as_str()
                .expect("backendSessionId")
                .to_string()
        })
        .collect()
}

/// A successor checkpoint must keep descriptors for sessions the draining
/// predecessor still owns; records no runtime owns are dropped as closed.
#[tokio::test]
async fn ssh_handover_persistence_keeps_predecessor_owned_records() {
    let dir = tempfile::tempdir().unwrap();
    let server = Arc::new(DaemonServer::new_with_paths(
        Some(dir.path().join("config")),
        Some(dir.path().join("auth")),
    ));
    let legacy = FakeLegacyDaemon::start(serde_json::json!({"type":"pingOk"}));
    server
        .session_router
        .add_legacy_peer(legacy.peer(vec!["predecessor-session".into()]));
    let durable_dir = dir.path().join("durable");
    std::fs::create_dir_all(&durable_dir).unwrap();
    let durable = durable_dir.join("remote_sessions.json");
    let mut persisted = crate::session::PersistedWorkspaceSession::default();
    persisted.version = 3;
    persisted.extra.insert(
        "remoteSessions".into(),
        serde_json::json!([
            handover_persistence_record("predecessor-session", "peer-target"),
            handover_persistence_record("closed-session", "closed-target"),
        ]),
    );
    crate::session::save_session_to_path(&durable, &persisted).unwrap();
    let local: crate::terminal::remote::RemoteSessionDescriptor = serde_json::from_value(
        handover_persistence_record("local-session", "local-target")["descriptor"].clone(),
    )
    .unwrap();
    server.terminal_service.remote().restore(local).unwrap();

    server
        .persist_remote_sessions_at(durable.clone())
        .await
        .unwrap();

    let ids = persisted_durable_ids(&durable);
    assert!(
        ids.contains(&"predecessor-session".to_string()),
        "predecessor-owned records must survive successor checkpoints, got {ids:?}"
    );
    assert!(ids.contains(&"local-session".to_string()));
    // Additive checkpoints cannot infer closure: a record no runtime owns is
    // preserved until the targeted teardown removes it with positive evidence.
    assert!(ids.contains(&"closed-session".to_string()), "additive checkpoints must not drop unknown records, got {ids:?}");
    crate::daemon::session_service::DaemonSessionService::remove_persisted_remote_record(
        durable.clone(),
        server.remote_persistence_lock.clone(),
        "closed-session".to_string(),
    )
    .await
    .unwrap();
    let ids = persisted_durable_ids(&durable);
    assert!(
        !ids.contains(&"closed-session".to_string()),
        "targeted teardown must remove exactly its own record, got {ids:?}"
    );
    assert!(ids.contains(&"predecessor-session".to_string()));
    assert!(ids.contains(&"local-session".to_string()));
}

type FakeRpc<'a, T> = std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<T, crate::ssh::bridge::BridgeError>> + Send + 'a>,
>;

/// A live fake remote helper: reads are fed by the test, writes are recorded.
struct LiveFakeHelper {
    reads: tokio::sync::Mutex<
        tokio::sync::mpsc::UnboundedReceiver<crate::ssh::bridge::ReadResult>,
    >,
    written: tokio::sync::watch::Sender<Vec<u8>>,
}

impl crate::terminal::remote::Transport for LiveFakeHelper {
    fn describe<'a>(
        &'a self,
        target: &'a crate::scoped_contracts::TargetRef,
    ) -> FakeRpc<'a, crate::ssh::bridge::DescribeResult> {
        Box::pin(async move {
            Ok(crate::ssh::bridge::DescribeResult {
                target: target.clone(),
                pid: crate::ssh::bridge::RemotePid(SSH_FIXTURE_REMOTE_PID),
                cwd: "/project".into(),
                cols: 80,
                rows: 24,
                cursor: crate::ssh::bridge::RemoteCursor(0),
                exited: false,
            })
        })
    }
    fn read<'a>(
        &'a self,
        _: &'a crate::scoped_contracts::TargetRef,
        _: crate::ssh::bridge::RemoteCursor,
        _: crate::ssh::bridge::RemoteCursor,
    ) -> FakeRpc<'a, crate::ssh::bridge::ReadResult> {
        Box::pin(async move {
            match self.reads.lock().await.recv().await {
                Some(read) => Ok(read),
                None => std::future::pending().await,
            }
        })
    }
    fn write<'a>(
        &'a self,
        _: &'a crate::scoped_contracts::TargetRef,
        bytes: &'a [u8],
    ) -> FakeRpc<'a, ()> {
        Box::pin(async move {
            self.written.send_modify(|all| all.extend_from_slice(bytes));
            Ok(())
        })
    }
    fn resize<'a>(&'a self, _: &'a crate::scoped_contracts::TargetRef, _: u16, _: u16) -> FakeRpc<'a, ()> {
        Box::pin(async move { Ok(()) })
    }
    fn stop<'a>(&'a self, _: &'a crate::scoped_contracts::TargetRef) -> FakeRpc<'a, ()> {
        Box::pin(async move { Ok(()) })
    }
}

const SSH_FIXTURE_REMOTE_PID: u32 = 4343;

fn ssh_fixture_output(
    target: &crate::scoped_contracts::TargetRef,
    cursor: u64,
    bytes: &[u8],
) -> crate::ssh::bridge::ReadResult {
    crate::ssh::bridge::ReadResult {
        target: target.clone(),
        pid: crate::ssh::bridge::RemotePid(SSH_FIXTURE_REMOTE_PID),
        cwd: "/project".into(),
        cursor: crate::ssh::bridge::RemoteCursor(cursor),
        after_sequence: cursor,
        gap: false,
        exited: false,
        chunks: vec![crate::ssh::bridge::ReadChunk {
            cursor: crate::ssh::bridge::RemoteCursor(cursor),
            sequence: cursor,
            data_base64: String::new(),
            bytes: bytes.to_vec(),
        }],
        agent_state: None,
    }
}

/// todo 8 (GAP-10), SSH fixture beside the paired one: a predecessor that owns a live SSH
/// session (fake remote helper) commits through the real v5 commit and keeps it (Draining).
/// The successor adopts the written route, then runs its real restore path over the persisted
/// remote snapshot and creates no controller for the retained session. Requests through the
/// successor reach the predecessor (same backend_session_id and remote_pid, input accepted),
/// output continues on the predecessor, and the predecessor retires only after the session
/// closes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ssh_session_stays_with_the_committed_predecessor_and_the_successor_restores_no_controller() {
    let dir = tempfile::tempdir().unwrap();
    let predecessor = Arc::new(DaemonServer::new_with_paths(
        Some(dir.path().join("pred/config")),
        Some(dir.path().join("pred/auth")),
    ));
    let descriptor: crate::terminal::remote::RemoteSessionDescriptor = serde_json::from_value(
        handover_persistence_record("retained-ssh", "retained-target")["descriptor"].clone(),
    )
    .unwrap();
    let target = descriptor.target.clone();
    let (reads_tx, reads_rx) = tokio::sync::mpsc::unbounded_channel();
    let (written_tx, mut written) = tokio::sync::watch::channel(Vec::new());
    let helper = Arc::new(LiveFakeHelper {
        reads: tokio::sync::Mutex::new(reads_rx),
        written: written_tx,
    });
    predecessor
        .terminal_service
        .remote()
        .live_import(
            crate::terminal::remote::RemoteExportState {
                descriptor,
                generation: 1,
                pending_size: None,
                pid: Some(crate::ssh::bridge::RemotePid(SSH_FIXTURE_REMOTE_PID)),
                bridge_transfer: None,
            },
            Some(helper as Arc<dyn crate::terminal::remote::Transport>),
        )
        .unwrap();
    let (_, mut output) = predecessor
        .terminal_service
        .output_hub()
        .subscribe("retained-ssh")
        .expect("the SSH fixture session has an output hub entry");
    let durable = dir.path().join("remote_sessions.json");
    // TransferSessions persists the remote snapshot before anything moves.
    predecessor.persist_remote_sessions_at(durable.clone()).await.unwrap();

    let legacy = dir.path().join("legacy-ssh.sock");
    Arc::clone(&predecessor)
        .serve_legacy_handover_socket(tokio::net::UnixListener::bind(&legacy).unwrap());
    predecessor.handover_manager.force_prepared_for_test(legacy.clone());
    let (retired_tx, mut retired) = tokio::sync::watch::channel(false);
    predecessor.handover_manager.set_retirement_action(move || {
        retired_tx.send_replace(true);
    });
    let manager = Arc::clone(&predecessor.handover_manager);
    let service = Arc::clone(&predecessor.terminal_service);
    tokio::task::spawn_blocking(move || manager.commit_handover_v5(&service))
        .await
        .unwrap()
        .expect("commit with a retained SSH session");
    assert_eq!(
        predecessor.handover_manager.status(),
        crate::daemon::handover::HandoverStatus::Draining
    );
    let routes = dir.path().join("handover_routes.json");
    let manifest = crate::daemon::manifest::HandoverManifest::load_from_path(&routes);
    assert!(manifest
        .routes
        .iter()
        .any(|r| r.legacy_socket_path == legacy && r.sessions.contains(&"retained-ssh".to_string())));

    let successor = Arc::new(DaemonServer::new_with_paths(
        Some(dir.path().join("succ/config")),
        Some(dir.path().join("succ/auth")),
    ));
    successor
        .session_router
        .adopt_routes_from_manifest_at(routes)
        .await
        .unwrap();
    successor.restore_remote_sessions_at(durable.clone()).await.unwrap();
    assert!(
        successor.terminal_service.remote().list().is_empty(),
        "the successor created an SSH controller for a retained session"
    );
    assert!(successor.terminal_service.paired().list().is_empty());
    assert!(persisted_durable_ids(&durable).contains(&"retained-ssh".to_string()));

    let reply = request_response(
        &successor,
        serde_json::json!({"type":"remoteSessionDetails","sessionId":"retained-ssh"}),
    )
    .await;
    let DaemonResponse::RemoteSessionDetailsOk { details, .. } = reply else {
        panic!("expected remoteSessionDetailsOk, got {reply:?}");
    };
    let details = details.expect("the predecessor still owns the retained SSH session");
    assert_eq!(details.descriptor.backend_session_id, "retained-ssh");
    assert_eq!(details.pid.map(|pid| pid.0), Some(SSH_FIXTURE_REMOTE_PID), "remote_pid changed");
    assert!(predecessor.terminal_service.remote().contains("retained-ssh"));

    // Input through the successor's request path is accepted by the predecessor's helper.
    let reply = request_response(
        &successor,
        serde_json::to_value(DaemonRequest::RemoteWrite {
            session_id: "retained-ssh".into(),
            generation: details.generation,
            data: b"todo8-ssh-input\n".to_vec(),
        })
        .unwrap(),
    )
    .await;
    assert!(matches!(reply, DaemonResponse::WriteOk), "routed SSH write: {reply:?}");
    tokio::time::timeout(Duration::from_secs(10), written.wait_for(|all| all.ends_with(b"todo8-ssh-input\n")))
        .await
        .expect("input never reached the SSH helper")
        .unwrap();

    // Output continues on the predecessor after the commit.
    reads_tx.send(ssh_fixture_output(&target, 1, b"todo8-ssh-output")).unwrap();
    let chunk = tokio::time::timeout(Duration::from_secs(10), output.recv())
        .await
        .expect("output never continued after the commit")
        .unwrap();
    assert_eq!(chunk, b"todo8-ssh-output".to_vec());

    // Chained upgrade B -> C while A's SSH session is open: B owns nothing, commits, retires;
    // its route is never written and A's route is untouched, so C routes to A directly.
    let legacy_b = dir.path().join("legacy-b.sock");
    successor.handover_manager.force_prepared_for_test(legacy_b.clone());
    let (b_retired_tx, mut b_retired) = tokio::sync::watch::channel(false);
    successor.handover_manager.set_retirement_action(move || {
        b_retired_tx.send_replace(true);
    });
    {
        let manager = Arc::clone(&successor.handover_manager);
        let service = Arc::clone(&successor.terminal_service);
        tokio::task::spawn_blocking(move || manager.commit_handover_v5(&service))
            .await
            .unwrap()
            .expect("B commits holding nothing of its own");
    }
    tokio::time::timeout(Duration::from_secs(10), b_retired.wait_for(|done| *done))
        .await
        .expect("B exits after its commit although A still serves a session")
        .unwrap();
    let routes_path = dir.path().join("handover_routes.json");
    let manifest = crate::daemon::manifest::HandoverManifest::load_from_path(&routes_path);
    assert!(manifest.routes.iter().all(|r| r.legacy_socket_path != legacy_b), "B wrote a route");
    assert!(manifest.routes.iter().any(|r| r.legacy_socket_path == legacy), "A's route was disturbed");
    let c = Arc::new(DaemonServer::new_with_paths(
        Some(dir.path().join("c/config")),
        Some(dir.path().join("c/auth")),
    ));
    c.session_router.adopt_routes_from_manifest_at(routes_path.clone()).await.unwrap();
    c.restore_remote_sessions_at(durable.clone()).await.unwrap();
    assert!(c.terminal_service.remote().list().is_empty(), "C created an SSH controller");
    let peer = c
        .session_router
        .find_legacy_peer_for_session("retained-ssh")
        .expect("C routes A's SSH session");
    assert_eq!(peer.socket_path(), legacy.as_path(), "C must talk to A directly");
    let reply = request_response(
        &c,
        serde_json::to_value(DaemonRequest::RemoteWrite {
            session_id: "retained-ssh".into(),
            generation: details.generation,
            data: b"todo8-chained\n".to_vec(),
        })
        .unwrap(),
    )
    .await;
    assert!(matches!(reply, DaemonResponse::WriteOk), "chained SSH write: {reply:?}");
    tokio::time::timeout(Duration::from_secs(10), written.wait_for(|all| all.ends_with(b"todo8-chained\n")))
        .await
        .expect("chained input never reached the SSH helper")
        .unwrap();

    // The predecessor stays until its retained session closes, then retires.
    assert!(!*retired.borrow(), "the predecessor retired while its SSH session was open");
    predecessor.terminal_service.remote().close("retained-ssh").await.unwrap();
    predecessor
        .handover_manager
        .check_retirement_if_empty(&predecessor.terminal_service);
    tokio::time::timeout(Duration::from_secs(10), retired.wait_for(|done| *done))
        .await
        .expect("the predecessor never retired after its last session closed")
        .unwrap();
    let manifest = crate::daemon::manifest::HandoverManifest::load_from_path(&routes_path);
    assert!(manifest.routes.iter().all(|r| r.legacy_socket_path != legacy), "A's route outlived A");
}

/// A failed durable-snapshot read must abort the checkpoint without overwriting
/// the prior bytes: preservation failure is an error, never a destructive write.
#[tokio::test]
async fn ssh_handover_persistence_read_failure_preserves_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let server = DaemonServer::new_with_paths(
        Some(dir.path().join("config")),
        Some(dir.path().join("auth")),
    );
    let durable = dir.path().join("remote_sessions.json");
    let poisoned = r#"{"version":3,"timestamp":0,"activeWorkspaceId":"","workspaces":{},"remoteSessions":{"descriptor":1}}"#;
    std::fs::write(&durable, poisoned).unwrap();
    let descriptor: crate::terminal::remote::RemoteSessionDescriptor = serde_json::from_value(
        handover_persistence_record("local-session", "local-target")["descriptor"].clone(),
    )
    .unwrap();
    server.terminal_service.remote().restore(descriptor).unwrap();

    let result = server.persist_remote_sessions_at(durable.clone()).await;

    assert!(
        result.is_err(),
        "a decode failure must abort the checkpoint, got {result:?}"
    );
    assert_eq!(
        std::fs::read_to_string(&durable).unwrap(),
        poisoned,
        "the prior snapshot must survive a failed checkpoint"
    );
}

/// A retiring predecessor must keep successor-owned records when its last session
/// closes: record deletion is targeted (the closing session only), and a cold
/// restore still finds the successor's descriptors.
#[tokio::test]
async fn ssh_handover_predecessor_retire_keeps_successor_records() {
    let dir = tempfile::tempdir().unwrap();
    let server = DaemonServer::new_with_paths(
        Some(dir.path().join("config")),
        Some(dir.path().join("auth")),
    );
    let durable = dir.path().join("remote_sessions.json");
    let mut persisted = crate::session::PersistedWorkspaceSession::default();
    persisted.version = 3;
    persisted.extra.insert(
        "remoteSessions".into(),
        serde_json::json!([
            handover_persistence_record("predecessor-owned", "peer-target"),
            handover_persistence_record("successor-owned", "successor-target"),
        ]),
    );
    crate::session::save_session_to_path(&durable, &persisted).unwrap();
    let local: crate::terminal::remote::RemoteSessionDescriptor = serde_json::from_value(
        handover_persistence_record("predecessor-owned", "peer-target")["descriptor"].clone(),
    )
    .unwrap();
    server.terminal_service.remote().restore(local).unwrap();

    // Successor checkpoint overlap: the predecessor persists while the successor's
    // record is only in the durable file (no route for it exists here).
    server
        .persist_remote_sessions_at(durable.clone())
        .await
        .unwrap();
    assert!(persisted_durable_ids(&durable).contains(&"successor-owned".to_string()));

    // The predecessor's last session closes: only its own record may be removed.
    crate::daemon::session_service::DaemonSessionService::remove_persisted_remote_record(
        durable.clone(),
        server.remote_persistence_lock.clone(),
        "predecessor-owned".to_string(),
    )
    .await
    .unwrap();
    let ids = persisted_durable_ids(&durable);
    assert_eq!(ids, vec!["successor-owned".to_string()]);

    // Cold restore after the predecessor retired: the successor session returns.
    drop(server);
    let fresh = DaemonServer::new_with_paths(
        Some(dir.path().join("config2")),
        Some(dir.path().join("auth2")),
    );
    fresh.restore_remote_sessions_at(durable).await.unwrap();
    let details = fresh
        .terminal_service
        .remote()
        .details("successor-owned")
        .expect("successor session must survive predecessor retirement");
    assert_eq!(
        details.state,
        crate::terminal::remote::RemoteConnectionState::Reconnecting
    );
}

#[tokio::test]
async fn ssh_reconnect_safety_atomic_attach_remote_generation_consistency() {
    let dir = tempfile::tempdir().unwrap();
    let server = DaemonServer::new_with_paths(
        Some(dir.path().join("config")),
        Some(dir.path().join("auth")),
    );
    let descriptor: crate::terminal::remote::RemoteSessionDescriptor = serde_json::from_value(serde_json::json!({
        "backendSessionId":"atomic-gen-backend", "target":{"hostId":"host","ownerId":"owner","epoch":"1","backendSessionId":"target"},
        "config":{"host":{"id":"host","label":"host","hostname":"127.0.0.1","port":1,"source":"manual","authMethod":"agent"},"environment":{"platform":"posix","executor":"sh","version":"test","home":"/tmp","temp":"/tmp","git":true},"helper":{"executable":"/helper","root":"/root"},"projectId":"ssh:abcd","projectPath":"/project","worktree":null,"agentIdentity":null},
        "clientRequestId":"req","remoteCursor":"1","cols":80,"rows":24
    })).unwrap();
    server
        .terminal_service
        .remote()
        .restore(descriptor)
        .unwrap();
    server
        .terminal_service
        .output_hub()
        .publish("atomic-gen-backend", b"initial query\x1b[6n".to_vec());

    // Call attach_remote_with_sequence directly to verify that the generation is atomically bound to the snapshot
    let (attachment, gen) = server
        .terminal_service
        .attach_remote_with_sequence("atomic-gen-backend", None)
        .unwrap();
    assert_eq!(gen, Some(1));
    assert_eq!(attachment.snapshot.history, b"initial query\x1b[6n");

    // Advance generation via disconnect + retry
    let entry = server
        .terminal_service
        .remote()
        .entry_for_test("atomic-gen-backend")
        .unwrap();
    entry.state.lock().details.state = crate::terminal::remote::RemoteConnectionState::Disconnected;
    server
        .terminal_service
        .remote()
        .retry("atomic-gen-backend")
        .unwrap();

    // The next atomic attachment snapshot immediately reflects the updated generation
    let (attachment2, gen2) = server
        .terminal_service
        .attach_remote_with_sequence("atomic-gen-backend", None)
        .unwrap();
    assert_eq!(gen2, Some(2));
    assert_eq!(attachment2.snapshot.history, b"initial query\x1b[6n");
}

#[cfg(unix)]
#[tokio::test]
async fn test_transfer_sessions_excludes_remote_sessions_from_handover() {
    let dir = tempfile::tempdir().unwrap();
    let server = Arc::new(DaemonServer::new_with_paths(
        Some(dir.path().join("config")),
        Some(dir.path().join("auth")),
    ));

    let remote_id = "test-remote-handover-session";
    let descriptor: crate::terminal::remote::RemoteSessionDescriptor = serde_json::from_value(serde_json::json!({
        "backendSessionId": remote_id,
        "target": {"hostId":"host","ownerId":"owner","epoch":"1","backendSessionId":"handover-target"},
        "config": {"host":{"id":"host","label":"host","hostname":"127.0.0.1","port":1,"source":"manual","authMethod":"agent"},"environment":{"platform":"posix","executor":"sh","version":"test","home":"/tmp","temp":"/tmp","git":true},"helper":{"executable":"/helper","root":"/root"},"projectId":"ssh:abcd","projectPath":"/project","worktree":null,"agentIdentity":null},
        "clientRequestId": "handover-request",
        "remoteCursor": "0",
        "cols": 80,
        "rows": 24
    })).unwrap();

    server
        .terminal_service
        .remote()
        .restore(descriptor)
        .expect("remote session restore must succeed");

    assert!(server.terminal_service.remote().contains(remote_id));
    assert!(server
        .terminal_service
        .list_sessions()
        .contains(&remote_id.to_string()));
    assert!(!server
        .terminal_service
        .pty_manager()
        .list_sessions()
        .contains(&remote_id.to_string()));

    let handover_path = dir.path().join("test_handover.sock");
    let listener = crate::daemon::handover_socket::HandoverSocketListener::bind(&handover_path)
        .expect("handover socket bind must succeed");

    let accept_handle = tokio::task::spawn_blocking(move || {
        let (stream, _creds) = listener.accept().map_err(|e| (e, 0usize))?;
        let mut exports = Vec::new();
        loop {
            match crate::daemon::handover_socket::recv_session(&stream) {
                Ok(Some(export)) => exports.push(export),
                Ok(None) => break,
                Err(error) => return Err((error, exports.len())),
            }
        }
        Ok::<_, (crate::daemon::handover_socket::HandoverSocketError, usize)>(exports)
    });

    let resp = request_response(
        &server,
        serde_json::json!({
            "type": "transferSessions",
            "handoverSocketPath": handover_path.to_string_lossy()
        }),
    )
    .await;

    match resp {
        DaemonResponse::TransferSessionsOk {
            transferred_count,
            requested_count,
        } => {
            assert_eq!(
                requested_count, 0,
                "requested_count must exclude remote sessions"
            );
            assert_eq!(
                transferred_count, 0,
                "transferred_count must be 0 for remote sessions"
            );
        }
        other => panic!("expected TransferSessionsOk, got: {other:?}"),
    }

    let exports = tokio::time::timeout(Duration::from_secs(5), accept_handle)
        .await
        .expect("accept task must not timeout")
        .expect("accept task must join")
        .expect("handover session drain must succeed");

    assert_eq!(
        exports.len(),
        0,
        "0 exports should arrive over handover socket"
    );
    assert!(server.terminal_service.remote().contains(remote_id));

    let ids = persisted_durable_ids(&server.remote_sessions_path);
    assert!(
        ids.contains(&remote_id.to_string()),
        "durable remote snapshot must contain remote session id"
    );
}
