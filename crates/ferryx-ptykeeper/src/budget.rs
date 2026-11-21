//! Keeper memory accounting.
//!
//! The keeper's aggregate cap is 768 MiB across every session it owns, with a 640 MiB sub-cap for
//! replay bytes. Both are hard: a charge that does not fit is refused with a typed error the daemon
//! can act on, never granted and never silently truncated.
//!
//! The replay sub-cap is 640 MiB rather than the plan's original 512 MiB because a session's whole
//! replay allocation is charged at spawn (see [`crate::keeper::SESSION_RESERVATION_BYTES`]), which
//! turns the byte cap into a session ceiling of `cap / 8 MiB`: at 512 MiB that ceiling is 64, below
//! the plan's own measured production shape of 71 panes (Phase 1, Test B), so the 71-session
//! footprint soak was refused at spawn 65. 640 MiB admits 80 sessions - the 71 the plan measures
//! with headroom - and still leaves 128 MiB of the 768 MiB aggregate for checkpoints and reply
//! buffers, so neither cap stops bounding anything.
//!
//! Four kinds of memory are charged:
//!
//! * **replay bytes** - retained bytes of the replay logs, charged as they grow and released when a
//!   checkpoint truncates them or an overflow reset discards them. Bounded by
//!   [`REPLAY_AGGREGATE_BYTES`] as well as by the aggregate cap. A session's whole per-session
//!   replay allocation is charged here at spawn, because [`crate::replay::ReplayLog`] allocates its
//!   full capacity up front; see [`crate::keeper::SESSION_RESERVATION_BYTES`].
//! * **reply buffers** - the copy one answer makes out of a replay log or a checkpoint, plus the
//!   base64 text and the serialized frame built from it. Reserved as a [`Charge`] before the copy
//!   exists and released only after the socket write returns.
//! * **in-flight checkpoint chunks** - an upload's declared size is reserved before a byte is
//!   accepted, so an oversized upload is refused rather than discovered too late. Chunk memory is
//!   released on every abort path.
//! * **committed checkpoints** - what the committed recovery point actually holds.
//!
//! Every byte is charged before it is allocated and released on every path that frees it, so a
//! refusal is the only way the keeper says no: it never drops data to stay under the cap.
//!
//! ## One ledger per keeper
//!
//! A [`MemoryBudget`] is the single accounting domain of one keeper process. Sessions, the replay
//! logs, the checkpoint store and every reply charge reach it through the *same*
//! `Arc<Mutex<MemoryBudget>>`, which is why the type is deliberately neither `Clone` nor `Copy`: a
//! value copy would be a second, independent ledger whose `used()` starts at zero, and two ledgers
//! each believe their own charges fit under the cap. Share the `Arc` instead, as
//! [`crate::keeper::Keeper`] does.

use std::fmt;
use std::sync::{Arc, Mutex};

use crate::error::{KeeperError, KeeperErrorCode};

/// Aggregate keeper memory cap: 768 MiB across all sessions.
pub const BUDGET_AGGREGATE_BYTES: usize = 768 * 1024 * 1024;

/// Aggregate replay cap: 640 MiB across all session replay logs.
///
/// Sized so the plan's 71-session shape fits: reservations are charged at the full 8 MiB per
/// session up front, so this cap is also a session ceiling of 80 (`640 MiB / 8 MiB`). The plan's
/// original 512 MiB admitted only 64, one pane short of the 71 it asks the soak to run.
pub const REPLAY_AGGREGATE_BYTES: usize = 640 * 1024 * 1024;

/// Why a charge was refused. Each variant maps to the wire code the daemon already understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetRefusal {
    /// The aggregate 768 MiB cap has no room for the charge.
    Memory { requested: u64, available: u64 },
    /// The 640 MiB replay sub-cap has no room for the charge.
    Replay { requested: u64, available: u64 },
    /// A checkpoint allocation does not fit; the previously committed checkpoint is preserved.
    Checkpoint { requested: u64, available: u64 },
}

impl BudgetRefusal {
    pub fn code(self) -> KeeperErrorCode {
        match self {
            Self::Memory { .. } => KeeperErrorCode::MemoryBudgetExhausted,
            Self::Replay { .. } => KeeperErrorCode::ReplayBudgetExhausted,
            Self::Checkpoint { .. } => KeeperErrorCode::CheckpointBudgetExhausted,
        }
    }

