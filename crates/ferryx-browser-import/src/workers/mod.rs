//! Import workers — one per data type.
//!
//! Each worker implements `ImportWorker` and handles parsing + writing to the
//! `ImportDestination`. Workers NEVER send `AllComplete` (INV-7).

pub mod autofill;
pub mod bookmark;
pub mod cookie;
pub mod essential;
pub mod favicon;
pub mod history;
pub mod password;
pub mod workspace;
mod workspace_folders;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;

use crate::orchestrator::{ImportProgress, ImportType};
use crate::{DetectedBrowser, ImportResult};

use super::orchestrator::ImportDestination;

/// Trait for import workers. Each worker handles one data type.
pub trait ImportWorker {
    /// Run the import. Returns the number of items imported.
    ///
    /// Workers may send `Update` progress events but MUST NOT send
    /// `AllComplete` or `TypeComplete` — that's the orchestrator's job (INV-7).
    fn run(
        &self,
        browser: &DetectedBrowser,
        destination: &dyn ImportDestination,
        cancelled: &Arc<AtomicBool>,
        progress: &Sender<ImportProgress>,
    ) -> ImportResult<u32>;
}

/// Helper: check if cancelled.
#[inline]
pub(crate) fn is_cancelled(flag: &AtomicBool) -> bool {
    flag.load(Ordering::SeqCst)
}

/// Helper: send a progress update (non-fatal if receiver dropped).
#[inline]
pub(crate) fn send_update(
    progress: &Sender<ImportProgress>,
    import_type: ImportType,
    imported: u32,
    status: &str,
) {
    let _ = progress.send(ImportProgress::Update {
        import_type,
        imported,
        status: status.to_string(),
    });
}

/// Maps a Chromium-family browser to its macOS Keychain `Safe Storage`
/// service/account pair. The derived AES-128 key decrypts BOTH cookies and
/// `Login Data` passwords (same `v10` pipeline), so cookie and password workers
/// share this mapping. Returns `None` for non-Chromium browsers.
#[cfg(target_os = "macos")]
pub(crate) fn keychain_service_account(
    browser_type: crate::BrowserType,
) -> Option<(&'static str, &'static str)> {
    use crate::BrowserType;
    match browser_type {
        BrowserType::Chrome => Some(("Chrome Safe Storage", "Chrome")),
        BrowserType::Brave => Some(("Brave Safe Storage", "Brave")),
        BrowserType::Arc => Some(("Arc Safe Storage", "Arc")),
        BrowserType::Edge => Some(("Microsoft Edge Safe Storage", "Microsoft Edge")),
        BrowserType::Vivaldi => Some(("Vivaldi Safe Storage", "Vivaldi")),
        BrowserType::Opera => Some(("Opera Safe Storage", "Opera")),
        _ => None,
    }
}

// allow: SIZE_OK — Lane K exact allowlist mandates inline tests in workers/mod.rs without structural/build edits

#[cfg(target_os = "macos")]
static TEST_KEYCHAIN_PROVIDER: std::sync::RwLock<
    Option<std::sync::Arc<dyn crate::decrypt::chromium_keychain::KeychainProvider + Send + Sync>>,
> = std::sync::RwLock::new(None);

#[cfg(target_os = "macos")]
static TEST_KEYCHAIN_TIMEOUT_OVERRIDE: std::sync::RwLock<Option<std::time::Duration>> =
    std::sync::RwLock::new(None);

/// Test-only hook to inject a KeychainProvider override for automated regression testing.
#[doc(hidden)]
#[cfg(all(target_os = "macos", test))]
pub(crate) fn set_keychain_provider_override_for_test(
    provider: Option<
        std::sync::Arc<dyn crate::decrypt::chromium_keychain::KeychainProvider + Send + Sync>,
    >,
) -> Option<std::sync::Arc<dyn crate::decrypt::chromium_keychain::KeychainProvider + Send + Sync>> {
    let mut lock = TEST_KEYCHAIN_PROVIDER.write().unwrap();
    let old = lock.clone();
    *lock = provider;
    old
}

/// Test-only hook to inject a timeout override for automated regression testing.
#[doc(hidden)]
#[cfg(all(target_os = "macos", test))]
pub(crate) fn set_keychain_timeout_override_for_test(timeout: Option<std::time::Duration>) {
    let mut lock = TEST_KEYCHAIN_TIMEOUT_OVERRIDE.write().unwrap();
    *lock = timeout;
}

