//! Verified completion of supervisor goals, driven by durable run completion.
//!
//! A run's settle is a durable receipt (a transport's settle notice, or the hub observer's idle
//! reading of a local PTY pane through [`super::receipts::record_pty_settle`]). The wake
//! scheduler turns it into a `turn_settled` wake and [`StateHandler`] moves the goal to
//! `verifying`; [`CompletionHandler`] then runs [`CompletionDriver::verify`] on that goal:
//!
//! - Only a settled run on the binding's current generation that no later instruction
//!   superseded is graded. Owner holds (pause, awaiting the owner) bump the goal revision but do
//!   not change what the worker was asked, so a run that outlived a pause is still graded. Only
//!   an instruction queued after the run (an owner correction, which also bumps the revision
//!   or the requirement set) makes it stale ([`STALE_SETTLE`]): that instruction goes out, or a
//!   fresh run against the current requirements is started within the turn budget.
//! - Before grading, the bound pane is re-read: a pane that is not idle (a quiet-timer idle
//!   that settled a run early) is not graded and the settle waits.
//! - Evidence is collected fresh from the bound worker session on its host, never from the
//!   lane's legacy pane, and the existing gap assessment grades it. `complete` is checked by the
//!   blind convergence evaluation, which gets the contract and attested evidence only (a fresh
//!   decision context, no earlier assessment). An agent claim or a cited ref Ferryx did not
//!   attest never proves a requirement.
//! - Each met requirement must also cite the check Ferryx itself ran from that requirement's
//!   verifier ([`evidence::requirement_check_ref`]), passing; observer state and the screen
//!   (worker-controlled text) never prove a requirement alone. A waived requirement needs no
//!   check. A verifier Ferryx cannot run holds the goal for the owner.
//! - Every unproven outcome with actionable work becomes one corrective run: the next ready
//!   intent at the same revision, counted as one continuation turn, without any owner input.
//!   A goal that is not `full`, or whose turn budget is spent, is held for the owner instead.
//!   Every corrective instruction carries the persisted grant (scope, allowed operations, what
//!   needs the owner); one that names a withheld operation is held for the owner instead.
//! - Every write is fenced on the goal revision and state the decision was made against, the
//!   lane pause and (for completion) the execution generation, in one transaction. An owner
//!   correction, pause or cancel that commits while the judge runs writes nothing stale.
//! - A judge failure leaves the goal `verifying` with a visible [`JUDGE_UNAVAILABLE`] or
//!   [`SUPERVISOR_DEGRADED`] detail and keeps its wake pending; the wake scheduler re-offers
//!   it, but the judge is asked again only after [`JUDGE_RETRY`] (doubled per consecutive
//!   failure, capped at [`JUDGE_RETRY_MAX`]). Never completed, never a model call per tick.
//! - Completion records its provenance (project, goal revision, host, run, request, session and
//!   source revision, the latter only for a run on this machine) in the goal's state detail and
//!   on every met requirement.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::Serialize;
use sqlx::SqlitePool;

use super::admission::{goal_grant, OperationClass};
use super::dispatcher::{LANE_PAUSED, OWNER_APPROVAL_REQUIRED, TURN_LIMIT};
use super::wakes::{StateHandler, WakeAck, WakeHandler, WakeTurn, STALE_SETTLE};
use super::{
    check_revision, execution_binding_of, goal_of, insert_dispatch, require_binding, require_goal,
    transition_goal, transition_goal_in, wakes, ExecutionBinding, RunState, SupervisorError,
    SupervisorGoal,
};
use crate::agent_control::{AgentControl, AgentOp};
use crate::goals::contract::{self, Contract};
use crate::goals::converge::{self, ConvergenceOutcome, RequirementVerdict};
use crate::goals::evidence::{requirement_check_ref, verifier_signal, EvidenceBundle};
use crate::goals::loop_driver::{verification_instruction, wrap_instruction};
use crate::goals::types::{Decision, GoalError, GoalMode, RequirementStatus};
use crate::goals::{assess, evidence, turns};
use crate::judge::Judge;
use crate::lanes::state::{self, LaneStatus, GOAL_CONVERGED_REASON};
use crate::storage::{now_rfc3339, WriteTx};

