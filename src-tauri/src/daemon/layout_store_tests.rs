//! Layout store behavior: revisions, stale-base conflict, per-workspace isolation and the
//! long-poll wake path. No wall-clock sleeps: the waiter is registered before the write, so the
//! only ordering the test relies on is that its own write happens after it subscribed.
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
        pane_tree: Some(SharedPane::leaf(format!("leaf-{id}"), session)),
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
            first: Box::new(SharedPane::leaf(format!("leaf-{id}-a"), "session-a")),
            second: Box::new(SharedPane::leaf(format!("leaf-{id}-b"), "session-b")),
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

#[tokio::test]
async fn layout_store_lru_bounds_buckets_and_refreshes_reads_and_writes() {
    use super::MAX_LAYOUT_BUCKETS;
    let service = service().session_service;
    for index in 0..MAX_LAYOUT_BUCKETS {
        service.put_layout(&format!("ws-{index}"), 0, vec![]).unwrap();
    }
    assert_eq!(service.layout("ws-0").revision, 1);
    service.put_layout("ws-1", 1, vec![]).unwrap();
    assert!(service.put_layout("ws-2", 0, vec![]).is_err());
    // A rejected write to an absent workspace must not allocate or evict a bucket.
    assert!(service.put_layout("missing", 1, vec![]).is_err());
    assert_eq!(service.layouts.lock().len(), MAX_LAYOUT_BUCKETS);
    for index in MAX_LAYOUT_BUCKETS..MAX_LAYOUT_BUCKETS + 64 {
        service.put_layout(&format!("ws-{index}"), 0, vec![]).unwrap();
        assert_eq!(service.layouts.lock().len(), MAX_LAYOUT_BUCKETS);
    }
    assert_eq!(service.layout("ws-0").revision, 1);
    assert_eq!(service.layout("ws-1").revision, 2);
    assert_eq!(service.layout("ws-2").revision, 1);
    assert_eq!(service.layout("ws-3").revision, 0);
    assert!(service.put_layout("ws-3", 0, vec![]).is_ok());
    assert_eq!(service.layouts.lock().len(), MAX_LAYOUT_BUCKETS);
}

#[tokio::test]
async fn layout_store_eviction_wakes_a_parked_poll_without_retaining_a_bucket() {
    use super::MAX_LAYOUT_BUCKETS;
    use futures_util::FutureExt;
    let service = service().session_service;
    for index in 0..MAX_LAYOUT_BUCKETS {
        service.put_layout(&format!("ws-{index}"), 0, vec![]).unwrap();
    }
    let receivers = service.layout_generation.receiver_count();
    let mut waiter = Box::pin(service.wait_layout("ws-0", 1, Duration::from_secs(30)));
    assert!(waiter.as_mut().now_or_never().is_none());
    assert_eq!(service.layout_generation.receiver_count(), receivers + 1);
    // Reading the other buckets makes the parked workspace the LRU bucket.
    for index in 1..MAX_LAYOUT_BUCKETS {
        assert_eq!(service.layout(&format!("ws-{index}")).revision, 1);
    }
    service.put_layout("new", 0, vec![]).unwrap();
    let layout = tokio::time::timeout(Duration::from_secs(5), waiter).await.unwrap();
    assert_eq!(layout.revision, 0);
    assert_eq!(service.layouts.lock().len(), MAX_LAYOUT_BUCKETS);
    assert!(!service.layouts.lock().contains_key(&("ws-0".to_owned(), None)));
    assert_eq!(service.layout_generation.receiver_count(), receivers);
}

#[tokio::test]
async fn layout_store_unregister_handler_forgets_the_document() {
    let service = service().session_service;
    service.put_layout("ws", 0, vec![tab("t1", "s1")]).unwrap();
    service.handle_unregister_workspace("ws").await.unwrap();
    assert_eq!(service.layout("ws"), SharedLayout::empty("ws"));
}

