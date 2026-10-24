//! The Supervisor view over the gateway: a paired phone reads the same snapshot the desktop
//! shows and pauses or resumes one lane. Changes reach it as `supervisorChanged` machine events.
//!
//! Access follows agent control: a mirror device is refused outright, a view-only machine device
//! may read, and only a machine Control device may pause. The lane key travels in the body
//! because the relay's path segments do not admit every key a person might name.

use super::*;

use crate::hub_runtime::LanePauseError;

/// Capability a machine Control device sees while this daemon has the hub open.
pub(crate) const SUPERVISOR_CAPABILITY: &str = "supervisorV1";

/// The wire code an ended lane (closed, failed or verified done) is refused with.
pub(crate) const SUPERVISOR_INVALID_STATE_CODE: &str = "INVALID_STATE";

/// The wire code a daemon without an open hub answers with, as the desktop's own request does.
pub(crate) const SUPERVISOR_UNAVAILABLE_CODE: &str = "SUPERVISOR_UNAVAILABLE";

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
    };
    machine_error_with_details(status, code, &message, serde_json::Map::new())
}


#[cfg(test)]
#[path = "supervisor_api_tests.rs"]
mod tests;
