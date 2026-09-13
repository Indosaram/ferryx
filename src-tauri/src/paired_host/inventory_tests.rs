use super::inventory::*;
use crate::scoped_contracts::Epoch;
use std::{fs, path::Path};

fn pairing(machine: &str) -> Pairing {
    Pairing {
        relay_origin: "https://RELAY.example:443/".into(),
        machine_id: machine.into(),
        display_label: format!("Machine {machine}"),
        grant_scope: GrantScope::Machine,
        device_token: format!(
            "private-fixture-bearer-{}",
            machine
                .bytes()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        ),
    }
}
fn path(root: &Path) -> std::path::PathBuf {
    root.join("paired-hosts.v1.json")
}
fn fixture() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("a13-inventory-")
        .tempdir()
        .unwrap()
}

#[test]
fn real_private_persistence_restart_and_secret_exclusion() {
    let root = fixture();
    let input = pairing("a /猫!");
    let mut store = Inventory::open(root.path()).unwrap();
    let row = store.pair(&input).unwrap();
    assert_eq!(row.host_id, "https://relay.example/host/a%20%2F%E7%8C%AB!");
    assert_eq!(row.generation, Epoch(1));
    assert_eq!(row.auth_status, AuthStatus::Paired);
    assert!(!row.online);
    assert_eq!(
        serde_json::from_str::<HostView>(&serde_json::to_string(&row).unwrap()).unwrap(),
        row
    );
    store
        .set_online(&row.host_id, row.generation, true)
        .unwrap();
    let lease = store.capture(&row.host_id, row.generation).unwrap();
    assert_eq!(lease.token().unwrap(), input.device_token);
    let outward = format!(
        "{} {:?} {:?}",
        serde_json::to_string(&store.list()).unwrap(),
        store,
        lease
    );
    assert!(!outward.contains(&input.device_token));
    assert!(!outward.contains("deviceToken"));
    let disk: serde_json::Value =
        serde_json::from_slice(&fs::read(path(root.path())).unwrap()).unwrap();
    assert_eq!(
        disk["hosts"][&row.host_id]["deviceToken"],
        input.device_token
    );
    assert_eq!(disk["hosts"][&row.host_id]["generation"], "1");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(root.path()).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(path(root.path()))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    drop(store);
    assert!(lease.token().is_err());
    let mut restarted = Inventory::open(root.path()).unwrap();
    assert_eq!(restarted.list(), vec![row.clone()]);
    assert_eq!(
        restarted
            .capture(&row.host_id, row.generation)
            .unwrap()
            .token()
            .unwrap(),
        input.device_token
    );
    root.close().unwrap();
}

#[tokio::test]
async fn re_pair_forget_and_revoke_cancel_only_captured_host_across_restart() {
    let root = fixture();
    let mut store = Inventory::open(root.path()).unwrap();
    let a = store.pair(&pairing("a")).unwrap();
    let b = store.pair(&pairing("b")).unwrap();
    let a_lease = store.capture(&a.host_id, a.generation).unwrap();
    let b_lease = store.capture(&b.host_id, b.generation).unwrap();
    let mut signal = a_lease.cancellation();
    let mut replacement = pairing("a");
    replacement.device_token = "replacement-private-bearer".into();
    let a2 = store.pair(&replacement).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(1), signal.changed())
        .await
        .unwrap()
        .unwrap();
    assert!(*signal.borrow());
    assert_eq!(a2.generation, Epoch(2));
    assert!(a_lease.token().is_err());
    assert!(store.validate_generation(&a.host_id, a.generation).is_err());
    assert!(store
        .mark_auth_unavailable(&a.host_id, a.generation, true)
        .is_err());
    assert!(b_lease.token().is_ok());
    let a2_lease = store.capture(&a2.host_id, a2.generation).unwrap();
    let mut forgotten = a2_lease.cancellation();
    store.forget(&a2.host_id, a2.generation).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(1), forgotten.changed())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(store.list(), vec![b.clone()]);
    assert!(b_lease.token().is_ok());
    let bytes = fs::read(path(root.path())).unwrap();
    assert!(!String::from_utf8(bytes)
        .unwrap()
        .contains(&replacement.device_token));
    drop(store);
    let mut restarted = Inventory::open(root.path()).unwrap();
    let a4 = restarted.pair(&replacement).unwrap();
    assert_eq!(a4.generation, Epoch(4));
    assert!(restarted
        .validate_generation(&a.host_id, a.generation)
        .is_err());
    restarted
        .mark_auth_unavailable(&a4.host_id, a4.generation, true)
        .unwrap();
    let revoked = restarted
        .list()
        .into_iter()
        .find(|v| v.host_id == a.host_id)
        .unwrap();
    assert_eq!(revoked.auth_status, AuthStatus::Revoked);
    assert!(restarted
        .capture(&revoked.host_id, revoked.generation)
        .is_err());
    assert_eq!(
        restarted
            .list()
            .into_iter()
            .find(|v| v.host_id == b.host_id)
            .unwrap(),
        b
    );
    root.close().unwrap();
}

