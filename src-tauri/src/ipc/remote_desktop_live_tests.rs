//! Live remote desktop session through Ferryx's own pieces: the daemon-owned host, the real
//! gateway routes behind the real bearer check, and the GUI client's loopback bridge, viewer
//! thread (decode + JPEG) and input mapper. The relay hop is skipped; the stream WebSocket is
//! dialed directly on loopback.
//!
//! Ignored by default: it needs a capturable display (an interactive desktop session with
//! screen capture permission). Run it with
//! `cargo test --manifest-path src-tauri/Cargo.toml --lib
//! ipc::remote_desktop::live_tests -- --ignored --nocapture`.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use ferryx_rd_app::{ClientSession, PairingStore, SessionConfig};
use tauri::ipc::{Channel, InvokeResponseBody, Response};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

use super::transport;
use super::viewer;
use super::wire::{self, InputMapper, RdInputEvent};
use crate::paired_host::client::{MachineClient, REMOTE_DESKTOP_MAX_FRAME_BYTES};
use crate::paired_host::service::PairedHostService;
use crate::remote::auth::{DeviceAccessScope, DevicePermission};
use crate::remote::remote_desktop::{RemoteDesktopHost, PAIR_ROUTE, STREAM_ROUTE};
use crate::remote::state::RemoteGatewayState;

const WANTED_FRAMES: usize = 30;
const FRAME_WINDOW: Duration = Duration::from_secs(20);
const SESSION_CHANGE_LIMIT: Duration = Duration::from_secs(10);

