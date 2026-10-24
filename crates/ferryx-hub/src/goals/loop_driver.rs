//! The supervisor goal loop: on every settled turn of a lane with a goal contract, collect
//! Ferryx's evidence, grade the lane against its contract, and either hand it to the owner,
//! propose (or send) the next bounded instruction, or verify completion blind.
//!
//! Lanes without a contract, or with mode `off`, are never touched. A proposal is persisted
//! before it is dispatched, and an uncertain delivery is never resent.
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use serde::Serialize;
use sqlx::SqlitePool;

use super::contract::{self, Contract, Proposal};
use super::converge::{self, ConvergenceOutcome};
use super::evidence::{self, EvidenceBundle};
use super::types::{Decision, GoalError, GoalMode, ProposalState, RequirementStatus, StopReason, TurnOrigin};
use super::{assess, turns};
use crate::agent_control::AgentControl;
use crate::judge::Judge;
use crate::lanes::state::{self, LaneStatus, GOAL_CONVERGED_REASON};

/// Refusal code of a send a person's typing blocked. Nothing was typed; the proposal is stale.
pub const CONTROL_CONFLICT: &str = "CONTROL_CONFLICT";

/// Refusal code of a send the caller could not confirm (no in-process observer, e.g. the
/// standalone `ferryx-cli mcp`). Nothing was typed; the proposal goes back to awaiting approval.
pub const OBSERVER_UNAVAILABLE: &str = "OBSERVER_UNAVAILABLE";

/// Consecutive refused `complete` decisions that stop the lane.
pub const DIVERGENCE_STOP_AFTER: u32 = 2;

const OWNERSHIP_PREAMBLE: &str = "<ferryx-review-ownership>Ferryx's supervisor checks this turn against the lane goal. Do not declare goal requirements met; report what you changed and how to verify it.</ferryx-review-ownership>";

/// What one loop pass changed, for the `supervisor_changed` payload `{laneKey, stopReason}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoopOutcome {
    pub lane_key: String,
    pub stop_reason: Option<StopReason>,
    pub awaiting_approval: bool,
    /// One instruction was sent and the pane started a turn.
    pub sent: bool,
}

impl LoopOutcome {
    fn new(lane_key: &str) -> Self {
        Self {
            lane_key: lane_key.to_owned(),
            stop_reason: None,
            awaiting_approval: false,
            sent: false,
        }
    }
}

static LANE_LOCKS: LazyLock<std::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> =
    LazyLock::new(Default::default);

fn lane_lock(lane_key: &str) -> Arc<tokio::sync::Mutex<()>> {
    let mut locks = LANE_LOCKS.lock().unwrap_or_else(|poison| poison.into_inner());
    Arc::clone(locks.entry(lane_key.to_owned()).or_default())
}

fn hub_error(error: impl std::fmt::Display) -> GoalError {
    GoalError::Hub(crate::error::HubError::message(error.to_string()))
}

/// The lane's pane and status, or `None` for a lane that no longer exists.
async fn lane_row(pool: &SqlitePool, lane_key: &str) -> Result<Option<(Option<String>, LaneStatus)>, GoalError> {
    let row: Option<(Option<String>, String)> =
        sqlx::query_as("SELECT omo_thread_id, status FROM lanes WHERE key = ?")
            .bind(lane_key)
            .fetch_optional(pool)
            .await?;
    row.map(|(session_id, status)| {
        let status = status.parse::<LaneStatus>().map_err(hub_error)?;
        Ok((session_id.filter(|id| !id.is_empty()), status))
    })
    .transpose()
}

fn ended(status: LaneStatus) -> bool {
    matches!(status, LaneStatus::VerifiedDone | LaneStatus::Closed | LaneStatus::Failed)
}

/// Records the stop and blocks the lane. Every stop goes through here.
async fn stop(
    pool: &SqlitePool,
    lane_key: &str,
    reason: StopReason,
    detail: Option<&str>,
) -> Result<LoopOutcome, GoalError> {
    contract::record_stop(pool, lane_key, reason, detail).await?;
    match state::status_of(pool, lane_key).await.map_err(hub_error)? {
        Some(LaneStatus::Blocked) => {}
        Some(status) if status.can_transition_to(LaneStatus::Blocked, Some(reason.as_str())) => {
            state::transition(pool, lane_key, LaneStatus::Blocked, Some(reason.as_str()))
                .await
                .map_err(hub_error)?;
        }
        // A lane mid-settlement keeps its status; the recorded stop still holds the loop.
        status => tracing::info!(lane = %lane_key, ?status, "a stopped goal lane was not moved to blocked"),
    }
    tracing::info!(lane = %lane_key, reason = %reason, "supervisor goal loop stopped the lane");
    Ok(LoopOutcome {
        stop_reason: Some(reason),
        ..LoopOutcome::new(lane_key)
    })
}

