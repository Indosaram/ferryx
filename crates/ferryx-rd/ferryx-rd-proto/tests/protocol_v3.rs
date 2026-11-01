use std::fmt::Debug;

use ferryx_rd_proto::*;

fn assert_round_trip<T>(value: &T)
where
    T: WireCodec + PartialEq + Debug,
{
    let encoded = value.encode().expect("encode");
    let decoded = T::decode(&encoded).expect("decode");
    assert_eq!(&decoded, value);
}

#[test]
fn packet_header_round_trips_every_packet_type() {
    for (index, packet_type) in PacketType::ALL.into_iter().enumerate() {
        assert_eq!(
            PacketType::try_from(packet_type as u8).unwrap(),
            packet_type
        );
        let header = PacketHeader::new(packet_type, index as u32, 0x89ab_cdef, 0x5a);
        assert_round_trip(&header);
        assert_eq!(header.encode().unwrap().len(), PacketHeader::SIZE);
    }
}

#[test]
fn packet_header_matches_swift_vector() {
    let header = PacketHeader::new(PacketType::Ping, 1, 2, 0);
    assert_eq!(
        header.encode().unwrap(),
        [0x1d, 0xec, 0x07, 0x01, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00]
    );
}

#[test]
fn packet_header_rejects_bad_magic_and_unknown_type() {
    let mut encoded = PacketHeader::new(PacketType::Ping, 1, 2, 0)
        .encode()
        .unwrap();
    encoded[0] = 0xff;
    assert!(matches!(
        PacketHeader::decode(&encoded),
        Err(CodecError::InvalidMagic { .. })
    ));

    encoded[0..2].copy_from_slice(&MAGIC.to_le_bytes());
    encoded[2] = 0xee;
    assert!(matches!(
        PacketHeader::decode(&encoded),
        Err(CodecError::UnknownPacketType(0xee))
    ));
}

#[test]
fn tcp_frame_writer_matches_vector_and_rejects_invalid_lengths() {
    let packet = PacketHeader::new(PacketType::Ping, 1, 2, 0)
        .encode()
        .unwrap();
    assert_eq!(
        TcpFrameWriter::encode(&packet).unwrap(),
        [
            0x0c, 0x00, 0x00, 0x00, 0x1d, 0xec, 0x07, 0x01, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00,
            0x00, 0x00,
        ]
    );
    assert!(TcpFrameWriter::encode(&[]).is_err());
    assert!(TcpFrameWriter::encode(&vec![0; MAX_TCP_FRAME_SIZE + 1]).is_err());
}

#[test]
fn tcp_frame_reader_buffers_partial_reads_and_splits_coalesced_frames() {
    let first = TcpFrameWriter::encode(b"first").unwrap();
    let second = TcpFrameWriter::encode(b"second").unwrap();
    let mut wire = first.clone();
    wire.extend_from_slice(&second);

    let mut reader = TcpFrameReader::new();
    assert!(reader.push(&wire[..2]).is_empty());
    assert!(reader.push(&wire[2..7]).is_empty());
    assert_eq!(
        reader.push(&wire[7..]),
        vec![
            TcpFrameEvent::Frame(b"first".to_vec()),
            TcpFrameEvent::Frame(b"second".to_vec()),
        ]
    );
    assert_eq!(reader.buffered_len(), 0);
}

#[test]
fn tcp_frame_reader_drops_invalid_buffer_then_resynchronizes() {
    for invalid in [0_u32, (MAX_TCP_FRAME_SIZE as u32) + 1] {
        let mut reader = TcpFrameReader::new();
        let mut bytes = invalid.to_le_bytes().to_vec();
        bytes.extend_from_slice(b"discarded trailing bytes");
        assert_eq!(
            reader.push(&bytes),
            vec![TcpFrameEvent::DroppedInvalidLength(invalid)]
        );
        assert_eq!(reader.buffered_len(), 0);
        assert_eq!(
            reader.push(&TcpFrameWriter::encode(b"resynced").unwrap()),
            vec![TcpFrameEvent::Frame(b"resynced".to_vec())]
        );
    }
}

