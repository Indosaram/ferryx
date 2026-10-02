use crate::daemon::server::DaemonServer;
use super::DurableRemoteSession;

fn persisted_records(path: &std::path::Path) -> Vec<DurableRemoteSession> {
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    serde_json::from_value(value["remoteSessions"].clone()).unwrap()
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


#[tokio::test]
async fn ssh_daemon_reboot_recovery_persistence_and_status_integration() {
    let dir = tempfile::tempdir().unwrap();
    let server = DaemonServer::new_with_paths(
        Some(dir.path().join("config")),
        Some(dir.path().join("auth")),
    );
    let durable = dir.path().join("remote_sessions.json");

    let initial_desc: crate::terminal::remote::RemoteSessionDescriptor = serde_json::from_value(serde_json::json!({
        "backendSessionId":"reboot-agent-backend",
        "target":{"hostId":"host","ownerId":"owner","epoch":"1","backendSessionId":"initial-target"},
        "config":{"host":{"id":"host","label":"host","hostname":"127.0.0.1","port":1,"source":"manual","authMethod":"agent"},"environment":{"platform":"posix","executor":"sh","version":"test","home":"/tmp","temp":"/tmp","git":true},"helper":{"executable":"/helper","root":"/root"},"projectId":"ssh:abcd","projectPath":"/project","worktree":null,"agentIdentity":null},
        "clientRequestId":"client-req-1","remoteCursor":"42","cols":80,"rows":24
    })).unwrap();

    // 1. Initial restore into server
    server.terminal_service.remote().restore(initial_desc.clone()).unwrap();
    server.session_service.persist_remote_sessions_at(durable.clone()).await.unwrap();

    // 2. Watcher publishes status event
    let mut event_rx = server.session_service.remote_event_tx.subscribe();
    let _watch_rx = server.session_service.watch_remote_session("reboot-agent-backend").unwrap();

    let ev = tokio::time::timeout(std::time::Duration::from_secs(5), event_rx.recv()).await.unwrap().unwrap();
    assert_eq!(ev.event, "terminal_remote_status");
    assert_eq!(ev.payload.get("sessionId").unwrap(), "reboot-agent-backend");

    // The watcher must retain the descriptor while publishing connection status.
    let ids = persisted_durable_ids(&durable);
    assert!(ids.contains(&"reboot-agent-backend".to_string()));

    // 4. Checkpoint sink updates recovered descriptor on disk with same backend ID
    let mut recovered_desc = initial_desc.clone();
    recovered_desc.target.backend_session_id = "recovered-target".into();
    recovered_desc.remote_cursor = crate::ssh::bridge::RemoteCursor(0);
    crate::daemon::session_service::DaemonSessionService::checkpoint_remote_session_descriptor(
        durable.clone(),
        server.remote_persistence_lock.clone(),
        &recovered_desc,
        server.session_metadata.clone(),
    ).await.unwrap();

    // 5. Restart load restores the updated descriptor with the new target
    drop(server);
    let restarted = DaemonServer::new_with_paths(
        Some(dir.path().join("config-restart")),
        Some(dir.path().join("auth-restart")),
    );
    restarted.session_service.restore_remote_sessions_at(durable).await.unwrap();

    let details = restarted
        .terminal_service
        .remote()
        .details("reboot-agent-backend")
        .expect("session restored across restart");
    assert_eq!(details.descriptor.backend_session_id, "reboot-agent-backend");
    assert_eq!(details.descriptor.target.backend_session_id, "recovered-target");
    assert_eq!(details.descriptor.remote_cursor, crate::ssh::bridge::RemoteCursor(0));
}

#[tokio::test]
async fn ssh_daemon_reboot_recovery_forward_only_target_guard() {
    let dir = tempfile::tempdir().unwrap();
    let server = DaemonServer::new_with_paths(
        Some(dir.path().join("config")),
        Some(dir.path().join("auth")),
    );
    let durable = dir.path().join("remote_sessions.json");

    let initial_desc: crate::terminal::remote::RemoteSessionDescriptor = serde_json::from_value(serde_json::json!({
        "backendSessionId":"forward-guard-backend",
        "target":{"hostId":"host","ownerId":"owner","epoch":"1","backendSessionId":"old-target"},
        "config":{"host":{"id":"host","label":"host","hostname":"127.0.0.1","port":1,"source":"manual","authMethod":"agent"},"environment":{"platform":"posix","executor":"sh","version":"test","home":"/tmp","temp":"/tmp","git":true},"helper":{"executable":"/helper","root":"/root"},"projectId":"ssh:abcd","projectPath":"/project","worktree":null,"agentIdentity":null},
        "clientRequestId":"client-req-guard","remoteCursor":"100","cols":80,"rows":24
    })).unwrap();

    // 1. Initial restore and save
    server.terminal_service.remote().restore(initial_desc.clone()).unwrap();
    server.session_service.persist_remote_sessions_at(durable.clone()).await.unwrap();

    // 2. Recovery checkpoint commits NEW target
    let mut recovered_desc = initial_desc.clone();
    recovered_desc.target.backend_session_id = "new-recovered-target".into();
    recovered_desc.remote_cursor = crate::ssh::bridge::RemoteCursor(0);
    crate::daemon::session_service::DaemonSessionService::checkpoint_remote_session_descriptor(
        durable.clone(),
        server.remote_persistence_lock.clone(),
        &recovered_desc,
        server.session_metadata.clone(),
    ).await.unwrap();

    // Verify durable file has new-recovered-target
    let after_rec = persisted_records(&durable);
    assert_eq!(after_rec[0].descriptor.target.backend_session_id, "new-recovered-target");

    // 3. Before in-memory adoption, ordinary bulk save runs with OLD descriptor (still in runtime):
    server.session_service.persist_remote_sessions_at(durable.clone()).await.unwrap();

    // CRITICAL INVARIANT: Durable target must STILL be new-recovered-target! Not regressed to old-target!
    let after_bulk = persisted_records(&durable);
    assert_eq!(after_bulk[0].descriptor.target.backend_session_id, "new-recovered-target");

    // 4. Stale close/remove of old incarnation cannot remove the newer recovered record:
    crate::daemon::session_service::DaemonSessionService::remove_persisted_remote_record_guarded(
        durable.clone(),
        server.remote_persistence_lock.clone(),
        "forward-guard-backend".to_string(),
        Some(initial_desc.target.clone()),
    ).await.unwrap();

    // Durable file still contains the record!
    let after_stale_remove = persisted_records(&durable);
    assert_eq!(after_stale_remove.len(), 1);
    assert_eq!(after_stale_remove[0].descriptor.target.backend_session_id, "new-recovered-target");

    // 5. Older predecessor stale snapshot with old descriptor cannot regress later:
    let predecessor_server = DaemonServer::new_with_paths(
        Some(dir.path().join("config-pred")),
        Some(dir.path().join("auth-pred")),
    );
    predecessor_server.terminal_service.remote().restore(initial_desc.clone()).unwrap();
    predecessor_server.session_service.persist_remote_sessions_at(durable.clone()).await.unwrap();

    let after_pred = persisted_records(&durable);
    assert_eq!(after_pred[0].descriptor.target.backend_session_id, "new-recovered-target");

    // 6. When runtime adopts new target, bulk save updates descriptor while preserving recovery marker:
    let adopted_server = DaemonServer::new_with_paths(
        Some(dir.path().join("config-adopted")),
        Some(dir.path().join("auth-adopted")),
    );
    adopted_server.terminal_service.remote().restore(recovered_desc.clone()).unwrap();
    adopted_server.session_service.persist_remote_sessions_at(durable.clone()).await.unwrap();

    let after_adopted_bulk = persisted_records(&durable);
    assert_eq!(after_adopted_bulk[0].descriptor.target.backend_session_id, "new-recovered-target");
}
