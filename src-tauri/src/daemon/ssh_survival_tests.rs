use super::*;

#[tokio::test]
async fn ssh_daemon_restart_restores_identity_without_local_pty() {
    let dir = tempfile::tempdir().unwrap();
    let server = DaemonServer::new_with_paths(Some(dir.path().join("config")), Some(dir.path().join("auth")));
    let descriptor: crate::terminal::remote::RemoteSessionDescriptor = serde_json::from_value(serde_json::json!({
        "backendSessionId":"original-pane-backend", "target":{"hostId":"host","ownerId":"owner","epoch":"1","backendSessionId":"original-target"},
        "config":{"host":{"id":"host","label":"host","hostname":"127.0.0.1","port":1,"source":"manual","authMethod":"agent"},"environment":{"platform":"posix","executor":"sh","version":"test","home":"/tmp","temp":"/tmp","git":true},"helper":{"executable":"/helper","root":"/root"},"projectId":"ssh:abcd","projectPath":"/project","worktree":null,"agentIdentity":null},
        "clientRequestId":"original-request","remoteCursor":"73","cols":80,"rows":24
    })).unwrap();
    let path = dir.path().join("identities.json");
    server.terminal_service.remote().restore(descriptor.clone()).unwrap();
    server.persist_remote_sessions_at(path.clone()).await.unwrap();
    drop(server);
    let restored = DaemonServer::new_with_paths(Some(dir.path().join("config2")), Some(dir.path().join("auth2")));
    restored.restore_remote_sessions_at(path).await.unwrap();
    assert!(restored.session_router.is_local_session("original-pane-backend"));
    assert!(restored.terminal_service.get_session("original-pane-backend").is_none());
    let mut replay_descriptor = descriptor.clone();
    replay_descriptor.remote_cursor = crate::ssh::bridge::RemoteCursor(0);
    assert_eq!(restored.terminal_service.remote().details("original-pane-backend").unwrap().descriptor, replay_descriptor);
    restored.terminal_service.output_hub().publish("original-pane-backend", b"retained replay".to_vec());
    assert!(matches!(restored.handle_describe_session("original-pane-backend"), DaemonResponse::DescribeSessionOk { .. }));
    let restored = Arc::new(restored);
    let (client, stream) = tokio::io::duplex(32 * 1024);
    let serving = tokio::spawn(restored.clone().handle_client(stream));
    let (read, mut write) = tokio::io::split(client);
    let mut lines = BufReader::new(read).lines();
    write.write_all(b"{\"type\":\"remoteSessionDetails\",\"sessionId\":\"original-pane-backend\"}\n").await.unwrap();
    let line = tokio::time::timeout(Duration::from_secs(3), lines.next_line()).await.unwrap().unwrap().unwrap();
    let reply: DaemonResponse = serde_json::from_str(&line).unwrap();
    assert!(matches!(reply, DaemonResponse::RemoteSessionDetailsOk { details: Some(_), legacy_direct_ssh: false }));
    write.write_all(b"{\"type\":\"attach\",\"sessionId\":\"original-pane-backend\",\"afterSequence\":999}\n").await.unwrap();
    let line = tokio::time::timeout(Duration::from_secs(3), lines.next_line()).await.unwrap().unwrap().unwrap();
    assert!(matches!(serde_json::from_str::<DaemonResponse>(&line).unwrap(), DaemonResponse::AttachOk { gap: Some(_), history, .. } if history == b"retained replay"));
    let line = tokio::time::timeout(Duration::from_secs(3), lines.next_line()).await.unwrap().unwrap().unwrap();
    assert!(matches!(serde_json::from_str::<DaemonStreamMessage>(&line).unwrap(), DaemonStreamMessage::RemoteStatus { .. }));
    serving.abort();
    assert!(serving.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn ssh_reconnect_safety_gap_precedes_recovered_output() {
    let hub = Arc::new(TerminalOutputHub::default());
    let (_, rx) = hub.register_session_channels("gap-session");
    let (client, server) = tokio::io::duplex(32 * 1024);
    let pump = tokio::spawn(DaemonServer::pump_sequenced_stream("gap-session".into(), rx, hub.clone(), server));
    hub.publish("gap-session", b"before".to_vec());
    hub.publish_gap("gap-session");
    hub.publish("gap-session", b"after".to_vec());
    let mut lines = BufReader::new(client).lines();
    let messages = tokio::time::timeout(Duration::from_secs(3), async {
        let mut result = Vec::new();
        for _ in 0..3 { result.push(serde_json::from_str::<DaemonStreamMessage>(&lines.next_line().await.unwrap().unwrap()).unwrap()); }
        result
    }).await.unwrap();
    assert!(matches!(&messages[0], DaemonStreamMessage::Output { data, .. } if data.as_ref() == b"before"));
    assert!(matches!(&messages[1], DaemonStreamMessage::Gap { .. }));
    assert!(matches!(&messages[2], DaemonStreamMessage::Output { data, .. } if data.as_ref() == b"after"));
    pump.abort(); assert!(pump.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn ssh_reconnect_safety_proxy_preserves_gap_marker() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::Builder::new().prefix("fx").tempdir_in("/tmp").unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = dir.path().join("daemon.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let (ready, release) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (read, mut write) = stream.into_split();
        let mut lines = BufReader::new(read).lines();
        assert!(lines.next_line().await.unwrap().unwrap().contains("handshake"));
        write.write_all(b"{\"type\":\"handshakeOk\",\"version\":3,\"pid\":1,\"epoch\":1}\n").await.unwrap();
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
        assert_eq!(output.sequence, 6); assert_eq!(output.bytes.as_ref(), b"x");
    }).await.unwrap();
    server.await.unwrap();
}

#[test]
fn ssh_reconnect_safety_retry_is_additive_protocol() {
    let request: DaemonRequest = serde_json::from_value(serde_json::json!({"type":"retryRemoteSession","sessionId":"original"})).unwrap();
    assert!(matches!(request, DaemonRequest::RetryRemoteSession { session_id } if session_id == "original"));
    assert_eq!(DAEMON_PROTOCOL_VERSION, 3);
}

#[tokio::test]
async fn ssh_reconnect_safety_atomic_attach_remote_generation_consistency() {
    let dir = tempfile::tempdir().unwrap();
    let server = DaemonServer::new_with_paths(Some(dir.path().join("config")), Some(dir.path().join("auth")));
    let descriptor: crate::terminal::remote::RemoteSessionDescriptor = serde_json::from_value(serde_json::json!({
        "backendSessionId":"atomic-gen-backend", "target":{"hostId":"host","ownerId":"owner","epoch":"1","backendSessionId":"target"},
        "config":{"host":{"id":"host","label":"host","hostname":"127.0.0.1","port":1,"source":"manual","authMethod":"agent"},"environment":{"platform":"posix","executor":"sh","version":"test","home":"/tmp","temp":"/tmp","git":true},"helper":{"executable":"/helper","root":"/root"},"projectId":"ssh:abcd","projectPath":"/project","worktree":null,"agentIdentity":null},
        "clientRequestId":"req","remoteCursor":"1","cols":80,"rows":24
    })).unwrap();
    server.terminal_service.remote().restore(descriptor).unwrap();
    server.terminal_service.output_hub().publish("atomic-gen-backend", b"initial query\x1b[6n".to_vec());

    // Call attach_remote_with_sequence directly to verify that the generation is atomically bound to the snapshot
    let (attachment, gen) = server.terminal_service.attach_remote_with_sequence("atomic-gen-backend", None).unwrap();
    assert_eq!(gen, Some(1));
    assert_eq!(attachment.snapshot.history, b"initial query\x1b[6n");

    // Advance generation via disconnect + retry
    let entry = server.terminal_service.remote().entry_for_test("atomic-gen-backend").unwrap();
    entry.state.lock().details.state = crate::terminal::remote::RemoteConnectionState::Disconnected;
    server.terminal_service.remote().retry("atomic-gen-backend").unwrap();

    // The next atomic attachment snapshot immediately reflects the updated generation
    let (attachment2, gen2) = server.terminal_service.attach_remote_with_sequence("atomic-gen-backend", None).unwrap();
    assert_eq!(gen2, Some(2));
    assert_eq!(attachment2.snapshot.history, b"initial query\x1b[6n");
}
