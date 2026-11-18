//! `GET`/`PUT /api/v1/workspace/layout`: the revisioned layout document a paired client mirrors.
//!
//! Both handlers answer from the in-process store rather than a daemon round trip: the GET is a
//! long-poll that parks up to 25 s, which a shared daemon connection and a project admission slot
//! cannot afford to hold.
use super::*;

/// Query string of the layout long-poll.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LayoutQuery {
    workspace_id: String,
    after_revision: Option<u64>,
}

/// `GET /api/v1/workspace/layout?workspaceId=..&afterRevision=..`
pub async fn get(
    State(state): State<Arc<RemoteGatewayState>>,
    axum::extract::Query(query): axum::extract::Query<LayoutQuery>,
    headers: HeaderMap,
) -> Response {
    let id = uuid::Uuid::new_v4().to_string();
    let device = match authorize(&state, &headers) {
        Ok(device) => device,
        Err(e) => return failure(&e, &id),
    };
    if query.workspace_id.trim().is_empty() {
        return failure("INVALID_REQUEST", &id);
    }
    let sessions = state
        .machine_services
        .as_ref()
        .expect("authorized services")
        .sessions
        .clone();
    let Some(after_revision) = query.after_revision else {
        return response(200, serde_json::json!({"layout": sessions.layout(&query.workspace_id)}));
    };
    let revoked = match state.auth_manager.device_revocation(&device) {
        Ok(revoked) => revoked,
        Err(_) => return failure("UNAUTHORIZED", &id),
    };
    // A parked poll must end when the device is revoked, not 25 s later.
    tokio::select! {
        biased;
        _ = wait_for_revocation(revoked) => failure("UNAUTHORIZED", &id),
        layout = sessions.wait_layout(
            &query.workspace_id,
            after_revision,
            crate::daemon::session_service::LAYOUT_LONG_POLL_TIMEOUT,
        ) => response(200, serde_json::json!({"layout": layout})),
    }
}

/// `PUT /api/v1/workspace/layout` with `{workspaceId, baseRevision, tabs}`.
pub async fn put(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let id = uuid::Uuid::new_v4().to_string();
    if let Err(e) = authorize(&state, &headers) {
        return failure(&e, &id);
    }
    let request: LayoutPutRequest = match decode_json(&body, MACHINE_JSON_MAX_BYTES) {
        Ok(request) => request,
        Err(DecodeError::PayloadTooLarge) => return failure("PAYLOAD_TOO_LARGE", &id),
        Err(DecodeError::InvalidRequest(_)) => return failure("INVALID_REQUEST", &id),
    };
    let sessions = state
        .machine_services
        .as_ref()
        .expect("authorized services")
        .sessions
        .clone();
    match sessions.put_layout(&request.workspace_id, request.base_revision, request.tabs) {
        Ok(layout) => response(200, serde_json::json!({"layout": layout})),
        // A stale base carries the stored document, so the client rebases without a second read.
        Err(layout) => response(
            409,
            serde_json::json!({"layout": layout, "conflict": true}),
        ),
    }
}

async fn wait_for_revocation(mut revoked: tokio::sync::watch::Receiver<bool>) {
    let _ = revoked.wait_for(|revoked| *revoked).await;
}
