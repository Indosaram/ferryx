//! The keeper's operation surface: the session registry and every verb that touches a PTY.
//!
//! Ownership model:
//!
//! * [`Keeper`] owns [`Session`] entries through `Arc`, and each `Session` owns one
//!   `portable_pty` master, its writer, its child killer and its [`ReplayLog`]. The keeper is
//!   therefore the only process holding the master descriptors: a daemon crash cannot take the
//!   child processes with it.
//! * Every mutating verb goes through [`EpochFence`] (so a stalled old daemon cannot read or write
//!   after its successor attached) and every allocation goes through [`MemoryBudget`] (so refusals
//!   are typed instead of turning into an OOM kill that would take the sessions with it).
//! * Locks nest only in one fixed order: the spawn admissions, then a session's own `ownership`
//!   gate, then the retired-id floors, then the epoch fence, then the registry, then the
//!   checkpoint store, then a session's own mutexes (its replay log last). The
//!   admissions mutex is held only to check, record or clear one id, never across a PTY open, and
//!   no other method takes it, so that order cannot be reversed. Every other guard in that list is
//!   taken as a temporary that is released before the next one is asked for. A blocking PTY read or
//!   write therefore cannot stall another connection's verb.
//! * Request-id bookkeeping is one table, not two: [`RequestTombstones`] is the only structure
//!   keyed by request id and it is bounded, so no code path can remember a request id past
//!   `RETIRED_REQUEST_IDS`. That is why a tombstone miss reads as "unknown", never "no fork".
//! * A session's `ownership` gate is the one deliberate nesting inside a verb: the epoch check and
//!   the effect it authorises happen under it, so a successor's attach cannot fence between the
//!   check and the write, resize, signal, checkpoint mutation, replay copy or retirement that check
//!   was performed for. Every epoch advance and every use of an epoch therefore serialises on it.
//! * `retired` is always taken before `epochs`. A spawn reads an id's floor and fences above it
//!   under one hold, and a retirement records the floor and forgets the fence under one hold, so
//!   neither can observe the other's half. That is what stops a reused id from restarting at an
//!   epoch its predecessor already handed out.
//!
//! The keeper never parses terminal output. Bytes read from the master go into the replay log
//! unchanged and come back out unchanged.

use std::collections::{HashMap, HashSet, VecDeque};
// Both are needed: `Write` is named bare by `Session::writer`, and `Read` supplies `.read()` to the
// pump's `Box<dyn std::io::Read + Send>`. Spelled `_`-less on purpose, so neither reads as unused.
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{SystemTime, UNIX_EPOCH};

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};

use crate::budget::{BudgetRefusal, MemoryBudget};
use crate::checkpoint::{CheckpointInfo, CheckpointStore};
use crate::epoch::EpochFence;
use crate::error::{KeeperError, KeeperErrorCode, KeeperResult};
use crate::replay::{ReplayGap, ReplayLog};
use crate::session::SessionState;

/// Memory charged to the aggregate budget for one live session. The plan's per-session replay cap
/// is 8 MiB, so reserving the cap up front makes the refusal threshold predictable instead of
/// letting the keeper discover exhaustion after it has already accepted the session.
///
/// Because the whole cap is charged at spawn, the aggregate replay sub-cap is also a session
/// ceiling of `REPLAY_AGGREGATE_BYTES / 8 MiB` = 80 with the current 640 MiB, against
/// `768 MiB / 8 MiB = 96` for the aggregate. That ordering is deliberate: the replay sub-cap binds
/// first, and it is sized above the plan's 71-pane production shape (Phase 1, Test B), which a
/// 512 MiB sub-cap would have refused at pane 65. Sixty sessions (480 MiB of replay) is the plan's
/// acceptance figure and leaves room for checkpoints; it is not the ceiling. Checkpoint bytes spend
/// only the aggregate, so a keeper holding checkpoints reaches the aggregate cap sooner than 96
/// sessions and the aggregate refusal is what it reports.
pub const SESSION_RESERVATION_BYTES: u64 = 8 * 1024 * 1024;

/// Most raw bytes one reply copies out of the replay log.
///
/// A session retains 8 MiB, but a single answer never materialises that: the page is what the
/// reply buffer and its base64 encoding are charged for, so a reply's footprint is a constant
/// regardless of how much history the log holds.
pub const REPLY_PAGE_BYTES: usize = 512 * 1024;

/// Charged per reply frame on top of its base64 payload, covering the JSON envelope a frame
/// carries (kind, request id, session id, offsets). An over-estimate is safe: the ledger may only
/// count a byte twice, never miss one the frame really holds.
pub const REPLY_FRAME_OVERHEAD_BYTES: u64 = 512;

/// The base64 length of `bytes`, which is what a reply holds as text for them.
pub fn base64_len(bytes: usize) -> u64 {
    (bytes as u64).div_ceil(3) * 4
}

/// Largest single `WRITE` payload. Kept well below the 1 MiB frame cap so one keystroke burst
/// cannot occupy a PTY write for long.
pub const MAX_WRITE_BYTES: usize = 64 * 1024;

/// Buffer for one read from a session master.
const PUMP_BUFFER_BYTES: usize = 32 * 1024;

/// Signals a `SIGNAL` frame may name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionSignal {
    Interrupt,
    Terminate,
    Kill,
    Stop,
    Continue,
}

impl SessionSignal {
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "interrupt" => Self::Interrupt,
            "terminate" => Self::Terminate,
            "kill" => Self::Kill,
            "stop" => Self::Stop,
            "continue" => Self::Continue,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Interrupt => "interrupt",
            Self::Terminate => "terminate",
            Self::Kill => "kill",
            Self::Stop => "stop",
            Self::Continue => "continue",
        }
    }
}

/// One `SPAWN` request, already parsed off the wire.
#[derive(Debug, Clone)]
pub struct SpawnRequest {
    pub session_id: String,
    pub request_id: String,
    pub argv: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: Option<PathBuf>,
    pub cols: u16,
    pub rows: u16,
    pub epoch: u64,
}

/// A session's identity as the daemon sees it. `pgid` and `process_start_time` exist so a daemon
/// can prove it reattached to the *same* process rather than to a fresh shell with a reused pid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnInfo {
    pub session_id: String,
    pub pid: Option<u32>,
    pub pgid: Option<i32>,
    pub process_start_time: Option<u64>,
    pub cols: u16,
    pub rows: u16,
    pub epoch: u64,
    pub sequence_offset: u64,
}

/// Result of an `ATTACH`: the checkpoint the daemon should restore, plus whatever replay suffix is
/// still retained. A gap means the retained suffix was discarded: the daemon resets its parser
/// before consuming anything that follows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachOutcome {
    pub session_id: String,
    pub epoch: u64,
    pub checkpoint: CheckpointInfo,
    pub sequence_offset: u64,
    pub data: Vec<u8>,
    pub replay_gap: Option<ReplayGap>,
    /// Whether the retained suffix continues past `data`, so the reader must page for the rest.
    pub more: bool,
}

/// Result of a `REPLAY` verb.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayOutcome {
    pub sequence_offset: u64,
    pub data: Vec<u8>,
    pub replay_gap: Option<ReplayGap>,
    /// Whether the retained suffix continues past `data`.
    pub more: bool,
}

/// One bounded page out of a session's replay log, plus whether more remains.
struct ReplayPage {
    head: u64,
    data: Vec<u8>,
    more: bool,
    gap: Option<ReplayGap>,
}

/// One live session. Everything that must block lives behind its own lock so the registry guard is
/// never held across I/O.
pub struct Session {
    session_id: String,
    generation: String,
    pid: Option<u32>,
    pgid: Option<i32>,
    process_start_time: Option<u64>,
    cols: AtomicU16,
    rows: AtomicU16,
    /// The master is `Send` but not `Sync`, and a `Session` is shared across connection threads
    /// through an `Arc`, so the master is wrapped: every use takes the lock for the length of one
    /// verb. Nothing holds it across a read, because reads happen on the pump thread from a
    /// cloned reader.
    master: Mutex<Box<dyn MasterPty + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    /// The replay log is shared with this session's pump thread, which appends every byte the
    /// master produces: one `Arc`, two holders, so the pump never reaches into the session.
    replay: Arc<Mutex<ReplayLog>>,
    pump: Mutex<Option<JoinHandle<()>>>,
    /// The handle that ends a blocking read, behind a mutex so a teardown can take a clone of it
    /// without holding the slot it lives in.
    interrupt: Mutex<Option<Arc<dyn portable_pty::ReaderInterrupt>>>,
    /// The budget this session's reservation is charged against, and how much it was charged. Both
    /// live on the session so the release follows the memory's real lifetime: the replay log and the
    /// child handles stay alive while any clone of this `Arc` does, which can outlast the registry
    /// entry the spawn installed.
    budget: Arc<Mutex<MemoryBudget>>,
    reserved_bytes: u64,
    /// Milliseconds of the last write, resize or attach, read from the keeper's [`Clock`]. Reporting
    /// only: no decision in the keeper reads it, so it needs no lock.
    last_activity_millis: AtomicU64,
    /// The source this session's activity stamps come from. Held per session so a test's clock
    /// reaches the session it created, without any lock around the clock itself.
    clock: Clock,
    /// Serialises "this epoch may act" with the act itself. Taken before the epoch fence and before
    /// the session's own I/O mutexes, never while the registry guard is held.
    ownership: Mutex<()>,
}

impl Session {
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn generation(&self) -> &str {
        &self.generation
    }

    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    pub fn pgid(&self) -> Option<i32> {
        self.pgid
    }

    pub fn process_start_time(&self) -> Option<u64> {
        self.process_start_time
    }

    pub fn size(&self) -> (u16, u16) {
        (self.cols.load(Ordering::Relaxed), self.rows.load(Ordering::Relaxed))
    }

    pub fn sequence_offset(&self) -> u64 {
        lock(&self.replay).head()
    }

    /// The registry snapshot for `OWNERSHIP_QUERY`/`OWNERSHIP_LIST`. The three replay figures are
    /// read under the log's guard so they describe one instant; `epoch` comes from the caller
    /// because the fence belongs to the keeper; `focused` is always `false` because no verb in this
    /// protocol reports focus yet.
    pub fn snapshot(&self, epoch: u64) -> SessionState {
        let (cols, rows) = self.size();
        let (head, retained, resets) = {
            let replay = lock(&self.replay);
            (replay.head(), replay.retained_bytes() as u64, replay.reset_count())
        };
        SessionState::new(self.session_id.clone(), self.generation.clone(), self.pid, cols, rows)
            .with_epoch(epoch)
            .with_replay(head, retained, resets)
            .with_activity(self.last_activity_millis(), false)
    }

    fn touch(&self) {
        self.last_activity_millis.store(self.clock.millis(), Ordering::Relaxed);
    }

    fn last_activity_millis(&self) -> u64 {
        self.last_activity_millis.load(Ordering::Relaxed)
    }

    fn info(&self, epoch: u64) -> SpawnInfo {
        let (cols, rows) = self.size();
        SpawnInfo {
            session_id: self.session_id.clone(),
            pid: self.pid,
            pgid: self.pgid,
            process_start_time: self.process_start_time,
            cols,
            rows,
            epoch,
            sequence_offset: self.sequence_offset(),
        }
    }

    /// Copies at most `limit` bytes of the suffix after `after`.
    ///
    /// The allocation this makes is bounded by `limit`, not by what the log retains, so a reply can
    /// never turn into a copy of the whole 8 MiB window.
    fn replay_page(&self, after: u64, limit: usize) -> ReplayPage {
        // `head` and `read_into_bounded` both take `&self`, so the guard needs no `mut`.
        let replay = lock(&self.replay);
        let head = replay.head();
        let mut data = Vec::new();
        match replay.read_into_bounded(after, limit, &mut data) {
            Ok((_copied, more)) => ReplayPage { head, data, more, gap: None },
            Err(gap) => ReplayPage { head, data: Vec::new(), more: false, gap: Some(gap) },
        }
    }

    /// How many bytes a read from `after` would return, without copying any. `None` when the range
    /// is a gap, so a caller can refuse before it allocates anything.
    fn covered_len(&self, after: u64) -> Option<usize> {
        lock(&self.replay).covered_len(after)
    }

    fn write(&self, data: &[u8]) -> std::io::Result<usize> {
        let mut writer = lock(&self.writer);
        writer.write_all(data)?;
        writer.flush()?;
        self.touch();
        Ok(data.len())
    }

    fn resize(&self, cols: u16, rows: u16) -> KeeperResult<()> {
        lock(&self.master)
            .resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .map_err(|error| {
                KeeperError::new(
                    KeeperErrorCode::InvalidSpawn,
                    format!("cannot resize session {}: {error}", self.session_id),
                )
            })?;
        self.cols.store(cols, Ordering::Relaxed);
        self.rows.store(rows, Ordering::Relaxed);
        self.touch();
        Ok(())
    }

    #[cfg(unix)]
    fn signal(&self, signal: SessionSignal) -> KeeperResult<()> {
        let Some(pid) = self.pid else {
            return Err(KeeperError::new(
                KeeperErrorCode::InvalidSpawn,
                format!("session {} has no pid to signal", self.session_id),
            ));
        };
        // The pid is a cached number: the child may have exited and the kernel may have handed the
        // same pid to an unrelated process. `(pid, start time)` is the identity that cannot be
        // recycled, so a mismatch means this session has no live child to signal.
        if !still_owns_process(pid, self.process_start_time) {
            return Err(KeeperError::new(
                KeeperErrorCode::InvalidSpawn,
                format!(
                    "session {} no longer owns pid {pid}: that pid now names another process",
                    self.session_id
                ),
            ));
        }
        let number = match signal {
            SessionSignal::Interrupt => libc::SIGINT,
            SessionSignal::Terminate => libc::SIGTERM,
            SessionSignal::Kill => libc::SIGKILL,
            SessionSignal::Stop => libc::SIGSTOP,
            SessionSignal::Continue => libc::SIGCONT,
        };
        if unsafe { libc::kill(pid as i32, number) } != 0 {
            let error = std::io::Error::last_os_error();
            // A child that already exited is not a keeper failure: the session is simply done.
            if error.raw_os_error() == Some(libc::ESRCH) {
                return Ok(());
            }
            return Err(KeeperError::new(
                KeeperErrorCode::InvalidSpawn,
                format!("cannot send {} to session {}: {error}", signal.as_str(), self.session_id),
            ));
        }
        Ok(())
    }

