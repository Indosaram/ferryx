//! Real gateway / TLS-PSK / encrypted media / viewer contracts without OS devices.
#![cfg(feature = "remote-desktop")]

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use ferryx_rd_app::{frame_queue::PlayoutMode, ClientSession, PairingStore, SessionConfig};
use ferryx_rd_host::session::{DisplayInfo, SyntheticHostObserver, SyntheticHostSpec, QUIC_TUNNEL_MAX_UDP_PAYLOAD_BYTES};
use ferryx_rd_host::{HostConfig, HostServer};
use ferryx_rd_proto::{InputEventType, StreamCodecPreference};
use tauri::ipc::{Channel, InvokeResponseBody, Response};
use tokio_tungstenite::{tungstenite::client::IntoClientRequest, MaybeTlsStream};

use super::{transport, viewer, wire};
use crate::ipc::remote_desktop_media::RdAudioPlayer;
use crate::ipc::remote_desktop_stats::RdSessionStats;
use crate::paired_host::client::{MachineClient, RdBridgeTransport, RdStreamConnection, RD_DGRAM_ID_HEADER, RD_DGRAM_OPT_IN_HEADER};
use crate::paired_host::direct::{create_quic_endpoint, generate_ephemeral_cert};
use crate::paired_host::direct::datagram::{spawn_datagram_reader, DatagramMux, DirectDatagramPeer, LoopbackPeerGuard};
use crate::paired_host::direct_route::{DirectGuard, GLOBAL_DIRECT_ROUTES};
use crate::paired_host::secure_http::PairedStream;
use crate::paired_host::service::PairedHostService;
use crate::remote::auth::{DeviceAccessScope, DevicePermission};
use crate::remote::remote_desktop::{pairing_for_device, STREAM_ROUTE};
use crate::remote::state::RemoteGatewayState;

const BOUND: Duration = Duration::from_secs(20);

struct QuicPair {
    server_endpoint: quinn::Endpoint,
    client_endpoint: quinn::Endpoint,
    host: quinn::Connection,
    client: quinn::Connection,
}

impl Drop for QuicPair {
    fn drop(&mut self) {
        self.server_endpoint.close(0u32.into(), b"contract finished");
        self.client_endpoint.close(0u32.into(), b"contract finished");
    }
}

async fn quic_pair() -> QuicPair {
    let server_sock = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let client_sock = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = server_sock.local_addr().unwrap();
    let server_cert = generate_ephemeral_cert().unwrap();
    let client_cert = generate_ephemeral_cert().unwrap();
    let server_endpoint = create_quic_endpoint(server_sock, &server_cert, &client_cert.cert_der, true).unwrap();
    let client_endpoint = create_quic_endpoint(client_sock, &client_cert, &server_cert.cert_der, false).unwrap();
    let (client, host) = tokio::join!(
        client_endpoint.connect(addr, "ferryx-direct").unwrap(),
        async { server_endpoint.accept().await.unwrap().await },
    );
    QuicPair { server_endpoint, client_endpoint, host: host.unwrap(), client: client.unwrap() }
}

struct Harness {
    state: Arc<RemoteGatewayState>,
    device_id: String,
    host_id: String,
    _quic: QuicPair,
    _peer: Option<LoopbackPeerGuard>,
    host_mux: Arc<DatagramMux>,
    client_mux: Arc<DatagramMux>,
    reader: tokio::task::JoinHandle<()>,
    server: tokio::task::JoinHandle<()>,
    bridge: Option<tokio::task::JoinHandle<()>>,
    bridge_stop: tokio::sync::watch::Sender<bool>,
    host_stop: Arc<AtomicBool>,
    host_done: mpsc::Receiver<()>,
    host_thread: Option<std::thread::JoinHandle<()>>,
    viewer_stop: Arc<AtomicBool>,
    viewer_done: mpsc::Receiver<()>,
    viewer_thread: Option<std::thread::JoinHandle<()>>,
    frames: mpsc::Receiver<Vec<u8>>,
    shared: viewer::ViewerShared,
    session: ClientSession,
    observer: Arc<SyntheticHostObserver>,
    // Fields drop in declaration order: retain runtime and stores until last.
    rt: tokio::runtime::Runtime,
    _dir: tempfile::TempDir,
}