/// True when a person spoke to the pane after the proposal was made.
async fn human_after(pool: &SqlitePool, lane_key: &str, proposal: &Proposal) -> Result<bool, GoalError> {
    match proposal.at.as_deref() {
        Some(at) => Ok(turns::latest_human_after(pool, lane_key, at).await?.is_some()),
        None => Ok(false),
    }
}

/// The text the pane receives: the ownership preamble, then the instruction.
pub fn wrap_instruction(instruction: &str) -> String {
    format!("{OWNERSHIP_PREAMBLE}\n\n{instruction}")
}

/// Sends the lane's persisted proposal once and records what happened. Shared by full mode
/// and by an owner approving a supervised proposal.
async fn dispatch<C: AgentControl + ?Sized>(
    pool: &SqlitePool,
    control: &C,
    lane_key: &str,
    session_id: &str,
) -> Result<LoopOutcome, GoalError> {
    let contract = contract::contract_of(pool, lane_key)
        .await?
        .ok_or_else(|| GoalError::NoContract(lane_key.to_owned()))?;
    let Some(proposal) = contract.proposal else {
        return Err(GoalError::InvalidContract(format!("lane {lane_key} has no proposal to send")));
    };
    if human_after(pool, lane_key, &proposal).await? {
        contract::set_proposal_state(pool, lane_key, ProposalState::Superseded, None).await?;
        return Ok(LoopOutcome::new(lane_key));
    }
    let request_id = proposal
        .request_id
        .clone()
        .unwrap_or_else(|| format!("{}{lane_key}-{}", turns::SUPERVISOR_REQUEST_PREFIX, contract.turns_used));
    let outcome = control
        .send_confirmed(None, session_id, &wrap_instruction(&proposal.text), &request_id)
        .await;
    match outcome {
        super::SendOutcome::Started => {
            contract::set_proposal_state(pool, lane_key, ProposalState::Sent, None).await?;
            turns::record_turn(pool, lane_key, TurnOrigin::SupervisorProposal, Some(&proposal.text), None).await?;
            Ok(LoopOutcome {
                sent: true,
                ..LoopOutcome::new(lane_key)
            })
        }
        super::SendOutcome::Refused { code } if code == CONTROL_CONFLICT => {
            contract::set_proposal_state(pool, lane_key, ProposalState::Superseded, None).await?;
            Ok(LoopOutcome::new(lane_key))
        }
        super::SendOutcome::Refused { code } if code == OBSERVER_UNAVAILABLE => {
            contract::set_proposal_state(
                pool,
                lane_key,
                ProposalState::AwaitingApproval,
                Some(&[ProposalState::Pending]),
            )
            .await?;
            Err(GoalError::InvalidContract(
                "approving a proposal requires the daemon observer".into(),
            ))
        }
        super::SendOutcome::Uncertain { code } | super::SendOutcome::Refused { code } => {
            contract::set_proposal_state(pool, lane_key, ProposalState::Uncertain, None).await?;
            stop(pool, lane_key, StopReason::DeliveryUncertain, Some(&code)).await
        }
    }
}

/// Counts the turn, persists the proposal, then waits for approval (supervised) or sends it
/// (full).
async fn propose<C: AgentControl + ?Sized>(
    pool: &SqlitePool,
    control: &C,
    contract: &Contract,
    session_id: &str,
    instruction: &str,
    gap_ids: &[String],
) -> Result<LoopOutcome, GoalError> {
    let lane_key = contract.lane_key.as_str();
    let turn = match contract::bump_turns(pool, lane_key).await {
        Ok(turn) => turn,
        Err(GoalError::TurnLimit { limit }) => {
            let detail = format!("{limit} of {limit} supervisor turns used");
            return stop(pool, lane_key, StopReason::TurnLimit, Some(&detail)).await;
        }
        Err(error) => return Err(error),
    };
    let request_id = format!("{}{lane_key}-{turn}", turns::SUPERVISOR_REQUEST_PREFIX);
    // A supervised proposal is written straight as awaiting approval: only `pending` means a
    // send may be in flight, so a crash here is never mistaken for an interrupted delivery.
    let initial = match contract.mode {
        GoalMode::Supervised => ProposalState::AwaitingApproval,
        GoalMode::Full | GoalMode::Off => ProposalState::Pending,
    };
    contract::set_proposal(pool, lane_key, instruction, gap_ids, initial, Some(&request_id)).await?;
    match contract.mode {
        GoalMode::Supervised => {
            Ok(LoopOutcome {
                awaiting_approval: true,
                ..LoopOutcome::new(lane_key)
            })
        }
        GoalMode::Full => dispatch(pool, control, lane_key, session_id).await,
        GoalMode::Off => Ok(LoopOutcome::new(lane_key)),
    }
}

