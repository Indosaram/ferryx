//! Layout store behavior: revisions, stale-base conflict, per-workspace isolation and the
//! long-poll wake path. No wall-clock sleeps: the waiter is registered before the write, so the
//! only ordering the test relies on is that its own write happens after it subscribed.
use super::*;
use crate::remote::machine_protocol::{SharedPane, SharedPaneDirection, SharedLayout, SharedTab};
use std::time::Duration;

fn service() -> crate::daemon::server::DaemonServer {
    crate::daemon::server::DaemonServer::new_with_paths(None, None)
}

fn tab(id: &str, session: &str) -> SharedTab {
    SharedTab {
        id: id.into(),
        kind: "terminal".into(),
        label: id.into(),
        pinned: false,
        pane_tree: Some(SharedPane::Leaf {
            leaf_id: format!("leaf-{id}"),
            backend_session_id: session.into(),
        }),
    }
}

fn split_tab(id: &str) -> SharedTab {
    SharedTab {
        id: id.into(),
        kind: "terminal".into(),
        label: id.into(),
        pinned: true,
        pane_tree: Some(SharedPane::Split {
            direction: SharedPaneDirection::Horizontal,
            ratio: 0.5,
            first: Box::new(SharedPane::Leaf {
                leaf_id: format!("leaf-{id}-a"),
                backend_session_id: "session-a".into(),
            }),
            second: Box::new(SharedPane::Leaf {
                leaf_id: format!("leaf-{id}-b"),
                backend_session_id: "session-b".into(),
            }),
        }),
    }
}

// DaemonServer starts background tasks, so it needs a Tokio runtime.
#[tokio::test]
async fn layout_store_starts_empty_at_revision_zero() {
    let service = service().session_service;
    let layout = service.layout("ws-unknown");
    assert_eq!(layout.workspace_id, "ws-unknown");
    assert_eq!(layout.revision, 0);
    assert!(layout.tabs.is_empty());
    // Reading does not create the document: a first publish still writes revision 1.
    let stored = service
        .put_layout("ws-unknown", 0, vec![tab("t1", "s1")])
        .expect("revision 0 is the empty document's revision");
    assert_eq!(stored.revision, 1);
}

// DaemonServer starts background tasks, so it needs a Tokio runtime.
#[tokio::test]
async fn layout_store_put_bumps_revision() {
    let service = service().session_service;
    let first = service
        .put_layout("ws", 0, vec![tab("t1", "s1")])
        .expect("first publish");
    assert_eq!((first.revision, first.tabs.len()), (1, 1));
    let second = service
        .put_layout("ws", 1, vec![tab("t1", "s1"), split_tab("t2")])
        .expect("second publish");
    assert_eq!(second.revision, 2);
    assert_eq!(second.tabs.len(), 2);
    // The nested split survives the round trip unchanged.
    assert_eq!(second.tabs[1], split_tab("t2"));
    assert_eq!(service.layout("ws"), second);
}

// DaemonServer starts background tasks, so it needs a Tokio runtime.
#[tokio::test]
async fn layout_store_put_with_stale_base_conflicts_and_keeps_current() {
    let service = service().session_service;
    service
        .put_layout("ws", 0, vec![tab("t1", "s1")])
        .expect("first publish");
    let conflict = service
        .put_layout("ws", 0, vec![tab("t9", "s9")])
        .expect_err("base 0 is stale once revision 1 exists");
    assert_eq!(conflict.revision, 1);
    assert_eq!(conflict.tabs, vec![tab("t1", "s1")]);
    // The rejected write left the stored document untouched.
    assert_eq!(service.layout("ws"), conflict);
}

// DaemonServer starts background tasks, so it needs a Tokio runtime.
#[tokio::test]
async fn layout_store_isolates_workspaces() {
    let service = service().session_service;
    service.put_layout("a", 0, vec![tab("a1", "s1")]).expect("a");
    service.put_layout("b", 0, vec![tab("b1", "s2")]).expect("b");
    assert_eq!(service.layout("a").tabs, vec![tab("a1", "s1")]);
    assert_eq!(service.layout("b").tabs, vec![tab("b1", "s2")]);
    // A write to one workspace does not advance the other's revision.
    service.put_layout("a", 1, vec![tab("a2", "s3")]).expect("a2");
    assert_eq!(service.layout("b").revision, 1);
}

