#[cfg(feature = "native-terminal")]
use std::sync::Arc;

use serde::Deserialize;
#[cfg(feature = "native-terminal")]
use tauri::{AppHandle, Manager, Runtime, State};
#[cfg(feature = "native-terminal")]
use tokio::sync::oneshot;

#[cfg(feature = "native-terminal")]
use crate::daemon::DaemonClient;
use crate::ipc::{run_blocking, IpcError};
#[cfg(feature = "native-terminal")]
use crate::native_terminal::composition::LogicalBounds;
#[cfg(feature = "native-terminal")]
use crate::native_terminal::surface_host::{
    NativeTerminalBoundsRequest, NativeTerminalSurfaceHostState,
};
use crate::terminal::{
    cached_terminal_preferences, reload_terminal_preferences, set_terminal_appearance,
    set_terminal_preference_overrides, TerminalAppearance, TerminalPreferenceOverrides,
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
    #[serde(default)]
    pub shell: Option<String>,
    #[serde(default)]
    pub scrollback: Option<usize>,
}

impl From<TerminalOverridesRequest> for TerminalPreferenceOverrides {
    fn from(request: TerminalOverridesRequest) -> Self {
        Self {
            font_family: request.font_family,
            font_size: request.font_size,
            macos_option_as_alt: request.macos_option_as_alt,
            shell: request.shell,
            scrollback: request.scrollback,
        }
    }
}

