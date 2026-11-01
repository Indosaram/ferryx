//! Autofill import worker.
//!
//! Ports `importers/autofill_importer.cc`. Reads autofill entries from the
//! source browser and writes them to the destination.

use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;
use std::sync::Arc;

use crate::orchestrator::{ImportDestination, ImportProgress, ImportType};
use crate::parsers;
use crate::{BrowserType, DetectedBrowser, ImportResult};

use super::{is_cancelled, send_update, ImportWorker};

pub struct AutofillWorker;

impl ImportWorker for AutofillWorker {
    fn run(
        &self,
        browser: &DetectedBrowser,
        destination: &dyn ImportDestination,
        cancelled: &Arc<AtomicBool>,
        progress: &Sender<ImportProgress>,
    ) -> ImportResult<u32> {
        // Firefox/Zen/Safari autofill not supported (matches C++ behavior).
        match browser.browser_type {
            BrowserType::Firefox | BrowserType::Zen | BrowserType::Safari => {
                return Ok(0);
            }
            _ => {}
        }

        let profile_dir = browser.profile_dir();

        let entries = parsers::chromium::autofill::parse_chromium_autofill(profile_dir)?;

        if entries.is_empty() {
            return Ok(0);
        }

        let mut count = 0u32;
        for entry in &entries {
            if is_cancelled(cancelled) {
                break;
            }
            destination.add_autofill(
                &entry.field_name,
                &entry.value,
                entry.times_used,
                entry.first_used,
                entry.last_used,
            );
            count += 1;
            if count.is_multiple_of(200) {
                send_update(
                    progress,
                    ImportType::Autofill,
                    count,
                    "Importing autofill...",
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
    fn test_autofill_worker_skips_firefox() {
        let dest = Arc::new(MockDestination::new());
        let cancelled = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();

        let browser = DetectedBrowser {
            browser_type: BrowserType::Firefox,
            display_name: "Firefox".to_string(),
            profile_path: std::path::PathBuf::from("/nonexistent/profile"),
            services_supported: 0xFF,
            requires_full_disk_access: false,
        };

        let result = AutofillWorker.run(&browser, &*dest, &cancelled, &tx);
        assert_eq!(result.unwrap(), 0);
        let msgs: Vec<_> = rx.try_iter().collect();
        assert!(msgs.is_empty());
    }

    #[test]
    fn test_autofill_worker_skips_safari() {
        let dest = Arc::new(MockDestination::new());
        let cancelled = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();

        let browser = DetectedBrowser {
            browser_type: BrowserType::Safari,
            display_name: "Safari".to_string(),
            profile_path: std::path::PathBuf::from("/nonexistent/profile"),
            services_supported: 0xFF,
            requires_full_disk_access: false,
        };

        let result = AutofillWorker.run(&browser, &*dest, &cancelled, &tx);
        assert_eq!(result.unwrap(), 0);
        let msgs: Vec<_> = rx.try_iter().collect();
        assert!(msgs.is_empty());
    }

    #[test]
    fn test_autofill_destination_interaction() {
        let dest = Arc::new(MockDestination::new());

        dest.add_autofill("email", "user@example.com", 10, 1000000, 2000000);
        dest.add_autofill("name", "John", 5, 500000, 1500000);

        let entries = dest.autofill.lock().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].field_name, "email");
        assert_eq!(entries[0].value, "user@example.com");
        assert_eq!(entries[1].field_name, "name");
    }
}
