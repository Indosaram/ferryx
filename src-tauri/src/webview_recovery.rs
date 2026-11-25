//! Recovery from a macOS WebKit content-process termination.
//!
//! WKWebView runs page content in a separate process. When that process is killed (jetsam under
//! memory pressure, a WebKit crash) the window keeps its frame but paints nothing until someone
//! reloads the webview. `tauri-runtime-wry` reloads on its own, but installing a
//! `Builder::on_web_content_process_terminate` hook replaces that fallback for EVERY webview, so
//! the hook itself must reload: app windows and `browser-*` guest webviews alike.
//!
//! Reloading blindly turns one crash into a reload storm: a page that kills its content process on
//! load would loop forever. [`WebviewCrashLoopGuard`] caps reloads per window label inside a
//! rolling window and suppresses the rest.
//!
//! Reloading an app window also invalidates its native terminal viewers: the GPU surfaces belonged
//! to the destroyed webview, and a fresh page would present stale frames for sessions it no longer
//! shows. Those viewers are dropped per window label. The sessions, their pumps and the daemon are
//! never touched, and session ownership in [`crate::window_registry`] is deliberately left alone:
//! that is the window-close path, which hands sessions to another window.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use parking_lot::Mutex;

/// How far back a label's reloads are counted.
const DEFAULT_RELOAD_WINDOW: Duration = Duration::from_secs(60);
/// Reloads allowed per label inside [`DEFAULT_RELOAD_WINDOW`] before suppression.
const DEFAULT_MAX_RELOADS: usize = 2;

/// What the crash hook did for one termination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReloadDecision {
    /// The webview was reloaded.
    Reload,
    /// The label already reloaded too often inside the window; left alone.
    Suppress,
}

/// Per-label rate limit for content-process recovery reloads.
///
/// A reload is recorded only when it is granted, so a suppressed label never extends its own ban.
/// Timestamps at least one full window old are pruned before the decision, which makes the window
/// a sliding one rather than a fixed bucket.
#[derive(Debug)]
pub struct WebviewCrashLoopGuard {
    window: Duration,
    max_reloads: usize,
    reloads: Mutex<HashMap<String, VecDeque<Instant>>>,
}

impl Default for WebviewCrashLoopGuard {
    fn default() -> Self {
        Self {
            window: DEFAULT_RELOAD_WINDOW,
            max_reloads: DEFAULT_MAX_RELOADS,
            reloads: Mutex::new(HashMap::new()),
        }
    }
}

impl WebviewCrashLoopGuard {
    /// Decides whether `label` may be reloaded at `now`, recording the reload when granted.
    pub fn decide(&self, label: &str, now: Instant) -> ReloadDecision {
        let window = self.window;
        let mut reloads = self.reloads.lock();
        let granted = reloads.entry(label.to_string()).or_default();
        while granted
            .front()
            .is_some_and(|timestamp| now.saturating_duration_since(*timestamp) >= window)
        {
            granted.pop_front();
        }
        if granted.len() >= self.max_reloads {
            return ReloadDecision::Suppress;
        }
        granted.push_back(now);
        ReloadDecision::Reload
    }
}

/// Handles one web content-process termination: rate-limit, drop the window's stale native
/// viewers, then reload.
#[cfg(feature = "desktop")]
pub fn handle_web_content_process_terminated<R: tauri::Runtime>(
    webview: &tauri::Webview<R>,
    guard: &WebviewCrashLoopGuard,
    now: Instant,
) -> ReloadDecision {
    recover_webview(&LiveWebview(webview), guard, now)
}

/// The two platform calls recovery makes on a live webview, behind a seam so a test can observe
/// the label they receive and whether they ran at all.
#[cfg(feature = "desktop")]
trait WebviewRecoveryTarget {
    /// The webview's window label (`main`, `ws-*`, `browser-*`).
    fn label(&self) -> &str;
    /// Drops the native terminal viewers this label held; returns the affected session ids.
    fn detach_viewers(&self, label: &str) -> Vec<String>;
    /// Reloads the webview.
    fn reload(&self) -> Result<(), String>;
}

#[cfg(feature = "desktop")]
struct LiveWebview<'a, R: tauri::Runtime>(&'a tauri::Webview<R>);

