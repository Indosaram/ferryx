//! Production keeper integration: which owner each session belongs to, the gates that decide it,
//! and the keeper-backed data path itself.
//!
//! Ownership is persisted per session in a durable record, never inferred from the running process,
//! because it has to outlive the daemon that wrote it. Three rules follow from that and are enforced
//! here rather than at each call site:
//!
//! * the enable flag routes **new** sessions only; a session keeps the owner it was created with,
//!   and a live FD migration is a separate operation that never happens as a side effect of a flip;
//! * disabling the flag leaves keeper-owned sessions attachable, so a flip cannot orphan them;
//! * a rollback to a daemon without keeper support is refused while any keeper-owned session
//!   exists, with a typed error naming the count: such a daemon cannot discover or service them.
//!
//! The keeper is a separate binary reached only across [`KeeperClient`]'s wire, so nothing here
//! links keeper code and no terminal output is ever parsed on the keeper side.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{RecvTimeoutError, SyncSender};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::daemon::keeper_client::{
    KeeperClient, KeeperClientError, KeeperConnectionPool, ReplayReply, KEEPER_MAX_PAYLOAD_BYTES,
};

/// Who owns a session's PTY master. Recorded per session and never re-derived.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionOwner {
    /// The keeper holds the master. The session survives an abnormal daemon exit.
    Keeper,
    /// The daemon holds the master, as it did before the keeper existed. Lost with the daemon.
    Daemon,
}

impl SessionOwner {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Keeper => "keeper",
            Self::Daemon => "daemon",
        }
    }
}

/// One session's durable ownership record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnershipRecord {
    pub session_id: String,
    pub owner: SessionOwner,
    /// The monotonic attach epoch this daemon holds for the session. Zero for direct-owned sessions,
    /// which have no keeper to fence.
    #[serde(default)]
    pub epoch: u64,
    #[serde(default)]
    pub cols: u16,
    #[serde(default)]
    pub rows: u16,
    /// The child pid the keeper reported at spawn, for the pid+start-time identity acceptance check.
    #[serde(default)]
    pub pid: u32,
    /// The child's kernel start time, so a pid the OS reused is not mistaken for this session.
    #[serde(default)]
    pub process_start_time: Option<u64>,
    /// The keeper generation that holds the master, so retention can refuse to delete a directory
    /// that still owns a session.
    #[serde(default)]
    pub generation: Option<String>,
    #[serde(default)]
    pub created_at: u64,
    /// The newest output offset a checkpoint covers, so a reattach knows what it may skip.
    ///
    /// `0` when no checkpoint is known, which is the only honest answer for a session merely
    /// adopted from discovery: it has a head, not a checkpoint.
    #[serde(default)]
    pub checkpoint_offset: u64,
}

/// The durable ownership table. A missing file is an empty table: a user who never enabled the
/// keeper has no records, which is not an error.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnershipTable {
    #[serde(default)]
    pub sessions: BTreeMap<String, OwnershipRecord>,
    /// The current enable flag, persisted next to the records so a restart cannot forget it.
    #[serde(default)]
    pub keeper_ownership_enabled: bool,
    /// The spawn request id each session was *first* dispatched under, keyed by session id.
    ///
    /// This is the durable half of spawn deduplication. The keeper retires a request id it has
    /// already forked, so a retry must present the same id it did the first time: a fresh uuid per
    /// attempt can never match that tombstone and, once the session has been killed, nothing stops
    /// the duplicate from forking a second child. Persisting the id here is what lets a retry after
    /// a daemon restart (the crash path this phase exists for) reuse it instead of minting again.
    ///
    /// Kept out of [`OwnershipRecord`] on purpose: the record is rewritten by reconcile and by
    /// checkpoint commits, and the request id must survive both untouched.
    #[serde(default)]
    pub spawn_requests: BTreeMap<String, String>,
}

impl OwnershipTable {
    pub fn owner_of(&self, session_id: &str) -> Option<SessionOwner> {
        self.sessions.get(session_id).map(|record| record.owner)
    }

    /// The request id a session's spawn was first dispatched under, when one was recorded.
    pub fn spawn_request_id(&self, session_id: &str) -> Option<&str> {
        self.spawn_requests.get(session_id).map(String::as_str)
    }

    /// Records the request id a spawn is dispatched under. First write wins: a retry of the same
    /// logical spawn must reuse the id already on disk, never replace it with a newer one.
    pub fn remember_spawn_request(&mut self, session_id: &str, request_id: &str) {
        self.spawn_requests
            .entry(session_id.to_string())
            .or_insert_with(|| request_id.to_string());
    }

    pub fn keeper_owned(&self) -> Vec<&OwnershipRecord> {
        self.sessions
            .values()
            .filter(|record| record.owner == SessionOwner::Keeper)
            .collect()
    }

    pub fn keeper_owned_count(&self) -> usize {
        self.keeper_owned().len()
    }

    pub fn record(&mut self, record: OwnershipRecord) {
        self.sessions.insert(record.session_id.clone(), record);
    }

    pub fn forget(&mut self, session_id: &str) -> Option<OwnershipRecord> {
        self.spawn_requests.remove(session_id);
        self.sessions.remove(session_id)
    }

    /// The owner a **new** session gets under the current flag. This is the only place the flag is
    /// consulted for routing, which is what keeps it a new-session-only rule.
    pub fn new_session_owner(&self) -> SessionOwner {
        if self.keeper_ownership_enabled {
            SessionOwner::Keeper
        } else {
            SessionOwner::Daemon
        }
    }
}

/// Why an ownership-table operation could not be completed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnershipError {
    /// The durable path could not be resolved (no data directory).
    NoDataDir,
    /// The durable file could not be read or written. The previous content is left in place.
    Io(String),
    /// The file parsed but its schema is not one this build understands.
    Corrupt(String),
}

impl std::fmt::Display for OwnershipError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoDataDir => write!(f, "no durable data directory for PTY ownership"),
            Self::Io(detail) => write!(f, "PTY ownership store io error: {detail}"),
            Self::Corrupt(detail) => write!(f, "PTY ownership store is unreadable: {detail}"),
        }
    }
}

impl std::error::Error for OwnershipError {}

/// A refusal to change the keeper-ownership flag or roll the daemon back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnershipRefusal {
    /// A rollback to a build without keeper support was requested while keeper sessions are live.
    /// `count` is the exact number the refusal names, so the operator can see what is at stake.
    RollbackWithKeeperSessions { count: usize },
    /// The flag asked for a state the store cannot reach.
    Unsupported { detail: String },
}

impl OwnershipRefusal {
    pub fn message(&self) -> String {
        match self {
            Self::RollbackWithKeeperSessions { count } => format!(
                "ROLLBACK_REFUSED_KEEPER_SESSIONS: {count} session(s) are owned by the PTY keeper; \
                 a daemon without keeper support cannot discover or service them, so the rollback \
                 is refused until those sessions end"
            ),
            Self::Unsupported { detail } => format!("OWNERSHIP_UNSUPPORTED: {detail}"),
        }
    }
}

impl std::fmt::Display for OwnershipRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for OwnershipRefusal {}

/// Name of the durable ownership file inside the private data directory.
pub const OWNERSHIP_FILE_NAME: &str = "pty-ownership.json";

/// The keeper's wire code for "this suffix does not fit one frame", which sends a reattach to the
/// framed `REPLAY` path instead of the inline one.
pub const FRAME_TOO_LARGE_CODE: &str = "FRAME_TOO_LARGE";

/// One page of a checkpoint download, matching the keeper's own clamp.
const CHECKPOINT_FETCH_PAGE_BYTES: u64 = 512 * 1024;

fn keeper_signal_name(signal: crate::terminal::TerminalSignal) -> &'static str {
    use crate::terminal::TerminalSignal as S;
    match signal {
        S::Interrupt => "interrupt",
        S::Terminate => "terminate",
        S::Kill => "kill",
        S::Stop => "stop",
        S::Continue => "continue",
    }
}

/// The durable ownership store: one file, atomic replace, owner-only mode.
pub struct OwnershipStore {
    path: PathBuf,
}

impl OwnershipStore {
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// `~/.ferryx/pty-ownership.json`, beside the daemon's other durable state.
    pub fn for_runtime() -> Result<Self, OwnershipError> {
        let base = crate::daemon::logging::logs_dir()
            .map_err(|_| OwnershipError::NoDataDir)?
            .parent()
            .map(Path::to_path_buf)
            .ok_or(OwnershipError::NoDataDir)?;
        Ok(Self::at(base.join(OWNERSHIP_FILE_NAME)))
    }

    /// A table inside `runtime_dir` itself. Isolation is a different *binding*, never a disabled
    /// feature: an isolated runtime activates ownership in its own directory and leaves the user's
    /// production table untouched.
    pub fn in_runtime_dir(runtime_dir: impl AsRef<Path>) -> Self {
        Self::at(runtime_dir.as_ref().join(OWNERSHIP_FILE_NAME))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<OwnershipTable, OwnershipError> {
        match fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|error| OwnershipError::Corrupt(error.to_string())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(OwnershipTable::default())
            }
            Err(error) => Err(OwnershipError::Io(error.to_string())),
        }
    }

    /// Publishes the table atomically: a reader either sees the previous document or this one.
    pub fn save(&self, table: &OwnershipTable) -> Result<(), OwnershipError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|error| OwnershipError::Io(error.to_string()))?;
        }
        let json = serde_json::to_vec_pretty(table)
            .map_err(|error| OwnershipError::Io(error.to_string()))?;
        let tmp = self.path.with_extension(format!(
            "tmp-{}-{}",
            std::process::id(),
            now_millis()
        ));
        fs::write(&tmp, &json).map_err(|error| OwnershipError::Io(error.to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600));
        }
        fs::rename(&tmp, &self.path).map_err(|error| OwnershipError::Io(error.to_string()))
    }

    /// Records one session's owner and persists it.
    pub fn record(&self, record: OwnershipRecord) -> Result<OwnershipTable, OwnershipError> {
        let mut table = self.load()?;
        table.record(record);
        self.save(&table)?;
        Ok(table)
    }

    pub fn forget(&self, session_id: &str) -> Result<OwnershipTable, OwnershipError> {
        let mut table = self.load()?;
        table.forget(session_id);
        self.save(&table)?;
        Ok(table)
    }

    /// Flips the enable flag. Existing sessions keep their recorded owner: the flip is a routing
    /// rule for new sessions, never a migration.
    pub fn set_keeper_enabled(&self, enabled: bool) -> Result<OwnershipTable, OwnershipError> {
        let mut table = self.load()?;
        table.keeper_ownership_enabled = enabled;
        self.save(&table)?;
        Ok(table)
    }

    /// The rollback gate. Refused, with the exact count, while any keeper-owned session is recorded.
    pub fn refuse_rollback_if_keeper_sessions(&self) -> Result<(), OwnershipRefusal> {
        let table = self.load().map_err(|error| OwnershipRefusal::Unsupported {
            detail: error.to_string(),
        })?;
        let count = table.keeper_owned_count();
        if count > 0 {
            return Err(OwnershipRefusal::RollbackWithKeeperSessions { count });
        }
        Ok(())
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

/// Checkpoint cadence: at most one checkpoint per 2 MiB of session output or 60 s, whichever first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckpointCadence {
    pub max_bytes: u64,
    pub max_interval: Duration,
}

impl Default for CheckpointCadence {
    fn default() -> Self {
        Self {
            max_bytes: 2 * 1024 * 1024,
            max_interval: Duration::from_secs(60),
        }
    }
}

/// The first wait after a checkpoint install failed. A failed install must not be retried on the
/// next frame: that re-serializes the whole terminal state and re-uploads it, which under budget
/// pressure turns one refusal into a per-frame hot loop.
pub const CHECKPOINT_RETRY_BASE: Duration = Duration::from_secs(5);
/// The longest wait between retries of a failing checkpoint.
pub const CHECKPOINT_RETRY_MAX: Duration = Duration::from_secs(300);
/// How many consecutive retryable failures are absorbed with a growing wait before the cadence goes
/// quiet. Recovery continues on the last committed checkpoint plus the replay log throughout.
pub const CHECKPOINT_FAILURES_BEFORE_QUIET: u32 = 6;

/// The wait before the next attempt after `failures` consecutive failures: an exponential backoff
/// off a floor, capped at a ceiling.
///
/// The floor is [`CHECKPOINT_RETRY_BASE`] or an eighth of the cadence, whichever is longer: a longer
/// cadence means a longer tolerable gap, and retrying faster than that would defeat the cadence the
/// plan set. The ceiling is [`CHECKPOINT_RETRY_MAX`], but **never below the floor** -- clamping a
/// long cadence's floor down to the cap would retry *faster* than the cadence, which is the exact
/// hot loop this backoff exists to prevent.
///
/// Typed and pure, so the retry rate is testable without a clock, a keeper or a sleep.
pub fn checkpoint_retry_delay(failures: u32, cadence: CheckpointCadence) -> Duration {
    if failures == 0 {
        return Duration::ZERO;
    }
    let floor = CHECKPOINT_RETRY_BASE.max(cadence.max_interval / 8);
    let ceiling = CHECKPOINT_RETRY_MAX.max(floor);
    if failures >= CHECKPOINT_FAILURES_BEFORE_QUIET {
        return ceiling;
    }
    floor
        .saturating_mul(1u32 << (failures - 1).min(6))
        .min(ceiling)
}

/// Per-session cadence state: how many bytes and how long since the last committed checkpoint.
#[derive(Debug, Clone)]
pub struct CheckpointClock {
    cadence: CheckpointCadence,
    bytes_since: u64,
    last_at: Instant,
    committed_offset: u64,
    /// Consecutive failed installs. Reset by any commit, and what the retry backoff is derived from.
    consecutive_failures: u32,
    /// Nothing is attempted before this instant. `None` means the cadence alone governs.
    retry_after: Option<Instant>,
}

impl CheckpointClock {
    pub fn new(cadence: CheckpointCadence, now: Instant) -> Self {
        Self {
            cadence,
            bytes_since: 0,
            last_at: now,
            committed_offset: 0,
            consecutive_failures: 0,
            retry_after: None,
        }
    }

    pub fn committed_offset(&self) -> u64 {
        self.committed_offset
    }

    pub fn consecutive_failures(&self) -> u32 {
        self.consecutive_failures
    }

    /// Whether the backoff (rather than the cadence) is currently holding attempts back.
    fn ready(&self, now: Instant) -> bool {
        match self.retry_after {
            Some(retry_at) => now >= retry_at,
            None => true,
        }
    }

