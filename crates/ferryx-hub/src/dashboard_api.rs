//! The omo-gateway dashboard API the omo web UI (`~/.ferryx/hub/web`) calls on the tracker port.
//!
//! omo's `src/dashboard.rs` is the spec: the Host/Origin guard, session detail, messages and
//! delete, cron job get/update/delete, and bot profile CRUD. List routes return omo's
//! `items`/`page`/`per_page`/`total` keys next to the keys ferryx already served, so both
//! readers keep working.
use crate::storage::WriteTx;
use std::net::IpAddr;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::SqlitePool;

use crate::chat::session::SessionKey;
use crate::http::HttpState;
use crate::storage::now_rfc3339;

const MAX_PAGE_SIZE: u32 = 200;
pub const CHAT_UNAVAILABLE: &str = "chat runtime is not configured";

pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    fn not_found(resource: &str) -> Self {
        Self::new(StatusCode::NOT_FOUND, format!("{resource} not found"))
    }

    fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({"error": self.message, "status": self.status.as_u16()})),
        )
            .into_response()
    }
}

type ApiResult<T> = Result<T, ApiError>;

// ---------------------------------------------------------------------------------------
// Host / Origin guard (omo `validate_host_and_origin`)
// ---------------------------------------------------------------------------------------

/// Rejects a request whose Host is not loopback (DNS rebinding), a WebSocket upgrade from a
/// foreign Origin, and a state-changing request carrying a foreign Origin (CSRF from a page
/// the owner merely visited). The public tracker proxy rewrites Host and Origin to loopback.
pub async fn validate_host_and_origin(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let host_str = match request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
    {
        Some(host) => host.to_owned(),
        None => match request.uri().authority() {
            Some(authority) => authority.as_str().to_owned(),
            None => return (StatusCode::BAD_REQUEST, "missing Host header").into_response(),
        },
    };
    if !is_loopback_host(extract_host(&host_str)) {
        return (StatusCode::FORBIDDEN, "untrusted Host header").into_response();
    }
    let origin = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let is_ws = request
        .headers()
        .get(header::UPGRADE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("websocket"));
    if is_ws {
        let Some(origin) = origin else {
            return (StatusCode::FORBIDDEN, "missing Origin on WebSocket upgrade").into_response();
        };
        if !is_same_origin(&origin, &host_str) {
            return (
                StatusCode::FORBIDDEN,
                "cross-origin WebSocket upgrade rejected",
            )
                .into_response();
        }
    } else if !request.method().is_safe() {
        if let Some(origin) = origin {
            if !is_same_origin(&origin, &host_str) {
                return (
                    StatusCode::FORBIDDEN,
                    "cross-origin state-changing request rejected",
                )
                    .into_response();
            }
        }
    }
    next.run(request).await
}

pub fn is_loopback_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    let host = host.trim_matches(|c| c == '[' || c == ']');
    host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

fn extract_host(host_str: &str) -> &str {
    let host_str = host_str.trim();
    if let Some(rest) = host_str.strip_prefix('[') {
        if let Some(end) = rest.find(']') {
            return &rest[..end];
        }
    }
    if host_str.matches(':').count() == 1 {
        if let Some(colon) = host_str.rfind(':') {
            return &host_str[..colon];
        }
    }
    host_str
}

fn extract_port(host_str: &str) -> Option<u16> {
    let host_str = host_str.trim();
    if let Some(rest) = host_str.strip_prefix('[') {
        if let Some(end) = rest.find(']') {
            return rest[end + 1..]
                .strip_prefix(':')
                .and_then(|port| port.parse().ok());
        }
    }
    if host_str.matches(':').count() == 1 {
        if let Some(colon) = host_str.rfind(':') {
            return host_str[colon + 1..].parse().ok();
        }
    }
    None
}

pub fn is_same_origin(origin: &str, host_header: &str) -> bool {
    let Ok(uri) = origin.parse::<axum::http::Uri>() else {
        return false;
    };
    let Some(authority) = uri.authority() else {
        return false;
    };
    if authority.as_str().eq_ignore_ascii_case(host_header) {
        return true;
    }
    let Some(origin_host) = uri.host() else {
        return false;
    };
    if !origin_host
        .trim_matches(|c| c == '[' || c == ']')
        .eq_ignore_ascii_case(extract_host(host_header))
    {
        return false;
    }
    let origin_port = uri.port_u16().or(match uri.scheme_str() {
        Some("http") | Some("ws") => Some(80),
        Some("https") | Some("wss") => Some(443),
        _ => None,
    });
    origin_port == extract_port(host_header)
}

// ---------------------------------------------------------------------------------------
// Paging and search
// ---------------------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
pub struct PageQuery {
    page: Option<u32>,
    per_page: Option<u32>,
    search: Option<String>,
}

impl PageQuery {
    fn values(&self) -> (u32, u32, i64) {
        let page = self.page.unwrap_or(1).max(1);
        let per_page = self.per_page.unwrap_or(50).clamp(1, MAX_PAGE_SIZE);
        let offset = i64::from((page - 1).saturating_mul(per_page));
        (page, per_page, offset)
    }

    fn pattern(&self) -> Option<String> {
        self.search
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(like_contains_pattern)
    }
}

/// `%value%` that matches `value` literally (queries use `ESCAPE '\'`).
fn like_contains_pattern(value: &str) -> String {
    let mut pattern = String::with_capacity(value.len() + 2);
    pattern.push('%');
    for ch in value.chars() {
        if matches!(ch, '%' | '_' | '\\') {
            pattern.push('\\');
        }
        pattern.push(ch);
    }
    pattern.push('%');
    pattern
}

// ---------------------------------------------------------------------------------------
// Sessions
// ---------------------------------------------------------------------------------------

type SessionRow = (
    String,
    String,
    Option<String>,
    String,
    Option<String>,
    String,
    String,
    String,
    String,
);

const SESSION_COLUMNS: &str = "session_key, platform, guild_id, channel_id, thread_id, user_id, state_json, created_at, updated_at";

fn session_view(row: SessionRow) -> Value {
    let (key, platform, guild, channel, thread, user, state, created, updated) = row;
    json!({
        "id": key,
        "session_key": key,
        "platform": platform,
        "guild_id": guild,
        "channel_id": channel,
        "thread_id": thread,
        "user_id": user,
        "state": serde_json::from_str::<Value>(&state).unwrap_or_else(|_| json!({})),
        "created_at": created,
        "updated_at": updated,
    })
}

pub async fn list_sessions(
    State(state): State<HttpState>,
    Query(query): Query<PageQuery>,
) -> ApiResult<Json<Value>> {
    let (page, per_page, offset) = query.values();
    let (rows, total): (Vec<SessionRow>, i64) = if let Some(pattern) = query.pattern() {
        let filter = "session_key LIKE ?1 ESCAPE '\\' OR platform LIKE ?1 ESCAPE '\\' OR channel_id LIKE ?1 ESCAPE '\\' OR user_id LIKE ?1 ESCAPE '\\'";
        let rows = sqlx::query_as(&format!(
            "SELECT {SESSION_COLUMNS} FROM sessions WHERE {filter} ORDER BY updated_at DESC LIMIT ?2 OFFSET ?3"
        ))
        .bind(&pattern)
        .bind(i64::from(per_page))
        .bind(offset)
        .fetch_all(&state.pool)
        .await?;
        let total = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM sessions WHERE {filter}"))
            .bind(&pattern)
            .fetch_one(&state.pool)
            .await?;
        (rows, total)
    } else {
        let rows = sqlx::query_as(&format!(
            "SELECT {SESSION_COLUMNS} FROM sessions ORDER BY updated_at DESC LIMIT ? OFFSET ?"
        ))
        .bind(i64::from(per_page))
        .bind(offset)
        .fetch_all(&state.pool)
        .await?;
        let total = sqlx::query_scalar("SELECT COUNT(*) FROM sessions")
            .fetch_one(&state.pool)
            .await?;
        (rows, total)
    };
    let external: Vec<(String, String)> =
        sqlx::query_as("SELECT lane_key, goal FROM external_sessions ORDER BY lane_key")
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default();
    let items: Vec<Value> = rows.into_iter().map(session_view).collect();
    Ok(Json(json!({
        "items": items,
        "page": page,
        "per_page": per_page,
        "total": total,
        "sessions": items,
        "external": external.into_iter().map(|(lane, goal)| json!({"lane_key": lane, "goal": goal})).collect::<Vec<_>>(),
    })))
}

