//! Integration tests for Remote Browser Screencast WebSocket lifecycle, tickets, driver lease, and reclamation
//! Authoritative Spec: docs/plans/REMOTE_BROWSER_SCREENCAST_PLAN_2026-09-17.md (§4.3, §4.4, §6.1, §6.2, §7.1, Phase 7A)

use super::browser_admission::{AdmissionOutcome, SubscriberQueue};
use super::browser_backend::{
    BrowserRemoteState, InProcessTestBackend, RemoteBrowserSessionSummary, UnavailableBrowserBackend,
};
use super::browser_protocol::{
    decode_binary_frame, encode_binary_frame, BrowserCaptureRect, BrowserFrameMetadata,
    BrowserImageFormat,
};
use super::server::create_remote_router;
use super::state::RemoteGatewayState;
use crate::remote::auth::DevicePermission;
use crate::terminal::TerminalService;
use crate::WorkspaceRegistry;
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use std::time::Instant;
use tokio_tungstenite::tungstenite::Message;

fn sample_jpeg_frame() -> &'static [u8] {
    &[
        0xFF, 0xD8, // SOI
        0xFF, 0xC0, // SOF0
        0x00, 0x11, // length = 17
        0x08,       // precision = 8
        0x00, 0x01, // height = 1
        0x00, 0x01, // width = 1
        0x03,       // 3 components
        0x01, 0x11, 0x00,
        0x02, 0x11, 0x00,
        0x03, 0x11, 0x00,
        0xFF, 0xDA, // SOS
        0x00, 0x08,
        0x01, 0x01, 0x00, 0x00, 0x3F, 0x00,
        0xFF, 0xD9, // EOI
    ]
}

