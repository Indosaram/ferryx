//! Goal-aware wakes: dependency waits, run settles, explicit deadlines, owner controls and
//! ordinary reports become durable [`PendingWake`] rows, and one scheduler turns them into at
//! most one handler turn per goal and event set.
//!
//! Every tick ([`WakeScheduler::tick`]) runs in this order, all of it database work:
//! 1. Apply the durable state-event log (migration 0021: triggers on `supervisor_goals` and
//!    `supervisor_dispatches` write it in the transaction that made the change) from the
//!    persisted cursor. A child goal that completed satisfies, and one that was cancelled
//!    fails, every pending wait on it; a child hold is recorded on those waits as progress.
//!    A run that settled on the current generation persists a `turn_settled` wake, and a
//!    goal still running with pending waits moves to `waiting_children`/`waiting_event`. An
//!    owner correction (revision bump in place), pause, cancel or resume persists an owner
//!    wake. Every effect is keyed (wait + event id, goal + cause + event id), so replaying the
//!    log after a crash before the cursor moved changes nothing.
//! 2. Sweep pending child waits whose child already ended: events that arrived before the
//!    wait existed are reconciled, with the same event ids as step 1.
//! 3. Owner wakes of every goal go first, before any other wake of any goal: alone, at once,
//!    with no batch and no judgment, whatever the goal does. A goal whose owner turn the
//!    handler did not take keeps its other wakes pending behind it.
//! 4. Then, per goal, deterministic state checks decide the other wakes, with no model call.
//!    Every decisive wake of the goal is read, and its ordinary reports oldest first, at most
//!    `WAKE_LIMIT` per goal and tick, so one goal's backlog never hides another goal:
//!    - wakes of an ended goal are closed;
//!    - a held, degraded or uncertain goal, or a busy one (dispatch in flight, dispatching, or
//!      `ready` with an unsent intent the dispatcher will claim), keeps its wakes pending and
//!      only its status changes;
//!    - with dependencies pending, only a dependency failure or a due deadline releases the
//!      goal; dependency-ready and reports wait;
//!    - a dependency, settle or due-deadline wake releases an idle goal at once, together with
//!      every report queued for it, so the result needed for progress is never held back by
//!      batching or the judge;
//!    - ordinary reports alone follow the lane dispatcher's policy
//!      ([`crate::lanes::dispatch::batch_window_elapsed`]): admitted once the oldest is 30s
//!      old and the goal had no delivery in the last 30s, never extended by newer reports.
//! 5. Only then, and only for a report-only batch, the wake judge; its score is cached on the
//!    turn row keyed by event set + goal revision + execution generation, and a failing judge
//!    is asked again for that goal only after a doubling backoff. Then the handler.
//!
//! A turn is written `pending` before the handler runs and settled with its wake receipts
//! after the handler acknowledges, so a crash re-delivers it under the same turn id (the
//! handler's idempotency key) and never loses it; a busy handler, or one that cannot act on a
//! decisive wake in the goal's current state ([`WakeAck::NotApplicable`]), leaves everything
//! pending. A storage error that fails the same way on every retry (a constraint, a row that
//! does not decode) is confined to its goal, or stops the log at its event while persisted
//! wakes are still scheduled; any other storage error stops the tick.
//! The loop ([`WakeScheduler::run`]) sleeps until the next batch or deadline is due, a
//! [`poke`] (the store sends one with every goal transition and owner correction), or the
//! bounded [`WAKE_RESCAN`] (a database read): nothing calls a model while a goal waits.
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::SqlitePool;
use tokio::sync::{mpsc, Notify};

use super::{
    add_wait_at, enqueue_wake, execution_binding_of, goal_of, in_flight_dispatch, record_wait_event,
    sha256_hex, transition_goal, wake_from_row, PendingWake, RunState, SupervisorError,
    SupervisorGoal, Wait, WaitEventReceipt, WaitKind, WaitOutcome, WaitTarget, WakeCause, WakeRow,
    WAKE_COLUMNS,
};
use crate::lanes::dispatch::{batch_window_elapsed, delivered_recently, BATCH_WINDOW_SECS};
use crate::storage::WriteTx;

/// How often the scheduler rereads durable state when nothing wakes it: a database read,
/// never a model call. It picks up changes committed by another process.
pub const WAKE_RESCAN: Duration = Duration::from_secs(5);

/// State events applied per read of the log.
const EVENT_BATCH: i64 = 256;

/// Ordinary reports read per goal and tick, oldest first. Decisive wakes (owner, dependency,
/// settle, deadline) are never capped: each one can be the result the goal needs.
pub(crate) const WAKE_LIMIT: i64 = 4096;

/// Owner wake causes, as stored.
const OWNER_CAUSES: &str = "'owner_control','owner_correction'";

/// After the judge fails for a goal it is asked again only after this long, doubling per
/// consecutive failure up to `JUDGE_RETRY_MAX_SECS`.
const JUDGE_RETRY_MIN_SECS: i64 = 30;
const JUDGE_RETRY_MAX_SECS: i64 = 600;

/// This scheduler's row in `supervisor_event_cursors`.
const CURSOR: &str = "wakes";

/// A report-only batch scoring below this is receipted as skipped (the lane dispatcher's
/// threshold).
const JUDGE_THRESHOLD: f64 = 0.5;

/// Applied state events older than this are pruned.
const EVENT_RETENTION: &str = "-7 days";

const MAX_DETAIL_CHARS: usize = 2_000;

/// `state_detail` prefixes the scheduler and [`StateHandler`] write.
pub const WAITING_ON: &str = "WAITING_ON";
pub const DEPENDENCY_FAILED: &str = "DEPENDENCY_FAILED";
pub const DEPENDENCIES_MET: &str = "DEPENDENCIES_MET";
pub const TURN_SETTLED: &str = "TURN_SETTLED";
pub const DEADLINE_PASSED: &str = "DEADLINE_PASSED";
/// A settled run dispatched at an older goal revision than the goal has now (an owner
/// correction or hold came after it): the worker is idle, but the run is not evidence for the
/// current contract.
pub const STALE_SETTLE: &str = "STALE_SETTLE";

fn batch_window() -> chrono::Duration {
    chrono::Duration::seconds(BATCH_WINDOW_SECS)
}

/// The daemon's wake-scheduler signal, shared by every in-process writer.
pub fn wake_signal() -> Arc<Notify> {
    static WAKE: OnceLock<Arc<Notify>> = OnceLock::new();
    Arc::clone(WAKE.get_or_init(|| Arc::new(Notify::new())))
}

/// Wakes this process's scheduler. Without one (`ferryx-cli mcp`) it does nothing; the
/// daemon's rescan reads the durable rows instead.
pub fn poke() {
    wake_signal().notify_one();
}

/// One handler turn: the wakes it consumes and the goal as the scheduler checked it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WakeTurn {
    /// Stable across re-deliveries of the same turn: the handler's idempotency key.
    pub turn_id: i64,
    pub goal_id: String,
    pub goal_revision: i64,
    pub generation: i64,
    pub run_state: RunState,
    /// Admitted without the batch window and the judge.
    pub urgent: bool,
    pub wakes: Vec<PendingWake>,
}

impl WakeTurn {
    pub fn has(&self, cause: WakeCause) -> bool {
        self.wakes.iter().any(|wake| wake.cause == cause)
    }
}

