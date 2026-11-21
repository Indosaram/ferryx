//! Phase 4 acceptance: the decision record's accounting, and the three handover failure
//! injections asserted through the real request and ownership gates (`handle_client`), not only on
//! the transaction file.
//!
//! (a) A successor that dies BEFORE the durable decision leaves the predecessor alive and serving,
//!     and a retried handover commits. (b) A successor that dies AFTER the decision leaves the
//!     predecessor unable to resume serving and unable to read or write the transferred PTY. (c) A
//!     predecessor that dies BEFORE the decision aborts the handover, leaves authority with its own
//!     durable state, and hands the successor no PTY.
//!
//! (c) covers direct-owned sessions only: a keeper-owned session stays reachable through the
//! keeper, whose durable ownership table and startup reconciliation are asserted directly below
//! against the real `OwnershipStore` rather than against a keeper process.
use super::*;
use crate::daemon::handover::{HandoverManager, HandoverStatus, SESSION_NOT_OWNED};
use crate::daemon::handover_request::{
    request_path, take_startup_request, write_request, HandoverRequest, StartupRole,
};
use crate::daemon::handover_transaction::{DecisionAccounting, HandoverState, HandoverTransaction};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// One request over the real `handle_client` loop, answered with its first response line.
async fn exchange(server: &Arc<DaemonServer>, request: DaemonRequest) -> DaemonResponse {
    let (client, server_side) = tokio::io::duplex(64 * 1024);
    let handler = Arc::clone(server);
    let serving = tokio::spawn(async move { handler.handle_client(server_side).await });
    let (read_half, mut write_half) = tokio::io::split(client);
    let mut line = serde_json::to_string(&request).unwrap();
    line.push('\n');
    write_half.write_all(line.as_bytes()).await.unwrap();
    let mut answer = String::new();
    BufReader::new(read_half).read_line(&mut answer).await.unwrap();
    drop(write_half);
    // Aborted and then awaited, so the served future - and the retirement guard it holds - is
    // dropped before the caller sends a request that has to drain in-flight work.
    serving.abort();
    let _ = serving.await;
    serde_json::from_str(answer.trim()).unwrap()
}

/// The decision record exactly as written, so a test can assert what its accounting says.
fn recorded_transaction(legacy: &Path) -> Option<HandoverTransaction> {
    let bytes = std::fs::read(legacy.with_extension("transaction")).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// A predecessor prepared for a handover on `root/legacy.sock`, holding one real `/bin/sh`
/// session and a retirement action that does not exit the test process.
fn predecessor_with_session(
    root: &Path,
) -> (Arc<DaemonServer>, PathBuf, String, tokio::sync::mpsc::Receiver<Vec<u8>>) {
    let server = Arc::new(DaemonServer::new_with_paths(
        Some(root.join("config")),
        Some(root.join("auth")),
    ));
    let legacy = root.join("legacy.sock");
    server.handover_manager.force_prepared_for_test(legacy.clone());
    server.handover_manager.set_retirement_action(|| {});
    let (session_id, output) = server
        .terminal_service
        .pty_manager()
        .spawn(portable_pty::CommandBuilder::new("/bin/sh"), 80, 24)
        .expect("spawn a local session");
    // `PtyManager::spawn` alone leaves the session out of the output hub; production never does,
    // because `TerminalService::register_output` registers the hub session for every spawn
    // (service.rs:688). Without it the attach gate answers SESSION_NOT_FOUND for a session this
    // daemon really owns, which would test the hub's registry rather than the handover gates.
    let _ = server.terminal_service.output_hub().register_session(&session_id);
    server
        .terminal_service
        .output_hub()
        .record_initial_size(&session_id, 80, 24);
    (server, legacy, session_id, output)
}

/// Waits (bounded) for `marker` on `output`; the error carries what was seen instead.
async fn output_contains(
    output: &mut tokio::sync::mpsc::Receiver<Vec<u8>>,
    marker: &str,
) -> Result<(), String> {
    let mut seen = Vec::new();
    let found = tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(chunk) = output.recv().await {
            seen.extend_from_slice(&chunk);
            if String::from_utf8_lossy(&seen).contains(marker) {
                return true;
            }
        }
        false
    })
    .await
    .unwrap_or(false);
    if found {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&seen).into_owned())
    }
}

/// A transferred session's child outlives the daemon that let go of it, so every test ends by
/// stopping the one it spawned. A pid that already exited is not an error.
fn kill_session_child(pid: Option<u32>) {
    if let Some(pid) = pid {
        // SAFETY: `kill` only delivers a signal to the pid of a child this test spawned itself.
        unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
    }
}

