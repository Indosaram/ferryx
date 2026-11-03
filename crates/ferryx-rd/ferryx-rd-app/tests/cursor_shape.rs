use std::{
    net::{SocketAddr, TcpStream, UdpSocket},
    sync::mpsc,
    thread,
    time::Duration,
};

use ferryx_rd_app::{
    ClientSession, CursorShapeCache, PairingRecord, SessionConfig, SessionEvent, MAX_CURSOR_SHAPES,
};
use ferryx_rd_net::{DatagramCipher, Direction, PskIdentity, TlsPskServer, TlsPskStream};
use ferryx_rd_proto::{
    Capabilities, ControlMessage, CursorShape, CursorUpdate, Handshake, PacketHeader, PacketType,
    WireCodec,
};

fn shape(shape_id: u32) -> CursorShape {
    CursorShape {
        shape_id,
        width: 2,
        height: 2,
        hotspot_x: 1,
        hotspot_y: 0,
        rgba: vec![shape_id as u8; 16],
    }
}

#[test]
fn cache_replaces_by_id_and_evicts_oldest_at_bound() {
    let mut cache = CursorShapeCache::default();
    for id in 1..=MAX_CURSOR_SHAPES as u32 {
        cache.insert(shape(id));
    }
    assert_eq!(cache.len(), MAX_CURSOR_SHAPES);
    // Re-announcing an id replaces it and refreshes its age.
    let mut replaced = shape(1);
    replaced.hotspot_x = 0;
    cache.insert(replaced.clone());
    assert_eq!(cache.len(), MAX_CURSOR_SHAPES);
    cache.insert(shape(1000));
    assert_eq!(cache.len(), MAX_CURSOR_SHAPES);
    assert_eq!(cache.get(1), Some(&replaced));
    assert!(cache.get(2).is_none(), "oldest untouched id is evicted");
    assert!(cache.get(1000).is_some());
    cache.clear();
    assert!(cache.is_empty());
}

struct Fixture {
    session: ClientSession,
    udp: UdpSocket,
    peer: SocketAddr,
    cipher: DatagramCipher,
    stream: TlsPskStream<TcpStream>,
    _temporary: tempfile::TempDir,
}

impl Fixture {
    fn new(host_capabilities: Capabilities) -> Self {
        let key = [0x51; 32];
        let listener = TlsPskServer::new([PskIdentity::pairing("cursor", &key).unwrap()])
            .unwrap()
            .bind("127.0.0.1:0")
            .unwrap();
        let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
        udp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let temporary = tempfile::tempdir().unwrap();
        let mut config = SessionConfig::direct("127.0.0.1", "cursor");
        assert!(config.capabilities.contains(Capabilities::CURSOR_SHAPE));
        config.tcp_port = listener.local_addr().unwrap().port();
        config.udp_port = udp.local_addr().unwrap().port();
        config.connect_timeout = Duration::from_secs(5);
        config.handshake_ack_timeout = Duration::from_secs(5);
        config.pairing_store_path = Some(temporary.path().join("pairings.json"));
        let (ready_tx, ready_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            let mut stream = listener.accept().unwrap();
            stream
                .ssl_stream()
                .get_ref()
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let frame = stream.read_frame().unwrap();
            let mut handshake = Handshake::decode(&frame[PacketHeader::SIZE..]).unwrap();
            handshake.capabilities =
                Capabilities::AUTHENTICATED_UDP_REGISTRATION | host_capabilities;
            let mut ack = PacketHeader::new(PacketType::HandshakeAck, 0, 0, 0)
                .encode()
                .unwrap();
            ack.extend_from_slice(&handshake.encode().unwrap());
            stream.write_frame(&ack).unwrap();
            let mut probe = [0; 1024];
            let (_, peer) = udp.recv_from(&mut probe).unwrap();
            ready_tx
                .send((udp, peer, handshake.session_salt, stream))
                .unwrap();
        });
        let session = ClientSession::new(config).unwrap();
        session
            .connect_with_pairing(PairingRecord {
                id: "cursor".into(),
                name: "fixture".into(),
                key: key.to_vec(),
                added_at_unix_ms: 0,
                last_endpoint: None,
                endpoint_aliases: Vec::new(),
                relay_url: None,
                relay_host_id: None,
                trust: ferryx_rd_proto::PairingTrust::Owner,
            })
            .unwrap();
        session
            .set_udp_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let (udp, peer, salt, stream) = ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        worker.join().unwrap();
        Self {
            session,
            udp,
            peer,
            cipher: DatagramCipher::derive(&key, &salt, Direction::HostToClient).unwrap(),
            stream,
            _temporary: temporary,
        }
    }

    fn send_control(&mut self, message: &ControlMessage) {
        let mut frame = PacketHeader::new(PacketType::Control, 1, 0, 0)
            .encode()
            .unwrap();
        frame.extend_from_slice(&message.encode().unwrap());
        self.stream.write_frame(&frame).unwrap();
    }

    fn cursor(&mut self, sequence: u32, payload: &[u8]) -> SessionEvent {
        let datagram = self
            .cipher
            .seal_datagram(
                &PacketHeader::new(PacketType::CursorUpdate, sequence, 42, 0),
                payload,
            )
            .unwrap();
        self.udp.send_to(&datagram, self.peer).unwrap();
        self.session.receive_udp_event().unwrap()
    }
}

const POSITION: CursorUpdate = CursorUpdate {
    x: 0.25,
    y: 0.75,
    cursor_type: 1,
};

#[test]
fn negotiated_host_shape_arrives_on_tcp_and_update_references_it() {
    let mut fixture = Fixture::new(Capabilities::CURSOR_SHAPE);
    fixture.send_control(&ControlMessage::CursorShape(shape(7)));
    assert_eq!(
        fixture.session.receive_tcp_event().unwrap(),
        SessionEvent::Ignored
    );
    assert_eq!(fixture.session.cursor_shape(7).unwrap(), Some(shape(7)));
    // Even an authenticated UDP control packet must not populate images.
    let bytes = fixture
        .cipher
        .seal_datagram(
            &PacketHeader::new(PacketType::Control, 9, 42, 0),
            &ControlMessage::CursorShape(shape(8)).encode().unwrap(),
        )
        .unwrap();
    fixture.udp.send_to(&bytes, fixture.peer).unwrap();
    assert!(fixture.session.receive_udp_event().is_err());
    assert!(fixture.session.cursor_shape(8).unwrap().is_none());
    let payload = POSITION.encode_with_shape(Some(7)).unwrap();
    match fixture.cursor(10, &payload) {
        SessionEvent::Cursor(cursor) => {
            assert_eq!((cursor.x, cursor.y, cursor.shape_id), (0.25, 0.75, 7));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn old_peer_keeps_legacy_cursor_and_rejects_unnegotiated_shape() {
    let mut fixture = Fixture::new(Capabilities::empty());
    match fixture.cursor(10, &POSITION.encode_with_shape(None).unwrap()) {
        SessionEvent::Cursor(cursor) => {
            assert_eq!(
                (cursor.x, cursor.cursor_type, cursor.shape_id),
                (0.25, 1, 0)
            );
        }
        other => panic!("unexpected {other:?}"),
    }
    fixture.send_control(&ControlMessage::CursorShape(shape(7)));
    assert!(fixture.session.receive_tcp_event().is_err());
    assert_eq!(fixture.session.cursor_shape(7).unwrap(), None);
}