    #[cfg(not(unix))]
    fn signal(&self, signal: SessionSignal) -> KeeperResult<()> {
        if signal == SessionSignal::Kill || signal == SessionSignal::Terminate {
            let mut killer = lock(&self.killer);
            killer.kill().map_err(|error| {
                KeeperError::new(
                    KeeperErrorCode::InvalidSpawn,
                    format!("cannot kill session {}: {error}", self.session_id),
                )
            })?;
            return Ok(());
        }
        Err(KeeperError::new(
            KeeperErrorCode::InvalidSpawn,
            format!(
                "signal {} is not supported on this platform; session {} is unchanged",
                signal.as_str(),
                self.session_id
            ),
        ))
    }

    /// Ends the read in flight, so the pump thread can be joined instead of leaked.
    fn interrupt_reader(&self) {
        // Cloned out of the slot before it is used: a blocking request must never be issued while
        // the lock that guards the slot is held.
        let interrupt = lock(&self.interrupt).clone();
        if let Some(interrupt) = interrupt {
            interrupt.request();
        }
    }

    /// Wraps the session's interrupt so a test can count how many requests a verb issued without
    /// ending the real pump. The handle the master handed out is still the one that is called.
    #[cfg(test)]
    fn watch_interrupts(&self, requests: Arc<std::sync::atomic::AtomicU32>) {
        let inner = lock(&self.interrupt).clone();
        *lock(&self.interrupt) = Some(Arc::new(WatchedInterrupt { inner, requests }));
    }

    fn join_pump(&self) {
        let handle = lock(&self.pump).take();
        if let Some(handle) = handle {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
struct WatchedInterrupt {
    inner: Option<Arc<dyn portable_pty::ReaderInterrupt>>,
    requests: Arc<std::sync::atomic::AtomicU32>,
}

#[cfg(test)]
impl portable_pty::ReaderInterrupt for WatchedInterrupt {
    fn request(&self) {
        self.requests.fetch_add(1, Ordering::SeqCst);
        if let Some(inner) = &self.inner {
            inner.request();
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // A session that leaves the registry must not leave a child behind, and must not leave the
        // pump thread blocked on a master nobody reads any more.
        self.interrupt_reader();
        if let Ok(killer) = self.killer.get_mut() {
            let _ = killer.kill();
        }
        if let Ok(pump) = self.pump.get_mut() {
            if let Some(handle) = pump.take() {
                let _ = handle.join();
            }
        }
        // The reservation is charged for this session's memory, not for its registry entry. Until
        // the last `Arc` is gone, a reader thread can still be cloning replay output out of this
        // session, so the bytes are still in use and must stay charged. It is released from the
        // same bucket it was taken from, or the sub-cap would leak by one session per kill.
        if self.reserved_bytes > 0 {
            lock(&self.budget).release_replay(self.reserved_bytes);
        }
    }
}

/// The keeper. Cheap to share: every method takes `&self` and synchronises internally.
pub struct Keeper {
    generation: String,
    /// The aggregate budget, still shared with the checkpoint store. It is a `Mutex` rather than a
    /// plain field because every charge mutates it and the keeper's methods all take `&self`.
    budget: Arc<Mutex<MemoryBudget>>,
    checkpoints: Mutex<CheckpointStore>,
    epochs: Mutex<EpochFence>,
    sessions: Mutex<HashMap<String, Arc<Session>>>,
    /// Session ids a spawn is currently creating. A second `SPAWN` for the same id waits on
    /// [`Keeper::admission_done`] instead of racing the first to the PTY.
    admissions: Mutex<HashSet<String>>,
    admission_done: Condvar,
    /// The newest epoch each retired session id was attached at. `EpochFence::forget` drops the
    /// entry when a session is killed, so this floor is what stops a reused id from restarting at
    /// epoch 1 and letting a stale operation from the previous incarnation validate again.
    retired: Mutex<RetiredEpochs>,
    /// The request ids this keeper has already forked for, with the session each named. This is the
    /// *only* table keyed by request id: an entry outlives its session, so a delayed duplicate of a
    /// spawn whose session was killed is refused instead of forking a second child, and there is no
    /// second, unbounded map that could hold the same ids past the bound.
    retired_requests: Mutex<RequestTombstones>,
    /// Where session activity timestamps come from. Injected so a test never depends on the wall
    /// clock; production passes [`now_millis`].
    clock: Clock,
    shutting_down: AtomicBool,
}

/// Retired-id epoch floors: one entry per recently retired session, plus a global floor that keeps
/// the ABA guarantee once entries are pruned for bounded memory.
#[derive(Debug, Default)]
struct RetiredEpochs {
    per_id: HashMap<String, u64>,
    floor: u64,
}

/// How many retired ids keep an individual floor before the map is collapsed into the global one.
const RETIRED_EPOCH_IDS: usize = 256;

impl RetiredEpochs {
    fn record(&mut self, session_id: &str, newest: u64) {
        self.floor = self.floor.max(newest);
        self.per_id.insert(session_id.to_owned(), newest);
        if self.per_id.len() > RETIRED_EPOCH_IDS {
            self.per_id.clear();
        }
    }

    fn next_epoch(&self, session_id: &str, requested: u64) -> u64 {
        let id_floor = self.per_id.get(session_id).copied().unwrap_or(0);
        requested
            .max(self.floor.saturating_add(1))
            .max(id_floor.saturating_add(1))
    }
}

/// How many forked request ids one keeper remembers: bounded, oldest-evicted, per keeper life.
///
/// The table is neither unbounded nor durable. It retains the most recent `RETIRED_REQUEST_IDS`
/// retired ids and drops the oldest beyond that, and the whole table goes away when the keeper
/// process exits. So:
///
/// * a **hit** is proof - that request id already forked a session, so a second fork under it is
///   refused;
/// * a **miss** proves nothing - the id may never have been seen, or it may have been evicted, and
///   either way a duplicate under it can still fork;
/// * after a keeper restart even a hit from the previous process cannot be recalled.
///
/// No caller may present a miss as evidence that a fork cannot happen. The only sound reading is the
/// positive one: refuse on a hit, keep charging on a miss.
const RETIRED_REQUEST_IDS: usize = 8192;

/// The request ids this keeper has already forked for, with the session each named.
///
/// Deliberately not `Default`: the derived value would carry a zero capacity and silently remember
/// nothing, which is the one failure mode this table must not have. Construct it with
/// [`RequestTombstones::new`].
#[derive(Debug)]
struct RequestTombstones {
    order: VecDeque<String>,
    sessions: HashMap<String, String>,
    /// The bound this table evicts at. A field rather than the constant so a test can drive eviction
    /// with a handful of entries instead of 8193; production always gets `RETIRED_REQUEST_IDS`.
    capacity: usize,
}

impl RequestTombstones {
    /// A table with the production bound, `RETIRED_REQUEST_IDS`.
    fn new() -> Self {
        Self::with_capacity(RETIRED_REQUEST_IDS)
    }

    /// The same table with an explicit bound. Only a test has a reason to ask for a small one.
    fn with_capacity(capacity: usize) -> Self {
        Self { order: VecDeque::new(), sessions: HashMap::new(), capacity }
    }

    /// Records that `request_id` forked `session_id`, evicting the oldest remembered id once the
    /// bound is reached. That eviction is exactly why [`Self::forked_session`]'s `None` carries no
    /// meaning.
    fn remember(&mut self, request_id: &str, session_id: &str) {
        if self.sessions.insert(request_id.to_owned(), session_id.to_owned()).is_none() {
            self.order.push_back(request_id.to_owned());
        }
        while self.order.len() > self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.sessions.remove(&oldest);
            }
        }
    }

    /// The session `request_id` was forked for, while this keeper still remembers it.
    ///
    /// A **positive oracle only**: `Some` means a fork has already happened under that id, so a
    /// second one must be refused. `None` means "unknown" - never "no fork".
    fn forked_session(&self, request_id: &str) -> Option<&str> {
        self.sessions.get(request_id).map(String::as_str)
    }
}

impl Keeper {
    pub fn new(generation: impl Into<String>, budget: Arc<Mutex<MemoryBudget>>) -> Self {
        Self::with_clock(generation, budget, default_clock())
    }

    /// A keeper whose session activity stamps come from `clock`. Only a test has a reason to pass
    /// one: production must report the wall clock, which is what a discovery reader expects.
    pub fn with_clock(
        generation: impl Into<String>,
        budget: Arc<Mutex<MemoryBudget>>,
        clock: Clock,
    ) -> Self {
        let checkpoints = CheckpointStore::new(Arc::clone(&budget));
        Self {
            generation: generation.into(),
            budget,
            checkpoints: Mutex::new(checkpoints),
            epochs: Mutex::new(EpochFence::new()),
            sessions: Mutex::new(HashMap::new()),
            retired_requests: Mutex::new(RequestTombstones::new()),
            admissions: Mutex::new(HashSet::new()),
            admission_done: Condvar::new(),
            retired: Mutex::new(RetiredEpochs::default()),
            clock,
            shutting_down: AtomicBool::new(false),
        }
    }

    /// A keeper with the default aggregate budget.
    pub fn with_default_budget(generation: impl Into<String>) -> Self {
        Self::new(
            generation,
            Arc::new(Mutex::new(MemoryBudget::new(crate::budget::BUDGET_AGGREGATE_BYTES as u64))),
        )
    }

    pub fn generation(&self) -> &str {
        &self.generation
    }

    pub fn budget(&self) -> MutexGuard<'_, MemoryBudget> {
        lock(&self.budget)
    }

    /// A handle to the aggregate budget for a [`crate::budget::Charge`], so a reservation can be
    /// owned by whoever keeps the memory alive rather than by the call that made it.
    pub fn budget_handle(&self) -> Arc<Mutex<MemoryBudget>> {
        Arc::clone(&self.budget)
    }

    pub fn is_shutting_down(&self) -> bool {
        self.shutting_down.load(Ordering::Relaxed)
    }

    pub fn session_count(&self) -> usize {
        lock(&self.sessions).len()
    }

    pub fn contains(&self, session_id: &str) -> bool {
        lock(&self.sessions).contains_key(session_id)
    }

    /// How many live sessions this keeper's install generation owns. Feeds install-dir retention:
    /// a generation that answers anything but 0 is never deleted.
    pub fn sessions_for_generation(&self, generation: &str) -> usize {
        lock(&self.sessions)
            .values()
            .filter(|session| session.generation() == generation)
            .count()
    }

    /// Spawns a PTY-backed child.
    ///
    /// Duplicate suppression has two tiers, and only one of them is a guarantee:
    ///
    /// 1. **Live session id** - a `SPAWN` naming a session this keeper still holds answers from that
    ///    session and forks nothing. Strong while the session lives, gone the moment it is killed.
    /// 2. **Retired request id** - a `SPAWN` whose `requestId` this keeper remembers forking is
    ///    refused with `INVALID_SPAWN` and a `SPAWN_REQUEST_RETIRED:` prefix. Bounded
    ///    ([`RETIRED_REQUEST_IDS`]) and in-memory, so it is a positive oracle only: a hit refuses, a
    ///    miss proves nothing and may still fork. See [`RequestTombstones`].
    ///
    /// A retry is therefore idempotent only while one of those two holds. There is no reply this
    /// verb can give that proves a *given absent* request id will never fork, so no caller may read
    /// "no tombstone" as such a proof.
    ///
    /// A refusal from either tier happens before any budget reservation and before any PTY exists, so
    /// a refused spawn charges nothing.
    ///
    /// Admission is exclusive per session id. Without that, two concurrent spawns for one id both
    /// miss the registry, both open a child, and the loser's child is leaked when its insert
    /// overwrites the winner. A second spawn therefore waits for the first and answers from the
    /// session it installed rather than opening a PTY of its own.
    pub fn spawn(&self, request: SpawnRequest) -> KeeperResult<SpawnInfo> {
        if self.is_shutting_down() {
            return Err(KeeperError::new(KeeperErrorCode::ShuttingDown, "keeper is shutting down"));
        }
        request.validate()?;

        loop {
            let mut admissions = lock(&self.admissions);
            if let Some(info) = self.existing_spawn(&request)? {
                return Ok(info);
            }
            if !admissions.contains(&request.session_id) {
                admissions.insert(request.session_id.clone());
                break;
            }
            // Another thread is creating exactly this id. It removes the admission and notifies on
            // both outcomes, so this thread either answers from its session or retries the whole
            // spawn; it never races it to a second PTY.
            let waiting = self
                .admission_done
                .wait(admissions)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            drop(waiting);
            if self.is_shutting_down() {
                return Err(KeeperError::new(
                    KeeperErrorCode::ShuttingDown,
                    "keeper is shutting down",
                ));
            }
        }

        let _admission = SpawnAdmission { keeper: self, session_id: &request.session_id };
        self.create_session(&request)
    }

    /// The answer for a spawn naming a session this keeper already owns, or `None` when a child has
    /// to be created. Called with the spawn gate held, so the registry cannot change under the
    /// answer.
    ///
    /// Two tiers, and the difference matters to a caller deciding what to do with a refusal:
    ///
    /// * the session is **live**: the answer is that session, and - when this call carried a request
    ///   id - that id is recorded in the retired-request table, so a duplicate arriving after the
    ///   session is gone is still refused. A retry that mints its own request id is recorded here
    ///   too, which is why a retried spawn followed by a kill cannot reopen the session under the
    ///   retry's id.
    /// * the session is **gone but the request id is remembered**: `SPAWN_REQUEST_RETIRED`, never a
    ///   second child.
    ///
    /// Neither tier is a proof about an id this keeper does not remember: the table is bounded and
    /// per-process (see [`RETIRED_REQUEST_IDS`]). A `None` from here means "no child for it right
    /// now", not "no child ever".
    fn existing_spawn(&self, request: &SpawnRequest) -> KeeperResult<Option<SpawnInfo>> {
        if !request.request_id.is_empty() {
            // The tombstone is the only request-id table, and a hit names the session that id
            // forked. A live hit answers from that session; a hit whose session is gone is the
            // retired tier below. There is no separate map to fall through, which is what makes
            // the bound in `RETIRED_REQUEST_IDS` the whole story about what this keeper remembers.
            let forked = lock(&self.retired_requests)
                .forked_session(&request.request_id)
                .map(str::to_owned);
            if let Some(forked) = forked {
                if let Ok(session) = self.session(&forked) {
                    let epoch = lock(&self.epochs).newest(&forked).unwrap_or(request.epoch);
                    return Ok(Some(session.info(epoch)));
                }
                return Err(KeeperError::new(
                    KeeperErrorCode::InvalidSpawn,
                    format!(
                        "SPAWN_REQUEST_RETIRED: request {} already forked session {forked}, which this keeper has since retired; retrying the same logical spawn must reuse this request id, and a new logical spawn must mint a new one - minting a new id here would fork a second child",
                        request.request_id
                    ),
                ));
            }
        }
        if !self.contains(&request.session_id) {
            return Ok(None);
        }
        // Same id, different request: the pid and start time already recorded are the truth, and
        // this request id must be retired with the session it named. A client is free to reach an
        // existing session under a fresh request id, so retiring only the id the session was created
        // under would leave that id outside the tombstone: once the session is killed and its
        // tombstone entry is all that is left, a duplicate of it forks again.
        if !request.request_id.is_empty() {
            lock(&self.retired_requests)
                .remember(&request.request_id, &request.session_id);
        }
        let epoch = lock(&self.epochs).newest(&request.session_id).unwrap_or(request.epoch);
        Ok(self.session(&request.session_id).ok().map(|session| session.info(epoch)))
    }

    /// Reserves the budget, opens the child, fences the epoch and commits the session. Every failure
    /// after the child exists tears that child down and returns its reservation, so a refused spawn
    /// leaves no process, no pump thread and no charged byte behind.
    fn create_session(&self, request: &SpawnRequest) -> KeeperResult<SpawnInfo> {
        // One guard for the whole decision, and no `lock(&self.budget)` inside the refusal: the
        // numbers reported and the reserve call come from the same observation, and a second guard
        // taken while this one is alive is a self-deadlock, formatting included.
        //
        // The reservation is charged to the *replay* bucket, not the generic one. It exists to cover
        // the session's `ReplayLog`, which `ReplayLog::new` allocates at its full 8 MiB up front, so
        // this is replay memory by construction. Charging it generically would leave `replay_bytes`
        // at zero for the keeper's whole life and the replay sub-cap would never refuse anything:
        // the aggregate 768 MiB cap alone would admit 96 sessions, and replay could claim all of it.
        // Charging it here makes both caps bind with no byte counted twice.
        let refusal = {
            let mut budget = lock(&self.budget);
            match budget.try_reserve_replay(SESSION_RESERVATION_BYTES) {
                Ok(()) => None,
                Err(refusal) => Some((refusal, budget.used(), budget.total())),
            }
        };
        if let Some((refusal, used, total)) = refusal {
            let (requested, available) = refusal.figures();
            let cap = match refusal {
                BudgetRefusal::Replay { .. } => "replay",
                _ => "aggregate",
            };
            // The code is the refusal's own, not one code plus a word in the message. A caller must
            // be able to tell "the replay sub-cap refused this spawn" from "the 768 MiB aggregate
            // cap refused it" without parsing prose, which is exactly what
            // `REPLAY_BUDGET_EXHAUSTED` and `MEMORY_BUDGET_EXHAUSTED` are for.
            return Err(KeeperError::new(
                refusal.code(),
                format!(
                    "spawning {} needs {SESSION_RESERVATION_BYTES} bytes of keeper {cap} budget for its replay log; the cap refused {requested} bytes with {available} available, and {used} of {total} bytes are in use",
                    request.session_id
                ),
            ));
        }

        let session = match self.open_session(request) {
            Ok(session) => session,
            Err(error) => {
                lock(&self.budget).release_replay(SESSION_RESERVATION_BYTES);
                return Err(error);
            }
        };

        // From here the child exists, so a fence refusal must take it down rather than drop the
        // handle: dropping a child does not terminate it.
        let epoch = match self.fence_spawn(&request.session_id, request.epoch, &session) {
            Ok(epoch) => epoch,
            Err(error) => {
                self.abandon_spawn(session);
                return Err(error);
            }
        };
        self.commit_spawn(request, session, epoch)
    }

    /// Fences a fresh session at an epoch above whatever a previous session with the same id reached,
    /// and publishes it, as one step.
    ///
    /// Ids are chosen by the daemon and the fence forgets a retired id, so without the floor a reused
    /// id would restart at epoch 1 and an operation captured from the retired incarnation would
    /// validate again (ABA). The floor, the fence and the registry insert are all taken under
    /// `retired`, which is also what [`Keeper::retire`] holds: a concurrent retirement either has
    /// already recorded a floor this spawn must clear, or has not yet forgotten the fence it must
    /// beat, and neither can interleave with a half-published session.
    fn fence_spawn(
        &self,
        session_id: &str,
        requested: u64,
        session: &Arc<Session>,
    ) -> KeeperResult<u64> {
        let retired = lock(&self.retired);
        let floor = retired.next_epoch(session_id, requested);
        let epoch = lock(&self.epochs).fence(session_id, floor)?;
        lock(&self.sessions).insert(session_id.to_owned(), Arc::clone(session));
        Ok(epoch)
    }

    /// Records a retired id's epoch floor and forgets its fence as one step, so no spawn can read the
    /// floor before it is written yet fence after the old fence is gone.
    fn forget_epoch(&self, session_id: &str) {
        let mut retired = lock(&self.retired);
        let mut epochs = lock(&self.epochs);
        let newest = epochs.newest(session_id);
        epochs.forget(session_id);
        if let Some(newest) = newest {
            retired.record(session_id, newest);
        }
    }

    /// Removes a session from the registry only while that entry is still `session`.
    ///
    /// A plain remove would let a retiring incarnation evict the session its successor already
    /// published under the same id, which turns a stale `KILL` into a live session nobody owns.
    fn remove_if_current(&self, session_id: &str, session: &Arc<Session>) -> bool {
        let mut sessions = lock(&self.sessions);
        match sessions.get(session_id) {
            Some(current) if Arc::ptr_eq(current, session) => {
                sessions.remove(session_id);
                true
            }
            _ => false,
        }
    }

    /// Publishes a child that `create_session` already opened, and takes it back down if a shutdown
    /// began while the child was being created.
    fn commit_spawn(
        &self,
        request: &SpawnRequest,
        session: Arc<Session>,
        epoch: u64,
    ) -> KeeperResult<SpawnInfo> {
        let info = session.info(epoch);
        if !request.request_id.is_empty() {
            lock(&self.retired_requests)
                .remember(&request.request_id, &request.session_id);
        }
        // `shutdown` sets its flag and then snapshots the registry. A spawn already past its first
        // check would insert after that snapshot and survive the shutdown, so the commit re-checks
        // and undoes itself instead of leaving a child the shutdown never saw.
        if self.is_shutting_down() {
            if self.remove_if_current(&request.session_id, &session) {
                self.forget_epoch(&request.session_id);
                self.abandon_spawn(session);
            }
            return Err(KeeperError::new(KeeperErrorCode::ShuttingDown, "keeper is shutting down"));
        }
        Ok(info)
    }

    /// Tears down a session that was opened but never committed to the registry: the child is
    /// killed, the pump ends, and the reservation returns when the last `Arc` drops.
    fn abandon_spawn(&self, session: Arc<Session>) {
        session.interrupt_reader();
        if let Ok(mut killer) = session.killer.lock() {
            let _ = killer.kill();
        }
        session.join_pump();
        drop(session);
    }

    fn open_session(&self, request: &SpawnRequest) -> KeeperResult<Arc<Session>> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: request.rows,
                cols: request.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| {
                KeeperError::new(
                    KeeperErrorCode::InvalidSpawn,
                    format!("cannot open a pty for {}: {error}", request.session_id),
                )
            })?;

