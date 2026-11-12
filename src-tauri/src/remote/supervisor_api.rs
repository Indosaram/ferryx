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

/// Capability of a holder that applies a standby's pane observations (`POST supervisor/observe`).
pub(crate) const SUPERVISOR_OBSERVE_CAPABILITY: &str = "supervisorObserveV1";

/// `POST /api/v1/supervisor/observe`: a standby's observation of its own panes (keystrokes,
/// relayed input, worker report), applied here as if local with `host_id` the paired host key
/// this holder binds the sender machine's panes under.
/// A machine Control device: a same-account machine peer regardless of the agent-control
/// switch, any other device only when the policy admits it. The sender is the machine the
/// account vouched for (else the device's installation id), never a body field; a view-only or
/// mirror device is refused (403) before the body is read.
pub(crate) async fn http_observe(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    body: Result<Json<serde_json::Value>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let device = match agent_control_service::http_machine_peer(&state, &headers, true) {
        Ok(device) => device,
        Err(response) => return *response,
    };
    let Ok(Json(observation)) = body else {
        return reference_chat_error(ScopeErrorCode::InvalidRequest, "the body must be one observation");
    };
    let hosts = crate::supervisor_move::paired_host_views().await;
    let sender = device.account_machine_id.as_deref().or(device.installation_id.as_deref());
    match crate::hub_runtime::apply_observation(sender, &hosts, observation).await {
        Ok(value) => no_store(value),
        Err(failure) => service_refusal(&failure),
    }
}

/// `POST /api/v1/supervisor/pause` body. With `laneKey` it pauses or resumes that one lane (the
/// original contract). Without it the owner pauses or resumes the whole supervisor (`{paused}`,
/// the frozen panel contract): the hub-wide flag recovery and answers read before they act.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct PauseBody {
    #[serde(default)]
    lane_key: Option<String>,
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

/// `GET /api/v1/supervisor/panel`: the whole panel in one snapshot. Owner only: it carries
/// prompt bodies, so only a machine Control device agent control admits may read it.
pub(crate) async fn http_panel(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = agent_control_service::http_device(&state, &headers, true) {
        return response;
    }
    no_store(serde_json::json!(crate::hub_runtime::supervisor_panel(&state).await))
}

/// `GET /api/v1/supervisor/prs`: the pull-request lane, ordered by who acts next. Owner only,
/// like the panel. Before the hub's first refresh (or without a hub generation in this daemon)
/// it is an empty lane with `outage: null` and `refreshedAtMs: 0`; a signed-out or missing `gh`
/// is an empty lane naming the outage, never an error status.
pub(crate) async fn http_prs(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = agent_control_service::http_device(&state, &headers, true) {
        return response;
    }
    let lane = crate::hub_runtime::supervisor_prs().unwrap_or(ferryx_hub::supervisor::pr_lane::PrLane {
        prs: Vec::new(),
        outage: None,
        refreshed_at_ms: 0,
    });
    no_store(serde_json::json!(lane))
}

/// `GET /api/v1/fleet/sessions`: `{sessions}` in the fleet shape. Owner only. This gateway has
/// no path to the hub's paired-host fleet, so the list is this computer's sessions mapped the
/// same way the hub maps each machine.
pub(crate) async fn http_fleet_sessions(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = agent_control_service::http_device(&state, &headers, true) {
        return response;
    }
    no_store(serde_json::json!({ "sessions": crate::hub_runtime::panel_autonomy::fleet_sessions(&state).await }))
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
            "the pause body must be {paused} or {laneKey, paused}",
        );
    };
    let by = format!("remote:{}", device.id);
    let Some(lane_key) = body.lane_key else {
        return match crate::hub_runtime::set_supervisor_paused(&state, body.paused, &by).await {
            Ok(changed) => no_store(serde_json::json!({ "paused": body.paused, "changed": changed })),
            Err(error) => control_refusal(&error),
        };
    };
    match crate::hub_runtime::set_lane_paused(&state, &lane_key, body.paused, &by).await {
        Ok(changed) => (
            StatusCode::OK,
            [(header::CACHE_CONTROL, "no-store")],
            Json(serde_json::json!({
                "laneKey": lane_key,
                "paused": body.paused,
                "changed": changed,
            })),
        )
            .into_response(),
        Err(error) => pause_refusal(&error),
    }
}

