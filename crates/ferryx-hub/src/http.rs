//! Dashboard HTTP API on the tracker port. Routes match the omo-gateway dashboard
//! the cloudflared tunnel already calls.
use std::path::{Path, PathBuf};

use axum::extract::ws::{Message, WebSocketUpgrade};
use axum::extract::{Path as UrlPath, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use sqlx::SqlitePool;

use crate::readiness::classify_disk_pressure;
use crate::storage::now_rfc3339;

#[derive(Clone)]
pub struct HttpState {
    pub pool: SqlitePool,
    pub web_root: PathBuf,
}

pub fn router(pool: SqlitePool, web_root: PathBuf) -> Router {
    let tracker = crate::tracker_linear::router().with_state(pool.clone());
    Router::new()
        .route("/api/health", get(health))
        .route("/api/ready", get(ready))
        .route("/api/readiness", get(ready))
        .route("/api/status", get(status))
        .route("/api/sessions", get(sessions))
        .route("/api/cron/jobs", get(cron_jobs).post(create_cron))
        .route("/api/cron/jobs/{id}/trigger", post(trigger_cron))
        .route("/api/cron/jobs/{id}/pause", post(pause_cron))
        .route("/api/cron/jobs/{id}/resume", post(resume_cron))
        .route("/api/cron/runs", get(cron_runs))
        .route("/api/approvals/pending", get(pending_approvals))
        .route("/api/approvals/{id}/resolve", post(resolve_approval))
        .route("/api/approvals/allowlist", get(allowlist))
        .route("/api/autonomy/precedents", get(precedents))
        .route("/api/bots", get(bots))
        .route("/api/memory", get(memory))
        .route("/api/tools", get(tools))
        .route("/api/skills", get(skills))
        .route("/api/config", get(config))
        .route("/api/logs", get(logs))
        .route("/api/logs/ws", get(logs_ws))
        .nest("/api/tracker", tracker)
        .fallback(static_or_missing)
        .with_state(HttpState { pool, web_root })
}

pub fn web_root() -> PathBuf {
    if let Some(path) = std::env::var_os("FERRYX_HUB_WEB_ROOT") {
        return PathBuf::from(path);
    }
    std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".omon").join("web"))
        .unwrap_or_else(|| PathBuf::from(".omon/web"))
}

async fn health() -> Json<Value> {
    Json(json!({"ok": true, "service": "ferryx-hub"}))
}

async fn ready() -> Json<Value> {
    let sample = std::ffi::CString::new("/")
        .ok()
        .and_then(|path| crate::readiness::disk_sample(&path));
    let (total, free) = sample.unzip();
    Json(json!({
        "ok": true,
        "disk": classify_disk_pressure(total, free)
    }))
}

async fn status(State(state): State<HttpState>) -> Response {
    let sessions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);
    let jobs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM scheduled_jobs")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);
    Json(json!({"ok": true, "sessions": sessions, "cronJobs": jobs})).into_response()
}

async fn sessions(State(state): State<HttpState>) -> Response {
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT session_key, platform, channel_id FROM sessions ORDER BY updated_at DESC",
    )
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();
    let external: Vec<(String, String)> =
        sqlx::query_as("SELECT lane_key, goal FROM external_sessions ORDER BY lane_key")
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default();
    Json(json!({
        "sessions": rows.into_iter().map(|(key, platform, channel)| json!({"session_key": key, "platform": platform, "channel_id": channel})).collect::<Vec<_>>(),
        "external": external.into_iter().map(|(lane, goal)| json!({"lane_key": lane, "goal": goal})).collect::<Vec<_>>()
    }))
    .into_response()
}

async fn cron_jobs(State(state): State<HttpState>) -> Response {
    let rows: Vec<(String, String, i64, Option<String>)> = sqlx::query_as(
        "SELECT id, expression, enabled, next_run_at FROM scheduled_jobs ORDER BY id",
    )
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();
    Json(json!({
        "jobs": rows.into_iter().map(|(id, expression, enabled, next)| json!({
            "id": id, "expression": expression, "enabled": enabled == 1, "next_run_at": next
        })).collect::<Vec<_>>()
    }))
    .into_response()
}

async fn create_cron(State(state): State<HttpState>, Json(body): Json<Value>) -> Response {
    let id = body
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let expression = body.get("expression").and_then(Value::as_str).unwrap_or("");
    let payload = body.get("payload").cloned().unwrap_or_else(|| json!({}));
    if id.is_empty() || expression.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "id and expression are required"})),
        )
            .into_response();
    }
    let payload_json = payload.to_string();
    let delivery = match crate::schedule::map_payload(&payload_json) {
        Ok((_, delivery)) => delivery,
        Err(error) => return (StatusCode::BAD_REQUEST, error.to_string()).into_response(),
    };
    let inserted = sqlx::query(
        "INSERT INTO scheduled_jobs(id, expression, payload_json, enabled, delivery_json) VALUES(?,?,?,1,?)",
    )
    .bind(&id)
    .bind(expression)
    .bind(&payload_json)
    .bind(&delivery)
    .execute(&state.pool)
    .await;
    match inserted {
        Ok(_) => {
            crate::schedule::poke_scheduler();
            (StatusCode::CREATED, Json(json!({"id": id}))).into_response()
        }
        Err(error) => (StatusCode::BAD_REQUEST, error.to_string()).into_response(),
    }
}

