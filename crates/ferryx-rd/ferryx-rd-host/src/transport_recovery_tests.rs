use super::*;
use ferryx_rd_proto::FrameChunk;

#[test]
fn negotiated_fec_and_gcm_nack_restore_lost_chunk_end_to_end() {
    let host = UdpSocket::bind("127.0.0.1:0").unwrap();
    let client = UdpSocket::bind("127.0.0.1:0").unwrap();
    host.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let key = [13; 32];
    let salt = [37; 16];
    let mut send = DatagramCipher::derive(&key, &salt, Direction::HostToClient).unwrap();
    let mut receive = DatagramCipher::derive(&key, &salt, Direction::HostToClient).unwrap();
    let mut c2h = DatagramCipher::derive(&key, &salt, Direction::ClientToHost).unwrap();
    let mut h_rx = DatagramCipher::derive(&key, &salt, Direction::ClientToHost).unwrap();
    let now = Instant::now();
    let data: Vec<u8> = (0..12000).map(|i| (i * 13) as u8).collect();
    let mut sender = UdpSender::default();
    sender.recovery.enabled = true;
    // Seed measured loss: clean links now intentionally send zero parity.
    sender.recovery.enqueue(
        ferryx_rd_proto::FrameNack {
            frame_id: 0,
            loss_permille: 75,
            rtt_ms: 10,
            missing: Vec::new(),
        },
        now,
    );
    sender
        .send_frame(
            &host,
            client.local_addr().unwrap(),
            &mut send,
            2,
            2,
            VideoFrame {
                data: data.clone(),
                is_key_frame: true,
                capture_at: now,
                encode_started_at: now,
                encode_completed_at: now,
            },
            now,
        )
        .unwrap();
    let mut frames = ferryx_rd_app::FrameAssembler::default();
    let mut parity = None;
    let mut lost_payload = None;
    let mut buffer = [0; 2048];
    loop {
        let (len, _) = client.recv_from(&mut buffer).unwrap();
        assert!(len <= UDP_PAYLOAD_BUDGET);
        let (header, payload) = receive.open_datagram(&buffer[..len]).unwrap();
        match header.packet_type {
            PacketType::FrameHeader => {
                frames
                    .push_header(
                        FrameHeader::decode(&payload).unwrap(),
                        header.timestamp_ms,
                        now,
                    )
                    .unwrap();
            }
            PacketType::FrameChunk => {
                let chunk = FrameChunk::decode(&payload).unwrap();
                if chunk.chunk_index == 1 {
                    lost_payload = Some(payload);
                } else {
                    assert!(frames.push_chunk(chunk, now).unwrap().is_none());
                }
            }
            PacketType::FrameParity => {
                parity = Some(ferryx_rd_proto::FrameParity::decode(&payload).unwrap());
            }
            PacketType::Ping => break,
            other => panic!("unexpected {other:?}"),
        }
    }
    // Pure XOR repair reaches the same assembled bytes without retransmit.
    assert_eq!(
        frames
            .push_parity(parity.unwrap(), now)
            .unwrap()
            .unwrap()
            .data,
        data
    );
    // Rebuild the loss, this time discarding parity too, and ask over GCM UDP.
    let nack = ferryx_rd_proto::FrameNack {
        frame_id: 1,
        loss_permille: 75,
        rtt_ms: 10,
        missing: vec![1],
    };
    let packet = c2h
        .seal_datagram(
            &PacketHeader::new(PacketType::FrameNack, 1, 0, 0),
            &nack.encode().unwrap(),
        )
        .unwrap();
    client.send_to(&packet, host.local_addr().unwrap()).unwrap();
    let (len, source) = host.recv_from(&mut buffer).unwrap();
    assert_eq!(source, client.local_addr().unwrap());
    let (header, payload) = h_rx.open_datagram(&buffer[..len]).unwrap();
    assert_eq!(header.packet_type, PacketType::FrameNack);
    assert!(
        h_rx.open_datagram(&buffer[..len]).is_err(),
        "GCM rejects replayed NACK"
    );
    for payload in sender
        .recovery
        .request(ferryx_rd_proto::FrameNack::decode(&payload).unwrap(), now)
    {
        assert_eq!(Some(&payload), lost_payload.as_ref());
        sender
            .send_packet(&host, source, &mut send, PacketType::FrameChunk, &payload)
            .unwrap();
    }
    let (len, _) = client.recv_from(&mut buffer).unwrap();
    let (header, payload) = receive.open_datagram(&buffer[..len]).unwrap();
    assert_eq!(header.packet_type, PacketType::FrameChunk);
    assert_eq!(Some(payload), lost_payload);
    assert_eq!(sender.recovery.redundancy(), 0.15);
}

