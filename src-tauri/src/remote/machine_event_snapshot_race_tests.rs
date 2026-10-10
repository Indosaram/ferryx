use crate::{
    daemon::server::DaemonServer,
    remote::{
        auth::{DeviceAccessScope, DevicePermission},
        server::create_remote_router,
    },
};
use futures_util::{FutureExt, StreamExt};
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

#[tokio::test]
async fn machine_event_snapshot_recovers_when_commit_overlaps_inventory() {
    // Given: a real inventory build paused after observing the catalog revision.
    let (root, owner, token) = crate::ipc::run_blocking(|| {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("project")).unwrap();
        let owner = DaemonServer::new_with_paths(
            Some(root.path().join("config")),
            Some(root.path().join("auth")),
        );
        let auth = &owner.remote_state().auth_manager;
        let pin = auth
            .create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine)
            .unwrap();
        let (token, _) = auth.exchange_pairing_code(&pin, "snapshot-race").unwrap();
        Ok((root, owner, token))
    })
    .await
    .unwrap();
    let state = owner.remote_state().clone();
    let service = state.machine_services.as_ref().unwrap().workspaces.clone();
    let (observed, observing) = tokio::sync::oneshot::channel();
    let observed = parking_lot::Mutex::new(Some(observed));
    let (release, released) = std::sync::mpsc::channel();
    let released = parking_lot::Mutex::new(released);
    let first = AtomicBool::new(true);
    *service.transaction_probe.write() = Some(Arc::new(move |phase| {
        if phase == "eventInventoryObserved" && first.swap(false, Ordering::AcqRel) {
            observed.lock().take().unwrap().send(()).unwrap();
            released
                .lock()
                .recv_timeout(Duration::from_secs(15))
                .unwrap();
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let mut tasks = tokio::task::JoinSet::new();
    let router = create_remote_router(state.clone());
    tasks.spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let result = std::panic::AssertUnwindSafe(async {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();
        let ticket: Value = serde_json::from_str(
            &client
                .post(format!("http://{address}/api/v1/socket-ticket"))
                .bearer_auth(&token)
                .header("content-type", "application/json")
                .body(json!({"target":"/api/v1/events"}).to_string())
                .send()
                .await
                .unwrap()
                .text()
                .await
                .unwrap(),
        )
        .unwrap();
        let (mut socket, _) = tokio_tungstenite::connect_async(format!(
            "ws://{address}/api/v1/events?ticket={}",
            ticket["ticket"].as_str().unwrap()
        ))
        .await
        .unwrap();
        tokio::time::timeout(Duration::from_secs(5), observing)
            .await
            .unwrap()
            .unwrap();
        // When: a real catalog commit completes before the paused snapshot commits.
        let writer = service.clone();
        let path = root.path().join("project");
        let workspace = crate::ipc::run_blocking(move || {
            writer
                .register_machine(path.to_str().unwrap())
                .map_err(crate::ipc::IpcError::internal)
        })
        .await
        .unwrap();
        release.send(()).unwrap();
        // Then: partial stale boundary is followed on the SAME WS by complete recovery.
        let mut saw_stale = false;
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let message = socket.next().await.unwrap().unwrap();
                let event: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
                if event["payload"]["error"] == "STALE_REVISION" {
                    saw_stale = true;
                }
                if event["payload"]["projects"]["projects"][0]["workspaceId"] == workspace {
                    assert_eq!(event["payload"]["projects"]["completeness"], "complete");
                    break;
                }
            }
        })
        .await
        .expect("stale snapshot must not reset machine WS");
        assert!(saw_stale);
        socket.close(None).await.unwrap();
    })
    .catch_unwind()
    .await;
    drop(release);
    *service.transaction_probe.write() = None;
    let mut count = service.machine_events.subscription_count();
    tokio::time::timeout(Duration::from_secs(10), count.wait_for(|n| *n == 0))
        .await
        .unwrap()
        .unwrap();
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
    drop((service, state, owner));
    crate::ipc::run_blocking(move || {
        root.close().unwrap();
        Ok(())
    })
    .await
    .unwrap();
    eprintln!("A12 deterministic actual catalog commit overlapped snapshot: stale boundary then complete same-WS recovery; listeners/subscriptions/root cleaned");
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

#[tokio::test]
async fn stale_snapshot_retry_does_not_discard_worktree_commit_event() {
    let (root, owner, token, workspace) = crate::ipc::run_blocking(|| {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("project");
        std::fs::create_dir(&repo).unwrap();
        crate::worktree::run_git(&repo, &["init", "--quiet"]).unwrap();
        crate::worktree::run_git(
            &repo,
            &[
                "-c",
                "user.name=A12",
                "-c",
                "user.email=a12@example.invalid",
                "commit",
                "--allow-empty",
                "-m",
                "base",
            ],
        )
        .unwrap();
        let owner = DaemonServer::new_with_paths(
            Some(root.path().join("config")),
            Some(root.path().join("auth")),
        );
        let auth = &owner.remote_state().auth_manager;
        let pin = auth
            .create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine)
            .unwrap();
        let (token, _) = auth.exchange_pairing_code(&pin, "snapshot-worktree-race").unwrap();
        let workspace = owner
            .remote_state()
            .machine_services
            .as_ref()
            .unwrap()
            .workspaces
            .register_machine(repo.to_str().unwrap())
            .unwrap();
        Ok((root, owner, token, workspace))
    })
    .await
    .unwrap();
    let state = owner.remote_state().clone();
    let service = state.machine_services.as_ref().unwrap().workspaces.clone();
    let (observed, observing) = tokio::sync::oneshot::channel();
    let observed = parking_lot::Mutex::new(Some(observed));
    let (release, released) = std::sync::mpsc::channel();
    let released = parking_lot::Mutex::new(released);
    let first = AtomicBool::new(true);
    *service.transaction_probe.write() = Some(Arc::new(move |phase| {
        if phase == "eventInventoryObserved" && first.swap(false, Ordering::AcqRel) {
            observed.lock().take().unwrap().send(()).unwrap();
            released
                .lock()
                .recv_timeout(Duration::from_secs(15))
                .unwrap();
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let router = create_remote_router(state.clone());
    let task = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async { let _ = stopped.await; })
            .await
            .unwrap();
    });
    let result = std::panic::AssertUnwindSafe(async {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();
        let ticket: Value = serde_json::from_str(
            &client
                .post(format!("http://{address}/api/v1/socket-ticket"))
                .bearer_auth(&token)
                .header("content-type", "application/json")
                .body(json!({"target":"/api/v1/events"}).to_string())
                .send()
                .await
                .unwrap()
                .text()
                .await
                .unwrap(),
        )
        .unwrap();
        let (mut socket, _) = tokio_tungstenite::connect_async(format!(
            "ws://{address}/api/v1/events?ticket={}",
            ticket["ticket"].as_str().unwrap()
        ))
        .await
        .unwrap();
        tokio::time::timeout(Duration::from_secs(5), observing)
            .await
            .unwrap()
            .unwrap();

        let response = client
            .post(format!("http://{address}/api/v1/workspace/worktrees"))
            .bearer_auth(&token)
            .body(
                json!({
                    "requestId": uuid::Uuid::new_v4().to_string(),
                    "workspaceId": workspace,
                    "worktree": {"wsId": workspace, "slug": "snapshot-race"}
                })
                .to_string(),
            )
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 201);
        let created: Value = serde_json::from_str(&response.text().await.unwrap()).unwrap();
        release.send(()).unwrap();

        let mut saw_stale = false;
        let mut saw_created = false;
        tokio::time::timeout(Duration::from_secs(10), async {
            while !saw_created {
                let message = socket.next().await.unwrap().unwrap();
                if !message.is_text() {
                    continue;
                }
                let event: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
                if event["payload"]["error"] == "STALE_REVISION" {
                    saw_stale = true;
                }
                if event["type"] == "worktreeCreated" {
                    assert_eq!(event["payload"], created);
                    saw_created = true;
                }
            }
        })
        .await
        .expect("stale snapshot retry must preserve the committed worktree event");
        assert!(saw_stale, "the mutation must overlap a stale inventory snapshot");
        socket.close(None).await.unwrap();
    })
    .catch_unwind()
    .await;
    drop(release);
    *service.transaction_probe.write() = None;
    let mut count = service.machine_events.subscription_count();
    tokio::time::timeout(Duration::from_secs(10), count.wait_for(|n| *n == 0))
        .await
        .unwrap()
        .unwrap();
    stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(10), task)
        .await
        .unwrap()
        .unwrap();
    drop((service, state, owner));
    crate::ipc::run_blocking(move || {
        root.close().unwrap();
        Ok(())
    })
    .await
    .unwrap();
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

