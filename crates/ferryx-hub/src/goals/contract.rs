//! Storage of a lane's goal contract. Completion is computed from these rows, never from an
//! agent's claim: a requirement becomes `met` only with a Ferryx-attested evidence ref.
//!
//! `revision` counts changes to the requirement set. Status updates compare it (so a status
//! written against an older requirement set is refused) but do not bump it.
use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use super::evidence::is_attested_ref;
use super::types::{GoalError, GoalMode, ProposalState, RequirementStatus, StopReason};
use crate::storage::WriteTx;
use crate::storage::now_rfc3339;

/// A requirement as the owner writes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequirementSpec {
    pub id: String,
    pub text: String,
    pub verifier: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Requirement {
    pub id: String,
    pub text: String,
    pub verifier: String,
    pub status: RequirementStatus,
    /// Evidence ref ids, e.g. `sig:0`, `git:clean`, `screen:<rev>`.
    pub evidence: Vec<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proposal {
    pub text: String,
    pub gap_ids: Vec<String>,
    pub state: ProposalState,
    pub request_id: Option<String>,
    pub at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contract {
    pub lane_key: String,
    pub mode: GoalMode,
    pub turn_limit: u32,
    pub turns_used: u32,
    pub revision: i64,
    pub stop_reason: Option<StopReason>,
    pub stop_detail: Option<String>,
    pub proposal: Option<Proposal>,
    pub requirements: Vec<Requirement>,
    pub updated_at: String,
}

/// True when every requirement is met or waived, and there is at least one.
pub fn all_met(contract: &Contract) -> bool {
    !contract.requirements.is_empty()
        && contract.requirements.iter().all(|requirement| {
            matches!(
                requirement.status,
                RequirementStatus::Met | RequirementStatus::Waived
            )
        })
}

fn validate(requirements: &[RequirementSpec], turn_limit: u32) -> Result<(), GoalError> {
    if requirements.is_empty() {
        return Err(GoalError::EmptyContract);
    }
    if turn_limit == 0 {
        return Err(GoalError::InvalidContract(
            "turn limit must be at least 1".into(),
        ));
    }
    let mut seen = HashSet::new();
    for requirement in requirements {
        if requirement.id.trim().is_empty() || requirement.text.trim().is_empty() {
            return Err(GoalError::InvalidContract(
                "every requirement needs an id and text".into(),
            ));
        }
        if requirement.verifier.trim().is_empty() {
            return Err(GoalError::InvalidContract(format!(
                "requirement {} needs a verifier",
                requirement.id
            )));
        }
        if !seen.insert(requirement.id.as_str()) {
            return Err(GoalError::InvalidContract(format!(
                "duplicate requirement id {}",
                requirement.id
            )));
        }
    }
    Ok(())
}

/// Creates a lane's contract, or replaces it when no supervisor turn is in flight (the
/// proposal is not `sent`). A replacement bumps the revision, resets every requirement to
/// pending, clears the stop, discards a proposal that was not sent and forgets an uncertain
/// one. Turns used carry over.
pub async fn set_contract(
    pool: &SqlitePool,
    lane_key: &str,
    requirements: &[RequirementSpec],
    mode: GoalMode,
    turn_limit: u32,
) -> Result<Contract, GoalError> {
    validate(requirements, turn_limit)?;
    let now = now_rfc3339();
    let mut tx = pool.begin_write().await?;
    let written = sqlx::query(
        "INSERT INTO lane_goal(lane_key, mode, turn_limit, updated_at) VALUES(?,?,?,?)
         ON CONFLICT(lane_key) DO UPDATE SET
            mode = excluded.mode,
            turn_limit = excluded.turn_limit,
            revision = lane_goal.revision + 1,
            stop_reason = NULL,
            stop_detail = NULL,
            converge_misses = 0,
            proposal_text = CASE WHEN lane_goal.proposal_state = 'uncertain'
                THEN NULL ELSE lane_goal.proposal_text END,
            proposal_gap_ids = CASE WHEN lane_goal.proposal_state = 'uncertain'
                THEN NULL ELSE lane_goal.proposal_gap_ids END,
            proposal_request_id = CASE WHEN lane_goal.proposal_state = 'uncertain'
                THEN NULL ELSE lane_goal.proposal_request_id END,
            proposal_at = CASE WHEN lane_goal.proposal_state = 'uncertain'
                THEN NULL ELSE lane_goal.proposal_at END,
            proposal_state = CASE
                WHEN lane_goal.proposal_state IN ('pending','awaiting_approval') THEN 'discarded'
                WHEN lane_goal.proposal_state = 'uncertain' THEN NULL
                ELSE lane_goal.proposal_state END,
            updated_at = excluded.updated_at
         WHERE lane_goal.proposal_state IS NULL OR lane_goal.proposal_state <> 'sent'",
    )
    .bind(lane_key)
    .bind(mode.as_str())
    .bind(i64::from(turn_limit))
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    if written.rows_affected() == 0 {
        return Err(GoalError::TurnInFlight(lane_key.to_owned()));
    }
    sqlx::query("DELETE FROM lane_goal_requirements WHERE lane_key = ?")
        .bind(lane_key)
        .execute(&mut *tx)
        .await?;
    for requirement in requirements {
        sqlx::query(
            "INSERT INTO lane_goal_requirements(lane_key, req_id, text, verifier, status, evidence_json, updated_at)
             VALUES(?,?,?,?,'pending','[]',?)",
        )
        .bind(lane_key)
        .bind(requirement.id.trim())
        .bind(&requirement.text)
        .bind(&requirement.verifier)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    contract_of(pool, lane_key)
        .await?
        .ok_or_else(|| GoalError::NoContract(lane_key.to_owned()))
}

type GoalRow = (
    String,
    i64,
    i64,
    i64,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
);

pub async fn contract_of(pool: &SqlitePool, lane_key: &str) -> Result<Option<Contract>, GoalError> {
    let row: Option<GoalRow> = sqlx::query_as(
        "SELECT mode, turn_limit, turns_used, revision, stop_reason, stop_detail, proposal_text,
                proposal_gap_ids, proposal_state, proposal_request_id, proposal_at, updated_at
         FROM lane_goal WHERE lane_key = ?",
    )
    .bind(lane_key)
    .fetch_optional(pool)
    .await?;
    let Some((
        mode,
        turn_limit,
        turns_used,
        revision,
        stop_reason,
        stop_detail,
        proposal_text,
        proposal_gap_ids,
        proposal_state,
        proposal_request_id,
        proposal_at,
        updated_at,
    )) = row
    else {
        return Ok(None);
    };
    let rows: Vec<(String, String, String, String, String, String)> = sqlx::query_as(
        "SELECT req_id, text, verifier, status, evidence_json, updated_at
         FROM lane_goal_requirements WHERE lane_key = ? ORDER BY rowid",
    )
    .bind(lane_key)
    .fetch_all(pool)
    .await?;
    let mut requirements = Vec::with_capacity(rows.len());
    for (id, text, verifier, status, evidence_json, updated_at) in rows {
        requirements.push(Requirement {
            id,
            text,
            verifier,
            status: status.parse::<RequirementStatus>()?,
            evidence: serde_json::from_str(&evidence_json).map_err(|error| {
                GoalError::InvalidContract(format!("stored evidence for {lane_key}: {error}"))
            })?,
            updated_at,
        });
    }
    let proposal = match proposal_state {
        Some(state) => Some(Proposal {
            text: proposal_text.unwrap_or_default(),
            gap_ids: match proposal_gap_ids.as_deref() {
                Some(text) => serde_json::from_str(text).map_err(|error| {
                    GoalError::InvalidContract(format!("stored gap ids for {lane_key}: {error}"))
                })?,
                None => Vec::new(),
            },
            state: state.parse::<ProposalState>()?,
            request_id: proposal_request_id,
            at: proposal_at,
        }),
        None => None,
    };
    Ok(Some(Contract {
        lane_key: lane_key.to_owned(),
        mode: mode.parse::<GoalMode>()?,
        turn_limit: u32::try_from(turn_limit).unwrap_or(0),
        turns_used: u32::try_from(turns_used).unwrap_or(u32::MAX),
        revision,
        stop_reason: stop_reason
            .as_deref()
            .map(str::parse::<StopReason>)
            .transpose()?,
        stop_detail,
        proposal,
        requirements,
        updated_at,
    }))
}

async fn revision_of(pool: &SqlitePool, lane_key: &str) -> Result<Option<i64>, GoalError> {
    Ok(
        sqlx::query_scalar("SELECT revision FROM lane_goal WHERE lane_key = ?")
            .bind(lane_key)
            .fetch_optional(pool)
            .await?,
    )
}

/// Sets one requirement's status and evidence when the contract is still at
/// `expected_revision`. `met` needs at least one Ferryx-attested ref; an agent claim alone
/// never satisfies a requirement.
pub async fn update_requirement(
    pool: &SqlitePool,
    lane_key: &str,
    req_id: &str,
    status: RequirementStatus,
    evidence: &[String],
    expected_revision: i64,
) -> Result<(), GoalError> {
    if status == RequirementStatus::Met && !evidence.iter().any(|item| is_attested_ref(item)) {
        return Err(GoalError::EvidenceRequired {
            req_id: req_id.to_owned(),
        });
    }
    let evidence_json = serde_json::to_string(evidence)
        .map_err(|error| GoalError::InvalidContract(error.to_string()))?;
    let updated = sqlx::query(
        "UPDATE lane_goal_requirements SET status = ?, evidence_json = ?, updated_at = ?
         WHERE lane_key = ? AND req_id = ?
           AND EXISTS(SELECT 1 FROM lane_goal WHERE lane_key = ? AND revision = ?)",
    )
    .bind(status.as_str())
    .bind(evidence_json)
    .bind(now_rfc3339())
    .bind(lane_key)
    .bind(req_id)
    .bind(lane_key)
    .bind(expected_revision)
    .execute(pool)
    .await?;
    if updated.rows_affected() == 1 {
        return Ok(());
    }
    match revision_of(pool, lane_key).await? {
        None => Err(GoalError::NoContract(lane_key.to_owned())),
        Some(actual) if actual != expected_revision => Err(GoalError::RevisionChanged {
            expected: expected_revision,
            actual,
        }),
        Some(_) => Err(GoalError::UnknownRequirement {
            lane_key: lane_key.to_owned(),
            req_id: req_id.to_owned(),
        }),
    }
}

async fn require_written(pool: &SqlitePool, lane_key: &str, rows: u64) -> Result<(), GoalError> {
    if rows == 0 && revision_of(pool, lane_key).await?.is_none() {
        return Err(GoalError::NoContract(lane_key.to_owned()));
    }
    Ok(())
}

pub async fn record_stop(
    pool: &SqlitePool,
    lane_key: &str,
    reason: StopReason,
    detail: Option<&str>,
) -> Result<(), GoalError> {
    let updated = sqlx::query(
        "UPDATE lane_goal SET stop_reason = ?, stop_detail = ?, updated_at = ? WHERE lane_key = ?",
    )
    .bind(reason.as_str())
    .bind(detail)
    .bind(now_rfc3339())
    .bind(lane_key)
    .execute(pool)
    .await?;
    require_written(pool, lane_key, updated.rows_affected()).await
}

pub async fn clear_stop(pool: &SqlitePool, lane_key: &str) -> Result<(), GoalError> {
    let updated = sqlx::query(
        "UPDATE lane_goal SET stop_reason = NULL, stop_detail = NULL, updated_at = ? WHERE lane_key = ?",
    )
    .bind(now_rfc3339())
    .bind(lane_key)
    .execute(pool)
    .await?;
    require_written(pool, lane_key, updated.rows_affected()).await
}

/// Persists the lane's one proposal, replacing any earlier one. The loop writes it before
/// dispatch, so a restart never loses track of what was (or may have been) sent.
pub async fn set_proposal(
    pool: &SqlitePool,
    lane_key: &str,
    text: &str,
    gap_ids: &[String],
    state: ProposalState,
    request_id: Option<&str>,
) -> Result<(), GoalError> {
    let gap_json = serde_json::to_string(gap_ids)
        .map_err(|error| GoalError::InvalidContract(error.to_string()))?;
    let now = now_rfc3339();
    let updated = sqlx::query(
        "UPDATE lane_goal SET proposal_text = ?, proposal_gap_ids = ?, proposal_state = ?,
                proposal_request_id = ?, proposal_at = ?, updated_at = ?
         WHERE lane_key = ?",
    )
    .bind(text)
    .bind(gap_json)
    .bind(state.as_str())
    .bind(request_id)
    .bind(&now)
    .bind(&now)
    .bind(lane_key)
    .execute(pool)
    .await?;
    require_written(pool, lane_key, updated.rows_affected()).await
}

/// Atomically claims the lane's awaiting proposal for sending: moves it from
/// `awaiting_approval` to `pending` in one statement. At most one approver (across processes)
/// wins; a proposal left `pending` by a crash mid-send is never approved again, and the next
/// settle stops the lane as `delivery_uncertain` instead of resending.
pub async fn claim_approval(pool: &SqlitePool, lane_key: &str) -> Result<(), GoalError> {
    let updated = sqlx::query(
        "UPDATE lane_goal SET proposal_state = 'pending', updated_at = ?
         WHERE lane_key = ? AND proposal_state = 'awaiting_approval'",
    )
    .bind(now_rfc3339())
    .bind(lane_key)
    .execute(pool)
    .await?;
    require_written(pool, lane_key, updated.rows_affected()).await?;
    if updated.rows_affected() == 1 {
        Ok(())
    } else {
        Err(GoalError::InvalidContract(format!(
            "lane {lane_key} has no proposal awaiting approval"
        )))
    }
}

/// Moves the proposal to `state`. With `from`, only a proposal currently in one of those
/// states moves. Returns whether it moved.
pub async fn set_proposal_state(
    pool: &SqlitePool,
    lane_key: &str,
    state: ProposalState,
    from: Option<&[ProposalState]>,
) -> Result<bool, GoalError> {
    let mut sql = String::from(
        "UPDATE lane_goal SET proposal_state = ?, updated_at = ? WHERE lane_key = ? AND proposal_state IS NOT NULL",
    );
    if let Some(from) = from {
        if from.is_empty() {
            return Ok(false);
        }
        let placeholders = from.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        sql.push_str(&format!(" AND proposal_state IN ({placeholders})"));
    }
    let mut query = sqlx::query(&sql)
        .bind(state.as_str())
        .bind(now_rfc3339())
        .bind(lane_key);
    for prior in from.unwrap_or_default() {
        query = query.bind(prior.as_str());
    }
    let updated = query.execute(pool).await?;
    require_written(pool, lane_key, updated.rows_affected()).await?;
    Ok(updated.rows_affected() == 1)
}

/// Changes how the loop treats the lane's proposals. The requirement set is untouched.
pub async fn set_mode(
    pool: &SqlitePool,
    lane_key: &str,
    mode: GoalMode,
) -> Result<Contract, GoalError> {
    let updated = sqlx::query("UPDATE lane_goal SET mode = ?, updated_at = ? WHERE lane_key = ?")
        .bind(mode.as_str())
        .bind(now_rfc3339())
        .bind(lane_key)
        .execute(pool)
        .await?;
    if updated.rows_affected() == 0 {
        return Err(GoalError::NoContract(lane_key.to_owned()));
    }
    contract_of(pool, lane_key)
        .await?
        .ok_or_else(|| GoalError::NoContract(lane_key.to_owned()))
}

/// Counts one more supervisor turn. Refuses once `turn_limit` turns were used.
pub async fn bump_turns(pool: &SqlitePool, lane_key: &str) -> Result<u32, GoalError> {
    let used: Option<i64> = sqlx::query_scalar(
        "UPDATE lane_goal SET turns_used = turns_used + 1, updated_at = ?
         WHERE lane_key = ? AND turns_used < turn_limit RETURNING turns_used",
    )
    .bind(now_rfc3339())
    .bind(lane_key)
    .fetch_optional(pool)
    .await?;
    if let Some(used) = used {
        return Ok(u32::try_from(used).unwrap_or(u32::MAX));
    }
    let limit: Option<i64> =
        sqlx::query_scalar("SELECT turn_limit FROM lane_goal WHERE lane_key = ?")
            .bind(lane_key)
            .fetch_optional(pool)
            .await?;
    match limit {
        Some(limit) => Err(GoalError::TurnLimit {
            limit: u32::try_from(limit).unwrap_or(0),
        }),
        None => Err(GoalError::NoContract(lane_key.to_owned())),
    }
}

/// Forgets the lane's proposal: its turn settled, or the goal converged.
pub async fn clear_proposal(pool: &SqlitePool, lane_key: &str) -> Result<(), GoalError> {
    let updated = sqlx::query(
        "UPDATE lane_goal SET proposal_text = NULL, proposal_gap_ids = NULL, proposal_state = NULL,
                proposal_request_id = NULL, proposal_at = NULL, updated_at = ?
         WHERE lane_key = ?",
    )
    .bind(now_rfc3339())
    .bind(lane_key)
    .execute(pool)
    .await?;
    require_written(pool, lane_key, updated.rows_affected()).await
}

/// Counts one more `complete` the convergence check refused and returns the streak.
pub async fn note_divergence(pool: &SqlitePool, lane_key: &str) -> Result<u32, GoalError> {
    let misses: Option<i64> = sqlx::query_scalar(
        "UPDATE lane_goal SET converge_misses = converge_misses + 1, updated_at = ?
         WHERE lane_key = ? RETURNING converge_misses",
    )
    .bind(now_rfc3339())
    .bind(lane_key)
    .fetch_optional(pool)
    .await?;
    misses
        .map(|misses| u32::try_from(misses).unwrap_or(u32::MAX))
        .ok_or_else(|| GoalError::NoContract(lane_key.to_owned()))
}

/// Ends a refused-`complete` streak: the lane made any other decision.
pub async fn reset_divergence(pool: &SqlitePool, lane_key: &str) -> Result<(), GoalError> {
    sqlx::query(
        "UPDATE lane_goal SET converge_misses = 0 WHERE lane_key = ? AND converge_misses <> 0",
    )
    .bind(lane_key)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lanes::state::insert_lane;

    fn spec(id: &str) -> RequirementSpec {
        RequirementSpec {
            id: id.into(),
            text: format!("requirement {id}"),
            verifier: format!("check {id}"),
        }
    }

    async fn lane_pool() -> SqlitePool {
        let pool = crate::storage::memory().await.unwrap();
        insert_lane(&pool, "lane-a", "A", "lead", "/tmp", "file marker")
            .await
            .unwrap();
        pool
    }

    #[tokio::test]
    async fn set_mode_changes_only_the_mode() {
        let pool = lane_pool().await;
        set_contract(&pool, "lane-a", &[spec("r1")], GoalMode::Supervised, 7)
            .await
            .unwrap();
        let changed = set_mode(&pool, "lane-a", GoalMode::Full).await.unwrap();
        assert_eq!(changed.mode, GoalMode::Full);
        assert_eq!(changed.revision, 1);
        assert!(matches!(
            set_mode(&pool, "missing", GoalMode::Off).await,
            Err(GoalError::NoContract(_))
        ));
    }

    #[tokio::test]
    async fn a_contract_round_trips_through_storage() {
        let pool = lane_pool().await;
        let written = set_contract(
            &pool,
            "lane-a",
            &[spec("r1"), spec("r2")],
            GoalMode::Full,
            7,
        )
        .await
        .unwrap();
        let read = contract_of(&pool, "lane-a").await.unwrap().unwrap();
        assert_eq!(written, read);
        assert_eq!(read.mode, GoalMode::Full);
        assert_eq!(read.turn_limit, 7);
        assert_eq!(read.turns_used, 0);
        assert_eq!(read.revision, 1);
        assert_eq!(read.stop_reason, None);
        assert_eq!(read.proposal, None);
        let ids: Vec<&str> = read
            .requirements
            .iter()
            .map(|item| item.id.as_str())
            .collect();
        assert_eq!(ids, ["r1", "r2"]);
        assert!(read
            .requirements
            .iter()
            .all(|item| item.status == RequirementStatus::Pending && item.evidence.is_empty()));
        assert_eq!(read.requirements[1].verifier, "check r2");
        assert!(contract_of(&pool, "missing").await.unwrap().is_none());

        record_stop(&pool, "lane-a", StopReason::TurnLimit, Some("20 of 20"))
            .await
            .unwrap();
        set_proposal(
            &pool,
            "lane-a",
            "run the tests",
            &["g1".to_owned()],
            ProposalState::AwaitingApproval,
            Some("hub-goal-lane-a-1"),
        )
        .await
        .unwrap();
        let read = contract_of(&pool, "lane-a").await.unwrap().unwrap();
        assert_eq!(read.stop_reason, Some(StopReason::TurnLimit));
        assert_eq!(read.stop_detail.as_deref(), Some("20 of 20"));
        let proposal = read.proposal.unwrap();
        assert_eq!(proposal.state, ProposalState::AwaitingApproval);
        assert_eq!(proposal.gap_ids, ["g1"]);
        assert!(proposal.at.is_some());
        clear_stop(&pool, "lane-a").await.unwrap();
        assert_eq!(
            contract_of(&pool, "lane-a")
                .await
                .unwrap()
                .unwrap()
                .stop_reason,
            None
        );
    }

    #[tokio::test]
    async fn an_empty_contract_is_refused() {
        let pool = lane_pool().await;
        let error = set_contract(&pool, "lane-a", &[], GoalMode::Supervised, 20)
            .await
            .unwrap_err();
        assert!(matches!(error, GoalError::EmptyContract));
        assert!(contract_of(&pool, "lane-a").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn a_stale_revision_is_refused_and_the_row_is_unchanged() {
        let pool = lane_pool().await;
        set_contract(&pool, "lane-a", &[spec("r1")], GoalMode::Supervised, 20)
            .await
            .unwrap();
        let replaced = set_contract(
            &pool,
            "lane-a",
            &[spec("r1"), spec("r2")],
            GoalMode::Supervised,
            20,
        )
        .await
        .unwrap();
        assert_eq!(replaced.revision, 2);
        let error = update_requirement(
            &pool,
            "lane-a",
            "r1",
            RequirementStatus::Met,
            &["sig:0".to_owned()],
            1,
        )
        .await
        .unwrap_err();
        assert!(matches!(
            error,
            GoalError::RevisionChanged {
                expected: 1,
                actual: 2
            }
        ));
        let read = contract_of(&pool, "lane-a").await.unwrap().unwrap();
        assert_eq!(read.requirements[0].status, RequirementStatus::Pending);
        assert!(read.requirements[0].evidence.is_empty());
        assert!(matches!(
            update_requirement(&pool, "lane-a", "nope", RequirementStatus::Unmet, &[], 2).await,
            Err(GoalError::UnknownRequirement { .. })
        ));
    }

    #[tokio::test]
    async fn the_turn_limit_refuses_the_turn_after_the_limit() {
        let pool = lane_pool().await;
        set_contract(&pool, "lane-a", &[spec("r1")], GoalMode::Full, 2)
            .await
            .unwrap();
        assert_eq!(bump_turns(&pool, "lane-a").await.unwrap(), 1);
        assert_eq!(bump_turns(&pool, "lane-a").await.unwrap(), 2);
        assert!(matches!(
            bump_turns(&pool, "lane-a").await,
            Err(GoalError::TurnLimit { limit: 2 })
        ));
        assert_eq!(
            contract_of(&pool, "lane-a")
                .await
                .unwrap()
                .unwrap()
                .turns_used,
            2
        );
        assert!(matches!(
            bump_turns(&pool, "missing").await,
            Err(GoalError::NoContract(_))
        ));
    }

    #[tokio::test]
    async fn the_mode_check_rejects_an_unknown_mode() {
        let pool = lane_pool().await;
        set_contract(&pool, "lane-a", &[spec("r1")], GoalMode::Supervised, 20)
            .await
            .unwrap();
        let refused = sqlx::query("UPDATE lane_goal SET mode = 'auto' WHERE lane_key = 'lane-a'")
            .execute(&pool)
            .await;
        assert!(refused.is_err());
        assert!("auto".parse::<GoalMode>().is_err());
        assert_eq!(
            contract_of(&pool, "lane-a").await.unwrap().unwrap().mode,
            GoalMode::Supervised
        );
    }

    #[tokio::test]
    async fn all_met_needs_every_requirement_met_or_waived() {
        let pool = lane_pool().await;
        let contract = set_contract(
            &pool,
            "lane-a",
            &[spec("r1"), spec("r2")],
            GoalMode::Supervised,
            20,
        )
        .await
        .unwrap();
        let revision = contract.revision;
        update_requirement(
            &pool,
            "lane-a",
            "r1",
            RequirementStatus::Met,
            &["sig:0".to_owned()],
            revision,
        )
        .await
        .unwrap();
        let partial = contract_of(&pool, "lane-a").await.unwrap().unwrap();
        assert!(!all_met(&partial), "r2 is still pending");
        update_requirement(
            &pool,
            "lane-a",
            "r2",
            RequirementStatus::Met,
            &["git:clean".to_owned()],
            revision,
        )
        .await
        .unwrap();
        let done = contract_of(&pool, "lane-a").await.unwrap().unwrap();
        assert!(all_met(&done));
        assert_eq!(done.requirements[0].evidence, ["sig:0"]);
        let mut waived = done.clone();
        waived.requirements[1].status = RequirementStatus::Waived;
        assert!(all_met(&waived));
        let mut empty = done;
        empty.requirements.clear();
        assert!(!all_met(&empty));
    }

    #[tokio::test]
    async fn a_sent_proposal_blocks_replacing_the_contract() {
        let pool = lane_pool().await;
        set_contract(&pool, "lane-a", &[spec("r1")], GoalMode::Full, 20)
            .await
            .unwrap();
        set_proposal(
            &pool,
            "lane-a",
            "do it",
            &["g1".to_owned()],
            ProposalState::Sent,
            Some("hub-goal-lane-a-1"),
        )
        .await
        .unwrap();
        assert!(matches!(
            set_contract(&pool, "lane-a", &[spec("r2")], GoalMode::Full, 20).await,
            Err(GoalError::TurnInFlight(_))
        ));
        assert!(!set_proposal_state(
            &pool,
            "lane-a",
            ProposalState::Superseded,
            Some(&[ProposalState::Pending, ProposalState::AwaitingApproval][..]),
        )
        .await
        .unwrap());
        assert!(
            set_proposal_state(&pool, "lane-a", ProposalState::Discarded, None)
                .await
                .unwrap()
        );
        let replaced = set_contract(&pool, "lane-a", &[spec("r2")], GoalMode::Full, 20)
            .await
            .unwrap();
        assert_eq!(replaced.revision, 2);
        assert_eq!(replaced.requirements[0].id, "r2");
    }

    #[tokio::test]
    async fn set_contract_clears_an_uncertain_proposal_and_stop_reason() {
        let pool = lane_pool().await;
        set_contract(&pool, "lane-a", &[spec("r1")], GoalMode::Full, 20)
            .await
            .unwrap();
        set_proposal(
            &pool,
            "lane-a",
            "do it",
            &["g1".to_owned()],
            ProposalState::Pending,
            Some("hub-goal-lane-a-1"),
        )
        .await
        .unwrap();
        assert!(
            set_proposal_state(&pool, "lane-a", ProposalState::Uncertain, None)
                .await
                .unwrap()
        );
        record_stop(
            &pool,
            "lane-a",
            StopReason::DeliveryUncertain,
            Some("SEND_TIMEOUT"),
        )
        .await
        .unwrap();
        let replaced = set_contract(&pool, "lane-a", &[spec("r2")], GoalMode::Full, 20)
            .await
            .unwrap();
        assert_eq!(replaced.revision, 2);
        assert_eq!(replaced.stop_reason, None);
        assert_eq!(replaced.proposal, None);
        let read = contract_of(&pool, "lane-a").await.unwrap().unwrap();
        assert_eq!(read.stop_reason, None);
        assert_eq!(read.proposal, None);
    }
}
