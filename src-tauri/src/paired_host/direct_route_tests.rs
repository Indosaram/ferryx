use super::*;

fn next(rx: &mut broadcast::Receiver<RouteEvent>) -> RouteEvent {
    rx.try_recv().expect("route event must already be emitted")
}

#[test]
fn unknown_host_uses_relay() {
    let routes = DirectRoutes::new();
    assert_eq!(routes.state("h"), RouteState::Relay);
    assert_eq!(routes.origin_for("h"), None);
}

#[test]
fn failed_attempt_stays_on_relay_and_backs_off() {
    let routes = DirectRoutes::new();
    let mut rx = routes.subscribe();
    let now = Instant::now();
    let epoch = routes.begin_attempt("h", now).unwrap();
    assert_eq!(next(&mut rx).state, RouteState::Attempting);
    assert_eq!(routes.origin_for("h"), None, "attempt never blocks relay");
    assert_eq!(routes.begin_attempt("h", now), None, "one attempt at a time");
    routes.fail("h", epoch, now);
    assert_eq!(next(&mut rx).state, RouteState::Relay);
    assert_eq!(routes.begin_attempt("h", now), None, "inside backoff");
    assert!(routes.begin_attempt("h", now + RETRY_BASE).is_some(), "retry after backoff");
}

#[test]
fn in_backoff_is_true_only_while_a_retry_is_pending() {
    let routes = DirectRoutes::new();
    let now = Instant::now();
    // A host nothing ever attempted is not in backoff: its cold first attempt must still wait.
    assert!(!routes.in_backoff("h", now));
    let epoch = routes.begin_attempt("h", now).unwrap();
    assert!(!routes.in_backoff("h", now), "an attempt in flight is not backoff");
    routes.fail("h", epoch, now);
    assert!(routes.in_backoff("h", now), "a failed attempt parks the host");
    assert!(!routes.in_backoff("h", now + RETRY_BASE), "the window closes at retry_at");
    // A published direct route is not backoff either, even with a stale retry_at.
    let epoch = routes.begin_attempt("h", now + RETRY_BASE).unwrap();
    routes.publish("h", epoch, "http://127.0.0.1:7".into(), DirectGuard::default());
    assert!(!routes.in_backoff("h", now));
}

#[test]
fn publish_then_loss_returns_to_relay_and_retries() {
    let routes = DirectRoutes::new();
    let mut rx = routes.subscribe();
    let now = Instant::now();
    let epoch = routes.begin_attempt("h", now).unwrap();
    next(&mut rx);
    assert!(routes.publish("h", epoch, "http://127.0.0.1:5000".into(), DirectGuard::default()));
    assert_eq!(
        next(&mut rx).state,
        RouteState::Direct { origin: "http://127.0.0.1:5000".into() }
    );
    assert_eq!(routes.origin_for("h").as_deref(), Some("http://127.0.0.1:5000"));
    routes.mark_disconnected("h", epoch, now);
    assert_eq!(next(&mut rx).state, RouteState::Disconnected);
    assert_eq!(routes.origin_for("h"), None, "lost direct falls back synchronously");
    let retry = routes.begin_attempt("h", now + RETRY_BASE).expect("background retry");
    assert!(retry > epoch);
}

#[test]
fn stale_epoch_cannot_publish_or_disconnect() {
    let routes = DirectRoutes::new();
    let now = Instant::now();
    let old = routes.begin_attempt("h", now).unwrap();
    routes.fail("h", old, now);
    let new = routes.begin_attempt("h", now + RETRY_BASE).unwrap();
    assert!(!routes.publish("h", old, "http://127.0.0.1:1".into(), DirectGuard::default()));
    assert!(routes.publish("h", new, "http://127.0.0.1:2".into(), DirectGuard::default()));
    routes.mark_disconnected("h", old, now);
    assert_eq!(routes.origin_for("h").as_deref(), Some("http://127.0.0.1:2"));
}

#[test]
fn reset_drops_direct_route() {
    let routes = DirectRoutes::new();
    let now = Instant::now();
    let epoch = routes.begin_attempt("h", now).unwrap();
    routes.publish("h", epoch, "http://127.0.0.1:3".into(), DirectGuard::default());
    routes.reset("h");
    assert_eq!(routes.origin_for("h"), None);
    assert!(routes.begin_attempt("h", now).is_some(), "reset clears backoff");
}

#[test]
fn attempt_from_before_reset_cannot_publish_into_new_attempt() {
    let routes = DirectRoutes::new();
    let now = Instant::now();
    let old = routes.begin_attempt("h", now).unwrap();
    routes.reset("h");
    let new = routes.begin_attempt("h", now).unwrap();
    assert_ne!(old, new, "epochs are never reused across reset");
    assert!(!routes.publish("h", old, "http://127.0.0.1:9".into(), DirectGuard::default()));
    assert_eq!(routes.state("h"), RouteState::Attempting);
}

#[tokio::test]
async fn guard_drop_aborts_tracked_tasks() {
    let (_tx, rx) = tokio::sync::oneshot::channel::<()>();
    let task = tokio::spawn(async move {
        let _ = rx.await;
    });
    let mut guard = DirectGuard::default();
    guard.track(task.abort_handle());
    drop(guard);
    assert!(task.await.unwrap_err().is_cancelled());
}
