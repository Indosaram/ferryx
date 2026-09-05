use super::*;
use crate::scoped_contracts::{Epoch, TransitionKind, TransitionSource};
fn event() -> InventoryTransition { InventoryTransition { revision: 7, target: TargetRef { host_id: "h".into(), owner_id: "o".into(), epoch: Epoch(u64::MAX), backend_session_id: "s".into() }, kind: TransitionKind::Waiting, source: TransitionSource::TerminalDetection { detector: "qualified".into() } } }
fn sub() -> Subscription { Subscription { endpoint: "https://fcm.googleapis.com/send/a".into(), keys: Keys { p256dh: "key".into(), auth: "auth".into() }, expiration_time: None } }
#[test]
fn retained_waiting_is_sent_once_and_idle_is_not_complete() {
    let mut store = PushStore::default(); store.subscribe("d", sub(), false).unwrap(); let mut e = event();
    assert_eq!(store.pending(&e, 0).len(), 1, "retained waiting must not be missed on startup");
    store.mark_delivered("d", &e); assert!(store.pending(&e, 0).is_empty());
    e.revision += 1; e.kind = TransitionKind::Idle; assert!(store.pending(&e, 0).is_empty());
    e.kind = TransitionKind::TaskComplete; assert!(store.pending(&e, 0).is_empty(), "terminal idle cannot confirm completion");
    e.source = TransitionSource::Provider { provider: crate::scoped_contracts::CanonicalProvider::Codex, request_id: None };
    assert_eq!(store.pending(&e, 0).len(), 1);
}
#[test]
fn revoke_and_endpoint_ownership_are_enforced() {
    let mut store = PushStore::default(); store.subscribe("d", sub(), false).unwrap();
    assert!(store.subscribe("foreign", sub(), false).is_err());
    assert!(store.unsubscribe("foreign", &sub().endpoint).is_err());
    store.revoke("d"); assert!(store.subscriptions.is_empty()); assert!(store.pending(&event(), 0).is_empty());
}
#[test]
fn endpoint_policy_rejects_ssrf_and_expiry_is_not_deliverable() {
    for url in ["http://fcm.googleapis.com/send/a", "https://127.0.0.1/a", "https://fcm.googleapis.com.evil.test/a", "https://user@fcm.googleapis.com/a", "https://fcm.googleapis.com:444/a", "https://fcm.googleapis.com/a#x"] { assert!(validate_endpoint(url).is_err(), "{url}"); }
    assert!(validate_endpoint(&sub().endpoint).is_ok());
    let mut store = PushStore::default(); let mut s = sub(); s.expiration_time = Some(10); store.subscribe("d", s, false).unwrap(); assert!(store.pending(&event(), 10).is_empty());
}
