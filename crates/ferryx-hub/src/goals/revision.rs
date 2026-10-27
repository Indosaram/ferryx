//! Criteria revision guard: only the owner's own words may change a lane goal. A revision
//! is proposed with an exact quote of owner-authored text (the lane title, the external
//! session goal, an owner reply, or a human_correction turn), and only an explicit
//! `accept` applies it. Supervisor turns and agent output never count as a source.
use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use super::contract::{contract_of, Contract, RequirementSpec};
use super::types::GoalError;
use crate::storage::WriteTx;
use crate::storage::now_rfc3339;

/// Shortest trimmed quote a revision may cite.
pub const QUOTE_MIN_CHARS: usize = 8;

/// One change to a lane's requirement set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum RevisionChange {
    Add { requirement: RequirementSpec },
    Modify { requirement: RequirementSpec },
    Waive { req_id: String },
    Replace { requirements: Vec<RequirementSpec> },
}

/// State of a stored revision proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RevisionState {
    Proposed,
    Accepted,
    Rejected,
}

impl RevisionState {
    pub fn as_str(self) -> &'static str {
        match self {
            RevisionState::Proposed => "proposed",
            RevisionState::Accepted => "accepted",
            RevisionState::Rejected => "rejected",
        }
    }
}

impl std::fmt::Display for RevisionState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::str::FromStr for RevisionState {
    type Err = GoalError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        match text {
            "proposed" => Ok(RevisionState::Proposed),
            "accepted" => Ok(RevisionState::Accepted),
            "rejected" => Ok(RevisionState::Rejected),
            other => Err(GoalError::Unknown {
                kind: "revision state",
                value: other.to_owned(),
            }),
        }
    }
}

/// What `change_json` stores: the change and the contract revision it was proposed against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct StoredChange {
    base_revision: i64,
    change: RevisionChange,
}

/// A stored revision proposal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingRevision {
    pub id: i64,
    pub lane_key: String,
    pub change: RevisionChange,
    /// The contract revision the change was proposed against.
    pub base_revision: i64,
    pub instruction_quote: String,
    pub state: RevisionState,
    pub created_at: String,
}

/// Every owner-authored text of a lane.
async fn owner_texts(pool: &SqlitePool, lane_key: &str) -> Result<Vec<String>, GoalError> {
    let mut texts: Vec<String> = sqlx::query_scalar("SELECT title FROM lanes WHERE key = ?")
        .bind(lane_key)
        .fetch_all(pool)
        .await?;
    texts.extend(
        sqlx::query_scalar::<_, String>("SELECT goal FROM external_sessions WHERE lane_key = ?")
            .bind(lane_key)
            .fetch_all(pool)
            .await?,
    );
    texts.extend(
        sqlx::query_scalar::<_, String>("SELECT text FROM lane_owner_replies WHERE lane_key = ?")
            .bind(lane_key)
            .fetch_all(pool)
            .await?,
    );
    texts.extend(
        sqlx::query_scalar::<_, String>(
            "SELECT text_excerpt FROM lane_turns
             WHERE lane_key = ? AND origin = 'human_correction' AND text_excerpt IS NOT NULL",
        )
        .bind(lane_key)
        .fetch_all(pool)
        .await?,
    );
    Ok(texts)
}

fn parse_row(
    (id, lane_key, change_json, instruction_quote, state, created_at): RevisionRow,
) -> Result<PendingRevision, GoalError> {
    let stored: StoredChange = serde_json::from_str(&change_json).map_err(|error| {
        GoalError::InvalidContract(format!("stored revision {id}: {error}"))
    })?;
    Ok(PendingRevision {
        id,
        lane_key,
        change: stored.change,
        base_revision: stored.base_revision,
        instruction_quote,
        state: state.parse::<RevisionState>()?,
        created_at,
    })
}

type RevisionRow = (i64, String, String, String, String, String);

const SELECT_REVISION: &str =
    "SELECT id, lane_key, change_json, instruction_quote, state, created_at FROM lane_goal_revisions";