async fn fetch_session(pool: &SqlitePool, id: &str) -> Result<Option<SessionRow>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {SESSION_COLUMNS} FROM sessions WHERE session_key = ?"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn get_session(
    State(state): State<HttpState>,
    UrlPath(id): UrlPath<String>,
) -> ApiResult<Json<Value>> {
    let row = fetch_session(&state.pool, &id)
        .await?
        .ok_or_else(|| ApiError::not_found("session"))?;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE session_key = ?")
        .bind(&id)
        .fetch_one(&state.pool)
        .await?;
    let mut view = session_view(row);
    view["message_count"] = json!(count);
    view["active"] = json!(false);
    Ok(Json(view))
}

pub async fn list_messages(
    State(state): State<HttpState>,
    UrlPath(id): UrlPath<String>,
    Query(query): Query<PageQuery>,
) -> ApiResult<Json<Value>> {
    if fetch_session(&state.pool, &id).await?.is_none() {
        return Err(ApiError::not_found("session"));
    }
    let (page, per_page, offset) = query.values();
    let rows: Vec<(Option<i64>, String, String, String, String, String)> = sqlx::query_as(
        "SELECT sequence, id, role, content, metadata_json, created_at FROM messages
         WHERE session_key = ? ORDER BY sequence IS NULL, sequence, created_at, rowid LIMIT ? OFFSET ?",
    )
    .bind(&id)
    .bind(i64::from(per_page))
    .bind(offset)
    .fetch_all(&state.pool)
    .await?;
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE session_key = ?")
        .bind(&id)
        .fetch_one(&state.pool)
        .await?;
    let items: Vec<Value> = rows
        .into_iter()
        .map(|(sequence, id, role, content, metadata, created)| {
            json!({
                "sequence": sequence,
                "id": id,
                "role": role,
                "content": content,
                "metadata": serde_json::from_str::<Value>(&metadata).unwrap_or_else(|_| json!({})),
                "created_at": created,
            })
        })
        .collect();
    Ok(Json(
        json!({"items": items, "page": page, "per_page": per_page, "total": total}),
    ))
}

/// Deletes a session and its transcript. Refused while a cron job is bound to it, as in omo.
pub async fn delete_session(
    State(state): State<HttpState>,
    UrlPath(id): UrlPath<String>,
) -> ApiResult<Response> {
    let bound: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM scheduled_jobs WHERE session_key = ?")
            .bind(&id)
            .fetch_one(&state.pool)
            .await?;
    if bound > 0 {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            format!("session has {bound} cron job(s); delete or move them first"),
        ));
    }
    let mut tx = state.pool.begin_write().await?;
    let deleted = sqlx::query(
        "DELETE FROM sessions WHERE session_key = ?1
         AND NOT EXISTS (SELECT 1 FROM scheduled_jobs WHERE session_key = ?1)",
    )
    .bind(&id)
    .execute(&mut *tx)
    .await?;
    if deleted.rows_affected() == 0 {
        return Err(ApiError::not_found("session"));
    }
    sqlx::query("DELETE FROM messages WHERE session_key = ?")
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    #[serde(alias = "content")]
    message: String,
}

fn web_session_key(id: &str) -> SessionKey {
    SessionKey::new("web", None::<String>, id, None::<String>, "dashboard")
}

/// omo's `resolve_session_key`: a stored session's key, or a dashboard-only web session.
async fn resolve_session_key(pool: &SqlitePool, id: &str) -> Result<SessionKey, ApiError> {
    type SessionRow = (String, Option<String>, String, Option<String>, String);
    let row: Option<SessionRow> = sqlx::query_as(
        "SELECT platform, guild_id, channel_id, thread_id, user_id FROM sessions WHERE session_key = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    if row.is_none() {
        return Ok(web_session_key(id));
    }
    if let Ok(key) = SessionKey::from_storage_key(id) {
        return Ok(key);
    }
    let (platform, guild, channel, thread, user) = row.expect("checked above");
    Ok(SessionKey::new(platform, guild, channel, thread, user))
}

/// The session id a dashboard id streams and chats under.
async fn resolve_storage_id(pool: &SqlitePool, id: &str) -> String {
    match resolve_session_key(pool, id).await {
        Ok(key) if key.platform == "web" => key.storage_key(),
        _ => id.to_owned(),
    }
}

pub fn chat_available() -> bool {
    #[cfg(feature = "hub-slack")]
    {
        crate::chat::slack_turn::installed().is_some()
    }
    #[cfg(not(feature = "hub-slack"))]
    {
        false
    }
}

/// Queues the owner's message as an omo turn of the session. False without a turn runtime.
fn submit_chat(pool: &SqlitePool, key: SessionKey, message: String) -> bool {
    #[cfg(feature = "hub-slack")]
    {
        crate::chat::slack_turn::submit_dashboard_message(pool, key, message)
    }
    #[cfg(not(feature = "hub-slack"))]
    {
        let _ = (pool, key, message);
        false
    }
}

/// omo `POST /api/sessions/{id}/chat`: runs the message as a session turn through
/// mhc RPC. A Slack session answers in its thread; a web session answers into the transcript.
pub async fn post_chat(
    State(state): State<HttpState>,
    UrlPath(id): UrlPath<String>,
    Json(request): Json<ChatRequest>,
) -> ApiResult<Response> {
    let message = request.message.trim();
    if message.is_empty() {
        return Err(ApiError::bad_request("message must not be empty"));
    }
    if !chat_available() {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            CHAT_UNAVAILABLE,
        ));
    }
    let key = resolve_session_key(&state.pool, &id).await?;
    let session_id = key.storage_key();
    if !submit_chat(&state.pool, key, message.to_owned()) {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            CHAT_UNAVAILABLE,
        ));
    }
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({"queued": true, "session_id": session_id})),
    )
        .into_response())
}

/// Interrupts the session's running omo turn, as `/omo-stop` does.
pub async fn stop_session(
    State(state): State<HttpState>,
    UrlPath(id): UrlPath<String>,
) -> ApiResult<Json<Value>> {
    let known: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE session_key = ?")
        .bind(&id)
        .fetch_one(&state.pool)
        .await?;
    if known == 0 && !crate::chat::turns::is_active(&id) {
        return Err(ApiError::not_found("session"));
    }
    let stopped = crate::chat::turns::stop(&id);
    Ok(Json(json!({"stopped": stopped, "session_id": id})))
}

pub async fn session_ws(
    ws: WebSocketUpgrade,
    State(state): State<HttpState>,
    UrlPath(id): UrlPath<String>,
) -> Response {
    ws.on_upgrade(move |socket| handle_session_socket(socket, state.pool, id))
}

