//! Real predecessor route, with no gateway-local PTY or remote path probe.
use ferryx_lib::daemon::server::DaemonServer;
use serde_json::{json, Value};
use std::{path::Path, sync::Arc, time::Duration};

pub async fn assert_retained_owner(old: Arc<DaemonServer>, root: &Path, session: &Value) {
    // Given: the original daemon still owns the already metadata-bearing PTY.
    let id = session["target"]["sessionId"].as_str().expect("session ID");
    let (legacy, _, listener) = old
        .handover_manager
        .prepare_handover(old.terminal_service())
        .expect("prepare real handover");
    let predecessor = old.clone();
    let mut tasks = tokio::task::JoinSet::new();
    let (disconnect, mut disconnects) = tokio::sync::mpsc::channel::<tokio::sync::oneshot::Sender<()>>(1);
    let accepting = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let admission = accepting.clone();
    tasks.spawn(async move {
        let mut clients = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                accepted = listener.accept() => {
                    let (stream, _) = accepted.expect("legacy accept");
                    if admission.load(std::sync::atomic::Ordering::Acquire) { clients.spawn(predecessor.clone().handle_client(stream)); }
                },
                Some(joined) = clients.join_next() => { joined.expect("legacy client"); }
                Some(drained) = disconnects.recv() => {
                    clients.abort_all();
                    while clients.join_next().await.is_some() {}
                    drained.send(()).expect("disconnect observed");
                }
            }
        }
    });
    let path = root.to_owned();
    let successor = tokio::task::spawn_blocking(move || {
        Arc::new(DaemonServer::new_with_paths(
            Some(path.join("config")),
            Some(path.join("auth")),
        ))
    })
    .await
    .expect("successor");
    let epoch = session["target"]["daemonEpoch"]
        .as_str()
        .expect("epoch")
        .parse::<u64>()
        .expect("numeric epoch");
    successor
        .remote_state()
        .daemon_epoch
        .store(epoch + 1, std::sync::atomic::Ordering::Release);
    successor
        .session_router
        .add_legacy_peer(Arc::new(ferryx_lib::daemon::proxy::LegacyPeer::new(
            legacy,
            vec![id.into()],
        )));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("gateway listener");
    let addr = listener.local_addr().expect("gateway address");
    let router = ferryx_lib::remote::server::create_remote_router(successor.remote_state().clone());
    tasks.spawn(async move {
        axum::serve(listener, router).await.expect("gateway HTTP");
    });
    // When: the successor describes the old target through real owner IPC.
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("client");
    let response = client
        .get(format!("http://{addr}/api/v1/sessions/{id}"))
        .bearer_auth("owner-token")
        .send()
        .await
        .expect("routed detail");
    let status = response.status();
    let body = response.text().await.expect("detail body");
    let mut live = super::connect_events(&client, &format!("http://{addr}"), "owner-token").await;
    let mut forwarder_count = successor.remote_state().machine_services.as_ref().expect("authority").workspaces.machine_events.forwarder_count();
    let mut owner_count = old.remote_state().machine_services.as_ref().expect("authority").workspaces.machine_events.owner_stream_count();
    let boundary = super::next_kind(&mut live, "inventoryInvalidated").await;
    assert_eq!(boundary["revision"], boundary["sequence"]);
    old.terminal_service()
        .write_input(id, b"printf '\\033]2;A12-successor-live\\007'\n")
        .expect("predecessor live change");
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let event = super::next_kind(&mut live, "sessionMetadataChanged").await;
            if event["payload"]["title"] == "A12-successor-live" {
                assert_eq!(event["payload"]["target"], session["target"]);
                assert_eq!(event["revision"], event["sequence"]);
                assert!(event["sessionRevision"].is_string());
                break;
            }
        }
    })
    .await
    .expect("predecessor change reaches successor WS");
    tokio::time::timeout(Duration::from_secs(5), forwarder_count.wait_for(|n| *n == 1)).await.expect("one forwarder").expect("count");
    tokio::time::timeout(Duration::from_secs(5), owner_count.wait_for(|n| *n == 1)).await.expect("one owner stream").expect("count");
    let (drained, drain) = tokio::sync::oneshot::channel();
    let started = std::time::Instant::now();
    disconnect.send(drained).await.expect("force actual owner IPC close");
    tokio::time::timeout(Duration::from_secs(5), drain).await.expect("owner IPC drain").expect("drained");
    let partial = super::next_kind(&mut live, "inventoryInvalidated").await;
    assert_eq!(partial["payload"]["completeness"], "partial");
    let recovery = tokio::time::timeout(Duration::from_secs(10), async {
        loop { let event = super::next_kind(&mut live, "inventoryInvalidated").await; if event["reason"] == "ownerRecovered" { break event; } }
    }).await.expect("actual owner IPC reconnect publishes recovery");
    assert!(started.elapsed() >= Duration::from_secs(1), "owner retry must honor backoff");
    assert_eq!(recovery["payload"]["completeness"], "complete");
    live.close(None).await.expect("successor events close");
    let mut subscriptions = successor
        .remote_state()
        .machine_services
        .as_ref()
        .expect("authority")
        .workspaces
        .machine_events
        .subscription_count();
    tokio::time::timeout(Duration::from_secs(10), subscriptions.wait_for(|n| *n == 0))
        .await
        .expect("forwarder lifetime")
        .expect("subscription count");
    let mut reconnect =
        super::connect_events(&client, &format!("http://{addr}"), "owner-token").await;
    let snapshot = super::next_kind(&mut reconnect, "inventoryInvalidated").await;
    assert_eq!(
        snapshot["payload"]["sessions"]["sessions"][0]["title"],
        "A12-successor-live"
    );
    let mut mirror =
        super::connect_events(&client, &format!("http://{addr}"), "mirror-token").await;
    old.terminal_service()
        .write_input(id, b"printf '\\033]2;A12-owner-title\\007'\n")
        .expect("restore owner title");
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let event = super::next_kind(&mut reconnect, "sessionMetadataChanged").await;
            if event["payload"]["title"] == "A12-owner-title" {
                break;
            }
        }
    })
    .await
    .expect("reconnected owner feed");
    successor
        .remote_state()
        .event_tx
        .send(json!({"event":"fixtureBarrier","payload":{}}).to_string())
        .expect("mirror barrier");
    use futures_util::StreamExt;
    let frame = tokio::time::timeout(Duration::from_secs(5), mirror.next())
        .await
        .expect("mirror bound")
        .expect("mirror open")
        .expect("frame");
    assert_eq!(
        serde_json::from_str::<Value>(frame.to_text().expect("text")).expect("JSON"),
        json!({"event":"fixtureBarrier","payload":{}})
    );
    accepting.store(false, std::sync::atomic::Ordering::Release);
    let (drained, drain) = tokio::sync::oneshot::channel();
    disconnect.send(drained).await.expect("force owner unavailable");
    tokio::time::timeout(Duration::from_secs(5), drain).await.expect("drain deadline").expect("drain");
    let exhausted = tokio::time::timeout(Duration::from_secs(15), async {
        loop { let event = super::next_kind(&mut reconnect, "inventoryInvalidated").await; if event["reason"] == "ownerUnavailable" { break event; } }
    }).await.expect("finite retry completion observed");
    assert_eq!(exhausted["payload"]["completeness"], "partial");
    assert_eq!(exhausted["payload"]["sessions"]["completeness"], "partial");
    tokio::time::timeout(Duration::from_secs(5), forwarder_count.wait_for(|n| *n == 0)).await.expect("exhausted forwarder drained").expect("count");
    accepting.store(true, std::sync::atomic::Ordering::Release);
    mirror.close(None).await.expect("mirror close");
    reconnect.close(None).await.expect("reconnect close");
    tokio::time::timeout(Duration::from_secs(10), subscriptions.wait_for(|n| *n == 0))
        .await
        .expect("reconnected forwarder cleanup")
        .expect("count");
    tokio::time::timeout(Duration::from_secs(5), forwarder_count.wait_for(|n| *n == 0)).await.expect("every forwarder cancelled and dropped").expect("count");
    tokio::time::timeout(Duration::from_secs(5), owner_count.wait_for(|n| *n == 0)).await.expect("every owner IPC stream drained").expect("count");
    eprintln!("A12 actual IPC disconnect/drain, >=1s reconnect backoff, authoritative partial/recovery; successor redial/mirror redaction; forwarders=0 owner_streams=0 subscriptions=0");
    // Join all owned listeners before assertions, including the failure path.
    tasks.abort_all();
    tokio::time::timeout(Duration::from_secs(10), async {
        while tasks.join_next().await.is_some() {}
    })
    .await
    .expect("listener joins");
    // Restore the fixture owner to active before its final PTY is closed: the
    // production draining owner otherwise correctly exits this entire test process.
    old.handover_manager
        .abort_handover()
        .expect("restore fixture owner lifecycle");
    // Then: retained metadata belongs to the predecessor, not the gateway epoch.
    assert_eq!(status, 200, "{body}");
    let detail: Value = serde_json::from_str(&body).expect("detail JSON");
    assert_eq!(detail["session"]["target"], session["target"]);
    assert_eq!(detail["session"]["title"], "A12-owner-title");
    assert_eq!(detail["status"], "running");
    assert!(successor.terminal_service().list_sessions().is_empty());
    assert_ne!(
        json!((epoch + 1).to_string()),
        detail["session"]["target"]["daemonEpoch"]
    );
    eprintln!("A12 real predecessor IPC/HTTP: retained title, exact original target, distinct gateway epoch, gateway PTY list empty; gateway/legacy listener tasks joined");
}