/// State-detail prefix of a completed goal; the rest is its [`RunEvidence`] as JSON.
pub const VERIFIED: &str = "VERIFIED";
/// State-detail prefix of a goal sent back to work after its run failed verification.
pub const VERIFIER_FAILED: &str = "VERIFIER_FAILED";
/// The judge could not grade the run; the goal is held degraded, never completed.
pub const JUDGE_UNAVAILABLE: &str = "JUDGE_UNAVAILABLE";
/// The supervisor engine reported a typed fault while grading.
pub const SUPERVISOR_DEGRADED: &str = "SUPERVISOR_DEGRADED";
/// Skip code of a decision whose goal moved (revision or state) while it was made.
pub const REVISION_CONFLICT: &str = "REVISION_CONFLICT";
/// Skip code of a settle whose pane is not idle when it is about to be graded.
pub const PANE_NOT_IDLE: &str = "PANE_NOT_IDLE";
/// Skip code of a degraded goal whose judge retry is not due yet.
pub const RETRY_NOT_DUE: &str = "RETRY_NOT_DUE";
/// Hold code: a requirement's verifier is not a check Ferryx can run, so only the owner can
/// accept it (waive it, or give a runnable verifier).
pub const VERIFIER_NOT_RUNNABLE: &str = "VERIFIER_NOT_RUNNABLE";
/// Hold code: a corrective instruction names an operation the grant withholds.
pub const GRANT_EXCEEDED: &str = "GRANT_EXCEEDED";
/// Hold code: the run is on another host, where Ferryx cannot run the requirement checks from
/// this hub (its checkout at the same path is not that host's), so only the owner can accept it.
pub const REMOTE_CHECK_UNAVAILABLE: &str = "REMOTE_CHECK_UNAVAILABLE";
/// First judge retry delay after a failure; doubled per consecutive failure up to
/// [`JUDGE_RETRY_MAX`].
pub const JUDGE_RETRY: Duration = Duration::from_secs(30);
pub const JUDGE_RETRY_MAX: Duration = Duration::from_secs(30 * 60);

/// What a completion is attached to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunEvidence {
    pub project_id: String,
    pub goal_id: String,
    pub goal_revision: i64,
    pub host_id: String,
    pub run_id: String,
    pub request_id: String,
    pub provider_session_id: Option<String>,
    /// `git rev-parse HEAD` of the lane's worktree when it was verified.
    pub source_revision: Option<String>,
    /// The attested refs the convergence verdicts cited.
    pub evidence_refs: Vec<String>,
}

impl RunEvidence {
    /// The provenance ref stored next to a met requirement's evidence refs.
    fn provenance_ref(&self) -> String {
        format!(
            "run:{}/{}/{}@r{}{}",
            self.project_id,
            self.host_id,
            self.run_id,
            self.goal_revision,
            self.source_revision
                .as_deref()
                .map(|head| format!("#{head}"))
                .unwrap_or_default()
        )
    }
}

/// What one verification pass did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verification {
    /// Every requirement is proven by attested evidence of the current run: `completed`.
    Completed(Box<RunEvidence>),
    /// The run failed verification; one corrective run is ready at the same revision.
    Corrective {
        request_id: String,
        unmet: Vec<String>,
        turns_used: u32,
    },
    /// The settled run answered an older revision; the owner's ready instruction at the
    /// current revision goes out instead.
    Resumed { request_id: String },
    /// Held for the owner, or degraded, with this detail.
    Held { state: RunState, detail: String },
    /// Nothing written: the goal is not verifying, its lane is paused, or it moved while the
    /// decision was made ([`REVISION_CONFLICT`]).
    Skipped { code: String },
}

fn skipped(code: &str) -> Verification {
    Verification::Skipped {
        code: code.to_owned(),
    }
}

/// The newest settled dispatch of a goal, and its instruction.
#[derive(Debug, Clone, sqlx::FromRow)]
struct SettledRun {
    id: i64,
    request_id: String,
    goal_revision: i64,
    generation: i64,
    run_id: Option<String>,
    provider_session_id: Option<String>,
}

pub struct CompletionDriver {
    pool: SqlitePool,
    judge: Arc<Judge>,
    control: Arc<dyn AgentControl>,
    /// The host id agent control reaches without a host argument.
    local_host_id: String,
    /// Per goal: consecutive judge failures and when the next grading is due.
    retries: Mutex<HashMap<String, (u32, Instant)>>,
}

impl CompletionDriver {
    pub fn new(
        pool: SqlitePool,
        judge: Arc<Judge>,
        control: Arc<dyn AgentControl>,
        local_host_id: impl Into<String>,
    ) -> Self {
        Self {
            pool,
            judge,
            control,
            local_host_id: local_host_id.into(),
            retries: Mutex::new(HashMap::new()),
        }
    }

    fn retry_due(&self, goal_id: &str) -> bool {
        let retries = self.retries.lock().unwrap_or_else(|poison| poison.into_inner());
        retries
            .get(goal_id)
            .is_none_or(|(_, due)| Instant::now() >= *due)
    }