/// Sends `ready`, then each message stored for the session after the socket opened as an
/// `event`, and answers `ping`. `message`/`chat` run an omo turn of the session (as omo's
/// multiplexer route did); `stop` interrupts the session's live omo turn.
async fn handle_session_socket(mut socket: WebSocket, pool: SqlitePool, id: String) {
    let id = resolve_storage_id(&pool, &id).await;
    let ready = json!({"type": "ready", "session_id": id, "chat_available": chat_available()});
    if socket
        .send(Message::Text(ready.to_string().into()))
        .await
        .is_err()
    {
        return;
    }
    let mut last: i64 =
        sqlx::query_scalar("SELECT COALESCE(MAX(rowid), 0) FROM messages WHERE session_key = ?")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .unwrap_or(0);
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(2));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = tick.tick() => {
                let rows: Vec<(i64, String, String, String, String)> = sqlx::query_as(
                    "SELECT rowid, id, role, content, created_at FROM messages WHERE session_key = ? AND rowid > ? ORDER BY rowid LIMIT 100",
                )
                .bind(&id)
                .bind(last)
                .fetch_all(&pool)
                .await
                .unwrap_or_default();
                for (rowid, message_id, role, content, created) in rows {
                    last = rowid;
                    let event = json!({"type": "event", "event": {
                        "type": "message", "session_id": id, "id": message_id,
                        "role": role, "content": content, "created_at": created,
                    }});
                    if socket.send(Message::Text(event.to_string().into())).await.is_err() {
                        return;
                    }
                }
            }
            incoming = socket.recv() => {
                let Some(Ok(message)) = incoming else { break };
                let reply = match message {
                    Message::Text(text) => {
                        let parsed: Value = serde_json::from_str(text.as_str())
                            .unwrap_or_else(|_| json!({"type": "message"}));
                        match parsed.get("type").and_then(Value::as_str).unwrap_or("message") {
                            "ping" => json!({"type": "pong"}),
                            "stop" => json!({"type": "stopped", "stopped": crate::chat::turns::stop(&id)}),
                            "message" | "chat" => {
                                let content = parsed.get("content").or_else(|| parsed.get("message")).and_then(Value::as_str).unwrap_or_default().trim().to_owned();
                                if content.is_empty() {
                                    json!({"type": "error", "message": "message must not be empty"})
                                } else {
                                    match resolve_session_key(&pool, &id).await {
                                        Ok(key) => {
                                            if submit_chat(&pool, key, content) {
                                                json!({"type": "accepted", "session_id": id})
                                            } else {
                                                json!({"type": "error", "message": CHAT_UNAVAILABLE})
                                            }
                                        }
                                        Err(error) => json!({"type": "error", "message": error.message}),
                                    }
                                }
                            }
                            other => json!({"type": "error", "message": format!("unsupported websocket message type {other}")}),
                        }
                    }
                    Message::Ping(payload) => {
                        if socket.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                        continue;
                    }
                    Message::Close(_) => break,
                    _ => continue,
                };
                if socket.send(Message::Text(reply.to_string().into())).await.is_err() {
                    break;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------
// Cron jobs
// ---------------------------------------------------------------------------------------

type JobRow = (
    String,
    Option<String>,
    String,
    String,
    i64,
    Option<String>,
    String,
    String,
);

const JOB_COLUMNS: &str =
    "id, session_key, expression, payload_json, enabled, next_run_at, created_at, updated_at";

fn job_view(row: JobRow) -> Value {
    let (id, session_key, expression, payload_json, enabled, next, created, updated) = row;
    let (payload, error) = match serde_json::from_str::<Value>(&payload_json) {
        Ok(payload) => (payload, Value::Null),
        Err(error) => (Value::Null, Value::String(error.to_string())),
    };
    let model = crate::cron_exec::job_model(&payload);
    let model_override = crate::cron_exec::model_override(&payload);
    json!({
        "id": id,
        "session_key": session_key,
        "expression": expression,
        "model": model,
        "model_override": model_override,
        "payload": payload,
        "enabled": enabled == 1,
        "next_run_at": next,
        "created_at": created,
        "updated_at": updated,
        "error": error,
    })
}

async fn fetch_job(pool: &SqlitePool, id: &str) -> Result<Option<JobRow>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {JOB_COLUMNS} FROM scheduled_jobs WHERE id = ?"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn list_cron_jobs(State(state): State<HttpState>) -> ApiResult<Json<Value>> {
    let rows: Vec<JobRow> = sqlx::query_as(&format!(
        "SELECT {JOB_COLUMNS} FROM scheduled_jobs ORDER BY enabled DESC, next_run_at IS NULL, next_run_at, id"
    ))
    .fetch_all(&state.pool)
    .await?;
    let items: Vec<Value> = rows.into_iter().map(job_view).collect();
    Ok(Json(json!({"items": items, "jobs": items})))
}

pub async fn get_cron_job(
    State(state): State<HttpState>,
    UrlPath(id): UrlPath<String>,
) -> ApiResult<Json<Value>> {
    let row = fetch_job(&state.pool, &id)
        .await?
        .ok_or_else(|| ApiError::not_found("cron job"))?;
    Ok(Json(job_view(row)))
}

/// The next occurrence for `expression` honoring the payload timezone, as the scheduler does.
pub fn first_run_at(expression: &str, payload: &Value) -> Result<String, ApiError> {
    crate::schedule::next_run_tz(
        expression,
        chrono::Utc::now(),
        crate::schedule::payload_timezone(payload),
    )
    .map(|next| next.to_rfc3339())
    .map_err(|error| ApiError::bad_request(error.to_string()))
}

#[derive(Debug, Deserialize)]
pub struct CronJobInput {
    id: Option<String>,
    expression: String,
    #[serde(default)]
    payload: Value,
    session_key: Option<String>,
    enabled: Option<bool>,
}

/// `POST /api/cron/jobs`. omo registered the job with its first run; ferryx used to insert it
/// with no `next_run_at`, so the scheduler never picked it up.
pub async fn create_cron_job(
    State(state): State<HttpState>,
    Json(input): Json<CronJobInput>,
) -> ApiResult<Response> {
    let expression = input.expression.trim().to_owned();
    if expression.is_empty() {
        return Err(ApiError::bad_request("expression must not be empty"));
    }
    if input.id.as_deref().is_some_and(|id| id.trim().is_empty()) {
        return Err(ApiError::bad_request("id must not be empty"));
    }
    if input.payload.is_null() {
        return Err(ApiError::bad_request("payload is required"));
    }
    check_model_override(&input.payload)?;
    let id = input
        .id
        .map(|id| id.trim().to_owned())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let payload_json = input.payload.to_string();
    let (_, delivery) = crate::schedule::map_payload(&payload_json)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    let enabled = input.enabled != Some(false);
    let next = first_run_at(&expression, &input.payload)?;
    let session_key = input.session_key.filter(|key| !key.trim().is_empty());
    let now = now_rfc3339();
    let inserted = sqlx::query(
        "INSERT INTO scheduled_jobs(id, session_key, expression, payload_json, enabled, next_run_at, delivery_json, created_at, updated_at)
         VALUES(?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO NOTHING",
    )
    .bind(&id)
    .bind(&session_key)
    .bind(&expression)
    .bind(&payload_json)
    .bind(i64::from(enabled))
    .bind(&next)
    .bind(&delivery)
    .bind(&now)
    .bind(&now)
    .execute(&state.pool)
    .await?;
    if inserted.rows_affected() == 0 {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            format!("cron job {id} already exists"),
        ));
    }
    crate::schedule::poke_scheduler();
    let row = fetch_job(&state.pool, &id)
        .await?
        .ok_or_else(|| ApiError::not_found("cron job"))?;
    Ok((StatusCode::CREATED, Json(job_view(row))).into_response())
}

#[derive(Debug, Deserialize)]
pub struct CronJobUpdateInput {
    expression: Option<String>,
    payload: Option<Value>,
    session_key: Option<String>,
    enabled: Option<bool>,
}

