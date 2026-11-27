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
        pane_tree: Some(crate::remote::machine_protocol::SharedPane::leaf(
            format!("leaf-{id}"),
            session,
        )),
    }
}

fn deep_tab(depth: usize) -> crate::remote::machine_protocol::SharedTab {
    use crate::remote::machine_protocol::{SharedPane, SharedPaneDirection};
    let mut tab = tab("deep", "s");
    for index in 1..depth {
        tab.pane_tree = Some(SharedPane::Split {
            direction: SharedPaneDirection::Horizontal,
            ratio: 0.5,
            first: Box::new(tab.pane_tree.take().unwrap()),
            second: Box::new(SharedPane::leaf(format!("side-{index}"), "s")),
        });
    }
    tab
}

#[tokio::test]
async fn layout_route_rejects_invalid_workspace_ids_on_reads_and_writes() {
    let fixture = fixture().await;
    let client = Fixture::client();
    let mut ids = vec!["", " ws", "ws ", "../escape", "a..b", "a/b", "a\\b", "a\nb", "a\0b"]
        .into_iter().map(str::to_owned).collect::<Vec<_>>();
    ids.push("a".repeat(129));
    for workspace_id in ids {
        for after_revision in [None, Some("0")] {
            let mut query = vec![("workspaceId", workspace_id.as_str())];
            if let Some(after) = after_revision {
                query.push(("afterRevision", after));
            }
            let read = client.get(fixture.url("/api/v1/workspace/layout"))
                .query(&query).bearer_auth(&fixture.control).send().await.unwrap();
            assert_eq!(read.status().as_u16(), 400, "{workspace_id:?}");
            let body: crate::remote::machine_protocol::ErrorEnvelope = read.json().await.unwrap();
            assert_eq!(body.error.code, "INVALID_REQUEST");
        }
        let write = client.put(fixture.url("/api/v1/workspace/layout"))
            .bearer_auth(&fixture.control)
            .json(&serde_json::json!({"workspaceId": workspace_id, "baseRevision": 0, "tabs": []}))
            .send().await.unwrap();
        assert_eq!(write.status().as_u16(), 400, "{workspace_id:?}");
        let body: crate::remote::machine_protocol::ErrorEnvelope = write.json().await.unwrap();
        assert_eq!(body.error.code, "INVALID_REQUEST");
        assert_eq!(fixture.state.machine_services.as_ref().unwrap().sessions.layout(&workspace_id).revision, 0);
    }
    fixture.close().await;
}