/// Fetches a Keychain-derived Chromium key through the process-wide
/// single-flight coordinator (design U11).
///
/// macOS Security framework's `SecKeychainFindGenericPassword` blocks the
/// caller indefinitely when the process is ad-hoc signed / lacks a stable ACL
/// (the system prompt may be hidden behind the welcome window).
///
/// U11 contract: at most ONE native Keychain worker executes globally. Callers
/// requesting the SAME (service, account) key while an attempt is in flight
/// SHARE that attempt (each with their own deadline; an expired caller simply
/// stops waiting and NEVER retires the active slot). A caller requesting a
/// DIFFERENT key while the slot is busy gets `None` immediately without
/// queueing another thread. The slot reopens only after the executing worker
/// completes. The shared attempt carries the Keychain password (zeroizing) and
/// each waiter derives its own `ChromiumKey`, so derived secrets stay scoped
/// to the attempt and its waiters. Thread spawn failures are reported and
/// retire the attempt instead of being swallowed as successful empty results.
#[cfg(target_os = "macos")]
pub(crate) fn fetch_keychain_key_cancellable(
    service: &'static str,
    account: &'static str,
    timeout: Option<std::time::Duration>,
    cancelled: Option<&std::sync::atomic::AtomicBool>,
) -> Option<crate::decrypt::chromium_keychain::ChromiumKey> {
    use crate::decrypt::chromium_keychain::{derive_aes_key, KeychainProvider, MacOsKeychain};

    let test_provider = TEST_KEYCHAIN_PROVIDER.read().unwrap().clone();
    let effective_timeout = match *TEST_KEYCHAIN_TIMEOUT_OVERRIDE.read().unwrap() {
        Some(t) => Some(t),
        None => timeout,
    };
    let deadline = effective_timeout.map(|t| std::time::Instant::now() + t);

    let attempt = match keychain_single_flight::coordinator().admit(service, account) {
        keychain_single_flight::Admission::Leader(attempt) => {
            // This caller installed a fresh attempt: spawn the one native
            // worker allowed to execute for it.
            let worker_attempt = std::sync::Arc::clone(&attempt);
            let spawned = std::thread::Builder::new()
                .name(format!("ferryx-import-keychain-{service}"))
                .spawn(move || {
                    let password = match &test_provider {
                        Some(provider) => provider.get_password(service, account).ok(),
                        None => MacOsKeychain.get_password(service, account).ok(),
                    };
                    worker_attempt.finish(password);
                    keychain_single_flight::coordinator().retire(service, account);
                });
            if let Err(spawn_error) = spawned {
                // Report the failure; nothing will ever complete this attempt,
                // so retire it and fail every waiter explicitly.
                tracing::warn!(%spawn_error, "keychain worker thread spawn failed");
                attempt.finish(None);
                keychain_single_flight::coordinator().retire(service, account);
                return None;
            }
            attempt
        }
        keychain_single_flight::Admission::Follower(attempt) => attempt,
        keychain_single_flight::Admission::SlotBusy => {
            // A different key is executing globally: unavailable immediately,
            // without queueing another native thread.
            return None;
        }
    };

    // Each caller waits on the shared attempt with its OWN deadline / cancellation.
    let resolved = attempt.wait_for_password(deadline, cancelled)?;
    let password = resolved?;
    Some(derive_aes_key(password.as_slice()))
}

#[cfg(all(target_os = "macos", test))]
pub(crate) fn fetch_keychain_key_with_timeout(
    service: &'static str,
    account: &'static str,
    timeout: std::time::Duration,
) -> Option<crate::decrypt::chromium_keychain::ChromiumKey> {
    fetch_keychain_key_cancellable(service, account, Some(timeout), None)
}

/// Process-wide single-flight coordination for native Keychain workers.
#[cfg(target_os = "macos")]
mod keychain_single_flight {
    use std::collections::HashMap;
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::Instant;
    use zeroize::Zeroizing;

    /// One in-flight Keychain attempt shared by every caller of the same
    /// (service, account) key. The Keychain password — not the derived key,
    /// which zeroizes per owner — is shared, so each waiter derives its own
    /// `ChromiumKey` and no derived secret outlives the attempt's waiters.
    pub(super) struct SharedAttempt {
        result: Mutex<Option<Option<Zeroizing<Vec<u8>>>>>,
        completed: Condvar,
    }

    impl SharedAttempt {
        fn new() -> Self {
            Self {
                result: Mutex::new(None),
                completed: Condvar::new(),
            }
        }

