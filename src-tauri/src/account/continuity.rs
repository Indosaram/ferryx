//! Account-wide selection intent, not session ownership. A conflict is final for that intent:
//! clients may publish again only after a fresh explicit selection, never by rebasing a 409.

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::service::{authenticate_user, parse_json, run_account_blocking, AccountState, ApiError};
use super::store::{now_secs, store_sqlite};

/// Each continuity field is accepted up to 4096 bytes, and a request carries four of them, so
/// the router-wide 4 KiB body limit would refuse as `413` what this handler types as
/// `400 INVALID_CONTINUITY`. The route therefore raises the limit enough for the field checks
/// to be the thing that decides validity, while still bounding the read.
pub const MAX_BODY_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ContinuityPointer {
    pub machine_id: Option<String>,
    pub workspace_id: Option<String>,
    pub worktree_slug: Option<String>,
    pub session_id: Option<String>,
    pub revision: i64,
    pub updated_at: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PutContinuity {
    pub expected_revision: i64,
    pub machine_id: Option<String>,
    pub workspace_id: Option<String>,
    pub worktree_slug: Option<String>,
    pub session_id: Option<String>,
}

impl PutContinuity {
    fn validate(&self) -> Result<(), ApiError> {
        let fields = [&self.machine_id, &self.workspace_id, &self.worktree_slug, &self.session_id];
        let cleared = fields.iter().all(|field| field.is_none());
        let selected = [&self.machine_id, &self.workspace_id, &self.session_id].iter().all(|field| {
            field.as_ref().is_some_and(|value| !value.trim().is_empty() && value.len() <= 4096)
        }) && self.worktree_slug.as_ref().map_or(true, |value| {
            !value.trim().is_empty() && value.len() <= 4096
        });
        if self.expected_revision < 0 || (!cleared && !selected) {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "INVALID_CONTINUITY",
                "expectedRevision must be nonnegative; machineId, workspaceId and sessionId must be nonempty strings with worktreeSlug null or nonempty, or all target fields must be null",
            ));
        }
        Ok(())
    }
}

fn current(conn: &Connection, user_id: &str) -> Result<Option<ContinuityPointer>, ApiError> {
    // Replicas predating v4 deliberately do not migrate on open.
    if !store_sqlite::codec::table_exists(conn, "continuity_pointers")? {
        return Ok(None);
    }
    conn.query_row(
        "SELECT machine_id, workspace_id, worktree_slug, session_id, revision, updated_at
         FROM continuity_pointers WHERE user_id = ?1",
        [user_id],
        |row| Ok(ContinuityPointer {
            machine_id: row.get(0)?,
            workspace_id: row.get(1)?,
            worktree_slug: row.get(2)?,
            session_id: row.get(3)?,
            revision: row.get(4)?,
            updated_at: row.get(5)?,
        }),
    )
    .optional()
    .map_err(|error| ApiError::internal(format!("Failed to read continuity pointer: {error}")))
}

fn compare_and_set(
    conn: &Connection,
    user_id: &str,
    request: PutContinuity,
) -> Result<ContinuityPointer, ApiError> {
    request.validate()?;
    let previous = current(conn, user_id)?;
    let revision = previous.as_ref().map_or(0, |pointer| pointer.revision);
    if request.expected_revision != revision {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "CONTINUITY_CONFLICT",
            "another device published newer selection intent",
        ).with_details(serde_json::json!({ "current": previous })));
    }
    let revision = revision.checked_add(1).ok_or_else(|| {
        ApiError::internal("continuity revision exhausted")
    })?;
    let pointer = ContinuityPointer {
        machine_id: request.machine_id,
        workspace_id: request.workspace_id,
        worktree_slug: request.worktree_slug,
        session_id: request.session_id,
        revision,
        updated_at: now_secs(),
    };
    conn.execute(
        "INSERT INTO continuity_pointers
         (user_id, machine_id, workspace_id, worktree_slug, session_id, revision, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(user_id) DO UPDATE SET machine_id = excluded.machine_id,
         workspace_id = excluded.workspace_id, worktree_slug = excluded.worktree_slug,
         session_id = excluded.session_id, revision = excluded.revision, updated_at = excluded.updated_at",
        params![user_id, pointer.machine_id, pointer.workspace_id, pointer.worktree_slug,
            pointer.session_id, pointer.revision, pointer.updated_at],
    ).map_err(|error| ApiError::internal(format!("Failed to write continuity pointer: {error}")))?;
    Ok(pointer)
}

pub async fn get(
    State(state): State<Arc<AccountState>>,
    headers: HeaderMap,
) -> Result<Json<Option<ContinuityPointer>>, ApiError> {
    run_account_blocking(move || {
        let user = authenticate_user(&state, &headers)?;
        store_sqlite::lease_read(&state.data_dir, |conn| current(conn, &user.user_id))
    }).await.map(Json)
}

pub async fn put(
    State(state): State<Arc<AccountState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<ContinuityPointer>, ApiError> {
    let auth_state = Arc::clone(&state);
    let user = run_account_blocking(move || authenticate_user(&auth_state, &headers)).await?;
    let request: PutContinuity = parse_json(body).await?;
    // Reuse the targeted BEGIN IMMEDIATE writer (including standby refusal and the shared
    // writer lock); whole-store saves deliberately leave this table alone.
    run_account_blocking(move || {
        store_sqlite::lease_transaction(&state.data_dir, |tx| {
            compare_and_set(tx, &user.user_id, request)
        })
    }).await.map(Json)
}

#[cfg(test)]
#[path = "continuity_tests.rs"]
mod tests;
