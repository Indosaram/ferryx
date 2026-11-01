//! The Supervisor view over the gateway: a paired phone reads the same snapshot the desktop
//! shows and pauses or resumes one lane. Changes reach it as `supervisorChanged` machine events.
//!
//! Access follows agent control: a mirror device is refused outright, a view-only machine device
//! may read, and only a machine Control device may pause. The lane key travels in the body
//! because the relay's path segments do not admit every key a person might name.

use super::*;

use crate::hub_runtime::{HubStartError, LanePauseError};

/// Capability a machine Control device sees while this daemon has the hub open.
pub(crate) const SUPERVISOR_CAPABILITY: &str = "supervisorV1";

/// The wire code an ended lane (closed, failed or verified done) is refused with.
pub(crate) const SUPERVISOR_INVALID_STATE_CODE: &str = "INVALID_STATE";

/// The wire code a daemon without an open hub answers with, as the desktop's own request does.
pub(crate) const SUPERVISOR_UNAVAILABLE_CODE: &str = "SUPERVISOR_UNAVAILABLE";

/// The wire code a standby daemon answers with: another machine holds the supervisor lease.
pub(crate) const SUPERVISOR_STANDBY_CODE: &str = "SUPERVISOR_STANDBY";

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct PauseBody {
    lane_key: String,
    paused: bool,
}

pub(crate) async fn http_snapshot(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = agent_control_service::http_device(&state, &headers, false) {
        return response;
    }
    (
        StatusCode::OK,
        [(header::CACHE_CONTROL, "no-store")],
        Json(crate::hub_runtime::supervisor_snapshot(&state).await),
    )
        .into_response()
}

pub(crate) async fn http_pause(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    body: Result<Json<PauseBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let device = match agent_control_service::http_device(&state, &headers, true) {
        Ok(device) => device,
        Err(response) => return response,
    };
    let Ok(Json(body)) = body else {
        return reference_chat_error(
            ScopeErrorCode::InvalidRequest,
            "the pause body must be {laneKey, paused}",
        );
    };
    let by = format!("remote:{}", device.id);
    match crate::hub_runtime::set_lane_paused(&state, &body.lane_key, body.paused, &by).await {
        Ok(changed) => (
            StatusCode::OK,
            [(header::CACHE_CONTROL, "no-store")],
            Json(serde_json::json!({
                "laneKey": body.lane_key,
                "paused": body.paused,
                "changed": changed,
            })),
        )
            .into_response(),
        Err(error) => pause_refusal(&error),
    }
}

/// The machine error envelope one pause refusal answers with.
fn pause_refusal(error: &LanePauseError) -> Response {
    let message = error.to_string();
    let (status, code) = match error {
        LanePauseError::Unavailable => {
            (StatusCode::SERVICE_UNAVAILABLE, SUPERVISOR_UNAVAILABLE_CODE)
        }
        LanePauseError::NotFound(_) => (StatusCode::NOT_FOUND, "NOT_FOUND"),
        LanePauseError::Ended(_) => (StatusCode::CONFLICT, SUPERVISOR_INVALID_STATE_CODE),
        LanePauseError::Storage(_) => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR"),
        LanePauseError::Supervisor(failure) => return service_refusal(failure),
    };
    machine_error_with_details(status, code, &message, serde_json::Map::new())
}

/// `POST /api/v1/supervisor/start`: start the stopped hub, as `ferryx hub start` does. No body.
/// Only a machine Control device the agent-control policy admits may start it; the daemon is
/// never restarted, so live PTY sessions stay up.
pub(crate) async fn http_start(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
) -> Response {
    let device = match agent_control_service::http_device(&state, &headers, true) {
        Ok(device) => device,
        Err(response) => return response,
    };
    let by = format!("remote:{}", device.id);
    match crate::hub_runtime::start_supervisor(&state, &by).await {
        Ok(started) => (
            StatusCode::OK,
            [(header::CACHE_CONTROL, "no-store")],
            Json(serde_json::json!({ "started": started })),
        )
            .into_response(),
        Err(error) => start_refusal(&error),
    }
}

/// The machine error envelope one start refusal answers with. A standby names the holder in
/// `details` as `{holderMachineId, holderLabel}`.
fn start_refusal(error: &HubStartError) -> Response {
    let mut details = serde_json::Map::new();
    let (status, code) = match error {
        HubStartError::Unavailable => (StatusCode::SERVICE_UNAVAILABLE, SUPERVISOR_UNAVAILABLE_CODE),
        HubStartError::HandedOver => (StatusCode::CONFLICT, SUPERVISOR_INVALID_STATE_CODE),
        HubStartError::Storage(_) => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR"),
        HubStartError::Standby { holder_machine_id, holder_label } => {
            details.insert("holderMachineId".into(), serde_json::json!(holder_machine_id));
            details.insert("holderLabel".into(), serde_json::json!(holder_label));
            (StatusCode::CONFLICT, SUPERVISOR_STANDBY_CODE)
        }
    };
    machine_error_with_details(status, code, &error.to_string(), details)
}

