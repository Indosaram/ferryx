//! Essential sites import worker.
//!
//! Ports `importers/essential_importer.cc`. Favorites selected essential sites
//! in the currently active space (Welcome flow).

use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;
use std::sync::Arc;

use crate::orchestrator::{ImportDestination, ImportProgress};
use crate::{DetectedBrowser, ImportResult};

use super::{is_cancelled, ImportWorker};

pub struct EssentialWorker {
    urls: Vec<String>,
}

impl EssentialWorker {
    pub fn new(urls: Vec<String>) -> Self {
        Self { urls }
    }
}

impl ImportWorker for EssentialWorker {
    fn run(
        &self,
        _browser: &DetectedBrowser,
        destination: &dyn ImportDestination,
        cancelled: &Arc<AtomicBool>,
        _progress: &Sender<ImportProgress>,
    ) -> ImportResult<u32> {
        if self.urls.is_empty() {
            return Ok(0);
        }

        let space_id = match destination.get_active_space_id() {
            Some(id) if !id.is_empty() => id,
            _ => return Ok(0),
        };

        let mut count = 0u32;
        for url_str in &self.urls {
            if is_cancelled(cancelled) {
                break;
            }
            // Basic URL validation — must have a scheme.
            if !url_str.contains("://") {
                continue;
            }
            if let Some(tab_id) = destination.create_tab(&space_id, url_str, "") {
                destination.favorite_tab(&tab_id);
                count += 1;
            }
        }

        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrator::ImportDestination;
    use crate::workers::tests::MockDestination;
    use crate::{BrowserType, DetectedBrowser};
    use std::sync::mpsc;

    fn make_browser() -> DetectedBrowser {
        DetectedBrowser {
            browser_type: BrowserType::Chrome,
            display_name: "Chrome".to_string(),
            profile_path: std::path::PathBuf::from("/nonexistent/profile"),
            services_supported: 0xFF,
            requires_full_disk_access: false,
        }
    }

    #[test]
    fn test_essential_worker_empty_urls() {
        let dest: Arc<dyn ImportDestination> = Arc::new(MockDestination::new());
        let cancelled = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();

        let worker = EssentialWorker::new(vec![]);
        let result = worker.run(&make_browser(), &*dest, &cancelled, &tx);
        assert_eq!(result.unwrap(), 0);
        let msgs: Vec<_> = rx.try_iter().collect();
        assert!(msgs.is_empty());
    }

    #[test]
    fn test_essential_worker_valid_urls() {
        let mock = Arc::new(MockDestination::new());
        let dest: Arc<dyn ImportDestination> = Arc::clone(&mock) as _;
        let cancelled = Arc::new(AtomicBool::new(false));
        let (tx, _rx) = mpsc::channel();

        let urls = vec![
            "https://google.com".to_string(),
            "https://github.com".to_string(),
            "invalid-no-scheme".to_string(),
            "https://rust-lang.org".to_string(),
        ];

        let worker = EssentialWorker::new(urls);
        let result = worker.run(&make_browser(), &*dest, &cancelled, &tx);
        assert_eq!(result.unwrap(), 3);

        let favorited = mock.favorited.lock().unwrap();
        assert_eq!(favorited.len(), 3);

        let tabs = mock.tabs.lock().unwrap();
        assert_eq!(tabs.len(), 3);
        assert_eq!(tabs[0].url, "https://google.com");
        assert_eq!(tabs[1].url, "https://github.com");
        assert_eq!(tabs[2].url, "https://rust-lang.org");
    }

    #[test]
    fn test_essential_worker_no_active_space() {
        let mock = Arc::new(MockDestination::new());
        *mock.active_space.lock().unwrap() = None;
        let dest: Arc<dyn ImportDestination> = Arc::clone(&mock) as _;

        let cancelled = Arc::new(AtomicBool::new(false));
        let (tx, _rx) = mpsc::channel();

        let urls = vec!["https://google.com".to_string()];
        let worker = EssentialWorker::new(urls);
        let result = worker.run(&make_browser(), &*dest, &cancelled, &tx);
        assert_eq!(result.unwrap(), 0);

        let tabs = mock.tabs.lock().unwrap();
        assert!(tabs.is_empty());
    }

    #[test]
    fn test_essential_worker_respects_cancellation() {
        let dest: Arc<dyn ImportDestination> = Arc::new(MockDestination::new());
        let cancelled = Arc::new(AtomicBool::new(true));
        let (tx, _rx) = mpsc::channel();

        let urls = vec![
            "https://google.com".to_string(),
            "https://github.com".to_string(),
        ];

        let worker = EssentialWorker::new(urls);
        let result = worker.run(&make_browser(), &*dest, &cancelled, &tx);
        assert_eq!(result.unwrap(), 0);
    }
}
