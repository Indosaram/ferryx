//! Relay->direct upgrade of live paired streams, driven through the owner actor.
use super::*;
use crate::paired_host::{
    direct_route::{DirectGuard, RouteState, GLOBAL_DIRECT_ROUTES},
    service::{PairRequest, PairedHostService, Secret},
};
use crate::remote::{
    machine_protocol::RemoteTerminalTarget,
    terminal_wire::{encode_frame, Metadata},
};
use crate::terminal::{output_hub::TerminalOutputHub, paired_daemon::Descriptor};
use axum::{
    extract::ws::{Message, WebSocketUpgrade},
    routing::{get, post},
    Json, Router,
};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};

const BOUND: Duration = Duration::from_secs(10);

fn attached(generation: &str) -> Message {
    Message::Text(json!({"type":"attached","target":{"machineId":"a","daemonEpoch":"1","sessionId":"s"},"generation":generation,"cols":80,"rows":24,"startSequence":"10","endSequence":"10","replayGap":null}).to_string().into())
}
fn output(sequence: u64, bytes: &[u8]) -> Message {
    Message::Binary(encode_frame(Metadata::Output { sequence, gap: None }, bytes, false).unwrap().into())
}

#[derive(Clone, Copy, PartialEq)]
enum DirectMode {
    /// Accepts after the test releases the gate, replays 13, streams 14, records input.
    Gated,
    /// Upgrades the socket, then drops it before the attached boundary.
    DropAfterUpgrade,
}

struct Fixture {
    service: PairedHostService,
    descriptor: Descriptor,
    direct_origin: String,
    relay_output: mpsc::Sender<u64>,
    relay_inputs: mpsc::Receiver<Vec<u8>>,
    direct_arrivals: mpsc::Receiver<(Option<String>, oneshot::Sender<()>)>,
    direct_inputs: mpsc::Receiver<Vec<u8>>,
    relay_attaches: Arc<AtomicUsize>,
    direct_attaches: Arc<AtomicUsize>,
    creates: Arc<AtomicUsize>,
    servers: Vec<tokio::task::JoinHandle<()>>,
    _root: tempfile::TempDir,
}

