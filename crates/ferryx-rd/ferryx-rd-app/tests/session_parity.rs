use std::{net::UdpSocket, sync::mpsc, thread, time::Duration};

use ferryx_rd_app::{
    ClientSession, PairingRecord, PairingStore, ReconnectState, SessionConfig, SessionEvent,
};
use ferryx_rd_net::{PskIdentity, TlsPskServer};
use ferryx_rd_proto::{
    Capabilities, ControlMessage, Handshake, PacketHeader, PacketType, PairingGrant, PairingTrust,
    PairingTrustGrant, WireCodec,
};

#[test]
fn interrupt_aborts_inflight_tls_and_joins_connect_worker() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let temp = tempfile::tempdir().unwrap();
    let mut config = SessionConfig::direct("127.0.0.1", "client");
    config.tcp_port = listener.local_addr().unwrap().port();
    config.pairing_store_path = Some(temp.path().join("pairs.json"));
    let session = ClientSession::new(config).unwrap();
    let interrupt = session.interrupt_handle();
    let (done_tx, done_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        done_tx
            .send(session.connect_with_pairing(PairingRecord::new("id", "fixture", vec![1; 32], 0)))
            .unwrap();
    });
    let (mut socket, _) = listener.accept().unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut hello = [0; 1];
    std::io::Read::read_exact(&mut socket, &mut hello).unwrap();
    interrupt.interrupt();
    assert!(matches!(
        done_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        Err(ferryx_rd_app::SessionError::Cancelled)
    ));
    worker.join().unwrap();
}

fn packet(kind: PacketType, payload: &[u8]) -> Vec<u8> {
    let mut result = PacketHeader::new(kind, 0, 0, 0).encode().unwrap();
    result.extend_from_slice(payload);
    result
}

fn ack(stream: &mut ferryx_rd_net::TlsPskStream<std::net::TcpStream>, capabilities: Capabilities) {
    let request = stream.read_frame().unwrap();
    let handshake = Handshake::decode(&request[PacketHeader::SIZE..]).unwrap();
    let response = Handshake {
        name: "fixture".into(),
        width: 1,
        height: 1,
        scale: 1.0,
        version: 3,
        capabilities,
        pairing_id: String::new(),
        session_salt: handshake.session_salt,
    };
    stream
        .write_frame(&packet(
            PacketType::HandshakeAck,
            &response.encode().unwrap(),
        ))
        .unwrap();
}

#[tokio::test]
async fn stored_legacy_pairing_connects_and_receives_udp_inside_runtime() {
    let key = [17; 32];
    let listener = TlsPskServer::new([PskIdentity::pairing("stored-id", &key).unwrap()])
        .unwrap()
        .bind("127.0.0.1:0")
        .unwrap();
    let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
    udp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let mut config = SessionConfig::direct("127.0.0.1", "client");
    config.tcp_port = listener.local_addr().unwrap().port();
    config.udp_port = udp.local_addr().unwrap().port();
    config.pairing_store_path = Some(temp.path().join("pairs.json"));
    let record = PairingRecord::new("stored-id", "fixture", key.to_vec(), 0);
    PairingStore::new(config.pairing_store_path.as_ref().unwrap())
        .save(record.clone())
        .unwrap();
    let (end_tx, end_rx) = mpsc::channel();
    let host = thread::spawn(move || {
        let mut stream = listener.accept().unwrap();
        let request = stream.read_frame().unwrap();
        let handshake = Handshake::decode(&request[PacketHeader::SIZE..]).unwrap();
        let response = Handshake {
            capabilities: Capabilities::AUTHENTICATED_UDP_REGISTRATION,
            ..handshake.clone()
        };
        stream
            .write_frame(&packet(
                PacketType::HandshakeAck,
                &response.encode().unwrap(),
            ))
            .unwrap();
        let mut registration = [0; 1024];
        let (_, peer) = udp.recv_from(&mut registration).unwrap();
        let mut cipher = ferryx_rd_net::DatagramCipher::derive(
            &key,
            &handshake.session_salt,
            ferryx_rd_net::Direction::HostToClient,
        )
        .unwrap();
        let ping = cipher
            .seal_datagram(&PacketHeader::new(PacketType::Ping, 1, 0, 0), &[])
            .unwrap();
        udp.send_to(&ping, peer).unwrap();
        end_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    });
    let session = ClientSession::new(config).unwrap();
    let ready = session.reconnect("stored-id").unwrap();
    assert_eq!(ready.pairing, record);
    assert_eq!(session.receive_udp_event().unwrap(), SessionEvent::Ping);
    session.disconnect().unwrap();
    end_tx.send(()).unwrap();
    host.join().unwrap();
}

