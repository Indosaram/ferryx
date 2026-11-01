//! Move and promotion against the real lease rules ([`crate::account::supervisor_lease`]) on a
//! temporary account store, with explicit relay times. The paired channel and the import are
//! fakes that record what they were asked.

use super::*;
use crate::account::supervisor_lease as lease;

const USER: &str = "user-1";
const A: &str = "mach-a";
const B: &str = "mach-b";

/// The relay: one account with machines A and B, answered by the production lease rules.
struct Relay {
    dir: tempfile::TempDir,
    now: parking_lot::Mutex<u64>,
}

impl Relay {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let conn = crate::account::store::store_sqlite::open_sqlite_connection(dir.path()).unwrap();
        conn.execute("INSERT INTO users (user_id, email, created_at) VALUES (?1, 'o@example.com', 0)", [USER])
            .unwrap();
        for machine in [A, B] {
            conn.execute(
                "INSERT INTO machines (machine_record_id, user_id, machine_id, display_name, public_key, \
                 attach_public_key, relay_origin, platform, enrollment_epoch, enrolled_at) \
                 VALUES (?1, ?2, ?1, ?1, 'k', 'k', 'r', 'macos', 1, 0)",
                rusqlite::params![machine, USER],
            )
            .unwrap();
        }
        Self { dir, now: parking_lot::Mutex::new(1_000) }
    }

    fn at(&self, secs: u64) {
        *self.now.lock() = secs;
    }

    fn tx<T>(&self, step: impl FnOnce(&rusqlite::Transaction<'_>, u64) -> Result<T, lease::LeaseError>) -> Result<T, lease::LeaseError> {
        let now = *self.now.lock();
        lease::lease_transaction(self.dir.path(), |tx| step(tx, now))
    }

    fn row(&self) -> crate::account::store::SupervisorLeaseRecord {
        lease::current_in(self.dir.path(), USER).unwrap().expect("a lease row")
    }

    fn acquire(&self, machine: &str, checkpoint: Option<&str>, force: bool) -> Result<u64, lease::LeaseError> {
        self.tx(|tx, now| lease::acquire(tx, USER, machine, now, checkpoint, force)).map(|row| row.fence)
    }

    /// The relay's query answer, as `query_outcome` + `describe_lease` give it.
    fn query(&self, machine: &str) -> SupervisorLeaseAck {
        let now = *self.now.lock();
        let row = lease::current_in(self.dir.path(), USER).unwrap();
        let (outcome, reason) = match &row {
            Some(row) if !row.released && now < row.expires_at + lease::GRACE_SECS => {
                if row.holder_machine_id == machine {
                    (SupervisorLeaseOutcome::Granted, None)
                } else {
                    (SupervisorLeaseOutcome::Refused, Some("LEASE_HELD".to_owned()))
                }
            }
            _ => (SupervisorLeaseOutcome::Released, None),
        };
        SupervisorLeaseAck {
            request_id: "q".into(),
            outcome,
            fence: row.as_ref().map(|row| row.fence),
            holder_machine_id: row.as_ref().map(|row| row.holder_machine_id.clone()),
            holder_label: row.as_ref().map(|row| format!("Laptop {}", row.holder_machine_id)),
            expires_in_secs: None,
            previous_expires_in_secs: None,
            checkpoint_id: row.and_then(|row| row.checkpoint_id),
            account_machine_count: Some(2),
            reason_code: reason,
        }
    }
}

/// Machine `me`'s lease client against the relay; acquires feed the checkpoint state the way
/// the lease client does.
struct Lease<'a> {
    relay: &'a Relay,
    me: &'static str,
    state: &'a CheckpointState,
    acquires: parking_lot::Mutex<Vec<bool>>,
}