#[tokio::test]
async fn layout_route_rejects_oversized_or_invalid_tabs_without_storing() {
    use crate::remote::machine_protocol::{SharedPane, SharedPaneDirection};
    let fixture = fixture().await;
    let client = Fixture::client();
    let too_many = (0..129).map(|index| tab(&format!("t-{index}"), "s")).collect::<Vec<_>>();
    let mut empty_leaf = tab("t", "s");
    empty_leaf.pane_tree = Some(SharedPane::leaf("", "s"));
    let mut duplicate_leaves = tab("t", "s");
    let leaf = duplicate_leaves.pane_tree.clone().unwrap();
    duplicate_leaves.pane_tree = Some(SharedPane::Split {
        direction: SharedPaneDirection::Horizontal, ratio: 0.5,
        first: Box::new(leaf.clone()), second: Box::new(leaf),
    });
    for tabs in [too_many, vec![deep_tab(17)], vec![tab("", "s")],
        vec![tab("t", "s"), tab("t", "s")], vec![empty_leaf], vec![duplicate_leaves]] {
        let write = client.put(fixture.url("/api/v1/workspace/layout"))
            .bearer_auth(&fixture.control)
            .json(&serde_json::json!({"workspaceId": "ws", "baseRevision": 0, "tabs": tabs}))
            .send().await.unwrap();
        assert_eq!(write.status().as_u16(), 400);
        let body: crate::remote::machine_protocol::ErrorEnvelope = write.json().await.unwrap();
        assert_eq!(body.error.code, "INVALID_REQUEST");
        assert_eq!(fixture.state.machine_services.as_ref().unwrap().sessions.layout("ws").revision, 0);
    }
    // Exact tab/depth/id limits remain usable without any workspace registration.
    let mut tabs = (0..127).map(|index| tab(&format!("t-{index}"), "s")).collect::<Vec<_>>();
    tabs.push(deep_tab(16));
    let write = client.put(fixture.url("/api/v1/workspace/layout"))
        .bearer_auth(&fixture.control)
        .json(&serde_json::json!({"workspaceId": "a".repeat(128), "baseRevision": 0, "tabs": tabs}))
        .send().await.unwrap();
    assert_eq!(write.status().as_u16(), 200);
    fixture.close().await;
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

#[tokio::test]
async fn layout_route_keeps_worktree_buckets_independent() {
    let fixture = fixture().await;
    let client = Fixture::client();
    let stored = fixture
        .state
        .machine_services
        .as_ref()
        .unwrap()
        .sessions
        .clone();

    for (key, tab_id) in [("main", "main-1"), ("wt:feature-a", "feature-1")] {
        let write = client
            .put(fixture.url("/api/v1/workspace/layout"))
            .bearer_auth(&fixture.control)
            .json(&serde_json::json!({
                "workspaceId": "ws",
                "worktreeKey": key,
                "baseRevision": 0,
                "tabs": [tab(tab_id, "s1")],
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(write.status().as_u16(), 200, "{key}");
        let body: serde_json::Value = serde_json::from_str(&write.text().await.unwrap()).unwrap();
        assert_eq!(body["layout"]["revision"], 1, "{key}");
        assert_eq!(body["layout"]["worktreeKey"], key);
    }

    // Each bucket answers its own document and its own revision.
    for (key, tab_id) in [("main", "main-1"), ("wt:feature-a", "feature-1")] {
        let read = client
            .get(fixture.url("/api/v1/workspace/layout"))
            .query(&[("workspaceId", "ws"), ("worktreeKey", key)])
            .bearer_auth(&fixture.control)
            .send()
            .await
            .unwrap();
        assert_eq!(read.status().as_u16(), 200, "{key}");
        let read: serde_json::Value = serde_json::from_str(&read.text().await.unwrap()).unwrap();
        assert_eq!(read["layout"]["revision"], 1, "{key}");
        assert_eq!(read["layout"]["tabs"][0]["id"], tab_id);
        assert_eq!(read["layout"]["worktreeKey"], key);
    }

    // The workspace-level document was never written by either bucket.
    assert_eq!(stored.layout("ws").revision, 0);
    assert_eq!(stored.layout_for("ws", Some("main")).revision, 1);
    assert_eq!(stored.layout_for("ws", Some("wt:feature-a")).revision, 1);

    // A stale base in one bucket conflicts with that bucket's document only.
    let conflict = client
        .put(fixture.url("/api/v1/workspace/layout"))
        .bearer_auth(&fixture.control)
        .json(&serde_json::json!({
            "workspaceId": "ws",
            "worktreeKey": "wt:feature-a",
            "baseRevision": 0,
            "tabs": [tab("stale", "s9")],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(conflict.status().as_u16(), 409);
    let conflict: serde_json::Value = serde_json::from_str(&conflict.text().await.unwrap()).unwrap();
    assert_eq!(conflict["layout"]["tabs"][0]["id"], "feature-1");
    assert_eq!(stored.layout_for("ws", Some("main")).tabs, vec![tab("main-1", "s1")]);
    fixture.close().await;
}

#[tokio::test]
async fn layout_route_rejects_path_like_worktree_keys_and_file_paths() {
    let fixture = fixture().await;
    let client = Fixture::client();
    let stored = fixture
        .state
        .machine_services
        .as_ref()
        .unwrap()
        .sessions
        .clone();

    for key in ["", "/Users/alice/repo", "C:\\repo", "wt:a:b", "wt:../x", "wt:a/b", "ext:zz"] {
        let read = client
            .get(fixture.url("/api/v1/workspace/layout"))
            .query(&[("workspaceId", "ws"), ("worktreeKey", key)])
            .bearer_auth(&fixture.control)
            .send()
            .await
            .unwrap();
        assert_eq!(read.status().as_u16(), 400, "{key:?}");
        let body: crate::remote::machine_protocol::ErrorEnvelope = read.json().await.unwrap();
        assert_eq!(body.error.code, "INVALID_REQUEST");

        let write = client
            .put(fixture.url("/api/v1/workspace/layout"))
            .bearer_auth(&fixture.control)
            .json(&serde_json::json!({
                "workspaceId": "ws",
                "worktreeKey": key,
                "baseRevision": 0,
                "tabs": [],
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(write.status().as_u16(), 400, "{key:?}");
        let body: crate::remote::machine_protocol::ErrorEnvelope = write.json().await.unwrap();
        assert_eq!(body.error.code, "INVALID_REQUEST");
    }
    assert_eq!(stored.layout("ws").revision, 0);

    // A leaf's file path and pane kind are validated on the way in.
    use crate::remote::machine_protocol::SharedPane;
    let file_leaf = |path: &str| {
        let mut tab = tab("t-file", "");
        tab.kind = "file".into();
        tab.pane_tree = Some(SharedPane::Leaf {
            leaf_id: "leaf-file".into(),
            backend_session_id: String::new(),
            pane_kind: Some("file".into()),
            browser_id: None,
            url: None,
            file_rel_path: Some(path.into()),
            dag_run_id: None,
            host_id: None,
        });
        tab
    };
    for path in ["/etc/passwd", "../secret", "C:\\repo", "a\\b"] {
        let write = client
            .put(fixture.url("/api/v1/workspace/layout"))
            .bearer_auth(&fixture.control)
            .json(&serde_json::json!({
                "workspaceId": "ws",
                "worktreeKey": "main",
                "baseRevision": 0,
                "tabs": [file_leaf(path)],
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(write.status().as_u16(), 400, "{path:?}");
        let body: crate::remote::machine_protocol::ErrorEnvelope = write.json().await.unwrap();
        assert_eq!(body.error.code, "INVALID_REQUEST");
    }
    // An unknown pane kind is refused the same way.
    let mut unknown_kind = file_leaf("src/main.rs");
    if let Some(SharedPane::Leaf { pane_kind, .. }) = &mut unknown_kind.pane_tree {
        *pane_kind = Some("editor".into());
    }
    let write = client
        .put(fixture.url("/api/v1/workspace/layout"))
        .bearer_auth(&fixture.control)
        .json(&serde_json::json!({
            "workspaceId": "ws",
            "worktreeKey": "main",
            "baseRevision": 0,
            "tabs": [unknown_kind],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(write.status().as_u16(), 400);
    assert_eq!(stored.layout_for("ws", Some("main")).revision, 0);
    fixture.close().await;
}

#[tokio::test]
async fn layout_route_round_trips_legacy_json_and_leaf_metadata() {
    let fixture = fixture().await;
    let client = Fixture::client();
    let stored = fixture
        .state
        .machine_services
        .as_ref()
        .unwrap()
        .sessions
        .clone();

    // Exactly the bytes a legacy `layoutSyncV1` publisher writes: no `worktreeKey`, no leaf
    // metadata. It lands in the workspace-level bucket.
    let legacy = serde_json::json!({
        "workspaceId": "ws",
        "baseRevision": 0,
        "tabs": [{
            "id": "t1", "kind": "terminal", "label": "shell", "pinned": false,
            "paneTree": {"type": "leaf", "leafId": "l1", "backendSessionId": "s1"},
        }],
    });
    let write = client
        .put(fixture.url("/api/v1/workspace/layout"))
        .bearer_auth(&fixture.control)
        .json(&legacy)
        .send()
        .await
        .unwrap();
    assert_eq!(write.status().as_u16(), 200);
    let write: serde_json::Value = serde_json::from_str(&write.text().await.unwrap()).unwrap();
    assert_eq!(write["layout"]["revision"], 1);
    assert!(
        write["layout"].get("worktreeKey").is_none(),
        "an absent bucket stays absent on the wire"
    );
    assert_eq!(
        write["layout"]["tabs"][0]["paneTree"],
        serde_json::json!({"type": "leaf", "leafId": "l1", "backendSessionId": "s1"})
    );
    assert_eq!(stored.layout("ws").revision, 1);

    // A metadata leaf in a worktree bucket round-trips every field losslessly.
    let write = client
        .put(fixture.url("/api/v1/workspace/layout"))
        .bearer_auth(&fixture.control)
        .json(&serde_json::json!({
            "workspaceId": "ws",
            "worktreeKey": "ext:0123456789abcdef",
            "baseRevision": 0,
            "tabs": [{
                "id": "t-file", "kind": "file", "label": "main.rs", "pinned": false,
                "paneTree": {
                    "type": "leaf", "leafId": "leaf-file", "backendSessionId": "",
                    "paneKind": "file", "browserId": "b1",
                    "url": "https://example.com/a?b=c", "fileRelPath": "src/main.rs",
                    "dagRunId": "run-42", "hostId": "host-1",
                },
            }],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(write.status().as_u16(), 200);
    let write: serde_json::Value = serde_json::from_str(&write.text().await.unwrap()).unwrap();
    assert_eq!(
        write["layout"]["tabs"][0]["paneTree"],
        serde_json::json!({
            "type": "leaf", "leafId": "leaf-file", "backendSessionId": "",
            "paneKind": "file", "browserId": "b1",
            "url": "https://example.com/a?b=c", "fileRelPath": "src/main.rs",
            "dagRunId": "run-42", "hostId": "host-1",
        })
    );
    let read = client
        .get(fixture.url("/api/v1/workspace/layout"))
        .query(&[("workspaceId", "ws"), ("worktreeKey", "ext:0123456789abcdef")])
        .bearer_auth(&fixture.control)
        .send()
        .await
        .unwrap();
    assert_eq!(read.status().as_u16(), 200);
    let read: serde_json::Value = serde_json::from_str(&read.text().await.unwrap()).unwrap();
    assert_eq!(read["layout"]["tabs"][0]["paneTree"], write["layout"]["tabs"][0]["paneTree"]);
    fixture.close().await;
}