/// Payload keys the scheduler maintains while a job runs; an edit must not reset them.
const SCHEDULER_OWNED_PAYLOAD_KEYS: [&str; 6] = [
    "last_status",
    "last_run_at",
    "last_occurrence",
    "last_error",
    "last_delivery_error",
    "retry_at",
];

fn preserve_scheduler_state(existing: &Value, updated: &mut Value) {
    let (Some(old), Some(new)) = (existing.as_object(), updated.as_object_mut()) else {
        return;
    };
    for key in SCHEDULER_OWNED_PAYLOAD_KEYS {
        if let Some(value) = old.get(key) {
            if !new.contains_key(key) {
                new.insert(key.to_owned(), value.clone());
            }
        }
    }
    if let Some(done) = old.get("repeat").and_then(|repeat| repeat.get("completed")) {
        if let Some(repeat) = new.get_mut("repeat").and_then(Value::as_object_mut) {
            repeat.entry("completed").or_insert_with(|| done.clone());
        }
    }
    if let Some(done) = old.get("completed") {
        if !new.contains_key("completed") && !new.contains_key("repeat") {
            new.insert("completed".to_owned(), done.clone());
        }
    }
}

/// `PUT /api/cron/jobs/{id}`: a partial update. Omitted fields keep the stored value, an
/// empty `session_key` clears the binding, and the next run is recomputed from the new
/// schedule (cleared while the job is paused, as omo's `pause` did).
pub async fn update_cron_job(
    State(state): State<HttpState>,
    UrlPath(id): UrlPath<String>,
    Json(input): Json<CronJobUpdateInput>,
) -> ApiResult<Json<Value>> {
    let (_, old_session, old_expression, old_payload_json, old_enabled, _, _, _) =
        fetch_job(&state.pool, &id)
            .await?
            .ok_or_else(|| ApiError::not_found("cron job"))?;
    let expression = match input.expression {
        Some(expression) if expression.trim().is_empty() => {
            return Err(ApiError::bad_request("expression must not be empty"))
        }
        Some(expression) => expression.trim().to_owned(),
        None => old_expression,
    };
    let old_payload: Value = serde_json::from_str(&old_payload_json).unwrap_or(Value::Null);
    let payload = match input.payload {
        Some(mut payload) => {
            preserve_scheduler_state(&old_payload, &mut payload);
            keep_model_override(&old_payload, &mut payload);
            payload
        }
        None if old_payload.is_null() => {
            return Err(ApiError::bad_request(
                "stored payload is not valid JSON; send a payload",
            ))
        }
        None => old_payload,
    };
    check_model_override(&payload)?;
    let session_key = match input.session_key {
        Some(key) if key.trim().is_empty() => None,
        Some(key) => Some(key),
        None => old_session,
    };
    let enabled = input.enabled.unwrap_or(old_enabled == 1);
    let payload_json = payload.to_string();
    let (_, delivery) = crate::schedule::map_payload(&payload_json)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    let next = if enabled {
        Some(first_run_at(&expression, &payload)?)
    } else {
        None
    };
    sqlx::query(
        "UPDATE scheduled_jobs SET session_key = ?, expression = ?, payload_json = ?, delivery_json = ?,
         enabled = ?, next_run_at = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&session_key)
    .bind(&expression)
    .bind(&payload_json)
    .bind(&delivery)
    .bind(i64::from(enabled))
    .bind(&next)
    .bind(now_rfc3339())
    .bind(&id)
    .execute(&state.pool)
    .await?;
    crate::schedule::poke_scheduler();
    let row = fetch_job(&state.pool, &id)
        .await?
        .ok_or_else(|| ApiError::not_found("cron job"))?;
    Ok(Json(job_view(row)))
}

/// A payload edit that does not mention `model_override` keeps the job's model; an explicit
/// null clears it.
fn keep_model_override(existing: &Value, updated: &mut Value) {
    let key = crate::cron_exec::MODEL_OVERRIDE_KEY;
    let (Some(old), Some(new)) = (existing.as_object(), updated.as_object_mut()) else {
        return;
    };
    match new.get(key) {
        Some(Value::Null) => {
            new.remove(key);
        }
        Some(_) => {}
        None => {
            if let Some(model) = old.get(key) {
                new.insert(key.to_owned(), model.clone());
            }
        }
    }
}

/// A stored `model_override` must be a `provider/model` string; a bad one is rejected
/// instead of silently running the job on the background model.
fn check_model_override(payload: &Value) -> ApiResult<()> {
    match payload.get(crate::cron_exec::MODEL_OVERRIDE_KEY) {
        None | Some(Value::Null) => Ok(()),
        Some(Value::String(model)) => crate::cron_exec::validate_model_id(model)
            .map(|_| ())
            .map_err(|error| ApiError::bad_request(error.to_string())),
        Some(_) => Err(ApiError::bad_request(
            "model_override must be a provider/model string",
        )),
    }
}

#[derive(Debug, Deserialize)]
pub struct CronModelInput {
    /// `provider/model`, or null / empty to go back to the background model.
    model: Option<String>,
}

/// `PUT /api/cron/jobs/{id}/model`: sets or clears the job's own model.
pub async fn set_cron_job_model(
    State(state): State<HttpState>,
    UrlPath(id): UrlPath<String>,
    Json(input): Json<CronModelInput>,
) -> ApiResult<Json<Value>> {
    let model = input.model.filter(|model| !model.trim().is_empty());
    crate::cron_exec::set_model_override(&state.pool, &id, model.as_deref())
        .await
        .map_err(|error| ApiError::bad_request(error.to_string()))?
        .ok_or_else(|| ApiError::not_found("cron job"))?;
    let row = fetch_job(&state.pool, &id)
        .await?
        .ok_or_else(|| ApiError::not_found("cron job"))?;
    Ok(Json(job_view(row)))
}

pub async fn delete_cron_job(
    State(state): State<HttpState>,
    UrlPath(id): UrlPath<String>,
) -> ApiResult<Response> {
    let deleted = sqlx::query("DELETE FROM scheduled_jobs WHERE id = ?")
        .bind(&id)
        .execute(&state.pool)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(ApiError::not_found("cron job"));
    }
    crate::schedule::poke_scheduler();
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Pause clears the next run; resume schedules the next occurrence from now. ferryx used to
/// flip `enabled` only, so a resumed job fired at a stale `next_run_at` (or never, when it
/// had none) and pausing an unknown id answered 200.
pub async fn set_cron_enabled(
    pool: &SqlitePool,
    id: &str,
    enabled: bool,
) -> ApiResult<Json<Value>> {
    let (_, _, expression, payload_json, _, _, _, _) = fetch_job(pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("cron job"))?;
    let next = if enabled {
        let payload: Value = serde_json::from_str(&payload_json).unwrap_or(Value::Null);
        Some(first_run_at(&expression, &payload)?)
    } else {
        None
    };
    sqlx::query(
        "UPDATE scheduled_jobs SET enabled = ?, next_run_at = ?, updated_at = ? WHERE id = ?",
    )
    .bind(i64::from(enabled))
    .bind(&next)
    .bind(now_rfc3339())
    .bind(id)
    .execute(pool)
    .await?;
    crate::schedule::poke_scheduler();
    let row = fetch_job(pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("cron job"))?;
    let mut view = job_view(row);
    view["enabled"] = json!(enabled);
    Ok(Json(view))
}

pub async fn pause_cron_job(
    State(state): State<HttpState>,
    UrlPath(id): UrlPath<String>,
) -> ApiResult<Json<Value>> {
    set_cron_enabled(&state.pool, &id, false).await
}

pub async fn resume_cron_job(
    State(state): State<HttpState>,
    UrlPath(id): UrlPath<String>,
) -> ApiResult<Json<Value>> {
    set_cron_enabled(&state.pool, &id, true).await
}

