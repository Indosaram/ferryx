use super::*;

/// Explicit conservative UDP payload budget ensuring datagrams never exceed
/// the 1280-byte MTU standard for IPv6 and Tailscale / WireGuard virtual interfaces.
///
/// Budget calculation:
/// ```text
///   1280 bytes MTU
/// -   40 bytes IPv6 header (or 20 bytes IPv4)
/// -    8 bytes UDP header
/// = 1232 bytes maximum UDP payload under IPv6
/// ```
///
/// To provide safe headroom for IP options and encapsulation headers,
/// MahoRD targets a conservative 1200-byte UDP payload budget.
pub const UDP_PAYLOAD_BUDGET: usize = 1200;

/// Encrypted envelope overhead:
/// PacketHeader::SIZE (12) + udp_gcm::NONCE_SIZE (12) + udp_gcm::TAG_SIZE (16) = 40 bytes.
pub const ENCRYPTED_DATAGRAM_OVERHEAD: usize =
    PacketHeader::SIZE + ferryx_rd_net::udp_gcm::NONCE_SIZE + ferryx_rd_net::udp_gcm::TAG_SIZE;

/// Maximum plaintext payload fitting within the 1200-byte UDP payload budget:
/// 1200 - 40 = 1160 bytes.
pub const MAX_PLAINTEXT_PAYLOAD: usize = UDP_PAYLOAD_BUDGET - ENCRYPTED_DATAGRAM_OVERHEAD;

/// Target MTU-safe video chunk size:
/// 1160 - FrameChunk::HEADER_SIZE (6) = 1154 bytes.
pub const TARGET_VIDEO_CHUNK_BYTES: usize = MAX_PLAINTEXT_PAYLOAD - FrameChunk::HEADER_SIZE;

/// Target MTU-safe audio fragment size:
/// 1160 - AudioFragmentHeader::SIZE (8) = 1152 bytes.
pub const TARGET_AUDIO_FRAGMENT_BYTES: usize = MAX_PLAINTEXT_PAYLOAD - AudioFragmentHeader::SIZE;

fn setup_sender_channel() -> (UdpSocket, UdpSocket, SocketAddr, DatagramCipher, DatagramCipher) {
    let tx = UdpSocket::bind("127.0.0.1:0").expect("bind tx socket");
    let rx = UdpSocket::bind("127.0.0.1:0").expect("bind rx socket");
    rx.set_read_timeout(Some(Duration::from_secs(3)))
        .expect("set read timeout");
    let peer = rx.local_addr().expect("rx local addr");
    let host_cipher =
        DatagramCipher::derive(&[0x55; 32], &[0xaa; 16], Direction::HostToClient).expect("cipher");
    let client_cipher =
        DatagramCipher::derive(&[0x55; 32], &[0xaa; 16], Direction::HostToClient).expect("cipher");
    (tx, rx, peer, host_cipher, client_cipher)
}

