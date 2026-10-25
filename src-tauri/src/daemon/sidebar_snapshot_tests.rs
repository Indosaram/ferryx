use super::*;
use crate::daemon::protocol::{DaemonRequest, DaemonResponse};
use crate::daemon::server::DaemonServer;
use crate::remote::auth::{DeviceAccessScope, DevicePermission};
use serde_json::json;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn model(label: &str) -> Value {
    json!({"schemaVersion": 1, "revision": 0, "generatedAt": "2026-10-24T00:00:00Z",
        "source": "desktop", "projects": [{"groupId": "g1", "label": label}],
        "collapsedGroupIds": [], "advancedFeatures": false, "emptyState": null})
}

struct Fixture {
    root: tempfile::TempDir,
    server: Arc<DaemonServer>,
    token: String,
    mirror_token: String,
}

async fn fixture() -> Fixture {
    crate::ipc::run_blocking(|| {
        let root = tempfile::tempdir().unwrap();
        let server = Arc::new(DaemonServer::new_with_paths(
            Some(root.path().join("data/config")),
            Some(root.path().join("data/auth")),
        ));
        let auth = &server.remote_state().auth_manager;
        let pin = auth
            .create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine)
            .unwrap();
        let (token, _) = auth.exchange_pairing_code(&pin, "sidebar machine").unwrap();
        let pin = auth
            .create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Mirror)
            .unwrap();
        let (mirror_token, _) = auth.exchange_pairing_code(&pin, "sidebar mirror").unwrap();
        Ok(Fixture {
            root,
            server,
            token,
            mirror_token,
        })
    })
    .await
    .unwrap()
}

/// One data-protocol request over handle_client, as the desktop's daemon client sends it.
async fn exchange(server: &Arc<DaemonServer>, request: &DaemonRequest) -> DaemonResponse {
    let mut frame = serde_json::to_value(request).unwrap();
    if let (Some(token), Value::Object(map)) = (server.transport_token(), &mut frame) {
        map.insert("token".into(), token.into());
    }
    let (client, server_side) = tokio::io::duplex(2 * 1024 * 1024);
    let handler = Arc::clone(server);
    let serving = tokio::spawn(async move { handler.handle_client(server_side).await });
    let (read_half, mut write_half) = tokio::io::split(client);
    write_half
        .write_all(format!("{frame}\n").as_bytes())
        .await
        .unwrap();
    let mut line = String::new();
    tokio::time::timeout(
        Duration::from_secs(10),
        BufReader::new(read_half).read_line(&mut line),
    )
    .await
    .unwrap()
    .unwrap();
    drop(write_half);
    serving.abort();
    serde_json::from_str(line.trim()).unwrap()
}

async fn get_sidebar(server: &Arc<DaemonServer>, token: Option<&str>) -> (u16, Value) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = crate::remote::server::create_remote_router(server.remote_state().clone());
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let gateway = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _stopped = stopped.await;
            })
            .await
            .unwrap();
    });
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    let request = client.get(format!("http://{addr}/api/v1/workspace/sidebar"));
    let request = match token {
        Some(token) => request.bearer_auth(token),
        None => request,
    };
    let response = request.send().await.unwrap();
    let status = response.status().as_u16();
    let body = response.json::<Value>().await.unwrap();
    stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(10), gateway)
        .await
        .unwrap()
        .unwrap();
    (status, body)
}