async fn trigger_cron(State(state): State<HttpState>, UrlPath(id): UrlPath<String>) -> Response {
    let now = now_rfc3339();
    let _ = sqlx::query(
        "UPDATE scheduled_jobs SET enabled = 1, next_run_at = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&now)
    .bind(&now)
    .bind(&id)
    .execute(&state.pool)
    .await;
    crate::schedule::poke_scheduler();
    Json(json!({"id": id, "next_run_at": now})).into_response()
}

async fn pause_cron(State(state): State<HttpState>, UrlPath(id): UrlPath<String>) -> Response {
    set_enabled(&state.pool, &id, false).await
}

async fn resume_cron(State(state): State<HttpState>, UrlPath(id): UrlPath<String>) -> Response {
    set_enabled(&state.pool, &id, true).await
}

async fn set_enabled(pool: &SqlitePool, id: &str, enabled: bool) -> Response {
    let _ = sqlx::query("UPDATE scheduled_jobs SET enabled = ?, updated_at = ? WHERE id = ?")
        .bind(i64::from(enabled))
        .bind(now_rfc3339())
        .bind(id)
        .execute(pool)
        .await;
    Json(json!({"id": id, "enabled": enabled})).into_response()
}

async fn cron_runs(State(state): State<HttpState>) -> Response {
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT id, job_id, status FROM cron_runs ORDER BY started_at DESC LIMIT 100",
    )
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();
    Json(json!({
        "runs": rows.into_iter().map(|(id, job, status)| json!({"id": id, "job_id": job, "status": status})).collect::<Vec<_>>()
    }))
    .into_response()
}

async fn pending_approvals(State(state): State<HttpState>) -> Response {
    let rows: Vec<(i64, String, String, String)> = sqlx::query_as(
        "SELECT id, signature, effect, summary FROM action_proposals WHERE state = 'pending' ORDER BY id",
    )
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();
    Json(json!({
        "approvals": rows.into_iter().map(|(id, signature, effect, summary)| json!({
            "id": id, "signature": signature, "effect": effect, "summary": summary
        })).collect::<Vec<_>>()
    }))
    .into_response()
}

async fn resolve_approval(
    State(state): State<HttpState>,
    UrlPath(id): UrlPath<i64>,
    Json(body): Json<Value>,
) -> Response {
    let allow = body.get("decision").and_then(Value::as_str) != Some("deny");
    match crate::ledger::resolve(
        &state.pool,
        id,
        allow,
        body.get("words").and_then(Value::as_str),
    )
    .await
    {
        Ok(()) => Json(json!({"id": id, "state": if allow { "approved" } else { "declined" }}))
            .into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, error.to_string()).into_response(),
    }
}

async fn allowlist(State(state): State<HttpState>) -> Response {
    let rows: Vec<String> =
        sqlx::query_scalar("SELECT pattern_key FROM approval_allowlist ORDER BY pattern_key")
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default();
    Json(json!({"allowlist": rows})).into_response()
}

async fn precedents(State(state): State<HttpState>) -> Response {
    let rows: Vec<(String, String, String)> =
        sqlx::query_as("SELECT signature, scope, decision FROM action_precedents ORDER BY id")
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default();
    Json(json!({
        "precedents": rows.into_iter().map(|(signature, scope, decision)| json!({
            "signature": signature, "scope": scope, "decision": decision
        })).collect::<Vec<_>>()
    }))
    .into_response()
}

async fn bots(State(state): State<HttpState>) -> Response {
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT bot_id, name, custom_settings_json FROM bot_profiles ORDER BY bot_id",
    )
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();
    Json(json!({
        "bots": rows.into_iter().map(|(id, name, settings)| json!({
            "bot_id": id,
            "name": name,
            "settings": public_settings(&settings)
        })).collect::<Vec<_>>()
    }))
    .into_response()
}

pub fn public_settings(text: &str) -> Value {
    let mut value: Value = serde_json::from_str(text).unwrap_or_else(|_| json!({}));
    if let Some(object) = value.as_object_mut() {
        object.retain(|key, _| {
            let key = key.to_ascii_lowercase();
            !key.contains("token")
                && !key.contains("secret")
                && key != "api_key"
                && key != "password"
        });
    }
    value
}

async fn memory(State(state): State<HttpState>) -> Response {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT id, content FROM memories ORDER BY updated_at DESC LIMIT 100")
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default();
    Json(json!({
        "memories": rows.into_iter().map(|(id, content)| json!({"id": id, "content": content})).collect::<Vec<_>>()
    }))
    .into_response()
}