#[tokio::test]
async fn layout_store_isolates_worktree_buckets_of_one_workspace() {
    let service = service().session_service;
    // `main` and `wt:feature-a` are separate documents: each starts at revision 0 and their
    // revisions advance independently, so neither publish can overwrite the other.
    assert_eq!(service.layout_for("ws", Some("main")), SharedLayout::empty_for("ws", Some("main")));
    let main = service
        .put_layout_for("ws", Some("main"), 0, vec![tab("main-1", "s1")])
        .expect("main publish");
    assert_eq!((main.revision, main.worktree_key.as_deref()), (1, Some("main")));
    let feature = service
        .put_layout_for("ws", Some("wt:feature-a"), 0, vec![tab("feature-1", "s2")])
        .expect("feature publish");
    assert_eq!((feature.revision, feature.worktree_key.as_deref()), (1, Some("wt:feature-a")));
    // The second publish of one bucket leaves the other's revision and tabs untouched.
    service
        .put_layout_for("ws", Some("main"), 1, vec![tab("main-2", "s3")])
        .expect("main republish");
    assert_eq!(service.layout_for("ws", Some("main")).tabs, vec![tab("main-2", "s3")]);
    assert_eq!(service.layout_for("ws", Some("wt:feature-a")).revision, 1);
    assert_eq!(service.layout_for("ws", Some("wt:feature-a")).tabs, vec![tab("feature-1", "s2")]);
    // The legacy workspace-level bucket is a third, independent document.
    assert_eq!(service.layout("ws").revision, 0);
    // A stale base in one bucket conflicts with that bucket, never with a sibling.
    let conflict = service
        .put_layout_for("ws", Some("wt:feature-a"), 0, vec![tab("stale", "s4")])
        .expect_err("base 0 is stale for this bucket");
    assert_eq!(conflict.tabs, vec![tab("feature-1", "s2")]);
}

#[tokio::test]
async fn layout_store_forget_clears_every_worktree_bucket() {
    use futures_util::FutureExt;
    let service = service().session_service;
    service.put_layout("ws", 0, vec![tab("legacy", "s0")]).unwrap();
    service.put_layout_for("ws", Some("main"), 0, vec![tab("m", "s1")]).unwrap();
    service.put_layout_for("ws", Some("wt:feature-a"), 0, vec![tab("f", "s2")]).unwrap();
    service.put_layout("other", 0, vec![tab("o", "s3")]).unwrap();
    let mut parked = Box::pin(service.wait_layout_for("ws", Some("wt:feature-a"), 1, Duration::from_secs(30)));
    assert!(parked.as_mut().now_or_never().is_none());
    service.forget_layout("ws");
    // The parked poll wakes on the forget and reads the empty document of its own bucket.
    let woken = tokio::time::timeout(Duration::from_secs(5), parked).await.unwrap();
    assert_eq!(woken, SharedLayout::empty_for("ws", Some("wt:feature-a")));
    for key in [None, Some("main"), Some("wt:feature-a")] {
        assert_eq!(service.layout_for("ws", key).revision, 0, "{key:?}");
    }
    // Another workspace's buckets survive, and a forgotten bucket accepts revision 0 again.
    assert_eq!(service.layout("other").revision, 1);
    assert!(service.put_layout_for("ws", Some("main"), 0, vec![]).is_ok());
}

#[tokio::test]
async fn layout_store_worktree_buckets_share_the_lru_bound() {
    use super::MAX_LAYOUT_BUCKETS;
    let service = service().session_service;
    for index in 0..MAX_LAYOUT_BUCKETS {
        let workspace = format!("ws-{index}");
        service.put_layout_for(&workspace, Some("main"), 0, vec![]).unwrap();
    }
    assert_eq!(service.layouts.lock().len(), MAX_LAYOUT_BUCKETS);
    // One more bucket evicts the least recently touched one, whichever workspace it belonged to.
    service.put_layout_for("fresh", Some("main"), 0, vec![]).unwrap();
    assert_eq!(service.layouts.lock().len(), MAX_LAYOUT_BUCKETS);
    assert_eq!(service.layout_for("ws-0", Some("main")).revision, 0);
    assert_eq!(service.layout_for("fresh", Some("main")).revision, 1);
}

