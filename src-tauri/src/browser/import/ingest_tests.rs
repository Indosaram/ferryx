use super::*;
use crate::browser::import::staging::{pending_dir, StagingWriter};
use crate::browser::library::bookmarks::BookmarkTree;
use crate::browser::vault::keystore::MemoryKeyStore;

/// A vault in a temp dir; `unavailable` makes the keystore refuse, as a locked Keychain does.
struct TestVault {
    dir: tempfile::TempDir,
    keystore: MemoryKeyStore,
}

impl TestVault {
    fn new() -> Self {
        Self { dir: tempfile::tempdir().unwrap(), keystore: MemoryKeyStore::default() }
    }

    fn unavailable() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
            keystore: MemoryKeyStore { unavailable: Some("no keychain".into()), ..Default::default() },
        }
    }

    fn open(&self) -> VaultStore {
        VaultStore::open(self.dir.path(), &self.keystore).unwrap()
    }
}

impl PasswordVault for TestVault {
    fn with_vault(
        &self,
        f: &mut dyn FnMut(&mut VaultStore) -> Result<(), VaultError>,
    ) -> Result<(), VaultError> {
        f(&mut VaultStore::open(self.dir.path(), &self.keystore)?)
    }
}

struct Fixture {
    _data: tempfile::TempDir,
    library: BrowserLibrary,
    pending: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let data = tempfile::tempdir().unwrap();
        let library = BrowserLibrary::new(data.path());
        let pending = pending_dir(data.path(), "default");
        Self { _data: data, library, pending }
    }

    fn stage(&self, source: &str, write: impl FnOnce(&StagingWriter)) {
        let writer = StagingWriter::new(self.pending.clone(), source);
        write(&writer);
        for data_type in [ImportDataType::Bookmarks, ImportDataType::History, ImportDataType::Passwords] {
            writer.finish(data_type).unwrap();
        }
    }

    fn tree(&self) -> BookmarkTree {
        self.library.with_bookmarks(|store| Ok(store.tree().clone())).unwrap()
    }

    fn files(&self) -> Vec<String> {
        std::fs::read_dir(&self.pending)
            .map(|entries| entries.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default()
    }
}

fn bookmark(title: &str, url: &str, folders: &[&str]) -> ImportedBookmark {
    ImportedBookmark {
        title: title.into(),
        url: url.into(),
        folder_path: folders.iter().map(|f| f.to_string()).collect(),
    }
}

fn visit(url: &str, at_unix: f64) -> ImportedHistoryEntry {
    ImportedHistoryEntry { url: url.into(), title: format!("Title {url}"), visited_at_unix: at_unix, visit_count: 2 }
}

fn login(origin: &str, username: &str, password: &str) -> ImportedPassword {
    ImportedPassword {
        origin_url: origin.into(),
        action_url: format!("{origin}/submit"),
        username: username.into(),
        password: password.into(),
    }
}

fn stage_everything(fixture: &Fixture) {
    fixture.stage("Chrome", |w| {
        w.add_bookmark(bookmark("Rust", "https://rust-lang.org/", &[])).unwrap();
        w.add_bookmark(bookmark("Docs", "https://docs.rs/", &["Work"])).unwrap();
        w.add_bookmark(bookmark("Settings", "chrome://settings", &[])).unwrap();
        w.add_history(visit("https://example.com/", 1_700_000_000.5)).unwrap();
        w.add_history(visit("https://example.com/", 1_700_000_100.0)).unwrap();
        w.add_history(visit("about:blank", 1_700_000_000.0)).unwrap();
        w.add_password(login("https://a.test", "alice", "S3NTINEL-a")).unwrap();
        w.add_password(login("https://b.test/login", "bob", "S3NTINEL-b")).unwrap();
        w.add_password(login("https://c.test", "carol", "")).unwrap();
    });
}

fn counts(outcomes: &[TypeIngest]) -> Vec<(ImportDataType, usize, usize, usize, Option<String>)> {
    outcomes
        .iter()
        .map(|o| (o.data_type, o.stored, o.duplicates, o.skipped, o.error.as_ref().map(|e| e.code.clone())))
        .collect()
}

