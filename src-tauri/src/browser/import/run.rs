//! Runs an import: picks the source browser, runs each requested data type through the
//! ported workers and collects one [`ImportTypeResult`] per type.

use super::cookie_sink::{CookieDelivery, CookieSink};
use super::staging::StagingWriter;
use super::{
    CookieEntry, DetectedBrowser, ImportDataType, ImportReport, ImportSink, ImportTypeError,
    ImportTypeResult, ImportedBookmark, ImportedHistoryEntry, ImportedPassword, SinkError,
    SinkOutcome,
};
use ferryx_browser_import::orchestrator::{ImportDestination, ImportProgress, ImportType, Orchestrator};
use ferryx_browser_import::{BrowserType, PasswordEntry};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

/// A browser profile Ferryx can import from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSource {
    /// Stable within a machine: `<browser>:<profile directory name>`.
    pub id: String,
    pub browser: String,
    pub name: String,
    pub types: Vec<ImportDataType>,
    pub requires_full_disk_access: bool,
    /// Passwords come from a file the user exports (Safari).
    pub passwords_need_file: bool,
}

fn browser_key(browser_type: BrowserType) -> &'static str {
    match browser_type {
        BrowserType::Chrome => "chrome",
        BrowserType::Arc => "arc",
        BrowserType::Brave => "brave",
        BrowserType::Edge => "edge",
        BrowserType::Vivaldi => "vivaldi",
        BrowserType::Opera => "opera",
        BrowserType::Firefox => "firefox",
        BrowserType::Zen => "zen",
        BrowserType::Safari => "safari",
    }
}

fn worker_type(data_type: ImportDataType) -> ImportType {
    match data_type {
        ImportDataType::Bookmarks => ImportType::Bookmarks,
        ImportDataType::History => ImportType::History,
        ImportDataType::Cookies => ImportType::Cookies,
        ImportDataType::Passwords => ImportType::Passwords,
    }
}

/// The types this source can provide. Arc and Zen keep their bookmarks as sidebar spaces,
/// which Ferryx does not import, so they offer no bookmarks.
fn source_types(browser: &DetectedBrowser) -> Vec<ImportDataType> {
    ImportDataType::ALL
        .into_iter()
        .filter(|data_type| browser.services_supported & worker_type(*data_type).to_bit() != 0)
        .filter(|data_type| {
            !(*data_type == ImportDataType::Bookmarks
                && matches!(browser.browser_type, BrowserType::Arc | BrowserType::Zen))
        })
        .collect()
}

pub fn source_id(browser: &DetectedBrowser) -> String {
    let dir = browser
        .profile_dir()
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    format!("{}:{dir}", browser_key(browser.browser_type))
}

pub fn describe(browser: &DetectedBrowser) -> ImportSource {
    ImportSource {
        id: source_id(browser),
        browser: browser_key(browser.browser_type).to_string(),
        name: browser.display_name.clone(),
        types: source_types(browser),
        requires_full_disk_access: browser.requires_full_disk_access,
        passwords_need_file: browser.browser_type == BrowserType::Safari,
    }
}

/// The sink of one Ferryx profile: staging files plus the cookie sink.
pub struct ProfileSink {
    staging: StagingWriter,
    cookies: CookieSink,
}

impl ProfileSink {
    pub fn new(staging: StagingWriter, cookies: CookieSink) -> Self {
        Self { staging, cookies }
    }
}

fn cookie_notes(delivery: &CookieDelivery) -> Vec<String> {
    let mut notes = Vec::new();
    if delivery.google_skipped > 0 {
        notes.push(format!(
            "{} Google cookies were left out; sign in to Google again in Ferryx",
            delivery.google_skipped
        ));
    }
    if delivery.staged_for_engine && !delivery.engine_running {
        notes.push("The Chromium engine applies the cookies when this profile next opens".into());
    }
    notes
}

impl ImportSink for ProfileSink {
    fn add_bookmark(&self, bookmark: ImportedBookmark) -> Result<(), SinkError> {
        self.staging.add_bookmark(bookmark)
    }

    fn add_history(&self, entry: ImportedHistoryEntry) -> Result<(), SinkError> {
        self.staging.add_history(entry)
    }