    /// The requested and available byte counts, for a log line that names the numbers.
    pub fn figures(self) -> (u64, u64) {
        match self {
            Self::Memory { requested, available }
            | Self::Replay { requested, available }
            | Self::Checkpoint { requested, available } => (requested, available),
        }
    }

    /// The typed error to return on the wire. `subject` names the session or upload so the daemon
    /// can correlate the refusal without a second round trip.
    pub fn as_error(self, subject: &str) -> KeeperError {
        let (requested, available) = self.figures();
        let what = match self {
            Self::Memory { .. } => "keeper memory budget",
            Self::Replay { .. } => "keeper replay budget",
            Self::Checkpoint { .. } => "keeper checkpoint budget",
        };
        KeeperError::new(
            self.code(),
            format!("{subject}: {what} refused {requested} bytes, {available} available"),
        )
    }
}

impl fmt::Display for BudgetRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (requested, available) = self.figures();
        let what = match self {
            Self::Memory { .. } => "aggregate memory",
            Self::Replay { .. } => "replay",
            Self::Checkpoint { .. } => "checkpoint",
        };
        write!(f, "{what} budget refused {requested} bytes ({available} available)")
    }
}

impl std::error::Error for BudgetRefusal {}

/// Typed accounting for every byte the keeper is allowed to hold.
///
/// Deliberately not `Clone`: see the module note "One ledger per keeper". A keeper that genuinely
/// needed a second accounting domain would need a second `Arc<Mutex<MemoryBudget>>` with caps
/// chosen explicitly, not a copy of this one's counters.
#[derive(Debug)]
pub struct MemoryBudget {
    total_bytes: u64,
    replay_limit: u64,
    reserved_bytes: u64,
    replay_bytes: u64,
    checkpoint_pending: u64,
    checkpoint_committed: u64,
}

impl Default for MemoryBudget {
    fn default() -> Self {
        Self::new(BUDGET_AGGREGATE_BYTES as u64)
    }
}

impl MemoryBudget {
    /// A budget of `total_bytes`, with the replay sub-cap at its plan value clamped to it.
    pub fn new(total_bytes: u64) -> Self {
        Self::with_limits(total_bytes as usize, REPLAY_AGGREGATE_BYTES)
    }

    /// A budget with explicit aggregate and replay caps.
    pub fn with_limits(total_bytes: usize, replay_bytes: usize) -> Self {
        let total = total_bytes as u64;
        Self {
            total_bytes: total,
            // A replay sub-cap above the aggregate cap is not a usable configuration: it would only
            // ever be reached after the aggregate cap refused. Clamp instead of panicking, so a bad
            // config cannot take the keeper down at startup.
            replay_limit: (replay_bytes as u64).min(total),
            reserved_bytes: 0,
            replay_bytes: 0,
            checkpoint_pending: 0,
            checkpoint_committed: 0,
        }
    }

    pub fn total(&self) -> u64 {
        self.total_bytes
    }

    /// Everything currently charged against the aggregate cap.
    pub fn used(&self) -> u64 {
        self.reserved_bytes
            .saturating_add(self.replay_bytes)
            .saturating_add(self.checkpoint_pending)
            .saturating_add(self.checkpoint_committed)
    }

    /// Same as [`MemoryBudget::used`], under the name checkpoint accounting uses.
    pub fn charged_bytes(&self) -> usize {
        self.used() as usize
    }

    pub fn available(&self) -> u64 {
        self.total_bytes.saturating_sub(self.used())
    }

    pub fn replay_limit(&self) -> u64 {
        self.replay_limit
    }

    /// Room left under the replay sub-cap, independently of the aggregate cap.
    pub fn replay_available(&self) -> u64 {
        self.replay_limit.saturating_sub(self.replay_bytes)
    }

    pub fn reserved_bytes(&self) -> u64 {
        self.reserved_bytes
    }

    pub fn replay_bytes(&self) -> u64 {
        self.replay_bytes
    }

    /// Bytes held by chunked checkpoint uploads that have neither committed nor aborted.
    pub fn checkpoint_pending_bytes(&self) -> usize {
        self.checkpoint_pending as usize
    }

    /// Bytes held by committed checkpoints.
    pub fn checkpoint_committed_bytes(&self) -> usize {
        self.checkpoint_committed as usize
    }

