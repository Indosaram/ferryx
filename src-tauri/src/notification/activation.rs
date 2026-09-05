//! Click-activation routing queue.
//!
//! When the user clicks a delivered OS notification, the platform callback runs
//! on an arbitrary thread (a background GCD queue on macOS, a dedicated waiter
//! thread on Windows/Linux). That thread cannot touch the WebView, so it drops
//! the originating pane target into this thread-safe queue and fires a single
//! app-wide wake signal. The frontend then drains the queue over IPC.
//!
//! Draining is atomic and one-shot: a target is handed to exactly one drain
//! call, so two overlapping wake signals can never double-route or lose a
//! click. The frontend subscribes to the wake event *before* its first startup
//! drain, so a click that lands during listener setup is still delivered.
//!
//! This module is fully platform-independent so the shared contract compiles
//! everywhere; only the code that *feeds* the queue is platform-specific.

use super::model::NotificationTarget;
use parking_lot::Mutex;
use std::sync::Arc;

/// App-wide event emitted when a notification click enqueues a target.
///
/// The frontend listens for this and drains via `cmd_notification_take_activations`.
pub const NOTIFICATION_ACTIVATED_EVENT: &str = "notification_activated";

/// Thread-safe, app-managed queue of pending click activations.
///
/// Registered once as Tauri managed state. The wake sink is installed at setup
/// (it needs the `AppHandle`), so the queue itself stays generic and unit
/// testable without a running app.
#[derive(Default)]
pub struct NotificationActivations {
    pending: Mutex<Vec<NotificationTarget>>,
    wake: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

impl NotificationActivations {
    pub fn new() -> Self {
        Self::default()
    }

    /// Install the app-wide wake sink, called once at setup with an emitter
    /// that fans `NOTIFICATION_ACTIVATED_EVENT` out to the frontend.
    pub fn set_wake_sink(&self, wake: Arc<dyn Fn() + Send + Sync>) {
        *self.wake.lock() = Some(wake);
    }

    /// Enqueue a click target and fire the wake signal. Safe from any thread.
    ///
    /// The target is enqueued before the wake fires, and the wake sink is
    /// cloned out before it runs so the configuration lock is never held across
    /// the external callback.
    pub fn push(&self, target: NotificationTarget) {
        self.pending.lock().push(target);
        let wake = self.wake.lock().clone();
        if let Some(wake) = wake {
            wake();
        }
    }

    /// Atomically take every pending target, leaving the queue empty. Each
    /// enqueued target is returned by exactly one call.
    pub fn drain(&self) -> Vec<NotificationTarget> {
        std::mem::take(&mut *self.pending.lock())
    }

    /// Pending count without draining. Test-only.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.pending.lock().len()
    }
}

/// Whether a platform click callback is the user activating the notification
/// body (default action) or merely dismissing it. Only a default activation
/// routes focus; a dismissal is ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActivationAction {
    Default,
    Dismiss,
}

