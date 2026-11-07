//! Bookmark import worker.
//!
//! Ports `importers/bookmark_importer.cc`. Reads parsed bookmarks from the
//! appropriate parser and writes them to the destination.

use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;
use std::sync::Arc;

use crate::orchestrator::{ImportDestination, ImportProgress, ImportType};
use crate::parsers;
use crate::{BookmarkEntry, BrowserType, DetectedBrowser, ImportResult};

use super::{is_cancelled, send_update, ImportWorker};

pub struct BookmarkWorker;

impl ImportWorker for BookmarkWorker {
    fn run(
        &self,
        browser: &DetectedBrowser,
        destination: &dyn ImportDestination,
        cancelled: &Arc<AtomicBool>,
        progress: &Sender<ImportProgress>,
    ) -> ImportResult<u32> {
        // Arc and Zen use workspace import for their tabs/bookmarks; skip standalone bookmarks.
        if browser.browser_type == BrowserType::Arc || browser.browser_type == BrowserType::Zen {
            return Ok(0);
        }

        let profile_dir = browser.profile_dir();

        let bookmarks = match browser.browser_type {
            BrowserType::Chrome
            | BrowserType::Brave
            | BrowserType::Edge
            | BrowserType::Vivaldi
            | BrowserType::Opera => {
                parsers::chromium::bookmarks::parse_chromium_bookmarks(profile_dir)?
            }
            BrowserType::Firefox => {
                parsers::firefox::bookmarks::parse_firefox_bookmarks(profile_dir)?
            }
            BrowserType::Safari => parsers::safari::bookmarks::parse_safari_bookmarks(profile_dir)?,
            _ => return Ok(0),
        };

        if bookmarks.is_empty() {
            return Ok(0);
        }

        let mut count = 0u32;
        let path: Vec<String> = Vec::new();
        count = import_entries(&bookmarks, destination, cancelled, progress, &path, count);

        Ok(count)
    }
}

/// Recursively import bookmark entries.
fn import_entries(
    entries: &[BookmarkEntry],
    destination: &dyn ImportDestination,
    cancelled: &Arc<AtomicBool>,
    progress: &Sender<ImportProgress>,
    folder_path: &[String],
    mut count: u32,
) -> u32 {
    for entry in entries {
        if is_cancelled(cancelled) {
            break;
        }

        if entry.is_folder {
            let mut child_path = folder_path.to_vec();
            child_path.push(entry.title.clone());
            count = import_entries(
                &entry.children,
                destination,
                cancelled,
                progress,
                &child_path,
                count,
            );
        } else if !entry.url.is_empty() {
            destination.add_bookmark(&entry.title, &entry.url, folder_path);
            count += 1;
            if count.is_multiple_of(100) {
                send_update(
                    progress,
                    ImportType::Bookmarks,
                    count,
                    "Importing bookmarks...",
                );
            }
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workers::tests::MockDestination;
    use std::sync::mpsc;

    #[test]
    fn test_bookmark_worker_empty_input() {
        // With a nonexistent path, the parser will return an error.
        // The worker propagates the error, orchestrator handles it.
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

        let result = BookmarkWorker.run(&browser, &*dest, &cancelled, &tx);
        // Should error because file doesn't exist
        assert!(result.is_err());
        // No progress events sent for empty/error case
        let msgs: Vec<_> = rx.try_iter().collect();
        assert!(msgs.is_empty());
    }

    #[test]
    fn test_bookmark_worker_skips_arc_zen() {
        let dest = Arc::new(MockDestination::new());
        let cancelled = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();

        let browser = DetectedBrowser {
            browser_type: BrowserType::Arc,
            display_name: "Arc".to_string(),
            profile_path: std::path::PathBuf::from("/nonexistent/profile"),
            services_supported: 0xFF,
            requires_full_disk_access: false,
        };

        let result = BookmarkWorker.run(&browser, &*dest, &cancelled, &tx);
        assert_eq!(result.unwrap(), 0);
        let msgs: Vec<_> = rx.try_iter().collect();
        assert!(msgs.is_empty());
    }

    #[test]
    fn test_import_entries_with_mock() {
        let dest = Arc::new(MockDestination::new());
        let cancelled = Arc::new(AtomicBool::new(false));
        let (tx, _rx) = mpsc::channel();

        let entries = vec![
            BookmarkEntry {
                title: "Folder".to_string(),
                url: String::new(),
                is_folder: true,
                children: vec![BookmarkEntry {
                    title: "Sub Page".to_string(),
                    url: "https://sub.example.com".to_string(),
                    is_folder: false,
                    children: vec![],
                }],
            },
            BookmarkEntry {
                title: "Page".to_string(),
                url: "https://example.com".to_string(),
                is_folder: false,
                children: vec![],
            },
        ];

        let count = import_entries(&entries, &*dest, &cancelled, &tx, &[], 0);
        assert_eq!(count, 2);

        let bookmarks = dest.bookmarks.lock().unwrap();
        assert_eq!(bookmarks.len(), 2);
        assert_eq!(bookmarks[0].title, "Sub Page");
        assert_eq!(bookmarks[0].folder_path, vec!["Folder"]);
        assert_eq!(bookmarks[1].title, "Page");
        assert!(bookmarks[1].folder_path.is_empty());
    }
}