#[test]
fn passive_session_reconnects_after_transport_loss() {
    let key = [7; 32];
    let listener = TlsPskServer::new([PskIdentity::pairing("id", &key).unwrap()])
        .unwrap()
        .bind("127.0.0.1:0")
        .unwrap();
    let address = listener.local_addr().unwrap();
    let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
    let (connected_tx, connected_rx) = mpsc::channel();
    let (end_tx, end_rx) = mpsc::channel();
    let host = thread::spawn(move || {
        let mut first = listener.accept().unwrap();
        ack(&mut first, Capabilities::AUTHENTICATED_UDP_REGISTRATION);
        drop(first);
        let mut second = listener.accept().unwrap();
        ack(
            &mut second,
            Capabilities::AUTHENTICATED_UDP_REGISTRATION | Capabilities::HOST_STATUS,
        );
        second
            .write_frame(&packet(
                PacketType::Control,
                &ControlMessage::HostStatus(ferryx_rd_proto::HostStatus {
                    guest_names: vec!["reconnected".into()],
                })
                .encode()
                .unwrap(),
            ))
            .unwrap();
        connected_tx.send(()).unwrap();
        end_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    });
    let temp = tempfile::tempdir().unwrap();
    let mut config = SessionConfig::direct("127.0.0.1", "client");
    config.tcp_port = address.port();
    config.udp_port = udp.local_addr().unwrap().port();
    config.pairing_store_path = Some(temp.path().join("pairs.json"));
    let session = ClientSession::new(config).unwrap();
    session
        .connect_with_pairing(PairingRecord::new("id", "fixture", key.to_vec(), 0))
        .unwrap();
    let mut runtime = session.spawn_tcp_runtime().unwrap();
    connected_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let event = runtime
        .events()
        .recv_timeout(Duration::from_secs(10))
        .unwrap()
        .unwrap();
    assert!(matches!(
        event,
        SessionEvent::StreamConfig(ControlMessage::HostStatus(_))
    ));
    assert_eq!(
        session.session_info().unwrap().reconnect,
        ReconnectState::Reconnected
    );
    runtime.stop().unwrap();
    end_tx.send(()).unwrap();
    host.join().unwrap();
}

#[test]
fn terminal_session_reason_survives_eof_and_prevents_reconnect() {
    let key = [8; 32];
    let listener = TlsPskServer::new([PskIdentity::pairing("id", &key).unwrap()])
        .unwrap()
        .bind("127.0.0.1:0")
        .unwrap();
    let address = listener.local_addr().unwrap();
    let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
    let host = thread::spawn(move || {
        let mut stream = listener.accept().unwrap();
        ack(
            &mut stream,
            Capabilities::AUTHENTICATED_UDP_REGISTRATION | Capabilities::SESSION_ENDED,
        );
        stream
            .write_frame(&packet(
                PacketType::Control,
                &ControlMessage::SessionEnded(ferryx_rd_proto::SessionEnded {
                    reason: ferryx_rd_proto::SessionEndReason::Kicked,
                })
                .encode()
                .unwrap(),
            ))
            .unwrap();
    });
    let temp = tempfile::tempdir().unwrap();
    let mut config = SessionConfig::direct("127.0.0.1", "client");
    config.tcp_port = address.port();
    config.udp_port = udp.local_addr().unwrap().port();
    config.pairing_store_path = Some(temp.path().join("pairs.json"));
    let session = ClientSession::new(config).unwrap();
    session
        .connect_with_pairing(PairingRecord::new("id", "fixture", key.to_vec(), 0))
        .unwrap();
    let mut runtime = session.spawn_tcp_runtime().unwrap();
    let event = runtime
        .events()
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    assert!(matches!(
        event,
        SessionEvent::StreamConfig(ControlMessage::SessionEnded(_))
    ));
    assert_eq!(
        session.session_info().unwrap().ended,
        Some(ferryx_rd_proto::SessionEndReason::Kicked)
    );
    assert_eq!(
        session.session_info().unwrap().reconnect,
        ReconnectState::Idle
    );
    runtime.stop().unwrap();
    host.join().unwrap();
}

#[test]
fn stopping_runtime_keeps_session_reconnectable() {
    // Worker-respawn recovery stops the TCP runtime and then reconnects the
    // same ClientSession; stopping must not leave the session cancelled.
    let key = [11; 32];
    let listener = TlsPskServer::new([PskIdentity::pairing("id", &key).unwrap()])
        .unwrap()
        .bind("127.0.0.1:0")
        .unwrap();
    let address = listener.local_addr().unwrap();
    let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
    let (end_tx, end_rx) = mpsc::channel::<()>();
    let host = thread::spawn(move || {
        let mut first = listener.accept().unwrap();
        ack(&mut first, Capabilities::AUTHENTICATED_UDP_REGISTRATION);
        let mut second = listener.accept().unwrap();
        ack(&mut second, Capabilities::AUTHENTICATED_UDP_REGISTRATION);
        end_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        drop(first);
    });
    let temp = tempfile::tempdir().unwrap();
    let mut config = SessionConfig::direct("127.0.0.1", "client");
    config.tcp_port = address.port();
    config.udp_port = udp.local_addr().unwrap().port();
    config.pairing_store_path = Some(temp.path().join("pairs.json"));
    let session = ClientSession::new(config).unwrap();
    let record = PairingRecord::new("id", "fixture", key.to_vec(), 0);
    session.connect_with_pairing(record.clone()).unwrap();
    let mut runtime = session.spawn_tcp_runtime().unwrap();
    runtime.stop().unwrap();
    assert!(!session.is_cancelled());
    session.connect_with_pairing(record).unwrap();
    session.disconnect().unwrap();
    end_tx.send(()).unwrap();
    host.join().unwrap();
}