async fn fixture(mode: DirectMode) -> Fixture {
    let caps = || async { Json(json!({"apiVersion":1,"machineId":"a","daemonEpoch":"1","platform":"linux","accessScope":"machine","permission":"control","capabilities":["terminalCreateV1","terminalStreamV1"],"limits":{"directoryEntries":1000,"terminalSessions":64}})) };
    let session = || async { Json(json!({"status":"running","session":{"target":{"machineId":"a","daemonEpoch":"1","sessionId":"s"},"workspaceId":"w","worktree":null,"cwd":"/fixture","cols":80,"rows":24,"running":true,"providerSession":null,"startSequence":"10","endSequence":"10"}})) };

    // Direct adapter.
    let direct_attaches = Arc::new(AtomicUsize::new(0));
    let (arrival_tx, direct_arrivals) = mpsc::channel(8);
    let (direct_input_tx, direct_inputs) = mpsc::channel(32);
    let direct_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let direct_origin = format!("http://{}", direct_listener.local_addr().unwrap());
    let attaches = direct_attaches.clone();
    let direct_router = Router::new()
        .route("/api/v1/capabilities", get(caps))
        .route("/api/v1/sessions/s", get(session))
        .route("/api/v1/terminal/s", get(move |ws: WebSocketUpgrade, uri: axum::http::Uri| {
            let (attaches, arrival_tx, direct_input_tx) = (attaches.clone(), arrival_tx.clone(), direct_input_tx.clone());
            async move {
                attaches.fetch_add(1, SeqCst);
                let query = uri.query().map(str::to_owned);
                ws.on_upgrade(move |mut socket| async move {
                    if mode == DirectMode::DropAfterUpgrade {
                        return;
                    }
                    let (gate_tx, gate) = oneshot::channel();
                    arrival_tx.send((query, gate_tx)).await.unwrap();
                    if gate.await.is_err() { return; }
                    socket.send(attached("8")).await.unwrap();
                    socket.send(Message::Binary(encode_frame(Metadata::Replay { start: Some(13), end: Some(13), gap: None }, b"d13", false).unwrap().into())).await.unwrap();
                    socket.send(output(14, b"d14")).await.unwrap();
                    while let Some(Ok(message)) = socket.recv().await {
                        if let Message::Binary(bytes) = message { let _ = direct_input_tx.send(bytes.to_vec()).await; }
                    }
                })
            }
        }));
    let direct = tokio::spawn(async move { axum::serve(direct_listener, direct_router).await.unwrap() });

    // Relay: one stream; the test drives its output, input is recorded.
    let relay_attaches = Arc::new(AtomicUsize::new(0));
    let creates = Arc::new(AtomicUsize::new(0));
    let (relay_output, relay_output_rx) = mpsc::channel::<u64>(32);
    let relay_output_rx = Arc::new(tokio::sync::Mutex::new(Some(relay_output_rx)));
    let (relay_input_tx, relay_inputs) = mpsc::channel(32);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (attaches, created) = (relay_attaches.clone(), creates.clone());
    let router = Router::new()
        .route("/api/v1/pair/exchange", post(|| async { Json(json!({"token":"fixture-secret","machineId":"a","device":{"id":"d","name":"d","permission":"control","accessScope":"machine","createdAt":1,"lastSeenAt":1}})) }))
        .route("/host/a/api/v1/capabilities", get(caps))
        .route("/host/a/api/v1/sessions", post(move || {
            let created = created.clone();
            async move { created.fetch_add(1, SeqCst); axum::http::StatusCode::INTERNAL_SERVER_ERROR }
        }))
        .route("/host/a/api/v1/sessions/s", get(session))
        .route("/host/a/api/v1/socket-ticket", post(|| async { Json(json!({"ticket":"fixture-ticket"})) }))
        .route("/host/a/api/v1/terminal/s", get(move |ws: WebSocketUpgrade| {
            let (attaches, relay_output_rx, relay_input_tx) = (attaches.clone(), relay_output_rx.clone(), relay_input_tx.clone());
            async move {
                attaches.fetch_add(1, SeqCst);
                ws.on_upgrade(move |mut socket| async move {
                    let Some(mut control) = relay_output_rx.lock().await.take() else { return };
                    socket.send(attached("7")).await.unwrap();
                    loop {
                        tokio::select! {
                            sequence = control.recv() => match sequence {
                                Some(sequence) => if socket.send(output(sequence, format!("r{sequence}").as_bytes())).await.is_err() { break },
                                None => break,
                            },
                            message = socket.recv() => match message {
                                Some(Ok(Message::Binary(bytes))) => { let _ = relay_input_tx.send(bytes.to_vec()).await; }
                                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                                Some(Ok(_)) => {}
                            },
                        }
                    }
                })
            }
        }));
    let relay = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

    let root = tempfile::tempdir().unwrap();
    let service = PairedHostService::open_test_loopback(root.path().join("data"));
    let host = service
        .pair(PairRequest { relay_origin: format!("http://{address}"), pin: Secret("fixture".into()), display_label: "fixture".into() })
        .await
        .unwrap();
    GLOBAL_DIRECT_ROUTES.reset(&host.host_id);
    let descriptor = Descriptor {
        host_id: host.host_id.clone(),
        generation: host.generation,
        target: RemoteTerminalTarget { machine_id: "a".into(), daemon_epoch: Epoch(1), session_id: "s".into() },
        after_sequence: None,
    };
    Fixture {
        service, descriptor, direct_origin, relay_output, relay_inputs, direct_arrivals, direct_inputs,
        relay_attaches, direct_attaches, creates, servers: vec![direct, relay], _root: root,
    }
}

