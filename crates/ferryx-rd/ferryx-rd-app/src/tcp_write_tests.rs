use super::*;
use ferryx_rd_net::TlsPskServer;

const WATCHDOG: Duration = Duration::from_secs(5);
const PRESSURE_FRAMES: usize = 32;

struct Fixture {
    session: ClientSession,
    peer: TlsPskStream<std::net::TcpStream>,
    _directory: tempfile::TempDir,
    _udp: UdpSocket,
}

fn packet(kind: PacketType, payload: &[u8]) -> Vec<u8> {
    let mut bytes = PacketHeader::new(kind, 0, 0, 0).encode().unwrap();
    bytes.extend_from_slice(payload);
    bytes
}

fn fixture() -> Fixture {
    fixture_with_capabilities(Capabilities::AUTHENTICATED_UDP_REGISTRATION)
}

fn fixture_with_capabilities(capabilities: Capabilities) -> Fixture {
    let psk = PskIdentity::pairing("runtime-write", &[19; 32]).unwrap();
    let server = TlsPskServer::new([psk]).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
    udp.set_read_timeout(Some(WATCHDOG)).unwrap();
    let server_udp = udp.try_clone().unwrap();
    let (peer_tx, peer_rx) = mpsc::channel();
    let host = thread::spawn(move || {
        let (socket, _) = listener.accept().unwrap();
        // Explicit kernel bounds, not random filler or elapsed time: 128KiB of
        // framed data cannot fit in the deliberately paused peer's capacity.
        rustix::net::sockopt::set_socket_recv_buffer_size(&socket, 4096).unwrap();
        socket.set_read_timeout(Some(WATCHDOG)).unwrap();
        socket.set_write_timeout(Some(WATCHDOG)).unwrap();
        let mut peer = server.accept_stream(socket).unwrap();
        assert_eq!(
            peer.negotiated_identity(),
            Some(b"maho-p1.runtime-write".as_slice())
        );
        let hello = peer.read_frame().unwrap();
        assert_eq!(
            PacketHeader::decode(&hello[..PacketHeader::SIZE])
                .unwrap()
                .packet_type,
            PacketType::Handshake
        );
        let mut hello = Handshake::decode(&hello[PacketHeader::SIZE..]).unwrap();
        hello.capabilities = capabilities;
        peer.write_frame(&packet(PacketType::HandshakeAck, &hello.encode().unwrap()))
            .unwrap();
        let mut wake = [0; 1024];
        let (wake_len, _) = server_udp.recv_from(&mut wake).unwrap();
        let mut c2h =
            DatagramCipher::derive(&[19; 32], &hello.session_salt, Direction::ClientToHost)
                .unwrap();
        let (wake_hdr, _) = c2h.open_datagram(&wake[..wake_len]).unwrap();
        assert_eq!(wake_hdr.packet_type, PacketType::Ping);
        peer_tx.send(peer).unwrap();
    });
    let directory = tempfile::tempdir().unwrap();
    let mut config = SessionConfig::direct("127.0.0.1", "write-test");
    config.tcp_port = address.port();
    config.udp_port = udp.local_addr().unwrap().port();
    config.connect_timeout = WATCHDOG;
    config.handshake_ack_timeout = WATCHDOG;
    config.pairing_store_path = Some(directory.path().join("pairings.json"));
    let session = ClientSession::new(config).unwrap();
    session
        .connect_with_pairing(PairingRecord {
            id: "runtime-write".into(),
            name: "peer".into(),
            key: vec![19; 32],
            added_at_unix_ms: 0,
            last_endpoint: None,
            endpoint_aliases: Vec::new(),
            relay_url: None,
            relay_host_id: None,
            trust: ferryx_rd_proto::PairingTrust::Owner,
        })
        .unwrap();
    let peer = peer_rx.recv_timeout(WATCHDOG).unwrap();
    host.join().unwrap();
    rustix::net::sockopt::set_socket_send_buffer_size(
        session
            .tcp
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .ssl_stream()
            .get_ref(),
        4096,
    )
    .unwrap();
    Fixture {
        session,
        peer,
        _directory: directory,
        _udp: udp,
    }
}

