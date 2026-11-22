//! Withdrawal contract for routed-notification click waiters.
//!
//! The registry is compiled straight into this test target, so these tests pin
//! the behaviour that keeps a superseded banner from stealing focus later: a
//! withdrawal wakes exactly one waiter, the woken waiter closes its own banner
//! and routes nothing, an action that already won is never closed, and a
//! replaced waiter cannot delete its successor's slot.

#[path = "../src/notification/waiter_registry.rs"]
mod waiter_registry;

use std::future::pending;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use waiter_registry::{drive_cancellable_waiter, WaiterOutcome, WaiterRegistry};

/// Bound on every await, so a contract violation fails the test instead of
/// hanging it. Nothing here waits on a clock to make progress.
const BOUND: Duration = Duration::from_secs(5);

fn key(workspace: &str, session: &str) -> (String, String) {
    (workspace.into(), session.into())
}

fn counter() -> Arc<AtomicUsize> {
    Arc::new(AtomicUsize::new(0))
}

fn bump(counter: &Arc<AtomicUsize>) {
    counter.fetch_add(1, Ordering::SeqCst);
}

fn count(counter: &Arc<AtomicUsize>) -> usize {
    counter.load(Ordering::SeqCst)
}

#[tokio::test]
async fn withdraw_signals_the_registered_waiter_once() {
    // Given: a registered waiter for one pane
    let registry = WaiterRegistry::default();
    let pane = key("ws-1", "fe-1");
    let (_token, cancel_rx) = registry.register(pane.clone());

    // When: the pane is withdrawn, then withdrawn again, then a pane that never
    // registered is withdrawn
    let first = registry.withdraw(&pane);
    let repeat = registry.withdraw(&pane);
    let unknown = registry.withdraw(&key("ws-9", "fe-9"));

    // Then: only the first withdrawal signalled, and it woke this waiter
    assert!(first);
    assert!(!repeat);
    assert!(!unknown);
    let signalled = tokio::time::timeout(BOUND, cancel_rx)
        .await
        .expect("withdraw signals the registered waiter");
    assert_eq!(signalled, Ok(()));
}

#[tokio::test]
async fn withdraw_winning_the_select_closes_the_banner_and_routes_nothing() {
    // Given: a waiter withdrawn before it starts driving
    let registry = WaiterRegistry::new();
    let pane = key("ws-1", "fe-1");
    let (token, cancel_rx) = registry.register(pane.clone());
    assert!(registry.withdraw(&pane));
    let closed = counter();
    let close_calls = Arc::clone(&closed);

    // When: the waiter drives with a platform wait that never resolves
    let outcome = tokio::time::timeout(
        BOUND,
        drive_cancellable_waiter(
            &registry,
            pane.clone(),
            token,
            pending::<Option<u32>>(),
            move || async move {
                bump(&close_calls);
            },
            cancel_rx,
        ),
    )
    .await
    .expect("withdrawal ends the wait");

    // Then: the cancellation path ran, closing this waiter's own banner exactly
    // once, and no activation was routed
    assert_eq!(outcome, WaiterOutcome::Withdrawn);
    assert_eq!(count(&closed), 1);
    // The slot is gone, so neither a release nor a later withdrawal finds it.
    assert!(!registry.finish_token(&pane, token));
    assert!(!registry.withdraw(&pane));
}

#[tokio::test]
async fn action_winning_the_select_routes_and_leaves_the_banner_open() {
    // Given: a registered waiter the user is about to act on
    let registry = WaiterRegistry::new();
    let pane = key("ws-1", "fe-1");
    let (token, cancel_rx) = registry.register(pane.clone());
    let closed = counter();
    let close_calls = Arc::clone(&closed);

    // When: the platform reports a click while no withdrawal is pending
    let outcome = tokio::time::timeout(
        BOUND,
        drive_cancellable_waiter(
            &registry,
            pane.clone(),
            token,
            async { Some(42u32) },
            move || async move {
                bump(&close_calls);
            },
            cancel_rx,
        ),
    )
    .await
    .expect("a ready action ends the wait");

    // Then: the action is handed back, the banner is left open, and the waiter
    // released its own slot
    assert_eq!(outcome, WaiterOutcome::Action(Some(42)));
    assert_eq!(count(&closed), 0);
    assert!(!registry.finish_token(&pane, token));
    assert!(!registry.withdraw(&pane));
}