/// A handler's answer. `Busy` keeps the turn and its wakes pending for a later tick.
/// `NotApplicable` keeps them pending too: the goal is in a state where the handler cannot act
/// on a decisive wake (a settle reaching a goal that resumed to `ready`), and receipting it as
/// delivered would lose it. It is offered again on later ticks and applies once the goal
/// reaches a state where it can.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeAck {
    Done,
    Busy,
    NotApplicable,
}

/// What acts on an admitted wake turn: the deterministic [`StateHandler`], or a decision
/// handler built on it. Runs outside every store transaction.
#[async_trait]
pub trait WakeHandler: Send + Sync {
    async fn handle(&self, turn: &WakeTurn) -> WakeAck;
}

/// Scores a report-only batch in `0.0..=1.0`; below 0.5 it is skipped.
#[async_trait]
pub trait WakeJudge: Send + Sync {
    async fn score(&self, state: &str) -> Result<f64, String>;
}

#[async_trait]
impl WakeJudge for crate::judge::Judge {
    async fn score(&self, state: &str) -> Result<f64, String> {
        self.noul(
            crate::judge::Purpose::EventWake,
            state,
            "Does this event need the lead agent's judgment now?",
        )
        .await
        .map_err(|error| error.to_string())
    }
}

/// The scheduler's notion of now, injected so batching is testable on a virtual clock.
pub trait WakeClock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

pub struct SystemClock;

impl WakeClock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// Why a goal's wakes stayed pending this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Deferral {
    /// Owner hold, degraded or delivery uncertain: status only.
    Held(RunState),
    /// A dispatch is in flight, the goal is dispatching, or it is `ready` with an unsent
    /// intent the dispatcher will claim.
    Busy,
    /// Dependencies are still pending and nothing releases the goal.
    Dependencies,
    /// Ordinary reports inside the batch window or the delivery spacing.
    Batching,
    HandlerBusy,
    /// The handler cannot act on a decisive wake in this run state; it stays pending.
    NotApplicable(RunState),
    /// The judge failed; it is asked again after a backoff (`next_due`).
    JudgeUnavailable,
}

/// What one tick did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TickReport {
    /// State events applied from the log.
    pub events: usize,
    /// Waits newly satisfied or failed.
    pub resolved_waits: usize,
    /// Goals moved into `waiting_children`/`waiting_event`.
    pub waiting: Vec<String>,
    /// Turns the handler acknowledged.
    pub delivered: Vec<i64>,
    /// Report-only turns the judge skipped.
    pub skipped: Vec<i64>,
    /// Wakes closed because their goal had ended.
    pub closed: usize,
    pub deferred: Vec<(String, Deferral)>,
    /// When the earliest deferred batch or deadline becomes due.
    pub next_due: Option<DateTime<Utc>>,
}

impl TickReport {
    fn due_at(&mut self, at: DateTime<Utc>) {
        self.next_due = Some(self.next_due.map_or(at, |due| due.min(at)));
    }
}

/// A refused dependency link. Every refusal is a client error (HTTP 400); nothing was written.
#[derive(Debug, thiserror::Error)]
pub enum DependencyError {
    #[error("goal {0} cannot depend on itself")]
    SelfDependency(String),
    #[error("goal {goal} depending on {child} would close a dependency cycle")]
    Cycle { goal: String, child: String },
    #[error("{0} belongs to a different owner")]
    CrossOwner(String),
    #[error(transparent)]
    Store(SupervisorError),
}

impl DependencyError {
    pub fn code(&self) -> &'static str {
        match self {
            DependencyError::SelfDependency(_) | DependencyError::Cycle { .. } => "WAIT_CYCLE",
            DependencyError::CrossOwner(_) => "CROSS_OWNER",
            DependencyError::Store(error) => error.code(),
        }
    }

    /// HTTP status of the shared API: a refused link is 400.
    pub fn http_status(&self) -> u16 {
        match self {
            DependencyError::SelfDependency(_)
            | DependencyError::Cycle { .. }
            | DependencyError::CrossOwner(_) => 400,
            DependencyError::Store(error) => match error.code() {
                "INVALID_REQUEST" => 400,
                "NOT_FOUND" => 404,
                "REVISION_CONFLICT" | "GOAL_CLOSED" => 409,
                _ => 500,
            },
        }
    }
}

impl From<SupervisorError> for DependencyError {
    fn from(error: SupervisorError) -> Self {
        match error {
            SupervisorError::SelfWait(goal) => DependencyError::SelfDependency(goal),
            SupervisorError::WaitCycle { goal, child } => DependencyError::Cycle { goal, child },
            SupervisorError::CrossOwner(what) => DependencyError::CrossOwner(what),
            other => DependencyError::Store(other),
        }
    }
}

/// Makes `goal_id` wait on a child goal or an external event. Self-dependencies, cycles and
/// links to another owner's goal are refused before commit. A child that already ended
/// resolves the new wait at once (an event that arrived before the wait is never lost).
/// Returns the wait as created.
pub async fn add_dependency(
    pool: &SqlitePool,
    goal_id: &str,
    target: &WaitTarget,
) -> Result<Wait, DependencyError> {
    add_dependency_at(pool, goal_id, target, None).await
}

/// [`add_dependency`] under a compare-and-set on the goal revision, checked inside the wait's
/// write transaction (`REVISION_CONFLICT`, nothing written). The owner request API uses it.
pub async fn add_dependency_at(
    pool: &SqlitePool,
    goal_id: &str,
    target: &WaitTarget,
    expected_revision: Option<i64>,
) -> Result<Wait, DependencyError> {
    let wait = add_wait_at(pool, goal_id, target, expected_revision).await?;
    if let WaitTarget::ChildGoal(child_id) = target {
        if let Some(child) = goal_of(pool, child_id).await? {
            if child.run_state.is_terminal() {
                resolve_child_waits(pool, child_id, child.run_state).await?;
            }
        }
    }
    poke();
    Ok(wait)
}

/// Moves a goal with pending waits into `waiting_children` (any child wait) or
/// `waiting_event` (external waits only). Compare-and-set on revision and state.
pub async fn enter_waiting(
    pool: &SqlitePool,
    goal_id: &str,
    expected_revision: i64,
    expected_state: RunState,
) -> Result<SupervisorGoal, SupervisorError> {
    let pending = pending_wait_kinds(pool, goal_id).await?;
    let to = if pending.contains(&WaitKind::ChildGoal) {
        RunState::WaitingChildren
    } else if pending.is_empty() {
        return Err(SupervisorError::Invalid(format!(
            "goal {goal_id} has no pending wait"
        )));
    } else {
        RunState::WaitingEvent
    };
    let detail = format!("{WAITING_ON}: {} pending dependencies", pending.len());
    transition_goal(
        pool,
        goal_id,
        expected_revision,
        expected_state,
        to,
        Some(&detail),
    )
    .await
}

/// Delivers one external event to every pending wait of `supervisor_id` on `event_key`.
/// A replay (same event id) or an event after the wait resolved changes nothing.
pub async fn deliver_event(
    pool: &SqlitePool,
    supervisor_id: &str,
    event_key: &str,
    event_id: &str,
    outcome: WaitOutcome,
    evidence: &[String],
) -> Result<Vec<WaitEventReceipt>, SupervisorError> {
    let waits: Vec<String> = sqlx::query_scalar(
        "SELECT wait_id FROM supervisor_waits
         WHERE supervisor_id = ? AND kind = 'external_event' AND event_key = ?
           AND state = 'pending'
         ORDER BY rowid",
    )
    .bind(supervisor_id)
    .bind(event_key)
    .fetch_all(pool)
    .await?;
    let mut receipts = Vec::with_capacity(waits.len());
    for wait_id in waits {
        receipts.push(record_wait_event(pool, &wait_id, event_id, outcome, evidence).await?);
    }
    poke();
    Ok(receipts)
}