/// Records a proposed revision when `instruction_quote` (trimmed, at least
/// [`QUOTE_MIN_CHARS`] characters) is an exact substring of owner-authored text for the
/// lane. Anything else is [`GoalError::QuoteNotFound`]. Nothing is applied here.
pub async fn propose(
    pool: &SqlitePool,
    lane_key: &str,
    change: &RevisionChange,
    instruction_quote: &str,
) -> Result<PendingRevision, GoalError> {
    let quote = instruction_quote.trim();
    if quote.chars().count() < QUOTE_MIN_CHARS {
        return Err(GoalError::QuoteNotFound);
    }
    if !owner_texts(pool, lane_key)
        .await?
        .iter()
        .any(|text| text.contains(quote))
    {
        return Err(GoalError::QuoteNotFound);
    }
    let contract =
        contract_of(pool, lane_key).await?.ok_or_else(|| GoalError::NoContract(lane_key.to_owned()))?;
    let change_json = serde_json::to_string(&StoredChange {
        base_revision: contract.revision,
        change: change.clone(),
    })
    .map_err(|error| GoalError::InvalidContract(error.to_string()))?;
    let row: RevisionRow = sqlx::query_as(
        "INSERT INTO lane_goal_revisions(lane_key, change_json, instruction_quote, state, created_at)
         VALUES(?,?,?,'proposed',?)
         RETURNING id, lane_key, change_json, instruction_quote, state, created_at",
    )
    .bind(lane_key)
    .bind(change_json)
    .bind(quote)
    .bind(now_rfc3339())
    .fetch_one(pool)
    .await?;
    parse_row(row)
}