    fn add_cookie(&self, cookie: &CookieEntry) -> Result<bool, SinkError> {
        self.cookies.add(cookie)
    }

    fn add_password(&self, password: ImportedPassword) -> Result<(), SinkError> {
        self.staging.add_password(password)
    }

    fn finish(&self, data_type: ImportDataType) -> Result<SinkOutcome, SinkError> {
        if data_type != ImportDataType::Cookies {
            let kept = self.staging.finish(data_type)?;
            return Ok(SinkOutcome { kept, ..Default::default() });
        }
        let delivery = self.cookies.finish()?;
        if delivery.cookies > 0 && !delivery.staged_for_engine && delivery.webviews == 0 {
            return Err(SinkError::NoDestination(
                "cookies: open a tab in this profile, then import again",
            ));
        }
        Ok(SinkOutcome {
            kept: delivery.cookies,
            notes: cookie_notes(&delivery),
            // Google cookies were already counted as skipped when `add_cookie` refused them.
            ..Default::default()
        })
    }

    fn discard(&self, data_type: ImportDataType) {
        if data_type == ImportDataType::Cookies {
            self.cookies.discard();
        } else {
            self.staging.discard(data_type);
        }
    }
}

/// Adapts the ported workers' destination calls to an [`ImportSink`], remembering the first
/// sink failure (the workers only see a bool) and how many records were not importable.
struct Adapter {
    sink: Arc<dyn ImportSink>,
    error: Mutex<Option<SinkError>>,
    skipped: Mutex<usize>,
}

impl Adapter {
    fn record<T>(&self, result: Result<T, SinkError>) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(error) => {
                let mut first = self.error.lock();
                if first.is_none() {
                    *first = Some(error);
                }
                None
            }
        }
    }

    fn start_type(&self) {
        *self.error.lock() = None;
        *self.skipped.lock() = 0;
    }
}

impl ImportDestination for Adapter {
    fn create_space(&self, _: &str, _: &str, _: &str) -> Option<String> {
        None
    }
    fn create_tab(&self, _: &str, _: &str, _: &str) -> Option<String> {
        None
    }
    fn create_tab_in_folder(&self, _: &str, _: &str, _: &str, _: &str) -> Option<String> {
        None
    }
    fn create_folder(&self, _: &str, _: &str, _: &str) -> Option<String> {
        None
    }
    fn pin_tab(&self, _: &str) {}
    fn favorite_tab(&self, _: &str) {}
    fn activate_space(&self, _: &str) {}
    fn get_active_space_id(&self) -> Option<String> {
        None
    }
    fn add_bookmark(&self, title: &str, url: &str, folder_path: &[String]) -> Option<String> {
        let bookmark = ImportedBookmark {
            title: title.to_string(),
            url: url.to_string(),
            folder_path: folder_path.to_vec(),
        };
        self.record(self.sink.add_bookmark(bookmark)).map(|()| String::new())
    }
    fn add_history(&self, url: &str, title: &str, visit_time: f64, visit_count: u32) -> bool {
        let entry = ImportedHistoryEntry {
            url: url.to_string(),
            title: title.to_string(),
            visited_at_unix: visit_time,
            visit_count,
        };
        self.record(self.sink.add_history(entry)).is_some()
    }
    fn add_cookie(&self, cookie: &CookieEntry) -> bool {
        match self.record(self.sink.add_cookie(cookie)) {
            Some(true) => true,
            Some(false) => {
                *self.skipped.lock() += 1;
                false
            }
            None => false,
        }
    }
    fn add_autofill(&self, _: &str, _: &str, _: i32, _: i64, _: i64) -> bool {
        false
    }
    fn add_favicon(&self, _: &str, _: &[u8]) -> bool {
        false
    }
    fn add_password(&self, entry: PasswordEntry) -> bool {
        let password = ImportedPassword {
            origin_url: entry.origin_url,
            action_url: entry.action_url,
            username: entry.username,
            password: entry.password,
        };
        self.record(self.sink.add_password(password)).is_some()
    }
}

/// One import run. [`ImportRun::cancel`] may be called from another thread while
/// [`ImportRun::run`] works; the type in progress stops at its next record and is discarded.
pub struct ImportRun {
    orchestrator: Orchestrator,
    adapter: Arc<Adapter>,
}