#[test]
fn layout_worktree_keys_accept_only_the_three_documented_forms() {
    use crate::remote::machine_protocol::validate_shared_worktree_key;
    for key in ["main", "wt:feature-a", "wt:a", "wt:A_1.2-3", "ext:0123456789abcdef"] {
        assert!(validate_shared_worktree_key(key), "{key:?}");
    }
    assert!(validate_shared_worktree_key(&format!("wt:{}", "a".repeat(128))));
    assert!(validate_shared_worktree_key(&format!("ext:{}", "0".repeat(64))));
    for key in [
        "",
        " ",
        "main ",
        " main",
        "MAIN",
        "main:x",
        "main/",
        "/main",
        "C:\\repo",
        "/Users/alice/repo",
        "wt:",
        "wt:a:b",
        "wt:../x",
        "wt:a..b",
        "wt:a/b",
        "wt:a\\b",
        "wt:a b",
        "wt:a\nb",
        "wt:a\0b",
        "ext:",
        "ext:0123456789abcde",
        "ext:0123456789ABCDEF",
        "ext:0123456789abcdefg",
    ] {
        assert!(!validate_shared_worktree_key(key), "{key:?}");
    }
    assert!(!validate_shared_worktree_key(&format!("wt:{}", "a".repeat(129))));
    assert!(!validate_shared_worktree_key(&format!("ext:{}", "0".repeat(65))));
}

#[test]
fn layout_file_rel_paths_reject_absolute_and_traversal_shapes() {
    use crate::remote::machine_protocol::validate_shared_file_rel_path;
    for path in ["src/main.rs", "README.md", "a/b/c.ts", "a/./b", ".env"] {
        assert!(validate_shared_file_rel_path(path), "{path:?}");
    }
    assert!(validate_shared_file_rel_path(&"a".repeat(512)));
    for path in [
        "",
        "/etc/passwd",
        "../secret",
        "a/../b",
        "a/..",
        "..",
        "C:\\repo\\file.rs",
        "C:/repo/file.rs",
        "a\\b",
        "a\nb",
        "a\0b",
    ] {
        assert!(!validate_shared_file_rel_path(path), "{path:?}");
    }
    assert!(!validate_shared_file_rel_path(&"a".repeat(513)));
}

#[test]
fn layout_leaf_metadata_is_validated_only_when_present() {
    use super::validate_layout_tabs;
    fn leaf_with(kind: Option<&str>, file: Option<&str>) -> SharedTab {
        let mut leaf_tab = tab("t", "s");
        leaf_tab.pane_tree = Some(SharedPane::Leaf {
            leaf_id: "leaf-t".into(),
            backend_session_id: String::new(),
            pane_kind: kind.map(str::to_owned),
            browser_id: None,
            url: None,
            file_rel_path: file.map(str::to_owned),
            dag_run_id: None,
            host_id: None,
        });
        leaf_tab
    }
    // Absent metadata is the legacy shape and stays valid.
    assert!(validate_layout_tabs(&[leaf_with(None, None)]));
    for kind in ["terminal", "browser", "file", "changes", "dag", "remoteDesktop"] {
        assert!(validate_layout_tabs(&[leaf_with(Some(kind), None)]), "{kind:?}");
    }
    for kind in ["", "Terminal", "editor", "terminal "] {
        assert!(!validate_layout_tabs(&[leaf_with(Some(kind), None)]), "{kind:?}");
    }
    assert!(validate_layout_tabs(&[leaf_with(Some("file"), Some("src/main.rs"))]));
    for path in ["/etc/passwd", "../secret", "C:\\repo", "a\\b"] {
        assert!(!validate_layout_tabs(&[leaf_with(Some("file"), Some(path))]), "{path:?}");
    }
}

#[test]
fn layout_workspace_ids_are_strict_without_requiring_registration() {
    use super::validate_layout_workspace_id;
    for id in ["", " ws", "ws ", "../escape", "a..b", "a/b", "a\\b", "a\nb", "a\0b"] {
        assert!(!validate_layout_workspace_id(id), "{id:?}");
    }
    assert!(!validate_layout_workspace_id(&"a".repeat(129)));
    assert!(validate_layout_workspace_id(&"a".repeat(128)));
    assert!(validate_layout_workspace_id("ws"));
}