/// Publishes a fresh direct route epoch for the host. `later` skips the retry backoff.
fn publish(host: &str, origin: &str, later: Duration) -> u64 {
    let epoch = GLOBAL_DIRECT_ROUTES.begin_attempt(host, std::time::Instant::now() + later).unwrap();
    assert!(GLOBAL_DIRECT_ROUTES.publish(host, epoch, origin.into(), DirectGuard::default()));
    epoch
}

async fn next_chunk(output: &mut tokio::sync::broadcast::Receiver<crate::terminal::output_hub::OutputChunk>) -> (u64, Vec<u8>) {
    let chunk = tokio::time::timeout(BOUND, output.recv()).await.unwrap().unwrap();
    (chunk.sequence, chunk.bytes.to_vec())
}

async fn upgraded(events: &mut tokio::sync::broadcast::Receiver<RecoveryTestEvent>) -> (u64, bool) {
    tokio::time::timeout(BOUND, async {
        loop {
            if let RecoveryTestEvent::Upgraded { epoch, direct } = events.recv().await.unwrap() { break (epoch, direct); }
        }
    }).await.unwrap()
}

async fn status(runtime: &Runtime, id: &str) -> (String, Option<Epoch>) {
    let (reply, received) = oneshot::channel();
    runtime.sender(id).unwrap().send(Command::Status(reply)).await.unwrap();
    let status = tokio::time::timeout(BOUND, received).await.unwrap().unwrap();
    (status.0, status.1)
}

async fn queue_write(runtime: &Runtime, id: &str, bytes: &[u8]) -> oneshot::Receiver<Result<(), String>> {
    let (reply, result) = oneshot::channel();
    runtime.sender(id).unwrap().send(Command::Write(0, bytes.to_vec(), reply)).await.unwrap();
    result
}

async fn write_ok(runtime: &Runtime, id: &str, bytes: &[u8]) {
    let reply = queue_write(runtime, id, bytes).await;
    tokio::time::timeout(BOUND, reply).await.unwrap().unwrap().unwrap();
}

/// Starts the actor on the relay and streams r11, r12 through it.
async fn relay_stream(fx: &Fixture, runtime: &Runtime, hub: &Arc<TerminalOutputHub>)
    -> (String, tokio::sync::broadcast::Receiver<crate::terminal::output_hub::OutputChunk>, Vec<(u64, Vec<u8>)>) {
    let (id, _) = runtime.get_or_rearm(fx.descriptor.clone(), hub.clone(), fx.service.clone()).unwrap();
    let mut output = hub.subscribe_with_sequence(&id, None).unwrap().receiver;
    fx.relay_output.send(11).await.unwrap();
    fx.relay_output.send(12).await.unwrap();
    let seen = vec![next_chunk(&mut output).await, next_chunk(&mut output).await];
    assert_eq!(status(runtime, &id).await, ("proxy_attached".into(), Some(Epoch(7))));
    (id, output, seen)
}