        /// Publishes the outcome exactly once and wakes every waiter.
        pub(super) fn finish(&self, password: Option<Zeroizing<Vec<u8>>>) {
            let mut guard = self.result.lock().unwrap();
            if guard.is_none() {
                *guard = Some(password);
            }
            self.completed.notify_all();
        }

        /// Waits for completion, caller cancellation, or the caller deadline.
        /// `None` means the caller cancelled or its deadline expired.
        pub(super) fn wait_for_password(
            &self,
            deadline: Option<Instant>,
            cancelled: Option<&std::sync::atomic::AtomicBool>,
        ) -> Option<Option<Zeroizing<Vec<u8>>>> {
            let mut guard = self.result.lock().unwrap();
            loop {
                if let Some(resolved) = &*guard {
                    return Some(resolved.clone());
                }
                if let Some(c) = cancelled {
                    if c.load(std::sync::atomic::Ordering::Relaxed) {
                        return None;
                    }
                }
                let now = Instant::now();
                if let Some(dl) = deadline {
                    if now >= dl {
                        return None;
                    }
                }
                let chunk_dur = std::time::Duration::from_millis(250);
                let wait_dur = match deadline {
                    Some(dl) => (dl - now).min(chunk_dur),
                    None => chunk_dur,
                };
                let (next, wait_result) =
                    self.completed.wait_timeout(guard, wait_dur).unwrap();
                guard = next;
                if wait_result.timed_out() && guard.is_none() {
                    if let Some(dl) = deadline {
                        if Instant::now() >= dl {
                            return None;
                        }
                    }
                }
            }
        }
    }

    /// Mutex-protected slot: `None` while idle, otherwise the single globally
    /// executing attempt keyed by (service, account).
    pub(super) struct Coordinator {
        active: Mutex<HashMap<(String, String), Arc<SharedAttempt>>>,
        #[cfg(test)]
        retired: Condvar,
    }

    impl Coordinator {
        fn new() -> Self {
            Self {
                active: Mutex::new(HashMap::new()),
                #[cfg(test)]
                retired: Condvar::new(),
            }
        }

        pub(super) fn admit(&self, service: &str, account: &str) -> Admission {
            let mut active = self.active.lock().unwrap();
            if active.is_empty() {
                let attempt = Arc::new(SharedAttempt::new());
                active.insert(
                    (service.to_string(), account.to_string()),
                    Arc::clone(&attempt),
                );
                return Admission::Leader(attempt);
            }
            if let Some(attempt) = active.get(&(service.to_string(), account.to_string())) {
                return Admission::Follower(Arc::clone(attempt));
            }
            Admission::SlotBusy
        }

        /// Reopens the slot after the executing worker completed. Waiters that
        /// joined just before retirement still read the published result.
        pub(super) fn retire(&self, service: &str, account: &str) {
            let mut active = self.active.lock().unwrap();
            active.remove(&(service.to_string(), account.to_string()));
            #[cfg(test)]
            self.retired.notify_all();
        }

        #[cfg(test)]
        pub(super) fn wait_for_idle(&self, timeout: std::time::Duration) -> bool {
            let active = self.active.lock().unwrap();
            let (active, _) = self.retired.wait_timeout_while(active, timeout, |active| {
                !active.is_empty()
            }).unwrap();
            active.is_empty()
        }
    }

    pub(super) enum Admission {
        /// The caller installed a fresh attempt and must spawn the worker.
        Leader(Arc<SharedAttempt>),
        /// The caller joins an already-executing attempt for the same key.
        Follower(Arc<SharedAttempt>),
        /// A different key is executing: unavailable, no new thread.
        SlotBusy,
    }

    /// Process-wide coordinator accessor (`HashMap::new` is not const).
    pub(super) fn coordinator() -> &'static Coordinator {
        static COORDINATOR: std::sync::OnceLock<Coordinator> = std::sync::OnceLock::new();
        COORDINATOR.get_or_init(Coordinator::new)
    }
}

#[cfg(test)]
pub mod tests {
    use super::super::orchestrator::ImportDestination;
    pub(in crate::workers) use super::workspace_folders::test_support::{
        MockAutofill, MockBookmark, MockCookie, MockFavicon, MockFolder, MockHistory, MockPassword,
        MockSpace, MockTab,
    };
    use std::sync::Mutex;