async fn close(fixture: Fixture) {
    crate::ipc::run_blocking(move || {
        drop(fixture.server);
        fixture.root.close().unwrap();
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn sidebar_route_serves_null_before_sync_then_the_synced_model() {
    let fixture = fixture().await;
    let (status, body) = get_sidebar(&fixture.server, Some(&fixture.token)).await;
    assert_eq!((status, body), (200, json!({"model": null})));

    let response = exchange(
        &fixture.server,
        &DaemonRequest::SyncDesktopSidebar {
            model: model("ferryx"),
        },
    )
    .await;
    assert!(
        matches!(
            response,
            DaemonResponse::SyncDesktopSidebarOk { revision: 1 }
        ),
        "{response:?}"
    );
    let (status, body) = get_sidebar(&fixture.server, Some(&fixture.token)).await;
    let mut expected = model("ferryx");
    expected["revision"] = json!(1);
    assert_eq!((status, body), (200, json!({"model": expected})));
    close(fixture).await;
}

#[tokio::test]
async fn sidebar_route_refuses_missing_and_mirror_credentials() {
    let fixture = fixture().await;
    let (status, body) = get_sidebar(&fixture.server, None).await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (401, Some("UNAUTHORIZED"))
    );
    let (status, body) = get_sidebar(&fixture.server, Some(&fixture.mirror_token)).await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (403, Some("MACHINE_ACCESS_REQUIRED"))
    );
    close(fixture).await;
}

#[tokio::test]
async fn sidebar_sync_rejects_oversize_and_bad_schema_with_structured_errors() {
    let fixture = fixture().await;
    let mut oversize = model("big");
    oversize["padding"] = json!("x".repeat(SIDEBAR_MODEL_MAX_BYTES));
    let mut wrong_version = model("v2");
    wrong_version["schemaVersion"] = json!(2);
    let cases = [
        (oversize, "PAYLOAD_TOO_LARGE"),
        (wrong_version, "INVALID_REQUEST"),
        (json!(["not", "an", "object"]), "INVALID_REQUEST"),
    ];
    for (rejected, expected) in cases {
        match exchange(
            &fixture.server,
            &DaemonRequest::SyncDesktopSidebar { model: rejected },
        )
        .await
        {
            DaemonResponse::Error { code, message, .. } => {
                assert_eq!(code.as_deref(), Some(expected));
                assert!(!message.is_empty());
                // The desktop parses the code into a typed IpcError, never the message.
                let parsed = crate::ipc::IpcErrorCode::from_code_str(expected);
                assert_ne!(parsed, crate::ipc::IpcErrorCode::InternalError);
            }
            other => panic!("expected a structured refusal for {expected}, got {other:?}"),
        }
    }
    let (_, body) = get_sidebar(&fixture.server, Some(&fixture.token)).await;
    assert_eq!(
        body,
        json!({"model": null}),
        "a refused model is never served"
    );
    close(fixture).await;
}

#[tokio::test]
async fn sidebar_changed_is_published_once_per_accepted_store() {
    let fixture = fixture().await;
    let service = Arc::clone(
        &fixture
            .server
            .remote_state()
            .machine_services
            .as_ref()
            .unwrap()
            .workspaces,
    );
    let mut events = service.machine_events.subscribe();
    let mut wrong_version = model("v0");
    wrong_version["schemaVersion"] = json!(0);
    for request in [model("a"), wrong_version, model("b")] {
        exchange(
            &fixture.server,
            &DaemonRequest::SyncDesktopSidebar { model: request },
        )
        .await;
    }
    let mut seen = Vec::new();
    while let Ok(event) = events.try_recv() {
        seen.push((event["type"].clone(), event["payload"].clone()));
    }
    assert_eq!(
        seen,
        vec![
            (json!("sidebarChanged"), json!({"revision": 1})),
            (json!("sidebarChanged"), json!({"revision": 2})),
        ]
    );
    drop(service);
    close(fixture).await;
}

#[tokio::test]
async fn sidebar_snapshot_survives_reload_from_disk() {
    crate::ipc::run_blocking(|| {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("data/desktop-sidebar.v1.json");
        let store = SidebarSnapshotStore::load(path.clone());
        assert!(store.model().is_none());
        let events = crate::remote::machine_events::MachineEvents::default();
        assert_eq!(store.store(model("first"), &events).unwrap(), 1);
        assert_eq!(store.store(model("second"), &events).unwrap(), 2);
        drop(store);
        let reloaded = SidebarSnapshotStore::load(path);
        let served = reloaded.model().unwrap();
        assert_eq!(served["projects"][0]["label"], "second");
        assert_eq!(served["revision"], 2);
        assert_eq!(
            reloaded.store(model("third"), &events).unwrap(),
            3,
            "revisions continue after reload"
        );
        drop(reloaded);
        root.close().unwrap();
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn sidebar_snapshot_ignores_a_corrupt_file() {
    crate::ipc::run_blocking(|| {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("desktop-sidebar.v1.json");
        std::fs::write(&path, b"{not json").unwrap();
        let store = SidebarSnapshotStore::load(path);
        assert!(store.model().is_none());
        let events = crate::remote::machine_events::MachineEvents::default();
        assert_eq!(store.store(model("fresh"), &events).unwrap(), 1);
        drop(store);
        root.close().unwrap();
        Ok(())
    })
    .await
    .unwrap();
}

fn live_model(label: &str) -> Value {
    let mut published = model(label);
    published["signals"] = json!({"unreadWorktreePaths": {"/p": true}, "agentStateByPath": {},
        "activityByWorktreePath": {}, "attentionRows": []});
    published
}

#[tokio::test]
async fn sidebar_snapshot_never_reloads_live_signals_from_disk() {
    crate::ipc::run_blocking(|| {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("data/desktop-sidebar.v1.json");
        let store = SidebarSnapshotStore::load(path.clone());
        let events = crate::remote::machine_events::MachineEvents::default();
        store.desktop_stream_opened(&events);
        store.store(live_model("published"), &events).unwrap();
        assert!(store.model().unwrap().get("signals").is_some());
        drop(store);
        let reloaded = SidebarSnapshotStore::load(path);
        let served = reloaded.model().unwrap();
        assert!(
            served.get("signals").is_none(),
            "a previous desktop session's live state is not served: {served}"
        );
        assert_eq!(served["projects"][0]["label"], "published", "the structure persists");
        drop(reloaded);
        root.close().unwrap();
        Ok(())
    })
    .await
    .unwrap();
}

/// Opens the desktop's event stream the way lib.rs's bridge does and returns the open stream.
async fn open_desktop_stream(
    server: &Arc<DaemonServer>,
) -> (BufReader<tokio::io::DuplexStream>, tokio::task::JoinHandle<()>) {
    let mut frame = serde_json::to_value(&DaemonRequest::SubscribeRemoteEvents).unwrap();
    if let (Some(token), Value::Object(map)) = (server.transport_token(), &mut frame) {
        map.insert("token".into(), token.into());
    }
    let (mut client, server_side) = tokio::io::duplex(64 * 1024);
    let handler = Arc::clone(server);
    let serving = tokio::spawn(async move { handler.handle_client(server_side).await });
    client
        .write_all(format!("{frame}\n").as_bytes())
        .await
        .unwrap();
    let mut reader = BufReader::new(client);
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line))
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        serde_json::from_str::<DaemonResponse>(line.trim()).unwrap(),
        DaemonResponse::SubscribeRemoteEventsOk
    ));
    (reader, serving)
}