#[tokio::test]
async fn relay_stream_moves_to_published_direct_route_from_cursor() {
    let mut fx = fixture(DirectMode::Gated).await;
    let runtime = Runtime::default();
    let hub = Arc::new(TerminalOutputHub::new(64));
    let (id, mut output, mut seen) = relay_stream(&fx, &runtime, &hub).await;
    write_ok(&runtime, &id, b"before").await;
    assert_eq!(tokio::time::timeout(BOUND, fx.relay_inputs.recv()).await.unwrap().unwrap(), b"before");

    let mut events = runtime.subscribe_recovery_events(&id).await;
    let epoch = publish(&fx.descriptor.host_id, &fx.direct_origin, Duration::ZERO);
    let (query, gate) = tokio::time::timeout(BOUND, fx.direct_arrivals.recv()).await.unwrap().unwrap();
    assert_eq!(query.as_deref(), Some("daemonEpoch=1&afterSequence=12"));
    // Input arriving mid-switch is queued, not refused, and lands on the new route.
    let during = queue_write(&runtime, &id, b"during").await;
    gate.send(()).unwrap();
    assert_eq!(upgraded(&mut events).await, (epoch, true));
    tokio::time::timeout(BOUND, during).await.unwrap().unwrap().unwrap();
    write_ok(&runtime, &id, b"after").await;
    seen.push(next_chunk(&mut output).await);
    seen.push(next_chunk(&mut output).await);
    assert_eq!(status(&runtime, &id).await, ("proxy_attached".into(), Some(Epoch(8))));

    // One continuous hub stream: consecutive sequences, no duplicate or missing bytes.
    assert!(seen.windows(2).all(|pair| pair[1].0 == pair[0].0 + 1), "{seen:?}");
    assert_eq!(seen.iter().map(|(_, b)| b.clone()).collect::<Vec<_>>(), vec![b"r11".to_vec(), b"r12".to_vec(), b"d13".to_vec(), b"d14".to_vec()]);
    assert_eq!(runtime.descriptor(&id).map(|d| d.after_sequence), Some(Some(Epoch(14))));
    assert_eq!(tokio::time::timeout(BOUND, fx.direct_inputs.recv()).await.unwrap().unwrap(), b"during");
    assert_eq!(tokio::time::timeout(BOUND, fx.direct_inputs.recv()).await.unwrap().unwrap(), b"after");
    runtime.detach(&id).await.unwrap();
    // The relay saw only the pre-switch input; nothing was replayed or created.
    assert!(fx.relay_inputs.try_recv().is_err());
    assert_eq!((fx.relay_attaches.load(SeqCst), fx.direct_attaches.load(SeqCst), fx.creates.load(SeqCst)), (1, 1, 0));
    GLOBAL_DIRECT_ROUTES.reset(&fx.descriptor.host_id);
    fx.servers.iter().for_each(|server| server.abort());
}

#[tokio::test]
async fn failed_direct_upgrade_keeps_stream_on_relay() {
    let mut fx = fixture(DirectMode::DropAfterUpgrade).await;
    let runtime = Runtime::default();
    let hub = Arc::new(TerminalOutputHub::new(64));
    let (id, mut output, mut seen) = relay_stream(&fx, &runtime, &hub).await;
    let mut events = runtime.subscribe_recovery_events(&id).await;
    let epoch = publish(&fx.descriptor.host_id, &fx.direct_origin, Duration::ZERO);
    assert_eq!(upgraded(&mut events).await, (epoch, false));
    assert_eq!(GLOBAL_DIRECT_ROUTES.state(&fx.descriptor.host_id), RouteState::Disconnected);
    assert_eq!(status(&runtime, &id).await, ("proxy_attached".into(), Some(Epoch(7))));
    fx.relay_output.send(13).await.unwrap();
    seen.push(next_chunk(&mut output).await);
    write_ok(&runtime, &id, b"still-relay").await;
    assert_eq!(tokio::time::timeout(BOUND, fx.relay_inputs.recv()).await.unwrap().unwrap(), b"still-relay");
    assert_eq!(seen.iter().map(|(_, b)| b.clone()).collect::<Vec<_>>(), vec![b"r11".to_vec(), b"r12".to_vec(), b"r13".to_vec()]);
    assert!(seen.windows(2).all(|pair| pair[1].0 == pair[0].0 + 1));
    assert_eq!((fx.relay_attaches.load(SeqCst), fx.direct_attaches.load(SeqCst), fx.creates.load(SeqCst)), (1, 1, 0));
    runtime.detach(&id).await.unwrap();
    GLOBAL_DIRECT_ROUTES.reset(&fx.descriptor.host_id);
    fx.servers.iter().for_each(|server| server.abort());
}

