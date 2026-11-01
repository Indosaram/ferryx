use ferryx_rd_app::{ClientSession, PairingRecord, SessionConfig, SessionEvent};
use ferryx_rd_net::{DatagramCipher, Direction, PskIdentity, TlsPskServer, TlsPskStream};
use ferryx_rd_proto::{
    Capabilities, FrameChunk, FrameHeader, FrameNack, Handshake, PacketHeader, PacketType,
    WireCodec,
};
use std::{
    net::{TcpStream, UdpSocket},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

struct Peer {
    session: ClientSession,
    udp: UdpSocket,
    addr: std::net::SocketAddr,
    send: DatagramCipher,
    receive: DatagramCipher,
    _stream: TlsPskStream<TcpStream>,
    _dir: tempfile::TempDir,
}
impl Peer {
    fn new(fec: bool) -> Self {
        let key = [39; 32];
        let listener = TlsPskServer::new([PskIdentity::pairing("recovery", &key).unwrap()])
            .unwrap()
            .bind("127.0.0.1:0")
            .unwrap();
        let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
        udp.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut config = SessionConfig::direct("127.0.0.1", "recovery");
        config.tcp_port = listener.local_addr().unwrap().port();
        config.udp_port = udp.local_addr().unwrap().port();
        config.pairing_store_path = Some(dir.path().join("pairings.json"));
        let (tx, rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            let mut stream = listener.accept().unwrap();
            let bytes = stream.read_frame().unwrap();
            let mut ack = Handshake::decode(&bytes[PacketHeader::SIZE..]).unwrap();
            ack.capabilities = Capabilities::AUTHENTICATED_UDP_REGISTRATION;
            if fec {
                ack.capabilities |= Capabilities::FEC_NACK;
            }
            let mut packet = PacketHeader::new(PacketType::HandshakeAck, 0, 0, 0)
                .encode()
                .unwrap();
            packet.extend_from_slice(&ack.encode().unwrap());
            stream.write_frame(&packet).unwrap();
            let mut receive =
                DatagramCipher::derive(&key, &ack.session_salt, Direction::ClientToHost).unwrap();
            let mut buffer = [0; 2048];
            let (len, addr) = udp.recv_from(&mut buffer).unwrap();
            assert_eq!(
                receive.open_datagram(&buffer[..len]).unwrap().0.packet_type,
                PacketType::Ping
            );
            tx.send((udp, addr, ack.session_salt, receive, stream))
                .unwrap();
        });
        let session = ClientSession::new(config).unwrap();
        session
            .connect_with_pairing(PairingRecord {
                id: "recovery".into(),
                name: "test".into(),
                key: key.to_vec(),
                added_at_unix_ms: 0,
                last_endpoint: None,
                endpoint_aliases: vec![],
                relay_url: None,
                relay_host_id: None,
                trust: ferryx_rd_proto::PairingTrust::Owner,
            })
            .unwrap();
        let (udp, addr, salt, receive, stream) = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        worker.join().unwrap();
        Self {
            session,
            udp,
            addr,
            send: DatagramCipher::derive(&key, &salt, Direction::HostToClient).unwrap(),
            receive,
            _stream: stream,
            _dir: dir,
        }
    }
    fn ingress(
        &mut self,
        kind: PacketType,
        seq: u32,
        payload: &[u8],
        now: Instant,
    ) -> SessionEvent {
        let packet = self
            .send
            .seal_datagram(&PacketHeader::new(kind, seq, 42, 0), payload)
            .unwrap();
        self.udp.send_to(&packet, self.addr).unwrap();
        self.session.receive_udp_event_at(now).unwrap()
    }
    fn feedback(&mut self) -> FrameNack {
        let mut buffer = [0; 2048];
        let (len, _) = self.udp.recv_from(&mut buffer).unwrap();
        let (header, payload) = self.receive.open_datagram(&buffer[..len]).unwrap();
        assert_eq!(header.packet_type, PacketType::FrameNack);
        FrameNack::decode(&payload).unwrap()
    }
}
fn header() -> FrameHeader {
    FrameHeader {
        frame_id: 1,
        width: 2,
        height: 2,
        is_key_frame: true,
        total_chunks: 3,
        total_size: 7,
    }
}

#[test]
fn negotiated_client_repairs_parity_and_suppresses_nack_without_rtt() {
    let mut peer = Peer::new(true);
    let now = Instant::now();
    assert_eq!(
        peer.ingress(PacketType::FrameHeader, 1, &header().encode().unwrap(), now),
        SessionEvent::Ignored
    );
    assert!(peer.feedback().missing.is_empty());
    let data = [vec![1, 2, 3], vec![4, 5, 6], vec![7]];
    for i in [0usize, 2] {
        assert_eq!(
            peer.ingress(
                PacketType::FrameChunk,
                2 + i as u32,
                &FrameChunk {
                    frame_id: 1,
                    chunk_index: i as u16,
                    data: data[i].clone()
                }
                .encode()
                .unwrap(),
                now
            ),
            SessionEvent::Ignored
        );
    }
    // Explicit receive clock crosses reorder grace, no wall-clock sleeps.
    peer.ingress(PacketType::Ping, 5, &[], now + Duration::from_millis(4));
    // Unknown RTT cannot justify a retransmission inside the frame deadline.
    // Synchronous ingress is complete, so absence needs no timer or sleep.
    peer.udp.set_nonblocking(true).unwrap();
    assert_eq!(
        peer.udp.recv_from(&mut [0; 2048]).unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    peer.udp.set_nonblocking(false).unwrap();
    let parity = ferryx_rd_proto::build_parity(1, &data, 1).remove(0);
    let event = peer.ingress(
        PacketType::FrameParity,
        6,
        &parity.encode().unwrap(),
        now + Duration::from_millis(5),
    );
    let SessionEvent::Frame(frame) = event else {
        panic!("expected repaired frame");
    };
    assert_eq!(frame.data, [1, 2, 3, 4, 5, 6, 7]);
    peer.ingress(PacketType::Ping, 7, &[], now + Duration::from_millis(205));
    assert_eq!(
        peer.feedback().loss_permille,
        333,
        "recovered loss feeds adaptive parity"
    );
    peer.session.disconnect().unwrap();
}

#[test]
fn old_host_does_not_negotiate_parity_or_receive_any_feedback() {
    let mut peer = Peer::new(false);
    let now = Instant::now();
    peer.ingress(PacketType::FrameHeader, 1, &header().encode().unwrap(), now);
    let parity = ferryx_rd_proto::build_parity(1, &[vec![1, 2, 3], vec![4, 5, 6], vec![7]], 1).remove(0);
    assert_eq!(
        peer.ingress(
            PacketType::FrameParity,
            2,
            &parity.encode().unwrap(),
            now + Duration::from_millis(10)
        ),
        SessionEvent::Ignored
    );
    peer.ingress(PacketType::Ping, 3, &[], now + Duration::from_millis(250));
    // Synchronous ingress is done, so nonblocking absence is deterministic.
    peer.udp.set_nonblocking(true).unwrap();
    assert_eq!(
        peer.udp.recv_from(&mut [0; 2048]).unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    peer.session.disconnect().unwrap();
}