fn pane_tree(depth: usize) -> SharedPane {
    let mut pane = SharedPane::leaf("leaf-0", "s");
    for index in 1..depth {
        pane = SharedPane::Split {
            direction: SharedPaneDirection::Horizontal,
            ratio: 0.5,
            first: Box::new(pane),
            second: Box::new(SharedPane::leaf(format!("leaf-{index}"), "s")),
        };
    }
    pane
}

#[test]
fn layout_tabs_validate_ids_and_exact_size_boundaries() {
    use super::{validate_layout_tabs, MAX_SHARED_LAYOUT_TABS, MAX_SHARED_PANE_DEPTH, MAX_SHARED_PANE_LEAVES};
    let mut tabs: Vec<_> = (0..MAX_SHARED_LAYOUT_TABS).map(|index| tab(&format!("t-{index}"), "s")).collect();
    assert!(validate_layout_tabs(&tabs));
    tabs.push(tab("too-many", "s"));
    assert!(!validate_layout_tabs(&tabs));
    assert!(!validate_layout_tabs(&[tab("", "s")]));
    assert!(!validate_layout_tabs(&[tab(" ", "s")]));
    assert!(!validate_layout_tabs(&[tab("t", "s"), tab("t", "s")]));
    let mut nested = tab("t", "s");
    nested.pane_tree = Some(pane_tree(MAX_SHARED_PANE_DEPTH));
    assert!(validate_layout_tabs(&[nested.clone()]));
    nested.pane_tree = Some(pane_tree(MAX_SHARED_PANE_DEPTH + 1));
    assert!(!validate_layout_tabs(&[nested]));
    for leaf_id in ["", " ", "leaf-t-b"] {
        let mut duplicate = split_tab("t");
        if let Some(SharedPane::Split { first, .. }) = &mut duplicate.pane_tree {
            **first = SharedPane::leaf(leaf_id, "s");
        }
        assert!(!validate_layout_tabs(&[duplicate]));
    }
    // A balanced tree reaches the leaf limit without approaching the depth limit.
    let mut panes: Vec<_> = (0..MAX_SHARED_PANE_LEAVES)
        .map(|index| SharedPane::leaf(format!("leaf-{index}"), "s"))
        .collect();
    while panes.len() > 1 {
        let mut next = Vec::new();
        let mut pairs = panes.into_iter();
        while let Some(first) = pairs.next() {
            next.push(SharedPane::Split {
                direction: SharedPaneDirection::Vertical, ratio: 0.5,
                first: Box::new(first), second: Box::new(pairs.next().unwrap()),
            });
        }
        panes = next;
    }
    let mut balanced = tab("t", "s");
    balanced.pane_tree = panes.pop();
    assert!(validate_layout_tabs(&[balanced.clone()]));
    balanced.pane_tree = Some(SharedPane::Split {
        direction: SharedPaneDirection::Horizontal, ratio: 0.5,
        first: Box::new(balanced.pane_tree.take().unwrap()),
        second: Box::new(SharedPane::leaf("extra", "s")),
    });
    assert!(!validate_layout_tabs(&[balanced]));
    // Leaf IDs are scoped per tab, not globally across a layout.
    let first = tab("first", "s");
    let mut second = first.clone();
    second.id = "second".into();
    assert!(validate_layout_tabs(&[first, second]));
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
            first: Box::new(SharedPane::leaf("l1", "s1")),
            second: Box::new(SharedPane::leaf("l2", "s2")),
        }),
    }];
    let document = SharedLayout {
        workspace_id: "ws".into(),
        worktree_key: None,
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
    // The worktree bucket rides the same document shape and is omitted when absent.
    let scoped = SharedLayout::empty_for("ws", Some("wt:feature-a"));
    assert_eq!(
        serde_json::to_value(&scoped).expect("serialize"),
        serde_json::json!({"workspaceId": "ws", "worktreeKey": "wt:feature-a", "revision": 0, "tabs": []})
    );
}