/// Re-renders every attached native terminal with the current font metrics and pushes the new
/// grid size to the daemon, so a font change is visible without waiting for a resize event.
#[cfg(feature = "native-terminal")]
async fn rerender_native_sessions<R: Runtime>(
    app: &AppHandle<R>,
    daemon_client: &DaemonClient,
    state: &NativeTerminalSurfaceHostState,
) -> Result<(), IpcError> {
    state.reapply_theme_to_sessions();
    state.reapply_scrollback_to_sessions();

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
        .get_window("main")
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
#[cfg(feature = "native-terminal")]
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

#[cfg(not(feature = "native-terminal"))]
#[tauri::command]
pub async fn cmd_terminal_preferences() -> Result<TerminalPreferences, IpcError> {
    run_blocking(|| Ok(reload_terminal_preferences().as_ref().clone())).await
}

#[cfg(feature = "native-terminal")]
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

#[cfg(not(feature = "native-terminal"))]
#[tauri::command]
pub async fn cmd_terminal_apply_overrides(
    overrides: TerminalOverridesRequest,
) -> Result<TerminalPreferences, IpcError> {
    run_blocking(move || {
        Ok(set_terminal_preference_overrides(overrides.into())
            .as_ref()
            .clone())
    })
    .await
}

/// Records the appearance and returns whether it changed plus the effective preferences.
///
/// Re-imports the Ghostty theme from disk on change, so it always runs on a blocking thread.
async fn push_terminal_appearance(
    appearance: TerminalAppearance,
) -> Result<(bool, TerminalPreferences), IpcError> {
    run_blocking(move || {
        let changed = set_terminal_appearance(appearance);
        Ok((changed, cached_terminal_preferences().as_ref().clone()))
    })
    .await
}

/// Repaints the macOS window backing with the current theme background.
///
/// `apply_viewport` already does this on every viewport update, but with no visible session no
/// update arrives, so the backing would keep the previous appearance's color.
#[cfg(all(feature = "native-terminal", target_os = "macos"))]
fn refresh_window_background<R: Runtime>(app: &AppHandle<R>) -> Result<(), IpcError> {
    let Some(window) = app.get_window("main") else {
        tracing::warn!("Main Ferryx window is unavailable; window background not refreshed");
        return Ok(());
    };
    let background = crate::native_terminal::renderer::RendererTheme::from(
        cached_terminal_preferences().as_ref(),
    )
    .background;
    let target = window.clone();
    window
        .run_on_main_thread(move || {
            let Ok(raw_window) = target.ns_window() else {
                return;
            };
            if raw_window.is_null() {
                return;
            }
            // SAFETY: the pointer comes from Tauri for a live window and is used on the main
            // thread, matching the setup path in lib.rs.
            let native_window = unsafe { &*(raw_window as *const objc2_app_kit::NSWindow) };
            crate::native_terminal::platform::macos::configure_window_background(
                native_window,
                background,
            );
        })
        .map_err(|error| {
            IpcError::internal(format!(
                "Could not dispatch native terminal window background refresh: {error}"
            ))
        })
}

/// Pushes the resolved app appearance and re-themes every live native session when it changed.
#[cfg(feature = "native-terminal")]
#[tauri::command]
pub async fn cmd_terminal_set_appearance<R: Runtime>(
    app: AppHandle<R>,
    daemon_client: State<'_, Arc<DaemonClient>>,
    state: State<'_, NativeTerminalSurfaceHostState>,
    appearance: TerminalAppearance,
) -> Result<TerminalPreferences, IpcError> {
    let (changed, effective) = push_terminal_appearance(appearance).await?;
    if changed {
        rerender_native_sessions(&app, daemon_client.inner(), state.inner()).await?;
        #[cfg(target_os = "macos")]
        refresh_window_background(&app)?;
    }
    Ok(effective)
}

#[cfg(not(feature = "native-terminal"))]
#[tauri::command]
pub async fn cmd_terminal_set_appearance(
    appearance: TerminalAppearance,
) -> Result<TerminalPreferences, IpcError> {
    push_terminal_appearance(appearance)
        .await
        .map(|(_, effective)| effective)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::*;
    use crate::terminal::{
        load_terminal_preferences_from_path_for, preference_cache_test_lock, terminal_appearance,
    };

    fn write_theme(dir: &Path, name: &str, background: &str) -> String {
        let path = dir.join(name);
        fs::write(&path, format!("background = {background}\n")).expect("write theme");
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn set_appearance_rethemes_only_on_change() {
        let _lock = preference_cache_test_lock();
        let previous = terminal_appearance();

        set_terminal_appearance(TerminalAppearance::Dark);
        assert!(set_terminal_appearance(TerminalAppearance::Light));
        assert!(!set_terminal_appearance(TerminalAppearance::Light));
        assert_eq!(terminal_appearance(), TerminalAppearance::Light);

        // Absolute theme paths keep the lookup inside the temp dir.
        let temp = tempfile::tempdir().expect("tempdir");
        let light = write_theme(temp.path(), "bright", "#fafafa");
        let dark = write_theme(temp.path(), "night", "#101010");
        let config = temp.path().join("config");
        fs::write(&config, format!("theme = light:{light},dark:{dark}\n")).expect("write config");

        let light_prefs =
            load_terminal_preferences_from_path_for(&config, TerminalAppearance::Light);
        let dark_prefs = load_terminal_preferences_from_path_for(&config, TerminalAppearance::Dark);
        assert_eq!(light_prefs.theme.background, "#fafafa");
        assert_eq!(dark_prefs.theme.background, "#101010");
        assert_ne!(light_prefs.theme.background, dark_prefs.theme.background);

        set_terminal_appearance(previous);
    }

    #[test]
    fn appearance_argument_rejects_unknown_values_with_a_typed_error() {
        assert_eq!(
            serde_json::from_value::<TerminalAppearance>(serde_json::json!("light"))
                .expect("light deserializes"),
            TerminalAppearance::Light
        );
        assert_eq!(
            serde_json::from_value::<TerminalAppearance>(serde_json::json!("dark"))
                .expect("dark deserializes"),
            TerminalAppearance::Dark
        );
        let error = serde_json::from_value::<TerminalAppearance>(serde_json::json!("blue"))
            .expect_err("unknown appearance must not deserialize");
        assert!(error.is_data(), "expected a data error, got {error:?}");
    }
}