/// Queues an ordinary worker report for `goal_id`, received at `received_at`. It joins the
/// goal's batch: due one batch window after it arrived, admitted with the oldest. A replay of
/// the same event id is the same wake.
pub async fn report(
    pool: &SqlitePool,
    goal_id: &str,
    event_id: &str,
    received_at: DateTime<Utc>,
) -> Result<PendingWake, SupervisorError> {
    let wake = enqueue_wake(
        pool,
        goal_id,
        WakeCause::Report,
        event_id,
        received_at + batch_window(),
    )
    .await?;
    poke();
    Ok(wake)
}

/// Persists an explicit deadline for `goal_id`; it wakes the goal once `due_at` passes.
pub async fn schedule_deadline(
    pool: &SqlitePool,
    goal_id: &str,
    deadline_id: &str,
    due_at: DateTime<Utc>,
) -> Result<PendingWake, SupervisorError> {
    let wake = enqueue_wake(pool, goal_id, WakeCause::Deadline, deadline_id, due_at).await?;
    poke();
    Ok(wake)
}

/// One row of `supervisor_state_events`.
#[derive(Debug, sqlx::FromRow)]
struct StateEvent {
    seq: i64,
    goal_id: String,
    source: String,
    request_id: Option<String>,
    from_state: Option<String>,
    to_state: String,
    from_revision: Option<i64>,
    to_revision: Option<i64>,
    generation: Option<i64>,
}

/// A wake set the scheduler decided to admit.
struct Batch {
    wakes: Vec<PendingWake>,
    urgent: bool,
    /// Report-only: the judge decides before the handler.
    judged: bool,
}

fn is_owner(cause: WakeCause) -> bool {
    matches!(cause, WakeCause::OwnerControl | WakeCause::OwnerCorrection)
}

/// Wakes whose loss would lose a run result, a dependency outcome or a deadline.
fn is_decisive(cause: WakeCause) -> bool {
    matches!(
        cause,
        WakeCause::TurnSettled
            | WakeCause::DependencyReady
            | WakeCause::DependencyFailed
            | WakeCause::Deadline
    )
}

/// A storage error that may pass on retry (busy database, pool, I/O). One that fails the same
/// way every time (a violated constraint, a row that does not decode) is not: it is confined
/// to the goal or log event it came from instead of stopping every goal's wakes.
fn transient(error: &SupervisorError) -> bool {
    let SupervisorError::Sql(error) = error else {
        return false;
    };
    let deterministic = match error {
        sqlx::Error::Database(database) => {
            !matches!(database.kind(), sqlx::error::ErrorKind::Other)
        }
        sqlx::Error::RowNotFound
        | sqlx::Error::TypeNotFound { .. }
        | sqlx::Error::ColumnIndexOutOfBounds { .. }
        | sqlx::Error::ColumnNotFound(_)
        | sqlx::Error::ColumnDecode { .. }
        | sqlx::Error::Decode(_) => true,
        _ => false,
    };
    !deterministic
}

/// The owner wake a goal state event carries: a revision bump in place is a correction;
/// entering pause or cancel, or leaving an owner hold, is a control. A hold the system
/// entered (`awaiting_owner`) wakes nothing: the goal now waits on the owner.
fn owner_cause(
    from: Option<RunState>,
    to: RunState,
    from_revision: Option<i64>,
    to_revision: Option<i64>,
) -> Option<WakeCause> {
    let bumped = matches!((from_revision, to_revision), (Some(old), Some(new)) if new > old);
    if bumped && from == Some(to) {
        return Some(WakeCause::OwnerCorrection);
    }
    if matches!(to, RunState::Paused | RunState::Cancelled) {
        return Some(WakeCause::OwnerControl);
    }
    if from.is_some_and(|from| matches!(from, RunState::Paused | RunState::AwaitingOwner))
        && !to.is_owner_hold()
    {
        return Some(WakeCause::OwnerControl);
    }
    None
}

fn parse_time(text: &str, fallback: DateTime<Utc>) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text)
        .map(|at| at.with_timezone(&Utc))
        .unwrap_or(fallback)
}

fn truncate(text: &str) -> String {
    text.chars().take(MAX_DETAIL_CHARS).collect()
}

async fn pending_wait_kinds(
    pool: &SqlitePool,
    goal_id: &str,
) -> Result<Vec<WaitKind>, SupervisorError> {
    let kinds: Vec<String> = sqlx::query_scalar(
        "SELECT kind FROM supervisor_waits WHERE goal_id = ? AND state = 'pending' ORDER BY rowid",
    )
    .bind(goal_id)
    .fetch_all(pool)
    .await?;
    kinds.iter().map(|kind| kind.parse::<WaitKind>()).collect()
}

/// The worker is busy with the goal: a dispatch in flight on the current generation, or an
/// unsent intent the dispatcher will claim. The dispatcher claims only from a `ready` goal at
/// the intent's revision (see [`super::ready_dispatches`]); an intent queued in any other
/// state (a correction enqueued while the run still goes) waits for the goal to reach `ready`
/// and does not make it busy, or a settled run would never move the goal on.
async fn goal_busy(pool: &SqlitePool, goal_id: &str) -> Result<bool, SupervisorError> {
    let mut conn = pool.acquire().await?;
    if in_flight_dispatch(&mut conn, goal_id).await?.is_some() {
        return Ok(true);
    }
    let unsent: bool = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM supervisor_dispatches d
            JOIN supervisor_goals g ON g.goal_id = d.goal_id
            WHERE d.goal_id = ? AND d.state = 'ready'
              AND g.run_state = 'ready' AND g.revision = d.goal_revision)",
    )
    .bind(goal_id)
    .fetch_one(&mut *conn)
    .await?;
    Ok(unsent)
}

async fn pending_child_waits(
    pool: &SqlitePool,
    child_id: &str,
) -> Result<Vec<String>, SupervisorError> {
    Ok(sqlx::query_scalar(
        "SELECT wait_id FROM supervisor_waits
         WHERE child_goal_id = ? AND state = 'pending' ORDER BY rowid",
    )
    .bind(child_id)
    .fetch_all(pool)
    .await?)
}

async fn child_evidence(
    pool: &SqlitePool,
    child_id: &str,
    state: RunState,
) -> Result<Vec<String>, SupervisorError> {
    let mut evidence = vec![format!("child goal {child_id} is {state}")];
    if let Some(detail) = goal_of(pool, child_id)
        .await?
        .and_then(|child| child.state_detail)
    {
        evidence.push(truncate(&detail));
    }
    Ok(evidence)
}