/// Decode a click payload and enqueue its target when routing is warranted.
///
/// The single seam both platform callbacks funnel through: dismiss actions
/// never route, a missing or malformed target is dropped (test/id-less or
/// stale/foreign payload), and a well-formed target on a default action is
/// enqueued, firing the wake. Returns `true` iff a target was enqueued.
pub(crate) fn route_activation(
    queue: &NotificationActivations,
    action: ActivationAction,
    target_json: Option<&str>,
) -> bool {
    if action != ActivationAction::Default {
        return false;
    }
    let Some(raw) = target_json else {
        return false;
    };
    match serde_json::from_str::<NotificationTarget>(raw) {
        Ok(target) => {
            queue.push(target);
            true
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn target(session: &str) -> NotificationTarget {
        NotificationTarget {
            workspace_id: "ws".into(),
            session_id: session.into(),
        }
    }

    #[test]
    fn drain_returns_pushed_targets_in_order_then_empties() {
        let queue = NotificationActivations::new();
        queue.push(target("a"));
        queue.push(target("b"));

        let drained = queue.drain();
        assert_eq!(
            drained.iter().map(|t| t.session_id.as_str()).collect::<Vec<_>>(),
            vec!["a", "b"]
        );

        // Draining again without a new push yields nothing: each target is
        // routed exactly once.
        assert!(queue.drain().is_empty());
    }

    #[test]
    fn startup_queued_activation_survives_until_first_drain() {
        // A click that lands before any drain (e.g. during frontend listener
        // setup) must still be delivered by the first drain.
        let queue = NotificationActivations::new();
        queue.push(target("startup"));
        assert_eq!(queue.len(), 1);

        let drained = queue.drain();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].session_id, "startup");
    }

    #[test]
    fn push_fires_the_wake_sink_once_per_push() {
        let queue = NotificationActivations::new();
        let wakes = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&wakes);
        queue.set_wake_sink(Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }));

        queue.push(target("a"));
        queue.push(target("b"));

        assert_eq!(wakes.load(Ordering::SeqCst), 2);
        assert_eq!(queue.len(), 2);
    }

    #[test]
    fn push_without_a_wake_sink_still_enqueues() {
        // The queue accepts activations before setup installs the wake sink;
        // the first drain (startup) then delivers them.
        let queue = NotificationActivations::new();
        queue.push(target("early"));
        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn route_default_action_with_valid_target_enqueues() {
        let queue = NotificationActivations::new();
        let json = r#"{"workspaceId":"ws-7","sessionId":"fe-3"}"#;
        assert!(route_activation(&queue, ActivationAction::Default, Some(json)));
        let drained = queue.drain();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].workspace_id, "ws-7");
        assert_eq!(drained[0].session_id, "fe-3");
    }

    #[test]
    fn route_dismiss_action_never_enqueues() {
        let queue = NotificationActivations::new();
        let json = r#"{"workspaceId":"ws-7","sessionId":"fe-3"}"#;
        assert!(!route_activation(&queue, ActivationAction::Dismiss, Some(json)));
        assert_eq!(queue.len(), 0);
    }

    #[test]
    fn route_drops_missing_and_malformed_target() {
        let queue = NotificationActivations::new();
        // Test / id-less notification: no payload at all.
        assert!(!route_activation(&queue, ActivationAction::Default, None));
        // Stale or foreign payload that is not our target shape.
        assert!(!route_activation(
            &queue,
            ActivationAction::Default,
            Some("not json")
        ));
        assert!(!route_activation(
            &queue,
            ActivationAction::Default,
            Some(r#"{"workspaceId":"ws"}"#)
        ));
        assert_eq!(queue.len(), 0);
    }

    #[test]
    fn route_enqueues_target_before_firing_wake() {
        // The wake sink must observe the target already present, so a drain it
        // triggers can never miss the just-routed activation.
        let queue = Arc::new(NotificationActivations::new());
        let observed = Arc::new(AtomicUsize::new(usize::MAX));
        let seen = Arc::clone(&observed);
        let q_for_sink = Arc::clone(&queue);
        queue.set_wake_sink(Arc::new(move || {
            seen.store(q_for_sink.len(), Ordering::SeqCst);
        }));

        let json = r#"{"workspaceId":"ws","sessionId":"fe"}"#;
        route_activation(&queue, ActivationAction::Default, Some(json));

        assert_eq!(observed.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn overlapping_pushes_and_drains_lose_nothing() {
        use std::sync::Barrier;
        use std::thread;

        // 8 pushers and 4 drainers all release from one Barrier so their pushes
        // and drains genuinely overlap, then every remaining item is drained.
        // The invariant: total drained across all threads plus the final drain
        // equals the number pushed, so no target is lost or double-counted.
        let queue = Arc::new(NotificationActivations::new());
        let start = Arc::new(Barrier::new(12));
        let drained_total = Arc::new(AtomicUsize::new(0));
        let mut handles = Vec::new();

        for i in 0..8 {
            let (q, b) = (Arc::clone(&queue), Arc::clone(&start));
            handles.push(thread::spawn(move || {
                b.wait();
                q.push(target(&format!("s{i}")));
            }));
        }
        for _ in 0..4 {
            let (q, b, total) = (
                Arc::clone(&queue),
                Arc::clone(&start),
                Arc::clone(&drained_total),
            );
            handles.push(thread::spawn(move || {
                b.wait();
                total.fetch_add(q.drain().len(), Ordering::SeqCst);
            }));
        }
        for h in handles {
            h.join().unwrap();
        }

        let leftover = queue.drain().len();
        assert_eq!(drained_total.load(Ordering::SeqCst) + leftover, 8);
    }
}
