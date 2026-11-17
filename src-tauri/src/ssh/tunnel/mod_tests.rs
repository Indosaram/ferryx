use super::*;
use serde_json::json;

fn spec() -> TunnelSpec {
    TunnelSpec {
        id: TunnelSpecId("spec-1".into()),
        label: "Database".into(),
        local_port: Some(5432),
        remote_host: "db.internal".into(),
        remote_port: 5432,
        enabled: true,
    }
}

#[test]
fn rejects_zero_local_port() {
    let mut value = spec();
    for port in [0, 1, 65535] {
        value.local_port = Some(port);
        assert_eq!(validate_spec(&value).is_ok(), port != 0);
    }
    value.remote_port = 0;
    assert_eq!(validate_spec(&value).unwrap_err().code, IpcErrorCode::InvalidArgument);
    value.remote_port = 5432;
    value.label.clear();
    assert!(validate_spec(&value).is_err());
    value.label = "Database".into();
    value.id.0.clear();
    assert!(validate_spec(&value).is_err());
}

#[test]
fn saved_spec_requires_a_port() {
    let mut value = spec();
    value.local_port = None;
    assert!(validate_spec(&value).is_ok());
    assert_eq!(validate_saved_spec(&value).unwrap_err().code, IpcErrorCode::InvalidArgument);
    value.local_port = Some(5432);
    assert!(validate_saved_spec(&value).is_ok());
}

#[test]
fn wire_decode_rejects_out_of_range_port() {
    for field in ["localPort", "remotePort"] {
        for port in [-1, 65536] {
            let mut wire = serde_json::to_value(spec()).unwrap();
            wire[field] = json!(port);
            assert!(serde_json::from_value::<TunnelSpec>(wire).is_err());
        }
    }
}

#[test]
fn rejects_empty_remote_host() {
    for (host, accepted) in [
        ("", false), ("a:1:b", false), ("a/b", false),
        ("a b", false), ("a\tb", false), ("a\0b", false),
        ("[::1]", true), ("::1", true), ("fd00::5", true),
        ("db.internal", true), ("127.0.0.1", true),
        ("-host", false), ("*", false), ("[a:1:b]", false),
    ] {
        let mut value = spec();
        value.remote_host = host.into();
        assert_eq!(validate_spec(&value).is_ok(), accepted, "{host:?}");
    }
}

#[test]
fn rejects_duplicate_local_port_in_one_host() {
    let first = spec();
    let mut second = first.clone();
    second.id = TunnelSpecId("spec-2".into());
    assert!(matches!(validate_specs_for_host(&[first.clone(), second.clone()])[0], SpecError::DuplicateLocalPort { local_port: 5432, .. }));
    second.local_port = Some(5433);
    second.id = first.id.clone();
    assert!(matches!(validate_specs_for_host(&[first, second])[0], SpecError::DuplicateId { .. }));
    let mut one = spec();
    one.local_port = None;
    let mut two = one.clone();
    two.id = TunnelSpecId("spec-2".into());
    assert!(validate_specs_for_host(&[one, two]).is_empty());
}

#[test]
fn accepts_identical_specs_for_two_hosts() {
    for _host in ["host-a", "host-b"] {
        assert!(validate_specs_for_host(&[spec()]).is_empty());
    }
}

#[test]
fn view_round_trips_with_lowercase_state() {
    for (state, token) in [
        (TunnelState::Stopped, "stopped"),
        (TunnelState::Listening, "listening"),
        (TunnelState::Reconnecting, "reconnecting"),
        (TunnelState::Failed, "failed"),
    ] {
        let mut view = TunnelView::stopped(&spec(), "host-a".into()).unwrap();
        view.state = state;
        if state != TunnelState::Stopped {
            view.runtime_id = Some(TunnelRuntimeId("runtime-1".into()));
            view.since = Some(1234);
        }
        if state == TunnelState::Failed {
            view.failure = Some(TunnelFailureCode::UnsupportedConfig);
            view.failure_piece = Some("config_alias".into());
        }
        let wire = serde_json::to_value(&view).unwrap();
        assert_eq!(wire["state"], token);
        assert!(wire.get("runtimeId").is_some());
        assert!(wire.get("failurePiece").is_some());
        assert_eq!(serde_json::from_value::<TunnelView>(wire).unwrap(), view);
    }
}

