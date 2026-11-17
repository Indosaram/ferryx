use super::*;
use crate::ssh::tunnel::TunnelSpecId;
use sha2::{Digest, Sha256};

fn spec(port: u16) -> TunnelSpec {
    TunnelSpec { id: TunnelSpecId(format!("spec-{port}")), label: "database".into(), local_port: Some(port), remote_host: "db.internal".into(), remote_port: 5432, enabled: true }
}
fn file() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ssh_tunnels.json");
    (dir, path)
}
fn save(path: &Path, host: &str, specs: Vec<TunnelSpec>, revision: Option<u64>, force: bool, discard_invalid: bool) -> Result<TunnelStore, IpcError> {
    lock_store(path)?.save_specs(host, specs, revision, force, discard_invalid, &TunnelStore::default(), 100)
}
fn digest(path: &Path) -> Vec<u8> { Sha256::digest(fs::read(path).unwrap()).to_vec() }
fn reason(error: IpcError, code: IpcErrorCode, reason: &str) {
    assert_eq!(error.code, code);
    assert_eq!(error.details.as_ref().unwrap()["reason"], reason);
}
fn partial(path: &Path) -> (String, String) {
    let a = r#"{ "localPort" : "bad-a", "untouched" : [ 1,  2 ] }"#.to_owned();
    let b = r#"{ "localPort" : "bad-b" }"#.to_owned();
    fs::write(path, format!(r#"{{"version":1,"hosts":{{"a":[{},{}],"b":[{},{}]}}}}"#, serde_json::to_string(&spec(5001)).unwrap(), a, serde_json::to_string(&spec(5002)).unwrap(), b)).unwrap();
    (a, b)
}
fn identity() -> TunnelHostIdentity {
    TunnelHostIdentity { hostname: "box".into(), username: Some("user".into()), port: Some(22), identity_file: Some("/key".into()), jump_host: None, disabled: Some(false) }
}

#[test]
fn force_does_not_overwrite_corrupt_store() {
    let (_dir, path) = file(); fs::write(&path, b"not json").unwrap(); let before = digest(&path);
    reason(save(&path, "a", vec![spec(5001)], None, true, false).unwrap_err(), IpcErrorCode::Unsupported, "corrupt_store");
    assert_eq!(digest(&path), before);
}
#[test]
fn malformed_definition_is_dropped_and_reported() {
    let (_dir, path) = file(); partial(&path);
    let store = load_store(&path, 100).unwrap();
    assert_eq!(store.hosts["a"], vec![spec(5001)]);
    assert!(store.host_has_invalid_definitions.contains(&"a".into()));
    assert_eq!(store.store_status.kind, "partial");
    assert_eq!(store.store_status.invalid_hosts.len(), 2);
}
#[test]
fn malformed_definition_preserves_siblings_and_other_hosts() {
    let (_dir, path) = file(); partial(&path);
    let store = load_store(&path, 100).unwrap();
    assert_eq!(store.hosts["a"], vec![spec(5001)]);
    assert_eq!(store.hosts["b"], vec![spec(5002)]);
    assert_eq!(store.invalid_rows.len(), 2);
}
#[test]
fn malformed_file_loads_empty_and_reset_quarantines_without_deleting() {
    let (_dir, path) = file(); fs::write(&path, b"{broken").unwrap(); let before = digest(&path);
    let store = load_store(&path, 100).unwrap();
    assert!(store.hosts.is_empty()); assert_eq!(store.store_status.kind, "corrupt"); assert!(!store.diagnostics.is_empty());
    assert_eq!(digest(&path), before);
    reason(save(&path, "a", vec![], Some(0), false, false).unwrap_err(), IpcErrorCode::Unsupported, "corrupt_store");
    let quarantined = lock_store(&path).unwrap().reset(123, &store).unwrap();
    assert_eq!(digest(&quarantined), before); assert!(path.exists());
    let fresh = save(&path, "a", vec![], Some(0), false, false).unwrap();
    assert!(fresh.host_revision("previously-unknown") >= 123);
    assert!(fresh.host_revision("previously-unknown") > store.host_revision("previously-unknown"));
    reason(lock_store(&path).unwrap().reset(124, &fresh).unwrap_err(), IpcErrorCode::InvalidArgument, "store_not_corrupt");
}
#[test]
fn partially_corrupt_rows_survive_save_list_round_trip_verbatim() {
    let (_dir, path) = file(); let (a, b) = partial(&path);
    save(&path, "c", vec![spec(5003)], Some(0), false, false).unwrap();
    let store = load_store(&path, 100).unwrap();
    assert_eq!(store.invalid_rows["a"], vec![a]); assert_eq!(store.invalid_rows["b"], vec![b]);
    assert_eq!(store.hosts["c"], vec![spec(5003)]);
    // A malformed host value (not just an array row) also survives unchanged.
    fs::write(&path, r#"{"version":1,"hosts":{"odd": { "not" : "an array" }}}"#).unwrap();
    save(&path, "c", vec![], Some(0), false, false).unwrap();
    assert!(fs::read_to_string(&path).unwrap().contains(r#""odd":{ "not" : "an array" }"#));
}
#[test]
fn invalid_saved_specs_are_rejected_before_any_write() {
    let (_dir, path) = file(); save(&path, "a", vec![spec(5001)], Some(0), false, false).unwrap(); let before = digest(&path);
    let mut missing = spec(5002); missing.local_port = None;
    let mut zero = spec(5002); zero.local_port = Some(0);
    let mut empty = spec(5002); empty.remote_host.clear();
    let mut remote_zero = spec(5002); remote_zero.remote_port = 0;
    let mut duplicate = spec(5002); duplicate.id = TunnelSpecId("another".into());
    for specs in [vec![missing], vec![zero], vec![empty], vec![remote_zero], vec![spec(5002), duplicate]] {
        assert_eq!(save(&path, "a", specs, Some(1), false, false).unwrap_err().code, IpcErrorCode::InvalidArgument);
        assert_eq!(digest(&path), before);
    }
}

// Frozen pre-feature schema: deliberately NOT SshHost/SshHostStore. An old writer
// decodes only these fields and drops every field it does not understand.
mod frozen_host_writer {
    use serde::{Deserialize, Serialize};
    #[derive(Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Host { id: String, label: String, hostname: String, username: Option<String>, port: Option<u16>, identity_file: Option<String>, jump_host: Option<String>, source: String, auth_method: String, disabled: Option<bool>, shell: Option<String> }
    #[derive(Serialize, Deserialize)]
    struct Store { hosts: Vec<Host>, #[serde(default)] tombstones: Vec<String> }
    pub fn rewrite(path: &std::path::Path) {
        let store: Store = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let temp = path.with_extension(format!("json.tmp.{}.{}", std::process::id(), uuid::Uuid::new_v4()));
        std::fs::write(&temp, serde_json::to_vec_pretty(&store).unwrap()).unwrap();
        std::fs::rename(temp, path).unwrap();
    }
}
fn hosts_file(path: &Path) -> PathBuf {
    let hosts = path.with_file_name("ssh_hosts.json");
    fs::write(&hosts, r#"{"hosts":[{"id":"a","label":"box","hostname":"box","source":"manual","authMethod":"key","unknownFutureField":true}],"tombstones":[]}"#).unwrap(); hosts
}
#[test]
fn frozen_legacy_host_writer_cannot_destroy_definitions() {
    let (_dir, path) = file(); let hosts = hosts_file(&path);
    save(&path, "a", vec![spec(5001)], Some(0), false, false).unwrap();
    frozen_host_writer::rewrite(&hosts);
    assert!(!fs::read_to_string(&hosts).unwrap().contains("unknownFutureField"));
    assert_eq!(load_store(&path, 100).unwrap().hosts["a"], vec![spec(5001)]);
    let before = digest(&hosts); save(&path, "a", vec![spec(5002)], Some(1), false, false).unwrap(); assert_eq!(digest(&hosts), before);
}

#[cfg(unix)]
#[test]
fn cross_process_flock_serializes_writers_and_detects_same_host_conflict() {
    use std::io::{Read, Write};
    use std::os::unix::{fs::PermissionsExt, net::UnixListener};
    use std::process::{Command, Stdio};
    const ROLE: &str = "FERRYX_TUNNEL_STORE_TEST_ROLE";
    if let Ok(role) = std::env::var(ROLE) {
        let path = PathBuf::from(std::env::var_os("FERRYX_TUNNEL_STORE_TEST_PATH").unwrap());
        let barrier = PathBuf::from(std::env::var_os("FERRYX_TUNNEL_STORE_TEST_BARRIER").unwrap());
        let same = std::env::var("FERRYX_TUNNEL_STORE_TEST_SAME").unwrap() == "yes";
        let initial = load_store(&path, 100).unwrap().definitions_revision;
        let mut signal = std::os::unix::net::UnixStream::connect(barrier).unwrap();
        signal.set_read_timeout(Some(std::time::Duration::from_secs(15))).unwrap();
        signal.write_all(b"ready").unwrap(); let mut go = [0]; signal.read_exact(&mut go).unwrap();
        let a = role == "writer-a";
        let result = save(&path, if same { "shared" } else if a { "a" } else { "b" }, vec![spec(if a { 5001 } else { 5002 })], if same { Some(initial) } else { None }, !same, false);
        if same {
            match result { Ok(_) => fs::write(path.with_extension(format!("winner-{role}")), b"ok").unwrap(), Err(e) => reason(e, IpcErrorCode::RequestConflict, "tunnel_store_conflict") }
        } else { result.unwrap(); }
        return;
    }
    for same in [false, true] {
        let (_dir, path) = file();
        // A short socket path avoids Darwin's 104-byte sockaddr_un limit. The
        // socket is the barrier file; release is event-driven, never a sleep.
        let barrier_dir = tempfile::Builder::new().prefix("fx-t2-").tempdir_in("/tmp").unwrap();
        let barrier = barrier_dir.path().join("barrier"); let listener = UnixListener::bind(&barrier).unwrap();
        let children: Vec<_> = ["writer-a", "writer-b"].into_iter().map(|role| Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "ssh::tunnel_store::tests::cross_process_flock_serializes_writers_and_detects_same_host_conflict", "--nocapture"])
            .env(ROLE, role).env("FERRYX_TUNNEL_STORE_TEST_PATH", &path).env("FERRYX_TUNNEL_STORE_TEST_BARRIER", &barrier)
            .env("FERRYX_TUNNEL_STORE_TEST_SAME", if same { "yes" } else { "no" }).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap()).collect();
        let mut streams = Vec::new();
        for _ in 0..2 {
            use std::os::fd::AsRawFd;
            let mut ready_fd = libc::pollfd { fd: listener.as_raw_fd(), events: libc::POLLIN, revents: 0 };
            assert_eq!(unsafe { libc::poll(&mut ready_fd, 1, 15_000) }, 1, "writer did not reach barrier");
            let (mut stream, _) = listener.accept().unwrap(); stream.set_read_timeout(Some(std::time::Duration::from_secs(15))).unwrap(); let mut ready = [0; 5]; stream.read_exact(&mut ready).unwrap(); assert_eq!(&ready, b"ready"); streams.push(stream); }
        for stream in &mut streams { stream.write_all(b"g").unwrap(); }
        for child in children { let output = child.wait_with_output().unwrap(); assert!(output.status.success(), "{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr)); }
        let store = load_store(&path, 100).unwrap();
        assert_eq!(fs::metadata(path.with_file_name("ssh_tunnels.lock")).unwrap().permissions().mode() & 0o777, 0o600);
        if same {
            assert_eq!(store.definitions_revision, 1); assert_eq!(store.hosts["shared"].len(), 1);
            let a = path.with_extension("winner-writer-a").exists(); let b = path.with_extension("winner-writer-b").exists(); assert_ne!(a, b);
            assert_eq!(store.hosts["shared"], vec![spec(if a { 5001 } else { 5002 })]);
        } else { assert_eq!(store.definitions_revision, 2); assert_eq!(store.hosts["a"], vec![spec(5001)]); assert_eq!(store.hosts["b"], vec![spec(5002)]); }
    }
}
#[test]
fn host_and_definition_writers_use_independent_temporary_files() {
    let (_dir, path) = file(); let hosts = hosts_file(&path);
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let p = &path; let start = &barrier;
        scope.spawn(move || { start.wait(); for port in 5000..5020 { save(p, "a", vec![spec(port)], None, true, false).unwrap(); } });
        let h = &hosts; let start = &barrier;
        scope.spawn(move || { start.wait(); for _ in 0..20 { frozen_host_writer::rewrite(h); } });
    });
    assert_eq!(load_store(&path, 100).unwrap().hosts["a"], vec![spec(5019)]);
    assert_eq!(serde_json::from_slice::<Value>(&fs::read(hosts).unwrap()).unwrap()["hosts"][0]["id"], "a");
}
#[test]
fn matching_expected_revision_succeeds() {
    let (_dir, path) = file(); let first = save(&path, "a", vec![spec(5001)], Some(0), false, false).unwrap();
    let second = save(&path, "a", vec![spec(5002)], Some(first.definitions_revision), false, false).unwrap(); assert_eq!(second.definitions_revision, 2);
}
#[test]
fn mismatched_expected_revision_refuses_without_write() {
    let (_dir, path) = file(); save(&path, "a", vec![spec(5001)], Some(0), false, false).unwrap(); let before = digest(&path);
    reason(save(&path, "a", vec![], Some(0), false, false).unwrap_err(), IpcErrorCode::RequestConflict, "tunnel_store_conflict"); assert_eq!(digest(&path), before);
}
#[test]
fn missing_revision_requires_explicit_force() {
    let (_dir, path) = file(); reason(save(&path, "a", vec![], None, false, false).unwrap_err(), IpcErrorCode::InvalidArgument, "revision_required"); assert!(!path.exists());
    assert_eq!(save(&path, "a", vec![], None, true, false).unwrap().definitions_revision, 1);
}
#[test]
fn unsupported_version_is_never_overwritten_even_with_force() {
    let (_dir, path) = file(); fs::write(&path, r#"{"version":2,"hosts":{}}"#).unwrap(); let before = digest(&path);
    assert_eq!(load_store(&path, 100).unwrap().store_status.kind, "unsupported_version");
    reason(save(&path, "a", vec![], None, true, false).unwrap_err(), IpcErrorCode::Unsupported, "unsupported_version"); assert_eq!(digest(&path), before);
    reason(lock_store(&path).unwrap().reset(123, &TunnelStore::default()).unwrap_err(), IpcErrorCode::InvalidArgument, "store_not_corrupt");
}
#[test]
fn authority_writes_do_not_advance_definitions_revision() {
    let (_dir, path) = file(); let mut memory = save(&path, "a", vec![spec(5001)], Some(0), false, false).unwrap();
    assert_eq!(memory.host_revision("unknown"), 0);
    memory.bump_host_revision("a", identity(), 100).unwrap(); lock_store(&path).unwrap().persist_authority(&memory, 100).unwrap();
    assert_eq!(load_store(&path, 100).unwrap().definitions_revision, 1);
    memory.stamp_identity("a", 1, 200); lock_store(&path).unwrap().persist_authority(&memory, 200).unwrap(); assert_eq!(load_store(&path, 200).unwrap().definitions_revision, 1);
}
#[test]
fn corrupt_store_authority_write_preserves_bytes_and_returns_corrupt_store() {
    let (_dir, path) = file();
    for bytes in ["broken", r#"{"version":99,"hosts":{}}"#] {
        fs::write(&path, bytes).unwrap(); let before = digest(&path); let mut memory = TunnelStore::default(); memory.bump_host_revision("a", identity(), 100).unwrap();
        reason(lock_store(&path).unwrap().persist_authority(&memory, 100).unwrap_err(), IpcErrorCode::Unsupported, "corrupt_store"); assert_eq!(digest(&path), before);
    }
}
#[test]
fn seed_floor_exceeds_fixture_revisions_and_never_goes_backwards() {
    let (_dir, path) = file(); fs::write(&path, r#"{"version":1,"hosts":{}}"#).unwrap();
    assert_eq!(load_store(&path, 100).unwrap().host_revision("missing"), 0);
    let mut store = TunnelStore::default(); store.host_revisions.extend([("a".into(), 42), ("b".into(), 500)]);
    let first = store.seed_floor(100); assert!(first >= 100); assert!(first > 500);
    assert!(store.seed_floor(1) >= first);
}
#[test]
fn authority_survives_restart_with_identity_pending_and_stale_rules() {
    let (_dir, path) = file(); let mut memory = TunnelStore::default(); let revision = memory.bump_host_revision("a", identity(), 100).unwrap();
    memory.stamp_identity("a", revision, 100); lock_store(&path).unwrap().persist_authority(&memory, 100).unwrap();
    let loaded = load_store(&path, 100).unwrap(); assert_eq!(loaded.host_revision("a"), revision);
    let entry = loaded.active_host_identity("a").unwrap(); assert_eq!(entry.fingerprint, identity()); assert_eq!(entry.recorded_at_unix_ms, Some(100)); assert_eq!(entry.revision, revision);
    assert_eq!(load_store(&path, 50).unwrap().active_host_identity("a").unwrap().recorded_at_unix_ms, Some(50));
    memory.mark_identity_pending("a", 150); assert_eq!(memory.active_host_identity("a").unwrap().recorded_at_unix_ms, None);
    lock_store(&path).unwrap().save_specs("b", vec![spec(5002)], Some(0), false, false, &memory, 200).unwrap();
    let disk: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap(); assert_eq!(disk["host_identities"]["a"]["recordedAtUnixMs"], 150);
    memory.stamp_identity("a", revision + 1, 200); assert_eq!(memory.active_host_identity("a").unwrap().recorded_at_unix_ms, None);
    memory.stamp_identity("a", revision, 200); lock_store(&path).unwrap().persist_authority(&memory, 200).unwrap();
    assert_eq!(load_store(&path, 200).unwrap().active_host_identity("a").unwrap().recorded_at_unix_ms, Some(200));
    let mut stale: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap(); stale["host_revisions"]["a"] = Value::from(revision + 1); fs::write(&path, serde_json::to_vec(&stale).unwrap()).unwrap(); assert!(load_store(&path, 200).unwrap().active_host_identity("a").is_none());
    // A failed write leaves memory ahead; the next successful write reconciles max(file, memory).
    let mut ahead = memory.clone(); ahead.bump_host_revision("a", identity(), 250).unwrap(); ahead.stamp_identity("a", revision + 1, 250);
    let before_fault = digest(&path);
    let fault = WriteFault; assert!(lock_store(&path).unwrap().persist_authority_with_fault(&ahead, 250, Some(&fault)).is_err());
    assert_eq!(digest(&path), before_fault);
    lock_store(&path).unwrap().persist_authority(&ahead, 250).unwrap();
    let recovered = load_store(&path, 250).unwrap(); assert_eq!(recovered.host_revision("a"), revision + 1); assert_eq!(recovered.active_host_identity("a").unwrap().recorded_at_unix_ms, Some(250));
    // An older detached stamp must not overwrite a handover peer's newer revision.
    lock_store(&path).unwrap().persist_authority(&memory, 300).unwrap(); assert_eq!(load_store(&path, 300).unwrap().host_revision("a"), revision + 1);
}
#[test]
fn discard_invalid_is_scoped_to_the_saved_host() {
    let (_dir, path) = file(); let (_, b) = partial(&path);
    let first = save(&path, "a", vec![spec(5001)], Some(0), false, true).unwrap();
    assert!(!first.invalid_rows.contains_key("a")); assert_eq!(first.invalid_rows["b"], vec![b]); assert_eq!(first.store_status.invalid_hosts, vec!["b".to_owned()]); assert_eq!(first.store_status.kind, "partial"); assert_eq!(first.hosts["a"], vec![spec(5001)]);
    let second = save(&path, "b", vec![spec(5002)], Some(1), false, true).unwrap(); assert_eq!(second.store_status.kind, "ok"); assert!(second.store_status.invalid_hosts.is_empty());
}
#[test]
fn absent_discard_invalid_succeeds_and_preserves_both_hosts_raw_rows() {
    let (_dir, path) = file(); let (a, b) = partial(&path);
    let result = save(&path, "a", vec![spec(5001)], Some(0), false, false).unwrap();
    assert_eq!(result.invalid_rows["a"], vec![a.clone()]); assert_eq!(result.invalid_rows["b"], vec![b.clone()]);
    let text = fs::read_to_string(&path).unwrap(); assert!(text.contains(&a)); assert!(text.contains(&b));
    let mut clock = TestClock::new(100); assert_eq!(clock.now_unix_ms(), 100); clock.set(50); assert_eq!(clock.now_unix_ms(), 50);
}