#[test]
fn negotiated_motion_uses_gcm_and_discrete_carries_reliable_barrier() {
    let Fixture {
        session,
        mut peer,
        _directory,
        _udp,
    } = fixture_with_capabilities(
        Capabilities::AUTHENTICATED_UDP_REGISTRATION | Capabilities::UDP_INPUT,
    );
    let motion = input(ferryx_rd_proto::InputEventType::MouseMove);
    session.send_input(motion).unwrap();
    let mut buffer = [0; 2048];
    let (length, _) = _udp.recv_from(&mut buffer).unwrap();
    let salt = session.state.lock().unwrap().session_salt.unwrap();
    let mut cipher = DatagramCipher::derive(&[19; 32], &salt, Direction::ClientToHost).unwrap();
    let (header, payload) = cipher.open_datagram(&buffer[..length]).unwrap();
    assert_eq!(header.packet_type, PacketType::InputBatch);
    let batch = ferryx_rd_proto::InputBatch::decode(&payload).unwrap();
    assert_eq!(batch.samples, vec![motion]);
    let key = input(ferryx_rd_proto::InputEventType::KeyDown);
    session.send_input(key).unwrap();
    let frame = peer.read_frame().unwrap();
    assert_eq!(
        PacketHeader::decode(&frame[..PacketHeader::SIZE])
            .unwrap()
            .packet_type,
        PacketType::InputEvent
    );
    let payload = &frame[PacketHeader::SIZE..];
    assert_eq!(
        InputEvent::decode(&payload[..InputEvent::SIZE]).unwrap(),
        key
    );
    assert_eq!(
        ferryx_rd_proto::InputBatch::decode(&payload[InputEvent::SIZE..]).unwrap(),
        batch
    );
}