#[test]
fn mirror_and_unknown_do_not_become_machine_authority() {
    let root = fixture();
    let mut store = Inventory::open(root.path()).unwrap();
    let mut input = pairing("mirror");
    input.grant_scope = GrantScope::Mirror;
    let row = store.pair(&input).unwrap();
    assert_eq!(row.auth_status, AuthStatus::NeedsMachineGrant);
    assert!(store.capture(&row.host_id, row.generation).is_err());
    store
        .mark_auth_unavailable(&row.host_id, row.generation, false)
        .unwrap();
    assert_eq!(
        Inventory::open(root.path()).unwrap().list()[0].auth_status,
        AuthStatus::Unknown
    );
    assert!(!String::from_utf8(fs::read(path(root.path())).unwrap())
        .unwrap()
        .contains(&input.device_token));
    root.close().unwrap();
}

#[test]
fn migration_requires_host_proof_and_verified_copy_and_preserves_source() {
    let root = fixture();
    let source = root.path().join("legacy.json");
    let input = pairing("migration");
    let original = serde_json::to_vec(&serde_json::json!({"hostId":"https://relay.example/host/migration", "deviceToken":input.device_token})).unwrap();
    fs::write(&source, &original).unwrap();
    let mut store = Inventory::open(root.path()).unwrap();
    let mut legacy = LegacyCredential {
        host_id: "https://relay.example".into(),
        pairing: input,
    };
    assert_eq!(
        store.migrate_copy(&legacy),
        Err(InventoryError::MigrationPending)
    );
    assert!(!path(root.path()).exists());
    legacy.host_id = "https://relay.example/host/migration".into();
    let receipt = store.migrate_copy(&legacy).unwrap();
    assert_eq!(receipt.generation, Epoch(1));
    assert_eq!(store.migrate_copy(&legacy).unwrap(), receipt);
    assert!(!serde_json::to_string(&receipt)
        .unwrap()
        .contains(&legacy.pairing.device_token));
    assert_eq!(fs::read(&source).unwrap(), original);
    let mut restarted = Inventory::open(root.path()).unwrap();
    assert_eq!(
        restarted
            .capture(&receipt.host_id, receipt.generation)
            .unwrap()
            .token()
            .unwrap(),
        legacy.pairing.device_token
    );
    restarted
        .forget(&receipt.host_id, receipt.generation)
        .unwrap();
    assert!(restarted.migrate_copy(&legacy).is_err());
    root.close().unwrap();
}

