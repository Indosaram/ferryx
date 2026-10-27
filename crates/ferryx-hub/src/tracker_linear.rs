//! Linear-style tracker API. The web UI at `~/.ferryx/hub/web` calls these routes with no bearer
//! token, matching omo-gateway. The owner list stays on `hub_tracker_issues`.
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::SqlitePool;

use crate::error::{HubError, Result};
use crate::storage::now_rfc3339;

pub fn router() -> Router<SqlitePool> {
    Router::new()
        .route("/bootstrap", get(bootstrap))
        .route("/issues", get(list_issues).post(create_issue))
        .route("/issues/batch", post(batch_issues))
        .route(
            "/issues/{id}",
            get(issue_detail).patch(patch_issue).delete(delete_issue),
        )
        .route("/issues/{id}/comments", post(add_comment))
        .route(
            "/comments/{id}",
            axum::routing::patch(patch_comment).delete(delete_comment),
        )
        .route("/comments/{id}/reactions", post(toggle_reaction))
        .route("/issues/{id}/relations", post(add_relation))
        .route("/relations/{id}", axum::routing::delete(delete_relation))
        .route("/issues/{id}/links", post(add_link))
        .route("/links/{id}", axum::routing::delete(delete_link))
        .route("/issues/{id}/subscribe", post(subscribe))
        .route("/issues/{id}/delegate", post(delegate))
        .route("/issues/{id}/lane-action", post(lane_action))
        .route("/projects", get(list_projects).post(create_project))
        .route(
            "/projects/{id}",
            get(get_project).patch(patch_project).delete(delete_project),
        )
        .route("/projects/{id}/updates", post(add_project_update))
        .route(
            "/project-updates/{id}",
            axum::routing::patch(patch_row).delete(delete_named),
        )
        .route("/projects/{id}/milestones", post(add_milestone))
        .route(
            "/milestones/{id}",
            axum::routing::patch(patch_row).delete(delete_named),
        )
        .route("/cycles", post(create_cycle))
        .route(
            "/cycles/{id}",
            axum::routing::patch(patch_row).delete(delete_named),
        )
        .route("/labels", post(create_label))
        .route(
            "/labels/{id}",
            axum::routing::patch(patch_row).delete(delete_named),
        )
        .route("/teams", post(create_team))
        .route("/teams/{id}", axum::routing::patch(patch_team))
        .route("/states", post(create_state))
        .route(
            "/states/{id}",
            axum::routing::patch(patch_row).delete(delete_named),
        )
        .route("/users", post(create_user))
        .route("/users/{id}", axum::routing::patch(patch_user))
        .route("/views", post(create_view))
        .route(
            "/views/{id}",
            axum::routing::patch(patch_row).delete(delete_named),
        )
        .route("/favorites", post(toggle_favorite))
        .route("/notifications", get(list_notifications))
        .route("/notifications/read-all", post(read_all_notifications))
        .route(
            "/notifications/{id}",
            axum::routing::patch(patch_notification),
        )
        .route("/search", get(search))
        .route("/sessions", get(list_sessions))
        .route("/sessions/{machine}/{session_id}/send", post(session_send))
        .route(
            "/sessions/{machine}/{session_id}/interrupt",
            post(session_interrupt),
        )
        .route(
            "/sessions/{machine}/{session_id}/answer",
            post(session_answer),
        )
}

fn ok(value: Value) -> (StatusCode, Json<Value>) {
    (StatusCode::OK, Json(value))
}

fn fail(status: StatusCode, message: impl Into<String>) -> (StatusCode, Json<Value>) {
    (status, Json(json!({"error": message.into()})))
}

async fn bootstrap(State(pool): State<SqlitePool>) -> (StatusCode, Json<Value>) {
    match bootstrap_value(&pool).await {
        Ok(value) => ok(value),
        Err(error) => fail(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    }
}

/// omo's `api::prepare`: inbox requests are mirrored before the tracker is read.
async fn prepare(pool: &SqlitePool) {
    if let Err(error) = crate::tracker_lanes::sync(pool).await {
        tracing::warn!(%error, "tracker lane sync failed");
    }
    if let Err(error) = crate::tracker_requests::sync(pool).await {
        tracing::warn!(%error, "tracker request sync failed");
    }
    crate::tracker_sessions::sync_issues(pool).await;
}

#[cfg(test)]
pub(crate) async fn issue_values_for_test(pool: &SqlitePool) -> Vec<Value> {
    list_issue_values(pool, None, false).await.unwrap()
}

async fn bootstrap_value(pool: &SqlitePool) -> Result<Value> {
    prepare(pool).await;
    let users = sqlx::query(
        "SELECT id, name, display_name, email, avatar_color, kind, model, is_me, created_at FROM tracker_users ORDER BY created_at",
    )
    .fetch_all(pool)
    .await?;
    let mut user_json = Vec::new();
    let mut me = None;
    for row in users {
        let value = user_json_row(&row)?;
        if value["is_me"] == true {
            me = Some(value.clone());
        }
        user_json.push(value);
    }
    let me = me.unwrap_or_else(|| {
        json!({
            "id": "local",
            "name": "local",
            "display_name": "Local",
            "email": null,
            "avatar_color": "#5b5bd6",
            "kind": "human",
            "model": null,
            "is_me": true,
            "created_at": now_rfc3339()
        })
    });
    let teams = rows_json(pool, "SELECT id, key, name, icon, color, issue_seq, system, cycles_enabled, cycle_weeks, triage_enabled, sort_order, created_at FROM tracker_teams ORDER BY sort_order").await?;
    let states = rows_json(pool, "SELECT id, team_id, name, type, color, position, lane_status, description, session_state, lane_paused FROM tracker_states ORDER BY position").await?;
    let labels = rows_json(pool, "SELECT id, team_id, name, color, parent_id, description, created_at FROM tracker_labels ORDER BY name").await?;
    let projects = projects_json(pool).await?;
    let cycles = rows_json(pool, "SELECT id, team_id, number, name, description, starts_at, ends_at, completed_at FROM tracker_cycles ORDER BY starts_at").await?;
    let milestones = rows_json(pool, "SELECT id, project_id, name, target_date, sort_order FROM tracker_milestones ORDER BY sort_order").await?;
    let views = rows_json(pool, "SELECT id, name, description, icon, color, owner_id, team_id, filters, display, shared, created_at, updated_at FROM tracker_views ORDER BY name").await?;
    let favorites = rows_json(pool, "SELECT id, user_id, kind, target_id, sort_order FROM tracker_favorites ORDER BY sort_order").await?;
    let unread: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tracker_notifications WHERE read_at IS NULL AND archived_at IS NULL",
    )
    .fetch_one(pool)
    .await?;
    Ok(json!({
        "me": me,
        "users": user_json,
        "teams": teams,
        "states": states,
        "labels": labels,
        "projects": projects,
        "cycles": cycles,
        "milestones": milestones,
        "views": views,
        "favorites": favorites,
        "unread_count": unread
    }))
}

fn user_json_row(row: &sqlx::sqlite::SqliteRow) -> Result<Value> {
    use sqlx::Row;
    Ok(json!({
        "id": row.try_get::<String, _>("id")?,
        "name": row.try_get::<String, _>("name")?,
        "display_name": row.try_get::<String, _>("display_name")?,
        "email": row.try_get::<Option<String>, _>("email")?,
        "avatar_color": row.try_get::<String, _>("avatar_color")?,
        "kind": row.try_get::<String, _>("kind")?,
        "model": row.try_get::<Option<String>, _>("model")?,
        "is_me": row.try_get::<i64, _>("is_me")? != 0,
        "created_at": row.try_get::<String, _>("created_at")?,
    }))
}

