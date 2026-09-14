use super::*;
use serde_json::{json, Value};

// Both runtimes read the same full-wire cases; no Rust-only field mutation.
fn parity_decode(kind: &str, value: &Value) -> Result<Value, DecodeError> {
    let bytes = serde_json::to_vec(value).unwrap();
    fn parse<T: serde::de::DeserializeOwned + Serialize>(bytes: &[u8], limit: usize) -> Result<Value, DecodeError> {
        Ok(serde_json::to_value(decode_json::<T>(bytes, limit)?)?)
    }
    match kind {
        "target" => parse::<RunTarget>(&bytes, MACHINE_JSON_MAX_BYTES),
        "pairedProject" => parse::<PairedProject>(&bytes, MACHINE_JSON_MAX_BYTES),
        "descriptor" => parse::<ProxyDescriptor>(&bytes, MACHINE_JSON_MAX_BYTES),
        "capabilities" => parse::<Capabilities>(&bytes, MACHINE_JSON_MAX_BYTES),
        "directories" => parse::<Directories>(&bytes, DIRECTORY_JSON_MAX_BYTES),
        "project" => parse::<Project>(&bytes, MACHINE_JSON_MAX_BYTES),
        "projects" => parse::<Projects>(&bytes, MACHINE_JSON_MAX_BYTES),
        "worktree" => parse::<Worktree>(&bytes, MACHINE_JSON_MAX_BYTES),
        "worktrees" => parse::<Worktrees>(&bytes, MACHINE_JSON_MAX_BYTES),
        "session" => parse::<Session>(&bytes, MACHINE_JSON_MAX_BYTES),
        "sessions" => parse::<Sessions>(&bytes, MACHINE_JSON_MAX_BYTES),
        "error" => parse::<ErrorEnvelope>(&bytes, MACHINE_JSON_MAX_BYTES),
        "attached" => parse::<Attached>(&bytes, CONTROL_JSON_MAX_BYTES),
        "worktreeStatus" => parse::<WorktreeStatus>(&bytes, MACHINE_JSON_MAX_BYTES),
        "sessionDetail" => parse::<SessionDetail>(&bytes, MACHINE_JSON_MAX_BYTES),
        "operation" => parse::<Operation>(&bytes, MACHINE_JSON_MAX_BYTES),
        "control" => parse::<Control>(&bytes, CONTROL_JSON_MAX_BYTES),
        "registerRequest" => parse::<RegisterRequest>(&bytes, MACHINE_JSON_MAX_BYTES),
        "unregisterRequest" => parse::<UnregisterRequest>(&bytes, MACHINE_JSON_MAX_BYTES),
        "createWorktreeRequest" => parse::<CreateWorktreeRequest>(&bytes, MACHINE_JSON_MAX_BYTES),
        "deleteWorktreeRequest" => parse::<DeleteWorktreeRequest>(&bytes, MACHINE_JSON_MAX_BYTES),
        "createSessionRequest" => parse::<CreateSessionRequest>(&bytes, MACHINE_JSON_MAX_BYTES),
        "closeSessionRequest" => parse::<CloseSessionRequest>(&bytes, MACHINE_JSON_MAX_BYTES),
        kind => panic!("uncovered parity kind {kind}"),
    }
}

#[test]
fn legacy_valid_ssh_targets_roundtrip_without_normalization() {
    for host in ["ssh-a", "saved-host-123", " host-with-padding "] {
        let value = json!({"kind":"ssh", "hostId":host});
        let parsed: RunTarget = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(parsed, RunTarget::Ssh { host_id: host.into() });
        assert_eq!(serde_json::to_value(parsed).unwrap(), value);
    }
    for host in ["", " \t"] {
        assert!(serde_json::from_value::<RunTarget>(json!({"kind":"ssh", "hostId":host})).is_err());
    }
}

#[test]
fn expanded_shared_parity_fixtures() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/evidence/paired-daemon/fixtures");
    let valid: Vec<Value> = serde_json::from_slice(&std::fs::read(root.join("parity-valid.json")).unwrap()).unwrap();
    for fixture in valid {
        let kind = fixture["kind"].as_str().unwrap();
        assert_eq!(parity_decode(kind, &fixture["value"]).unwrap(), fixture["value"], "valid {kind}");
    }
    let invalid: Vec<Value> = serde_json::from_slice(&std::fs::read(root.join("parity-invalid.json")).unwrap()).unwrap();
    let mut accepted = Vec::new();
    for fixture in invalid {
        if parity_decode(fixture["kind"].as_str().unwrap(), &fixture["value"]).is_ok() {
            accepted.push(fixture["name"].clone());
        }
    }
    for case in &accepted { println!("ACCEPTED_INVALID {case}"); }
    assert!(accepted.is_empty(), "accepted {} invalid full-wire fixtures; see individual ACCEPTED_INVALID lines", accepted.len());
}

