//! A12 owner-output regression through the authenticated HTTP/WS surface.
use ferryx_lib::{daemon::server::DaemonServer, remote::server::create_remote_router};
use futures_util::{FutureExt, StreamExt};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
#[cfg(unix)]
#[path = "support/session_metadata_handover.rs"]
mod handover;
#[cfg(target_os = "macos")]
#[path = "support/session_metadata_provider.rs"]
mod provider;

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn next_kind(socket: &mut Socket, kind: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let frame = socket
                .next()
                .await
                .expect("event socket open")
                .expect("valid WS frame");
            if frame.is_text() {
                let event: Value =
                    serde_json::from_str(frame.to_text().expect("text frame")).expect("JSON event");
                if event["type"] == kind {
                    return event;
                }
            }
        }
    })
    .await
    .expect("owner output must publish sessionMetadataChanged before deadline")
}

async fn connect_events(client: &reqwest::Client, base: &str, token: &str) -> Socket {
    let response = client
        .post(format!("{base}/api/v1/socket-ticket"))
        .bearer_auth(token)
        .header("content-type", "application/json")
        .body(json!({"target":"/api/v1/events"}).to_string())
        .send()
        .await
        .expect("ticket HTTP response");
    assert_eq!(response.status(), 200);
    let ticket: Value =
        serde_json::from_str(&response.text().await.expect("ticket body")).expect("ticket JSON");
    let url = format!(
        "{}/api/v1/events?ticket={}",
        base.replacen("http://", "ws://", 1),
        ticket["ticket"].as_str().expect("ticket")
    );
    tokio_tungstenite::connect_async(url)
        .await
        .expect("authenticated WS")
        .0
}

