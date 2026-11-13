//! `/api/agent/*`: the hub's lanes, inbox requests, agent sessions and owner locale as plain
//! JSON for an external tracker (openlinear), which mirrors them into its own issues. No
//! tracker concept lives here: no issue, state or team, and no `tracker_*` table is read or
//! written. The spec a client codes against is `.evidence/openlinear-split-20261111/HUB_API.md`.
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::{Row, SqlitePool};

use crate::agent_control::AgentControl;
use crate::error::{HubError, Result};
use crate::sessions::live::Snapshot;

#[derive(Clone)]
pub struct AgentState {
    pool: SqlitePool,
    /// Tests inject a control; the hub uses the one `sessions::live::install` set, read per
    /// request because it is installed after the router is built.
    control: Option<Arc<dyn AgentControl>>,
}

impl AgentState {
    fn control(&self) -> Option<Arc<dyn AgentControl>> {
        self.control
            .clone()
            .or_else(crate::sessions::live::installed)
    }
}

pub fn router<S: Clone + Send + Sync + 'static>(pool: SqlitePool) -> Router<S> {
    router_with(pool, None)
}

fn router_with<S: Clone + Send + Sync + 'static>(
    pool: SqlitePool,
    control: Option<Arc<dyn AgentControl>>,
) -> Router<S> {
    Router::new()
        .route("/lanes", get(list_lanes).post(create_lane))
        .route("/lanes/{key}", get(lane_detail))
        .route("/lanes/{key}/action", post(lane_action))
        .route("/requests", get(requests))
        .route("/sessions", get(sessions))
        .route("/sessions/{machine}/{session_id}/send", post(session_send))
        .route(
            "/sessions/{machine}/{session_id}/interrupt",
            post(session_interrupt),
        )
        .route(
            "/sessions/{machine}/{session_id}/answer",
            post(session_answer),
        )
        .route("/locale", get(locale))
        .with_state(AgentState { pool, control })
}

type Reply = (StatusCode, Json<Value>);

fn fail(status: StatusCode, message: impl Into<String>) -> Reply {
    (status, Json(json!({"error": message.into()})))
}