#[test]
fn each_kind_reaches_its_store_and_the_staged_files_are_deleted() {
    let fixture = Fixture::new();
    let vault = TestVault::new();
    stage_everything(&fixture);
    assert_eq!(fixture.files().len(), 3);

    let outcomes = ingest_pending(&fixture.pending, &fixture.library, &vault);
    assert_eq!(
        counts(&outcomes),
        vec![
            (ImportDataType::Bookmarks, 2, 0, 1, None),
            (ImportDataType::History, 2, 0, 1, None),
            (ImportDataType::Passwords, 2, 0, 1, None),
        ]
    );
    assert!(fixture.files().is_empty(), "staged files remain: {:?}", fixture.files());

    let tree = fixture.tree();
    let imported = tree.folders.iter().find(|f| f.name == "Imported from Chrome").expect("source folder");
    assert_eq!(imported.parent_id, BAR_ROOT);
    let work = tree.folders.iter().find(|f| f.name == "Work").expect("nested folder");
    assert_eq!(work.parent_id, imported.id);
    assert!(tree.bookmarks.iter().any(|b| b.url == "https://docs.rs/" && b.parent_id == work.id));
    assert!(tree.bookmarks.iter().any(|b| b.url == "https://rust-lang.org/" && b.parent_id == imported.id));

    let history = fixture.library.with_history(|h| h.list("", None, 10)).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].url, "https://example.com/");
    assert_eq!(history[0].last_visited_at, 1_700_000_100_000);
    assert_eq!(history[0].title.as_deref(), Some("Title https://example.com/"));

    let store = vault.open();
    let saved = store.list(None);
    assert_eq!(saved.len(), 2);
    let bob = saved.iter().find(|item| item.username == "bob").unwrap();
    assert_eq!(bob.origin, "https://b.test");
    assert_eq!(store.credential(&bob.id).unwrap().password.as_str(), "S3NTINEL-b");
}

#[test]
fn a_second_import_of_the_same_data_adds_nothing_and_counts_duplicates() {
    let fixture = Fixture::new();
    let vault = TestVault::new();
    stage_everything(&fixture);
    ingest_pending(&fixture.pending, &fixture.library, &vault);

    stage_everything(&fixture);
    fixture.stage("Chrome", |w| {
        // Same origin and username with a new password updates the saved login.
        w.add_password(login("https://a.test", "alice", "S3NTINEL-a2")).unwrap();
        // Same url at a new time is a new visit.
        w.add_history(visit("https://example.com/", 1_700_000_200.0)).unwrap();
    });
    let outcomes = ingest_pending(&fixture.pending, &fixture.library, &vault);
    assert_eq!(
        counts(&outcomes),
        vec![
            (ImportDataType::Bookmarks, 0, 2, 1, None),
            (ImportDataType::History, 1, 2, 1, None),
            (ImportDataType::Passwords, 1, 2, 1, None),
        ]
    );
    assert_eq!(fixture.tree().bookmarks.len(), 2);
    assert_eq!(fixture.tree().folders.len(), 2);
    let history = fixture.library.with_history(|h| h.list("", None, 10)).unwrap();
    assert_eq!(history[0].last_visited_at, 1_700_000_200_000);
    let store = vault.open();
    assert_eq!(store.list(None).len(), 2);
    let alice = store.list(None).into_iter().find(|item| item.username == "alice").unwrap();
    assert_eq!(store.credential(&alice.id).unwrap().password.as_str(), "S3NTINEL-a2");
    assert!(fixture.files().is_empty());
}

#[test]
fn a_bookmark_already_in_the_same_folder_is_a_duplicate_but_another_folder_is_not() {
    let fixture = Fixture::new();
    let vault = TestVault::new();
    fixture.stage("Firefox", |w| {
        w.add_bookmark(bookmark("A", "https://a.example/", &["Toolbar"])).unwrap();
        w.add_bookmark(bookmark("A again", "https://a.example/", &["Toolbar"])).unwrap();
        w.add_bookmark(bookmark("A elsewhere", "https://a.example/", &["Other"])).unwrap();
    });
    let outcomes = ingest_pending(&fixture.pending, &fixture.library, &vault);
    assert_eq!(counts(&outcomes), vec![(ImportDataType::Bookmarks, 2, 1, 0, None)]);
}

