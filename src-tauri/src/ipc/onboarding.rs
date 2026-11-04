//! File bootstrap followed by capability-gated, one-time daemon migration.
use super::IpcError;
use crate::daemon::{client::DaemonClient, onboarding::{self, State}, protocol::{DaemonRequest, DaemonResponse}};

async fn bootstrap(cache: Option<State>) -> Result<State, IpcError> {
    super::run_blocking(move || {
        let path = onboarding::path(false).map_err(IpcError::internal)?;
        if let Some(state) = onboarding::read(&path).map_err(IpcError::internal)? { return Ok(state); }
        let state = cache.unwrap_or_default();
        onboarding::write(&path, &state).map_err(IpcError::internal)?;
        Ok(state)
    }).await
}

async fn daemon() -> Result<Option<DaemonClient>, IpcError> {
    if onboarding::qa_no_daemon() { return Ok(None); }
    let client = DaemonClient::existing_only(std::time::Duration::ZERO);
    match client.send_request(DaemonRequest::GetCapabilities).await {
        Ok(DaemonResponse::CapabilitiesOk { capabilities }) =>
            Ok(capabilities.iter().any(|cap| cap == onboarding::CAPABILITY).then_some(client)),
        Ok(DaemonResponse::Error { message, .. }) => Err(IpcError::internal(message)),
        Ok(_) => Err(IpcError::internal("Unexpected onboarding capabilities response")),
        Err(error) => {
            tracing::debug!(%error, "Onboarding remains in its bootstrap file; daemon unavailable");
            Ok(None)
        }
    }
}

fn state_from(reply: DaemonResponse) -> Result<Option<State>, IpcError> {
    match reply {
        DaemonResponse::OnboardingOk { state } => Ok(state),
        DaemonResponse::Error { message, .. } => Err(IpcError::internal(message)),
        _ => Err(IpcError::internal("Unexpected onboarding response")),
    }
}

#[tauri::command]
pub async fn cmd_onboarding_get(cache: Option<State>) -> Result<State, IpcError> {
    let local = bootstrap(cache).await?;
    let Some(client) = daemon().await? else { return Ok(local); };
    if let Some(state) = state_from(client.send_request(DaemonRequest::OnboardingGet).await?)? {
        return Ok(state);
    }
    state_from(client.send_request(DaemonRequest::OnboardingSet { state: local, migrate: true }).await?)?
        .ok_or_else(|| IpcError::internal("Daemon did not migrate onboarding"))
}

#[tauri::command]
pub async fn cmd_onboarding_set(state: State) -> Result<State, IpcError> {
    state.validate().map_err(IpcError::internal)?;
    if let Some(client) = daemon().await? {
        return state_from(client.send_request(DaemonRequest::OnboardingSet { state, migrate: false }).await?)?
            .ok_or_else(|| IpcError::internal("Daemon did not save onboarding"));
    }
    super::run_blocking(move || {
        onboarding::write(&onboarding::path(false).map_err(IpcError::internal)?, &state)
            .map_err(IpcError::internal)?;
        Ok(state)
    }).await
}