    fn note_judge_failure(&self, goal_id: &str) {
        let mut retries = self.retries.lock().unwrap_or_else(|poison| poison.into_inner());
        let failures = retries.get(goal_id).map_or(0, |(count, _)| *count) + 1;
        let delay = JUDGE_RETRY
            .saturating_mul(1u32 << (failures - 1).min(10))
            .min(JUDGE_RETRY_MAX);
        retries.insert(goal_id.to_owned(), (failures, Instant::now() + delay));
    }

    fn clear_retry(&self, goal_id: &str) {
        self.retries
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .remove(goal_id);
    }

    /// Grades the goal's settled run once, if the goal is `verifying` (or `degraded` by an
    /// earlier judge failure whose retry is due). See the module docs.
    pub async fn verify(&self, goal_id: &str) -> Result<Verification, SupervisorError> {
        let Some(goal) = goal_of(&self.pool, goal_id).await? else {
            return Ok(skipped("NOT_FOUND"));
        };
        if goal.run_state != RunState::Verifying {
            return Ok(skipped(goal.run_state.as_str()));
        }
        if !self.retry_due(goal_id) {
            return Ok(skipped(RETRY_NOT_DUE));
        }
        let Some(binding) = execution_binding_of(&self.pool, goal_id).await? else {
            return Ok(skipped("NOT_FOUND"));
        };
        // A person's lane pause gates ongoing decisions as well as unsent work.
        if crate::lanes::pause::is_paused(&self.pool, &goal.lane_key)
            .await
            .unwrap_or(true)
        {
            return Ok(skipped(LANE_PAUSED));
        }
        let Some(contract) = contract::contract_of(&self.pool, &goal.lane_key).await? else {
            let detail = format!(
                "{OWNER_APPROVAL_REQUIRED}: lane {} has no goal contract",
                goal.lane_key
            );
            return self.hold(&goal, RunState::AwaitingOwner, detail).await;
        };
        let settled: Option<SettledRun> = sqlx::query_as(
            "SELECT id, request_id, goal_revision, generation, run_id, provider_session_id
               FROM supervisor_dispatches
              WHERE goal_id = ? AND state = 'settled'
              ORDER BY id DESC LIMIT 1",
        )
        .bind(goal_id)
        .fetch_optional(&self.pool)
        .await?;
        let current = match settled {
            Some(run)
                if run.generation == binding.generation
                    && run.run_id.is_some()
                    && !self.superseded(&goal, &run).await? =>
            {
                Some(run)
            }
            _ => None,
        };
        let Some(run) = current else {
            return self.stale(&goal, &contract).await;
        };
        // Every requirement that is not waived needs a check Ferryx itself can run.
        let unrunnable: Vec<&str> = contract
            .requirements
            .iter()
            .filter(|requirement| requirement.status != RequirementStatus::Waived)
            .filter(|requirement| verifier_signal(&requirement.verifier).is_none())
            .map(|requirement| requirement.id.as_str())
            .collect();
        if !unrunnable.is_empty() {
            let detail = format!(
                "{VERIFIER_NOT_RUNNABLE}: Ferryx cannot run the verifier of {}; the owner waives it or gives a command or file check",
                unrunnable.join(", ")
            );
            return self.hold(&goal, RunState::AwaitingOwner, detail).await;
        }
        let host = (binding.host_id != self.local_host_id).then_some(binding.host_id.as_str());
        let checked: Vec<&str> = contract
            .requirements
            .iter()
            .filter(|requirement| requirement.status != RequirementStatus::Waived)
            .map(|requirement| requirement.id.as_str())
            .collect();
        if let (Some(host), false) = (host, checked.is_empty()) {
            let detail = format!(
                "{REMOTE_CHECK_UNAVAILABLE}: the run is on host {host}; Ferryx cannot run the checks of {} from this hub",
                checked.join(", ")
            );
            return self.hold(&goal, RunState::AwaitingOwner, detail).await;
        }
        // A verifier script runs as it was at admission; one that cannot be (no admitted
        // revision, or the script is not in it) is held, never run as the worker left it.
        if host.is_none() {
            let unpinned =
                evidence::unpinned_verifiers(&self.pool, &goal.lane_key, &contract.requirements)
                    .await?;
            if !unpinned.is_empty() {
                let detail = format!(
                    "{}; the owner waives the requirement or commits its verifier",
                    unpinned.join("; ")
                );
                return self.hold(&goal, RunState::AwaitingOwner, detail).await;
            }
        }
        // An early settle (a quiet-timer idle) is not graded: the pane must read idle now.
        if let Some(session_id) = run.provider_session_id.as_deref() {
            if !self.pane_idle(host, session_id).await {
                return Ok(skipped(PANE_NOT_IDLE));
            }
        }
        let bundle = evidence::collect_for_run(
            &self.pool,
            self.control.as_ref(),
            &goal.lane_key,
            host,
            run.provider_session_id.clone(),
            &contract.requirements,
        )
        .await?;
        let recent = turns::turns_of(&self.pool, &goal.lane_key).await?;
        let assessment = match assess::assess(&self.judge, &contract, &bundle, &recent).await {
            Ok(assessment) => assessment,
            Err(error) => return self.judge_failed(&goal, error).await,
        };
        self.clear_retry(goal_id);
        match assessment.decision {
            Decision::Stop { reason } => {
                let gaps: Vec<&str> = assessment
                    .gaps
                    .iter()
                    .map(|gap| gap.summary.as_str())
                    .collect();
                let detail = format!("{}: {}", reason.as_str(), gaps.join("; "));
                self.hold(&goal, RunState::AwaitingOwner, detail).await
            }
            Decision::Continue {
                instruction,
                gap_ids,
                ..
            } => {
                let mut unmet: Vec<String> = assessment
                    .gaps
                    .iter()
                    .filter(|gap| gap_ids.contains(&gap.id))
                    .flat_map(|gap| gap.requirement_ids.iter().cloned())
                    .collect();
                unmet.sort();
                unmet.dedup();
                self.correct(&goal, &instruction, unmet).await
            }
            // The blind check: contract and attested evidence only, in a fresh context.
            Decision::Complete => match converge::converge(&self.judge, &contract, &bundle).await {
                Err(error) => self.judge_failed(&goal, error).await,
                Ok(ConvergenceOutcome::Diverged { unmet_ids, .. }) => {
                    self.correct(&goal, &verification_instruction(&unmet_ids), unmet_ids)
                        .await
                }
                Ok(ConvergenceOutcome::Converged { verdicts }) => {
                    let unchecked = unproven_by_ferryx(&contract, &verdicts, &bundle);
                    if unchecked.is_empty() {
                        self.finish(&goal, &binding, &run, &contract, &verdicts).await
                    } else {
                        self.correct(&goal, &verification_instruction(&unchecked), unchecked)
                            .await
                    }
                }
            },
        }
    }