    /// Counts produced bytes and reports whether a checkpoint is now due.
    pub fn observe(&mut self, produced: u64, now: Instant) -> bool {
        self.bytes_since = self.bytes_since.saturating_add(produced);
        self.ready(now)
            && (self.bytes_since >= self.cadence.max_bytes
                || now.saturating_duration_since(self.last_at) >= self.cadence.max_interval)
    }

    /// Records a committed checkpoint at `offset` and resets both counters.
    pub fn committed(&mut self, offset: u64, now: Instant) {
        self.committed_offset = offset;
        self.bytes_since = 0;
        self.last_at = now;
        self.consecutive_failures = 0;
        self.retry_after = None;
    }

    /// Records that a checkpoint was due but there was nothing new to commit: no serialized state
    /// yet, or a state that has not advanced past the committed offset.
    ///
    /// Neither is a failure, but both thresholds still reset. Leaving `bytes_since` over the limit
    /// would re-serialize the state on every frame while a session produces nothing committable.
    pub fn postponed(&mut self, now: Instant) {
        self.bytes_since = 0;
        self.last_at = now;
    }

    /// Records that a checkpoint was *attempted* and did not commit, so neither counter keeps
    /// tripping on the same accumulated output.
    ///
    /// Without this a failed install (the keeper's `CHECKPOINT_BUDGET_EXHAUSTED` is sanctioned by
    /// the plan) leaves `bytes_since` above the threshold, and the next frame re-serializes the
    /// whole state and re-uploads up to the cap again: a per-frame hot loop under budget pressure.
    /// `committed_offset` is deliberately untouched, so recovery still resumes from the last state
    /// that really committed and the replay log covers the rest.
    ///
    /// `retryable` comes from the keeper's typed wire code, never from its message. A refusal that
    /// cannot improve (a stale checkpoint, a fenced epoch) still waits out the cadence -- a hot loop
    /// is never the answer -- but is treated as settled at once instead of climbing the backoff.
    pub fn skipped(&mut self, now: Instant, retryable: bool) {
        self.bytes_since = 0;
        self.last_at = now;
        self.consecutive_failures = if retryable {
            self.consecutive_failures.saturating_add(1)
        } else {
            CHECKPOINT_FAILURES_BEFORE_QUIET
        };
        self.retry_after =
            now.checked_add(checkpoint_retry_delay(self.consecutive_failures, self.cadence));
    }

    /// Whether the wall-clock half of the cadence has elapsed since the last commit or skip.
    ///
    /// This exists so an idle session can checkpoint on time. It is evaluated by a timer that is
    /// independent of frame reads: a socket read timeout would have to resume a frame whose header
    /// or payload was only partly read, and reframing from the middle of a frame desynchronizes the
    /// protocol permanently.
    pub fn interval_elapsed(&self, now: Instant) -> bool {
        self.ready(now)
            && now.saturating_duration_since(self.last_at) >= self.cadence.max_interval
    }
}

/// What one bounded tail read produced.
struct TailRead {
    bytes: Vec<u8>,
    end: u64,
    gap: Option<ReplayGapKind>,
}

/// Where a recovery must start reading, and whether that is past the caller's own offset.
///
/// The keeper's log is authoritative and usually far longer than anything one recovery can hand a
/// reader, whose hub ring is `tail_bytes`. Starting at the caller's offset reads the oldest retained
/// bytes and leaves the ring evicting its own tail as the pump pages forward -- so the history a
/// client is served is a prefix of the stream that ends before the bytes it cares about. Starting at
/// `head - tail_bytes` reads exactly what the ring keeps, so one chunk covers the whole of it.
///
/// `skipped` says the caller's offset was before that window. Nothing was lost: the prefix was never
/// going to fit. The caller still records that its history begins mid-stream.
fn recovery_read_start(after_sequence: u64, head: u64, tail_bytes: u64) -> (u64, bool) {
    let window_start = head.saturating_sub(tail_bytes);
    if after_sequence >= window_start {
        return (after_sequence, false);
    }
    (window_start, true)
}

/// What a reattach recovered, and whether the keeper had to drop bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveredSession {
    pub session_id: String,
    pub epoch: u64,
    /// The offset the replay/checkpoint data ends at; the next read resumes here.
    pub sequence_offset: u64,
    /// The committed checkpoint the keeper still holds, if any.
    pub checkpoint_id: Option<String>,
    /// The checkpoint's own bytes: the serialized terminal state the retained suffix continues
    /// from. Absent when the keeper holds no checkpoint, or nothing has committed one yet.
    pub checkpoint_bytes: Option<Vec<u8>>,
    /// Bytes replayed after the checkpoint, in order.
    pub replayed: Vec<u8>,
    /// Set when the retained log no longer reached the caller's offset. `replayed` is then empty
    /// and the caller **must** reset its parser before consuming any later byte: this is the
    /// plan's explicitly lossy reset, not a preserved display state.
    pub replay_gap: Option<ReplayGapKind>,
    /// The caller's offset was below the tail this recovery read, so the bytes before the tail were
    /// never read at all. Distinct from [`Self::replay_gap`]: nothing was *lost* by the keeper -- the
    /// prefix was simply beyond what one recovery can retain -- so no parser reset is due, only a
    /// note that this hub's history begins mid-stream.
    pub prefix_skipped: bool,
}

/// The gap offsets, kept separate from `keeper_client::ReplayGap` so this module's public surface
/// does not leak the wire type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplayGapKind {
    pub requested_after_sequence: u64,
    pub available_from_sequence: u64,
}

impl RecoveredSession {
    /// True when the caller must reset its terminal parser before applying later output.
    pub fn requires_parser_reset(&self) -> bool {
        self.replay_gap.is_some()
    }
}

/// Why a keeper-backed session operation failed.
#[derive(Debug)]
pub enum KeeperSessionError {
    /// The keeper connection failed or refused. The keeper's code is preserved verbatim.
    Keeper(KeeperClientError),
    /// No keeper generation is installed or none can serve this daemon. The existing keeper and its
    /// sessions are preserved: nothing here replaces or prunes one.
    Incompatible(String),
    /// The session is not owned by the keeper, so no keeper verb may act on it.
    NotKeeperOwned { session_id: String },
    /// The keeper accepted only part of a write. The remaining bytes are not in the PTY, so the
    /// caller is told instead of being handed a success that produces no echo.
    ShortWrite {
        session_id: String,
        accepted: usize,
        offered: usize,
    },
}

impl std::fmt::Display for KeeperSessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Keeper(error) => write!(f, "{error}"),
            Self::Incompatible(detail) => write!(f, "keeper incompatible: {detail}"),
            Self::NotKeeperOwned { session_id } => {
                write!(f, "session {session_id} is not owned by the keeper")
            }
            Self::ShortWrite { session_id, accepted, offered } => write!(
                f,
                "session {session_id}: the keeper accepted {accepted} of {offered} input bytes"
            ),
        }
    }
}

impl std::error::Error for KeeperSessionError {}

impl From<KeeperClientError> for KeeperSessionError {
    fn from(error: KeeperClientError) -> Self {
        Self::Keeper(error)
    }
}

impl KeeperSessionError {
    /// Whether retrying the same operation could succeed, decided from typed values only.
    ///
    /// Never from a message: the keeper's refusals carry a wire code, and the codes that describe a
    /// condition which will not improve (a fenced epoch, a stale checkpoint, a frame this client
    /// built wrong) are exactly the ones a retry loop must not keep paying for.
    pub fn is_transient(&self) -> bool {
        match self {
            // A dropped or refused connection may come back; a protocol mismatch cannot.
            Self::Keeper(KeeperClientError::Io(_)) => true,
            Self::Keeper(KeeperClientError::Protocol(_)) => false,
            Self::Keeper(error @ KeeperClientError::Refused { .. }) => {
                !error.is_epoch_mismatch() && !error.is_stale_checkpoint()
            }
            // A missing generation, an unowned session or a truncated write is not fixed by
            // trying again: the bytes are gone either way, and the caller must hear about it.
            Self::Incompatible(_) | Self::NotKeeperOwned { .. } | Self::ShortWrite { .. } => false,
        }
    }
}

/// A fresh request id for one **logical** spawn.
///
/// Minted where the spawn begins and then reused by every retry of that spawn, including a retry
/// after a daemon restart (it is persisted in [`OwnershipTable::spawn_requests`]). The keeper
/// retires a request id that already forked a session, so reusing the id is the only way its
/// tombstone can recognise the retry as the same spawn.
pub fn mint_spawn_request_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// The spec a new keeper-owned session is spawned from: exactly what a keeper `SPAWN` carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeeperSpawnSpec {
    pub session_id: String,
    pub argv: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: Option<PathBuf>,
    pub cols: u16,
    pub rows: u16,
    /// The idempotency key of this logical spawn, stable across every retry of it.
    pub request_id: String,
}

impl KeeperSpawnSpec {
    /// Builds the spec from the command a caller already composed for a direct PTY, so the keeper
    /// runs the same program, arguments, environment and working directory the daemon would have.
    ///
    /// The environment is the full one the command builder holds, not only the variables this
    /// daemon set: the keeper starts the child from its own environment, which is launchd's and not
    /// this process's, so a base-env-only difference would drop PATH, TERM and the locale that
    /// `pty.rs` deliberately arranges for every pane.
    pub fn from_command(
        session_id: impl Into<String>,
        cmd: &portable_pty::CommandBuilder,
        cols: u16,
        rows: u16,
    ) -> Self {
        Self::from_command_with_request_id(session_id, cmd, cols, rows, mint_spawn_request_id())
    }

    /// The same spec under a caller-supplied request id, which is what a retry of an already
    /// dispatched spawn passes so the keeper can recognise it as that spawn and not a new one.
    pub fn from_command_with_request_id(
        session_id: impl Into<String>,
        cmd: &portable_pty::CommandBuilder,
        cols: u16,
        rows: u16,
        request_id: impl Into<String>,
    ) -> Self {
        let argv: Vec<String> = cmd
            .get_argv()
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        let env: Vec<(String, String)> = cmd
            .iter_full_env_as_str()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        let cwd = cmd.get_cwd().map(PathBuf::from);
        Self {
            session_id: session_id.into(),
            argv,
            env,
            cwd,
            cols,
            rows,
            request_id: request_id.into(),
        }
    }

    /// Returns this spec carrying `request_id`, for a caller that learns the durable id only after
    /// the spec was built from the command.
    pub fn with_request_id(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = request_id.into();
        self
    }
}

/// The daemon-side keeper session host. Every method is a real keeper verb; nothing is simulated.
///
/// Every verb is issued on a connection borrowed from [`Self::pool`], never on a connection of its
/// own. The keeper refuses the ninth simultaneous peer, so a host that opened a connection per call
/// (or per session) would put the daemon's own session count against a keeper cap it cannot raise.
/// One pool per runtime keeps the daemon's footprint constant instead.
pub struct KeeperSessionHost {
    runtime_dir: PathBuf,
    pool: Arc<KeeperConnectionPool>,
}

impl KeeperSessionHost {
    pub fn new(runtime_dir: impl Into<PathBuf>) -> Self {
        let runtime_dir = runtime_dir.into();
        let pool = Arc::new(KeeperConnectionPool::new(runtime_dir.clone()));
        Self { runtime_dir, pool }
    }

    /// A host over an existing pool, for a component that must share the runtime's bounded
    /// connection budget instead of opening a second set.
    pub fn sharing_pool(&self) -> Self {
        Self {
            runtime_dir: self.runtime_dir.clone(),
            pool: Arc::clone(&self.pool),
        }
    }

    pub fn for_runtime() -> Self {
        Self::new(crate::daemon::server::get_runtime_dir())
    }

    /// The bounded connections every verb of this host is issued on.
    pub fn pool(&self) -> &Arc<KeeperConnectionPool> {
        &self.pool
    }

    fn with_client<T>(
        &self,
        verb: impl FnOnce(&mut KeeperClient) -> Result<T, KeeperClientError>,
    ) -> Result<T, KeeperSessionError> {
        self.pool.with_client(verb).map_err(KeeperSessionError::Keeper)
    }

    /// Spawns a keeper-owned session and records its ownership durably. The spawn is idempotent on
    /// the spec's request id, so a retry after a lost reply returns the existing PTY.
    ///
    /// The request id is written to the durable table **before** the frame is sent, and an id
    /// already on disk wins over the one in `spec`. That ordering is what makes a retry safe across
    /// a daemon restart: the retry reads the id the first attempt dispatched under instead of
    /// minting a new one the keeper's tombstone cannot recognise.
    pub fn spawn(
        &self,
        store: &OwnershipStore,
        spec: &KeeperSpawnSpec,
        epoch: u64,
    ) -> Result<OwnershipRecord, KeeperSessionError> {
        let request_id = self.record_spawn_request(store, &spec.session_id, &spec.request_id)?;
        // The fenced-by-request-id form, always. `spawn` mints an id per call, which is correct
        // only for a spawn that cannot be retried; this path can be retried all the way across a
        // daemon restart, so it must present the id the first attempt dispatched under.
        let reply = self.with_client(|client| {
            client.spawn_with_request_id(
                &spec.session_id,
                &spec.argv,
                &spec.env,
                spec.cwd.as_deref(),
                spec.cols,
                spec.rows,
                epoch,
                Some(request_id.as_str()),
            )
        })?;
        let record = OwnershipRecord {
            session_id: reply.session_id.clone(),
            owner: SessionOwner::Keeper,
            epoch: reply.epoch,
            cols: reply.cols,
            rows: reply.rows,
            pid: reply.pid,
            process_start_time: reply.process_start_time,
            generation: None,
            created_at: now_millis(),
            checkpoint_offset: reply.sequence_offset,
        };
        store.record(record.clone()).map_err(|error| {
            KeeperSessionError::Incompatible(format!("ownership not persisted: {error}"))
        })?;
        Ok(record)
    }

    /// Records the request id this spawn dispatches under and returns the id to actually send.
    ///
    /// A crash between this write and the reply leaves the id on disk with a `Reserved` admission
    /// slot, which is exactly the state a retry must resume from: it re-sends the same id, so the
    /// keeper answers from the child it already forked or refuses the retired id by name. Neither
    /// outcome forks a second child.
    fn record_spawn_request(
        &self,
        store: &OwnershipStore,
        session_id: &str,
        request_id: &str,
    ) -> Result<String, KeeperSessionError> {
        let mut table = store.load().map_err(|error| {
            KeeperSessionError::Incompatible(format!("ownership not readable: {error}"))
        })?;
        if let Some(recorded) = table.spawn_request_id(session_id) {
            return Ok(recorded.to_string());
        }
        table.remember_spawn_request(session_id, request_id);
        store.save(&table).map_err(|error| {
            KeeperSessionError::Incompatible(format!("spawn request not persisted: {error}"))
        })?;
        Ok(request_id.to_string())
    }

