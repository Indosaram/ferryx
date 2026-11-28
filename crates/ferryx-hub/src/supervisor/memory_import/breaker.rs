//! Persisted UTC-day input-token breaker for owner agent-memory import.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use super::{OMO_MEMORY_CAUSE, SupervisorError};

const KEY: &str = "supervisor_omo_memory_token_breaker";
const ENV: &str = "FERRYX_HUB_OMO_MEMORY_DAILY_TOKENS";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyTokens {
    pub day: String,
    pub cap: i64,
    pub input_tokens: i64,
    pub unknown_calls: i64,
    pub reached: bool,
}

#[derive(Debug, Clone)]
pub enum DailyTokensTime {
    DateTime(DateTime<Utc>),
    Rfc3339(String),
}

impl From<DateTime<Utc>> for DailyTokensTime {
    fn from(dt: DateTime<Utc>) -> Self {
        Self::DateTime(dt)
    }
}

impl From<&str> for DailyTokensTime {
    fn from(s: &str) -> Self {
        Self::Rfc3339(s.to_string())
    }
}

impl From<String> for DailyTokensTime {
    fn from(s: String) -> Self {
        Self::Rfc3339(s)
    }
}

impl From<&String> for DailyTokensTime {
    fn from(s: &String) -> Self {
        Self::Rfc3339(s.clone())
    }
}

pub fn daily_cap() -> i64 {
    crate::hub_env::var(ENV).ok().and_then(|value| value.trim().parse().ok())
        .filter(|value| *value >= 0).unwrap_or(2_000_000)
}

/// A reached breaker is latched until a later UTC day, even if the owner changes the cap
/// or old rows are removed. A backwards clock never opens it early.
pub async fn daily_tokens(pool: &SqlitePool, now: impl Into<DailyTokensTime>, cap: i64) -> Result<DailyTokens, SupervisorError> {
    let day = match now.into() {
        DailyTokensTime::DateTime(dt) => dt.date_naive().to_string(),
        DailyTokensTime::Rfc3339(s) => match DateTime::parse_from_rfc3339(&s) {
            Ok(dt) => dt.with_timezone(&Utc).date_naive().to_string(),
            Err(_) if s.len() >= 10 && s.chars().nth(4) == Some('-') && s.chars().nth(7) == Some('-') => {
                s[..10].to_string()
            }
            Err(error) => {
                return Err(SupervisorError::Invalid(format!(
                    "invalid timestamp for daily tokens: {error}"
                )))
            }
        },
    };
    let stored: Option<String> = sqlx::query_scalar("SELECT value FROM hub_meta WHERE key = ?")
        .bind(KEY).fetch_optional(pool).await?;
    let previous = stored.map(|text| serde_json::from_str::<DailyTokens>(&text))
        .transpose().map_err(|error| SupervisorError::Invalid(format!("invalid memory token breaker: {error}")))?;
    if let Some(previous) = previous {
        if previous.reached && previous.day >= day {
            return Ok(previous);
        }
    }
    let (input_tokens, unknown_calls): (i64, i64) = sqlx::query_as(
        "SELECT COALESCE(SUM(input_tokens), 0),
                COALESCE(SUM(CASE WHEN input_tokens IS NULL AND outcome != 'MHC_PROTOCOL' THEN 1 ELSE 0 END), 0)
         FROM supervisor_model_calls WHERE cause = ? AND date(created_at) = ?",
    ).bind(OMO_MEMORY_CAUSE).bind(&day).fetch_one(pool).await?;
    let status = DailyTokens { day, cap, input_tokens, unknown_calls,
        reached: cap <= 0 || unknown_calls > 0 || input_tokens >= cap };
    sqlx::query("INSERT INTO hub_meta(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value")
        .bind(KEY).bind(serde_json::to_string(&status).map_err(|error| SupervisorError::Invalid(error.to_string()))?)
        .execute(pool).await?;
    Ok(status)
}
