use super::*;

#[test]
fn receiver_export_retains_legacy_flat_fields_and_rank_meaning() {
    // Given: lifetime count exceeds the ring, with distinct percentile ranks
    // so every reported field has a unique expected value.
    let mut recorder = LatencyRecorder::with_capacity(100);
    for value in 1..=150_u64 {
        recorder.record_us(value);
    }
    let temporary = tempfile::tempdir().unwrap();
    let mut config = SessionConfig::direct("127.0.0.1", "empty");
    config.pairing_store_path = Some(temporary.path().join("pairings.json"));
    let session = ClientSession::new(config).unwrap();
    // When: exporting a real, unsampled session and the existing recorder.
    let json: serde_json::Value = serde_json::from_str(
        &stats_json(&recorder, &session.receiver_snapshot().unwrap()).unwrap(),
    )
    .unwrap();
    // Then: frames preserves the lifetime count, and each percentile keeps its
    // rank over the retained ring (samples 51..=150, so p50 < p95 < p99 < max).
    for (field, value) in [
        ("frames", 150),
        ("p50_us", 101),
        ("p95_us", 145),
        ("p99_us", 149),
        ("max_us", 150),
    ] {
        assert_eq!(json[field], value, "{field}");
    }
    assert_eq!(
        json.get("receiver_snapshot"),
        Some(&serde_json::to_value(session.receiver_snapshot().unwrap()).unwrap())
    );
}