#[tokio::test]
async fn test_browser_websocket_full_lifecycle_and_reconnection() {
    // 1. Setup server state and InProcessTestBackend
    let terminal_service = Arc::new(TerminalService::default());
    let registry = WorkspaceRegistry::new();
    let state = Arc::new(RemoteGatewayState::new(terminal_service, registry));

    let pin = state
        .auth_manager
        .create_pairing_code(DevicePermission::Control);
    let (control_token, _) = state
        .auth_manager
        .exchange_pairing_code(&pin, "control-device-1")
        .expect("Exchange pairing code");

    let test_backend = Arc::new(InProcessTestBackend::new());
    test_backend.sessions.lock().await.push(RemoteBrowserSessionSummary {
        browser_id: "b1".into(),
        title: Some("Example Home".into()),
        url: Some("https://example.com".into()),
        visible: true,
    });
    test_backend.states.lock().await.insert(
        "b1".into(),
        BrowserRemoteState {
            browser_id: "b1".into(),
            url: Some("https://example.com".into()),
            title: Some("Example Home".into()),
            document_generation: "1".into(),
            viewport_revision: "1".into(),
            loading: false,
            paused: false,
            pause_reason: None,
        },
    );
    state.set_browser_backend(test_backend.clone());

    // 2. Bind TCP listener on 127.0.0.1:0 and start Axum server
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Bind TCP listener");
    let addr = listener.local_addr().expect("Local addr");
    let router = create_remote_router(Arc::clone(&state));

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server_task = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });

    let http_client = reqwest::Client::builder().no_proxy().build().unwrap();

    // 3. Issue single-use socket ticket for /api/v1/browser/b1
    let ticket_payload = serde_json::json!({ "target": "/api/v1/browser/b1" });
    let ticket_resp = http_client
        .post(format!("http://{addr}/api/v1/socket-ticket"))
        .header("Authorization", format!("Bearer {control_token}"))
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(&ticket_payload).unwrap())
        .send()
        .await
        .expect("Request socket ticket");
    assert_eq!(ticket_resp.status(), reqwest::StatusCode::OK);

    let ticket_text = ticket_resp.text().await.expect("Read ticket text");
    let ticket_val: serde_json::Value = serde_json::from_str(&ticket_text).expect("Parse ticket json");
    let ticket = ticket_val["ticket"].as_str().expect("Ticket string").to_string();

    // 4. WebSocket upgrade: connect to ws://{addr}/api/v1/browser/b1?ticket={ticket}
    let ws_url = format!("ws://{addr}/api/v1/browser/b1?ticket={ticket}");
    let (mut ws_stream, upgrade_resp) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WebSocket handshake must succeed");
    assert_eq!(upgrade_resp.status(), 101, "Expected HTTP 101 Switching Protocols");

    // 5. Replaying the exact same ticket must be rejected (single-use ticket enforcement)
    let replayed = tokio_tungstenite::connect_async(&ws_url).await;
    assert!(replayed.is_err(), "Replayed single-use ticket must be rejected with 401");

    // 6. Hello handshake: server immediately sends BrowserHello
    let hello_msg = ws_stream
        .next()
        .await
        .expect("Receive hello")
        .expect("Valid hello frame");
    let hello_text = match hello_msg {
        Message::Text(t) => t,
        other => panic!("Expected text frame for Hello, got: {:?}", other),
    };
    let hello_json: serde_json::Value = serde_json::from_str(&hello_text).unwrap();
    assert_eq!(hello_json["type"], "browserHello");
    assert_eq!(hello_json["browserId"], "b1");
    assert_eq!(hello_json["protocolVersion"], 1);

    // 7. Subscription negotiation: client sends BrowserSubscribe
    let sub_req = serde_json::json!({
        "type": "browserSubscribe",
        "requestId": "req-sub-1",
        "viewerInstanceId": "v1",
        "options": {
            "format": "jpeg",
            "quality": 70,
            "intervalMs": 250,
            "maxEdge": 1280
        }
    });
    ws_stream
        .send(Message::Text(sub_req.to_string().into()))
        .await
        .expect("Send BrowserSubscribe");

    let sub_reply = ws_stream
        .next()
        .await
        .expect("Receive subscribed")
        .expect("Valid frame");
    let sub_json: serde_json::Value = serde_json::from_str(&sub_reply.into_text().unwrap()).unwrap();
    assert_eq!(sub_json["type"], "browserSubscribed");
    assert_eq!(sub_json["requestId"], "req-sub-1");
    assert_eq!(sub_json["streamId"], 1);
    let subscription_id = sub_json["subscriptionId"].as_str().unwrap().to_string();

    // Verify initial frame reception through queue pipeline and codec
    assert!(state.admission_controller.should_capture("b1"));
    let mut initial_queue = SubscriberQueue::new(subscription_id.clone(), 1);
    let meta = BrowserFrameMetadata {
        offset_top: 0.0,
        page_scale_factor: 1.0,
        device_width: 1024.0,
        device_height: 768.0,
        image_width: 1,
        image_height: 1,
        scroll_offset_x: 0.0,
        scroll_offset_y: 0.0,
        timestamp: 1726560000.0,
        stream_id: 1,
        browser_instance_id: "bi1".into(),
        browser_service_epoch: "1".into(),
        desktop_epoch: "1".into(),
        document_generation: "1".into(),
        viewport_revision: "1".into(),
        capture_rect: BrowserCaptureRect {
            x: 0.0,
            y: 0.0,
            width: 1024.0,
            height: 768.0,
        },
        geometry_source: "wkSnapshot".into(),
    };
    let initial_frame_bytes =
        encode_binary_frame(BrowserImageFormat::Jpeg, 1, &meta, sample_jpeg_frame())
            .expect("Encode initial frame");
    let admit_outcome = initial_queue.enqueue_frame(1, initial_frame_bytes.clone(), Instant::now());
    assert_eq!(admit_outcome, AdmissionOutcome::Admit);

    let decoded_initial = decode_binary_frame(&initial_frame_bytes).expect("Decode initial frame");
    assert_eq!(decoded_initial.seq, 1);
    assert_eq!(decoded_initial.format, BrowserImageFormat::Jpeg);
    assert_eq!(decoded_initial.metadata.browser_instance_id, "bi1");

    // Client-to-server binary frame must be rejected by server
    ws_stream
        .send(Message::Binary(vec![0x62, 1, 1, 1].into()))
        .await
        .expect("Send binary to server");
    let reject_reply = ws_stream
        .next()
        .await
        .expect("Receive error")
        .expect("Valid frame");
    let reject_json: serde_json::Value =
        serde_json::from_str(&reject_reply.into_text().unwrap()).unwrap();
    assert_eq!(reject_json["type"], "browserError");
    assert!(reject_json["message"]
        .as_str()
        .unwrap()
        .contains("rejected"));

    // 8. Driver claim with Control permission -> DriverLease granted
    let claim_req = serde_json::json!({
        "type": "browserDriverClaim",
        "requestId": "req-claim-1",
        "subscriptionId": subscription_id,
        "browserId": "b1"
    });
    ws_stream
        .send(Message::Text(claim_req.to_string().into()))
        .await
        .expect("Send driver claim");

    let claim_reply = ws_stream
        .next()
        .await
        .expect("Receive claim reply")
        .expect("Valid frame");
    let claim_json: serde_json::Value =
        serde_json::from_str(&claim_reply.into_text().unwrap()).unwrap();
    assert_eq!(claim_json["type"], "browserDriverClaimed");
    assert_eq!(claim_json["requestId"], "req-claim-1");
    let lease_epoch = claim_json["leaseEpoch"].as_str().unwrap().to_string();

    // 9. Heartbeat renewal keeping lease alive
    let hb_req = serde_json::json!({
        "type": "browserHeartbeat",
        "requestId": "req-hb-1",
        "leaseEpoch": lease_epoch,
        "subscriptionId": subscription_id
    });
    ws_stream
        .send(Message::Text(hb_req.to_string().into()))
        .await
        .expect("Send heartbeat");

    let hb_reply = ws_stream
        .next()
        .await
        .expect("Receive pong")
        .expect("Valid frame");
    let pong_json: serde_json::Value =
        serde_json::from_str(&hb_reply.into_text().unwrap()).unwrap();
    assert_eq!(pong_json["type"], "browserPong");
    assert_eq!(pong_json["requestId"], "req-hb-1");

    // Command execution works while lease is held
    let cmd_req = serde_json::json!({
        "type": "browserCommand",
        "requestId": "req-cmd-1",
        "requestSeq": "1",
        "browserId": "b1",
        "leaseEpoch": lease_epoch,
        "browserInstanceId": "bi1",
        "desktopEpoch": "1",
        "documentGeneration": "1",
        "command": "getState",
        "params": null
    });
    ws_stream
        .send(Message::Text(cmd_req.to_string().into()))
        .await
        .expect("Send command");

    let cmd_reply = ws_stream
        .next()
        .await
        .expect("Receive cmd reply")
        .expect("Valid frame");
    let cmd_json: serde_json::Value =
        serde_json::from_str(&cmd_reply.into_text().unwrap()).unwrap();
    assert_eq!(cmd_json["type"], "browserResult");
    assert_eq!(cmd_json["requestId"], "req-cmd-1");

    // 10. Desktop reclaim revoking remote driver lease -> driver notified
    let reclaimed_lease = state.admission_controller.broker.reclaim_desktop();
    assert!(reclaimed_lease.is_some(), "Desktop owner reclaims lease");

    // Immediate push: remote driver holding lease receives browserDriverRevoked
    let revoked_reply = ws_stream
        .next()
        .await
        .expect("Receive driver revoked message")
        .expect("Valid frame");
    let revoked_json: serde_json::Value =
        serde_json::from_str(&revoked_reply.into_text().unwrap()).unwrap();
    assert_eq!(revoked_json["type"], "browserDriverRevoked");

    // Subsequent command from remote driver must be rejected (driver notified)
    let cmd_after_reclaim = serde_json::json!({
        "type": "browserCommand",
        "requestId": "req-cmd-2",
        "requestSeq": "2",
        "browserId": "b1",
        "leaseEpoch": lease_epoch,
        "browserInstanceId": "bi1",
        "desktopEpoch": "1",
        "documentGeneration": "1",
        "command": "getState",
        "params": null
    });
    ws_stream
        .send(Message::Text(cmd_after_reclaim.to_string().into()))
        .await
        .expect("Send command after reclaim");

    let reclaim_err_reply = ws_stream
        .next()
        .await
        .expect("Receive error")
        .expect("Valid frame");
    let reclaim_err_json: serde_json::Value =
        serde_json::from_str(&reclaim_err_reply.into_text().unwrap()).unwrap();
    assert_eq!(reclaim_err_json["type"], "browserError");
    assert_eq!(reclaim_err_json["code"], "BROWSER_DRIVER_BUSY");
    assert!(reclaim_err_json["message"]
        .as_str()
        .unwrap()
        .contains("Not currently active driver"));

    // Close first connection cleanly
    let _ = ws_stream.close(None).await;

    // 11. Backend unavailable transition (GUI exit) -> returns 503 BROWSER_UNAVAILABLE
    state.set_browser_backend(Arc::new(UnavailableBrowserBackend));
    state.bump_browser_service_epoch();

    let unavail_resp = http_client
        .get(format!(
            "http://{addr}/api/v1/browser/sessions?workspaceId=ws1&worktreeSlug=main"
        ))
        .header("Authorization", format!("Bearer {control_token}"))
        .send()
        .await
        .expect("List sessions during unavailable");
    assert_eq!(
        unavail_resp.status(),
        reqwest::StatusCode::SERVICE_UNAVAILABLE,
        "Unavailable backend must return 503"
    );
    let unavail_text = unavail_resp.text().await.unwrap();
    assert!(unavail_text.contains("BROWSER_UNAVAILABLE"));

    // Restore test backend for reconnection test
    state.set_browser_backend(test_backend.clone());
    state.bump_browser_service_epoch();

    // 12. Reconnection with fresh ticket -> new subscription, NO mutation replay
    let fresh_payload = serde_json::json!({ "target": "/api/v1/browser/b1" });
    let fresh_ticket_resp = http_client
        .post(format!("http://{addr}/api/v1/socket-ticket"))
        .header("Authorization", format!("Bearer {control_token}"))
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(&fresh_payload).unwrap())
        .send()
        .await
        .expect("Issue fresh ticket");
    assert_eq!(fresh_ticket_resp.status(), reqwest::StatusCode::OK);
    let fresh_ticket_text = fresh_ticket_resp.text().await.unwrap();
    let fresh_ticket_json: serde_json::Value = serde_json::from_str(&fresh_ticket_text).unwrap();
    let fresh_ticket = fresh_ticket_json["ticket"].as_str().unwrap();

    let fresh_ws_url = format!("ws://{addr}/api/v1/browser/b1?ticket={fresh_ticket}");
    let (mut fresh_ws, _) = tokio_tungstenite::connect_async(&fresh_ws_url)
        .await
        .expect("Reconnection with fresh ticket must succeed");

    // Fresh hello handshake
    let fresh_hello = fresh_ws.next().await.unwrap().unwrap().into_text().unwrap();
    let fresh_hello_json: serde_json::Value = serde_json::from_str(&fresh_hello).unwrap();
    assert_eq!(fresh_hello_json["type"], "browserHello");

    // Fresh subscription negotiation
    let fresh_sub_req = serde_json::json!({
        "type": "browserSubscribe",
        "requestId": "req-fresh-sub-1",
        "viewerInstanceId": "v2",
        "options": {
            "format": "jpeg",
            "quality": 70
        }
    });
    fresh_ws
        .send(Message::Text(fresh_sub_req.to_string().into()))
        .await
        .unwrap();

    let fresh_sub_reply = fresh_ws.next().await.unwrap().unwrap().into_text().unwrap();
    let fresh_sub_json: serde_json::Value = serde_json::from_str(&fresh_sub_reply).unwrap();
    assert_eq!(fresh_sub_json["type"], "browserSubscribed");
    let fresh_sub_id = fresh_sub_json["subscriptionId"].as_str().unwrap().to_string();
    assert_ne!(
        fresh_sub_id, subscription_id,
        "New connection must have distinct subscription ID"
    );

    // Acquire driver on fresh connection
    let fresh_claim = serde_json::json!({
        "type": "browserDriverClaim",
        "requestId": "req-fresh-claim-1",
        "subscriptionId": fresh_sub_id,
        "browserId": "b1"
    });
    fresh_ws
        .send(Message::Text(fresh_claim.to_string().into()))
        .await
        .unwrap();

    let fresh_claim_reply = fresh_ws.next().await.unwrap().unwrap().into_text().unwrap();
    let fresh_claim_json: serde_json::Value = serde_json::from_str(&fresh_claim_reply).unwrap();
    assert_eq!(fresh_claim_json["type"], "browserDriverClaimed");
    let fresh_lease_epoch = fresh_claim_json["leaseEpoch"].as_str().unwrap();

    // Send command with sequence 1 (same sequence number as connection 1):
    // MUST execute fresh with NO mutation replay from previous connection
    let fresh_cmd = serde_json::json!({
        "type": "browserCommand",
        "requestId": "req-fresh-cmd-1",
        "requestSeq": "1",
        "browserId": "b1",
        "leaseEpoch": fresh_lease_epoch,
        "browserInstanceId": "bi1",
        "desktopEpoch": "1",
        "documentGeneration": "1",
        "command": "getState",
        "params": null
    });
    fresh_ws
        .send(Message::Text(fresh_cmd.to_string().into()))
        .await
        .unwrap();

    let fresh_cmd_reply = fresh_ws.next().await.unwrap().unwrap().into_text().unwrap();
    let fresh_cmd_json: serde_json::Value = serde_json::from_str(&fresh_cmd_reply).unwrap();
    assert_eq!(fresh_cmd_json["type"], "browserResult");
    assert_eq!(fresh_cmd_json["requestId"], "req-fresh-cmd-1");

    // Clean up
    let _ = fresh_ws.close(None).await;
    let _ = shutdown_tx.send(());
    let _ = server_task.await;
}

