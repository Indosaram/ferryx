use std::str::FromStr;

use sqlx::SqlitePool;
use thiserror::Error;

use crate::storage::now_rfc3339;

/// Lane lifecycle. `verified_done` is reachable only from `done_claimed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaneStatus {
    Queued,
    Working,
    Blocked,
    DoneClaimed,
    NotDone,
    VerifiedDone,
    Closed,
    Failed,
}

impl LaneStatus {
    pub const ALL: [LaneStatus; 8] = [
        LaneStatus::Queued,
        LaneStatus::Working,
        LaneStatus::Blocked,
        LaneStatus::DoneClaimed,
        LaneStatus::NotDone,
        LaneStatus::VerifiedDone,
        LaneStatus::Closed,
        LaneStatus::Failed,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            LaneStatus::Queued => "queued",
            LaneStatus::Working => "working",
            LaneStatus::Blocked => "blocked",
            LaneStatus::DoneClaimed => "done_claimed",
            LaneStatus::NotDone => "not_done",
            LaneStatus::VerifiedDone => "verified_done",
            LaneStatus::Closed => "closed",
            LaneStatus::Failed => "failed",
        }
    }

    pub fn allowed_from(self, reason: Option<&str>) -> &'static [LaneStatus] {
        match self {
            LaneStatus::Queued => &[],
            LaneStatus::Working => &[
                LaneStatus::Queued,
                LaneStatus::Blocked,
                LaneStatus::NotDone,
                LaneStatus::Closed,
                LaneStatus::Failed,
            ],
            LaneStatus::Blocked => &[LaneStatus::Working],
            LaneStatus::DoneClaimed => &[
                LaneStatus::Working,
                LaneStatus::Blocked,
                LaneStatus::NotDone,
            ],
            LaneStatus::NotDone => &[LaneStatus::DoneClaimed],
            LaneStatus::VerifiedDone => &[LaneStatus::DoneClaimed],
            LaneStatus::Closed => {
                if reason.is_some_and(|reason| reason.starts_with("forced:")) {
                    &[LaneStatus::Working, LaneStatus::VerifiedDone]
                } else {
                    &[LaneStatus::VerifiedDone]
                }
            }
            LaneStatus::Failed => &[LaneStatus::Queued, LaneStatus::Working, LaneStatus::Blocked],
        }
    }

    pub fn can_transition_to(self, to: LaneStatus, reason: Option<&str>) -> bool {
        to.allowed_from(reason).contains(&self)
    }
}

impl std::fmt::Display for LaneStatus {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("unknown lane status: {0}")]
pub struct UnknownLaneStatus(pub String);

impl FromStr for LaneStatus {
    type Err = UnknownLaneStatus;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        match text {
            "queued" => Ok(LaneStatus::Queued),
            "working" => Ok(LaneStatus::Working),
            "blocked" => Ok(LaneStatus::Blocked),
            "done_claimed" => Ok(LaneStatus::DoneClaimed),
            "not_done" => Ok(LaneStatus::NotDone),
            "verified_done" => Ok(LaneStatus::VerifiedDone),
            "closed" => Ok(LaneStatus::Closed),
            "failed" => Ok(LaneStatus::Failed),
            other => Err(UnknownLaneStatus(other.to_owned())),
        }
    }
}

#[derive(Debug, Error)]
pub enum LaneTransitionError {
    #[error("illegal lane transition from {from:?} to {to:?} (reason: {reason:?})")]
    IllegalTransition {
        from: Option<LaneStatus>,
        to: LaneStatus,
        reason: Option<String>,
    },
    #[error("lane not found: {0}")]
    LaneNotFound(String),
    #[error("storage error: {0}")]
    Storage(String),
}

/// Free text must never equal a reason that settlement bookkeeping matches on.
pub fn safe_reason(text: &str) -> String {
    match text {
        "reopened" | "settlement_started" | "escalated" => format!("note: {text}"),
        _ => text.to_owned(),
    }
}