#[derive(Debug, Default, Deserialize)]
pub struct CronRunsQuery {
    job_id: Option<String>,
    status: Option<String>,
    limit: Option<u32>,
}

pub async fn list_cron_runs(
    State(state): State<HttpState>,
    Query(query): Query<CronRunsQuery>,
) -> ApiResult<Json<Value>> {
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let rows: Vec<CronRunRow> =
        sqlx::query_as(
            "SELECT id, job_id, lease_until, started_at, finished_at, status, error, owner_pid FROM cron_runs
             WHERE (?1 IS NULL OR job_id = ?1) AND (?2 IS NULL OR status = ?2)
             ORDER BY started_at DESC LIMIT ?3",
        )
        .bind(&query.job_id)
        .bind(&query.status)
        .bind(i64::from(limit))
        .fetch_all(&state.pool)
        .await?;
    let items: Vec<Value> = rows
        .into_iter()
        .map(|(id, job, lease, started, finished, status, error, pid)| {
            json!({
                "id": id, "run_id": id, "job_id": job, "lease_expires_at": lease,
                "started_at": started, "completed_at": finished, "status": status,
                "attempt": 1, "error": error, "owner_pid": pid,
            })
        })
        .collect();
    Ok(Json(json!({"items": items, "runs": items})))
}

// ---------------------------------------------------------------------------------------
// Bot profiles
// ---------------------------------------------------------------------------------------

type BotRow = (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    String,
    String,
);

const BOT_COLUMNS: &str =
    "bot_id, name, model, system_prompt, enabled_toolsets, custom_settings_json, created_at, updated_at";

fn bot_view(row: BotRow) -> Value {
    let (bot_id, name, model, prompt, toolsets, settings, created, updated) = row;
    let settings = crate::http::public_settings(&settings);
    json!({
        "bot_id": bot_id,
        "name": name,
        "model": model,
        "system_prompt": prompt,
        "enabled_toolsets": toolsets.map(|t| t.split(',').map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).collect::<Vec<_>>()),
        "custom_settings": settings,
        "settings": settings,
        "created_at": created,
        "updated_at": updated,
    })
}

pub async fn list_bots(State(state): State<HttpState>) -> ApiResult<Json<Value>> {
    let rows: Vec<BotRow> = sqlx::query_as(&format!(
        "SELECT {BOT_COLUMNS} FROM bot_profiles ORDER BY name ASC"
    ))
    .fetch_all(&state.pool)
    .await?;
    let items: Vec<Value> = rows.into_iter().map(bot_view).collect();
    Ok(Json(json!({
        "items": items,
        "total": items.len(),
        "runtime_connections": Value::Null,
        "bots": items,
    })))
}

pub async fn get_bot(
    State(state): State<HttpState>,
    UrlPath(bot_id): UrlPath<String>,
) -> ApiResult<Json<Value>> {
    let row: BotRow = sqlx::query_as(&format!(
        "SELECT {BOT_COLUMNS} FROM bot_profiles WHERE bot_id = ?"
    ))
    .bind(&bot_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("bot profile"))?;
    Ok(Json(bot_view(row)))
}

#[derive(Debug, Deserialize)]
pub struct CreateBotPayload {
    bot_id: String,
    name: String,
    model: Option<String>,
    system_prompt: Option<String>,
    enabled_toolsets: Option<Vec<String>>,
    custom_settings: Option<Value>,
}

pub async fn create_bot(
    State(state): State<HttpState>,
    Json(payload): Json<CreateBotPayload>,
) -> ApiResult<Json<Value>> {
    let bot_id = payload.bot_id.trim();
    if bot_id.is_empty() {
        return Err(ApiError::bad_request("bot_id cannot be empty"));
    }
    let name = payload.name.trim();
    if name.is_empty() {
        return Err(ApiError::bad_request("name cannot be empty"));
    }
    let model = payload.model.filter(|m| !m.trim().is_empty());
    let prompt = payload.system_prompt.filter(|p| !p.trim().is_empty());
    let toolsets = payload
        .enabled_toolsets
        .filter(|ts| !ts.is_empty())
        .map(|ts| ts.join(","));
    let settings = payload
        .custom_settings
        .map(|cs| cs.to_string())
        .unwrap_or_else(|| "{}".into());
    let now = now_rfc3339();
    let inserted = sqlx::query(
        "INSERT INTO bot_profiles (bot_id, name, model, system_prompt, enabled_toolsets, custom_settings_json, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(bot_id) DO NOTHING",
    )
    .bind(bot_id)
    .bind(name)
    .bind(&model)
    .bind(&prompt)
    .bind(&toolsets)
    .bind(&settings)
    .bind(&now)
    .bind(&now)
    .execute(&state.pool)
    .await?;
    if inserted.rows_affected() == 0 {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            format!("bot profile {bot_id} already exists"),
        ));
    }
    Ok(Json(
        json!({"status": "created", "bot_id": bot_id, "name": name}),
    ))
}

pub async fn delete_bot(
    State(state): State<HttpState>,
    UrlPath(bot_id): UrlPath<String>,
) -> ApiResult<Json<Value>> {
    let deleted = sqlx::query("DELETE FROM bot_profiles WHERE bot_id = ?")
        .bind(&bot_id)
        .execute(&state.pool)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(ApiError::not_found("bot profile"));
    }
    Ok(Json(json!({"status": "deleted", "bot_id": bot_id})))
}

#[derive(Debug, Deserialize)]
pub struct UpdateBotPayload {
    name: Option<String>,
    model: Option<String>,
    system_prompt: Option<String>,
    enabled_toolsets: Option<Vec<String>>,
    custom_settings: Option<Value>,
}

/// Omitted fields keep the stored value, an empty one clears it to NULL ("use the default").
/// Secret keys hidden from the GET view are carried over when custom settings are replaced,
/// so saving the form does not erase a token the form never saw.
pub async fn update_bot(
    State(state): State<HttpState>,
    UrlPath(bot_id): UrlPath<String>,
    Json(payload): Json<UpdateBotPayload>,
) -> ApiResult<Json<Value>> {
    let name = match payload.name.as_deref().map(str::trim) {
        Some("") => return Err(ApiError::bad_request("name cannot be empty")),
        other => other.map(str::to_owned),
    };
    let (set_model, model) = match payload.model {
        Some(m) if m.trim().is_empty() => (true, None),
        Some(m) => (true, Some(m)),
        None => (false, None),
    };
    let (set_prompt, prompt) = match payload.system_prompt {
        Some(p) if p.trim().is_empty() => (true, None),
        Some(p) => (true, Some(p)),
        None => (false, None),
    };
    let (set_toolsets, toolsets) = match payload.enabled_toolsets {
        Some(ts) if ts.is_empty() => (true, None),
        Some(ts) => (true, Some(ts.join(","))),
        None => (false, None),
    };
    let settings = match payload.custom_settings {
        Some(mut settings) => {
            let stored: Option<String> = sqlx::query_scalar(
                "SELECT custom_settings_json FROM bot_profiles WHERE bot_id = ?",
            )
            .bind(&bot_id)
            .fetch_optional(&state.pool)
            .await?;
            if let (Some(stored), Some(object)) = (stored, settings.as_object_mut()) {
                let stored: Value = serde_json::from_str(&stored).unwrap_or_else(|_| json!({}));
                let public = crate::http::public_settings(&stored.to_string());
                if let Some(stored) = stored.as_object() {
                    for (key, value) in stored {
                        if public.get(key).is_none() && !object.contains_key(key) {
                            object.insert(key.clone(), value.clone());
                        }
                    }
                }
            }
            Some(settings.to_string())
        }
        None => None,
    };
    let updated = sqlx::query(
        "UPDATE bot_profiles SET
            name = COALESCE(?, name),
            model = CASE WHEN ? THEN ? ELSE model END,
            system_prompt = CASE WHEN ? THEN ? ELSE system_prompt END,
            enabled_toolsets = CASE WHEN ? THEN ? ELSE enabled_toolsets END,
            custom_settings_json = COALESCE(?, custom_settings_json),
            updated_at = ?
         WHERE bot_id = ?",
    )
    .bind(&name)
    .bind(set_model)
    .bind(&model)
    .bind(set_prompt)
    .bind(&prompt)
    .bind(set_toolsets)
    .bind(&toolsets)
    .bind(&settings)
    .bind(now_rfc3339())
    .bind(&bot_id)
    .execute(&state.pool)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(ApiError::not_found("bot profile"));
    }
    get_bot(State(state), UrlPath(bot_id)).await
}

