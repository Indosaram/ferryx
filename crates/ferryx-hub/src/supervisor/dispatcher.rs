//! The supervisor's initial dispatcher. It runs only inside the daemon's hub, where the
//! observer that confirms a turn start lives. The `ferryx-cli mcp` process admits work but
//! never transmits.
//!
//! It acts only on committed ready intents ([`ready_dispatches`]). For each one it:
//! 1. checks the grant before claiming;
//! 2. claims the intent transactionally ([`claim_dispatch`]);
//! 3. launches and attaches the worker pane ([`attach_session`], never `lanes.omo_thread_id`).
//!    Like the legacy lane runner, a lane at a repository root first gets a task-owned
//!    worktree ([`crate::lanes::worktree::create`], off with `OMON_LANE_AUTO_WORKTREE=0`), so
//!    goals on one project never share the owner's checkout;
//! 4. re-checks the grant;
//! 5. confirms the claim ([`confirm_claim_before_transmit`]) exactly once and transmits the
//!    stable request id once, immediately after `Ok`, with no other await in between;
//! 6. records each receipt as a separate fact ([`super::receipts`]): accepted when the target
//!    took the bytes, started only from a target-side receipt naming this request id and the
//!    bound session, settled when the transport reports that run's end, uncertain otherwise.
//!    Acceptance alone, or activity of another session or request, never starts a run.
//!    Accepted, started and the goal's move to `running` commit in one transaction
//!    ([`super::record_start`]); if that fails, the dispatch is held `delivery_uncertain`
//!    ([`START_NOT_RECORDED`]). Right after the commit a local PTY run is settled from the
//!    observer's [`IdleLedger`] when its ending idle reading already arrived.
//!
//! Every claim gets exactly one confirm call. On a hold before transmission, that call
//! releases the claim. Otherwise it is the final check before the transmit, and nothing
//! confirms again after the transmit, except when the transport proves nothing left this hub
//! or was typed: a missing receipt capability ([`Delivery::Unsupported`], a degraded hold) or
//! a refusal whose code proves nothing was typed ([`Delivery::Refused`], an owner hold; see
//! [`crate::agent_control::PRE_WRITE_REFUSALS`]). That claim is released like a hold before
//! transmission. A store error after the claim and before the transmit holds the goal
//! ([`DISPATCH_FAILED`]) and releases the claim the same way, so no row stays `claimed`. A
//! released claim is never transmitted. A claim left behind by a dispatcher that
//! stopped (crash, `hub stop`) cannot be proven untransmitted, so [`Dispatcher::run`] first
//! calls [`reconcile_claims_after_restart`] (retried with a bounded backoff), which holds it
//! `delivery_uncertain`. It is never resent: the owner answers it (the `resolve_delivery`
//! request, or [`super::resolve_uncertain_delivery`]), and a resend is a new dispatch.
//! The dispatcher never advances an execution generation: rebinding a worker is the
//! reattach policy's job, not the first dispatch's.
//!
//! Wake: [`Dispatcher::run`] drains at start, which is the startup drain. It then waits on a
//! [`Notify`] that it subscribes to before every drain, so a wake fired during a drain is
//! never lost. Admissions inside the daemon call [`poke`]. Admissions committed by another
//! process reach it through the durable ready intent and the bounded [`DISPATCH_RESCAN`].
//! That rescan is a database query, never a model call.
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use sqlx::SqlitePool;
use tokio::sync::{mpsc, oneshot, Notify};

use super::admission::goal_grant;
use super::receipts::{
    record_pty_settle, watch_paired_settle, IdleLedger, PtySettle, PtyWorker, SettleReport,
    SettleWatch, WorkerTransport,
};
use super::{
    attach_session, claim_dispatch, confirm_claim_before_transmit, execution_binding_of, goal_of,
    hold_live_run, identity_of, ready_dispatches, reconcile_claims_after_restart, record_receipt,
    record_start, transition_goal, Dispatch, DispatchRow, ExecutionBinding, ExecutionKind, Receipt,
    RunState, SupervisorError, SupervisorGoal, DISPATCH_COLUMNS,
};
use crate::agent_control::{
    AgentControl, AgentOp, Delivery, StartReceipt, TaskLauncher, WorkerSend, RECEIPT_MISMATCH,
};
use crate::goals::types::{GoalMode, SendOutcome};
use crate::lanes::state::LaneStatus;

/// How often the dispatcher rereads durable ready intents when nothing wakes it: the path by
/// which an admission committed in another process is picked up.
pub const DISPATCH_RESCAN: Duration = Duration::from_secs(5);

/// Most ready intents one drain considers.
const DRAIN_LIMIT: u32 = 256;

/// Attempts at restart reconciliation before the first drain, and the first retry delay
/// (doubled after each failure: at most 0.25 + 0.5 + 1 + 2 + 4 seconds in all).
const RECONCILE_ATTEMPTS: u32 = 6;
const RECONCILE_BACKOFF: Duration = Duration::from_millis(250);

/// Attempts at listing this machine's panes for the restart census, and the first retry delay
/// (doubled after each failure: at most 0.25 + 0.5 seconds in all).
const PANE_LIST_ATTEMPTS: u32 = 3;
const PANE_LIST_BACKOFF: Duration = Duration::from_millis(250);

