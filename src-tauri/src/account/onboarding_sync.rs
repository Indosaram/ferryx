//! Monotonic, account-scoped wizard progress. Machine steps never leave the device.
use std::sync::Arc;
use std::time::Duration;

use axum::{body::Bytes, extract::State, http::{HeaderMap, StatusCode}, Json};

use super::service::{authenticate_user, parse_json, run_account_blocking, AccountState, ApiError};
use super::store::{now_secs, OnboardingRecord};

pub async fn get_progress(
    State(state): State<Arc<AccountState>>,
    headers: HeaderMap,
) -> Result<Json<OnboardingRecord>, ApiError> {
    run_account_blocking(move || {
        let user = authenticate_user(&state, &headers)?;
        let store = state.load().map_err(ApiError::internal)?;
        Ok(Json(store.onboarding.get(&user.user_id).cloned().unwrap_or_default()))
    }).await
}

pub async fn put_progress(
    State(state): State<Arc<AccountState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<OnboardingRecord>, ApiError> {
    // Authenticate even malformed requests, before parsing or consuming the per-user budget.
    let user = {
        let state = Arc::clone(&state);
        let headers = headers.clone();
        run_account_blocking(move || authenticate_user(&state, &headers)).await?
    };
    if !state.allow_onboarding_rate(
        &format!("progress:{}", user.user_id), 60, Duration::from_secs(60),
    ) {
        return Err(ApiError::new(StatusCode::TOO_MANY_REQUESTS, "RATE_LIMITED", "too many onboarding updates"));
    }
    let mut request: OnboardingRecord = parse_json(body).await?;
    for step in &request.completed_steps {
        if matches!(step.as_str(), "enroll" | "cli" | "skills" | "project" | "agents" | "import") {
            return Err(ApiError::new(StatusCode::BAD_REQUEST, "MACHINE_STEP_NOT_SYNCABLE", "machine steps cannot be synced"));
        }
        if !matches!(step.as_str(), "account" | "intro" | "features" | "supervisor" | "phone" | "try") {
            return Err(ApiError::new(StatusCode::BAD_REQUEST, "UNKNOWN_ONBOARDING_STEP", "unknown onboarding step"));
        }
    }
    // Seconds since Unix epoch; reject neither offline timestamps nor old clients that omit it,
    // but do not let a faulty clock pin ordering in the future.
    let now = now_secs();
    request.updated_at = if request.updated_at == 0 { now } else { request.updated_at.min(now) };
    run_account_blocking(move || {
        // Recheck the session under the write transaction (logout/deletion can race the parse).
        let token = super::service::bearer(&headers).ok_or_else(||
            ApiError::new(StatusCode::UNAUTHORIZED, "UNAUTHORIZED", "missing bearer token"))?;
        state.mutate(|store| {
            let user = store.user_for_session(&token, now_secs()).cloned().ok_or_else(||
                ApiError::new(StatusCode::UNAUTHORIZED, "UNAUTHORIZED", "unknown or expired session"))?;
            let merged = store.onboarding.get(&user.user_id).cloned().unwrap_or_default().merge(&request);
            store.onboarding.insert(user.user_id, merged.clone());
            Ok(Json(merged))
        })
    }).await
}
