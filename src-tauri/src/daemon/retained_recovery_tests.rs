use super::*;
use std::os::unix::fs::PermissionsExt;

fn fixture() -> (tempfile::TempDir, Arc<DaemonServer>) {
    let dir = tempfile::Builder::new().prefix("fxrc").tempdir_in("/tmp").unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let daemon = Arc::new(DaemonServer::new_with_paths(
        Some(dir.path().join("config")), Some(dir.path().join("auth")),
    ));
    (dir, daemon)
}

fn dead_socket(dir: &Path) -> PathBuf {
    let mut process = std::process::Command::new("/usr/bin/true").spawn().unwrap();
    let pid = process.id();
    process.wait().unwrap();
    let path = dir.join(format!("legacy-{pid}-1.sock"));
    drop(std::os::unix::net::UnixListener::bind(&path).unwrap());
    path
}

async fn request(daemon: &Arc<DaemonServer>, value: DaemonRequest) -> DaemonResponse {
    let (client, server) = tokio::io::duplex(65536);
    let serving = daemon.clone();
    let task = tokio::spawn(async move { serving.handle_client(server).await });
    let (read, mut write) = tokio::io::split(client);
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    write.write_all(&bytes).await.unwrap();
    let mut reader = BufReader::new(read);
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line)).await.unwrap().unwrap();
    drop(write);
    drop(reader);
    task.await.unwrap();
    serde_json::from_str(&line).unwrap()
}

fn persist_ssh(daemon: &DaemonServer, id: &str) {
    let mut saved = crate::session::PersistedWorkspaceSession { version: 3, ..Default::default() };
    saved.extra.insert("remoteSessions".into(), serde_json::json!([
        handover_persistence_record(id, "existing-target")
    ]));
    crate::session::save_session_to_path(&daemon.remote_sessions_path, &saved).unwrap();
}

#[tokio::test]
async fn retained_recovery_retries_dead_predecessor_without_replacing_ssh_target() {
    let (dir, _) = fixture();
    let (_reads, receiver) = tokio::sync::mpsc::unbounded_channel();
    let (_, inner_reads) = tokio::sync::mpsc::unbounded_channel();
    let helper = Arc::new(RetryHelper {
        inner: LiveFakeHelper {
            reads: tokio::sync::Mutex::new(inner_reads),
            written: tokio::sync::watch::channel(Vec::new()).0,
        },
        reads: tokio::sync::Mutex::new(receiver),
    });
    let terminals = Arc::new(TerminalService::with_remote_connector_for_test(Arc::new(RetryConnector(helper.clone()))));
    let daemon = Arc::new(DaemonServer::new_with_terminal_service(
        Some(dir.path().join("config")), Some(dir.path().join("auth")), Some(terminals),
    ));
    persist_ssh(&daemon, "retained-ssh");
    let peer = Arc::new(crate::daemon::proxy::LegacyPeer::new(dead_socket(dir.path()), vec!["retained-ssh".into()]));
    daemon.session_router.add_legacy_peer(peer);

    let (first, second) = tokio::join!(
        request(&daemon, DaemonRequest::RetryRemoteSession { session_id: "retained-ssh".into() }),
        request(&daemon, DaemonRequest::RetryRemoteSession { session_id: "retained-ssh".into() }),
    );

    assert!(matches!(first, DaemonResponse::RetryRemoteSessionOk), "{first:?}");
    assert!(matches!(second, DaemonResponse::RetryRemoteSessionOk), "{second:?}");
    let mut states = daemon.terminal_service.remote().subscribe("retained-ssh").unwrap();
    tokio::time::timeout(Duration::from_secs(5), states.wait_for(|details|
        details.state == crate::terminal::remote::RemoteConnectionState::Connected
    )).await.unwrap().unwrap();
    let details = daemon.terminal_service.remote().details("retained-ssh").unwrap();
    assert_eq!(details.descriptor.target.backend_session_id, "existing-target");
    assert_eq!(details.descriptor.target.owner_id, "owner");
    assert_eq!(details.descriptor.target.epoch, crate::scoped_contracts::Epoch(1));
    assert_eq!(daemon.terminal_service.remote().list(), vec!["retained-ssh"]);
    assert!(daemon.session_router.find_legacy_peer_for_session("retained-ssh").is_none());
    daemon.restore_remote_sessions_at(daemon.remote_sessions_path.clone()).await.unwrap();
    assert_eq!(daemon.terminal_service.remote().list(), vec!["retained-ssh"]);
    daemon.terminal_service.remote().close("retained-ssh").await.unwrap();
}