    /// True when an instruction of a later goal revision was queued after `run` (an owner
    /// correction; a requirement change arrives as one too): the run no longer answers what the
    /// worker is asked now. An owner hold alone bumps the goal revision but queues nothing, and
    /// a resume's re-queue of a never-sent intent comes only when no run settled after it.
    async fn superseded(
        &self,
        goal: &SupervisorGoal,
        run: &SettledRun,
    ) -> Result<bool, SupervisorError> {
        Ok(sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM supervisor_dispatches
                WHERE goal_id = ? AND id > ? AND goal_revision > ?)",
        )
        .bind(&goal.goal_id)
        .bind(run.id)
        .bind(run.goal_revision)
        .fetch_one(&self.pool)
        .await?)
    }

    /// The bound pane reads idle now with no pending prompt (`session.pending`). An unreadable
    /// pane counts as not idle.
    async fn pane_idle(&self, host: Option<&str>, session_id: &str) -> bool {
        let read = self
            .control
            .execute(
                host,
                AgentOp::Read {
                    session_id: session_id.to_owned(),
                },
            )
            .await;
        match read {
            Ok(value) => {
                let state = value.pointer("/session/state").and_then(|state| state.as_str());
                let prompt = value
                    .pointer("/session/pending")
                    .is_some_and(|pending| !pending.is_null() && pending != &serde_json::json!([]));
                matches!(state, Some("idle" | "done")) && !prompt
            }
            Err(error) => {
                tracing::debug!(%error, session_id, "the settled pane was not read; grading waits");
                false
            }
        }
    }

    /// The settled run is not evidence for the current revision. The owner's instruction at
    /// this revision goes out when there is one; otherwise a fresh run works against the
    /// current requirements, within the turn budget.
    async fn stale(
        &self,
        goal: &SupervisorGoal,
        contract: &Contract,
    ) -> Result<Verification, SupervisorError> {
        let ready: Option<String> = sqlx::query_scalar(
            "SELECT request_id FROM supervisor_dispatches
              WHERE goal_id = ? AND state = 'ready' AND goal_revision = ?
              ORDER BY id LIMIT 1",
        )
        .bind(&goal.goal_id)
        .bind(goal.revision)
        .fetch_optional(&self.pool)
        .await?;
        if let Some(request_id) = ready {
            let detail = format!(
                "{STALE_SETTLE}: the last run answered an older revision; {request_id} at revision {} goes out",
                goal.revision
            );
            return match transition_goal(
                &self.pool,
                &goal.goal_id,
                goal.revision,
                RunState::Verifying,
                RunState::Ready,
                Some(&detail),
            )
            .await
            {
                Ok(_) => {
                    super::dispatcher::poke();
                    Ok(Verification::Resumed { request_id })
                }
                Err(error) => conflict_or(error),
            };
        }
        let unmet: Vec<String> = contract
            .requirements
            .iter()
            .map(|requirement| requirement.id.clone())
            .collect();
        let listed: Vec<String> = contract
            .requirements
            .iter()
            .map(|requirement| {
                format!(
                    "{} ({}; verify with: {})",
                    requirement.id, requirement.text, requirement.verifier
                )
            })
            .collect();
        let instruction = format!(
            "No finished run answers the current goal (revision {}), so earlier work does not count as verified. Work against the current requirements: {}. Run each verifier and report the exact commands and results.",
            goal.revision,
            listed.join("; ")
        );
        self.correct(goal, &instruction, unmet).await
    }

    /// Moves a still-verifying goal at the decision's revision to a hold.
    async fn hold(
        &self,
        goal: &SupervisorGoal,
        to: RunState,
        detail: String,
    ) -> Result<Verification, SupervisorError> {
        match transition_goal(
            &self.pool,
            &goal.goal_id,
            goal.revision,
            RunState::Verifying,
            to,
            Some(&detail),
        )
        .await
        {
            Ok(_) => Ok(Verification::Held { state: to, detail }),
            Err(error) => conflict_or(error),
        }
    }

    async fn judge_failed(
        &self,
        goal: &SupervisorGoal,
        error: GoalError,
    ) -> Result<Verification, SupervisorError> {
        let (code, detail) = match error {
            GoalError::JudgeUnavailable => (
                JUDGE_UNAVAILABLE,
                format!("{JUDGE_UNAVAILABLE}: the run could not be graded yet; it is not completed"),
            ),
            GoalError::SupervisorFault(fault) => {
                (SUPERVISOR_DEGRADED, format!("{SUPERVISOR_DEGRADED}: {fault}"))
            }
            other => return Err(SupervisorError::Goal(other)),
        };
        self.note_judge_failure(&goal.goal_id);
        // The goal stays verifying with a visible reason; its wake stays pending and the judge
        // is asked again only once the backoff is due.
        sqlx::query(
            "UPDATE supervisor_goals SET state_detail = ?, updated_at = ?
              WHERE goal_id = ? AND revision = ? AND run_state = 'verifying'",
        )
        .bind(&detail)
        .bind(now_rfc3339())
        .bind(&goal.goal_id)
        .bind(goal.revision)
        .execute(&self.pool)
        .await?;
        Ok(skipped(code))
    }

    /// One corrective run at the decision's revision, counted as one continuation turn, in
    /// one transaction with the goal's move back to `ready`. Without `full` mode, or with the
    /// turn budget spent, or when the instruction names an operation the grant withholds, the
    /// goal is held for the owner instead. A lane pause or an owner instruction already queued
    /// at this revision writes nothing. The instruction carries the persisted grant.
    async fn correct(
        &self,
        goal: &SupervisorGoal,
        instruction: &str,
        unmet: Vec<String>,
    ) -> Result<Verification, SupervisorError> {
        let grant = goal_grant(&self.pool, &goal.goal_id).await?;
        let withheld = withheld_named(instruction, &grant);
        let cwd: Option<String> = sqlx::query_scalar("SELECT cwd FROM lanes WHERE key = ?")
            .bind(&goal.lane_key)
            .fetch_optional(&self.pool)
            .await?;
        let binding_host = execution_binding_of(&self.pool, &goal.goal_id)
            .await?
            .map(|binding| binding.host_id)
            .unwrap_or_default();
        let instruction = format!(
            "{}\n\n{}",
            instruction.trim(),
            grant_text(cwd.as_deref().unwrap_or(""), &binding_host, &grant)
        );
        let mut tx = self.pool.begin_write().await?;
        let now = require_goal(&mut tx, &goal.goal_id).await?;
        if check_revision(&now, goal.revision).is_err() || now.run_state != RunState::Verifying {
            return Ok(skipped(REVISION_CONFLICT));
        }
        if lane_paused_in(&mut tx, &goal.lane_key).await? {
            return Ok(skipped(LANE_PAUSED));
        }
        let queued: Option<String> = sqlx::query_scalar(
            "SELECT request_id FROM supervisor_dispatches
              WHERE goal_id = ? AND state = 'ready' AND goal_revision = ?
              ORDER BY id LIMIT 1",
        )
        .bind(&goal.goal_id)
        .bind(goal.revision)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(request_id) = queued {
            // An instruction is already waiting at this revision (an owner's): it goes out
            // instead of a second one, and no turn is counted.
            let detail = format!("{request_id} was already queued; it goes out instead of a correction");
            transition_goal_in(
                &mut tx,
                &goal.goal_id,
                goal.revision,
                RunState::Verifying,
                RunState::Ready,
                Some(&detail),
            )
            .await?;
            tx.commit().await?;
            wakes::poke();
            super::dispatcher::poke();
            return Ok(Verification::Resumed { request_id });
        }
        let terms: Option<(String, i64, i64)> = sqlx::query_as(
            "SELECT mode, turn_limit, turns_used FROM lane_goal WHERE lane_key = ?",
        )
        .bind(&goal.lane_key)
        .fetch_optional(&mut *tx)
        .await?;
        let unproven = if unmet.is_empty() {
            "the goal".to_owned()
        } else {
            unmet.join(", ")
        };
        let hold = match terms {
            None => Some(format!(
                "{OWNER_APPROVAL_REQUIRED}: lane {} has no goal contract",
                goal.lane_key
            )),
            Some((mode, _, _)) if mode != GoalMode::Full.as_str() => Some(format!(
                "{OWNER_APPROVAL_REQUIRED}: unproven {unproven}; a {mode} goal does not continue without the owner"
            )),
            Some((_, limit, used)) if used >= limit => Some(format!(
                "{TURN_LIMIT}: {used} of {limit} turns are used; unproven {unproven}"
            )),
            Some(_) if !withheld.is_empty() => Some(format!(
                "{GRANT_EXCEEDED}: the corrective instruction for {unproven} names {}, which the grant withholds; the owner decides",
                withheld.join(", ")
            )),
            Some(_) => None,
        };
        if let Some(detail) = hold {
            transition_goal_in(
                &mut tx,
                &goal.goal_id,
                goal.revision,
                RunState::Verifying,
                RunState::AwaitingOwner,
                Some(&detail),
            )
            .await?;
            tx.commit().await?;
            wakes::poke();
            return Ok(Verification::Held {
                state: RunState::AwaitingOwner,
                detail,
            });
        }
        let turns_used: i64 = sqlx::query_scalar(
            "UPDATE lane_goal SET turns_used = turns_used + 1, updated_at = ?
              WHERE lane_key = ? RETURNING turns_used",
        )
        .bind(now_rfc3339())
        .bind(&goal.lane_key)
        .fetch_one(&mut *tx)
        .await?;
        let generation = require_binding(&mut tx, &goal.goal_id).await?.generation;
        let dispatch = insert_dispatch(
            &mut tx,
            &goal.goal_id,
            goal.revision,
            generation,
            &wrap_instruction(&instruction),
            &now_rfc3339(),
        )
        .await?;
        let detail = format!(
            "{VERIFIER_FAILED}: unproven {unproven}; corrective run {}",
            dispatch.request_id
        );
        transition_goal_in(
            &mut tx,
            &goal.goal_id,
            goal.revision,
            RunState::Verifying,
            RunState::Ready,
            Some(&detail),
        )
        .await?;
        tx.commit().await?;
        wakes::poke();
        super::dispatcher::poke();
        Ok(Verification::Corrective {
            request_id: dispatch.request_id,
            unmet,
            turns_used: u32::try_from(turns_used).unwrap_or(u32::MAX),
        })
    }

    /// Marks every requirement met with its verdict's refs plus the run's provenance and
    /// completes the goal, in one transaction fenced on the decision's goal revision and the
    /// contract revision, the run's execution generation and the lane pause. The source
    /// revision is read only for a run on this machine.
    async fn finish(
        &self,
        goal: &SupervisorGoal,
        binding: &ExecutionBinding,
        run: &SettledRun,
        contract: &Contract,
        verdicts: &[RequirementVerdict],
    ) -> Result<Verification, SupervisorError> {
        let cwd: Option<String> = sqlx::query_scalar("SELECT cwd FROM lanes WHERE key = ?")
            .bind(&goal.lane_key)
            .fetch_optional(&self.pool)
            .await?;
        let source_revision = match cwd {
            Some(cwd) if binding.host_id == self.local_host_id => {
                evidence::source_revision(&cwd).await
            }
            _ => None,
        };
        let mut refs: Vec<String> = verdicts
            .iter()
            .flat_map(|verdict| verdict.evidence_refs.iter().cloned())
            .collect();
        refs.sort();
        refs.dedup();
        let record = RunEvidence {
            project_id: goal.project_id.clone(),
            goal_id: goal.goal_id.clone(),
            goal_revision: goal.revision,
            host_id: binding.host_id.clone(),
            run_id: run.run_id.clone().unwrap_or_default(),
            request_id: run.request_id.clone(),
            provider_session_id: run.provider_session_id.clone(),
            source_revision,
            evidence_refs: refs,
        };
        let provenance = record.provenance_ref();
        let detail = format!(
            "{VERIFIED}: {}",
            serde_json::to_string(&record).unwrap_or_default()
        );

        let mut tx = self.pool.begin_write().await?;
        let now = require_goal(&mut tx, &goal.goal_id).await?;
        if check_revision(&now, goal.revision).is_err() || now.run_state != RunState::Verifying {
            return Ok(skipped(REVISION_CONFLICT));
        }
        if require_binding(&mut tx, &goal.goal_id).await?.generation != run.generation {
            // A stale generation never marks a newer run complete.
            return Ok(skipped(REVISION_CONFLICT));
        }
        if lane_paused_in(&mut tx, &goal.lane_key).await? {
            return Ok(skipped(LANE_PAUSED));
        }
        let at = now_rfc3339();
        for verdict in verdicts {
            let mut evidence_refs = verdict.evidence_refs.clone();
            evidence_refs.push(provenance.clone());
            let evidence_json = serde_json::to_string(&evidence_refs)
                .map_err(|error| SupervisorError::Invalid(error.to_string()))?;
            let updated = sqlx::query(
                "UPDATE lane_goal_requirements SET status = ?, evidence_json = ?, updated_at = ?
                  WHERE lane_key = ? AND req_id = ?
                    AND EXISTS(SELECT 1 FROM lane_goal WHERE lane_key = ? AND revision = ?)",
            )
            .bind(RequirementStatus::Met.as_str())
            .bind(evidence_json)
            .bind(&at)
            .bind(&goal.lane_key)
            .bind(&verdict.req_id)
            .bind(&goal.lane_key)
            .bind(contract.revision)
            .execute(&mut *tx)
            .await?;
            if updated.rows_affected() != 1 {
                // The requirement set changed since it was graded: nothing is written.
                return Ok(skipped(REVISION_CONFLICT));
            }
        }
        transition_goal_in(
            &mut tx,
            &goal.goal_id,
            goal.revision,
            RunState::Verifying,
            RunState::Completed,
            Some(&detail),
        )
        .await?;
        tx.commit().await?;
        wakes::poke();
        self.clear_retry(&goal.goal_id);
        if let Err(error) = state::transition(
            &self.pool,
            &goal.lane_key,
            LaneStatus::VerifiedDone,
            Some(GOAL_CONVERGED_REASON),
        )
        .await
        {
            tracing::debug!(%error, lane = %goal.lane_key, "lane status was not projected to verified_done");
        }
        Ok(Verification::Completed(Box::new(record)))
    }
}

