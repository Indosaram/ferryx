//! `/api/lanes/lifecycle`: the server-owned sidebar order and its pin / settle / snooze /
//! archive writes, as plain JSON over the hub's own axum router. State comes from the outer
//! [`crate::http::HttpState`] this router is nested into (it carries the pool), which is how
//! every other surface on that router is wired: no separate state type, no `with_state` here.
//!
//! The read path is one call: `GET /api/lanes/lifecycle` answers the whole model, sections by
//! workspace and rows in render order, so a client renders from it instead of a local order.
//!
//! Writes are deliberately narrow. A state change is `POST /api/lanes/lifecycle/apply`; the
//! desktop holds the 5 s undo window and simply does not call it while the window is open, so a
//! cancelled action costs zero rows. `expectedRevision` makes a raced write a 409 instead of a
//! lost update.
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::SqlitePool;

use super::lifecycle::{
    self, Applied, LaneLifecycle, LaneLifecycleModel, LifecycleOp, DEFAULT_SNOOZE_SECONDS,
};
use crate::error::{HubError, Result};
use crate::http::HttpState;

pub fn router() -> Router<HttpState> {
    Router::new()
        .route("/", get(read).post(apply))
        .route("/apply", post(apply))
        .route("/pin-tabs", post(pin_tabs))
        .route("/import", post(import))
        .route("/order", post(order))
        .route("/identities", post(identities))
}

type Reply = (StatusCode, Json<Value>);

/// Desktop IPC uses the same handlers and wire bodies as HTTP, against the daemon-owned pool.
/// This is deliberately limited to the lifecycle surface, not a general HTTP proxy.
pub async fn desktop_request(pool: SqlitePool, operation: &str, body: Value) -> (u16, Value) {
    let state = State(HttpState { pool, web_root: std::path::PathBuf::new() });
    let reply = match operation {
        "read" => read(state).await,
        "apply" => match serde_json::from_value(body) {
            Ok(body) => apply(state, Json(body)).await,
            Err(error) => fail(StatusCode::BAD_REQUEST, error.to_string()),
        },
        "pin-tabs" => match serde_json::from_value(body) {
            Ok(body) => pin_tabs(state, Json(body)).await,
            Err(error) => fail(StatusCode::BAD_REQUEST, error.to_string()),
        },
        "import" => match serde_json::from_value(body) {
            Ok(body) => import(state, Json(body)).await,
            Err(error) => fail(StatusCode::BAD_REQUEST, error.to_string()),
        },
        "identities" => match serde_json::from_value(body) {
            Ok(body) => identities(state, Json(body)).await,
            Err(error) => fail(StatusCode::BAD_REQUEST, error.to_string()),
        },
        "order" => match serde_json::from_value(body) {
            Ok(body) => order(state, Json(body)).await,
            Err(error) => fail(StatusCode::BAD_REQUEST, error.to_string()),
        },
        _ => fail(StatusCode::BAD_REQUEST, "unknown lifecycle operation"),
    };
    (reply.0.as_u16(), reply.1.0)
}

fn fail(status: StatusCode, message: impl Into<String>) -> Reply {
    (status, Json(json!({"error": message.into()})))
}

