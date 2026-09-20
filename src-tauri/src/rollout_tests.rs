//! In-process retirement rehearsal: no daemon launch, PTY, HOME lookup, or process exit.
use crate::{daemon::handover as handover_seam, terminal};

#[tokio::test]
async fn a24_rollback_waits_for_drain_and_preserves_unrelated_owner() {
    use handover_seam::{HandoverManager, HandoverStatus};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::time::Duration;
    let root = tempfile::tempdir_in(std::env::var_os("TMPDIR").expect("isolated TMPDIR")).unwrap();
    let old = Arc::new(HandoverManager::new(root.path().join("old.sock")));
    let unrelated = HandoverManager::new(root.path().join("unrelated.sock"));
    let terminals = Arc::new(terminal::TerminalService::default());
    let unrelated_terminals = Arc::new(terminal::TerminalService::default());
    let unrelated_retirements = Arc::new(AtomicUsize::new(0));
    let count = unrelated_retirements.clone();
    unrelated.set_retirement_action(move || {
        count.fetch_add(1, Ordering::SeqCst);
    });
    let (retired, mut observed) = tokio::sync::watch::channel(false);
    old.set_retirement_action(move || {
        retired.send_replace(true);
    });
    let connection = old.retain_request(terminals.clone()).unwrap();
    let operation = old.retain_request(terminals.clone()).unwrap();
    let mut committed = old.subscribe_client_abort();
    let mut other_committed = unrelated.subscribe_client_abort();
    std::fs::write(root.path().join("unrelated.sock"), b"unrelated-owner-route").unwrap();
    std::fs::write(root.path().join("credentials"), b"fixture-credential-bytes").unwrap();
    old.commit_handover(&terminals).unwrap();
    committed.try_recv().unwrap();
    drop(connection);
    old.check_retirement_if_empty(&terminals);
    assert_eq!(old.status(), HandoverStatus::Draining);
    assert!(!*observed.borrow());
    drop(operation);
    tokio::time::timeout(Duration::from_secs(5), observed.wait_for(|done| *done))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(old.status(), HandoverStatus::Retired);
    assert!(old.retain_request(terminals.clone()).is_err());
    unrelated.check_retirement_if_empty(&unrelated_terminals);
    assert_eq!(unrelated.status(), HandoverStatus::Active);
    assert_eq!(unrelated_retirements.load(Ordering::SeqCst), 0);
    assert!(matches!(
        other_committed.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
    assert_eq!(
        std::fs::read(root.path().join("unrelated.sock")).unwrap(),
        b"unrelated-owner-route"
    );
    assert_eq!(
        std::fs::read(root.path().join("credentials")).unwrap(),
        b"fixture-credential-bytes"
    );
    drop(old);
    drop(unrelated);
    root.close().unwrap();
}
