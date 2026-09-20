//! Unit and roundtrip tests for Remote Browser Screencast protocol codec and DTOs
//! Authoritative Spec: docs/plans/REMOTE_BROWSER_SCREENCAST_PLAN_2026-09-17.md (§4.1, §4.2, §7.1, Phase 7A)

use super::browser_protocol::*;
use super::browser_ws::BrowserWsSession;
use crate::remote::auth::DevicePermission;
use std::time::Instant;

fn sample_test_png() -> &'static [u8] {
    &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG magic
        0x00, 0x00, 0x00, 0x0D, // IHDR length = 13
        0x49, 0x48, 0x44, 0x52, // "IHDR"
        0x00, 0x00, 0x00, 0x01, // width = 1
        0x00, 0x00, 0x00, 0x01, // height = 1
        0x08, 0x06, 0x00, 0x00, 0x00, // 8-bit truecolor+alpha
        0x1F, 0x15, 0xC4, 0x89, // CRC
        0x00, 0x00, 0x00, 0x0A, // IDAT length = 10
        0x49, 0x44, 0x41, 0x54, // "IDAT"
        0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, // zlib payload
        0x0D, 0x0A, 0x2D, 0xB4, // CRC
        0x00, 0x00, 0x00, 0x00, // IEND length = 0
        0x49, 0x45, 0x4E, 0x44, // "IEND"
        0xAE, 0x42, 0x60, 0x82, // CRC
    ]
}

fn sample_test_jpeg() -> &'static [u8] {
    &[
        0xFF, 0xD8, // SOI
        0xFF, 0xC0, // SOF0
        0x00, 0x11, // length = 17
        0x08, // precision = 8
        0x00, 0x01, // height = 1
        0x00, 0x01, // width = 1
        0x03, // 3 components
        0x01, 0x11, 0x00, 0x02, 0x11, 0x00, 0x03, 0x11, 0x00, 0xFF, 0xDA, // SOS
        0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00, 0xFF, 0xD9, // EOI
    ]
}

fn valid_test_metadata() -> BrowserFrameMetadata {
    BrowserFrameMetadata {
        offset_top: 0.0,
        page_scale_factor: 1.0,
        device_width: 1280.0,
        device_height: 800.0,
        image_width: 1,
        image_height: 1,
        scroll_offset_x: 0.0,
        scroll_offset_y: 0.0,
        timestamp: 1726560000.0,
        stream_id: 1,
        browser_instance_id: "bi_test_123".into(),
        browser_service_epoch: "42".into(),
        desktop_epoch: "7".into(),
        document_generation: "15".into(),
        viewport_revision: "3".into(),
        capture_rect: BrowserCaptureRect {
            x: 0.0,
            y: 0.0,
            width: 1280.0,
            height: 800.0,
        },
        geometry_source: "wkSnapshot".into(),
    }
}

// ---------------------------------------------------------------------------
// 16-byte binary envelope tests
// ---------------------------------------------------------------------------

#[test]
fn test_envelope_header_boundary_rejection() {
    // 0 to 15 bytes must be rejected with BufferTooShort
    for len in 0..HEADER_BYTE_LENGTH {
        let truncated_buf = vec![0u8; len];
        let err = decode_binary_frame(&truncated_buf).expect_err("Lengths 0..15 must be rejected");
        assert_eq!(
            err,
            ProtocolCodecError::BufferTooShort {
                expected: HEADER_BYTE_LENGTH,
                actual: len,
            }
        );
    }
}

#[test]
fn test_envelope_field_corruption_rejection() {
    let meta = valid_test_metadata();
    let encoded =
        encode_binary_frame(BrowserImageFormat::Png, 1, &meta, sample_test_png()).unwrap();

    // 1. Invalid Kind (byte 0: must be 0x62)
    let mut bad_kind = encoded.clone();
    bad_kind[0] = 0x61;
    assert_eq!(
        decode_binary_frame(&bad_kind).unwrap_err(),
        ProtocolCodecError::InvalidKind(0x61)
    );

    // 2. Invalid Version (byte 1: must be 1)
    let mut bad_ver = encoded.clone();
    bad_ver[1] = 2;
    assert_eq!(
        decode_binary_frame(&bad_ver).unwrap_err(),
        ProtocolCodecError::InvalidVersion(2)
    );

    // 3. Invalid Opcode (byte 2: must be 1)
    let mut bad_opcode = encoded.clone();
    bad_opcode[2] = 0;
    assert_eq!(
        decode_binary_frame(&bad_opcode).unwrap_err(),
        ProtocolCodecError::InvalidOpcode(0)
    );

    // 4. Invalid Format (byte 3: must be 1 or 2)
    let mut bad_format = encoded.clone();
    bad_format[3] = 3;
    assert_eq!(
        decode_binary_frame(&bad_format).unwrap_err(),
        ProtocolCodecError::InvalidFormat(3)
    );

    // 5. Reserved non-zero (bytes 12..16: must be 0)
    let mut bad_reserved = encoded.clone();
    bad_reserved[12] = 1;
    assert_eq!(
        decode_binary_frame(&bad_reserved).unwrap_err(),
        ProtocolCodecError::ReservedNonZero(1)
    );
}