/// Hold codes (`state_detail` prefixes) the dispatcher writes.
pub use super::receipts::MHC_WORKER_UNAVAILABLE;
pub const REMOTE_LAUNCH_UNAVAILABLE: &str = "REMOTE_LAUNCH_UNAVAILABLE";
pub const GRANT_REVOKED: &str = "GRANT_REVOKED";
pub const OWNER_APPROVAL_REQUIRED: &str = "OWNER_APPROVAL_REQUIRED";
pub const TURN_LIMIT: &str = "TURN_LIMIT";
pub const LAUNCH_FAILED: &str = "LAUNCH_FAILED";
pub const SEND_REFUSED: &str = "SEND_REFUSED";
pub const LANE_PAUSED: &str = "LANE_PAUSED";
/// A store error after the claim and before the transmit; nothing was sent.
pub const DISPATCH_FAILED: &str = "DISPATCH_FAILED";
/// A causal start receipt arrived but its start could not be committed (with the goal's move
/// to running); the dispatch is uncertain and is not resent.
pub const START_NOT_RECORDED: &str = "START_NOT_RECORDED";
/// Restart census: a local PTY run whose pane is not a live session of this daemon any more.
/// Its end can never be observed; the goal is held, never settled and never resent.
pub const PTY_RUN_LOST: &str = "PTY_RUN_LOST";

/// The daemon's dispatcher wake, shared by every in-process admission.
pub fn dispatch_wake() -> Arc<Notify> {
    static WAKE: OnceLock<Arc<Notify>> = OnceLock::new();
    Arc::clone(WAKE.get_or_init(|| Arc::new(Notify::new())))
}

/// Wakes this process's dispatcher. In a process without one (`ferryx-cli mcp`) it does
/// nothing; the daemon's rescan finds the durable intent instead.
pub fn poke() {
    dispatch_wake().notify_one();
}

/// What one drain did with one ready intent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchResult {
    /// Transmitted once; `outcome` is what the target's receipts showed: `Started` only for a
    /// start receipt naming this request and session, otherwise `Uncertain`.
    Transmitted {
        request_id: String,
        outcome: SendOutcome,
    },
    /// Not transmitted: the goal was held (before or after claiming, or because the transport
    /// proved nothing was sent or typed) for the reason given.
    Held {
        request_id: String,
        state: RunState,
        detail: String,
    },
    /// Claimed, then released before transmission because a hold, correction or generation
    /// change landed first (`SUPERSEDED`; the dispatch is `released`).
    Released {
        request_id: String,
        code: String,
        why: String,
    },
    /// Not eligible right now (lost a race, paused lane). Nothing changed.
    Skipped { request_id: String, code: String },
}

fn skipped(request_id: &str, code: &str) -> DispatchResult {
    DispatchResult::Skipped {
        request_id: request_id.to_owned(),
        code: code.to_owned(),
    }
}

fn released(request_id: &str, error: &SupervisorError) -> DispatchResult {
    DispatchResult::Released {
        request_id: request_id.to_owned(),
        code: error.code().to_owned(),
        why: error.to_string(),
    }
}

/// A launched and attached worker pane, or the result of holding the goal instead.
enum Launch {
    Session(String),
    Held(DispatchResult),
}

pub struct Dispatcher {
    pool: SqlitePool,
    /// Agent control of this machine and paired hosts, for the restart census.
    control: Arc<dyn AgentControl>,
    /// How the census watches a paired-host run it re-arms.
    settle_watch: SettleWatch,
    /// Terminal agents on this machine and paired hosts.
    pty: Arc<dyn WorkerTransport>,
    /// Native mhc worker sessions, when an engine for them is wired.
    mhc: Option<Arc<dyn WorkerTransport>>,
    launcher: Arc<dyn TaskLauncher>,
    /// The host id this dispatcher can launch panes on (`launch_pane` has no host argument).
    local_host_id: String,
    rescan: Duration,
    /// Told how many intents each drain handled, after the drain's wake subscription.
    drained: Option<mpsc::UnboundedSender<usize>>,
    /// Told the request id of every settle a transport reported and the store recorded.
    settled: Option<mpsc::UnboundedSender<String>>,
    /// Idle readings the hub's observer saw on this machine, for runs whose start commits
    /// after their ending reading arrived.
    idle: Arc<IdleLedger>,
}

impl Dispatcher {
    pub fn new(
        pool: SqlitePool,
        control: Arc<dyn AgentControl>,
        launcher: Arc<dyn TaskLauncher>,
        local_host_id: impl Into<String>,
    ) -> Self {
        Self {
            pool,
            pty: Arc::new(PtyWorker::new(Arc::clone(&control))),
            control,
            settle_watch: SettleWatch::default(),
            mhc: None,
            launcher,
            local_host_id: local_host_id.into(),
            rescan: DISPATCH_RESCAN,
            drained: None,
            settled: None,
            idle: Arc::new(IdleLedger::default()),
        }
    }

    /// Shares the hub observer's idle readings of this machine's panes (see [`IdleLedger`]).
    pub fn with_idle_ledger(mut self, idle: Arc<IdleLedger>) -> Self {
        self.idle = idle;
        self
    }

    /// Delivers to terminal agents through `worker` instead of the default [`PtyWorker`].
    pub fn with_pty_worker(mut self, worker: Arc<dyn WorkerTransport>) -> Self {
        self.pty = worker;
        self
    }

