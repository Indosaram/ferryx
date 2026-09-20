use crate::{
    daemon::server::DaemonServer,
    remote::{
        auth::{DeviceAccessScope, DevicePermission},
        server::create_remote_router,
    },
};
use futures_util::{FutureExt, StreamExt};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn connect(address: std::net::SocketAddr, token: &str) -> Socket {
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let response = client
        .post(format!("http://{address}/api/v1/socket-ticket"))
        .bearer_auth(token)
        .header("content-type", "application/json")
        .body(json!({"target":"/api/v1/events"}).to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let ticket: Value = serde_json::from_str(&response.text().await.unwrap()).unwrap();
    tokio_tungstenite::connect_async(format!(
        "ws://{address}/api/v1/events?ticket={}",
        ticket["ticket"].as_str().unwrap()
    ))
    .await
    .unwrap()
    .0
}

#[tokio::test]
async fn machine_event_cancellation_releases_socket_when_catalog_refresh_is_blocked() {
    contention(false).await;
}

#[tokio::test]
async fn machine_event_cancellation_read_budget_expires_when_catalog_refresh_is_blocked() {
    contention(true).await;
}

async fn contention(expire: bool) {
    // Given a private authority and actual catalog mutex held by a joined writer.
    let (root, owner, token) = crate::ipc::run_blocking(|| {
        let root = tempfile::tempdir().unwrap();
        let owner = DaemonServer::new_with_paths(
            Some(root.path().join("config")),
            Some(root.path().join("auth")),
        );
        let auth = &owner.remote_state().auth_manager;
        let pin = auth
            .create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine)
            .unwrap();
        let (token, _) = auth
            .exchange_pairing_code(&pin, "watch-cancellation")
            .unwrap();
        Ok((root, owner, token))
    })
    .await
    .unwrap();
    let state = owner.remote_state().clone();
    let service = state.machine_services.as_ref().unwrap().workspaces.clone();
    let events = &service.machine_events;
    let mut subscriptions = events.subscription_count();
    let (requested, mut refreshes) = tokio::sync::mpsc::channel(4);
    *service.transaction_probe.write() = Some(Arc::new(move |phase| {
        if phase == "watchRefreshRequested" {
            requested.try_send(()).unwrap();
        }
    }));
    let (held, holding) = tokio::sync::oneshot::channel();
    let (release, released) = std::sync::mpsc::channel();
    let writer_service = service.clone();
    let writer = std::thread::spawn(move || {
        let _catalog = writer_service.catalog.lock();
        held.send(()).unwrap();
        released.recv_timeout(Duration::from_secs(60)).unwrap();
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let router = create_remote_router(state.clone());
    let gateway = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    let outcome = std::panic::AssertUnwindSafe(async {
        tokio::time::timeout(Duration::from_secs(5), holding).await.unwrap().unwrap();
        // When the real machine WebSocket reaches refresh, close or exhaust its read budget.
        for occupied in 1..=2 {
            let start = std::time::Instant::now();
            let mut socket = connect(address, &token).await;
            tokio::time::timeout(Duration::from_secs(5), refreshes.recv()).await.unwrap().unwrap();
            tokio::time::timeout(Duration::from_secs(5), subscriptions.wait_for(|n| *n == 1)).await.unwrap().unwrap();
            if expire {
                let frame = tokio::time::timeout(Duration::from_secs(11), socket.next()).await
                    .expect("10-second read budget did not include catalog-contended watch refresh").unwrap().unwrap();
                let boundary: Value = serde_json::from_str(frame.to_text().unwrap()).unwrap();
                assert_eq!(boundary["payload"]["completeness"], "partial");
                assert!(start.elapsed() >= Duration::from_secs(9));
            } else {
                socket.close(None).await.unwrap();
            }
            // Then socket accounting ends while blocked workers retain watch admission.
            tokio::time::timeout(Duration::from_secs(2), subscriptions.wait_for(|n| *n == 0)).await
                .expect("client close could not release subscription while actual catalog mutex held").unwrap();
            assert_eq!(events.watch_slots.available_permits(), 2 - occupied);
            eprintln!("A12_WATCH expire={expire} disconnected=true catalog_held=true retained_workers={occupied}");
        }
        // Both workers remain held. Reconnecting can queue cancellable admission,
        // but cannot manufacture a third blocked worker or native watcher.
        for _ in 0..3 {
            let mut socket = connect(address, &token).await;
            tokio::time::timeout(Duration::from_secs(5), subscriptions.wait_for(|n| *n == 1)).await.unwrap().unwrap();
            socket.close(None).await.unwrap();
            tokio::time::timeout(Duration::from_secs(2), subscriptions.wait_for(|n| *n == 0)).await.unwrap().unwrap();
            assert_eq!(events.watch_slots.available_permits(), 0);
        }
        assert!(matches!(refreshes.try_recv(), Err(tokio::sync::mpsc::error::TryRecvError::Empty)));
        eprintln!("A12_WATCH expire={expire} reconnects=3 extra_refresh_workers=0");
    }).catch_unwind().await;
    // Always release contention and join owned work before surfacing assertions.
    release.send(()).unwrap();
    crate::ipc::run_blocking(move || {
        writer.join().unwrap();
        Ok(())
    })
    .await
    .unwrap();
    let drained = tokio::time::timeout(
        Duration::from_secs(10),
        events.watch_slots.clone().acquire_many_owned(2),
    )
    .await;
    let unsubscribed =
        tokio::time::timeout(Duration::from_secs(12), subscriptions.wait_for(|n| *n == 0)).await;
    *service.transaction_probe.write() = None;
    stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(10), gateway)
        .await
        .unwrap()
        .unwrap();
    let drained = drained.unwrap().unwrap();
    unsubscribed.unwrap().unwrap();
    drop(drained);
    drop((service, state, owner));
    crate::ipc::run_blocking(move || {
        root.close().unwrap();
        Ok(())
    })
    .await
    .unwrap();
    eprintln!("A12_WATCH cleanup writer_joined=true workers_drained=true subscriptions=0 gateway_joined=true private_root_removed=true");
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}