    /// Mock destination for testing workers.
    #[derive(Debug)]
    pub struct MockDestination {
        pub spaces: Mutex<Vec<MockSpace>>,
        pub tabs: Mutex<Vec<MockTab>>,
        pub folders: Mutex<Vec<MockFolder>>,
        pub bookmarks: Mutex<Vec<MockBookmark>>,
        pub history: Mutex<Vec<MockHistory>>,
        pub cookies: Mutex<Vec<MockCookie>>,
        pub autofill: Mutex<Vec<MockAutofill>>,
        pub favicons: Mutex<Vec<MockFavicon>>,
        pub passwords: Mutex<Vec<MockPassword>>,
        pub reject_passwords: bool,
        pub pinned: Mutex<Vec<String>>,
        pub favorited: Mutex<Vec<String>>,
        pub active_space: Mutex<Option<String>>,
        next_id: Mutex<u32>,
    }

    impl Default for MockDestination {
        fn default() -> Self {
            Self::new()
        }
    }

    impl MockDestination {
        pub fn new() -> Self {
            Self {
                spaces: Mutex::new(Vec::new()),
                tabs: Mutex::new(Vec::new()),
                folders: Mutex::new(Vec::new()),
                bookmarks: Mutex::new(Vec::new()),
                history: Mutex::new(Vec::new()),
                cookies: Mutex::new(Vec::new()),
                autofill: Mutex::new(Vec::new()),
                favicons: Mutex::new(Vec::new()),
                passwords: Mutex::new(Vec::new()),
                reject_passwords: false,
                pinned: Mutex::new(Vec::new()),
                favorited: Mutex::new(Vec::new()),
                active_space: Mutex::new(Some("default-space".to_string())),
                next_id: Mutex::new(1),
            }
        }

        fn next_id(&self) -> String {
            let mut id = self.next_id.lock().unwrap();
            let val = format!("id-{}", *id);
            *id += 1;
            val
        }
    }

    impl ImportDestination for MockDestination {
        fn create_space(&self, name: &str, theme: &str, icon: &str) -> Option<String> {
            let id = self.next_id();
            self.spaces.lock().unwrap().push(MockSpace {
                id: id.clone(),
                name: name.to_string(),
                theme: theme.to_string(),
                icon: icon.to_string(),
            });
            Some(id)
        }

        fn create_tab(&self, space_id: &str, url: &str, title: &str) -> Option<String> {
            let id = self.next_id();
            self.tabs.lock().unwrap().push(MockTab {
                id: id.clone(),
                space_id: space_id.to_string(),
                url: url.to_string(),
                title: title.to_string(),
                folder_id: None,
            });
            Some(id)
        }

        fn create_tab_in_folder(
            &self,
            space_id: &str,
            url: &str,
            title: &str,
            folder_id: &str,
        ) -> Option<String> {
            let id = self.next_id();
            self.tabs.lock().unwrap().push(MockTab {
                id: id.clone(),
                space_id: space_id.to_string(),
                url: url.to_string(),
                title: title.to_string(),
                folder_id: Some(folder_id.to_string()),
            });
            Some(id)
        }

        fn create_folder(&self, space_id: &str, name: &str, parent_id: &str) -> Option<String> {
            let id = self.next_id();
            self.folders.lock().unwrap().push(MockFolder {
                id: id.clone(),
                space_id: space_id.to_string(),
                name: name.to_string(),
                parent_id: parent_id.to_string(),
            });
            Some(id)
        }

        fn pin_tab(&self, tab_id: &str) {
            self.pinned.lock().unwrap().push(tab_id.to_string());
        }

        fn favorite_tab(&self, tab_id: &str) {
            self.favorited.lock().unwrap().push(tab_id.to_string());
        }

        fn activate_space(&self, space_id: &str) {
            *self.active_space.lock().unwrap() = Some(space_id.to_string());
        }

        fn get_active_space_id(&self) -> Option<String> {
            self.active_space.lock().unwrap().clone()
        }

        fn add_bookmark(&self, title: &str, url: &str, folder_path: &[String]) -> Option<String> {
            let id = self.next_id();
            self.bookmarks.lock().unwrap().push(MockBookmark {
                title: title.to_string(),
                url: url.to_string(),
                folder_path: folder_path.to_vec(),
            });
            Some(id)
        }

        fn add_history(&self, url: &str, title: &str, visit_time: f64, visit_count: u32) -> bool {
            self.history.lock().unwrap().push(MockHistory {
                url: url.to_string(),
                title: title.to_string(),
                visit_time,
                visit_count,
            });
            true
        }

