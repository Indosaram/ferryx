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
fn linking_a_saved_ssh_host_is_explicit_and_survives_restart() {
    let root = fixture();
    let input = pairing("machine-ssh-link");
    let mut store = Inventory::open(root.path()).unwrap();
    let row = store.pair(&input).unwrap();
    assert_eq!(
        row.ssh_host_id, None,
        "nothing may link an SSH host without an explicit action"
    );
    let before: serde_json::Value =
        serde_json::from_slice(&fs::read(path(root.path())).unwrap()).unwrap();
    assert!(
        before["hosts"][&row.host_id].get("sshHostId").is_none(),
        "an unlinked machine carries no sshHostId on disk"
    );

    store
        .set_ssh_host_id(&row.host_id, row.generation, Some("ssh-host-7".into()))
        .unwrap();
    assert_eq!(store.list()[0].ssh_host_id.as_deref(), Some("ssh-host-7"));
    let disk: serde_json::Value =
        serde_json::from_slice(&fs::read(path(root.path())).unwrap()).unwrap();
    assert_eq!(disk["hosts"][&row.host_id]["sshHostId"], "ssh-host-7");

    drop(store);
    let mut reopened = Inventory::open(root.path()).unwrap();
    assert_eq!(
        reopened.list()[0].ssh_host_id.as_deref(),
        Some("ssh-host-7"),
        "the link survives a restart"
    );
    reopened
        .set_ssh_host_id(&row.host_id, row.generation, None)
        .unwrap();
    assert_eq!(reopened.list()[0].ssh_host_id, None);
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
    assert!(!row.online, "a host nobody has probed is not online");
    assert_eq!(
        serde_json::from_str::<HostView>(&serde_json::to_string(&row).unwrap()).unwrap(),
        row
    );
    store
        .set_online(&row.host_id, row.generation, true)
        .unwrap();
    assert!(store.list()[0].online, "a probe that reached the host sets online");
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

/// `Inventory::open` and `normalize_origin` read the process-global opt-in. These tests
/// assert the closed default, so a leaked `FERRYX_ALLOW_INSECURE_RELAY` must not reach them.
/// The lock keeps one test's restore from turning the opt-in back on under another.
struct ClearedInsecureOptIn {
    env: Option<String>,
    flag: bool,
    _lock: std::sync::MutexGuard<'static, ()>,
}
impl Drop for ClearedInsecureOptIn {
    fn drop(&mut self) {
        match self.env.take() {
            Some(value) => unsafe { std::env::set_var("FERRYX_ALLOW_INSECURE_RELAY", value) },
            None => unsafe { std::env::remove_var("FERRYX_ALLOW_INSECURE_RELAY") },
        }
        crate::remote::relay_client::ALLOW_INSECURE_RELAY
            .store(self.flag, std::sync::atomic::Ordering::Relaxed);
    }
}
fn clear_insecure_relay_opt_in() -> ClearedInsecureOptIn {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let lock = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let env = std::env::var("FERRYX_ALLOW_INSECURE_RELAY").ok();
    let flag = crate::remote::relay_client::ALLOW_INSECURE_RELAY
        .swap(false, std::sync::atomic::Ordering::Relaxed);
    unsafe { std::env::remove_var("FERRYX_ALLOW_INSECURE_RELAY") };
    ClearedInsecureOptIn {
        env,
        flag,
        _lock: lock,
    }
}

#[test]
fn test_loopback_is_explicit_and_not_a_production_origin() {
    let _guard = clear_insecure_relay_opt_in();
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
    let _guard = clear_insecure_relay_opt_in();
    let root = fixture();
    let mut input = pairing("loopback");
    input.relay_origin = "http://127.0.0.1:43123".into();
    assert!(Inventory::open(root.path()).unwrap().pair(&input).is_err());
    let mut store = Inventory::open_test_loopback(root.path()).unwrap();
    let row = store.pair(&input).unwrap();
    drop(store);
    assert_eq!(
        Inventory::open_test_loopback(root.path()).unwrap().list(),
        vec![row.clone()]
    );
    let bytes = fs::read(path(root.path())).unwrap();
    let mut closed = Inventory::open(root.path()).expect("one insecure row must not fail the file");
    let listed = closed.list();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].host_id, row.host_id);
    assert_eq!(listed[0].auth_status, AuthStatus::Unknown);
    assert!(!listed[0].online);
    assert_eq!(
        listed[0].unavailable_reason.as_deref(),
        Some(INSECURE_LOOPBACK_RELAY)
    );
    assert_eq!(
        closed.capture(&row.host_id, row.generation).unwrap_err(),
        InventoryError::InsecureRelay
    );
    assert_eq!(fs::read(path(root.path())).unwrap(), bytes);
    root.close().unwrap();
}

