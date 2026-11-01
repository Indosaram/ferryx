//! Account-scoped supervisor lease with a monotonic fence.
//!
//! One machine per account holds the supervisor lease. Every grant to a new holder mints a fence
//! `(relay_epoch << 32) | counter` that never decreases for the account: `supervisor_lease_fences`
//! keeps the per-account high-water mark, and `supervisor_relay_epoch` is bumped by every standby
//! promotion so a relay restored from a lagging replica can neither reissue a used fence nor
//! revive an old holder. Lease rows are never deleted, only marked released.
//!
//! The rule functions take a connection (normally the `BEGIN IMMEDIATE` transaction from
//! [`lease_transaction`]) and touch only the lease tables, so a renew never loads or saves the
//! whole account store. All times are the relay's own clock, passed in by the caller.

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};

use crate::account::store::store_sqlite::codec::{
    supervisor_lease_from_row, table_exists, SUPERVISOR_LEASE_SELECT,
};
use crate::account::store::SupervisorLeaseRecord;

pub use crate::account::store::store_sqlite::{lease_read, lease_transaction};

/// Lease lifetime granted by every acquire and renew.
pub const TTL_SECS: u64 = 120;
/// How often a holder is expected to renew.
pub const RENEW_SECS: u64 = 20;
/// Extra time after expiry during which another machine still cannot take the lease.
pub const GRACE_SECS: u64 = 30;

pub const LEASE_UNSUPPORTED: &str = "LEASE_UNSUPPORTED";

const LEASES_TABLE: &str = "supervisor_leases";
const FENCES_TABLE: &str = "supervisor_lease_fences";
const EPOCH_TABLE: &str = "supervisor_relay_epoch";
const COUNTER_MASK: u64 = 0xFFFF_FFFF;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeaseError {
    /// The store has no lease tables (a read-only replica that predates schema v3).
    Unsupported,
    /// The machine is not enrolled to this account.
    NotEnrolled,
    /// Another machine holds a live (or in-grace) lease.
    Held { holder_machine_id: String, expires_at: u64 },
    /// The lease is free but the candidate's checkpoint is not the last recorded one.
    CheckpointMismatch { expected: Option<String> },
    /// Renew refused: wrong holder or fence, released row, or a fence from an older epoch.
    NotHolder,
    /// The fence space cannot advance (recorded fence from a later epoch, or counter exhausted).
    FenceExhausted,
    Store(String),
}

impl LeaseError {
    pub fn code(&self) -> &'static str {
        match self {
            LeaseError::Unsupported => LEASE_UNSUPPORTED,
            LeaseError::NotEnrolled => "LEASE_MACHINE_NOT_ENROLLED",
            LeaseError::Held { .. } => "LEASE_HELD",
            LeaseError::CheckpointMismatch { .. } => "LEASE_CHECKPOINT_MISMATCH",
            LeaseError::NotHolder => "LEASE_NOT_HOLDER",
            LeaseError::FenceExhausted => "LEASE_FENCE_EXHAUSTED",
            LeaseError::Store(_) => "LEASE_STORE_ERROR",
        }
    }
}

impl std::fmt::Display for LeaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LeaseError::Store(message) => write!(f, "{}: {message}", self.code()),
            LeaseError::Held {
                holder_machine_id,
                expires_at,
            } => write!(
                f,
                "{}: held by {holder_machine_id} until {expires_at}",
                self.code()
            ),
            _ => f.write_str(self.code()),
        }
    }
}

impl From<String> for LeaseError {
    fn from(message: String) -> Self {
        LeaseError::Store(message)
    }
}

fn sql_error(context: &str) -> impl Fn(rusqlite::Error) -> LeaseError + '_ {
    move |error| LeaseError::Store(format!("{context}: {error}"))
}

/// The relay epoch a fence was minted in.
pub fn fence_epoch(fence: u64) -> u64 {
    fence >> 32
}