async fn rows_json(pool: &SqlitePool, sql: &str) -> Result<Vec<Value>> {
    use sqlx::Column;
    use sqlx::Row;
    let rows = sqlx::query(sql).fetch_all(pool).await?;
    let mut out = Vec::new();
    for row in rows {
        let mut object = serde_json::Map::new();
        for column in row.columns() {
            let name = column.name().to_owned();
            let value = json_cell(&row, &name)?;
            object.insert(name, value);
        }
        out.push(Value::Object(object));
    }
    Ok(out)
}

fn json_cell(row: &sqlx::sqlite::SqliteRow, name: &str) -> Result<Value> {
    use sqlx::Row;
    if let Ok(value) = row.try_get::<Option<String>, _>(name) {
        return Ok(value.map(Value::String).unwrap_or(Value::Null));
    }
    if let Ok(value) = row.try_get::<Option<i64>, _>(name) {
        return Ok(value.map(|number| json!(number)).unwrap_or(Value::Null));
    }
    if let Ok(value) = row.try_get::<Option<f64>, _>(name) {
        return Ok(value.map(|number| json!(number)).unwrap_or(Value::Null));
    }
    Err(HubError::message(format!(
        "tracker column {name} has an unsupported type"
    )))
}

async fn projects_json(pool: &SqlitePool) -> Result<Vec<Value>> {
    let mut projects = rows_json(
        pool,
        "SELECT id, name, description, summary, icon, color, status, priority, lead_id, start_date, target_date, sort_order, created_at, updated_at, archived_at FROM tracker_projects ORDER BY sort_order",
    )
    .await?;
    for project in &mut projects {
        let id = project["id"].as_str().unwrap_or("").to_owned();
        let teams: Vec<String> =
            sqlx::query_scalar("SELECT team_id FROM tracker_project_teams WHERE project_id = ?")
                .bind(&id)
                .fetch_all(pool)
                .await?;
        let members: Vec<String> =
            sqlx::query_scalar("SELECT user_id FROM tracker_project_members WHERE project_id = ?")
                .bind(&id)
                .fetch_all(pool)
                .await?;
        let issue_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM tracker_issues WHERE project_id = ? AND archived_at IS NULL",
        )
        .bind(&id)
        .fetch_one(pool)
        .await?;
        let completed: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM tracker_issues i JOIN tracker_states s ON s.id = i.state_id WHERE i.project_id = ? AND s.type = 'completed'",
        )
        .bind(&id)
        .fetch_one(pool)
        .await?;
        project["team_ids"] = json!(teams);
        project["member_ids"] = json!(members);
        project["issue_count"] = json!(issue_count);
        project["completed_issue_count"] = json!(completed);
        project["latest_health"] = Value::Null;
    }
    Ok(projects)
}

#[derive(Deserialize)]
struct IssueQuery {
    team_id: Option<String>,
    #[serde(default)]
    include_archived: bool,
}

async fn list_issues(
    State(pool): State<SqlitePool>,
    Query(query): Query<IssueQuery>,
) -> (StatusCode, Json<Value>) {
    match list_issue_values(&pool, query.team_id.as_deref(), query.include_archived).await {
        Ok(items) => ok(json!({"items": items})),
        Err(error) => fail(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    }
}

async fn list_issue_values(
    pool: &SqlitePool,
    team_id: Option<&str>,
    include_archived: bool,
) -> Result<Vec<Value>> {
    prepare(pool).await;
    let rows: Vec<String> = if let Some(team) = team_id {
        sqlx::query_scalar(
            "SELECT id FROM tracker_issues WHERE team_id = ? ORDER BY sort_order, number",
        )
        .bind(team)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_scalar("SELECT id FROM tracker_issues ORDER BY sort_order, number")
            .fetch_all(pool)
            .await?
    };
    let mut items = Vec::new();
    for id in rows {
        let issue = load_issue(pool, &id).await?;
        if !include_archived && !issue["archived_at"].is_null() {
            continue;
        }
        items.push(issue);
    }
    Ok(items)
}

async fn load_issue(pool: &SqlitePool, id: &str) -> Result<Value> {
    use sqlx::{Column, Row};
    let row = sqlx::query(
        "SELECT id, team_id, number, identifier, title, description, state_id, priority, estimate, assignee_id, creator_id, project_id, milestone_id, cycle_id, parent_id, due_date, sort_order, lane_key, created_at, updated_at, started_at, completed_at, canceled_at, archived_at, snoozed_until, session_ref FROM tracker_issues WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| HubError::message(format!("issue {id} was not found")))?;
    let mut object = serde_json::Map::new();
    for column in row.columns() {
        let name = column.name().to_owned();
        object.insert(name.clone(), json_cell(&row, &name)?);
    }
    let labels: Vec<String> =
        sqlx::query_scalar("SELECT label_id FROM tracker_issue_labels WHERE issue_id = ?")
            .bind(id)
            .fetch_all(pool)
            .await?;
    let subscribers: Vec<String> =
        sqlx::query_scalar("SELECT user_id FROM tracker_subscribers WHERE issue_id = ?")
            .bind(id)
            .fetch_all(pool)
            .await?;
    let sub_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM tracker_issues WHERE parent_id = ?")
            .bind(id)
            .fetch_one(pool)
            .await?;
    let sub_done: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tracker_issues i JOIN tracker_states s ON s.id = i.state_id WHERE i.parent_id = ? AND s.type = 'completed'",
    )
    .bind(id)
    .fetch_one(pool)
    .await?;
    let comments: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM tracker_comments WHERE issue_id = ?")
            .bind(id)
            .fetch_one(pool)
            .await?;
    object.insert("label_ids".into(), json!(labels));
    object.insert("subscriber_ids".into(), json!(subscribers));
    object.insert("sub_issue_count".into(), json!(sub_count));
    object.insert("sub_issue_completed".into(), json!(sub_done));
    object.insert("comment_count".into(), json!(comments));
    object.insert("lane".into(), Value::Null);
    let session = match object.get("session_ref").and_then(Value::as_str) {
        Some(reference) => {
            let state: Option<Option<String>> =
                sqlx::query_scalar("SELECT session_state FROM tracker_states WHERE id = ?")
                    .bind(
                        object
                            .get("state_id")
                            .and_then(Value::as_str)
                            .unwrap_or_default(),
                    )
                    .fetch_optional(pool)
                    .await?;
            crate::tracker_sessions::session_summary(reference, state.flatten().as_deref())
                .unwrap_or(Value::Null)
        }
        None => Value::Null,
    };
    object.insert("session".into(), session);
    Ok(Value::Object(object))
}

async fn create_issue(
    State(pool): State<SqlitePool>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    match create_issue_row(&pool, &body).await {
        Ok(issue) => (StatusCode::CREATED, Json(issue)),
        Err(error) => fail(StatusCode::BAD_REQUEST, error.to_string()),
    }
}

async fn create_issue_row(pool: &SqlitePool, body: &Value) -> Result<Value> {
    let team_id = body.get("team_id").and_then(Value::as_str).unwrap_or("");
    let title = body
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if team_id.is_empty() || title.is_empty() {
        return Err(HubError::message("team_id and title are required"));
    }
    let (key, seq): (String, i64) = sqlx::query_as(
        "UPDATE tracker_teams SET issue_seq = issue_seq + 1 WHERE id = ? RETURNING key, issue_seq",
    )
    .bind(team_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| HubError::message("team was not found"))?;
    let state_id = match body.get("state_id").and_then(Value::as_str) {
        Some(state) if !state.is_empty() => state.to_owned(),
        _ => sqlx::query_scalar(
            "SELECT id FROM tracker_states WHERE team_id = ? AND type = 'unstarted' ORDER BY position LIMIT 1",
        )
        .bind(team_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| HubError::message("team has no unstarted state"))?,
    };
    let creator_id = match body.get("creator_id").and_then(Value::as_str) {
        Some(id) if !id.is_empty() => id.to_owned(),
        _ => sqlx::query_scalar(
            "SELECT id FROM tracker_users WHERE is_me = 1 ORDER BY created_at LIMIT 1",
        )
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| HubError::message("tracker has no user to own the issue"))?,
    };
    let id = uuid::Uuid::new_v4().to_string();
    let now = now_rfc3339();
    let description = body
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("");
    let priority = body.get("priority").and_then(Value::as_i64).unwrap_or(0);
    sqlx::query(
        "INSERT INTO tracker_issues(id, team_id, number, identifier, title, description, state_id, priority, creator_id, created_at, updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,?)",
    )
    .bind(&id)
    .bind(team_id)
    .bind(seq)
    .bind(format!("{key}-{seq}"))
    .bind(title)
    .bind(description)
    .bind(&state_id)
    .bind(priority)
    .bind(&creator_id)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;
    record_activity(pool, &id, Some(&creator_id), "created", None, Some(title)).await?;
    load_issue(pool, &id).await
}