        fn add_cookie(
            &self,
            cookie: &crate::CookieEntry,
        ) -> bool {
            self.cookies.lock().unwrap().push(MockCookie {
                host: cookie.host.clone(),
                name: cookie.name.clone(),
                value: cookie.value.clone(),
                path: cookie.path.clone(),
            });
            true
        }

        fn add_autofill(
            &self,
            field_name: &str,
            value: &str,
            _times_used: i32,
            _first_used: i64,
            _last_used: i64,
        ) -> bool {
            self.autofill.lock().unwrap().push(MockAutofill {
                field_name: field_name.to_string(),
                value: value.to_string(),
            });
            true
        }

        fn add_favicon(&self, url: &str, png_bytes: &[u8]) -> bool {
            self.favicons.lock().unwrap().push(MockFavicon {
                url: url.to_string(),
                data_len: png_bytes.len(),
            });
            true
        }

        fn add_password(&self, entry: crate::PasswordEntry) -> bool {
            if self.reject_passwords {
                return false;
            }
            self.passwords.lock().unwrap().push(MockPassword {
                origin_url: entry.origin_url,
                username: entry.username,
            });
            true
        }
    }

    #[cfg(target_os = "macos")]
    use super::super::{BrowserType, DetectedBrowser};
    #[cfg(target_os = "macos")]
    use super::password::PasswordWorker;
    #[cfg(target_os = "macos")]
    use super::ImportWorker;
    #[cfg(target_os = "macos")]
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    #[cfg(target_os = "macos")]
    use std::sync::{Arc, Condvar};

    #[cfg(target_os = "macos")]
    struct BlockingKeychainBoundary {
        active_workers: AtomicUsize,
        max_active_workers: AtomicUsize,
        total_spawned_attempts: AtomicUsize,
        entered_services: Mutex<Vec<String>>,
        entered_pair: (Mutex<bool>, Condvar),
        release_pair: (Mutex<bool>, Condvar),
        password: Vec<u8>,
    }

    #[cfg(target_os = "macos")]
    impl BlockingKeychainBoundary {
        fn new(password: &[u8]) -> Self {
            Self {
                active_workers: AtomicUsize::new(0),
                max_active_workers: AtomicUsize::new(0),
                total_spawned_attempts: AtomicUsize::new(0),
                entered_services: Mutex::new(Vec::new()),
                entered_pair: (Mutex::new(false), Condvar::new()),
                release_pair: (Mutex::new(false), Condvar::new()),
                password: password.to_vec(),
            }
        }

        fn wait_for_entered(&self, timeout: std::time::Duration) -> bool {
            let (lock, cvar) = &self.entered_pair;
            let mut guard = lock.lock().unwrap();
            let start = std::time::Instant::now();
            while !*guard {
                let elapsed = start.elapsed();
                if elapsed >= timeout {
                    return false;
                }
                let (next_guard, timeout_res) =
                    cvar.wait_timeout(guard, timeout - elapsed).unwrap();
                guard = next_guard;
                if timeout_res.timed_out() {
                    break;
                }
            }
            *guard
        }

        fn wait_for_attempts(&self, target: usize, timeout: std::time::Duration) -> bool {
            let (lock, cvar) = &self.entered_pair;
            let mut guard = lock.lock().unwrap();
            let start = std::time::Instant::now();
            while self.total_spawned_attempts.load(Ordering::SeqCst) < target {
                let elapsed = start.elapsed();
                if elapsed >= timeout {
                    return false;
                }
                let (next_guard, timeout_res) =
                    cvar.wait_timeout(guard, timeout - elapsed).unwrap();
                guard = next_guard;
                if timeout_res.timed_out() {
                    break;
                }
            }
            self.total_spawned_attempts.load(Ordering::SeqCst) >= target
        }

        fn wait_for_active_workers(&self, target: usize, timeout: std::time::Duration) -> bool {
            assert_eq!(target, 0);
            // Provider return is not worker completion. Wait on the coordinator's
            // retirement event, under the same mutex that protects its slot.
            super::keychain_single_flight::coordinator().wait_for_idle(timeout)
        }

        fn release_all(&self) {
            let (lock, cvar) = &self.release_pair;
            let mut guard = lock.lock().unwrap();
            *guard = true;
            cvar.notify_all();
        }

        fn reset_release_gate(&self) {
            let (lock, _) = &self.release_pair;
            let mut guard = lock.lock().unwrap();
            *guard = false;
        }
    }