fn ensure_lease_tables(conn: &Connection) -> Result<(), LeaseError> {
    for table in [LEASES_TABLE, FENCES_TABLE, EPOCH_TABLE] {
        if !table_exists(conn, table)? {
            return Err(LeaseError::Unsupported);
        }
    }
    Ok(())
}

/// Current relay epoch (0 until the first standby promotion).
pub fn relay_epoch(conn: &Connection) -> Result<u64, LeaseError> {
    ensure_lease_tables(conn)?;
    let epoch: Option<i64> = conn
        .query_row("SELECT MAX(epoch) FROM supervisor_relay_epoch", [], |row| row.get(0))
        .map_err(sql_error("Failed to read relay epoch"))?;
    Ok(epoch.unwrap_or(0) as u64)
}

/// Advances the relay epoch by one and returns the new value. Called by a standby promotion.
pub fn bump_relay_epoch(conn: &Connection) -> Result<u64, LeaseError> {
    let next = relay_epoch(conn)? + 1;
    conn.execute("DELETE FROM supervisor_relay_epoch", [])
        .map_err(sql_error("Failed to clear relay epoch"))?;
    conn.execute(
        "INSERT INTO supervisor_relay_epoch (epoch) VALUES (?1)",
        params![next as i64],
    )
    .map_err(sql_error("Failed to write relay epoch"))?;
    Ok(next)
}

/// The account's lease row, released or not.
pub fn current(conn: &Connection, user_id: &str) -> Result<Option<SupervisorLeaseRecord>, LeaseError> {
    ensure_lease_tables(conn)?;
    conn.query_row(
        &format!("{SUPERVISOR_LEASE_SELECT} WHERE user_id = ?1"),
        params![user_id],
        supervisor_lease_from_row,
    )
    .optional()
    .map_err(sql_error("Failed to read supervisor lease"))
}