#[test]
fn actual_write_failure_keeps_disk_and_legacy_authority_and_fences_leases() {
    let root = fixture();
    let mut store = Inventory::open(root.path()).unwrap();
    let a = store.pair(&pairing("a")).unwrap();
    let lease = store.capture(&a.host_id, a.generation).unwrap();
    let original = fs::read(path(root.path())).unwrap();
    // Deterministic open failure even as root; no permission/timing luck.
    let temp = path(root.path()).with_extension(format!("tmp.{}", std::process::id()));
    fs::create_dir(&temp).unwrap();
    let legacy = LegacyCredential {
        host_id: "https://relay.example/host/b".into(),
        pairing: pairing("b"),
    };
    let source = root.path().join("legacy-token");
    fs::write(&source, &legacy.pairing.device_token).unwrap();
    assert_eq!(
        store.migrate_copy(&legacy),
        Err(InventoryError::MigrationPending)
    );
    assert_eq!(fs::read(path(root.path())).unwrap(), original);
    assert_eq!(
        fs::read_to_string(source).unwrap(),
        legacy.pairing.device_token
    );
    assert!(lease.token().is_err());
    assert_eq!(store.list()[0].auth_status, AuthStatus::Unknown);
    fs::remove_dir(temp).unwrap();
    assert_eq!(Inventory::open(root.path()).unwrap().list(), vec![a]);
    root.close().unwrap();
}

#[test]
fn corrupt_unknown_newer_and_external_replacement_never_overwrite() {
    for bytes in [
        b"{private-malformed-token".as_slice(),
        b"{\"version\":2,\"hosts\":{},\"generations\":{}}",
        b"{\"version\":1,\"hosts\":{},\"generations\":{},\"secret\":\"private-malformed-token\"}",
    ] {
        let root = fixture();
        fs::write(path(root.path()), bytes).unwrap();
        let error = Inventory::open(root.path()).unwrap_err();
        assert!(!format!("{error:?} {error}").contains("private-malformed-token"));
        assert_eq!(fs::read(path(root.path())).unwrap(), bytes);
        root.close().unwrap();
    }
    let root = fixture();
    let mut store = Inventory::open(root.path()).unwrap();
    store.pair(&pairing("a")).unwrap();
    fs::write(path(root.path()), b"newer-authority").unwrap();
    assert!(store.pair(&pairing("b")).is_err());
    assert_eq!(fs::read(path(root.path())).unwrap(), b"newer-authority");
    root.close().unwrap();
}

#[test]
fn test_loopback_is_explicit_and_not_a_production_origin() {
    for origin in [
        "http://localhost:8080",
        "http://127.0.0.1:8080",
        "http://[::1]:8080",
    ] {
        assert!(normalize_origin(origin).is_err());
        assert!(normalize_test_loopback_origin(origin).is_ok());
    }
    for origin in [
        "http://relay.example",
        "http://localhost.evil",
        "https://@relay.example",
        "https://relay.example/path",
        "https://relay.example?token=secret",
        "https://relay.example/#secret",
        "ftp://relay.example",
        " https://relay.example",
    ] {
        assert!(normalize_origin(origin).is_err());
        assert!(normalize_test_loopback_origin(origin).is_err());
    }
}

#[test]
fn fixture_loopback_store_roundtrips_but_production_fails_closed() {
    let root = fixture();
    let mut input = pairing("loopback");
    input.relay_origin = "http://127.0.0.1:43123".into();
    assert!(Inventory::open(root.path()).unwrap().pair(&input).is_err());
    let mut store = Inventory::open_test_loopback(root.path()).unwrap();
    let row = store.pair(&input).unwrap();
    drop(store);
    assert_eq!(
        Inventory::open_test_loopback(root.path()).unwrap().list(),
        vec![row]
    );
    let bytes = fs::read(path(root.path())).unwrap();
    assert!(Inventory::open(root.path()).is_err());
    assert_eq!(fs::read(path(root.path())).unwrap(), bytes);
    root.close().unwrap();
}

#[test]
fn public_http_and_userinfo_fail_closed() {
    assert!(normalize_origin("http://relay.example").is_err());
    assert!(normalize_origin("https://secret@relay.example").is_err());
}

#[test]
fn origin_normalization_is_identity_stable() {
    assert_eq!(
        normalize_origin("https://RELAY.example:443/").unwrap(),
        "https://relay.example"
    );
}
