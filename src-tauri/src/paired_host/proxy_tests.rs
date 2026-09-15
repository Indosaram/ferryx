use super::client::*;
use crate::{remote::{machine_protocol as m, terminal_wire::{encode_frame, Metadata}}, scoped_contracts::Epoch, terminal::{output_hub::TerminalOutputHub, paired_daemon::{Descriptor, Proxy}}};
use axum::{Router, routing::{get, post}, Json, extract::ws::{WebSocketUpgrade, Message}};
use serde_json::json;
use std::{sync::Arc, time::Duration};

#[tokio::test]
async fn paired_socket_decodes_output_and_fences_input_without_local_pty() {
    socket_fixture(false, false, None).await;
}

#[tokio::test]
async fn native_actor_delivers_controls_and_output_without_local_pty() {
    socket_fixture(true, false, None).await;
}

#[tokio::test]
async fn a23_dropping_connected_proxy_releases_socket_and_hub() {
    socket_fixture(false, true, None).await;
}

#[tokio::test]
async fn spontaneous_eof_reaps_native_owner() {
    socket_fixture(true, false, Some(false)).await;
}

#[tokio::test]
async fn spontaneous_remote_exit_reaps_native_owner() {
    socket_fixture(true, false, Some(true)).await;
}

async fn socket_fixture(native: bool, drop_connected: bool, spontaneous: Option<bool>) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().to_owned();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (input_tx, input_rx) = tokio::sync::oneshot::channel();
    let input_tx = Arc::new(tokio::sync::Mutex::new(Some(input_tx)));
    let (closed_tx, closed_rx) = tokio::sync::oneshot::channel();
    let closed_tx = Arc::new(tokio::sync::Mutex::new(Some(closed_tx)));
    let (terminate_tx, terminate_rx) = tokio::sync::oneshot::channel::<()>();
    let terminate_rx = Arc::new(tokio::sync::Mutex::new(Some(terminate_rx)));
    let router = Router::new()
        .route("/api/v1/pair/exchange", post(|| async { Json(json!({"token":"fixture-secret","machineId":"a","device":{"id":"d","name":"d","permission":"control","accessScope":"machine","createdAt":1,"lastSeenAt":1}})) }))
        .route("/host/a/api/v1/capabilities", get(|| async { Json(json!({"apiVersion":1,"machineId":"a","daemonEpoch":"1","platform":"linux","accessScope":"machine","permission":"control","capabilities":["terminalCreateV1","terminalStreamV1"],"limits":{"directoryEntries":1000,"terminalSessions":64}})) }))
        .route("/host/a/api/v1/sessions/s", get(|| async { Json(json!({"status":"running","session":{"target":{"machineId":"a","daemonEpoch":"1","sessionId":"s"},"workspaceId":"w","worktree":null,"cwd":"/fixture","cols":80,"rows":24,"running":true,"providerSession":null,"startSequence":"10","endSequence":"10"}})) }))
        .route("/host/a/api/v1/terminal/s", get(move |ws: WebSocketUpgrade, headers: axum::http::HeaderMap, uri: axum::http::Uri| {
            let input_tx = input_tx.clone();
            let closed_tx = closed_tx.clone();
            let terminate_rx = terminate_rx.clone();
            async move {
                assert_eq!(headers["authorization"], "Bearer fixture-secret");
                assert_eq!(uri.query(), Some("daemonEpoch=1"));
                ws.on_upgrade(move |mut socket| async move {
                    socket.send(Message::Text(json!({"type":"attached","target":{"machineId":"a","daemonEpoch":"1","sessionId":"s"},"generation":"7","cols":80,"rows":24,"startSequence":"10","endSequence":"10","replayGap":null}).to_string().into())).await.unwrap();
                    socket.send(Message::Binary(encode_frame(Metadata::Replay { start: Some(10), end: Some(10), gap: None }, b"hello", false).unwrap().into())).await.unwrap();
                    let mut messages = Vec::new();
                    for _ in 0..3 { messages.push(socket.recv().await.unwrap().unwrap()); }
                    socket.send(Message::Binary(encode_frame(Metadata::Output { sequence: 11, gap: None }, b"world", false).unwrap().into())).await.unwrap();
                    input_tx.lock().await.take().unwrap().send(messages).unwrap();
                    if let Some(remote_exit) = spontaneous {
                        terminate_rx.lock().await.take().unwrap().await.unwrap();
                        if remote_exit {
                            socket.send(Message::Text(json!({"type":"exit","target":{"machineId":"a","daemonEpoch":"1","sessionId":"s"}}).to_string().into())).await.unwrap();
                            while let Some(message) = socket.recv().await { if matches!(message, Ok(Message::Close(_)) | Err(_)) { break; } }
                        }
                    } else {
                        while let Some(message) = socket.recv().await { if matches!(message, Ok(Message::Close(_)) | Err(_)) { break; } }
                    }
                    drop(socket);
                    closed_tx.lock().await.take().unwrap().send(()).unwrap();
                })
            }
        }));
    let (shutdown, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move { axum::serve(listener, router).with_graceful_shutdown(async { let _ = stopped.await; }).await.unwrap(); });
    let service = super::service::PairedHostService::open_test_loopback(path.join("data"));
    let host = service.pair(super::service::PairRequest { relay_origin: format!("http://{address}"), pin: super::service::Secret("fixture".into()), display_label: "fixture".into() }).await.unwrap();
    let descriptor = Descriptor { host_id: host.host_id.clone(), generation: host.generation, target: m::RemoteTerminalTarget { machine_id: "a".into(), daemon_epoch: Epoch(1), session_id: "s".into() }, after_sequence: None };
    let hub = Arc::new(TerminalOutputHub::new(32));
    let mut proxy = Proxy::new(descriptor, hub.clone()).unwrap();
    let mut output = hub.subscribe_with_sequence(proxy.id(), None).unwrap().receiver;
    proxy.reattach(&MachineClient::new(), &service).await.unwrap();
    let terminal = crate::terminal::TerminalService::new(Arc::new(crate::terminal::PtyManager::new()), hub.clone());
    let id = proxy.id().to_owned();
    let mut proxy = if native {
        terminal.paired().install(proxy).unwrap();
        let router = crate::daemon::proxy::SessionRouter::new(Arc::new(terminal.clone()));
        assert!(router.is_local_session(&id));
        assert!(terminal.attach_with_sequence(&id, None).is_ok());
        assert!(terminal.write_input(&id, b"unfenced").is_err());
        assert!(terminal.write_input_operation(&id, 6, b"wrong".to_vec()).unwrap().await.is_err());
        terminal.write_input_operation(&id, 7, b"input".to_vec()).unwrap().await.unwrap();
        terminal.resize_operation(&id, 7, 100, 40).unwrap().await.unwrap();
        terminal.paired().interrupt(&id, 7).unwrap().await.unwrap();
        None
    } else {
        assert!(proxy.write(Epoch(6), b"wrong").await.is_err());
        assert_eq!(proxy.write(Epoch(7), &vec![0; 64 * 1024 + 1]).await.unwrap_err().code, "PAYLOAD_TOO_LARGE");
        proxy.write(Epoch(7), b"input").await.unwrap();
        proxy.resize(Epoch(7), 100, 40).await.unwrap();
        proxy.interrupt(Epoch(7)).await.unwrap();
        Some(proxy)
    };
    let input = tokio::time::timeout(Duration::from_secs(5), input_rx).await.unwrap().unwrap();
    assert_eq!(input[0], Message::Binary(b"input".to_vec().into()));
    for (message, expected) in input[1..].iter().zip([json!({"type":"resize","generation":"7","cols":100,"rows":40}), json!({"type":"signal","generation":"7","signal":"interrupt"})]) {
        let Message::Text(text) = message else { panic!("expected control"); };
        assert_eq!(serde_json::from_str::<serde_json::Value>(text).unwrap(), expected);
    }
    if let Some(proxy) = proxy.as_mut() { proxy.receive().await.unwrap(); }
    let first = tokio::time::timeout(Duration::from_secs(5), output.recv()).await.unwrap().unwrap();
    let observed = first.bytes.clone();
    if let Some(proxy) = proxy.as_mut() {
        assert_eq!(first.sequence, 2);
        assert_eq!(proxy.descriptor().after_sequence, Some(Epoch(10)));
        proxy.receive().await.unwrap();
        assert_eq!(proxy.descriptor().after_sequence, Some(Epoch(11)));
        if !drop_connected { proxy.detach().await.unwrap(); }
    } else {
        let live = tokio::time::timeout(Duration::from_secs(5), output.recv()).await.unwrap().unwrap();
        assert_eq!(&*live.bytes, b"world");
        assert!(live.sequence > first.sequence);
        if spontaneous.is_some() {
            let pending = terminal.write_input_operation(&id, 7, b"racing".to_vec()).unwrap();
            let completed = terminal.paired().completion_probe(&id, pending);
            terminate_tx.send(()).unwrap();
            tokio::time::timeout(Duration::from_secs(5), completed).await.unwrap();
            assert!(matches!(terminal.write_input_operation(&id, 7, b"dead".to_vec()), Err(crate::terminal::PtyError::Other(code)) if code == "PAIRED_PROXY_MISSING"));
        } else {
            terminal.paired().detach(&id).await.unwrap();
        }
        assert!(terminal.write_input_operation(&id, 7, b"detached".to_vec()).is_err());
        assert!(terminal.list_sessions().is_empty());
        assert!(terminal.pty_manager().list_sessions().is_empty());
    }
    drop(proxy);
    assert!(!hub.has_session(&id));
    tokio::time::timeout(Duration::from_secs(5), closed_rx).await.unwrap().unwrap();
    shutdown.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), server).await.unwrap().unwrap();
    assert!(tokio::net::TcpStream::connect(address).await.is_err());
    drop(service);
    root.close().unwrap();
    assert!(!path.exists());
    assert_eq!(&*observed, b"hello");
}

#[tokio::test]
async fn native_registry_routes_paired_ids_without_local_fallback() {
    let terminal = crate::terminal::TerminalService::default();
    let descriptor = Descriptor { host_id: "host-a".into(), generation: Epoch(1), target: m::RemoteTerminalTarget { machine_id: "a".into(), daemon_epoch: Epoch(1), session_id: "same".into() }, after_sequence: None };
    let proxy = Proxy::new(descriptor.clone(), terminal.output_hub().clone()).unwrap();
    let id = proxy.id().to_owned();
    let mut other = descriptor;
    other.host_id = "host-b".into();
    let other = Proxy::new(other, terminal.output_hub().clone()).unwrap();
    assert_ne!(id, other.id());
    terminal.paired().install(proxy).unwrap();
    assert!(terminal.list_sessions().contains(&id));
    assert!(terminal.attach_with_sequence(&id, None).is_ok());
    assert!(terminal.write_input_operation(&id, 7, b"detached".to_vec()).unwrap().await.is_err());
    assert!(terminal.close_session(&id).await.is_ok());
    drop(other);
    assert!(!terminal.output_hub().has_session(&id));
    assert!(terminal.pty_manager().list_sessions().is_empty());
    assert!(terminal.list_sessions().is_empty());
}