async fn record_activity(
    pool: &SqlitePool,
    issue_id: &str,
    actor_id: Option<&str>,
    kind: &str,
    from_value: Option<&str>,
    to_value: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO tracker_activity(issue_id, actor_id, kind, from_value, to_value, created_at) VALUES(?,?,?,?,?,?)",
    )
    .bind(issue_id)
    .bind(actor_id)
    .bind(kind)
    .bind(from_value)
    .bind(to_value)
    .bind(now_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

async fn issue_detail(
    State(pool): State<SqlitePool>,
    Path(id): Path<String>,
) -> (StatusCode, Json<Value>) {
    match load_issue(&pool, &id).await {
        Ok(issue) => ok(issue),
        Err(error) => fail(StatusCode::NOT_FOUND, error.to_string()),
    }
}

/// omo's `timestamps_for`: the started/completed/canceled stamps an issue gets in a state.
fn timestamps_for(
    state_type: &str,
    started_at: Option<String>,
    now: &str,
) -> (Option<String>, Option<String>, Option<String>) {
    match state_type {
        "started" => (
            Some(started_at.unwrap_or_else(|| now.to_owned())),
            None,
            None,
        ),
        "completed" => (
            Some(started_at.unwrap_or_else(|| now.to_owned())),
            Some(now.to_owned()),
            None,
        ),
        "canceled" => (started_at, None, Some(now.to_owned())),
        _ => (None, None, None),
    }
}

/// omo's `lanes::handle_lane_state_transition`: moving a lane-linked issue drives its lane.
async fn lane_state_transition(
    pool: &SqlitePool,
    lane_key: &str,
    state_type: &str,
    force_note: Option<&str>,
) -> std::result::Result<(), String> {
    use crate::lanes::state::LaneStatus;
    let status: Option<String> = sqlx::query_scalar("SELECT status FROM lanes WHERE key = ?")
        .bind(lane_key)
        .fetch_optional(pool)
        .await
        .map_err(|error| error.to_string())?;
    let Some(status) = status else {
        return Err(format!("IllegalTransition: lane {lane_key} not found"));
    };
    let (target, reason) = match state_type {
        "completed" if status == "verified_done" => (LaneStatus::Closed, None),
        "completed" => match force_note.map(str::trim).filter(|note| !note.is_empty()) {
            Some(note) => (LaneStatus::Closed, Some(format!("forced: {note}"))),
            None => return Err("lane must be verified before closing; use force close".into()),
        },
        "canceled" if matches!(status.as_str(), "working" | "blocked" | "verified_done") => (
            LaneStatus::Closed,
            Some("forced: canceled from tracker".to_owned()),
        ),
        "canceled" => {
            return Err(format!(
                "IllegalTransition: lane cannot be canceled from status {status}"
            ))
        }
        "started" | "unstarted" | "backlog" if matches!(status.as_str(), "closed" | "failed") => {
            (LaneStatus::Working, Some("note: reopened".to_owned()))
        }
        "started" | "unstarted" | "backlog" => {
            return Err(format!(
                "IllegalTransition: lane is already in active status {status}"
            ))
        }
        other => {
            return Err(format!(
                "IllegalTransition: unsupported target state type {other}"
            ))
        }
    };
    if target == LaneStatus::Closed {
        // omo closes through the close intent: worktree removed, unmerged branch reported.
        crate::lanes::worktree::close(pool, lane_key, reason.as_deref())
            .await
            .map_err(|error| error.to_string())?;
    } else {
        crate::lanes::state::transition(pool, lane_key, target, reason.as_deref())
            .await
            .map_err(|error| error.to_string())?;
    }
    crate::tracker_lanes::sync(pool)
        .await
        .map_err(|error| error.to_string())
}

/// state_id, started_at, lane_key, session_ref of the issue being patched.
type PatchRow = (String, Option<String>, Option<String>, Option<String>);

async fn patch_issue(
    State(pool): State<SqlitePool>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let current: Option<PatchRow> = match sqlx::query_as(
        "SELECT state_id, started_at, lane_key, session_ref FROM tracker_issues WHERE id = ?",
    )
    .bind(&id)
    .fetch_optional(&pool)
    .await
    {
        Ok(row) => row,
        Err(error) => return fail(StatusCode::BAD_REQUEST, error.to_string()),
    };
    let Some((current_state, started_at, lane_key, session_ref)) = current else {
        return fail(StatusCode::NOT_FOUND, "issue not found");
    };
    if session_ref.is_some() && (body.get("title").is_some() || body.get("description").is_some()) {
        return fail(
            StatusCode::CONFLICT,
            "session issues mirror the live session title and description",
        );
    }
    if let Some(title) = body.get("title").and_then(Value::as_str) {
        if let Err(error) =
            sqlx::query("UPDATE tracker_issues SET title = ?, updated_at = ? WHERE id = ?")
                .bind(title)
                .bind(now_rfc3339())
                .bind(&id)
                .execute(&pool)
                .await
        {
            return fail(StatusCode::BAD_REQUEST, error.to_string());
        }
        let _ = record_activity(&pool, &id, None, "title", None, Some(title)).await;
    }
    if let Some(description) = body.get("description").and_then(Value::as_str) {
        let _ =
            sqlx::query("UPDATE tracker_issues SET description = ?, updated_at = ? WHERE id = ?")
                .bind(description)
                .bind(now_rfc3339())
                .bind(&id)
                .execute(&pool)
                .await;
    }
    if let Some(state_id) = body
        .get("state_id")
        .and_then(Value::as_str)
        .filter(|state| *state != current_state)
    {
        let state: Option<(String, i64)> =
            match sqlx::query_as("SELECT type, lane_paused FROM tracker_states WHERE id = ?")
                .bind(state_id)
                .fetch_optional(&pool)
                .await
            {
                Ok(state) => state,
                Err(error) => return fail(StatusCode::BAD_REQUEST, error.to_string()),
            };
        let Some((state_type, lane_paused)) = state else {
            return fail(StatusCode::BAD_REQUEST, "unknown state_id");
        };
        if let Some(reference) = session_ref.as_deref() {
            // Sync owns SES states; the one allowed move (Ended on a streaming session)
            // interrupts it and the re-sync records whatever state the session lands in.
            if let Err(error) =
                crate::tracker_sessions::manual_state_move(&pool, reference, state_id).await
            {
                return fail(StatusCode::CONFLICT, error.to_string());
            }
        } else {
            if let Some(key) = lane_key.as_deref() {
                if lane_paused == 1 {
                    return fail(StatusCode::CONFLICT, "Paused is set by lane pause");
                }
                let force_note = body.get("force_note").and_then(Value::as_str);
                if let Err(error) = lane_state_transition(&pool, key, &state_type, force_note).await
                {
                    return fail(StatusCode::CONFLICT, error);
                }
            }
            let now = now_rfc3339();
            let (started, completed, canceled) =
                timestamps_for(&state_type, started_at.clone(), &now);
            if let Err(error) = sqlx::query(
                "UPDATE tracker_issues SET state_id = ?, started_at = ?, completed_at = ?, canceled_at = ?, updated_at = ? WHERE id = ?",
            )
            .bind(state_id)
            .bind(&started)
            .bind(&completed)
            .bind(&canceled)
            .bind(&now)
            .bind(&id)
            .execute(&pool)
            .await
            {
                return fail(StatusCode::BAD_REQUEST, error.to_string());
            }
            let _ = record_activity(
                &pool,
                &id,
                None,
                "state",
                Some(current_state.as_str()),
                Some(state_id),
            )
            .await;
        }
    }
    match load_issue(&pool, &id).await {
        Ok(issue) => ok(issue),
        Err(error) => fail(StatusCode::NOT_FOUND, error.to_string()),
    }
}

async fn delete_issue(State(pool): State<SqlitePool>, Path(id): Path<String>) -> StatusCode {
    let reference: Option<Option<String>> =
        sqlx::query_scalar("SELECT session_ref FROM tracker_issues WHERE id = ?")
            .bind(&id)
            .fetch_optional(&pool)
            .await
            .unwrap_or_default();
    if let Some(reference) = reference.flatten() {
        // Session sync re-creates issues for live sessions; the tombstone keeps this one gone.
        if let Err(error) = crate::tracker_sessions::tombstone(&pool, &reference).await {
            tracing::warn!(%error, "session tombstone failed");
            return StatusCode::INTERNAL_SERVER_ERROR;
        }
    }
    let _ = sqlx::query("DELETE FROM tracker_issues WHERE id = ?")
        .bind(id)
        .execute(&pool)
        .await;
    StatusCode::NO_CONTENT
}

async fn add_comment(
    State(pool): State<SqlitePool>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let text = body
        .get("body")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_owned();
    if text.is_empty() {
        return fail(StatusCode::BAD_REQUEST, "comment body is required");
    }
    let user_id = match body.get("user_id").and_then(Value::as_str) {
        Some(user) if !user.is_empty() => user.to_owned(),
        _ => match sqlx::query_scalar("SELECT id FROM tracker_users WHERE is_me = 1 LIMIT 1")
            .fetch_optional(&pool)
            .await
        {
            Ok(Some(user)) => user,
            _ => return fail(StatusCode::BAD_REQUEST, "comment needs a user"),
        },
    };
    let comment_id = uuid::Uuid::new_v4().to_string();
    let now = now_rfc3339();
    if let Err(error) = sqlx::query(
        "INSERT INTO tracker_comments(id, issue_id, user_id, body, created_at, updated_at) VALUES(?,?,?,?,?,?)",
    )
    .bind(&comment_id)
    .bind(&id)
    .bind(&user_id)
    .bind(&text)
    .bind(&now)
    .bind(&now)
    .execute(&pool)
    .await
    {
        return fail(StatusCode::BAD_REQUEST, error.to_string());
    }
    let _ = record_activity(&pool, &id, Some(&user_id), "comment", None, Some(&text)).await;
    (
        StatusCode::CREATED,
        Json(
            json!({"id": comment_id, "issue_id": id, "user_id": user_id, "body": text, "created_at": now}),
        ),
    )
}

async fn patch_comment(
    State(pool): State<SqlitePool>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let text = body.get("body").and_then(Value::as_str).unwrap_or("");
    let _ = sqlx::query(
        "UPDATE tracker_comments SET body = ?, edited = 1, updated_at = ? WHERE id = ?",
    )
    .bind(text)
    .bind(now_rfc3339())
    .bind(&id)
    .execute(&pool)
    .await;
    ok(json!({"id": id, "body": text}))
}

async fn delete_comment(State(pool): State<SqlitePool>, Path(id): Path<String>) -> StatusCode {
    let _ = sqlx::query("DELETE FROM tracker_comments WHERE id = ?")
        .bind(id)
        .execute(&pool)
        .await;
    StatusCode::NO_CONTENT
}

async fn toggle_reaction(
    State(pool): State<SqlitePool>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let emoji = body
        .get("emoji")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let user_id = body
        .get("user_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    if emoji.is_empty() || user_id.is_empty() {
        return fail(StatusCode::BAD_REQUEST, "emoji and user_id are required");
    }
    let existing: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM tracker_reactions WHERE comment_id = ? AND user_id = ? AND emoji = ?",
    )
    .bind(&id)
    .bind(&user_id)
    .bind(&emoji)
    .fetch_optional(&pool)
    .await
    .ok()
    .flatten();
    if existing.is_some() {
        let _ = sqlx::query(
            "DELETE FROM tracker_reactions WHERE comment_id = ? AND user_id = ? AND emoji = ?",
        )
        .bind(&id)
        .bind(&user_id)
        .bind(&emoji)
        .execute(&pool)
        .await;
        return ok(json!({"on": false}));
    }
    let inserted =
        sqlx::query("INSERT INTO tracker_reactions(comment_id, user_id, emoji) VALUES(?,?,?)")
            .bind(&id)
            .bind(&user_id)
            .bind(&emoji)
            .execute(&pool)
            .await;
    match inserted {
        Ok(_) => ok(json!({"on": true})),
        Err(error) => fail(StatusCode::BAD_REQUEST, error.to_string()),
    }
}