    /// Reserves an arbitrary charge against the aggregate cap. `false` means it does not fit; the
    /// caller turns that into its own typed refusal so the message names what was being done.
    pub fn try_reserve(&mut self, bytes: u64) -> bool {
        if bytes > self.available() {
            return false;
        }
        self.reserved_bytes = self.reserved_bytes.saturating_add(bytes);
        true
    }

    /// Releases a reservation taken by [`MemoryBudget::try_reserve`]. Saturates, so a double release
    /// cannot underflow the accounting into a permissive state.
    pub fn release(&mut self, bytes: u64) {
        self.reserved_bytes = self.reserved_bytes.saturating_sub(bytes);
    }

    /// Charges replay retention. Both caps must fit: the replay sub-cap and the aggregate cap, so a
    /// replay growth cannot push the keeper past its aggregate limit.
    pub fn try_reserve_replay(&mut self, bytes: u64) -> Result<(), BudgetRefusal> {
        let replay_available = self.replay_available();
        if bytes > replay_available {
            return Err(BudgetRefusal::Replay { requested: bytes, available: replay_available });
        }
        let available = self.available();
        if bytes > available {
            return Err(BudgetRefusal::Memory { requested: bytes, available });
        }
        self.replay_bytes = self.replay_bytes.saturating_add(bytes);
        Ok(())
    }

    /// Releases replay retention, on truncation at a checkpoint offset or on an overflow reset.
    pub fn release_replay(&mut self, bytes: u64) {
        self.replay_bytes = self.replay_bytes.saturating_sub(bytes);
    }

    /// Charges the declared size of a chunked checkpoint upload before a byte is accepted.
    pub fn try_reserve_checkpoint_chunk(&mut self, bytes: usize) -> Result<(), BudgetRefusal> {
        let requested = bytes as u64;
        let available = self.available();
        if requested > available {
            return Err(BudgetRefusal::Checkpoint { requested, available });
        }
        self.checkpoint_pending = self.checkpoint_pending.saturating_add(requested);
        Ok(())
    }

    /// Releases an in-flight chunk allocation that will never commit. This is the abort path and
    /// must run on every one of them, including a refused later chunk.
    pub fn abort_checkpoint_chunk(&mut self, bytes: usize) {
        self.checkpoint_pending = self.checkpoint_pending.saturating_sub(bytes as u64);
    }

    /// Converts an in-flight chunk allocation into a committed checkpoint allocation. The bytes stay
    /// charged; only the bucket changes.
    ///
    /// Fails closed: the caller names the bytes it is committing, and that figure is what the
    /// committed bucket records. Clamping the request down to whatever happened to be pending would
    /// keep memory the store really holds out of the ledger, which is exactly how a hard cap gets
    /// breached. If the pending bucket was somehow short, the committed side still counts every
    /// byte and the pending side floors at zero, so the ledger can only over-state, never under.
    pub fn commit_checkpoint_chunk(&mut self, bytes: usize) {
        let bytes = bytes as u64;
        self.checkpoint_pending = self.checkpoint_pending.saturating_sub(bytes);
        self.checkpoint_committed = self.checkpoint_committed.saturating_add(bytes);
    }

    /// Releases a committed checkpoint's bytes, when a newer one replaces it or the session dies.
    pub fn release_checkpoint_committed(&mut self, bytes: usize) {
        self.checkpoint_committed = self.checkpoint_committed.saturating_sub(bytes as u64);
    }
}

/// A live reservation against the aggregate cap, released when it is dropped.
///
/// Owned rather than borrowed from a `&Keeper`, because the memory it covers outlives the call that
/// charged for it: a reply's copy and its encoded frames are still allocated while the connection
/// writes them, and a guard that released at the end of the request handler would call those bytes
/// free while they are still held. Every holder drops it after the last byte that charge covers.
#[derive(Debug)]
pub struct Charge {
    budget: Arc<Mutex<MemoryBudget>>,
    bytes: u64,
}

impl Charge {
    /// A charge of nothing, ready to grow. Holds the budget it will be billed to.
    pub fn empty(budget: Arc<Mutex<MemoryBudget>>) -> Self {
        Self { budget, bytes: 0 }
    }