#[test]
fn trust_reconnect_does_not_overwrite_stored_pairing_fields() {
    let key = [12; 32];
    let listener = TlsPskServer::new([PskIdentity::pairing("id", &key).unwrap()])
        .unwrap()
        .bind("127.0.0.1:0")
        .unwrap();
    let address = listener.local_addr().unwrap();
    let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
    let (end_tx, end_rx) = mpsc::channel::<()>();
    let host = thread::spawn(move || {
        let mut stream = listener.accept().unwrap();
        ack(
            &mut stream,
            Capabilities::AUTHENTICATED_UDP_REGISTRATION | Capabilities::PAIRING_TRUST,
        );
        stream
            .write_frame(&packet(
                PacketType::Control,
                &ControlMessage::PairingTrustGrant(PairingTrustGrant {
                    trust: PairingTrust::Owner,
                })
                .encode()
                .unwrap(),
            ))
            .unwrap();
        end_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    });
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("pairs.json");
    let store = PairingStore::new(path.clone());
    let stale = PairingRecord::new("id", "fixture", key.to_vec(), 0);
    let mut stored = stale.clone();
    stored.relay_url = Some("wss://relay.example".into());
    store.save(stored.clone()).unwrap();
    let mut config = SessionConfig::direct("127.0.0.1", "client");
    config.tcp_port = address.port();
    config.udp_port = udp.local_addr().unwrap().port();
    config.pairing_store_path = Some(path);
    let session = ClientSession::new(config).unwrap();
    session.connect_with_pairing(stale).unwrap();
    assert_eq!(store.load("id").unwrap(), Some(stored));
    session.disconnect().unwrap();
    end_tx.send(()).unwrap();
    host.join().unwrap();
}

fn bootstrap_grant(trust: PairingTrust) -> (PairingRecord, Vec<PairingRecord>) {
    let listener = TlsPskServer::new([PskIdentity::bootstrap("12345678").unwrap()])
        .unwrap()
        .bind("127.0.0.1:0")
        .unwrap();
    let address = listener.local_addr().unwrap();
    let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("pairs.json");
    let inspect_path = path.clone();
    let host = thread::spawn(move || {
        let mut stream = listener.accept().unwrap();
        stream.read_frame().unwrap();
        stream
            .write_frame(&packet(
                PacketType::PairingGrant,
                &PairingGrant {
                    pairing_id: "id".into(),
                    host_name: "fixture".into(),
                    key: [9; 32],
                }
                .encode()
                .unwrap(),
            ))
            .unwrap();
        ack(
            &mut stream,
            Capabilities::AUTHENTICATED_UDP_REGISTRATION | Capabilities::PAIRING_TRUST,
        );
        let request = stream.read_frame().unwrap();
        assert_eq!(
            ControlMessage::decode(&request[PacketHeader::SIZE..]).unwrap(),
            ControlMessage::PairingTrustRequest(ferryx_rd_proto::PairingTrustRequest {
                trust: PairingTrust::Owner
            })
        );
        assert!(PairingStore::new(inspect_path)
            .load_all()
            .unwrap()
            .is_empty());
        stream
            .write_frame(&packet(
                PacketType::Control,
                &ControlMessage::PairingTrustGrant(PairingTrustGrant { trust })
                    .encode()
                    .unwrap(),
            ))
            .unwrap();
    });
    let mut config = SessionConfig::direct("127.0.0.1", "client");
    config.tcp_port = address.port();
    config.udp_port = udp.local_addr().unwrap().port();
    config.pairing_store_path = Some(path.clone());
    let session = ClientSession::new(config).unwrap();
    let ready = session.pair_with_pin("12345678").unwrap();
    host.join().unwrap();
    (ready.pairing, PairingStore::new(path).load_all().unwrap())
}

#[test]
fn guest_once_grant_never_persists_provisional_key() {
    let (record, saved) = bootstrap_grant(PairingTrust::GuestOnce);
    assert_eq!(record.trust, PairingTrust::GuestOnce);
    assert!(saved.is_empty());
}

#[test]
fn owner_grant_persists_only_after_authorization() {
    let (record, saved) = bootstrap_grant(PairingTrust::Owner);
    assert_eq!(saved, vec![record]);
}