/// Marks every requirement met with its verdict's evidence and finishes the lane.
async fn finish(
    pool: &SqlitePool,
    contract: &Contract,
    verdicts: &[converge::RequirementVerdict],
) -> Result<LoopOutcome, GoalError> {
    let lane_key = contract.lane_key.as_str();
    for verdict in verdicts {
        contract::update_requirement(
            pool,
            lane_key,
            &verdict.req_id,
            RequirementStatus::Met,
            &verdict.evidence_refs,
            contract.revision,
        )
        .await?;
    }
    contract::clear_stop(pool, lane_key).await?;
    contract::clear_proposal(pool, lane_key).await?;
    contract::reset_divergence(pool, lane_key).await?;
    state::transition(pool, lane_key, LaneStatus::VerifiedDone, Some(GOAL_CONVERGED_REASON))
        .await
        .map_err(hub_error)?;
    Ok(LoopOutcome::new(lane_key))
}

fn verification_instruction(unmet_ids: &[String]) -> String {
    format!(
        "The lane goal is not verified yet. Requirements still unproven: {}. For each one, run the check its verifier names, fix what fails, and report the exact commands you ran and their results.",
        unmet_ids.join(", ")
    )
}

async fn decide<C: AgentControl + ?Sized>(
    pool: &SqlitePool,
    judge: &Judge,
    control: &C,
    contract: &Contract,
    session_id: &str,
    bundle: &EvidenceBundle,
) -> Result<LoopOutcome, GoalError> {
    let lane_key = contract.lane_key.as_str();
    let recent_turns = turns::turns_of(pool, lane_key).await?;
    let assessment = match assess::assess(judge, contract, bundle, &recent_turns).await {
        Ok(assessment) => assessment,
        Err(GoalError::JudgeUnavailable) => {
            return stop(pool, lane_key, StopReason::JudgeUnavailable, None).await;
        }
        Err(error) => return Err(error),
    };
    match assessment.decision {
        Decision::Stop { reason } => {
            contract::reset_divergence(pool, lane_key).await?;
            let detail = assessment
                .gaps
                .iter()
                .map(|gap| gap.summary.as_str())
                .collect::<Vec<_>>()
                .join("; ");
            stop(pool, lane_key, reason, Some(detail.as_str()).filter(|text| !text.is_empty())).await
        }
        Decision::Continue { instruction, gap_ids, .. } => {
            contract::reset_divergence(pool, lane_key).await?;
            propose(pool, control, contract, session_id, &instruction, &gap_ids).await
        }
        Decision::Complete => match converge::converge(judge, contract, bundle).await {
            Err(GoalError::JudgeUnavailable) => stop(pool, lane_key, StopReason::JudgeUnavailable, None).await,
            Err(error) => Err(error),
            Ok(ConvergenceOutcome::Converged { verdicts }) => finish(pool, contract, &verdicts).await,
            Ok(ConvergenceOutcome::Diverged { unmet_ids, .. }) => {
                if contract::note_divergence(pool, lane_key).await? >= DIVERGENCE_STOP_AFTER {
                    let detail = format!("unproven: {}", unmet_ids.join(", "));
                    return stop(pool, lane_key, StopReason::CompletionEvidenceMissing, Some(&detail)).await;
                }
                propose(pool, control, contract, session_id, &verification_instruction(&unmet_ids), &unmet_ids).await
            }
        },
    }
}