    /// Reserves `bytes` in one step, or `None` when they do not fit. Nothing is charged on refusal.
    pub fn try_reserve(budget: Arc<Mutex<MemoryBudget>>, bytes: u64) -> Option<Self> {
        let mut charge = Self::empty(budget);
        charge.grow(bytes).then_some(charge)
    }

    /// Adds `bytes` to this charge. `false` means they did not fit and nothing was added.
    pub fn grow(&mut self, bytes: u64) -> bool {
        if bytes == 0 {
            return true;
        }
        if !self.lock().try_reserve(bytes) {
            return false;
        }
        self.bytes = self.bytes.saturating_add(bytes);
        true
    }

    /// Returns part of the charge early, for a bound that turned out to be an over-estimate.
    pub fn release(&mut self, bytes: u64) {
        let bytes = bytes.min(self.bytes);
        if bytes == 0 {
            return;
        }
        self.lock().release(bytes);
        self.bytes -= bytes;
    }

    /// Takes over another charge against the same budget, so one guard can cover both allocations.
    pub fn absorb(&mut self, other: Self) {
        debug_assert!(
            Arc::ptr_eq(&self.budget, &other.budget),
            "two charges against different budgets cannot be merged"
        );
        let bytes = other.bytes;
        let mut other = other;
        other.bytes = 0;
        self.bytes = self.bytes.saturating_add(bytes);
    }

