//! Machine HTTP adapters. PTYs are created only by DaemonSessionService.
use super::{machine_protocol::*, state::RemoteGatewayState, workspace_api};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use sha2::{Digest, Sha256};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Instant,
};

pub(super) fn failure(code: &str, request: &str) -> Response {
    let status = match code {
        "UNAUTHORIZED" => 401,
        "MACHINE_ACCESS_REQUIRED" => 403,
        "SESSION_NOT_FOUND" | "PROJECT_NOT_FOUND" | "WORKTREE_NOT_FOUND" => 404,
        "REQUEST_CONFLICT"
        | "OPERATION_OUTCOME_UNKNOWN"
        | "OPERATION_RESULT_EXPIRED"
        | "STALE_EPOCH"
        | "SESSION_EXPIRED"
        | "SESSION_OWNERSHIP_CHANGED"
        | "CONTROL_CONFLICT"
        | "PARENT_SESSION_MISMATCH"
        | "AGENT_SESSION_CONFLICT" => 409,
        "AGENT_RESUME_UNSUPPORTED" | "MACHINE_OWNER_UNSUPPORTED" => 422,
        "CAPACITY_EXCEEDED" => 429,
        "MACHINE_SERVICE_UNAVAILABLE" | "HOST_UNAVAILABLE" => 503,
        "TIMEOUT" => 504,
        "PAYLOAD_TOO_LARGE" => 413,
        _ => 400,
    };
    json(
        status,
        ErrorEnvelope {
            error: MachineError {
                code: code.into(),
                message: code.into(),
                retryable: matches!(status, 429 | 503 | 504),
                request_id: request.into(),
                details: Default::default(),
            },
        },
    )
}
fn json(status: u16, value: impl serde::Serialize) -> Response {
    (
        StatusCode::from_u16(status).expect("status"),
        [("cache-control", "no-store")],
        Json(value),
    )
        .into_response()
}
fn replay(record: super::machine_operation_journal::Record) -> Response {
    match record.operation {
        Operation::Completed {
            outcome: OperationOutcome::Session { session },
            ..
        } => json(record.status, session),
        Operation::Completed {
            outcome: OperationOutcome::NoContent,
            ..
        } => (StatusCode::NO_CONTENT, [("cache-control", "no-store")]).into_response(),
        Operation::Completed {
            outcome: OperationOutcome::Error { error },
            ..
        } => json(record.status, ErrorEnvelope { error }),
        Operation::OutcomeUnknown { request_id } => {
            failure("OPERATION_OUTCOME_UNKNOWN", &request_id)
        }
        Operation::ResultExpired { request_id } => failure("OPERATION_RESULT_EXPIRED", &request_id),
        operation => json(record.status, operation),
    }
}
async fn reconcile(
    workspaces: Arc<crate::daemon::workspace_service::DaemonWorkspaceService>,
    device: String,
    request: String,
) -> Result<Option<super::machine_operation_journal::Record>, String> {
    crate::ipc::run_blocking(move || Ok(workspaces.journal.reconcile(&device, &request)))
        .await
        .map_err(|_| "MACHINE_SERVICE_UNAVAILABLE".to_string())?
}
fn keys(value: &serde_json::Value, allowed: &[&str]) -> Result<(), String> {
    if value
        .as_object()
        .is_none_or(|map| map.keys().any(|key| !allowed.contains(&key.as_str())))
    {
        return Err("INVALID_REQUEST".into());
    }
    Ok(())
}
fn decode_create(body: &[u8]) -> Result<CreateSessionRequest, String> {
    let value: serde_json::Value =
        decode_json(body, MACHINE_JSON_MAX_BYTES).map_err(|_| "INVALID_REQUEST")?;
    keys(
        &value,
        &[
            "requestId",
            "workspaceId",
            "worktree",
            "cols",
            "rows",
            "inheritFromSessionId",
            "cwdRelative",
            "startup",
        ],
    )?;
    if !value["worktree"].is_null() {
        keys(&value["worktree"], &["wsId", "slug"])?;
    }
    match value["startup"]["kind"].as_str() {
        Some("shell") => keys(&value["startup"], &["kind"])?,
        Some("agentResume") => {
            keys(&value["startup"], &["kind", "agentType", "providerSession"])?;
            // A client may name an authoritative provider ID, never a transcript pathname.
            keys(&value["startup"]["providerSession"], &["key", "id"])?;
        }
        _ => return Err("INVALID_REQUEST".into()),
    }
    let request: CreateSessionRequest =
        serde_json::from_value(value).map_err(|_| "INVALID_REQUEST")?;
    if request.cols == 0
        || request.rows == 0
        || request.cols > 1000
        || request.rows > 1000
        || (request.cwd_relative.is_some() && request.inherit_from_session_id.is_some())
        || request
            .worktree
            .as_ref()
            .is_some_and(|w| w.ws_id != request.workspace_id)
    {
        return Err("INVALID_REQUEST".into());
    }
    if let Some(relative) = &request.cwd_relative {
        if relative.len() > 4096
            || relative.chars().any(char::is_control)
            || std::path::Path::new(relative).components().any(|c| {
                !matches!(
                    c,
                    std::path::Component::Normal(_) | std::path::Component::CurDir
                )
            })
        {
            return Err("INVALID_PATH".into());
        }
    }
    Ok(request)
}