/// (a) The successor exported the session and then died: no commit, no decision. The predecessor
/// takes the session back through the real abort gate, still serves it through the real attach and
/// write gates, and a retried handover commits.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn successor_dying_before_the_decision_leaves_the_predecessor_serving_and_a_retry_committing() {
    let root = tempfile::tempdir().unwrap();
    let (server, legacy, session_id, mut output) = predecessor_with_session(root.path());
    // The candidate adopted the session (its reader is parked here) and then died.
    let export = server
        .terminal_service
        .pty_manager()
        .export_session(&session_id)
        .expect("the predecessor exports the session for the successor");
    let child = export.pid;
    server.handover_manager.note_transferred_local(&session_id);
    assert!(recorded_transaction(&legacy).is_none(), "no decision exists while the successor is gone");
    assert_eq!(
        server.handover_manager.status(),
        HandoverStatus::Prepared,
        "precondition: the predecessor is still waiting for its successor"
    );

    // The pre-commit rollback gate: the predecessor takes its exported sessions back and records
    // the abort decision, the only durable decision this handover ever writes.
    assert!(matches!(
        exchange(&server, DaemonRequest::AbortHandover).await,
        DaemonResponse::AbortHandoverOk
    ));
    drop(export);

    // Alive, and serving the session it never gave up.
    assert_eq!(server.handover_manager.status(), HandoverStatus::Active);
    assert!(server.handover_manager.accept_gate_open());
    assert!(matches!(
        exchange(&server, DaemonRequest::Attach { session_id: session_id.clone(), after_sequence: None }).await,
        DaemonResponse::AttachOk { .. }
    ));
    assert!(matches!(
        exchange(
            &server,
            DaemonRequest::Write {
                session_id: session_id.clone(),
                data: b"printf 'BEFORE_%s\\n' 'DECISION'\n".to_vec(),
            }
        )
        .await,
        DaemonResponse::WriteOk
    ));
    output_contains(&mut output, "BEFORE_DECISION")
        .await
        .expect("the resumed reader serves the session again");

    // The abort decision names the session it is about and declares only the successor identity
    // unpopulated, so nothing in it can be read as "never committed" and its populated inventory
    // is not declared absent.
    let record = recorded_transaction(&legacy).expect("an abort decision");
    assert_eq!(record.state, HandoverState::Active);
    assert_eq!(
        record.decision_accounting,
        Some(DecisionAccounting::SuccessorIdentityUnused),
        "only the successor identity is unpopulated by construction"
    );
    assert_eq!(record.successor_pid, 0, "the predecessor never learns the candidate's pid");
    assert!(!record.session_inventory.is_empty(), "the inventory names what the decision is about");
    assert_eq!(
        record.session_inventory.iter().map(|entry| entry.session_id.as_str()).collect::<Vec<_>>(),
        vec![session_id.as_str()]
    );
    assert_eq!(HandoverManager::unpopulated_accounting(&record), Some("successorPid"));

    // The retry commits on this same predecessor.
    server.handover_manager.force_prepared_for_test(root.path().join("legacy-retry.sock"));
    assert!(matches!(
        exchange(&server, DaemonRequest::CommitHandover { legacy_socket_path: None }).await,
        DaemonResponse::CommitHandoverOk
    ));
    assert_eq!(server.handover_manager.status(), HandoverStatus::Retired);
    server.terminal_service.close_session(&session_id).await.ok();
    kill_session_child(child);
}

