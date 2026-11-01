//! Favicon import worker.
//!
//! Ports `importers/favicon_importer.cc`. Reads favicons from the source browser
//! and writes them to the destination.

use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;
use std::sync::Arc;

use crate::orchestrator::{ImportDestination, ImportProgress, ImportType};
use crate::parsers;
use crate::{BrowserType, DetectedBrowser, ImportResult};

use super::{is_cancelled, send_update, ImportWorker};

pub struct FaviconWorker;

impl ImportWorker for FaviconWorker {
    fn run(
        &self,
        browser: &DetectedBrowser,
        destination: &dyn ImportDestination,
        cancelled: &Arc<AtomicBool>,
        progress: &Sender<ImportProgress>,
    ) -> ImportResult<u32> {
        let profile_dir = browser.profile_dir();

        let favicons: Vec<(Vec<String>, Vec<u8>)> = match browser.browser_type {
            BrowserType::Chrome
            | BrowserType::Arc
            | BrowserType::Brave
            | BrowserType::Edge
            | BrowserType::Vivaldi
            | BrowserType::Opera => {
                parsers::chromium::favicons::parse_chromium_favicons(profile_dir)?
                    .into_iter()
                    .map(|e| (e.page_urls, e.image_data))
                    .collect()
            }
            BrowserType::Safari => parsers::safari::favicon::parse_safari_favicons(profile_dir)?
                .into_iter()
                .map(|e| (e.page_urls, e.image_data))
                .collect(),
            BrowserType::Firefox | BrowserType::Zen => {
                parsers::firefox::favicon::parse_firefox_favicons(profile_dir)?
                    .into_iter()
                    .map(|e| (e.page_urls, e.image_data))
                    .collect()
            }
        };

        if favicons.is_empty() {
            return Ok(0);
        }

        let mut count = 0u32;
        for (page_urls, image_data) in &favicons {
            if is_cancelled(cancelled) {
                break;
            }
            for page_url in page_urls {
                destination.add_favicon(page_url, image_data);
            }
            count += 1;
            if count.is_multiple_of(100) {
                send_update(
                    progress,
                    ImportType::Favicons,
                    count,
                    "Importing favicons...",
                );
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
    fn test_favicon_worker_nonexistent_safari() {
        let dest = Arc::new(MockDestination::new());
        let cancelled = Arc::new(AtomicBool::new(false));
        let (tx, _rx) = mpsc::channel();

        let browser = DetectedBrowser {
            browser_type: BrowserType::Safari,
            display_name: "Safari".to_string(),
            profile_path: std::path::PathBuf::from("/nonexistent/Library/Safari/Bookmarks.plist"),
            services_supported: 0xFF,
            requires_full_disk_access: false,
        };

        let result = FaviconWorker.run(&browser, &*dest, &cancelled, &tx);
        assert!(result.is_err());
    }

    #[test]
    fn test_favicon_destination_interaction() {
        let dest = Arc::new(MockDestination::new());

        let png_data = vec![0x89, 0x50, 0x4E, 0x47]; // PNG magic bytes (mock)
        dest.add_favicon("https://example.com", &png_data);

        let favicons = dest.favicons.lock().unwrap();
        assert_eq!(favicons.len(), 1);
        assert_eq!(favicons[0].url, "https://example.com");
        assert_eq!(favicons[0].data_len, 4);
    }

    #[test]
    fn test_favicon_worker_nonexistent_chromium() {
        let dest = Arc::new(MockDestination::new());
        let cancelled = Arc::new(AtomicBool::new(false));
        let (tx, _rx) = mpsc::channel();

        let browser = DetectedBrowser {
            browser_type: BrowserType::Chrome,
            display_name: "Chrome".to_string(),
            profile_path: std::path::PathBuf::from("/nonexistent/Default/Preferences"),
            services_supported: 0xFF,
            requires_full_disk_access: false,
        };

        let result = FaviconWorker.run(&browser, &*dest, &cancelled, &tx);
        assert!(result.is_err());
    }

    #[test]
    fn test_favicon_worker_imports_firefox() {
        use rusqlite::Connection;

        let tmp = tempfile::tempdir().unwrap();
        let profile = tmp.path();
        let conn = Connection::open(profile.join("favicons.sqlite")).unwrap();
        conn.execute_batch(
            "CREATE TABLE moz_icons (id INTEGER PRIMARY KEY, icon_url TEXT NOT NULL, data BLOB);
             CREATE TABLE moz_pages_w_icons (id INTEGER PRIMARY KEY, page_url TEXT NOT NULL);
             CREATE TABLE moz_icons_to_pages (page_id INTEGER NOT NULL, icon_id INTEGER NOT NULL);",
        )
        .unwrap();
        let png: Vec<u8> = vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00,
            0x00, 0x90, 0x77, 0x53, 0xDE,
        ];
        conn.execute(
            "INSERT INTO moz_icons (id, icon_url, data) VALUES (1, 'https://ff.com/fav', ?1)",
            [&png],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO moz_pages_w_icons (id, page_url) VALUES (1, 'https://ff.com')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO moz_icons_to_pages (page_id, icon_id) VALUES (1, 1)",
            [],
        )
        .unwrap();
        drop(conn);

        let dest = Arc::new(MockDestination::new());
        let cancelled = Arc::new(AtomicBool::new(false));
        let (tx, _rx) = mpsc::channel();

        let browser = DetectedBrowser {
            browser_type: BrowserType::Firefox,
            display_name: "Firefox".to_string(),
            profile_path: profile.join("prefs.js"),
            services_supported: 0xFF,
            requires_full_disk_access: false,
        };

        let count = FaviconWorker
            .run(&browser, &*dest, &cancelled, &tx)
            .unwrap();
        assert_eq!(count, 1);
        let favicons = dest.favicons.lock().unwrap();
        assert_eq!(favicons.len(), 1);
        assert_eq!(favicons[0].url, "https://ff.com");
    }
}
