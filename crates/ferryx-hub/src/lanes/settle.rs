//! Settlement moves `done_claimed` to `verified_done` when every signal passes and the
//! judge accepts the claim. It runs when a claim arrives, not on a poll.
use sqlx::SqlitePool;

use super::signals::{check_signals, parse_signals};
use super::state::{transition, LaneStatus};
use crate::error::{HubError, Result};
use crate::judge::{Choice, Judge, Purpose};

pub struct Settlement {
    pub passed: bool,
    pub detail: String,
}

pub async fn settle(
    pool: &SqlitePool,
    key: &str,
    cwd: &str,
    done_signals: &str,
    claim_evidence: &str,
    judge: &Judge,
) -> Result<Settlement> {
    let signals =
        parse_signals(done_signals).map_err(|error| HubError::message(error.to_string()))?;
    let checks = check_signals(&signals, std::path::Path::new(cwd)).await;
    if checks.iter().any(|check| !check.passed) {
        transition(pool, key, LaneStatus::NotDone, Some("signals failed")).await?;
        return Ok(Settlement {
            passed: false,
            detail: checks
                .into_iter()
                .map(|check| check.detail)
                .collect::<Vec<_>>()
                .join("; "),
        });
    }
    let choice = judge
        .choice(
            Purpose::DoneDiff,
            claim_evidence,
            "Does the claim evidence show the lane is done? Reply yes or no.",
            &["yes", "no"],
        )
        .await?;
    if choice.as_str() == "yes" {
        transition(
            pool,
            key,
            LaneStatus::VerifiedDone,
            Some("settlement_started"),
        )
        .await?;
        Ok(Settlement {
            passed: true,
            detail: choice.as_str().to_owned(),
        })
    } else {
        transition(pool, key, LaneStatus::NotDone, Some("judge declined")).await?;
        Ok(Settlement {
            passed: false,
            detail: choice.as_str().to_owned(),
        })
    }
}

pub fn accepting_fallback(selected: &str) -> Choice {
    Choice::fallback(selected)
}