        let mut command = CommandBuilder::from_argv(
            request.argv.iter().map(std::ffi::OsString::from).collect(),
        );
        for (key, value) in &request.env {
            command.env(key, value);
        }
        if let Some(cwd) = &request.cwd {
            command.cwd(cwd);
        }

        let mut child = pair.slave.spawn_command(command).map_err(|error| {
            KeeperError::new(
                KeeperErrorCode::InvalidSpawn,
                format!("cannot spawn {}: {error}", request.argv.join(" ")),
            )
        })?;
        // The slave is dropped immediately: the child holds its own copy, and keeping ours open
        // would keep the PTY alive after the child exits.
        drop(pair.slave);

        // The child exists from here, so a handle that cannot be set up has to end it: dropping the
        // child handle closes our reference, it does not terminate the process.
        let reader = match pair.master.try_clone_reader() {
            Ok(reader) => reader,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(KeeperError::new(
                    KeeperErrorCode::InvalidSpawn,
                    format!("cannot clone the reader for {}: {error}", request.session_id),
                ));
            }
        };
        let writer = match pair.master.take_writer() {
            Ok(writer) => writer,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(KeeperError::new(
                    KeeperErrorCode::InvalidSpawn,
                    format!("cannot take the writer for {}: {error}", request.session_id),
                ));
            }
        };
        let pid = child.process_id();
        #[cfg(unix)]
        let pgid = pair.master.process_group_leader();
        #[cfg(not(unix))]
        let pgid: Option<i32> = None;
        let process_start_time = pid.and_then(process_start_time);
        let killer = child.clone_killer();
        let interrupt = pair.master.interrupt_handle();

        let replay = Arc::new(Mutex::new(ReplayLog::new(
            request.session_id.clone(),
            crate::replay::REPLAY_SESSION_BYTES,
        )));
        let pump = spawn_pump(request.session_id.clone(), reader, Arc::clone(&replay));

        Ok(Arc::new(Session {
            session_id: request.session_id.clone(),
            generation: self.generation.clone(),
            pid,
            pgid,
            process_start_time,
            cols: AtomicU16::new(request.cols),
            rows: AtomicU16::new(request.rows),
            master: Mutex::new(pair.master),
            writer: Mutex::new(writer),
            killer: Mutex::new(killer),
            replay,
            pump: Mutex::new(Some(pump)),
            interrupt: Mutex::new(interrupt),
            budget: Arc::clone(&self.budget),
            reserved_bytes: SESSION_RESERVATION_BYTES,
            last_activity_millis: AtomicU64::new(self.clock.millis()),
            clock: self.clock.clone(),
            ownership: Mutex::new(()),
        }))
    }

    /// Writes input to a session. The epoch is verified first: an old daemon must not be able to
    /// type into a session its successor now owns.
    pub fn write(&self, session_id: &str, epoch: u64, data: &[u8]) -> KeeperResult<usize> {
        if data.len() > MAX_WRITE_BYTES {
            return Err(KeeperError::new(
                KeeperErrorCode::FrameTooLarge,
                format!("write of {} bytes exceeds the {MAX_WRITE_BYTES} byte cap", data.len()),
            ));
        }
        let session = self.session(session_id)?;
        let _ownership = lock(&session.ownership);
        lock(&self.epochs).verify(session_id, epoch)?;
        session.write(data).map_err(|error| {
            KeeperError::new(
                KeeperErrorCode::UnknownSession,
                format!("cannot write to session {session_id}: {error}"),
            )
        })
    }

    pub fn resize(&self, session_id: &str, epoch: u64, cols: u16, rows: u16) -> KeeperResult<()> {
        if cols == 0 || rows == 0 {
            return Err(KeeperError::new(
                KeeperErrorCode::InvalidSpawn,
                format!("resize to {cols}x{rows} is not a usable size"),
            ));
        }
        let session = self.session(session_id)?;
        let _ownership = lock(&session.ownership);
        lock(&self.epochs).verify(session_id, epoch)?;
        session.resize(cols, rows)
    }

    pub fn signal(&self, session_id: &str, epoch: u64, signal: SessionSignal) -> KeeperResult<()> {
        let session = self.session(session_id)?;
        let _ownership = lock(&session.ownership);
        lock(&self.epochs).verify(session_id, epoch)?;
        session.signal(signal)
    }

    /// Attaches: fences the epoch, reports the committed checkpoint and the retained replay suffix.
    pub fn attach(&self, session_id: &str, epoch: u64, after: u64) -> KeeperResult<AttachOutcome> {
        self.attach_page(session_id, epoch, after, REPLY_PAGE_BYTES)
    }

    /// `ATTACH` with an explicit page bound, so a caller that wants a smaller inline answer does not
    /// pay for a page it will not use.
    pub fn attach_page(
        &self,
        session_id: &str,
        epoch: u64,
        after: u64,
        limit: usize,
    ) -> KeeperResult<AttachOutcome> {
        let session = self.session(session_id)?;
        // The gate covers only the fence, so a successor attaching waits for an in-flight write to
        // land and no longer for the replay copy below.
        let epoch = {
            let _ownership = lock(&session.ownership);
            lock(&self.epochs).fence(session_id, epoch)?
        };
        session.touch();
        // Checkpoints then replay, under the store guard, exactly as the commit path takes them: a
        // committer that has trimmed the replay has also already stored its checkpoint, so this
        // reader can never see the checkpoint beside bytes that checkpoint already covers.
        let (checkpoint, page) = {
            let checkpoints = lock(&self.checkpoints);
            let checkpoint = checkpoints.stored(session_id);
            let page = session.replay_page(after, limit);
            (checkpoint, page)
        };
        Ok(AttachOutcome {
            session_id: session_id.to_owned(),
            epoch,
            checkpoint,
            sequence_offset: page.head,
            data: page.data,
            replay_gap: page.gap,
            more: page.more,
        })
    }

    /// Releases an attachment without touching the PTY. The newest epoch stays fenced, so the
    /// detaching daemon cannot come back with an older one.
    ///
    /// Gated by `ownership` like every other epoch use, even though it mutates nothing: taking the
    /// same gate is what lets a reader trust that a `DETACH` answered `Ok` was verified against an
    /// epoch no concurrent advance had already replaced. A verb that skipped the gate could only ever
    /// be *more* permissive than the ones it sits beside, which is the one direction a fence must not
    /// vary.
    pub fn detach(&self, session_id: &str, epoch: u64) -> KeeperResult<()> {
        let session = self.session(session_id)?;
        let _ownership = lock(&session.ownership);
        lock(&self.epochs).verify(session_id, epoch)?;
        Ok(())
    }

    /// How many bytes a replay from `after` would copy right now, without copying them. `None` when
    /// the range has already fallen out of the log, which is the caller's chance to refuse before
    /// spending budget on a page it cannot deliver. A length probe only: it never copies session
    /// output, so it is the one replay read that is not gated by the epoch.
    pub fn page_len(&self, session_id: &str, after: u64) -> KeeperResult<Option<usize>> {
        Ok(self.session(session_id)?.covered_len(after))
    }

    /// An unfenced `REPLAY` page, for the legacy wire form that carries no epoch.
    ///
    /// Kept because the shipped daemon sends `REPLAY` without an epoch and a keeper that refused it
    /// would strand every attached session. A caller that holds an epoch uses
    /// [`Keeper::replay_fenced`], which cannot read a successor's output.
    pub fn replay(&self, session_id: &str, from: u64) -> KeeperResult<ReplayOutcome> {
        self.replay_page(session_id, from, REPLY_PAGE_BYTES)
    }

    /// An unfenced `REPLAY` page bounded by `limit`, for the live output tap: the tap only needs the
    /// next chunk and must not pay for a whole page to get it.
    pub fn replay_bounded(
        &self,
        session_id: &str,
        from: u64,
        limit: usize,
    ) -> KeeperResult<ReplayOutcome> {
        self.replay_page(session_id, from, limit)
    }

    /// The unfenced `HEAD`, for the legacy wire form that carries no epoch.
    pub fn head(&self, session_id: &str) -> KeeperResult<u64> {
        Ok(self.session(session_id)?.sequence_offset())
    }

    fn replay_page(&self, session_id: &str, from: u64, limit: usize) -> KeeperResult<ReplayOutcome> {
        let session = self.session(session_id)?;
        let page = session.replay_page(from, limit);
        Ok(ReplayOutcome {
            sequence_offset: page.head,
            data: page.data,
            replay_gap: page.gap,
            more: page.more,
        })
    }

    /// A `REPLAY` page the caller's epoch still owns.
    pub fn replay_fenced(
        &self,
        session_id: &str,
        from: u64,
        epoch: u64,
    ) -> KeeperResult<ReplayOutcome> {
        self.replay_fenced_bounded(session_id, from, epoch, REPLY_PAGE_BYTES)
    }

    /// `REPLAY` with an explicit page bound, serialised with every epoch advance.
    ///
    /// The copy is held under the session's `ownership` gate rather than under the global epoch
    /// fence: `attach` fences under the same gate, so no epoch can advance while these bytes are
    /// copied, and a page-sized copy does not stall every other session's verbs on the one lock that
    /// every advance needs.
    pub fn replay_fenced_bounded(
        &self,
        session_id: &str,
        from: u64,
        epoch: u64,
        limit: usize,
    ) -> KeeperResult<ReplayOutcome> {
        let session = self.session(session_id)?;
        let _ownership = lock(&session.ownership);
        lock(&self.epochs).verify(session_id, epoch)?;
        let page = session.replay_page(from, limit);
        Ok(ReplayOutcome {
            sequence_offset: page.head,
            data: page.data,
            replay_gap: page.gap,
            more: page.more,
        })
    }

    /// `HEAD`, fenced the same way as [`Keeper::replay_fenced`].
    pub fn head_fenced(&self, session_id: &str, epoch: u64) -> KeeperResult<u64> {
        let session = self.session(session_id)?;
        let _ownership = lock(&session.ownership);
        lock(&self.epochs).verify(session_id, epoch)?;
        Ok(session.sequence_offset())
    }

    /// Kills the child and forgets the session, without an epoch check. Retirement paths that hold no
    /// epoch of their own use this; a peer's `KILL` uses [`Keeper::kill_fenced`].
    pub fn kill(&self, session_id: &str) -> KeeperResult<()> {
        self.retire(session_id, None)
    }

    /// `KILL`, serialised with every epoch advance and with every other use of the session.
    ///
    /// Returns once the pump thread is joined, so a caller that observed a successful `KILL` never
    /// races the reader. A caller whose epoch is no longer the newest gets `EpochMismatch` and
    /// nothing is touched, so a stalled predecessor cannot retire the session its successor now
    /// owns.
    pub fn kill_fenced(&self, session_id: &str, epoch: u64) -> KeeperResult<()> {
        self.retire(session_id, Some(epoch))
    }

    fn retire(&self, session_id: &str, epoch: Option<u64>) -> KeeperResult<()> {
        let session = self.session(session_id)?;
        {
            // The same gate a write, resize, signal, checkpoint mutation or attach holds, so
            // retirement cannot land between one of those verbs being authorised and it reaching the
            // PTY, and an attach cannot advance the epoch between this verify and the forget.
            let _ownership = lock(&session.ownership);
            // `retired` is the outer guard here, matching `fence_spawn`: the floor, the fence and the
            // registry entry are one step, so a spawn cannot interleave with a half-retired session.
            {
                let mut retired = lock(&self.retired);
                let mut epochs = lock(&self.epochs);
                if let Some(epoch) = epoch {
                    epochs.verify(session_id, epoch)?;
                }
                // The registry is the authority on which incarnation owns the id, so the entry is
                // removed before anything is recorded or forgotten. A retirement overtaken by a
                // successor must not record the successor's epoch as retired or forget its fence:
                // that leaves a live session no verb can address (verify answers from an absent
                // fence) and no respawn can re-fence below the floor just raised.
                {
                    let mut sessions = lock(&self.sessions);
                    match sessions.get(session_id) {
                        Some(current) if Arc::ptr_eq(current, &session) => {
                            sessions.remove(session_id);
                        }
                        _ => {
                            return Err(KeeperError::new(
                                KeeperErrorCode::UnknownSession,
                                format!(
                                    "session {session_id} was replaced before it could be retired"
                                ),
                            ));
                        }
                    }
                }
                // Recorded in the same critical section that forgets the fence: the daemon may reuse
                // the id, and a fresh incarnation must not start below the epoch this one reached.
                if let Some(newest) = epochs.newest(session_id) {
                    retired.record(session_id, newest);
                }
                epochs.forget(session_id);
            }
            lock(&self.checkpoints).forget_session(session_id);
        }
        session.interrupt_reader();
        if let Ok(mut killer) = session.killer.lock() {
            let _ = killer.kill();
        }
        session.join_pump();
        // The session's own `Drop` returns its reservation, and only once the last `Arc` clone is
        // gone: a clone still reading replay bytes out of this session is still using the memory.
        drop(session);
        Ok(())
    }

    /// Installs a checkpoint that arrived complete in one frame.
    pub fn install_checkpoint(
        &self,
        session_id: &str,
        checkpoint_id: &str,
        sequence_offset: u64,
        total_bytes: u64,
        bytes: Vec<u8>,
        epoch: u64,
    ) -> KeeperResult<CheckpointInfo> {
        self.with_fenced_checkpoints(session_id, epoch, |checkpoints| {
            let info =
                checkpoints.install(session_id, checkpoint_id, sequence_offset, total_bytes, bytes)?;
            let covered = info.sequence_offset;
            Ok((info, Some(covered)))
        })
    }

    /// Opens a chunked checkpoint upload. The full declared size is charged to the budget before a
    /// single byte is accepted.
    pub fn begin_checkpoint(
        &self,
        session_id: &str,
        checkpoint_id: &str,
        sequence_offset: u64,
        total_bytes: u64,
        epoch: u64,
    ) -> KeeperResult<crate::checkpoint::CheckpointProgress> {
        self.with_fenced_checkpoints(session_id, epoch, |checkpoints| {
            Ok((checkpoints.begin(session_id, checkpoint_id, sequence_offset, total_bytes)?, None))
        })
    }

    pub fn checkpoint_chunk(
        &self,
        session_id: &str,
        checkpoint_id: &str,
        chunk_index: u32,
        data: &[u8],
        epoch: u64,
    ) -> KeeperResult<crate::checkpoint::CheckpointProgress> {
        self.with_fenced_checkpoints(session_id, epoch, |checkpoints| {
            Ok((checkpoints.push_chunk(session_id, checkpoint_id, chunk_index, data)?, None))
        })
    }

    pub fn commit_checkpoint(
        &self,
        session_id: &str,
        checkpoint_id: &str,
        sequence_offset: u64,
        epoch: u64,
    ) -> KeeperResult<CheckpointInfo> {
        self.with_fenced_checkpoints(session_id, epoch, |checkpoints| {
            let info = checkpoints.commit(session_id, checkpoint_id, sequence_offset)?;
            let covered = info.sequence_offset;
            Ok((info, Some(covered)))
        })
    }

    /// Abandons a chunked upload. Idempotent: a commit that already landed is left alone.
    ///
    /// Fenced by its originating epoch: this runs on the connection-teardown path, where the id may
    /// already name a successor's upload. A fence that no longer matches reports `false` and touches
    /// nothing, which is the fail-closed answer for a caller that cannot report an error.
    pub fn abort_checkpoint(&self, session_id: &str, checkpoint_id: &str, epoch: u64) -> bool {
        // A session that is already gone holds no upload to abort. `false` is the fail-closed answer
        // this teardown-only path can report.
        let Ok(session) = self.session(session_id) else {
            return false;
        };
        // The same gate every other checkpoint mutation takes, so this cannot abort between one of
        // those being authorised and it reaching the store, and it does not hold the global epoch
        // fence across the store mutation the way a bare `epochs` guard would.
        let _ownership = lock(&session.ownership);
        if lock(&self.epochs).verify(session_id, epoch).is_err() {
            return false;
        }
        let mut checkpoints = lock(&self.checkpoints);
        checkpoints.abort(session_id, checkpoint_id)
    }

    pub fn checkpoint_info(&self, session_id: &str) -> KeeperResult<CheckpointInfo> {
        self.require_session(session_id)?;
        Ok(lock(&self.checkpoints).stored(session_id))
    }

    /// The committed checkpoint's identity, gated like any other read.
    ///
    /// A checkpoint download binds its answer to this identity and re-verifies it per chunk, so a
    /// commit that replaced it mid-read is reported as stale. Gating the first read is what stops a
    /// caller whose epoch was superseded from opening that download against a successor's recovery
    /// point at all.
    pub fn checkpoint_info_fenced(
        &self,
        session_id: &str,
        epoch: u64,
    ) -> KeeperResult<CheckpointInfo> {
        let session = self.session(session_id)?;
        let _ownership = lock(&session.ownership);
        lock(&self.epochs).verify(session_id, epoch)?;
        Ok(lock(&self.checkpoints).stored(session_id))
    }

    /// The committed checkpoint's bytes, for a future retrieval verb. `None` when none is committed.
    pub fn checkpoint_bytes(&self, session_id: &str) -> Option<Vec<u8>> {
        lock(&self.checkpoints)
            .checkpoint(session_id)
            .map(|checkpoint| checkpoint.bytes().to_vec())
    }

    /// Pins the committed checkpoint's identity for a paged download. Byte-free: opening a download
    /// costs the budget nothing, and every chunk re-verifies the identity it was pinned at.
    pub fn checkpoint_pin(&self, session_id: &str) -> Option<crate::checkpoint::CheckpointRef> {
        lock(&self.checkpoints).pin(session_id)
    }

    /// Copies one bounded slice of a pinned checkpoint into the caller's buffer.
    pub fn checkpoint_read_chunk(
        &self,
        pinned: &crate::checkpoint::CheckpointRef,
        offset: u64,
        out: &mut [u8],
    ) -> KeeperResult<crate::checkpoint::CheckpointChunk> {
        lock(&self.checkpoints).read_chunk(pinned, offset, out)
    }

    /// Authoritative discovery: every live session with its pid, start time and size.
    ///
    /// Sorted by session id, so the answer does not depend on `HashMap` iteration order.
    pub fn ownership(&self) -> Vec<SessionState> {
        let mut sessions: Vec<Arc<Session>> = lock(&self.sessions).values().cloned().collect();
        sessions.sort_by(|left, right| left.session_id().cmp(right.session_id()));
        // Nesting `epochs` then `replay` matches the order `spawn` already takes them in.
        let epochs = lock(&self.epochs);
        sessions
            .iter()
            .map(|session| {
                let epoch = epochs.newest(session.session_id()).unwrap_or(0);
                session.snapshot(epoch)
            })
            .collect()
    }

    pub fn session_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = lock(&self.sessions).keys().cloned().collect();
        ids.sort();
        ids
    }

    pub fn newest_epoch(&self, session_id: &str) -> Option<u64> {
        lock(&self.epochs).newest(session_id)
    }

    /// Refuses every later spawn and kills what is owned. Called by shutdown paths only.
    ///
    /// The registry is *drained*, not re-scanned: [`Keeper::session_ids`] plus [`Keeper::kill`] would
    /// spin forever on a session whose retirement keeps failing, and a busy loop in a shutdown path
    /// is a hang, not a retry. Each drained session is torn down here directly, recording its epoch
    /// floor and forgetting its fence exactly as `retire` does, so a shutdown leaves no session that
    /// a later respawn could fence below and no charge that nothing will release.
    pub fn shutdown(&self) {
        if self.shutting_down.swap(true, Ordering::SeqCst) {
            return;
        }
        // A spawn parked on this condvar, not polling the flag, must be woken to see the refusal.
        self.admission_done.notify_all();
        // A spawn that already passed its first check may commit after the first snapshot. Such
        // spawns are finite, because the flag now refuses every new one, so sweeping until the
        // registry is empty terminates and leaves nothing the shutdown never saw.
        loop {
            let drained: Vec<(String, Arc<Session>)> =
                lock(&self.sessions).drain().collect();
            if drained.is_empty() {
                return;
            }
            for (session_id, session) in drained {
                {
                    // The same gate `retire` takes, for the same reason: a checkpoint mutation that
                    // is authorising itself right now must not charge a session whose store entry
                    // this teardown is about to forget, or the charge would outlive the record that
                    // makes it reachable. Taken only around the bookkeeping, never across the kill.
                    let _ownership = lock(&session.ownership);
                    // The same order `retire` takes: retired, then epochs, and the registry entry is
                    // already gone (drained above), so this records the floor of an incarnation that
                    // is definitely no longer published.
                    let mut retired = lock(&self.retired);
                    let mut epochs = lock(&self.epochs);
                    if let Some(newest) = epochs.newest(&session_id) {
                        retired.record(&session_id, newest);
                    }
                    epochs.forget(&session_id);
                    drop(epochs);
                    drop(retired);
                    lock(&self.checkpoints).forget_session(&session_id);
                }
                session.interrupt_reader();
                if let Ok(mut killer) = session.killer.lock() {
                    let _ = killer.kill();
                }
                session.join_pump();
                // The reservation returns with the last `Arc`; a clone still reading replay bytes is
                // still holding the memory it was charged for.
                drop(session);
            }
        }
    }

    /// Runs one checkpoint mutation under the session's `ownership` gate.
    ///
    /// The gate is what makes the epoch check and the mutation one step: `attach` fences under the
    /// same gate, so no epoch can advance between them, and [`Keeper::retire`] holds it too, so a
    /// retirement cannot forget this session's charges while the mutation is charging them. Neither
    /// could be had from the store alone: these verbs create store state rather than matching
    /// existing state, and a successor's session is a fresh `Arc` whose own guard could never
    /// exclude the predecessor.
    ///
    /// The work held here is in-memory and bounded: no socket write, no allocation of the reply
    /// frame, and no blocking I/O. Reply bytes are produced after the guard is released. Trimming
    /// before releasing the store guard stops an `attach` from pairing a checkpoint with replay bytes
    /// it already covers.
    ///
    /// Lock order: ownership, then epochs (released immediately), then checkpoints, then replay.
    /// Every other path that takes checkpoints holds no epoch fence, so this adds no cycle.
    fn with_fenced_checkpoints<T>(
        &self,
        session_id: &str,
        epoch: u64,
        mutate: impl FnOnce(&mut CheckpointStore) -> KeeperResult<(T, Option<u64>)>,
    ) -> KeeperResult<T> {
        let session = self.session(session_id)?;
        let _ownership = lock(&session.ownership);
        lock(&self.epochs).verify(session_id, epoch)?;
        let mut checkpoints = lock(&self.checkpoints);
        let (value, truncate_to) = mutate(&mut checkpoints)?;
        if let Some(offset) = truncate_to {
            lock(&session.replay).truncate_before(offset);
        }
        Ok(value)
    }

    fn session(&self, session_id: &str) -> KeeperResult<Arc<Session>> {
        lock(&self.sessions)
            .get(session_id)
            .cloned()
            .ok_or_else(|| KeeperError::unknown_session(session_id))
    }

    fn require_session(&self, session_id: &str) -> KeeperResult<()> {
        if self.contains(session_id) {
            Ok(())
        } else {
            Err(KeeperError::unknown_session(session_id))
        }
    }

}