#[tokio::test]
async fn desktop_focus_change_is_published_to_machine_subscribers_once() {
    // Given: a machine-scoped gateway and a subscriber on its own event domain.
    let (root, owner) = crate::ipc::run_blocking(|| {
        let root = tempfile::tempdir().unwrap();
        let owner = DaemonServer::new_with_paths(
            Some(root.path().join("config")),
            Some(root.path().join("auth")),
        );
        Ok((root, owner))
    })
    .await
    .unwrap();
    let state = owner.remote_state().clone();
    let mut events = state
        .machine_services
        .as_ref()
        .unwrap()
        .workspaces
        .machine_events
        .subscribe();
    let focus = |session: &str| crate::remote::RemoteActiveDesktopSelection {
        workspace_id: Some("ws".into()),
        session_id: Some(session.into()),
        ..Default::default()
    };
    // When: the desktop focuses a session, republishes the identical focus, then moves.
    state.set_active_selection(focus("s1"));
    state.set_active_selection(focus("s1"));
    state.set_active_selection(focus("s2"));
    // Then: only real focus changes reach machine clients, as a bare pointer.
    let mut seen = Vec::new();
    while let Ok(event) = events.try_recv() {
        assert_eq!(event["type"], "desktopSelectionChanged");
        assert_eq!(event["payload"], json!({"workspaceId": "ws", "worktreeSlug": null}));
        seen.push(event["sessionId"].as_str().unwrap().to_string());
    }
    assert_eq!(seen, ["s1", "s2"]);
    drop((state, owner));
    crate::ipc::run_blocking(move || {
        root.close().unwrap();
        Ok(())
    })
    .await
    .unwrap();
}