/// Resolves every pending wait on a child that ended: completed satisfies, cancelled fails.
/// The event id names the child and its final state, so every path that sees the same end
/// (the log, the sweep, a new wait) records it once. Returns the waits it resolved.
async fn resolve_child_waits(
    pool: &SqlitePool,
    child_id: &str,
    state: RunState,
) -> Result<usize, SupervisorError> {
    let outcome = match state {
        RunState::Completed => WaitOutcome::Satisfied,
        RunState::Cancelled => WaitOutcome::Failed,
        _ => return Ok(0),
    };
    let waits = pending_child_waits(pool, child_id).await?;
    if waits.is_empty() {
        return Ok(0);
    }
    let evidence = child_evidence(pool, child_id, state).await?;
    let event_id = format!("goal:{child_id}:{state}");
    let mut resolved = 0;
    for wait_id in waits {
        let receipt = record_wait_event(pool, &wait_id, &event_id, outcome, &evidence).await?;
        if receipt.inserted && receipt.wait.resolved_at.is_some() {
            resolved += 1;
        }
    }
    Ok(resolved)
}

/// Records a child's hold on the waits of its parents as progress: status only, no wake.
async fn note_child_progress(
    pool: &SqlitePool,
    child_id: &str,
    state: RunState,
    seq: i64,
) -> Result<(), SupervisorError> {
    let waits = pending_child_waits(pool, child_id).await?;
    if waits.is_empty() {
        return Ok(());
    }
    let evidence = child_evidence(pool, child_id, state).await?;
    let event_id = format!("state:{seq}");
    for wait_id in waits {
        record_wait_event(pool, &wait_id, &event_id, WaitOutcome::Progress, &evidence).await?;
    }
    Ok(())
}

/// Every failed dependency of the goal, as hold detail.
async fn failure_detail(pool: &SqlitePool, goal_id: &str) -> Result<String, SupervisorError> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT e.event_id, e.evidence_json FROM supervisor_wait_events e
         JOIN supervisor_waits w ON w.wait_id = e.wait_id
         WHERE w.goal_id = ? AND e.outcome = 'failed'
         ORDER BY e.received_at, e.event_id",
    )
    .bind(goal_id)
    .fetch_all(pool)
    .await?;
    let failures: Vec<String> = rows
        .into_iter()
        .map(|(event_id, evidence)| {
            let evidence: Vec<String> = serde_json::from_str(&evidence).unwrap_or_default();
            if evidence.is_empty() {
                event_id
            } else {
                format!("{event_id} ({})", evidence.join("; "))
            }
        })
        .collect();
    Ok(truncate(&format!(
        "{DEPENDENCY_FAILED}: {}; a failed dependency is not success",
        failures.join(", ")
    )))
}

/// Every unconsumed owner wake of every goal, by goal, oldest first.
async fn owner_wakes(pool: &SqlitePool) -> Result<Vec<PendingWake>, SupervisorError> {
    let sql = format!(
        "SELECT {WAKE_COLUMNS} FROM supervisor_pending_wakes
         WHERE consumed_at IS NULL AND cause IN ({OWNER_CAUSES}) ORDER BY goal_id, id"
    );
    let rows: Vec<WakeRow> = sqlx::query_as(&sql).fetch_all(pool).await?;
    rows.into_iter().map(wake_from_row).collect()
}

/// Every goal with an unconsumed wake.
async fn goals_with_wakes(pool: &SqlitePool) -> Result<Vec<String>, SupervisorError> {
    Ok(sqlx::query_scalar(
        "SELECT DISTINCT goal_id FROM supervisor_pending_wakes
         WHERE consumed_at IS NULL ORDER BY goal_id",
    )
    .fetch_all(pool)
    .await?)
}

/// The goal's unconsumed wakes other than owner wakes, in id order: every decisive one and
/// its oldest `WAKE_LIMIT` reports.
async fn goal_wakes(pool: &SqlitePool, goal_id: &str) -> Result<Vec<PendingWake>, SupervisorError> {
    let decisive = format!(
        "SELECT {WAKE_COLUMNS} FROM supervisor_pending_wakes
         WHERE goal_id = ? AND consumed_at IS NULL AND cause NOT IN ({OWNER_CAUSES}, 'report')
         ORDER BY id"
    );
    let reports = format!(
        "SELECT {WAKE_COLUMNS} FROM supervisor_pending_wakes
         WHERE goal_id = ? AND consumed_at IS NULL AND cause = 'report'
         ORDER BY id LIMIT ?"
    );
    let mut rows: Vec<WakeRow> = sqlx::query_as(&decisive)
        .bind(goal_id)
        .fetch_all(pool)
        .await?;
    let page: Vec<WakeRow> = sqlx::query_as(&reports)
        .bind(goal_id)
        .bind(WAKE_LIMIT)
        .fetch_all(pool)
        .await?;
    rows.extend(page);
    let mut wakes = rows
        .into_iter()
        .map(wake_from_row)
        .collect::<Result<Vec<_>, _>>()?;
    wakes.sort_unstable_by_key(|wake| wake.id);
    Ok(wakes)
}