/// Holds one session id's spawn admission. Dropped on every path, including an unwind, so a panic
/// inside a spawn can never leave later spawns for that id waiting forever.
struct SpawnAdmission<'a> {
    keeper: &'a Keeper,
    session_id: &'a str,
}

impl Drop for SpawnAdmission<'_> {
    fn drop(&mut self) {
        lock(&self.keeper.admissions).remove(self.session_id);
        self.keeper.admission_done.notify_all();
    }
}

impl SpawnRequest {
    fn validate(&self) -> KeeperResult<()> {
        if self.session_id.trim().is_empty() {
            return Err(KeeperError::new(KeeperErrorCode::InvalidSpawn, "spawn without a session id"));
        }
        if self.argv.is_empty() || self.argv[0].trim().is_empty() {
            return Err(KeeperError::new(KeeperErrorCode::InvalidSpawn, "spawn without an argv"));
        }
        if self.cols == 0 || self.rows == 0 {
            return Err(KeeperError::new(
                KeeperErrorCode::InvalidSpawn,
                format!("spawn size {}x{} is not usable", self.cols, self.rows),
            ));
        }
        Ok(())
    }
}

/// Reads a master until the child closes it, appending every byte to the replay log untouched.
fn spawn_pump(
    session_id: String,
    mut reader: Box<dyn std::io::Read + Send>,
    replay: Arc<Mutex<ReplayLog>>,
) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name(format!("pty-pump-{session_id}"))
        .spawn(move || {
            let mut buffer = vec![0u8; PUMP_BUFFER_BYTES];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => return,
                    Ok(count) => {
                        let mut replay = lock(&replay);
                        replay.push(&buffer[..count]);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => return,
                }
            }
        })
        .expect("the keeper cannot run without its pty pump threads")
}