#[test]
fn video_datagrams_fit_mtu_budget_and_reassemble() {
    let (tx, rx, peer, mut host_cipher, mut client_cipher) = setup_sender_channel();
    let mut sender = UdpSender::default();
    let mut packet_buffer = [0_u8; 65536];

    // Test cases covering:
    // 1. Sub-chunk boundary (small frame)
    // 2. Exact single-chunk boundary (TARGET_VIDEO_CHUNK_BYTES = 1154)
    // 3. Exact multiple chunks (2 * TARGET_VIDEO_CHUNK_BYTES = 2308)
    // 4. Multiple chunks with tail (2 * TARGET_VIDEO_CHUNK_BYTES + 377 = 2685)
    let test_cases = [
        (500, true),
        (TARGET_VIDEO_CHUNK_BYTES, false),
        (TARGET_VIDEO_CHUNK_BYTES * 2, true),
        (TARGET_VIDEO_CHUNK_BYTES * 2 + 377, false),
    ];

    let mut expected_sequence = sender.sequence;

    for (case_idx, &(data_len, is_key)) in test_cases.iter().enumerate() {
        let pattern = (0..data_len)
            .map(|i| ((i + case_idx * 17) % 251) as u8)
            .collect::<Vec<u8>>();
        let now = Instant::now();
        let frame = VideoFrame {
            data: pattern.clone(),
            is_key_frame: is_key,
            capture_at: now,
            encode_started_at: now,
            encode_completed_at: now,
        };

        sender
            .send_frame(&tx, peer, &mut host_cipher, 1920, 1080, frame, now)
            .expect("send_frame must succeed");

        // Receive datagram 0: FrameHeader
        let size = rx.recv(&mut packet_buffer).expect("recv FrameHeader");
        assert!(
            size <= UDP_PAYLOAD_BUDGET,
            "FrameHeader datagram size {size} exceeds conservative UDP budget {UDP_PAYLOAD_BUDGET}"
        );
        let (header, payload) = client_cipher
            .open_datagram(&packet_buffer[..size])
            .expect("open FrameHeader datagram");

        expected_sequence = expected_sequence.wrapping_add(1);
        assert_eq!(
            header.sequence, expected_sequence,
            "FrameHeader sequence mismatch"
        );
        assert_eq!(header.packet_type, PacketType::FrameHeader);

        let frame_header = FrameHeader::decode(&payload).expect("decode FrameHeader");
        assert_eq!(frame_header.frame_id, sender.frame_id);
        assert_eq!(frame_header.width, 1920);
        assert_eq!(frame_header.height, 1080);
        assert_eq!(frame_header.is_key_frame, is_key, "keyframe flag must be preserved");
        assert_eq!(frame_header.total_size as usize, data_len);

        let total_chunks = frame_header.total_chunks as usize;

        // Receive chunk datagrams
        let mut reassembled_chunks = std::collections::BTreeMap::<u16, Vec<u8>>::new();
        for _ in 0..total_chunks {
            let size = rx.recv(&mut packet_buffer).expect("recv FrameChunk");
            assert!(
                size <= UDP_PAYLOAD_BUDGET,
                "FrameChunk datagram size {size} exceeds conservative UDP budget {UDP_PAYLOAD_BUDGET}"
            );
            let (chunk_pkt_header, chunk_payload) = client_cipher
                .open_datagram(&packet_buffer[..size])
                .expect("open FrameChunk datagram");

            expected_sequence = expected_sequence.wrapping_add(1);
            assert_eq!(
                chunk_pkt_header.sequence, expected_sequence,
                "FrameChunk sequence continuity check"
            );
            assert_eq!(chunk_pkt_header.packet_type, PacketType::FrameChunk);

            let chunk = FrameChunk::decode(&chunk_payload).expect("decode FrameChunk");
            assert_eq!(chunk.frame_id, sender.frame_id);
            assert!(
                chunk.data.len() <= TARGET_VIDEO_CHUNK_BYTES,
                "video chunk length {} exceeds MTU target {}",
                chunk.data.len(),
                TARGET_VIDEO_CHUNK_BYTES
            );
            assert!(
                reassembled_chunks.insert(chunk.chunk_index, chunk.data).is_none(),
                "duplicate chunk index {}",
                chunk.chunk_index
            );
        }

        // Receive trailing Ping datagram (TimestampStats)
        let size = rx.recv(&mut packet_buffer).expect("recv Ping");
        assert!(
            size <= UDP_PAYLOAD_BUDGET,
            "Ping datagram size {size} exceeds conservative UDP budget {UDP_PAYLOAD_BUDGET}"
        );
        let (ping_header, ping_payload) = client_cipher
            .open_datagram(&packet_buffer[..size])
            .expect("open Ping datagram");

        expected_sequence = expected_sequence.wrapping_add(1);
        assert_eq!(ping_header.sequence, expected_sequence, "Ping sequence mismatch");
        assert_eq!(ping_header.packet_type, PacketType::Ping);

        let stats = TimestampStats::decode(&ping_payload).expect("decode TimestampStats");
        assert_eq!(stats.frame_id, sender.frame_id, "timestamp stats frame_id must match");
        assert!(stats.capture_us <= stats.encode_start_us);
        assert!(stats.encode_start_us <= stats.encode_end_us);
        assert!(stats.encode_end_us <= stats.send_us);

        // Authenticated reassembly verification
        assert_eq!(reassembled_chunks.len(), total_chunks);
        let mut reassembled_bytes = Vec::with_capacity(data_len);
        for chunk_idx in 0..total_chunks {
            let chunk_data = reassembled_chunks
                .get(&(chunk_idx as u16))
                .expect("missing chunk");
            reassembled_bytes.extend_from_slice(chunk_data);
        }
        assert_eq!(
            reassembled_bytes, pattern,
            "authenticated reassembled video frame does not match original bytes"
        );
    }
}