    #[cfg(target_os = "macos")]
    impl crate::decrypt::chromium_keychain::KeychainProvider for BlockingKeychainBoundary {
        fn get_password(
            &self,
            service: &str,
            account: &str,
        ) -> Result<zeroize::Zeroizing<Vec<u8>>, crate::decrypt::chromium_keychain::KeychainError>
        {
            self.total_spawned_attempts.fetch_add(1, Ordering::SeqCst);
            let cur = self.active_workers.fetch_add(1, Ordering::SeqCst) + 1;
            let mut max = self.max_active_workers.load(Ordering::SeqCst);
            while cur > max {
                match self.max_active_workers.compare_exchange_weak(
                    max,
                    cur,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                ) {
                    Ok(_) => break,
                    Err(actual) => max = actual,
                }
            }
            {
                let mut list = self.entered_services.lock().unwrap();
                list.push(format!("{service}/{account}"));
            }
            {
                let (lock, cvar) = &self.entered_pair;
                let mut guard = lock.lock().unwrap();
                *guard = true;
                cvar.notify_all();
            }
            {
                let (lock, cvar) = &self.release_pair;
                let mut guard = lock.lock().unwrap();
                while !*guard {
                    guard = cvar.wait(guard).unwrap();
                }
            }
            self.active_workers.fetch_sub(1, Ordering::SeqCst);
            {
                let (lock, cvar) = &self.entered_pair;
                let _guard = lock.lock().unwrap();
                cvar.notify_all();
            }
            Ok(zeroize::Zeroizing::new(self.password.clone()))
        }
    }

    #[cfg(target_os = "macos")]
    struct TestCleanupGuard {
        boundary: Arc<BlockingKeychainBoundary>,
    }

    #[cfg(target_os = "macos")]
    impl Drop for TestCleanupGuard {
        fn drop(&mut self) {
            self.boundary.release_all();
            assert!(self.boundary.wait_for_active_workers(0, std::time::Duration::from_secs(3)),
                "native worker must retire before releasing the serial test guard");
            super::set_keychain_provider_override_for_test(None);
            super::set_keychain_timeout_override_for_test(None);
        }
    }

    #[cfg(target_os = "macos")]
    static TEST_SERIAL_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    #[cfg(target_os = "macos")]
    fn review_provider_idle_is_not_coordinator_retirement() {
        let _serial_lock = TEST_SERIAL_LOCK.lock().unwrap();
        let boundary = BlockingKeychainBoundary::new(b"retirement-test");
        let coordinator = super::keychain_single_flight::coordinator();
        // Deterministically reproduce the interval after the provider decrements
        // its counter but before the native closure retires the installed slot.
        let attempt = coordinator.admit("Chrome Safe Storage", "Chrome");
        assert!(matches!(attempt, super::keychain_single_flight::Admission::Leader(_)));
        let prematurely_finished = boundary.wait_for_active_workers(0, std::time::Duration::ZERO);
        coordinator.retire("Chrome Safe Storage", "Chrome");
        assert!(!prematurely_finished, "provider counter must not release the test's retirement barrier");
        assert!(boundary.wait_for_active_workers(0, std::time::Duration::ZERO));
    }

