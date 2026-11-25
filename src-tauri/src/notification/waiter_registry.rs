//! Cancellable click waiters for routed desktop notifications.
//!
//! A routed notification shows a banner and then blocks a dedicated OS thread
//! until the user acts on it. When a newer notification for the same pane
//! replaces that banner, or the pane goes away, the old waiter has to stop
//! waiting, close *its own* banner, and route nothing. That bookkeeping is
//! exactly what this module owns.
//!
//! Only `std`, `parking_lot`, and `tokio` primitives are used here: the module
//! imports neither the notification crate nor the desktop framework, so it
//! compiles on every platform and with `--no-default-features`.

use parking_lot::Mutex;
use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::oneshot;

/// Pane a banner belongs to: workspace id and frontend session id.
///
/// The same pair a click payload carries, so a withdrawal addresses exactly the
/// banner a later dispatch would replace.
type WaiterKey = (String, String);

/// Registry of the live click waiter for each pane.
pub struct WaiterRegistry {
    next_token: AtomicU64,
    active: Mutex<HashMap<WaiterKey, (u64, oneshot::Sender<()>)>>,
}

impl WaiterRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            // Tokens start at 1 so a slot can never hold a zero "no waiter" id.
            next_token: AtomicU64::new(1),
            active: Mutex::new(HashMap::new()),
        }
    }

    /// Register a waiter for `key`, replacing and cancelling any waiter that
    /// already holds the pane.
    ///
    /// The returned token names this registration; [`finish_token`] uses it to
    /// prove the caller still owns the slot. The returned receiver fires once
    /// when this waiter is withdrawn or replaced.
    ///
    /// [`finish_token`]: WaiterRegistry::finish_token
    pub fn register(&self, key: WaiterKey) -> (u64, oneshot::Receiver<()>) {
        let token = self.next_token.fetch_add(1, Ordering::Relaxed);
        let (cancel_tx, cancel_rx) = oneshot::channel();
        // The lock is released before the replaced waiter is woken, so the
        // woken task can re-enter the registry without contending on it.
        let replaced = self.active.lock().insert(key, (token, cancel_tx));
        if let Some((_, old_cancel_tx)) = replaced {
            // The replaced waiter closes its own banner when it wakes.
            let _ = old_cancel_tx.send(());
        }
        (token, cancel_rx)
    }

    /// Withdraw the waiter for `key`, waking it so it closes its own banner.
    ///
    /// Returns `true` iff a live waiter was signalled; a repeated or unknown
    /// key is a no-op that returns `false`.
    pub fn withdraw(&self, key: &WaiterKey) -> bool {
        let Some((_, cancel_tx)) = self.active.lock().remove(key) else {
            return false;
        };
        cancel_tx.send(()).is_ok()
    }

    /// Release `key` on behalf of the waiter that owns `token`.
    ///
    /// A replaced waiter exits *after* its successor registered, so this only
    /// removes the slot when the caller still owns it. Returns whether the slot
    /// was removed.
    pub fn finish_token(&self, key: &WaiterKey, token: u64) -> bool {
        let mut active = self.active.lock();
        let owns_slot = active.get(key).is_some_and(|(current, _)| *current == token);
        if !owns_slot {
            return false;
        }
        active.remove(key);
        true
    }
}

impl Default for WaiterRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// How a cancellable waiter ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaiterOutcome<T> {
    /// The user acted on the banner; the value is the classified platform response.
    Action(T),
    /// The waiter was withdrawn or replaced; its banner is already closed.
    Withdrawn,
}

/// Wait for the user's action or a withdrawal, whichever comes first.
///
/// `wait_fut` performs the platform wait and yields its classified result;
/// `close_fut` closes *this* waiter's banner and runs only when the wait was
/// cancelled. Cancellation is biased, so a withdrawal that landed while the
/// platform response was still being classified never routes an activation.
/// The registry slot is released on both paths, and only by the waiter that
/// still owns `token`.
pub async fn drive_cancellable_waiter<T, W, C>(
    registry: &WaiterRegistry,
    key: WaiterKey,
    token: u64,
    wait_fut: W,
    close_fut: impl FnOnce() -> C,
    cancel_rx: oneshot::Receiver<()>,
) -> WaiterOutcome<T>
where
    W: Future<Output = T>,
    C: Future<Output = ()>,
{
    tokio::select! {
        biased;
        // A dropped sender means the slot was removed without a signal, which is
        // the same decision as a withdrawal: stop waiting.
        _ = cancel_rx => {
            registry.finish_token(&key, token);
            close_fut().await;
            WaiterOutcome::Withdrawn
        }
        action = wait_fut => {
            registry.finish_token(&key, token);
            WaiterOutcome::Action(action)
        }
    }
}