#[test]
fn test_big_endian_dimension_verification_and_mismatch_rejection() {
    let mut meta = valid_test_metadata();
    meta.image_width = 999; // Intentionally mismatch image payload dimension (1x1)
    meta.image_height = 999;

    // JPEG dimension mismatch
    let err_jpeg = encode_binary_frame(BrowserImageFormat::Jpeg, 1, &meta, sample_test_jpeg());
    assert!(err_jpeg.is_ok()); // Encoding valid metadata succeeds
    let decode_jpeg_err = decode_binary_frame(&err_jpeg.unwrap()).unwrap_err();
    assert_eq!(
        decode_jpeg_err,
        ProtocolCodecError::DimensionMismatch {
            header_w: 1,
            header_h: 1,
            meta_w: 999,
            meta_h: 999,
        }
    );

    // PNG dimension mismatch
    let err_png = encode_binary_frame(BrowserImageFormat::Png, 1, &meta, sample_test_png());
    assert!(err_png.is_ok());
    let decode_png_err = decode_binary_frame(&err_png.unwrap()).unwrap_err();
    assert_eq!(
        decode_png_err,
        ProtocolCodecError::DimensionMismatch {
            header_w: 1,
            header_h: 1,
            meta_w: 999,
            meta_h: 999,
        }
    );
}

#[test]
fn test_size_limits_image_and_metadata_and_pixels() {
    // 1. Image byte cap 2 MiB
    let meta = valid_test_metadata();
    let oversized_image = vec![0u8; MAX_FRAME_PAYLOAD_BYTES + 1];
    let err_oversized = encode_binary_frame(BrowserImageFormat::Png, 1, &meta, &oversized_image);
    assert!(matches!(
        err_oversized,
        Err(ProtocolCodecError::BufferTooLarge { .. })
    ));

    // 2. Max edge dimension: 2048 px cap
    let mut edge_meta = valid_test_metadata();
    edge_meta.image_width = 2049;
    assert_eq!(
        encode_binary_frame(BrowserImageFormat::Png, 1, &edge_meta, sample_test_png()).unwrap_err(),
        ProtocolCodecError::ImageEdgeExceeded {
            width: 2049,
            height: 1,
        }
    );

    // 3. Max pixels: 4 MP cap (2000 x 2001 = 4,002,000 > 4,000,000)
    let mut pixel_meta = valid_test_metadata();
    pixel_meta.image_width = 2000;
    pixel_meta.image_height = 2001;
    assert_eq!(
        encode_binary_frame(BrowserImageFormat::Png, 1, &pixel_meta, sample_test_png())
            .unwrap_err(),
        ProtocolCodecError::ImagePixelsExceeded { pixels: 4_002_000 }
    );
}

#[test]
fn test_ferryx_extension_fields_and_geometry_source() {
    // 1. geometry_source must be exactly "wkSnapshot"
    let mut meta = valid_test_metadata();
    meta.geometry_source = "cdpSnapshot".into();
    let err =
        encode_binary_frame(BrowserImageFormat::Png, 1, &meta, sample_test_png()).unwrap_err();
    assert_eq!(
        err,
        ProtocolCodecError::InvalidGeometrySource("cdpSnapshot".into())
    );

    // 2. Deserializing metadata without required Ferryx fields fails
    let missing_field_json = r#"{
        "offsetTop": 0.0,
        "pageScaleFactor": 1.0,
        "deviceWidth": 800.0,
        "deviceHeight": 600.0,
        "imageWidth": 1,
        "imageHeight": 1,
        "scrollOffsetX": 0.0,
        "scrollOffsetY": 0.0,
        "timestamp": 1726560000.0
    }"#;
    let res: Result<BrowserFrameMetadata, _> = serde_json::from_str(missing_field_json);
    assert!(
        res.is_err(),
        "Missing Ferryx extension fields must fail deserialization"
    );
}

