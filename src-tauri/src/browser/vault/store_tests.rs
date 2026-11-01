use super::*;
use crate::browser::vault::keystore::MemoryKeyStore;

fn open(dir: &Path, keystore: &MemoryKeyStore) -> VaultStore {
    VaultStore::open(dir, keystore).expect("vault opens")
}

fn read_file(dir: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(dir.join(STORE_FILE)).unwrap()).unwrap()
}

#[test]
fn a_saved_login_survives_reopening_and_is_encrypted_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    let keystore = MemoryKeyStore::default();
    let saved = open(dir.path(), &keystore)
        .save("https://example.com", "alice@example.com", "S3NTINEL-pw")
        .unwrap();

    let raw = std::fs::read_to_string(dir.path().join(STORE_FILE)).unwrap();
    assert!(!raw.contains("S3NTINEL-pw"));
    assert!(!raw.contains("alice@example.com"));
    assert_eq!(keystore.slots.lock().len(), 1);

    let reopened = open(dir.path(), &keystore);
    assert_eq!(reopened.list(None), vec![saved.clone()]);
    let credential = reopened.credential(&saved.id).unwrap();
    assert_eq!(credential.username.as_str(), "alice@example.com");
    assert_eq!(credential.password.as_str(), "S3NTINEL-pw");
}

#[test]
fn the_same_username_updates_and_a_new_username_adds() {
    let dir = tempfile::tempdir().unwrap();
    let keystore = MemoryKeyStore::default();
    let mut store = open(dir.path(), &keystore);
    let origin = canonicalize_origin("https://example.com").unwrap();
    assert_eq!(store.classify(&origin, "a", "1").unwrap(), SaveKind::New);
    let first = store.save("https://example.com", "a", "pw-1").unwrap();
    assert_eq!(store.classify(&origin, "a", "pw-1").unwrap(), SaveKind::Unchanged);
    assert_eq!(store.classify(&origin, "a", "pw-2").unwrap(), SaveKind::Update { id: first.id.clone() });
    let updated = store.save("https://example.com", "a", "pw-2").unwrap();
    assert_eq!(updated.id, first.id);
    assert_eq!(store.credential(&first.id).unwrap().password.as_str(), "pw-2");
    store.save("https://example.com", "b", "pw-3").unwrap();
    assert_eq!(store.list(None).len(), 2);
}

#[test]
fn matches_are_exact_origin_only() {
    let dir = tempfile::tempdir().unwrap();
    let keystore = MemoryKeyStore::default();
    let mut store = open(dir.path(), &keystore);
    let saved = store.save("https://example.com", "a", "pw").unwrap();
    assert_eq!(store.matches("https://example.com/login?x=1"), vec![saved]);
    for page in [
        "http://example.com/login",
        "https://example.com:8443/",
        "https://sub.example.com/",
        "https://example.com.evil.test/",
        "about:blank",
    ] {
        assert!(store.matches(page).is_empty(), "{page} must not match");
    }
}

#[test]
fn list_searches_origin_and_username_and_delete_persists() {
    let dir = tempfile::tempdir().unwrap();
    let keystore = MemoryKeyStore::default();
    let mut store = open(dir.path(), &keystore);
    let a = store.save("https://mail.test", "alice", "pw").unwrap();
    store.save("https://bank.test", "bob", "pw").unwrap();
    assert_eq!(store.list(Some("MAIL")).len(), 1);
    assert_eq!(store.list(Some("bob")).len(), 1);
    assert_eq!(store.list(Some("  ")).len(), 2);
    store.delete(&a.id).unwrap();
    assert!(matches!(store.delete(&a.id), Err(VaultError::NotFound(_))));
    assert_eq!(open(dir.path(), &keystore).list(None).len(), 1);
}

#[test]
fn update_keeps_the_password_when_none_is_given() {
    let dir = tempfile::tempdir().unwrap();
    let keystore = MemoryKeyStore::default();
    let mut store = open(dir.path(), &keystore);
    let saved = store.save("https://example.com", "a", "pw").unwrap();
    let renamed = store.update(&saved.id, "b", None).unwrap();
    assert_eq!(renamed.username, "b");
    assert_eq!(store.credential(&saved.id).unwrap().password.as_str(), "pw");
    assert!(matches!(store.update(&saved.id, "b", Some("")), Err(VaultError::InvalidArgument(_))));
}