async fn add_relation(
    State(pool): State<SqlitePool>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let related = body
        .get("related_issue_id")
        .and_then(Value::as_str)
        .unwrap_or("");
    let kind = body
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("related");
    let relation_id = uuid::Uuid::new_v4().to_string();
    match sqlx::query(
        "INSERT INTO tracker_relations(id, issue_id, related_issue_id, type, created_at) VALUES(?,?,?,?,?)",
    )
    .bind(&relation_id)
    .bind(&id)
    .bind(related)
    .bind(kind)
    .bind(now_rfc3339())
    .execute(&pool)
    .await
    {
        Ok(_) => {
            let _ = record_activity(&pool, &id, None, "relation", None, Some(kind)).await;
            (StatusCode::CREATED, Json(json!({"id": relation_id})))
        }
        Err(error) => fail(StatusCode::BAD_REQUEST, error.to_string()),
    }
}

async fn delete_relation(State(pool): State<SqlitePool>, Path(id): Path<String>) -> StatusCode {
    let _ = sqlx::query("DELETE FROM tracker_relations WHERE id = ?")
        .bind(id)
        .execute(&pool)
        .await;
    StatusCode::NO_CONTENT
}

async fn add_link(
    State(pool): State<SqlitePool>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let url = body.get("url").and_then(Value::as_str).unwrap_or("");
    let title = body.get("title").and_then(Value::as_str).unwrap_or(url);
    let link_id = uuid::Uuid::new_v4().to_string();
    match sqlx::query(
        "INSERT INTO tracker_links(id, issue_id, url, title, created_at) VALUES(?,?,?,?,?)",
    )
    .bind(&link_id)
    .bind(&id)
    .bind(url)
    .bind(title)
    .bind(now_rfc3339())
    .execute(&pool)
    .await
    {
        Ok(_) => (StatusCode::CREATED, Json(json!({"id": link_id}))),
        Err(error) => fail(StatusCode::BAD_REQUEST, error.to_string()),
    }
}