fn cancelled_result(data_type: ImportDataType) -> ImportTypeResult {
    ImportTypeResult {
        data_type,
        imported: 0,
        duplicates: 0,
        skipped: 0,
        notes: Vec::new(),
        error: Some(ImportTypeError::new("cancelled", "the import was cancelled")),
    }
}

impl ImportRun {
    pub fn new(sink: Arc<dyn ImportSink>, password_csv_path: Option<PathBuf>) -> Self {
        let adapter = Arc::new(Adapter { sink, error: Mutex::new(None), skipped: Mutex::new(0) });
        let mut orchestrator = Orchestrator::new(Arc::clone(&adapter) as Arc<dyn ImportDestination>);
        if let Some(path) = password_csv_path {
            orchestrator.set_password_csv_path(path);
        }
        Self { orchestrator, adapter }
    }

    pub fn cancel(&self) {
        self.orchestrator.cancel();
    }

    pub fn run(&self, browser: &DetectedBrowser, types: &[ImportDataType], has_password_file: bool) -> ImportReport {
        let offered = source_types(browser);
        let results = ImportDataType::ALL
            .into_iter()
            .filter(|data_type| types.contains(data_type))
            .map(|data_type| {
                if !offered.contains(&data_type) {
                    return ImportTypeResult {
                        data_type,
                        imported: 0,
                        duplicates: 0,
                        skipped: 0,
                        notes: Vec::new(),
                        error: Some(ImportTypeError::new(
                            "unsupported",
                            format!("{} has no {} to import", browser.display_name, data_type.as_str()),
                        )),
                    };
                }
                if data_type == ImportDataType::Passwords
                    && browser.browser_type == BrowserType::Safari
                    && !has_password_file
                {
                    return ImportTypeResult {
                        data_type,
                        imported: 0,
                        duplicates: 0,
                        skipped: 0,
                        notes: Vec::new(),
                        error: Some(ImportTypeError::new(
                            "no-file",
                            "export Safari passwords to a CSV file, then choose that file",
                        )),
                    };
                }
                self.run_type(browser, data_type)
            })
            .collect();
        ImportReport { results }
    }

    fn run_type(&self, browser: &DetectedBrowser, data_type: ImportDataType) -> ImportTypeResult {
        if self.orchestrator.is_cancelled() {
            return cancelled_result(data_type);
        }
        self.adapter.start_type();
        let (progress, events) = std::sync::mpsc::channel();
        let outcome = self.orchestrator.run_type(worker_type(data_type), browser, &progress);
        drop(progress);
        let mut notes: Vec<String> = events
            .try_iter()
            .filter_map(|event| match event {
                ImportProgress::Summary { message, .. } => Some(message),
                _ => None,
            })
            .collect();
        let sink = &self.adapter.sink;
        let skipped = *self.adapter.skipped.lock();
        let failure = match outcome {
            Err(error) => Some(ImportTypeError::from_import(&error)),
            Ok(_) if self.orchestrator.is_cancelled() => {
                sink.discard(data_type);
                return cancelled_result(data_type);
            }
            Ok(_) => self.adapter.error.lock().take().map(|error| ImportTypeError::from_sink(&error)),
        };
        if let Some(error) = failure {
            sink.discard(data_type);
            return ImportTypeResult { data_type, imported: 0, duplicates: 0, skipped, notes, error: Some(error) };
        }
        match sink.finish(data_type) {
            Ok(kept) => {
                notes.extend(kept.notes);
                ImportTypeResult {
                    data_type,
                    imported: kept.kept,
                    duplicates: 0,
                    skipped: skipped + kept.skipped,
                    notes,
                    error: None,
                }
            }
            Err(error) => ImportTypeResult {
                data_type,
                imported: 0,
                duplicates: 0,
                skipped,
                notes,
                error: Some(ImportTypeError::from_sink(&error)),
            },
        }
    }
}

/// Every importable browser profile on this machine. Blocking (directory scans).
pub fn detect_sources() -> Vec<(ImportSource, DetectedBrowser)> {
    ferryx_browser_import::detect::detect_installed_browsers()
        .into_iter()
        .map(|browser| (describe(&browser), browser))
        .filter(|(source, _)| !source.types.is_empty())
        .collect()
}