async fn proposed(pool: &SqlitePool, id: i64) -> Result<PendingRevision, GoalError> {
    let row: Option<RevisionRow> = sqlx::query_as(&format!("{SELECT_REVISION} WHERE id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await?;
    let revision = row
        .map(parse_row)
        .transpose()?
        .ok_or_else(|| GoalError::InvalidContract(format!("no revision {id}")))?;
    if revision.state != RevisionState::Proposed {
        return Err(GoalError::InvalidContract(format!(
            "revision {id} is already {}",
            revision.state
        )));
    }
    Ok(revision)
}

fn check_spec(requirement: &RequirementSpec) -> Result<(), GoalError> {
    if requirement.id.trim().is_empty()
        || requirement.text.trim().is_empty()
        || requirement.verifier.trim().is_empty()
    {
        return Err(GoalError::InvalidContract(
            "every requirement needs an id, text and verifier".into(),
        ));
    }
    Ok(())
}

/// Applies a proposed revision when the contract is still at the revision it was proposed
/// against, bumps the contract revision, and marks the proposal accepted.
pub async fn accept(pool: &SqlitePool, id: i64) -> Result<Contract, GoalError> {
    let revision = proposed(pool, id).await?;
    let lane_key = revision.lane_key.as_str();
    let now = now_rfc3339();
    let mut tx = pool.begin_write().await?;
    let bumped = sqlx::query(
        "UPDATE lane_goal SET revision = revision + 1, updated_at = ? WHERE lane_key = ? AND revision = ?",
    )
    .bind(&now)
    .bind(lane_key)
    .bind(revision.base_revision)
    .execute(&mut *tx)
    .await?;
    if bumped.rows_affected() == 0 {
        let actual: Option<i64> =
            sqlx::query_scalar("SELECT revision FROM lane_goal WHERE lane_key = ?")
                .bind(lane_key)
                .fetch_optional(&mut *tx)
                .await?;
        return Err(match actual {
            Some(actual) => GoalError::RevisionChanged {
                expected: revision.base_revision,
                actual,
            },
            None => GoalError::NoContract(lane_key.to_owned()),
        });
    }
    let insert = "INSERT INTO lane_goal_requirements(lane_key, req_id, text, verifier, status, evidence_json, updated_at)
                  VALUES(?,?,?,?,'pending','[]',?)";
    match &revision.change {
        RevisionChange::Add { requirement } => {
            check_spec(requirement)?;
            let exists: Option<i64> = sqlx::query_scalar(
                "SELECT 1 FROM lane_goal_requirements WHERE lane_key = ? AND req_id = ?",
            )
            .bind(lane_key)
            .bind(requirement.id.trim())
            .fetch_optional(&mut *tx)
            .await?;
            if exists.is_some() {
                return Err(GoalError::InvalidContract(format!(
                    "duplicate requirement id {}",
                    requirement.id
                )));
            }
            sqlx::query(insert)
                .bind(lane_key)
                .bind(requirement.id.trim())
                .bind(&requirement.text)
                .bind(&requirement.verifier)
                .bind(&now)
                .execute(&mut *tx)
                .await?;
        }
        RevisionChange::Modify { requirement } => {
            check_spec(requirement)?;
            // A changed requirement must be proven again.
            let updated = sqlx::query(
                "UPDATE lane_goal_requirements SET text = ?, verifier = ?, status = 'pending',
                        evidence_json = '[]', updated_at = ?
                 WHERE lane_key = ? AND req_id = ?",
            )
            .bind(&requirement.text)
            .bind(&requirement.verifier)
            .bind(&now)
            .bind(lane_key)
            .bind(requirement.id.trim())
            .execute(&mut *tx)
            .await?;
            if updated.rows_affected() == 0 {
                return Err(GoalError::UnknownRequirement {
                    lane_key: lane_key.to_owned(),
                    req_id: requirement.id.clone(),
                });
            }
        }
        RevisionChange::Waive { req_id } => {
            let updated = sqlx::query(
                "UPDATE lane_goal_requirements SET status = 'waived', updated_at = ?
                 WHERE lane_key = ? AND req_id = ?",
            )
            .bind(&now)
            .bind(lane_key)
            .bind(req_id)
            .execute(&mut *tx)
            .await?;
            if updated.rows_affected() == 0 {
                return Err(GoalError::UnknownRequirement {
                    lane_key: lane_key.to_owned(),
                    req_id: req_id.clone(),
                });
            }
        }
        RevisionChange::Replace { requirements } => {
            if requirements.is_empty() {
                return Err(GoalError::EmptyContract);
            }
            let mut seen = HashSet::new();
            for requirement in requirements {
                check_spec(requirement)?;
                if !seen.insert(requirement.id.trim()) {
                    return Err(GoalError::InvalidContract(format!(
                        "duplicate requirement id {}",
                        requirement.id
                    )));
                }
            }
            sqlx::query("DELETE FROM lane_goal_requirements WHERE lane_key = ?")
                .bind(lane_key)
                .execute(&mut *tx)
                .await?;
            for requirement in requirements {
                sqlx::query(insert)
                    .bind(lane_key)
                    .bind(requirement.id.trim())
                    .bind(&requirement.text)
                    .bind(&requirement.verifier)
                    .bind(&now)
                    .execute(&mut *tx)
                    .await?;
            }
        }
    }
    sqlx::query("UPDATE lane_goal_revisions SET state = 'accepted' WHERE id = ? AND state = 'proposed'")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    contract_of(pool, lane_key)
        .await?
        .ok_or_else(|| GoalError::NoContract(lane_key.to_owned()))
}

/// Marks a proposed revision rejected. The contract is untouched.
pub async fn reject(pool: &SqlitePool, id: i64) -> Result<(), GoalError> {
    proposed(pool, id).await?;
    sqlx::query("UPDATE lane_goal_revisions SET state = 'rejected' WHERE id = ? AND state = 'proposed'")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// The lane a stored revision belongs to, or `None` when there is no such revision.
pub async fn lane_of(pool: &SqlitePool, id: i64) -> Result<Option<String>, GoalError> {
    Ok(
        sqlx::query_scalar("SELECT lane_key FROM lane_goal_revisions WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await?,
    )
}

/// The lane's revisions still awaiting a decision, oldest first.
pub async fn list_pending(
    pool: &SqlitePool,
    lane_key: &str,
) -> Result<Vec<PendingRevision>, GoalError> {
    let rows: Vec<RevisionRow> = sqlx::query_as(&format!(
        "{SELECT_REVISION} WHERE lane_key = ? AND state = 'proposed' ORDER BY id"
    ))
    .bind(lane_key)
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(parse_row).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::goals::contract::set_contract;
    use crate::goals::turns::record_turn_at;
    use crate::goals::types::{GoalMode, RequirementStatus, TurnOrigin};
    use crate::lanes::state::insert_lane;

    const TITLE: &str = "Add a dark mode toggle to the settings page";
    const REPLY: &str = "please also make sure the toggle persists across restarts";
    const CORRECTION: &str = "no, keep the default theme light for new users";
    const SUPERVISOR: &str = "add an accessibility audit of every page";

    fn spec(id: &str, text: &str) -> RequirementSpec {
        RequirementSpec {
            id: id.into(),
            text: text.into(),
            verifier: format!("check {id}"),
        }
    }

    fn add_r2() -> RevisionChange {
        RevisionChange::Add {
            requirement: spec("r2", "the toggle persists across restarts"),
        }
    }

    async fn lane_pool() -> SqlitePool {
        let pool = crate::storage::memory().await.unwrap();
        insert_lane(&pool, "rev", TITLE, "lead", "/tmp", "file marker")
            .await
            .unwrap();
        set_contract(&pool, "rev", &[spec("r1", "a dark mode toggle exists")], GoalMode::Supervised, 20)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO lane_owner_replies(message_id, lane_key, author_id, text, created_at)
             VALUES('m1','rev','U1',?,'2026-10-24T10:00:00+00:00')",
        )
        .bind(REPLY)
        .execute(&pool)
        .await
        .unwrap();
        record_turn_at(&pool, "rev", TurnOrigin::HumanCorrection, Some(CORRECTION), None, "2026-10-24T10:01:00+00:00")
            .await
            .unwrap();
        record_turn_at(&pool, "rev", TurnOrigin::SupervisorProposal, Some(SUPERVISOR), None, "2026-10-24T10:02:00+00:00")
            .await
            .unwrap();
        pool
    }

    async fn state_of(pool: &SqlitePool, id: i64) -> String {
        sqlx::query_scalar("SELECT state FROM lane_goal_revisions WHERE id = ?")
            .bind(id)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn an_exact_owner_reply_quote_is_proposed() {
        let pool = lane_pool().await;
        let revision = propose(&pool, "rev", &add_r2(), "  make sure the toggle persists  ")
            .await
            .unwrap();
        assert_eq!(revision.state, RevisionState::Proposed);
        assert_eq!(revision.instruction_quote, "make sure the toggle persists");
        assert_eq!(revision.base_revision, 1);
        assert_eq!(revision.change, add_r2());
        assert_eq!(list_pending(&pool, "rev").await.unwrap(), [revision]);
    }

    #[tokio::test]
    async fn an_exact_human_correction_quote_is_proposed() {
        let pool = lane_pool().await;
        let revision = propose(&pool, "rev", &add_r2(), "keep the default theme light")
            .await
            .unwrap();
        assert_eq!(revision.state, RevisionState::Proposed);
    }

    #[tokio::test]
    async fn an_exact_lane_goal_quote_is_proposed() {
        let pool = lane_pool().await;
        assert!(propose(&pool, "rev", &add_r2(), "dark mode toggle to the settings")
            .await
            .is_ok());
        sqlx::query("INSERT INTO external_sessions(lane_key, goal, send_key) VALUES('rev', 'ship the theme picker to beta users', 'k')")
            .execute(&pool)
            .await
            .unwrap();
        assert!(propose(&pool, "rev", &add_r2(), "theme picker to beta")
            .await
            .is_ok());
        assert_eq!(list_pending(&pool, "rev").await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn a_supervisor_turn_quote_is_refused() {
        let pool = lane_pool().await;
        assert!(matches!(
            propose(&pool, "rev", &add_r2(), "an accessibility audit of every page").await,
            Err(GoalError::QuoteNotFound)
        ));
        assert!(list_pending(&pool, "rev").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_paraphrase_or_a_short_quote_is_refused() {
        let pool = lane_pool().await;
        for quote in [
            "make sure the toggle survives restarts",
            "Please Also Make Sure",
            "toggle",
            "  dark   ",
            "",
        ] {
            assert!(
                matches!(
                    propose(&pool, "rev", &add_r2(), quote).await,
                    Err(GoalError::QuoteNotFound)
                ),
                "{quote:?} must be refused"
            );
        }
        assert!(list_pending(&pool, "rev").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn accept_bumps_the_revision_and_changes_requirements() {
        let pool = lane_pool().await;
        let added = propose(&pool, "rev", &add_r2(), "make sure the toggle persists")
            .await
            .unwrap();
        let contract = accept(&pool, added.id).await.unwrap();
        assert_eq!(contract.revision, 2);
        let ids: Vec<&str> = contract.requirements.iter().map(|item| item.id.as_str()).collect();
        assert_eq!(ids, ["r1", "r2"]);
        assert_eq!(state_of(&pool, added.id).await, "accepted");
        assert!(list_pending(&pool, "rev").await.unwrap().is_empty());

        let waive = RevisionChange::Waive { req_id: "r1".into() };
        let waived = propose(&pool, "rev", &waive, "keep the default theme light").await.unwrap();
        let contract = accept(&pool, waived.id).await.unwrap();
        assert_eq!(contract.revision, 3);
        assert_eq!(contract.requirements[0].status, RequirementStatus::Waived);

        let modify = RevisionChange::Modify {
            requirement: spec("r2", "the toggle persists across app restarts"),
        };
        let modified = propose(&pool, "rev", &modify, "persists across restarts").await.unwrap();
        let contract = accept(&pool, modified.id).await.unwrap();
        assert_eq!(contract.revision, 4);
        assert_eq!(contract.requirements[1].text, "the toggle persists across app restarts");

        let replace = RevisionChange::Replace {
            requirements: vec![spec("r9", "the theme picker ships")],
        };
        let replaced = propose(&pool, "rev", &replace, "dark mode toggle").await.unwrap();
        let contract = accept(&pool, replaced.id).await.unwrap();
        assert_eq!(contract.revision, 5);
        assert_eq!(contract.requirements.len(), 1);
        assert_eq!(contract.requirements[0].id, "r9");
        assert!(accept(&pool, replaced.id).await.is_err(), "accepted twice");
    }

    #[tokio::test]
    async fn accept_on_a_stale_revision_is_refused() {
        let pool = lane_pool().await;
        let revision = propose(&pool, "rev", &add_r2(), "make sure the toggle persists")
            .await
            .unwrap();
        set_contract(&pool, "rev", &[spec("r1", "a dark mode toggle exists")], GoalMode::Supervised, 20)
            .await
            .unwrap();
        assert!(matches!(
            accept(&pool, revision.id).await,
            Err(GoalError::RevisionChanged {
                expected: 1,
                actual: 2
            })
        ));
        let contract = contract_of(&pool, "rev").await.unwrap().unwrap();
        assert_eq!(contract.revision, 2);
        assert_eq!(contract.requirements.len(), 1);
        assert_eq!(state_of(&pool, revision.id).await, "proposed");
    }

    #[tokio::test]
    async fn reject_marks_the_proposal_and_leaves_the_contract() {
        let pool = lane_pool().await;
        let before = contract_of(&pool, "rev").await.unwrap().unwrap();
        let revision = propose(&pool, "rev", &add_r2(), "make sure the toggle persists")
            .await
            .unwrap();
        reject(&pool, revision.id).await.unwrap();
        assert_eq!(state_of(&pool, revision.id).await, "rejected");
        assert_eq!(contract_of(&pool, "rev").await.unwrap().unwrap(), before);
        assert!(list_pending(&pool, "rev").await.unwrap().is_empty());
        assert!(accept(&pool, revision.id).await.is_err());
    }
}