/// A transition the goal's movement refused is a skip, not an error.
fn conflict_or(error: SupervisorError) -> Result<Verification, SupervisorError> {
    match error {
        SupervisorError::RevisionConflict { .. }
        | SupervisorError::StateConflict { .. }
        | SupervisorError::IllegalTransition { .. }
        | SupervisorError::GoalClosed { .. } => Ok(skipped(REVISION_CONFLICT)),
        other => Err(other),
    }
}

/// The lane has an active pause, read inside the caller's write transaction.
async fn lane_paused_in(
    conn: &mut sqlx::SqliteConnection,
    lane_key: &str,
) -> Result<bool, SupervisorError> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM lane_pauses WHERE lane_key = ? AND resumed_at IS NULL)",
    )
    .bind(lane_key)
    .fetch_one(&mut *conn)
    .await?)
}

/// Requirements a convergence verdict calls met without citing the passing check Ferryx ran
/// from that requirement's own verifier. Waived requirements need none.
fn unproven_by_ferryx(
    contract: &Contract,
    verdicts: &[RequirementVerdict],
    bundle: &EvidenceBundle,
) -> Vec<String> {
    contract
        .requirements
        .iter()
        .filter(|requirement| requirement.status != RequirementStatus::Waived)
        .filter(|requirement| {
            let check = requirement_check_ref(&requirement.id);
            let cited = verdicts.iter().any(|verdict| {
                verdict.req_id == requirement.id
                    && verdict.met
                    && verdict.evidence_refs.contains(&check)
            });
            let passed = bundle
                .get(&check)
                .is_some_and(|item| item.passed == Some(true));
            !(cited && passed)
        })
        .map(|requirement| requirement.id.clone())
        .collect()
}