/// The machine error envelope one direct-control refusal answers with.
fn control_refusal(error: &crate::hub_runtime::SupervisorControlError) -> Response {
    use crate::hub_runtime::SupervisorControlError as Refused;
    use ferryx_hub::supervisor::undo::UndoError;
    let status = match error {
        Refused::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        Refused::NoOwnerHome => StatusCode::BAD_REQUEST,
        Refused::Storage(_) => StatusCode::INTERNAL_SERVER_ERROR,
        Refused::Undo(UndoError::NotSupported) => StatusCode::UNPROCESSABLE_ENTITY,
        Refused::Undo(UndoError::Nothing(_)) => StatusCode::NOT_FOUND,
        Refused::Undo(UndoError::Stale(_)) => StatusCode::CONFLICT,
        Refused::Undo(UndoError::Storage(_)) => StatusCode::INTERNAL_SERVER_ERROR,
        Refused::Undo(UndoError::Failed(_)) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    machine_error_with_details(status, error.code(), &error.to_string(), serde_json::Map::new())
}

/// `POST /api/v1/supervisor/actions/{id}/undo`: the panel's Undo on one action-log line. Only a
/// machine Control device the agent-control policy admits may undo; no body.
pub(crate) async fn http_action_undo(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let device = match agent_control_service::http_device(&state, &headers, true) {
        Ok(device) => device,
        Err(response) => return response,
    };
    let Ok(id) = id.parse::<i64>() else {
        return reference_chat_error(ScopeErrorCode::InvalidRequest, "the action id must be a number");
    };
    tracing::info!(device = %device.id, id, "supervisor action undo requested");
    match crate::hub_runtime::undo_supervisor_action(&state, id).await {
        Ok(()) => no_store(serde_json::json!({ "undone": id })),
        Err(error) => control_refusal(&error),
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

/// The machine error envelope a setup refusal answers with. A standby names the holder in
/// `details` as `{holderMachineId, holderLabel}`.
fn setup_refusal(error: &crate::hub_runtime::SetupError) -> Response {
    if let crate::hub_runtime::SetupError::Standby(failure) = error {
        return service_refusal(failure);
    }
    let code = error.code();
    let status = match code {
        SUPERVISOR_UNAVAILABLE_CODE => StatusCode::SERVICE_UNAVAILABLE,
        // Retryable: Slack did not name the owner's team yet; nothing was provisioned.
        crate::hub_runtime::SLACK_TEAM_UNRESOLVED => StatusCode::SERVICE_UNAVAILABLE,
        SUPERVISOR_INVALID_STATE_CODE => StatusCode::CONFLICT,
        other => StatusCode::from_u16(ferryx_hub::supervisor::request::status_for_code(other))
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
    };
    machine_error_with_details(status, code, &error.to_string(), serde_json::Map::new())
}

/// `GET /api/v1/supervisor/setup`: the shared SetupStatus. Any non-mirror device may read it.
pub(crate) async fn http_setup_status(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = agent_control_service::http_device(&state, &headers, false) {
        return response;
    }
    match crate::hub_runtime::supervisor_setup_status(&state).await {
        Ok(value) => no_store(value),
        Err(error) => setup_refusal(&error),
    }
}

/// `POST /api/v1/supervisor/setup` `{importMemory, projects: [{name, path}]}`: runs automatic
/// setup. Only a machine Control device the agent-control policy admits may run it; the daemon
/// is never restarted, so live PTY sessions stay up.
pub(crate) async fn http_setup_run(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    body: Result<Json<crate::hub_runtime::SetupRunRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    if let Err(response) = agent_control_service::http_device(&state, &headers, true) {
        return response;
    }
    let Ok(Json(request)) = body else {
        return reference_chat_error(
            ScopeErrorCode::InvalidRequest,
            "the setup body must be {importMemory, projects: [{name, path}]}",
        );
    };
    match crate::hub_runtime::supervisor_setup_run(&state, request).await {
        Ok(value) => no_store(value),
        Err(error) => setup_refusal(&error),
    }
}

/// `POST /api/v1/supervisor/adopt-all`: adopts every adoptable legacy lane as the owner.
/// Machine Control devices only.
pub(crate) async fn http_adopt_all(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
) -> Response {
    let device = match agent_control_service::http_device(&state, &headers, true) {
        Ok(device) => device,
        Err(response) => return response,
    };
    let caller = crate::hub_runtime::SupervisorCaller::Remote { device_id: device.id.clone() };
    match crate::hub_runtime::supervisor_adopt_all(&state, caller).await {
        Ok(value) => no_store(value),
        Err(error) => setup_refusal(&error),
    }
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
/// (403) and only machine Control devices the agent-control policy admits may send it; a
/// standby of the same account forwarding its owner's request is admitted regardless of the
/// switch. The device never names the actor, the daemon's owner identity is used.
pub(crate) async fn http_request(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    body: Result<Json<serde_json::Value>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let device = match agent_control_service::http_machine_peer(&state, &headers, true) {
        Ok(device) => device,
        Err(response) => return *response,
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

/// `POST /api/v1/supervisor/converse {text, mentions, requestId}`: one owner message answered
/// by the supervisor on the `app` channel; both rows land in the app transcript. Only a machine
/// Control device the agent-control policy admits may send it; any other device is 403.
pub(crate) async fn http_converse(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    body: Result<
        Json<ferryx_hub::supervisor::transcript::ConverseInput>,
        axum::extract::rejection::JsonRejection,
    >,
) -> Response {
    let device = match agent_control_service::http_device(&state, &headers, true) {
        Ok(device) => device,
        Err(response) => return response,
    };
    let Ok(Json(input)) = body else {
        return reference_chat_error(
            ScopeErrorCode::InvalidRequest,
            "the body must be {text, mentions, requestId}",
        );
    };
    let caller = crate::hub_runtime::SupervisorCaller::Remote {
        device_id: device.id.clone(),
    };
    match crate::hub_runtime::supervisor_converse(&state, caller, input).await {
        Ok(answer) => no_store(answer),
        Err(failure) => service_refusal(&failure),
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TranscriptQuery {
    #[serde(default)]
    cursor: Option<i64>,
}

/// `GET /api/v1/supervisor/transcript?cursor=`: one page of the app transcript, oldest first.
pub(crate) async fn http_transcript(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    query: Result<
        axum::extract::Query<TranscriptQuery>,
        axum::extract::rejection::QueryRejection,
    >,
) -> Response {
    if let Err(response) = agent_control_service::http_device(&state, &headers, false) {
        return response;
    }
    let Ok(axum::extract::Query(query)) = query else {
        return reference_chat_error(ScopeErrorCode::InvalidRequest, "the query must be [cursor]");
    };
    match crate::hub_runtime::supervisor_transcript(&state, query.cursor.unwrap_or(0)).await {
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

/// The machine error envelope a move refusal answers with (`SUPERVISOR_STANDBY` details
/// `{holderMachineId, holderLabel}`, `SUPERVISOR_FORCE_REQUIRED` details
/// `{holderLabel, checkpointAt}`).
fn move_refusal(error: crate::supervisor_checkpoint::MoveError) -> Response {
    let status = StatusCode::from_u16(error.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    machine_error_with_details(status, &error.code, &error.message, *error.details)
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct MoveBody {
    #[serde(default)]
    force: bool,
    /// The owner's explicit consent to archive and replace a local supervisor with another
    /// identity (`--replace-local`); without it that move is refused.
    #[serde(default)]
    replace_local: bool,
}

/// `POST /api/v1/supervisor/move` on the target machine: `{force, replaceLocal?}` ->
/// `{moved}`. Only a machine Control device the agent-control policy admits may move the
/// supervisor.
pub(crate) async fn http_move(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    body: Result<Json<MoveBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let device = match agent_control_service::http_device(&state, &headers, true) {
        Ok(device) => device,
        Err(response) => return response,
    };
    let Ok(Json(body)) = body else {
        return reference_chat_error(ScopeErrorCode::InvalidRequest, "the move body must be {force, replaceLocal?}");
    };
    let by = format!("remote:{}", device.id);
    match crate::supervisor_move::move_here_installed(&state, body.force, body.replace_local, &by).await {
        Ok(moved) => no_store(serde_json::json!({ "moved": moved })),
        Err(error) => move_refusal(error),
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct AdoptArchivedBody {
    /// The archived goals to adopt; missing or empty adopts every adoptable goal (`--all`).
    #[serde(default)]
    goal_ids: Option<Vec<String>>,
}

/// `POST /api/v1/supervisor/adopt-archived` on the machine whose archived supervisor owns the
/// goals: `ferryx hub adopt-archived` for a phone (and for a Windows standby, whose CLI may not
/// mutate the supervisor over the daemon socket). Each listed goal is adopted with one
/// [`crate::hub_runtime::adopt_archived_in_process`] call, submitting it to the holder in
/// process (never over the daemon socket); no ids adopts every adoptable goal. Answers 200
/// `{adopted: [AdoptReport...]}` as the CLI prints it, a goal whose adoption failed as a whole
/// reported as `{goalId, adopted: false, error}`. When every entry failed with an error, the
/// first error is the typed answer instead, so a refused send is never a success. A machine
/// Control device the agent-control policy admits only, as for a move; not a machine-peer
/// route.
pub(crate) async fn http_adopt_archived(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    body: Result<Json<AdoptArchivedBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let device = match agent_control_service::http_device(&state, &headers, true) {
        Ok(device) => device,
        Err(response) => return response,
    };
    let Ok(Json(body)) = body else {
        return reference_chat_error(ScopeErrorCode::InvalidRequest, "the body must be {goalIds?}");
    };
    let selections: Vec<Option<String>> = match body.goal_ids.filter(|ids| !ids.is_empty()) {
        Some(ids) => ids.into_iter().map(Some).collect(),
        None => vec![None],
    };
    let mut adopted = Vec::new();
    for selection in selections {
        match crate::hub_runtime::adopt_archived_in_process(&state, &device.id, selection.clone()).await {
            Ok(reports) => {
                adopted.extend(reports.into_iter().map(|report| serde_json::to_value(report).unwrap_or_default()));
            }
            Err(failure) => adopted.push(serde_json::json!({
                "goalId": selection,
                "adopted": false,
                "error": failure.to_json(),
            })),
        }
    }
    match all_failed(&adopted) {
        Some(failure) => service_refusal(&failure),
        None => no_store(serde_json::json!({ "adopted": adopted })),
    }
}

/// The first entry's refusal when every adopt entry carries an error (and there is at least
/// one), read back from its `RequestFailure::to_json` shape; `None` otherwise.
fn all_failed(adopted: &[serde_json::Value]) -> Option<ferryx_hub::supervisor::request::RequestFailure> {
    use ferryx_hub::supervisor::request::RequestFailure;
    let errors: Vec<&serde_json::Value> = adopted
        .iter()
        .filter_map(|entry| entry.get("error").filter(|error| !error.is_null()))
        .collect();
    if errors.is_empty() || errors.len() != adopted.len() {
        return None;
    }
    let error = errors[0];
    let text = |key: &str| error.get(key).and_then(serde_json::Value::as_str).map(str::to_owned);
    let code = text("code").unwrap_or_else(|| "INTERNAL".to_owned());
    let mut failure = RequestFailure::new(&code, text("message").unwrap_or_default());
    if let Some(status) = error.get("status").and_then(serde_json::Value::as_u64).and_then(|s| u16::try_from(s).ok()) {
        failure.status = status;
    }
    if error.get("holderMachineId").is_some() || error.get("holderLabel").is_some() {
        failure.holder = Some((text("holderMachineId"), text("holderLabel")));
    }
    Some(failure)
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct ReleaseToBody {
    target_machine_id: String,
}

/// `POST /api/v1/supervisor/release-to` on the holder, reached by the move target over the
/// paired encrypted channel: stops the hub, publishes the final checkpoint and releases the
/// lease reserved for `targetMachineId`. Answers `{checkpointId}`. A same-account machine peer
/// is admitted regardless of the agent-control switch.
pub(crate) async fn http_release_to(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    body: Result<Json<ReleaseToBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    if let Err(response) = agent_control_service::http_machine_peer(&state, &headers, true) {
        return *response;
    }
    let Ok(Json(body)) = body else {
        return reference_chat_error(ScopeErrorCode::InvalidRequest, "the body must be {targetMachineId}");
    };
    match crate::hub_runtime::release_supervisor_to(&body.target_machine_id).await {
        Ok(checkpoint_id) => no_store(serde_json::json!({ "checkpointId": checkpoint_id })),
        Err(error) => move_refusal(error),
    }
}

/// `GET /api/v1/supervisor/checkpoint`: this machine's newest checkpoint archive, its id in
/// [`crate::supervisor_checkpoint::CHECKPOINT_ID_HEADER`]. The archive carries secrets, so
/// a Control agent-control device grant is required (403 otherwise, view-only included): a
/// same-account machine peer regardless of the switch, any other device only when the policy
/// admits it. 404 when none was published.
pub(crate) async fn http_checkpoint(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = agent_control_service::http_machine_peer(&state, &headers, true) {
        return *response;
    }
    let Some(dir) = crate::supervisor_checkpoint::checkpoint_dir() else {
        return machine_error_with_details(StatusCode::NOT_FOUND, "NOT_FOUND", "no supervisor checkpoint", serde_json::Map::new());
    };
    let store = crate::supervisor_checkpoint::CheckpointStore::new(dir);
    let read = tokio::task::spawn_blocking(move || {
        let newest = store.newest()?;
        let bytes = std::fs::read(store.path_of(&newest.id)).ok()?;
        Some((newest.id, bytes))
    })
    .await
    .ok()
    .flatten();
    match read {
        Some((id, bytes)) => (
            StatusCode::OK,
            [
                (header::CACHE_CONTROL, "no-store".to_owned()),
                (header::CONTENT_TYPE, "application/zstd".to_owned()),
                (header::HeaderName::from_static(crate::supervisor_checkpoint::CHECKPOINT_ID_HEADER), id),
            ],
            bytes,
        )
            .into_response(),
        None => machine_error_with_details(StatusCode::NOT_FOUND, "NOT_FOUND", "no supervisor checkpoint", serde_json::Map::new()),
    }
}