async fn tools() -> Json<Value> {
    Json(json!({"tools": crate::context::TOOLS}))
}

async fn skills() -> Json<Value> {
    Json(json!({"skills": []}))
}

async fn config() -> Json<Value> {
    Json(json!({"service": "ferryx-hub", "tracker": true}))
}

async fn logs(State(state): State<HttpState>) -> Response {
    let rows: Vec<(i64, String, String, String)> =
        sqlx::query_as("SELECT id, at, level, message FROM hub_log ORDER BY id DESC LIMIT 200")
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default();
    Json(json!({
        "logs": rows.into_iter().map(|(id, at, level, message)| json!({
            "id": id, "at": at, "level": level, "message": message
        })).collect::<Vec<_>>()
    }))
    .into_response()
}

async fn logs_ws(ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(|mut socket| async move {
        let _ = socket
            .send(Message::Text(r#"{"kind":"ready"}"#.into()))
            .await;
    })
}

async fn static_or_missing(
    State(state): State<HttpState>,
    request: axum::extract::Request,
) -> Response {
    let path = request.uri().path();
    if path.starts_with("/api/") {
        return (StatusCode::NOT_FOUND, Json(json!({"error": "not found"}))).into_response();
    }
    match read_static(&state.web_root, path).await {
        Some((mime, bytes)) => ([(header::CONTENT_TYPE, mime)], bytes).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn read_static(root: &Path, path: &str) -> Option<(&'static str, Vec<u8>)> {
    let relative = path.trim_start_matches('/');
    if relative.split('/').any(|part| part == "..") {
        return None;
    }
    let file = if relative.is_empty() {
        root.join("index.html")
    } else {
        root.join(relative)
    };
    let file = match tokio::fs::read(&file).await {
        Ok(bytes) => file_with_mime(file, bytes),
        Err(_) if !relative.is_empty() => {
            let index = root.join("index.html");
            tokio::fs::read(&index)
                .await
                .ok()
                .map(|bytes| ("text/html; charset=utf-8", bytes))?
        }
        Err(_) => return None,
    };
    Some(file)
}

fn file_with_mime(path: PathBuf, bytes: Vec<u8>) -> (&'static str, Vec<u8>) {
    let mime = match path.extension().and_then(|ext| ext.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("json") => "application/json",
        _ => "application/octet-stream",
    };
    (mime, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    #[tokio::test]
    async fn health_cron_approvals_and_the_tracker_page() {
        let pool = crate::storage::memory().await.unwrap();
        let root = std::env::temp_dir().join(format!("ferryx-hub-web-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("index.html"), "<h1>tracker</h1>").unwrap();
        let app = router(pool.clone(), root.clone());
        let health = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(health.status(), StatusCode::OK);
        let ready = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/ready")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ready.status(), StatusCode::OK);
        let created = app.clone().oneshot(
            Request::builder().method("POST").uri("/api/cron/jobs").header("content-type", "application/json")
                .body(Body::from(r#"{"id":"daily","expression":"every 15m","payload":{"deliver":"slack:C1"}}"#)).unwrap(),
        ).await.unwrap();
        assert_eq!(created.status(), StatusCode::CREATED);
        let jobs = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/cron/jobs")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(jobs.into_body(), 64 * 1024)
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&body).contains("daily"));
        crate::ledger::propose(&pool, "loop", "s", "rm", "delete", true)
            .await
            .unwrap();
        let pending = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/approvals/pending")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let pending_body = axum::body::to_bytes(pending.into_body(), 64 * 1024)
            .await
            .unwrap();
        let pending_json: Value = serde_json::from_slice(&pending_body).unwrap();
        let id = pending_json["approvals"][0]["id"].as_i64().unwrap();
        let resolved = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/approvals/{id}/resolve"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"decision":"deny"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resolved.status(), StatusCode::OK);
        sqlx::query("INSERT INTO bot_profiles(bot_id, name, custom_settings_json) VALUES('b','one','{\"token\":\"x\",\"theme\":\"dark\"}')")
            .execute(&pool).await.unwrap();
        let bots = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/bots")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bots_body = String::from_utf8(
            axum::body::to_bytes(bots.into_body(), 64 * 1024)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(!bots_body.contains("\"token\""));
        assert!(bots_body.contains("dark"));
        let page = app
            .clone()
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(page.status(), StatusCode::OK);
        let page_body = axum::body::to_bytes(page.into_body(), 64 * 1024)
            .await
            .unwrap();
        assert!(page_body.windows(7).any(|window| window == b"tracker"));
        let tools = app
            .oneshot(
                Request::builder()
                    .uri("/api/tools")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let tools_body = axum::body::to_bytes(tools.into_body(), 64 * 1024)
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&tools_body).contains("message_context"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn profile_settings_drop_secrets() {
        let value = public_settings(r#"{"token":"xoxb-secret","theme":"dark"}"#);
        assert!(value.get("token").is_none());
        assert_eq!(value["theme"], "dark");
    }
}
