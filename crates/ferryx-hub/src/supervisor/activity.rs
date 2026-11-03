//! Persisted supervisor action log (`supervisor_actions`, migration 0028).
//!
//! One row per incident: [`ActionLog::upsert_incident`] inserts the first event of an
//! `incident_key` and updates the same row in place afterwards.
use async_trait::async_trait;
use serde::de::DeserializeOwned;
use sqlx::SqlitePool;

use super::contract::{ActionEntry, ActionLog, SessionRef};
use crate::error::{HubError, Result};
use crate::storage::WriteTx;

/// [`ActionLog`] over the hub's SQLite pool.
#[derive(Clone)]
pub struct SqliteActionLog {
    pool: SqlitePool,
}

type Row = (
    i64,
    String,
    Option<String>,
    String,
    String,
    String,
    String,
    Option<String>,
    bool,
    i64,
    i64,
);

const COLUMNS: &str = "id, incident_key, session_ref, kind, summary, basis, evidence_json, \
                       undo_json, live, created_at, updated_at";

impl SqliteActionLog {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

fn to_json<T: serde::Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|error| HubError::message(error.to_string()))
}

fn from_json<T: DeserializeOwned>(text: &str) -> Result<T> {
    serde_json::from_str(text).map_err(|error| HubError::message(error.to_string()))
}

fn entry_of(row: Row) -> Result<ActionEntry> {
    let (
        id,
        incident_key,
        session_ref,
        kind,
        summary,
        basis,
        evidence,
        undo,
        live,
        created,
        updated,
    ) = row;
    Ok(ActionEntry {
        id,
        incident_key,
        session_ref: session_ref.as_deref().map(SessionRef::parse).transpose()?,
        kind: from_json(&to_json(&kind)?)?,
        summary,
        basis,
        evidence: from_json(&evidence)?,
        undo: undo.as_deref().map(from_json).transpose()?,
        live,
        created_at_ms: created,
        updated_at_ms: updated,
    })
}

fn kind_text(entry: &ActionEntry) -> Result<String> {
    from_json(&to_json(&entry.kind)?)
}