    /// Bytes this guard currently holds charged.
    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, MemoryBudget> {
        self.budget
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Drop for Charge {
    fn drop(&mut self) {
        if self.bytes > 0 {
            self.lock().release(self.bytes);
            self.bytes = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_the_plan_caps() {
        let budget = MemoryBudget::new(BUDGET_AGGREGATE_BYTES as u64);
        assert_eq!(budget.total(), BUDGET_AGGREGATE_BYTES as u64);
        assert_eq!(budget.replay_limit(), REPLAY_AGGREGATE_BYTES as u64);
        assert_eq!(budget.available(), budget.total());
        assert_eq!(MemoryBudget::default().total(), BUDGET_AGGREGATE_BYTES as u64);
    }

    #[test]
    fn aggregate_replay_cap_refuses_growth_past_the_sub_cap() {
        let mut budget = MemoryBudget::with_limits(BUDGET_AGGREGATE_BYTES, 16);
        budget.try_reserve_replay(12).expect("12 of 16 fits");
        assert_eq!(budget.replay_bytes(), 12);

        let refusal = budget.try_reserve_replay(8).expect_err("the replay sub-cap must refuse");
        assert_eq!(refusal, BudgetRefusal::Replay { requested: 8, available: 4 });
        assert_eq!(refusal.code(), KeeperErrorCode::ReplayBudgetExhausted);
        assert_eq!(budget.replay_bytes(), 12, "a refusal never partially charges");

        budget.release_replay(12);
        budget.try_reserve_replay(16).expect("the sub-cap reopens after a release");
    }

    #[test]
    fn replay_growth_cannot_borrow_from_the_aggregate_headroom_alone() {
        // Aggregate 64 with 16 already reserved leaves 48; the replay sub-cap alone would have
        // allowed 56, so the aggregate cap is what refuses.
        let mut budget = MemoryBudget::with_limits(64, 64);
        assert!(budget.try_reserve(16), "session reservation fits");
        let refusal = budget.try_reserve_replay(56).expect_err("the aggregate cap must refuse");
        assert_eq!(refusal, BudgetRefusal::Memory { requested: 56, available: 48 });
        assert_eq!(refusal.code(), KeeperErrorCode::MemoryBudgetExhausted);
        assert_eq!(budget.replay_bytes(), 0);
    }

    #[test]
    fn a_chunk_allocation_is_refused_when_the_aggregate_cap_is_full() {
        let mut budget = MemoryBudget::with_limits(1024, 512);
        budget.try_reserve_checkpoint_chunk(768).expect("the first chunk fits");
        let refusal = budget
            .try_reserve_checkpoint_chunk(512)
            .expect_err("a chunk beyond the cap must be refused");
        assert_eq!(refusal, BudgetRefusal::Checkpoint { requested: 512, available: 256 });
        assert_eq!(refusal.code(), KeeperErrorCode::CheckpointBudgetExhausted);
        assert_eq!(budget.checkpoint_pending_bytes(), 768);
        assert_eq!(budget.checkpoint_committed_bytes(), 0);
        assert!(refusal.as_error("checkpoint cp1").message.contains("checkpoint budget"));
    }

    #[test]
    fn an_aborted_chunk_allocation_returns_its_bytes_to_the_cap() {
        let mut budget = MemoryBudget::with_limits(1024, 512);
        budget.try_reserve_checkpoint_chunk(1000).expect("fits");
        assert_eq!(budget.available(), 24);
        budget.abort_checkpoint_chunk(1000);
        assert_eq!(budget.checkpoint_pending_bytes(), 0);
        assert_eq!(budget.available(), 1024);
        budget.try_reserve_checkpoint_chunk(1000).expect("capacity is available again");
    }

    #[test]
    fn a_committed_chunk_stays_charged_and_session_release_returns_the_rest() {
        let mut budget = MemoryBudget::with_limits(1024, 512);
        assert!(budget.try_reserve(64), "session reservation");
        budget.try_reserve_checkpoint_chunk(128).expect("chunk fits");
        budget.commit_checkpoint_chunk(128);
        assert_eq!(budget.checkpoint_pending_bytes(), 0);
        assert_eq!(budget.checkpoint_committed_bytes(), 128);
        assert_eq!(budget.used(), 64 + 128);

        budget.release_checkpoint_committed(128);
        budget.release(64);
        assert_eq!(budget.used(), 0);
        budget.release(64);
        assert_eq!(budget.used(), 0, "a double release cannot go negative");
    }

    #[test]
    fn a_session_reservation_is_refused_past_the_aggregate_cap() {
        let mut budget = MemoryBudget::with_limits(128, 128);
        assert!(budget.try_reserve(64));
        assert!(budget.try_reserve(64));
        assert!(!budget.try_reserve(64), "a third reservation cannot fit");
        assert_eq!(budget.reserved_bytes(), 128);
    }

    #[test]
    fn a_commit_larger_than_the_pending_bucket_still_charges_every_byte() {
        // The upload and the ledger can only disagree through a bug, and the refusal to trust the
        // smaller figure is the point: silently clamping here would leave committed bytes the store
        // really holds outside the cap it is supposed to respect.
        let mut budget = MemoryBudget::with_limits(4096, 512);
        budget.try_reserve_checkpoint_chunk(128).expect("chunk fits");
        budget.commit_checkpoint_chunk(256);
        assert_eq!(budget.checkpoint_pending_bytes(), 0, "the bucket floors at zero");
        assert_eq!(
            budget.checkpoint_committed_bytes(),
            256,
            "every committed byte is charged, not just what the pending bucket held"
        );
        assert_eq!(budget.used(), 256);
    }

    #[test]
    fn a_charge_returns_its_bytes_on_drop_and_can_be_grown_first() {
        let budget = Arc::new(Mutex::new(MemoryBudget::with_limits(1024, 512)));
        {
            let charge = Charge::try_reserve(Arc::clone(&budget), 256).expect("fits");
            assert_eq!(charge.bytes(), 256);
            assert_eq!(budget.lock().expect("budget").available(), 768);
        }
        assert_eq!(
            budget.lock().expect("budget").available(),
            1024,
            "dropping the guard returns exactly what it held"
        );

        let mut charge = Charge::empty(Arc::clone(&budget));
        assert!(charge.grow(512));
        assert!(!charge.grow(1024), "a charge that does not fit grows by nothing");
        assert_eq!(charge.bytes(), 512, "the refused growth charged nothing");
        charge.release(256);
        assert_eq!(charge.bytes(), 256);
        assert_eq!(budget.lock().expect("budget").available(), 768);
    }

    #[test]
    fn an_absorbed_charge_releases_once_under_the_surviving_guard() {
        let budget = Arc::new(Mutex::new(MemoryBudget::with_limits(1024, 512)));
        let mut outer = Charge::empty(Arc::clone(&budget));
        assert!(outer.grow(128));
        let inner = Charge::try_reserve(Arc::clone(&budget), 384).expect("fits");
        outer.absorb(inner);
        assert_eq!(outer.bytes(), 512);
        assert_eq!(budget.lock().expect("budget").available(), 512);
        drop(outer);
        assert_eq!(
            budget.lock().expect("budget").available(),
            1024,
            "the absorbed bytes were not released by the guard that was merged away"
        );
    }
}