    pub fn write_input(
        &self,
        session_id: &str,
        epoch: u64,
        data: &[u8],
    ) -> Result<u64, KeeperSessionError> {
        self.with_client(|client| client.write(session_id, epoch, data))
    }

    pub fn resize(
        &self,
        session_id: &str,
        epoch: u64,
        cols: u16,
        rows: u16,
    ) -> Result<(), KeeperSessionError> {
        self.with_client(|client| client.resize(session_id, epoch, cols, rows))
    }

    pub fn signal(
        &self,
        session_id: &str,
        epoch: u64,
        signal: &str,
    ) -> Result<(), KeeperSessionError> {
        self.with_client(|client| client.signal(session_id, epoch, signal))
    }

    /// Ends the session's child and forgets it, then drops the durable record.
    pub fn close(
        &self,
        store: &OwnershipStore,
        session_id: &str,
        epoch: u64,
    ) -> Result<(), KeeperSessionError> {
        self.with_client(|client| client.kill(session_id, epoch))?;
        store.forget(session_id).map_err(|error| {
            KeeperSessionError::Incompatible(format!("ownership not cleared: {error}"))
        })?;
        Ok(())
    }

    /// Reads one page of the session's output from `from_sequence`, fenced at `epoch`.
    ///
    /// This is the whole live-output path: the keeper answers with the retained bytes the reader has
    /// not taken yet, or a gap event when the log no longer reaches that offset. Polling it from a
    /// small shared pool is what keeps the daemon's keeper connections bounded, where a permanent
    /// tap per session made the keeper's connection cap the daemon's session cap.
    pub fn replay_page(
        &self,
        session_id: &str,
        epoch: u64,
        from_sequence: u64,
    ) -> Result<ReplayReply, KeeperSessionError> {
        self.with_client(|client| client.replay_fenced(session_id, epoch, from_sequence))
    }

    /// Attaches the session at `epoch` and recovers everything the caller missed: the committed
    /// checkpoint, then the retained suffix the attach carried, or an explicit replay gap.
    ///
    /// One attach does all of it. The keeper fences the epoch on `ATTACH` and refuses anything at or
    /// below the newest one, so this is the call that establishes which reader owns the session. It
    /// deliberately asks for **no** live tap: the connection goes back to the pool as soon as the
    /// answer is read, and output is drained from then on with [`Self::replay_page`], so N sessions
    /// cost a bounded number of keeper connections instead of one each.
    pub fn attach_and_recover(
        &self,
        session_id: &str,
        epoch: u64,
        after_sequence: u64,
        tail_bytes: Option<u64>,
    ) -> Result<RecoveredSession, KeeperSessionError> {
        let (reply, replayed, delivered) = self.with_client(|client| {
            client.attach_recovering(session_id, epoch, after_sequence)
        })?;
        let checkpoint_bytes = self.fetch_checkpoint(session_id, epoch, &reply);
        let gap = reply.gap.map(|gap| ReplayGapKind {
            requested_after_sequence: gap.requested_after_sequence,
            available_from_sequence: gap.available_from_sequence,
        });
        // The attach answers with the session head and one page of the suffix. When the caller's
        // offset is further back than a single recovery can retain, that page is the *wrong* page:
        // it is the oldest retained bytes, and the ring would evict most of everything the pump
        // added after it. Reading the tail instead makes the first chunk the pump publishes the
        // whole history a reader can be served, so the first attach after recovery is already
        // correct rather than a prefix that only becomes the history once the last page lands.
        //
        // Only without a checkpoint. A restored checkpoint is the terminal state the *next* byte
        // continues from, so the feed must start exactly where it ends; skipping part of that
        // suffix would apply post-skip bytes to state that never saw them. With no checkpoint the
        // parser starts empty, and a tail it parses from its first byte is a history that begins
        // mid-stream rather than a corrupted one.
        if gap.is_none() && checkpoint_bytes.is_none() {
            if let Some(tail) = tail_bytes {
                let (start, skipped) = recovery_read_start(after_sequence, reply.sequence_offset, tail);
                if skipped {
                    let tail = self.read_tail(session_id, epoch, start, reply.sequence_offset)?;
                    return Ok(RecoveredSession {
                        session_id: reply.session_id,
                        epoch: reply.epoch,
                        sequence_offset: tail.end,
                        checkpoint_id: reply.checkpoint_id,
                        checkpoint_bytes,
                        replayed: tail.bytes,
                        replay_gap: tail.gap,
                        prefix_skipped: tail.gap.is_none(),
                    });
                }
            }
        }
        Ok(RecoveredSession {
            session_id: reply.session_id,
            epoch: reply.epoch,
            sequence_offset: delivered,
            checkpoint_id: reply.checkpoint_id,
            checkpoint_bytes,
            replayed,
            replay_gap: gap,
            prefix_skipped: false,
        })
    }

    /// Reads the recovered tail from `start`, following `more` until the keeper has nothing past the
    /// page or `head` is reached.
    ///
    /// Bounded by `head`, not by a page count: the keeper pages in `REPLAY_CHUNK_BYTES` steps, and a
    /// recovery has already decided how much it wants. A gap inside the tail is surfaced rather than
    /// partly applied, because a partial history would be indistinguishable from a complete one.
    fn read_tail(
        &self,
        session_id: &str,
        epoch: u64,
        start: u64,
        head: u64,
    ) -> Result<TailRead, KeeperSessionError> {
        let mut bytes = Vec::new();
        let mut offset = start;
        loop {
            let page = self.replay_page(session_id, epoch, offset)?;
            if let Some(gap) = page.gap {
                return Ok(TailRead {
                    bytes: Vec::new(),
                    end: gap.available_from_sequence,
                    gap: Some(ReplayGapKind {
                        requested_after_sequence: gap.requested_after_sequence,
                        available_from_sequence: gap.available_from_sequence,
                    }),
                });
            }
            let mut forwarded = 0u64;
            for (sequence_offset, data) in &page.chunks {
                let data = decode_base64(data);
                if data.is_empty() {
                    continue;
                }
                offset = sequence_offset.saturating_add(data.len() as u64);
                forwarded = forwarded.saturating_add(data.len() as u64);
                bytes.extend_from_slice(&data);
            }
            // No progress and nothing past this point: the keeper has no more retained output, so
            // the tail is what has been read.
            if forwarded == 0 || !page.more || offset >= head {
                return Ok(TailRead { bytes, end: offset, gap: None });
            }
        }
    }

    /// Reads the committed checkpoint's bytes, which are the terminal state the retained suffix
    /// continues from. A missing checkpoint is not a failure: the suffix and a gap cover the
    /// session, and a fatal error here would refuse a recovery the keeper can still serve.
    fn fetch_checkpoint(
        &self,
        session_id: &str,
        epoch: u64,
        reply: &crate::daemon::keeper_client::AttachReply,
    ) -> Option<Vec<u8>> {
        let checkpoint_id = reply.checkpoint_id.as_deref()?;
        let sequence_offset = reply.checkpoint_sequence_offset;
        self.with_client(|client| {
            client.checkpoint_fetch(
                session_id,
                epoch,
                checkpoint_id,
                sequence_offset,
                CHECKPOINT_FETCH_PAGE_BYTES,
            )
        })
        .ok()
    }

    /// Installs a daemon-produced checkpoint at `offset`, chunking it when it exceeds the frame cap
    /// so installation stays atomic and a partial upload is discarded by the keeper.
    ///
    /// Abandonment is structural, not a verb. The whole install runs on **one** pooled connection,
    /// and any failure throws that connection away instead of returning it to the pool: the keeper
    /// scopes staged uploads to the connection (`serve_connection` builds its `OpenUploads` from the
    /// shared keeper state and drops it when the connection ends, which aborts every upload that
    /// did not reach `CHECKPOINT_COMMIT`). So a mid-chunk failure cannot leave staging behind for a
    /// *later* attempt to collide with, and no cancel frame is needed here. A committed upload has
    /// nothing left to abort, so reusing the connection is safe on the success path.
    pub fn install_checkpoint(
        &self,
        session_id: &str,
        epoch: u64,
        checkpoint_id: &str,
        offset: u64,
        bytes: &[u8],
    ) -> Result<u64, KeeperSessionError> {
        self.with_client(|client| {
            if bytes.len() <= KEEPER_MAX_PAYLOAD_BYTES as usize / 2 {
                let reply = client.checkpoint_install(
                    session_id,
                    epoch,
                    checkpoint_id,
                    offset,
                    bytes.len() as u64,
                    Some(bytes),
                )?;
                return Ok(reply.stored_bytes);
            }
            client.checkpoint_install(
                session_id,
                epoch,
                checkpoint_id,
                offset,
                bytes.len() as u64,
                None,
            )?;
            let mut stored = 0;
            for (index, chunk) in bytes
                .chunks(KEEPER_MAX_PAYLOAD_BYTES as usize / 2)
                .enumerate()
            {
                let reply = client.checkpoint_chunk(
                    session_id,
                    epoch,
                    checkpoint_id,
                    index as u32,
                    chunk,
                )?;
                stored = reply.stored_bytes;
            }
            let committed =
                client.checkpoint_commit(session_id, epoch, checkpoint_id, offset)?;
            Ok(committed.stored_bytes.max(stored))
        })
    }

    /// The offset the keeper's committed checkpoint covers, or `0` when it holds none.
    ///
    /// Deliberately unfenced: this is asked *before* the `ATTACH` that fences the epoch, so it must
    /// not present one. The keeper answers `0` for a session with no committed checkpoint
    /// (see `CheckpointInfo`), which is the same answer its log would give for a replay from the
    /// floor.
    pub fn committed_checkpoint_offset(&self, session_id: &str) -> Result<u64, KeeperSessionError> {
        let reply = self.with_client(|client| client.checkpoint_query(session_id))?;
        Ok(reply.sequence_offset)
    }

    /// Releases the attachment without ending the session, leaving the PTY for the successor.
    pub fn detach(&self, session_id: &str, epoch: u64) -> Result<(), KeeperSessionError> {
        self.with_client(|client| client.detach(session_id, epoch))
    }

    /// Authoritative discovery. The keeper is the source of truth for which sessions exist, so this
    /// is what reconciles a restarting daemon's view instead of a persisted list alone.
    pub fn discover(&self) -> Result<Vec<KeeperSessionView>, KeeperSessionError> {
        let entries = self.with_client(|client| client.ownership())?;
        Ok(entries
            .into_iter()
            .map(|entry| KeeperSessionView {
                session_id: entry.session_id,
                pid: entry.pid,
                epoch: entry.epoch,
                sequence_offset: entry.sequence_offset,
                cols: entry.cols,
                rows: entry.rows,
                owner: entry.owner,
                generation: entry.generation,
            })
            .collect())
    }

    pub fn runtime_dir(&self) -> &Path {
        &self.runtime_dir
    }
}

/// One session as the keeper reports it during discovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeeperSessionView {
    pub session_id: String,
    pub pid: u32,
    pub epoch: u64,
    pub sequence_offset: u64,
    pub cols: u16,
    pub rows: u16,
    pub owner: String,
    /// The keeper generation that owns the session, as the keeper reports it. This is what makes
    /// "may this generation be deleted" answerable: retention must never remove a directory whose
    /// binary is still running someone's session.
    pub generation: String,
}

/// Everything the daemon learns at startup about keeper-held sessions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StartupDiscovery {
    pub sessions: Vec<KeeperSessionView>,
    /// Sessions the durable table expected but the keeper no longer holds: reported, never silently
    /// dropped, because their processes are gone with the keeper.
    pub vanished: Vec<String>,
}

impl StartupDiscovery {
    pub fn keeper_session_count(&self) -> usize {
        self.sessions.len()
    }
}

/// Reconciles the durable table against the keeper's authoritative list.
///
/// A keeper session missing from the keeper's list is reported in `vanished` and its record is
/// removed: the PTY is gone, so keeping the record would claim a survival that did not happen. A
/// session the keeper holds that the daemon has no record of is adopted as keeper-owned, because
/// the keeper's registry is authoritative.
pub fn reconcile_startup(
    store: &OwnershipStore,
    views: Vec<KeeperSessionView>,
) -> Result<StartupDiscovery, OwnershipError> {
    let mut table = store.load()?;
    let live: std::collections::BTreeSet<&str> =
        views.iter().map(|view| view.session_id.as_str()).collect();
    let vanished: Vec<String> = table
        .keeper_owned()
        .iter()
        .map(|record| record.session_id.clone())
        .filter(|session_id| !live.contains(session_id.as_str()))
        .collect();
    for session_id in &vanished {
        table.forget(session_id);
    }
    for view in &views {
        let entry = table
            .sessions
            .entry(view.session_id.clone())
            .or_insert_with(|| OwnershipRecord {
                session_id: view.session_id.clone(),
                owner: SessionOwner::Keeper,
                epoch: view.epoch,
                cols: view.cols,
                rows: view.rows,
                pid: view.pid,
                process_start_time: None,
                generation: Some(view.generation.clone()),
                created_at: now_millis(),
                // A discovery view reports the keeper's **head**: how much output exists, not how
                // much of it a serialized state covers. Recording the head here claimed a
                // checkpoint over the whole log, so the first reattach resumed at the head and the
                // keeper, holding nothing past it, had nothing left to send. That is the Darwin
                // run14b failure: `attached=1` with an empty history for the session's whole life
                // while the keeper still held all 720 KiB of it. A newly adopted record knows of no
                // checkpoint, and `0` is the value that says so -- the replay then starts at
                // whatever the keeper retains, and a real checkpoint, if one exists, arrives with
                // its own bytes and the gap that names where the suffix resumes.
                checkpoint_offset: 0,
            });
        entry.owner = SessionOwner::Keeper;
        entry.epoch = view.epoch;
        entry.cols = view.cols;
        entry.rows = view.rows;
        entry.pid = view.pid;
        entry.generation = Some(view.generation.clone());
    }
    store.save(&table)?;
    Ok(StartupDiscovery {
        sessions: views,
        vanished,
    })
}

/// The production startup pass: discover the keeper's sessions and reconcile the durable table.
///
/// Absent keeper is not an error at this level: a runtime with no keeper simply has no
/// keeper-owned sessions, and supervised restart stays off through `crash_supervisor`.
pub fn startup_discovery(store: &OwnershipStore) -> Result<StartupDiscovery, KeeperSessionError> {
    let host = KeeperSessionHost::for_runtime();
    let views = host.discover()?;
    reconcile_startup(store, views)
        .map_err(|error| KeeperSessionError::Incompatible(error.to_string()))
}

