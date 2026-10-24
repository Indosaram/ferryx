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
        assert_eq!(store.store(model("first")).unwrap(), 1);
        assert_eq!(store.store(model("second")).unwrap(), 2);
        drop(store);
        let reloaded = SidebarSnapshotStore::load(path);
        let served = reloaded.model().unwrap();
        assert_eq!(served["projects"][0]["label"], "second");
        assert_eq!(served["revision"], 2);
        assert_eq!(
            reloaded.store(model("third")).unwrap(),
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
        assert_eq!(store.store(model("fresh")).unwrap(), 1);
        drop(store);
        root.close().unwrap();
        Ok(())
    })
    .await
    .unwrap();
}
