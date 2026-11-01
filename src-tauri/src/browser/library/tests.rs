use super::bookmarks::{BookmarkStore, ImportedBookmark, LoadOutcome, BAR_ROOT, OTHER_ROOT};
use super::history::{frecency, HistoryStore, ImportedVisit};
use super::recorder::{HistoryRecorder, RecordAction};
use super::suggest::{parse_open_search, suggest_url};
use crate::browser::{BrowserManager, BrowserProfileId, CreateBrowserRequest};

const DAY: i64 = 86_400_000;

fn store_in(dir: &tempfile::TempDir) -> (BookmarkStore, LoadOutcome) {
    BookmarkStore::open(BookmarkStore::default_path(dir.path()))
}

#[test]
fn bookmarks_add_move_delete_survive_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let (mut store, outcome) = store_in(&dir);
    assert_eq!(outcome, LoadOutcome::Fresh);
    let folder = store.create_folder("Work", BAR_ROOT).unwrap();
    let a = store.add_bookmark("A", "https://a.example/", BAR_ROOT).unwrap();
    let b = store.add_bookmark("", "https://b.example/", BAR_ROOT).unwrap();
    assert_eq!(b.title, "https://b.example/");
    store.move_item(&b.id, &folder.id, None).unwrap();
    store.move_item(&a.id, BAR_ROOT, Some(0)).unwrap();
    store.update(&a.id, Some("A2"), None).unwrap();
    store.save().unwrap();

    let (mut reopened, outcome) = store_in(&dir);
    assert_eq!(outcome, LoadOutcome::Loaded);
    assert_eq!(reopened.children(BAR_ROOT), vec![(a.id.clone(), false), (folder.id.clone(), true)]);
    assert_eq!(reopened.children(&folder.id), vec![(b.id.clone(), false)]);
    assert_eq!(reopened.search("a2", 10).len(), 1);

    reopened.delete(&folder.id).unwrap();
    reopened.save().unwrap();
    let (again, _) = store_in(&dir);
    assert!(again.tree().folders.is_empty());
    assert_eq!(again.tree().bookmarks.len(), 1);
}

#[test]
fn bookmarks_reject_bad_urls_and_folder_cycles() {
    let dir = tempfile::tempdir().unwrap();
    let (mut store, _) = store_in(&dir);
    assert!(store.add_bookmark("x", "javascript:alert(1)", BAR_ROOT).is_err());
    assert!(store.add_bookmark("x", "https://x.example/", "missing").is_err());
    let outer = store.create_folder("Outer", OTHER_ROOT).unwrap();
    let inner = store.create_folder("Inner", &outer.id).unwrap();
    assert!(store.move_item(&outer.id, &inner.id, None).is_err());
}

#[test]
fn corrupted_bookmark_file_recovers_from_backup() {
    let dir = tempfile::tempdir().unwrap();
    let path = BookmarkStore::default_path(dir.path());
    let (mut store, _) = store_in(&dir);
    store.add_bookmark("Kept", "https://kept.example/", BAR_ROOT).unwrap();
    store.save().unwrap();
    // A second save rotates the first good copy into the backup.
    store.add_bookmark("Second", "https://second.example/", BAR_ROOT).unwrap();
    store.save().unwrap();
    std::fs::write(&path, b"{ torn").unwrap();

    let (recovered, outcome) = store_in(&dir);
    assert_eq!(outcome, LoadOutcome::RecoveredFromBackup);
    assert_eq!(recovered.tree().bookmarks.len(), 1);
    assert_eq!(recovered.tree().bookmarks[0].title, "Kept");
}

#[test]
fn unreadable_store_and_backup_are_moved_aside() {
    let dir = tempfile::tempdir().unwrap();
    let path = BookmarkStore::default_path(dir.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"garbage").unwrap();
    let (store, outcome) = store_in(&dir);
    assert_eq!(outcome, LoadOutcome::ResetCorrupt);
    assert!(store.tree().bookmarks.is_empty());
    assert!(!path.exists());
    let kept = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .filter_map(Result::ok)
        .any(|entry| entry.file_name().to_string_lossy().starts_with("bookmarks.json.corrupt-"));
    assert!(kept);
}

#[test]
fn bookmark_import_builds_folders_and_skips_duplicates() {
    let dir = tempfile::tempdir().unwrap();
    let (mut store, _) = store_in(&dir);
    let entries = vec![
        ImportedBookmark { title: "Rust".into(), url: "https://rust-lang.org/".into(), folder_path: vec!["Dev".into(), "Lang".into()] },
        ImportedBookmark { title: "Rust".into(), url: "https://rust-lang.org/".into(), folder_path: vec!["Dev".into(), "Lang".into()] },
        ImportedBookmark { title: "Top".into(), url: "https://top.example/".into(), folder_path: vec![] },
        ImportedBookmark { title: "Bad".into(), url: "chrome://settings".into(), folder_path: vec![] },
    ];
    assert_eq!(store.import(OTHER_ROOT, &entries).unwrap(), 2);
    assert_eq!(store.tree().folders.len(), 2);
    assert!(store.import("nowhere", &entries).is_err());
}

#[test]
fn history_records_counts_and_never_records_private_tabs() {
    let mut history = HistoryStore::open_in_memory().unwrap();
    assert!(history.record_visit("https://a.example/", Some("A"), 1_000, false).unwrap());
    assert!(history.record_visit("https://a.example/", None, 2_000, false).unwrap());
    assert!(!history.record_visit("https://secret.example/", Some("S"), 3_000, true).unwrap());
    assert!(!history.record_visit("about:blank", None, 3_000, false).unwrap());
    let all = history.list("", None, 100).unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].visit_count, 2);
    assert_eq!(all[0].title.as_deref(), Some("A"));
    assert_eq!(all[0].last_visited_at, 2_000);
}

