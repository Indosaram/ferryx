use ferryx_lib::{daemon::server::DaemonServer, remote::server::create_remote_router};
use futures_util::{FutureExt, StreamExt};
use serde_json::{json, Value};
use std::time::Duration;

type Socket = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
async fn next_kind(socket: &mut Socket, kind: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let message = socket.next().await.unwrap().unwrap();
            let value: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
            if value["type"] == "inventoryInvalidated" && value["payload"]["error"].is_string() { eprintln!("A12 snapshot failure while awaiting {kind}: {value}"); }
            if value["type"] == kind { return value; }
        }
    }).await.expect("expected event before deadline")
}

#[tokio::test]
async fn machine_events_begin_with_authoritative_snapshot() {
    let _trace = tracing::subscriber::set_default(tracing_subscriber::fmt().with_max_level(tracing::Level::WARN).with_test_writer().finish());
    let (root, owner) = tokio::task::spawn_blocking(|| {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("project")).unwrap();
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        std::fs::write(root.path().join("auth"), json!({"devices":{"owner":{"id":"owner","name":"owner","permission":"control","accessScope":"machine","createdAt":now,"lastSeenAt":now},"mirror":{"id":"mirror","name":"mirror","permission":"control","accessScope":"mirror","createdAt":now,"lastSeenAt":now}},"tokens":{"fixture-token":"owner","mirror-token":"mirror"}}).to_string()).unwrap();
        let owner = DaemonServer::new_with_paths(Some(root.path().join("config")), Some(root.path().join("auth")));
        (root, owner)
    }).await.unwrap();
    let state = owner.remote_state().clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let router = create_remote_router(state.clone());
    let task = tokio::spawn(async move { axum::serve(listener, router).with_graceful_shutdown(async { let _ = stopped.await; }).await.unwrap(); });
    let result = std::panic::AssertUnwindSafe(async {
        let client = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(10)).build().unwrap();
        let response = client.post(format!("http://{addr}/api/v1/socket-ticket")).bearer_auth("fixture-token")
            .header("content-type", "application/json").body(json!({"target":"/api/v1/events"}).to_string()).send().await.unwrap();
        assert_eq!(response.status(), 200);
        let ticket: Value = serde_json::from_str(&response.text().await.unwrap()).unwrap();
        let (mut socket, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/api/v1/events?ticket={}", ticket["ticket"].as_str().unwrap())).await.unwrap();
        let message = tokio::time::timeout(Duration::from_secs(3), socket.next()).await
            .expect("authorized machine event subscription must send an authoritative snapshot boundary").unwrap().unwrap();
        let value: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
        assert_eq!(value["type"], "inventoryInvalidated");
        assert!(value["sequence"].is_string());
        assert!(value["revision"].is_string());
        assert_eq!(value["payload"]["projects"]["completeness"], "complete");
        assert!(value["payload"]["projects"]["projects"].is_array());
        assert_eq!(value["payload"]["sessions"]["completeness"], "complete");
        let mirror_ticket: Value = serde_json::from_str(&client.post(format!("http://{addr}/api/v1/socket-ticket")).bearer_auth("mirror-token")
            .header("content-type", "application/json").body(json!({"target":"/api/v1/events"}).to_string()).send().await.unwrap().text().await.unwrap()).unwrap();
        let (mut mirror, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/api/v1/events?ticket={}", mirror_ticket["ticket"].as_str().unwrap())).await.unwrap();
        let response = client.post(format!("http://{addr}/api/v1/workspace/projects")).bearer_auth("fixture-token")
            .body(json!({"requestId":uuid::Uuid::new_v4().to_string(),"repoPath":root.path().join("project")}).to_string()).send().await.unwrap();
        assert_eq!(response.status(), 201);
        let project: Value = serde_json::from_str(&response.text().await.unwrap()).unwrap();
        let message = tokio::time::timeout(Duration::from_secs(10), socket.next()).await.unwrap().unwrap().unwrap();
        let event: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
        assert_eq!(event["type"], "projectRegistered");
        assert_eq!(event["workspaceId"], project["workspaceId"]);
        assert_eq!(event["payload"], project);
        assert!(event["sequence"].as_str().unwrap().parse::<u64>().unwrap() > value["sequence"].as_str().unwrap().parse::<u64>().unwrap());
        eprintln!("A12 real WS projectRegistered canonical identity matches HTTP201; sequence newer than snapshot");
        // Ordered marker on the mirror broadcaster proves no machine event was
        // queued ahead of it; absence is not asserted using a lucky timeout.
        state.event_tx.send(json!({"event":"fixtureBarrier","payload":{}}).to_string()).unwrap();
        let mirror_message = tokio::time::timeout(Duration::from_secs(5), mirror.next()).await.unwrap().unwrap().unwrap();
        let mirror_message: Value = serde_json::from_str(mirror_message.to_text().unwrap()).unwrap();
        assert_eq!(mirror_message, json!({"event":"fixtureBarrier","payload":{}}));
        mirror.close(None).await.unwrap();
        eprintln!("A12 real mirror WS receives ordered barrier only, no machine path payload");
        let boundary = next_kind(&mut socket, "inventoryInvalidated").await;
        assert_eq!(boundary["payload"]["projects"]["projects"][0], project);
        // Native notify invalidates an externally removed root without an HTTP read.
        let removed_root = root.path().join("project");
        tokio::task::spawn_blocking(move || std::fs::remove_dir(removed_root).unwrap()).await.unwrap();
        let availability = next_kind(&mut socket, "projectAvailabilityChanged").await;
        assert_eq!(availability["payload"]["availability"], "missing");
        assert_eq!(availability["workspaceId"], project["workspaceId"]);
        let restored_root = root.path().join("project");
        tokio::task::spawn_blocking(move || std::fs::create_dir(restored_root).unwrap()).await.unwrap();
        let availability = next_kind(&mut socket, "projectAvailabilityChanged").await;
        assert_eq!(availability["payload"]["availability"], "ready");
        eprintln!("A12 native watcher missing/ready availability events preserve registered identity");
        let git_root = root.path().join("project");
        tokio::task::spawn_blocking(move || {
            for args in [vec!["init", "--quiet"], vec!["-c", "user.name=A12", "-c", "user.email=a12@example.invalid", "commit", "--allow-empty", "-m", "base"]] {
                assert!(std::process::Command::new("git").arg("-C").arg(&git_root).args(args).status().unwrap().success());
            }
        }).await.unwrap();
        let workspace = project["workspaceId"].as_str().unwrap();
        let created = client.post(format!("http://{addr}/api/v1/workspace/worktrees")).bearer_auth("fixture-token")
            .body(json!({"requestId":uuid::Uuid::new_v4().to_string(),"workspaceId":workspace,"worktree":{"wsId":workspace,"slug":"event"}}).to_string()).send().await.unwrap();
        assert_eq!(created.status(), 201);
        let created: Value = serde_json::from_str(&created.text().await.unwrap()).unwrap();
        let event = next_kind(&mut socket, "worktreeCreated").await;
        assert_eq!(event["payload"], created);
        let listing: Value = serde_json::from_str(&client.get(format!("http://{addr}/api/v1/workspace/worktrees?workspaceId={workspace}")).bearer_auth("fixture-token").send().await.unwrap().text().await.unwrap()).unwrap();
        let deleted = client.delete(format!("http://{addr}/api/v1/workspace/worktrees")).bearer_auth("fixture-token")
            .body(json!({"requestId":uuid::Uuid::new_v4().to_string(),"workspaceId":workspace,"worktree":{"wsId":workspace,"slug":"event"},"deleteBranch":false,"expectedRevision":listing["revision"]}).to_string()).send().await.unwrap();
        assert_eq!(deleted.status(), 204);
        let removed = next_kind(&mut socket, "worktreeRemoved").await;
        assert_eq!(removed["payload"], created);
        assert_ne!(removed["revision"], event["revision"]);
        eprintln!("A12 HTTP-created/deleted worktree records equal real WS committed payloads");
        let branch_root = root.path().join("project");
        tokio::task::spawn_blocking(move || assert!(std::process::Command::new("git").arg("-C").arg(branch_root).args(["checkout", "-q", "-b", "external-event"]).status().unwrap().success())).await.unwrap();
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let boundary = next_kind(&mut socket, "inventoryInvalidated").await;
                if boundary["payload"]["projects"]["projects"].as_array().is_some_and(|rows| rows.iter().any(|p| p["workspaceId"] == workspace && p["gitBranch"] == "external-event")) { break; }
            }
        }).await.unwrap();
        eprintln!("A12 external Git checkout triggers debounced authoritative branch snapshot without polling HTTP");
        let session_response = client.post(format!("http://{addr}/api/v1/sessions")).bearer_auth("fixture-token")
            .body(json!({"requestId":uuid::Uuid::new_v4().to_string(),"workspaceId":workspace,"worktree":null,"cols":80,"rows":24,"inheritFromSessionId":null,"cwdRelative":null,"startup":{"kind":"shell"}}).to_string()).send().await.unwrap();
        assert_eq!(session_response.status(), 201);
        let session: Value = serde_json::from_str(&session_response.text().await.unwrap()).unwrap();
        let started = next_kind(&mut socket, "sessionStarted").await;
        assert_eq!(started["payload"]["target"], session["target"]);
        let original = owner.terminal_service().get_session(session["target"]["sessionId"].as_str().unwrap()).unwrap();
        let events = &state.machine_services.as_ref().unwrap().workspaces.machine_events;
        let mut subscriptions = events.subscription_count();
        socket.close(None).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), subscriptions.wait_for(|n| *n == 0)).await.unwrap().unwrap();
        // Hold the real bounded snapshot admission before dialing. The route has
        // already subscribed when its observable subscription count changes.
        let slots = events.snapshot_slots.clone().acquire_many_owned(2).await.unwrap();
        let ticket: Value = serde_json::from_str(&client.post(format!("http://{addr}/api/v1/socket-ticket")).bearer_auth("fixture-token")
            .header("content-type", "application/json").body(json!({"target":"/api/v1/events"}).to_string()).send().await.unwrap().text().await.unwrap()).unwrap();
        let (mut overlap, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/api/v1/events?ticket={}", ticket["ticket"].as_str().unwrap())).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), subscriptions.wait_for(|n| *n == 1)).await.unwrap().unwrap();
        let service = state.machine_services.as_ref().unwrap().workspaces.clone();
        let path = root.path().join("plain");
        tokio::task::spawn_blocking(move || {
            std::fs::create_dir(&path).unwrap();
            for index in 0..70 { service.register(&format!("local-{index}"), path.to_str().unwrap()).unwrap(); }
        }).await.unwrap();
        drop(slots);
        let overlap_boundary = next_kind(&mut overlap, "inventoryInvalidated").await;
        assert_eq!(overlap_boundary["payload"]["projects"]["projects"].as_array().unwrap().len(), 71);
        let lag = tokio::time::timeout(Duration::from_secs(15), async {
            loop { let event = next_kind(&mut overlap, "inventoryInvalidated").await; if event["reason"] == "lag" { break event; } }
        }).await.unwrap();
        assert_eq!(lag["payload"]["projects"]["projects"].as_array().unwrap().len(), 71);
        assert_eq!(owner.terminal_service().list_sessions().len(), 1);
        assert!(std::sync::Arc::ptr_eq(&original, &owner.terminal_service().get_session(session["target"]["sessionId"].as_str().unwrap()).unwrap()));
        assert_eq!(lag["payload"]["sessions"]["sessions"][0]["target"], session["target"]);
        let closed = client.delete(format!("http://{addr}/api/v1/sessions/{}", session["target"]["sessionId"].as_str().unwrap())).bearer_auth("fixture-token")
            .body(json!({"requestId":uuid::Uuid::new_v4().to_string(),"daemonEpoch":session["target"]["daemonEpoch"]}).to_string()).send().await.unwrap();
        assert_eq!(closed.status(), 204);
        assert!(original.is_reaped());
        overlap.close(None).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), subscriptions.wait_for(|n| *n == 0)).await.unwrap().unwrap();
        eprintln!("A12 receiver-before-snapshot overlap=70 committed local registrations; real WS lag resnapshot=71; reconnect retains original PTY Arc/target; explicit close reaped; unsubscribe observed");
        let slots = events.snapshot_slots.clone().acquire_many_owned(2).await.unwrap();
        let ticket: Value = serde_json::from_str(&client.post(format!("http://{addr}/api/v1/socket-ticket")).bearer_auth("fixture-token")
            .header("content-type", "application/json").body(json!({"target":"/api/v1/events"}).to_string()).send().await.unwrap().text().await.unwrap()).unwrap();
        let (mut cancelled, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/api/v1/events?ticket={}", ticket["ticket"].as_str().unwrap())).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), subscriptions.wait_for(|n| *n == 1)).await.unwrap().unwrap();
        cancelled.close(None).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), subscriptions.wait_for(|n| *n == 0)).await.unwrap().unwrap();
        drop(slots);
        assert_eq!(events.snapshot_slots.available_permits(), 2);
        eprintln!("A12 unsubscribe while snapshot admission blocked cancels before permit release; watches dropped");
    }).catch_unwind().await;
    for id in owner.terminal_service().list_sessions() { owner.terminal_service().close_session(&id).await.unwrap(); }
    state.auth_manager.revoke_device("owner");
    state.auth_manager.revoke_device("mirror");
    stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(10), task).await.unwrap().unwrap();
    assert!(tokio::net::TcpStream::connect(addr).await.is_err());
    assert!(owner.terminal_service().list_sessions().is_empty());
    drop(owner); drop(state);
    tokio::task::spawn_blocking(move || root.close().unwrap()).await.unwrap();
    eprintln!("A12 cleanup: listener joined/refused; all owned PTYs closed; private root removed");
    if let Err(panic) = result { std::panic::resume_unwind(panic); }
}