fn sample_handshake(capabilities: Capabilities) -> Handshake {
    Handshake {
        name: "host-a".into(),
        width: 2560,
        height: 1440,
        scale: 2.0,
        version: PROTOCOL_VERSION,
        capabilities,
        pairing_id: "ABCD-1234".into(),
        session_salt: [0x11; 16],
    }
}

#[test]
fn handshake_v3_round_trip_carries_identity_salt_and_all_caps() {
    let capabilities = Capabilities::STREAM_CONFIGURATION
        | Capabilities::CLIPBOARD_SYNC
        | Capabilities::TEXT_CLIPBOARD_SYNC;
    let handshake = sample_handshake(capabilities);
    assert_round_trip(&handshake);

    let decoded = Handshake::decode(&handshake.encode().unwrap()).unwrap();
    assert!(decoded
        .capabilities
        .contains(Capabilities::STREAM_CONFIGURATION));
    assert!(decoded.capabilities.contains(Capabilities::CLIPBOARD_SYNC));
    assert!(decoded
        .capabilities
        .contains(Capabilities::TEXT_CLIPBOARD_SYNC));

    for capability in [
        Capabilities::STREAM_CONFIGURATION,
        Capabilities::CLIPBOARD_SYNC,
        Capabilities::TEXT_CLIPBOARD_SYNC,
    ] {
        let decoded = Handshake::decode(&sample_handshake(capability).encode().unwrap()).unwrap();
        assert_eq!(decoded.capabilities.bits(), capability.bits());
    }
}

#[test]
fn handshake_preserves_unknown_caps_and_rejects_wrong_version() {
    let handshake = sample_handshake(Capabilities::from_bits_retain(1 << 63));
    assert_eq!(
        Handshake::decode(&handshake.encode().unwrap())
            .unwrap()
            .capabilities
            .bits(),
        1 << 63
    );

    let mut encoded = sample_handshake(Capabilities::empty()).encode().unwrap();
    let version_offset = 2 + "host-a".len() + 2 + 2 + 4;
    encoded[version_offset] = 1;
    assert!(matches!(
        Handshake::decode(&encoded),
        Err(CodecError::UnsupportedVersion(1))
    ));
}

#[test]
fn handshake_rejects_oversized_pairing_id() {
    let mut handshake = sample_handshake(Capabilities::empty());
    handshake.pairing_id = "x".repeat(MAX_PAIRING_ID_BYTES + 1);
    assert!(handshake.encode().is_err());

    let mut encoded = sample_handshake(Capabilities::empty()).encode().unwrap();
    let id_len_offset = 2 + "host-a".len() + 2 + 2 + 4 + 1 + 8;
    encoded[id_len_offset..id_len_offset + 2]
        .copy_from_slice(&((MAX_PAIRING_ID_BYTES + 1) as u16).to_le_bytes());
    assert!(Handshake::decode(&encoded).is_err());
}

#[test]
fn pairing_payloads_round_trip_and_match_swift_request_vector() {
    let request = PairingRequest {
        name: "client-mac".into(),
    };
    assert_eq!(
        request.encode().unwrap(),
        [0x0a, 0x00, b'c', b'l', b'i', b'e', b'n', b't', b'-', b'm', b'a', b'c']
    );
    assert_round_trip(&request);

    let grant = PairingGrant {
        pairing_id: "UUID-1".into(),
        host_name: "host-mac".into(),
        key: [0x42; PAIRING_KEY_SIZE],
    };
    assert_round_trip(&grant);

    for (raw, reason) in PairingRejectReason::ALL.into_iter().enumerate() {
        assert_eq!(reason as usize, raw);
        assert_round_trip(&PairingReject { reason });
    }
}

#[test]
fn pairing_grant_rejects_wrong_key_length() {
    let grant = PairingGrant {
        pairing_id: "UUID-2".into(),
        host_name: "host".into(),
        key: [1; PAIRING_KEY_SIZE],
    };
    let mut encoded = grant.encode().unwrap();
    let key_length_offset = 1 + "UUID-2".len() + 2 + "host".len();
    encoded[key_length_offset] = 31;
    assert!(matches!(
        PairingGrant::decode(&encoded),
        Err(CodecError::InvalidLength {
            field: "pairing key",
            ..
        })
    ));
}