    #[test]
    fn memory_thread_keychain_timeout_keeps_single_flight() {
        // No-op on non-macOS: the keychain boundary under test is macOS-only.
        #[cfg(target_os = "macos")]
        {
            let _serial_lock = TEST_SERIAL_LOCK.lock().unwrap();
            let boundary = Arc::new(BlockingKeychainBoundary::new(b"single-flight-key"));
            let _guard = TestCleanupGuard {
                boundary: boundary.clone(),
            };
            super::set_keychain_provider_override_for_test(Some(boundary.clone()));
            super::set_keychain_timeout_override_for_test(Some(std::time::Duration::from_millis(
                40,
            )));

            // Given: Attempt 1 starts with a short timeout and enters Keychain boundary
            let t1 = std::thread::spawn(|| {
                super::fetch_keychain_key_with_timeout(
                    "Chrome Safe Storage",
                    "Chrome",
                    std::time::Duration::from_millis(40),
                )
            });

            assert!(
                boundary.wait_for_entered(std::time::Duration::from_secs(3)),
                "deadlock guard: attempt 1 failed to enter Keychain boundary"
            );

            // Wait for attempt 1 caller to observe timeout
            let res1 = t1.join().expect("caller thread 1 panicked");
            assert!(
                res1.is_none(),
                "attempt 1 should have returned None after timeout"
            );

            // Verify attempt 1's native worker is STILL parked inside Keychain
            assert_eq!(
                boundary.active_workers.load(Ordering::SeqCst),
                1,
                "attempt 1 worker must still be parked in Keychain"
            );

            // When: while attempt 1's worker is STILL executing in Keychain, issue repeated requests
            // for the SAME key ("Chrome Safe Storage", "Chrome") across both PasswordWorker and
            // fetch_keychain_key_with_timeout.
            let temp_dir = tempfile::tempdir().unwrap();
            let profile_dir = temp_dir.path();
            std::fs::write(profile_dir.join("Login Data"), b"").unwrap();
            let browser = DetectedBrowser {
                browser_type: BrowserType::Chrome,
                display_name: "Chrome".to_string(),
                profile_path: profile_dir.join("Preferences"),
                services_supported: 0xFF,
                requires_full_disk_access: false,
            };
            let dest = Arc::new(MockDestination::new());
            let cancelled = Arc::new(AtomicBool::new(false));
            let (tx, _rx) = std::sync::mpsc::channel();
            let pw_worker = PasswordWorker::new(None);
            let pw_handle =
                std::thread::spawn(move || pw_worker.run(&browser, &*dest, &cancelled, &tx));

            let mut handles = Vec::new();
            for _ in 0..4 {
                handles.push(std::thread::spawn(|| {
                    super::fetch_keychain_key_with_timeout(
                        "Chrome Safe Storage",
                        "Chrome",
                        std::time::Duration::from_millis(40),
                    )
                }));
            }

            let _ = pw_handle.join().expect("password worker panicked");
            for h in handles {
                let _ = h.join().expect("fetch thread panicked");
            }

            // Then: under U11 single-flight coordination, all callers for the same active key
            // MUST share the single active attempt. Caller timeout must NOT clear the active slot
            // or spawn duplicate replacement native worker threads while the attempt is executing.
            // Baseline failure: every repeated call unconditionally spawns a new thread and enters
            // get_password, causing max_active_workers > 1 and total_spawned_attempts > 1.
            let peak_active = boundary.max_active_workers.load(Ordering::SeqCst);
            let total_spawned = boundary.total_spawned_attempts.load(Ordering::SeqCst);

            assert_eq!(
                peak_active,
                1,
                "U11 single-flight violation: expected at most 1 active worker globally, but observed {peak_active}"
            );
            assert_eq!(
                total_spawned,
                1,
                "U11 single-flight violation: expected repeated requests while busy to share the single in-flight attempt, but observed {total_spawned} spawned attempts"
            );

            boundary.release_all();
            assert!(
                boundary.wait_for_active_workers(0, std::time::Duration::from_secs(3)),
                "workers failed to drain after release"
            );
        }
    }

    #[test]
    fn memory_thread_keychain_different_key_does_not_spawn() {
        // No-op on non-macOS: the keychain boundary under test is macOS-only.
        #[cfg(target_os = "macos")]
        {
            let _serial_lock = TEST_SERIAL_LOCK.lock().unwrap();
            let boundary = Arc::new(BlockingKeychainBoundary::new(b"different-key-test"));
            let _guard = TestCleanupGuard {
                boundary: boundary.clone(),
            };
            super::set_keychain_provider_override_for_test(Some(boundary.clone()));

            // Given: worker 1 for "Chrome Safe Storage" is parked inside Keychain boundary
            let t1 = std::thread::spawn(|| {
                super::fetch_keychain_key_with_timeout(
                    "Chrome Safe Storage",
                    "Chrome",
                    std::time::Duration::from_secs(5),
                )
            });

            assert!(
                boundary.wait_for_entered(std::time::Duration::from_secs(3)),
                "deadlock guard: Chrome attempt failed to enter Keychain boundary"
            );
            assert_eq!(
                boundary.active_workers.load(Ordering::SeqCst),
                1,
                "expected exactly 1 active Chrome worker"
            );

            // When: while Chrome worker is actively running globally, attempt a DIFFERENT key
            // ("Brave Safe Storage", "Brave")
            let different_key_res = super::fetch_keychain_key_with_timeout(
                "Brave Safe Storage",
                "Brave",
                std::time::Duration::from_millis(50),
            );

            // Then: U11 contract states: "At most one native worker runs globally;
            // a different key while busy returns unavailable immediately, without queueing another thread."
            // Baseline failure: baseline ignores global concurrency and spawns a thread for Brave,
            // entering Keychain concurrently so max_active_workers reaches 2 and Brave is recorded.
            assert!(
                different_key_res.is_none(),
                "U11 concurrency violation: expected distinct key while busy to return unavailable (None) immediately"
            );

            let peak_active = boundary.max_active_workers.load(Ordering::SeqCst);
            let total_spawned = boundary.total_spawned_attempts.load(Ordering::SeqCst);
            let services = boundary.entered_services.lock().unwrap().clone();

            assert_eq!(
                peak_active,
                1,
                "U11 global limit violation: expected at most 1 global worker, but observed {peak_active}"
            );
            assert_eq!(
                total_spawned,
                1,
                "U11 global limit violation: expected only Chrome to have entered Keychain, but total attempts = {total_spawned}"
            );
            assert!(
                !services.iter().any(|s| s.contains("Brave")),
                "U11 violation: Brave should NOT have entered Keychain while Chrome worker was executing, entered: {services:?}"
            );

            // Release Chrome worker and await completion
            boundary.release_all();
            let chrome_res = t1.join().expect("Chrome caller thread panicked");
            assert!(
                chrome_res.is_some(),
                "Chrome should successfully derive key upon release"
            );
            assert!(
                boundary.wait_for_active_workers(0, std::time::Duration::from_secs(3)),
                "workers failed to drain after release"
            );
        }
    }