pub async fn transition(
    pool: &SqlitePool,
    key: &str,
    to: LaneStatus,
    reason: Option<&str>,
) -> Result<(), LaneTransitionError> {
    let allowed = to.allowed_from(reason);
    if allowed.is_empty() {
        return illegal(pool, key, to, reason).await;
    }
    let previous: Option<String> = sqlx::query_scalar("SELECT status FROM lanes WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await
        .map_err(|error| LaneTransitionError::Storage(error.to_string()))?;
    let placeholders = allowed.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "UPDATE lanes SET status = ?, updated_at = ? WHERE key = ? AND status IN ({placeholders})"
    );
    let mut query = sqlx::query(&sql)
        .bind(to.as_str())
        .bind(now_rfc3339())
        .bind(key);
    for status in allowed {
        query = query.bind(status.as_str());
    }
    let updated = query
        .execute(pool)
        .await
        .map_err(|error| LaneTransitionError::Storage(error.to_string()))?;
    if updated.rows_affected() == 0 {
        return illegal(pool, key, to, reason).await;
    }
    let reopened =
        to == LaneStatus::Working && matches!(previous.as_deref(), Some("closed" | "failed"));
    sqlx::query(
        "INSERT INTO lane_history(lane_key, at, from_status, to_status, reason) VALUES(?,?,?,?,?)",
    )
    .bind(key)
    .bind(now_rfc3339())
    .bind(previous)
    .bind(to.as_str())
    .bind(reason)
    .execute(pool)
    .await
    .map_err(|error| LaneTransitionError::Storage(error.to_string()))?;
    if reopened {
        super::pause::clear_on_reopen(pool, key)
            .await
            .map_err(|error| LaneTransitionError::Storage(error.to_string()))?;
    }
    Ok(())
}