/// Words that name each operation class in an instruction, matched as whole words.
fn operation_words(operation: OperationClass) -> &'static [&'static str] {
    match operation {
        OperationClass::Edit | OperationClass::Test | OperationClass::Commit => &[],
        OperationClass::Push => &["push", "git push", "force-push"],
        OperationClass::Publish => &["publish", "release", "deploy", "npm publish", "cargo publish"],
        OperationClass::Spend => &["purchase", "buy", "pay", "subscribe", "billing"],
        OperationClass::Destructive => &["rm -rf", "force-push", "reset --hard", "drop table", "delete the branch"],
    }
}

/// The withheld operations a corrective instruction names. Edit, test and commit are never
/// withheld words: they are what any correction does within the worktree.
fn withheld_named(instruction: &str, grant: &[OperationClass]) -> Vec<&'static str> {
    let lower = instruction.to_ascii_lowercase();
    let words: Vec<&str> = lower
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-'))
        .filter(|word| !word.is_empty())
        .collect();
    let names = |phrase: &str| {
        if phrase.contains(' ') {
            lower.contains(phrase)
        } else {
            words.iter().any(|word| *word == phrase)
        }
    };
    OperationClass::ALL
        .iter()
        .filter(|operation| !grant.contains(operation))
        .filter(|operation| operation_words(**operation).iter().any(|phrase| names(phrase)))
        .map(|operation| operation.as_str())
        .collect()
}