// ---------------------------------------------------------------------------------------
// Status, memory, approvals, precedents (omo shapes next to ferryx keys)
// ---------------------------------------------------------------------------------------

fn started() -> std::time::Instant {
    static STARTED: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    *STARTED.get_or_init(std::time::Instant::now)
}

pub fn mark_started() {
    let _ = started();
}

async fn count(pool: &SqlitePool, sql: &str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(pool).await.unwrap_or(0)
}

pub async fn status(State(state): State<HttpState>) -> Json<Value> {
    let sessions = count(&state.pool, "SELECT COUNT(*) FROM sessions").await;
    let jobs = count(&state.pool, "SELECT COUNT(*) FROM scheduled_jobs").await;
    let messages = count(&state.pool, "SELECT COUNT(*) FROM messages").await;
    let memories = count(&state.pool, "SELECT COUNT(*) FROM memories").await;
    let pending = count(
        &state.pool,
        "SELECT COUNT(*) FROM action_proposals WHERE state = 'pending'",
    )
    .await;
    let sample = std::ffi::CString::new("/")
        .ok()
        .and_then(|path| crate::readiness::disk_sample(&path));
    let (total, free) = sample.unzip();
    let pressure = crate::readiness::classify_disk_pressure(total, free);
    let used_percent = match (total, free) {
        (Some(total), Some(free)) if total > 0 => {
            Some(((total.saturating_sub(free)) as f64 / total as f64 * 100.0).round())
        }
        _ => None,
    };
    let status = if pressure == "ok" { "ok" } else { "degraded" };
    Json(json!({
        "ok": true,
        "sessions": sessions,
        "cronJobs": jobs,
        "status": status,
        "model": crate::hub_env::var("OMON_DEFAULT_MODEL").ok(),
        "uptime_seconds": started().elapsed().as_secs(),
        "active_sessions": 0,
        "busy_sessions": 0,
        "chat_available": false,
        "database": {
            "sessions": sessions, "messages": messages, "cron_jobs": jobs,
            "memories": memories, "pool_size": state.pool.size(), "pool_idle": state.pool.num_idle(),
        },
        "disk": {
            "status": status, "total_bytes": total, "available_bytes": free,
            "workspace_total_bytes": total, "workspace_available_bytes": free,
            "total_mb": total.map(|t| t / (1024 * 1024)), "available_mb": free.map(|f| f / (1024 * 1024)),
            "free_mb": free.map(|f| f / (1024 * 1024)), "used_percent": used_percent, "pressure": pressure,
        },
        "pending_approvals": pending,
        "time": now_rfc3339(),
    }))
}

pub async fn health() -> Json<Value> {
    Json(json!({
        "ok": true,
        "service": "ferryx-hub",
        "status": "ok",
        "uptime_seconds": started().elapsed().as_secs(),
        "time": now_rfc3339(),
    }))
}

pub async fn memory(
    State(state): State<HttpState>,
    Query(query): Query<PageQuery>,
) -> ApiResult<Json<Value>> {
    let (page, per_page, offset) = query.values();
    let pattern = query.pattern();
    let rows: Vec<(String, String, String, String, String, String)> = sqlx::query_as(
        "SELECT id, session_key, content, metadata_json, created_at, updated_at FROM memories
         WHERE ?1 IS NULL OR content LIKE ?1 ESCAPE '\\' OR session_key LIKE ?1 ESCAPE '\\'
         ORDER BY updated_at DESC LIMIT ?2 OFFSET ?3",
    )
    .bind(&pattern)
    .bind(i64::from(per_page))
    .bind(offset)
    .fetch_all(&state.pool)
    .await?;
    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM memories WHERE ?1 IS NULL OR content LIKE ?1 ESCAPE '\\' OR session_key LIKE ?1 ESCAPE '\\'",
    )
    .bind(&pattern)
    .fetch_one(&state.pool)
    .await?;
    let items: Vec<Value> = rows
        .into_iter()
        .map(|(id, key, content, metadata, created, updated)| {
            json!({
                "id": id, "session_key": key, "content": content,
                "metadata": serde_json::from_str::<Value>(&metadata).unwrap_or_else(|_| json!({})),
                "created_at": created, "updated_at": updated,
            })
        })
        .collect();
    Ok(Json(json!({
        "items": items, "page": page, "per_page": per_page, "total": total, "memories": items,
    })))
}

type CronRunRow = (
    String,
    String,
    Option<String>,
    String,
    Option<String>,
    String,
    Option<String>,
    Option<i64>,
);
type ApprovalRow = (
    i64,
    String,
    String,
    String,
    String,
    String,
    String,
    i64,
    String,
);
type PrecedentRow = (
    i64,
    String,
    String,
    String,
    String,
    String,
    i64,
    i64,
    String,
    Option<String>,
    String,
    String,
);

pub async fn pending_approvals(State(state): State<HttpState>) -> ApiResult<Json<Value>> {
    let rows: Vec<ApprovalRow> = sqlx::query_as(
        "SELECT id, origin, session_key, signature, effect, target, summary, tainted, created_at
         FROM action_proposals WHERE state = 'pending' ORDER BY id",
    )
    .fetch_all(&state.pool)
    .await?;
    let items: Vec<Value> = rows
        .into_iter()
        .map(
            |(id, origin, session, signature, effect, target, summary, tainted, created)| {
                json!({
                    "id": id, "origin": origin, "session_key": session, "signature": signature,
                    "effect": effect, "target": target, "summary": summary, "command": summary,
                    "tainted": tainted == 1, "created_at": created,
                })
            },
        )
        .collect();
    Ok(Json(json!({
        "items": items, "pending_count": items.len(), "approvals": items,
    })))
}

pub async fn allowlist(State(state): State<HttpState>) -> ApiResult<Json<Value>> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT pattern_key, created_at FROM approval_allowlist
         UNION SELECT signature, created_at FROM action_precedents WHERE decision = 'allow' AND revoked_at IS NULL
         ORDER BY 2 DESC, 1",
    )
    .fetch_all(&state.pool)
    .await?;
    let keys: Vec<&String> = rows.iter().map(|(key, _)| key).collect();
    let items: Vec<Value> = rows
        .iter()
        .map(|(key, created)| json!({"pattern_key": key, "created_at": created}))
        .collect();
    Ok(Json(json!({"items": items, "allowlist": keys})))
}

pub async fn precedents(State(state): State<HttpState>) -> ApiResult<Json<Value>> {
    let rows: Vec<PrecedentRow> =
        sqlx::query_as(
            "SELECT id, signature, effect, target_scope, decision, scope, approvals, declines, origin, owner_words, created_at, updated_at
             FROM action_precedents WHERE revoked_at IS NULL ORDER BY updated_at DESC, id DESC",
        )
        .fetch_all(&state.pool)
        .await?;
    let items: Vec<Value> = rows
        .into_iter()
        .map(|(id, signature, effect, target, decision, scope, approvals, declines, origin, words, created, updated)| {
            json!({
                "id": id, "signature": signature, "effect": effect, "target_scope": target,
                "decision": decision, "scope": scope, "approvals": approvals, "declines": declines,
                "origin": origin, "owner_words": words, "created_at": created, "updated_at": updated,
            })
        })
        .collect();
    Ok(Json(json!({"items": items, "precedents": items})))
}

