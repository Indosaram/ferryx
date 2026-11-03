use ferryx_rd_proto::{parity_count_for, FrameChunk, FrameParity, MAX_CHUNKS_PER_FRAME};

#[test]
fn disabled_and_nonfinite_redundancy_never_add_parity() {
    for chunks in [0, 1, 4, 5, 100, MAX_CHUNKS_PER_FRAME as usize, usize::MAX] {
        for redundancy in [0.0, -0.0, -0.1, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(parity_count_for(chunks, redundancy), 0);
        }
    }
}

#[test]
fn adaptive_parity_never_exceeds_twenty_percent_for_valid_frames() {
    for chunks in 1..=usize::from(MAX_CHUNKS_PER_FRAME) {
        for redundancy in [0.0, 0.01, 0.05, 0.1, 0.15, 0.2] {
            let parity = usize::from(parity_count_for(chunks, redundancy));
            assert!(parity * 5 <= chunks, "{parity} parity for {chunks} chunks");
            assert!(parity as f64 <= chunks as f64 * redundancy);
        }
    }
    assert_eq!(parity_count_for(1, 0.2), 0);
    assert_eq!(parity_count_for(5, 0.2), 1);
    assert_eq!(parity_count_for(usize::MAX, 1.0), MAX_CHUNKS_PER_FRAME);
}

#[test]
fn parity_uses_its_own_header_budget() {
    // Production sender chooses 1150-byte data when FEC is negotiated, not
    // the legacy 1154: parity's header is four bytes larger than FrameChunk's.
    assert_eq!(FrameParity::HEADER_SIZE, 10);
    assert_eq!(FrameChunk::HEADER_SIZE, 6);
    let encrypted_envelope = 40;
    let parity_data = 1200 - encrypted_envelope - FrameParity::HEADER_SIZE;
    assert_eq!(parity_data, 1150);
    assert!(encrypted_envelope + FrameChunk::HEADER_SIZE + parity_data <= 1200);
}