#[async_trait]
impl ActionLog for SqliteActionLog {
    async fn upsert_incident(&self, entry: ActionEntry) -> Result<ActionEntry> {
        let evidence = to_json(&entry.evidence)?;
        let undo = entry.undo.as_ref().map(to_json).transpose()?;
        let mut tx = self.pool.begin_write().await?;
        sqlx::query(
            "INSERT INTO supervisor_actions(incident_key, session_ref, kind, summary, basis,
                 evidence_json, undo_json, live, created_at, updated_at)
             VALUES(?,?,?,?,?,?,?,?,?,?)
             ON CONFLICT(incident_key) DO UPDATE SET
                 kind = excluded.kind, summary = excluded.summary, basis = excluded.basis,
                 evidence_json = excluded.evidence_json, undo_json = excluded.undo_json,
                 live = excluded.live, updated_at = excluded.updated_at",
        )
        .bind(&entry.incident_key)
        .bind(entry.session_ref.as_ref().map(SessionRef::as_str))
        .bind(kind_text(&entry)?)
        .bind(&entry.summary)
        .bind(&entry.basis)
        .bind(evidence)
        .bind(undo)
        .bind(entry.live)
        .bind(entry.created_at_ms)
        .bind(entry.updated_at_ms)
        .execute(&mut *tx)
        .await?;
        let row: Row = sqlx::query_as(&format!(
            "SELECT {COLUMNS} FROM supervisor_actions WHERE incident_key = ?"
        ))
        .bind(&entry.incident_key)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        entry_of(row)
    }

    async fn list_today(&self, since_ms: i64) -> Result<Vec<ActionEntry>> {
        let rows: Vec<Row> = sqlx::query_as(&format!(
            "SELECT {COLUMNS} FROM supervisor_actions WHERE updated_at >= ?
             ORDER BY updated_at DESC, id DESC"
        ))
        .bind(since_ms)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(entry_of).collect()
    }

    async fn mark_undone(&self, id: i64, now_ms: i64) -> Result<()> {
        let done = sqlx::query(
            "UPDATE supervisor_actions SET undone_at = ?, undo_json = NULL, updated_at = ?
             WHERE id = ?",
        )
        .bind(now_ms)
        .bind(now_ms)
        .bind(id)
        .execute(&self.pool)
        .await?;
        if done.rows_affected() == 0 {
            return Err(HubError::message(format!("no supervisor action {id}")));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::supervisor::contract::{ActionKind, EvidenceRef, UndoHandle};

    fn entry(summary: &str, live: bool, at_ms: i64) -> ActionEntry {
        ActionEntry {
            id: 0,
            incident_key: "retry:h1:s1:42".to_string(),
            session_ref: Some(SessionRef::new("h1", "s1")),
            kind: ActionKind::Retried,
            summary: summary.to_string(),
            basis: "upstream 529".to_string(),
            evidence: vec![EvidenceRef {
                kind: "receipt".to_string(),
                id: "r1".to_string(),
            }],
            undo: Some(UndoHandle {
                kind: "stop".to_string(),
                payload: serde_json::json!({"sessionRef": "h1:s1"}),
            }),
            live,
            created_at_ms: at_ms,
            updated_at_ms: at_ms,
        }
    }

    async fn log() -> (SqliteActionLog, SqlitePool) {
        let pool = crate::storage::memory().await.unwrap();
        (SqliteActionLog::new(pool.clone()), pool)
    }

    #[tokio::test]
    async fn three_upserts_of_one_incident_leave_one_row_with_the_latest_summary() {
        let (log, pool) = log().await;
        let first = log
            .upsert_incident(entry("retrying in 1m", true, 1_000))
            .await
            .unwrap();
        log.upsert_incident(entry("retrying in 2m", true, 2_000))
            .await
            .unwrap();
        let last = log
            .upsert_incident(entry("resumed after retry", false, 3_000))
            .await
            .unwrap();

        let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM supervisor_actions")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(rows, 1);
        assert_eq!(last.id, first.id);
        assert_eq!(last.summary, "resumed after retry");
        assert!(!last.live);
        assert_eq!(last.created_at_ms, 1_000);
        assert_eq!(last.updated_at_ms, 3_000);
        assert_eq!(last.kind, ActionKind::Retried);
        assert_eq!(last.session_ref, Some(SessionRef::new("h1", "s1")));
        assert_eq!(last.evidence.len(), 1);
        assert_eq!(log.list_today(0).await.unwrap(), vec![last]);
    }

    #[tokio::test]
    async fn list_today_is_newest_first_and_bounded_by_since() {
        let (log, _pool) = log().await;
        log.upsert_incident(entry("old", false, 1_000))
            .await
            .unwrap();
        let mut newer = entry("newer", false, 5_000);
        newer.incident_key = "other".to_string();
        log.upsert_incident(newer).await.unwrap();

        let all = log.list_today(0).await.unwrap();
        let summaries: Vec<&str> = all.iter().map(|e| e.summary.as_str()).collect();
        assert_eq!(summaries, ["newer", "old"]);
        assert_eq!(log.list_today(2_000).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn mark_undone_sets_undone_at_and_clears_the_undo_handle() {
        let (log, pool) = log().await;
        let stored = log
            .upsert_incident(entry("retrying", true, 1_000))
            .await
            .unwrap();
        assert!(stored.undo.is_some());

        log.mark_undone(stored.id, 9_000).await.unwrap();

        let undone_at: Option<i64> =
            sqlx::query_scalar("SELECT undone_at FROM supervisor_actions WHERE id = ?")
                .bind(stored.id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(undone_at, Some(9_000));
        let listed = log.list_today(0).await.unwrap();
        assert_eq!(listed[0].undo, None);
        assert!(log.mark_undone(stored.id + 100, 9_000).await.is_err());
    }
}