/// The dashboard web root: `DASHBOARD_WEB_ROOT` (the omo plist name) unless
/// `FERRYX_HUB_WEB_ROOT` overrides it, else `~/.ferryx/hub/web`.
pub fn web_root_from(
    ferryx: Option<std::ffi::OsString>,
    omo: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> std::path::PathBuf {
    if let Some(path) = ferryx.filter(|p| !p.is_empty()) {
        return path.into();
    }
    if let Some(path) = omo.filter(|p| !p.is_empty()) {
        return path.into();
    }
    home.map(|home| std::path::PathBuf::from(home).join(".ferryx/hub/web"))
        .unwrap_or_else(|| std::path::PathBuf::from(".ferryx/hub/web"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    async fn app() -> (axum::Router, SqlitePool) {
        let pool = crate::storage::memory().await.unwrap();
        (
            crate::http::router(pool.clone(), std::env::temp_dir()),
            pool,
        )
    }

    async fn call(
        app: &axum::Router,
        method: &str,
        path: &str,
        body: Option<Value>,
        headers: &[(&str, &str)],
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder()
            .method(method)
            .uri(path)
            .header("host", "127.0.0.1:9119");
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        let request = match body {
            Some(body) => builder
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
            None => builder.body(Body::empty()).unwrap(),
        };
        let response = app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        let value = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
        (status, value)
    }

    #[test]
    fn host_and_origin_rules_match_omo() {
        assert!(is_loopback_host("localhost"));
        assert!(is_loopback_host("127.0.0.1"));
        assert!(is_loopback_host("[::1]"));
        assert!(!is_loopback_host("tracker.example.com"));
        assert_eq!(extract_host("127.0.0.1:9119"), "127.0.0.1");
        assert_eq!(extract_host("[::1]:9119"), "::1");
        assert!(is_same_origin("http://127.0.0.1:9119", "127.0.0.1:9119"));
        assert!(is_same_origin("http://localhost", "localhost:80"));
        assert!(!is_same_origin("https://evil.example", "127.0.0.1:9119"));
        assert!(!is_same_origin("http://127.0.0.1:9120", "127.0.0.1:9119"));
        assert!(!is_same_origin("null", "127.0.0.1:9119"));
    }

    #[tokio::test]
    async fn the_guard_refuses_foreign_hosts_and_cross_site_writes() {
        let (app, _pool) = app().await;
        let request = Request::builder()
            .uri("/api/status")
            .header("host", "evil.example")
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let (status, _) = call(&app, "GET", "/api/status", None, &[]).await;
        assert_eq!(status, StatusCode::OK);
        // A GET from another origin is allowed (no state change); a POST is not.
        let (status, _) = call(
            &app,
            "GET",
            "/api/status",
            None,
            &[("origin", "https://evil.example")],
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = call(
            &app,
            "POST",
            "/api/cron/jobs",
            Some(json!({"id": "x", "expression": "every 5m", "payload": {}})),
            &[("origin", "https://evil.example")],
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = call(
            &app,
            "POST",
            "/api/cron/jobs",
            Some(json!({"id": "x", "expression": "every 5m", "payload": {}})),
            &[("origin", "http://127.0.0.1:9119")],
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        // A websocket upgrade needs a same-origin Origin.
        let (status, _) = call(
            &app,
            "GET",
            "/api/logs/ws",
            None,
            &[("upgrade", "websocket"), ("connection", "upgrade")],
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn stop_interrupts_the_sessions_live_turn() {
        let (app, pool) = app().await;
        sqlx::query("INSERT INTO sessions(session_key, platform, channel_id, user_id, state_json) VALUES('slack:CSTOP:9.0','slack','CSTOP','U1','{}')")
            .execute(&pool).await.unwrap();
        let path = "/api/sessions/slack:CSTOP:9.0/stop";
        let (status, body) = call(&app, "POST", path, None, &[]).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body,
            json!({"stopped": false, "session_id": "slack:CSTOP:9.0"})
        );
        let guard = crate::chat::turns::begin("slack:CSTOP:9.0", "CSTOP");
        let cancelled = crate::chat::turns::cancelled("slack:CSTOP:9.0").unwrap();
        let (_, body) = call(&app, "POST", path, None, &[]).await;
        assert_eq!(body["stopped"], true);
        assert!(*cancelled.borrow());
        drop(guard);
        let (status, _) = call(&app, "POST", "/api/sessions/nope/stop", None, &[]).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn session_detail_messages_and_delete_follow_omo() {
        let (app, pool) = app().await;
        sqlx::query("INSERT INTO sessions(session_key, platform, channel_id, user_id, state_json) VALUES('slack:C1:1.0','slack','C1','U1','{\"title\":\"t\"}'), ('slack:C2','slack','C2','U2','{}')")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO messages(id, session_key, role, content, sequence) VALUES('m2','slack:C1:1.0','assistant','two',2), ('m1','slack:C1:1.0','user','one',1)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO scheduled_jobs(id, session_key, expression, payload_json) VALUES('bound','slack:C2','every 5m','{}')")
            .execute(&pool).await.unwrap();

        let (status, list) = call(&app, "GET", "/api/sessions?search=C1", None, &[]).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(list["total"], 1);
        assert_eq!(list["items"][0]["id"], "slack:C1:1.0");
        assert_eq!(list["sessions"][0]["session_key"], "slack:C1:1.0");

        let (status, detail) = call(&app, "GET", "/api/sessions/slack:C1:1.0", None, &[]).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(detail["message_count"], 2);
        assert_eq!(detail["state"]["title"], "t");

        let (_, messages) = call(
            &app,
            "GET",
            "/api/sessions/slack:C1:1.0/messages?page=1&per_page=10",
            None,
            &[],
        )
        .await;
        assert_eq!(messages["total"], 2);
        assert_eq!(messages["items"][0]["content"], "one");
        assert_eq!(messages["items"][1]["content"], "two");

        let (status, _) = call(&app, "GET", "/api/sessions/missing", None, &[]).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = call(&app, "DELETE", "/api/sessions/slack:C2", None, &[]).await;
        assert_eq!(status, StatusCode::CONFLICT);
        let (status, _) = call(&app, "DELETE", "/api/sessions/slack:C1:1.0", None, &[]).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(left, 0);

        let (status, body) = call(
            &app,
            "POST",
            "/api/sessions/slack:C2/chat",
            Some(json!({"message": "hi"})),
            &[],
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"], CHAT_UNAVAILABLE);
    }

    #[tokio::test]
    async fn cron_jobs_are_scheduled_edited_paused_and_deleted() {
        let (app, pool) = app().await;
        let (status, created) = call(
            &app,
            "POST",
            "/api/cron/jobs",
            Some(json!({"id": "daily", "expression": "0 9 * * *", "payload": {"command": "true", "deliver": "slack:C1", "timezone": "Asia/Seoul"}})),
            &[],
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert!(
            created["next_run_at"].is_string(),
            "a created job is scheduled: {created}"
        );
        let (status, _) = call(
            &app,
            "POST",
            "/api/cron/jobs",
            Some(json!({"id": "daily", "expression": "every 5m", "payload": {}})),
            &[],
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        let (status, _) = call(
            &app,
            "POST",
            "/api/cron/jobs",
            Some(json!({"id": "bad", "expression": "nonsense", "payload": {}})),
            &[],
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        sqlx::query("UPDATE scheduled_jobs SET payload_json = json_set(payload_json, '$.last_status', 'succeeded') WHERE id = 'daily'")
            .execute(&pool).await.unwrap();
        let (status, updated) = call(
            &app,
            "PUT",
            "/api/cron/jobs/daily",
            Some(json!({"expression": "every 30m", "payload": {"command": "echo hi"}})),
            &[],
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{updated}");
        assert_eq!(updated["expression"], "every 30m");
        assert_eq!(updated["payload"]["command"], "echo hi");
        assert_eq!(
            updated["payload"]["last_status"], "succeeded",
            "scheduler state survives an edit"
        );
        let delivery: String =
            sqlx::query_scalar("SELECT delivery_json FROM scheduled_jobs WHERE id = 'daily'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(delivery, "[]", "delivery follows the new payload");

        assert_eq!(updated["model"], json!(crate::model_policy::background_model()));
        assert!(updated["model_override"].is_null());
        let (status, chosen) = call(
            &app,
            "PUT",
            "/api/cron/jobs/daily/model",
            Some(json!({"model": "mahoquot/claude-sonnet-5-5"})),
            &[],
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{chosen}");
        assert_eq!(chosen["model"], "mahoquot/claude-sonnet-5-5");
        assert_eq!(chosen["model_override"], "mahoquot/claude-sonnet-5-5");
        let (status, _) = call(
            &app,
            "PUT",
            "/api/cron/jobs/daily/model",
            Some(json!({"model": "sonnet"})),
            &[],
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _) = call(
            &app,
            "PUT",
            "/api/cron/jobs/missing/model",
            Some(json!({"model": null})),
            &[],
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (_, edited) = call(
            &app,
            "PUT",
            "/api/cron/jobs/daily",
            Some(json!({"payload": {"command": "echo hi"}})),
            &[],
        )
        .await;
        assert_eq!(
            edited["model_override"], "mahoquot/claude-sonnet-5-5",
            "a payload edit keeps the job model"
        );
        let (status, _) = call(
            &app,
            "PUT",
            "/api/cron/jobs/daily",
            Some(json!({"payload": {"command": "echo hi", "model_override": 7}})),
            &[],
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (_, cleared) = call(
            &app,
            "PUT",
            "/api/cron/jobs/daily/model",
            Some(json!({"model": ""})),
            &[],
        )
        .await;
        assert!(cleared["model_override"].is_null());
        assert_eq!(cleared["model"], json!(crate::model_policy::background_model()));

        let (_, paused) = call(&app, "POST", "/api/cron/jobs/daily/pause", None, &[]).await;
        assert_eq!(paused["enabled"], false);
        assert!(paused["next_run_at"].is_null());
        let (_, resumed) = call(&app, "POST", "/api/cron/jobs/daily/resume", None, &[]).await;
        assert_eq!(resumed["enabled"], true);
        assert!(resumed["next_run_at"].is_string());
        let (status, _) = call(&app, "POST", "/api/cron/jobs/missing/pause", None, &[]).await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let (_, one) = call(&app, "GET", "/api/cron/jobs/daily", None, &[]).await;
        assert_eq!(one["id"], "daily");
        let (_, list) = call(&app, "GET", "/api/cron/jobs", None, &[]).await;
        assert_eq!(list["items"].as_array().unwrap().len(), 1);
        assert_eq!(list["jobs"][0]["id"], "daily");

        sqlx::query("INSERT INTO cron_runs(id, job_id, status, started_at) VALUES('r1','daily','succeeded','2026-10-23T00:00:00Z'), ('r2','other','failed','2026-10-23T01:00:00Z')")
            .execute(&pool).await.unwrap();
        let (_, runs) = call(&app, "GET", "/api/cron/runs?job_id=daily", None, &[]).await;
        assert_eq!(runs["items"].as_array().unwrap().len(), 1);
        assert_eq!(runs["items"][0]["run_id"], "r1");

        let (status, _) = call(&app, "DELETE", "/api/cron/jobs/daily", None, &[]).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, _) = call(&app, "DELETE", "/api/cron/jobs/daily", None, &[]).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn bot_profiles_round_trip_without_leaking_or_losing_secrets() {
        let (app, pool) = app().await;
        let (status, _) = call(
            &app,
            "POST",
            "/api/bots",
            Some(json!({"bot_id": "B1", "name": "omo", "model": "", "enabled_toolsets": ["web", "shell"], "custom_settings": {"token": "xoxb-1", "theme": "dark"}})),
            &[],
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let model: Option<String> =
            sqlx::query_scalar("SELECT model FROM bot_profiles WHERE bot_id = 'B1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(model, None, "a blank model is stored as NULL");
        let (status, _) = call(
            &app,
            "POST",
            "/api/bots",
            Some(json!({"bot_id": "B1", "name": "dup"})),
            &[],
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);

        let (_, bot) = call(&app, "GET", "/api/bots/B1", None, &[]).await;
        assert_eq!(bot["enabled_toolsets"], json!(["web", "shell"]));
        assert!(bot["custom_settings"].get("token").is_none());
        assert_eq!(bot["custom_settings"]["theme"], "dark");

        let (status, updated) = call(
            &app,
            "PUT",
            "/api/bots/B1",
            Some(json!({"name": "omo2", "custom_settings": {"theme": "light"}})),
            &[],
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(updated["name"], "omo2");
        assert_eq!(
            updated["enabled_toolsets"],
            json!(["web", "shell"]),
            "omitted fields are kept"
        );
        let stored: String =
            sqlx::query_scalar("SELECT custom_settings_json FROM bot_profiles WHERE bot_id = 'B1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let stored: Value = serde_json::from_str(&stored).unwrap();
        assert_eq!(
            stored["token"], "xoxb-1",
            "a hidden secret survives a form save"
        );
        assert_eq!(stored["theme"], "light");

        let (_, list) = call(&app, "GET", "/api/bots", None, &[]).await;
        assert_eq!(list["total"], 1);
        assert_eq!(list["bots"][0]["bot_id"], "B1");
        let (status, _) = call(&app, "DELETE", "/api/bots/B1", None, &[]).await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = call(&app, "GET", "/api/bots/B1", None, &[]).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn list_routes_carry_omo_keys() {
        let (app, pool) = app().await;
        crate::ledger::propose(&pool, "loop", "s", "rm", "delete", true)
            .await
            .unwrap();
        let (_, pending) = call(&app, "GET", "/api/approvals/pending", None, &[]).await;
        assert_eq!(pending["pending_count"], 1);
        assert_eq!(pending["items"][0]["id"], pending["approvals"][0]["id"]);
        let (_, status) = call(&app, "GET", "/api/status", None, &[]).await;
        assert!(status["database"]["sessions"].is_number());
        assert_eq!(status["pending_approvals"], 1);
        let (_, memory) = call(&app, "GET", "/api/memory", None, &[]).await;
        assert_eq!(memory["total"], 0);
        let (_, health) = call(&app, "GET", "/api/health", None, &[]).await;
        assert_eq!(
            (health["ok"].clone(), health["status"].clone()),
            (json!(true), json!("ok"))
        );
    }

    #[test]
    fn the_web_root_honors_the_omo_plist_name() {
        use std::ffi::OsString;
        let home = Some(OsString::from("/Users/me"));
        assert_eq!(
            web_root_from(None, Some(OsString::from("/srv/web")), home.clone()),
            std::path::PathBuf::from("/srv/web")
        );
        assert_eq!(
            web_root_from(
                Some(OsString::from("/x")),
                Some(OsString::from("/srv/web")),
                home.clone()
            ),
            std::path::PathBuf::from("/x")
        );
        assert_eq!(
            web_root_from(None, None, home),
            std::path::PathBuf::from("/Users/me/.ferryx/hub/web")
        );
    }
}