/// (b) After the durable decision the predecessor neither resumes serving nor reads or writes the
/// transferred PTY: the real ownership gate answers SESSION_NOT_OWNED and the PTY registry no
/// longer holds the master.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn successor_dying_after_the_decision_leaves_the_predecessor_unable_to_resume_the_pty() {
    let root = tempfile::tempdir().unwrap();
    let (server, legacy, session_id, _output) = predecessor_with_session(root.path());
    let export = server
        .terminal_service
        .pty_manager()
        .export_session(&session_id)
        .expect("the predecessor exports the session for the successor");
    let child = export.pid;
    server.handover_manager.note_transferred_local(&session_id);

    assert!(matches!(
        exchange(&server, DaemonRequest::CommitHandover { legacy_socket_path: None }).await,
        DaemonResponse::CommitHandoverOk
    ));
    // The successor died after the decision: the descriptor copies it held are gone, the decision
    // is not.
    drop(export);

    // The decision is durable, names the session it handed over, and declares only the successor
    // identity unpopulated.
    assert_eq!(HandoverManager::recorded_decision(&legacy).unwrap(), Some(HandoverState::Retired));
    let record = recorded_transaction(&legacy).expect("a committed decision");
    assert_eq!(
        record.decision_accounting,
        Some(DecisionAccounting::SuccessorIdentityUnused),
        "only the successor identity is unpopulated by construction"
    );
    assert!(!record.session_inventory.is_empty(), "the inventory names what the decision is about");
    assert_eq!(
        record.session_inventory.iter().map(|entry| entry.session_id.as_str()).collect::<Vec<_>>(),
        vec![session_id.as_str()]
    );
    assert_eq!(server.handover_manager.status(), HandoverStatus::Retired);

    // Every real gate refuses the transferred session.
    for request in [
        DaemonRequest::Attach { session_id: session_id.clone(), after_sequence: None },
        DaemonRequest::Write { session_id: session_id.clone(), data: b"x".to_vec() },
    ] {
        match exchange(&server, request).await {
            DaemonResponse::Error { code, .. } => assert_eq!(code.as_deref(), Some(SESSION_NOT_OWNED)),
            other => panic!("a committed predecessor answered a transferred session with {other:?}"),
        }
    }
    // Resuming serving is refused, not merely unused: nothing is parked to resume and the registry
    // no longer holds the master the successor adopted.
    assert_eq!(server.terminal_service.pty_manager().resume_paused_readers(), 0);
    assert!(!server.terminal_service.pty_manager().has_session(&session_id));
    assert!(server.terminal_service.get_session(&session_id).is_none());
    assert!(server.terminal_service.pty_manager().write_input(&session_id, b"x").is_err());
    assert!(server.handover_manager.session_not_owned(&session_id));
    // A committed transaction is never returned to prepared: the abort is refused and the durable
    // decision is unchanged.
    assert!(server.handover_manager.abort_handover().is_err());
    assert_eq!(HandoverManager::recorded_decision(&legacy).unwrap(), Some(HandoverState::Retired));
    kill_session_child(child);
}

/// A real reaped process: its pid names no live process, so no start time can match it.
fn reaped_pid() -> u32 {
    let mut child = std::process::Command::new("/usr/bin/true").spawn().expect("spawn a throwaway process");
    let pid = child.id();
    child.wait().expect("reap the throwaway process");
    pid
}

/// (c) The predecessor died before any decision. The successor is never admitted as a candidate,
/// and no durable decision exists to make it an owner, so it adopts no direct-owned PTY.
/// Direct-owned sessions only: keeper-owned sessions stay reachable through the keeper (Phase 3).
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn predecessor_dying_before_the_decision_admits_no_successor_and_hands_over_no_ptys() {
    const GENERATION: &str = "test-generation";
    let root = tempfile::tempdir().unwrap();
    // tempfile's directory is 0700, which is what the daemon's runtime-path validation requires.
    let runtime = root.path().to_path_buf();

    // The predecessor had exported this session to the candidate and then died with no decision.
    // Its prepared legacy path is what its decision record would sit beside, so the socket file
    // outliving the process is what makes that path readable at all.
    let (predecessor, legacy, session_id, _output) = predecessor_with_session(root.path());
    assert_eq!(
        legacy,
        runtime.join("legacy.sock"),
        "the decision record belongs beside the legacy socket this predecessor prepared"
    );
    drop(std::os::unix::net::UnixListener::bind(&legacy).expect("bind the predecessor's socket"));
    assert_eq!(predecessor.handover_manager.status(), HandoverStatus::Prepared);
    let export = predecessor
        .terminal_service
        .pty_manager()
        .export_session(&session_id)
        .expect("the predecessor had exported the session before it died");
    let child = export.pid;
    predecessor.handover_manager.note_transferred_local(&session_id);
    drop(export);
    drop(predecessor);
    // Non-vacuity: this is the prepared legacy's own transaction path, and nothing wrote there.
    assert_eq!(legacy.with_extension("transaction"), runtime.join("legacy.transaction"));
    assert!(
        !legacy.with_extension("transaction").exists(),
        "the predecessor died before writing any decision beside its own legacy socket"
    );
    assert_eq!(
        HandoverManager::recorded_decision(&legacy).unwrap(),
        None,
        "authority stays with the dead predecessor's durable state: no decision exists"
    );
    assert_eq!(
        crate::daemon::handover::resolve_commit(false, HandoverManager::recorded_decision(&legacy).unwrap()),
        crate::daemon::handover::CommitResolution::Abort,
        "with no Retired decision a candidate must abort, never become the owner"
    );

    // The real startup gate: a request naming a dead predecessor admits no candidate at all.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    write_request(
        &runtime,
        &HandoverRequest::new(
            legacy.clone(),
            reaped_pid(),
            1,
            7,
            GENERATION.to_string(),
            runtime.join("daemon"),
            now,
        ),
    )
    .unwrap();
    match take_startup_request(
        &runtime,
        Some(GENERATION),
        now,
        &|pid| crate::daemon::owner_admission::process_start_time(pid),
    ) {
        StartupRole::Exit { reason } => assert!(
            reason.contains("no longer the process that wrote the request"),
            "a dead predecessor must be the refusal, not an unrelated one: {reason}"
        ),
        other => panic!("a dead predecessor must not admit a candidate: {other:?}"),
    }
    assert!(!request_path(&runtime).exists(), "the request authorizes one start only");
    assert_eq!(
        take_startup_request(
            &runtime,
            Some(GENERATION),
            now,
            &|pid| crate::daemon::owner_admission::process_start_time(pid),
        ),
        StartupRole::Ordinary
    );

    // The successor knows the session only through a route to its dead predecessor.
    let successor = Arc::new(DaemonServer::new_with_paths(
        Some(root.path().join("succ-config")),
        Some(root.path().join("succ-auth")),
    ));
    successor.session_router.add_legacy_peer(Arc::new(crate::daemon::proxy::LegacyPeer::new(
        legacy.clone(),
        vec![session_id.clone()],
    )));
    match exchange(
        &successor,
        DaemonRequest::Attach { session_id: session_id.clone(), after_sequence: None },
    )
    .await
    {
        // The route is followed to the dead predecessor and fails there, which is the point: the
        // successor never substitutes itself for the owner it could not confirm.
        DaemonResponse::Error { .. } => {}
        other => panic!("a successor must not serve a PTY it never adopted: {other:?}"),
    }
    // Not adopted: no local session, no PTY master, no ownership claim, and no decision of its own.
    assert!(successor.terminal_service.get_session(&session_id).is_none());
    assert!(successor.terminal_service.pty_manager().list_sessions().is_empty());
    assert!(successor.handover_manager.transferred_locals().is_empty());
    assert!(!successor.handover_manager.session_not_owned(&session_id));
    assert_eq!(successor.handover_manager.status(), HandoverStatus::Active);
    assert_eq!(HandoverManager::recorded_decision(&legacy).unwrap(), None);
    kill_session_child(child);
}