/// What one activation attempt did to the new-session routing flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivationOutcome {
    /// The flag now routes new sessions to the keeper, naming the keeper that proved live.
    Activated { keeper_pid: Option<u32> },
    /// The flag already routed new sessions to the keeper; nothing was rewritten.
    AlreadyActive,
    /// No keeper answered this runtime's socket, so the flag was left exactly as it was.
    NotProven,
    /// New sessions stay direct-owned on purpose, and `keeper_sessions` sessions the keeper still
    /// holds were discovered and attached anyway.
    ///
    /// This is the disable half of the flag, and it is deliberately a success rather than a refusal:
    /// the flag is a rule for *new* sessions only, so turning it off must not be read as "the keeper
    /// is not serving this runtime". Reporting the count is what makes the distinction auditable --
    /// a zero here means the disable stranded nothing, and a non-zero one names how much is still
    /// being served by the keeper after the flag was turned off.
    Suppressed { keeper_sessions: usize },
}

impl ActivationOutcome {
    /// Whether the flag routes new sessions to the keeper here.
    ///
    /// `Suppressed` answers `false` on purpose: supervised restart needs the flag *set*, and a
    /// disable that reported itself active would let the next startup pass believe durability is
    /// on for new sessions when the operator asked for the opposite.
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Activated { .. } | Self::AlreadyActive)
    }
}

/// Routes new sessions to the keeper, but only once `proof` proves a live keeper for this runtime.
///
/// `proof` must be bound to the same runtime as `store`; that binding is the isolation, not a
/// disabled path. An isolated runtime activates its own table against its own keeper, so a harness
/// exercises real keeper-owned sessions while the user's production table is never read or written.
///
/// Not proving a keeper is not an error and never clears the flag: a runtime that has lost sight of
/// its keeper for a moment must not silently re-route future panes to the weaker owner.
pub fn activate_ownership(
    store: &OwnershipStore,
    proof: &dyn crate::daemon::crash_supervisor::KeeperPresence,
) -> Result<ActivationOutcome, OwnershipError> {
    let status = proof.live_keeper();
    if !status.is_live() {
        return Ok(ActivationOutcome::NotProven);
    }
    let mut table = store.load()?;
    if table.keeper_ownership_enabled {
        return Ok(ActivationOutcome::AlreadyActive);
    }
    table.keeper_ownership_enabled = true;
    store.save(&table)?;
    Ok(ActivationOutcome::Activated {
        keeper_pid: status.pid(),
    })
}

fn decode_base64(text: &str) -> Vec<u8> {
    use base64::engine::general_purpose::STANDARD as BASE64;
    use base64::Engine as _;
    BASE64.decode(text).unwrap_or_default()
}

/// The checkpoint id one commit at `offset` uses.
///
/// Derived, not random, so a retried install of the same state names the same upload: the keeper
/// keys an open upload by `(session, checkpoint_id)`, and a retry under the same id releases the
/// superseded attempt instead of charging a second buffer against the aggregate cap.
pub fn checkpoint_id_for(epoch: u64, offset: u64) -> String {
    format!("cp-{epoch:x}-{offset:x}")
}

/// Runs one checkpoint attempt against the keeper: read the serialized state, name the offset that
/// state covers, and install it.
///
/// The offset comes from the same observation as the bytes (see [`CheckpointSource`]), converted
/// through `bias` and clamped to what this pump has actually delivered. A commit only advances the
/// session's durable offset; a failure records the backoff so the next frame does not immediately
/// re-serialize and re-upload the whole state.
fn run_checkpoint(
    ownership: &KeeperOwnership,
    checkpoints: &parking_lot::RwLock<Option<Arc<dyn CheckpointSource>>>,
    host: &KeeperSessionHost,
    session_id: &str,
    epoch: u64,
    delivered_offset: u64,
    bias: KeeperOffsetBias,
) {
    let now = Instant::now();
    let Some(source) = checkpoints.read().clone() else {
        // No producer installed: there is nothing to commit, and that is not a failure.
        ownership.checkpoint_postponed(session_id, now);
        return;
    };
    let Some(snapshot) = source.checkpoint_bytes(session_id) else {
        ownership.checkpoint_postponed(session_id, now);
        return;
    };
    // The hub can never have applied more than this pump delivered; clamping is what keeps a
    // mis-measured bias from committing an offset that covers bytes the state never saw.
    let offset = bias.apply(snapshot.applied_offset).min(delivered_offset);
    let committed = ownership.committed_offset(session_id);
    if offset == 0 || offset <= committed || snapshot.bytes.is_empty() {
        ownership.checkpoint_postponed(session_id, now);
        return;
    }
    let checkpoint_id = checkpoint_id_for(epoch, offset);
    match host.install_checkpoint(session_id, epoch, &checkpoint_id, offset, &snapshot.bytes) {
        Ok(_) => ownership.checkpoint_committed(session_id, offset, now),
        Err(error) => {
            let retryable = error.is_transient();
            let failures = ownership.checkpoint_skipped(session_id, now, retryable);
            tracing::warn!(
                session_id,
                offset,
                committed,
                failures,
                retryable,
                %error,
                "keeper checkpoint did not commit; continuing on the last committed state plus replay"
            );
        }
    }
}

/// One page of a session's output turned into the events the pump applies.
///
/// A page is what `REPLAY` answers: the retained bytes from a cursor, or a single gap when the log
/// no longer reaches it. Events keep the shape the pump matches on -- `Output { sequence_offset,
/// data }` and `Gap { .. }` -- in the order the keeper numbered them.
pub fn replay_events(reply: ReplayReply) -> Vec<AttachedEvent> {
    if let Some(gap) = reply.gap {
        // A gap carries no bytes: partial chunks are dropped by the client rather than passed off
        // as a complete history, so the parser is reset before anything else is applied.
        return vec![AttachedEvent::Gap {
            requested_after_sequence: gap.requested_after_sequence,
            available_from_sequence: gap.available_from_sequence,
        }];
    }
    reply
        .chunks
        .into_iter()
        .map(|(sequence_offset, data)| AttachedEvent::Output {
            sequence_offset,
            data: decode_base64(&data),
        })
        .filter(|event| match event {
            // A chunk that decodes to nothing is indistinguishable from no chunk, and queueing it
            // would move nothing forward while looking like progress.
            AttachedEvent::Output { data, .. } => !data.is_empty(),
            AttachedEvent::Gap { .. } => true,
        })
        .collect()
}

/// Translates the hub's cumulative published-byte counter into the keeper's output offset.
///
/// The two axes differ by a constant known only when the pump starts: the hub has published
/// `hub_base` bytes and the keeper's next byte is at `keeper_base`. After that every published byte
/// advances both by one, so the pair stays exact for the life of the pump. A checkpoint must name an
/// offset derived through this, never the pump's own read count: the pump runs ahead of the hub by
/// however much sits in the channel, and committing the pump's offset against a state that covers
/// less makes the keeper truncate replay past what the state knows, silently dropping those bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeeperOffsetBias {
    hub_base: u64,
    keeper_base: u64,
}

impl KeeperOffsetBias {
    pub fn new(hub_base: u64, keeper_base: u64) -> Self {
        Self {
            hub_base,
            keeper_base,
        }
    }

    /// The keeper offset of the byte just past the hub's `applied_offset`.
    pub fn apply(&self, applied_offset: u64) -> u64 {
        if applied_offset >= self.hub_base {
            self.keeper_base
                .saturating_add(applied_offset - self.hub_base)
        } else {
            // The hub went backwards (an import resets its counter). Nothing above the base is
            // covered by this pump, so the honest answer is the base itself.
            self.keeper_base
        }
    }
}

/// One thing a reader got out of the keeper's output: a page's bytes, or the gap that says the
/// keeper no longer holds what the reader asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachedEvent {
    /// Terminal bytes at `sequence_offset`, to be fed to the parser as-is.
    Output { sequence_offset: u64, data: Vec<u8> },
    /// The keeper dropped bytes past `available_from_sequence`, so the parser must be reset before
    /// the next `Output`. This is the plan's lossy reset, surfaced rather than hidden.
    Gap {
        requested_after_sequence: u64,
        available_from_sequence: u64,
    },
}

/// The epoch source for attaches. A process-wide counter seeded from the wall clock keeps successive
/// daemons strictly increasing, so a stalled predecessor cannot consume output after a successor
/// attached.
#[derive(Debug)]
pub struct EpochSource {
    next: AtomicU64,
}

impl EpochSource {
    pub fn seeded(value: u64) -> Self {
        Self {
            next: AtomicU64::new(value.max(1)),
        }
    }

    /// A new epoch, seeded from the clock so a fresh process still outranks its predecessor.
    pub fn new() -> Self {
        Self::seeded(now_millis().saturating_mul(1000))
    }

    pub fn acquire(&self) -> u64 {
        self.next.fetch_add(1, Ordering::SeqCst) + 1
    }
}

impl Default for EpochSource {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared state the daemon holds for the keeper path: the durable table, the epoch source and the
/// cadence clocks.
pub struct KeeperOwnership {
    store: OwnershipStore,
    epochs: EpochSource,
    clocks: parking_lot::Mutex<BTreeMap<String, CheckpointClock>>,
    cadence: CheckpointCadence,
}

impl KeeperOwnership {
    pub fn new(store: OwnershipStore, cadence: CheckpointCadence) -> Self {
        Self {
            store,
            epochs: EpochSource::new(),
            clocks: parking_lot::Mutex::new(BTreeMap::new()),
            cadence,
        }
    }

    pub fn store(&self) -> &OwnershipStore {
        &self.store
    }

    /// The epoch a new keeper session is spawned or attached at.
    pub fn next_epoch(&self) -> u64 {
        self.epochs.acquire()
    }

    /// Whether a new session goes to the keeper under the current durable flag.
    pub fn routes_new_sessions_to_keeper(&self) -> bool {
        self.store
            .load()
            .map(|table| table.new_session_owner() == SessionOwner::Keeper)
            .unwrap_or(false)
    }

    /// Counts output for a session and reports whether a checkpoint is due now.
    pub fn checkpoint_due(&self, session_id: &str, produced: u64, now: Instant) -> bool {
        let mut clocks = self.clocks.lock();
        clocks
            .entry(session_id.to_string())
            .or_insert_with(|| CheckpointClock::new(self.cadence, now))
            .observe(produced, now)
    }

    /// The newest offset a committed checkpoint for `session_id` covers.
    pub fn committed_offset(&self, session_id: &str) -> u64 {
        self.clocks
            .lock()
            .get(session_id)
            .map(CheckpointClock::committed_offset)
            .unwrap_or(0)
    }

    /// Records a failed attempt so the cadence waits a full threshold before retrying.
    ///
    /// `retryable` must come from [`KeeperClientError::wire_code`], never from a message. Returns
    /// how many consecutive failures this session has now accumulated.
    pub fn checkpoint_skipped(&self, session_id: &str, now: Instant, retryable: bool) -> u32 {
        let mut clocks = self.clocks.lock();
        let clock = clocks
            .entry(session_id.to_string())
            .or_insert_with(|| CheckpointClock::new(self.cadence, now));
        clock.skipped(now, retryable);
        clock.consecutive_failures()
    }

    /// Records that a checkpoint was due but had nothing new to commit. Not a failure: the retry
    /// backoff is left exactly as it was, only the cadence counters reset.
    pub fn checkpoint_postponed(&self, session_id: &str, now: Instant) {
        self.clocks
            .lock()
            .entry(session_id.to_string())
            .or_insert_with(|| CheckpointClock::new(self.cadence, now))
            .postponed(now);
    }

    /// Whether the wall-clock half of the cadence has elapsed for an idle session.
    pub fn checkpoint_interval_elapsed(&self, session_id: &str, now: Instant) -> bool {
        let mut clocks = self.clocks.lock();
        clocks
            .entry(session_id.to_string())
            .or_insert_with(|| CheckpointClock::new(self.cadence, now))
            .interval_elapsed(now)
    }

    /// Seeds a session's clock with the offset the keeper already holds a checkpoint at.
    ///
    /// Call this when a pump starts. A fresh process has no clock, so without the seed its first
    /// commit would name an offset at or below the one the keeper already stores, and the keeper
    /// would answer `STALE_CHECKPOINT` -- a refusal that reads like a bug and costs a full
    /// serialize-and-upload to discover. The seed is the smallest offset the keeper will accept.
    pub fn checkpoint_anchor(&self, session_id: &str, offset: u64, now: Instant) {
        if offset == 0 {
            return;
        }
        let mut clocks = self.clocks.lock();
        let clock = clocks
            .entry(session_id.to_string())
            .or_insert_with(|| CheckpointClock::new(self.cadence, now));
        if clock.committed_offset() < offset {
            clock.committed(offset, now);
        }
    }

    /// Marks a checkpoint committed at `offset`, advancing the session's durable offset.
    pub fn checkpoint_committed(&self, session_id: &str, offset: u64, now: Instant) {
        self.clocks
            .lock()
            .entry(session_id.to_string())
            .or_insert_with(|| CheckpointClock::new(self.cadence, now))
            .committed(offset, now);
        if let Ok(mut table) = self.store.load() {
            if let Some(record) = table.sessions.get_mut(session_id) {
                record.checkpoint_offset = offset;
                let _ = self.store.save(&table);
            }
        }
    }

    /// Forgets a session's cadence state when it ends.
    pub fn forget(&self, session_id: &str) {
        self.clocks.lock().remove(session_id);
    }
}

/// Supplies the bytes of the terminal state a checkpoint commits.
///
/// The daemon cannot derive them: a checkpoint is serialized terminal state, which only the
/// embedder holding the parser can produce. Implementors run on the keeper path's own thread and
/// must be cheap, because a slow source delays the session's output pump.
pub trait CheckpointSource: Send + Sync {
    /// The serialized terminal state of `session_id` **and** the exact output offset whose bytes
    /// that state has applied, or `None` when this runtime has no state for the session yet (the
    /// checkpoint is then skipped, never faked).
    ///
    /// The offset is returned by the same call that serializes the state, and both are taken under
    /// whatever lock the implementor already holds while publishing, so the pair is one atomic
    /// observation. A caller may not substitute the pump's byte count: the pump is ahead of the hub
    /// by however much sits in the channel, so committing `delivered_offset` against `state@X` makes
    /// the keeper truncate replay at `delivered_offset` while recovery resumes from `X` -- silently
    /// dropping `X..delivered_offset`.
    fn checkpoint_bytes(&self, session_id: &str) -> Option<CheckpointSnapshot>;
}

impl<F> CheckpointSource for F
where
    F: Fn(&str) -> Option<CheckpointSnapshot> + Send + Sync,
{
    fn checkpoint_bytes(&self, session_id: &str) -> Option<CheckpointSnapshot> {
        self(session_id)
    }
}

/// A serialized terminal state together with the offset it covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointSnapshot {
    /// The serialized terminal state.
    pub bytes: Vec<u8>,
    /// Every output byte below this offset is reflected in `bytes`; none at or above it is.
    pub applied_offset: u64,
}