fn internal(error: HubError) -> Reply {
    fail(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

fn model_value(model: &LaneLifecycleModel) -> Value {
    json!({ "sections": model.sections, "revision": model.revision })
}

fn row_value(row: &LaneLifecycle) -> Value {
    json!(row)
}

/// `GET /api/lanes/lifecycle`: every live row in render order. Archived rows are left out; a
/// row's own state stays readable one at a time through the same read's `archived` write answer.
async fn read(State(state): State<HttpState>) -> Reply {
    match lifecycle::model(&state.pool, &lifecycle::now()).await {
        Ok(model) => (StatusCode::OK, Json(model_value(&model))),
        Err(error) => internal(error),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApplyBody {
    worktree_key: String,
    #[serde(default)]
    workspace_id: String,
    #[serde(default)]
    path: String,
    action: String,
    #[serde(default)]
    pinned: bool,
    #[serde(default)]
    until: Option<String>,
    #[serde(default)]
    expected_revision: Option<i64>,
}

/// `POST /api/lanes/lifecycle/apply`: one state change. 200 with the row, 409 with the row the
/// server actually holds when `expectedRevision` is behind (the undo guard), 400 for a bad
/// action or key.
async fn apply(State(state): State<HttpState>, Json(body): Json<ApplyBody>) -> Reply {
    let Some(op) = LifecycleOp::parse(&body.action, body.pinned, body.until.clone()) else {
        return fail(
            StatusCode::BAD_REQUEST,
            "action must be pin, settle, snooze, archive or activate",
        );
    };
    if let LifecycleOp::Snooze { until: Some(until) } = &op {
        if chrono::DateTime::parse_from_rfc3339(until).is_err() {
            return fail(StatusCode::BAD_REQUEST, "until must be an RFC 3339 timestamp");
        }
    }
    let now = lifecycle::now();
    match lifecycle::apply(
        &state.pool,
        &body.worktree_key,
        &body.workspace_id,
        &body.path,
        &op,
        body.expected_revision,
        &now,
    )
    .await
    {
        Ok(Applied::Written(row)) => (StatusCode::OK, Json(json!({ "row": row_value(&row), "stale": false }))),
        Ok(Applied::Stale(row)) => (
            StatusCode::CONFLICT,
            Json(json!({ "row": row_value(&row), "stale": true, "error": "the row moved on" })),
        ),
        Err(error) => fail(StatusCode::BAD_REQUEST, error.to_string()),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PinTabsBody {
    worktree_key: String,
    #[serde(default)]
    workspace_id: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    pinned_tabs: Vec<String>,
}

/// `POST /api/lanes/lifecycle/pin-tabs`: replaces one row's folded pin set.
///
/// Only ids that belong to the row reach here: the client folds `SET_TAB_PINNED` under the owning
/// worktree before calling, so the server never has to know what a tab is. An empty set is how a
/// row stops reading `pinned`.
async fn pin_tabs(State(state): State<HttpState>, Json(body): Json<PinTabsBody>) -> Reply {
    let now = lifecycle::now();
    // A pin set is a set: duplicates would make the list grow without bound on a double toggle.
    let mut pinned = body.pinned_tabs;
    pinned.sort();
    pinned.dedup();
    match lifecycle::set_pinned_tabs(
        &state.pool,
        &body.worktree_key,
        &body.workspace_id,
        &body.path,
        &pinned,
        &now,
    )
    .await
    {
        Ok(row) => (StatusCode::OK, Json(row_value(&row))),
        Err(error) => fail(StatusCode::BAD_REQUEST, error.to_string()),
    }
}

#[derive(Deserialize)]
struct ImportBody {
    #[serde(default)]
    order: std::collections::BTreeMap<String, Vec<String>>,
}

/// `POST /api/lanes/lifecycle/import`: the ONE-TIME handover of the desktop's `localStorage`
/// order. `{imported: false}` after the first time; nothing changes then.
async fn import(State(state): State<HttpState>, Json(body): Json<ImportBody>) -> Reply {
    let now = lifecycle::now();
    match lifecycle::import_local_order(&state.pool, &body.order, &now).await {
        Ok(imported) => {
            let model = lifecycle::model(&state.pool, &now).await.ok();
            (
                StatusCode::OK,
                Json(json!({ "imported": imported, "model": model.as_ref().map(model_value) })),
            )
        }
        Err(error) => fail(StatusCode::BAD_REQUEST, error.to_string()),
    }
}

/// Resolve path-only imports using authoritative identities from the sidebar's worktree list.
async fn identities(
    State(state): State<HttpState>,
    Json(rows): Json<Vec<lifecycle::ListedIdentity>>,
) -> Reply {
    match lifecycle::reconcile_identities(&state.pool, &rows).await {
        Ok(()) => match lifecycle::model(&state.pool, &lifecycle::now()).await {
            Ok(model) => (StatusCode::OK, Json(model_value(&model))),
            Err(error) => internal(error),
        },
        Err(error) => fail(StatusCode::BAD_REQUEST, error.to_string()),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OrderBody {
    workspace_id: String,
    #[serde(default)]
    identifiers: Vec<String>,
}

/// `POST /api/lanes/lifecycle/order`: the server-owned section order after a drag.
///
/// `identifiers` are what the drag handler had: a lifecycle key, a `sidebar-worktree:` sortable
/// id, or a bare path. Each is resolved to one stored row - exactly, then by its trailing path -
/// and a path nothing claims becomes a `p:<workspace>/<path>` row, so the first drag on a fresh
/// install still orders. The listing's identity reconciliation promotes provisional path rows
/// before rendering, without waiting for a lifecycle action.
async fn order(State(state): State<HttpState>, Json(body): Json<OrderBody>) -> Reply {
    let now = lifecycle::now();
    let mut lanes = Vec::new();
    for identifier in &body.identifiers {
        match resolve_identifier(&state.pool, &body.workspace_id, identifier).await {
            Ok(Some(key)) => lanes.push(key),
            // An identifier naming no row at all is dropped, never guessed into a key of a state
            // no read would return.
            Ok(None) => {}
            Err(error) => return internal(error),
        }
    }
    match lifecycle::reorder(&state.pool, &body.workspace_id, &lanes, &now).await {
        Ok(()) => match lifecycle::model(&state.pool, &now).await {
            Ok(model) => (StatusCode::OK, Json(model_value(&model))),
            Err(error) => internal(error),
        },
        Err(error) => fail(StatusCode::BAD_REQUEST, error.to_string()),
    }
}

/// The lifecycle key an order identifier names, if any.
///
/// The identifier -> key step itself is [`lifecycle::key_for_identifier`], shared with the import
/// so the two cannot mint two keys for one row. This adds only what the drag path needs beyond
/// that: a stored row whose `worktree_path` is this identifier's path wins over the derived key,
/// because it is the row that already carries the owner's state and order.
async fn resolve_identifier(pool: &SqlitePool, workspace_id: &str, identifier: &str) -> Result<Option<String>> {
    let trimmed = identifier.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    // A key is already exact: the path lookup below exists only to map a *path* onto the row
    // that owns it, and letting it run for a key could hand back a different row.
    if lifecycle::valid_key(trimmed) {
        return Ok(Some(trimmed.to_owned()));
    }
    let path = lifecycle::identifier_path(trimmed);
    let stored: Option<String> = sqlx::query_scalar(
        "SELECT worktree_key FROM lane_lifecycles
          WHERE worktree_path = ? AND workspace_id = ?
          ORDER BY CASE WHEN worktree_key LIKE 'w:%' THEN 0 ELSE 1 END, revision DESC
          LIMIT 1",
    )
    .bind(path)
    .bind(workspace_id)
    .fetch_optional(pool)
    .await?;
    if let Some(key) = stored {
        return Ok(Some(key));
    }
    // Nothing claimed that path yet: the only honest key left is the derived one, which names no
    // row at all when the path has no segment (a sortable id whose path part is empty) -- that is
    // dropped rather than turned into a 400 for the whole drag.
    Ok(lifecycle::key_for_identifier(workspace_id, trimmed))
}

/// The default snooze length, so a caller can render "Snoozed until ..." before writing.
pub fn default_snooze_seconds() -> i64 {
    DEFAULT_SNOOZE_SECONDS
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    /// The REAL mount: `crate::http::router`, so a test proves the nesting as well as the
    /// handlers. The Host guard needs a loopback Host; a POST with no Origin is same-origin and
    /// passes, which is exactly how the desktop's fetch arrives.
    fn mounted(pool: SqlitePool) -> Router {
        crate::http::router(pool, std::env::temp_dir())
    }

    /// The prefix `crate::http::router` nests this surface at, and the same string the desktop's
    /// transport prepends (`LANE_LIFECYCLE_PATH` in `ui/src/lib/sidebarPersistence.ts`).
    const MOUNT: &str = "/api/lanes/lifecycle";

    /// `uri` is a sub-path of `MOUNT`: `""` is the collection route itself, `/apply` a write.
    ///
    /// The collection route is the bare mount, never `MOUNT/`: axum 0.8 removed the automatic
    /// trailing-slash redirect, so `/api/lanes/lifecycle/` matches no route and falls through to
    /// `static_or_missing`, which answers 404 for every `/api/` path. Asking for `MOUNT/` would
    /// test a URL the desktop never sends -- its read is `httpJson("")`, i.e. exactly `MOUNT`.
    async fn call(app: &Router, method: &str, uri: &str, body: &str) -> (StatusCode, Value) {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(format!("{MOUNT}{uri}"))
                    .header("host", "127.0.0.1:9119")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_owned()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    #[tokio::test]
    async fn desktop_ipc_uses_the_same_lifecycle_handlers_as_http() {
        let pool = crate::storage::memory().await.expect("memory database");
        let body = json!({
            "worktreeKey": "p:ferryx/code/ferryx", "workspaceId": "ferryx",
            "path": "/code/ferryx", "action": "pin", "pinned": true,
            "until": null, "expectedRevision": 0,
        });
        let (status, written) = desktop_request(pool.clone(), "apply", body.clone()).await;
        assert_eq!(status, 200);
        assert_eq!(written["row"]["effectiveState"], "pinned");
        let (status, stale) = desktop_request(pool.clone(), "apply", body).await;
        assert_eq!(status, 409);
        assert_eq!(stale["stale"], true);
        let app = mounted(pool);
        let (status, read) = call(&app, "GET", "", "").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(read["sections"][0]["rows"][0], written["row"]);
    }

    #[tokio::test]
    async fn the_client_pin_row_payload_is_written_read_back_and_ordered_first() {
        let pool = crate::storage::memory().await.unwrap();
        let app = mounted(pool.clone());
        let (status, _) = call(
            &app,
            "POST",
            "/apply",
            r#"{"worktreeKey":"p:ferryx/b","workspaceId":"ferryx","path":"/code/ferryx/b","action":"activate"}"#,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, body) = call(
            &app,
            "POST",
            "/apply",
            // commitLaneLifecycle's actual request, including its initial revision and null until.
            r#"{"worktreeKey":"p:ferryx/a","workspaceId":"ferryx","path":"/code/ferryx/a","action":"pin","pinned":true,"until":null,"expectedRevision":0}"#,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["row"]["effectiveState"], "pinned");
        let (status, body) = call(&app, "GET", "", "").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["sections"][0]["workspaceId"], "ferryx");
        assert_eq!(body["sections"][0]["rows"][0]["worktreeKey"], "p:ferryx/a");
    }

    #[tokio::test]
    async fn an_archive_leaves_the_live_list_and_a_stale_write_conflicts() {
        let pool = crate::storage::memory().await.unwrap();
        let app = mounted(pool.clone());
        let (_, body) = call(
            &app,
            "POST",
            "/apply",
            r#"{"worktreeKey":"p:ferryx/a","workspaceId":"ferryx","path":"/a","action":"snooze"}"#,
        )
        .await;
        assert_eq!(body["row"]["effectiveState"], "snoozed");
        let revision = body["row"]["revision"].as_i64().unwrap();
        let (status, body) = call(
            &app,
            "POST",
            "/apply",
            r#"{"worktreeKey":"p:ferryx/a","workspaceId":"ferryx","path":"/a","action":"archive","expectedRevision":0}"#,
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["stale"], true);
        assert_eq!(body["row"]["state"], "snoozed");
        let (status, _) = call(
            &app,
            "POST",
            "/apply",
            &format!(
                r#"{{"worktreeKey":"p:ferryx/a","workspaceId":"ferryx","path":"/a","action":"archive","expectedRevision":{revision}}}"#
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (_, body) = call(&app, "GET", "", "").await;
        assert_eq!(body["sections"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn a_pin_set_is_written_deduped_and_cleared() {
        let pool = crate::storage::memory().await.unwrap();
        let app = mounted(pool.clone());
        let key = "w:ferryx/polish";
        let body = format!(
            r#"{{"worktreeKey":"{key}","workspaceId":"ferryx","path":"/code/ferryx/polish","pinnedTabs":["tab-2","tab-1","tab-2"]}}"#
        );
        let (status, written) = call(&app, "POST", "/pin-tabs", &body).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(written["pinnedTabs"], json!(["tab-1", "tab-2"]));
        assert_eq!(written["effectiveState"], "pinned");
        let (_, cleared) = call(
            &app,
            "POST",
            "/pin-tabs",
            &format!(r#"{{"worktreeKey":"{key}","workspaceId":"ferryx","path":"/code/ferryx/polish","pinnedTabs":[]}}"#),
        )
        .await;
        assert_eq!(cleared["effectiveState"], "active");
        assert_eq!(cleared["pinnedTabs"], json!([]));
    }

    /// The reader surface, over the REAL mount a client calls: `GET /api/lanes/lifecycle` answers a
    /// row carrying the recorded pull request, and a row with none carries `null`.
    ///
    /// Fails if the row stops serialising `pull_request` (the field would be absent from the JSON
    /// a sidebar reads) or if `model` stops joining it (`pullRequest` would be `null` for the row
    /// that does have one). The second row keeps this from passing on "every row has a PR".
    #[tokio::test]
    async fn a_row_reports_its_pull_request_over_the_http_mount() {
        let pool = crate::storage::memory().await.unwrap();
        // A lane running in the row's directory is what the join rule resolves through.
        sqlx::query(
            "INSERT INTO lanes(key, title, lead_session_key, cwd, session_key, status,
                 done_signals, last_activity_at)
             VALUES('ledger', 'Lane', 'lead', '/code/ledger', 'lead', 'working', '[]',
                    '2026-11-01T00:00:00+00:00')",
        )
        .execute(&pool)
        .await
        .unwrap();
        crate::lanes::pull_request::record(
            &pool,
            &crate::lanes::pull_request::LanePullRequest {
                lane_key: "ledger".to_owned(),
                repo: "acme/widgets".to_owned(),
                pr_number: 12,
                state: "merged".to_owned(),
                url: "https://github.com/acme/widgets/pull/12".to_owned(),
                title: "Fix the parser".to_owned(),
                head_branch: "lane/ledger".to_owned(),
                observed_at: "2026-11-01T00:00:00+00:00".to_owned(),
            },
        )
        .await
        .unwrap();

        let app = mounted(pool.clone());
        // The row the client renders, activated through the same surface it uses.
        let (status, _) = call(
            &app,
            "POST",
            "/apply",
            r#"{"worktreeKey":"w:ferryx/ledger","workspaceId":"ferryx","path":"/code/ledger","action":"activate"}"#,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        call(
            &app,
            "POST",
            "/apply",
            r#"{"worktreeKey":"p:ferryx/code/mine","workspaceId":"ferryx","path":"/code/mine","action":"activate"}"#,
        )
        .await;

        let (status, body) = call(&app, "GET", "", "").await;
        assert_eq!(status, StatusCode::OK);
        let rows = body["sections"][0]["rows"].as_array().unwrap();
        let linked = rows
            .iter()
            .find(|row| row["worktreeKey"] == "w:ferryx/ledger")
            .expect("the linked row is listed");
        assert_eq!(linked["pullRequest"]["prNumber"], 12);
        assert_eq!(linked["pullRequest"]["state"], "merged");
        assert_eq!(linked["pullRequest"]["laneKey"], "ledger");
        assert_eq!(
            linked["pullRequest"]["headBranch"],
            "lane/ledger",
            "the wire names are the camelCase ones the TypeScript view mirrors"
        );
        let unlinked = rows
            .iter()
            .find(|row| row["worktreeKey"] == "p:ferryx/code/mine")
            .expect("the unlinked row is listed");
        assert!(
            unlinked
                .as_object()
                .is_some_and(|fields| fields.contains_key("pullRequest")),
            "the field is present and null, so a client can tell 'no PR' from 'not sent'"
        );
        assert!(unlinked["pullRequest"].is_null());
    }

    #[tokio::test]
    async fn a_bad_action_is_refused_before_any_write() {
        let pool = crate::storage::memory().await.unwrap();
        let app = mounted(pool.clone());
        let (status, _) = call(
            &app,
            "POST",
            "/apply",
            r#"{"worktreeKey":"p:ferryx/a","workspaceId":"ferryx","path":"/a","action":"delete"}"#,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM lane_lifecycles")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    /// The payload the desktop actually sends, which is the point: `localStorage` holds the
    /// identifiers the drag handler had (`worktreeSortableId` output, or a bare path from an
    /// older build), never lifecycle keys. `loadWorktreeOrderRaw`
    /// (`ui/src/lib/sidebarPersistence.ts`) forwards them verbatim, so an import that only accepts
    /// keys 400s on the first real client -- which is exactly what this test pins.
    #[tokio::test]
    async fn the_import_accepts_the_identifiers_local_storage_actually_holds() {
        let pool = crate::storage::memory().await.unwrap();
        let app = mounted(pool.clone());
        // One sortable id (what today's drag writes) and one bare path (what an older build
        // wrote), in the order the owner dragged them.
        let body = r#"{"order":{"ferryx":[
            "sidebar-worktree:ferryx:ferryx:/code/ferryx/b",
            "/code/ferryx/a"]}}"#;
        let (status, first) = call(&app, "POST", "/import", body).await;
        assert_eq!(status, StatusCode::OK, "{first}");
        assert_eq!(first["imported"], true);
        let (_, read) = call(&app, "GET", "", "").await;
        let keys: Vec<&str> = read["sections"][0]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["worktreeKey"].as_str().unwrap())
            .collect();
        // Both identifiers became the same `p:` keys the drag path derives, in the same order.
        assert_eq!(keys, ["p:ferryx/code/ferryx/b", "p:ferryx/code/ferryx/a"]);
    }

    /// Without reconciliation this returns p:path, then a slug action creates a second row.
    /// Checking only row count before that action would miss the render-key mismatch.
    #[tokio::test]
    async fn a_path_import_and_slug_render_are_one_row_over_the_real_surface() {
        let pool = crate::storage::memory().await.expect("memory database");
        let app = mounted(pool.clone());
        let (status, _) = call(&app, "POST", "/import",
            r#"{"order":{"ferryx":["/code/managed","/code/plain"]}}"#).await;
        assert_eq!(status, StatusCode::OK);
        let identities = json!([{
            "worktreeKey": "w:ferryx/managed", "workspaceId": "ferryx", "path": "/code/managed"
        }]);
        let (status, model) = call(&app, "POST", "/identities", &identities.to_string()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(model["sections"][0]["rows"][0]["worktreeKey"], "w:ferryx/managed");
        let (status, _) = call(&app, "POST", "/apply",
            r#"{"worktreeKey":"w:ferryx/managed","workspaceId":"ferryx","path":"/code/managed","action":"activate","expectedRevision":1}"#).await;
        assert_eq!(status, StatusCode::OK);
        // A subsequent legacy path reorder cannot recreate the provisional alias.
        let (status, _) = call(&app, "POST", "/order",
            r#"{"workspaceId":"ferryx","identifiers":["p:ferryx/code/managed","p:ferryx/code/plain"]}"#).await;
        assert_eq!(status, StatusCode::OK);
        let (status, model) = desktop_request(pool.clone(), "identities", identities).await;
        assert_eq!(status, 200);
        let rows = model["sections"][0]["rows"].as_array().expect("rows");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["worktreeKey"], "w:ferryx/managed");
        assert_eq!(rows[0]["worktreePath"], "/code/managed");
        assert_eq!(rows[1]["worktreePath"], "/code/plain");
        let aliases: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM lane_lifecycles WHERE worktree_key = 'p:ferryx/code/managed'"
        ).fetch_one(&pool).await.expect("alias count");
        assert_eq!(aliases, 0);
    }

    #[tokio::test]
    async fn the_import_hands_the_local_order_over_exactly_once() {
        let pool = crate::storage::memory().await.unwrap();
        let app = mounted(pool.clone());
        let body = r#"{"order":{"ferryx":["p:ferryx/b","p:ferryx/a"]}}"#;
        let (status, first) = call(&app, "POST", "/import", body).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(first["imported"], true);
        let (_, second) = call(&app, "POST", "/import", body).await;
        assert_eq!(second["imported"], false);
        let (_, read) = call(&app, "GET", "", "").await;
        let keys: Vec<&str> = read["sections"][0]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["worktreeKey"].as_str().unwrap())
            .collect();
        assert_eq!(keys, ["p:ferryx/b", "p:ferryx/a"]);
    }

    #[tokio::test]
    async fn a_dragged_sortable_id_orders_the_row_by_its_path() {
        let pool = crate::storage::memory().await.unwrap();
        let app = mounted(pool.clone());
        call(
            &app,
            "POST",
            "/apply",
            r#"{"worktreeKey":"p:ferryx/b","workspaceId":"ferryx","path":"/code/ferryx/b","action":"pin","pinned":true}"#,
        )
        .await;
        call(
            &app,
            "POST",
            "/apply",
            r#"{"worktreeKey":"p:ferryx/a","workspaceId":"ferryx","path":"/code/ferryx/a","action":"activate"}"#,
        )
        .await;
        let (status, body) = call(
            &app,
            "POST",
            "/order",
            r#"{"workspaceId":"ferryx","identifiers":[
                "sidebar-worktree:ferryx:ferryx:/code/ferryx/a",
                "sidebar-worktree:ferryx:ferryx:/code/ferryx/b"]}"#,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        // The unpinned row now leads its section even though the pinned one sorts first by state.
        assert_eq!(body["sections"][0]["rows"][0]["worktreeKey"], "p:ferryx/b");
        let positions: Vec<i64> = body["sections"][0]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["position"].as_i64().unwrap())
            .collect();
        assert_eq!(positions, [1, 0]);
    }
}