/// Process start time in microseconds since the epoch, so `(pid, start time)` identifies one
/// process and not a recycled pid. `None` when the platform cannot answer: the keeper reports what
/// it has rather than inventing an identity.
///
/// `libc` defines no `kinfo_proc` for Apple targets (it exists only for the FreeBSD family), so the
/// macOS answer comes from `proc_pidinfo(PROC_PIDTBSDINFO)`, whose `pbi_start_tvsec`/`pbi_start_tvusec`
/// pair is the same identity the daemon's `owner_admission` reads. Comparing the two sides is only
/// meaningful if both use this source.
#[cfg(target_os = "macos")]
pub fn process_start_time(pid: u32) -> Option<u64> {
    // SAFETY: `proc_bsdinfo` is a plain C struct and the all-zero pattern is a valid value for it.
    let mut info = unsafe { std::mem::zeroed::<libc::proc_bsdinfo>() };
    let size = std::mem::size_of_val(&info) as i32;
    // SAFETY: `info` is a valid, writable `proc_bsdinfo` of exactly `size` bytes, and `read` is the
    // kernel's own answer for that buffer rather than a count that could overrun it.
    let read = unsafe {
        libc::proc_pidinfo(
            pid as i32,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size,
        )
    };
    if read != size || info.pbi_pid != pid {
        return None;
    }
    if info.pbi_start_tvsec == 0 {
        return None;
    }
    info.pbi_start_tvsec
        .checked_mul(1_000_000)?
        .checked_add(info.pbi_start_tvusec)
}

/// Process start time in microseconds since the epoch, the contract stated above: Linux is the one
/// platform whose kernel does not answer in epoch time, because `/proc/<pid>/stat` counts ticks since
/// boot, so `btime` from `/proc/stat` is added here. Reporting the raw field would make the value an
/// uptime that every epoch-comparing reader - the daemon's own `started_at_unix_ms` for a direct
/// session, and the Phase 4 acceptance harness - reads as a process that started days ago, i.e. as a
/// recycled pid. This is the same `btime + ticks` conversion `terminal::suspension::unix` makes.
///
/// `btime` is derived from the wall clock, so a clock step between spawn and a later reading moves
/// this value, exactly as it can move the macOS arm; that is a property of identifying a process by
/// an epoch instant, and it is shared by both platforms rather than introduced here.
#[cfg(target_os = "linux")]
pub fn process_start_time(pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // Field 22 of `stat` is the start time in clock ticks after the process name, which may itself
    // contain spaces and parentheses, so counting starts after the last `)`.
    let rest = &stat[stat.rfind(')')? + 1..];
    let ticks: u64 = rest.split_whitespace().nth(19)?.parse().ok()?;
    let ticks_per_second = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if ticks_per_second <= 0 {
        return None;
    }
    // The ticks are counted since boot, so `btime` is what turns them into the documented epoch
    // microseconds - the same conversion `terminal::suspension::unix` makes for a direct session.
    let boot_seconds: u64 = std::fs::read_to_string("/proc/stat")
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("btime ")?.trim().parse().ok())?;
    boot_seconds
        .checked_mul(1_000_000)?
        .checked_add(ticks.saturating_mul(1_000_000) / ticks_per_second as u64)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn process_start_time(_pid: u32) -> Option<u64> {
    None
}

/// Whether `pid` still names the process this session spawned, judged by the start time recorded at
/// spawn. `true` when the platform cannot answer either side of that comparison: refusing every
/// signal on such a platform would be worse than the recycled-pid window it cannot detect.
#[cfg(unix)]
fn still_owns_process(pid: u32, recorded_start_time: Option<u64>) -> bool {
    match (recorded_start_time, process_start_time(pid)) {
        (Some(recorded), Some(current)) => recorded == current,
        _ => true,
    }
}

/// Locks a mutex, recovering from a poisoned guard: the keeper's state is plain data and one
/// panicking connection thread must not take the sessions down with it.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Wall-clock milliseconds for session activity. Monotonicity is not needed: the field is compared
/// with "never touched" (`0`), not ordered, so a clock step cannot turn an idle session active.
fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}

/// Where a session's activity timestamp comes from.
///
/// Injected rather than called directly, for one reason: a test must not depend on the wall clock.
/// An assertion that activity rose above zero passes for the wrong reason when the real clock
/// supplies the number, and a test that wanted a specific instant could otherwise only reach it by
/// sleeping - which this crate never does. Production passes [`now_millis`].
#[derive(Clone)]
pub struct Clock(Arc<dyn Fn() -> u64 + Send + Sync>);

impl Clock {
    /// The production source: the wall clock, which is what a discovery reader expects to see.
    pub fn wall() -> Self {
        Self(Arc::new(now_millis))
    }

    /// A clock that answers `millis` every time, whatever the wall clock says.
    pub fn fixed(millis: u64) -> Self {
        Self(Arc::new(move || millis))
    }

    /// A clock that hands out each value of `stamps` in turn and then repeats the last one, so a
    /// test can make two touches distinguishable without waiting between them.
    pub fn sequence(stamps: &[u64]) -> Self {
        let stamps = stamps.to_vec();
        let next = Arc::new(AtomicU64::new(0));
        Self(Arc::new(move || {
            let index = next.fetch_add(1, Ordering::SeqCst) as usize;
            stamps.get(index).copied().or_else(|| stamps.last().copied()).unwrap_or(0)
        }))
    }

    /// The current instant in milliseconds, from this clock.
    pub fn millis(&self) -> u64 {
        (self.0)()
    }
}