/// One lane turn settled (the pane went Idle with no prompt). `worked` says whether a Working
/// reading was observed since the previous settle; a bare Idle while a sent proposal is still
/// in flight is ignored.
///
/// Returns `None` when nothing changed (no contract, mode off, a stopped or paused lane, an
/// awaiting proposal), else what changed, for `supervisor_changed`.
pub async fn on_turn_settled<C: AgentControl + ?Sized>(
    pool: &SqlitePool,
    judge: &Judge,
    control: &C,
    lane_key: &str,
    worked: bool,
) -> Result<Option<LoopOutcome>, GoalError> {
    let Some(contract) = contract::contract_of(pool, lane_key).await? else {
        return Ok(None);
    };
    if contract.mode == GoalMode::Off {
        return Ok(None);
    }
    let lock = lane_lock(lane_key);
    let _guard = lock.lock().await;
    // Re-read under the lock: a concurrent pass may have changed it.
    let Some(mut contract) = contract::contract_of(pool, lane_key).await? else {
        return Ok(None);
    };
    if contract.mode == GoalMode::Off || contract.stop_reason.is_some() {
        return Ok(None);
    }
    let Some((Some(session_id), status)) = lane_row(pool, lane_key).await? else {
        return Ok(None);
    };
    if ended(status) || crate::lanes::pause::is_paused(pool, lane_key).await? {
        return Ok(None);
    }
    let mut superseded = false;
    if let Some(proposal) = contract.proposal.clone() {
        match proposal.state {
            ProposalState::Sent if !worked => return Ok(None),
            ProposalState::Sent => contract::clear_proposal(pool, lane_key).await?,
            ProposalState::Uncertain => return Ok(None),
            ProposalState::Pending | ProposalState::AwaitingApproval => {
                if human_after(pool, lane_key, &proposal).await? {
                    contract::set_proposal_state(pool, lane_key, ProposalState::Superseded, None).await?;
                    superseded = true;
                } else if proposal.state == ProposalState::AwaitingApproval {
                    return Ok(None);
                } else {
                    // A pending proposal outside a pass means a pass died mid-dispatch: the
                    // bytes may have reached the pane. Never resend; hand the lane over.
                    contract::set_proposal_state(pool, lane_key, ProposalState::Uncertain, None).await?;
                    let detail = "interrupted before delivery was confirmed";
                    return stop(pool, lane_key, StopReason::DeliveryUncertain, Some(detail)).await.map(Some);
                }
            }
            ProposalState::Superseded | ProposalState::Discarded => {}
        }
        contract = contract::contract_of(pool, lane_key)
            .await?
            .ok_or_else(|| GoalError::NoContract(lane_key.to_owned()))?;
    }
    let bundle = evidence::collect(pool, control, lane_key).await?;
    let outcome = decide(pool, judge, control, &contract, &session_id, &bundle).await;
    if superseded {
        tracing::info!(lane = %lane_key, "a person spoke after the supervisor proposal; it was superseded");
    }
    outcome.map(Some)
}