fn roundtrip<T: serde::de::DeserializeOwned + Serialize>(value: &Value, limit: usize) {
    let wire = serde_json::to_vec(value).unwrap();
    let parsed: T = decode_json(&wire, limit).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), *value);
}

#[test]
fn shared_fixtures_roundtrip_through_public_decoder() {
    let fixtures: Vec<Value> = serde_json::from_str(include_str!("../../../docs/evidence/paired-daemon/fixtures/contracts.json")).unwrap();
    for fixture in fixtures {
        let value = &fixture["value"];
        match fixture["kind"].as_str().unwrap() {
            "capabilities" => roundtrip::<Capabilities>(value, MACHINE_JSON_MAX_BYTES),
            "directories" => roundtrip::<Directories>(value, DIRECTORY_JSON_MAX_BYTES),
            "project" => roundtrip::<Project>(value, MACHINE_JSON_MAX_BYTES),
            "projects" => roundtrip::<Projects>(value, MACHINE_JSON_MAX_BYTES),
            "worktree" => roundtrip::<Worktree>(value, MACHINE_JSON_MAX_BYTES),
            "worktrees" => roundtrip::<Worktrees>(value, MACHINE_JSON_MAX_BYTES),
            "session" => roundtrip::<Session>(value, MACHINE_JSON_MAX_BYTES),
            "sessions" => roundtrip::<Sessions>(value, MACHINE_JSON_MAX_BYTES),
            "error" => roundtrip::<ErrorEnvelope>(value, MACHINE_JSON_MAX_BYTES),
            "attached" => roundtrip::<Attached>(value, CONTROL_JSON_MAX_BYTES),
            kind => panic!("uncovered fixture {kind}"),
        }
    }
}

#[test]
fn identity_is_host_qualified_and_preserves_large_epoch() {
    let a = RemoteTerminalTarget { machine_id: "machine".into(), daemon_epoch: Epoch(9007199254740993), session_id: "pty-1".into() };
    assert_ne!(desktop_workspace_id("a", "project").unwrap(), desktop_workspace_id("b", "project").unwrap());
    assert_ne!(proxy_backend_id("a", &a).unwrap(), proxy_backend_id("b", &a).unwrap());
    assert_eq!(serde_json::to_value(&a).unwrap()["daemonEpoch"], "9007199254740993");
    assert_ne!(desktop_workspace_id("a\",\"b", "c").unwrap(), desktop_workspace_id("a", "b\",\"c").unwrap());
}

#[test]
fn lifecycle_and_descriptor_fixtures_roundtrip() {
    let fixtures: Vec<Value> = serde_json::from_str(include_str!("../../../docs/evidence/paired-daemon/fixtures/lifecycle.json")).unwrap();
    for fixture in fixtures {
        let v = &fixture["value"];
        match fixture["kind"].as_str().unwrap() {
            "worktreeStatus" => roundtrip::<WorktreeStatus>(v, MACHINE_JSON_MAX_BYTES),
            "sessionDetail" => roundtrip::<SessionDetail>(v, MACHINE_JSON_MAX_BYTES),
            "operation" => roundtrip::<Operation>(v, MACHINE_JSON_MAX_BYTES),
            "control" => roundtrip::<Control>(v, CONTROL_JSON_MAX_BYTES),
            "registerRequest" => roundtrip::<RegisterRequest>(v, MACHINE_JSON_MAX_BYTES),
            "unregisterRequest" => roundtrip::<UnregisterRequest>(v, MACHINE_JSON_MAX_BYTES),
            "createWorktreeRequest" => roundtrip::<CreateWorktreeRequest>(v, MACHINE_JSON_MAX_BYTES),
            "deleteWorktreeRequest" => roundtrip::<DeleteWorktreeRequest>(v, MACHINE_JSON_MAX_BYTES),
            "createSessionRequest" => roundtrip::<CreateSessionRequest>(v, MACHINE_JSON_MAX_BYTES),
            "closeSessionRequest" => roundtrip::<CloseSessionRequest>(v, MACHINE_JSON_MAX_BYTES),
            kind => panic!("uncovered fixture {kind}"),
        }
    }
    let descriptor: Value = serde_json::from_str(include_str!("../../../docs/evidence/paired-daemon/fixtures/descriptor.json")).unwrap();
    roundtrip::<ProxyDescriptor>(&descriptor, MACHINE_JSON_MAX_BYTES);
    let parsed: ProxyDescriptor = serde_json::from_value(descriptor).unwrap();
    assert_eq!(parsed.remote_target.daemon_epoch, Epoch(9007199254740993));
    assert_eq!(parsed.remote_cursor, Epoch(9007199254740995));
    assert_eq!(parsed.native_attachment.daemon_epoch, Epoch(17));
    assert_eq!(parsed.native_attachment.last_output_sequence, Epoch(23));
    println!("SURFACE {}", serde_json::to_string(&parsed).unwrap());
}