/// The production clock. Named separately so [`Keeper::new`] and a test read the same function.
fn default_clock() -> Clock {
    Clock::wall()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keeper() -> Keeper {
        Keeper::with_default_budget("test-generation")
    }

    fn keeper_with(total_bytes: usize, replay_bytes: usize) -> Keeper {
        Keeper::new(
            "test-generation",
            Arc::new(Mutex::new(MemoryBudget::with_limits(total_bytes, replay_bytes))),
        )
    }

    /// A keeper whose session activity stamps come from `stamps` rather than the wall clock.
    fn keeper_with_clock(stamps: &[u64]) -> Keeper {
        Keeper::with_clock(
            "test-generation",
            Arc::new(Mutex::new(MemoryBudget::default())),
            Clock::sequence(stamps),
        )
    }

    /// A buffer whose capacity carries no slack, so a capacity-based charge is the size the test
    /// asked for rather than whatever the allocator happened to hand back.
    fn exact(bytes: &[u8]) -> Vec<u8> {
        let mut buffer = Vec::with_capacity(bytes.len());
        buffer.extend_from_slice(bytes);
        buffer
    }

    fn request(session_id: &str) -> SpawnRequest {
        SpawnRequest {
            session_id: session_id.to_owned(),
            request_id: format!("request-{session_id}"),
            argv: vec!["/bin/cat".to_owned()],
            env: Vec::new(),
            cwd: None,
            cols: 80,
            rows: 24,
            epoch: 1,
        }
    }

    #[test]
    fn spawn_refused_by_the_budget_reports_the_figure_and_charges_nothing() {
        // One byte short of a session reservation: `try_reserve_replay` refuses, and the refusal
        // path must finish without taking the budget guard a second time. A second guard here
        // would hang this test instead of failing it, which is why the assertion is simply that it
        // returns.
        //
        // The 1024-byte replay sub-cap is what refuses, so the code is `REPLAY_BUDGET_EXHAUSTED`.
        // The message still names the replay cap, but nothing may dispatch on that: the code is the
        // distinction, and the two-cap test below proves the aggregate cap keeps its own code.
        let keeper = keeper_with(SESSION_RESERVATION_BYTES as usize - 1, 1024);
        let error = keeper.spawn(request("s1")).expect_err("one byte short must refuse");
        assert_eq!(error.code, KeeperErrorCode::ReplayBudgetExhausted);
        assert!(error.message.contains(&SESSION_RESERVATION_BYTES.to_string()));
        assert!(error.message.contains("of"));
        assert_eq!(keeper.session_count(), 0);
        let budget = keeper.budget();
        assert_eq!(budget.used(), 0, "a refused spawn charges nothing");
        assert_eq!(budget.reserved_bytes(), 0);
        assert_eq!(budget.replay_bytes(), 0, "a refused spawn charges no replay");
    }

    #[test]
    fn concurrent_spawns_of_one_id_create_one_child_and_charge_one_reservation() {
        const THREADS: usize = 4;
        let keeper = Arc::new(keeper());
        let barrier = Arc::new(std::sync::Barrier::new(THREADS));
        let workers: Vec<_> = (0..THREADS)
            .map(|index| {
                let keeper = Arc::clone(&keeper);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    let mut request = request("s1");
                    request.request_id = format!("request-{index}");
                    barrier.wait();
                    keeper.spawn(request).expect("every racer answers with the one session")
                })
            })
            .collect();
        let infos: Vec<SpawnInfo> = workers
            .into_iter()
            .map(|worker| worker.join().expect("spawn thread"))
            .collect();

        assert_eq!(keeper.session_count(), 1, "racing spawns must not create two sessions");
        let pid = infos[0].pid;
        for info in &infos {
            assert_eq!(info.pid, pid, "every racer must answer for the same child");
        }
        assert_eq!(
            keeper.budget().replay_bytes(),
            SESSION_RESERVATION_BYTES,
            "one child, one replay reservation"
        );
        keeper.kill("s1").expect("kill");
        assert_eq!(keeper.budget().replay_bytes(), 0);
    }

    #[test]
    fn a_kill_releases_the_reservation_only_when_the_last_session_owner_is_gone() {
        let keeper = keeper();
        keeper.spawn(request("s1")).expect("spawn");
        let held = keeper.session("s1").expect("session");
        keeper.kill("s1").expect("kill");
        assert_eq!(
            keeper.budget().replay_bytes(),
            SESSION_RESERVATION_BYTES,
            "a clone still holding the replay log keeps its bytes charged"
        );
        drop(held);
        assert_eq!(
            keeper.budget().replay_bytes(),
            0,
            "the reservation follows the last owner, not the registry entry"
        );
    }

    #[test]
    fn concurrent_admission_cannot_exceed_the_plan_ceiling() {
        const THREADS: usize = 3;
        let keeper = Arc::new(keeper_with(
            (SESSION_RESERVATION_BYTES as usize) * 2 + 1024,
            (SESSION_RESERVATION_BYTES as usize) * 2,
        ));
        let barrier = Arc::new(std::sync::Barrier::new(THREADS));
        let workers: Vec<_> = (0..THREADS)
            .map(|index| {
                let keeper = Arc::clone(&keeper);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    let id = format!("s{index}");
                    let mut request = request(&id);
                    request.request_id = format!("request-{index}");
                    barrier.wait();
                    keeper.spawn(request).map(|_| id)
                })
            })
            .collect();
        let results: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().expect("spawn thread"))
            .collect();

        let admitted: Vec<String> = results
            .iter()
            .filter_map(|result| result.as_ref().ok().cloned())
            .collect();
        let refusals: Vec<KeeperError> = results
            .iter()
            .filter_map(|result| result.as_ref().err().cloned())
            .collect();
        assert_eq!(admitted.len(), 2, "the ceiling admits exactly two admissions");
        assert_eq!(refusals.len(), 1, "the third admission is refused, never over-granted");
        assert_eq!(
            refusals[0].code,
            KeeperErrorCode::ReplayBudgetExhausted,
            "the replay sub-cap binds first here, so the refusal carries that code"
        );
        assert_eq!(
            keeper.budget().replay_bytes(),
            SESSION_RESERVATION_BYTES * 2,
            "charges equal the admitted sessions, not the attempted ones"
        );
        assert_eq!(keeper.session_count(), 2, "a refused spawn installs no session");
        for id in admitted {
            keeper.kill(&id).expect("kill");
        }
        assert_eq!(keeper.budget().replay_bytes(), 0);
    }

    #[test]
    fn detach_releases_the_attachment_without_ending_the_reader() {
        let keeper = keeper();
        keeper.spawn(request("s1")).expect("spawn");
        let interrupts = Arc::new(std::sync::atomic::AtomicU32::new(0));
        keeper
            .session("s1")
            .expect("session")
            .watch_interrupts(Arc::clone(&interrupts));

        keeper.attach("s1", 2, 0).expect("attach");
        keeper.detach("s1", 2).expect("detach");
        assert_eq!(
            interrupts.load(Ordering::SeqCst),
            0,
            "detach must not stop the pump that keeps collecting output"
        );
        assert!(keeper.contains("s1"), "a detached session is still owned");

        keeper.kill("s1").expect("kill");
        assert!(
            interrupts.load(Ordering::SeqCst) >= 1,
            "the kill path still ends the reader"
        );
    }

    #[test]
    fn a_reused_session_id_does_not_reopen_the_retired_incarnation_s_epochs() {
        let keeper = keeper();
        keeper.spawn(request("s1")).expect("spawn");
        keeper.attach("s1", 5, 0).expect("attach at 5");
        keeper.kill("s1").expect("kill");
        assert_eq!(keeper.newest_epoch("s1"), None, "the fence forgot the retired id");

        let mut reuse = request("s1");
        reuse.request_id = "request-second".to_owned();
        reuse.epoch = 1;
        let respawned = keeper.spawn(reuse).expect("respawn");
        assert!(
            respawned.epoch > 5,
            "a reused id must start above the retired incarnation, got {}",
            respawned.epoch
        );
        assert_eq!(
            keeper.write("s1", 1, b"x").expect_err("epoch 1 belongs to the retired incarnation").code,
            KeeperErrorCode::EpochMismatch
        );
        keeper.write("s1", respawned.epoch, b"x").expect("the new epoch writes");
        keeper.kill("s1").expect("kill");
    }

    #[test]
    fn concurrent_checkpoint_replacement_never_double_charges_or_leaks_a_charge() {
        let keeper = Arc::new(keeper());
        keeper.spawn(request("s1")).expect("spawn");
        keeper
            .install_checkpoint("s1", "c0", 1, 4, vec![0; 4], 1)
            .expect("first checkpoint");

        let barrier = Arc::new(std::sync::Barrier::new(2));
        let workers: Vec<_> = [(2u64, "c1"), (3u64, "c2")]
            .into_iter()
            .map(|(offset, id)| {
                let keeper = Arc::clone(&keeper);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    keeper.install_checkpoint("s1", id, offset, 4, vec![1; 4], 1)
                })
            })
            .collect();
        for worker in workers {
            match worker.join().expect("install thread") {
                Ok(info) => assert_eq!(info.bytes(), 4),
                Err(error) => assert_eq!(error.code, KeeperErrorCode::StaleCheckpoint),
            }
        }

        let committed = keeper.checkpoint_info("s1").expect("session").bytes();
        let budget = keeper.budget();
        assert_eq!(
            budget.checkpoint_committed_bytes() as u64,
            committed,
            "the committed bucket must equal what the session actually holds"
        );
        assert_eq!(
            budget.used(),
            SESSION_RESERVATION_BYTES + committed,
            "one session reservation plus one committed checkpoint, however many installs raced"
        );
        drop(budget);
        keeper.kill("s1").expect("kill");
        assert_eq!(keeper.budget().used(), 0, "kill returns every charge it held");
    }

    #[test]
    fn a_stale_abort_leaves_a_successors_upload_intact() {
        let keeper = keeper();
        keeper.spawn(request("s1")).expect("spawn");
        keeper
            .begin_checkpoint("s1", "shared", 9, 4096, 1)
            .expect("the epoch that owns the session opens the upload");
        assert!(keeper.budget().checkpoint_pending_bytes() > 0);

        keeper.attach("s1", 2, 0).expect("a successor takes the session");
        assert!(
            !keeper.abort_checkpoint("s1", "shared", 1),
            "the retired epoch must not release the upload it no longer owns"
        );
        assert!(
            keeper.budget().checkpoint_pending_bytes() > 0,
            "a stale abort must leave the charge, and so the upload, in place"
        );

        keeper.kill("s1").expect("kill");
        assert_eq!(keeper.budget().used(), 0);
    }

    #[test]
    fn a_stale_epoch_cannot_install_over_the_committed_checkpoint() {
        let keeper = keeper();
        keeper.spawn(request("s1")).expect("spawn");
        keeper
            .install_checkpoint("s1", "mine", 4, 4, b"abcd".to_vec(), 1)
            .expect("the owning epoch installs");
        keeper.attach("s1", 2, 0).expect("a successor takes the session");

        let refusal = keeper
            .install_checkpoint("s1", "theirs", 9, 4, b"wxyz".to_vec(), 1)
            .expect_err("a superseded epoch must not replace the checkpoint");
        assert_eq!(refusal.code, KeeperErrorCode::EpochMismatch);
        assert_eq!(
            keeper.checkpoint_info("s1").expect("session").checkpoint_id,
            "mine",
            "the committed checkpoint is still the one its owner installed"
        );
        keeper.kill("s1").expect("kill");
    }

    #[test]
    fn a_fenced_replay_and_head_refuse_an_epoch_a_successor_replaced() {
        let keeper = keeper();
        keeper.spawn(request("s1")).expect("spawn");
        {
            let session = keeper.session("s1").expect("session");
            lock(&session.replay).push(b"hello");
        }
        keeper.attach("s1", 2, 0).expect("a successor takes the session");

        assert_eq!(
            keeper.replay_fenced("s1", 0, 1).expect_err("stale replay").code,
            KeeperErrorCode::EpochMismatch
        );
        assert_eq!(
            keeper.head_fenced("s1", 1).expect_err("stale head").code,
            KeeperErrorCode::EpochMismatch
        );

        assert_eq!(keeper.head_fenced("s1", 2).expect("the owner reads"), 5);
        assert_eq!(keeper.replay_fenced("s1", 0, 2).expect("the owner replays").data, b"hello");
        keeper.kill("s1").expect("kill");
    }

    #[test]
    fn a_stale_kill_does_not_retire_the_session_its_successor_owns() {
        let keeper = keeper();
        keeper.spawn(request("s1")).expect("spawn");
        keeper.attach("s1", 2, 0).expect("a successor takes the session");

        assert_eq!(
            keeper.kill_fenced("s1", 1).expect_err("a superseded epoch must not kill").code,
            KeeperErrorCode::EpochMismatch
        );
        assert!(keeper.contains("s1"), "the session its successor owns is untouched");
        assert_eq!(keeper.newest_epoch("s1"), Some(2), "the fence is unchanged");
        keeper.kill("s1").expect("kill");
    }

    #[test]
    fn a_duplicate_spawn_for_a_retired_request_id_forks_no_second_child() {
        let keeper = keeper();
        let first = keeper.spawn(request("s1")).expect("spawn");
        keeper.kill("s1").expect("kill");
        assert_eq!(keeper.budget().used(), 0);

        let duplicate = keeper
            .spawn(request("s1"))
            .expect_err("a request id that already forked must not fork again");
        assert_eq!(duplicate.code, KeeperErrorCode::InvalidSpawn);
        assert!(
            duplicate.message.contains("SPAWN_REQUEST_RETIRED"),
            "the refusal names itself: {}",
            duplicate.message
        );
        assert_eq!(keeper.session_count(), 0, "no child was created");
        assert_eq!(keeper.budget().used(), 0, "a refused duplicate charges nothing");
        assert!(first.pid.is_some());

        let mut fresh = request("s1");
        fresh.request_id = "request-second".to_owned();
        keeper.spawn(fresh).expect("a new request id may spawn the reused session id");
        keeper.kill("s1").expect("kill");
    }

    #[test]
    fn spawn_without_argv_or_size_is_refused_before_a_pty_exists() {
        let keeper = keeper();
        let mut empty = request("s1");
        empty.argv.clear();
        assert_eq!(empty.validate().unwrap_err().code, KeeperErrorCode::InvalidSpawn);

        let mut narrow = request("s2");
        narrow.cols = 0;
        assert_eq!(narrow.validate().unwrap_err().code, KeeperErrorCode::InvalidSpawn);
        assert_eq!(keeper.session_count(), 0);
    }

    #[test]
    fn spawn_is_idempotent_on_request_id_and_creates_one_pty() {
        let keeper = keeper();
        let first = keeper.spawn(request("s1")).expect("spawn");
        let again = keeper.spawn(request("s1")).expect("retried spawn");
        assert_eq!(first.pid, again.pid);
        assert_eq!(first.process_start_time, again.process_start_time);
        assert_eq!(keeper.session_count(), 1);
        keeper.kill("s1").expect("kill");
        assert_eq!(keeper.session_count(), 0);
    }

    #[test]
    fn a_stale_epoch_cannot_write_to_a_session_a_successor_attached() {
        let keeper = keeper();
        keeper.spawn(request("s1")).expect("spawn");
        keeper.attach("s1", 2, 0).expect("attach at epoch 2");
        let error = keeper.write("s1", 1, b"x").expect_err("epoch 1 is fenced");
        assert_eq!(error.code, KeeperErrorCode::EpochMismatch);
        keeper.write("s1", 2, b"x").expect("the newest epoch writes");
        keeper.kill("s1").expect("kill");
    }

    #[test]
    fn killing_a_session_releases_its_reservation_and_its_checkpoint() {
        let keeper = keeper();
        keeper.spawn(request("s1")).expect("spawn");
        let used_after_spawn = keeper.budget().used();
        assert!(used_after_spawn >= SESSION_RESERVATION_BYTES);
        keeper
            .install_checkpoint("s1", "c1", 10, 4, vec![1, 2, 3, 4], 1)
            .expect("checkpoint");
        keeper.kill("s1").expect("kill");
        let used_after_kill = keeper.budget().used();
        assert!(
            used_after_kill + SESSION_RESERVATION_BYTES <= used_after_spawn,
            "kill released the session reservation: {used_after_kill} + {SESSION_RESERVATION_BYTES} <= {used_after_spawn}"
        );
        assert!(keeper.checkpoint_bytes("s1").is_none());
        assert!(keeper.newest_epoch("s1").is_none());
    }

    #[test]
    fn ownership_reports_the_same_pid_after_reattach() {
        let keeper = keeper();
        let spawned = keeper.spawn(request("s1")).expect("spawn");
        keeper.attach("s1", 2, 0).expect("attach");
        let owned = keeper.ownership();
        assert_eq!(owned.len(), 1);
        assert_eq!(owned[0].pid(), spawned.pid);
        keeper.kill("s1").expect("kill");
    }

    #[test]
    fn an_unknown_session_is_reported_unknown_not_silently_ignored() {
        let keeper = keeper();
        for result in [
            keeper.write("nope", 1, b"").map(|_| ()),
            keeper.resize("nope", 1, 80, 24),
            keeper.attach("nope", 1, 0).map(|_| ()),
            keeper.detach("nope", 1),
            keeper.head("nope").map(|_| ()),
            keeper.kill("nope"),
        ] {
            assert_eq!(result.unwrap_err().code, KeeperErrorCode::UnknownSession);
        }
    }

    #[test]
    fn a_retired_session_takes_its_pending_and_committed_checkpoint_charges_with_it() {
        let keeper = keeper();
        keeper.spawn(request("s1")).expect("spawn");
        keeper
            .install_checkpoint("s1", "c1", 1, 4, vec![7; 4], 1)
            .expect("install");
        keeper.begin_checkpoint("s1", "c2", 9, 1024, 1).expect("begin");
        assert!(keeper.budget().checkpoint_pending_bytes() > 0);
        assert!(keeper.budget().checkpoint_committed_bytes() > 0);

        keeper.kill("s1").expect("kill");
        let budget = keeper.budget();
        assert_eq!(budget.checkpoint_pending_bytes(), 0, "a pending upload cannot outlive its session");
        assert_eq!(
            budget.checkpoint_committed_bytes(),
            0,
            "a committed checkpoint cannot outlive its session"
        );
        assert_eq!(budget.used(), 0, "kill returns every charge it held");
    }

    #[test]
    fn a_checkpoint_for_a_retired_session_is_refused_and_charges_nothing() {
        let keeper = keeper();
        keeper.spawn(request("s1")).expect("spawn");
        keeper.kill("s1").expect("kill");

        let installed = keeper
            .install_checkpoint("s1", "c1", 1, 4, vec![7; 4], 1)
            .expect_err("a retired session cannot take a checkpoint");
        assert_eq!(installed.code, KeeperErrorCode::UnknownSession);
        let begun = keeper
            .begin_checkpoint("s1", "c2", 1, 1024, 1)
            .expect_err("a retired session cannot open an upload");
        assert_eq!(begun.code, KeeperErrorCode::UnknownSession);
        assert_eq!(keeper.budget().used(), 0, "a refused checkpoint charges nothing");
    }

    #[test]
    fn an_attach_pairs_the_committed_checkpoint_with_the_replay_that_follows_it() {
        let keeper = keeper();
        keeper.spawn(request("s1")).expect("spawn");
        {
            let session = keeper.session("s1").expect("session");
            lock(&session.replay).push(b"0123456789");
        }
        keeper
            .install_checkpoint("s1", "c1", 4, 4, b"abcd".to_vec(), 1)
            .expect("checkpoint");

        // The checkpoint covers `[0, 4)`, so the retained log starts at 4 and nothing before it can
        // be served: a reader that asked from 0 must be told where its history actually begins.
        let from_zero = keeper.attach("s1", 2, 0).expect("attach from 0");
        assert_eq!(from_zero.checkpoint.sequence_offset, 4);
        assert_eq!(from_zero.sequence_offset, 10);
        assert!(from_zero.data.is_empty(), "the covered bytes are not resent");
        let gap = from_zero.replay_gap.expect("a reader behind the checkpoint is told so");
        assert_eq!(gap.retained_from_offset, 4);

        let from_checkpoint = keeper.attach("s1", 3, 4).expect("attach from 4");
        assert_eq!(from_checkpoint.data, b"456789");
        assert!(from_checkpoint.replay_gap.is_none(), "a covered request is not a gap");
        keeper.kill("s1").expect("kill");
    }

    #[test]
    fn ownership_reports_the_epoch_and_the_replay_state_a_reattach_needs() {
        // Injected clock: "spawn and attach both stamp activity" is asserted as a specific instant
        // rather than as "greater than zero", which the wall clock would satisfy by accident.
        let keeper = keeper_with_clock(&[1_500, 1_750]);
        keeper.spawn(request("s1")).expect("spawn");
        keeper.attach("s1", 3, 0).expect("attach");
        {
            let session = keeper.session("s1").expect("session");
            lock(&session.replay).push(b"hello");
        }

        let owned = keeper.ownership();
        assert_eq!(owned.len(), 1);
        let state = &owned[0];
        assert_eq!(state.epoch(), 3, "a reattaching daemon learns the epoch it must use");
        assert_eq!(state.sequence_offset(), 5);
        assert_eq!(state.replay_retained_bytes, 5);
        assert_eq!(state.replay_resets, 0);
        assert_eq!(
            state.last_activity_millis(),
            1_750,
            "spawn and attach both stamp activity; the attach's stamp is the later one"
        );
        keeper.kill("s1").expect("kill");
    }

    #[test]
    fn a_write_stamps_activity_so_discovery_can_tell_an_active_session_from_an_idle_one() {
        // An injected clock, so "activity rose" is a fact about the keeper's ordering and not about
        // how fast the wall clock moved. Spawn stamps 1_000, the write stamps 2_000.
        let keeper = keeper_with_clock(&[1_000, 2_000]);
        keeper.spawn(request("s1")).expect("spawn");
        assert_eq!(
            keeper.ownership()[0].last_activity_millis(),
            1_000,
            "the spawn stamp is the first value the injected clock hands out"
        );
        keeper.write("s1", 1, b"x").expect("write");
        assert_eq!(
            keeper.ownership()[0].last_activity_millis(),
            2_000,
            "a write restamps the session it reached, with the clock's next value"
        );
        keeper.kill("s1").expect("kill");
    }

    #[test]
    fn a_fixed_clock_pins_every_activity_stamp_so_no_test_reads_the_wall_clock() {
        // The mirror of the test above: a fixed clock must not advance, and an unwritten session
        // must keep reporting the instant it was stamped at rather than something later.
        let keeper = Keeper::with_clock(
            "test-generation",
            Arc::new(Mutex::new(MemoryBudget::default())),
            Clock::fixed(4_242),
        );
        keeper.spawn(request("s1")).expect("spawn");
        keeper.spawn(request("s2")).expect("spawn");
        keeper.write("s1", 1, b"x").expect("write");
        let owned = keeper.ownership();
        assert_eq!(owned.len(), 2);
        for state in &owned {
            assert_eq!(
                state.last_activity_millis(),
                4_242,
                "a fixed clock reports the same instant however often it is read"
            );
        }
        keeper.kill("s1").expect("kill");
        keeper.kill("s2").expect("kill");
    }

    #[test]
    fn a_clock_sequence_repeats_its_last_value_rather_than_exhausting() {
        // A session can be touched more often than a test cares to enumerate. Running out must not
        // wrap to a stamp that looks idle, which is the one value discovery treats specially.
        let clock = Clock::sequence(&[7, 9]);
        assert_eq!(clock.millis(), 7);
        assert_eq!(clock.millis(), 9);
        assert_eq!(clock.millis(), 9, "past the end the last value repeats");
        assert_eq!(Clock::sequence(&[]).millis(), 0);
    }

    #[test]
    fn shutting_down_kills_every_session_and_refuses_new_spawns() {
        let keeper = keeper();
        keeper.spawn(request("s1")).expect("spawn");
        keeper.spawn(request("s2")).expect("spawn");
        keeper.shutdown();
        assert_eq!(keeper.session_count(), 0);
        assert_eq!(
            keeper.spawn(request("s3")).unwrap_err().code,
            KeeperErrorCode::ShuttingDown
        );
    }

    #[test]
    fn shutdown_returns_every_charge_a_live_session_held() {
        // A shutdown is the last chance to make the ledger honest: the process is about to exit, so
        // anything still charged is memory nothing will ever release. Both checkpoint buckets must
        // come back too, not just the session's replay reservation.
        let keeper = keeper();
        keeper.spawn(request("s1")).expect("spawn");
        keeper
            .install_checkpoint("s1", "c1", 4, 4, exact(b"abcd"), 1)
            .expect("committed checkpoint");
        keeper.begin_checkpoint("s1", "c2", 9, 1024, 1).expect("open upload");
        assert!(keeper.budget().checkpoint_committed_bytes() > 0);
        assert!(keeper.budget().checkpoint_pending_bytes() > 0);

        keeper.shutdown();

        {
            let budget = keeper.budget();
            assert_eq!(budget.replay_bytes(), 0, "the session reservation is returned");
            assert_eq!(
                budget.checkpoint_committed_bytes(),
                0,
                "the committed checkpoint is returned"
            );
            assert_eq!(budget.checkpoint_pending_bytes(), 0, "the open upload is returned");
            assert_eq!(budget.used(), 0, "shutdown leaves no charge behind");
        }
        assert!(keeper.checkpoint_bytes("s1").is_none(), "no checkpoint outlives its session");
        assert!(keeper.newest_epoch("s1").is_none(), "no fence outlives its session");
        assert_eq!(keeper.session_count(), 0);
    }

    #[test]
    fn a_second_shutdown_is_a_no_op() {
        // `SHUTDOWN` is reachable from more than one teardown path, so repeating it must be free and
        // must not try to kill sessions that are already gone.
        let keeper = keeper();
        keeper.spawn(request("s1")).expect("spawn");
        keeper.shutdown();
        keeper.shutdown();
        assert_eq!(keeper.session_count(), 0);
        assert_eq!(keeper.budget().used(), 0);
        assert_eq!(
            keeper.spawn(request("s2")).unwrap_err().code,
            KeeperErrorCode::ShuttingDown,
            "the refusal survives a repeated shutdown"
        );
    }

    #[test]
    fn a_retry_under_a_second_request_id_is_retired_with_the_session_it_named() {
        // The retry tier of `existing_spawn` records the id it arrived under, not only the id the
        // session was created with. Without that, a client that retried under a fresh id would fall
        // outside the tombstone: once the session is killed, a duplicate of that retry forks again.
        let keeper = keeper();
        let first = keeper.spawn(request("s1")).expect("spawn");
        let mut retry = request("s1");
        retry.request_id = "request-retry".to_owned();
        let retried = keeper.spawn(retry).expect("a retry answers with the same session");
        assert_eq!(retried.pid, first.pid, "the retry did not open a second PTY");
        assert_eq!(keeper.session_count(), 1);

        keeper.kill("s1").expect("kill");
        assert_eq!(keeper.session_count(), 0);

        let duplicate = keeper
            .spawn({
                let mut again = request("s1");
                again.request_id = "request-retry".to_owned();
                again
            })
            .expect_err("the retry's id is retired too");
        assert_eq!(duplicate.code, KeeperErrorCode::InvalidSpawn);
        assert!(duplicate.message.contains("SPAWN_REQUEST_RETIRED"));
        assert_eq!(
            duplicate.offset, None,
            "an id refusal carries no recovery offset to resume from"
        );
        assert_eq!(keeper.session_count(), 0, "the duplicate forked nothing");
        assert_eq!(keeper.budget().used(), 0, "the duplicate charged nothing");
    }

    #[test]
    fn the_tombstone_is_bounded_so_absence_from_it_proves_nothing() {
        // Pure table test: no PTY, no keeper. The bound is what makes "no tombstone" a non-answer,
        // so the eviction it performs has to be observable rather than assumed.
        let mut tombstones = RequestTombstones::with_capacity(2);
        tombstones.remember("r1", "s1");
        tombstones.remember("r2", "s2");
        assert_eq!(tombstones.forked_session("r1"), Some("s1"), "a hit is proof");
        assert_eq!(tombstones.forked_session("r2"), Some("s2"));

        tombstones.remember("r3", "s3");
        assert_eq!(
            tombstones.forked_session("r1"),
            None,
            "the oldest id is evicted, so its absence cannot be read as 'never forked'"
        );
        assert_eq!(tombstones.forked_session("r2"), Some("s2"), "the bound evicts only the oldest");
        assert_eq!(tombstones.forked_session("r3"), Some("s3"));

        // Re-remembering a live id must not grow the order queue and evict a second, unrelated id.
        tombstones.remember("r3", "s3");
        assert_eq!(tombstones.forked_session("r2"), Some("s2"));
        assert_eq!(tombstones.order.len(), 2, "a repeated id is not queued twice");

        let production = RequestTombstones::new();
        assert_eq!(
            production.capacity, RETIRED_REQUEST_IDS,
            "the production table takes the plan bound, and `Default` cannot hand back a zero"
        );
    }

    #[test]
    fn a_stale_detach_is_fenced_off_like_every_other_epoch_use() {
        // Detach mutates nothing, but it must not be the one verb that answers `Ok` for an epoch a
        // successor already replaced: a daemon that took that answer as "still mine" would keep
        // writing into a session it lost.
        let keeper = keeper();
        keeper.spawn(request("s1")).expect("spawn");
        keeper.attach("s1", 2, 0).expect("a successor takes the session");

        assert_eq!(
            keeper.detach("s1", 1).expect_err("epoch 1 is fenced").code,
            KeeperErrorCode::EpochMismatch
        );
        keeper.detach("s1", 2).expect("the newest epoch detaches");
        assert!(keeper.contains("s1"), "detach keeps the session");
        keeper.kill("s1").expect("kill");
    }

    #[test]
    fn the_keeper_charges_sessions_replay_and_checkpoints_to_one_ledger() {
        // The whole point of the shared `Arc`: a second ledger would each believe it fits, and the
        // plan's 768 MiB aggregate cap would bind on neither. Identity, not equality - two budgets
        // with equal counters are exactly the failure this asserts against.
        let keeper = keeper();
        keeper.spawn(request("s1")).expect("spawn");
        let session = keeper.session("s1").expect("session");
        assert!(
            Arc::ptr_eq(&keeper.budget_handle(), &session.budget),
            "the session charges the keeper's ledger"
        );
        assert!(
            Arc::ptr_eq(&keeper.budget_handle(), &lock(&keeper.checkpoints).budget_handle()),
            "the checkpoint store charges the keeper's ledger"
        );
        // The session handle is an `Arc<Session>` clone, and its reservation is released by
        // `Session::drop` once the *last* owner is gone (contract §2: "the session's replay
        // reservation is returned with the last `Arc`"; `Session`'s own `Drop` comment in this
        // file). Holding it across the kill below would keep 8 MiB charged by design, so it is
        // dropped here once the identity assertions above have used it - the ledger is still the
        // same one either way, which is what this test is about.
        drop(session);

        // And that they are observably the same ledger: a session charge is visible to the store and
        // a checkpoint charge is visible to the session's view of the budget.
        let used_with_session = keeper.budget().used();
        assert!(used_with_session >= SESSION_RESERVATION_BYTES);
        keeper
            .install_checkpoint("s1", "c1", 4, 4, exact(b"abcd"), 1)
            .expect("checkpoint");
        let used_with_checkpoint = keeper.budget().used();
        assert_eq!(
            used_with_checkpoint,
            used_with_session + 4,
            "the checkpoint charge lands on the same ledger the session is charged to"
        );
        assert_eq!(
            lock(&keeper.checkpoints).committed_bytes(),
            4,
            "the store reports the charge the keeper's ledger holds"
        );
        keeper.kill("s1").expect("kill");
        assert_eq!(keeper.budget().used(), 0);
    }

    #[test]
    fn every_checkpoint_staging_path_returns_its_charge() {
        // Abort, commit and kill are three different ways an upload stops being in flight, and a
        // charge that survives any one of them is memory no peer can free.
        let keeper = keeper();
        keeper.spawn(request("s1")).expect("spawn");
        let base = keeper.budget().used();

        // Open, then abort.
        keeper.begin_checkpoint("s1", "a", 1, 1024, 1).expect("begin");
        let opened = keeper.budget().checkpoint_pending_bytes();
        assert!(opened > 0);
        assert!(keeper.abort_checkpoint("s1", "a", 1), "the owning epoch aborts");
        assert_eq!(keeper.budget().checkpoint_pending_bytes(), 0, "abort returns the charge");
        assert_eq!(keeper.budget().used(), base, "abort returns the charge and nothing else");

        // Open, fill, commit. The charge is measured rather than assumed: the store bills the
        // allocation it keeps, which is the declared size rounded by the allocator.
        keeper.begin_checkpoint("s1", "b", 5, 4, 1).expect("begin");
        let staged = keeper.budget().checkpoint_pending_bytes();
        assert!(staged >= 4, "the whole declared size is charged before a byte arrives");
        keeper.checkpoint_chunk("s1", "b", 0, b"abcd", 1).expect("chunk");
        assert_eq!(
            keeper.budget().checkpoint_pending_bytes(),
            staged,
            "a chunk fills the buffer that was already charged instead of growing it"
        );
        keeper.commit_checkpoint("s1", "b", 5, 1).expect("commit");
        assert_eq!(keeper.budget().checkpoint_pending_bytes(), 0, "commit empties the pending bucket");
        // `used()` is `u64`; `checkpoint_pending_bytes()` is `usize`, so the staged charge is cast
        // rather than silently widening the comparison on one side only.
        assert_eq!(
            keeper.budget().used(),
            base + staged as u64,
            "commit moves the charge to the committed bucket, it does not drop it"
        );

        // Then the session dies with both buckets populated.
        keeper.begin_checkpoint("s1", "c", 9, 1024, 1).expect("begin");
        assert!(keeper.budget().checkpoint_pending_bytes() > 0);
        keeper.kill("s1").expect("kill");
        assert_eq!(keeper.budget().used(), 0, "kill returns every charge the session held");
    }

    #[test]
    fn a_replay_read_past_an_overflow_is_a_gap_that_resumes_at_the_head() {
        // Goal 2's second half, at the level where it is decidable: the ring is 8 MiB and a real PTY
        // cannot be made to emit exactly 8 MiB + 1 deterministically, so the overflow is driven
        // through the session's own log. The reader's answer is what matters, and it must be a gap -
        // never a short answer that looks complete.
        let keeper = keeper();
        keeper.spawn(request("s1")).expect("spawn");
        // The guard borrows `session`, so the binding must outlive it: returning
        // `lock(&session.replay)` from a block tail moved the guard past the `session` it borrows.
        let session = keeper.session("s1").expect("session");
        let capacity = lock(&session.replay).capacity();
        assert_eq!(
            capacity,
            crate::REPLAY_SESSION_BYTES,
            "the production ring is the plan cap"
        );

        // Fill exactly, then one byte more. A run that exactly fills is retained; the overflow is the
        // byte after it, which discards the whole suffix and moves the resume point to the head.
        let fill = vec![b'a'; capacity];
        {
            lock(&session.replay).push(&fill);
        }
        assert_eq!(keeper.head("s1").expect("head"), capacity as u64);
        let full = keeper.replay("s1", 0).expect("a full ring still replays from 0");
        assert!(full.replay_gap.is_none(), "a window that exactly fills is not a gap");
        assert_eq!(full.data.len(), REPLY_PAGE_BYTES, "one page, and the rest is `more`");
        assert!(full.more);

        {
            lock(&session.replay).push(b"x");
        }
        let head = keeper.head("s1").expect("head");
        assert_eq!(head, capacity as u64 + 1);

        let overflowed = keeper.replay("s1", 0).expect("a gap is an answer, not an error");
        let gap = overflowed.replay_gap.expect("the reader is told the history is gone");
        assert_eq!(
            gap.resumed_at_offset, head,
            "the resume point is the output head, never the first discarded byte"
        );
        assert_eq!(gap.requested_after_offset, 0);
        assert_eq!(
            gap.retained_from_offset, head,
            "nothing of the discarded suffix is claimed as retained"
        );
        assert!(overflowed.data.is_empty(), "a gap carries no bytes to mistake for history");
        assert_eq!(overflowed.sequence_offset, head);
        assert!(!overflowed.more);

        // And the gap is not sticky: a read from the new head is a clean, empty answer.
        let from_head = keeper.replay("s1", head).expect("a read at the head is fine");
        assert!(from_head.replay_gap.is_none(), "reading at the head is not a gap");
        assert!(from_head.data.is_empty());
        keeper.kill("s1").expect("kill");
    }

    #[test]
    fn the_two_budget_caps_refuse_with_their_own_codes_and_one_shared_ledger_binds() {
        // Goal 2's "typed code, never a message prefix" needs both caps to be reachable *and*
        // distinguishable. Arranging the aggregate to bind while the replay sub-cap still has room
        // takes a checkpoint: checkpoint bytes spend the aggregate and never the sub-cap.
        //
        // Total is two reservations plus one byte, so the replay sub-cap is 16 MiB and has exactly
        // one reservation left after s1's. The 4-byte checkpoint then spends 4 of the aggregate's
        // last 8 MiB + 1 byte, leaving 8 MiB - 3: the sub-cap check passes and the aggregate refuses.
        let committed = exact(b"abcd");
        let capacity = committed.capacity() as u64;
        let keeper = keeper_with(
            (SESSION_RESERVATION_BYTES * 2 + 1) as usize,
            (SESSION_RESERVATION_BYTES * 2) as usize,
        );
        keeper.spawn(request("s1")).expect("spawn");
        keeper
            .install_checkpoint("s1", "cp-1", 4, capacity, exact(b"abcd"), 1)
            .expect("a 4-byte checkpoint fits");
        assert_eq!(
            keeper.budget().replay_available(),
            SESSION_RESERVATION_BYTES,
            "the sub-cap has room for exactly one more reservation"
        );
        assert_eq!(
            keeper.budget().available(),
            SESSION_RESERVATION_BYTES - capacity + 1,
            "the aggregate has three bytes fewer than one more reservation"
        );

        // The sub-cap check passes and the aggregate refuses, so the code must be the aggregate's.
        let aggregate = keeper
            .spawn(request("s2"))
            .expect_err("the aggregate cap is exhausted, whatever the sub-cap holds");
        assert_eq!(aggregate.code, KeeperErrorCode::MemoryBudgetExhausted);
        assert_eq!(
            keeper.session_count(),
            1,
            "the refused spawn opened no second session"
        );
        assert_eq!(
            keeper.budget().used(),
            SESSION_RESERVATION_BYTES + capacity,
            "a refused spawn charges nothing on top of what s1 already holds"
        );
        assert_eq!(
            keeper.checkpoint_bytes("s1").as_deref(),
            Some(&b"abcd"[..]),
            "and it evicted nothing: a refusal is not an eviction"
        );

        // The sub-cap refusal carries the *other* code; see
        // `spawn_refused_by_the_budget_reports_the_figure_and_charges_nothing`. What this test adds
        // is that the aggregate cap keeps its own code while both caps read one ledger.
        keeper.kill("s1").expect("kill");
        assert_eq!(keeper.budget().used(), 0, "kill returned both charges");
    }

    #[test]
    fn a_checkpoint_that_does_not_fit_keeps_the_previous_one_and_is_typed() {
        // Exactly the session reservation plus the committed checkpoint's capacity: one more byte of
        // new checkpoint cannot fit at its peak, and the refusal must say so by name rather than
        // dropping the recovery point the daemon still needs.
        let committed = exact(b"abcd");
        let capacity = committed.capacity() as u64;
        let declared = committed.len() as u64;
        assert_eq!(capacity, declared, "the helper must hand over a slack-free buffer");
        let keeper = keeper_with(
            (SESSION_RESERVATION_BYTES + capacity) as usize,
            SESSION_RESERVATION_BYTES as usize,
        );
        keeper.spawn(request("s1")).expect("spawn");
        keeper
            .install_checkpoint("s1", "cp-1", 4, declared, exact(b"abcd"), 1)
            .expect("the first checkpoint fits");

        let refusal = keeper
            .install_checkpoint("s1", "cp-2", 9, declared, exact(b"wxyz"), 1)
            .expect_err("the peak does not fit");
        assert_eq!(refusal.code, KeeperErrorCode::CheckpointBudgetExhausted);
        assert!(refusal.message.contains("cp-2"), "the refusal names what was refused");
        assert_eq!(
            refusal.offset,
            Some(4),
            "the refusal names the committed offset the daemon can still resume from"
        );
        assert_eq!(
            keeper.checkpoint_bytes("s1").as_deref(),
            Some(&b"abcd"[..]),
            "the previous checkpoint survived the refusal"
        );
        assert_eq!(
            keeper.budget().used(),
            SESSION_RESERVATION_BYTES + capacity,
            "a refused checkpoint charges nothing and releases nothing"
        );
        keeper.kill("s1").expect("kill");
    }

    #[test]
    fn size_is_tracked_across_resize_and_reported_in_ownership() {
        let keeper = keeper();
        keeper.spawn(request("s1")).expect("spawn");
        keeper.resize("s1", 1, 132, 43).expect("resize");
        let owned = keeper.ownership();
        assert_eq!((owned[0].cols(), owned[0].rows()), (132, 43));
        keeper.kill("s1").expect("kill");
    }

    #[test]
    fn signals_parse_to_their_wire_names() {
        for name in ["interrupt", "terminate", "kill", "stop", "continue"] {
            let signal = SessionSignal::parse(name).expect("known signal");
            assert_eq!(signal.as_str(), name);
        }
        assert!(SessionSignal::parse("suspend").is_none());
    }

    #[test]
    fn a_start_time_is_reported_for_this_process() {
        let start = process_start_time(std::process::id()).expect("own start time");
        assert!(start > 0);
        // The unit is documented as microseconds since the epoch, so the reading has to land inside
        // this process's own lifetime. A since-boot value (the native unit of the Linux field) is
        // days in the past and sails straight past a bare `> 0`, which is how the mismatch survived.
        // The one-hour ceiling is the test binary's own runtime: no case in this crate takes minutes,
        // so a larger gap means the value is not epoch microseconds rather than that the suite is slow.
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the wall clock is after the epoch")
            .as_micros() as u64;
        assert!(start <= now, "the reported start {start}us is later than now {now}us");
        assert!(
            now - start < 3_600 * 1_000_000,
            "the reported start is {}s in the past, so it is not epoch microseconds",
            (now - start) / 1_000_000
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_recycled_pid_is_not_mistaken_for_this_session_s_child() {
        let own = std::process::id();
        let real = process_start_time(own).expect("own start time");
        assert!(still_owns_process(own, Some(real)), "the recorded identity matches itself");
        assert!(
            !still_owns_process(own, Some(real.saturating_add(1))),
            "a different start time is a different process, so a stale session must not signal it"
        );
        assert!(
            still_owns_process(own, None),
            "without a recorded identity the keeper does not refuse on a guess"
        );
    }
}