/// The grant every corrective instruction carries, worded like the first instruction's.
fn grant_text(cwd: &str, host_id: &str, grant: &[OperationClass]) -> String {
    let allowed: Vec<&str> = grant.iter().map(|operation| operation.as_str()).collect();
    let withheld: Vec<&str> = OperationClass::ALL
        .iter()
        .filter(|operation| !grant.contains(operation))
        .map(|operation| operation.as_str())
        .collect();
    let allowed = if allowed.is_empty() {
        "none; this request is read-only: inspect and report, and change no files".to_owned()
    } else {
        allowed.join(", ")
    };
    let mut text = format!(
        "Scope: {cwd} on host {host_id}; work only inside your starting directory. Allowed operations: {allowed}."
    );
    if !withheld.is_empty() {
        text.push_str(&format!(" Ask the owner before any of: {}.", withheld.join(", ")));
    }
    text
}

/// The daemon's wake handler: [`StateHandler`]'s deterministic moves, then a verification
/// pass for a goal they left `verifying` (a settle, met dependencies, or an owner correction
/// that landed while a decision was made). A goal that moved while it was graded, or a store
/// error, keeps the turn pending (`Busy`) so it is graded again against the new state; a
/// paused lane keeps it pending until the lane resumes.
pub struct CompletionHandler {
    state: StateHandler,
    driver: CompletionDriver,
}

impl CompletionHandler {
    pub fn new(pool: SqlitePool, driver: CompletionDriver) -> Self {
        Self {
            state: StateHandler::new(pool),
            driver,
        }
    }
}

#[async_trait]
impl WakeHandler for CompletionHandler {
    async fn handle(&self, turn: &WakeTurn) -> WakeAck {
        let ack = self.state.handle(turn).await;
        if ack != WakeAck::Done {
            return ack;
        }
        match self.driver.verify(&turn.goal_id).await {
            Ok(Verification::Skipped { code }) if code == REVISION_CONFLICT => WakeAck::Busy,
            // Kept pending and re-offered: a paused lane until it resumes, a pane not idle
            // until it is, a judge failure until its backoff is due.
            Ok(Verification::Skipped { code })
                if [LANE_PAUSED, PANE_NOT_IDLE, RETRY_NOT_DUE, JUDGE_UNAVAILABLE, SUPERVISOR_DEGRADED]
                    .contains(&code.as_str()) =>
            {
                WakeAck::NotApplicable
            }
            Ok(_) => WakeAck::Done,
            Err(error) => {
                tracing::warn!(%error, goal_id = %turn.goal_id, "a settled run was not verified; retrying later");
                WakeAck::Busy
            }
        }
    }
}