async fn delete_link(State(pool): State<SqlitePool>, Path(id): Path<String>) -> StatusCode {
    let _ = sqlx::query("DELETE FROM tracker_links WHERE id = ?")
        .bind(id)
        .execute(&pool)
        .await;
    StatusCode::NO_CONTENT
}

async fn subscribe(
    State(pool): State<SqlitePool>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let user_id = body.get("user_id").and_then(Value::as_str).unwrap_or("");
    let inserted =
        sqlx::query("INSERT OR IGNORE INTO tracker_subscribers(issue_id, user_id) VALUES(?,?)")
            .bind(&id)
            .bind(user_id)
            .execute(&pool)
            .await;
    match inserted {
        Ok(_) => ok(json!({"issue_id": id, "user_id": user_id})),
        Err(error) => fail(StatusCode::BAD_REQUEST, error.to_string()),
    }
}

async fn delegate(
    State(pool): State<SqlitePool>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let assignee = body
        .get("assignee_id")
        .and_then(Value::as_str)
        .unwrap_or("");
    let cwd = body.get("cwd").and_then(Value::as_str).unwrap_or("").trim();
    let done = body
        .get("done")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if assignee.is_empty() && (cwd.is_empty() || done.is_empty()) {
        return fail(
            StatusCode::BAD_REQUEST,
            "assignee_id, or cwd and done, is required",
        );
    }
    if !assignee.is_empty() {
        if let Err(error) =
            sqlx::query("UPDATE tracker_issues SET assignee_id = ?, updated_at = ? WHERE id = ?")
                .bind(assignee)
                .bind(now_rfc3339())
                .bind(&id)
                .execute(&pool)
                .await
        {
            return fail(StatusCode::BAD_REQUEST, error.to_string());
        }
        if let Err(error) =
            record_activity(&pool, &id, None, "delegate", None, Some(assignee)).await
        {
            return fail(StatusCode::INTERNAL_SERVER_ERROR, error.to_string());
        }
    }
    if !cwd.is_empty() && !done.is_empty() {
        match open_delegated_lane(
            &pool,
            &id,
            cwd,
            done,
            body.get("thread").and_then(Value::as_str),
        )
        .await
        {
            Ok(lane_key) => {
                if let Err(error) =
                    record_activity(&pool, &id, None, "delegated", None, Some(&lane_key)).await
                {
                    return fail(StatusCode::INTERNAL_SERVER_ERROR, error.to_string());
                }
            }
            Err(error) => return fail(StatusCode::CONFLICT, error.to_string()),
        }
    }
    match load_issue(&pool, &id).await {
        Ok(issue) => ok(issue),
        Err(error) => fail(StatusCode::NOT_FOUND, error.to_string()),
    }
}

async fn open_delegated_lane(
    pool: &SqlitePool,
    issue_id: &str,
    cwd: &str,
    done: &str,
    thread: Option<&str>,
) -> Result<String> {
    let row: Option<(String, String, String, Option<String>)> = sqlx::query_as(
        "SELECT identifier, title, COALESCE(description, ''), lane_key FROM tracker_issues WHERE id = ?",
    )
    .bind(issue_id)
    .fetch_optional(pool)
    .await?;
    let Some((identifier, title, description, lane_key)) = row else {
        return Err(crate::error::HubError::message("issue not found"));
    };
    if lane_key.is_some() {
        return Err(crate::error::HubError::message(
            "issue already delegated to a lane",
        ));
    }
    let lane_key = sanitize_lane_key(&format!(
        "{}-{:04x}",
        identifier.to_lowercase(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.subsec_nanos() & 0xffff)
            .unwrap_or(1)
    ));
    let brief_dir = crate::hub_env::var("OMON_TRACKER_BRIEF_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("ferryx-tracker-briefs"));
    tokio::fs::create_dir_all(&brief_dir)
        .await
        .map_err(|error| crate::error::HubError::message(error.to_string()))?;
    let brief_path = brief_dir.join(format!("{lane_key}.md"));
    tokio::fs::write(
        &brief_path,
        format!("# {identifier}: {title}\n\n{description}"),
    )
    .await
    .map_err(|error| crate::error::HubError::message(error.to_string()))?;
    crate::lanes::state::insert_lane(pool, &lane_key, &title, "tracker", cwd, done)
        .await
        .map_err(|error| crate::error::HubError::message(error.to_string()))?;
    if let Some(thread) = thread.filter(|value| crate::lanes::threads::parse_ref(value).is_some()) {
        sqlx::query("UPDATE lanes SET thread_channel_id = ? WHERE key = ?")
            .bind(thread)
            .bind(&lane_key)
            .execute(pool)
            .await?;
    }
    sqlx::query("UPDATE tracker_issues SET lane_key = ?, updated_at = ? WHERE id = ?")
        .bind(&lane_key)
        .bind(now_rfc3339())
        .bind(issue_id)
        .execute(pool)
        .await?;
    let _ = brief_path;
    Ok(lane_key)
}

fn sanitize_lane_key(key: &str) -> String {
    let mut out = String::new();
    for ch in key.chars() {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' {
            out.push(ch);
        } else if ch.is_ascii_uppercase() {
            out.push(ch.to_ascii_lowercase());
        }
    }
    if out.is_empty() {
        out.push_str("lane-1");
    }
    if out.len() > 48 {
        out.truncate(48);
    }
    out
}

async fn lane_action(
    State(pool): State<SqlitePool>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let action = body.get("action").and_then(Value::as_str).unwrap_or("note");
    let note = body
        .get("note")
        .or_else(|| body.get("text"))
        .and_then(Value::as_str)
        .unwrap_or(action);
    if matches!(action, "send" | "object" | "close" | "reopen") {
        let lane_key: Option<String> =
            match sqlx::query_scalar("SELECT lane_key FROM tracker_issues WHERE id = ?")
                .bind(&id)
                .fetch_optional(&pool)
                .await
            {
                Ok(lane_key) => lane_key,
                Err(error) => return fail(StatusCode::BAD_REQUEST, error.to_string()),
            };
        let Some(lane_key) = lane_key.filter(|key| !key.is_empty()) else {
            return fail(StatusCode::CONFLICT, "issue is not linked to a lane");
        };
        let force = body.get("force").and_then(Value::as_bool).unwrap_or(false);
        let reason = match action {
            "close" if force => Some(if note.starts_with("forced:") {
                note.to_owned()
            } else if note.is_empty() {
                "forced: closed from tracker".into()
            } else {
                format!("forced: {note}")
            }),
            "reopen" => Some("note: reopened".into()),
            _ => Some(note.to_owned()),
        };
        let target = match action {
            "send" | "reopen" => crate::lanes::state::LaneStatus::Working,
            "object" => crate::lanes::state::LaneStatus::Blocked,
            "close" => crate::lanes::state::LaneStatus::Closed,
            _ => crate::lanes::state::LaneStatus::Working,
        };
        if let Err(error) =
            crate::lanes::state::transition(&pool, &lane_key, target, reason.as_deref()).await
        {
            return fail(StatusCode::CONFLICT, error.to_string());
        }
        let kind = if action == "object" {
            "blocked"
        } else {
            "lane_action"
        };
        let _ = sqlx::query(
            "INSERT INTO lane_events(lane_key, lead_session_key, kind, body, created_at) VALUES(?,?,?,?,?)",
        )
        .bind(&lane_key)
        .bind("tracker")
        .bind(kind)
        .bind(note)
        .bind(now_rfc3339())
        .execute(&pool)
        .await;
    }
    if let Err(error) = record_activity(&pool, &id, None, "lane_action", None, Some(note)).await {
        return fail(StatusCode::BAD_REQUEST, error.to_string());
    }
    ok(json!({"id": id, "action": action}))
}