fn internal(error: crate::error::HubError) -> Reply {
    fail(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

// ---------------------------------------------------------------------------------------
// Lanes
// ---------------------------------------------------------------------------------------

const LANE_COLUMNS: &str = "key, title, cwd, brief_path, status, created_at";

/// Lane statuses the tracker's lane mirror notifies the owner about.
const NOTIFY_STATUSES: [&str; 4] = ["blocked", "done_claimed", "verified_done", "failed"];
/// Transitions kept per lane, newest last.
const TRANSITION_LIMIT: i64 = 20;

/// The last [`TRANSITION_LIMIT`] real status changes into a [`NOTIFY_STATUSES`] status
/// (pause/resume rows keep the status and are left out), oldest first.
async fn transitions(pool: &SqlitePool, key: &str) -> Result<Vec<Value>> {
    let mut rows: Vec<(String, Option<String>, String, Option<String>)> = sqlx::query_as(
        "SELECT at, from_status, to_status, reason FROM lane_history
          WHERE lane_key = ? AND to_status IN (?, ?, ?, ?) AND from_status IS NOT to_status
          ORDER BY id DESC LIMIT ?",
    )
    .bind(key)
    .bind(NOTIFY_STATUSES[0])
    .bind(NOTIFY_STATUSES[1])
    .bind(NOTIFY_STATUSES[2])
    .bind(NOTIFY_STATUSES[3])
    .bind(TRANSITION_LIMIT)
    .fetch_all(pool)
    .await?;
    rows.reverse();
    Ok(rows
        .into_iter()
        .map(|(at, from, to, reason)| json!({"at": at, "from_status": from, "to_status": to, "reason": reason}))
        .collect())
}

/// A lane as the tracker's lane mirror reads it: the row, whether an unresumed pause holds it,
/// the brief text `read_brief_content` returns, and its notifying transitions. `model` is
/// always null: lanes store none and the mirror's agent user falls back to "agent".
async fn lane_value(
    pool: &SqlitePool,
    row: &sqlx::sqlite::SqliteRow,
    paused: &std::collections::HashSet<String>,
) -> Result<Value> {
    let key: String = row.get("key");
    let brief_path: String = row.get("brief_path");
    let brief = crate::lanes::actions::read_brief_content(&brief_path).await;
    let transitions = transitions(pool, &key).await?;
    Ok(json!({
        "key": key,
        "title": row.get::<String, _>("title"),
        "cwd": row.get::<String, _>("cwd"),
        "brief_path": brief_path,
        "brief": brief,
        "model": Value::Null,
        "status": row.get::<String, _>("status"),
        "paused": paused.contains(&key),
        "created_at": row.get::<String, _>("created_at"),
        "transitions": transitions,
    }))
}

async fn load_lane(pool: &SqlitePool, key: &str) -> Result<Option<Value>> {
    let row = sqlx::query(&format!("SELECT {LANE_COLUMNS} FROM lanes WHERE key = ?"))
        .bind(key)
        .fetch_optional(pool)
        .await?;
    let Some(row) = row else { return Ok(None) };
    let paused = crate::lanes::pause::paused_keys(pool).await?;
    Ok(Some(lane_value(pool, &row, &paused).await?))
}

async fn lanes_value(pool: &SqlitePool) -> Result<Value> {
    let rows = sqlx::query(&format!(
        "SELECT {LANE_COLUMNS} FROM lanes ORDER BY created_at ASC"
    ))
    .fetch_all(pool)
    .await?;
    let paused = crate::lanes::pause::paused_keys(pool).await?;
    let mut lanes = Vec::with_capacity(rows.len());
    for row in &rows {
        lanes.push(lane_value(pool, row, &paused).await?);
    }
    Ok(json!({ "lanes": lanes }))
}

async fn list_lanes(State(state): State<AgentState>) -> Reply {
    match lanes_value(&state.pool).await {
        Ok(value) => (StatusCode::OK, Json(value)),
        Err(error) => internal(error),
    }
}

/// Columns the tracker's lane panel reads that the hub's `lanes` table does not have; served
/// as null so the panel's shape holds.
const PANEL_ONLY_COLUMNS: [&str; 4] = ["model", "objection", "last_turn_outcome", "last_report"];

/// One JSON cell: text, integer, real or null.
fn json_cell(row: &sqlx::sqlite::SqliteRow, name: &str) -> Result<Value> {
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

/// Every row of `sql` (one `?` bound to `bind`) as a JSON object keyed by column name.
async fn rows_json_bound(pool: &SqlitePool, sql: &str, bind: &str) -> Result<Vec<Value>> {
    use sqlx::Column;
    let rows = sqlx::query(sql).bind(bind).fetch_all(pool).await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let mut object = serde_json::Map::new();
        for column in row.columns() {
            let name = column.name().to_owned();
            object.insert(name.clone(), json_cell(&row, &name)?);
        }
        out.push(Value::Object(object));
    }
    Ok(out)
}

/// The issue page's lane panel: every `lanes` column, all `lane_history` rows, and the last
/// 200 `lane_transcript` and `lane_events` rows, each ascending by id. None for an unknown lane.
async fn lane_detail_value(pool: &SqlitePool, key: &str) -> Result<Option<Value>> {
    let lane = rows_json_bound(pool, "SELECT * FROM lanes WHERE key = ?", key)
        .await?
        .into_iter()
        .next();
    let Some(Value::Object(mut lane)) = lane else {
        return Ok(None);
    };
    for column in PANEL_ONLY_COLUMNS {
        lane.entry(column).or_insert(Value::Null);
    }
    let history = rows_json_bound(
        pool,
        "SELECT id, at, from_status, to_status, reason FROM lane_history WHERE lane_key = ? ORDER BY id",
        key,
    )
    .await?;
    let transcript = rows_json_bound(
        pool,
        "SELECT id, at, role, text FROM (SELECT id, at, role, text FROM lane_transcript
          WHERE lane_key = ? ORDER BY id DESC LIMIT 200) ORDER BY id",
        key,
    )
    .await?;
    let events = rows_json_bound(
        pool,
        "SELECT id, kind, body, created_at FROM (SELECT id, kind, body, created_at FROM lane_events
          WHERE lane_key = ? ORDER BY id DESC LIMIT 200) ORDER BY id",
        key,
    )
    .await?;
    Ok(Some(json!({
        "lane": Value::Object(lane),
        "history": history,
        "transcript": transcript,
        "events": events,
    })))
}

async fn lane_detail(State(state): State<AgentState>, Path(key): Path<String>) -> Reply {
    match lane_detail_value(&state.pool, &key).await {
        Ok(Some(detail)) => (StatusCode::OK, Json(detail)),
        Ok(None) => fail(StatusCode::NOT_FOUND, format!("lane {key} was not found")),
        Err(error) => internal(error),
    }
}