#[tokio::test]
async fn sidebar_live_signals_are_dropped_when_the_desktop_disconnects() {
    let fixture = fixture().await;
    let service = Arc::clone(
        &fixture
            .server
            .remote_state()
            .machine_services
            .as_ref()
            .unwrap()
            .workspaces,
    );
    let streams = service.desktop_stream_count();
    let wait_streams = |streams: &tokio::sync::watch::Receiver<usize>, n: usize| {
        let mut streams = streams.clone();
        async move {
            tokio::time::timeout(Duration::from_secs(10), streams.wait_for(|open| *open == n))
                .await
                .expect("desktop stream count")
                .unwrap();
        }
    };

    // Published while no desktop is connected: the live part is never stored.
    exchange(
        &fixture.server,
        &DaemonRequest::SyncDesktopSidebar {
            model: live_model("early"),
        },
    )
    .await;
    let (_, body) = get_sidebar(&fixture.server, Some(&fixture.token)).await;
    assert!(body["model"].get("signals").is_none(), "{body}");

    let (first, _first_serving) = open_desktop_stream(&fixture.server).await;
    let (mut second, _second_serving) = open_desktop_stream(&fixture.server).await;
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(10), second.read_line(&mut line)).await.unwrap().unwrap();
    assert_eq!(serde_json::from_str::<Value>(line.trim()).unwrap()["event"], "desktop_local_projects_resync");
    wait_streams(&streams, 2).await;
    exchange(&fixture.server, &DaemonRequest::SyncDesktopSidebar { model: live_model("desktop-b") }).await;
    let response = exchange(
        &fixture.server,
        &DaemonRequest::SyncDesktopSidebar {
            model: live_model("ferryx"),
        },
    )
    .await;
    assert!(
        matches!(response, DaemonResponse::SyncDesktopSidebarOk { revision: 5 }),
        "{response:?}"
    );
    let (_, body) = get_sidebar(&fixture.server, Some(&fixture.token)).await;
    assert!(body["model"].get("signals").is_some(), "{body}");
    let mut events = service.machine_events.subscribe();

    drop(first);
    wait_streams(&streams, 1).await;
    let (_, body) = get_sidebar(&fixture.server, Some(&fixture.token)).await;
    assert!(body["model"].get("signals").is_none(), "{body}");
    assert_eq!(body["model"]["revision"], 6);
    assert_eq!(events.try_recv().unwrap()["payload"]["revision"], 6);
    line.clear();
    tokio::time::timeout(Duration::from_secs(10), second.read_line(&mut line)).await.unwrap().unwrap();
    let resync: Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(resync["event"], "desktop_local_projects_resync");
    let mut replacement = live_model("desktop-b");
    replacement["signals"]["unreadWorktreePaths"] = json!({"/desktop-b": true});
    exchange(&fixture.server, &DaemonRequest::SyncDesktopSidebar { model: replacement }).await;
    let (_, body) = get_sidebar(&fixture.server, Some(&fixture.token)).await;
    assert_eq!(body["model"]["signals"]["unreadWorktreePaths"], json!({"/desktop-b": true}));
    assert_eq!(events.try_recv().unwrap()["payload"]["revision"], 7);

    // The last desktop stream ends (the app quit): the live part goes, announced once.
    drop(second);
    wait_streams(&streams, 0).await;
    let event = events.try_recv().expect("sidebarChanged after the desktop disconnects");
    assert_eq!(
        (event["type"].clone(), event["payload"].clone()),
        (json!("sidebarChanged"), json!({"revision": 8}))
    );
    assert!(events.try_recv().is_err(), "announced once");
    let (_, body) = get_sidebar(&fixture.server, Some(&fixture.token)).await;
    assert!(body["model"].get("signals").is_none(), "{body}");
    assert_eq!(body["model"]["revision"], 8);
    assert_eq!(body["model"]["projects"][0]["label"], "desktop-b", "the structure stays");
    drop(service);
    close(fixture).await;
}