#[test]
fn input_event_round_trips_every_type_and_modifier_bit() {
    let modifiers = Modifiers::SHIFT
        | Modifiers::CONTROL
        | Modifiers::OPTION
        | Modifiers::COMMAND
        | Modifiers::CAPS_LOCK;
    for (raw, event_type) in InputEventType::ALL.into_iter().enumerate() {
        assert_eq!(event_type as usize, raw);
        let event = InputEvent {
            event_type,
            x: 0.5,
            y: 0.75,
            key_code: 36,
            modifiers,
            scroll_dx: 1.5,
            scroll_dy: -2.0,
        };
        assert_round_trip(&event);
        assert_eq!(event.encode().unwrap().len(), InputEvent::SIZE);
    }
    assert!(modifiers.contains(Modifiers::SHIFT));
    assert!(modifiers.contains(Modifiers::CONTROL));
    assert!(modifiers.contains(Modifiers::OPTION));
    assert!(modifiers.contains(Modifiers::COMMAND));
    assert!(modifiers.contains(Modifiers::CAPS_LOCK));
}

#[test]
fn input_event_supports_agent_control_types() {
    assert_eq!(
        InputEventType::try_from(11).unwrap(),
        InputEventType::MiddleMouseDown
    );
    assert_eq!(
        InputEventType::try_from(12).unwrap(),
        InputEventType::MiddleMouseUp
    );
    assert_eq!(InputEventType::try_from(13).unwrap(), InputEventType::Reset);
    assert_eq!(
        InputEventType::try_from(14).unwrap(),
        InputEventType::RelativeMove
    );
    assert_eq!(
        InputEventType::try_from(15).unwrap(),
        InputEventType::GamepadAxis
    );
    assert_eq!(
        InputEventType::try_from(16).unwrap(),
        InputEventType::GamepadButtonDown
    );
    assert_eq!(
        InputEventType::try_from(17).unwrap(),
        InputEventType::GamepadButtonUp
    );
    assert_eq!(
        InputEventType::try_from(18).unwrap(),
        InputEventType::PenMove
    );
    assert_eq!(
        InputEventType::try_from(19).unwrap(),
        InputEventType::PenDown
    );
    assert_eq!(InputEventType::try_from(20).unwrap(), InputEventType::PenUp);

    // Pen helpers
    let pen = InputEvent::pen_down(0.5, 0.5, 0.8, 15.0, -10.0);
    assert_eq!(pen.event_type, InputEventType::PenDown);
    assert_eq!(pen.scroll_dx, 0.8);
    assert_eq!(pen.scroll_dy, 15.0);
    assert_round_trip(&pen);

    // Gamepad helpers
    let gp_axis = InputEvent::gamepad_axis(0, 1, 0.5, -0.5);
    assert_eq!(gp_axis.event_type, InputEventType::GamepadAxis);
    assert_round_trip(&gp_axis);

    let gp_btn = InputEvent::gamepad_button(1, 4, true);
    assert_eq!(gp_btn.event_type, InputEventType::GamepadButtonDown);
    assert_round_trip(&gp_btn);
}

#[test]
fn input_normalization_clamps_and_flips_bottom_left_y() {
    assert_eq!(
        normalize_client_coordinates(50.0, 25.0, 100.0, 100.0),
        (0.5, 0.75)
    );
    assert_eq!(
        normalize_client_coordinates(-5.0, 150.0, 100.0, 100.0),
        (0.0, 0.0)
    );
    assert_eq!(
        map_to_host_pixels(0.5, 0.25, 1920.0, 1080.0),
        (960.0, 270.0)
    );
}

fn sample_configuration() -> StreamConfiguration {
    StreamConfiguration {
        width: 3440,
        height: 1440,
        bitrate: 9_000_000,
        frames_per_second: 90,
    }
}