#[test]
fn coalesced_samples_share_one_input_datagram() {
    let Fixture {
        session,
        peer: _peer,
        _directory,
        _udp,
    } = fixture_with_capabilities(
        Capabilities::AUTHENTICATED_UDP_REGISTRATION | Capabilities::UDP_INPUT,
    );
    let events: Vec<_> = (0..16)
        .map(|i| {
            let mut e = input(ferryx_rd_proto::InputEventType::RelativeMove);
            e.scroll_dx = i as f32;
            e
        })
        .collect();
    session.send_inputs(&events).unwrap();
    let mut buffer = [0; 2048];
    let (length, _) = _udp.recv_from(&mut buffer).unwrap();
    let salt = session.state.lock().unwrap().session_salt.unwrap();
    let mut cipher = DatagramCipher::derive(&[19; 32], &salt, Direction::ClientToHost).unwrap();
    let (header, payload) = cipher.open_datagram(&buffer[..length]).unwrap();
    assert_eq!(header.packet_type, PacketType::InputBatch);
    assert_eq!(
        ferryx_rd_proto::InputBatch::decode(&payload).unwrap().samples,
        events
    );
    _udp.set_nonblocking(true).unwrap();
    assert_eq!(
        _udp.recv_from(&mut buffer).unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
}

#[test]
fn oversized_coalesced_group_is_split_at_mtu_with_reliable_checkpoint() {
    let Fixture {
        session,
        mut peer,
        _directory,
        _udp,
    } = fixture_with_capabilities(
        Capabilities::AUTHENTICATED_UDP_REGISTRATION | Capabilities::UDP_INPUT,
    );
    let events =
        vec![input(ferryx_rd_proto::InputEventType::MouseMove); ferryx_rd_proto::MAX_INPUT_SAMPLES + 1];
    session.send_inputs(&events).unwrap();
    let mut buffer = [0; 2048];
    let salt = session.state.lock().unwrap().session_salt.unwrap();
    let mut cipher = DatagramCipher::derive(&[19; 32], &salt, Direction::ClientToHost).unwrap();
    for expected in [ferryx_rd_proto::MAX_INPUT_SAMPLES, 1] {
        let (length, _) = _udp.recv_from(&mut buffer).unwrap();
        assert!(length <= 1200);
        let (_, payload) = cipher.open_datagram(&buffer[..length]).unwrap();
        assert_eq!(
            ferryx_rd_proto::InputBatch::decode(&payload)
                .unwrap()
                .samples
                .len(),
            expected
        );
    }
    let frame = peer.read_frame().unwrap();
    assert_eq!(
        PacketHeader::decode(&frame[..PacketHeader::SIZE])
            .unwrap()
            .packet_type,
        PacketType::InputBatch
    );
    assert_eq!(
        ferryx_rd_proto::InputBatch::decode(&frame[PacketHeader::SIZE..])
            .unwrap()
            .samples
            .len(),
        ferryx_rd_proto::MAX_INPUT_SAMPLES
    );
}

#[test]
fn old_host_batch_falls_back_to_ordered_individual_tcp_events() {
    let Fixture {
        session,
        mut peer,
        _directory,
        _udp,
    } = fixture();
    let events = [
        input(ferryx_rd_proto::InputEventType::MouseMove),
        input(ferryx_rd_proto::InputEventType::KeyDown),
        input(ferryx_rd_proto::InputEventType::RelativeMove),
    ];
    session.send_inputs(&events).unwrap();
    for expected in events {
        let frame = peer.read_frame().unwrap();
        assert_eq!(
            PacketHeader::decode(&frame[..PacketHeader::SIZE])
                .unwrap()
                .packet_type,
            PacketType::InputEvent
        );
        assert_eq!(
            InputEvent::decode(&frame[PacketHeader::SIZE..]).unwrap(),
            expected
        );
    }
}

#[test]
fn old_host_keeps_motion_on_unchanged_tcp_wire() {
    let Fixture {
        session,
        mut peer,
        _directory,
        _udp,
    } = fixture();
    let event = input(ferryx_rd_proto::InputEventType::MouseMove);
    session.send_input(event).unwrap();
    let frame = peer.read_frame().unwrap();
    assert_eq!(
        PacketHeader::decode(&frame[..PacketHeader::SIZE])
            .unwrap()
            .packet_type,
        PacketType::InputEvent
    );
    assert_eq!(&frame[PacketHeader::SIZE..], event.encode().unwrap());
}

fn text(i: usize) -> String {
    format!("{i:04}{}", "x".repeat(4092))
}

fn pressurize(session: &ClientSession) {
    let (blocked_tx, blocked_rx) = mpsc::channel();
    *session.tcp_write_wait.lock().unwrap() = Some(blocked_tx);
    for i in 0..PRESSURE_FRAMES {
        session.send_clipboard_text(text(i)).unwrap();
    }
    blocked_rx
        .recv_timeout(WATCHDOG)
        .expect("actual SSL_write must reach kernel WouldBlock");
    assert!(session
        .tcp
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .has_pending_frames());
}

fn input(kind: ferryx_rd_proto::InputEventType) -> InputEvent {
    InputEvent {
        event_type: kind,
        x: 0.25,
        y: 0.75,
        key_code: 0,
        modifiers: ferryx_rd_proto::Modifiers::empty(),
        scroll_dx: 0.0,
        scroll_dy: 0.0,
    }
}

#[test]
fn runtime_pressure_drains_ordered_controls_input_and_generated_pong() {
    let Fixture {
        session,
        mut peer,
        _directory,
        _udp,
    } = fixture();
    let mut runtime = session.spawn_tcp_runtime().unwrap();
    assert!(matches!(
        session.spawn_tcp_runtime(),
        Err(SessionError::TcpRuntimeAlreadyRunning)
    ));
    pressurize(&session);
    let down = input(ferryx_rd_proto::InputEventType::LeftMouseDown);
    let up = input(ferryx_rd_proto::InputEventType::LeftMouseUp);
    session.send_input(down).unwrap();
    session.send_input(up).unwrap();
    // Callback must admit Pong without blocking on writable, before peer reads.
    peer.write_frame(&packet(PacketType::Ping, &[])).unwrap();
    assert_eq!(
        runtime.events().recv_timeout(WATCHDOG).unwrap().unwrap(),
        SessionEvent::Ping
    );
    let (done_tx, done_rx) = mpsc::channel();
    let host = thread::spawn(move || {
        let mut frames = Vec::new();
        for _ in 0..PRESSURE_FRAMES + 3 {
            frames.push(peer.read_frame().unwrap());
        }
        done_tx.send((peer, frames)).unwrap();
    });
    let (_peer, frames) = done_rx.recv_timeout(WATCHDOG).unwrap();
    host.join().unwrap();
    for (i, frame) in frames.iter().enumerate() {
        let header = PacketHeader::decode(&frame[..PacketHeader::SIZE]).unwrap();
        assert_eq!(
            header.sequence,
            i as u32 + 2,
            "wire sequence must match admission order"
        );
        let payload = &frame[PacketHeader::SIZE..];
        if i < PRESSURE_FRAMES {
            assert_eq!(header.packet_type, PacketType::Control);
            let ControlMessage::ClipboardSyncUpdate(update) =
                ControlMessage::decode(payload).unwrap()
            else {
                panic!("clipboard frame replaced");
            };
            assert_eq!(update.text, text(i));
        } else if i == PRESSURE_FRAMES {
            assert_eq!(InputEvent::decode(payload).unwrap(), down);
        } else if i == PRESSURE_FRAMES + 1 {
            assert_eq!(InputEvent::decode(payload).unwrap(), up);
        } else {
            assert_eq!(
                ControlMessage::decode(payload).unwrap(),
                ControlMessage::Pong
            );
        }
    }
    runtime.stop().unwrap();
    assert!(session.tcp.lock().unwrap().is_none());
    session.disconnect().unwrap();
}

#[test]
fn runtime_stop_cancels_actual_pressured_writer_without_peer_release() {
    let Fixture {
        session,
        peer: _peer,
        _directory,
        _udp,
    } = fixture();
    let mut runtime = session.spawn_tcp_runtime().unwrap();
    pressurize(&session);
    let (done_tx, done_rx) = mpsc::channel();
    let stopper = thread::spawn(move || {
        let result = runtime.stop();
        done_tx.send((runtime, result)).unwrap();
    });
    let (runtime, result) = done_rx
        .recv_timeout(WATCHDOG)
        .expect("stop must not wait for peer reads");
    result.unwrap();
    stopper.join().unwrap();
    assert!(
        session.tcp.lock().unwrap().is_none(),
        "partial stream and queue must be destroyed together"
    );
    assert!(session.tcp_runtime.lock().unwrap().is_none());
    assert!(matches!(
        runtime.events().try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
    assert!(matches!(
        session.send_control(ControlMessage::Pong),
        Err(SessionError::NotReady)
    ));
    session.disconnect().unwrap();
}

#[test]
fn disconnect_cancels_actual_pressured_writer() {
    let Fixture {
        session,
        peer: _peer,
        _directory,
        _udp,
    } = fixture();
    let mut runtime = session.spawn_tcp_runtime().unwrap();
    pressurize(&session);
    let caller = session.clone();
    let (done_tx, done_rx) = mpsc::channel();
    let disconnect = thread::spawn(move || {
        caller.disconnect().unwrap();
        runtime.stop().unwrap();
        done_tx.send(()).unwrap();
    });
    done_rx
        .recv_timeout(WATCHDOG)
        .expect("disconnect and owned join must not wait for peer reads");
    disconnect.join().unwrap();
    assert_eq!(session.state().unwrap(), SessionState::Disconnected);
    assert!(session.tcp.lock().unwrap().is_none());
}

#[test]
fn runtime_surfaces_callback_backpressure_instead_of_dropping_pong() {
    let Fixture {
        session,
        mut peer,
        _directory,
        _udp,
    } = fixture();
    let mut runtime = session.spawn_tcp_runtime().unwrap();
    let (blocked_tx, blocked_rx) = mpsc::channel();
    *session.tcp_write_wait.lock().unwrap() = Some(blocked_tx);
    // One maximum-size framing payload consumes the entire byte admission cap
    // until its final byte is acknowledged. The paused bounded peer cannot
    // complete it; no timing-dependent search for a saturated queue is needed.
    session
        .send_packet(
            PacketType::Control,
            &vec![0; ferryx_rd_proto::MAX_TCP_FRAME_SIZE - PacketHeader::SIZE],
        )
        .unwrap();
    blocked_rx.recv_timeout(WATCHDOG).unwrap();
    let sequence = session.state.lock().unwrap().next_tcp_sequence;
    assert!(
        matches!(session.send_control(ControlMessage::Pong), Err(SessionError::Tls(TlsPskError::Io(e))) if e.kind() == io::ErrorKind::WouldBlock)
    );
    assert_eq!(session.state.lock().unwrap().next_tcp_sequence, sequence);
    peer.write_frame(&packet(PacketType::Ping, &[])).unwrap();
    assert!(
        matches!(runtime.events().recv_timeout(WATCHDOG).unwrap(), Err(SessionError::Tls(TlsPskError::Io(e))) if e.kind() == io::ErrorKind::WouldBlock)
    );
    runtime.stop().unwrap();
    assert!(session.tcp.lock().unwrap().is_none());
    session.disconnect().unwrap();
}