/// The typed accounting marker names exactly what the record really leaves unpopulated: a
/// populated inventory is never declared absent, a fully accounted record declares nothing, and
/// an undeclared record is not written as unpopulated at all.
#[test]
fn decision_accounting_is_declared_only_when_the_record_is_unpopulated() {
    let legacy = std::path::PathBuf::from("/tmp/ferryx-test/legacy.sock");
    let identity = legacy.to_string_lossy().into_owned();
    let mut transaction =
        HandoverTransaction::new_simple(&identity, &identity, std::process::id(), 0, 0, 0, &identity);
    assert_eq!(transaction.decision_accounting, None);
    assert_eq!(HandoverManager::unpopulated_accounting(&transaction), Some("successorPid"));
    assert_eq!(
        HandoverManager::accounting_declaration(&transaction),
        Some(DecisionAccounting::SuccessorIdentityAndInventoryUnused),
        "no successor identity and no exported session: both are unpopulated"
    );

    transaction.successor_pid = 4242;
    assert_eq!(HandoverManager::unpopulated_accounting(&transaction), Some("successorEpoch"));
    transaction.successor_epoch = 9;
    assert_eq!(HandoverManager::unpopulated_accounting(&transaction), Some("sessionInventory"));
    assert_eq!(
        HandoverManager::accounting_declaration(&transaction),
        Some(DecisionAccounting::SessionInventoryUnused),
        "the identity is populated now, so only the inventory may be declared unused"
    );

    transaction.session_inventory =
        vec![crate::daemon::handover_transaction::SessionInventoryEntry::pty("s-1")];
    assert_eq!(HandoverManager::unpopulated_accounting(&transaction), None);
    assert_eq!(
        HandoverManager::accounting_declaration(&transaction),
        None,
        "a fully accounted record declares nothing unpopulated"
    );

    transaction.successor_pid = 0;
    transaction.successor_epoch = 0;
    assert_eq!(
        HandoverManager::accounting_declaration(&transaction),
        Some(DecisionAccounting::SuccessorIdentityUnused),
        "an empty identity with a populated inventory is the production decision shape"
    );

    // Undeclared is not written at all, so an older reader sees exactly the record it wrote.
    let serialized = serde_json::to_value(&transaction).unwrap();
    assert!(serialized.get("decisionAccounting").is_none());
    let older: HandoverTransaction = serde_json::from_value(serialized).unwrap();
    assert_eq!(older.decision_accounting, None);

    transaction.decision_accounting = Some(DecisionAccounting::SuccessorIdentityUnused);
    let serialized = serde_json::to_value(&transaction).unwrap();
    assert_eq!(serialized["decisionAccounting"], serde_json::json!("successor_identity_unused"));
    assert_eq!(
        serde_json::from_value::<HandoverTransaction>(serialized)
            .unwrap()
            .decision_accounting,
        Some(DecisionAccounting::SuccessorIdentityUnused)
    );
}