    #[test]
    fn memory_thread_keychain_completion_reopens_slot() {
        // No-op on non-macOS: the keychain boundary under test is macOS-only.
        #[cfg(target_os = "macos")]
        {
            let _serial_lock = TEST_SERIAL_LOCK.lock().unwrap();
            let boundary = Arc::new(BlockingKeychainBoundary::new(b"reopen-slot-key"));
            let _guard = TestCleanupGuard {
                boundary: boundary.clone(),
            };
            super::set_keychain_provider_override_for_test(Some(boundary.clone()));
            super::set_keychain_timeout_override_for_test(Some(std::time::Duration::from_millis(
                40,
            )));

            // Given: attempt 1 for Chrome is started with short timeout (40ms)
            let t1 = std::thread::spawn(|| {
                super::fetch_keychain_key_with_timeout(
                    "Chrome Safe Storage",
                    "Chrome",
                    std::time::Duration::from_millis(40),
                )
            });

            assert!(
                boundary.wait_for_entered(std::time::Duration::from_secs(3)),
                "deadlock guard: Chrome attempt failed to enter Keychain boundary"
            );

            let res1 = t1.join().expect("Chrome caller thread panicked");
            assert!(res1.is_none(), "expected attempt 1 to time out");

            // Verify that while attempt 1's worker is STILL executing in Keychain, the slot remains busy
            let busy_check = super::fetch_keychain_key_with_timeout(
                "Edge Safe Storage",
                "Edge",
                std::time::Duration::from_millis(40),
            );
            assert!(
                busy_check.is_none(),
                "U11 violation: distinct key must return unavailable while worker is busy"
            );
            let active_during_busy = boundary.max_active_workers.load(Ordering::SeqCst);
            assert_eq!(
                active_during_busy,
                1,
                "U11 violation: active workers must not exceed 1 while slot is busy, got {active_during_busy}"
            );

            // When: Attempt 1's worker is released and completes
            boundary.release_all();
            assert!(
                boundary.wait_for_active_workers(0, std::time::Duration::from_secs(3)),
                "attempt 1 worker failed to drain after release"
            );

            // Reset release gate for next attempt
            boundary.reset_release_gate();
            super::set_keychain_timeout_override_for_test(Some(std::time::Duration::from_secs(3)));

            // Now that attempt 1 worker has completed, attempt 2 (Edge) must be able to acquire slot and succeed
            let t2 = std::thread::spawn(|| {
                super::fetch_keychain_key_with_timeout(
                    "Edge Safe Storage",
                    "Edge",
                    std::time::Duration::from_secs(3),
                )
            });

            assert!(
                boundary.wait_for_attempts(2, std::time::Duration::from_secs(3)),
                "deadlock guard: second attempt failed to enter Keychain after slot reopened"
            );

            boundary.release_all();
            let res2 = t2.join().expect("Edge caller thread panicked");
            assert!(
                res2.is_some(),
                "U11 contract violation: after worker completion, slot must reopen and next attempt must succeed"
            );
            assert!(
                boundary.wait_for_active_workers(0, std::time::Duration::from_secs(3)),
                "second worker failed to drain after release"
            );
        }
    }
}