/// When the goal's last turn was delivered (the batch spacing).
async fn last_delivery(
    pool: &SqlitePool,
    goal_id: &str,
    now: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>, SupervisorError> {
    let last: Option<String> = sqlx::query_scalar(
        "SELECT settled_at FROM supervisor_wake_turns
         WHERE goal_id = ? AND state = 'delivered' AND settled_at IS NOT NULL
         ORDER BY julianday(settled_at) DESC, id DESC LIMIT 1",
    )
    .bind(goal_id)
    .fetch_optional(pool)
    .await?;
    Ok(last.map(|last| parse_time(&last, now)))
}

/// Settles a turn (or, with no turn, closes wakes of an ended goal): one receipt per wake and
/// the wakes consumed, in one transaction.
async fn settle_wakes(
    pool: &SqlitePool,
    turn: Option<(i64, &str)>,
    wake_ids: &[i64],
    now: DateTime<Utc>,
) -> Result<(), SupervisorError> {
    let stamp = now.to_rfc3339();
    let mut tx = pool.begin_write().await?;
    let (turn_id, disposition) = match turn {
        Some((turn_id, state)) => {
            sqlx::query(
                "UPDATE supervisor_wake_turns SET state = ?, settled_at = ?
                 WHERE id = ? AND state = 'pending'",
            )
            .bind(state)
            .bind(&stamp)
            .bind(turn_id)
            .execute(&mut *tx)
            .await?;
            (Some(turn_id), state)
        }
        None => (None, "closed"),
    };
    for wake_id in wake_ids {
        sqlx::query(
            "INSERT INTO supervisor_wake_receipts(wake_id, turn_id, disposition, recorded_at)
             VALUES(?,?,?,?) ON CONFLICT(wake_id) DO NOTHING",
        )
        .bind(wake_id)
        .bind(turn_id)
        .bind(disposition)
        .bind(&stamp)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE supervisor_pending_wakes SET consumed_at = ?
             WHERE id = ? AND consumed_at IS NULL",
        )
        .bind(&stamp)
        .bind(wake_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// The scheduler. One per hub generation; see the module docs for the tick order.
pub struct WakeScheduler {
    pool: SqlitePool,
    handler: Arc<dyn WakeHandler>,
    judge: Option<Arc<dyn WakeJudge>>,
    clock: Arc<dyn WakeClock>,
    rescan: Duration,
    /// Told what each loop tick did, after the tick's wake subscription.
    ticked: Option<mpsc::UnboundedSender<TickReport>>,
    /// Per goal: when a judge that failed may be asked again, and its consecutive failures.
    judge_backoff: Mutex<HashMap<String, (DateTime<Utc>, u32)>>,
}

impl WakeScheduler {
    pub fn new(pool: SqlitePool, handler: Arc<dyn WakeHandler>) -> Self {
        Self {
            pool,
            handler,
            judge: None,
            clock: Arc::new(SystemClock),
            rescan: WAKE_RESCAN,
            ticked: None,
            judge_backoff: Mutex::new(HashMap::new()),
        }
    }

    /// Judges report-only batches before the handler. Without a judge they go straight to
    /// the handler once due.
    pub fn with_judge(mut self, judge: Arc<dyn WakeJudge>) -> Self {
        self.judge = Some(judge);
        self
    }

    pub fn with_clock(mut self, clock: Arc<dyn WakeClock>) -> Self {
        self.clock = clock;
        self
    }

    pub fn with_rescan(mut self, rescan: Duration) -> Self {
        self.rescan = rescan;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_tick_probe(mut self, probe: mpsc::UnboundedSender<TickReport>) -> Self {
        self.ticked = Some(probe);
        self
    }

    /// Ticks at start (recovering everything durable), then on every [`poke`], when the
    /// earliest deferred batch or deadline is due, and on the bounded rescan.
    pub async fn run(self, wake: Arc<Notify>) {
        loop {
            let notified = wake.notified();
            tokio::pin!(notified);
            // Subscribe before ticking: a poke during the tick is kept, not lost.
            notified.as_mut().enable();
            let now = self.clock.now();
            let report = match self.tick(now).await {
                Ok(report) => report,
                Err(error) => {
                    tracing::warn!(%error, "supervisor wake tick failed; every wake stays pending");
                    TickReport::default()
                }
            };
            let pause = match report.next_due {
                Some(due) => (due - now)
                    .to_std()
                    .unwrap_or(Duration::ZERO)
                    .min(self.rescan),
                None => self.rescan,
            };
            if let Some(probe) = &self.ticked {
                let _ = probe.send(report);
            }
            tokio::select! {
                () = &mut notified => {}
                () = tokio::time::sleep(pause) => {}
            }
        }
    }

    /// One pass at `now`. A storage error that may pass on retry stops the pass with nothing
    /// lost: the cursor only moves after its events applied, and wakes are consumed only with
    /// their receipts. One that fails the same way every time is confined: the log stops at
    /// its event (persisted wakes are still scheduled), or that one goal's wakes stay pending.
    pub async fn tick(&self, now: DateTime<Utc>) -> Result<TickReport, SupervisorError> {
        let mut report = TickReport::default();
        if let Err(error) = self.apply_state_events(now, &mut report).await {
            if transient(&error) {
                return Err(error);
            }
            tracing::warn!(%error, "supervisor state log stopped at an event that cannot apply; pending wakes are still scheduled");
        }
        if let Err(error) = self.sweep_ended_children(&mut report).await {
            if transient(&error) {
                return Err(error);
            }
            tracing::warn!(%error, "supervisor child sweep failed; it is retried next tick");
        }
        // Owner precedence across goals: every goal's owner wakes before any other scheduling.
        let mut owners = BTreeMap::<String, Vec<PendingWake>>::new();
        for wake in owner_wakes(&self.pool).await? {
            owners.entry(wake.goal_id.clone()).or_default().push(wake);
        }
        let mut owner_pending = BTreeSet::new();
        for (goal_id, wakes) in owners {
            let taken = match self.schedule_owner(&goal_id, wakes, now, &mut report).await {
                Ok(taken) => taken,
                Err(error) if transient(&error) => return Err(error),
                Err(error) => {
                    tracing::warn!(%error, goal_id = %goal_id, "supervisor owner wakes were not delivered; they stay pending");
                    false
                }
            };
            if !taken {
                owner_pending.insert(goal_id);
            }
        }
        for goal_id in goals_with_wakes(&self.pool).await? {
            if owner_pending.contains(&goal_id) {
                // Nothing overtakes an owner wake the handler has not taken yet.
                continue;
            }
            let scheduled = match goal_wakes(&self.pool, &goal_id).await {
                Ok(wakes) if wakes.is_empty() => Ok(()),
                Ok(wakes) => self.schedule_goal(&goal_id, wakes, now, &mut report).await,
                Err(error) => Err(error),
            };
            match scheduled {
                Ok(()) => {}
                Err(error) if transient(&error) => return Err(error),
                Err(error) => {
                    tracing::warn!(%error, goal_id = %goal_id, "supervisor wakes were not scheduled; they stay pending");
                }
            }
        }
        Ok(report)
    }

    /// A goal's owner wakes, at once and alone, whatever the goal does. Returns whether they
    /// were consumed; when not (the handler was busy) the goal's other wakes wait behind them.
    async fn schedule_owner(
        &self,
        goal_id: &str,
        wakes: Vec<PendingWake>,
        now: DateTime<Utc>,
        report: &mut TickReport,
    ) -> Result<bool, SupervisorError> {
        let Some(goal) = goal_of(&self.pool, goal_id).await? else {
            return Ok(false);
        };
        let generation = execution_binding_of(&self.pool, goal_id)
            .await?
            .map_or(0, |binding| binding.generation);
        let batch = Batch {
            wakes,
            urgent: true,
            judged: false,
        };
        self.deliver(&goal, generation, batch, now, report).await
    }

    /// When the judge may next be asked about `goal_id` after it failed, if it did.
    fn judge_retry_at(&self, goal_id: &str) -> Option<DateTime<Utc>> {
        self.judge_backoff
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(goal_id)
            .map(|(retry_at, _)| *retry_at)
    }

    /// Records a failed judgment for `goal_id`; returns when to ask again.
    fn judge_failed(&self, goal_id: &str, now: DateTime<Utc>) -> DateTime<Utc> {
        let mut backoff = self
            .judge_backoff
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let entry = backoff.entry(goal_id.to_owned()).or_insert((now, 0));
        entry.1 = entry.1.saturating_add(1);
        let delay = (JUDGE_RETRY_MIN_SECS << (entry.1 - 1).min(5)).min(JUDGE_RETRY_MAX_SECS);
        entry.0 = now + chrono::Duration::seconds(delay);
        entry.0
    }

    fn judge_answered(&self, goal_id: &str) {
        self.judge_backoff
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(goal_id);
    }

    async fn apply_state_events(
        &self,
        now: DateTime<Utc>,
        report: &mut TickReport,
    ) -> Result<(), SupervisorError> {
        loop {
            let cursor: i64 = sqlx::query_scalar(
                "SELECT COALESCE(
                    (SELECT last_seq FROM supervisor_event_cursors WHERE consumer = ?), 0)",
            )
            .bind(CURSOR)
            .fetch_one(&self.pool)
            .await?;
            let events: Vec<StateEvent> = sqlx::query_as(
                "SELECT seq, goal_id, source, request_id, from_state, to_state, from_revision,
                        to_revision, generation
                 FROM supervisor_state_events WHERE seq > ? ORDER BY seq LIMIT ?",
            )
            .bind(cursor)
            .bind(EVENT_BATCH)
            .fetch_all(&self.pool)
            .await?;
            let Some(last) = events.last().map(|event| event.seq) else {
                return Ok(());
            };
            for event in &events {
                match self.apply_state_event(event, now, report).await {
                    Ok(()) => {}
                    Err(SupervisorError::Sql(error)) => return Err(SupervisorError::Sql(error)),
                    // One unusable event must not stall the log behind it.
                    Err(error) => {
                        tracing::warn!(%error, seq = event.seq, goal_id = %event.goal_id, "supervisor state event was not applied");
                    }
                }
                report.events += 1;
            }
            sqlx::query(
                "INSERT INTO supervisor_event_cursors(consumer, last_seq, updated_at)
                 VALUES(?,?,?)
                 ON CONFLICT(consumer) DO UPDATE SET
                    last_seq = MAX(supervisor_event_cursors.last_seq, excluded.last_seq),
                    updated_at = excluded.updated_at",
            )
            .bind(CURSOR)
            .bind(last)
            .bind(now.to_rfc3339())
            .execute(&self.pool)
            .await?;
            if (events.len() as i64) < EVENT_BATCH {
                sqlx::query(
                    "DELETE FROM supervisor_state_events
                     WHERE seq <= ? AND julianday(recorded_at) < julianday('now', ?)",
                )
                .bind(last)
                .bind(EVENT_RETENTION)
                .execute(&self.pool)
                .await?;
                return Ok(());
            }
        }
    }

    async fn apply_state_event(
        &self,
        event: &StateEvent,
        now: DateTime<Utc>,
        report: &mut TickReport,
    ) -> Result<(), SupervisorError> {
        match event.source.as_str() {
            "goal" => self.apply_goal_event(event, now, report).await,
            "dispatch" if event.to_state == "settled" => {
                self.apply_settle(event, now, report).await
            }
            _ => Ok(()),
        }
    }

    async fn apply_goal_event(
        &self,
        event: &StateEvent,
        now: DateTime<Utc>,
        report: &mut TickReport,
    ) -> Result<(), SupervisorError> {
        let to: RunState = event.to_state.parse()?;
        let from = event
            .from_state
            .as_deref()
            .map(str::parse::<RunState>)
            .transpose()?;
        match to {
            RunState::Completed | RunState::Cancelled => {
                report.resolved_waits +=
                    resolve_child_waits(&self.pool, &event.goal_id, to).await?;
            }
            RunState::Degraded
            | RunState::AwaitingOwner
            | RunState::DeliveryUncertain
            | RunState::Paused => {
                note_child_progress(&self.pool, &event.goal_id, to, event.seq).await?;
            }
            _ => {}
        }
        if let Some(cause) = owner_cause(from, to, event.from_revision, event.to_revision) {
            let event_id = format!("state:{}", event.seq);
            enqueue_wake(&self.pool, &event.goal_id, cause, &event_id, now).await?;
        }
        Ok(())
    }

    /// A run settled: persist its `turn_settled` wake (never dropped, only deferred while
    /// dependencies are pending) and, when the goal is still `running` with pending waits and
    /// nothing in flight, move it to waiting without any model call. A settle from a fenced
    /// generation is ignored.
    ///
    /// A settle whose dispatch answered an older goal revision is still kept: it is the only
    /// sign that the worker went idle, and dropping it would strand a corrected or resumed goal
    /// in `running`. It never counts as evidence for the current contract: [`StateHandler`]
    /// marks such a move [`STALE_SETTLE`], and the decision handler must check the revision.
    async fn apply_settle(
        &self,
        event: &StateEvent,
        now: DateTime<Utc>,
        report: &mut TickReport,
    ) -> Result<(), SupervisorError> {
        let Some(request_id) = event.request_id.as_deref() else {
            return Ok(());
        };
        let Some(goal) = goal_of(&self.pool, &event.goal_id).await? else {
            return Ok(());
        };
        if goal.run_state.is_terminal() {
            return Ok(());
        }
        let Some(binding) = execution_binding_of(&self.pool, &goal.goal_id).await? else {
            return Ok(());
        };
        if event.generation != Some(binding.generation) {
            return Ok(());
        }
        let event_id = format!("dispatch:{request_id}:settled");
        enqueue_wake(
            &self.pool,
            &goal.goal_id,
            WakeCause::TurnSettled,
            &event_id,
            now,
        )
        .await?;
        if goal.run_state == RunState::Running
            && !pending_wait_kinds(&self.pool, &goal.goal_id)
                .await?
                .is_empty()
            && !goal_busy(&self.pool, &goal.goal_id).await?
        {
            match enter_waiting(&self.pool, &goal.goal_id, goal.revision, RunState::Running).await {
                Ok(_) => report.waiting.push(goal.goal_id.clone()),
                Err(SupervisorError::Sql(error)) => return Err(SupervisorError::Sql(error)),
                Err(error) => {
                    tracing::debug!(%error, goal_id = %goal.goal_id, "goal did not move to waiting");
                }
            }
        }
        Ok(())
    }

    /// Already-arrived child ends: pending waits whose child completed or was cancelled.
    async fn sweep_ended_children(&self, report: &mut TickReport) -> Result<(), SupervisorError> {
        let ended: Vec<(String, String)> = sqlx::query_as(
            "SELECT DISTINCT g.goal_id, g.run_state FROM supervisor_waits w
             JOIN supervisor_goals g ON g.goal_id = w.child_goal_id
             WHERE w.state = 'pending' AND g.run_state IN ('completed','cancelled')",
        )
        .fetch_all(&self.pool)
        .await?;
        for (child_id, state) in ended {
            let state = state.parse::<RunState>()?;
            report.resolved_waits += resolve_child_waits(&self.pool, &child_id, state).await?;
        }
        Ok(())
    }

    /// The deterministic checks for one goal's unconsumed wakes other than owner wakes (those
    /// were handled for every goal first). Nothing here calls a model.
    async fn schedule_goal(
        &self,
        goal_id: &str,
        wakes: Vec<PendingWake>,
        now: DateTime<Utc>,
        report: &mut TickReport,
    ) -> Result<(), SupervisorError> {
        let Some(goal) = goal_of(&self.pool, goal_id).await? else {
            return Ok(());
        };
        let generation = execution_binding_of(&self.pool, goal_id)
            .await?
            .map_or(0, |binding| binding.generation);
        // Owner wakes went out before any goal was scheduled (see `tick`); never batch one.
        let rest: Vec<PendingWake> = wakes
            .into_iter()
            .filter(|wake| !is_owner(wake.cause))
            .collect();
        if goal.run_state.is_terminal() {
            let ids: Vec<i64> = rest.iter().map(|wake| wake.id).collect();
            settle_wakes(&self.pool, None, &ids, now).await?;
            super::changes::announce(&goal.goal_id);
            report.closed += ids.len();
            return Ok(());
        }
        let mut present = Vec::with_capacity(rest.len());
        let mut next_due: Option<DateTime<Utc>> = None;
        for wake in rest {
            let due = parse_time(&wake.due_at, now);
            if wake.cause == WakeCause::Deadline && due > now {
                next_due = Some(next_due.map_or(due, |at| at.min(due)));
                continue;
            }
            present.push(wake);
        }
        if let Some(due) = next_due {
            report.due_at(due);
        }
        if present.is_empty() {
            return Ok(());
        }
        let deferral = if matches!(
            goal.run_state,
            RunState::Paused
                | RunState::AwaitingOwner
                | RunState::Degraded
                | RunState::DeliveryUncertain
        ) {
            Some(Deferral::Held(goal.run_state))
        } else if goal.run_state == RunState::Dispatching || goal_busy(&self.pool, goal_id).await? {
            Some(Deferral::Busy)
        } else {
            None
        };
        if let Some(deferral) = deferral {
            report.deferred.push((goal_id.to_owned(), deferral));
            return Ok(());
        }
        let waits_pending = !pending_wait_kinds(&self.pool, goal_id).await?.is_empty();
        let decisive = present.iter().any(|wake| wake.cause != WakeCause::Report);
        if waits_pending {
            let releases = present.iter().any(|wake| {
                matches!(
                    wake.cause,
                    WakeCause::DependencyFailed | WakeCause::Deadline
                )
            });
            if !releases {
                report
                    .deferred
                    .push((goal_id.to_owned(), Deferral::Dependencies));
                return Ok(());
            }
        }
        if waits_pending || decisive {
            let batch = Batch {
                wakes: present,
                urgent: true,
                judged: false,
            };
            return self
                .deliver(&goal, generation, batch, now, report)
                .await
                .map(drop);
        }
        // Ordinary reports only: the lane dispatcher's window, anchored on the oldest.
        let window = batch_window();
        let oldest = present
            .iter()
            .map(|wake| parse_time(&wake.due_at, now) - window)
            .min()
            .unwrap_or(now);
        let last = last_delivery(&self.pool, goal_id, now).await?;
        let wait_until = if !batch_window_elapsed(oldest, now) {
            Some(oldest + window)
        } else if delivered_recently(last, now) {
            last.map(|last| last + window)
        } else {
            None
        };
        if let Some(due) = wait_until {
            report.due_at(due);
            report
                .deferred
                .push((goal_id.to_owned(), Deferral::Batching));
            return Ok(());
        }
        let batch = Batch {
            wakes: present,
            urgent: false,
            judged: true,
        };
        self.deliver(&goal, generation, batch, now, report)
            .await
            .map(drop)
    }

    /// Writes the turn `pending`, judges a report-only batch once per cache key, runs the
    /// handler outside every transaction, then settles the turn with its receipts. Returns
    /// whether the wakes were consumed (delivered, skipped, or settled by another scheduler).
    async fn deliver(
        &self,
        goal: &SupervisorGoal,
        generation: i64,
        batch: Batch,
        now: DateTime<Utc>,
        report: &mut TickReport,
    ) -> Result<bool, SupervisorError> {
        let mut ids: Vec<i64> = batch.wakes.iter().map(|wake| wake.id).collect();
        ids.sort_unstable();
        let key = sha256_hex(&ids.iter().map(i64::to_string).collect::<Vec<_>>().join(","));
        let mut causes: Vec<&'static str> =
            batch.wakes.iter().map(|wake| wake.cause.as_str()).collect();
        causes.sort_unstable();
        causes.dedup();
        let causes = serde_json::to_string(&causes)
            .map_err(|error| SupervisorError::Invalid(error.to_string()))?;
        let (turn_id, state, cached) = self
            .open_turn(goal, generation, &key, &causes, batch.urgent, now)
            .await?;
        if state != "pending" {
            // Another scheduler settled this exact turn; its wakes were consumed with it.
            return Ok(true);
        }
        if let Some(judge) = self.judge.as_ref().filter(|_| batch.judged) {
            let score = match cached {
                Some(score) => score,
                None => {
                    let backing_off = self.judge_retry_at(&goal.goal_id).filter(|at| *at > now);
                    let score = match backing_off {
                        Some(_) => None,
                        None => {
                            self.judge_once(judge.as_ref(), goal, &batch, turn_id)
                                .await?
                        }
                    };
                    match score {
                        Some(score) => {
                            self.judge_answered(&goal.goal_id);
                            score
                        }
                        None => {
                            // A failing judge is asked again only after a backoff, never on
                            // every rescan.
                            let retry_at = backing_off
                                .unwrap_or_else(|| self.judge_failed(&goal.goal_id, now));
                            report.due_at(retry_at);
                            report
                                .deferred
                                .push((goal.goal_id.clone(), Deferral::JudgeUnavailable));
                            return Ok(false);
                        }
                    }
                }
            };
            if score < JUDGE_THRESHOLD {
                settle_wakes(&self.pool, Some((turn_id, "skipped")), &ids, now).await?;
                super::changes::announce(&goal.goal_id);
                report.skipped.push(turn_id);
                return Ok(true);
            }
        }
        let turn = WakeTurn {
            turn_id,
            goal_id: goal.goal_id.clone(),
            goal_revision: goal.revision,
            generation,
            run_state: goal.run_state,
            urgent: batch.urgent,
            wakes: batch.wakes,
        };
        let consumed = match self.handler.handle(&turn).await {
            WakeAck::Done => {
                settle_wakes(&self.pool, Some((turn_id, "delivered")), &ids, now).await?;
                super::changes::announce(&goal.goal_id);
                report.delivered.push(turn_id);
                true
            }
            WakeAck::Busy => {
                report
                    .deferred
                    .push((goal.goal_id.clone(), Deferral::HandlerBusy));
                false
            }
            WakeAck::NotApplicable => {
                report.deferred.push((
                    goal.goal_id.clone(),
                    Deferral::NotApplicable(goal.run_state),
                ));
                false
            }
        };
        Ok(consumed)
    }

    /// Scores a report-only batch once and caches the score on its turn row. `None` when the
    /// judge is unavailable: the reports stay pending.
    async fn judge_once(
        &self,
        judge: &dyn WakeJudge,
        goal: &SupervisorGoal,
        batch: &Batch,
        turn_id: i64,
    ) -> Result<Option<f64>, SupervisorError> {
        let events: Vec<&str> = batch
            .wakes
            .iter()
            .map(|wake| wake.event_id.as_str())
            .collect();
        let state = format!(
            "goal {} ({}, revision {}): {} ordinary reports: {}",
            goal.goal_id,
            goal.run_state,
            goal.revision,
            events.len(),
            events.join(", ")
        );
        match judge.score(&state).await {
            Ok(score) => {
                sqlx::query("UPDATE supervisor_wake_turns SET judge_score = ? WHERE id = ?")
                    .bind(score)
                    .bind(turn_id)
                    .execute(&self.pool)
                    .await?;
                Ok(Some(score))
            }
            Err(error) => {
                tracing::warn!(%error, goal_id = %goal.goal_id, "wake judgment unavailable; reports stay pending");
                Ok(None)
            }
        }
    }

    /// The goal's pending turn for exactly this event set, revision and generation, created
    /// or reused (its cached judgment included); any other pending turn of the goal is
    /// superseded. Returns (id, state, cached score).
    async fn open_turn(
        &self,
        goal: &SupervisorGoal,
        generation: i64,
        key: &str,
        causes: &str,
        urgent: bool,
        now: DateTime<Utc>,
    ) -> Result<(i64, String, Option<f64>), SupervisorError> {
        let stamp = now.to_rfc3339();
        let mut tx = self.pool.begin_write().await?;
        sqlx::query(
            "UPDATE supervisor_wake_turns SET state = 'superseded', settled_at = ?
             WHERE goal_id = ? AND state = 'pending'
               AND NOT (event_set_sha256 = ? AND goal_revision = ? AND generation = ?)",
        )
        .bind(&stamp)
        .bind(&goal.goal_id)
        .bind(key)
        .bind(goal.revision)
        .bind(generation)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO supervisor_wake_turns(goal_id, goal_revision, generation,
                    event_set_sha256, causes, urgent, state, created_at)
             VALUES(?,?,?,?,?,?,'pending',?)
             ON CONFLICT(goal_id, event_set_sha256, goal_revision, generation)
             DO UPDATE SET state = 'pending', settled_at = NULL
             WHERE supervisor_wake_turns.state = 'superseded'",
        )
        .bind(&goal.goal_id)
        .bind(goal.revision)
        .bind(generation)
        .bind(key)
        .bind(causes)
        .bind(urgent)
        .bind(&stamp)
        .execute(&mut *tx)
        .await?;
        let row: (i64, String, Option<f64>) = sqlx::query_as(
            "SELECT id, state, judge_score FROM supervisor_wake_turns
             WHERE goal_id = ? AND event_set_sha256 = ? AND goal_revision = ? AND generation = ?",
        )
        .bind(&goal.goal_id)
        .bind(key)
        .bind(goal.revision)
        .bind(generation)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(row)
    }
}

/// The deterministic wake handler: it applies the run-state moves the execution contract
/// fixes without judgment and calls no model.
/// - A failed dependency holds the goal for the owner with the failure evidence
///   ([`DEPENDENCY_FAILED`]); it is never success.
/// - A passed deadline holds the goal for the owner ([`DEADLINE_PASSED`]).
/// - With no wait pending, a dependency-ready or turn-settled wake moves a running or waiting
///   goal to `verifying` ([`DEPENDENCIES_MET`], [`TURN_SETTLED`], or [`STALE_SETTLE`] when
///   every settled run in the turn answered an older goal revision).
/// - Owner wakes need nothing more: the revision bump that produced them already fences every
///   decision made before them.
/// - Reports are status only.
///
/// A decisive wake (settle, dependency, deadline) is never acknowledged without its effect:
/// in a state this handler cannot act from (`ready` after a resume, `dispatching`, a hold),
/// with a wait added since the scheduler checked, or after an error it cannot resolve, it
/// answers [`WakeAck::NotApplicable`] and the wake stays pending for whoever can act on it. A
/// goal that moved since the scheduler checked it (revision or state conflict), became busy,
/// or a storage error answers `Busy`, so the wake is retried against the new state instead of
/// being lost.
pub struct StateHandler {
    pool: SqlitePool,
}

impl StateHandler {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    async fn apply(&self, turn: &WakeTurn) -> Result<WakeAck, SupervisorError> {
        if turn.has(WakeCause::OwnerControl) || turn.has(WakeCause::OwnerCorrection) {
            return Ok(WakeAck::Done);
        }
        let Some(goal) = goal_of(&self.pool, &turn.goal_id).await? else {
            return Ok(WakeAck::Done);
        };
        let decisive = turn.wakes.iter().any(|wake| is_decisive(wake.cause));
        if !matches!(
            goal.run_state,
            RunState::Running
                | RunState::WaitingChildren
                | RunState::WaitingEvent
                | RunState::Verifying
        ) {
            // Reports are status only and an ended goal's wakes are closed. From any other
            // state nothing here can act on a decisive wake, and receipting it as delivered
            // would lose the very result progress needs.
            return Ok(if decisive && !goal.run_state.is_terminal() {
                WakeAck::NotApplicable
            } else {
                WakeAck::Done
            });
        }
        if goal_busy(&self.pool, &goal.goal_id).await? {
            return Ok(WakeAck::Busy);
        }
        if turn.has(WakeCause::DependencyFailed) {
            let detail = failure_detail(&self.pool, &goal.goal_id).await?;
            transition_goal(
                &self.pool,
                &goal.goal_id,
                goal.revision,
                goal.run_state,
                RunState::AwaitingOwner,
                Some(&detail),
            )
            .await?;
            return Ok(WakeAck::Done);
        }
        if turn.has(WakeCause::Deadline) {
            let deadlines: Vec<&str> = turn
                .wakes
                .iter()
                .filter(|wake| wake.cause == WakeCause::Deadline)
                .map(|wake| wake.event_id.as_str())
                .collect();
            let detail = truncate(&format!("{DEADLINE_PASSED}: {}", deadlines.join(", ")));
            transition_goal(
                &self.pool,
                &goal.goal_id,
                goal.revision,
                goal.run_state,
                RunState::AwaitingOwner,
                Some(&detail),
            )
            .await?;
            return Ok(WakeAck::Done);
        }
        let ended = turn.has(WakeCause::DependencyReady) || turn.has(WakeCause::TurnSettled);
        if !ended || goal.run_state == RunState::Verifying {
            return Ok(WakeAck::Done);
        }
        if !pending_wait_kinds(&self.pool, &goal.goal_id)
            .await?
            .is_empty()
        {
            // A wait added after the scheduler checked: the result waits for it.
            return Ok(WakeAck::NotApplicable);
        }
        let detail = if turn.has(WakeCause::DependencyReady) {
            DEPENDENCIES_MET.to_owned()
        } else {
            settle_detail(&self.pool, turn, goal.revision).await?
        };
        transition_goal(
            &self.pool,
            &goal.goal_id,
            goal.revision,
            goal.run_state,
            RunState::Verifying,
            Some(&detail),
        )
        .await?;
        Ok(WakeAck::Done)
    }
}

/// [`TURN_SETTLED`], or [`STALE_SETTLE`] when every settled run in the turn was dispatched at
/// an older goal revision than `revision`.
async fn settle_detail(
    pool: &SqlitePool,
    turn: &WakeTurn,
    revision: i64,
) -> Result<String, SupervisorError> {
    let mut older = Vec::new();
    for wake in turn
        .wakes
        .iter()
        .filter(|wake| wake.cause == WakeCause::TurnSettled)
    {
        let Some(request_id) = wake
            .event_id
            .strip_prefix("dispatch:")
            .and_then(|rest| rest.strip_suffix(":settled"))
        else {
            return Ok(TURN_SETTLED.to_owned());
        };
        let dispatched: Option<i64> = sqlx::query_scalar(
            "SELECT goal_revision FROM supervisor_dispatches WHERE request_id = ?",
        )
        .bind(request_id)
        .fetch_optional(pool)
        .await?;
        match dispatched {
            Some(at) if at < revision => older.push(format!("{request_id} (revision {at})")),
            _ => return Ok(TURN_SETTLED.to_owned()),
        }
    }
    if older.is_empty() {
        return Ok(TURN_SETTLED.to_owned());
    }
    Ok(truncate(&format!(
        "{STALE_SETTLE}: {} settled before goal revision {revision}; not evidence for the current contract",
        older.join(", ")
    )))
}

#[async_trait]
impl WakeHandler for StateHandler {
    async fn handle(&self, turn: &WakeTurn) -> WakeAck {
        match self.apply(turn).await {
            Ok(ack) => ack,
            Err(error) if matches!(error, SupervisorError::Sql(_)) => {
                tracing::warn!(%error, turn_id = turn.turn_id, "wake turn was not applied; retrying later");
                WakeAck::Busy
            }
            Err(error) if error.code() == "REVISION_CONFLICT" => {
                tracing::debug!(%error, turn_id = turn.turn_id, "goal moved before its wake applied; rechecking");
                WakeAck::Busy
            }
            Err(error) if turn.wakes.iter().any(|wake| is_decisive(wake.cause)) => {
                tracing::warn!(%error, turn_id = turn.turn_id, "decisive wake could not be applied; it stays pending");
                WakeAck::NotApplicable
            }
            Err(error) => {
                tracing::warn!(%error, turn_id = turn.turn_id, "wake turn needs no state change");
                WakeAck::Done
            }
        }
    }
}
