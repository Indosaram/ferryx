#![cfg(unix)]
#[path = "support/machine_owner_socket.rs"]
mod socket;
use ferryx_lib::{daemon::server::DaemonServer, remote::server::create_remote_router};
use futures_util::FutureExt;
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};

const DEADLINE: Duration = Duration::from_secs(15);

async fn reply(request: reqwest::RequestBuilder, status: u16) -> Value {
    let response = request.send().await.expect("HTTP response");
    let actual = response.status().as_u16();
    let body = response.text().await.expect("HTTP body");
    assert_eq!(actual, status, "{body}");
    serde_json::from_str(&body).expect("JSON response")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn original_target_remains_running_when_gateway_hands_over() {
    // Given: all process-global library state resolves below a private supervisor root.
    let home = std::path::PathBuf::from(std::env::var_os("HOME").expect("private HOME"));
    assert!(home.parent().and_then(|p| p.file_name()).and_then(|p| p.to_str()).is_some_and(|name| name.starts_with("a10-owner-")), "run using the private supervisor");
    for key in ["FERRYX_RUNTIME_DIR", "FERRYX_DATA_DIR", "FERRYX_SESSION_DIR", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "TMPDIR"] {
        assert!(std::path::PathBuf::from(std::env::var_os(key).expect(key)).starts_with(home.parent().expect("supervisor")));
    }
    let root = tempfile::tempdir().expect("private fixture");
    let path = root.path().to_owned();
    let old = tokio::task::spawn_blocking(move || {
        std::fs::create_dir(path.join("project")).expect("project");
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).expect("clock").as_secs();
        let mut devices = serde_json::Map::new();
        for (id, scope) in [("owner", "machine"), ("other", "machine"), ("mirror", "mirror")] {
            devices.insert(id.into(), json!({"id":id,"name":id,"permission":"control","accessScope":scope,"createdAt":now,"lastSeenAt":now}));
        }
        std::fs::write(path.join("auth"), json!({"devices":devices,"tokens":{"owner-token":"owner","other-token":"other","mirror-token":"mirror"}}).to_string()).expect("auth fixture");
        Arc::new(DaemonServer::new_with_paths(Some(path.join("config")), Some(path.join("auth"))))
    }).await.expect("owner initialization");
    let (retired_tx, mut retired_rx) = tokio::sync::watch::channel(false);
    old.handover_manager.set_retirement_action(move || { retired_tx.send_replace(true); });
    let mut tasks = tokio::task::JoinSet::new();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("old gateway");
    let old_addr = listener.local_addr().expect("old address");
    let router = create_remote_router(old.remote_state().clone());
    tasks.spawn(async move { axum::serve(listener, router).await.expect("old HTTP server"); });
    let result = std::panic::AssertUnwindSafe(async {
        let client = reqwest::Client::builder().no_proxy().timeout(DEADLINE).build().expect("HTTP client");
        let base = format!("http://{old_addr}");
        let project = reply(client.post(format!("{base}/api/v1/workspace/projects")).bearer_auth("owner-token")
            .body(json!({"requestId":uuid::Uuid::new_v4().to_string(),"repoPath":root.path().join("project")}).to_string()), 201).await;
        let session = reply(client.post(format!("{base}/api/v1/sessions")).bearer_auth("owner-token")
            .body(json!({"requestId":uuid::Uuid::new_v4().to_string(),"workspaceId":project["workspaceId"],"worktree":null,"cols":80,"rows":24,"inheritFromSessionId":null,"cwdRelative":null,"startup":{"kind":"shell"}}).to_string()), 201).await;
        let id = session["target"]["sessionId"].as_str().expect("raw ID");
        let pty = old.terminal_service().get_session(id).expect("original PTY");
        let pid = pty.pid();
        // When: the real handover manager commits a persisted route to the original owner.
        let (legacy, _, listener) = old.handover_manager.prepare_handover(old.terminal_service()).expect("prepare handover");
        let predecessor = old.clone();
        tasks.spawn(async move {
            let mut clients = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let (stream, _) = accepted.expect("legacy accept");
                        clients.spawn(predecessor.clone().handle_client(stream));
                    }
                    Some(result) = clients.join_next() => { result.expect("legacy client joined"); }
                }
            }
        });
        let predecessor = old.clone();
        // This contract proves the legacy v4 routing path (durable route + predecessor keeps the
        // live PTY), so it commits through `commit_handover_v4` explicitly instead of relying on
        // the environment-dependent dispatcher.
        tokio::task::spawn_blocking(move || predecessor.handover_manager.commit_handover_v4(predecessor.terminal_service())).await.expect("commit worker").expect("commit handover");
        let path = root.path().to_owned();
        let new = tokio::task::spawn_blocking(move || Arc::new(DaemonServer::new_with_paths(Some(path.join("config")), Some(path.join("auth"))))).await.expect("new owner");
        let owner_epoch = session["target"]["daemonEpoch"].as_str().expect("epoch").parse::<u64>().expect("numeric epoch");
        new.remote_state().daemon_epoch.store(owner_epoch + 1, std::sync::atomic::Ordering::Release);
        new.session_router.adopt_routes_from_manifest().await.expect("adopt actual handover route");
        assert_eq!(new.session_router.find_legacy_peer_for_session(id).expect("adopted peer").socket_path(), legacy);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("new gateway");
        let addr = listener.local_addr().expect("new address");
        let router = create_remote_router(new.remote_state().clone());
        tasks.spawn(async move { axum::serve(listener, router).await.expect("new HTTP server"); });
        // Then: a surviving target is running at the original epoch, not the gateway epoch.
        let old_detail = reply(client.get(format!("{base}/api/v1/sessions/{id}")).bearer_auth("owner-token"), 200).await;
        let detail = reply(client.get(format!("http://{addr}/api/v1/sessions/{id}")).bearer_auth("owner-token"), 200).await;
        eprintln!("A10 owner handover original_pid={pid:?} current_pid={:?} owner_epoch={owner_epoch} gateway_epoch={} old_detail={old_detail} new_detail={detail}", pty.pid(), owner_epoch + 1);
        assert_eq!(pty.pid(), pid);
        assert_eq!(old_detail["status"], "running");
        let missing = reply(client.get(format!("http://{addr}/api/v1/sessions/missing")).bearer_auth("owner-token"), 404).await;
        assert_eq!(missing["error"]["code"], "SESSION_NOT_FOUND");
        let (mut first, first_boundary) = socket::attach(&base, &session["target"]).await;
        let (mut routed, boundary) = socket::attach(&format!("http://{addr}"), &session["target"]).await;
        socket::closed(&mut first).await;
        assert!(boundary["generation"].as_str().expect("generation").parse::<u64>().expect("generation number") > first_boundary["generation"].as_str().expect("generation").parse::<u64>().expect("generation number"));
        let proof = socket::proof(&mut routed).await;
        assert_eq!(proof, format!("{}:{}", pid.expect("original PID"), session["cwd"].as_str().expect("original cwd")));
        socket::stale_resize(&mut routed, &first_boundary["generation"]).await;
        assert_eq!(pty.get_size(), (80, 24));
        socket::denied(&format!("http://{addr}"), &session["target"], "other-token", 409).await;
        socket::denied(&base, &session["target"], "other-token", 409).await;
        socket::denied(&format!("http://{addr}"), &session["target"], "mirror-token", 403).await;
        socket::resize(&mut routed, &boundary["generation"]).await;
        assert_eq!(pty.get_size(), (103, 37));
        eprintln!("A10 routed HTTP/WS/PTY proof={proof} generation={} geometry=103x37", boundary["generation"]);
        assert_eq!(detail["status"], "running", "a routed live PTY must not expire at the replacement gateway");
        assert_eq!(detail["session"]["target"], session["target"]);
        let mirror = reply(client.get(format!("http://{addr}/api/v1/sessions")).bearer_auth("mirror-token"), 200).await;
        assert!(!mirror.to_string().contains(id), "machine target excluded from actual mirror inventory");
        let conflict = reply(client.delete(format!("http://{addr}/api/v1/sessions/{id}")).bearer_auth("other-token")
            .body(json!({"requestId":uuid::Uuid::new_v4().to_string(),"daemonEpoch":owner_epoch.to_string()}).to_string()), 409).await;
        assert_eq!(conflict["error"]["code"], "CONTROL_CONFLICT");
        let stale = reply(client.delete(format!("http://{addr}/api/v1/sessions/{id}")).bearer_auth("owner-token")
            .body(json!({"requestId":uuid::Uuid::new_v4().to_string(),"daemonEpoch":(owner_epoch + 1).to_string()}).to_string()), 409).await;
        assert_eq!(stale["error"]["code"], "STALE_EPOCH");
        let inventory = reply(client.get(format!("http://{addr}/api/v1/sessions")).bearer_auth("owner-token"), 200).await;
        assert_eq!(inventory["completeness"], "complete");
        assert_eq!(inventory["sessions"][0]["target"], session["target"]);
        assert_eq!(inventory["sessions"][0]["running"], true);
        // A disconnected predecessor retains its original row; reconnect does not Create.
        let hidden = legacy.with_extension("offline");
        std::fs::rename(&legacy, &hidden).expect("isolate owned predecessor address");
        let partial_response = client.get(format!("http://{addr}/api/v1/sessions")).bearer_auth("owner-token").send().await;
        std::fs::rename(&hidden, &legacy).expect("restore owned predecessor address");
        let partial_response = partial_response.expect("partial response");
        assert_eq!(partial_response.status(), 200);
        let partial: Value = serde_json::from_str(&partial_response.text().await.expect("partial body")).expect("partial JSON");
        assert_eq!(partial["completeness"], "partial");
        assert_eq!(partial["sessions"][0]["target"], session["target"]);
        assert_eq!(partial["unavailableWorkspaceIds"][0], project["workspaceId"]);
        assert_eq!(old.terminal_service().list_sessions(), vec![id.to_owned()]);
        assert!(new.terminal_service().list_sessions().is_empty());
        let response = client.delete(format!("http://{addr}/api/v1/sessions/{id}")).bearer_auth("owner-token")
            .body(json!({"requestId":uuid::Uuid::new_v4().to_string(),"daemonEpoch":owner_epoch.to_string()}).to_string()).send().await.expect("routed close");
        assert_eq!(response.status(), 204, "{}", response.text().await.expect("close body"));
        socket::closed(&mut routed).await;
        assert!(pty.is_reaped());
        eprintln!("A10 owner cleanup reaped pid={:?}", pty.pid());
        drop(routed);
    }).catch_unwind().await;
    for id in old.terminal_service().list_sessions() {
        let pty = old.terminal_service().get_session(&id).expect("cleanup PTY");
        old.terminal_service().close_session(&id).await.expect("close owned PTY");
        assert!(pty.is_reaped());
        eprintln!("A10 owner cleanup reaped pid={:?}", pty.pid());
    }
    tasks.abort_all();
    while let Some(result) = tasks.join_next().await {
        if let Err(error) = result { assert!(error.is_cancelled(), "{error}"); }
    }
    if old.handover_manager.status() != ferryx_lib::daemon::handover::HandoverStatus::Active {
        old.handover_manager.check_retirement_if_empty(old.terminal_service());
        tokio::time::timeout(DEADLINE, retired_rx.wait_for(|retired| *retired))
            .await.expect("bounded retirement cleanup").expect("retirement action observed");
        assert_eq!(old.handover_manager.status(), ferryx_lib::daemon::handover::HandoverStatus::Retired);
        eprintln!("A10 owner retirement action observed after route cleanup");
    }
    drop(old);
    root.close().expect("remove fixture root");
    eprintln!("A10 owner cleanup gateway/legacy tasks joined; PTYs reaped; fixture root removed");
    if let Err(panic) = result { std::panic::resume_unwind(panic); }
}