#[test]
fn audio_datagrams_fit_mtu_budget_and_reassemble() {
    let (tx, rx, peer, mut host_cipher, mut client_cipher) = setup_sender_channel();
    let mut sender = UdpSender::default();
    let mut packet_buffer = [0_u8; 65536];

    // Test cases covering:
    // 1. Sub-fragment boundary (small audio block)
    // 2. Exact single-fragment boundary (TARGET_AUDIO_FRAGMENT_BYTES = 1152)
    // 3. Exact multiple fragments (2 * TARGET_AUDIO_FRAGMENT_BYTES = 2304)
    // 4. Multiple fragments with tail (2 * TARGET_AUDIO_FRAGMENT_BYTES + 511 = 2815)
    let test_cases = [
        300,
        TARGET_AUDIO_FRAGMENT_BYTES,
        TARGET_AUDIO_FRAGMENT_BYTES * 2,
        TARGET_AUDIO_FRAGMENT_BYTES * 2 + 511,
    ];

    let mut expected_sequence = sender.sequence;

    for (case_idx, &data_len) in test_cases.iter().enumerate() {
        let pattern = (0..data_len)
            .map(|i| ((i + case_idx * 31) % 251) as u8)
            .collect::<Vec<u8>>();

        sender
            .send_audio(&tx, peer, &mut host_cipher, &pattern)
            .expect("send_audio must succeed");

        let mut reassembled_fragments = std::collections::BTreeMap::<u16, Vec<u8>>::new();
        let mut expected_count = None;

        let mut received = 0;
        loop {
            let size = rx.recv(&mut packet_buffer).expect("recv AudioFrame");
            assert!(
                size <= UDP_PAYLOAD_BUDGET,
                "AudioFrame datagram size {size} exceeds conservative UDP budget {UDP_PAYLOAD_BUDGET}"
            );
            let (header, payload) = client_cipher
                .open_datagram(&packet_buffer[..size])
                .expect("open AudioFrame datagram");

            expected_sequence = expected_sequence.wrapping_add(1);
            assert_eq!(
                header.sequence, expected_sequence,
                "AudioFrame sequence continuity check"
            );
            assert_eq!(header.packet_type, PacketType::AudioFrame);

            let fragment = AudioFragment::decode(&payload).expect("decode AudioFragment");
            assert_eq!(fragment.header.frame_id, sender.audio_frame_id);
            assert!(
                fragment.data.len() <= TARGET_AUDIO_FRAGMENT_BYTES,
                "audio fragment length {} exceeds MTU target {}",
                fragment.data.len(),
                TARGET_AUDIO_FRAGMENT_BYTES
            );

            if let Some(count) = expected_count {
                assert_eq!(fragment.header.fragment_count, count);
            } else {
                expected_count = Some(fragment.header.fragment_count);
            }

            assert!(
                reassembled_fragments
                    .insert(fragment.header.fragment_index, fragment.data)
                    .is_none(),
                "duplicate audio fragment index"
            );

            received += 1;
            if received == expected_count.unwrap() as usize {
                break;
            }
        }

        // Authenticated reassembly verification
        let total_fragments = expected_count.unwrap() as usize;
        assert_eq!(reassembled_fragments.len(), total_fragments);
        let mut reassembled_bytes = Vec::with_capacity(data_len);
        for frag_idx in 0..total_fragments {
            let frag_data = reassembled_fragments
                .get(&(frag_idx as u16))
                .expect("missing fragment");
            reassembled_bytes.extend_from_slice(frag_data);
        }
        assert_eq!(
            reassembled_bytes, pattern,
            "authenticated reassembled audio block does not match original bytes"
        );
    }
}

#[test]
fn video_frame_exceeding_max_chunks_is_rejected() {
    let (tx, _rx, peer, mut host_cipher, _) = setup_sender_channel();
    let mut sender = UdpSender::default();
    let initial_sequence = sender.sequence;
    let initial_frame_id = sender.frame_id;
    let now = Instant::now();

    // Sized to require 8193 chunks (> MAX_CHUNKS_PER_FRAME = 8192)
    let oversized_len = TARGET_VIDEO_CHUNK_BYTES * (ferryx_rd_proto::MAX_CHUNKS_PER_FRAME as usize) + 1;
    let frame = VideoFrame {
        data: vec![0x33; oversized_len],
        is_key_frame: true,
        capture_at: now,
        encode_started_at: now,
        encode_completed_at: now,
    };

    let result = sender.send_frame(&tx, peer, &mut host_cipher, 1920, 1080, frame, now);
    assert_eq!(
        sender.sequence, initial_sequence,
        "sequence must remain unchanged on rejected frame"
    );
    assert_eq!(
        sender.frame_id, initial_frame_id,
        "frame_id must remain unchanged on rejected frame"
    );
    match result {
        Err(SessionError::Codec(ferryx_rd_proto::CodecError::LengthLimit {
            field: "frame chunks",
            actual,
            max,
        })) => {
            assert_eq!(actual, (ferryx_rd_proto::MAX_CHUNKS_PER_FRAME + 1) as usize);
            assert_eq!(max, ferryx_rd_proto::MAX_CHUNKS_PER_FRAME as usize);
        }
        other => panic!("expected typed CodecError::LengthLimit, got: {other:?}"),
    }
}

#[test]
fn audio_empty_data_is_noop() {
    let (tx, _rx, peer, mut host_cipher, _) = setup_sender_channel();
    let mut sender = UdpSender::default();
    let seq_before = sender.sequence;
    let audio_id_before = sender.audio_frame_id;
    sender
        .send_audio(&tx, peer, &mut host_cipher, &[])
        .expect("empty audio should succeed as no-op");
    assert_eq!(sender.sequence, seq_before);
    assert_eq!(sender.audio_frame_id, audio_id_before);
}