pub(super) async fn create(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    request: axum::extract::Request,
) -> Result<Response, Response> {
    mutation(state, headers, request, None).await
}
pub(super) async fn close(
    State(state): State<Arc<RemoteGatewayState>>,
    path: Result<Path<String>, axum::extract::rejection::PathRejection>,
    headers: HeaderMap,
    request: axum::extract::Request,
) -> Result<Response, Response> {
    // Keep malformed path rejection behind authenticated admission too.
    mutation(state, headers, request, Some(path.map(|p| p.0))).await
}
async fn mutation(
    state: Arc<RemoteGatewayState>,
    headers: HeaderMap,
    request: axum::extract::Request,
    path: Option<Result<String, axum::extract::rejection::PathRejection>>,
) -> Result<Response, Response> {
    let fallback = uuid::Uuid::new_v4().to_string();
    let admission = workspace_api::admit(state.clone(), headers.clone(), true, &fallback).await?;
    let body = super::server::project_body(request, &admission).await?;
    let close_id = path
        .transpose()
        .map_err(|_| failure("INVALID_REQUEST", &fallback))?;
    let value: serde_json::Value =
        serde_json::from_slice(&body).map_err(|_| failure("INVALID_REQUEST", &fallback))?;
    let operation_id = value["requestId"]
        .as_str()
        .filter(|s| uuid::Uuid::parse_str(s).is_ok())
        .unwrap_or(&fallback)
        .to_string();
    let deadline = admission.deadline;
    let mut revoked = admission.revoked.clone();
    let cancel = workspace_api::CancelWork(
        Arc::new(AtomicBool::new(false)),
        Arc::new(tokio::sync::Notify::new()),
    );
    let cancelled = cancel.0.clone();
    let check: Arc<dyn Fn() -> Result<(), String> + Send + Sync> = Arc::new(move || {
        if *admission.revoked.borrow() {
            return Err("UNAUTHORIZED".into());
        }
        if cancelled.load(Ordering::Acquire) || Instant::now() >= deadline {
            return Err("TIMEOUT".into());
        }
        Ok(())
    });
    let id = operation_id.clone();
    let task = tokio::spawn(async move {
        let auth_state = state.clone();
        let auth_headers = headers.clone();
        let device = crate::ipc::run_blocking(move || {
            Ok(super::server::authenticate_machine_request(
                &auth_state,
                &auth_headers,
            ))
        })
        .await
        .map_err(|_| failure("MACHINE_SERVICE_UNAVAILABLE", &id))??;
        check().map_err(|e| failure(&e, &id))?;
        let services = state.machine_services.as_ref().expect("admitted");
        let kind = if close_id.is_some() {
            "closeSession"
        } else {
            "createSession"
        };
        let create = if close_id.is_none() {
            Some(decode_create(&body).map_err(|e| failure(&e, &id))?)
        } else {
            None
        };
        let close = if close_id.is_some() {
            keys(&value, &["requestId", "daemonEpoch"]).map_err(|e| failure(&e, &id))?;
            Some(
                serde_json::from_value::<CloseSessionRequest>(value)
                    .map_err(|_| failure("INVALID_REQUEST", &id))?,
            )
        } else {
            None
        };
        let digest = format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&(kind, &close_id, &create, &close)).expect("request")
            )
        );
        let previous = reconcile(services.workspaces.clone(), device.id.clone(), id.clone())
            .await
            .map_err(|e| failure(&e, &id))?;
        check().map_err(|e| failure(&e, &id))?;
        if let Some(record) = previous {
            if record.kind != kind || record.digest != digest {
                return Err(failure("REQUEST_CONFLICT", &id));
            }
            return Ok(replay(record));
        }
        if let Some(session_id) = &close_id {
            if let Some(peer) = services
                .sessions
                .router()
                .find_legacy_peer_for_session(session_id)
            {
                let token = super::server::extract_token(&headers)
                    .ok_or_else(|| failure("UNAUTHORIZED", &id))?;
                check().map_err(|e| failure(&e, &id))?;
                let (status, body) = peer
                    .close_machine_http(session_id, &token, &body)
                    .await
                    .map_err(|e| failure(&e, &id))?;
                check().map_err(|e| failure(&e, &id))?;
                let status =
                    StatusCode::from_u16(status).map_err(|_| failure("HOST_UNAVAILABLE", &id))?;
                return Ok((
                    status,
                    [
                        ("cache-control", "no-store"),
                        ("content-type", "application/json"),
                    ],
                    body,
                )
                    .into_response());
            }
        }
        let epoch = crate::scoped_contracts::Epoch(state.daemon_epoch.load(Ordering::Acquire));
        if let (Some(session_id), Some(close)) = (close_id, close) {
            if let Err(error) = services
                .sessions
                .close_machine(
                    &device.id,
                    &id,
                    &digest,
                    &session_id,
                    close.daemon_epoch,
                    epoch,
                    check.clone(),
                )
                .await
            {
                let workspaces = services.workspaces.clone();
                let device = device.id.clone();
                let request = id.clone();
                crate::ipc::run_blocking(move || {
                    if workspaces
                        .journal
                        .reconcile(&device, &request)
                        .map_err(crate::ipc::IpcError::internal)?
                        .is_some_and(|r| matches!(r.operation, Operation::Pending { .. }))
                    {
                        workspaces
                            .journal
                            .mark_unknown(&device, &request)
                            .map_err(crate::ipc::IpcError::internal)?;
                    }
                    Ok(())
                })
                .await
                .map_err(|_| failure("OPERATION_OUTCOME_UNKNOWN", &id))?;
                return Err(failure(&error, &id));
            }
        } else {
            let request = create.expect("create");
            let identity = super::server::load_gateway_identity(state.clone()).await?;
            let target = RemoteTerminalTarget {
                machine_id: identity.machine_id,
                daemon_epoch: epoch,
                session_id: uuid::Uuid::new_v4().to_string(),
            };
            services
                .sessions
                .spawn_machine(
                    request,
                    device.id.clone(),
                    digest,
                    target,
                    deadline,
                    check.clone(),
                )
                .await
                .map_err(|e| failure(&e, &id))?;
        }
        check().map_err(|e| failure(&e, &id))?;
        let record = reconcile(services.workspaces.clone(), device.id.clone(), id.clone())
            .await
            .map_err(|e| failure(&e, &id))?
            .ok_or_else(|| failure("OPERATION_OUTCOME_UNKNOWN", &id))?;
        #[cfg(test)]
        {
            let probe = services.workspaces.transaction_probe.read().clone();
            if let Some(probe) = probe {
                crate::ipc::run_blocking(move || {
                    probe("sessionBeforeResponse");
                    Ok(())
                })
                .await
                .map_err(|_| failure("OPERATION_OUTCOME_UNKNOWN", &id))?;
            }
        }
        check().map_err(|e| failure(&e, &id))?;
        Ok(replay(record))
    });
    tokio::select! {
        biased;
        _ = revoked.wait_for(|v| *v) => Err(failure("UNAUTHORIZED", &operation_id)),
        result = tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), task) => match result {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(failure("OPERATION_OUTCOME_UNKNOWN", &operation_id)),
            Err(_) => Err(failure("TIMEOUT", &operation_id)),
        }
    }
}