fn pause_lifecycle(
    store: &SidebarSnapshotStore,
    stage: &'static str,
) -> (std::sync::mpsc::Receiver<()>, std::sync::mpsc::Sender<()>) {
    let (reached, paused) = std::sync::mpsc::channel();
    let (resume, resumed) = std::sync::mpsc::channel();
    let resumed = Mutex::new(Some(resumed));
    *store.lifecycle_probe.write() = Some(Arc::new(move |at| {
        if at == stage {
            let receiver = resumed.lock().take();
            if let Some(receiver) = receiver {
                reached.send(()).unwrap();
                receiver.recv_timeout(Duration::from_secs(10)).unwrap();
            }
        }
        Ok(())
    }));
    (paused, resume)
}

#[tokio::test]
async fn sidebar_publish_cannot_restore_signals_after_final_close() {
    let fixture = fixture().await;
    let service = Arc::clone(&fixture.server.remote_state().machine_services.as_ref().unwrap().workspaces);
    crate::ipc::run_blocking(move || {
        service.sidebar.desktop_stream_opened(&service.machine_events);
        service.sync_desktop_sidebar(live_model("initial")).unwrap();
        let mut events = service.machine_events.subscribe();
        let (paused, resume) = pause_lifecycle(&service.sidebar, "presenceCaptured");
        let publisher = Arc::clone(&service);
        let publish = std::thread::spawn(move || publisher.sync_desktop_sidebar(live_model("racing")));
        paused.recv_timeout(Duration::from_secs(10)).unwrap();

        service.desktop_stream_closed();
        let closed = service.sidebar.model().unwrap();
        let close_event = events.try_recv().unwrap();
        resume.send(()).unwrap();
        let result = publish.join().unwrap();
        *service.sidebar.lifecycle_probe.write() = None;

        assert!(closed.get("signals").is_none());
        assert_eq!(close_event["payload"]["revision"], closed["revision"]);
        let last_model = service.sidebar.model().unwrap();
        assert!(matches!(result, Err(SidebarRejection::StaleLifetime)), "{result:?}");
        assert_eq!(last_model["revision"], closed["revision"]);
        assert!(last_model.get("signals").is_none(), "{last_model}");
        assert_eq!(last_model["projects"][0]["label"], "initial");
        assert!(events.try_recv().is_err());
        Ok(())
    }).await.unwrap();
    close(fixture).await;
}

