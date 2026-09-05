pub use ferryx_lib::{scoped_contracts, remote, terminal};
#[path = "../src/ferryx_scope/control/mod.rs"]
mod control;
use control::*;
use scoped_contracts::*;

fn target(host: &str) -> TargetRef { TargetRef { host_id: host.into(), owner_id: "owner".into(), epoch: Epoch(1), backend_session_id: "same-id".into() } }
fn inventory(cap: usize) -> Inventory {
    let mut i = Inventory::new(cap);
    for host in ["a", "b"] {
        i.register_host(host.into(), "owner".into(), Epoch(1), true);
        i.insert(Agent { target: target(host), workspace_id: host.into(), label: host.into(), state: "idle".into(), revision: 0, source: TransitionSource::Lifecycle }).unwrap();
    }
    i
}
#[test]
fn global_waiting_and_stale_epoch_and_completion_provenance() {
    let mut i = inventory(2);
    for host in ["a", "b"] { i.report(&target(host), TransitionKind::Waiting, TransitionSource::TerminalDetection { detector: "qualified".into() }).unwrap(); }
    assert_eq!(i.snapshot().items.iter().filter(|a| a.state == "waiting").count(), 2);
    assert_eq!(i.snapshot().completeness, InventoryCompleteness::Complete);
    assert!(i.report(&target("a"), TransitionKind::TaskComplete, TransitionSource::Lifecycle).is_err());
    i.register_host("a".into(), "owner".into(), Epoch(2), false);
    assert_eq!(i.validate(&target("a")).unwrap_err().code, ScopeErrorCode::TargetExpired);
}
#[test]
fn atomic_snapshot_delta_gap_and_tombstone() {
    let mut i = inventory(1);
    let (snapshot, mut rx) = i.subscribe();
    i.report(&target("a"), TransitionKind::Working, TransitionSource::Lifecycle).unwrap();
    assert!(rx.try_recv().unwrap().revision > snapshot.revision);
    i.report(&target("a"), TransitionKind::Stopped, TransitionSource::Lifecycle).unwrap();
    assert_eq!(i.validate(&target("a")).unwrap_err().code, ScopeErrorCode::TargetExpired);
    assert!(matches!(i.replay(0), EventReplay::Gap { .. }));
}
