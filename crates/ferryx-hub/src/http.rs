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

use crate::dashboard_api as api;
use crate::readiness::classify_disk_pressure;
use crate::storage::now_rfc3339;

#[derive(Clone)]
pub struct HttpState {
    pub pool: SqlitePool,
    pub web_root: PathBuf,
}

pub fn router(pool: SqlitePool, web_root: PathBuf) -> Router {
    api::mark_started();
    let tracker = crate::tracker_linear::router().with_state(pool.clone());
    Router::new()
        .route("/api/health", get(api::health))
        .route("/api/ready", get(ready))
        .route("/api/readiness", get(ready))
        .route("/api/status", get(api::status))
        .route("/api/sessions", get(api::list_sessions))
        .route("/api/sessions/{id}", get(api::get_session).delete(api::delete_session))
        .route("/api/sessions/{id}/messages", get(api::list_messages))
        .route("/api/sessions/{id}/chat", post(api::post_chat))
        .route("/api/sessions/{id}/stop", post(api::stop_session))
        .route("/api/sessions/{id}/ws", get(api::session_ws))
        .route("/api/cron/jobs", get(api::list_cron_jobs).post(api::create_cron_job))
        .route(
            "/api/cron/jobs/{id}",
            get(api::get_cron_job).put(api::update_cron_job).delete(api::delete_cron_job),
        )
        .route("/api/cron/jobs/{id}/trigger", post(trigger_cron))
        .route("/api/cron/jobs/{id}/pause", post(api::pause_cron_job))
        .route("/api/cron/jobs/{id}/resume", post(api::resume_cron_job))
        .route("/api/cron/runs", get(api::list_cron_runs))
        .route("/api/approvals/pending", get(api::pending_approvals))
        .route("/api/approvals/{id}/resolve", post(resolve_approval))
        .route("/api/approvals/allowlist", get(api::allowlist))
        .route("/api/autonomy/precedents", get(api::precedents))
        .route("/api/bots", get(api::list_bots).post(api::create_bot))
        .route("/api/bots/{id}", get(api::get_bot).put(api::update_bot).delete(api::delete_bot))
        .route("/api/memory", get(api::memory))
        .route("/api/tools", get(tools))
        .route("/api/skills", get(skills))
        .route("/api/config", get(config))
        .route("/api/logs", get(logs))
        .route("/api/logs/ws", get(logs_ws))
        .nest("/api/tracker", tracker)
        .fallback(static_or_missing)
        .layer(axum::middleware::from_fn(api::validate_host_and_origin))
        .with_state(HttpState { pool, web_root })
}

pub fn web_root() -> PathBuf {
    api::web_root_from(
        crate::hub_env::var_os("FERRYX_HUB_WEB_ROOT"),
        crate::hub_env::var_os("DASHBOARD_WEB_ROOT"),
        crate::hub_env::var_os("HOME"),
    )
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

async fn trigger_cron(State(state): State<HttpState>, UrlPath(id): UrlPath<String>) -> Response {
    // Same gate as the run-now socket: a publishing job is not rerun from the dashboard.
    let stored: Option<(String, i64)> =
        match sqlx::query_as("SELECT payload_json, enabled FROM scheduled_jobs WHERE id = ?")
            .bind(&id)
            .fetch_optional(&state.pool)
            .await
        {
            Ok(stored) => stored,
            Err(error) => {
                return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
            }
        };
    let Some((payload_json, enabled)) = stored else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "cron job not found"})),
        )
            .into_response();
    };
    let payload: Value = serde_json::from_str(&payload_json).unwrap_or(Value::Null);
    if crate::schedule::publishing(&id, &payload) {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error": "publishing rerun requires owner approval through remediation"})),
        )
            .into_response();
    }
    // A paused job stays paused. Triggering it used to set enabled = 1, so one click
    // silently resumed its schedule for good. Resume it, or run it once on the cron
    // socket with {"id":...,"force":true}.
    if enabled != 1 {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error": "cron job is paused; resume it first or run it once with force on the cron socket"})),
        )
            .into_response();
    }
    let now = now_rfc3339();
    let _ = sqlx::query(
        "UPDATE scheduled_jobs SET next_run_at = ?, updated_at = ? WHERE id = ? AND enabled = 1",
    )
    .bind(&now)
    .bind(&now)
    .bind(&id)
    .execute(&state.pool)
    .await;
    crate::schedule::poke_scheduler();
    Json(json!({"id": id, "next_run_at": now})).into_response()
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

async fn tools() -> Json<Value> {
    Json(json!({"tools": crate::context::TOOLS}))
}

async fn skills() -> Json<Value> {
    let names = crate::tools::list_skills(crate::tools::skills_root(&json!({})));
    Json(json!({"skills": names}))
}

fn log_bus() -> &'static tokio::sync::broadcast::Sender<String> {
    static BUS: std::sync::OnceLock<tokio::sync::broadcast::Sender<String>> =
        std::sync::OnceLock::new();
    BUS.get_or_init(|| tokio::sync::broadcast::channel(256).0)
}