#[tokio::test]
async fn sidebar_old_publish_cannot_replace_new_lifetime_snapshot() {
    let fixture = fixture().await;
    let service = Arc::clone(&fixture.server.remote_state().machine_services.as_ref().unwrap().workspaces);
    crate::ipc::run_blocking(move || {
        service.sidebar.desktop_stream_opened(&service.machine_events);
        service.sync_desktop_sidebar(live_model("desktop-a")).unwrap();
        let mut events = service.machine_events.subscribe();
        let (paused, resume) = pause_lifecycle(&service.sidebar, "presenceCaptured");
        let publisher = Arc::clone(&service);
        let publish = std::thread::spawn(move || publisher.sync_desktop_sidebar(live_model("old-a")));
        paused.recv_timeout(Duration::from_secs(10)).unwrap();

        service.desktop_stream_closed();
        assert_eq!(events.try_recv().unwrap()["payload"]["revision"], 2);
        service.sidebar.desktop_stream_opened(&service.machine_events);
        assert_eq!(events.try_recv().unwrap()["payload"]["revision"], 3);
        let mut replacement = live_model("desktop-b");
        replacement["signals"]["unreadWorktreePaths"] = json!({"/desktop-b": true});
        let revision = service.sync_desktop_sidebar(replacement).unwrap();
        let accepted = service.sidebar.model().unwrap();
        assert_eq!(events.try_recv().unwrap()["payload"]["revision"], revision);

        resume.send(()).unwrap();
        let result = publish.join().unwrap();
        *service.sidebar.lifecycle_probe.write() = None;

        assert!(matches!(result, Err(SidebarRejection::StaleLifetime)), "{result:?}");
        assert_eq!(SidebarRejection::StaleLifetime.code(), "STALE_EPOCH");
        assert_eq!(accepted["projects"][0]["label"], "desktop-b");
        assert_eq!(accepted["signals"]["unreadWorktreePaths"], json!({"/desktop-b": true}));
        assert_eq!(service.sidebar.model().unwrap(), accepted);
        let persisted: Value = serde_json::from_slice(&std::fs::read(&service.sidebar.path).unwrap()).unwrap();
        assert_eq!(persisted, *accepted);
        assert!(events.try_recv().is_err(), "rejected publish must not announce");
        Ok(())
    }).await.unwrap();
    close(fixture).await;
}