pub(super) async fn list(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    uri: axum::http::Uri,
) -> Result<Response, Response> {
    // `workspaceId` scopes the listing; an absent query lists every workspace.
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Scope {
        workspace_id: Option<String>,
    }
    let scope: Scope = axum::extract::Query::try_from_uri(&uri)
        .map(|axum::extract::Query(scope)| scope)
        .map_err(|_| failure("INVALID_REQUEST", &uuid::Uuid::new_v4().to_string()))?;
    read(state, headers, None, scope.workspace_id).await
}
pub(super) async fn detail(
    State(state): State<Arc<RemoteGatewayState>>,
    path: Result<Path<String>, axum::extract::rejection::PathRejection>,
    headers: HeaderMap,
) -> Result<Response, Response> {
    read(state, headers, Some(path.map(|p| p.0)), None).await
}
async fn read(
    state: Arc<RemoteGatewayState>,
    headers: HeaderMap,
    path: Option<Result<String, axum::extract::rejection::PathRejection>>,
    workspace: Option<String>,
) -> Result<Response, Response> {
    let id = uuid::Uuid::new_v4().to_string();
    let admission = workspace_api::admit(state.clone(), headers, false, &id).await?;
    let path = path
        .transpose()
        .map_err(|_| failure("INVALID_REQUEST", &id))?;
    let sessions = state
        .machine_services
        .as_ref()
        .expect("admitted")
        .sessions
        .clone();
    let epoch = crate::scoped_contracts::Epoch(state.daemon_epoch.load(Ordering::Acquire));
    let deadline = admission.deadline;
    let mut revoked = admission.revoked.clone();
    let cancel = workspace_api::CancelWork(
        Arc::new(AtomicBool::new(false)),
        Arc::new(tokio::sync::Notify::new()),
    );
    let cancelled = cancel.0.clone();
    let task = tokio::spawn(async move {
        let check = || {
            if *admission.revoked.borrow() {
                return Err("UNAUTHORIZED".to_string());
            }
            if cancelled.load(Ordering::Acquire) || Instant::now() >= deadline {
                return Err("TIMEOUT".to_string());
            }
            Ok(())
        };
        check()?;
        let response = match path {
            Some(path) => json(200, sessions.machine_detail_routed(&path, epoch).await?),
            None => {
                let mut rows = sessions.machine_sessions_routed(epoch).await?;
                if let Some(workspace) = &workspace {
                    rows.sessions.retain(|row| &row.workspace_id == workspace);
                }
                json(200, rows)
            }
        };
        check()?;
        Ok::<_, String>(response)
    });
    tokio::select! {
        biased;
        _ = revoked.wait_for(|v| *v) => Err(failure("UNAUTHORIZED", &id)),
        result = tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), task) => match result {
            Ok(Ok(Ok(response))) => Ok(response),
            Ok(Ok(Err(code))) => Err(failure(&code, &id)),
            Ok(Err(_)) => Err(failure("MACHINE_SERVICE_UNAVAILABLE", &id)),
            Err(_) => Err(failure("TIMEOUT", &id)),
        }
    }
}
