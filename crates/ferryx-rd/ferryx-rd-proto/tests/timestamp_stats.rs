use ferryx_rd_proto::{TimestampStats, TIMESTAMP_STATS_MAGIC};

const STATS: TimestampStats = TimestampStats {
    frame_id: 0x0403_0201,
    capture_us: 0x0c0b_0a09_0807_0605,
    encode_start_us: 0x1413_1211_100f_0e0d,
    encode_end_us: 0x1c1b_1a19_1817_1615,
    send_us: 0x2423_2221_201f_1e1d,
};

// Independent wire fixture: magic, u32 frame ID, then four u64 timestamps.
const LEGACY_BYTES: [u8; 42] = [
    b'E', b'R', b'D', b'T', b'S', b'1', 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a,
    0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a,
    0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x20, 0x21, 0x22, 0x23, 0x24,
];

#[test]
fn encode_preserves_legacy_length_magic_and_little_endian_field_order() {
    // Given the original by-value, infallible encoding API.
    let encode: fn(TimestampStats) -> Vec<u8> = TimestampStats::encode;
    // When encoding distinct bytes in every field.
    let bytes = encode(STATS);
    // Then the entire legacy wire representation is unchanged.
    assert_eq!(TimestampStats::SIZE, 42);
    assert_eq!(TIMESTAMP_STATS_MAGIC, b"ERDTS1");
    assert_eq!(bytes, LEGACY_BYTES);
}

#[test]
fn decode_preserves_legacy_little_endian_field_order() {
    // Given the original optional decoding API and an independent wire fixture.
    let decode: fn(&[u8]) -> Option<TimestampStats> = TimestampStats::decode;
    // When decoding the legacy bytes.
    let decoded = decode(&LEGACY_BYTES);
    // Then every field retains its legacy value.
    assert_eq!(decoded, Some(STATS));
}

#[test]
fn decode_rejects_every_truncation_and_trailing_bytes() {
    // Given every short prefix, including inputs shorter than the magic.
    for length in 0..LEGACY_BYTES.len() {
        // When decoding an incomplete payload.
        let decoded = TimestampStats::decode(&LEGACY_BYTES[..length]);
        // Then no prefix is accepted.
        assert_eq!(decoded, None, "accepted prefix length {length}");
    }
    // Given otherwise valid payloads with trailing data.
    for trailing_length in [1, 6, 42, 1024] {
        let mut bytes = LEGACY_BYTES.to_vec();
        bytes.resize(LEGACY_BYTES.len() + trailing_length, 0);
        // When decoding an overlong payload.
        let decoded = TimestampStats::decode(&bytes);
        // Then trailing data is not ignored.
        assert_eq!(decoded, None, "accepted {trailing_length} trailing bytes");
    }
}

#[test]
fn decode_rejects_a_change_to_any_magic_byte() {
    // Given each one-byte corruption of the magic, with the exact valid length.
    for index in 0..6 {
        let mut bytes = LEGACY_BYTES;
        bytes[index] ^= 0xff;
        // When decoding the corrupted payload.
        let decoded = TimestampStats::decode(&bytes);
        // Then the magic mismatch is rejected.
        assert_eq!(decoded, None, "accepted corrupt magic byte {index}");
    }
}

#[test]
fn round_trip_preserves_zero_maximum_and_unordered_timestamps() {
    // Given values the legacy codec accepts without semantic validation.
    for stats in [
        TimestampStats {
            frame_id: 0,
            capture_us: 0,
            encode_start_us: 0,
            encode_end_us: 0,
            send_us: 0,
        },
        TimestampStats {
            frame_id: u32::MAX,
            capture_us: u64::MAX,
            encode_start_us: u64::MAX,
            encode_end_us: u64::MAX,
            send_us: u64::MAX,
        },
        TimestampStats {
            frame_id: u32::MAX,
            capture_us: u64::MAX,
            encode_start_us: 20,
            encode_end_us: 10,
            send_us: 0,
        },
    ] {
        // When round-tripping through the shared codec.
        let decoded = TimestampStats::decode(&stats.encode());
        // Then no timestamp ordering or range restriction has been added.
        assert_eq!(decoded, Some(stats));
    }
}