#[tokio::test]
async fn sidebar_old_publish_is_rejected_after_new_lifetime_opens() {
    let fixture = fixture().await;
    let service = Arc::clone(&fixture.server.remote_state().machine_services.as_ref().unwrap().workspaces);
    crate::ipc::run_blocking(move || {
        service.sidebar.desktop_stream_opened(&service.machine_events);
        service.sync_desktop_sidebar(live_model("initial")).unwrap();
        let mut events = service.machine_events.subscribe();
        let (paused, resume) = pause_lifecycle(&service.sidebar, "presenceCaptured");
        let publisher = Arc::clone(&service);
        let publish = std::thread::spawn(move || publisher.sync_desktop_sidebar(live_model("old-a")));
        paused.recv_timeout(Duration::from_secs(10)).unwrap();

        service.desktop_stream_closed();
        let closed = service.sidebar.model().unwrap();
        assert_eq!(events.try_recv().unwrap()["payload"]["revision"], closed["revision"]);
        service.sidebar.desktop_stream_opened(&service.machine_events);
        let opened = service.sidebar.model().unwrap();
        assert_eq!(events.try_recv().unwrap()["payload"]["revision"], opened["revision"]);
        resume.send(()).unwrap();
        let result = publish.join().unwrap();
        *service.sidebar.lifecycle_probe.write() = None;

        assert!(matches!(result, Err(SidebarRejection::StaleLifetime)), "{result:?}");
        assert_eq!(service.sidebar.model().unwrap(), opened);
        let persisted: Value = serde_json::from_slice(&std::fs::read(&service.sidebar.path).unwrap()).unwrap();
        assert_eq!(persisted, *closed);
        assert!(events.try_recv().is_err(), "rejected publish must not announce");
        Ok(())
    }).await.unwrap();
    close(fixture).await;
}

#[tokio::test]
async fn sidebar_final_close_invalidates_even_when_persistence_fails() {
    let fixture = fixture().await;
    let service = Arc::clone(&fixture.server.remote_state().machine_services.as_ref().unwrap().workspaces);
    crate::ipc::run_blocking(move || {
        service.sidebar.desktop_stream_opened(&service.machine_events);
        service.sync_desktop_sidebar(live_model("published")).unwrap();
        let mut events = service.machine_events.subscribe();
        *service.sidebar.lifecycle_probe.write() = Some(Arc::new(|stage| {
            if stage == "dropLivePersist" {
                return Err(std::io::Error::other("injected sidebar persistence failure"));
            }
            Ok(())
        }));

        service.desktop_stream_closed();

        let current = service.sidebar.model().unwrap();
        assert!(current.get("signals").is_none(), "{current}");
        assert_eq!(current["revision"], 2);
        let event = events.try_recv().expect("invalidation even on persistence failure");
        assert_eq!(event["type"], "sidebarChanged");
        assert_eq!(event["payload"]["revision"], current["revision"]);
        assert!(events.try_recv().is_err());
        let persisted: Value = serde_json::from_slice(&std::fs::read(&service.sidebar.path).unwrap()).unwrap();
        assert_eq!(persisted["revision"], 1, "the injected write failed");
        assert!(persisted.get("signals").is_some());
        *service.sidebar.lifecycle_probe.write() = None;
        Ok(())
    }).await.unwrap();
    close(fixture).await;
}