    /// Delivers to native mhc worker sessions through `worker`. Without one, an mhc binding is
    /// held [`MHC_WORKER_UNAVAILABLE`].
    pub fn with_mhc_worker(mut self, worker: Arc<dyn WorkerTransport>) -> Self {
        self.mhc = Some(worker);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_rescan(mut self, rescan: Duration) -> Self {
        self.rescan = rescan;
        self
    }

    /// How the restart census watches the paired-host runs it re-arms.
    pub fn with_census_settle_watch(mut self, watch: SettleWatch) -> Self {
        self.settle_watch = watch;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_drain_probe(mut self, probe: mpsc::UnboundedSender<usize>) -> Self {
        self.drained = Some(probe);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_settle_probe(mut self, probe: mpsc::UnboundedSender<String>) -> Self {
        self.settled = Some(probe);
        self
    }

    /// Holds claims left by a dispatcher that stopped, then drains forever: at start, on every
    /// wake and on each rescan. Started once per hub generation, after the previous
    /// generation's tasks were aborted, so no other dispatcher of this hub is running.
    pub async fn run(self, wake: Arc<Notify>) {
        self.reconcile_at_start().await;
        self.census_started_runs().await;
        loop {
            let notified = wake.notified();
            tokio::pin!(notified);
            // Subscribe before draining: a wake fired while the drain runs is kept, not lost.
            notified.as_mut().enable();
            let handled = self.drain().await.len();
            if let Some(probe) = &self.drained {
                let _ = probe.send(handled);
            }
            tokio::select! {
                () = &mut notified => {}
                () = tokio::time::sleep(self.rescan) => {}
            }
        }
    }

    /// Restart reconciliation before the first drain, retried with a bounded backoff. If it
    /// still fails, the stale claims stay `claimed`: never resent (a drain only reads `ready`
    /// intents), but their goals wait until a later start reconciles them.
    async fn reconcile_at_start(&self) {
        let mut delay = RECONCILE_BACKOFF;
        for attempt in 1..=RECONCILE_ATTEMPTS {
            match reconcile_claims_after_restart(&self.pool).await {
                Ok(held) => {
                    if !held.is_empty() {
                        tracing::warn!(
                            held = held.len(),
                            "unconfirmed supervisor claims held as delivery_uncertain"
                        );
                    }
                    return;
                }
                Err(error) if attempt < RECONCILE_ATTEMPTS => {
                    tracing::warn!(%error, attempt, "supervisor claims were not reconciled; retrying");
                    tokio::time::sleep(delay).await;
                    delay *= 2;
                }
                Err(error) => tracing::error!(
                    %error,
                    attempts = RECONCILE_ATTEMPTS,
                    "supervisor claims were not reconciled at start; stale claims stay claimed and are not resent"
                ),
            }
        }
    }

    /// One pass over the committed ready intents, oldest first. Safe to run concurrently with
    /// another dispatcher: the claim lets exactly one of them transmit.
    pub async fn drain(&self) -> Vec<DispatchResult> {
        let ready = match ready_dispatches(&self.pool, DRAIN_LIMIT).await {
            Ok(ready) => ready,
            Err(error) => {
                tracing::warn!(%error, "ready supervisor dispatches were not read");
                return Vec::new();
            }
        };
        let mut results = Vec::with_capacity(ready.len());
        for dispatch in &ready {
            match self.dispatch_one(dispatch).await {
                Ok(result) => results.push(result),
                Err(error) => {
                    tracing::warn!(%error, request_id = %dispatch.request_id, "supervisor dispatch failed");
                    results.push(skipped(&dispatch.request_id, error.code()));
                }
            }
        }
        results
    }

    /// Restart census, once per hub generation after [`reconcile_claims_after_restart`]: every
    /// started PTY run of a current execution generation whose goal is still open was left by
    /// a predecessor, whose watchers died with it. Nothing is sent and nothing is settled here.
    /// - A run on a paired host is watched again from its start reading
    ///   ([`watch_paired_settle`]): its settle is recorded, and an end that cannot be observed
    ///   (the host's observer restarted, waits keep failing) holds a `running` goal degraded
    ///   with [`super::receipts::PAIRED_SETTLE_UNOBSERVED`]. It is never left unwatched.
    /// - A run on this machine is settled by the hub observer
    ///   ([`super::receipts::record_pty_settle_across_restart`]) while its pane lives. A pane
    ///   that is not a live session of this daemon holds a `running` goal degraded with
    ///   [`PTY_RUN_LOST`]. A census whose session list cannot be read holds nothing: the
    ///   observer still covers a live pane.
    ///
    /// Native mhc worker runs are not reattached here: no daemon wires an mhc worker yet
    /// (`with_mhc_worker` is test-only), and that wiring must add its own census.
    pub async fn census_started_runs(&self) -> usize {
        let sql = format!(
            "SELECT {} FROM supervisor_dispatches d
             JOIN supervisor_execution_bindings b
               ON b.goal_id = d.goal_id AND b.generation = d.generation
             JOIN supervisor_goals g ON g.goal_id = d.goal_id
             WHERE d.state = 'started' AND b.kind = 'pty' AND d.run_id = b.run_id
               AND g.run_state NOT IN ('completed','cancelled')
             ORDER BY d.id",
            DISPATCH_COLUMNS
                .split(", ")
                .map(|column| format!("d.{}", column.trim()))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let rows: Vec<DispatchRow> = match sqlx::query_as(&sql).fetch_all(&self.pool).await {
            Ok(rows) => rows,
            Err(error) => {
                tracing::warn!(%error, "started supervisor runs were not read at start; the observer still settles live local panes");
                return 0;
            }
        };
        let mut local_panes: Option<Option<Vec<String>>> = None;
        let mut handled = 0;
        for row in rows {
            let Ok(dispatch) = super::dispatch_from_row(row) else {
                continue;
            };
            let (Some(run_id), Some(session_id)) = (
                dispatch.run_id.clone(),
                dispatch.provider_session_id.clone(),
            ) else {
                continue;
            };
            let Ok(Some(binding)) = execution_binding_of(&self.pool, &dispatch.goal_id).await
            else {
                continue;
            };
            let run = StartReceipt {
                request_id: dispatch.request_id.clone(),
                session_id: session_id.clone(),
                run_id,
            };
            handled += 1;
            if binding.host_id != self.local_host_id {
                let (notice, settle) = oneshot::channel();
                tokio::spawn(watch_paired_settle(
                    Arc::clone(&self.control),
                    binding.host_id.clone(),
                    run.clone(),
                    self.settle_watch,
                    notice,
                ));
                self.watch_settle(&dispatch, binding.host_id.clone(), run, settle);
                continue;
            }
            if local_panes.is_none() {
                local_panes = Some(self.local_panes().await);
            }
            let Some(Some(panes)) = &local_panes else {
                continue;
            };
            if panes.iter().any(|pane| pane == &session_id) {
                continue;
            }
            let reason =
                format!("pane {session_id} is not a live session of this daemon after the restart");
            hold_unobserved(
                &self.pool,
                &dispatch.goal_id,
                &dispatch.request_id,
                &self.local_host_id,
                &run,
                PTY_RUN_LOST,
                &reason,
            )
            .await;
        }
        handled
    }

    /// Session ids of this machine's live panes, or `None` when they cannot be read after
    /// [`PANE_LIST_ATTEMPTS`] bounded tries.
    async fn local_panes(&self) -> Option<Vec<String>> {
        let mut backoff = PANE_LIST_BACKOFF;
        for attempt in 1..=PANE_LIST_ATTEMPTS {
            let failure = match self.control.execute(None, AgentOp::List).await {
                Ok(listed) => match listed.get("sessions").and_then(serde_json::Value::as_array) {
                    Some(sessions) => {
                        return Some(
                            sessions
                                .iter()
                                .filter_map(|session| {
                                    session.get("sessionId").and_then(|id| id.as_str())
                                })
                                .map(str::to_owned)
                                .collect(),
                        )
                    }
                    None => "the list reply has no sessions".to_owned(),
                },
                Err(error) => error.to_string(),
            };
            if attempt == PANE_LIST_ATTEMPTS {
                tracing::warn!(
                    %failure,
                    attempt,
                    "local panes were not listed at start; no local run is held"
                );
                break;
            }
            tokio::time::sleep(backoff).await;
            backoff = backoff.saturating_mul(2);
        }
        None
    }

    async fn dispatch_one(&self, dispatch: &Dispatch) -> Result<DispatchResult, SupervisorError> {
        let request_id = dispatch.request_id.as_str();
        let Some(goal) = goal_of(&self.pool, &dispatch.goal_id).await? else {
            return Ok(skipped(request_id, "NOT_FOUND"));
        };
        if goal.run_state != RunState::Ready || goal.revision != dispatch.goal_revision {
            return Ok(skipped(request_id, "REVISION_CONFLICT"));
        }
        // An upgrade not cut over yet, or a rollback: nothing is held, launched or sent, and
        // the intent stays `ready` for the next cutover (the claim refuses it as well).
        if !super::cutover::unified_open_for(&self.pool, &goal.supervisor_id).await? {
            return Ok(skipped(request_id, super::cutover::SUPERVISOR_INTAKE_CLOSED));
        }
        let Some(binding) = execution_binding_of(&self.pool, &goal.goal_id).await? else {
            return Ok(skipped(request_id, "NOT_FOUND"));
        };
        if let Some((to, detail)) = self.grant_hold(&goal, &binding).await? {
            return match transition_goal(
                &self.pool,
                &goal.goal_id,
                goal.revision,
                RunState::Ready,
                to,
                Some(&detail),
            )
            .await
            {
                Ok(_) => Ok(DispatchResult::Held {
                    request_id: request_id.to_owned(),
                    state: to,
                    detail,
                }),
                Err(SupervisorError::Sql(error)) => Err(SupervisorError::Sql(error)),
                Err(error) => Ok(skipped(request_id, error.code())),
            };
        }
        if self.lane_paused(&goal).await {
            return Ok(skipped(request_id, LANE_PAUSED));
        }
        let claimed = match claim_dispatch(
            &self.pool,
            request_id,
            goal.revision,
            binding.generation,
        )
        .await
        {
            Ok(claimed) => claimed,
            Err(SupervisorError::Sql(error)) => return Err(SupervisorError::Sql(error)),
            Err(error) => return Ok(skipped(request_id, error.code())),
        };

        // From here the claim is ours: every exit makes one effective confirm call, which
        // either releases the claim (hold) or clears the transmit that follows it. An error
        // can only happen while nothing was sent, so it holds the goal and releases the claim
        // instead of leaving it `claimed` until the next restart.
        let result = match self.run_claimed(&goal, &binding, &claimed).await {
            Ok(result) => Ok(result),
            Err(error) => {
                tracing::warn!(%error, request_id, "supervisor dispatch failed after its claim; holding the goal");
                self.hold_claimed(
                    &claimed,
                    RunState::Degraded,
                    format!("{DISPATCH_FAILED}: {error}; nothing was sent"),
                )
                .await
            }
        };
        super::delivery::forget(request_id);
        result
    }

    /// Launch, re-check, confirm and transmit for a claimed intent. Returns `Err` only while
    /// nothing was sent: before the transmit, or from the hold after a transport reported it
    /// sent or typed nothing.
    async fn run_claimed(
        &self,
        goal: &SupervisorGoal,
        binding: &ExecutionBinding,
        claimed: &Dispatch,
    ) -> Result<DispatchResult, SupervisorError> {
        let request_id = claimed.request_id.as_str();
        let session = match binding.provider_session_id.clone() {
            Some(session) => session,
            None => match self.launch(goal, claimed).await? {
                Launch::Session(session) => session,
                Launch::Held(result) => return Ok(result),
            },
        };
        if self.lane_paused(goal).await {
            return self
                .hold_claimed(
                    claimed,
                    RunState::Paused,
                    format!("{LANE_PAUSED}: the owner paused lane {}", goal.lane_key),
                )
                .await;
        }
        let Some(binding) = execution_binding_of(&self.pool, &goal.goal_id).await? else {
            return self
                .hold_claimed(claimed, RunState::Degraded, "the execution binding disappeared".into())
                .await;
        };
        if let Some((to, detail)) = self.grant_hold(goal, &binding).await? {
            return self.hold_claimed(claimed, to, detail).await;
        }
        let transport = match binding.kind {
            ExecutionKind::Pty => Arc::clone(&self.pty),
            ExecutionKind::Mhc => match &self.mhc {
                Some(mhc) => Arc::clone(mhc),
                None => {
                    let detail = format!("{MHC_WORKER_UNAVAILABLE}: no native mhc worker is wired to this dispatcher");
                    return self.hold_claimed(claimed, RunState::Degraded, detail).await;
                }
            },
        };
        // The last check; the transmit follows with no other await in between.
        let host = (binding.host_id != self.local_host_id).then_some(binding.host_id.as_str());
        let send = WorkerSend {
            host_id: host.map(str::to_owned),
            session_id: session,
            text: claimed.instruction.clone(),
            request_id: request_id.to_owned(),
        };
        let (settle_tx, settle_rx) = oneshot::channel();
        let sent = match confirm_claim_before_transmit(&self.pool, request_id).await {
            Ok(sent) => sent,
            Err(SupervisorError::Sql(error)) => return Err(SupervisorError::Sql(error)),
            Err(error) => return Ok(released(request_id, &error)),
        };
        let delivery = transport.deliver(&send, settle_tx).await;
        // An owner hold, correction or lane pause that landed while the transport waited for
        // the pane revoked this request in the typing fence, and the transport refused it
        // before the first byte. A goal hold or correction moved the goal, so the confirm call
        // releases the claim; a lane pause did not, so the goal is held paused.
        if matches!(delivery, Delivery::Refused { .. })
            && super::delivery::check_before_typing(request_id).is_err()
        {
            return match confirm_claim_before_transmit(&self.pool, request_id).await {
                Ok(_) => {
                    let detail = format!(
                        "{LANE_PAUSED}: the owner paused lane {} before {request_id} was typed",
                        goal.lane_key
                    );
                    self.hold_claimed(claimed, RunState::Paused, detail).await
                }
                Err(SupervisorError::Sql(error)) => Err(SupervisorError::Sql(error)),
                Err(error) => Ok(released(request_id, &error)),
            };
        }
        self.settle_transmission(goal, &sent, &send, delivery, settle_rx)
            .await
    }

    /// Why the goal must not dispatch now, checked against durable state: the worker kind and
    /// host this dispatcher can serve, the owner's identity, the persisted operation grant,
    /// the project's host mapping, the goal mode (only `full` continues without the owner) and
    /// the turn budget.
    async fn grant_hold(
        &self,
        goal: &SupervisorGoal,
        binding: &ExecutionBinding,
    ) -> Result<Option<(RunState, String)>, SupervisorError> {
        if binding.kind == ExecutionKind::Mhc
            && (self.mhc.is_none() || binding.provider_session_id.is_none())
        {
            return Ok(Some((
                RunState::Degraded,
                format!(
                    "{MHC_WORKER_UNAVAILABLE}: no native mhc worker session is wired to this dispatcher"
                ),
            )));
        }
        if binding.kind == ExecutionKind::Pty
            && binding.host_id != self.local_host_id
            && binding.provider_session_id.is_none()
        {
            return Ok(Some((
                RunState::Degraded,
                format!(
                    "{REMOTE_LAUNCH_UNAVAILABLE}: a worker cannot be launched on host {} from here",
                    binding.host_id
                ),
            )));
        }
        let owner = identity_of(&self.pool, &goal.owner_principal).await?;
        if owner.is_none_or(|identity| identity.supervisor_id != goal.supervisor_id) {
            return Ok(Some((
                RunState::AwaitingOwner,
                format!(
                    "{GRANT_REVOKED}: {} no longer owns this supervisor",
                    goal.owner_principal
                ),
            )));
        }
        // The worker instruction names the grant persisted at admission; without a readable
        // grant nothing is dispatched.
        match goal_grant(&self.pool, &goal.goal_id).await {
            Ok(_) => {}
            Err(SupervisorError::Sql(error)) => return Err(SupervisorError::Sql(error)),
            Err(error) => {
                return Ok(Some((RunState::AwaitingOwner, format!("{GRANT_REVOKED}: {error}"))))
            }
        }
        let mapped: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM supervisor_project_hosts
             WHERE project_id = ? AND host_id = ?)",
        )
        .bind(&goal.project_id)
        .bind(&binding.host_id)
        .fetch_one(&self.pool)
        .await?;
        if !mapped {
            return Ok(Some((
                RunState::AwaitingOwner,
                format!(
                    "{GRANT_REVOKED}: project {} is no longer mapped on host {}",
                    goal.project_id, binding.host_id
                ),
            )));
        }
        let contract: Option<(String, i64, i64)> = sqlx::query_as(
            "SELECT mode, turn_limit, turns_used FROM lane_goal WHERE lane_key = ?",
        )
        .bind(&goal.lane_key)
        .fetch_optional(&self.pool)
        .await?;
        let Some((mode, turn_limit, turns_used)) = contract else {
            return Ok(Some((
                RunState::AwaitingOwner,
                format!("{OWNER_APPROVAL_REQUIRED}: lane {} has no goal contract", goal.lane_key),
            )));
        };
        if mode != GoalMode::Full.as_str() {
            return Ok(Some((
                RunState::AwaitingOwner,
                format!("{OWNER_APPROVAL_REQUIRED}: a {mode} goal is not dispatched without the owner"),
            )));
        }
        // The first dispatch consumes no turn; each corrective run counts its turn when it is
        // enqueued ([`super::completion`]), so a dispatch goes out while the count is within
        // the limit. A count past it was spent elsewhere and holds.
        if turns_used > turn_limit {
            return Ok(Some((
                RunState::AwaitingOwner,
                format!("{TURN_LIMIT}: {turns_used} of {turn_limit} turns are used"),
            )));
        }
        Ok(None)
    }

    /// A person's lane pause gates unsent work. Unreadable counts as paused.
    async fn lane_paused(&self, goal: &SupervisorGoal) -> bool {
        match crate::lanes::pause::is_paused(&self.pool, &goal.lane_key).await {
            Ok(paused) => paused,
            Err(error) => {
                tracing::warn!(%error, lane = %goal.lane_key, "lane pause was not read; not dispatching");
                true
            }
        }
    }

    /// Launches the worker pane in the lane's directory and binds it to the current
    /// generation. A lane at a repository root first gets its task-owned worktree
    /// (`<repo>-lanes/<lane>` on branch `lane/<lane>`), exactly like the legacy lane runner;
    /// `create` records it and points `lanes.cwd` at it, and a lane that already owns one keeps
    /// the one `lanes.cwd` names.
    async fn launch(
        &self,
        goal: &SupervisorGoal,
        claimed: &Dispatch,
    ) -> Result<Launch, SupervisorError> {
        let cwd: Option<String> = sqlx::query_scalar("SELECT cwd FROM lanes WHERE key = ?")
            .bind(&goal.lane_key)
            .fetch_optional(&self.pool)
            .await?;
        let Some(cwd) = cwd else {
            let detail = format!("{LAUNCH_FAILED}: lane {} is missing", goal.lane_key);
            return Ok(Launch::Held(
                self.hold_claimed(claimed, RunState::Degraded, detail).await?,
            ));
        };
        let cwd = match crate::lanes::worktree::create(&self.pool, &goal.lane_key, &cwd).await {
            Ok(Some(tree)) => tree,
            Ok(None) => cwd,
            Err(error) => {
                let detail = format!(
                    "{LAUNCH_FAILED}: the worktree for lane {} was not created: {error}",
                    goal.lane_key
                );
                return Ok(Launch::Held(
                    self.hold_claimed(claimed, RunState::Degraded, detail).await?,
                ));
            }
        };
        let session = match self.launcher.launch_pane(&cwd, &[]).await {
            Ok(session) => session,
            Err(error) => {
                let detail = format!("{LAUNCH_FAILED}: {error}");
                return Ok(Launch::Held(
                    self.hold_claimed(claimed, RunState::Degraded, detail).await?,
                ));
            }
        };
        match attach_session(&self.pool, &goal.goal_id, claimed.generation, &session).await {
            Ok(_) => Ok(Launch::Session(session)),
            Err(SupervisorError::Sql(error)) => Err(SupervisorError::Sql(error)),
            Err(error) => {
                tracing::warn!(%error, session_id = %session, goal_id = %goal.goal_id, "a launched pane was not bound to the goal and is left unused");
                let detail = format!("{LAUNCH_FAILED}: pane {session} was not bound: {error}");
                Ok(Launch::Held(
                    self.hold_claimed(claimed, RunState::Degraded, detail).await?,
                ))
            }
        }
    }

    /// Only while nothing was transmitted (before the transmit, or after a transport proved it
    /// sent or typed nothing): moves the goal out of `dispatching`, then makes the confirm call
    /// that releases the claim. If the goal already moved (an owner hold landed first), only
    /// the release applies.
    async fn hold_claimed(
        &self,
        claimed: &Dispatch,
        to: RunState,
        detail: String,
    ) -> Result<DispatchResult, SupervisorError> {
        match transition_goal(
            &self.pool,
            &claimed.goal_id,
            claimed.goal_revision,
            RunState::Dispatching,
            to,
            Some(&detail),
        )
        .await
        {
            Ok(_) => {}
            Err(SupervisorError::Sql(error)) => return Err(SupervisorError::Sql(error)),
            Err(error) => {
                tracing::debug!(%error, request_id = %claimed.request_id, "goal moved before the dispatcher held it");
            }
        }
        match confirm_claim_before_transmit(&self.pool, &claimed.request_id).await {
            Ok(_) => {
                tracing::warn!(request_id = %claimed.request_id, "a held dispatch is still claimed; it is not transmitted");
            }
            Err(SupervisorError::Sql(error)) => return Err(SupervisorError::Sql(error)),
            Err(_) => {}
        }
        Ok(DispatchResult::Held {
            request_id: claimed.request_id.clone(),
            state: to,
            detail,
        })
    }

    /// Records what the transmission produced, each stage as its own receipt.
    ///
    /// - Nothing sent ([`Delivery::Unsupported`]: no causal observer for this worker) holds the
    ///   goal degraded with the capability code and releases the claim.
    /// - Nothing typed ([`Delivery::Refused`]) holds the goal for the owner and releases the
    ///   claim; it is never resent on its own.
    /// - A start receipt naming this request and the bound session records accepted, started
    ///   with the target's run id and the goal's move to running in one transaction. If that
    ///   fails, the dispatch is uncertain ([`START_NOT_RECORDED`]). The settle of that same run
    ///   comes from the transport's notice, or for a local PTY run from the hub observer
    ///   (already-seen readings through the [`IdleLedger`]).
    /// - Anything else (no receipt, a receipt for another request or session, an unknown
    ///   outcome) records accepted when the target confirmed it, then uncertain, and holds
    ///   `delivery_uncertain`. It is never resent; the owner answers it (the
    ///   `resolve_delivery` request, or [`super::resolve_uncertain_delivery`]).
    ///
    /// Returns `Err` only from a hold, i.e. when nothing was sent.
    async fn settle_transmission(
        &self,
        goal: &SupervisorGoal,
        sent: &Dispatch,
        send: &WorkerSend,
        delivery: Delivery,
        settle: oneshot::Receiver<SettleReport>,
    ) -> Result<DispatchResult, SupervisorError> {
        let host = send.host_id.as_deref().unwrap_or(self.local_host_id.as_str());
        match delivery {
            Delivery::Unsupported { code } => {
                let detail = format!(
                    "{code}: the worker on host {host} cannot report which run {} started; nothing was sent",
                    sent.request_id
                );
                self.hold_claimed(sent, RunState::Degraded, detail).await
            }
            Delivery::Refused { code } => {
                let detail = format!(
                    "{SEND_REFUSED}: {code}; nothing was typed for {}, the owner decides whether to send it again",
                    sent.request_id
                );
                self.hold_claimed(sent, RunState::AwaitingOwner, detail).await
            }
            Delivery::Started(receipt) if receipt.names(send) => {
                if let Err(error) = self.record_started(goal, sent, &receipt).await {
                    tracing::warn!(%error, request_id = %sent.request_id, run_id = %receipt.run_id, "a causal start was not recorded; the dispatch is held delivery_uncertain");
                    return Ok(self
                        .record_uncertain(sent, true, START_NOT_RECORDED.to_owned())
                        .await);
                }
                if send.host_id.is_none() {
                    self.settle_if_idle_seen(&receipt).await;
                }
                self.watch_settle(sent, host.to_owned(), receipt, settle);
                Ok(DispatchResult::Transmitted {
                    request_id: sent.request_id.clone(),
                    outcome: SendOutcome::Started,
                })
            }
            Delivery::Started(receipt) => {
                tracing::warn!(
                    request_id = %sent.request_id,
                    receipt_request = %receipt.request_id,
                    receipt_session = %receipt.session_id,
                    "a start receipt named another request or session; the run is not started"
                );
                Ok(self
                    .record_uncertain(sent, true, RECEIPT_MISMATCH.to_owned())
                    .await)
            }
            Delivery::Uncertain { accepted, code } => {
                Ok(self.record_uncertain(sent, accepted, code).await)
            }
        }
    }

    /// A causal start: accepted, started with the target's run id and session, and the goal's
    /// move to running, in one transaction ([`record_start`]). Then the lane is projected
    /// working and the dispatch turn is recorded once. `Err` wrote no start.
    async fn record_started(
        &self,
        goal: &SupervisorGoal,
        sent: &Dispatch,
        receipt: &StartReceipt,
    ) -> Result<(), SupervisorError> {
        record_start(
            &self.pool,
            &sent.request_id,
            sent.generation,
            &receipt.run_id,
            &receipt.session_id,
        )
        .await?;
        if let Err(error) = crate::lanes::state::transition(
            &self.pool,
            &goal.lane_key,
            LaneStatus::Working,
            Some("supervisor dispatch"),
        )
        .await
        {
            tracing::debug!(%error, lane = %goal.lane_key, "lane status was not projected to working");
        }
        if let Some(origin) = crate::goals::turns::origin_for_request_id(Some(&sent.request_id)) {
            if let Err(error) = crate::goals::turns::record_turn(
                &self.pool,
                &goal.lane_key,
                origin,
                Some(&sent.instruction),
                None,
            )
            .await
            {
                tracing::warn!(%error, request_id = %sent.request_id, "the dispatch turn was not recorded");
            }
        }
        Ok(())
    }

    /// Settles a local PTY run whose ending idle reading the hub observer saw before this
    /// start was committed: the observer found no started run then and publishes no second
    /// reading of an unchanged pane.
    async fn settle_if_idle_seen(&self, receipt: &StartReceipt) {
        let Some((instance, sequence)) = self.idle.idle_after(&receipt.session_id, &receipt.run_id)
        else {
            return;
        };
        match record_pty_settle(
            &self.pool,
            &self.local_host_id,
            &receipt.session_id,
            &instance,
            sequence,
        )
        .await
        {
            Ok(PtySettle::Settled(dispatch)) => {
                if let Some(probe) = &self.settled {
                    let _ = probe.send(dispatch.request_id);
                }
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(%error, run_id = %receipt.run_id, "an already observed settle was not recorded");
            }
        }
    }

    /// No start can be attributed to this dispatch: accepted (when the target confirmed it),
    /// then uncertain, and the goal holds `delivery_uncertain`. Never resent.
    async fn record_uncertain(&self, sent: &Dispatch, accepted: bool, code: String) -> DispatchResult {
        if accepted {
            if let Err(error) =
                record_receipt(&self.pool, &sent.request_id, sent.generation, &Receipt::Accepted)
                    .await
            {
                tracing::warn!(%error, request_id = %sent.request_id, "accepted receipt was not recorded");
            }
        }
        let receipt = Receipt::Uncertain { code: code.clone() };
        if let Err(error) =
            record_receipt(&self.pool, &sent.request_id, sent.generation, &receipt).await
        {
            tracing::warn!(%error, request_id = %sent.request_id, "uncertain receipt was not recorded");
        }
        let stage = if accepted { "accepted" } else { "sent" };
        let detail = format!(
            "{code}: {} was {stage} but no start of its run was confirmed; it is not resent",
            sent.request_id
        );
        if let Err(error) = transition_goal(
            &self.pool,
            &sent.goal_id,
            sent.goal_revision,
            RunState::Dispatching,
            RunState::DeliveryUncertain,
            Some(&detail),
        )
        .await
        {
            tracing::warn!(%error, request_id = %sent.request_id, "goal did not move to delivery_uncertain");
        }
        DispatchResult::Transmitted {
            request_id: sent.request_id.clone(),
            outcome: SendOutcome::Uncertain { code },
        }
    }

    /// Records the settle a transport reports for the run it started. A notice naming any
    /// other run is ignored; a transport that leaves the end to another observer drops the
    /// notice unsent. A run whose end can no longer be observed holds its goal degraded with
    /// the transport's code, instead of leaving it `running` unseen.
    fn watch_settle(
        &self,
        sent: &Dispatch,
        host: String,
        started: StartReceipt,
        settle: oneshot::Receiver<SettleReport>,
    ) {
        let pool = self.pool.clone();
        let request_id = sent.request_id.clone();
        let goal_id = sent.goal_id.clone();
        let generation = sent.generation;
        let probe = self.settled.clone();
        tokio::spawn(async move {
            let Ok(report) = settle.await else {
                return;
            };
            let ended = match report {
                SettleReport::Settled(ended) => ended,
                SettleReport::Unobserved { run, code, reason } => {
                    if run != started {
                        tracing::warn!(request_id = %request_id, run_id = %run.run_id, "an unobserved settle named another run; it was ignored");
                        return;
                    }
                    hold_unobserved(&pool, &goal_id, &request_id, &host, &run, &code, &reason).await;
                    if let Some(probe) = probe {
                        let _ = probe.send(request_id);
                    }
                    return;
                }
            };
            if ended != started {
                tracing::warn!(request_id = %request_id, run_id = %ended.run_id, "a settle named another run; it was ignored");
                return;
            }
            let settled = Receipt::Settled {
                run_id: ended.run_id,
                provider_session_id: ended.session_id,
            };
            match record_receipt(&pool, &request_id, generation, &settled).await {
                Ok(_) => {
                    if let Some(probe) = probe {
                        let _ = probe.send(request_id);
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, request_id = %request_id, "settled receipt was not recorded");
                }
            }
        });
    }
}

/// Holds the goal of a run whose end can no longer be observed: `degraded` with the code,
/// while that run is still the goal's live run and the goal is still `running`. The dispatch
/// stays started (in flight), so nothing is sent until the owner decides.
async fn hold_unobserved(
    pool: &SqlitePool,
    goal_id: &str,
    request_id: &str,
    host: &str,
    run: &StartReceipt,
    code: &str,
    reason: &str,
) {
    let detail = format!(
        "{code}: {reason}; run {} of {request_id} may still be running on host {host}",
        run.run_id
    );
    if let Err(error) = hold_live_run(pool, goal_id, request_id, &run.run_id, &detail).await {
        tracing::warn!(%error, request_id, "the goal of an unobserved run was not held");
    }
}
