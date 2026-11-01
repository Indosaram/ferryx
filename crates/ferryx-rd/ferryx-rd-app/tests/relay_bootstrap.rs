use futures_util::{SinkExt, StreamExt};
use ferryx_rd_app::{ClientSession, SessionConfig};
use ferryx_rd_net::{PskIdentity, TlsPskServer};
use ferryx_rd_proto::{
    Capabilities, Handshake, PacketHeader, PacketType, PairingGrant, PairingTrust,
    RelayConnectCode, RelayControl, WireCodec,
};
use std::{
    net::{TcpListener, UdpSocket},
    thread,
    time::Duration,
};
use tokio_tungstenite::{accept_async, tungstenite::Message};

fn packet(kind: PacketType, payload: Vec<u8>) -> Vec<u8> {
    let mut bytes = PacketHeader::new(kind, 0, 0, 0).encode().unwrap();
    bytes.extend(payload);
    bytes
}

// A subprocess supplies relay credentials without mutating the test process's
// global environment while other session tests run.
#[test]
fn pin_bootstrap_direct_first_then_ciphertext_relay_with_legacy_host() {
    if std::env::var_os("MAHO_TEST_RELAY_CHILD").is_none() {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "pin_bootstrap_direct_first_then_ciphertext_relay_with_legacy_host",
                "--nocapture",
            ])
            .env("MAHO_TEST_RELAY_CHILD", "1")
            .env("RELAY_AUTH_SECRET", "test-secret")
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    let (finished_tx, finished_rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        if finished_rx.recv_timeout(Duration::from_secs(30)).is_err() {
            eprintln!("relay bootstrap test exceeded its bounded completion deadline");
            std::process::exit(1);
        }
    });
    let pin = "12345678";
    let tls = TlsPskServer::new([PskIdentity::bootstrap(pin).unwrap()])
        .unwrap()
        .bind("127.0.0.1:0")
        .unwrap();
    let tls_addr = tls.local_addr().unwrap();
    let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
    udp.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let udp_port = udp.local_addr().unwrap().port();
    let host = thread::spawn(move || {
        let mut stream = tls.accept().unwrap();
        let request = stream.read_frame().unwrap();
        assert_eq!(
            PacketHeader::decode(&request[..PacketHeader::SIZE])
                .unwrap()
                .packet_type,
            PacketType::PairingRequest
        );
        let grant = PairingGrant {
            pairing_id: "relay-first-pair".into(),
            host_name: "host".into(),
            key: [42; 32],
        };
        stream
            .write_frame(&packet(PacketType::PairingGrant, grant.encode().unwrap()))
            .unwrap();
        let hello = stream.read_frame().unwrap();
        let hello = Handshake::decode(&hello[PacketHeader::SIZE..]).unwrap();
        // Legacy host: no new relay capability or pairing-trust extension.
        let ack = Handshake {
            name: "host".into(),
            width: 640,
            height: 480,
            scale: 1.0,
            version: ferryx_rd_proto::PROTOCOL_VERSION,
            capabilities: Capabilities::AUTHENTICATED_UDP_REGISTRATION,
            pairing_id: String::new(),
            session_salt: [0; 16],
        };
        stream
            .write_frame(&packet(PacketType::HandshakeAck, ack.encode().unwrap()))
            .unwrap();
        let mut bytes = [0; 2048];
        let (len, _) = udp.recv_from(&mut bytes).unwrap();
        let mut cipher = ferryx_rd_net::DatagramCipher::derive(
            &grant.key,
            &hello.session_salt,
            ferryx_rd_net::Direction::ClientToHost,
        )
        .unwrap();
        assert_eq!(
            cipher.open_datagram(&bytes[..len]).unwrap().0.packet_type,
            PacketType::Ping
        );
    });
    let direct = TcpListener::bind("127.0.0.1:0").unwrap();
    let direct_port = direct.local_addr().unwrap().port();
    let (attempt_tx, attempt_rx) = std::sync::mpsc::channel();
    let rejected = thread::spawn(move || {
        let (socket, _) = direct.accept().unwrap();
        attempt_tx.send(()).unwrap();
        drop(socket); // Force direct TLS failure before relay bootstrap.
    });
    let relay = TcpListener::bind("127.0.0.1:0").unwrap();
    relay.set_nonblocking(true).unwrap();
    let relay_addr = relay.local_addr().unwrap();
    let bridge = thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async move {
            let relay = tokio::net::TcpListener::from_std(relay).unwrap();
            let (socket, _) = relay.accept().await.unwrap();
            attempt_rx.try_recv().expect("direct attempt must precede relay arrival");
            let mut ws = accept_async(socket).await.unwrap();
            let Message::Text(control) = ws.next().await.unwrap().unwrap() else { panic!("expected control") };
            assert!(!control.contains(pin));
            let control: RelayControl = serde_json::from_str(&control).unwrap();
            assert_eq!(control, RelayControl::Connect { host_id: "exact-host-id".into(), token: ferryx_rd_proto::relay_token(b"test-secret", "exact-host-id") });
            ws.send(Message::Text(serde_json::to_string(&RelayControl::Connected).unwrap().into())).await.unwrap();
            let tcp = tokio::net::TcpStream::connect(tls_addr).await.unwrap();
            let (mut read, mut write) = tcp.into_split();
            let udp = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let mut bytes = [0; 16384];
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            loop {
                tokio::select! {
                    incoming = ws.next() => {
                        let Some(Ok(Message::Binary(bytes))) = incoming else { break };
                        assert!(!bytes.windows(pin.len()).any(|window| window == pin.as_bytes()));
                        let frame = ferryx_rd_proto::decode_relay_frame(&bytes).unwrap();
                        if frame.channel == ferryx_rd_proto::RELAY_CHANNEL_TCP {
                            write.write_all(&frame.payload).await.unwrap();
                        } else {
                            udp.send_to(&frame.payload, ("127.0.0.1", udp_port)).await.unwrap();
                            break;
                        }
                    }
                    len = read.read(&mut bytes) => {
                        let len = len.unwrap();
                        if len == 0 { break; }
                        assert!(!bytes[..len].windows(pin.len()).any(|window| window == pin.as_bytes()));
                        let frame = ferryx_rd_proto::encode_relay_frame(&[0;16], ferryx_rd_proto::RELAY_CHANNEL_TCP, &bytes[..len]).unwrap();
                        ws.send(Message::Binary(frame.into())).await.unwrap();
                    }
                }
            }
        });
    });
    let temp = tempfile::tempdir().unwrap();
    let mut config = SessionConfig::direct("127.0.0.1", "relay-client");
    config.tcp_port = direct_port;
    config.pairing_store_path = Some(temp.path().join("pairings.json"));
    let session = ClientSession::new(config).unwrap();
    let route = RelayConnectCode {
        host_id: "exact-host-id".into(),
        url: format!("ws://{relay_addr}"),
    };
    let ready = session
        .pair_with_pin_via_relay(pin, PairingTrust::Owner, Some(&route))
        .unwrap();
    assert!(session.is_relayed());
    assert_eq!(
        ready.pairing.relay_host_id.as_deref(),
        Some("exact-host-id")
    );
    assert_eq!(ready.pairing.relay_url.as_deref(), Some(route.url.as_str()));
    assert_eq!(
        ferryx_rd_app::PairingStore::new(temp.path().join("pairings.json"))
            .load(&ready.pairing.id)
            .unwrap()
            .unwrap()
            .relay_host_id,
        ready.pairing.relay_host_id
    );
    rejected.join().unwrap();
    bridge.join().unwrap();
    host.join().unwrap();
    session.disconnect().unwrap();
    finished_tx.send(()).unwrap();
}