#[tokio::test]
async fn sidebar_nonfinal_close_fences_old_admission_and_survivor_republishes() {
    let fixture = fixture().await;
    let service = Arc::clone(&fixture.server.remote_state().machine_services.as_ref().unwrap().workspaces);
    crate::ipc::run_blocking(move || {
        service.sidebar.desktop_stream_opened(&service.machine_events);
        service.sidebar.desktop_stream_opened(&service.machine_events);
        service.sync_desktop_sidebar(live_model("desktop-b")).unwrap();
        service.sync_desktop_sidebar(live_model("desktop-a")).unwrap();
        let mut epochs = service.sidebar.desktop_epoch();
        let mut events = service.machine_events.subscribe();
        let (paused, resume) = pause_lifecycle(&service.sidebar, "presenceCaptured");
        let publisher = Arc::clone(&service);
        let publish = std::thread::spawn(move || publisher.sync_desktop_sidebar(live_model("old-a")));
        paused.recv_timeout(Duration::from_secs(10)).unwrap();

        service.desktop_stream_closed();
        let invalidated = service.sidebar.model().unwrap();
        resume.send(()).unwrap();
        let result = publish.join().unwrap();
        *service.sidebar.lifecycle_probe.write() = None;

        assert!(invalidated.get("signals").is_none(), "{invalidated}");
        assert_eq!(invalidated["revision"], 3);
        assert_eq!(events.try_recv().unwrap()["payload"]["revision"], 3);
        assert!(epochs.has_changed().unwrap());
        assert_eq!(*epochs.borrow_and_update(), 3);
        assert!(matches!(result, Err(SidebarRejection::StaleLifetime)), "{result:?}");
        assert_eq!(SidebarRejection::StaleLifetime.code(), "STALE_EPOCH");
        let mut replacement = live_model("desktop-b");
        replacement["signals"]["unreadWorktreePaths"] = json!({"/b": true});
        service.sync_desktop_sidebar(replacement).unwrap();
        let current = service.sidebar.model().unwrap();
        assert_eq!(current["signals"]["unreadWorktreePaths"], json!({"/b": true}));
        assert_eq!(current["projects"][0]["label"], "desktop-b");
        Ok(())
    }).await.unwrap();
    close(fixture).await;
}

#[tokio::test]
async fn sidebar_open_during_publish_persistence_fences_memory_commit() {
    let fixture = fixture().await;
    let service = Arc::clone(&fixture.server.remote_state().machine_services.as_ref().unwrap().workspaces);
    crate::ipc::run_blocking(move || {
        service.sidebar.desktop_stream_opened(&service.machine_events);
        service.sync_desktop_sidebar(live_model("initial")).unwrap();
        let (paused, resume) = pause_lifecycle(&service.sidebar, "publishPersist");
        let publisher = Arc::clone(&service);
        let publish = std::thread::spawn(move || publisher.sync_desktop_sidebar(live_model("old-a")));
        paused.recv_timeout(Duration::from_secs(10)).unwrap();
        service.sidebar.desktop_stream_opened(&service.machine_events);
        let opened = service.sidebar.model().unwrap();
        resume.send(()).unwrap();
        let result = publish.join().unwrap();
        *service.sidebar.lifecycle_probe.write() = None;
        assert!(matches!(result, Err(SidebarRejection::StaleLifetime)), "{result:?}");
        assert_eq!(service.sidebar.model().unwrap(), opened);
        assert!(opened.get("signals").is_none());
        service.sync_desktop_sidebar(live_model("current-b")).unwrap();
        assert!(service.sidebar.model().unwrap().get("signals").is_some());
        Ok(())
    }).await.unwrap();
    close(fixture).await;
}

#[tokio::test]
async fn sidebar_open_does_not_wait_for_stalled_close_persistence() {
    let fixture = fixture().await;
    let service = Arc::clone(&fixture.server.remote_state().machine_services.as_ref().unwrap().workspaces);
    crate::ipc::run_blocking(move || {
        service.sidebar.desktop_stream_opened(&service.machine_events);
        service.sync_desktop_sidebar(live_model("published")).unwrap();
        let (paused, resume) = pause_lifecycle(&service.sidebar, "dropLivePersist");
        let closer = Arc::clone(&service);
        let close = std::thread::spawn(move || closer.desktop_stream_closed());
        paused.recv_timeout(Duration::from_secs(10)).unwrap();
        let (opened, opening) = std::sync::mpsc::channel();
        let opener = Arc::clone(&service);

        let open = std::thread::spawn(move || {
            let guard = opener.desktop_stream_opened();
            opened.send(()).unwrap();
            guard
        });
        let result = opening.recv_timeout(Duration::from_secs(2));
        resume.send(()).unwrap();
        close.join().unwrap();
        let guard = open.join().unwrap();
        *service.sidebar.lifecycle_probe.write() = None;
        std::thread::spawn(move || drop(guard)).join().unwrap();

        result.expect("stream open must finish while close persistence is still stalled");
        assert!(service.sidebar.model().unwrap().get("signals").is_none());
        Ok(())
    }).await.unwrap();
    close(fixture).await;
}