impl Harness {
    fn new(direct: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(4).enable_all().build().unwrap();
        let state = {
            let _entered = rt.enter();
            Arc::new(RemoteGatewayState::new_with_paths(
                Arc::new(crate::terminal::TerminalService::default()),
                crate::worktree::WorkspaceRegistry::new(),
                Some(dir.path().join("remote-config.json")),
                Some(dir.path().join("remote-auth.json")),
            ))
        };
        let code = state.auth_manager.create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine).unwrap();
        let (token, device) = state.auth_manager.exchange_pairing_code(&code, "contract viewer").unwrap();
        let host_store = ferryx_rd_host::PairingStore::new(dir.path().join("host-pairings.json"));
        let record = pairing_for_device(&host_store, &device.id, &device.name).unwrap();
        use base64::Engine as _;
        let grant = serde_json::to_vec(&serde_json::json!({
            "pairingId": record.id,
            "keyB64": base64::engine::general_purpose::STANDARD.encode(record.key),
            "hostName": "synthetic"
        })).unwrap();
        let pairing = transport::pairing_record(&grant, 0).unwrap();
        let store_path = dir.path().join("client-pairings.json");
        PairingStore::new(store_path.clone()).save(pairing.clone()).unwrap();
        let fixture = include_str!("../../../crates/ferryx-rd/ferryx-rd-decode/tests/fixtures/black_16x16.hevc.hex");
        let keyframe_bytes = fixture.split_whitespace().map(|hex| u8::from_str_radix(hex, 16).unwrap()).collect();
        let config = HostConfig {
            tcp_addr: "127.0.0.1:0".parse().unwrap(),
            udp_addr: "127.0.0.1:0".parse().unwrap(),
            bootstrap_pin: None,
            pairing_window: BOUND,
            pairing_store: host_store,
            host_name: "synthetic".into(),
            display: DisplayInfo { desktop_x: 0, desktop_y: 0, logical_width: 16, logical_height: 16, pixel_width: 16, pixel_height: 16, scale_factor_milli: 1000 },
            frames_per_second: 60,
            bitrate: 1_500_000,
            capture_audio: true,
            consent_sender: None,
            output_name: None,
            allow_input: Arc::new(AtomicBool::new(true)),
            allow_clipboard: Arc::new(AtomicBool::new(false)),
            max_udp_payload_bytes: QUIC_TUNNEL_MAX_UDP_PAYLOAD_BYTES,
        };
        let (host, observer) = HostServer::bind_synthetic_for_test(config, SyntheticHostSpec {
            keyframe_bytes,
            codec: StreamCodecPreference::Hevc,
            audio_pcm_48khz_f32_le: (0..480 * 2).flat_map(|_| 0.125_f32.to_le_bytes()).collect(),
            width: 16,
            height: 16,
            // Media cadence is stimulus, not a synchronization delay.
            frame_interval: Duration::from_secs(1) / 60,
        }).unwrap();
        state.remote_desktop.set_test_endpoints(host.tcp_addr().unwrap(), host.udp_addr().unwrap());
        let host_stop = Arc::new(AtomicBool::new(false));
        let (host_tx, host_done) = mpsc::channel();
        let host_thread = std::thread::spawn({
            let stop = Arc::clone(&host_stop);
            move || { host.serve_with_stop(stop).unwrap(); host_tx.send(()).unwrap(); }
        });
        let host_id = format!("rd-contract-{}", uuid::Uuid::new_v4());
        let shared = viewer::ViewerShared::new(&host_id, 60, PlayoutMode::Immediate);
        *shared.audio.lock() = RdAudioPlayer::new_headless_for_test();
        // Keep the HEVC fixture on HEVC even when a platform chooses software decode.
        let options = viewer::ViewerOptions { codec_override: Some(StreamCodecPreference::Hevc) };
        let host_mux = DatagramMux::new();
        let client_mux = DatagramMux::new();
        let (quic, peer, reader, server, bridge, bridge_stop, tcp_port, udp_port) = rt.block_on(async {
            tokio::time::timeout(BOUND, async {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let addr = listener.local_addr().unwrap();
                let router = crate::remote::create_remote_router(Arc::clone(&state));
                let server = tokio::spawn(async move {
                    axum::serve(listener, router.into_make_service_with_connect_info::<SocketAddr>()).await.unwrap();
                });
                let quic = quic_pair().await;
                let reader = spawn_datagram_reader(quic.host.clone(), Arc::clone(&host_mux));
                let client_reader = spawn_datagram_reader(quic.client.clone(), Arc::clone(&client_mux));
                let mut guard = DirectGuard::new(quic.client.clone(), quic.client_endpoint.clone()).with_datagram_mux(Arc::clone(&client_mux));
                guard.track(client_reader.abort_handle());
                let epoch = GLOBAL_DIRECT_ROUTES.begin_attempt(&host_id, Instant::now()).unwrap();
                assert!(GLOBAL_DIRECT_ROUTES.publish(&host_id, epoch, format!("http://{addr}"), guard));
                let tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
                let peer = direct.then(|| state.direct_datagram_peers.register(tcp.local_addr().unwrap(), DirectDatagramPeer {
                    connection: quic.host.clone(), mux: Arc::clone(&host_mux),
                }));
                let mut request = format!("ws://{addr}{STREAM_ROUTE}").into_client_request().unwrap();
                request.headers_mut().insert("authorization", format!("Bearer {token}").parse().unwrap());
                request.headers_mut().insert(RD_DGRAM_OPT_IN_HEADER, "1".parse().unwrap());
                let (socket, response) = tokio_tungstenite::client_async_with_config(request, MaybeTlsStream::Plain(PairedStream::Net(tcp)), None).await.unwrap();
                let channel_id = response.headers().get(RD_DGRAM_ID_HEADER).map(|value| {
                    let hex = value.to_str().unwrap();
                    assert_eq!(hex.len(), 16);
                    let id = u64::from_str_radix(hex, 16).unwrap();
                    assert_ne!(id, 0);
                    id
                });
                assert_eq!(channel_id.is_some(), direct);
                assert_eq!(host_mux.len(), usize::from(direct));
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let udp = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
                let tcp_port = listener.local_addr().unwrap().port();
                let udp_port = udp.local_addr().unwrap().port();
                let (bridge_stop, stopped) = tokio::sync::watch::channel(false);
                let bridge = tokio::spawn(transport::start_bridge(listener, udp, RdStreamConnection {
                    socket, transport: RdBridgeTransport::Direct { epoch, dgram_channel_id: channel_id },
                }, transport::StreamSource {
                    client: Arc::new(MachineClient::new()),
                    service: PairedHostService::open(dir.path().join("client")),
                    host_id: host_id.clone(),
                    generation: crate::scoped_contracts::Epoch(0),
                }, stopped, Some(Arc::clone(&shared.telemetry))));
                (quic, peer, reader, server, bridge, bridge_stop, tcp_port, udp_port)
            }).await.expect("bounded gateway setup")
        });
        let mut config = SessionConfig::direct("127.0.0.1", "contract viewer");
        config.tcp_port = tcp_port;
        config.udp_port = udp_port;
        config.pairing_store_path = Some(store_path);
        let session = ClientSession::new(config).unwrap();
        let (tx, frames) = mpsc::channel();
        let channel = Channel::<Response>::new(move |body: InvokeResponseBody| {
            if let InvokeResponseBody::Raw(bytes) = body { let _ = tx.send(bytes); }
            Ok(())
        });
        let viewer_stop = Arc::new(AtomicBool::new(false));
        let (done, viewer_done) = mpsc::channel();
        let viewer_config = viewer::ViewerConfig {
            session: session.clone(), pairing, loopback_tcp: tcp_port, loopback_udp: udp_port,
            frames: channel, stop: Arc::clone(&viewer_stop),
            on_exit: Box::new(move || { done.send(()).unwrap(); }),
        };
        let viewer_thread = std::thread::spawn({
            let shared = shared.clone();
            move || viewer::run_with(viewer_config, shared, options)
        });
        Self { rt, _dir: dir, state, device_id: device.id, host_id, _quic: quic, _peer: peer,
            host_mux, client_mux, reader, server, bridge: Some(bridge), bridge_stop,
            host_stop, host_done, host_thread: Some(host_thread), viewer_stop, viewer_done,
            viewer_thread: Some(viewer_thread), frames, shared, session, observer }
    }

    fn stats(&self) -> RdSessionStats {
        self.shared.telemetry.lock().snapshot(None, Instant::now())
    }

    fn media_ready(&self, mode: &str) {
        let deadline = Instant::now() + BOUND;
        let mut video_count = 0;
        loop {
            let bytes = self.frames.recv_timeout(deadline.saturating_duration_since(Instant::now())).expect("decoded video and PCM within bound");
            match bytes[0] {
                wire::FRAME_KIND_VIDEO => {
                    assert_eq!(u32::from_le_bytes(bytes[1..5].try_into().unwrap()), 16);
                    assert_eq!(u32::from_le_bytes(bytes[5..9].try_into().unwrap()), 16);
                    assert!(bytes[17..].starts_with(&[0xff, 0xd8]));
                    video_count += 1;
                }
                wire::FRAME_KIND_STATUS => {
                    let status: serde_json::Value = serde_json::from_slice(&bytes[1..]).unwrap();
                    assert_ne!(status["state"], "ended", "{status}");
                }
                wire::FRAME_KIND_CURSOR | wire::FRAME_KIND_NATIVE_PRESENTED => {}
                other => panic!("unexpected viewer message {other}"),
            }
            // Each channel delivery is an actual pipeline event, not a polling timer.
            let stats = self.stats();
            if video_count >= 3 && stats.present_fps > 0.0 && self.shared.audio.lock().queued_samples() > 0 {
                assert_eq!(stats.transport_mode, mode);
                assert_eq!(stats.codec, "hevc");
                assert!(stats.decode_us.is_some());
                assert!(stats.present_us.is_some());
                let audio = self.shared.audio.lock();
                assert_eq!(audio.queued_samples() % 2, 0, "stereo PCM");
                let (active, underruns, drops) = audio.stats();
                assert!(!active, "headless PCM never opens hardware");
                assert_eq!(underruns, 0, "no hardware consumer can underrun");
                // With no consumer, bounded overflow is expected on slow builders;
                // it must preserve complete stereo frames, not depend on decode speed.
                assert_eq!(drops % 2, 0, "overflow preserves stereo frames");
                break;
            }
        }
    }

    fn shutdown(&mut self) {
        self.viewer_stop.store(true, Ordering::SeqCst);
        self.session.interrupt();
        self.bridge_stop.send_replace(true);
        self.viewer_done.recv_timeout(BOUND).expect("viewer shutdown is bounded");
        self.viewer_thread.take().unwrap().join().expect("viewer did not panic");
        self.rt.block_on(async {
            tokio::time::timeout(BOUND, self.bridge.take().unwrap()).await.expect("bridge shutdown is bounded").expect("bridge did not panic");
        });
        assert!(self.shared.frame_queue.current().is_none());
        assert!(self.shared.session_handle.lock().is_none());
        assert_eq!(self.shared.audio.lock().queued_samples(), 0);
        self.host_stop.store(true, Ordering::SeqCst);
        self.host_done.recv_timeout(BOUND).expect("host shutdown is bounded");
        self.host_thread.take().unwrap().join().expect("host did not panic");
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.viewer_stop.store(true, Ordering::SeqCst);
        self.session.interrupt();
        self.bridge_stop.send_replace(true);
        self.host_stop.store(true, Ordering::SeqCst);
        if let Some(bridge) = &self.bridge { bridge.abort(); }
        self.server.abort();
        self.reader.abort();
        GLOBAL_DIRECT_ROUTES.reset(&self.host_id);
    }
}