#[tokio::test]
async fn publishes_owner_metadata_when_inactive_shell_changes_title_and_cwd() {
    // Given: runner isolates HOME/data/runtime/temp before any process initialization.
    let data = std::env::var_os("FERRYX_DATA_DIR").expect("run with isolated FERRYX_DATA_DIR");
    let home = std::env::var_os("HOME").expect("isolated HOME");
    assert!(std::path::Path::new(&data).parent() == std::path::Path::new(&home).parent());
    let (root, owner) = tokio::task::spawn_blocking(|| {
        let root = tempfile::tempdir().expect("private fixture");
        std::fs::create_dir_all(root.path().join("project/child")).expect("project fixture");
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).expect("clock").as_secs();
        std::fs::write(root.path().join("auth"), json!({"devices":{
            "owner":{"id":"owner","name":"owner","permission":"control","accessScope":"machine","createdAt":now,"lastSeenAt":now},
            "mirror":{"id":"mirror","name":"mirror","permission":"control","accessScope":"mirror","createdAt":now,"lastSeenAt":now}},
            "tokens":{"owner-token":"owner","mirror-token":"mirror"}}).to_string()).expect("private auth");
        let owner = Arc::new(DaemonServer::new_with_paths(Some(root.path().join("config")), Some(root.path().join("auth"))));
        (root, owner)
    }).await.expect("owner constructed");
    let state = owner.remote_state().clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("ephemeral listener");
    let address = listener.local_addr().expect("listener address");
    let base = format!("http://{address}");
    let router = create_remote_router(state.clone());
    let mut tasks = tokio::task::JoinSet::new();
    #[cfg(unix)]
    let agent_listener = owner.spawn_agent_state_listener().expect("canonical report listener");
    tasks.spawn(async move {
        axum::serve(listener, router).await.expect("HTTP server");
    });
    let result = std::panic::AssertUnwindSafe(async {
        let client = reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(10)).build().expect("HTTP client");
        let mut machine = connect_events(&client, &base, "owner-token").await;
        next_kind(&mut machine, "inventoryInvalidated").await;
        let mut mirror = connect_events(&client, &base, "mirror-token").await;
        let registered = client.post(format!("{base}/api/v1/workspace/projects")).bearer_auth("owner-token")
            .body(json!({"requestId":uuid::Uuid::new_v4().to_string(),"repoPath":root.path().join("project")}).to_string())
            .send().await.expect("registration response");
        assert_eq!(registered.status(), 201);
        let project: Value = serde_json::from_str(&registered.text().await.expect("project body")).expect("project");
        let created = client.post(format!("{base}/api/v1/sessions")).bearer_auth("owner-token")
            .body(json!({"requestId":uuid::Uuid::new_v4().to_string(),"workspaceId":project["workspaceId"],
                "worktree":null,"cols":80,"rows":24,"inheritFromSessionId":null,"cwdRelative":null,"startup":{"kind":"shell"}}).to_string())
            .send().await.expect("create response");
        assert_eq!(created.status(), 201);
        let session: Value = serde_json::from_str(&created.text().await.expect("session body")).expect("session");
        next_kind(&mut machine, "sessionStarted").await;
        let id = session["target"]["sessionId"].as_str().expect("session ID");
        let before: Value = serde_json::from_str(&client.get(format!("{base}/api/v1/sessions")).bearer_auth("owner-token")
            .send().await.expect("list response").text().await.expect("session list body")).expect("session list");
        let (_, mut output) = owner.terminal_service().output_hub().subscribe(id).expect("subscribe before input");
        // No workspace selection or terminal attachment: owner runs while inactive.
        // When: the real owned shell changes directory and emits a terminal title.
        owner.terminal_service().write_input(id, b"cd child && printf '\\033]2;A12-owner-title\\007'\n").expect("PTY command");
        tokio::time::timeout(Duration::from_secs(5), async {
            let mut bytes = Vec::new();
            loop {
                bytes.extend(output.recv().await.expect("PTY output"));
                if bytes.windows(b"\x1b]2;A12-owner-title\x07".len()).any(|part| part == b"\x1b]2;A12-owner-title\x07") { break; }
            }
        }).await.expect("real PTY emitted complete title control");
        eprintln!("A12 RED seam: real inactive owner PTY emitted title control after cd; subscribed machine WS awaiting metadata");
        // Then: only the machine projection carries exact owner metadata/revision.
        let event = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let event = next_kind(&mut machine, "sessionMetadataChanged").await;
                if event["payload"]["title"] == "A12-owner-title" { break event; }
            }
        }).await.expect("title-bearing owner metadata");
        assert_eq!(event["payload"]["target"], session["target"]);
        assert_eq!(event["payload"]["title"], "A12-owner-title");
        assert_eq!(event["payload"]["cwd"], std::fs::canonicalize(root.path().join("project/child")).expect("fixture CWD").to_str().expect("UTF-8 fixture"));
        assert!(event["sequence"].as_str().expect("event cursor").parse::<u64>().expect("cursor") > 0);
        assert_eq!(event["revision"], event["sequence"]);
        assert!(event["sessionRevision"].is_string());
        state.event_tx.send(json!({"event":"fixtureBarrier","payload":{}}).to_string()).expect("mirror subscribed");
        let frame = tokio::time::timeout(Duration::from_secs(5), mirror.next()).await.expect("mirror barrier").expect("mirror open").expect("mirror frame");
        let projected: Value = serde_json::from_str(frame.to_text().expect("mirror text")).expect("mirror JSON");
        assert_eq!(projected, json!({"event":"fixtureBarrier","payload":{}}));
        let target: ferryx_lib::remote::machine_protocol::RemoteTerminalTarget = serde_json::from_value(session["target"].clone()).expect("typed owner target");
        let authority = &state.machine_services.as_ref().expect("authority").sessions;
        let hint = ferryx_lib::daemon::protocol::AgentStateReport { session_id: id.into(), state: "idle".into(), agent: Some("omo".into()), provider_session: Some(ferryx_lib::daemon::protocol::AgentProviderSession { key: ferryx_lib::daemon::protocol::AgentProviderSessionKey::SessionId, id: "forged-provider".into(), transcript_path: None }) };
        let mut stale = target.clone(); stale.daemon_epoch.0 += 1;
        assert!(authority.validate_machine_agent_report(stale, hint.clone()).await.is_err());
        let mut foreign = target.clone(); foreign.machine_id = "forged-machine".into();
        assert!(authority.validate_machine_agent_report(foreign, hint.clone()).await.is_err());
        let mut missing = target.clone(); missing.session_id = "unowned-session".into();
        let mut missing_hint = hint.clone(); missing_hint.session_id = missing.session_id.clone();
        assert!(authority.validate_machine_agent_report(missing, missing_hint).await.is_err());
        assert!(authority.validate_machine_agent_report(target, hint).await.is_err());
        let after: Value = serde_json::from_str(&client.get(format!("{base}/api/v1/sessions")).bearer_auth("owner-token").send().await.expect("inventory").text().await.expect("body")).expect("JSON");
        assert_eq!(after["sessions"][0]["title"], event["payload"]["title"]);
        assert_eq!(after["sessions"][0]["providerSession"], Value::Null);
        assert!(after["revision"].as_str().expect("session revision").parse::<u64>().expect("revision") > before["revision"].as_str().expect("prior revision").parse::<u64>().expect("revision"));
        #[cfg(target_os = "macos")]
        {
            provider::publish_discovered(&owner, root.path(), &session).await;
            let provider_event = tokio::time::timeout(Duration::from_secs(5), async {
                loop { let event = next_kind(&mut machine, "sessionMetadataChanged").await; if event["payload"]["agentType"] == "omo" { break event; } }
            }).await.expect("validated provider event");
            assert_eq!(provider_event["payload"]["target"], session["target"]);
            assert!(provider_event["payload"]["providerSession"]["id"].is_string());
        }
        #[cfg(unix)]
        handover::assert_retained_owner(owner.clone(), root.path(), &session).await;
        let closed = client.delete(format!("{base}/api/v1/sessions/{id}")).bearer_auth("owner-token")
            .body(json!({"requestId":uuid::Uuid::new_v4().to_string(),"daemonEpoch":session["target"]["daemonEpoch"]}).to_string()).send().await.expect("close response");
        assert_eq!(closed.status(), 204);
        let exited = next_kind(&mut machine, "sessionExited").await;
        assert_eq!(exited["payload"]["session"]["title"], event["payload"]["title"]);
        assert_eq!(exited["payload"]["session"]["cwd"], event["payload"]["cwd"]);
        let detail: Value = serde_json::from_str(&client.get(format!("{base}/api/v1/sessions/{id}")).bearer_auth("owner-token").send().await.expect("detail").text().await.expect("detail body")).expect("detail JSON");
        assert_eq!(detail["status"], "exited");
        assert_eq!(detail["session"]["title"], event["payload"]["title"]);
        eprintln!("A12 metadata machine WS + mirror barrier + HTTP retained/exit metadata; stale epoch/forged machine/unowned session/forged provider rejected");
        machine.close(None).await.expect("machine close");
        mirror.close(None).await.expect("mirror close");
    }).catch_unwind().await;
    // Cleanup also runs for the expected RED assertion, with bounded task joins.
    for id in owner.terminal_service().list_sessions() {
        tokio::time::timeout(
            Duration::from_secs(10),
            owner.terminal_service().close_session(&id),
        )
        .await
        .expect("bounded owned PTY cleanup")
        .expect("PTY close");
    }
    state.auth_manager.revoke_device("owner");
    state.auth_manager.revoke_device("mirror");
    #[cfg(unix)]
    {
        agent_listener.abort();
        assert!(agent_listener.await.expect_err("listener cancelled").is_cancelled());
    }
    tasks.abort_all();
    tokio::time::timeout(Duration::from_secs(10), async {
        while tasks.join_next().await.is_some() {}
    })
    .await
    .expect("server task cleanup");
    let mut subscriptions = state
        .machine_services
        .as_ref()
        .expect("machine authority")
        .workspaces
        .machine_events
        .subscription_count();
    tokio::time::timeout(
        Duration::from_secs(10),
        subscriptions.wait_for(|count| *count == 0),
    )
    .await
    .expect("event handlers released")
    .expect("subscription state");
    drop(owner);
    drop(state);
    tokio::task::spawn_blocking(move || root.close().expect("remove fixture root"))
        .await
        .expect("fixture cleanup");
    eprintln!("A12 cleanup: owned PTYs closed, grants revoked, listener task joined, machine subscriptions zero, fixture removed");
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}
