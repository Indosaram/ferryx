//! `GET`/`PUT /api/v1/workspace/layout` on the real router over a private loopback listener.
use crate::{
    daemon::server::DaemonServer,
    remote::{
        auth::{DeviceAccessScope, DevicePermission},
        server::create_remote_router,
    },
};
use std::{sync::Arc, time::Duration};

struct Fixture {
    addr: std::net::SocketAddr,
    control: String,
    view: String,
    state: Arc<crate::remote::state::RemoteGatewayState>,
    /// Kept alive: dropping it would delete the auth store the fixture's tokens live in.
    _root: tempfile::TempDir,
    _server: DaemonServer,
    stop: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}

async fn fixture() -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let server = DaemonServer::new_with_paths(
        Some(root.path().join("data/config")),
        Some(root.path().join("data/auth")),
    );
    let state = server.remote_state().clone();
    let pin = state
        .auth_manager
        .create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine)
        .unwrap();
    let (control, _) = state
        .auth_manager
        .exchange_pairing_code(&pin, "layout host")
        .unwrap();
    // A View device cannot hold a Machine grant, so this pairing is Mirror scope: exactly the
    // caller the layout route must refuse.
    let view_pin = state
        .auth_manager
        .create_pairing_code(DevicePermission::View);
    let (view, view_device) = state
        .auth_manager
        .exchange_pairing_code(&view_pin, "layout mirror")
        .unwrap();
    assert_eq!(view_device.access_scope, DeviceAccessScope::Mirror);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let router_state = state.clone();
    let task = tokio::spawn(async move {
        axum::serve(listener, create_remote_router(router_state))
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    Fixture {
        addr,
        control,
        view,
        state,
        _root: root,
        _server: server,
        stop,
        task,
    }
}

impl Fixture {
    fn client() -> reqwest::Client {
        reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(15))
            .build()
            .unwrap()
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }

    async fn close(self) {
        let _ = self.stop.send(());
        self.task.await.unwrap();
    }
}

fn tab(id: &str, session: &str) -> crate::remote::machine_protocol::SharedTab {
    crate::remote::machine_protocol::SharedTab {
        id: id.into(),
        kind: "terminal".into(),
        label: id.into(),
        pinned: false,
        pane_tree: Some(crate::remote::machine_protocol::SharedPane::Leaf {
            leaf_id: format!("leaf-{id}"),
            backend_session_id: session.into(),
        }),
    }
}

#[tokio::test]
async fn layout_route_refuses_a_view_device_for_reads_and_writes() {
    let fixture = fixture().await;
    let client = Fixture::client();
    let read = client
        .get(fixture.url("/api/v1/workspace/layout?workspaceId=ws"))
        .bearer_auth(&fixture.view)
        .send()
        .await
        .unwrap();
    assert_eq!(read.status().as_u16(), 403);
    let read_body: crate::remote::machine_protocol::ErrorEnvelope =
        serde_json::from_str(&read.text().await.unwrap()).unwrap();
    assert_eq!(read_body.error.code, "MACHINE_ACCESS_REQUIRED");

    let write = client
        .put(fixture.url("/api/v1/workspace/layout"))
        .bearer_auth(&fixture.view)
        .json(&serde_json::json!({
            "workspaceId": "ws",
            "baseRevision": 0,
            "tabs": [tab("t1", "s1")],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(write.status().as_u16(), 403);
    let write_body: crate::remote::machine_protocol::ErrorEnvelope =
        serde_json::from_str(&write.text().await.unwrap()).unwrap();
    assert_eq!(write_body.error.code, "MACHINE_ACCESS_REQUIRED");
    // A refused write never reached the store.
    assert_eq!(
        fixture
            .state
            .machine_services
            .as_ref()
            .unwrap()
            .sessions
            .layout("ws")
            .revision,
        0
    );
    fixture.close().await;
}

#[tokio::test]
async fn layout_route_retires_the_read_capability_from_view_devices() {
    let fixture = fixture().await;
    let client = Fixture::client();
    let control = client
        .get(fixture.url("/api/v1/capabilities"))
        .bearer_auth(&fixture.control)
        .send()
        .await
        .unwrap();
    let control: serde_json::Value = serde_json::from_str(&control.text().await.unwrap()).unwrap();
    assert!(control["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .any(|name| name == crate::remote::machine_protocol::LAYOUT_SYNC_CAPABILITY));

    let view = client
        .get(fixture.url("/api/v1/capabilities"))
        .bearer_auth(&fixture.view)
        .send()
        .await
        .unwrap();
    let view: serde_json::Value = serde_json::from_str(&view.text().await.unwrap()).unwrap();
    assert!(!view["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .any(|name| name == crate::remote::machine_protocol::LAYOUT_SYNC_CAPABILITY));
    fixture.close().await;
}

#[tokio::test]
async fn layout_route_round_trips_a_publish_and_a_stale_base_conflict() {
    let fixture = fixture().await;
    let client = Fixture::client();
    let stored = fixture
        .state
        .machine_services
        .as_ref()
        .unwrap()
        .sessions
        .clone();

    let publish = client
        .put(fixture.url("/api/v1/workspace/layout"))
        .bearer_auth(&fixture.control)
        .json(&serde_json::json!({
            "workspaceId": "ws",
            "baseRevision": 0,
            "tabs": [tab("t1", "s1")],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(publish.status().as_u16(), 200);
    let publish: serde_json::Value =
        serde_json::from_str(&publish.text().await.unwrap()).unwrap();
    assert_eq!(publish["layout"]["revision"], 1);
    assert_eq!(publish["layout"]["workspaceId"], "ws");
    assert!(publish.get("conflict").is_none());

    // A second publish from the now-stale base 0 answers 409 with the stored document.
    let conflict = client
        .put(fixture.url("/api/v1/workspace/layout"))
        .bearer_auth(&fixture.control)
        .json(&serde_json::json!({
            "workspaceId": "ws",
            "baseRevision": 0,
            "tabs": [tab("t9", "s9")],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(conflict.status().as_u16(), 409);
    let conflict: serde_json::Value =
        serde_json::from_str(&conflict.text().await.unwrap()).unwrap();
    assert_eq!(conflict["conflict"], true);
    assert_eq!(conflict["layout"]["revision"], 1);
    assert_eq!(conflict["layout"]["tabs"][0]["id"], "t1");
    assert_eq!(stored.layout("ws").revision, 1);

    // A plain read answers the stored document; an afterRevision already behind it returns
    // immediately instead of parking.
    let read = client
        .get(fixture.url("/api/v1/workspace/layout?workspaceId=ws"))
        .bearer_auth(&fixture.control)
        .send()
        .await
        .unwrap();
    assert_eq!(read.status().as_u16(), 200);
    let read: serde_json::Value = serde_json::from_str(&read.text().await.unwrap()).unwrap();
    assert_eq!(read["layout"]["revision"], 1);

    let ahead = client
        .get(fixture.url("/api/v1/workspace/layout?workspaceId=ws&afterRevision=0"))
        .bearer_auth(&fixture.control)
        .send()
        .await
        .unwrap();
    assert_eq!(ahead.status().as_u16(), 200);
    let ahead: serde_json::Value = serde_json::from_str(&ahead.text().await.unwrap()).unwrap();
    assert_eq!(ahead["layout"]["revision"], 1);
    fixture.close().await;
}