/// Password-manager exports Ferryx reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PasswordFileFormat {
    OnePasswordCsv,
    OnePasswordPux,
    BitwardenCsv,
    BitwardenOrganizationCsv,
    BitwardenJson,
    ApplePasswordsCsv,
    KeepassxcCsv,
    KeepassCsv,
}

impl PasswordFileFormat {
    fn source_format(self) -> ferryx_browser_import::PasswordImportSourceFormat {
        use ferryx_browser_import::PasswordImportSourceFormat as F;
        match self {
            Self::OnePasswordCsv => F::OnePasswordCsv,
            Self::OnePasswordPux => F::OnePasswordPux,
            Self::BitwardenCsv => F::BitwardenIndividualCsv,
            Self::BitwardenOrganizationCsv => F::BitwardenOrganizationCsv,
            Self::BitwardenJson => F::BitwardenJson,
            Self::ApplePasswordsCsv => F::ApplePasswordsCsv,
            Self::KeepassxcCsv => F::KeePassXcCsv,
            Self::KeepassCsv => F::KeePassClassicCsv,
        }
    }
}

/// Largest password export Ferryx reads (a 1PUX with attachments can be large).
const MAX_PASSWORD_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// Reads a password-manager export into `sink`. Blocking (file I/O).
pub fn import_password_file(
    sink: &dyn ImportSink,
    format: PasswordFileFormat,
    path: &std::path::Path,
) -> ImportTypeResult {
    let fail = |error: ImportTypeError| ImportTypeResult {
        data_type: ImportDataType::Passwords,
        imported: 0,
        duplicates: 0,
        skipped: 0,
        notes: Vec::new(),
        error: Some(error),
    };
    let io = |error: std::io::Error| {
        ImportTypeError::from_import(&ferryx_browser_import::sqlite_snapshot::io_error(path, &error))
    };
    match std::fs::metadata(path) {
        Ok(meta) if meta.len() > MAX_PASSWORD_FILE_BYTES => {
            return fail(ImportTypeError::new("parse", "the file is too large for a password export"));
        }
        Ok(_) => {}
        Err(error) => return fail(io(error)),
    }
    let bytes = match std::fs::read(path) {
        Ok(bytes) => ferryx_browser_import::Zeroizing::new(bytes),
        Err(error) => return fail(io(error)),
    };
    let parsed = match ferryx_browser_import::parse_password_import_source_file(format.source_format(), &bytes) {
        Ok(parsed) => parsed,
        Err(error) => return fail(ImportTypeError::from_import(&error)),
    };
    for credential in parsed.credentials {
        let password = ImportedPassword {
            origin_url: credential.origin_url,
            action_url: String::new(),
            username: credential.username,
            password: credential.password.to_string(),
        };
        if let Err(error) = sink.add_password(password) {
            sink.discard(ImportDataType::Passwords);
            return fail(ImportTypeError::from_sink(&error));
        }
    }
    match sink.finish(ImportDataType::Passwords) {
        Ok(kept) => ImportTypeResult {
            data_type: ImportDataType::Passwords,
            imported: kept.kept,
            duplicates: 0,
            skipped: parsed.preview.skipped,
            notes: parsed.preview.safe_messages,
            error: None,
        },
        Err(error) => fail(ImportTypeError::from_sink(&error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferryx_browser_import::ImportServices;

    /// Records what reaches the sink; `fail_history` makes history writes fail.
    #[derive(Default)]
    struct RecordingSink {
        bookmarks: Mutex<Vec<ImportedBookmark>>,
        history: Mutex<Vec<ImportedHistoryEntry>>,
        finished: Mutex<Vec<ImportDataType>>,
        discarded: Mutex<Vec<ImportDataType>>,
        fail_history: bool,
    }

    impl ImportSink for RecordingSink {
        fn add_bookmark(&self, bookmark: ImportedBookmark) -> Result<(), SinkError> {
            self.bookmarks.lock().push(bookmark);
            Ok(())
        }
        fn add_history(&self, entry: ImportedHistoryEntry) -> Result<(), SinkError> {
            if self.fail_history {
                return Err(SinkError::Failed("disk full".into()));
            }
            self.history.lock().push(entry);
            Ok(())
        }
        fn add_cookie(&self, _: &CookieEntry) -> Result<bool, SinkError> {
            Ok(true)
        }
        fn add_password(&self, _: ImportedPassword) -> Result<(), SinkError> {
            Ok(())
        }
        fn finish(&self, data_type: ImportDataType) -> Result<SinkOutcome, SinkError> {
            self.finished.lock().push(data_type);
            let kept = match data_type {
                ImportDataType::Bookmarks => self.bookmarks.lock().len(),
                ImportDataType::History => self.history.lock().len(),
                _ => 0,
            };
            Ok(SinkOutcome { kept, ..Default::default() })
        }
        fn discard(&self, data_type: ImportDataType) {
            self.discarded.lock().push(data_type);
        }
    }

    fn chrome_profile() -> (tempfile::TempDir, DetectedBrowser) {
        let root = tempfile::tempdir().unwrap();
        let profile = root.path().join("Default");
        std::fs::create_dir_all(&profile).unwrap();
        let bookmarks = serde_json::json!({
            "roots": {
                "bookmark_bar": { "children": [
                    { "type": "url", "name": "Rust", "url": "https://rust-lang.org/" },
                    { "type": "folder", "name": "Work", "children": [
                        { "type": "url", "name": "Docs", "url": "https://docs.rs/" }
                    ]}
                ]}
            }
        });
        std::fs::write(profile.join("Bookmarks"), bookmarks.to_string()).unwrap();
        let conn = rusqlite::Connection::open(profile.join("History")).unwrap();
        conn.execute_batch(
            "CREATE TABLE urls (id INTEGER PRIMARY KEY, url TEXT, title TEXT, visit_count INTEGER, last_visit_time INTEGER);
             INSERT INTO urls (url, title, visit_count, last_visit_time)
             VALUES ('https://example.com/', 'Example', 4, 13350000000000000);",
        )
        .unwrap();
        drop(conn);
        let browser = DetectedBrowser {
            browser_type: BrowserType::Chrome,
            display_name: "Chrome".into(),
            profile_path: profile,
            services_supported: ImportServices::BOOKMARKS | ImportServices::HISTORY | ImportServices::COOKIES,
            requires_full_disk_access: false,
        };
        (root, browser)
    }

    #[test]
    fn bookmarks_and_history_reach_the_sink_and_are_finished() {
        let (_root, browser) = chrome_profile();
        let sink = Arc::new(RecordingSink::default());
        let run = ImportRun::new(Arc::clone(&sink) as Arc<dyn ImportSink>, None);
        let report = run.run(&browser, &[ImportDataType::History, ImportDataType::Bookmarks], false);

        let summary: Vec<_> = report
            .results
            .iter()
            .map(|r| (r.data_type, r.imported, r.error.clone()))
            .collect();
        assert_eq!(
            summary,
            vec![(ImportDataType::Bookmarks, 2, None), (ImportDataType::History, 1, None)]
        );
        assert_eq!(sink.bookmarks.lock()[1].folder_path, vec!["Work".to_string()]);
        let visit = &sink.history.lock()[0];
        assert_eq!(visit.url, "https://example.com/");
        assert_eq!(visit.visit_count, 4);
        // 13350000000000000 µs since 1601 = 1_705_526_400 s since 1970.
        assert!((visit.visited_at_unix - 1_705_526_400.0).abs() < 1.0);
        assert_eq!(*sink.finished.lock(), vec![ImportDataType::Bookmarks, ImportDataType::History]);
    }

    #[test]
    fn one_failing_type_does_not_stop_the_others() {
        let (root, browser) = chrome_profile();
        std::fs::remove_file(root.path().join("Default").join("Bookmarks")).unwrap();
        let sink = Arc::new(RecordingSink { fail_history: true, ..Default::default() });
        let run = ImportRun::new(Arc::clone(&sink) as Arc<dyn ImportSink>, None);
        let report = run.run(
            &browser,
            &[ImportDataType::Bookmarks, ImportDataType::History, ImportDataType::Passwords],
            false,
        );
        let codes: Vec<_> = report
            .results
            .iter()
            .map(|r| (r.data_type, r.error.as_ref().map(|e| e.code.clone())))
            .collect();
        assert_eq!(
            codes,
            vec![
                (ImportDataType::Bookmarks, Some("not-found".to_string())),
                (ImportDataType::History, Some("sink-failed".to_string())),
                (ImportDataType::Passwords, Some("unsupported".to_string())),
            ]
        );
        assert_eq!(*sink.discarded.lock(), vec![ImportDataType::Bookmarks, ImportDataType::History]);
        assert!(sink.finished.lock().is_empty());
    }

    #[test]
    fn a_cancelled_run_imports_nothing_more() {
        let (_root, browser) = chrome_profile();
        let sink = Arc::new(RecordingSink::default());
        let run = ImportRun::new(Arc::clone(&sink) as Arc<dyn ImportSink>, None);
        run.cancel();
        let report = run.run(&browser, &[ImportDataType::Bookmarks], false);
        assert_eq!(report.results[0].error.as_ref().map(|e| e.code.as_str()), Some("cancelled"));
        assert!(sink.bookmarks.lock().is_empty());
    }

    #[test]
    fn safari_passwords_without_a_file_ask_for_one() {
        let browser = DetectedBrowser {
            browser_type: BrowserType::Safari,
            display_name: "Safari".into(),
            profile_path: PathBuf::from("/nonexistent/Library/Safari/Bookmarks.plist"),
            services_supported: ImportServices::PASSWORDS,
            requires_full_disk_access: true,
        };
        let source = describe(&browser);
        assert_eq!(source.id, "safari:Safari");
        assert!(source.passwords_need_file && source.requires_full_disk_access);
        let run = ImportRun::new(Arc::new(RecordingSink::default()), None);
        let report = run.run(&browser, &[ImportDataType::Passwords], false);
        assert_eq!(report.results[0].error.as_ref().map(|e| e.code.as_str()), Some("no-file"));
    }

    #[test]
    fn arc_offers_no_bookmarks() {
        let browser = DetectedBrowser {
            browser_type: BrowserType::Arc,
            display_name: "Arc".into(),
            profile_path: PathBuf::from("/nonexistent/Arc/User Data/Default"),
            services_supported: ImportServices::BOOKMARKS | ImportServices::HISTORY,
            requires_full_disk_access: false,
        };
        assert_eq!(describe(&browser).types, vec![ImportDataType::History]);
    }

    fn fixture(relative: &str) -> PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../crates/ferryx-browser-import/test_data/fixtures/password_import_sources")
            .join(relative)
    }

    fn staging_sink(dir: &std::path::Path) -> ProfileSink {
        ProfileSink::new(
            StagingWriter::new(dir.to_path_buf(), "Bitwarden JSON"),
            CookieSink::new(
                None,
                Box::new(|_: &std::path::Path| false),
                Box::new(|_: &[cookie::Cookie<'static>]| -> Result<usize, String> { Ok(0) }),
            ),
        )
    }

    #[test]
    fn a_bitwarden_json_export_is_staged_as_passwords() {
        let root = tempfile::tempdir().unwrap();
        let dir = super::super::staging::pending_dir(root.path(), "default");
        let sink = staging_sink(&dir);
        let result = import_password_file(&sink, PasswordFileFormat::BitwardenJson, &fixture("bitwarden/valid_mixed.json"));
        assert_eq!(result.error, None);
        assert!(result.imported > 0);
        let files: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
        assert_eq!(files.len(), 1);
        let text = std::fs::read_to_string(files[0].path()).unwrap();
        assert_eq!(text.lines().count(), result.imported);
        let first: serde_json::Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
        assert_eq!(first["source"], "Bitwarden JSON");
        assert!(first["password"].as_str().is_some_and(|p| !p.is_empty()));
    }

    #[test]
    fn a_malformed_export_is_a_parse_error_and_stages_nothing() {
        let root = tempfile::tempdir().unwrap();
        let dir = super::super::staging::pending_dir(root.path(), "default");
        let sink = staging_sink(&dir);
        let result = import_password_file(&sink, PasswordFileFormat::BitwardenJson, &fixture("bitwarden/malformed_json.txt"));
        assert_eq!(result.error.map(|e| e.code), Some("parse".to_string()));
        assert!(!dir.exists());
    }
}
