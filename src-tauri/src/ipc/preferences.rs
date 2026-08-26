use std::sync::Arc;

use serde::Deserialize;
use tauri::{AppHandle, Manager, Runtime, State};
use tokio::sync::oneshot;

use crate::daemon::DaemonClient;
use crate::ipc::{run_blocking, IpcError};
use crate::native_terminal::composition::LogicalBounds;
use crate::native_terminal::surface_host::{
    NativeTerminalBoundsRequest, NativeTerminalSurfaceHostState,
};
use crate::terminal::{
    reload_terminal_preferences, set_terminal_preference_overrides, TerminalPreferenceOverrides,
    TerminalPreferences,
};

/// Local terminal overrides from the settings UI, where `null` means "follow Ghostty".
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalOverridesRequest {
    #[serde(default)]
    pub font_family: Option<String>,
    #[serde(default)]
    pub font_size: Option<f32>,
    #[serde(default)]
    pub macos_option_as_alt: Option<bool>,
}

impl From<TerminalOverridesRequest> for TerminalPreferenceOverrides {
    fn from(request: TerminalOverridesRequest) -> Self {
        Self {
            font_family: request.font_family,
            font_size: request.font_size,
            macos_option_as_alt: request.macos_option_as_alt,
        }
    }
}

/// Re-renders every attached native terminal with the current font metrics and pushes the new
/// grid size to the daemon, so a font change is visible without waiting for a resize event.
async fn rerender_native_sessions<R: Runtime>(
    app: &AppHandle<R>,
    daemon_client: &DaemonClient,
    state: &NativeTerminalSurfaceHostState,
) -> Result<(), IpcError> {
    let sessions: Vec<(String, LogicalBounds)> = state
        .registered_session_ids()
        .into_iter()
        .filter_map(|session_id| {
            state
                .session_logical_bounds(&session_id)
                .map(|bounds| (session_id, bounds))
        })
        .collect();
    if sessions.is_empty() {
        return Ok(());
    }

    let window = app
        .get_webview_window("main")
        .ok_or_else(|| IpcError::internal("Main Ferryx window is unavailable"))?;
    let state_inner = state.clone();
    let surface_window = window.clone();
    let (sender, receiver) = oneshot::channel();
    window
        .run_on_main_thread(move || {
            let resized = sessions
                .into_iter()
                .filter_map(|(session_id, bounds)| {
                    let request = NativeTerminalBoundsRequest {
                        session_id: session_id.clone(),
                        bounds,
                    };
                    state_inner
                        .render(&surface_window, request)
                        .ok()
                        .map(|receipt| (session_id, receipt.cols, receipt.rows))
                })
                .collect::<Vec<_>>();
            let _ = sender.send(resized);
        })
        .map_err(|error| {
            IpcError::internal(format!(
                "Could not dispatch native terminal font re-render: {error}"
            ))
        })?;

    let resized = receiver.await.map_err(|_| {
        IpcError::internal("Main thread stopped before native terminal font re-render completed")
    })?;

    for (session_id, cols, rows) in resized {
        let _ = daemon_client.resize_terminal(&session_id, cols, rows).await;
    }
    Ok(())
}

/// Re-imports the Ghostty configuration and returns it without local overrides applied.
#[tauri::command]
pub async fn cmd_terminal_preferences<R: Runtime>(
    app: AppHandle<R>,
    daemon_client: State<'_, Arc<DaemonClient>>,
    state: State<'_, NativeTerminalSurfaceHostState>,
) -> Result<TerminalPreferences, IpcError> {
    let imported = run_blocking(|| Ok(reload_terminal_preferences().as_ref().clone())).await?;
    rerender_native_sessions(&app, daemon_client.inner(), state.inner()).await?;
    Ok(imported)
}

#[tauri::command]
pub async fn cmd_terminal_apply_overrides<R: Runtime>(
    app: AppHandle<R>,
    daemon_client: State<'_, Arc<DaemonClient>>,
    state: State<'_, NativeTerminalSurfaceHostState>,
    overrides: TerminalOverridesRequest,
) -> Result<TerminalPreferences, IpcError> {
    let effective = run_blocking(move || {
        Ok(set_terminal_preference_overrides(overrides.into())
            .as_ref()
            .clone())
    })
    .await?;
    rerender_native_sessions(&app, daemon_client.inner(), state.inner()).await?;
    Ok(effective)
}