/// While the actor is busy the route flaps far past the event buffer; the lag
/// is absorbed and only the epoch that is still live costs a network attach.
#[tokio::test]
async fn flapping_direct_route_costs_one_attach_per_live_epoch() {
    let mut fx = fixture(DirectMode::Gated).await;
    let runtime = Runtime::default();
    let hub = Arc::new(TerminalOutputHub::new(64));
    let (id, _output, _) = relay_stream(&fx, &runtime, &hub).await;
    let host = fx.descriptor.host_id.clone();
    let mut events = runtime.subscribe_recovery_events(&id).await;
    let (started, release) = (oneshot::channel(), oneshot::channel::<()>());
    runtime.block_actor(&id, started.0, release.1);
    tokio::time::timeout(BOUND, started.1).await.unwrap().unwrap();
    for flap in 1..=40u32 {
        let epoch = publish(&host, &fx.direct_origin, Duration::from_secs(1_000 * u64::from(flap)));
        GLOBAL_DIRECT_ROUTES.mark_disconnected(&host, epoch, std::time::Instant::now());
    }
    let live = publish(&host, &fx.direct_origin, Duration::from_secs(100_000));
    release.0.send(()).unwrap();
    let (_, gate) = tokio::time::timeout(BOUND, fx.direct_arrivals.recv()).await.unwrap().unwrap();
    gate.send(()).unwrap();
    tokio::time::timeout(BOUND, async {
        loop {
            if upgraded(&mut events).await == (live, true) { break; }
        }
    }).await.unwrap();
    assert_eq!(status(&runtime, &id).await, ("proxy_attached".into(), Some(Epoch(8))));
    // A repeated event for an already-tried epoch is inert; so is a stream on direct.
    GLOBAL_DIRECT_ROUTES.mark_disconnected(&host, live, std::time::Instant::now());
    assert_eq!(status(&runtime, &id).await.0, "proxy_attached");
    assert_eq!((fx.relay_attaches.load(SeqCst), fx.direct_attaches.load(SeqCst), fx.creates.load(SeqCst)), (1, 1, 0));
    assert!(fx.direct_arrivals.try_recv().is_err());
    runtime.detach(&id).await.unwrap();
    GLOBAL_DIRECT_ROUTES.reset(&host);
    fx.servers.iter().for_each(|server| server.abort());
}

#[tokio::test]
async fn direct_or_exited_proxy_is_not_upgraded() {
    let mut fx = fixture(DirectMode::Gated).await;
    let host = fx.descriptor.host_id.clone();
    let epoch = publish(&host, &fx.direct_origin, Duration::ZERO);
    let hub = Arc::new(TerminalOutputHub::new(64));
    let mut proxy = Proxy::new(fx.descriptor.clone(), hub).unwrap();
    let (client, service) = (crate::paired_host::client::MachineClient::new(), fx.service.clone());
    let attach = tokio::spawn(async move {
        proxy.reattach(&client, &service).await.map(|()| proxy)
    });
    let (_, gate) = tokio::time::timeout(BOUND, fx.direct_arrivals.recv()).await.unwrap().unwrap();
    gate.send(()).unwrap();
    let mut proxy = tokio::time::timeout(BOUND, attach).await.unwrap().unwrap().unwrap();
    assert!(!proxy.relay_attached(), "a stream already on direct is not a relay stream");
    assert!(!proxy.upgrade_direct(epoch).await.unwrap());
    assert_eq!(proxy.controller(), Some(Epoch(8)));
    proxy.recovery_failed(&crate::paired_host::client::ClientError::local("PAIRED_SESSION_EXITED"));
    assert!(proxy.exited());
    assert!(!proxy.relay_attached());
    assert!(!proxy.upgrade_direct(epoch).await.unwrap());
    assert_eq!((fx.relay_attaches.load(SeqCst), fx.direct_attaches.load(SeqCst), fx.creates.load(SeqCst)), (0, 1, 0));
    GLOBAL_DIRECT_ROUTES.reset(&host);
    fx.servers.iter().for_each(|server| server.abort());
}