#[test]
fn every_control_payload_body_round_trips() {
    assert_round_trip(&BitrateAdjust {
        target_bitrate: -1_234_567,
    });
    assert_round_trip(&sample_configuration());
    assert_round_trip(&StreamConfigurationRequest {
        request_id: 42,
        desired: sample_configuration(),
    });
    assert_round_trip(&StreamConfigurationResponse {
        request_id: 42,
        active: sample_configuration(),
    });
    assert_round_trip(&StreamConfigurationReject {
        request_id: 7,
        reason: StreamConfigurationErrorCode::UnsupportedDimensions,
        message: "too large".into(),
    });
    assert_round_trip(&StreamConfigurationError {
        request_id: 8,
        error_code: StreamConfigurationErrorCode::InvalidRequest,
        message: "bad request".into(),
    });
    assert_round_trip(&ClipboardSyncRequest {
        request_id: 11,
        direction: ClipboardSyncDirection::Bidirectional,
        origin: ClipboardSyncOrigin::LocalPasteboard,
    });
    assert_round_trip(&ClipboardSyncUpdate {
        request_id: 12,
        direction: ClipboardSyncDirection::HostToClient,
        origin: ClipboardSyncOrigin::RemotePasteboard,
        text: "hello clipboard".into(),
    });
    assert_round_trip(&ClipboardSyncError {
        request_id: 13,
        direction: ClipboardSyncDirection::ClientToHost,
        origin: ClipboardSyncOrigin::SyncedFromPeer,
        error_code: 9,
        message: "clipboard rejected".into(),
    });
}

#[test]
fn every_control_message_type_round_trips() {
    let messages = [
        ControlMessage::RequestKeyFrame,
        ControlMessage::StartStream,
        ControlMessage::StopStream,
        ControlMessage::Disconnect,
        ControlMessage::Ping,
        ControlMessage::Pong,
        ControlMessage::BitrateAdjust(BitrateAdjust {
            target_bitrate: 8_000_000,
        }),
        ControlMessage::StreamConfigRequest(StreamConfigurationRequest {
            request_id: 1,
            desired: sample_configuration(),
        }),
        ControlMessage::StreamConfigResponse(StreamConfigurationResponse {
            request_id: 1,
            active: sample_configuration(),
        }),
        ControlMessage::StreamConfigReject(StreamConfigurationReject {
            request_id: 1,
            reason: StreamConfigurationErrorCode::RejectedByPeer,
            message: "no".into(),
        }),
        ControlMessage::StreamConfigError(StreamConfigurationError {
            request_id: 1,
            error_code: StreamConfigurationErrorCode::UnsupportedFps,
            message: "fps".into(),
        }),
        ControlMessage::ClipboardSyncRequest(ClipboardSyncRequest {
            request_id: 2,
            direction: ClipboardSyncDirection::Bidirectional,
            origin: ClipboardSyncOrigin::LocalPasteboard,
        }),
        ControlMessage::ClipboardSyncUpdate(ClipboardSyncUpdate {
            request_id: 2,
            direction: ClipboardSyncDirection::HostToClient,
            origin: ClipboardSyncOrigin::RemotePasteboard,
            text: "hello".into(),
        }),
        ControlMessage::ClipboardSyncError(ClipboardSyncError {
            request_id: 2,
            direction: ClipboardSyncDirection::ClientToHost,
            origin: ClipboardSyncOrigin::SyncedFromPeer,
            error_code: 3,
            message: "error".into(),
        }),
    ];

    for (raw, (expected_type, message)) in ControlMessageType::ALL
        .into_iter()
        .zip(messages)
        .enumerate()
    {
        assert_eq!(expected_type as usize, raw);
        assert_eq!(message.message_type(), expected_type);
        assert_eq!(message.encode().unwrap()[0], expected_type as u8);
        assert_round_trip(&message);
    }
}

#[test]
fn control_payloads_match_little_endian_layouts() {
    assert_eq!(
        BitrateAdjust { target_bitrate: -2 }.encode().unwrap(),
        (-2_i32).to_le_bytes()
    );
    assert_eq!(
        StreamConfigurationRequest {
            request_id: 0x0102_0304,
            desired: StreamConfiguration {
                width: 1,
                height: 2,
                bitrate: 3,
                frames_per_second: 4,
            },
        }
        .encode()
        .unwrap(),
        [0x04, 0x03, 0x02, 0x01, 1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0, 4, 0,]
    );
}