#[test]
fn an_insecure_saved_relay_does_not_hide_the_other_hosts() {
    let _guard = clear_insecure_relay_opt_in();
    let root = fixture();
    let mut store = Inventory::open_test_loopback(root.path()).unwrap();
    let secure = store.pair(&pairing("secure")).unwrap();
    let mut loopback = pairing("loop");
    loopback.relay_origin = "http://127.0.0.1:43123".into();
    let insecure = store.pair(&loopback).unwrap();
    drop(store);
    let bytes = fs::read(path(root.path())).unwrap();

    let mut closed = Inventory::open(root.path()).unwrap();
    let listed = closed.list();
    let secure_view = listed
        .iter()
        .find(|host| host.host_id == secure.host_id)
        .expect("https host");
    assert_eq!(secure_view.auth_status, AuthStatus::Paired);
    assert!(secure_view.unavailable_reason.is_none());
    let blocked = listed
        .iter()
        .find(|host| host.host_id == insecure.host_id)
        .expect("loopback host");
    assert_eq!(blocked.auth_status, AuthStatus::Unknown);
    assert!(!blocked.online);
    assert_eq!(
        blocked.unavailable_reason.as_deref(),
        Some(INSECURE_LOOPBACK_RELAY)
    );
    assert!(closed
        .capture(&secure.host_id, secure.generation)
        .unwrap()
        .token()
        .is_ok());
    assert_eq!(
        closed
            .capture(&insecure.host_id, insecure.generation)
            .unwrap_err(),
        InventoryError::InsecureRelay
    );
    let read = closed
        .read_verified(&secure.host_id, secure.generation)
        .expect("the https host stays readable");
    assert!(read.unavailable_reason.is_none());
    closed
        .set_ssh_host_id(
            &secure.host_id,
            secure.generation,
            Some("saved-ssh".into()),
        )
        .expect("editing a healthy host must not fence the inventory");
    let text = fs::read_to_string(path(root.path())).unwrap();
    assert!(text.contains("127.0.0.1"), "the insecure row stays on disk");
    assert_ne!(fs::read(path(root.path())).unwrap(), bytes);

    let again = Inventory::open(root.path()).unwrap();
    assert!(again.list().iter().any(|host| {
        host.host_id == secure.host_id && host.unavailable_reason.is_none() && host.ssh_host_id.is_some()
    }));
    assert!(again.list().iter().any(|host| {
        host.host_id == insecure.host_id
            && host.unavailable_reason.as_deref() == Some(INSECURE_LOOPBACK_RELAY)
    }));
    let opted = Inventory::open_test_loopback(root.path()).unwrap();
    assert!(opted.list().iter().all(|host| {
        host.unavailable_reason.is_none() && host.auth_status == AuthStatus::Paired
    }));
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

#[test]
fn legacy_host_view_decodes_without_device_id_and_omits_none_in_wire_format() {
    let legacy_json = r#"{
        "hostId": "https://relay.example/host/legacy-01",
        "relayOrigin": "https://relay.example",
        "machineId": "legacy-01",
        "displayLabel": "Legacy 01",
        "grantScope": "machine",
        "generation": "1",
        "authStatus": "paired",
        "online": true,
        "sshHostId": null
    }"#;
    let view: HostView = serde_json::from_str(legacy_json).expect("legacy HostView must decode");
    assert_eq!(view.host_id, "https://relay.example/host/legacy-01");
    assert_eq!(view.relay_origin, "https://relay.example");
    assert_eq!(view.machine_id, "legacy-01");
    assert_eq!(view.display_label, "Legacy 01");
    assert_eq!(view.grant_scope, GrantScope::Machine);
    assert_eq!(view.generation, Epoch(1));
    assert_eq!(view.auth_status, AuthStatus::Paired);
    assert_eq!(view.online, true);
    assert_eq!(view.ssh_host_id, None);
    assert_eq!(view.device_id, None);
    assert_eq!(view.unavailable_reason, None);

    let serialized = serde_json::to_string(&view).expect("HostView serializes");
    assert!(!serialized.contains("deviceId"));
}

