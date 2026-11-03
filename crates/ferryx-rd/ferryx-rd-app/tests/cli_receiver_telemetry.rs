use std::{net::UdpSocket, time::Duration};

use ferryx_rd_net::{DatagramCipher, Direction, PskIdentity, TlsPskServer};
use ferryx_rd_proto::{
    ControlMessage, FrameChunk, FrameHeader, Handshake, PacketHeader, PacketType, TimestampStats,
    WireCodec,
};
use serde_json::Value;

const DEADLINE: Duration = Duration::from_secs(15);

#[path = "support/client_exe.rs"]
mod client_exe;

#[derive(Clone, Copy, Debug)]
enum Scenario {
    Video,
    MediaError,
    Empty,
}

// Every observation comes through the executable's real ClientSession. The peer
// knows only the wire payloads it sends, never a metrics setter or JSON fixture.
async fn execute(scenario: Scenario, unwritable: bool) -> Option<Value> {
    let temporary = tempfile::tempdir().unwrap();
    let stats_path = temporary.path().join("stats.json");
    if unwritable {
        std::fs::create_dir(&stats_path).unwrap();
    }
    let key = [0x39; 32];
    let server = TlsPskServer::new([PskIdentity::pairing("cli-telemetry", &key).unwrap()]).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let tcp_port = listener.local_addr().unwrap().port();
    let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
    udp.set_read_timeout(Some(DEADLINE)).unwrap();
    let udp_port = udp.local_addr().unwrap().port();
    // Subscribe to peer completion before starting the executable. Socket reads
    // are event waits with failure deadlines, not timing-based readiness polls.
    let peer = tokio::spawn(async move {
        let (socket, _) = tokio::time::timeout(DEADLINE, listener.accept())
            .await
            .unwrap()
            .unwrap();
        let socket = socket.into_std().unwrap();
        tokio::task::spawn_blocking(move || {
            let mut stream = server
                .accept_stream_until(socket, std::time::Instant::now() + DEADLINE)
                .unwrap();
            stream
                .ssl_stream()
                .get_ref()
                .set_read_timeout(Some(DEADLINE))
                .unwrap();
            stream
                .ssl_stream()
                .get_ref()
                .set_write_timeout(Some(DEADLINE))
                .unwrap();
            let frame = stream.read_frame().unwrap();
            assert_eq!(
                PacketHeader::decode(&frame[..PacketHeader::SIZE])
                    .unwrap()
                    .packet_type,
                PacketType::Handshake
            );
            let mut handshake = Handshake::decode(&frame[PacketHeader::SIZE..]).unwrap();
            handshake.capabilities = ferryx_rd_proto::Capabilities::AUTHENTICATED_UDP_REGISTRATION;
            let mut ack = PacketHeader::new(PacketType::HandshakeAck, 0, 0, 0)
                .encode()
                .unwrap();
            ack.extend_from_slice(&handshake.encode().unwrap());
            stream.write_frame(&ack).unwrap();
            let mut probe = [0; 1024];
            let (probe_len, address) = udp.recv_from(&mut probe).unwrap();
            let mut c2h =
                DatagramCipher::derive(&key, &handshake.session_salt, Direction::ClientToHost)
                    .unwrap();
            let (probe_hdr, _) = c2h.open_datagram(&probe[..probe_len]).unwrap();
            assert_eq!(probe_hdr.packet_type, PacketType::Ping);
            let mut cipher =
                DatagramCipher::derive(&key, &handshake.session_salt, Direction::HostToClient)
                    .unwrap();
            let mut packets = Vec::new();
            if !matches!(scenario, Scenario::Empty) {
                let stamps = TimestampStats {
                    frame_id: 0,
                    capture_us: 1000,
                    encode_start_us: 1010,
                    encode_end_us: 1030,
                    send_us: 1060,
                }
                .encode();
                // Fresh nonce, duplicate host identity: bytes count twice, stages once.
                packets.push((PacketType::Ping, 0, stamps.clone()));
                packets.push((PacketType::Ping, 1, stamps));
                match scenario {
                    Scenario::Video => {
                        let hex = include_str!("fixtures/hevc-continuity.hex")
                            .lines()
                            .next()
                            .unwrap();
                        let data: Vec<u8> = (0..hex.len())
                            .step_by(2)
                            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
                            .collect();
                        packets.push((
                            PacketType::FrameHeader,
                            2,
                            FrameHeader {
                                frame_id: 0,
                                width: 32,
                                height: 32,
                                is_key_frame: true,
                                total_chunks: 1,
                                total_size: data.len() as u32,
                            }
                            .encode()
                            .unwrap(),
                        ));
                        packets.push((
                            PacketType::FrameChunk,
                            3,
                            FrameChunk {
                                frame_id: 0,
                                chunk_index: 0,
                                data,
                            }
                            .encode()
                            .unwrap(),
                        ));
                    }
                    // Hard-window eviction creates known loss without waiting for
                    // the reorder grace or assuming any scheduler execution speed.
                    Scenario::MediaError => packets.push((PacketType::FrameHeader, 5000, vec![])),
                    Scenario::Empty => unreachable!(),
                }
            }
            let count = packets.len() as u64;
            let mut bytes = 0u64;
            for (kind, sequence, payload) in packets {
                let datagram = cipher
                    .seal_datagram(&PacketHeader::new(kind, sequence, 42, 0), &payload)
                    .unwrap();
                bytes += datagram.len() as u64;
                assert_eq!(udp.send_to(&datagram, address).unwrap(), datagram.len());
            }
            loop {
                let frame = stream.read_frame().unwrap();
                let header = PacketHeader::decode(&frame[..PacketHeader::SIZE]).unwrap();
                assert_eq!(header.packet_type, PacketType::Control);
                match ControlMessage::decode(&frame[PacketHeader::SIZE..]).unwrap() {
                    ControlMessage::RequestKeyFrame => {}
                    ControlMessage::Disconnect => break,
                    other => panic!("unexpected control: {other:?}"),
                }
            }
            // ClientSession closes the underlying socket without TLS close_notify.
            // Observe actual TCP EOF instead of depending on OpenSSL's error mapping.
            let mut socket = stream.ssl_stream().get_ref();
            assert_eq!(std::io::Read::read(&mut socket, &mut [0; 1]).unwrap(), 0);
            (count, bytes)
        })
        .await
        .unwrap()
    });
    let mut command = tokio::process::Command::new(client_exe::client_exe());
    command
        .args([
            "--host",
            "127.0.0.1",
            "--tcp-port",
            &tcp_port.to_string(),
            "--udp-port",
            &udp_port.to_string(),
            "--pairing-id",
            "cli-telemetry",
            "--psk-hex",
            &"39".repeat(32),
            "--frames",
            if matches!(scenario, Scenario::Empty) {
                "0"
            } else {
                "1"
            },
            "--timeout-secs",
            "10",
            "--stats-json",
        ])
        .arg(&stats_path)
        .arg("--pairing-store")
        .arg(temporary.path().join("pairings.json"))
        .env("RUST_LOG", "info")
        .kill_on_drop(true);
    let output_result = tokio::time::timeout(DEADLINE, command.output()).await;
    let peer_result = tokio::time::timeout(DEADLINE, peer).await;
    let output = output_result.expect("executable exit deadline").unwrap();
    let (count, bytes) = peer_result.expect("peer cleanup deadline").unwrap();
    // Inspect secrets without ever printing captured raw executable logs.
    for stream in [&output.stdout, &output.stderr] {
        assert!(
            !String::from_utf8_lossy(stream).contains(&"39".repeat(32)),
            "secret leaked"
        );
    }
    println!("CLI_CLEANUP scenario={scenario:?} unwritable={unwritable} disconnect=true tcp_eof=true child_exit={:?}", output.status.code());
    if unwritable {
        assert!(!output.status.success());
        assert!(stats_path.is_dir());
        return None;
    }
    let json: Value = serde_json::from_slice(&std::fs::read(&stats_path).unwrap()).unwrap();
    println!("CLI_EMITTED_JSON scenario={scenario:?} expected_datagrams={count} expected_bytes={bytes} {json}");
    let snapshot = json
        .get("receiver_snapshot")
        .expect("receiver snapshot missing from executable JSON");
    assert_eq!(snapshot["udp_authenticated_datagrams"], count);
    assert_eq!(snapshot["udp_authenticated_bytes"], bytes);
    for field in ["frames", "p50_us", "p95_us", "p99_us", "max_us"] {
        assert!(json[field].is_u64(), "legacy field {field}");
    }
    if matches!(scenario, Scenario::Empty) {
        assert!(output.status.success());
        assert_eq!(
            snapshot,
            &serde_json::to_value(ferryx_rd_app::ReceiverSnapshot::default()).unwrap()
        );
        assert_eq!(json["frames"], 0);
    } else {
        for (field, duration) in [
            ("host_ready_to_encode_us", 10),
            ("host_encode_us", 20),
            ("host_encode_to_send_complete_us", 30),
        ] {
            assert_eq!(
                snapshot[field],
                serde_json::json!({"frames": 1, "p50_us": duration, "p95_us": duration, "p99_us": duration, "max_us": duration})
            );
        }
        // Linux recvmmsg assigns one arrival timestamp to the entire batch.
        // A single batch has no observed interval, so throughput is legitimately
        // null even though authenticated bytes were received. Exact interval
        // arithmetic and the zero-interval case are covered with supplied clocks
        // by receiver_throughput_uses_only_positive_observed_interval_and_expires.
        let throughput = &snapshot["udp_receive_bps"];
        assert!(snapshot.get("udp_receive_bps").is_some());
        if !throughput.is_null() {
            let bps = throughput.as_f64().expect("throughput must be numeric");
            assert!(bps.is_finite() && bps > 0.0);
        }
        if let Some(ratio) = snapshot["packet_loss_ratio"].as_f64() {
            assert!((0.0..=1.0).contains(&ratio));
            assert_eq!(
                ratio,
                snapshot["loss_missing_packets"].as_u64().unwrap() as f64
                    / snapshot["loss_expected_packets"].as_u64().unwrap() as f64
            );
        }
        match scenario {
            Scenario::Video => {
                assert!(output.status.success());
                assert_eq!(json["frames"], 1);
                assert_eq!(snapshot["receive_assembly_us"]["frames"], 1);
                assert_eq!(snapshot["loss_missing_packets"], 0);
            }
            Scenario::MediaError => {
                assert!(
                    !output.status.success(),
                    "authenticated malformed media must retain its error"
                );
                assert_eq!(json["frames"], 0);
                assert!(snapshot["receive_assembly_us"].is_null());
                assert!(snapshot["packet_loss_ratio"].as_f64().unwrap() > 0.0);
                assert!(snapshot["loss_missing_packets"].as_u64().unwrap() > 0);
            }
            Scenario::Empty => unreachable!(),
        }
    }
    Some(json)
}

#[tokio::test]
async fn actual_executable_exports_authenticated_receiver_and_legacy_decode_statistics() {
    execute(Scenario::Video, false).await;
    // A second invocation with no datagrams must not inherit the first session.
    execute(Scenario::Empty, false).await;
}

#[tokio::test]
async fn actual_executable_exports_receiver_on_media_error_and_closes_transport() {
    execute(Scenario::MediaError, false).await;
}

#[tokio::test]
async fn actual_executable_closes_transport_even_when_stats_file_is_unwritable() {
    execute(Scenario::Video, true).await;
}
