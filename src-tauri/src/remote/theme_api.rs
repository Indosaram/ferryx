use std::sync::Arc;

use axum::{
    extract::{Path as AxumPath, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};

use crate::remote::state::RemoteGatewayState;
use crate::theme::{
    default_theme_settings_path, default_themes_dir, is_valid_theme_id, read_published_themes_in,
    read_theme_background_bytes_in,
};

pub(super) async fn list_themes(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
) -> Response {
    let id = uuid::Uuid::new_v4().to_string();
    let _admission = match super::workspace_api::admit(state, headers, false, &id).await {
        Ok(admission) => admission,
        Err(refusal) => return refusal.into(),
    };
    let Ok(mut snapshot) = crate::ipc::run_blocking(move || {
        Ok(read_published_themes_in(
            &default_themes_dir(),
            &default_theme_settings_path(),
        ))
    })
    .await
    else {
        return super::session_api::failure("MACHINE_SERVICE_UNAVAILABLE", &id);
    };
    snapshot.themes_dir.clear();
    (
        StatusCode::OK,
        [(header::CACHE_CONTROL, "no-store")],
        Json(snapshot),
    )
        .into_response()
}

pub(super) async fn get_theme_background(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    AxumPath(theme_id): AxumPath<String>,
) -> Response {
    let id = uuid::Uuid::new_v4().to_string();
    let _admission = match super::workspace_api::admit(state, headers, false, &id).await {
        Ok(admission) => admission,
        Err(refusal) => return refusal.into(),
    };
    if !is_valid_theme_id(&theme_id) {
        return (StatusCode::BAD_REQUEST, "Invalid theme id").into_response();
    }
    let result = crate::ipc::run_blocking(move || {
        Ok(read_theme_background_bytes_in(&default_themes_dir(), &theme_id))
    })
    .await;
    match result {
        Ok(Ok((bytes, mime, _revision))) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, mime),
                (header::CACHE_CONTROL, "no-store"),
            ],
            bytes,
        )
            .into_response(),
        Ok(Err(_)) => (StatusCode::NOT_FOUND, "Theme background not found").into_response(),
        Err(_) => super::session_api::failure("MACHINE_SERVICE_UNAVAILABLE", &id),
    }
}