#[cfg(feature = "desktop")]
impl<R: tauri::Runtime> WebviewRecoveryTarget for LiveWebview<'_, R> {
    fn label(&self) -> &str {
        self.0.label()
    }

    fn detach_viewers(&self, label: &str) -> Vec<String> {
        detach_window_viewers(self.0, label)
    }

    fn reload(&self) -> Result<(), String> {
        self.0.reload().map_err(|error| error.to_string())
    }
}

/// Drops the native terminal viewers held by app window `label`; a guest `browser-*` webview holds
/// none, and a reload must never take a session away from another window.
#[cfg(feature = "desktop")]
fn detach_window_viewers<R: tauri::Runtime>(
    webview: &tauri::Webview<R>,
    label: &str,
) -> Vec<String> {
    use tauri::Manager;

    if !crate::window_registry::is_app_window_label(label) {
        return Vec::new();
    }
    match webview
        .try_state::<crate::native_terminal::surface_host::NativeTerminalSurfaceHostState>()
    {
        Some(surfaces) => surfaces.forget_window_viewers(label),
        None => Vec::new(),
    }
}

/// The recovery decision plus its side effects for one termination.
#[cfg(feature = "desktop")]
fn recover_webview(
    target: &impl WebviewRecoveryTarget,
    guard: &WebviewCrashLoopGuard,
    now: Instant,
) -> ReloadDecision {
    let label = target.label();
    if guard.decide(label, now) == ReloadDecision::Suppress {
        tracing::error!(
            label = %label,
            "webview content process crashed repeatedly within 60s; suppressing reload"
        );
        return ReloadDecision::Suppress;
    }
    let detached = target.detach_viewers(label);
    if !detached.is_empty() {
        tracing::info!(
            label = %label,
            detached_count = detached.len(),
            "detached stale native terminal viewers before webview reload"
        );
    }
    match target.reload() {
        Ok(()) => {
            tracing::warn!(label = %label, "reloaded webview after content process termination")
        }
        Err(error) => tracing::error!(
            label = %label,
            %error,
            "failed to reload webview after content process termination"
        ),
    }
    ReloadDecision::Reload
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(base: Instant, seconds: u64) -> Instant {
        base + Duration::from_secs(seconds)
    }

    #[test]
    fn reloads_are_granted_twice_then_suppressed_until_the_window_slides() {
        let guard = WebviewCrashLoopGuard::default();
        let base = Instant::now();

        assert_eq!(guard.decide("main", at(base, 0)), ReloadDecision::Reload);
        assert_eq!(guard.decide("main", at(base, 10)), ReloadDecision::Reload);
        assert_eq!(guard.decide("main", at(base, 59)), ReloadDecision::Suppress);
        // Both earlier reloads are at least 60s old now, so the label is eligible again.
        assert_eq!(guard.decide("main", at(base, 75)), ReloadDecision::Reload);
    }

    #[test]
    fn a_suppressed_label_does_not_suppress_other_windows() {
        let guard = WebviewCrashLoopGuard::default();
        let base = Instant::now();

        assert_eq!(guard.decide("main", at(base, 0)), ReloadDecision::Reload);
        assert_eq!(guard.decide("main", at(base, 20)), ReloadDecision::Reload);
        assert_eq!(guard.decide("main", at(base, 20)), ReloadDecision::Suppress);
        assert_eq!(guard.decide("ws-secondary", at(base, 20)), ReloadDecision::Reload);
        assert_eq!(guard.decide("browser-123", at(base, 20)), ReloadDecision::Reload);
    }

    #[cfg(feature = "desktop")]
    mod desktop {
        use super::*;
        use std::sync::Mutex as StdMutex;
        use tauri::{Listener, Manager};

        type MockApp = tauri::App<tauri::test::MockRuntime>;
        type MockWindow = tauri::WebviewWindow<tauri::test::MockRuntime>;

        fn app_with(labels: &[&str]) -> (MockApp, Vec<MockWindow>) {
            let app = tauri::test::mock_app();
            app.manage(crate::window_registry::WindowRegistry::default());
            app.manage(
                crate::native_terminal::surface_host::NativeTerminalSurfaceHostState::default(),
            );
            let windows = labels
                .iter()
                .map(|label| mock_window(&app, label))
                .collect();
            (app, windows)
        }

        fn mock_window(app: &MockApp, label: &str) -> MockWindow {
            tauri::WebviewWindowBuilder::new(app, label, tauri::WebviewUrl::default())
                .build()
                .expect("mock webview window")
        }

        /// A recovery target that records every call instead of touching a webview.
        #[derive(Default)]
        struct Recorder {
            label: String,
            detached: StdMutex<Vec<String>>,
            reloads: StdMutex<usize>,
        }

        impl Recorder {
            fn new(label: &str) -> Self {
                Self {
                    label: label.to_string(),
                    ..Self::default()
                }
            }
        }

        impl WebviewRecoveryTarget for Recorder {
            fn label(&self) -> &str {
                &self.label
            }

            fn detach_viewers(&self, label: &str) -> Vec<String> {
                self.detached.lock().unwrap().push(label.to_string());
                Vec::new()
            }

            fn reload(&self) -> Result<(), String> {
                *self.reloads.lock().unwrap() += 1;
                Ok(())
            }
        }

        #[test]
        fn terminating_an_app_window_reloads_without_touching_session_ownership() {
            let (app, windows) = app_with(&["main", "ws-abc"]);
            let registry = app.state::<crate::window_registry::WindowRegistry>();
            registry.set_owner("s-main", "main");
            registry.set_owner("s-abc", "ws-abc");
            let (orphans, orphaned) = std::sync::mpsc::channel();
            windows[1].listen(crate::WINDOW_SESSIONS_ORPHANED_EVENT, move |event| {
                let _ = orphans.send(event.payload().to_owned());
            });

            let decision = handle_web_content_process_terminated(
                windows[0].as_ref(),
                &WebviewCrashLoopGuard::default(),
                Instant::now(),
            );

            assert_eq!(decision, ReloadDecision::Reload);
            assert_eq!(registry.owner("s-main").as_deref(), Some("main"));
            assert_eq!(registry.owner("s-abc").as_deref(), Some("ws-abc"));
            assert!(
                orphaned.try_recv().is_err(),
                "a webview reload never orphans sessions"
            );
        }

        #[test]
        fn an_app_window_reload_detaches_viewers_for_its_own_label() {
            let target = Recorder::new("ws-abc");
            let decision = recover_webview(&target, &WebviewCrashLoopGuard::default(), Instant::now());

            assert_eq!(decision, ReloadDecision::Reload);
            assert_eq!(*target.detached.lock().unwrap(), vec!["ws-abc".to_string()]);
            assert_eq!(*target.reloads.lock().unwrap(), 1);
        }

        #[test]
        fn a_guest_webview_reload_leaves_app_viewers_alone() {
            let (app, windows) = app_with(&["main"]);
            let registry = app.state::<crate::window_registry::WindowRegistry>();
            registry.set_owner("s-main", "main");
            // A guest is a CHILD webview of an app window, exactly like the browser panes.
            let guest = windows[0]
                .as_ref()
                .window()
                .add_child(
                    tauri::WebviewBuilder::new("browser-123", tauri::WebviewUrl::default()),
                    tauri::LogicalPosition::new(0.0, 0.0),
                    tauri::LogicalSize::new(400.0, 300.0),
                )
                .expect("guest child webview");

            let decision = handle_web_content_process_terminated(
                &guest,
                &WebviewCrashLoopGuard::default(),
                Instant::now(),
            );

            assert_eq!(decision, ReloadDecision::Reload);
            assert_eq!(registry.owner("s-main").as_deref(), Some("main"));
            assert!(LiveWebview(&guest).detach_viewers("browser-123").is_empty());
        }

        #[test]
        fn a_suppressed_label_neither_detaches_nor_reloads() {
            let guard = WebviewCrashLoopGuard::default();
            let now = Instant::now();
            assert_eq!(guard.decide("main", now), ReloadDecision::Reload);
            assert_eq!(guard.decide("main", now), ReloadDecision::Reload);
            let target = Recorder::new("main");

            let decision = recover_webview(&target, &guard, now);

            assert_eq!(decision, ReloadDecision::Suppress);
            let detached = target.detached.lock().unwrap().clone();
            assert!(detached.is_empty(), "{detached:?}");
            assert_eq!(*target.reloads.lock().unwrap(), 0);
        }
    }
}