#[derive(Deserialize)]
struct NewLane {
    #[serde(default)]
    title: String,
    #[serde(default)]
    brief: String,
    #[serde(default)]
    cwd: String,
    #[serde(default)]
    done: String,
    /// The requester's Slack thread ref (`slack:<channel>:<ts>`); ignored unless valid.
    thread: Option<String>,
    /// Key prefix; the hub appends `-<4 hex>`. Defaults to "lane".
    key_base: Option<String>,
}

/// The lane side of a tracker delegation: opens a queued lane, writes its brief, binds the
/// thread. 201 with the lane.
async fn create_lane(State(state): State<AgentState>, Json(body): Json<NewLane>) -> Reply {
    let title = body.title.trim();
    let cwd = body.cwd.trim();
    let done = body.done.trim();
    if title.is_empty() || cwd.is_empty() || done.is_empty() {
        return fail(StatusCode::BAD_REQUEST, "title, cwd and done are required");
    }
    let key_base = body
        .key_base
        .as_deref()
        .map(str::trim)
        .filter(|base| !base.is_empty())
        .unwrap_or("lane")
        .to_lowercase();
    let key = match crate::lanes::actions::open_lane(
        &state.pool,
        &key_base,
        title,
        &body.brief,
        cwd,
        done,
        body.thread.as_deref(),
    )
    .await
    {
        Ok(key) => key,
        Err(error) => return fail(StatusCode::CONFLICT, error.to_string()),
    };
    match load_lane(&state.pool, &key).await {
        Ok(Some(lane)) => (StatusCode::CREATED, Json(json!({ "lane": lane }))),
        Ok(None) => fail(StatusCode::INTERNAL_SERVER_ERROR, "the new lane vanished"),
        Err(error) => internal(error),
    }
}

#[derive(Deserialize)]
struct LaneActionBody {
    #[serde(default)]
    action: String,
    note: Option<String>,
    text: Option<String>,
    #[serde(default)]
    force: bool,
}