#[test]
fn failure_codes_are_the_ten_snake_case_strings() {
    let codes = [
        (TunnelFailureCode::PortInUse, "port_in_use"),
        (TunnelFailureCode::HostUnreachable, "host_unreachable"),
        (TunnelFailureCode::AuthFailed, "auth_failed"),
        (TunnelFailureCode::HostKeyRejected, "host_key_rejected"),
        (TunnelFailureCode::ChannelFailed, "channel_failed"),
        (TunnelFailureCode::UnsupportedPlatform, "unsupported_platform"),
        (TunnelFailureCode::UnsupportedConfig, "unsupported_config"),
        (TunnelFailureCode::DaemonUnavailable, "daemon_unavailable"),
        (TunnelFailureCode::CredentialUnavailable, "credential_unavailable"),
        (TunnelFailureCode::HostChanged, "host_changed"),
    ];
    for (code, token) in codes {
        assert_eq!(serde_json::to_value(code).unwrap(), token);
        assert_eq!(serde_json::from_value::<TunnelFailureCode>(json!(token)).unwrap(), code);
    }
    assert_eq!(serde_json::to_string(&TunnelFailureCode::HostUnreachable).unwrap(), "\"host_unreachable\"");
    assert_eq!(UNSUPPORTED_CONFIG_PIECES.len(), 14);
    assert!(UNSUPPORTED_CONFIG_PIECES.contains(&"unsupported_directive"));
}

#[test]
fn stopped_view_has_no_runtime_id() {
    let view = TunnelView::stopped(&spec(), "host-a".into()).unwrap();
    assert_eq!(view.state, TunnelState::Stopped);
    assert_eq!(view.runtime_id, None);
    assert_eq!(view.failure, None);
    assert_eq!(view.failure_piece, None);
    let mut wire = serde_json::to_value(&view).unwrap();
    wire["runtimeId"] = json!("runtime-1");
    assert!(serde_json::from_value::<TunnelView>(wire).is_err());
    for state in ["listening", "reconnecting", "failed"] {
        let mut wire = serde_json::to_value(&view).unwrap();
        wire["state"] = json!(state);
        assert!(serde_json::from_value::<TunnelView>(wire).is_err());
    }
    let mut wire = serde_json::to_value(&view).unwrap();
    wire["failurePiece"] = json!("config_alias");
    assert!(serde_json::from_value::<TunnelView>(wire).is_err());
    let mut unsaved = spec();
    unsaved.local_port = None;
    assert!(TunnelView::stopped(&unsaved, "host-a".into()).is_err());
}

#[test]
fn tunnel_spec_keys_are_camel_case() {
    let mut wire = serde_json::to_value(spec()).unwrap();
    let keys: std::collections::BTreeSet<_> = wire.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, ["id", "label", "localPort", "remoteHost", "remotePort", "enabled"].into_iter().collect());
    wire.as_object_mut().unwrap().remove("id");
    assert_eq!(serde_json::from_value::<TunnelSpec>(wire).unwrap().id.0, "");
    let identity = TunnelHostIdentity {
        hostname: "bastion".into(), username: Some("user".into()), port: Some(22),
        identity_file: Some("/keys/id".into()), jump_host: None, disabled: Some(false),
    };
    let wire = serde_json::to_value(&identity).unwrap();
    let keys: std::collections::BTreeSet<_> = wire.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, ["hostname", "username", "port", "identityFile", "jumpHost", "disabled"].into_iter().collect());
    assert_eq!(serde_json::from_value::<TunnelHostIdentity>(wire).unwrap(), identity);
}