#[test]
fn control_rejects_unknown_message_and_payload_enum_values() {
    assert!(matches!(
        ControlMessage::decode(&[0xff]),
        Err(CodecError::UnknownControlMessageType(0xff))
    ));
    // Bodyless messages must consume exactly one byte.
    assert!(matches!(
        ControlMessage::decode(&[ControlMessageType::Ping as u8, 0xaa]),
        Err(CodecError::TrailingBytes { .. })
    ));
    assert_eq!(
        ControlMessage::decode(&[ControlMessageType::Ping as u8]).unwrap(),
        ControlMessage::Ping
    );

    let mut request = ClipboardSyncRequest {
        request_id: 1,
        direction: ClipboardSyncDirection::Bidirectional,
        origin: ClipboardSyncOrigin::LocalPasteboard,
    }
    .encode()
    .unwrap();
    request[4] = 0xff;
    assert!(ClipboardSyncRequest::decode(&request).is_err());
}

#[test]
fn clipboard_update_rejects_more_than_four_kib_utf8() {
    let update = ClipboardSyncUpdate {
        request_id: 1,
        direction: ClipboardSyncDirection::HostToClient,
        origin: ClipboardSyncOrigin::LocalPasteboard,
        text: "a".repeat(MAX_CLIPBOARD_TEXT_BYTES + 1),
    };
    assert!(update.encode().is_err());
}

#[test]
fn video_payloads_round_trip_and_enforce_caps() {
    let header = FrameHeader {
        frame_id: 999,
        width: 3840,
        height: 2160,
        is_key_frame: true,
        // 12 chunks can carry at most 12 * MAX_VIDEO_CHUNK_BYTES; this size
        // needs 362 chunks (362 * 1382 = 500284 >= 500000).
        total_chunks: 362,
        total_size: 500_000,
    };
    assert_round_trip(&header);
    assert_eq!(header.encode().unwrap().len(), FrameHeader::SIZE);

    let chunk = FrameChunk {
        frame_id: 42,
        chunk_index: 3,
        data: vec![0xab; 1024],
    };
    assert_round_trip(&chunk);

    let mut invalid = header.clone();
    invalid.total_chunks = MAX_CHUNKS_PER_FRAME + 1;
    assert!(invalid.encode().is_err());
    invalid = header.clone();
    invalid.total_size = MAX_FRAME_BYTES + 1;
    assert!(invalid.encode().is_err());

    let mut malformed = header.encode().unwrap();
    malformed[9..11].copy_from_slice(&(MAX_CHUNKS_PER_FRAME + 1).to_le_bytes());
    assert!(FrameHeader::decode(&malformed).is_err());
    malformed = header.encode().unwrap();
    malformed[12..16].copy_from_slice(&(MAX_FRAME_BYTES + 1).to_le_bytes());
    assert!(FrameHeader::decode(&malformed).is_err());

    let oversized_chunk = FrameChunk {
        frame_id: 1,
        chunk_index: 0,
        data: vec![0; MAX_VIDEO_CHUNK_BYTES + 1],
    };
    assert!(oversized_chunk.encode().is_err());
}

#[test]
fn cursor_and_audio_payloads_round_trip() {
    let cursor = CursorUpdate {
        x: 0.333,
        y: 0.666,
        cursor_type: 2,
    };
    assert_round_trip(&cursor);
    assert_eq!(cursor.encode().unwrap().len(), CursorUpdate::SIZE);

    let header = AudioFragmentHeader {
        frame_id: 123,
        fragment_index: 1,
        fragment_count: 3,
    };
    assert_round_trip(&header);
    let fragment = AudioFragment {
        header,
        data: vec![0x55; 512],
    };
    assert_round_trip(&fragment);
}

#[test]
fn audio_fragment_rejects_invalid_index_count_and_data_length() {
    for header in [
        AudioFragmentHeader {
            frame_id: 1,
            fragment_index: 0,
            fragment_count: 0,
        },
        AudioFragmentHeader {
            frame_id: 1,
            fragment_index: 2,
            fragment_count: 2,
        },
    ] {
        assert!(header.encode().is_err());
    }

    let oversized = AudioFragment {
        header: AudioFragmentHeader {
            frame_id: 1,
            fragment_index: 0,
            fragment_count: 1,
        },
        data: vec![0; MAX_AUDIO_FRAGMENT_BYTES + 1],
    };
    assert!(oversized.encode().is_err());
}