#[test]
fn a_wrong_key_in_the_keystore_reports_corruption_and_keeps_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let keystore = MemoryKeyStore::default();
    open(dir.path(), &keystore).save("https://example.com", "a", "pw").unwrap();
    let before = std::fs::read(dir.path().join(STORE_FILE)).unwrap();
    for slot in keystore.slots.lock().values_mut() {
        *slot = vec![9; 32];
    }
    assert!(matches!(VaultStore::open(dir.path(), &keystore), Err(VaultError::Corrupted(_))));
    assert_eq!(std::fs::read(dir.path().join(STORE_FILE)).unwrap(), before);
}

#[test]
fn a_missing_or_malformed_key_reports_corruption() {
    let dir = tempfile::tempdir().unwrap();
    let keystore = MemoryKeyStore::default();
    open(dir.path(), &keystore).save("https://example.com", "a", "pw").unwrap();
    for slot in keystore.slots.lock().values_mut() {
        *slot = vec![1; 7];
    }
    assert!(matches!(VaultStore::open(dir.path(), &keystore), Err(VaultError::Corrupted(_))));
    keystore.slots.lock().clear();
    assert!(matches!(VaultStore::open(dir.path(), &keystore), Err(VaultError::Corrupted(_))));
}

#[test]
fn a_corrupted_store_file_is_reported_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let keystore = MemoryKeyStore::default();
    std::fs::write(dir.path().join(STORE_FILE), b"{not json").unwrap();
    assert!(matches!(VaultStore::open(dir.path(), &keystore), Err(VaultError::Corrupted(_))));
    assert_eq!(std::fs::read(dir.path().join(STORE_FILE)).unwrap(), b"{not json");
}

#[test]
fn a_flipped_ciphertext_byte_or_a_moved_record_fails_authentication() {
    let dir = tempfile::tempdir().unwrap();
    let keystore = MemoryKeyStore::default();
    {
        let mut store = open(dir.path(), &keystore);
        store.save("https://example.com", "a", "pw").unwrap();
        store.save("https://other.test", "b", "pw2").unwrap();
    }
    let pristine = read_file(dir.path());

    let mut flipped = pristine.clone();
    let byte = flipped["items"][0]["record"]["ciphertext"][0].as_u64().unwrap();
    flipped["items"][0]["record"]["ciphertext"][0] = serde_json::json!((byte + 1) % 256);
    std::fs::write(dir.path().join(STORE_FILE), serde_json::to_vec(&flipped).unwrap()).unwrap();
    assert!(matches!(VaultStore::open(dir.path(), &keystore), Err(VaultError::Corrupted(_))));

    // A record moved to another origin (an attacker editing the file to get a fill elsewhere).
    let mut moved = pristine;
    moved["items"][0]["origin"] = serde_json::json!("https://evil.test");
    std::fs::write(dir.path().join(STORE_FILE), serde_json::to_vec(&moved).unwrap()).unwrap();
    assert!(matches!(VaultStore::open(dir.path(), &keystore), Err(VaultError::Corrupted(_))));
}

#[test]
fn no_keystore_means_unavailable_and_no_file() {
    let dir = tempfile::tempdir().unwrap();
    let keystore = MemoryKeyStore { unavailable: Some("no secret service".into()), ..Default::default() };
    assert!(matches!(VaultStore::open(dir.path(), &keystore), Err(VaultError::Unavailable(_))));
    assert!(!dir.path().join(STORE_FILE).exists());
}

#[test]
fn bad_inputs_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let keystore = MemoryKeyStore::default();
    let mut store = open(dir.path(), &keystore);
    assert!(matches!(store.save("https://example.com/login", "a", "pw"), Err(VaultError::InvalidArgument(_))));
    assert!(matches!(store.save("file:///x", "a", "pw"), Err(VaultError::InvalidArgument(_))));
    assert!(matches!(store.save("https://example.com", "a", ""), Err(VaultError::InvalidArgument(_))));
    assert!(matches!(store.credential("nope"), Err(VaultError::NotFound(_))));
}
