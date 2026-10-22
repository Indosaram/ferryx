//! Owner-only tracker. A missing token is unauthorized. A wrong token is forbidden.
use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::{from_fn_with_state, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::SqlitePool;

use crate::error::Result;
use crate::storage::now_rfc3339;

#[derive(Clone)]
pub struct Tracker {
    pool: SqlitePool,
    owner_token: String,
}

impl Tracker {
    pub fn new(pool: SqlitePool, owner_token: impl Into<String>) -> Arc<Self> {
        Arc::new(Self {
            pool,
            owner_token: owner_token.into(),
        })
    }

    pub fn router(self: &Arc<Self>) -> Router {
        Router::new()
            .route("/api/tracker/issues", get(list_issues).post(create_issue))
            .layer(from_fn_with_state(Arc::clone(self), authorize))
            .with_state(Arc::clone(self))
    }

    pub async fn list(&self) -> Result<Vec<Value>> {
        let rows: Vec<(i64, String, String, String)> =
            sqlx::query_as("SELECT id, title, body, status FROM hub_tracker_issues ORDER BY id")
                .fetch_all(&self.pool)
                .await?;
        Ok(rows
            .into_iter()
            .map(|(id, title, body, status)| json!({"id": id, "title": title, "body": body, "status": status}))
            .collect())
    }

    pub async fn create(&self, title: &str, body: &str) -> Result<i64> {
        let done = sqlx::query(
            "INSERT INTO hub_tracker_issues(title, body, status, created_at) VALUES(?,?, 'open', ?)",
        )
        .bind(title)
        .bind(body)
        .bind(now_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(done.last_insert_rowid())
    }
}

async fn authorize(State(tracker): State<Arc<Tracker>>, request: Request, next: Next) -> Response {
    let presented = bearer(request.headers());
    match presented {
        Some(token) if token == tracker.owner_token && !tracker.owner_token.is_empty() => {
            next.run(request).await
        }
        Some(_) => StatusCode::FORBIDDEN.into_response(),
        None => StatusCode::UNAUTHORIZED.into_response(),
    }
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
}

async fn list_issues(State(tracker): State<Arc<Tracker>>) -> Response {
    match tracker.list().await {
        Ok(issues) => Json(json!({"issues": issues})).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct NewIssue {
    title: String,
    #[serde(default)]
    body: String,
}

async fn create_issue(
    State(tracker): State<Arc<Tracker>>,
    Json(issue): Json<NewIssue>,
) -> Response {
    match tracker.create(&issue.title, &issue.body).await {
        Ok(id) => (StatusCode::CREATED, Json(json!({"id": id}))).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    #[tokio::test]
    async fn the_owner_can_create_an_issue_and_a_stranger_cannot() {
        let pool = crate::storage::memory().await.unwrap();
        let tracker = Tracker::new(pool, "owner-token");
        let app = tracker.router();
        let denied = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/tracker/issues")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
        let forbidden = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/tracker/issues")
                    .header(header::AUTHORIZATION, "Bearer other")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);
        let created = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/tracker/issues")
                    .header(header::AUTHORIZATION, "Bearer owner-token")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"title":"fix the gate"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(created.status(), StatusCode::CREATED);
        let listed = app
            .oneshot(
                Request::builder()
                    .uri("/api/tracker/issues")
                    .header(header::AUTHORIZATION, "Bearer owner-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(listed.status(), StatusCode::OK);
    }
}
