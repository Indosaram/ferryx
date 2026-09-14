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