/// The machine error envelope a shared supervisor service refusal answers with: its own HTTP
/// status (400/403/404/409/503) and stable code.
fn service_refusal(failure: &ferryx_hub::supervisor::request::RequestFailure) -> Response {
    let status = StatusCode::from_u16(failure.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mut details = serde_json::Map::new();
    if let Some((holder_machine_id, holder_label)) = &failure.holder {
        details.insert("holderMachineId".into(), serde_json::json!(holder_machine_id));
        details.insert("holderLabel".into(), serde_json::json!(holder_label));
    }
    machine_error_with_details(
        status,
        &failure.code,
        &failure.message,
        details,
    )
}

fn no_store(value: serde_json::Value) -> Response {
    (
        StatusCode::OK,
        [(header::CACHE_CONTROL, "no-store")],
        Json(value),
    )
        .into_response()
}

/// `POST /api/v1/supervisor/request`: one unified request envelope. Mirror devices are refused
/// (403) and only machine Control devices the agent-control policy admits may send it; the
/// device never names the actor, the daemon's owner identity is used.
pub(crate) async fn http_request(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    body: Result<Json<serde_json::Value>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let device = match agent_control_service::http_device(&state, &headers, true) {
        Ok(device) => device,
        Err(response) => return response,
    };
    let Ok(Json(envelope)) = body else {
        return reference_chat_error(
            ScopeErrorCode::InvalidRequest,
            "the body must be a supervisor request envelope",
        );
    };
    let caller = crate::hub_runtime::SupervisorCaller::Remote {
        device_id: device.id.clone(),
    };
    match crate::hub_runtime::supervisor_request(&state, caller, envelope).await {
        Ok(receipt) => no_store(receipt),
        Err(failure) => service_refusal(&failure),
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct ConversationQuery {
    project_id: String,
    #[serde(default)]
    after: Option<i64>,
}

/// `GET /api/v1/supervisor/conversation?projectId=...&after=...`.
pub(crate) async fn http_conversation(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    query: Result<
        axum::extract::Query<ConversationQuery>,
        axum::extract::rejection::QueryRejection,
    >,
) -> Response {
    let device = match agent_control_service::http_device(&state, &headers, false) {
        Ok(device) => device,
        Err(response) => return response,
    };
    let Ok(axum::extract::Query(query)) = query else {
        return reference_chat_error(
            ScopeErrorCode::InvalidRequest,
            "the query must be projectId[&after]",
        );
    };
    let caller = crate::hub_runtime::SupervisorCaller::Remote {
        device_id: device.id.clone(),
    };
    match crate::hub_runtime::supervisor_conversation(
        &state,
        caller,
        &query.project_id,
        query.after.unwrap_or(0),
    )
    .await
    {
        Ok(page) => no_store(page),
        Err(failure) => service_refusal(&failure),
    }
}

/// `GET /api/v1/supervisor/memory`: the owner's native memory receipts with their provenance.
/// Without the engine in this daemon it is 503 `MEMORY_UNAVAILABLE`, never an empty success.
pub(crate) async fn http_memory(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
) -> Response {
    let device = match agent_control_service::http_device(&state, &headers, false) {
        Ok(device) => device,
        Err(response) => return response,
    };
    let caller = crate::hub_runtime::SupervisorCaller::Remote {
        device_id: device.id.clone(),
    };
    match crate::hub_runtime::supervisor_memory(&state, caller).await {
        Ok(value) => no_store(value),
        Err(failure) => service_refusal(&failure),
    }
}

/// `POST /api/v1/supervisor/memory`: a `teach` or `forget` envelope, served like any request.
pub(crate) async fn http_memory_post(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    body: Result<Json<serde_json::Value>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let device = match agent_control_service::http_device(&state, &headers, true) {
        Ok(device) => device,
        Err(response) => return response,
    };
    let Ok(Json(envelope)) = body else {
        return reference_chat_error(
            ScopeErrorCode::InvalidRequest,
            "the body must be a teach or forget envelope",
        );
    };
    if !matches!(
        envelope.get("action").and_then(serde_json::Value::as_str),
        Some("teach" | "forget")
    ) {
        return reference_chat_error(
            ScopeErrorCode::InvalidRequest,
            "the memory route takes only teach or forget",
        );
    }
    let caller = crate::hub_runtime::SupervisorCaller::Remote {
        device_id: device.id.clone(),
    };
    match crate::hub_runtime::supervisor_request(&state, caller, envelope).await {
        Ok(receipt) => no_store(receipt),
        Err(failure) => service_refusal(&failure),
    }
}

#[cfg(test)]
#[path = "supervisor_api_tests.rs"]
mod tests;