/// The lane side of a tracker lane action: send / object / close / reopen.
async fn lane_action(
    State(state): State<AgentState>,
    Path(key): Path<String>,
    Json(body): Json<LaneActionBody>,
) -> Reply {
    let action = body.action.as_str();
    if !crate::lanes::actions::is_lane_action(action) {
        return fail(
            StatusCode::BAD_REQUEST,
            "action must be send, object, close or reopen",
        );
    }
    match load_lane(&state.pool, &key).await {
        Ok(Some(_)) => {}
        Ok(None) => return fail(StatusCode::NOT_FOUND, format!("lane {key} was not found")),
        Err(error) => return internal(error),
    }
    // Ownership is refused before the transition is judged, as the tracker lane-action does:
    // an owned lane answers SUPERVISOR_OWNED whatever the action would have done.
    match crate::supervisor::supervisor_owns_lane(&state.pool, &key).await {
        Ok(false) => {}
        Ok(true) => {
            return fail(
                StatusCode::CONFLICT,
                format!(
                    "{}: lane {key} belongs to a supervisor goal; act on it through the supervisor",
                    crate::supervisor::SUPERVISOR_OWNED
                ),
            )
        }
        Err(error) => return fail(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    }
    let note = body.note.or(body.text).unwrap_or_else(|| action.to_owned());
    if let Err(error) =
        crate::lanes::actions::apply_lane_action(&state.pool, &key, action, &note, body.force)
            .await
    {
        return fail(StatusCode::CONFLICT, error);
    }
    match load_lane(&state.pool, &key).await {
        Ok(Some(lane)) => (StatusCode::OK, Json(json!({ "lane": lane }))),
        Ok(None) => fail(StatusCode::NOT_FOUND, format!("lane {key} was not found")),
        Err(error) => internal(error),
    }
}

// ---------------------------------------------------------------------------------------
// Requests and locale
// ---------------------------------------------------------------------------------------

const REQUESTERS_ENV: &str = "TRACKER_REQUESTERS";

/// Requester names from `TRACKER_REQUESTERS` (comma separated); each matches inbox authors
/// containing it. Unset means no requesters.
fn requesters() -> Vec<String> {
    crate::hub_env::var(REQUESTERS_ENV)
        .unwrap_or_default()
        .split(',')
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .collect()
}

fn locale_value() -> Value {
    let locale = crate::locale::current();
    json!({
        "language": locale.language,
        "korean": locale.is_korean(),
        "timezone": locale.timezone.name(),
    })
}

async fn locale() -> Reply {
    (StatusCode::OK, Json(locale_value()))
}

/// What the tracker's request mirror reads: every `bot`-classed inbox item whose author
/// contains a requester name, one entry per (requester, item) match, oldest first per requester.
async fn requests_value(pool: &SqlitePool, names: &[String]) -> Result<Value> {
    let mut items = Vec::new();
    for name in names {
        let rows = sqlx::query(
            "SELECT i.id, i.source, i.author, i.snippet, i.received_at, d.class, d.status, d.reason,
                    d.due_at, d.proposed_action
               FROM inbox_items i
               JOIN inbox_decisions d ON d.item_id = i.id
              WHERE instr(coalesce(i.author, ''), ?) > 0 AND d.class = 'bot'
              ORDER BY i.received_at",
        )
        .bind(name)
        .fetch_all(pool)
        .await?;
        for row in rows {
            items.push(json!({
                "item_id": row.get::<String, _>("id"),
                "requester": name,
                "source": row.get::<String, _>("source"),
                "author": row.get::<Option<String>, _>("author"),
                "snippet": row.get::<String, _>("snippet"),
                "received_at": row.get::<String, _>("received_at"),
                "class": row.get::<String, _>("class"),
                "status": row.get::<String, _>("status"),
                "reason": row.get::<String, _>("reason"),
                "due_at": row.get::<Option<String>, _>("due_at"),
                "proposed_action": row.get::<Option<String>, _>("proposed_action"),
            }));
        }
    }
    Ok(json!({
        "requesters": names.iter().map(|name| json!({"name": name, "label": name})).collect::<Vec<_>>(),
        "locale": locale_value(),
        "items": items,
    }))
}

async fn requests(State(state): State<AgentState>) -> Reply {
    match requests_value(&state.pool, &requesters()).await {
        Ok(value) => (StatusCode::OK, Json(value)),
        Err(error) => internal(error),
    }
}

// ---------------------------------------------------------------------------------------
// Sessions
// ---------------------------------------------------------------------------------------

fn snapshot_value((machine, snapshot): &Snapshot) -> Value {
    match snapshot {
        Ok(sessions) => json!({"machine": machine, "ok": true, "error": null, "sessions": sessions}),
        Err(error) => json!({"machine": machine, "ok": false, "error": error, "sessions": []}),
    }
}

#[derive(Deserialize)]
struct SessionsQuery {
    refresh: Option<String>,
}

async fn sessions(State(state): State<AgentState>, Query(query): Query<SessionsQuery>) -> Reply {
    let refresh = query
        .refresh
        .as_deref()
        .is_some_and(|value| !matches!(value, "" | "0" | "false"));
    let snapshots = match state.control() {
        Some(control) => {
            crate::sessions::live::snapshots_on(control.as_ref(), &state.pool, refresh).await
        }
        None => Vec::new(),
    };
    (
        StatusCode::OK,
        Json(json!({
            "snapshots": snapshots.iter().map(snapshot_value).collect::<Vec<_>>(),
            "board": crate::sessions::live::shape(&snapshots, chrono::Utc::now()),
        })),
    )
}

/// The session action result shape the tracker's Sessions board reads.
fn session_result(result: Result<Value>) -> Reply {
    match result {
        Ok(result) => (StatusCode::OK, Json(json!({"ok": true, "result": result}))),
        Err(error) => fail(StatusCode::BAD_REQUEST, error.to_string()),
    }
}

fn no_control() -> Reply {
    fail(StatusCode::BAD_REQUEST, "agent control is not available")
}

async fn session_send(
    State(state): State<AgentState>,
    Path((machine, session_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Reply {
    let Some(control) = state.control() else {
        return no_control();
    };
    let text = body.get("text").and_then(Value::as_str).unwrap_or("");
    session_result(
        crate::sessions::live::send_on(control.as_ref(), &state.pool, &machine, &session_id, text)
            .await,
    )
}

async fn session_interrupt(
    State(state): State<AgentState>,
    Path((machine, session_id)): Path<(String, String)>,
) -> Reply {
    let Some(control) = state.control() else {
        return no_control();
    };
    session_result(
        crate::sessions::live::interrupt_on(control.as_ref(), &state.pool, &machine, &session_id)
            .await,
    )
}

async fn session_answer(
    State(state): State<AgentState>,
    Path((machine, session_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Reply {
    let Some(control) = state.control() else {
        return no_control();
    };
    let text = body.get("text").and_then(Value::as_str).unwrap_or("");
    let request = body
        .get("request_id")
        .or_else(|| body.get("request"))
        .and_then(Value::as_str);
    session_result(
        crate::sessions::live::answer_on(
            control.as_ref(),
            &state.pool,
            &machine,
            &session_id,
            text,
            request,
        )
        .await,
    )
}

#[cfg(test)]
mod tests;