/// What one keeper-backed session is doing right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeeperSessionStatus {
    pub session_id: String,
    pub epoch: u64,
    /// True once a `REPLAY_GAP` was surfaced and a parser reset is required before the next byte.
    pub parser_reset_required: bool,
    pub sequence_offset: u64,
    /// The child pid and its kernel start time, so a keeper-owned session reports the same identity
    /// a direct one does and a reused pid cannot be mistaken for it.
    pub pid: u32,
    /// Epoch MICROseconds, exactly as the keeper reports it: convert with
    /// `terminal::keeper_start_time_ms` before putting it in a milliseconds field.
    pub process_start_time: Option<u64>,
}

/// The hook that applies a fetched checkpoint's bytes to a recovered session.
type RestoreHook = Arc<dyn Fn(&str, &[u8]) + Send + Sync>;

/// The reader of the receiver's cumulative published-byte counter for a session.
type HubPublishedReader = Arc<dyn Fn(&str) -> Option<u64> + Send + Sync>;

/// The live keeper session runtime: one output pump per keeper-owned session, the checkpoint
/// cadence, and the recovery path.
///
/// Output is **polled**, not pushed. A keeper connection has exactly one role and carries at most
/// one session's live tap, so a connection per attached session would make the daemon's session
/// count into the keeper's connection count -- and the keeper refuses the ninth peer by contract.
/// Instead each session's pump asks for the bytes its reader has not taken yet with a fenced
/// `REPLAY`, over connections borrowed from one small pool shared by every verb this runtime
/// issues. A `REPLAY_GAP` answer still sets the session's reset flag, and the pump then drops every
/// byte until one arrives at or past the resume offset: a discontinuous suffix is never fed to a
/// parser.
pub struct KeeperRuntime {
    host: KeeperSessionHost,
    ownership: Arc<KeeperOwnership>,
    /// How the receiver is told the keeper dropped bytes, so it resets its parser at the seam
    /// instead of hiding a discontinuity behind a seamless byte stream.
    gap_sink: Arc<dyn Fn(&str) + Send + Sync>,
    /// How the committed checkpoint's bytes are applied on recovery, before the retained suffix
    /// that continues from them.
    restore: RestoreHook,
    checkpoints: Arc<parking_lot::RwLock<Option<Arc<dyn CheckpointSource>>>>,
    statuses: Arc<parking_lot::Mutex<BTreeMap<String, KeeperSessionStatus>>>,
    /// The epoch each session's pump holds. Newer than the spawn's, because the keeper refuses an
    /// attach at or below the newest epoch it has seen for the session, and the pump names it on
    /// every fenced poll.
    pump_epochs: Arc<parking_lot::Mutex<BTreeMap<String, u64>>>,
    /// One nudge per pump, so a keystroke is drained at once instead of at the next idle interval.
    /// A send carries no payload and a full channel is not an error: it means the pump is already
    /// awake and will read the bytes the nudge was about.
    wakes: Arc<parking_lot::Mutex<BTreeMap<String, SyncSender<()>>>>,
    /// How many trailing bytes a recovery reads from the keeper's log, when the runtime knows.
    ///
    /// Set from the hub the recovered bytes are published into, so the first chunk a recovery
    /// publishes is already everything that hub can hold. `None` leaves the keeper's own page as the
    /// only bound, which is what a runtime with no hub behind it needs.
    ring_capacity_bytes: Option<u64>,
    /// How many bytes the receiver's hub has already published for a session. Read once when a pump
    /// starts, to convert the hub's byte counter into the keeper's output offset (see
    /// [`KeeperOffsetBias`]). `None` on a runtime with no hub, which leaves the delivery offset as
    /// the only available answer.
    hub_published: HubPublishedReader,
}

/// The receiver a keeper session's bytes are pushed into, sized like a direct PTY's read channel.
const KEEPER_OUTPUT_CAPACITY: usize = 1024;

/// How long a pump waits for new output while the session is active.
///
/// Each poll is a round trip through the shared pool, so this is a latency/CPU trade: it bounds how
/// late a keystroke's echo can be when no nudge reaches the pump first. Input nudges the pump, so in
/// steady state this is the cost of noticing output the daemon did not cause.
const KEEPER_POLL_BUSY: Duration = Duration::from_millis(10);

/// The longest a quiet pump waits between correcting polls.
///
/// This is the latency a session's spontaneous output can be late by: bytes it produced on its own,
/// with no daemon write to nudge the pump. Kept just under a tenth of a second so it stays below what
/// a reader perceives, while a hundred idle sessions cost about ten polls a second each against a pool
/// whose permits are repaid by a single round trip.
const KEEPER_POLL_IDLE_MAX: Duration = Duration::from_millis(100);

/// How many consecutive failed polls end a session's pump.
///
/// One failed poll is a socket, not a session: the next poll reconnects. A run of them is the
/// keeper being gone or refusing this daemon, and a pump that kept polling would hold a reader that
/// can never deliver anything again -- which is what the old permanent tap's close reported.
const KEEPER_POLL_FAILURES_BEFORE_END: u32 = 8;

/// The keeper's wire code for a session it no longer holds.
///
/// It matters more under polling than it did on a tap: a tap's socket simply closed, whereas a poll
/// keeps asking a registry that no longer has the session. Retrying that is not a transient socket
/// problem, so the pump reads the code and ends instead of spending a backoff on every turn.
const KEEPER_CODE_UNKNOWN_SESSION: &str = "UNKNOWN_SESSION";

/// Whether a poll failed because the keeper no longer holds this session at all.
fn keeper_reports_session_gone(error: &KeeperSessionError) -> bool {
    matches!(
        error,
        KeeperSessionError::Keeper(client)
            if client.wire_code() == Some(KEEPER_CODE_UNKNOWN_SESSION)
    )
}

/// Refuses an input write the keeper only partly accepted.
///
/// `WRITE` is answered with the byte count the keeper took. Anything short of the offered frame
/// means part of the input is not in the PTY, so returning success would hand the caller a
/// keystroke that produces no echo and nothing downstream could tell that apart from a slow
/// program. The count is checked here, at the boundary, rather than trusted from the caller.
fn demand_full_write(
    session_id: &str,
    offered: usize,
    accepted: u64,
) -> Result<(), KeeperSessionError> {
    let accepted = usize::try_from(accepted).unwrap_or(usize::MAX);
    if accepted == offered {
        return Ok(());
    }
    Err(KeeperSessionError::ShortWrite {
        session_id: session_id.to_string(),
        accepted,
        offered,
    })
}

impl KeeperRuntime {
    pub fn new(
        host: KeeperSessionHost,
        ownership: Arc<KeeperOwnership>,
    ) -> Self {
        Self {
            host,
            ownership,
            gap_sink: Arc::new(|_| {}),
            restore: Arc::new(|_, _| {}),
            ring_capacity_bytes: None,
            checkpoints: Arc::new(parking_lot::RwLock::new(None)),
            statuses: Arc::new(parking_lot::Mutex::new(BTreeMap::new())),
            pump_epochs: Arc::new(parking_lot::Mutex::new(BTreeMap::new())),
            wakes: Arc::new(parking_lot::Mutex::new(BTreeMap::new())),
            hub_published: Arc::new(|_| None),
        }
    }

    /// Installs the hook called when the keeper dropped output, so the session's parser is reset.
    pub fn set_gap_sink(&mut self, sink: Arc<dyn Fn(&str) + Send + Sync>) {
        self.gap_sink = sink;
    }

    /// Declares how many trailing bytes a recovery may read: the capacity of the hub it feeds.
    pub fn set_ring_capacity_bytes(&mut self, capacity: usize) {
        self.ring_capacity_bytes = u64::try_from(capacity).ok().filter(|bytes| *bytes > 0);
    }

    /// Installs the hook that applies a fetched checkpoint's bytes to a recovered session.
    pub fn set_restore(&mut self, restore: RestoreHook) {
        self.restore = restore;
    }

    /// Installs the reader of the receiver's cumulative published-byte counter for a session.
    ///
    /// This is what lets a checkpoint name the offset its own serialized state covers instead of
    /// This is what lets a checkpoint name the offset its own serialized state covers instead of
    /// the pump's byte count. Without it the pump cannot know where the hub has got to, and a
    /// checkpoint would have to be skipped rather than committed against a guessed offset.
    pub fn set_hub_published(&mut self, read: HubPublishedReader) {
        self.hub_published = read;
    }

    /// Installs the producer that supplies checkpoint state.
    pub fn set_checkpoint_source(&self, source: Arc<dyn CheckpointSource>) {
        *self.checkpoints.write() = Some(source);
    }

    pub fn ownership(&self) -> &Arc<KeeperOwnership> {
        &self.ownership
    }

    /// Routes new sessions to this runtime's keeper once that keeper proves live. The flag is
    /// written through the store `spawn` consults, and the proof is bound to this runtime's socket.
    pub fn activate_new_sessions(&self) -> Result<ActivationOutcome, OwnershipError> {
        let probe = crate::daemon::crash_supervisor::KeeperSocketProbe::new(self.host.runtime_dir());
        activate_ownership(self.ownership.store(), &probe)
    }

    pub fn status(&self, session_id: &str) -> Option<KeeperSessionStatus> {
        self.statuses.lock().get(session_id).cloned()
    }

    pub fn statuses(&self) -> Vec<KeeperSessionStatus> {
        self.statuses.lock().values().cloned().collect()
    }

    pub fn owned_sessions(&self) -> Vec<String> {
        self.statuses.lock().keys().cloned().collect()
    }

    /// Spawns a keeper-owned session and starts its output pump. The returned receiver is the same
    /// shape a direct-owned session produces, so the caller's output registration is unchanged.
    pub fn spawn(
        &self,
        spec: &KeeperSpawnSpec,
    ) -> Result<(OwnershipRecord, tokio::sync::mpsc::Receiver<Vec<u8>>), KeeperSessionError> {
        let record = self
            .host
            .spawn(self.ownership.store(), spec, self.ownership.next_epoch())?;
        let (_, status, rx) = self.start_pump(
            &record.session_id,
            record.checkpoint_offset,
            record.pid,
            record.process_start_time,
        )?;
        self.statuses.lock().insert(record.session_id.clone(), status);
        Ok((record, rx))
    }

    /// The offset a pump must resume reading from, given what the durable record claims.
    ///
    /// The record's `checkpoint_offset` names the newest byte a checkpoint covers, so it is only a
    /// valid place to start when **this process** holds the state that checkpoint describes. When
    /// the hub has published nothing for the session -- a successor daemon, or a retried spawn whose
    /// first attempt never started a pump -- the keeper's log is the only source of its history, and
    /// the record's offset cannot be trusted to describe what the hub holds, because a predecessor
    /// that adopted the session from discovery may have written the keeper's *head* there instead of
    /// a checkpoint offset. Resuming at a head reads a suffix that starts after everything the hub is
    /// missing: Darwin run14b, where the keeper held 720249 bytes, the pump registered an empty hub,
    /// and every client attach answered `historyBytes: 0` for the session's whole life.
    ///
    /// So the hub-absent case asks the keeper, which is authoritative about its own checkpoint: the
    /// committed offset when one exists (the log is truncated to it, so starting earlier would be a
    /// gap) and `0` when none does (the whole retained log is the history). A hub that already holds
    /// state keeps the recorded offset, because re-reading bytes it has would duplicate them.
    fn resume_offset(&self, session_id: &str, checkpoint_offset: u64) -> u64 {
        if (self.hub_published)(session_id).is_some() {
            return checkpoint_offset;
        }
        // A keeper that cannot answer leaves the full retained log as the answer: replaying too much
        // is recoverable, replaying from a head the hub never saw is not.
        self.host
            .committed_checkpoint_offset(session_id)
            .unwrap_or(0)
    }

    /// Reattaches a keeper session after a daemon restart and starts its pump.
    pub fn attach(
        &self,
        session_id: &str,
    ) -> Result<(RecoveredSession, tokio::sync::mpsc::Receiver<Vec<u8>>), KeeperSessionError> {
        let stored = self
            .ownership
            .store()
            .load()
            .ok()
            .and_then(|table| table.sessions.get(session_id).cloned())
            .ok_or_else(|| KeeperSessionError::NotKeeperOwned {
                session_id: session_id.to_string(),
            })?;
        if stored.owner != SessionOwner::Keeper {
            return Err(KeeperSessionError::NotKeeperOwned {
                session_id: session_id.to_string(),
            });
        }
        let (recovered, status, rx) = self.start_pump(
            session_id,
            self.resume_offset(session_id, stored.checkpoint_offset),
            stored.pid,
            stored.process_start_time,
        )?;
        self.statuses.lock().insert(session_id.to_string(), status);
        Ok((recovered, rx))
    }

    /// Writes to the PTY and nudges the session's pump, so the echo is drained at once instead of
    /// at the next idle interval. The nudge is sent **after** the write: a pump woken before the
    /// bytes exist would find nothing and fall back to waiting.
    pub fn write_input(
        &self,
        session_id: &str,
        data: &[u8],
    ) -> Result<(), KeeperSessionError> {
        let epoch = self.current_epoch(session_id)?;
        let accepted = self.host.write_input(session_id, epoch, data)?;
        demand_full_write(session_id, data.len(), accepted)?;
        self.wake(session_id);
        Ok(())
    }

    pub fn resize(&self, session_id: &str, cols: u16, rows: u16) -> Result<(), KeeperSessionError> {
        let epoch = self.current_epoch(session_id)?;
        self.host.resize(session_id, epoch, cols, rows)?;
        // A resize changes what the child redraws, so its answer is output like any other.
        self.wake(session_id);
        Ok(())
    }

    pub fn signal(
        &self,
        session_id: &str,
        signal: crate::terminal::TerminalSignal,
    ) -> Result<(), KeeperSessionError> {
        let epoch = self.current_epoch(session_id)?;
        self.host.signal(session_id, epoch, keeper_signal_name(signal))
    }