fn last_fence(conn: &Connection, user_id: &str) -> Result<u64, LeaseError> {
    let fence: Option<i64> = conn
        .query_row(
            "SELECT last_fence FROM supervisor_lease_fences WHERE user_id = ?1",
            params![user_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql_error("Failed to read last fence"))?;
    Ok(fence.unwrap_or(0) as u64)
}

/// Same lookup as `account::service::enrolled_machine_by_id` plus the owner check, read inside
/// the lease transaction instead of loading the whole store.
fn ensure_enrolled(conn: &Connection, user_id: &str, machine_id: &str) -> Result<(), LeaseError> {
    let count: i64 = conn
        .query_row(
            "SELECT count(*) FROM machines WHERE machine_id = ?1 AND user_id = ?2",
            params![machine_id, user_id],
            |row| row.get(0),
        )
        .map_err(sql_error("Failed to check machine enrollment"))?;
    if count == 0 {
        return Err(LeaseError::NotEnrolled);
    }
    Ok(())
}

/// Next fence for the account in the current epoch: above every fence recorded for it.
fn next_fence(high_water: u64, epoch: u64) -> Result<u64, LeaseError> {
    let counter = match fence_epoch(high_water).cmp(&epoch) {
        std::cmp::Ordering::Equal => high_water & COUNTER_MASK,
        std::cmp::Ordering::Less => 0,
        // A fence from a later epoch than this relay's: granting here would go backwards.
        std::cmp::Ordering::Greater => return Err(LeaseError::FenceExhausted),
    };
    if counter >= COUNTER_MASK || epoch > (i64::MAX as u64) >> 32 {
        return Err(LeaseError::FenceExhausted);
    }
    Ok((epoch << 32) | (counter + 1))
}

fn write_lease(conn: &Connection, lease: &SupervisorLeaseRecord) -> Result<(), LeaseError> {
    conn.execute(
        "INSERT INTO supervisor_leases (user_id, holder_machine_id, fence, expires_at, \
         checkpoint_id, checkpoint_at, released, reserved_for, reserved_fence, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) \
         ON CONFLICT(user_id) DO UPDATE SET \
         holder_machine_id=excluded.holder_machine_id, fence=excluded.fence, \
         expires_at=excluded.expires_at, checkpoint_id=excluded.checkpoint_id, \
         checkpoint_at=excluded.checkpoint_at, released=excluded.released, \
         reserved_for=excluded.reserved_for, reserved_fence=excluded.reserved_fence, \
         updated_at=excluded.updated_at",
        params![
            lease.user_id,
            lease.holder_machine_id,
            lease.fence as i64,
            lease.expires_at as i64,
            lease.checkpoint_id,
            lease.checkpoint_at.map(|v| v as i64),
            i64::from(lease.released),
            lease.reserved_for,
            lease.reserved_fence.map(|v| v as i64),
            lease.updated_at as i64
        ],
    )
    .map_err(sql_error("Failed to write supervisor lease"))?;
    conn.execute(
        "INSERT INTO supervisor_lease_fences (user_id, last_fence) VALUES (?1, ?2) \
         ON CONFLICT(user_id) DO UPDATE SET \
         last_fence = MAX(supervisor_lease_fences.last_fence, excluded.last_fence)",
        params![lease.user_id, lease.fence as i64],
    )
    .map_err(sql_error("Failed to write supervisor lease fence"))?;
    Ok(())
}

/// A held lease for `machine_id` with `fence`, valid `TTL_SECS` from `now`, no reservation.
/// A `checkpoint` of `None` keeps `previous`.
fn granted(
    user_id: &str,
    machine_id: &str,
    fence: u64,
    now: u64,
    checkpoint: Option<&str>,
    previous: Option<&SupervisorLeaseRecord>,
) -> SupervisorLeaseRecord {
    let (checkpoint_id, checkpoint_at) = match checkpoint {
        Some(checkpoint) => (Some(checkpoint.to_string()), Some(now)),
        None => previous.map_or((None, None), |row| (row.checkpoint_id.clone(), row.checkpoint_at)),
    };
    SupervisorLeaseRecord {
        user_id: user_id.to_string(),
        holder_machine_id: machine_id.to_string(),
        fence,
        expires_at: now + TTL_SECS,
        checkpoint_id,
        checkpoint_at,
        released: false,
        reserved_for: None,
        reserved_fence: None,
        updated_at: now,
    }
}

/// Grants (or extends) the account's supervisor lease to `machine_id`.
///
/// Granted iff there is no row; or the row's holder is this machine and its fence is from the
/// current epoch (same fence, extended, un-released); or the row is free (released, past
/// `expires_at + GRACE_SECS`, or from an older epoch) and the checkpoint rule holds: no recorded
/// checkpoint, the candidate matches it, a same-epoch release reserved it for this machine, or
/// `force`. Every other grant mints a new fence above every fence recorded for the account.
pub fn acquire(
    conn: &Connection,
    user_id: &str,
    machine_id: &str,
    now: u64,
    candidate_checkpoint: Option<&str>,
    force: bool,
) -> Result<SupervisorLeaseRecord, LeaseError> {
    let epoch = relay_epoch(conn)?;
    ensure_enrolled(conn, user_id, machine_id)?;
    let existing = current(conn, user_id)?;

    if let Some(row) = &existing {
        if row.holder_machine_id == machine_id && fence_epoch(row.fence) == epoch {
            let lease = granted(user_id, machine_id, row.fence, now, candidate_checkpoint, Some(row));
            write_lease(conn, &lease)?;
            return Ok(lease);
        }

        let free = row.released
            || now >= row.expires_at.saturating_add(GRACE_SECS)
            || fence_epoch(row.fence) < epoch;
        if !free {
            return Err(LeaseError::Held {
                holder_machine_id: row.holder_machine_id.clone(),
                expires_at: row.expires_at,
            });
        }
        let reserved_here = row.released
            && row.reserved_for.as_deref() == Some(machine_id)
            && row.reserved_fence == Some(row.fence)
            && row.reserved_fence.map(fence_epoch) == Some(epoch);
        let checkpoint_ok = row.checkpoint_id.is_none()
            || candidate_checkpoint == row.checkpoint_id.as_deref()
            || reserved_here
            || force;
        if !checkpoint_ok {
            return Err(LeaseError::CheckpointMismatch {
                expected: row.checkpoint_id.clone(),
            });
        }
    }

    let high_water = last_fence(conn, user_id)?.max(existing.as_ref().map_or(0, |row| row.fence));
    let fence = next_fence(high_water, epoch)?;
    let lease = granted(
        user_id,
        machine_id,
        fence,
        now,
        candidate_checkpoint,
        existing.as_ref(),
    );
    write_lease(conn, &lease)?;
    Ok(lease)
}

/// Extends the lease by `TTL_SECS` from `now`. Granted iff holder and fence match, the row is
/// not released and the fence is from the current epoch; within one epoch, expiry alone does
/// not block a renew. A `checkpoint_id` of `None` keeps the recorded checkpoint.
pub fn renew(
    conn: &Connection,
    user_id: &str,
    machine_id: &str,
    fence: u64,
    now: u64,
    checkpoint_id: Option<&str>,
) -> Result<SupervisorLeaseRecord, LeaseError> {
    let epoch = relay_epoch(conn)?;
    ensure_enrolled(conn, user_id, machine_id)?;
    let Some(row) = current(conn, user_id)? else {
        return Err(LeaseError::NotHolder);
    };
    if row.holder_machine_id != machine_id
        || row.fence != fence
        || row.released
        || fence_epoch(fence) != epoch
    {
        return Err(LeaseError::NotHolder);
    }
    let lease = granted(user_id, machine_id, fence, now, checkpoint_id, Some(&row));
    write_lease(conn, &lease)?;
    Ok(lease)
}

/// Marks the lease released when `machine_id` and `fence` match the row; anything else is
/// ignored and returns `Ok(false)`. The row keeps its holder, fence and checkpoint (a
/// `checkpoint_id` of `None` keeps the recorded one) for the checkpoint rule.
pub fn release(
    conn: &Connection,
    user_id: &str,
    machine_id: &str,
    fence: u64,
    now: u64,
    checkpoint_id: Option<&str>,
) -> Result<bool, LeaseError> {
    release_inner(conn, user_id, machine_id, fence, now, checkpoint_id, None)
}

/// [`release`] that also reserves the released fence for `target_machine_id`, which may then
/// acquire without presenting the checkpoint while the relay stays in the same epoch.
pub fn release_to(
    conn: &Connection,
    user_id: &str,
    machine_id: &str,
    fence: u64,
    now: u64,
    checkpoint_id: Option<&str>,
    target_machine_id: &str,
) -> Result<bool, LeaseError> {
    release_inner(
        conn,
        user_id,
        machine_id,
        fence,
        now,
        checkpoint_id,
        Some(target_machine_id),
    )
}

fn release_inner(
    conn: &Connection,
    user_id: &str,
    machine_id: &str,
    fence: u64,
    now: u64,
    checkpoint_id: Option<&str>,
    target_machine_id: Option<&str>,
) -> Result<bool, LeaseError> {
    let Some(row) = current(conn, user_id)? else {
        return Ok(false);
    };
    if row.holder_machine_id != machine_id || row.fence != fence {
        return Ok(false);
    }
    let (checkpoint_id, checkpoint_at) = match checkpoint_id {
        Some(checkpoint) => (Some(checkpoint.to_string()), Some(now)),
        None => (row.checkpoint_id, row.checkpoint_at),
    };
    let lease = SupervisorLeaseRecord {
        user_id: user_id.to_string(),
        holder_machine_id: row.holder_machine_id,
        fence: row.fence,
        expires_at: now,
        checkpoint_id,
        checkpoint_at,
        released: true,
        reserved_for: target_machine_id.map(str::to_string),
        reserved_fence: target_machine_id.map(|_| row.fence),
        updated_at: now,
    };
    write_lease(conn, &lease)?;
    Ok(true)
}

/// Reads the account's lease from the store in `dir`; works on a read-only replica and reports
/// [`LeaseError::Unsupported`] when the replica predates the lease tables.
pub fn current_in(dir: &Path, user_id: &str) -> Result<Option<SupervisorLeaseRecord>, LeaseError> {
    lease_read(dir, |conn| current(conn, user_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::store::store_sqlite::{STANDBY_MARKER_FILENAME, STORE_SQLITE_FILENAME};
    use crate::account::store::{AccountStore, MachineRecord, UserRecord};

    const USER: &str = "u1";
    const A: &str = "mach-a";
    const B: &str = "mach-b";
    const T0: u64 = 1_000_000;

    fn machine(machine_id: &str) -> MachineRecord {
        MachineRecord {
            machine_record_id: format!("rec-{machine_id}"),
            owner_user_id: USER.into(),
            machine_id: machine_id.into(),
            display_name: machine_id.into(),
            public_key: format!("pk-{machine_id}"),
            attach_public_key: format!("apk-{machine_id}"),
            relay_origin: "https://relay.test".into(),
            platform: "linux".into(),
            enrollment_epoch: 1,
            enrolled_at: 1,
            last_seen_at: 0,
        }
    }

    fn seeded() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp");
        let mut store = AccountStore::default();
        store.users.insert(
            USER.into(),
            UserRecord {
                user_id: USER.into(),
                email: "u1@test.local".into(),
                created_at: 1,
            },
        );
        for id in [A, B] {
            let record = machine(id);
            store.machines.insert(record.machine_record_id.clone(), record);
        }
        store.save(dir.path()).expect("seed store");
        dir
    }

    fn acq(
        dir: &Path,
        machine_id: &str,
        now: u64,
        checkpoint: Option<&str>,
        force: bool,
    ) -> Result<SupervisorLeaseRecord, LeaseError> {
        lease_transaction(dir, |tx| acquire(tx, USER, machine_id, now, checkpoint, force))
    }

    fn ren(
        dir: &Path,
        machine_id: &str,
        fence: u64,
        now: u64,
        checkpoint: Option<&str>,
    ) -> Result<SupervisorLeaseRecord, LeaseError> {
        lease_transaction(dir, |tx| renew(tx, USER, machine_id, fence, now, checkpoint))
    }

    fn rel(
        dir: &Path,
        machine_id: &str,
        fence: u64,
        now: u64,
        checkpoint: Option<&str>,
    ) -> Result<bool, LeaseError> {
        lease_transaction(dir, |tx| release(tx, USER, machine_id, fence, now, checkpoint))
    }

    fn rel_to(dir: &Path, machine_id: &str, fence: u64, now: u64, target: &str) -> bool {
        lease_transaction(dir, |tx| {
            release_to(tx, USER, machine_id, fence, now, None, target)
        })
        .expect("release_to")
    }

    fn bump(dir: &Path) -> u64 {
        lease_transaction(dir, |tx| bump_relay_epoch(tx)).expect("bump epoch")
    }

    fn row(dir: &Path) -> SupervisorLeaseRecord {
        current_in(dir, USER).expect("read lease").expect("lease row")
    }

    fn past_grace(lease: &SupervisorLeaseRecord) -> u64 {
        lease.expires_at + GRACE_SECS
    }

    #[test]
    fn first_acquire_gets_fence_one() {
        let dir = seeded();
        let lease = acq(dir.path(), A, T0, None, false).expect("first acquire");
        assert_eq!(lease.fence, 1);
        assert_eq!(lease.expires_at, T0 + TTL_SECS);
        assert_eq!(row(dir.path()), lease);
    }

    #[test]
    fn unenrolled_machine_is_refused() {
        let dir = seeded();
        assert_eq!(
            acq(dir.path(), "mach-unknown", T0, None, false),
            Err(LeaseError::NotEnrolled)
        );
        assert_eq!(current_in(dir.path(), USER), Ok(None));
    }

    #[test]
    fn second_machine_refused_while_live_and_within_grace() {
        let dir = seeded();
        let lease = acq(dir.path(), A, T0, Some("ck1"), false).expect("A acquires");
        assert!(matches!(
            acq(dir.path(), B, T0 + 10, Some("ck1"), false),
            Err(LeaseError::Held { .. })
        ));
        // Expired but still inside the grace window.
        assert!(matches!(
            acq(dir.path(), B, lease.expires_at + GRACE_SECS - 1, Some("ck1"), false),
            Err(LeaseError::Held { .. })
        ));
    }

    #[test]
    fn after_grace_stale_checkpoint_refused_matching_granted_force_granted() {
        let dir = seeded();
        let lease = acq(dir.path(), A, T0, Some("ck1"), false).expect("A acquires");
        let after = past_grace(&lease);
        assert_eq!(
            acq(dir.path(), B, after, Some("ck0"), false),
            Err(LeaseError::CheckpointMismatch {
                expected: Some("ck1".into())
            })
        );
        assert!(matches!(
            acq(dir.path(), B, after, None, false),
            Err(LeaseError::CheckpointMismatch { .. })
        ));
        let taken = acq(dir.path(), B, after, Some("ck1"), false).expect("matching checkpoint");
        assert_eq!(taken.fence, 2);
        assert_eq!(taken.holder_machine_id, B);

        let forced = acq(dir.path(), A, past_grace(&taken), Some("stale"), true).expect("force");
        assert_eq!(forced.fence, 3);
        assert_eq!(forced.checkpoint_id.as_deref(), Some("stale"));
    }

    #[test]
    fn old_fence_renew_refused_after_takeover() {
        let dir = seeded();
        let lease = acq(dir.path(), A, T0, Some("ck1"), false).expect("A acquires");
        let taken = acq(dir.path(), B, past_grace(&lease), Some("ck1"), false).expect("B takes");
        assert_eq!(
            ren(dir.path(), A, lease.fence, past_grace(&lease) + 1, None),
            Err(LeaseError::NotHolder)
        );
        assert_eq!(row(dir.path()), taken);
    }

    #[test]
    fn renew_extends_even_after_expiry_within_epoch_and_keeps_checkpoint() {
        let dir = seeded();
        let lease = acq(dir.path(), A, T0, Some("ck1"), false).expect("A acquires");
        let late = past_grace(&lease) + 100;
        let renewed = ren(dir.path(), A, lease.fence, late, None).expect("renew");
        assert_eq!(renewed.fence, lease.fence);
        assert_eq!(renewed.expires_at, late + TTL_SECS);
        assert_eq!(renewed.checkpoint_id.as_deref(), Some("ck1"));
    }

    #[test]
    fn release_by_non_holder_is_ignored() {
        let dir = seeded();
        let lease = acq(dir.path(), A, T0, Some("ck1"), false).expect("A acquires");
        assert_eq!(rel(dir.path(), B, lease.fence, T0 + 1, None), Ok(false));
        assert_eq!(rel(dir.path(), A, lease.fence + 1, T0 + 1, None), Ok(false));
        assert_eq!(row(dir.path()), lease);
    }

    #[test]
    fn fence_never_decreases_after_release_and_reacquire() {
        let dir = seeded();
        let first = acq(dir.path(), A, T0, None, false).expect("A acquires");
        assert_eq!(rel(dir.path(), A, first.fence, T0 + 5, None), Ok(true));
        let released = row(dir.path());
        assert!(released.released);
        assert_eq!(released.fence, first.fence);
        assert_eq!(released.expires_at, T0 + 5);

        // Same holder re-acquires its own released row: same fence, un-released.
        let again = acq(dir.path(), A, T0 + 6, None, false).expect("A re-acquires");
        assert_eq!(again.fence, first.fence);
        assert!(!again.released);

        assert_eq!(rel(dir.path(), A, again.fence, T0 + 7, None), Ok(true));
        let other = acq(dir.path(), B, T0 + 8, None, false).expect("B acquires released row");
        assert!(other.fence > again.fence);
        assert_eq!(rel(dir.path(), B, other.fence, T0 + 9, None), Ok(true));
        let back = acq(dir.path(), A, T0 + 10, None, false).expect("A acquires again");
        assert!(back.fence > other.fence);
    }

    #[test]
    fn released_row_needs_checkpoint_unless_reserved() {
        let dir = seeded();
        let lease = acq(dir.path(), A, T0, Some("ck1"), false).expect("A acquires");
        assert_eq!(rel(dir.path(), A, lease.fence, T0 + 1, None), Ok(true));
        assert!(matches!(
            acq(dir.path(), B, T0 + 2, None, false),
            Err(LeaseError::CheckpointMismatch { .. })
        ));

        let again = acq(dir.path(), A, T0 + 3, None, false).expect("A re-acquires");
        assert!(rel_to(dir.path(), A, again.fence, T0 + 4, B));
        let reserved = row(dir.path());
        assert_eq!(reserved.reserved_for.as_deref(), Some(B));
        assert_eq!(reserved.reserved_fence, Some(again.fence));
        let granted = acq(dir.path(), B, T0 + 5, None, false).expect("reserved machine");
        assert!(granted.fence > again.fence);
        assert_eq!(granted.reserved_for, None);
        assert_eq!(granted.reserved_fence, None);
        assert_eq!(granted.checkpoint_id.as_deref(), Some("ck1"));
    }

    #[test]
    fn reservation_dropped_when_previous_holder_reacquires_with_newer_checkpoint() {
        let dir = seeded();
        let lease = acq(dir.path(), A, T0, Some("ck1"), false).expect("A acquires");
        assert!(rel_to(dir.path(), A, lease.fence, T0 + 1, B));
        let again = acq(dir.path(), A, T0 + 2, None, false).expect("A re-acquires");
        assert_eq!(again.fence, lease.fence);
        let renewed = ren(dir.path(), A, again.fence, T0 + 3, Some("ck2")).expect("renew ck2");
        assert_eq!(
            acq(dir.path(), B, past_grace(&renewed), Some("ck1"), false),
            Err(LeaseError::CheckpointMismatch {
                expected: Some("ck2".into())
            })
        );
    }

    #[test]
    fn release_without_checkpoint_keeps_stored_checkpoint() {
        let dir = seeded();
        let lease = acq(dir.path(), A, T0, Some("ck1"), false).expect("A acquires");
        assert_eq!(rel(dir.path(), A, lease.fence, T0 + 1, None), Ok(true));
        assert_eq!(row(dir.path()).checkpoint_id.as_deref(), Some("ck1"));
        let again = acq(dir.path(), A, T0 + 2, None, false).expect("A re-acquires");
        assert_eq!(rel(dir.path(), A, again.fence, T0 + 3, Some("ck2")), Ok(true));
        assert_eq!(row(dir.path()).checkpoint_id.as_deref(), Some("ck2"));
    }

    #[test]
    fn same_holder_across_epoch_bump_gets_new_epoch_fence_and_old_fence_cannot_renew() {
        let dir = seeded();
        let lease = acq(dir.path(), A, T0, Some("ck1"), false).expect("A acquires");
        assert_eq!(bump(dir.path()), 1);
        assert_eq!(
            ren(dir.path(), A, lease.fence, T0 + 1, None),
            Err(LeaseError::NotHolder)
        );
        let again = acq(dir.path(), A, T0 + 2, Some("ck1"), false).expect("A re-acquires");
        assert_eq!(again.fence, (1 << 32) | 1);
        assert_eq!(fence_epoch(again.fence), 1);
        let renewed = ren(dir.path(), A, again.fence, T0 + 3, None).expect("new fence renews");
        assert_eq!(renewed.fence, again.fence);
    }

    #[test]
    fn reservation_from_older_epoch_is_not_honored() {
        let dir = seeded();
        let lease = acq(dir.path(), A, T0, Some("ck1"), false).expect("A acquires");
        assert!(rel_to(dir.path(), A, lease.fence, T0 + 1, B));
        bump(dir.path());
        assert!(matches!(
            acq(dir.path(), B, T0 + 2, None, false),
            Err(LeaseError::CheckpointMismatch { .. })
        ));
        let granted = acq(dir.path(), B, T0 + 3, Some("ck1"), false).expect("matching checkpoint");
        assert_eq!(granted.fence, (1 << 32) | 1);
    }

    /// A relay restored from a lagging replica, then promoted (epoch bump), must never grant a
    /// fence at or below any fence the old primary issued, and the restored old-epoch row must
    /// not be renewable once it has expired.
    #[test]
    fn restored_replica_after_epoch_bump_never_reissues_a_used_fence() {
        let primary = seeded();
        let first = acq(primary.path(), A, T0, Some("ck1"), false).expect("A acquires");
        let replica = tempfile::tempdir().expect("replica dir");
        let snapshot = replica.path().join(STORE_SQLITE_FILENAME);
        let snapshot_path = snapshot.to_str().expect("utf8 path").to_string();
        lease_read(primary.path(), |conn| {
            conn.execute("VACUUM INTO ?1", params![snapshot_path])
                .map_err(sql_error("Failed to snapshot primary"))
        })
        .expect("snapshot the lagging replica");

        // The primary keeps issuing fences after the snapshot.
        let mut max_issued = first.fence;
        let mut holder = first.clone();
        for machine_id in [B, A, B] {
            holder = acq(primary.path(), machine_id, past_grace(&holder), Some("ck1"), false)
                .expect("takeover on primary");
            assert!(holder.fence > max_issued);
            max_issued = holder.fence;
        }

        // Promotion of the restored replica bumps the epoch.
        assert_eq!(bump(replica.path()), 1);
        let restored = row(replica.path());
        assert_eq!(restored.fence, first.fence);
        assert_eq!(
            ren(replica.path(), A, restored.fence, past_grace(&restored) + 1, None),
            Err(LeaseError::NotHolder)
        );
        let mut holder = restored;
        for machine_id in [B, A, B] {
            holder = acq(replica.path(), machine_id, past_grace(&holder), Some("ck1"), false)
                .expect("grant on promoted replica");
            assert!(
                holder.fence > max_issued,
                "fence {} must exceed every pre-restore fence {max_issued}",
                holder.fence
            );
        }
    }

    #[test]
    fn read_only_v2_replica_reports_lease_unsupported_and_is_not_migrated() {
        let dir = tempfile::tempdir().expect("temp");
        std::fs::write(dir.path().join(STANDBY_MARKER_FILENAME), b"").expect("marker");
        assert_eq!(current_in(dir.path(), USER), Err(LeaseError::Unsupported));
        assert_eq!(LeaseError::Unsupported.code(), LEASE_UNSUPPORTED);
        let version: u32 = lease_read(dir.path(), |conn| {
            conn.query_row("PRAGMA user_version", [], |version| version.get(0))
                .map_err(sql_error("Failed to read user_version"))
        })
        .expect("version");
        assert_eq!(version, 2);
        let store = AccountStore::load(dir.path()).expect("load v2 replica");
        assert!(store.supervisor_leases.is_empty());
        assert!(matches!(
            acq(dir.path(), A, T0, None, false),
            Err(LeaseError::Store(message)) if message.starts_with("ACCOUNT_STORE_READ_ONLY")
        ));
    }

    #[test]
    fn whole_store_save_does_not_roll_back_a_lease() {
        let dir = seeded();
        let stale_store = AccountStore::load(dir.path()).expect("load before lease");
        let first = acq(dir.path(), A, T0, Some("ck1"), false).expect("A acquires");
        let snapshot = AccountStore::load(dir.path()).expect("load with lease");
        assert_eq!(snapshot.supervisor_leases.get(USER), Some(&first));
        let renewed = ren(dir.path(), A, first.fence, T0 + 20, Some("ck2")).expect("renew");
        snapshot.save(dir.path()).expect("save older snapshot");
        stale_store.save(dir.path()).expect("save store without lease");
        assert_eq!(row(dir.path()), renewed);
    }
}
