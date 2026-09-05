use super::*;
use serde_json::json;

fn roundtrip<T: serde::de::DeserializeOwned + serde::Serialize>(wire: serde_json::Value) {
    // Given expected machine JSON, when decoded/encoded, then its shape is exact.
    let dto: T = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(dto).unwrap(), wire);
}

#[test]
fn shared_envelopes_roundtrip() {
    roundtrip::<MutationEnvelope<String>>(json!({"requestId":"create-1","params":"fixture"}));
    roundtrip::<ScopeResult<String>>(json!({"ok":true,"data":"accepted","requestId":"r1"}));
    roundtrip::<ScopeResult<String>>(json!({"ok":false,"requestId":"r1","error":{
        "code":"TARGET_EXPIRED","message":"fixture", "retryable":false,"details":null}}));
    roundtrip::<EventReplay<String, String>>(json!({"kind":"gap","afterSequence":12,
        "snapshot":{"revision":4,"items":[],"completeness":"partial","unavailableHosts":["host-b"]}}));
}

#[test]
fn result_rejects_mismatched_discriminants() {
    for wire in [json!({"ok":false,"data":"x","requestId":"r"}),
        json!({"ok":true,"error":{"code":"TIMEOUT","message":"fixture","retryable":true,"details":null},"requestId":"r"})] {
        assert!(serde_json::from_value::<ScopeResult<String>>(wire).is_err());
    }
}

#[test]
fn producer_boundaries_roundtrip() {
    roundtrip::<RunTarget>(json!({"kind":"ssh","hostId":"host-b"}));
    assert_eq!(serde_json::to_value(RunTarget::default()).unwrap(), json!({"kind":"local"}));
    roundtrip::<Vec<ScopeCapability>>(json!(["scopeControlV1","sshHelperV1","managedCodexV1","captureV1"]));
    roundtrip::<ConversationClaimKey>(json!({"hostId":"h","provider":"codex","conversationId":"provider-id"}));
    roundtrip::<ConversationOwner>(json!({"kind":"provisional","requestId":"thread-start-1"}));
    roundtrip::<ChatDraft>(json!({"text":"unsent fixture","attachments":[{
        "hostId":"h","attachmentId":"opaque-1","sha256":"fixture-hash","sizeBytes":17,"mediaType":"image/png"}]}));
    roundtrip::<TransitionSource>(json!({"kind":"provider","provider":"codex","requestId":"callback-1"}));
    assert_eq!((ATTACHMENT_MAX_FILE_BYTES, ATTACHMENT_MAX_FILES_PER_TURN,
        ATTACHMENT_MAX_TURN_BYTES, ATTACHMENT_UNREFERENCED_TTL_MS),
        (10_485_760, 4, 20_971_520, 86_400_000));
}

#[test]
fn target_roundtrip_preserves_full_u64_epoch_as_string() {
    // Given a runtime identity beyond JS integer precision.
    let target = TargetRef {
        host_id: "host-a".into(), owner_id: "owner-a".into(),
        epoch: Epoch(u64::MAX), backend_session_id: "pty-1".into(),
    };
    // When serialized through the production codec.
    let wire = serde_json::to_value(&target).unwrap();
    // Then every routing component and the exact decimal string survive.
    assert_eq!(wire, json!({"hostId":"host-a", "ownerId":"owner-a",
        "epoch":"18446744073709551615", "backendSessionId":"pty-1"}));
    assert_eq!(serde_json::from_value::<TargetRef>(wire).unwrap(), target);
}

#[test]
fn epoch_rejects_noncanonical_or_out_of_range_wire_values() {
    // Given invalid wire representations, when decoded, then all are rejected.
    for wire in [json!(1), json!("01"), json!("+1"), json!("-1"), json!("1.0"),
        json!(""), json!(" 1"), json!("18446744073709551616")] {
        assert!(serde_json::from_value::<Epoch>(wire.clone()).is_err(), "{wire}");
    }
}