    /// Nudges the session's pump to poll now. A full channel is not a problem: it means the pump is
    /// already awake and will read the bytes this nudge was about.
    fn wake(&self, session_id: &str) {
        if let Some(wake) = self.wakes.lock().get(session_id) {
            let _ = wake.try_send(());
        }
    }

    pub fn close(&self, session_id: &str) -> Result<(), KeeperSessionError> {
        let epoch = self.current_epoch(session_id).unwrap_or_default();
        let result = self
            .host
            .close(self.ownership.store(), session_id, epoch);
        self.pump_epochs.lock().remove(session_id);
        self.wakes.lock().remove(session_id);
        self.statuses.lock().remove(session_id);
        self.ownership.forget(session_id);
        result
    }

    /// True when this runtime serves the session, which is what makes the call sites' routing
    /// decision a lookup rather than a guess.
    pub fn owns(&self, session_id: &str) -> bool {
        self.statuses.lock().contains_key(session_id)
    }

    /// The epoch every verb for `session_id` must present: the one its pump holds.
    ///
    /// The keeper fences an epoch on `ATTACH`, and a pump always attaches, so for a served session
    /// the newest epoch in force is the pump's. Reading it from the status record instead is what
    /// let a write present an epoch the keeper had already superseded: the keeper refused it with
    /// `EPOCH_MISMATCH` and the keystroke vanished. `close` already preferred the pump's epoch for
    /// exactly this reason; every verb now reads the one source.
    fn current_epoch(&self, session_id: &str) -> Result<u64, KeeperSessionError> {
        if let Some(epoch) = self.pump_epochs.lock().get(session_id).copied() {
            return Ok(epoch);
        }
        // No pump: either one is about to start or it has ended. The status epoch is the best
        // knowledge left, and the keeper is the one that decides whether it still holds.
        self.statuses
            .lock()
            .get(session_id)
            .map(|status| status.epoch)
            .ok_or_else(|| KeeperSessionError::NotKeeperOwned {
                session_id: session_id.to_string(),
            })
    }

    /// Attaches the session at a fresh epoch and starts the pump that forwards its bytes into `rx`.
    ///
    /// The attach is what fences the epoch: the keeper refuses anything at or below the newest one
    /// it has seen, so this single call is what makes this runtime the session's reader. The attach
    /// asks for no live tap, so its connection returns to the shared pool and the pump drains output
    /// with fenced `REPLAY` pages from then on.
    fn start_pump(
        &self,
        session_id: &str,
        start_offset: u64,
        pid: u32,
        process_start_time: Option<u64>,
    ) -> Result<
        (
            RecoveredSession,
            KeeperSessionStatus,
            tokio::sync::mpsc::Receiver<Vec<u8>>,
        ),
        KeeperSessionError,
    > {
        let epoch = self.ownership.next_epoch();
        // How much history one recovery may read: the receiver's ring, which is the most a client
        // attach can be handed in one snapshot.
        //
        // Only when this process holds **nothing** for the session. A hub that already published
        // bytes keeps them, and its pump resumes from the recorded offset precisely so those bytes
        // are not read twice; clamping that read to the tail would re-publish a suffix the hub
        // already has. The empty-hub case is the only one where nothing is lost by skipping a
        // prefix, because there is nothing there to skip.
        let tail_bytes = match (self.hub_published)(session_id) {
            Some(_) => None,
            None => self.ring_capacity_bytes,
        };
        let recovered =
            self.host
                .attach_and_recover(session_id, epoch, start_offset, tail_bytes)?;
        let (tx, rx) = tokio::sync::mpsc::channel::<Vec<u8>>(KEEPER_OUTPUT_CAPACITY);
        let (wake_tx, wake_rx) = std::sync::mpsc::sync_channel::<()>(1);
        self.pump_epochs.lock().insert(session_id.to_string(), epoch);
        self.wakes.lock().insert(session_id.to_string(), wake_tx);
        if let Some(bytes) = recovered.checkpoint_bytes.as_deref() {
            (self.restore)(session_id, bytes);
        }
        if recovered.requires_parser_reset() {
            (self.gap_sink)(session_id);
        }
        // `try_send`, not `blocking_send`: this runs on the spawn caller's thread, which is a Tokio
        // worker, where blocking on a channel panics. The channel is empty and sized for far more
        // than one chunk, so the only failure that can happen here is a dropped receiver.
        let replayed_len = recovered.replayed.len() as u64;
        if !recovered.replayed.is_empty()
            && tx.try_send(recovered.replayed.clone()).is_err()
        {
            // No pump was started, so the entries registered above would outlive the receiver they
            // belong to: drop them rather than leave a nudge pointing at nothing.
            self.pump_epochs.lock().remove(session_id);
            self.wakes.lock().remove(session_id);
            return Err(KeeperSessionError::Incompatible(format!(
                "the recovered tail of {session_id} could not be queued for its reader"
            )));
        }
        let gap_sink = Arc::clone(&self.gap_sink);
        let ownership = Arc::clone(&self.ownership);
        let checkpoints = Arc::clone(&self.checkpoints);
        let statuses = Arc::clone(&self.statuses);
        let host = self.host.sharing_pool();
        let pump_session = session_id.to_string();
        let gap = recovered.replay_gap;
        let delivered = recovered.sequence_offset;
        // The hub's published-byte counter and the keeper's output offset are two axes over the
        // same byte stream. The bias between them is measured here, from the counts as they stand
        // before any of this pump's bytes reach the hub: the hub has published `hub_base` bytes and
        // the first byte it will apply next is the one the keeper numbered `delivered -
        // replayed_len`, because the queued replay is exactly `replayed_len` bytes long. From there
        // `hub_expected` advances by the length of every chunk this pump forwards, which keeps the
        // mapping exact even while chunks are still in flight.
        let hub_base = (self.hub_published)(session_id).unwrap_or(0);
        let keeper_base = delivered.saturating_sub(replayed_len);
        // What the keeper already holds a checkpoint at, when that is knowable: the retained suffix
        // begins exactly where the committed state ends, so `delivered - replayed_len` is that
        // offset whenever a checkpoint exists and no gap truncated the suffix. Seeding the cadence
        // with it turns "this pump's first commit names the offset the keeper already stores" -- a
        // `STALE_CHECKPOINT` refusal that looks like a defect and costs a full serialize-and-upload
        // to discover -- into a postponement the cadence handles on its own.
        if recovered.checkpoint_id.is_some() && gap.is_none() && replayed_len > 0 {
            self.ownership
                .checkpoint_anchor(session_id, keeper_base, Instant::now());
        }
        // `mut` because a gap re-measures it: the bias has to absorb the jump the keeper's log made.
        let mut bias = KeeperOffsetBias::new(hub_base, keeper_base);
        std::thread::Builder::new()
            .name(format!("ferryx-keeper-replay-{session_id}"))
            .spawn(move || {
                // The offset a gap named: bytes at or below it are the ones the keeper dropped, so
                // the reader resumes at it instead of waiting for a reattach that may never come.
                let mut floor: Option<u64> = gap.map(|gap| gap.available_from_sequence);
                let mut offset = delivered;
                let mut hub_expected = hub_base;
                // Start responsive: a session that just spawned usually has a prompt to deliver.
                let mut wait = KEEPER_POLL_BUSY;
                // A pooled connection that fails once is replaced by the next poll. A run of them
                // means the keeper itself is gone or refusing this daemon, which is the same seam
                // the old permanent tap's close named: the pump ends instead of polling a dead
                // socket for the life of the process.
                let mut consecutive_failures = 0u32;
                'pump: loop {
                    let mut backlog = false;
                    let mut forwarded = false;
                    match host.replay_page(&pump_session, epoch, offset) {
                        Ok(reply) => {
                            consecutive_failures = 0;
                            // `more` says the keeper still holds bytes past this page, so a page that
                            // arrives with a backlog is drained at once instead of waiting, and the
                            // idle backoff only applies to a genuinely quiet session.
                            backlog = reply.more;
                            for event in replay_events(reply) {
                                match event {
                                    AttachedEvent::Gap {
                                        available_from_sequence,
                                        ..
                                    } => {
                                        // The reset is signalled *before* any post-gap byte can be
                                        // forwarded: the Output arm below is the only thing that
                                        // queues bytes, and a gapped page only lets through bytes at
                                        // or after `floor`, so the sink always runs first. That is
                                        // the ordering plan rev5 requires: reset the parser, then
                                        // consume.
                                        offset = available_from_sequence;
                                        floor = Some(available_from_sequence);
                                        // The keeper jumped its offset over the bytes it dropped
                                        // while the hub never published them. The bias has to absorb
                                        // that jump, or every later checkpoint would name an offset
                                        // the serialized state does not cover.
                                        bias = KeeperOffsetBias::new(
                                            hub_expected,
                                            available_from_sequence,
                                        );
                                        gap_sink(&pump_session);
                                        if let Some(status) =
                                            statuses.lock().get_mut(&pump_session)
                                        {
                                            status.parser_reset_required = true;
                                            status.sequence_offset = offset;
                                        }
                                    }
                                    AttachedEvent::Output {
                                        sequence_offset,
                                        data,
                                    } => {
                                        if let Some(reachable) = floor {
                                            if sequence_offset < reachable {
                                                continue;
                                            }
                                            floor = None;
                                            if let Some(status) =
                                                statuses.lock().get_mut(&pump_session)
                                            {
                                                status.parser_reset_required = false;
                                            }
                                        }
                                        offset = sequence_offset
                                            .saturating_add(data.len() as u64);
                                        if let Some(status) =
                                            statuses.lock().get_mut(&pump_session)
                                        {
                                            status.sequence_offset = offset;
                                        }
                                        let produced = data.len() as u64;
                                        hub_expected = hub_expected.saturating_add(produced);
                                        // A dropped receiver means nobody will read this session
                                        // again, which is this pump's reason to end.
                                        if tx.blocking_send(data).is_err() {
                                            break 'pump;
                                        }
                                        forwarded = true;
                                        if !ownership.checkpoint_due(
                                            &pump_session,
                                            produced,
                                            Instant::now(),
                                        ) {
                                            continue;
                                        }
                                        run_checkpoint(
                                            &ownership,
                                            &checkpoints,
                                            &host,
                                            &pump_session,
                                            epoch,
                                            offset,
                                            bias,
                                        );
                                    }
                                }
                            }
                        }
                        // The keeper has no such session any more: its child exited, or a successor
                        // retired it. Nothing this pump can poll will change that, so it ends and
                        // drops the receiver, which is how the session's output stream closes.
                        Err(error) if keeper_reports_session_gone(&error) => {
                            tracing::info!(
                                session_id = %pump_session,
                                %error,
                                "keeper no longer holds this session; ending its pump"
                            );
                            break 'pump;
                        }
                        // A dropped pooled connection says nothing about the session: the next poll
                        // reconnects, so the pump waits and tries again rather than ending a live
                        // session's reader over one socket.
                        Err(error) if error.is_transient() => {
                            consecutive_failures += 1;
                            tracing::warn!(
                                session_id = %pump_session,
                                %error,
                                consecutive_failures,
                                "keeper replay poll failed; retrying on a fresh connection"
                            );
                            if consecutive_failures >= KEEPER_POLL_FAILURES_BEFORE_END {
                                break 'pump;
                            }
                        }
                        // A fenced epoch, a session the keeper no longer holds or a protocol this
                        // client cannot read is not a condition a retry improves. Ending here is
                        // what keeps a superseded daemon from polling a session a successor owns.
                        Err(error) => {
                            tracing::warn!(
                                session_id = %pump_session,
                                %error,
                                "keeper replay poll ended this session's pump"
                            );
                            break 'pump;
                        }
                    }
                    // The wall-clock half of the cadence is evaluated on every turn, busy or not:
                    // a session that produced output and then went quiet still checkpoints on time.
                    if ownership.checkpoint_interval_elapsed(&pump_session, Instant::now()) {
                        run_checkpoint(
                            &ownership,
                            &checkpoints,
                            &host,
                            &pump_session,
                            epoch,
                            offset,
                            bias,
                        );
                    }
                    wait = if forwarded {
                        KEEPER_POLL_BUSY
                    } else {
                        (wait * 2).min(KEEPER_POLL_IDLE_MAX)
                    };
                    // Only a page that actually moved bytes skips the wait. A `more` with nothing
                    // forwarded is not progress, and continuing on it would spin this pump without
                    // advancing the cursor the keeper is paging from.
                    if backlog && forwarded {
                        continue;
                    }
                    match wake_rx.recv_timeout(wait) {
                        Ok(()) | Err(RecvTimeoutError::Timeout) => {}
                        // The runtime dropped this session's nudge, which happens exactly when the
                        // session was closed: nothing left to pump.
                        Err(RecvTimeoutError::Disconnected) => break 'pump,
                    }
                }
                // This pump is the only publisher for the session, so its end is the seam where the
                // cadence state can no longer advance: dropping it keeps a later attach from
                // inheriting a stale backoff or a committed offset belonging to a dead pump.
                ownership.forget(&pump_session);
            })
            .map_err(|error| KeeperSessionError::Incompatible(error.to_string()))?;
        Ok((
            RecoveredSession {
                replayed: Vec::new(),
                ..recovered
            },
            KeeperSessionStatus {
                session_id: session_id.to_string(),
                epoch,
                parser_reset_required: gap.is_some(),
                sequence_offset: delivered,
                pid,
                process_start_time,
            },
            rx,
        ))
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (tempfile::TempDir, OwnershipStore) {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = OwnershipStore::at(dir.path().join(OWNERSHIP_FILE_NAME));
        (dir, store)
    }

    fn keeper_record(session_id: &str) -> OwnershipRecord {
        OwnershipRecord {
            session_id: session_id.to_string(),
            owner: SessionOwner::Keeper,
            epoch: 7,
            cols: 80,
            rows: 24,
            pid: 4242,
            process_start_time: Some(1_700_000_000_000),
            generation: Some("1.0.0".into()),
            created_at: 1,
            checkpoint_offset: 128,
        }
    }

    #[test]
    fn the_flag_routes_new_sessions_only_and_never_rewrites_an_existing_owner() {
        let (_dir, store) = temp_store();
        store
            .record(OwnershipRecord {
                owner: SessionOwner::Daemon,
                ..keeper_record("direct-1")
            })
            .expect("record direct");
        let table = store.set_keeper_enabled(true).expect("enable");
        assert_eq!(table.new_session_owner(), SessionOwner::Keeper);
        assert_eq!(table.owner_of("direct-1"), Some(SessionOwner::Daemon));

        let table = store.set_keeper_enabled(false).expect("disable");
        assert_eq!(table.new_session_owner(), SessionOwner::Daemon);
        assert_eq!(table.owner_of("direct-1"), Some(SessionOwner::Daemon));
    }