#[test]
fn a_failing_store_still_deletes_every_staged_file_and_the_other_kinds_are_kept() {
    let fixture = Fixture::new();
    let vault = TestVault::unavailable();
    stage_everything(&fixture);
    // A torn line in the history file is skipped, not fatal.
    let history_file = fixture.files().into_iter().find(|name| name.starts_with("history-")).unwrap();
    let path = fixture.pending.join(history_file);
    let mut text = std::fs::read_to_string(&path).unwrap();
    text.push_str("{\"source\":\"Chrome\",\"url\":\n");
    std::fs::write(&path, text).unwrap();

    let outcomes = ingest_pending(&fixture.pending, &fixture.library, &vault);
    assert_eq!(
        counts(&outcomes),
        vec![
            (ImportDataType::Bookmarks, 2, 0, 1, None),
            (ImportDataType::History, 2, 0, 2, None),
            (ImportDataType::Passwords, 0, 0, 1, Some("vault-unavailable".to_string())),
        ]
    );
    assert!(fixture.files().is_empty(), "plaintext left behind: {:?}", fixture.files());
    assert_eq!(fixture.tree().bookmarks.len(), 2);
}

#[test]
fn a_partial_file_of_a_dead_process_is_deleted_and_one_of_this_process_is_left_alone() {
    let fixture = Fixture::new();
    let vault = TestVault::new();
    std::fs::create_dir_all(&fixture.pending).unwrap();
    let stale = fixture.pending.join("passwords-1-4294967295.jsonl.partial");
    std::fs::write(&stale, "{\"password\":\"S3NTINEL\"}\n").unwrap();
    let live = fixture.pending.join(format!("history-1-{}.jsonl.partial", std::process::id()));
    std::fs::write(&live, "").unwrap();

    let outcomes = ingest_pending(&fixture.pending, &fixture.library, &vault);
    assert!(outcomes.is_empty());
    assert!(!stale.exists());
    assert!(live.exists());
}

#[test]
fn a_missing_pending_dir_ingests_nothing() {
    let fixture = Fixture::new();
    assert!(ingest_pending(&fixture.pending, &fixture.library, &TestVault::new()).is_empty());
}

#[test]
fn shred_overwrites_before_deleting() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("passwords-1-1.jsonl");
    std::fs::write(&path, "S3NTINEL").unwrap();
    wipe(&path).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), vec![0u8; 8]);
    shred(&path).unwrap();
    assert!(!path.exists());
}

#[test]
fn apply_replaces_staged_counts_with_stored_counts_and_appends_leftovers() {
    let mut results = vec![ImportTypeResult {
        data_type: ImportDataType::Bookmarks,
        imported: 5,
        duplicates: 0,
        skipped: 1,
        notes: vec!["note".into()],
        error: None,
    }];
    let mut bookmarks = TypeIngest::new(ImportDataType::Bookmarks);
    bookmarks.stored = 3;
    bookmarks.duplicates = 2;
    let mut passwords = TypeIngest::new(ImportDataType::Passwords);
    passwords.stored = 1;
    passwords.fail(ImportTypeError::new("vault-unavailable", "no keychain"));
    apply(&mut results, vec![bookmarks, passwords]);

    assert_eq!((results[0].imported, results[0].duplicates, results[0].skipped), (3, 2, 1));
    assert_eq!(results[0].notes, vec!["note".to_string()]);
    assert_eq!(results[1].data_type, ImportDataType::Passwords);
    assert_eq!(results[1].error.as_ref().map(|e| e.code.as_str()), Some("vault-unavailable"));
    let wire = serde_json::to_value(&results[0]).unwrap();
    assert_eq!(wire["duplicates"], 2);
}