#[async_trait::async_trait]
impl MoveLease for Lease<'_> {
    fn machine_id(&self) -> String {
        self.me.to_owned()
    }

    async fn query(&self) -> Option<SupervisorLeaseAck> {
        Some(self.relay.query(self.me))
    }

    async fn acquire(&self, force: bool) -> bool {
        self.acquires.lock().push(force);
        let local = self.state.local().map(|local| local.id);
        match self.relay.acquire(self.me, local.as_deref(), force) {
            Ok(_) => {
                self.state.note_refusal(None);
                true
            }
            Err(error) => {
                let row = self.relay.row();
                self.state.note_refusal(Some(LeaseRefusal {
                    code: error.code().to_owned(),
                    expected_checkpoint: row.checkpoint_id,
                    holder_machine_id: Some(row.holder_machine_id),
                    holder_label: None,
                }));
                false
            }
        }
    }
}

/// The old holder A: on release-to it stops, publishes `archive` and releases reserved for
/// the target, as the holder endpoint does. `reachable: false` is a holder that is gone.
struct Holder<'a> {
    relay: &'a Relay,
    fence: u64,
    archive: Vec<u8>,
    reachable: bool,
    released_to: parking_lot::Mutex<Option<String>>,
}

#[async_trait::async_trait]
impl MovePeer for Holder<'_> {
    async fn release_to(&self, holder: &str, target: &str) -> Result<Option<String>, MoveError> {
        assert_eq!(holder, A, "the move asks the machine the relay names");
        if !self.reachable {
            return Err(MoveError::new(503, HOLDER_UNREACHABLE, "gone"));
        }
        let id = checkpoint_id_of(&self.archive);
        let released = self
            .relay
            .tx(|tx, now| lease::release_to(tx, USER, A, self.fence, now, Some(id.as_str()), target))
            .unwrap();
        assert!(released);
        *self.released_to.lock() = Some(target.to_owned());
        Ok(Some(id))
    }

    async fn fetch(&self, _holder: &str) -> Result<(String, Vec<u8>), MoveError> {
        Ok((checkpoint_id_of(&self.archive), self.archive.clone()))
    }
}

#[derive(Default)]
struct Importer {
    imported: parking_lot::Mutex<Vec<PathBuf>>,
    discarded: parking_lot::Mutex<u32>,
    refuse: Option<&'static str>,
}

#[async_trait::async_trait]
impl MoveImport for Importer {
    async fn import(&self, archive: &Path) -> Result<(), MoveError> {
        if let Some(code) = self.refuse {
            return Err(MoveError::new(409, code, "refused"));
        }
        assert!(archive.exists(), "the import reads the stored checkpoint");
        self.imported.lock().push(archive.to_owned());
        Ok(())
    }

    async fn discard(&self) {
        *self.discarded.lock() += 1;
    }
}

fn holder<'a>(relay: &'a Relay, fence: u64, reachable: bool) -> Holder<'a> {
    Holder { relay, fence, archive: b"goals and lease".to_vec(), reachable, released_to: Default::default() }
}

fn lease<'a>(relay: &'a Relay, state: &'a CheckpointState) -> Lease<'a> {
    Lease { relay, me: B, state, acquires: Default::default() }
}

fn store() -> (tempfile::TempDir, CheckpointStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = CheckpointStore::new(dir.path().join(CHECKPOINT_DIR));
    (dir, store)
}

#[tokio::test]
async fn a_move_with_a_live_holder_transfers_the_checkpoint_and_the_lease() {
    let relay = Relay::new();
    let fence = relay.acquire(A, None, false).unwrap();
    let (_dir, store) = store();
    let state = CheckpointState::default();
    let (holder, lease, importer) = (holder(&relay, fence, true), lease(&relay, &state), Importer::default());

    let moved = move_here(&lease, &holder, &importer, &store, &state, false, 1_000).await;

    assert_eq!(moved, Ok(true));
    assert_eq!(holder.released_to.lock().as_deref(), Some(B), "A released reserved for B");
    let row = relay.row();
    assert_eq!(row.holder_machine_id, B);
    assert_eq!(row.fence, fence + 1, "the target holds the next fence");
    let id = checkpoint_id_of(&holder.archive);
    assert_eq!(row.checkpoint_id.as_deref(), Some(id.as_str()));
    assert_eq!(*importer.imported.lock(), vec![store.path_of(&id)], "the final checkpoint was imported");
    assert_eq!(*lease.acquires.lock(), vec![false], "never forced");
    // The old holder is a standby now: its renew is refused.
    let renew = relay.tx(|tx, now| lease::renew(tx, USER, A, fence, now, None));
    assert_eq!(renew.unwrap_err(), lease::LeaseError::NotHolder);
    assert_eq!(relay.query(A).outcome, SupervisorLeaseOutcome::Refused);
}