#[tokio::test]
async fn test_view_only_device_driver_claim_rejected_with_typed_error() {
    let terminal_service = Arc::new(TerminalService::default());
    let registry = WorkspaceRegistry::new();
    let state = Arc::new(RemoteGatewayState::new(terminal_service, registry));

    // Create View-only device pairing
    let pin = state
        .auth_manager
        .create_pairing_code(DevicePermission::View);
    let (view_token, _) = state
        .auth_manager
        .exchange_pairing_code(&pin, "view-only-device")
        .expect("Exchange view-only pairing code");

    let test_backend = Arc::new(InProcessTestBackend::new());
    state.set_browser_backend(test_backend);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Bind TCP listener");
    let addr = listener.local_addr().expect("Local addr");
    let router = create_remote_router(Arc::clone(&state));

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server_task = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });

    let http_client = reqwest::Client::builder().no_proxy().build().unwrap();

    // Issue ticket for view-only device
    let ticket_payload = serde_json::json!({ "target": "/api/v1/browser/b1" });
    let ticket_resp = http_client
        .post(format!("http://{addr}/api/v1/socket-ticket"))
        .header("Authorization", format!("Bearer {view_token}"))
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(&ticket_payload).unwrap())
        .send()
        .await
        .unwrap();
    assert_eq!(ticket_resp.status(), reqwest::StatusCode::OK);
    let ticket_text = ticket_resp.text().await.unwrap();
    let ticket: serde_json::Value = serde_json::from_str(&ticket_text).unwrap();
    let ticket_str = ticket["ticket"].as_str().unwrap();

    let ws_url = format!("ws://{addr}/api/v1/browser/b1?ticket={ticket_str}");
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();

    // Read Hello
    let _ = ws.next().await.unwrap().unwrap();

    // Subscribe succeeds for View-only device
    let sub_req = serde_json::json!({
        "type": "browserSubscribe",
        "requestId": "r-view-sub",
        "viewerInstanceId": "v1",
        "options": { "format": "jpeg" }
    });
    ws.send(Message::Text(sub_req.to_string().into())).await.unwrap();
    let sub_reply = ws.next().await.unwrap().unwrap().into_text().unwrap();
    let sub_json: serde_json::Value = serde_json::from_str(&sub_reply).unwrap();
    let sub_id = sub_json["subscriptionId"].as_str().unwrap();

    // Driver claim with View-only permission MUST be rejected with typed error BROWSER_FORBIDDEN
    let claim_req = serde_json::json!({
        "type": "browserDriverClaim",
        "requestId": "r-view-claim",
        "subscriptionId": sub_id,
        "browserId": "b1"
    });
    ws.send(Message::Text(claim_req.to_string().into())).await.unwrap();

    let claim_reply = ws.next().await.unwrap().unwrap().into_text().unwrap();
    let claim_json: serde_json::Value = serde_json::from_str(&claim_reply).unwrap();
    assert_eq!(claim_json["type"], "browserError");
    assert_eq!(claim_json["code"], "BROWSER_FORBIDDEN");
    assert!(claim_json["message"]
        .as_str()
        .unwrap()
        .contains("Control permission required"));

    let _ = ws.close(None).await;
    let _ = shutdown_tx.send(());
    let _ = server_task.await;
}