#[test]
fn known_answer_full_hashes_match_shared_fixtures() {
    let fixtures: Vec<Value> = serde_json::from_str(include_str!("../../../docs/evidence/paired-daemon/fixtures/identities.json")).unwrap();
    for v in fixtures {
        let target: RemoteTerminalTarget = serde_json::from_value(v["target"].clone()).unwrap();
        assert_eq!(desktop_workspace_id(v["hostId"].as_str().unwrap(), v["remoteWorkspaceId"].as_str().unwrap()).unwrap(), v["workspaceId"]);
        assert_eq!(proxy_backend_id(v["hostId"].as_str().unwrap(), &target).unwrap(), v["localProxyId"]);
    }
}

#[test]
fn shared_invalid_fixtures_are_rejected() {
    let valid: Vec<Value> = serde_json::from_str(include_str!("../../../docs/evidence/paired-daemon/fixtures/contracts.json")).unwrap();
    let invalid: Vec<Value> = serde_json::from_str(include_str!("../../../docs/evidence/paired-daemon/fixtures/invalid.json")).unwrap();
    let mut accepted = Vec::new();
    for case in invalid {
        let mut value = valid.iter().find(|base| base["kind"] == case["kind"]).unwrap()["value"].clone();
        let field = case["field"].as_str().unwrap();
        if case["omit"] == true { value.as_object_mut().unwrap().remove(field); }
        else { value[field] = case["value"].clone(); }
        let bytes = serde_json::to_vec(&value).unwrap();
        let rejected = match case["kind"].as_str().unwrap() {
            "session" => decode_json::<Session>(&bytes, MACHINE_JSON_MAX_BYTES).is_err(),
            "attached" => decode_json::<Attached>(&bytes, CONTROL_JSON_MAX_BYTES).is_err(),
            "capabilities" => decode_json::<Capabilities>(&bytes, MACHINE_JSON_MAX_BYTES).is_err(),
            "project" => decode_json::<Project>(&bytes, MACHINE_JSON_MAX_BYTES).is_err(),
            "worktree" => decode_json::<Worktree>(&bytes, MACHINE_JSON_MAX_BYTES).is_err(),
            "directories" => decode_json::<Directories>(&bytes, DIRECTORY_JSON_MAX_BYTES).is_err(),
            kind => panic!("uncovered invalid fixture {kind}"),
        };
        if !rejected { accepted.push(case["name"].clone()); }
    }
    assert!(accepted.is_empty(), "accepted invalid fixtures: {accepted:?}");
}

#[test]
fn missing_metadata_and_unknown_targets_never_become_local() {
    let id = desktop_workspace_id("a", "project").unwrap();
    let valid = json!({"workspaceId":id,"repoRoot":"/app","target":{"kind":"pairedDaemon","hostId":"a"},"remoteWorkspaceId":"project"});
    roundtrip::<PairedProject>(&valid, MACHINE_JSON_MAX_BYTES);
    for field in ["target", "remoteWorkspaceId"] {
        let mut bad = valid.clone(); bad.as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<PairedProject>(bad).is_err());
    }
    for target in [json!({"kind":"pairedDaemon"}), json!({"kind":"future"}), json!({"kind":"pairedDaemon","hostId":" "})] {
        assert!(serde_json::from_value::<RunTarget>(target).is_err());
    }
}

#[test]
fn malformed_epochs_and_oversized_json_are_rejected() {
    for epoch in [json!(9007199254740993_u64), json!("01"), json!("+1"), json!("-1"), json!("1.0"), json!("18446744073709551616"), json!(" 1"), json!("")] {
        assert!(serde_json::from_value::<RemoteTerminalTarget>(json!({"machineId":"m","daemonEpoch":epoch,"sessionId":"s"})).is_err());
    }
    let bytes = vec![b' '; MACHINE_JSON_MAX_BYTES + 1];
    assert!(matches!(decode_json::<Project>(&bytes, MACHINE_JSON_MAX_BYTES), Err(DecodeError::PayloadTooLarge)));
}