    #[test]
    fn disabling_leaves_keeper_sessions_recorded_and_attachable() {
        let (_dir, store) = temp_store();
        store
            .record(keeper_record("keeper-1"))
            .expect("record keeper");
        let table = store.set_keeper_enabled(false).expect("disable");
        assert_eq!(table.owner_of("keeper-1"), Some(SessionOwner::Keeper));
        assert_eq!(table.keeper_owned_count(), 1);
    }

    #[test]
    fn rollback_is_refused_with_the_exact_count_until_keeper_sessions_end() {
        let (_dir, store) = temp_store();
        store.record(keeper_record("keeper-1")).expect("first");
        store.record(keeper_record("keeper-2")).expect("second");

        match store.refuse_rollback_if_keeper_sessions() {
            Err(OwnershipRefusal::RollbackWithKeeperSessions { count }) => assert_eq!(count, 2),
            other => panic!("rollback must be refused, got {other:?}"),
        }
        assert!(store
            .refuse_rollback_if_keeper_sessions()
            .expect_err("still refused")
            .message()
            .contains("ROLLBACK_REFUSED_KEEPER_SESSIONS: 2 session(s)"));

        store.forget("keeper-1").expect("end first");
        store.forget("keeper-2").expect("end second");
        store
            .refuse_rollback_if_keeper_sessions()
            .expect("rollback allowed once no keeper session lives");
    }

    #[test]
    fn a_missing_store_is_empty_rather_than_an_error() {
        let (_dir, store) = temp_store();
        let table = store.load().expect("missing file is empty");
        assert!(table.sessions.is_empty());
        assert!(!table.keeper_ownership_enabled);
    }

    #[test]
    fn checkpoint_cadence_fires_on_bytes_then_on_the_interval() {
        let start = Instant::now();
        let mut clock = CheckpointClock::new(CheckpointCadence::default(), start);
        assert!(!clock.observe(1024, start));
        assert!(clock.observe(2 * 1024 * 1024, start));

        clock.committed(4096, start);
        assert_eq!(clock.committed_offset(), 4096);
        assert!(!clock.observe(1, start + Duration::from_secs(59)));
        assert!(clock.observe(1, start + Duration::from_secs(60)));
    }

    #[test]
    fn reconcile_reports_sessions_the_keeper_no_longer_holds() {
        let (_dir, store) = temp_store();
        store.record(keeper_record("keeper-gone")).expect("record");
        let discovery = reconcile_startup(
            &store,
            vec![KeeperSessionView {
                session_id: "keeper-live".into(),
                pid: 99,
                epoch: 11,
                sequence_offset: 512,
                cols: 100,
                rows: 40,
                owner: "keeper".into(),
                generation: "1.2.3".into(),
            }],
        )
        .expect("reconcile");

        assert_eq!(discovery.vanished, vec!["keeper-gone".to_string()]);
        assert_eq!(discovery.keeper_session_count(), 1);
        let table = store.load().expect("table");
        assert!(!table.sessions.contains_key("keeper-gone"));
        assert_eq!(table.owner_of("keeper-live"), Some(SessionOwner::Keeper));
    }

    #[test]
    fn an_attached_gap_page_surfaces_both_offsets() {
        // A gap carries no bytes: partial chunks are dropped by the client rather than passed off as
        // a complete history, so the page folds to the gap alone and the parser is reset before any
        // later byte is applied.
        let page = ReplayReply {
            chunks: vec![(4096, String::new())],
            end_sequence: 0,
            more: false,
            gap: Some(crate::daemon::keeper_client::ReplayGap {
                requested_after_sequence: 12,
                available_from_sequence: 4096,
            }),
        };
        assert_eq!(
            replay_events(page),
            vec![AttachedEvent::Gap {
                requested_after_sequence: 12,
                available_from_sequence: 4096,
            }]
        );
    }

    #[test]
    fn an_output_page_carries_each_chunks_offset_and_bytes() {
        use base64::engine::general_purpose::STANDARD as BASE64;
        use base64::Engine as _;
        // An empty chunk is not an event: queueing it would move nothing forward while looking like
        // progress, so only the chunk that decodes to bytes survives.
        let page = ReplayReply {
            chunks: vec![(60, String::new()), (64, BASE64.encode(b"hi"))],
            end_sequence: 66,
            more: false,
            gap: None,
        };
        assert_eq!(
            replay_events(page),
            vec![AttachedEvent::Output {
                sequence_offset: 64,
                data: b"hi".to_vec(),
            }]
        );
    }

    /// A recovery reads the tail its hub can hold, not the oldest retained page the keeper offers.
    ///
    /// The keeper's `ATTACH` copies at most one 512 KiB page and its ack names the session **head**,
    /// not where that page ends. Starting a recovery at a caller offset near the log's start
    /// therefore delivers the oldest page and leaves the ring evicting its own tail as the pump pages
    /// forward: the history a client is served is a prefix that stops before the bytes it cares
    /// about. This is the no-gap case's arithmetic -- a 720 KiB log, a 512 KiB ring, and `READY:` in
    /// the last few hundred bytes -- so the numbers below are that case's, not invented ones.
    #[test]
    fn a_recovery_reads_the_tail_its_ring_can_hold_not_the_oldest_page() {
        let head = 720_249u64;
        let ring = 512 * 1024u64;

        // The successor daemon's own resume offset is the keeper's committed checkpoint offset,
        // which is 0 when none was ever committed.
        let (start, skipped) = recovery_read_start(0, head, ring);
        assert!(skipped, "a 720 KiB log does not fit a 512 KiB ring, so a prefix is skipped");
        assert_eq!(start, head - ring, "the read begins at the first byte the ring will keep");
        assert!(
            start > 0 && start + ring == head,
            "the window covers exactly the ring's worth of trailing bytes"
        );
        // The failure this replaces: reading from 0 handed over `[0, 524288)` and left
        // `[524288, 720249)` -- where `READY:` sits, at 720109 -- unread. So the window must begin
        // at or before 524288, or exactly those unread bytes stay unread.
        assert!(start < 720_109, "READY: falls inside the window that is actually read");
        assert!(
            start <= 524_288,
            "the window covers [524288, head), which the old first page never read"
        );

        // A log that already fits the ring is read whole from the caller's offset: nothing skipped.
        let (start, skipped) = recovery_read_start(0, 4096, ring);
        assert_eq!((start, skipped), (0, false));

        // A caller resuming inside the window keeps its own offset: only the excess before the
        // window is dropped, never bytes it has already applied.
        let (start, skipped) = recovery_read_start(700_000, head, ring);
        assert_eq!((start, skipped), (700_000, false));

        // A head below the ring size must not underflow into a huge start.
        let (start, skipped) = recovery_read_start(0, 10, ring);
        assert_eq!((start, skipped), (0, false));
    }

    #[test]
    fn recovered_session_reports_a_required_parser_reset_only_on_a_gap() {
        let intact = RecoveredSession {
            session_id: "s".into(),
            epoch: 1,
            sequence_offset: 8,
            checkpoint_id: None,
            checkpoint_bytes: None,
            replayed: b"abc".to_vec(),
            replay_gap: None,
            prefix_skipped: false,
        };
        assert!(!intact.requires_parser_reset());
        let gapped = RecoveredSession {
            replay_gap: Some(ReplayGapKind {
                requested_after_sequence: 1,
                available_from_sequence: 9,
            }),
            ..intact
        };
        assert!(gapped.requires_parser_reset());
    }

    #[test]
    fn epochs_are_strictly_increasing() {
        let source = EpochSource::seeded(41);
        let first = source.acquire();
        let second = source.acquire();
        assert!(second > first);
        assert!(first >= 41);
    }

    /// A runtime with no keeper socket behind it: enough for every verb test whose answer is decided
    /// before a frame is written.
    fn runtime_without_a_keeper() -> (tempfile::TempDir, KeeperRuntime) {
        let dir = tempfile::tempdir().expect("runtime dir");
        let store = OwnershipStore::in_runtime_dir(dir.path());
        let ownership = Arc::new(KeeperOwnership::new(store, CheckpointCadence::default()));
        let runtime = KeeperRuntime::new(KeeperSessionHost::new(dir.path()), ownership);
        (dir, runtime)
    }

    fn server_status(session_id: &str, epoch: u64) -> KeeperSessionStatus {
        KeeperSessionStatus {
            session_id: session_id.to_string(),
            epoch,
            parser_reset_required: false,
            sequence_offset: 0,
            pid: 4242,
            process_start_time: None,
        }
    }

    #[test]
    fn an_input_write_presents_the_pumps_epoch_not_the_attach_that_started_it() {
        // The keeper fences the epoch on ATTACH and then refuses anything older, so the pump's
        // epoch is the only one a later write may present. Reading the status record instead is
        // what let a keystroke arrive at the keeper naming an epoch a successor had superseded:
        // the keeper refused it and the input vanished with the echo it would have produced.
        let (_dir, runtime) = runtime_without_a_keeper();
        runtime
            .statuses
            .lock()
            .insert("s-1".into(), server_status("s-1", 7));
        runtime.pump_epochs.lock().insert("s-1".into(), 11);
        assert!(matches!(runtime.current_epoch("s-1"), Ok(11)));

        // With the pump gone the status is all that is left, and the keeper decides whether it
        // still holds; refusing locally would be a guess.
        runtime.pump_epochs.lock().remove("s-1");
        assert!(matches!(runtime.current_epoch("s-1"), Ok(7)));
    }

    #[test]
    fn a_verb_for_a_session_this_runtime_does_not_serve_is_refused_by_name() {
        let (_dir, runtime) = runtime_without_a_keeper();
        match runtime.write_input("s-missing", b"x") {
            Err(KeeperSessionError::NotKeeperOwned { session_id }) => {
                assert_eq!(session_id, "s-missing")
            }
            other => panic!("an unserved session must be refused, got {other:?}"),
        }
    }

    #[test]
    fn a_write_the_keeper_only_partly_accepted_surfaces_as_an_error() {
        assert!(matches!(demand_full_write("s-1", 4, 4), Ok(())));
        let refused = demand_full_write("s-1", 4, 3).expect_err("a short write is not success");
        assert!(
            matches!(
                refused,
                KeeperSessionError::ShortWrite { accepted: 3, offered: 4, .. }
            ),
            "{refused:?}"
        );
        assert!(refused.to_string().contains("3 of 4"));
        // Retrying cannot put the missing byte back, so nothing may treat this as transient.
        assert!(!refused.is_transient());
    }

    struct KeeperAnswer(crate::daemon::crash_supervisor::KeeperStatus);

    impl crate::daemon::crash_supervisor::KeeperPresence for KeeperAnswer {
        fn live_keeper(&self) -> crate::daemon::crash_supervisor::KeeperStatus {
            self.0.clone()
        }
    }

    fn live() -> KeeperAnswer {
        KeeperAnswer(crate::daemon::crash_supervisor::KeeperStatus::Live { pid: Some(7) })
    }

    fn absent() -> KeeperAnswer {
        KeeperAnswer(crate::daemon::crash_supervisor::KeeperStatus::Absent {
            detail: "no keeper".into(),
        })
    }

    #[test]
    fn activation_is_refused_without_a_proven_keeper_and_never_flips_the_flag() {
        let (_dir, store) = temp_store();
        assert_eq!(
            activate_ownership(&store, &absent()).expect("absent keeper is not an error"),
            ActivationOutcome::NotProven
        );
        assert!(!store.load().expect("table").keeper_ownership_enabled);
        assert_eq!(
            store.load().expect("table").new_session_owner(),
            SessionOwner::Daemon
        );
    }

    #[test]
    fn a_proven_keeper_routes_new_sessions_to_it_and_reports_its_pid() {
        let (_dir, store) = temp_store();
        assert_eq!(
            activate_ownership(&store, &live()).expect("activate"),
            ActivationOutcome::Activated {
                keeper_pid: Some(7)
            }
        );
        let table = store.load().expect("table");
        assert!(table.keeper_ownership_enabled);
        assert_eq!(table.new_session_owner(), SessionOwner::Keeper);
        assert_eq!(
            activate_ownership(&store, &live()).expect("idempotent"),
            ActivationOutcome::AlreadyActive
        );
    }

    #[test]
    fn activation_never_rewrites_an_existing_sessions_owner() {
        let (_dir, store) = temp_store();
        store
            .record(OwnershipRecord {
                owner: SessionOwner::Daemon,
                ..keeper_record("direct-1")
            })
            .expect("record");
        activate_ownership(&store, &live()).expect("activate");
        assert_eq!(
            store.load().expect("table").owner_of("direct-1"),
            Some(SessionOwner::Daemon)
        );
    }

    #[test]
    fn an_isolated_runtime_keeps_its_own_table_and_never_the_production_one() {
        let runtime = tempfile::tempdir().expect("runtime");
        let store = OwnershipStore::in_runtime_dir(runtime.path());
        assert_eq!(store.path(), runtime.path().join(OWNERSHIP_FILE_NAME));
        activate_ownership(&store, &live()).expect("activate");
        assert!(store.load().expect("table").keeper_ownership_enabled);
        assert!(store.path().starts_with(runtime.path()));
    }

    /// Every clock in these tests is a chosen `Instant`, advanced by arithmetic. Nothing sleeps, so
    /// a cadence assertion cannot pass because the machine happened to be slow or fast.
    fn at(base: Instant, secs: u64) -> Instant {
        base + Duration::from_secs(secs)
    }

    #[test]
    fn discovery_after_a_disable_still_adopts_keeper_sessions_as_keeper_owned() {
        // The successor daemon after a disable: the flag is off, so new sessions are direct-owned,
        // and it has never seen this session. Discovery is the only thing that can make the session
        // reachable here, and it must record it as keeper-owned -- recording it as daemon-owned (or
        // dropping it) would make `KeeperRuntime::attach` refuse it and strand a live PTY.
        let (_dir, store) = temp_store();
        store.set_keeper_enabled(false).expect("flag off");
        let discovery = reconcile_startup(
            &store,
            vec![KeeperSessionView {
                session_id: "keeper-live".into(),
                pid: 99,
                epoch: 11,
                sequence_offset: 512,
                cols: 100,
                rows: 40,
                owner: "keeper".into(),
                generation: "1.2.3".into(),
            }],
        )
        .expect("reconcile");

        assert_eq!(discovery.keeper_session_count(), 1);
        assert!(discovery.vanished.is_empty());
        let table = store.load().expect("table");
        assert_eq!(
            table.owner_of("keeper-live"),
            Some(SessionOwner::Keeper),
            "discovery must not re-derive ownership from the new-session flag"
        );
        // The flag is untouched by discovery: routing stays disabled for new sessions.
        assert!(!table.keeper_ownership_enabled);
        assert_eq!(table.new_session_owner(), SessionOwner::Daemon);
        assert_eq!(
            table.sessions["keeper-live"].checkpoint_offset, 0,
            "discovery reports a head, not a checkpoint: claiming one over the whole log made the \
             first reattach resume at the head, where the keeper has nothing left to replay"
        );
    }