#[test]
fn old_peer_packetization_stays_legacy_without_parity_or_cache() {
    let host = UdpSocket::bind("127.0.0.1:0").unwrap();
    let client = UdpSocket::bind("127.0.0.1:0").unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut send = DatagramCipher::derive(&[1; 32], &[2; 16], Direction::HostToClient).unwrap();
    let mut receive = DatagramCipher::derive(&[1; 32], &[2; 16], Direction::HostToClient).unwrap();
    let now = Instant::now();
    let mut sender = UdpSender::default();
    sender
        .send_frame(
            &host,
            client.local_addr().unwrap(),
            &mut send,
            2,
            2,
            VideoFrame {
                data: vec![1; SENDER_MAX_VIDEO_CHUNK_BYTES + 1],
                is_key_frame: true,
                capture_at: now,
                encode_started_at: now,
                encode_completed_at: now,
            },
            now,
        )
        .unwrap();
    let mut kinds = Vec::new();
    let mut buffer = [0; 2048];
    loop {
        let (len, _) = client.recv_from(&mut buffer).unwrap();
        let (header, payload) = receive.open_datagram(&buffer[..len]).unwrap();
        kinds.push(header.packet_type);
        if header.packet_type == PacketType::FrameChunk {
            let chunk = FrameChunk::decode(&payload).unwrap();
            assert_eq!(
                chunk.data.len(),
                if chunk.chunk_index == 0 {
                    SENDER_MAX_VIDEO_CHUNK_BYTES
                } else {
                    1
                }
            );
        }
        if header.packet_type == PacketType::Ping {
            break;
        }
    }
    assert_eq!(
        kinds,
        [
            PacketType::FrameHeader,
            PacketType::FrameChunk,
            PacketType::FrameChunk,
            PacketType::Ping
        ]
    );
    assert!(sender
        .recovery
        .request(
            ferryx_rd_proto::FrameNack {
                frame_id: 1,
                loss_permille: 1000,
                rtt_ms: 10,
                missing: vec![0]
            },
            now
        )
        .is_empty());
}

#[test]
fn fresh_counter_retransmit_reassembles_beyond_replay_window() {
    let now = Instant::now();
    let mut sender = UdpSender::default();
    sender.recovery.enabled = true;
    sender.recovery.begin(1, now);
    let mut send = DatagramCipher::derive(&[19; 32], &[23; 16], Direction::HostToClient).unwrap();
    let mut receive =
        DatagramCipher::derive(&[19; 32], &[23; 16], Direction::HostToClient).unwrap();
    let mut frames = ferryx_rd_app::FrameAssembler::default();
    frames
        .push_header(
            FrameHeader {
                frame_id: 1,
                width: 2,
                height: 2,
                is_key_frame: true,
                total_chunks: 2,
                total_size: 5,
            },
            42,
            now,
        )
        .unwrap();
    let first = FrameChunk {
        frame_id: 1,
        chunk_index: 0,
        data: vec![1, 2, 3],
    };
    let missing = FrameChunk {
        frame_id: 1,
        chunk_index: 1,
        data: vec![4, 5],
    };
    let packet = send
        .seal_datagram(
            &PacketHeader::new(PacketType::FrameChunk, 1, 42, 0),
            &first.encode().unwrap(),
        )
        .unwrap();
    let (_, payload) = receive.open_datagram(&packet).unwrap();
    assert!(frames
        .push_chunk(FrameChunk::decode(&payload).unwrap(), now)
        .unwrap()
        .is_none());
    let payload = missing.encode().unwrap();
    sender.recovery.retain(1, &payload);
    // This encrypted packet is genuinely lost: never opened by the receiver.
    let original = send
        .seal_datagram(
            &PacketHeader::new(PacketType::FrameChunk, 2, 42, 0),
            &payload,
        )
        .unwrap();
    for sequence in 3..4100 {
        let packet = send
            .seal_datagram(&PacketHeader::new(PacketType::Ping, sequence, 42, 0), &[])
            .unwrap();
        receive.open_datagram(&packet).unwrap();
    }
    assert!(matches!(
        receive.open_datagram(&original),
        Err(ferryx_rd_net::DatagramError::Replay)
    ));
    let nack = ferryx_rd_proto::FrameNack {
        frame_id: 1,
        loss_permille: 100,
        rtt_ms: 10,
        missing: vec![1],
    };
    let mut c2h = DatagramCipher::derive(&[19; 32], &[23; 16], Direction::ClientToHost).unwrap();
    let mut host_receive =
        DatagramCipher::derive(&[19; 32], &[23; 16], Direction::ClientToHost).unwrap();
    let request = c2h
        .seal_datagram(
            &PacketHeader::new(PacketType::FrameNack, 1, 42, 0),
            &nack.encode().unwrap(),
        )
        .unwrap();
    let (_, request) = host_receive.open_datagram(&request).unwrap();
    sender
        .recovery
        .enqueue(ferryx_rd_proto::FrameNack::decode(&request).unwrap(), now);
    let payload = sender.recovery.next(now).unwrap();
    let mut retransmit = Vec::new();
    sender
        .send_selected_packet(&mut send, PacketType::FrameChunk, &payload, |bytes| {
            retransmit.extend_from_slice(bytes);
            Ok(bytes.len())
        })
        .unwrap();
    let (header, payload) = receive.open_datagram(&retransmit).unwrap();
    assert_eq!(header.packet_type, PacketType::FrameChunk);
    let frame = frames
        .push_chunk(FrameChunk::decode(&payload).unwrap(), now)
        .unwrap()
        .expect("fresh-counter retransmit repairs missing chunk");
    assert_eq!(frame.data, [1, 2, 3, 4, 5]);
    assert!(matches!(
        receive.open_datagram(&retransmit),
        Err(ferryx_rd_net::DatagramError::Replay)
    ));
}
