//! Approval ledger. An allowlisted signature is auto-allowed. Yolo auto-allows a clean
//! action and still holds a tainted one for the owner.
use sqlx::SqlitePool;

use crate::error::Result;
use crate::storage::now_rfc3339;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Pending,
    AutoAllowed,
    AutoDenied,
    Approved,
    Declined,
}

impl Decision {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::AutoAllowed => "auto_allowed",
            Self::AutoDenied => "auto_denied",
            Self::Approved => "approved",
            Self::Declined => "declined",
        }
    }
}

pub async fn allow(pool: &SqlitePool, pattern: &str) -> Result<()> {
    sqlx::query("INSERT OR IGNORE INTO approval_allowlist(pattern_key, created_at) VALUES(?, ?)")
        .bind(pattern)
        .bind(now_rfc3339())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn is_allowed(pool: &SqlitePool, pattern: &str) -> Result<bool> {
    let found: Option<i64> =
        sqlx::query_scalar("SELECT 1 FROM approval_allowlist WHERE pattern_key = ?")
            .bind(pattern)
            .fetch_optional(pool)
            .await?;
    Ok(found.is_some())
}

pub async fn set_yolo(pool: &SqlitePool, session_key: &str, on: bool) -> Result<()> {
    sqlx::query(
        "INSERT INTO action_loop_state(session_key, yolo, updated_at) VALUES(?,?,?)
         ON CONFLICT(session_key) DO UPDATE SET yolo = excluded.yolo, updated_at = excluded.updated_at",
    )
    .bind(session_key)
    .bind(i64::from(on))
    .bind(now_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn yolo(pool: &SqlitePool, session_key: &str) -> Result<bool> {
    let flag: Option<i64> =
        sqlx::query_scalar("SELECT yolo FROM action_loop_state WHERE session_key = ?")
            .bind(session_key)
            .fetch_optional(pool)
            .await?;
    Ok(flag == Some(1))
}

pub async fn propose(
    pool: &SqlitePool,
    origin: &str,
    session_key: &str,
    signature: &str,
    effect: &str,
    tainted: bool,
) -> Result<i64> {
    let yolo_on = yolo(pool, session_key).await?;
    let allowed = is_allowed(pool, signature).await?;
    let decision = if allowed || (yolo_on && !tainted) {
        Decision::AutoAllowed
    } else {
        Decision::Pending
    };
    let id = sqlx::query(
        "INSERT INTO action_proposals(origin, session_key, signature, effect, tainted, state, created_at) VALUES(?,?,?,?,?,?,?)",
    )
    .bind(origin)
    .bind(session_key)
    .bind(signature)
    .bind(effect)
    .bind(i64::from(tainted))
    .bind(decision.as_str())
    .bind(now_rfc3339())
    .execute(pool)
    .await?
    .last_insert_rowid();
    Ok(id)
}

pub async fn resolve(
    pool: &SqlitePool,
    id: i64,
    allow: bool,
    owner_words: Option<&str>,
) -> Result<()> {
    let state = if allow {
        Decision::Approved
    } else {
        Decision::Declined
    };
    sqlx::query(
        "UPDATE action_proposals SET state = ?, owner_words = ?, decided_at = ? WHERE id = ?",
    )
    .bind(state.as_str())
    .bind(owner_words)
    .bind(now_rfc3339())
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn remember(
    pool: &SqlitePool,
    signature: &str,
    scope: &str,
    decision: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO action_precedents(signature, effect, decision, scope, origin, created_at, updated_at)
         VALUES(?, ?, ?, ?, 'owner', ?, ?)
         ON CONFLICT(signature, scope) DO UPDATE SET decision = excluded.decision, updated_at = excluded.updated_at",
    )
    .bind(signature)
    .bind(signature)
    .bind(decision)
    .bind(scope)
    .bind(now_rfc3339())
    .bind(now_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn allowlist_and_yolo_skip_the_owner_and_taint_does_not() {
        let pool = crate::storage::memory().await.unwrap();
        allow(&pool, "git status").await.unwrap();
        let allowed = propose(&pool, "loop", "s", "git status", "read", false)
            .await
            .unwrap();
        let state: String = sqlx::query_scalar("SELECT state FROM action_proposals WHERE id = ?")
            .bind(allowed)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(state, "auto_allowed");
        set_yolo(&pool, "s", true).await.unwrap();
        let clean = propose(&pool, "loop", "s", "cargo test", "test", false)
            .await
            .unwrap();
        let clean_state: String =
            sqlx::query_scalar("SELECT state FROM action_proposals WHERE id = ?")
                .bind(clean)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(clean_state, "auto_allowed");
        let tainted = propose(&pool, "loop", "s", "rm -rf", "delete", true)
            .await
            .unwrap();
        let tainted_state: String =
            sqlx::query_scalar("SELECT state FROM action_proposals WHERE id = ?")
                .bind(tainted)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(tainted_state, "pending");
        resolve(&pool, tainted, false, Some("no")).await.unwrap();
        remember(&pool, "rm -rf", "exact", "deny").await.unwrap();
        remember(&pool, "rm -rf", "exact", "deny").await.unwrap();
        let precedents: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM action_precedents")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(precedents, 1);
    }
}