    #[test]
    fn a_pump_resumes_at_the_keepers_checkpoint_not_a_head_written_by_an_adopter() {
        // The decision the fix turns on, in both directions, with no socket and no timing: when this
        // process holds no hub state for the session, the record's offset is not a resume cursor --
        // a predecessor that adopted the session from discovery may have written the keeper's head
        // there -- so the keeper is asked instead. With no keeper to ask, the answer must be the
        // retained floor (`0`), never the recorded head: replaying too much is recoverable, replaying
        // from a head the hub never saw is the run14b empty history.
        let (_dir, mut runtime) = runtime_without_a_keeper();
        let head = 720_249;
        assert_eq!(
            runtime.resume_offset("nogap", head),
            0,
            "a hub with nothing published for the session must not resume at a recorded head"
        );

        // A hub that already holds the session's bytes keeps the recorded offset: those bytes are
        // already in its scrollback, and replaying them would duplicate them.
        let published = Arc::new(parking_lot::Mutex::new(Some(4096u64)));
        let seen = Arc::clone(&published);
        runtime.set_hub_published(Arc::new(move |_: &str| *seen.lock()));
        assert_eq!(
            runtime.resume_offset("nogap", 4096),
            4096,
            "a hub that already holds the session's state resumes where that state ends"
        );
    }

    #[test]
    fn an_adopted_session_reattaches_from_the_keeper_floor_not_its_head() {
        // Darwin run14b, case `no-gap-history-altscreen-downtime-input`: a successor daemon adopted
        // a session the harness had spawned through the keeper, whose log held 720249 bytes at
        // offset 0. Discovery reported that head, the record stored it as a checkpoint offset, and
        // the first reattach asked the keeper for the suffix *after* the head. The keeper had
        // nothing past it, so the pump registered an empty hub and every client attach during the
        // next 30s answered `historyBytes: 0` while all 720 KiB sat retained. The resume cursor an
        // adopted record must carry is therefore the keeper's retained floor, never its head.
        let (_dir, store) = temp_store();
        let head = 720_249;
        reconcile_startup(
            &store,
            vec![KeeperSessionView {
                session_id: "nogap".into(),
                pid: 6909,
                epoch: 1,
                sequence_offset: head,
                cols: 80,
                rows: 24,
                owner: "keeper".into(),
                generation: "1.2.3".into(),
            }],
        )
        .expect("reconcile");

        let table = store.load().expect("table");
        let resume_at = table.sessions["nogap"].checkpoint_offset;
        assert_eq!(
            resume_at, 0,
            "an adopted session has no checkpoint, so a reattach must resume at the keeper's \
             retained floor: resuming at {head} asks for bytes past the head and yields no history"
        );
    }

    #[test]
    fn an_existing_record_keeps_its_checkpoint_offset_through_discovery() {
        // The complement of the case above: discovery must only supply a resume cursor for a record
        // it creates. A record that already names a committed checkpoint keeps it, or the successor
        // would replay from the floor and redeliver bytes the checkpoint already covers.
        let (_dir, store) = temp_store();
        store
            .record(OwnershipRecord {
                checkpoint_offset: 4096,
                ..keeper_record("checkpointed")
            })
            .expect("record");
        reconcile_startup(
            &store,
            vec![KeeperSessionView {
                session_id: "checkpointed".into(),
                pid: 4242,
                epoch: 8,
                sequence_offset: 9000,
                cols: 80,
                rows: 24,
                owner: "keeper".into(),
                generation: "1.0.0".into(),
            }],
        )
        .expect("reconcile");

        let table = store.load().expect("table");
        assert_eq!(
            table.sessions["checkpointed"].checkpoint_offset, 4096,
            "discovery refreshes a known record's live fields but must not overwrite the offset \
             its committed checkpoint covers"
        );
    }

    #[test]
    fn the_retry_delay_grows_from_the_base_and_is_capped() {
        let cadence = CheckpointCadence::default();
        assert_eq!(checkpoint_retry_delay(0, cadence), Duration::ZERO);
        let first = checkpoint_retry_delay(1, cadence);
        let second = checkpoint_retry_delay(2, cadence);
        let third = checkpoint_retry_delay(3, cadence);
        assert!(first < second && second < third, "{first:?} {second:?} {third:?}");
        // Capped, and the cap is reached no later than the point where the cadence goes quiet.
        assert_eq!(
            checkpoint_retry_delay(CHECKPOINT_FAILURES_BEFORE_QUIET, cadence),
            CHECKPOINT_RETRY_MAX
        );
        assert_eq!(
            checkpoint_retry_delay(CHECKPOINT_FAILURES_BEFORE_QUIET + 40, cadence),
            CHECKPOINT_RETRY_MAX
        );
        // A longer cadence never makes the retry faster than an eighth of that cadence.
        let slow = CheckpointCadence {
            max_bytes: 1,
            max_interval: Duration::from_secs(4800),
        };
        assert!(checkpoint_retry_delay(1, slow) >= Duration::from_secs(600));
    }

    #[test]
    fn a_failed_install_backs_off_instead_of_retrying_on_the_next_frame() {
        let start = Instant::now();
        let cadence = CheckpointCadence::default();
        let mut clock = CheckpointClock::new(cadence, start);
        // A full chunk of output makes a checkpoint due immediately.
        assert!(clock.observe(cadence.max_bytes, start));
        clock.skipped(start, true);
        assert_eq!(clock.consecutive_failures(), 1);
        // The very next frame produces the same amount again, but the backoff holds it back.
        assert!(!clock.observe(cadence.max_bytes, start));
        // ... and it is released exactly when the backoff expires, not before.
        let delay = checkpoint_retry_delay(1, cadence);
        // Output accumulates across the wait, but the backoff alone decides: the accumulated bytes
        // cross the byte threshold long before the wait ends, so the release instant is the only
        // thing under test here.
        assert!(!clock.observe(cadence.max_bytes, start + delay - Duration::from_millis(1)));
        assert!(clock.observe(0, start + delay));
    }

    #[test]
    fn a_refusal_that_cannot_improve_goes_quiet_without_climbing() {
        let start = Instant::now();
        let cadence = CheckpointCadence::default();
        let mut clock = CheckpointClock::new(cadence, start);
        clock.skipped(start, false);
        assert_eq!(clock.consecutive_failures(), CHECKPOINT_FAILURES_BEFORE_QUIET);
        // Similarly a commit clears both the counter and the backoff.
        let mut clock = CheckpointClock::new(cadence, start);
        clock.skipped(start, true);
        clock.committed(900, at(start, 1));
        assert_eq!(clock.consecutive_failures(), 0);
        assert_eq!(clock.committed_offset(), 900);
        assert!(clock.observe(cadence.max_bytes, at(start, 1)));
    }

    #[test]
    fn a_postponed_checkpoint_resets_the_thresholds_without_advancing_the_offset() {
        let start = Instant::now();
        let cadence = CheckpointCadence::default();
        let mut clock = CheckpointClock::new(cadence, start);
        clock.committed(512, start);
        assert!(clock.observe(cadence.max_bytes, start));
        clock.postponed(start);
        assert_eq!(clock.committed_offset(), 512);
        assert!(!clock.observe(cadence.max_bytes - 1, start));
    }

    #[test]
    fn the_idle_cadence_fires_on_the_interval_and_not_before_it() {
        let start = Instant::now();
        let cadence = CheckpointCadence::default();
        let mut clock = CheckpointClock::new(cadence, start);
        assert!(!clock.interval_elapsed(at(start, 59)));
        assert!(clock.interval_elapsed(at(start, 60)));
        // An idle skip restarts the interval, so a quiet session is not checkpointed every frame.
        clock.postponed(at(start, 60));
        assert!(!clock.interval_elapsed(at(start, 100)));
        assert!(clock.interval_elapsed(at(start, 120)));
    }

    #[test]
    fn the_idle_cadence_observe_cannot_fire_before_the_wall_clock_half() {
        let start = Instant::now();
        let cadence = CheckpointCadence::default();
        let mut clock = CheckpointClock::new(cadence, start);
        // Bytes alone do not fire; the byte threshold is the *other* half of "whichever first".
        assert!(!clock.observe(cadence.max_bytes - 1, at(start, 1)));
        assert!(!clock.interval_elapsed(at(start, 59)));
        assert!(clock.interval_elapsed(at(start, 60)));
    }

    #[test]
    fn an_anchor_seeds_the_committed_offset_so_the_first_commit_is_not_stale() {
        let start = Instant::now();
        let (_dir, store) = temp_store();
        let ownership = KeeperOwnership::new(store, CheckpointCadence::default());
        assert_eq!(ownership.committed_offset("s1"), 0);
        ownership.checkpoint_anchor("s1", 4096, start);
        assert_eq!(ownership.committed_offset("s1"), 4096);
        // An anchor below what is already committed never walks the offset backwards.
        ownership.checkpoint_committed("s1", 8192, start);
        ownership.checkpoint_anchor("s1", 4096, start);
        assert_eq!(ownership.committed_offset("s1"), 8192);
        // A zero anchor is not a seed: it would claim a commit that does not exist.
        ownership.checkpoint_anchor("s2", 0, start);
        assert_eq!(ownership.committed_offset("s2"), 0);
    }

    #[test]
    fn the_hub_counter_maps_to_the_keeper_offset_through_its_bias() {
        // The hub had published 100 bytes when the pump started, and its next byte is keeper byte
        // 40_000: the two axes differ by a constant that the bias must preserve.
        let bias = KeeperOffsetBias::new(100, 40_000);
        assert_eq!(bias.apply(100), 40_000);
        assert_eq!(bias.apply(150), 40_050);
        assert_eq!(bias.apply(1000), 40_900);
        // A hub that went backwards cannot name an offset below the base: that would commit state
        // covering bytes this pump never delivered.
        assert_eq!(bias.apply(0), 40_000);
        assert_eq!(bias.apply(99), 40_000);
        // No overflow at the top of the range.
        let edge = KeeperOffsetBias::new(0, u64::MAX - 1);
        assert_eq!(edge.apply(10), u64::MAX);
    }

    #[test]
    fn a_checkpoint_id_is_derived_from_the_epoch_and_offset() {
        assert_eq!(checkpoint_id_for(0x1f, 0x2000), "cp-1f-2000");
        // The same state under the same epoch names the same upload, so a retry releases the
        // superseded attempt instead of charging a second buffer.
        assert_eq!(checkpoint_id_for(7, 512), checkpoint_id_for(7, 512));
        assert_ne!(checkpoint_id_for(7, 512), checkpoint_id_for(8, 512));
        assert_ne!(checkpoint_id_for(7, 512), checkpoint_id_for(7, 513));
    }

    #[test]
    fn only_the_unknown_session_code_ends_a_pump_without_retrying() {
        use crate::daemon::keeper_client::KeeperClientError;
        let refused = |code: &str| {
            KeeperSessionError::Keeper(KeeperClientError::Refused {
                code: code.to_string(),
                message: String::new(),
            })
        };
        // A vanished session will not come back, so its pump ends on the code rather than spending
        // the retry budget on a registry that no longer holds it.
        assert!(keeper_reports_session_gone(&refused(KEEPER_CODE_UNKNOWN_SESSION)));
        // Everything else keeps the socket-shadowing behaviour: a busy keeper, an exhausted budget
        // and a dropped connection are all worth another poll, and a fenced epoch is neither gone
        // nor retryable -- it just ends the pump through the non-transient arm.
        assert!(!keeper_reports_session_gone(&refused("MEMORY_BUDGET_EXHAUSTED")));
        assert!(!keeper_reports_session_gone(&refused("EPOCH_MISMATCH")));
        assert!(!keeper_reports_session_gone(&KeeperSessionError::Keeper(
            KeeperClientError::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "UNKNOWN_SESSION in the text, which is not the code",
            ))
        )));
    }

    #[test]
    fn transience_is_decided_by_wire_code_and_never_by_message() {
        use crate::daemon::keeper_client::KeeperClientError;
        let io = KeeperSessionError::Keeper(KeeperClientError::Io(std::io::Error::new(
            std::io::ErrorKind::ConnectionRefused,
            "message that mentions EPOCH_MISMATCH to bait a message-sniffing classifier",
        )));
        assert!(io.is_transient(), "a dropped connection may come back");
        assert!(!KeeperSessionError::Keeper(KeeperClientError::Protocol(
            "STALE_CHECKPOINT in the text".to_string()
        ))
        .is_transient());
        assert!(!KeeperSessionError::Incompatible("io error".to_string()).is_transient());
        assert!(!KeeperSessionError::NotKeeperOwned {
            session_id: "s".to_string()
        }
        .is_transient());
        // A refusal is transient by default -- the keeper was busy, or a budget is exhausted -- but
        // the two codes that describe a permanently fenced condition are not, and are recognised by
        // their wire code alone.
        let refused = |code: &str| {
            KeeperSessionError::Keeper(KeeperClientError::Refused {
                code: code.to_string(),
                message: "text is irrelevant to the classification".to_string(),
            })
        };
        assert!(refused("CHECKPOINT_BUDGET_EXHAUSTED").is_transient());
        assert!(!refused("EPOCH_MISMATCH").is_transient());
        assert!(!refused("STALE_CHECKPOINT").is_transient());
        // A message that *says* EPOCH_MISMATCH under a different code is still transient: only the
        // code decides.
        assert!(KeeperSessionError::Keeper(KeeperClientError::Refused {
            code: "BUSY".to_string(),
            message: "EPOCH_MISMATCH".to_string(),
        })
        .is_transient());
    }

    #[test]
    fn a_suppressed_activation_reports_the_keeper_sessions_it_still_serves() {
        let suppressed = ActivationOutcome::Suppressed { keeper_sessions: 3 };
        assert!(!suppressed.is_active(), "a disable must not report durability for new sessions");
        assert_eq!(
            suppressed,
            ActivationOutcome::Suppressed { keeper_sessions: 3 }
        );
        assert!(ActivationOutcome::AlreadyActive.is_active());
    }
}