pub async fn record_log(pool: &SqlitePool, level: &str, message: &str) -> Result<(), sqlx::Error> {
    let at = now_rfc3339();
    let id = sqlx::query("INSERT INTO hub_log(at, level, message) VALUES(?,?,?)")
        .bind(&at)
        .bind(level)
        .bind(message)
        .execute(pool)
        .await?
        .last_insert_rowid();
    let entry = json!({"id": id, "at": at, "level": level, "message": message}).to_string();
    let _ = log_bus().send(entry);
    Ok(())
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

async fn logs_ws(ws: WebSocketUpgrade, State(state): State<HttpState>) -> Response {
    ws.on_upgrade(move |socket| handle_logs_socket(socket, state.pool))
}

async fn handle_logs_socket(mut socket: axum::extract::ws::WebSocket, pool: SqlitePool) {
    let mut receiver = log_bus().subscribe();
    if socket
        .send(Message::Text(r#"{"kind":"ready"}"#.into()))
        .await
        .is_err()
    {
        return;
    }
    let rows: Vec<(i64, String, String, String)> =
        sqlx::query_as("SELECT id, at, level, message FROM hub_log ORDER BY id DESC LIMIT 100")
            .fetch_all(&pool)
            .await
            .unwrap_or_default();
    let mut last_sent = 0_i64;
    for (id, at, level, message) in rows.into_iter().rev() {
        last_sent = id;
        let text =
            json!({"type":"log","entry":{"id": id, "at": at, "level": level, "message": message}})
                .to_string();
        if socket.send(Message::Text(text.into())).await.is_err() {
            return;
        }
    }
    loop {
        tokio::select! {
            entry = receiver.recv() => {
                match entry {
                    Ok(entry) => {
                        let parsed: Value = serde_json::from_str(&entry).unwrap_or(Value::String(entry));
                        let id = parsed.get("id").and_then(Value::as_i64).unwrap_or(0);
                        if id <= last_sent {
                            continue;
                        }
                        last_sent = id;
                        let text = json!({"type":"log","entry": parsed}).to_string();
                        if socket.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        let text = json!({"type":"warning","message": format!("skipped {skipped} log entries")}).to_string();
                        if socket.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Ping(payload))) => {
                        if socket.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                    _ => {}
                }
            }
        }
    }
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
                    .uri("http://127.0.0.1/api/health")
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
                    .uri("http://127.0.0.1/api/ready")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ready.status(), StatusCode::OK);
        let created = app.clone().oneshot(
            Request::builder().method("POST").uri("http://127.0.0.1/api/cron/jobs").header("content-type", "application/json")
                .body(Body::from(r#"{"id":"daily","expression":"every 15m","payload":{"deliver":"slack:C1"}}"#)).unwrap(),
        ).await.unwrap();
        assert_eq!(created.status(), StatusCode::CREATED);
        let jobs = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("http://127.0.0.1/api/cron/jobs")
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
                    .uri("http://127.0.0.1/api/approvals/pending")
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
                    .uri(format!("http://127.0.0.1/api/approvals/{id}/resolve"))
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
                    .uri("http://127.0.0.1/api/bots")
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
            .oneshot(Request::builder().uri("http://127.0.0.1/").body(Body::empty()).unwrap())
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
                    .uri("http://127.0.0.1/api/tools")
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

    #[tokio::test]
    async fn the_dashboard_trigger_keeps_the_publishing_gate() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;
        let pool = crate::storage::memory().await.unwrap();
        sqlx::query("INSERT INTO scheduled_jobs(id, expression, payload_json, enabled, next_run_at) VALUES('omon-playstore-daily-publish','0 0 * * *','{}',0,'2099-01-01T00:00:00Z')")
            .execute(&pool).await.unwrap();
        let app = router(pool.clone(), std::env::temp_dir());
        for (id, want) in [
            ("omon-playstore-daily-publish", StatusCode::CONFLICT),
            ("missing", StatusCode::NOT_FOUND),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("http://127.0.0.1/api/cron/jobs/{id}/trigger"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), want, "{id}");
        }
        let (enabled, next): (i64, String) = sqlx::query_as("SELECT enabled, next_run_at FROM scheduled_jobs WHERE id = 'omon-playstore-daily-publish'")
            .fetch_one(&pool).await.unwrap();
        assert_eq!((enabled, next.as_str()), (0, "2099-01-01T00:00:00Z"));
    }

    #[tokio::test]
    async fn the_dashboard_trigger_leaves_a_paused_job_paused() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;
        let pool = crate::storage::memory().await.unwrap();
        sqlx::query("INSERT INTO scheduled_jobs(id, expression, payload_json, enabled, next_run_at) VALUES('paused','0 0 * * *','{\"command\":\"true\"}',0,'2099-01-01T00:00:00Z'), ('live','0 0 * * *','{\"command\":\"true\"}',1,'2099-01-01T00:00:00Z')")
            .execute(&pool).await.unwrap();
        let app = router(pool.clone(), std::env::temp_dir());
        for (id, want) in [("paused", StatusCode::CONFLICT), ("live", StatusCode::OK)] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("http://127.0.0.1/api/cron/jobs/{id}/trigger"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), want, "{id}");
        }
        let rows: Vec<(String, i64, String)> =
            sqlx::query_as("SELECT id, enabled, next_run_at FROM scheduled_jobs ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(
            (rows[0].1, rows[0].2.as_str() < "2099"),
            (1, true),
            "live job is due now"
        );
        assert_eq!(
            (rows[1].1, rows[1].2.as_str()),
            (0, "2099-01-01T00:00:00Z"),
            "paused job untouched"
        );
    }
}