/// Given a predecessor that died before its decision, when a successor reconciles startup against
/// the same durable ownership table, then the keeper-owned session is still recorded as keeper-owned
/// and discoverable, a direct-owned session is never turned into a keeper-owned one, and a
/// keeper-owned record the keeper no longer holds is reported vanished instead of fabricated alive.
#[test]
fn keeper_owned_sessions_survive_a_predecessor_death_that_aborts_the_handover() {
    use crate::daemon::keeper_session::{
        reconcile_startup, KeeperSessionView, OwnershipRecord, OwnershipStore, SessionOwner,
    };

    let root = tempfile::tempdir().unwrap();
    let store = OwnershipStore::at(root.path().join("pty-ownership.json"));
    let keeper_session = "keeper-owned-session";
    let direct_session = "direct-owned-session";
    for (session_id, owner, pid) in
        [(keeper_session, SessionOwner::Keeper, 4242), (direct_session, SessionOwner::Daemon, 4343)]
    {
        store
            .record(OwnershipRecord {
                session_id: session_id.to_string(),
                owner,
                epoch: if owner == SessionOwner::Keeper { 7 } else { 0 },
                cols: 80,
                rows: 24,
                pid,
                process_start_time: Some(1_700_000_000_000),
                generation: (owner == SessionOwner::Keeper).then(|| "gen-1".to_string()),
                created_at: 1_700_000_000_000,
                checkpoint_offset: 0,
            })
            .unwrap();
    }
    assert_eq!(store.load().unwrap().keeper_owned_count(), 1);
    assert!(
        matches!(
            store.refuse_rollback_if_keeper_sessions(),
            Err(crate::daemon::keeper_session::OwnershipRefusal::RollbackWithKeeperSessions { count: 1 })
        ),
        "a keeper-owned session still blocks rollback after the predecessor dies"
    );

    let discovery = reconcile_startup(
        &store,
        vec![KeeperSessionView {
            session_id: keeper_session.to_string(),
            pid: 4242,
            epoch: 8,
            sequence_offset: 128,
            cols: 80,
            rows: 24,
            owner: SessionOwner::Keeper.as_str().to_string(),
            generation: "gen-1".to_string(),
        }],
    )
    .unwrap();
    assert_eq!(
        discovery.sessions.iter().map(|view| view.session_id.as_str()).collect::<Vec<_>>(),
        vec![keeper_session],
        "the keeper answers for the session it still holds, so it stays discoverable"
    );
    assert!(discovery.vanished.is_empty(), "a session the keeper still holds did not vanish");

    let table = store.load().unwrap();
    assert_eq!(
        table.owner_of(keeper_session),
        Some(SessionOwner::Keeper),
        "authority stays with the keeper's durable state, not with the dead predecessor"
    );
    assert_eq!(
        table.owner_of(direct_session),
        Some(SessionOwner::Daemon),
        "a direct-owned session is never relabelled keeper-owned by reconciliation"
    );
    assert!(table.sessions[keeper_session].epoch >= 8, "the adopted epoch is the keeper's");

    store.record(OwnershipRecord {
        session_id: "keeper-session-now-gone".to_string(),
        owner: SessionOwner::Keeper,
        epoch: 3,
        cols: 80,
        rows: 24,
        pid: 4444,
        process_start_time: Some(1_700_000_000_000),
        generation: Some("gen-1".to_string()),
        created_at: 1_700_000_000_000,
        checkpoint_offset: 0,
    })
    .unwrap();
    let discovery = reconcile_startup(&store, Vec::new()).unwrap();
    assert!(
        discovery.vanished.contains(&"keeper-session-now-gone".to_string()),
        "a keeper-owned record the keeper no longer holds is reported, never assumed alive: {:?}",
        discovery.vanished
    );
    assert_eq!(
        store.load().unwrap().owner_of("keeper-session-now-gone"),
        None,
        "the vanished record is removed rather than claiming a survival that did not happen"
    );
}