#[test]
fn test_decimal_string_u64_validation() {
    let meta = valid_test_metadata();

    // Verify all 4 epoch/generation fields parse into valid u64
    let s_epoch: u64 = meta
        .browser_service_epoch
        .parse()
        .expect("browser_service_epoch is u64");
    assert_eq!(s_epoch, 42);

    let d_epoch: u64 = meta.desktop_epoch.parse().expect("desktop_epoch is u64");
    assert_eq!(d_epoch, 7);

    let doc_gen: u64 = meta
        .document_generation
        .parse()
        .expect("document_generation is u64");
    assert_eq!(doc_gen, 15);

    let vp_rev: u64 = meta
        .viewport_revision
        .parse()
        .expect("viewport_revision is u64");
    assert_eq!(vp_rev, 3);

    // Explicit test that non-decimal string viewport_revision is rejected during validation
    for bad_vp in ["", "invalid_3.5", "-1", "0x10", "12a", " ", "1.0"] {
        let mut invalid_meta = valid_test_metadata();
        invalid_meta.viewport_revision = bad_vp.into();
        let err = encode_binary_frame(BrowserImageFormat::Png, 1, &invalid_meta, sample_test_png())
            .unwrap_err();
        assert_eq!(
            err,
            ProtocolCodecError::InvalidDecimalString("viewport_revision"),
            "Expected InvalidDecimalString for viewport_revision '{bad_vp}'"
        );
    }
}

#[test]
fn test_max_image_pixels_boundary() {
    assert_eq!(
        MAX_IMAGE_PIXELS, 4_000_000,
        "MAX_IMAGE_PIXELS must equal 4 MP exactly"
    );

    // 2000 x 2000 = 4,000,000 pixels (within limit)
    let mut boundary_meta = valid_test_metadata();
    boundary_meta.image_width = 2000;
    boundary_meta.image_height = 2000;
    assert!(validate_metadata(&boundary_meta).is_ok());

    // 2000 x 2001 = 4,002,000 pixels (> 4,000,000 cap) -> ImagePixelsExceeded
    boundary_meta.image_height = 2001;
    let err = validate_metadata(&boundary_meta).unwrap_err();
    assert_eq!(
        err,
        ProtocolCodecError::ImagePixelsExceeded { pixels: 4_002_000 }
    );
}

#[test]
fn test_client_to_server_binary_rejection() {
    let mut session = BrowserWsSession::new(
        "conn-test".into(),
        "dev-test".into(),
        "b-test".into(),
        DevicePermission::Control,
        Instant::now(),
    );

    // Binary frame sent from client to server must be explicitly rejected
    let dummy_frame = vec![PROTOCOL_KIND, PROTOCOL_VERSION, OPCODE_FRAME, FORMAT_PNG];
    let err = session.handle_client_binary(&dummy_frame);
    assert!(err.is_err());
    assert!(
        err.unwrap_err().contains("rejected"),
        "Client binary frame must be rejected"
    );
}

// ---------------------------------------------------------------------------
// JSON Protocol DTOs: strict camelCase and deny_unknown_fields tests
// ---------------------------------------------------------------------------