#[tokio::test]
async fn retained_recovery_keeps_route_when_predecessor_process_is_alive() {
    let (dir, daemon) = fixture();
    persist_ssh(&daemon, "retained-ssh");
    let path = dir.path().join(format!("legacy-{}-1.sock", std::process::id()));
    drop(std::os::unix::net::UnixListener::bind(&path).unwrap());
    daemon.session_router.add_legacy_peer(Arc::new(crate::daemon::proxy::LegacyPeer::new(path, vec!["retained-ssh".into()])));

    let result = request(&daemon, DaemonRequest::RetryRemoteSession { session_id: "retained-ssh".into() }).await;

    assert!(matches!(result, DaemonResponse::Error { .. }));
    assert!(!daemon.terminal_service.remote().contains("retained-ssh"));
    assert!(daemon.session_router.find_legacy_peer_for_session("retained-ssh").is_some());
}

#[tokio::test]
async fn retained_recovery_preserves_missing_descriptor_route_without_spawning() {
    let (dir, daemon) = fixture();
    daemon.session_router.add_legacy_peer(Arc::new(crate::daemon::proxy::LegacyPeer::new(dead_socket(dir.path()), vec!["missing".into()])));

    let result = request(&daemon, DaemonRequest::RetryRemoteSession { session_id: "missing".into() }).await;

    assert!(matches!(result, DaemonResponse::Error { .. }));
    assert!(daemon.terminal_service.list_sessions().is_empty());
    assert!(daemon.session_router.find_legacy_peer_for_session("missing").is_some());
}

#[tokio::test]
async fn retained_recovery_does_not_rearm_a_retired_daemon() {
    let (dir, daemon) = fixture();
    persist_ssh(&daemon, "retained-ssh");
    daemon.session_router.add_legacy_peer(Arc::new(crate::daemon::proxy::LegacyPeer::new(
        dead_socket(dir.path()), vec!["retained-ssh".into()],
    )));
    daemon.handover_manager.set_retirement_action(|| {});
    daemon.handover_manager.retire();

    let result = request(&daemon, DaemonRequest::RetryRemoteSession { session_id: "retained-ssh".into() }).await;

    assert!(matches!(result, DaemonResponse::Error { .. }));
    assert!(!daemon.terminal_service.remote().contains("retained-ssh"));
}

#[tokio::test]
async fn retained_recovery_rearms_paired_actor_with_unchanged_remote_identity() {
    let (dir, daemon) = fixture();
    let descriptor: crate::terminal::paired_daemon::Descriptor = serde_json::from_value(serde_json::json!({
        "hostId":"https://relay.example.test/host/retained", "generation":"1",
        "target":{"machineId":"retained","daemonEpoch":"7","sessionId":"existing-paired"},
        "afterSequence":"3"
    })).unwrap();
    let id = crate::remote::machine_protocol::proxy_backend_id(&descriptor.host_id, &descriptor.target).unwrap();
    let path = dir.path().join("paired_descriptors.json");
    std::fs::write(&path, serde_json::to_vec(&serde_json::json!({id.clone():descriptor})).unwrap()).unwrap();
    daemon.terminal_service.paired().set_store_path(path);
    daemon.session_router.add_legacy_peer(Arc::new(crate::daemon::proxy::LegacyPeer::new(dead_socket(dir.path()), vec![id.clone()])));

    let (first, second) = tokio::join!(
        request(&daemon, DaemonRequest::PairedRecoveryOutcome { backend_session_id: id.clone() }),
        request(&daemon, DaemonRequest::PairedRecoveryOutcome { backend_session_id: id.clone() }),
    );

    let outcome = |response| match response {
        DaemonResponse::PairedRecoveryOutcomeResult { outcome: Some(value) } => value,
        other => panic!("recovery did not install an actor: {other:?}"),
    };
    let first = outcome(first);
    let second = outcome(second);
    assert_eq!(first.incarnation, second.incarnation);
    assert_eq!(first.daemon_epoch, "7");
    assert_eq!(daemon.terminal_service.paired().descriptor(&id).unwrap().target, descriptor.target);
    assert_eq!(daemon.terminal_service.paired().list(), vec![id.clone()]);
    daemon.terminal_service.paired().detach(&id).await.unwrap();
}