/// Waits on host state-change events until `done(session count)` holds or the bound passes,
/// returning the last observed count.
fn wait_for_sessions(
    host: &RemoteDesktopHost,
    events: &mpsc::Receiver<String>,
    done: impl Fn(usize) -> bool,
) -> usize {
    let deadline = Instant::now() + SESSION_CHANGE_LIMIT;
    loop {
        let count = host.state().sessions.len();
        if done(count) {
            return count;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if events.recv_timeout(remaining).is_err() {
            return host.state().sessions.len();
        }
    }
}

struct VideoFrame {
    width: u32,
    height: u32,
    frame_id: u64,
}

enum Message {
    Video(VideoFrame),
    Status { state: String, reason: Option<String> },
}

fn parse_message(bytes: &[u8]) -> Message {
    match bytes.first() {
        Some(&wire::FRAME_KIND_VIDEO) => {
            assert!(bytes.len() > 17, "video message carries a header and JPEG bytes");
            let u32_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
            let jpeg = &bytes[17..];
            assert!(jpeg.starts_with(&[0xFF, 0xD8]), "frame payload is a JPEG");
            Message::Video(VideoFrame {
                width: u32_at(1),
                height: u32_at(5),
                frame_id: u64::from_le_bytes(bytes[9..17].try_into().unwrap()),
            })
        }
        Some(&wire::FRAME_KIND_STATUS) => {
            let body: serde_json::Value =
                serde_json::from_slice(&bytes[1..]).expect("status message is JSON");
            Message::Status {
                state: body["state"].as_str().expect("status state").to_owned(),
                reason: body["reason"].as_str().map(str::to_owned),
            }
        }
        other => panic!("unknown frame channel message kind {other:?}"),
    }
}

#[test]
#[ignore = "live: needs a capturable desktop session"]
fn remote_desktop_session_streams_frames_end_to_end() {
    let dir = tempfile::tempdir().expect("tempdir");
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("runtime");
    // 1. The daemon-owned host, wired into a gateway state whose auth and RD stores live in
    //    the tempdir (`<auth dir>/remote-desktop`), never in the developer's data dir.
    let state = {
        let _context = rt.enter();
        Arc::new(RemoteGatewayState::new_with_paths(
            Arc::new(crate::terminal::TerminalService::default()),
            crate::worktree::WorkspaceRegistry::new(),
            Some(dir.path().join("remote-config.json")),
            Some(dir.path().join("remote-auth.json")),
        ))
    };
    let host = Arc::clone(&state.remote_desktop);
    let (event_tx, events) = mpsc::channel::<String>();
    host.set_event_sink(Arc::new(move |name: &str, _payload: serde_json::Value| {
        let _ = event_tx.send(name.to_owned());
    }));
    let started = host.set(true, true, true).expect("enable the host");
    assert!(
        started.enabled && started.running,
        "host did not start: {:?}",
        started.error
    );
    assert!(dir.path().join("remote-desktop").is_dir());

    // 2. The real gateway router on loopback and a Machine-scope Control device.
    let listener = rt
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .expect("bind gateway");
    let gateway = listener.local_addr().expect("gateway addr");
    let router = crate::remote::create_remote_router(Arc::clone(&state));
    let server = rt.spawn(async move { axum::serve(listener, router).await });
    let code = state
        .auth_manager
        .create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine)
        .expect("machine pairing code");
    let (token, _device) = state
        .auth_manager
        .exchange_pairing_code(&code, "rd live e2e")
        .expect("exchange pairing code");

    // Pairing through the real route.
    let (status, body) = rt.block_on(async {
        let http = reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("http client");
        let response = http
            .post(format!("http://{gateway}{PAIR_ROUTE}"))
            .bearer_auth(&token)
            .send()
            .await
            .expect("pair request");
        let status = response.status().as_u16();
        (status, response.bytes().await.expect("pair body").to_vec())
    });
    assert_eq!(status, 200, "pair: {}", String::from_utf8_lossy(&body));
    let grant: serde_json::Value = serde_json::from_slice(&body).expect("pair JSON");
    for key in ["pairingId", "keyB64", "hostName"] {
        assert!(grant[key].is_string(), "pair response lacks {key}: {grant}");
    }
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_millis() as u64;
    // The client's own grant parser (validates the 32-byte key).
    let pairing = transport::pairing_record(&body, now_ms).expect("pairing record");
    let client_dir = dir.path().join("client");
    std::fs::create_dir_all(&client_dir).expect("client dir");
    let store_path = client_dir.join("remote-desktop-pairings.json");
    PairingStore::new(store_path.clone())
        .save(pairing.clone())
        .expect("store client pairing");

    // 3. The client: stream socket, loopback bridge, ClientSession on the viewer thread.
    let first = rt
        .block_on(async {
            let mut request = format!("ws://{gateway}{STREAM_ROUTE}")
                .into_client_request()
                .expect("stream request");
            request.headers_mut().insert(
                "authorization",
                format!("Bearer {token}").parse().expect("bearer header"),
            );
            let config = WebSocketConfig::default()
                .max_message_size(Some(REMOTE_DESKTOP_MAX_FRAME_BYTES))
                .max_frame_size(Some(REMOTE_DESKTOP_MAX_FRAME_BYTES));
            crate::paired_host::secure_http::connect_net_websocket(request, config).await
        })
        .expect("open the stream socket");
    let (bridge_listener, bridge_udp) = rt.block_on(async {
        (
            tokio::net::TcpListener::bind(("127.0.0.1", 0))
                .await
                .expect("bridge tcp"),
            tokio::net::UdpSocket::bind(("127.0.0.1", 0))
                .await
                .expect("bridge udp"),
        )
    });
    let loopback_tcp = bridge_listener.local_addr().expect("bridge tcp addr").port();
    let loopback_udp = bridge_udp.local_addr().expect("bridge udp addr").port();
    let (bridge_stop, bridge_stopped) = tokio::sync::watch::channel(false);
    // A reopen (only after the first TCP connection drops) targets an unknown paired host and
    // fails closed; this test expects exactly one connection.
    let bridge = rt.spawn(transport::run_bridge(
        bridge_listener,
        bridge_udp,
        first,
        transport::StreamSource {
            client: Arc::new(MachineClient::new()),
            service: PairedHostService::open(client_dir.clone()),
            host_id: "rd-live-e2e".to_owned(),
            generation: crate::scoped_contracts::Epoch(0),
        },
        bridge_stopped,
    ));

    let mut config = SessionConfig::direct("127.0.0.1", "Ferryx");
    config.tcp_port = loopback_tcp;
    config.udp_port = loopback_udp;
    config.pairing_store_path = Some(store_path);
    let session = ClientSession::new(config).expect("client session");
    let (frame_tx, frames) = mpsc::channel::<Vec<u8>>();
    let channel = Channel::<Response>::new(move |body: InvokeResponseBody| {
        if let InvokeResponseBody::Raw(bytes) = body {
            let _ = frame_tx.send(bytes);
        }
        Ok(())
    });
    let stop = Arc::new(AtomicBool::new(false));
    let viewer_config = viewer::ViewerConfig {
        session: session.clone(),
        pairing,
        loopback_tcp,
        loopback_udp,
        frames: channel,
        stop: Arc::clone(&stop),
        on_exit: Box::new(|| {}),
    };
    let window_started = Instant::now();
    let viewer_thread = std::thread::Builder::new()
        .name("rd-live-e2e-viewer".to_owned())
        .spawn(move || viewer::run(viewer_config))
        .expect("viewer thread");

    // 4. Frames, input and host bookkeeping.
    let deadline = window_started + FRAME_WINDOW;
    let mut received: Vec<VideoFrame> = Vec::new();
    let mut connected = false;
    let mut input_sent = false;
    while received.len() < WANTED_FRAMES {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let bytes = frames.recv_timeout(remaining).unwrap_or_else(|_| {
            panic!(
                "only {} decoded frames within {FRAME_WINDOW:?} (connected: {connected})",
                received.len()
            )
        });
        match parse_message(&bytes) {
            Message::Status { state, reason } => match state.as_str() {
                "connected" => connected = true,
                "ended" => panic!("session ended early: {reason:?}"),
                _ => {}
            },
            Message::Video(frame) => {
                assert!(connected, "video before the connected status");
                assert!(frame.width > 0 && frame.height > 0, "empty frame size");
                received.push(frame);
                if !input_sent {
                    input_sent = true;
                    let sessions = wait_for_sessions(&host, &events, |count| count == 1);
                    assert_eq!(sessions, 1, "host registers the session while it streams");
                    let event = InputMapper::default()
                        .map(&RdInputEvent::MouseMove { x: 0.5, y: 0.5 })
                        .expect("mouseMove maps");
                    session.send_inputs(&[event]).expect("mouseMove accepted");
                }
            }
        }
    }
    assert!(
        received
            .windows(2)
            .all(|pair| pair[0].frame_id < pair[1].frame_id),
        "frame ids increase"
    );
    let codec = session
        .stream_state()
        .ok()
        .and_then(|stream| stream.active)
        .map(|active| format!("{:?}", active.codec))
        .unwrap_or_else(|| "host-default".to_owned());
    let last = received.last().expect("frames");
    let (width, height) = (last.width, last.height);

    // Disconnect exactly as cmd_rd_disconnect does, then expect a clean teardown.
    stop.store(true, Ordering::SeqCst);
    session.interrupt();
    bridge_stop.send_replace(true);
    viewer_thread.join().expect("viewer thread exits cleanly");
    let ended = frames
        .try_iter()
        .map(|bytes| parse_message(&bytes))
        .any(|message| matches!(message, Message::Status { state, .. } if state == "ended"));
    assert!(ended, "viewer reports the ended status");
    rt.block_on(async { tokio::time::timeout(SESSION_CHANGE_LIMIT, bridge).await })
        .expect("bridge stops")
        .expect("bridge task");
    let sessions = wait_for_sessions(&host, &events, |count| count == 0);
    assert_eq!(sessions, 0, "host session count returns to 0");

    assert!(!server.is_finished(), "gateway kept serving for the whole session");
    server.abort();
    let stopped = host.set(false, true, true).expect("disable the host");
    assert!(!stopped.running && stopped.sessions.is_empty());

    println!(
        "RD_E2E frames={} size={width}x{height} codec={codec}",
        received.len()
    );
}
