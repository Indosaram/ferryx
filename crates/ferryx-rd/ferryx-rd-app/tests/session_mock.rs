use std::{net::UdpSocket, sync::mpsc, thread, time::Duration};

use ferryx_rd_app::{ClientSession, SessionConfig, SessionError, SessionState};
use ferryx_rd_net::{PskIdentity, TlsPskServer};
use ferryx_rd_proto::{
    BitrateAdjust, Capabilities, ControlMessage, Handshake, InputEvent, InputEventType, Modifiers,
    PacketHeader, PacketType, PairingGrant, PairingRequest, StreamConfiguration,
    StreamConfigurationResponse, WireCodec, PROTOCOL_VERSION,
};

fn packet(packet_type: PacketType, payload: &[u8]) -> Vec<u8> {
    let mut packet = PacketHeader::new(packet_type, 0, 0, 0).encode().unwrap();
    packet.extend_from_slice(payload);
    packet
}

fn split_packet(packet: &[u8]) -> (PacketHeader, &[u8]) {
    (
        PacketHeader::decode(&packet[..PacketHeader::SIZE]).unwrap(),
        &packet[PacketHeader::SIZE..],
    )
}

#[test]
fn mock_server_pairing_handshake_and_input_round_trip() {
    let pin = "12345678";
    let psk = PskIdentity::bootstrap(pin).unwrap();
    let listener = TlsPskServer::new([psk])
        .unwrap()
        .bind("127.0.0.1:0")
        .unwrap();
    let tcp_address = listener.local_addr().unwrap();
    let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
    let udp_port = udp.local_addr().unwrap().port();
    let (input_tx, input_rx) = mpsc::channel();

    let server = thread::spawn(move || {
        let mut stream = listener.accept().unwrap();
        let request_packet = stream.read_frame().unwrap();
        let (header, payload) = split_packet(&request_packet);
        assert_eq!(header.packet_type, PacketType::PairingRequest);
        assert_eq!(PairingRequest::decode(payload).unwrap().name, "rust-client");

        let grant = PairingGrant {
            pairing_id: "pairing-1".to_owned(),
            host_name: "mock-host".to_owned(),
            key: [0x5a; 32],
        };
        stream
            .write_frame(&packet(PacketType::PairingGrant, &grant.encode().unwrap()))
            .unwrap();

        let handshake_packet = stream.read_frame().unwrap();
        let (header, payload) = split_packet(&handshake_packet);
        assert_eq!(header.packet_type, PacketType::Handshake);
        let handshake = Handshake::decode(payload).unwrap();
        assert_eq!(handshake.pairing_id, "pairing-1");
        assert_ne!(handshake.session_salt, [0; 16]);

        let acknowledgement = Handshake {
            name: "mock-host".to_owned(),
            width: 1920,
            height: 1080,
            scale: 1.0,
            version: PROTOCOL_VERSION,
            capabilities: Capabilities::TEXT_CLIPBOARD_SYNC
                | Capabilities::AUTHENTICATED_UDP_REGISTRATION,
            pairing_id: String::new(),
            session_salt: [0; 16],
        };
        stream
            .write_frame(&packet(
                PacketType::HandshakeAck,
                &acknowledgement.encode().unwrap(),
            ))
            .unwrap();

        let mut ping = [0_u8; 1024];
        let (ping_len, _) = udp.recv_from(&mut ping).unwrap();
        let mut c2h = ferryx_rd_net::DatagramCipher::derive(
            &grant.key,
            &handshake.session_salt,
            ferryx_rd_net::Direction::ClientToHost,
        )
        .unwrap();
        let (ping_hdr, _) = c2h.open_datagram(&ping[..ping_len]).unwrap();
        assert_eq!(ping_hdr.packet_type, PacketType::Ping);

        let input_packet = stream.read_frame().unwrap();
        let (header, payload) = split_packet(&input_packet);
        assert_eq!(header.packet_type, PacketType::InputEvent);
        input_tx.send(InputEvent::decode(payload).unwrap()).unwrap();
    });

    let temporary = tempfile::tempdir().unwrap();
    let pairing_path = temporary.path().join("pairing-keys.json");
    let config = SessionConfig {
        host: "127.0.0.1".to_owned(),
        tcp_port: tcp_address.port(),
        udp_port,
        client_name: "rust-client".to_owned(),
        capabilities: Capabilities::TEXT_CLIPBOARD_SYNC
            | Capabilities::AUTHENTICATED_UDP_REGISTRATION,
        pairing_store_path: Some(pairing_path.clone()),
        connect_timeout: ferryx_rd_app::CONNECT_TIMEOUT,
        handshake_ack_timeout: ferryx_rd_app::HANDSHAKE_ACK_TIMEOUT,
    };
    let session = ClientSession::new(config).unwrap();
    let route = ferryx_rd_proto::RelayConnectCode {
        host_id: "unused-relay-host".into(),
        url: "ws://127.0.0.1:1".into(),
    };
    let ready = session
        .pair_with_pin_via_relay(pin, ferryx_rd_proto::PairingTrust::Owner, Some(&route))
        .unwrap();
    assert!(
        !session.is_relayed(),
        "successful direct bootstrap must not open the relay"
    );
    assert_eq!(ready.server.name, "mock-host");
    assert_eq!(session.state().unwrap(), SessionState::Ready);

    let input = InputEvent {
        event_type: InputEventType::KeyDown,
        x: 0.0,
        y: 0.0,
        key_code: 0x00,
        modifiers: Modifiers::COMMAND,
        scroll_dx: 0.0,
        scroll_dy: 0.0,
    };
    session.send_input(input).unwrap();
    assert_eq!(input_rx.recv().unwrap(), input);
    server.join().unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(pairing_path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn handshake_ack_timeout_is_reported() {
    let pin = "12345678";
    let psk = PskIdentity::bootstrap(pin).unwrap();
    let listener = TlsPskServer::new([psk])
        .unwrap()
        .bind("127.0.0.1:0")
        .unwrap();
    let tcp_address = listener.local_addr().unwrap();
    let (completed_tx, completed_rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let mut stream = listener.accept().unwrap();
        let _ = stream.read_frame().unwrap();
        let grant = PairingGrant {
            pairing_id: "pairing-timeout".to_owned(),
            host_name: "mock-host".to_owned(),
            key: [0x11; 32],
        };
        stream
            .write_frame(&packet(PacketType::PairingGrant, &grant.encode().unwrap()))
            .unwrap();
        let _ = stream.read_frame().unwrap();
        completed_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    });
    let temporary = tempfile::tempdir().unwrap();
    let config = SessionConfig {
        host: "127.0.0.1".to_owned(),
        tcp_port: tcp_address.port(),
        udp_port: 9,
        client_name: "rust-client".to_owned(),
        capabilities: Capabilities::empty(),
        pairing_store_path: Some(temporary.path().join("pairings.json")),
        connect_timeout: Duration::from_secs(1),
        handshake_ack_timeout: Duration::from_millis(50),
    };
    let session = ClientSession::new(config).unwrap();
    let result = session.pair_with_pin(pin);
    completed_tx.send(()).unwrap();
    server.join().unwrap();
    assert!(matches!(result, Err(SessionError::HandshakeAckTimeout)));
}

#[test]
fn connect_with_pairing_direct_round_trip() {
    let pairing_id = "test-psk-pairing-id";
    let key = [0x42; 32];
    let psk = PskIdentity::pairing(pairing_id, &key).unwrap();
    let listener = TlsPskServer::new([psk])
        .unwrap()
        .bind("127.0.0.1:0")
        .unwrap();
    let tcp_address = listener.local_addr().unwrap();
    let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
    let udp_port = udp.local_addr().unwrap().port();

    let server = thread::spawn(move || {
        let mut stream = listener.accept().unwrap();
        let handshake_packet = stream.read_frame().unwrap();
        let (header, payload) = split_packet(&handshake_packet);
        assert_eq!(header.packet_type, PacketType::Handshake);
        let handshake = Handshake::decode(payload).unwrap();
        assert_eq!(handshake.pairing_id, pairing_id);

        let acknowledgement = Handshake {
            name: "mock-host".to_owned(),
            width: 1920,
            height: 1080,
            scale: 1.0,
            version: PROTOCOL_VERSION,
            capabilities: Capabilities::AUTHENTICATED_UDP_REGISTRATION,
            pairing_id: String::new(),
            session_salt: [0; 16],
        };
        stream
            .write_frame(&packet(
                PacketType::HandshakeAck,
                &acknowledgement.encode().unwrap(),
            ))
            .unwrap();

        let mut ping = [0_u8; 1024];
        let (ping_len, _) = udp.recv_from(&mut ping).unwrap();
        let mut c2h = ferryx_rd_net::DatagramCipher::derive(
            &key,
            &handshake.session_salt,
            ferryx_rd_net::Direction::ClientToHost,
        )
        .unwrap();
        let (ping_hdr, _) = c2h.open_datagram(&ping[..ping_len]).unwrap();
        assert_eq!(ping_hdr.packet_type, PacketType::Ping);
    });

    let config = SessionConfig {
        host: "127.0.0.1".to_owned(),
        tcp_port: tcp_address.port(),
        udp_port,
        client_name: "rust-client".to_owned(),
        capabilities: Capabilities::AUTHENTICATED_UDP_REGISTRATION,
        pairing_store_path: None,
        connect_timeout: Duration::from_secs(1),
        handshake_ack_timeout: Duration::from_secs(1),
    };
    let session = ClientSession::new(config).unwrap();
    let record = ferryx_rd_app::PairingRecord {
        id: pairing_id.to_string(),
        name: "mock-host".to_string(),
        key: key.to_vec(),
        added_at_unix_ms: 0,
        last_endpoint: None,
        endpoint_aliases: Vec::new(),
        relay_url: None,
        relay_host_id: None,
        trust: ferryx_rd_proto::PairingTrust::Owner,
    };
    let ready = session.connect_with_pairing(record).unwrap();
    assert_eq!(ready.server.name, "mock-host");
    assert_eq!(session.state().unwrap(), SessionState::Ready);
    server.join().unwrap();
}

#[test]
fn pre_ready_input_is_refused() {
    let temporary = tempfile::tempdir().unwrap();
    let mut config = SessionConfig::direct("127.0.0.1", "rust-client");
    config.pairing_store_path = Some(temporary.path().join("pairings.json"));
    let session = ClientSession::new(config).unwrap();
    let event = InputEvent {
        event_type: InputEventType::MouseMove,
        x: 0.5,
        y: 0.5,
        key_code: 0,
        modifiers: Modifiers::empty(),
        scroll_dx: 0.0,
        scroll_dy: 0.0,
    };
    assert!(matches!(
        session.send_input(event),
        Err(SessionError::NotReady)
    ));
}

#[test]
fn stalled_consumer_retains_latest_clipboard_and_terminal_error() {
    use ferryx_rd_proto::{
        ClipboardSyncDirection, ClipboardSyncOrigin, ClipboardSyncUpdate, ControlMessage,
    };
    let key = [42; 32];
    let listener = TlsPskServer::new([PskIdentity::pairing("bounded", &key).unwrap()])
        .unwrap()
        .bind("127.0.0.1:0")
        .unwrap();
    let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let mut config = SessionConfig::direct("127.0.0.1", "bounded");
    config.tcp_port = listener.local_addr().unwrap().port();
    config.udp_port = udp.local_addr().unwrap().port();
    config.pairing_store_path = Some(temporary.path().join("pairings.json"));
    let (done_tx, done_rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let mut stream = listener.accept().unwrap();
        stream
            .ssl_stream()
            .get_ref()
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let frame = stream.read_frame().unwrap();
        let (_, payload) = split_packet(&frame);
        let mut ack = Handshake::decode(payload).unwrap();
        ack.capabilities =
            Capabilities::TEXT_CLIPBOARD_SYNC | Capabilities::AUTHENTICATED_UDP_REGISTRATION;
        stream
            .write_frame(&packet(PacketType::HandshakeAck, &ack.encode().unwrap()))
            .unwrap();
        for index in 0..128 {
            let update = ControlMessage::ClipboardSyncUpdate(ClipboardSyncUpdate {
                request_id: index,
                direction: ClipboardSyncDirection::HostToClient,
                origin: ClipboardSyncOrigin::LocalPasteboard,
                text: index.to_string(),
            });
            stream
                .write_frame(&packet(PacketType::Control, &update.encode().unwrap()))
                .unwrap();
            stream.write_frame(&packet(PacketType::Ping, &[])).unwrap();
            let pong = stream.read_frame().unwrap();
            assert_eq!(
                ControlMessage::decode(split_packet(&pong).1).unwrap(),
                ControlMessage::Pong
            );
        }
        let mut reader = stream.ssl_stream().get_ref().try_clone().unwrap();
        reader
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .ssl_stream()
            .get_ref()
            .shutdown(std::net::Shutdown::Write)
            .unwrap();
        let mut probe = [0_u8; 1];
        let read_result = std::io::Read::read(&mut reader, &mut probe);
        assert_eq!(read_result.unwrap(), 0);
        done_tx.send(()).unwrap();
    });
    let session = ClientSession::new(config).unwrap();
    session
        .connect_with_pairing(ferryx_rd_app::PairingRecord {
            id: "bounded".into(),
            name: "host".into(),
            key: key.to_vec(),
            added_at_unix_ms: 0,
            last_endpoint: None,
            endpoint_aliases: Vec::new(),
            relay_url: None,
            relay_host_id: None,
            trust: ferryx_rd_proto::PairingTrust::GuestOnce,
        })
        .unwrap();
    let mut runtime = session.spawn_tcp_runtime().unwrap();
    done_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    server.join().unwrap();
    runtime.stop().unwrap();
    let mut events = Vec::new();
    loop {
        let event = runtime
            .events()
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        let terminal = event.is_err();
        events.push(event);
        if terminal {
            break;
        }
    }
    session.disconnect().unwrap();
    assert!(events.len() <= 3, "retained {} events", events.len());
    assert!(events.iter().any(
        |event| matches!(event, Ok(ferryx_rd_app::SessionEvent::Clipboard(text)) if text == "127")
    ));
    assert!(
        events.iter().any(|event| event.is_err()),
        "terminal error must be retained"
    );
    assert!(matches!(
        runtime.events().try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
}

#[test]
fn mock_server_abr_bitrate_adjust_round_trip() {
    // Given: authenticated session with mock server listening for control events.
    let key = [0x42; 32];
    let listener = TlsPskServer::new([PskIdentity::pairing("abr-test", &key).unwrap()])
        .unwrap()
        .bind("127.0.0.1:0")
        .unwrap();
    let tcp_address = listener.local_addr().unwrap();
    let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
    let udp_port = udp.local_addr().unwrap().port();
    let (bitrate_tx, bitrate_rx) = mpsc::channel();

    let server = thread::spawn(move || {
        let mut stream = listener.accept().unwrap();
        stream
            .ssl_stream()
            .get_ref()
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let frame = stream.read_frame().unwrap();
        let (_, payload) = split_packet(&frame);
        stream
            .write_frame(&packet(PacketType::HandshakeAck, payload))
            .unwrap();

        let mut probe = [0_u8; 1024];
        udp.recv_from(&mut probe).unwrap();

        let control_packet = stream.read_frame().unwrap();
        let (header, payload) = split_packet(&control_packet);
        assert_eq!(header.packet_type, PacketType::Control);
        match ControlMessage::decode(payload).unwrap() {
            ControlMessage::BitrateAdjust(BitrateAdjust { target_bitrate }) => {
                bitrate_tx.send(target_bitrate).unwrap();
            }
            other => panic!("unexpected control: {other:?}"),
        }
    });

    let temporary = tempfile::tempdir().unwrap();
    let config = SessionConfig {
        host: "127.0.0.1".to_owned(),
        tcp_port: tcp_address.port(),
        udp_port,
        client_name: "abr-client".to_owned(),
        capabilities: Capabilities::AUTHENTICATED_UDP_REGISTRATION,
        pairing_store_path: Some(temporary.path().join("pairings.json")),
        connect_timeout: Duration::from_secs(5),
        handshake_ack_timeout: Duration::from_secs(5),
    };
    let session = ClientSession::new(config).unwrap();
    session
        .connect_with_pairing(ferryx_rd_app::PairingRecord {
            id: "abr-test".into(),
            name: "host".into(),
            key: key.to_vec(),
            added_at_unix_ms: 0,
            last_endpoint: None,
            endpoint_aliases: Vec::new(),
            relay_url: None,
            relay_host_id: None,
            trust: ferryx_rd_proto::PairingTrust::Owner,
        })
        .unwrap();

    // When: client requests bitrate adjustment through send_bitrate_adjust.
    session.send_bitrate_adjust(6_000_000).unwrap();

    // Then: mock server receives the exact adjusted target bitrate.
    assert_eq!(
        bitrate_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        6_000_000
    );

    server.join().unwrap();
    session.disconnect().unwrap();
}

#[test]
fn mock_server_stream_config_negotiation_round_trip() {
    // Given: authenticated session with mock server responding to stream config request.
    let key = [0x66; 32];
    let listener = TlsPskServer::new([PskIdentity::pairing("config-client", &key).unwrap()])
        .unwrap()
        .bind("127.0.0.1:0")
        .unwrap();
    let tcp_address = listener.local_addr().unwrap();
    let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
    let udp_port = udp.local_addr().unwrap().port();

    let server = thread::spawn(move || {
        let mut stream = listener.accept().unwrap();
        stream
            .ssl_stream()
            .get_ref()
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let frame = stream.read_frame().unwrap();
        let (_, payload) = split_packet(&frame);
        stream
            .write_frame(&packet(PacketType::HandshakeAck, payload))
            .unwrap();

        let mut probe = [0_u8; 1024];
        udp.recv_from(&mut probe).unwrap();

        let req_frame = stream.read_frame().unwrap();
        let (header, payload) = split_packet(&req_frame);
        assert_eq!(header.packet_type, PacketType::Control);
        let req = match ControlMessage::decode(payload).unwrap() {
            ControlMessage::StreamConfigRequest(req) => req,
            other => panic!("expected StreamConfigRequest, got {other:?}"),
        };
        assert_eq!(req.request_id, 42);
        assert_eq!(req.desired.width, 2560);
        assert_eq!(req.desired.height, 1440);

        let response = ControlMessage::StreamConfigResponse(StreamConfigurationResponse {
            request_id: req.request_id,
            active: req.desired,
        });
        stream
            .write_frame(&packet(PacketType::Control, &response.encode().unwrap()))
            .unwrap();
    });

    let temporary = tempfile::tempdir().unwrap();
    let config = SessionConfig {
        host: "127.0.0.1".to_owned(),
        tcp_port: tcp_address.port(),
        udp_port,
        client_name: "config-client".to_owned(),
        capabilities: Capabilities::AUTHENTICATED_UDP_REGISTRATION,
        pairing_store_path: Some(temporary.path().join("pairings.json")),
        connect_timeout: Duration::from_secs(5),
        handshake_ack_timeout: Duration::from_secs(5),
    };
    let session = ClientSession::new(config).unwrap();
    session
        .connect_with_pairing(ferryx_rd_app::PairingRecord {
            id: "config-client".into(),
            name: "host".into(),
            key: key.to_vec(),
            added_at_unix_ms: 0,
            last_endpoint: None,
            endpoint_aliases: Vec::new(),
            relay_url: None,
            relay_host_id: None,
            trust: ferryx_rd_proto::PairingTrust::Owner,
        })
        .unwrap();

    // When: client submits stream configuration request.
    let desired = StreamConfiguration {
        width: 2560,
        height: 1440,
        bitrate: 15_000_000,
        frames_per_second: 60,
    };
    session.request_stream_config(42, desired).unwrap();
    let event = session.receive_tcp_event().unwrap();

    // Then: client receives matching StreamConfigResponse.
    match event {
        ferryx_rd_app::SessionEvent::StreamConfig(ControlMessage::StreamConfigResponse(res)) => {
            assert_eq!(res.request_id, 42);
            assert_eq!(res.active, desired);
        }
        other => panic!("expected StreamConfigResponse event, got {other:?}"),
    }

    server.join().unwrap();
    session.disconnect().unwrap();
}

#[test]
fn udp_client_rejects_host_missing_authenticated_registration_capability() {
    let pairing_id = "test-caps-id";
    let key = [0x55; 32];
    let psk = PskIdentity::pairing(pairing_id, &key).unwrap();
    let listener = TlsPskServer::new([psk])
        .unwrap()
        .bind("127.0.0.1:0")
        .unwrap();
    let tcp_address = listener.local_addr().unwrap();
    let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
    let udp_port = udp.local_addr().unwrap().port();

    let server = thread::spawn(move || {
        let mut stream = listener.accept().unwrap();
        let handshake_packet = stream.read_frame().unwrap();
        let (header, payload) = split_packet(&handshake_packet);
        assert_eq!(header.packet_type, PacketType::Handshake);
        let handshake = Handshake::decode(payload).unwrap();
        assert_eq!(handshake.pairing_id, pairing_id);

        // Host sends acknowledgement with Capabilities::empty() (missing AUTHENTICATED_UDP_REGISTRATION)
        let acknowledgement = Handshake {
            name: "mock-host".to_owned(),
            width: 1920,
            height: 1080,
            scale: 1.0,
            version: PROTOCOL_VERSION,
            capabilities: Capabilities::empty(),
            pairing_id: String::new(),
            session_salt: [0; 16],
        };
        stream
            .write_frame(&packet(
                PacketType::HandshakeAck,
                &acknowledgement.encode().unwrap(),
            ))
            .unwrap();
    });

    let config = SessionConfig {
        host: "127.0.0.1".to_owned(),
        tcp_port: tcp_address.port(),
        udp_port,
        client_name: "rust-client".to_owned(),
        capabilities: Capabilities::STREAM_CONFIGURATION
            | Capabilities::AUTHENTICATED_UDP_REGISTRATION,
        pairing_store_path: None,
        connect_timeout: Duration::from_secs(2),
        handshake_ack_timeout: Duration::from_secs(2),
    };
    let session = ClientSession::new(config).unwrap();
    let record = ferryx_rd_app::PairingRecord {
        id: pairing_id.to_string(),
        name: "mock-host".to_string(),
        key: key.to_vec(),
        added_at_unix_ms: 0,
        last_endpoint: None,
        endpoint_aliases: Vec::new(),
        relay_url: None,
        relay_host_id: None,
        trust: ferryx_rd_proto::PairingTrust::Owner,
    };
    let result = session.connect_with_pairing(record);
    assert!(
        matches!(result, Err(SessionError::MissingAuthenticatedRegistration)),
        "client must fail before Ready when host lacks authenticated registration capability, got: {result:?}"
    );
    assert_ne!(
        session.state().unwrap(),
        SessionState::Ready,
        "session must not be Ready"
    );
    server.join().unwrap();
}

#[test]
fn udp_client_sends_authenticated_registration_and_preserves_nonce() {
    let pairing_id = "test-reg-nonce-id";
    let key = [0x66; 32];
    let psk = PskIdentity::pairing(pairing_id, &key).unwrap();
    let listener = TlsPskServer::new([psk])
        .unwrap()
        .bind("127.0.0.1:0")
        .unwrap();
    let tcp_address = listener.local_addr().unwrap();
    let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
    let udp_port = udp.local_addr().unwrap().port();

    let server = thread::spawn(move || {
        let mut stream = listener.accept().unwrap();
        let handshake_packet = stream.read_frame().unwrap();
        let (header, payload) = split_packet(&handshake_packet);
        assert_eq!(header.packet_type, PacketType::Handshake);
        let handshake = Handshake::decode(payload).unwrap();
        let salt = handshake.session_salt;

        let acknowledgement = Handshake {
            name: "mock-host".to_owned(),
            width: 1920,
            height: 1080,
            scale: 1.0,
            version: PROTOCOL_VERSION,
            capabilities: Capabilities::STREAM_CONFIGURATION
                | Capabilities::AUTHENTICATED_UDP_REGISTRATION,
            pairing_id: String::new(),
            session_salt: [0; 16],
        };
        stream
            .write_frame(&packet(
                PacketType::HandshakeAck,
                &acknowledgement.encode().unwrap(),
            ))
            .unwrap();

        // Host receives registration datagram from client
        let mut reg_buf = [0_u8; 1024];
        let (reg_len, _client_udp_addr) = udp.recv_from(&mut reg_buf).unwrap();

        // Must open cleanly with ClientToHost cipher (consuming nonce counter 1)
        let mut c2h_cipher =
            ferryx_rd_net::DatagramCipher::derive(&key, &salt, ferryx_rd_net::Direction::ClientToHost)
                .unwrap();
        let (reg_header, reg_payload) = c2h_cipher.open_datagram(&reg_buf[..reg_len]).unwrap();
        assert_eq!(
            reg_header.packet_type,
            PacketType::Ping,
            "registration must be PacketType::Ping"
        );
        assert!(
            reg_payload.is_empty(),
            "registration ping payload must be empty"
        );

        // The nonce counter 1 was consumed by registration. Replaying counter 1 must fail.
        assert!(
            c2h_cipher
                .open(
                    &reg_buf[PacketHeader::SIZE..reg_len],
                    &reg_buf[..PacketHeader::SIZE]
                )
                .is_err(),
            "replayed counter 1 must be rejected by host replay window"
        );

        // Next, host awaits subsequent session UDP datagram sent by the client.
        // If the client preserved cipher nonce ownership, this packet uses nonce counter 2,
        // which the SAME host c2h_cipher cleanly opens and verifies.
        let mut subseq_buf = [0_u8; 1024];
        let (subseq_len, _) = udp.recv_from(&mut subseq_buf).unwrap();
        let (subseq_header, subseq_payload) =
            c2h_cipher.open_datagram(&subseq_buf[..subseq_len]).unwrap();
        assert_eq!(subseq_header.packet_type, PacketType::Ping);
        assert_eq!(subseq_payload, b"subsequent-session-packet");
        assert_eq!(subseq_header.sequence, 1);
    });

    let config = SessionConfig {
        host: "127.0.0.1".to_owned(),
        tcp_port: tcp_address.port(),
        udp_port,
        client_name: "rust-client".to_owned(),
        capabilities: Capabilities::STREAM_CONFIGURATION
            | Capabilities::AUTHENTICATED_UDP_REGISTRATION,
        pairing_store_path: None,
        connect_timeout: Duration::from_secs(2),
        handshake_ack_timeout: Duration::from_secs(2),
    };
    let session = ClientSession::new(config).unwrap();
    let record = ferryx_rd_app::PairingRecord {
        id: pairing_id.to_string(),
        name: "mock-host".to_string(),
        key: key.to_vec(),
        added_at_unix_ms: 0,
        last_endpoint: None,
        endpoint_aliases: Vec::new(),
        relay_url: None,
        relay_host_id: None,
        trust: ferryx_rd_proto::PairingTrust::Owner,
    };
    let ready = session.connect_with_pairing(record).unwrap();
    assert_eq!(ready.server.name, "mock-host");
    assert_eq!(session.state().unwrap(), SessionState::Ready);

    // Exercise actual subsequent session UDP send to prove client sender was not re-derived
    session
        .send_udp(PacketType::Ping, b"subsequent-session-packet")
        .unwrap();

    server.join().unwrap();
}