#[tokio::test]
async fn a_failed_import_leaves_the_reserved_lease_to_the_old_holder() {
    let relay = Relay::new();
    let fence = relay.acquire(A, None, false).unwrap();
    let (_dir, store) = store();
    let state = CheckpointState::default();
    let importer = Importer { refuse: Some(ferryx_hub::checkpoint::SUPERVISOR_IDENTITY_MISMATCH), ..Default::default() };
    let (holder, lease) = (holder(&relay, fence, true), lease(&relay, &state));

    let error = move_here(&lease, &holder, &importer, &store, &state, false, 1_000).await.unwrap_err();

    assert_eq!(error.code, ferryx_hub::checkpoint::SUPERVISOR_IDENTITY_MISMATCH);
    assert!(lease.acquires.lock().is_empty(), "no acquire after a refused import");
    // Task 1 same-holder rule: A re-acquires its own row and the reservation is cleared.
    assert_eq!(relay.acquire(A, None, false), Ok(fence));
    assert_eq!(relay.row().reserved_for, None);
}

#[tokio::test]
async fn a_live_but_unreachable_holder_keeps_the_lease() {
    let relay = Relay::new();
    let fence = relay.acquire(A, None, false).unwrap();
    let (_dir, store) = store();
    let state = CheckpointState::default();
    let (holder, lease, importer) = (holder(&relay, fence, false), lease(&relay, &state), Importer::default());

    let error = move_here(&lease, &holder, &importer, &store, &state, true, 1_000).await.unwrap_err();

    assert_eq!((error.status, error.code.as_str()), (409, SUPERVISOR_STANDBY));
    assert_eq!(error.details["holderMachineId"], A);
    assert_eq!(error.details["holderLabel"], "Laptop mach-a");
    assert!(lease.acquires.lock().is_empty(), "even force does not take a live lease");
    assert_eq!(relay.row().holder_machine_id, A);
}

/// A holds with checkpoint `synced` recorded, then disappears; B's newest is `local`.
fn expired_with(relay: &Relay, synced: &[u8]) -> u64 {
    let id = checkpoint_id_of(synced);
    let fence = relay.acquire(A, Some(id.as_str()), false).unwrap();
    relay.at(1_000 + lease::TTL_SECS + lease::GRACE_SECS);
    fence
}

#[tokio::test]
async fn an_expired_holder_with_a_matching_checkpoint_is_taken_over_without_force() {
    let relay = Relay::new();
    let fence = expired_with(&relay, b"synced");
    let (_dir, store) = store();
    let state = CheckpointState::default();
    let synced = store.save(&checkpoint_id_of(b"synced"), b"synced", 1_100, true).unwrap();
    state.set_local(Some(synced.clone()));
    let (holder, lease, importer) = (holder(&relay, fence, false), lease(&relay, &state), Importer::default());

    let moved = move_here(&lease, &holder, &importer, &store, &state, false, 1_200).await;

    assert_eq!(moved, Ok(true));
    assert_eq!(*lease.acquires.lock(), vec![false]);
    assert_eq!(*importer.imported.lock(), vec![store.path_of(&synced.id)]);
    assert_eq!(relay.row().holder_machine_id, B);
    assert_eq!(relay.row().fence, fence + 1);
}