#[test]
fn host_view_roundtrips_explicit_device_id_machine_values() {
    let explicit_json = r#"{
        "hostId": "https://relay.example/host/explicit-01",
        "relayOrigin": "https://relay.example",
        "machineId": "explicit-01",
        "displayLabel": "Explicit 01",
        "grantScope": "machine",
        "generation": "3",
        "authStatus": "paired",
        "online": true,
        "sshHostId": "saved-ssh-42",
        "deviceId": "dev-explicit-authenticated-01"
    }"#;
    let view: HostView = serde_json::from_str(explicit_json).expect("HostView with deviceId must decode");
    assert_eq!(view.host_id, "https://relay.example/host/explicit-01");
    assert_eq!(view.device_id.as_deref(), Some("dev-explicit-authenticated-01"));
    assert_eq!(view.ssh_host_id.as_deref(), Some("saved-ssh-42"));
    assert_eq!(view.generation, Epoch(3));

    let serialized = serde_json::to_string(&view).expect("HostView serializes");
    assert!(serialized.contains(r#""deviceId":"dev-explicit-authenticated-01""#));
}

#[test]
fn legacy_inventory_disk_record_decodes_without_device_id() {
    let root = fixture();
    let legacy_disk = r#"{
        "version": 1,
        "hosts": {
            "https://relay.example/host/legacy-disk-01": {
                "relayOrigin": "https://relay.example",
                "machineId": "legacy-disk-01",
                "displayLabel": "Legacy Disk",
                "grantScope": "machine",
                "deviceToken": "legacy-secret-token-1234",
                "generation": "1",
                "authStatus": "paired"
            }
        },
        "generations": {
            "https://relay.example/host/legacy-disk-01": "1"
        }
    }"#;
    fs::write(path(root.path()), legacy_disk.as_bytes()).unwrap();

    let store = Inventory::open(root.path()).expect("legacy disk must load");
    let hosts = store.list();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].host_id, "https://relay.example/host/legacy-disk-01");
    assert_eq!(hosts[0].device_id, None);
    assert_eq!(hosts[0].generation, Epoch(1));
    assert_eq!(hosts[0].auth_status, AuthStatus::Paired);
    root.close().unwrap();
}

#[test]
fn pair_with_device_persists_explicit_device_id_across_reopen() {
    let root = fixture();
    let input = pairing("device-persist");
    let mut store = Inventory::open(root.path()).unwrap();
    let row = store
        .pair_with_device(&input, Some("dev-authenticated-99".into()))
        .expect("pair_with_device succeeds");
    assert_eq!(row.device_id.as_deref(), Some("dev-authenticated-99"));

    let disk: serde_json::Value =
        serde_json::from_slice(&fs::read(path(root.path())).unwrap()).unwrap();
    assert_eq!(
        disk["hosts"][&row.host_id]["deviceId"],
        "dev-authenticated-99"
    );

    drop(store);
    let reopened = Inventory::open(root.path()).expect("reopen inventory");
    let list = reopened.list();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].host_id, row.host_id);
    assert_eq!(list[0].device_id.as_deref(), Some("dev-authenticated-99"));
    root.close().unwrap();
}

fn host_attach() -> HostAttach {
    HostAttach {
        // base64 of 32 bytes
        machine_attach_public_key: "AQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyA=".into(),
        enrollment_epoch: "7".into(),
    }
}

#[test]
fn a_host_attach_round_trips_through_the_inventory_file() {
    let root = fixture();
    let input = pairing("attach-persist");
    let mut store = Inventory::open(root.path()).unwrap();
    let row = store
        .pair_attached(&input, Some("dev-attach".into()), host_attach())
        .expect("pair_attached succeeds");
    let disk: serde_json::Value =
        serde_json::from_slice(&fs::read(path(root.path())).unwrap()).unwrap();
    assert_eq!(
        disk["hosts"][&row.host_id]["attach"]["machineAttachPublicKey"],
        host_attach().machine_attach_public_key
    );
    assert_eq!(disk["hosts"][&row.host_id]["attach"]["enrollmentEpoch"], "7");

    drop(store);
    let mut reopened = Inventory::open(root.path()).expect("reopen inventory");
    let lease = reopened.capture(&row.host_id, row.generation).unwrap();
    assert_eq!(lease.attach(), Some(&host_attach()));
    root.close().unwrap();
}

#[test]
fn an_old_row_without_attach_still_loads_and_leases_no_attach() {
    let root = fixture();
    let input = pairing("attach-legacy");
    let mut store = Inventory::open(root.path()).unwrap();
    let row = store.pair(&input).unwrap();
    let disk: serde_json::Value =
        serde_json::from_slice(&fs::read(path(root.path())).unwrap()).unwrap();
    assert!(disk["hosts"][&row.host_id].get("attach").is_none());

    drop(store);
    let mut reopened = Inventory::open(root.path()).expect("old row loads");
    let lease = reopened.capture(&row.host_id, row.generation).unwrap();
    assert!(lease.attach().is_none());
    root.close().unwrap();
}

#[test]
fn an_invalid_host_attach_is_refused() {
    let root = fixture();
    let mut store = Inventory::open(root.path()).unwrap();
    let short = HostAttach {
        machine_attach_public_key: "AQID".into(),
        enrollment_epoch: "7".into(),
    };
    assert_eq!(
        store.pair_attached(&pairing("attach-bad"), None, short).unwrap_err(),
        InventoryError::InvalidInput
    );
    root.close().unwrap();
}