async fn batch_issues(
    State(pool): State<SqlitePool>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let ids = body
        .get("ids")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut items = Vec::new();
    for id in ids {
        if let Some(id) = id.as_str() {
            if let Ok(issue) = load_issue(&pool, id).await {
                items.push(issue);
            }
        }
    }
    ok(json!({"items": items}))
}

async fn list_projects(State(pool): State<SqlitePool>) -> (StatusCode, Json<Value>) {
    match projects_json(&pool).await {
        Ok(items) => ok(json!({"items": items})),
        Err(error) => fail(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    }
}

async fn create_project(
    State(pool): State<SqlitePool>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let name = body
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_owned();
    if name.is_empty() {
        return fail(StatusCode::BAD_REQUEST, "name is required");
    }
    let id = uuid::Uuid::new_v4().to_string();
    let now = now_rfc3339();
    let color = body
        .get("color")
        .and_then(Value::as_str)
        .unwrap_or("#5b5bd6");
    match sqlx::query(
        "INSERT INTO tracker_projects(id, name, color, status, created_at, updated_at) VALUES(?,?,?,'planned',?,?)",
    )
    .bind(&id)
    .bind(&name)
    .bind(color)
    .bind(&now)
    .bind(&now)
    .execute(&pool)
    .await
    {
        Ok(_) => (StatusCode::CREATED, Json(json!({"id": id, "name": name}))),
        Err(error) => fail(StatusCode::BAD_REQUEST, error.to_string()),
    }
}

async fn get_project(
    State(pool): State<SqlitePool>,
    Path(id): Path<String>,
) -> (StatusCode, Json<Value>) {
    match projects_json(&pool).await {
        Ok(items) => items
            .into_iter()
            .find(|item| item["id"] == id)
            .map(|project| ok(json!({"project": project, "updates": [], "milestones": [], "member_ids": project_or_empty(&project, "member_ids"), "team_ids": project_or_empty(&project, "team_ids")})))
            .unwrap_or_else(|| fail(StatusCode::NOT_FOUND, "project was not found")),
        Err(error) => fail(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    }
}

fn project_or_empty(project: &Value, key: &str) -> Value {
    project.get(key).cloned().unwrap_or_else(|| json!([]))
}

async fn patch_project(
    State(pool): State<SqlitePool>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    if let Some(name) = body.get("name").and_then(Value::as_str) {
        let _ = sqlx::query("UPDATE tracker_projects SET name = ?, updated_at = ? WHERE id = ?")
            .bind(name)
            .bind(now_rfc3339())
            .bind(&id)
            .execute(&pool)
            .await;
    }
    ok(json!({"id": id}))
}

async fn delete_project(State(pool): State<SqlitePool>, Path(id): Path<String>) -> StatusCode {
    let _ = sqlx::query("DELETE FROM tracker_projects WHERE id = ?")
        .bind(id)
        .execute(&pool)
        .await;
    StatusCode::NO_CONTENT
}

async fn add_project_update(
    State(pool): State<SqlitePool>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let update_id = uuid::Uuid::new_v4().to_string();
    let user_id = body.get("user_id").and_then(Value::as_str).unwrap_or("");
    let health = body
        .get("health")
        .and_then(Value::as_str)
        .unwrap_or("on_track");
    let text = body.get("body").and_then(Value::as_str).unwrap_or("");
    let now = now_rfc3339();
    match sqlx::query(
        "INSERT INTO tracker_project_updates(id, project_id, user_id, health, body, created_at, updated_at) VALUES(?,?,?,?,?,?,?)",
    )
    .bind(&update_id)
    .bind(&id)
    .bind(user_id)
    .bind(health)
    .bind(text)
    .bind(&now)
    .bind(&now)
    .execute(&pool)
    .await
    {
        Ok(_) => (StatusCode::CREATED, Json(json!({"id": update_id}))),
        Err(error) => fail(StatusCode::BAD_REQUEST, error.to_string()),
    }
}

async fn add_milestone(
    State(pool): State<SqlitePool>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let milestone_id = uuid::Uuid::new_v4().to_string();
    let name = body.get("name").and_then(Value::as_str).unwrap_or("");
    match sqlx::query(
        "INSERT INTO tracker_milestones(id, project_id, name, target_date) VALUES(?,?,?,?)",
    )
    .bind(&milestone_id)
    .bind(&id)
    .bind(name)
    .bind(body.get("target_date").and_then(Value::as_str))
    .execute(&pool)
    .await
    {
        Ok(_) => (
            StatusCode::CREATED,
            Json(json!({"id": milestone_id, "name": name})),
        ),
        Err(error) => fail(StatusCode::BAD_REQUEST, error.to_string()),
    }
}

async fn create_cycle(
    State(pool): State<SqlitePool>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let id = uuid::Uuid::new_v4().to_string();
    let team_id = body.get("team_id").and_then(Value::as_str).unwrap_or("");
    let number = body.get("number").and_then(Value::as_i64).unwrap_or(1);
    let starts = body.get("starts_at").and_then(Value::as_str).unwrap_or("");
    let ends = body.get("ends_at").and_then(Value::as_str).unwrap_or("");
    match sqlx::query("INSERT INTO tracker_cycles(id, team_id, number, name, starts_at, ends_at) VALUES(?,?,?,?,?,?)")
        .bind(&id)
        .bind(team_id)
        .bind(number)
        .bind(body.get("name").and_then(Value::as_str))
        .bind(starts)
        .bind(ends)
        .execute(&pool)
        .await
    {
        Ok(_) => (StatusCode::CREATED, Json(json!({"id": id}))),
        Err(error) => fail(StatusCode::BAD_REQUEST, error.to_string()),
    }
}

async fn create_label(
    State(pool): State<SqlitePool>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let id = uuid::Uuid::new_v4().to_string();
    let name = body.get("name").and_then(Value::as_str).unwrap_or("");
    let color = body.get("color").and_then(Value::as_str).unwrap_or("#888");
    match sqlx::query(
        "INSERT INTO tracker_labels(id, team_id, name, color, created_at) VALUES(?,?,?,?,?)",
    )
    .bind(&id)
    .bind(body.get("team_id").and_then(Value::as_str))
    .bind(name)
    .bind(color)
    .bind(now_rfc3339())
    .execute(&pool)
    .await
    {
        Ok(_) => (StatusCode::CREATED, Json(json!({"id": id}))),
        Err(error) => fail(StatusCode::BAD_REQUEST, error.to_string()),
    }
}

async fn create_team(
    State(pool): State<SqlitePool>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let id = uuid::Uuid::new_v4().to_string();
    let key = body.get("key").and_then(Value::as_str).unwrap_or("");
    let name = body.get("name").and_then(Value::as_str).unwrap_or(key);
    match sqlx::query(
        "INSERT INTO tracker_teams(id, key, name, icon, color, created_at) VALUES(?,?,?,?,?,?)",
    )
    .bind(&id)
    .bind(key)
    .bind(name)
    .bind(body.get("icon").and_then(Value::as_str).unwrap_or("box"))
    .bind(
        body.get("color")
            .and_then(Value::as_str)
            .unwrap_or("#5b5bd6"),
    )
    .bind(now_rfc3339())
    .execute(&pool)
    .await
    {
        Ok(_) => (StatusCode::CREATED, Json(json!({"id": id, "key": key}))),
        Err(error) => fail(StatusCode::BAD_REQUEST, error.to_string()),
    }
}

async fn patch_team(
    State(pool): State<SqlitePool>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    if let Some(name) = body.get("name").and_then(Value::as_str) {
        let _ = sqlx::query("UPDATE tracker_teams SET name = ? WHERE id = ?")
            .bind(name)
            .bind(&id)
            .execute(&pool)
            .await;
    }
    ok(json!({"id": id}))
}

async fn create_state(
    State(pool): State<SqlitePool>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let id = uuid::Uuid::new_v4().to_string();
    match sqlx::query(
        "INSERT INTO tracker_states(id, team_id, name, type, color) VALUES(?,?,?,?,?)",
    )
    .bind(&id)
    .bind(body.get("team_id").and_then(Value::as_str).unwrap_or(""))
    .bind(body.get("name").and_then(Value::as_str).unwrap_or(""))
    .bind(
        body.get("type")
            .and_then(Value::as_str)
            .unwrap_or("unstarted"),
    )
    .bind(body.get("color").and_then(Value::as_str).unwrap_or("#888"))
    .execute(&pool)
    .await
    {
        Ok(_) => (StatusCode::CREATED, Json(json!({"id": id}))),
        Err(error) => fail(StatusCode::BAD_REQUEST, error.to_string()),
    }
}

async fn create_user(
    State(pool): State<SqlitePool>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let id = uuid::Uuid::new_v4().to_string();
    let name = body.get("name").and_then(Value::as_str).unwrap_or("");
    let display = body
        .get("display_name")
        .and_then(Value::as_str)
        .unwrap_or(name);
    match sqlx::query(
        "INSERT INTO tracker_users(id, name, display_name, email, avatar_color, kind, created_at) VALUES(?,?,?,?,?,?,?)",
    )
    .bind(&id)
    .bind(name)
    .bind(display)
    .bind(body.get("email").and_then(Value::as_str))
    .bind(body.get("avatar_color").and_then(Value::as_str).unwrap_or("#888"))
    .bind(body.get("kind").and_then(Value::as_str).unwrap_or("human"))
    .bind(now_rfc3339())
    .execute(&pool)
    .await
    {
        Ok(_) => (StatusCode::CREATED, Json(json!({"id": id, "name": name, "display_name": display}))),
        Err(error) => fail(StatusCode::BAD_REQUEST, error.to_string()),
    }
}

async fn patch_user(
    State(pool): State<SqlitePool>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    if let Some(display) = body.get("display_name").and_then(Value::as_str) {
        let _ = sqlx::query("UPDATE tracker_users SET display_name = ? WHERE id = ?")
            .bind(display)
            .bind(&id)
            .execute(&pool)
            .await;
    }
    ok(json!({"id": id}))
}

async fn create_view(
    State(pool): State<SqlitePool>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let id = uuid::Uuid::new_v4().to_string();
    let now = now_rfc3339();
    match sqlx::query(
        "INSERT INTO tracker_views(id, name, owner_id, created_at, updated_at) VALUES(?,?,?,?,?)",
    )
    .bind(&id)
    .bind(body.get("name").and_then(Value::as_str).unwrap_or("View"))
    .bind(body.get("owner_id").and_then(Value::as_str).unwrap_or(""))
    .bind(&now)
    .bind(&now)
    .execute(&pool)
    .await
    {
        Ok(_) => (StatusCode::CREATED, Json(json!({"id": id}))),
        Err(error) => fail(StatusCode::BAD_REQUEST, error.to_string()),
    }
}

async fn toggle_favorite(
    State(pool): State<SqlitePool>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let user_id = body.get("user_id").and_then(Value::as_str).unwrap_or("");
    let kind = body.get("kind").and_then(Value::as_str).unwrap_or("issue");
    let target = body.get("target_id").and_then(Value::as_str).unwrap_or("");
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT id FROM tracker_favorites WHERE user_id = ? AND kind = ? AND target_id = ?",
    )
    .bind(user_id)
    .bind(kind)
    .bind(target)
    .fetch_optional(&pool)
    .await
    .ok()
    .flatten();
    if let Some(id) = existing {
        let _ = sqlx::query("DELETE FROM tracker_favorites WHERE id = ?")
            .bind(&id)
            .execute(&pool)
            .await;
        return ok(json!({"on": false, "id": id}));
    }
    let id = uuid::Uuid::new_v4().to_string();
    match sqlx::query("INSERT INTO tracker_favorites(id, user_id, kind, target_id) VALUES(?,?,?,?)")
        .bind(&id)
        .bind(user_id)
        .bind(kind)
        .bind(target)
        .execute(&pool)
        .await
    {
        Ok(_) => ok(json!({"on": true, "id": id})),
        Err(error) => fail(StatusCode::BAD_REQUEST, error.to_string()),
    }
}

async fn list_notifications(State(pool): State<SqlitePool>) -> (StatusCode, Json<Value>) {
    match rows_json(&pool, "SELECT id, user_id, issue_id, project_id, kind, actor_id, body, created_at, read_at, snoozed_until, archived_at FROM tracker_notifications ORDER BY created_at DESC").await {
        Ok(items) => ok(json!({"items": items})),
        Err(error) => fail(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    }
}

async fn read_all_notifications(State(pool): State<SqlitePool>) -> (StatusCode, Json<Value>) {
    let _ = sqlx::query("UPDATE tracker_notifications SET read_at = ? WHERE read_at IS NULL")
        .bind(now_rfc3339())
        .execute(&pool)
        .await;
    ok(json!({"ok": true}))
}

async fn patch_notification(
    State(pool): State<SqlitePool>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    if body.get("read").and_then(Value::as_bool) == Some(true) {
        let _ = sqlx::query("UPDATE tracker_notifications SET read_at = ? WHERE id = ?")
            .bind(now_rfc3339())
            .bind(&id)
            .execute(&pool)
            .await;
    }
    ok(json!({"id": id}))
}

async fn search(
    State(pool): State<SqlitePool>,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> (StatusCode, Json<Value>) {
    let q = query.get("q").map(String::as_str).unwrap_or("");
    let like = format!("%{q}%");
    match sqlx::query_scalar::<_, String>(
        "SELECT id FROM tracker_issues WHERE title LIKE ? OR description LIKE ? OR identifier LIKE ? ORDER BY updated_at DESC LIMIT 50",
    )
    .bind(&like)
    .bind(&like)
    .bind(&like)
    .fetch_all(&pool)
    .await
    {
        Ok(ids) => {
            let mut items = Vec::new();
            for id in ids {
                if let Ok(issue) = load_issue(&pool, &id).await {
                    items.push(issue);
                }
            }
            ok(json!({"items": items}))
        }
        Err(error) => fail(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    }
}

/// omo's sessions board: every live agent pane on this machine and paired hosts.
async fn list_sessions(State(pool): State<SqlitePool>) -> (StatusCode, Json<Value>) {
    ok(crate::tracker_sessions::board(&pool).await)
}

fn session_result(result: Result<Value>) -> (StatusCode, Json<Value>) {
    match result {
        Ok(result) => ok(json!({"ok": true, "result": result})),
        Err(error) => fail(StatusCode::BAD_REQUEST, error.to_string()),
    }
}

async fn session_send(
    State(pool): State<SqlitePool>,
    Path((machine, session_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let text = body.get("text").and_then(Value::as_str).unwrap_or("");
    session_result(crate::tracker_sessions::send(&pool, &machine, &session_id, text).await)
}

async fn session_interrupt(
    State(pool): State<SqlitePool>,
    Path((machine, session_id)): Path<(String, String)>,
) -> (StatusCode, Json<Value>) {
    session_result(crate::tracker_sessions::interrupt(&pool, &machine, &session_id).await)
}

async fn session_answer(
    State(pool): State<SqlitePool>,
    Path((machine, session_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let text = body.get("text").and_then(Value::as_str).unwrap_or("");
    let request = body.get("request_id").and_then(Value::as_str);
    session_result(
        crate::tracker_sessions::answer(&pool, &machine, &session_id, text, request).await,
    )
}

async fn patch_row() -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn delete_named() -> StatusCode {
    StatusCode::NO_CONTENT
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    async fn seed(pool: &SqlitePool) {
        sqlx::query("INSERT INTO tracker_users(id, name, display_name, avatar_color, kind, is_me, created_at) VALUES('u1','ada','Ada','#000','human',1,'2020-01-01T00:00:00Z')")
            .execute(pool).await.unwrap();
        sqlx::query("INSERT INTO tracker_teams(id, key, name, icon, color, created_at) VALUES('t1','AGT','Agents','bot','#fff','2020-01-01T00:00:00Z')")
            .execute(pool).await.unwrap();
        sqlx::query("INSERT INTO tracker_states(id, team_id, name, type, color) VALUES('s1','t1','Todo','unstarted','#fff')")
            .execute(pool).await.unwrap();
        sqlx::query("INSERT INTO tracker_users(id, name, display_name, avatar_color, kind, is_me, created_at) VALUES('u2','bot','Bot','#111','agent',0,'2020-01-01T00:00:00Z')")
            .execute(pool).await.unwrap();
    }

    #[tokio::test]
    async fn bootstrap_create_comment_delegate_and_lane_action() {
        let pool = crate::storage::memory().await.unwrap();
        seed(&pool).await;
        let app = router().with_state(pool.clone());
        let boot = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/bootstrap")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(boot.status(), StatusCode::OK);
        let created = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/issues")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"team_id":"t1","title":"Import the ledger","description":"body"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(created.status(), StatusCode::CREATED);
        let bytes = axum::body::to_bytes(created.into_body(), 64 * 1024)
            .await
            .unwrap();
        let issue: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(issue["identifier"], "AGT-1");
        assert_eq!(issue["description"], "body");
        let id = issue["id"].as_str().unwrap();
        let comment = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/issues/{id}/comments"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"body":"noted"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(comment.status(), StatusCode::CREATED);
        let delegated = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/issues/{id}/delegate"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"assignee_id":"u2"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(delegated.status(), StatusCode::OK);
        let action = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/issues/{id}/lane-action"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"action":"pause","note":"waiting"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(action.status(), StatusCode::OK);
        let kinds: Vec<String> =
            sqlx::query_scalar("SELECT kind FROM tracker_activity WHERE issue_id = ? ORDER BY id")
                .bind(id)
                .fetch_all(&pool)
                .await
                .unwrap();
        assert!(kinds.contains(&"created".to_string()));
        assert!(kinds.contains(&"comment".to_string()));
        assert!(kinds.contains(&"delegate".to_string()));
        assert!(kinds.contains(&"lane_action".to_string()));
    }

    async fn call(app: &Router, method: &str, uri: &str, body: &str) -> (StatusCode, Value) {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_owned()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 256 * 1024)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    #[tokio::test]
    async fn session_issues_are_sync_owned_and_deleting_one_tombstones_it() {
        let pool = crate::storage::memory().await.unwrap();
        seed(&pool).await;
        let live = vec![(
            "local".to_owned(),
            Ok(vec![crate::tracker_sessions::LiveSession::from_entry(
                &json!({"sessionId": "abc", "agent": "claude", "state": "idle"}),
            )
            .unwrap()]),
        )];
        crate::tracker_sessions::sync_from_snapshots(&pool, &live, chrono::Utc::now())
            .await
            .unwrap();
        let (id, ended): (String, String) = sqlx::query_as(
            "SELECT i.id, s.id FROM tracker_issues i JOIN tracker_states s ON s.team_id = i.team_id AND s.session_state = 'ended' WHERE i.session_ref = 'local:abc'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let app = router().with_state(pool.clone());
        let (status, issue) = call(&app, "GET", &format!("/issues/{id}"), "").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(issue["session"]["session_id"], "abc");
        assert_eq!(issue["session"]["state"], "idle");
        let (status, _) = call(&app, "PATCH", &format!("/issues/{id}"), r#"{"title":"x"}"#).await;
        assert_eq!(status, StatusCode::CONFLICT);
        // Ended is only reachable by interrupting a streaming session; an idle one is refused.
        let (status, _) = call(
            &app,
            "PATCH",
            &format!("/issues/{id}"),
            &json!({"state_id": ended}).to_string(),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        let (status, _) = call(&app, "DELETE", &format!("/issues/{id}"), "").await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        crate::tracker_sessions::sync_from_snapshots(&pool, &live, chrono::Utc::now())
            .await
            .unwrap();
        let recreated: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM tracker_issues WHERE session_ref = 'local:abc'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(recreated, 0);
    }

    #[tokio::test]
    async fn moving_a_lane_issue_drives_its_lane_and_stamps_the_issue() {
        let pool = crate::storage::memory().await.unwrap();
        seed(&pool).await;
        for (id, kind, paused) in [("s-done", "completed", 0), ("s-paused", "started", 1)] {
            sqlx::query("INSERT INTO tracker_states(id, team_id, name, type, color, lane_paused) VALUES(?, 't1', ?, ?, '#fff', ?)")
                .bind(id).bind(id).bind(kind).bind(paused)
                .execute(&pool).await.unwrap();
        }
        sqlx::query(
            "INSERT INTO lanes(key, title, lead_session_key, cwd, session_key, status, done_signals, last_activity_at)
             VALUES('ledger', 'Ledger', 'lead', '/tmp', 'lane', 'working', '[]', '2020-01-01T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO tracker_issues(id, team_id, number, identifier, title, state_id, priority, sort_order, lane_key, creator_id, created_at, updated_at)
             VALUES('i1', 't1', 1, 'AGT-1', 'Ledger', 's1', 0, 0, 'ledger', 'u1', '2020', '2020')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let app = router().with_state(pool.clone());
        let lane_status = || async {
            sqlx::query_scalar::<_, String>("SELECT status FROM lanes WHERE key = 'ledger'")
                .fetch_one(&pool)
                .await
                .unwrap()
        };
        let (status, _) = call(&app, "PATCH", "/issues/i1", r#"{"state_id":"s-paused"}"#).await;
        assert_eq!(status, StatusCode::CONFLICT);
        let (status, body) = call(&app, "PATCH", "/issues/i1", r#"{"state_id":"s-done"}"#).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(body["error"].as_str().unwrap().contains("verified"));
        assert_eq!(lane_status().await, "working");
        let (status, issue) = call(
            &app,
            "PATCH",
            "/issues/i1",
            r#"{"state_id":"s-done","force_note":"shipped by hand"}"#,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(issue["state_id"], "s-done");
        assert!(issue["completed_at"].is_string());
        assert!(issue["started_at"].is_string());
        assert_eq!(lane_status().await, "closed");
        let (status, issue) = call(&app, "PATCH", "/issues/i1", r#"{"state_id":"s1"}"#).await;
        assert_eq!(status, StatusCode::OK);
        assert!(issue["completed_at"].is_null());
        assert_eq!(lane_status().await, "working");
    }
}
