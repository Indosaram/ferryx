//! What the desktop shows as supervised: every live lane bound to a pane, and every pane on the
//! watch list. Read-only, except for the pause a person sets from the desktop.
use serde::Serialize;
use sqlx::SqlitePool;

use crate::error::Result;
use crate::goals::contract::{self, Contract};
use crate::goals::revision::{self, PendingRevision, RevisionChange};
use crate::goals::types::{GoalMode, ProposalState, RequirementStatus, StopReason};

/// Statuses that are history, not supervision.
const ENDED: &str = "('closed', 'failed', 'verified_done')";

/// One lane the supervisor drives, as the desktop's Supervisor view lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupervisedLane {
    pub key: String,
    pub title: String,
    pub status: String,
    /// The pane the lane runs in, once launched or adopted.
    pub session_id: Option<String>,
    pub cwd: String,
    /// The adopted session's goal; launched lanes carry their title only.
    pub goal: Option<String>,
    pub paused: bool,
    pub last_activity_at: String,
    /// The reason on the lane's newest history row (a transition, a pause or a resume).
    pub last_reason: Option<String>,
    /// The lane's goal contract. Absent (no key at all) for a lane without one, so existing
    /// consumers see the same shape as before.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub goal_contract: Option<GoalView>,
}

/// A lane's goal contract as the Supervisor view, CLI and MCP show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalView {
    pub mode: GoalMode,
    pub turns_used: u32,
    pub turn_limit: u32,
    pub revision: i64,
    pub requirements: Vec<RequirementView>,
    pub stop_reason: Option<StopReason>,
    pub proposal: Option<ProposalView>,
    pub pending_revisions: Vec<PendingRevisionView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequirementView {
    pub id: String,
    pub text: String,
    pub verifier: String,
    pub status: RequirementStatus,
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposalView {
    pub text: String,
    pub state: ProposalState,
    pub gap_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingRevisionView {
    pub id: i64,
    pub change: RevisionChange,
    pub instruction_quote: String,
}

impl GoalView {
    pub fn new(contract: Contract, pending: Vec<PendingRevision>) -> Self {
        Self {
            mode: contract.mode,
            turns_used: contract.turns_used,
            turn_limit: contract.turn_limit,
            revision: contract.revision,
            requirements: contract
                .requirements
                .into_iter()
                .map(|requirement| RequirementView {
                    id: requirement.id,
                    text: requirement.text,
                    verifier: requirement.verifier,
                    status: requirement.status,
                    evidence_refs: requirement.evidence,
                })
                .collect(),
            stop_reason: contract.stop_reason,
            proposal: contract.proposal.map(|proposal| ProposalView {
                text: proposal.text,
                state: proposal.state,
                gap_ids: proposal.gap_ids,
            }),
            pending_revisions: pending
                .into_iter()
                .map(|pending| PendingRevisionView {
                    id: pending.id,
                    change: pending.change,
                    instruction_quote: pending.instruction_quote,
                })
                .collect(),
        }
    }
}

/// The lane's goal view, or `None` when the lane has no contract.
pub async fn goal_view(pool: &SqlitePool, lane_key: &str) -> Result<Option<GoalView>> {
    let Some(contract) = contract::contract_of(pool, lane_key).await? else {
        return Ok(None);
    };
    let pending = revision::list_pending(pool, lane_key).await?;
    Ok(Some(GoalView::new(contract, pending)))
}

type LaneRow = (
    String,
    String,
    String,
    Option<String>,
    String,
    Option<String>,
    bool,
    String,
    Option<String>,
);

/// Lanes that are still live, most recently active first.
pub async fn supervised_lanes(pool: &SqlitePool) -> Result<Vec<SupervisedLane>> {
    let rows: Vec<LaneRow> = sqlx::query_as(&format!(
        "SELECT lanes.key, lanes.title, lanes.status, lanes.omo_thread_id, lanes.cwd, external_sessions.goal,
                EXISTS(SELECT 1 FROM lane_pauses WHERE lane_pauses.lane_key = lanes.key AND lane_pauses.resumed_at IS NULL),
                COALESCE(lanes.last_activity_at, lanes.updated_at),
                (SELECT lane_history.reason FROM lane_history WHERE lane_history.lane_key = lanes.key
                 ORDER BY lane_history.id DESC LIMIT 1)
         FROM lanes LEFT JOIN external_sessions ON external_sessions.lane_key = lanes.key
         WHERE lanes.status NOT IN {ENDED}
         ORDER BY 8 DESC, lanes.key"
    ))
    .fetch_all(pool)
    .await?;
    let mut lanes = Vec::with_capacity(rows.len());
    for (key, title, status, session_id, cwd, goal, paused, last_activity_at, last_reason) in rows {
        let goal_contract = goal_view(pool, &key).await?;
        lanes.push(SupervisedLane {
            key,
            title,
            status,
            session_id: session_id.filter(|id| !id.is_empty()),
            cwd,
            goal,
            paused,
            last_activity_at,
            last_reason,
            goal_contract,
        });
    }
    Ok(lanes)
}

/// Local panes on the watch list (targets of the form `local:<session>`).
pub async fn watched_local_sessions(pool: &SqlitePool) -> Result<Vec<String>> {
    let targets: Vec<String> =
        sqlx::query_scalar("SELECT target FROM session_watches ORDER BY added_at")
            .fetch_all(pool)
            .await?;
    Ok(targets
        .into_iter()
        .filter_map(|target| target.strip_prefix("local:").map(str::to_owned))
        .collect())
}

/// True when `session_id` is the pane of a live lane a person paused.
pub async fn session_paused(pool: &SqlitePool, session_id: &str) -> Result<bool> {
    Ok(sqlx::query_scalar(&format!(
        "SELECT EXISTS(SELECT 1 FROM lanes JOIN lane_pauses ON lane_pauses.lane_key = lanes.key
         WHERE lanes.omo_thread_id = ? AND lane_pauses.resumed_at IS NULL
           AND lanes.status NOT IN {ENDED})"
    ))
    .bind(session_id)
    .fetch_one(pool)
    .await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::goals::types::GoalMode;
    use crate::lanes::pause::pause;
    use crate::lanes::state::{insert_lane, set_pane, transition, LaneStatus};

    #[tokio::test]
    async fn live_lanes_carry_their_pane_goal_and_pause() {
        let pool = crate::storage::memory().await.unwrap();
        insert_lane(&pool, "ledger", "Ledger", "lead", "/repo", "file marker")
            .await
            .unwrap();
        transition(&pool, "ledger", LaneStatus::Working, Some("launched"))
            .await
            .unwrap();
        set_pane(&pool, "ledger", "pane-1").await.unwrap();
        insert_lane(&pool, "old", "Old", "lead", "/repo", "file marker")
            .await
            .unwrap();
        transition(&pool, "old", LaneStatus::Working, Some("launched"))
            .await
            .unwrap();
        transition(&pool, "old", LaneStatus::Failed, Some("gave up"))
            .await
            .unwrap();
        sqlx::query("INSERT INTO external_sessions(lane_key, goal, send_key, cursor_revision, cursor_text) VALUES('ledger','Get tests green','ext-ledger','r','')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO session_watches(target, added_at) VALUES('local:pane-2','2026-10-23T00:00:00Z'),('ssh:box:pane-3','2026-10-23T00:00:01Z')")
            .execute(&pool)
            .await
            .unwrap();

        let lanes = supervised_lanes(&pool).await.unwrap();
        assert_eq!(lanes.len(), 1, "a failed lane is not supervised: {lanes:?}");
        assert_eq!(lanes[0].session_id.as_deref(), Some("pane-1"));
        assert_eq!(lanes[0].goal.as_deref(), Some("Get tests green"));
        assert!(!lanes[0].paused);
        assert_eq!(lanes[0].last_reason.as_deref(), Some("launched"));
        assert!(!session_paused(&pool, "pane-1").await.unwrap());

        assert!(pause(&pool, "ledger", "desktop").await.unwrap());
        let paused = &supervised_lanes(&pool).await.unwrap()[0];
        assert!(paused.paused);
        assert_eq!(paused.last_reason.as_deref(), Some("paused"));
        assert!(session_paused(&pool, "pane-1").await.unwrap());
        assert!(!session_paused(&pool, "pane-2").await.unwrap());

        assert_eq!(
            watched_local_sessions(&pool).await.unwrap(),
            vec!["pane-2".to_string()]
        );
    }

    /// The serialized shape of a contract-less lane, exactly as it was before goal contracts.
    #[test]
    fn a_lane_without_a_contract_serializes_byte_identically_to_before() {
        let lane = SupervisedLane {
            key: "ledger".into(),
            title: "Ledger".into(),
            status: "working".into(),
            session_id: Some("pane-1".into()),
            cwd: "/repo".into(),
            goal: None,
            paused: false,
            last_activity_at: "2026-10-24T00:00:00Z".into(),
            last_reason: Some("launched".into()),
            goal_contract: None,
        };
        assert_eq!(
            serde_json::to_string(&lane).unwrap(),
            r#"{"key":"ledger","title":"Ledger","status":"working","sessionId":"pane-1","cwd":"/repo","goal":null,"paused":false,"lastActivityAt":"2026-10-24T00:00:00Z","lastReason":"launched"}"#
        );
    }

    #[tokio::test]
    async fn a_lane_with_a_contract_carries_its_goal_view() {
        use crate::goals::contract::{set_contract, RequirementSpec};
        let pool = crate::storage::memory().await.unwrap();
        insert_lane(&pool, "goal", "Goal", "lead", "/repo", "file marker")
            .await
            .unwrap();
        insert_lane(&pool, "plain", "Plain", "lead", "/repo", "file marker")
            .await
            .unwrap();
        let spec = RequirementSpec {
            id: "r1".into(),
            text: "tests pass".into(),
            verifier: "cargo test".into(),
        };
        set_contract(&pool, "goal", &[spec], GoalMode::Supervised, 5)
            .await
            .unwrap();

        let lanes = supervised_lanes(&pool).await.unwrap();
        let goal = lanes.iter().find(|lane| lane.key == "goal").unwrap();
        let plain = lanes.iter().find(|lane| lane.key == "plain").unwrap();
        assert!(plain.goal_contract.is_none());
        assert!(!serde_json::to_value(plain)
            .unwrap()
            .as_object()
            .unwrap()
            .contains_key("goalContract"));
        let view = goal.goal_contract.as_ref().unwrap();
        assert_eq!(view.turn_limit, 5);
        assert_eq!(view.requirements[0].id, "r1");
        let json = serde_json::to_value(goal).unwrap();
        assert_eq!(json["goalContract"]["mode"], "supervised");
        assert_eq!(
            json["goalContract"]["requirements"][0]["evidenceRefs"],
            serde_json::json!([])
        );
        assert_eq!(
            json["goalContract"]["pendingRevisions"],
            serde_json::json!([])
        );
    }

    #[tokio::test]
    async fn a_lane_without_history_has_no_last_reason() {
        let pool = crate::storage::memory().await.unwrap();
        insert_lane(&pool, "fresh", "Fresh", "lead", "/repo", "file marker")
            .await
            .unwrap();

        let lanes = supervised_lanes(&pool).await.unwrap();

        assert_eq!(lanes[0].last_reason, None);
    }
}