#[tokio::test]
async fn re_registering_cancels_the_old_waiter_without_stealing_the_new_slot() {
    // Given: a waiter holding a pane
    let registry = WaiterRegistry::new();
    let pane = key("ws-1", "fe-1");
    let (first_token, first_cancel_rx) = registry.register(pane.clone());

    // When: a newer notification registers the same pane
    let (second_token, second_cancel_rx) = registry.register(pane.clone());

    // Then: the tokens differ, and the replaced waiter cannot release a slot it
    // no longer owns
    assert_ne!(first_token, second_token);
    assert!(!registry.finish_token(&pane, first_token));

    let first_closed = counter();
    let first_close_calls = Arc::clone(&first_closed);
    let first_outcome = tokio::time::timeout(
        BOUND,
        drive_cancellable_waiter(
            &registry,
            pane.clone(),
            first_token,
            pending::<Option<u32>>(),
            move || async move {
                bump(&first_close_calls);
            },
            first_cancel_rx,
        ),
    )
    .await
    .expect("the replaced waiter wakes");

    // The replaced waiter closed its own banner, exited, and left the slot to
    // its successor.
    assert_eq!(first_outcome, WaiterOutcome::Withdrawn);
    assert_eq!(count(&first_closed), 1);
    assert!(!registry.finish_token(&pane, first_token));

    // And a later withdrawal still reaches the newer waiter.
    assert!(registry.withdraw(&pane));
    let second_closed = counter();
    let second_close_calls = Arc::clone(&second_closed);
    let second_outcome = tokio::time::timeout(
        BOUND,
        drive_cancellable_waiter(
            &registry,
            pane.clone(),
            second_token,
            pending::<Option<u32>>(),
            move || async move {
                bump(&second_close_calls);
            },
            second_cancel_rx,
        ),
    )
    .await
    .expect("the surviving waiter wakes");

    assert_eq!(second_outcome, WaiterOutcome::Withdrawn);
    assert_eq!(count(&second_closed), 1);
    assert!(!registry.finish_token(&pane, second_token));
}

#[tokio::test]
async fn a_withdrawal_for_one_pane_leaves_another_pane_waiting() {
    // Given: two panes in the same workspace, each with a live waiter
    let registry = WaiterRegistry::new();
    let first_pane = key("ws-1", "fe-1");
    let second_pane = key("ws-1", "fe-2");
    let (_first_token, first_cancel_rx) = registry.register(first_pane.clone());
    let (second_token, second_cancel_rx) = registry.register(second_pane.clone());

    // When: only the first pane is withdrawn
    assert!(registry.withdraw(&first_pane));

    // Then: the first pane's waiter wakes and the second pane keeps its slot,
    // so releasing it succeeds and its receiver was never signalled
    let first = tokio::time::timeout(BOUND, first_cancel_rx)
        .await
        .expect("the withdrawn pane wakes");
    assert_eq!(first, Ok(()));
    assert!(registry.finish_token(&second_pane, second_token));
    assert!(second_cancel_rx.await.is_err());
}

#[tokio::test]
async fn dropping_the_sender_without_a_signal_still_ends_the_wait() {
    // Given: a waiter whose slot was released by its owner, which drops the
    // sender without signalling
    let registry = WaiterRegistry::new();
    let pane = key("ws-1", "fe-1");
    let (token, cancel_rx) = registry.register(pane.clone());
    assert!(registry.finish_token(&pane, token));
    let closed = counter();
    let close_calls = Arc::clone(&closed);

    // When: the waiter drives with that receiver
    let outcome = tokio::time::timeout(
        BOUND,
        drive_cancellable_waiter(
            &registry,
            pane,
            token,
            pending::<Option<u32>>(),
            move || async move {
                bump(&close_calls);
            },
            cancel_rx,
        ),
    )
    .await
    .expect("a dropped sender ends the wait");

    // Then: it takes the withdrawal path, closing its banner and routing nothing
    assert_eq!(outcome, WaiterOutcome::Withdrawn);
    assert_eq!(count(&closed), 1);
}

#[tokio::test]
async fn a_foreign_token_cannot_retire_a_live_slot() {
    // Given: a fresh registration
    let registry = WaiterRegistry::default();
    let pane = key("ws-1", "fe-1");
    let (token, cancel_rx) = registry.register(pane.clone());

    // When: a token this waiter never received tries to release the pane
    let foreign = registry.finish_token(&pane, token.wrapping_add(1));

    // Then: the slot survives, so the owner's receiver is still live
    assert!(!foreign);
    assert!(registry.withdraw(&pane));
    let signalled = tokio::time::timeout(BOUND, cancel_rx)
        .await
        .expect("the owner's receiver is live");
    assert_eq!(signalled, Ok(()));
}
