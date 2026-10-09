//! Explicit start is the only remote operation allowed to create a managed child.
use super::{managed_chat_api::{authorize_chat_target, register_supervisor_provider, scoped_error, scoped_success, MANAGED_PROVIDERS}, state::RemoteGatewayState};
use crate::{ferryx_scope::chat::Supervisor, scoped_contracts::{CanonicalProvider, ScopeErrorCode, TargetRef}};
use axum::{extract::State, http::{HeaderMap, StatusCode}, response::Response, Json};
use serde::Deserialize;
use serde_json::json;
use std::sync::{Arc, LazyLock};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManagedChatStartRequest {
    pub request_id: String,
    pub target: TargetRef,
    pub provider: CanonicalProvider,
}

// Serializes admission only, never a turn or callback. No request retries/spawn replay.
static START_GATE: LazyLock<tokio::sync::Mutex<()>> = LazyLock::new(|| tokio::sync::Mutex::new(()));

pub async fn managed_chat_start(State(state): State<Arc<RemoteGatewayState>>, headers: HeaderMap, Json(req): Json<ManagedChatStartRequest>) -> Response {
    if let Err(response) = authorize_chat_target(&state, &headers, &req.target, &req.request_id).await { return response; }
    let _gate = START_GATE.lock().await;
    if req.provider != CanonicalProvider::Codex {
        return scoped_error(StatusCode::UNPROCESSABLE_ENTITY, ScopeErrorCode::Unsupported, "Only managed Codex is supported", &req.request_id);
    }
    if MANAGED_PROVIDERS.lock().contains_key(&req.target.backend_session_id) {
        return scoped_error(StatusCode::CONFLICT, ScopeErrorCode::RequestConflict, "Stop the existing managed provider before starting another", &req.request_id);
    }
    let identity = match super::server::load_gateway_identity(state.clone()).await {
        Ok(identity) => identity,
        Err(_) => return scoped_error(StatusCode::SERVICE_UNAVAILABLE, ScopeErrorCode::Unsupported, "Machine identity unavailable", &req.request_id),
    };
    if identity.machine_id != req.target.host_id {
        return scoped_error(StatusCode::CONFLICT, ScopeErrorCode::TargetExpired, "Target is not this host", &req.request_id);
    }
    let Some(services) = &state.machine_services else {
        return scoped_error(StatusCode::UNPROCESSABLE_ENTITY, ScopeErrorCode::Unsupported, "Managed launch requires the owning daemon", &req.request_id);
    };
    // A managed child is independent of the terminal, not a conversion of its PTY.
    let Some(pty) = state.terminal_service.get_session(&req.target.backend_session_id) else {
        return scoped_error(StatusCode::UNPROCESSABLE_ENTITY, ScopeErrorCode::Unsupported, "Start on the session-owning host; SSH and legacy proxy targets are unsupported", &req.request_id);
    };
    let Some(cwd) = pty.worktree_path() else {
        return scoped_error(StatusCode::UNPROCESSABLE_ENTITY, ScopeErrorCode::Unsupported, "Session has no trusted worktree directory", &req.request_id);
    };
    let mut command = tokio::process::Command::new("codex");
    command.args(["app-server", "--listen", "stdio://"]).current_dir(&cwd);
    #[cfg(test)]
    if let Some(fixture) = state.managed_chat_command.lock().take() { command = fixture; }
    let claims = Arc::new(crate::daemon::managed_chat::ManagedClaims(services.sessions.clone()));
    let supervisor = match Supervisor::spawn(command, req.target.clone(), claims, None).await {
        Ok(supervisor) => Arc::new(supervisor),
        Err(error) => return scoped_error(StatusCode::UNPROCESSABLE_ENTITY, ScopeErrorCode::Unsupported, format!("Managed Codex unavailable: {error:?}"), &req.request_id),
    };
    let thread = supervisor.request("thread/start", json!({"cwd":cwd,"approvalPolicy":"untrusted"})).await;
    let thread_id = match thread.as_ref().ok().and_then(|value| value["thread"]["id"].as_str()) {
        Some(id) if !id.is_empty() => id.to_string(),
        _ => {
            if let Err(error) = supervisor.stop().await { tracing::warn!(?error, "Failed start cleanup"); }
            return scoped_error(StatusCode::UNPROCESSABLE_ENTITY, ScopeErrorCode::Unsupported, "Provider did not create a managed thread", &req.request_id);
        }
    };
    register_supervisor_provider(req.target.clone(), supervisor.clone(), Some(thread_id.clone()), Some(state.clone()));
    // Publish before the final check so a concurrent close cannot miss registration.
    if !matches!(pty.state(), crate::terminal::PtySessionState::Running | crate::terminal::PtySessionState::Starting) {
        super::managed_chat_api::unregister_managed_provider(&req.target.backend_session_id);
        if let Err(error) = supervisor.stop().await { tracing::warn!(?error, "Expired start cleanup"); }
        return scoped_error(StatusCode::CONFLICT, ScopeErrorCode::TargetExpired, "Session exited during managed launch", &req.request_id);
    }
    scoped_success(json!({"target":req.target,"provider":"codex","threadId":thread_id}), &req.request_id)
}

#[cfg(test)]
#[path = "managed_chat_lifecycle_tests.rs"]
mod tests;