#[test]
fn test_json_dto_camel_case_and_deny_unknown_fields() {
    // 1. ClientMessage::BrowserSubscribe valid camelCase
    let valid_sub = r#"{
        "type": "browserSubscribe",
        "requestId": "r1",
        "viewerInstanceId": "v1",
        "options": {
            "format": "jpeg",
            "quality": 75,
            "intervalMs": 250,
            "maxEdge": 1280
        }
    }"#;
    let msg: ClientMessage =
        serde_json::from_str(valid_sub).expect("Valid subscribe deserialization");
    assert!(matches!(msg, ClientMessage::BrowserSubscribe { .. }));

    // 2. Unknown field in ClientMessage must be rejected (deny_unknown_fields)
    let bad_sub = r#"{
        "type": "browserSubscribe",
        "requestId": "r1",
        "viewerInstanceId": "v1",
        "unknownProperty": "malicious",
        "options": {
            "format": "jpeg"
        }
    }"#;
    let err_sub: Result<ClientMessage, _> = serde_json::from_str(bad_sub);
    assert!(
        err_sub.is_err(),
        "Unknown field in ClientMessage must be rejected"
    );

    // 3. Unknown field in options must be rejected
    let bad_opts = r#"{
        "type": "browserSubscribe",
        "requestId": "r1",
        "viewerInstanceId": "v1",
        "options": {
            "format": "jpeg",
            "extraParam": 123
        }
    }"#;
    let err_opts: Result<ClientMessage, _> = serde_json::from_str(bad_opts);
    assert!(
        err_opts.is_err(),
        "Unknown field in BrowserSubscribeOptions must be rejected"
    );

    // 4. ServerMessage::BrowserHello valid camelCase
    let valid_hello = r#"{
        "type": "browserHello",
        "browserId": "b1",
        "browserInstanceId": "bi1",
        "browserServiceEpoch": "1",
        "desktopEpoch": "1",
        "protocolVersion": 1,
        "supportedCommands": ["navigate", "click"]
    }"#;
    let s_msg: ServerMessage =
        serde_json::from_str(valid_hello).expect("Valid hello deserialization");
    assert!(matches!(s_msg, ServerMessage::BrowserHello { .. }));

    // 5. Unknown field in ServerMessage must be rejected
    let bad_hello = r#"{
        "type": "browserHello",
        "browserId": "b1",
        "browserInstanceId": "bi1",
        "browserServiceEpoch": "1",
        "desktopEpoch": "1",
        "protocolVersion": 1,
        "supportedCommands": [],
        "extraLeak": "internal-path"
    }"#;
    let err_hello: Result<ServerMessage, _> = serde_json::from_str(bad_hello);
    assert!(
        err_hello.is_err(),
        "Unknown field in ServerMessage must be rejected"
    );

    // 6. ClientMessage::BrowserCommand valid camelCase
    let valid_cmd = r#"{
        "type": "browserCommand",
        "requestId": "cmd-1",
        "requestSeq": "10",
        "browserId": "b1",
        "leaseEpoch": "42",
        "browserInstanceId": "bi1",
        "desktopEpoch": "7",
        "documentGeneration": "15",
        "command": "click",
        "params": { "reference": "btn1" }
    }"#;
    let cmd_msg: ClientMessage =
        serde_json::from_str(valid_cmd).expect("Valid command deserialization");
    assert!(matches!(cmd_msg, ClientMessage::BrowserCommand { .. }));

    // 7. ClientMessage::BrowserPause valid camelCase and unknown field rejection
    // R4-10: pause/resume carry the client's (browserId, streamId) pair.
    let valid_pause = r#"{ "type": "browserPause", "browserId": "b1", "streamId": 1 }"#;
    let pause_msg: ClientMessage =
        serde_json::from_str(valid_pause).expect("Valid pause deserialization");
    assert!(matches!(pause_msg, ClientMessage::BrowserPause { .. }));

    let bad_pause =
        r#"{ "type": "browserPause", "browserId": "b1", "streamId": 1, "unknownExtra": 123 }"#;
    assert!(serde_json::from_str::<ClientMessage>(bad_pause).is_err());

    // 8. ClientMessage::BrowserResume valid camelCase and unknown field rejection
    let valid_resume = r#"{ "type": "browserResume", "browserId": "b1", "streamId": 1 }"#;
    let resume_msg: ClientMessage =
        serde_json::from_str(valid_resume).expect("Valid resume deserialization");
    assert!(matches!(resume_msg, ClientMessage::BrowserResume { .. }));

    let bad_resume =
        r#"{ "type": "browserResume", "browserId": "b1", "streamId": 1, "unknownExtra": 456 }"#;
    assert!(serde_json::from_str::<ClientMessage>(bad_resume).is_err());
}

// ---------------------------------------------------------------------------
// Golden byte roundtrip test
// ---------------------------------------------------------------------------

#[test]
fn test_golden_byte_roundtrip() {
    let meta = valid_test_metadata();
    let png_bytes = sample_test_png();
    let seq = 42u32;

    let encoded = encode_binary_frame(BrowserImageFormat::Png, seq, &meta, png_bytes)
        .expect("Encoding golden frame");

    // Header layout verification (exact 16 bytes)
    assert_eq!(encoded[0], 0x62, "kind must be 0x62");
    assert_eq!(encoded[1], 1, "version must be 1");
    assert_eq!(encoded[2], 1, "opcode must be 1 (Frame)");
    assert_eq!(encoded[3], 2, "format must be 2 (PNG)");

    let decoded_seq = u32::from_le_bytes([encoded[4], encoded[5], encoded[6], encoded[7]]);
    assert_eq!(decoded_seq, 42);

    let meta_len = u32::from_le_bytes([encoded[8], encoded[9], encoded[10], encoded[11]]) as usize;
    let reserved = u32::from_le_bytes([encoded[12], encoded[13], encoded[14], encoded[15]]);
    assert_eq!(reserved, 0, "reserved must be 0");

    // Metadata payload verification
    let meta_slice = &encoded[16..16 + meta_len];
    let decoded_meta: BrowserFrameMetadata = serde_json::from_slice(meta_slice)
        .expect("Metadata slice must parse to BrowserFrameMetadata");
    assert_eq!(decoded_meta.stream_id, meta.stream_id);
    assert_eq!(decoded_meta.browser_instance_id, meta.browser_instance_id);
    assert_eq!(decoded_meta.geometry_source, "wkSnapshot");

    // Image payload verification
    let image_slice = &encoded[16 + meta_len..];
    assert_eq!(image_slice, png_bytes);

    // Full roundtrip via decode_binary_frame
    let decoded_frame = decode_binary_frame(&encoded).expect("Decode binary frame");
    assert_eq!(decoded_frame.format, BrowserImageFormat::Png);
    assert_eq!(decoded_frame.seq, 42);
    assert_eq!(decoded_frame.metadata.image_width, 1);
    assert_eq!(decoded_frame.metadata.image_height, 1);
    assert_eq!(decoded_frame.image_bytes, png_bytes);
}