async fn illegal(
    pool: &SqlitePool,
    key: &str,
    to: LaneStatus,
    reason: Option<&str>,
) -> Result<(), LaneTransitionError> {
    let row: Option<(String,)> = sqlx::query_as("SELECT status FROM lanes WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await
        .map_err(|error| LaneTransitionError::Storage(error.to_string()))?;
    match row {
        None => Err(LaneTransitionError::LaneNotFound(key.to_owned())),
        Some((status,)) => Err(LaneTransitionError::IllegalTransition {
            from: status.parse().ok(),
            to,
            reason: reason.map(str::to_owned),
        }),
    }
}

pub async fn insert_lane(
    pool: &SqlitePool,
    key: &str,
    title: &str,
    lead: &str,
    cwd: &str,
    done_signals: &str,
) -> Result<(), LaneTransitionError> {
    if !valid_key(key) {
        return Err(LaneTransitionError::Storage(format!(
            "invalid lane key {key}"
        )));
    }
    sqlx::query(
        "INSERT INTO lanes(key,title,lead_session_key,cwd,session_key,status,done_signals,last_activity_at) VALUES(?,?,?,?,?,'queued',?,?)",
    )
    .bind(key)
    .bind(title)
    .bind(lead)
    .bind(cwd)
    .bind(lead)
    .bind(done_signals)
    .bind(now_rfc3339())
    .execute(pool)
    .await
    .map_err(|error| LaneTransitionError::Storage(error.to_string()))?;
    Ok(())
}

pub fn valid_key(key: &str) -> bool {
    (1..=48).contains(&key.len())
        && key
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
}

pub async fn status_of(
    pool: &SqlitePool,
    key: &str,
) -> Result<Option<LaneStatus>, LaneTransitionError> {
    let row: Option<String> = sqlx::query_scalar("SELECT status FROM lanes WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await
        .map_err(|error| LaneTransitionError::Storage(error.to_string()))?;
    row.map(|status| {
        status
            .parse()
            .map_err(|error: UnknownLaneStatus| LaneTransitionError::Storage(error.to_string()))
    })
    .transpose()
}

pub async fn set_pane(
    pool: &SqlitePool,
    key: &str,
    session_id: &str,
) -> Result<(), LaneTransitionError> {
    sqlx::query("UPDATE lanes SET omo_thread_id = ?, updated_at = ? WHERE key = ?")
        .bind(session_id)
        .bind(now_rfc3339())
        .bind(key)
        .execute(pool)
        .await
        .map_err(|error| LaneTransitionError::Storage(error.to_string()))?;
    Ok(())
}

pub async fn set_claim(
    pool: &SqlitePool,
    key: &str,
    evidence: &str,
) -> Result<(), LaneTransitionError> {
    sqlx::query("UPDATE lanes SET claim_evidence = ?, claim_at = ?, updated_at = ? WHERE key = ?")
        .bind(evidence)
        .bind(now_rfc3339())
        .bind(now_rfc3339())
        .bind(key)
        .execute(pool)
        .await
        .map_err(|error| LaneTransitionError::Storage(error.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_status_pair_matches_the_ported_table() {
        for from in LaneStatus::ALL {
            for to in LaneStatus::ALL {
                let ordinary = from.can_transition_to(to, None);
                let forced = from.can_transition_to(to, Some("forced: admin requested"));
                let expected = matches!(
                    (from, to),
                    (LaneStatus::Queued, LaneStatus::Working)
                        | (LaneStatus::Queued, LaneStatus::Failed)
                        | (LaneStatus::Working, LaneStatus::Blocked)
                        | (LaneStatus::Working, LaneStatus::DoneClaimed)
                        | (LaneStatus::Working, LaneStatus::Failed)
                        | (LaneStatus::Blocked, LaneStatus::Working)
                        | (LaneStatus::Blocked, LaneStatus::DoneClaimed)
                        | (LaneStatus::Blocked, LaneStatus::Failed)
                        | (LaneStatus::DoneClaimed, LaneStatus::VerifiedDone)
                        | (LaneStatus::DoneClaimed, LaneStatus::NotDone)
                        | (LaneStatus::NotDone, LaneStatus::Working)
                        | (LaneStatus::NotDone, LaneStatus::DoneClaimed)
                        | (LaneStatus::VerifiedDone, LaneStatus::Closed)
                        | (LaneStatus::Closed, LaneStatus::Working)
                        | (LaneStatus::Failed, LaneStatus::Working)
                );
                assert_eq!(ordinary, expected, "{from:?} -> {to:?}");
                let expected_forced =
                    matches!((from, to), (LaneStatus::Working, LaneStatus::Closed)) || expected;
                assert_eq!(forced, expected_forced, "forced {from:?} -> {to:?}");
            }
        }
    }

    #[test]
    fn safe_reason_prefixes_bookkeeping_words() {
        assert_eq!(safe_reason("reopened"), "note: reopened");
        assert_eq!(safe_reason("owner asked"), "owner asked");
    }

    #[tokio::test]
    async fn verified_done_is_refused_until_the_lane_is_done_claimed() {
        let pool = crate::storage::memory().await.unwrap();
        insert_lane(&pool, "lane-a", "A", "lead", "/tmp", "file marker")
            .await
            .unwrap();
        let error = transition(&pool, "lane-a", LaneStatus::VerifiedDone, None)
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            LaneTransitionError::IllegalTransition { .. }
        ));
        transition(&pool, "lane-a", LaneStatus::Working, None)
            .await
            .unwrap();
        transition(&pool, "lane-a", LaneStatus::DoneClaimed, Some("claim"))
            .await
            .unwrap();
        transition(&pool, "lane-a", LaneStatus::VerifiedDone, None)
            .await
            .unwrap();
        assert_eq!(
            status_of(&pool, "lane-a").await.unwrap(),
            Some(LaneStatus::VerifiedDone)
        );
    }
}
