use base64::Engine as _;
use serde::Serialize;

use crate::ipc::error::{IpcError, IpcErrorCode};
use crate::ipc::run_blocking;
use crate::theme::{
    clear_environment_theme_in, default_theme_settings_path, default_themes_dir,
    read_published_themes_in, read_theme_background_bytes_in, set_environment_theme_in,
    EnvironmentThemesSnapshot,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeBackgroundData {
    pub data_url: String,
    pub mime_type: String,
    pub revision: String,
}

#[tauri::command]
pub async fn cmd_theme_list_environment() -> Result<EnvironmentThemesSnapshot, IpcError> {
    run_blocking(move || {
        Ok(read_published_themes_in(
            &default_themes_dir(),
            &default_theme_settings_path(),
        ))
    })
    .await
}

#[tauri::command]
pub async fn cmd_theme_set_environment(
    target: Option<String>,
    appearance: Option<String>,
) -> Result<EnvironmentThemesSnapshot, IpcError> {
    run_blocking(move || {
        set_environment_theme_in(
            &default_themes_dir(),
            &default_theme_settings_path(),
            target.as_deref(),
            appearance.as_deref(),
        )
        .map_err(|msg| IpcError::new(IpcErrorCode::InvalidRequest, msg))
    })
    .await
}

#[tauri::command]
pub async fn cmd_theme_clear_environment() -> Result<EnvironmentThemesSnapshot, IpcError> {
    run_blocking(move || {
        clear_environment_theme_in(&default_themes_dir(), &default_theme_settings_path())
            .map_err(IpcError::internal)
    })
    .await
}

#[tauri::command]
pub async fn cmd_theme_read_background(theme_id: String) -> Result<ThemeBackgroundData, IpcError> {
    run_blocking(move || {
        let (bytes, mime, revision) =
            read_theme_background_bytes_in(&default_themes_dir(), &theme_id)
                .map_err(|msg| IpcError::new(IpcErrorCode::IoError, msg))?;
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        Ok(ThemeBackgroundData {
            data_url: format!("data:{mime};base64,{encoded}"),
            mime_type: mime.to_string(),
            revision,
        })
    })
    .await
}