#[test]
fn direct_quic_decodes_video_records_telemetry_and_queues_headless_pcm() {
    let mut h = Harness::new(true);
    h.media_ready("quic_datagram");
    assert_eq!(h.host_mux.len(), 1);
    assert_eq!(h.client_mux.len(), 1);
    h.shutdown();
}

#[test]
fn unregistered_peer_decodes_video_on_ws_direct_fallback() {
    let mut h = Harness::new(false);
    h.media_ready("ws_direct");
    assert_eq!(h.host_mux.len(), 0);
    assert_eq!(h.client_mux.len(), 0);
    h.shutdown();
}

#[test]
fn live_refresh_change_requests_75_fps_and_updates_stats() {
    let mut h = Harness::new(true);
    h.media_ready("quic_datagram");
    assert!(h.observer.wait_for_restart(|&(fps, _, _)| fps == 60, BOUND));
    assert_eq!(h.stats().target_fps, 60);
    assert!(h.shared.apply_refresh(75));
    assert!(viewer::send_stream_options(&h.session, &h.shared, false));
    assert!(h.observer.wait_for_restart(|&(fps, _, _)| fps == 75, BOUND));
    assert_eq!(h.stats().target_fps, 75);
    h.shutdown();
}

#[test]
fn relative_move_reaches_the_host_input_observer() {
    let mut h = Harness::new(true);
    h.media_ready("quic_datagram");
    let event = wire::InputMapper::default().map(&wire::RdInputEvent::RelativeMove { dx: 12.0, dy: -5.0 }).unwrap();
    h.session.send_input(event).unwrap();
    assert!(h.observer.wait_for_input(|ev| ev.event_type == InputEventType::RelativeMove && ev.scroll_dx == 12.0 && ev.scroll_dy == -5.0, BOUND));
    h.shutdown();
}

#[test]
fn midstream_revocation_closes_bridge_removes_channels_and_allows_clean_shutdown() {
    let mut h = Harness::new(true);
    h.media_ready("quic_datagram");
    assert_eq!(h.host_mux.len(), 1);
    assert!(h.state.auth_manager.revoke_device(&h.device_id).unwrap());
    let deadline = Instant::now() + BOUND;
    // The viewer's reconnect/ended event proves the stream socket closed. The host
    // channel guard drops before that close, and the client pump drops its guard
    // before the viewer can observe its TCP EOF. Do not clear either mux ourselves.
    loop {
        let bytes = h.frames.recv_timeout(deadline.saturating_duration_since(Instant::now())).expect("revocation closes the stream");
        if bytes[0] == wire::FRAME_KIND_STATUS {
            let status: serde_json::Value = serde_json::from_slice(&bytes[1..]).unwrap();
            if status["state"] == "reconnecting" || status["state"] == "ended" { break; }
        }
    }
    assert_eq!(h.host_mux.len(), 0);
    assert_eq!(h.client_mux.len(), 0);
    h.shutdown();
}