#[tokio::test]
async fn layout_store_long_poll_wakes_on_put() {
    use futures_util::FutureExt;
    let service = service().session_service;
    let mut waiter = Box::pin(service.wait_layout("ws", 0, Duration::from_secs(30)));
    // The first poll subscribes and then parks: the loop only re-reads once the change signal
    // fires, so a `None` here is proof the poll is parked and not spinning on a stale read.
    assert!(
        waiter.as_mut().now_or_never().is_none(),
        "a poll with nothing published must park, not answer"
    );
    service
        .put_layout("ws", 0, vec![tab("t1", "s1")])
        .expect("publish wakes the poll");
    let layout = tokio::time::timeout(Duration::from_secs(5), waiter)
        .await
        .expect("the parked poll answers once the write lands");
    assert_eq!(layout.revision, 1);
}

#[tokio::test]
async fn layout_store_long_poll_answers_at_once_when_already_ahead() {
    let service = service().session_service;
    service.put_layout("ws", 0, vec![tab("t1", "s1")]).expect("publish");
    let layout = service
        .wait_layout("ws", 0, Duration::from_secs(30))
        .await;
    assert_eq!(layout.revision, 1);
}

#[tokio::test]
async fn layout_store_long_poll_times_out_with_the_stored_document() {
    let service = service().session_service;
    service.put_layout("ws", 0, vec![tab("t1", "s1")]).expect("publish");
    // Waiting on the current revision with no writer must end at the timeout, not park forever.
    let layout = service
        .wait_layout("ws", 1, Duration::from_millis(20))
        .await;
    assert_eq!(layout.revision, 1);
}

#[tokio::test]
async fn layout_store_forgets_document_when_workspace_unregisters() {
    let service = service().session_service;
    service
        .put_layout("ws", 0, vec![tab("t1", "s1")])
        .expect("publish");
    service.forget_layout("ws");
    let layout = service.layout("ws");
    assert_eq!(layout.revision, 0);
    assert!(layout.tabs.is_empty());
    // A forgotten workspace accepts a fresh revision-0 publish again.
    assert!(service.put_layout("ws", 0, vec![tab("t1", "s1")]).is_ok());
}

#[tokio::test]
async fn layout_store_long_poll_answers_at_once_after_a_revision_regression() {
    // A fresh daemon (restart or handover) holds revision 0 while the client still remembers
    // revision 5 from the old one: the poll must answer instead of parking until the timeout.
    let service = service().session_service;
    let layout = tokio::time::timeout(
        Duration::from_secs(5),
        service.wait_layout("ws", 5, Duration::from_secs(30)),
    )
    .await
    .expect("a regressed revision answers without parking");
    assert_eq!(layout.revision, 0);
}

#[test]
fn layout_store_wire_shape_matches_the_contract() {
    let tabs = vec![SharedTab {
        id: "t1".into(),
        kind: "terminal".into(),
        label: "shell".into(),
        pinned: false,
        pane_tree: Some(SharedPane::Split {
            direction: SharedPaneDirection::Vertical,
            ratio: 0.25,
            first: Box::new(SharedPane::Leaf {
                leaf_id: "l1".into(),
                backend_session_id: "s1".into(),
            }),
            second: Box::new(SharedPane::Leaf {
                leaf_id: "l2".into(),
                backend_session_id: "s2".into(),
            }),
        }),
    }];
    let document = SharedLayout {
        workspace_id: "ws".into(),
        revision: 3,
        tabs,
    };
    assert_eq!(
        serde_json::to_value(&document).expect("serialize"),
        serde_json::json!({
            "workspaceId": "ws",
            "revision": 3,
            "tabs": [{
                "id": "t1",
                "kind": "terminal",
                "label": "shell",
                "pinned": false,
                "paneTree": {
                    "type": "split",
                    "direction": "vertical",
                    "ratio": 0.25,
                    "first": {"type": "leaf", "leafId": "l1", "backendSessionId": "s1"},
                    "second": {"type": "leaf", "leafId": "l2", "backendSessionId": "s2"},
                },
            }],
        })
    );
}
