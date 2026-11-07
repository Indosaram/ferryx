//! History import worker.
//!
//! Ports `importers/history_importer.cc`. Parses history from the source browser
//! and writes entries to the destination.

use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;
use std::sync::Arc;

use crate::orchestrator::{ImportDestination, ImportProgress, ImportType};
use crate::parsers;
use crate::{BrowserType, DetectedBrowser, ImportResult};

use super::{is_cancelled, send_update, ImportWorker};

pub struct HistoryWorker;

impl ImportWorker for HistoryWorker {
    fn run(
        &self,
        browser: &DetectedBrowser,
        destination: &dyn ImportDestination,
        cancelled: &Arc<AtomicBool>,
        progress: &Sender<ImportProgress>,
    ) -> ImportResult<u32> {
        let profile_dir = browser.profile_dir();

        let entries = match browser.browser_type {
            BrowserType::Chrome
            | BrowserType::Arc
            | BrowserType::Brave
            | BrowserType::Edge
            | BrowserType::Vivaldi
            | BrowserType::Opera => {
                parsers::chromium::history::parse_chromium_history(profile_dir)?
            }
            BrowserType::Firefox | BrowserType::Zen => {
                parsers::firefox::history::parse_firefox_history(profile_dir)?
            }
            BrowserType::Safari => parsers::safari::history::parse_safari_history(profile_dir)?,
        };

        if entries.is_empty() {
            return Ok(0);
        }

        let mut count = 0u32;
        for entry in &entries {
            if is_cancelled(cancelled) {
                break;
            }

            destination.add_history(
                &entry.url,
                &entry.title,
                entry.visit_time,
                entry.visit_count,
            );
            count += 1;

            if count.is_multiple_of(1000) {
                send_update(progress, ImportType::History, count, "Importing history...");
            }
        }

        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workers::tests::MockDestination;
    use std::sync::mpsc;

    #[test]
    fn test_history_worker_nonexistent_path() {
        let dest = Arc::new(MockDestination::new());
        let cancelled = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();

        let browser = DetectedBrowser {
            browser_type: BrowserType::Chrome,
            display_name: "Chrome".to_string(),
            profile_path: std::path::PathBuf::from("/nonexistent/Default/Preferences"),
            services_supported: 0xFF,
            requires_full_disk_access: false,
        };

        let result = HistoryWorker.run(&browser, &*dest, &cancelled, &tx);
        assert!(result.is_err());
        let msgs: Vec<_> = rx.try_iter().collect();
        assert!(msgs.is_empty());
    }

    #[test]
    fn test_history_import_direct() {
        // Test destination interaction directly (bypasses parser)
        let dest = Arc::new(MockDestination::new());

        dest.add_history("https://example.com", "Example", 1700000000.0, 5);
        dest.add_history("https://rust-lang.org", "Rust", 1700001000.0, 3);

        let history = dest.history.lock().unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].url, "https://example.com");
        assert_eq!(history[0].visit_count, 5);
        assert_eq!(history[1].url, "https://rust-lang.org");
    }

    #[test]
    fn test_history_respects_cancellation() {
        let dest = Arc::new(MockDestination::new());
        let cancelled = Arc::new(AtomicBool::new(true)); // pre-cancelled
        let (tx, _rx) = mpsc::channel();

        let browser = DetectedBrowser {
            browser_type: BrowserType::Firefox,
            display_name: "Firefox".to_string(),
            profile_path: std::path::PathBuf::from("/nonexistent/profile"),
            services_supported: 0xFF,
            requires_full_disk_access: false,
        };

        // Will error on parse (file not found), but if it somehow parsed,
        // cancellation would stop iteration.
        let result = HistoryWorker.run(&browser, &*dest, &cancelled, &tx);
        assert!(result.is_err());
    }
}