#[test]
fn layout_legacy_documents_deserialize_without_worktree_key_or_leaf_metadata() {
    // The exact bytes a `layoutSyncV1` host publishes today: no `worktreeKey`, no leaf metadata
    // and no `backendSessionId` on a non-terminal leaf.
    let legacy = serde_json::json!({
        "workspaceId": "ws",
        "revision": 4,
        "tabs": [
            {
                "id": "t1", "kind": "terminal", "label": "shell", "pinned": false,
                "paneTree": {"type": "leaf", "leafId": "l1", "backendSessionId": "s1"},
            },
            {"id": "t2", "kind": "changes", "label": "changes", "pinned": false, "paneTree": null},
        ],
    });
    let layout: SharedLayout = serde_json::from_value(legacy.clone()).expect("legacy document");
    assert_eq!(layout.worktree_key, None);
    assert_eq!(layout.revision, 4);
    assert_eq!(layout.tabs.len(), 2);
    assert_eq!(
        layout.tabs[0].pane_tree,
        Some(SharedPane::leaf("l1", "s1")),
        "a leaf without metadata reads as a terminal leaf"
    );
    // The absent bucket and metadata stay absent on the way back out.
    assert_eq!(serde_json::to_value(&layout).expect("serialize"), legacy);

    // A leaf that omits `backendSessionId` at all (a non-terminal kind) still decodes.
    let bare: SharedLayout = serde_json::from_value(serde_json::json!({
        "workspaceId": "ws",
        "revision": 1,
        "tabs": [{
            "id": "t", "kind": "browser", "label": "browser", "pinned": false,
            "paneTree": {"type": "leaf", "leafId": "l1", "paneKind": "browser", "url": "https://example.com"},
        }],
    }))
    .expect("a metadata leaf without a backend session");
    match &bare.tabs[0].pane_tree {
        Some(SharedPane::Leaf { backend_session_id, pane_kind, url, .. }) => {
            assert_eq!(backend_session_id, "");
            assert_eq!(pane_kind.as_deref(), Some("browser"));
            assert_eq!(url.as_deref(), Some("https://example.com"));
        }
        other => panic!("expected a leaf, got {other:?}"),
    }
}

#[test]
fn layout_non_terminal_leaf_metadata_round_trips_losslessly() {
    let leaf = SharedPane::Leaf {
        leaf_id: "leaf-file".into(),
        backend_session_id: String::new(),
        pane_kind: Some("file".into()),
        browser_id: Some("browser-7".into()),
        url: Some("https://example.com/a?b=c".into()),
        file_rel_path: Some("src/main.rs".into()),
        dag_run_id: Some("run-42".into()),
        host_id: Some("host-1".into()),
    };
    let encoded = serde_json::to_value(&leaf).expect("serialize");
    assert_eq!(
        encoded,
        serde_json::json!({
            "type": "leaf",
            "leafId": "leaf-file",
            "backendSessionId": "",
            "paneKind": "file",
            "browserId": "browser-7",
            "url": "https://example.com/a?b=c",
            "fileRelPath": "src/main.rs",
            "dagRunId": "run-42",
            "hostId": "host-1",
        })
    );
    let decoded: SharedPane = serde_json::from_value(encoded).expect("deserialize");
    assert_eq!(decoded, leaf);
}

#[test]
fn layout_documents_keep_leaf_metadata_through_the_store() {
    let leaf = SharedPane::Leaf {
        leaf_id: "leaf-browser".into(),
        backend_session_id: String::new(),
        pane_kind: Some("browser".into()),
        browser_id: Some("b1".into()),
        url: Some("https://example.com".into()),
        file_rel_path: None,
        dag_run_id: None,
        host_id: Some("host-1".into()),
    };
    let mut tab = tab("t1", "s1");
    tab.pane_tree = Some(leaf.clone());
    // The store keeps the document as handed to it: metadata is data, not view state.
    let document = SharedLayout {
        workspace_id: "ws".into(),
        worktree_key: Some("ext:0123456789abcdef".into()),
        revision: 1,
        tabs: vec![tab],
    };
    let encoded = serde_json::to_vec(&document).expect("serialize");
    let decoded: SharedLayout = serde_json::from_slice(&encoded).expect("deserialize");
    assert_eq!(decoded, document);
    assert_eq!(decoded.tabs[0].pane_tree.as_ref(), Some(&leaf));
}