#[test]
fn history_search_delete_and_clear_range() {
    let mut history = HistoryStore::open_in_memory().unwrap();
    history.record_visit("https://docs.example/rust", Some("Rust docs"), 1_000, false).unwrap();
    history.record_visit("https://news.example/", Some("News"), 5_000, false).unwrap();
    history.record_visit("https://news.example/", Some("News"), 9_000, false).unwrap();
    history.record_visit("https://100%.example/", None, 6_000, false).unwrap();
    assert_eq!(history.list("rust docs", None, 10).unwrap().len(), 1);
    assert_eq!(history.list("100%", None, 10).unwrap().len(), 1);
    assert_eq!(history.list("", Some(6_000), 10).unwrap().len(), 1);

    // Clearing [4000, 7000) removes the 100% page and one news visit; news keeps its later visit.
    assert_eq!(history.clear_range(4_000, 7_000).unwrap(), 1);
    let left = history.list("", None, 10).unwrap();
    assert_eq!(left.iter().map(|e| e.url.as_str()).collect::<Vec<_>>(), vec!["https://news.example/", "https://docs.example/rust"]);
    // Clearing the window holding news' last visit recomputes it from the remaining visits.
    history.clear_range(8_000, 10_000).unwrap();
    assert_eq!(history.list("news", None, 10).unwrap().len(), 0);

    assert!(history.delete_url("https://docs.example/rust").unwrap());
    assert!(history.list("", None, 10).unwrap().is_empty());
}

#[test]
fn history_suggest_ranks_by_frecency() {
    let mut history = HistoryStore::open_in_memory().unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    history
        .import(&[
            ImportedVisit { url: "https://old.example/".into(), title: None, visited_at: now - 200 * DAY, visit_count: 20 },
            ImportedVisit { url: "https://fresh.example/".into(), title: None, visited_at: now - DAY, visit_count: 3 },
        ])
        .unwrap();
    let ranked = history.suggest("example", 5).unwrap();
    assert_eq!(ranked[0].url, "https://fresh.example/");
    assert!(frecency(3, now - DAY, now) > frecency(20, now - 200 * DAY, now));
}

#[test]
fn history_meta_marks_one_time_migration() {
    let dir = tempfile::tempdir().unwrap();
    let path = HistoryStore::default_path(dir.path());
    {
        let mut history = HistoryStore::open(&path).unwrap();
        assert_eq!(history.meta("migrated").unwrap(), None);
        history.set_meta("migrated", "1").unwrap();
    }
    let history = HistoryStore::open(&path).unwrap();
    assert_eq!(history.meta("migrated").unwrap().as_deref(), Some("1"));
}

fn session(manager: &BrowserManager, id: &str, profile: Option<BrowserProfileId>) {
    manager
        .register_session(CreateBrowserRequest {
            browser_id: Some(id.into()),
            workspace_id: None,
            worktree_path: None,
            url: "about:blank".into(),
            profile,
            zoom_factor: None,
            bounds: None,
            visible: None,
        })
        .unwrap();
}

#[test]
fn recorder_records_one_visit_per_settled_url_then_titles() {
    let manager = BrowserManager::new();
    session(&manager, "b1", None);
    let recorder = HistoryRecorder::default();
    let settle = |url: Option<&str>, title: Option<&str>, loading: bool| {
        manager
            .update_navigation_state("b1", url.map(Into::into), title.map(Into::into), Some(loading), None, None, None)
            .unwrap()
    };
    // Disabled until the UI reports the history setting.
    assert_eq!(recorder.observe(&settle(Some("https://a.example/"), None, false)), None);
    recorder.set_enabled(true);
    assert_eq!(recorder.observe(&settle(Some("https://b.example/"), None, true)), None);
    assert_eq!(
        recorder.observe(&settle(None, None, false)),
        Some(RecordAction::Visit { url: "https://b.example/".into(), title: None })
    );
    assert_eq!(
        recorder.observe(&settle(None, Some("B"), false)),
        Some(RecordAction::Title { url: "https://b.example/".into(), title: "B".into() })
    );
    assert_eq!(recorder.observe(&settle(None, Some("B"), false)), None);
}

#[test]
fn recorder_ignores_private_profile() {
    let manager = BrowserManager::new();
    session(&manager, "p1", Some(BrowserProfileId::Private));
    let recorder = HistoryRecorder::default();
    recorder.set_enabled(true);
    let state = manager
        .update_navigation_state("p1", Some("https://private.example/".into()), Some("P".into()), Some(false), None, None, None)
        .unwrap();
    assert_eq!(recorder.observe(&state), None);
}

#[test]
fn suggest_urls_and_open_search_parsing() {
    let url = suggest_url("duckduckgo", "a b&c").unwrap();
    assert!(url.starts_with("https://duckduckgo.com/ac/?q=a+b%26c"));
    assert!(suggest_url("unknown", "x").is_none());
    assert_eq!(
        parse_open_search(br#"["rus",["rust","rust lang"," "]]"#, 5),
        Some(vec!["rust".to_string(), "rust lang".to_string()])
    );
    assert_eq!(parse_open_search(b"<html>", 5), None);
}