#[test]
fn invalid_utf8_is_rejected() {
    assert!(PairingRequest::decode(&[1, 0, 0xff]).is_err());
}

macro_rules! truncation_test {
    ($name:ident, $ty:ty, $value:expr) => {
        #[test]
        fn $name() {
            let value: $ty = $value;
            let bytes = value.encode().expect("canonical value encodes");
            assert!(!bytes.is_empty());
            for cut in 0..bytes.len() {
                let result = <$ty>::decode(&bytes[..cut]);
                assert!(
                    result.is_err(),
                    "{} accepted truncation at {cut}/{}: {result:?}",
                    stringify!($ty),
                    bytes.len()
                );
            }
        }
    };
}

truncation_test!(
    truncated_packet_header,
    PacketHeader,
    PacketHeader::new(PacketType::Ping, 1, 2, 0)
);
truncation_test!(
    truncated_handshake,
    Handshake,
    sample_handshake(Capabilities::all())
);
truncation_test!(
    truncated_pairing_request,
    PairingRequest,
    PairingRequest {
        name: "client".into()
    }
);
truncation_test!(
    truncated_pairing_grant,
    PairingGrant,
    PairingGrant {
        pairing_id: "id".into(),
        host_name: "host".into(),
        key: [7; PAIRING_KEY_SIZE]
    }
);
truncation_test!(
    truncated_pairing_reject,
    PairingReject,
    PairingReject {
        reason: PairingRejectReason::LockedOut
    }
);
truncation_test!(
    truncated_input_event,
    InputEvent,
    InputEvent {
        event_type: InputEventType::KeyDown,
        x: 0.0,
        y: 0.0,
        key_code: 36,
        modifiers: Modifiers::COMMAND,
        scroll_dx: 0.0,
        scroll_dy: 0.0
    }
);
truncation_test!(
    truncated_bitrate_adjust,
    BitrateAdjust,
    BitrateAdjust {
        target_bitrate: 8_000_000
    }
);
truncation_test!(
    truncated_stream_configuration,
    StreamConfiguration,
    sample_configuration()
);
truncation_test!(
    truncated_stream_configuration_request,
    StreamConfigurationRequest,
    StreamConfigurationRequest {
        request_id: 1,
        desired: sample_configuration()
    }
);
truncation_test!(
    truncated_stream_configuration_response,
    StreamConfigurationResponse,
    StreamConfigurationResponse {
        request_id: 1,
        active: sample_configuration()
    }
);
truncation_test!(
    truncated_stream_configuration_reject,
    StreamConfigurationReject,
    StreamConfigurationReject {
        request_id: 1,
        reason: StreamConfigurationErrorCode::RejectedByPeer,
        message: "no".into()
    }
);
truncation_test!(
    truncated_stream_configuration_error,
    StreamConfigurationError,
    StreamConfigurationError {
        request_id: 1,
        error_code: StreamConfigurationErrorCode::InvalidRequest,
        message: "error".into()
    }
);
truncation_test!(
    truncated_clipboard_sync_request,
    ClipboardSyncRequest,
    ClipboardSyncRequest {
        request_id: 1,
        direction: ClipboardSyncDirection::Bidirectional,
        origin: ClipboardSyncOrigin::LocalPasteboard
    }
);
truncation_test!(
    truncated_clipboard_sync_update,
    ClipboardSyncUpdate,
    ClipboardSyncUpdate {
        request_id: 1,
        direction: ClipboardSyncDirection::HostToClient,
        origin: ClipboardSyncOrigin::RemotePasteboard,
        text: "hello".into()
    }
);
truncation_test!(
    truncated_clipboard_sync_error,
    ClipboardSyncError,
    ClipboardSyncError {
        request_id: 1,
        direction: ClipboardSyncDirection::ClientToHost,
        origin: ClipboardSyncOrigin::SyncedFromPeer,
        error_code: 3,
        message: "error".into()
    }
);
truncation_test!(
    truncated_control_message,
    ControlMessage,
    ControlMessage::ClipboardSyncUpdate(ClipboardSyncUpdate {
        request_id: 1,
        direction: ClipboardSyncDirection::HostToClient,
        origin: ClipboardSyncOrigin::RemotePasteboard,
        text: "hello".into()
    })
);
truncation_test!(
    truncated_frame_header,
    FrameHeader,
    FrameHeader {
        frame_id: 1,
        width: 1920,
        height: 1080,
        is_key_frame: true,
        total_chunks: 2,
        total_size: 1024
    }
);
truncation_test!(
    truncated_frame_chunk_header,
    FrameChunk,
    FrameChunk {
        frame_id: 1,
        chunk_index: 0,
        data: Vec::new()
    }
);
truncation_test!(
    truncated_cursor_update,
    CursorUpdate,
    CursorUpdate {
        x: 0.5,
        y: 0.5,
        cursor_type: 0
    }
);
truncation_test!(
    truncated_audio_fragment_header,
    AudioFragmentHeader,
    AudioFragmentHeader {
        frame_id: 1,
        fragment_index: 0,
        fragment_count: 1
    }
);
truncation_test!(
    truncated_audio_fragment,
    AudioFragment,
    AudioFragment {
        header: AudioFragmentHeader {
            frame_id: 1,
            fragment_index: 0,
            fragment_count: 1
        },
        data: Vec::new()
    }
);
truncation_test!(
    truncated_color_metadata,
    ColorMetadata,
    ColorMetadata {
        range: ColorRange::Full,
        matrix: ColorMatrix::Bt2020,
        chroma: ChromaSubsampling::Yuv444,
    }
);