#[tokio::test]
async fn a_stale_checkpoint_needs_force_and_names_what_is_lost() {
    let relay = Relay::new();
    let fence = expired_with(&relay, b"synced");
    let (_dir, store) = store();
    let state = CheckpointState::default();
    state.set_local(Some(store.save(&checkpoint_id_of(b"older"), b"older", 900, true).unwrap()));
    let (holder, lease, importer) = (holder(&relay, fence, false), lease(&relay, &state), Importer::default());

    let error = move_here(&lease, &holder, &importer, &store, &state, false, 1_200).await.unwrap_err();

    assert_eq!(error.code, SUPERVISOR_FORCE_REQUIRED);
    assert_eq!(error.details["holderLabel"], "Laptop mach-a");
    assert_eq!(error.details["checkpointAt"], 900);
    assert!(error.message.contains("changes since 900 on Laptop mach-a will be lost"), "{}", error.message);
    assert!(lease.acquires.lock().is_empty(), "no acquire without the confirm flag");
    assert_eq!(relay.row().holder_machine_id, A);

    // The explicit confirm takes over, forced, without importing the stale checkpoint.
    let moved = move_here(&lease, &holder, &importer, &store, &state, true, 1_200).await;
    assert_eq!(moved, Ok(true));
    assert_eq!(*lease.acquires.lock(), vec![true]);
    assert!(importer.imported.lock().is_empty());
    assert_eq!(relay.row().holder_machine_id, B);
}

#[test]
fn auto_promotion_needs_the_recorded_checkpoint() {
    let relay = Relay::new();
    expired_with(&relay, b"synced");
    let (_dir, store) = store();
    let state = CheckpointState::default();

    // No checkpoint here: no promotion, and status says why.
    state.note_query(&relay.query(B));
    assert!(!state.may_auto_acquire());
    assert_eq!(promotion_code(&state), Some("CHECKPOINT_MISSING"));

    // A stale one: still none.
    state.set_local(Some(store.save(&checkpoint_id_of(b"older"), b"older", 900, true).unwrap()));
    state.note_query(&relay.query(B));
    assert!(!state.may_auto_acquire());
    assert_eq!(promotion_code(&state), Some("CHECKPOINT_STALE"));
    let status = state.status(1_000);
    assert_eq!(status.checkpoint_id, Some(checkpoint_id_of(b"older")));
    assert_eq!(status.checkpoint_age_secs, Some(100));
    assert!(status.block_reason.is_some_and(|reason| reason.contains("stale")));

    // The matching one: promotable, and the relay grants it unforced.
    let synced = store.save(&checkpoint_id_of(b"synced"), b"synced", 1_100, true).unwrap();
    state.set_local(Some(synced.clone()));
    state.note_query(&relay.query(B));
    assert!(state.may_auto_acquire());
    assert_eq!(promotion_code(&state), None);
    assert!(relay.acquire(B, Some(synced.id.as_str()), false).is_ok());

    // A machine whose owner stopped the hub never takes part.
    state.set_hub_stopped(true);
    assert!(!state.may_auto_acquire());
    assert_eq!(promotion_code(&state), Some("HUB_STOPPED"));
}

#[test]
fn a_live_holder_blocks_auto_promotion() {
    let relay = Relay::new();
    relay.acquire(A, None, false).unwrap();
    let state = CheckpointState::default();
    state.note_query(&relay.query(B));
    assert!(!state.may_auto_acquire());
    assert_eq!(promotion_code(&state), Some("LEASE_HELD"));
}

fn promotion_code(state: &CheckpointState) -> Option<&'static str> {
    state.block_reason().map(|block| match block {
        PromotionBlock::HubStopped => "HUB_STOPPED",
        PromotionBlock::CheckpointMissing { .. } => "CHECKPOINT_MISSING",
        PromotionBlock::CheckpointStale { .. } => "CHECKPOINT_STALE",
        PromotionBlock::HolderLive { .. } => "LEASE_HELD",
        PromotionBlock::Refused { .. } => "REFUSED",
    })
}

#[test]
fn the_store_keeps_the_newest_two_owner_only() {
    let (_dir, store) = store();
    let ids: Vec<String> = [b"one".as_slice(), b"two".as_slice(), b"three".as_slice()]
        .iter()
        .enumerate()
        .map(|(at, bytes)| store.save(&checkpoint_id_of(bytes), bytes, at as u64, true).unwrap().id)
        .collect();
    assert_eq!(store.newest().map(|entry| entry.id), Some(ids[2].clone()));
    assert!(store.find(&ids[1]).is_some());
    assert!(!store.path_of(&ids[0]).exists(), "only the newest two are kept");
    assert!(store.save(&ids[0], b"not one", 9, true).is_err(), "an archive must match its id");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(store.path_of(&ids[2])).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