/// The owner approved the lane's awaiting proposal: send it through the same path full mode
/// uses. The proposal is atomically claimed (`awaiting_approval` -> `pending`) before the
/// send, so a racing or repeated approval, even after a crash mid-send, is refused and the
/// proposal is never sent twice.
pub async fn approve_proposal<C: AgentControl + ?Sized>(
    pool: &SqlitePool,
    control: &C,
    lane_key: &str,
) -> Result<LoopOutcome, GoalError> {
    let lock = lane_lock(lane_key);
    let _guard = lock.lock().await;
    let contract = contract::contract_of(pool, lane_key)
        .await?
        .ok_or_else(|| GoalError::NoContract(lane_key.to_owned()))?;
    if contract.proposal.as_ref().map(|proposal| proposal.state) != Some(ProposalState::AwaitingApproval) {
        return Err(GoalError::InvalidContract(format!(
            "lane {lane_key} has no proposal awaiting approval"
        )));
    }
    let Some((Some(session_id), _)) = lane_row(pool, lane_key).await? else {
        return Err(GoalError::InvalidContract(format!("lane {lane_key} has no pane")));
    };
    if let Some(proposal) = contract.proposal.as_ref() {
        if human_after(pool, lane_key, proposal).await? {
            contract::set_proposal_state(
                pool,
                lane_key,
                ProposalState::Superseded,
                Some(&[ProposalState::AwaitingApproval]),
            )
            .await?;
            return Ok(LoopOutcome::new(lane_key));
        }
    }
    contract::claim_approval(pool, lane_key).await?;
    dispatch(pool, control, lane_key, &session_id).await
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use async_trait::async_trait;
    use serde_json::{json, Value};

    use super::*;
    use crate::agent_control::AgentOp;
    use crate::error::{HubError, Result};
    use crate::goals::contract::{set_contract, RequirementSpec};
    use crate::goals::turns::record_turn_at;
    use crate::goals::SendOutcome;
    use crate::lanes::kibitzer::ModelTurn;
    use crate::lanes::state::{insert_lane, set_pane, status_of, transition};

    const LANE: &str = "goal-a";

    struct ScriptedModel {
        answers: Mutex<VecDeque<String>>,
        prompts: Mutex<Vec<String>>,
    }

    impl ScriptedModel {
        fn new(answers: &[&str]) -> Arc<Self> {
            Arc::new(Self {
                answers: Mutex::new(answers.iter().map(|answer| (*answer).to_owned()).collect()),
                prompts: Mutex::new(Vec::new()),
            })
        }

        fn calls(&self) -> usize {
            self.prompts.lock().unwrap().len()
        }
    }

    #[async_trait]
    impl ModelTurn for ScriptedModel {
        async fn complete(&self, prompt: &str) -> Result<String> {
            self.prompts.lock().unwrap().push(prompt.to_owned());
            self.answers
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| HubError::message("no scripted answer"))
        }
    }

    /// Answers reads with an idle pane and counts confirmed sends, returning scripted outcomes.
    #[derive(Default)]
    struct FakeControl {
        outcomes: Mutex<VecDeque<SendOutcome>>,
        sends: Mutex<Vec<(String, String, String)>>,
    }

    impl FakeControl {
        fn scripted(outcomes: Vec<SendOutcome>) -> Self {
            Self {
                outcomes: Mutex::new(outcomes.into()),
                ..Self::default()
            }
        }

        fn sends(&self) -> Vec<(String, String, String)> {
            self.sends.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl AgentControl for FakeControl {
        async fn execute(&self, _host_id: Option<&str>, op: AgentOp) -> Result<Value> {
            match op {
                AgentOp::Read { .. } => Ok(json!({
                    "session": {
                        "state": "idle",
                        "stateSource": "provider",
                        "tier": "provider",
                        "screen": {"revision": "r1", "text": "$ cargo test\nok"}
                    }
                })),
                other => Err(HubError::message(format!("unexpected op {other:?}"))),
            }
        }

        async fn send_confirmed(
            &self,
            _host_id: Option<&str>,
            session_id: &str,
            text: &str,
            request_id: &str,
        ) -> SendOutcome {
            self.sends
                .lock()
                .unwrap()
                .push((session_id.to_owned(), text.to_owned(), request_id.to_owned()));
            self.outcomes.lock().unwrap().pop_front().unwrap_or(SendOutcome::Started)
        }
    }

    const CONTINUE: &str = r#"{"gaps":[{"id":"g1","kind":"work","requirement_ids":["r1"],"summary":"tests not run"}],"decision":{"decision":"continue","instruction":"run the tests","gap_ids":["g1"],"expected_change":"tests pass","verification":"sig:0"}}"#;
    const COMPLETE: &str = r#"{"gaps":[],"decision":{"decision":"complete"}}"#;
    const CONVERGED: &str = r#"{"requirements":[{"req_id":"r1","met":true,"evidence_refs":["git:clean"]}]}"#;
    const DIVERGED: &str = r#"{"requirements":[{"req_id":"r1","met":false,"evidence_refs":[]}]}"#;

    async fn lane_pool(mode: Option<GoalMode>) -> SqlitePool {
        let pool = crate::storage::memory().await.unwrap();
        let cwd = std::env::temp_dir().join(format!("ferryx-goal-loop-{}", std::process::id()));
        std::fs::create_dir_all(&cwd).unwrap();
        insert_lane(&pool, LANE, "Goal lane", "lead", &cwd.to_string_lossy(), "file marker")
            .await
            .unwrap();
        set_pane(&pool, LANE, "pane-1").await.unwrap();
        transition(&pool, LANE, LaneStatus::Working, Some("launched")).await.unwrap();
        if let Some(mode) = mode {
            let spec = RequirementSpec {
                id: "r1".into(),
                text: "the tests pass".into(),
                verifier: "cargo test".into(),
            };
            set_contract(&pool, LANE, &[spec], mode, 20).await.unwrap();
        }
        pool
    }

    fn judge(pool: &SqlitePool, model: &Arc<ScriptedModel>) -> Judge {
        Judge::bare().with_pool(pool.clone()).with_fallback(model.clone())
    }

    async fn goal(pool: &SqlitePool) -> Contract {
        contract::contract_of(pool, LANE).await.unwrap().unwrap()
    }

    async fn origins(pool: &SqlitePool) -> Vec<TurnOrigin> {
        turns::turns_of(pool, LANE)
            .await
            .unwrap()
            .into_iter()
            .map(|turn| turn.origin)
            .collect()
    }

    #[tokio::test]
    async fn a_supervised_continue_awaits_approval_and_sends_nothing() {
        let pool = lane_pool(Some(GoalMode::Supervised)).await;
        let model = ScriptedModel::new(&[CONTINUE]);
        let control = FakeControl::default();
        let outcome = on_turn_settled(&pool, &judge(&pool, &model), &control, LANE, true)
            .await
            .unwrap()
            .unwrap();
        assert!(outcome.awaiting_approval);
        assert_eq!(outcome.stop_reason, None);
        assert!(control.sends().is_empty());
        let proposal = goal(&pool).await.proposal.unwrap();
        assert_eq!(proposal.state, ProposalState::AwaitingApproval);
        assert_eq!(proposal.text, "run the tests");
        assert_eq!(proposal.request_id.as_deref(), Some("hub-goal-goal-a-1"));
        // Another settle while the proposal waits neither asks the judge nor sends.
        assert!(on_turn_settled(&pool, &judge(&pool, &model), &control, LANE, true)
            .await
            .unwrap()
            .is_none());
        assert_eq!(model.calls(), 1);
        assert!(control.sends().is_empty());
    }

    #[tokio::test]
    async fn a_full_continue_sends_once_and_records_a_supervisor_turn() {
        let pool = lane_pool(Some(GoalMode::Full)).await;
        let model = ScriptedModel::new(&[CONTINUE]);
        let control = FakeControl::default();
        let outcome = on_turn_settled(&pool, &judge(&pool, &model), &control, LANE, true)
            .await
            .unwrap()
            .unwrap();
        assert!(outcome.sent);
        let sends = control.sends();
        assert_eq!(sends.len(), 1);
        assert_eq!(sends[0].0, "pane-1");
        assert!(sends[0].1.starts_with("<ferryx-review-ownership>"));
        assert!(sends[0].1.ends_with("\n\nrun the tests"));
        assert_eq!(sends[0].2, "hub-goal-goal-a-1");
        let contract = goal(&pool).await;
        assert_eq!(contract.proposal.unwrap().state, ProposalState::Sent);
        assert_eq!(contract.turns_used, 1);
        assert_eq!(origins(&pool).await, [TurnOrigin::SupervisorProposal]);
        // An Idle with no Working since the send is the same turn: ignored.
        assert!(on_turn_settled(&pool, &judge(&pool, &model), &control, LANE, false)
            .await
            .unwrap()
            .is_none());
        assert_eq!(model.calls(), 1);
    }

    #[tokio::test]
    async fn an_uncertain_delivery_stops_the_lane_and_is_never_resent() {
        let pool = lane_pool(Some(GoalMode::Full)).await;
        let model = ScriptedModel::new(&[CONTINUE, CONTINUE]);
        let control = FakeControl::scripted(vec![SendOutcome::Uncertain {
            code: "TURN_NOT_OBSERVED".into(),
        }]);
        let outcome = on_turn_settled(&pool, &judge(&pool, &model), &control, LANE, true)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(outcome.stop_reason, Some(StopReason::DeliveryUncertain));
        let contract = goal(&pool).await;
        assert_eq!(contract.stop_reason, Some(StopReason::DeliveryUncertain));
        assert_eq!(contract.stop_detail.as_deref(), Some("TURN_NOT_OBSERVED"));
        assert_eq!(contract.proposal.unwrap().state, ProposalState::Uncertain);
        assert_eq!(status_of(&pool, LANE).await.unwrap(), Some(LaneStatus::Blocked));
        assert!(on_turn_settled(&pool, &judge(&pool, &model), &control, LANE, true)
            .await
            .unwrap()
            .is_none());
        assert_eq!(control.sends().len(), 1, "exactly one write");
        assert_eq!(model.calls(), 1);
        assert!(origins(&pool).await.is_empty());
    }

    #[tokio::test]
    async fn a_human_turn_after_the_proposal_supersedes_it() {
        let pool = lane_pool(Some(GoalMode::Supervised)).await;
        let model = ScriptedModel::new(&[CONTINUE]);
        let control = FakeControl::default();
        on_turn_settled(&pool, &judge(&pool, &model), &control, LANE, true)
            .await
            .unwrap();
        record_turn_at(
            &pool,
            LANE,
            TurnOrigin::HumanCorrection,
            Some("no, use the long flag"),
            None,
            "2099-01-01T00:00:00+00:00",
        )
        .await
        .unwrap();
        let outcome = approve_proposal(&pool, &control, LANE).await.unwrap();
        assert!(!outcome.sent);
        assert_eq!(goal(&pool).await.proposal.unwrap().state, ProposalState::Superseded);
        assert!(control.sends().is_empty());
        assert!(approve_proposal(&pool, &control, LANE).await.is_err());
        assert!(control.sends().is_empty());
    }

    /// Records a send, signals that it is in flight, then never finishes: an approval whose
    /// process dies while delivery is still unconfirmed.
    struct ParkedControl {
        entered: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
        sends: Mutex<usize>,
    }

    #[async_trait]
    impl AgentControl for ParkedControl {
        async fn execute(&self, _host_id: Option<&str>, op: AgentOp) -> Result<Value> {
            Err(HubError::message(format!("unexpected op {op:?}")))
        }

        async fn send_confirmed(
            &self,
            _host_id: Option<&str>,
            _session_id: &str,
            _text: &str,
            _request_id: &str,
        ) -> SendOutcome {
            *self.sends.lock().unwrap() += 1;
            let entered = self.entered.lock().unwrap().take();
            if let Some(entered) = entered {
                let _ = entered.send(());
            }
            std::future::pending().await
        }
    }

    #[tokio::test]
    async fn an_interrupted_approval_cannot_be_approved_twice_and_stops_uncertain_on_settle() {
        let pool = lane_pool(Some(GoalMode::Supervised)).await;
        let model = ScriptedModel::new(&[CONTINUE]);
        let control = FakeControl::default();
        let outcome = on_turn_settled(&pool, &judge(&pool, &model), &control, LANE, true)
            .await
            .unwrap()
            .unwrap();
        assert!(outcome.awaiting_approval);

        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let parked = Arc::new(ParkedControl {
            entered: Mutex::new(Some(entered_tx)),
            sends: Mutex::new(0),
        });
        let approving = {
            let pool = pool.clone();
            let parked = Arc::clone(&parked);
            tokio::spawn(async move { approve_proposal(&pool, parked.as_ref(), LANE).await })
        };
        entered_rx.await.unwrap();
        approving.abort();
        assert!(approving.await.unwrap_err().is_cancelled());
        assert_eq!(*parked.sends.lock().unwrap(), 1);
        assert_eq!(goal(&pool).await.proposal.unwrap().state, ProposalState::Pending);

        let again = approve_proposal(&pool, &control, LANE).await;
        assert!(matches!(again, Err(GoalError::InvalidContract(_))), "{again:?}");
        assert!(control.sends().is_empty());

        let settled = on_turn_settled(&pool, &judge(&pool, &model), &control, LANE, true)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(settled.stop_reason, Some(StopReason::DeliveryUncertain));
        let contract = goal(&pool).await;
        assert_eq!(contract.stop_reason, Some(StopReason::DeliveryUncertain));
        assert_eq!(contract.proposal.unwrap().state, ProposalState::Uncertain);
        assert!(control.sends().is_empty());
        assert_eq!(*parked.sends.lock().unwrap(), 1, "exactly one write");
        assert_eq!(model.calls(), 1);
    }

    #[tokio::test]
    async fn an_observer_unavailable_refusal_restores_awaiting_approval_without_stopping() {
        let pool = lane_pool(Some(GoalMode::Supervised)).await;
        contract::set_proposal(
            &pool,
            LANE,
            "run the tests",
            &["g1".to_owned()],
            ProposalState::AwaitingApproval,
            Some("hub-goal-goal-a-1"),
        )
        .await
        .unwrap();
        let control = FakeControl::scripted(vec![SendOutcome::Refused {
            code: OBSERVER_UNAVAILABLE.into(),
        }]);
        let result = approve_proposal(&pool, &control, LANE).await;
        assert!(matches!(result, Err(GoalError::InvalidContract(_))), "{result:?}");
        let contract = goal(&pool).await;
        assert_eq!(contract.proposal.unwrap().state, ProposalState::AwaitingApproval);
        assert_eq!(contract.stop_reason, None);
        assert_eq!(control.sends().len(), 1);
    }

    #[tokio::test]
    async fn a_person_typing_refuses_the_send_and_supersedes_the_proposal() {
        let pool = lane_pool(Some(GoalMode::Full)).await;
        let model = ScriptedModel::new(&[CONTINUE]);
        let control = FakeControl::scripted(vec![SendOutcome::Refused {
            code: CONTROL_CONFLICT.into(),
        }]);
        let outcome = on_turn_settled(&pool, &judge(&pool, &model), &control, LANE, true)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(outcome.stop_reason, None);
        assert!(!outcome.sent);
        let contract = goal(&pool).await;
        assert_eq!(contract.proposal.unwrap().state, ProposalState::Superseded);
        assert_eq!(contract.stop_reason, None);
        assert_eq!(status_of(&pool, LANE).await.unwrap(), Some(LaneStatus::Working));
    }

    #[tokio::test]
    async fn the_turn_after_the_limit_stops_the_lane() {
        let pool = lane_pool(Some(GoalMode::Full)).await;
        sqlx::query("UPDATE lane_goal SET turns_used = 20 WHERE lane_key = ?")
            .bind(LANE)
            .execute(&pool)
            .await
            .unwrap();
        let model = ScriptedModel::new(&[CONTINUE]);
        let control = FakeControl::default();
        let outcome = on_turn_settled(&pool, &judge(&pool, &model), &control, LANE, true)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(outcome.stop_reason, Some(StopReason::TurnLimit));
        let contract = goal(&pool).await;
        assert_eq!(contract.stop_reason, Some(StopReason::TurnLimit));
        assert_eq!(contract.turns_used, 20);
        assert_eq!(contract.proposal, None);
        assert_eq!(status_of(&pool, LANE).await.unwrap(), Some(LaneStatus::Blocked));
        assert!(control.sends().is_empty());
    }

    #[tokio::test]
    async fn a_converged_completion_verifies_the_lane() {
        let pool = lane_pool(Some(GoalMode::Full)).await;
        let model = ScriptedModel::new(&[COMPLETE, CONVERGED]);
        let control = FakeControl::default();
        let outcome = on_turn_settled(&pool, &judge(&pool, &model), &control, LANE, true)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(outcome.stop_reason, None);
        assert_eq!(status_of(&pool, LANE).await.unwrap(), Some(LaneStatus::VerifiedDone));
        let contract = goal(&pool).await;
        assert_eq!(contract.requirements[0].status, RequirementStatus::Met);
        assert_eq!(contract.requirements[0].evidence, ["git:clean"]);
        assert!(contract::all_met(&contract));
        assert!(control.sends().is_empty());
        assert_eq!(model.calls(), 2);
    }

    #[tokio::test]
    async fn two_refused_completions_in_a_row_stop_for_missing_evidence() {
        let pool = lane_pool(Some(GoalMode::Full)).await;
        let model = ScriptedModel::new(&[COMPLETE, DIVERGED, COMPLETE, DIVERGED]);
        let control = FakeControl::default();
        let first = on_turn_settled(&pool, &judge(&pool, &model), &control, LANE, true)
            .await
            .unwrap()
            .unwrap();
        assert!(first.sent, "the first refusal asks for verification");
        let sends = control.sends();
        assert_eq!(sends.len(), 1);
        assert!(sends[0].1.contains("Requirements still unproven: r1"));
        let second = on_turn_settled(&pool, &judge(&pool, &model), &control, LANE, true)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(second.stop_reason, Some(StopReason::CompletionEvidenceMissing));
        assert_eq!(goal(&pool).await.stop_reason, Some(StopReason::CompletionEvidenceMissing));
        assert_eq!(status_of(&pool, LANE).await.unwrap(), Some(LaneStatus::Blocked));
        assert_eq!(control.sends().len(), 1);
    }

    #[tokio::test]
    async fn a_lane_without_a_contract_is_never_assessed() {
        let pool = lane_pool(None).await;
        let model = ScriptedModel::new(&[CONTINUE]);
        let control = FakeControl::default();
        assert!(on_turn_settled(&pool, &judge(&pool, &model), &control, LANE, true)
            .await
            .unwrap()
            .is_none());
        assert_eq!(model.calls(), 0);
        assert!(control.sends().is_empty());
        assert_eq!(status_of(&pool, LANE).await.unwrap(), Some(LaneStatus::Working));

        let off = lane_pool(Some(GoalMode::Off)).await;
        assert!(on_turn_settled(&off, &judge(&off, &model), &control, LANE, true)
            .await
            .unwrap()
            .is_none());
        assert_eq!(model.calls(), 0);
    }

    #[tokio::test]
    async fn an_unavailable_judge_stops_the_lane() {
        let pool = lane_pool(Some(GoalMode::Full)).await;
        let control = FakeControl::default();
        let outcome = on_turn_settled(&pool, &Judge::bare(), &control, LANE, true)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(outcome.stop_reason, Some(StopReason::JudgeUnavailable));
        assert_eq!(status_of(&pool, LANE).await.unwrap(), Some(LaneStatus::Blocked));
        assert!(control.sends().is_empty());
    }
}