#[test]
fn color_metadata_round_trips_and_rejects_unknown_enums() {
    let meta = ColorMetadata {
        range: ColorRange::Full,
        matrix: ColorMatrix::Bt709,
        chroma: ChromaSubsampling::Yuv444,
    };
    assert_round_trip(&meta);

    let default_meta = ColorMetadata::default();
    assert_round_trip(&default_meta);
    assert_eq!(default_meta.range, ColorRange::Limited);
    assert_eq!(default_meta.matrix, ColorMatrix::Bt709);
    assert_eq!(default_meta.chroma, ChromaSubsampling::Yuv420);

    assert!(matches!(
        ColorMetadata::decode(&[2, 0, 0]),
        Err(CodecError::UnknownColorRange(2))
    ));
    assert!(matches!(
        ColorMetadata::decode(&[0, 5, 0]),
        Err(CodecError::UnknownColorMatrix(5))
    ));
    assert!(matches!(
        ColorMetadata::decode(&[0, 0, 9]),
        Err(CodecError::UnknownChromaSubsampling(9))
    ));
}

#[test]
fn capabilities_color_444_round_trip() {
    let caps = Capabilities::STREAM_CONFIGURATION
        | Capabilities::CLIPBOARD_SYNC
        | Capabilities::TEXT_CLIPBOARD_SYNC
        | Capabilities::COLOR_444
        | Capabilities::COLOR_HDR
        | Capabilities::GAMEPAD
        | Capabilities::PEN_INPUT;
    assert!(caps.contains(Capabilities::COLOR_444));
    assert!(caps.contains(Capabilities::COLOR_HDR));
    assert!(caps.contains(Capabilities::GAMEPAD));
    assert!(caps.contains(Capabilities::PEN_INPUT));

    let handshake = sample_handshake(caps);
    assert_round_trip(&handshake);
    let decoded = Handshake::decode(&handshake.encode().unwrap()).unwrap();
    assert!(decoded.capabilities.contains(Capabilities::COLOR_444));
    assert!(decoded.capabilities.contains(Capabilities::COLOR_HDR));
    assert!(decoded.capabilities.contains(Capabilities::GAMEPAD));
    assert!(decoded.capabilities.contains(Capabilities::PEN_INPUT));
}

#[test]
fn capabilities_authenticated_udp_registration_round_trip() {
    let caps = Capabilities::AUTHENTICATED_UDP_REGISTRATION;
    assert_eq!(caps.bits(), 1 << 7);
    assert!(Capabilities::all().contains(Capabilities::AUTHENTICATED_UDP_REGISTRATION));

    let handshake = sample_handshake(caps);
    assert_round_trip(&handshake);
    let decoded = Handshake::decode(&handshake.encode().unwrap()).unwrap();
    assert!(decoded
        .capabilities
        .contains(Capabilities::AUTHENTICATED_UDP_REGISTRATION));
}
