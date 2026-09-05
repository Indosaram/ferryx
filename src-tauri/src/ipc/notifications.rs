//! Notification IPC commands.
//!
//! The frontend never calls `@tauri-apps/plugin-notification` or
//! `plugin-dialog` directly. Every OS interaction goes through these typed
//! commands, which keeps platform differences out of React, gives tests one
//! mockable contract, and avoids granting broad guest plugin capabilities.

use crate::ipc::{run_blocking, IpcError};
#[cfg(target_os = "macos")]
use crate::notification::format_badge_label;
use crate::notification::{
    open_system_notification_settings, picked_audio_file, DispatchNotificationRequest,
    DispatchNotificationResult, NativeNotificationBackend, NotificationActivations,
    NotificationAudioPlayer, NotificationContent, NotificationPermissionRequestDto,
    NotificationPermissionStatusDto, NotificationProbeResult, NotificationService, NotificationSound,
    NotificationTarget, OpenSystemSettingsResult, PickedAudioFile, PlaySoundResult,
    SetBadgeCountResult, SUPPORTED_AUDIO_EXTENSIONS,
};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Manager, Runtime, State};
use tauri_plugin_dialog::DialogExt;
#[cfg(not(target_os = "macos"))]
use tauri_plugin_notification::NotificationExt;

/// Native backend built on `tauri-plugin-notification`.
pub struct TauriNotificationBackend<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> TauriNotificationBackend<R> {
    pub fn new(app: AppHandle<R>) -> Self {
        Self { app }
    }
}

impl<R: Runtime> NativeNotificationBackend for TauriNotificationBackend<R> {
    fn submit(&self, content: &NotificationContent) -> Result<(), String> {
        // macOS refuses to serve one process over both notification APIs, and our permission
        // query already registers us as a modern UNUserNotificationCenter client, so the plugin's
        // legacy bridge would be denied. Submit over the same API we query.
        #[cfg(target_os = "macos")]
        {
            return crate::notification::permission::macos::submit_notification(content);
        }
        // Windows/Linux: route clicks through notify-rust when the notification
        // carries a destination. notify-rust's `wait_for_response` gives us the
        // real activation the tauri plugin drops. Test / id-less notifications
        // (no target) still show through the plugin.
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        {
            if content.target.is_some() {
                let app_id = self.app.config().identifier.clone();
                return crate::notification::notify_rust_adapter::submit_with_click_routing(
                    content,
                    &app_id,
                    Arc::clone(self.app.state::<Arc<NotificationActivations>>().inner()),
                );
            }
            return self
                .app
                .notification()
                .builder()
                .title(&content.title)
                .body(&content.body)
                .show()
                .map_err(|error| error.to_string());
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            self.app
                .notification()
                .builder()
                .title(&content.title)
                .body(&content.body)
                .show()
                .map_err(|error| error.to_string())
        }
    }
}

/// Build a service bound to the running app.
///
/// The permission provider is the authoritative platform one; on macOS that is
/// `UNUserNotificationCenter`, not Tauri's desktop `permission_state()`.
fn service_for<R: Runtime>(app: &AppHandle<R>) -> NotificationService {
    NotificationService::with_platform_permissions(Box::new(TauriNotificationBackend::new(
        app.clone(),
    )))
}

/// Raise one native notification for an application event.
#[tauri::command]
pub async fn cmd_notification_dispatch<R: Runtime>(
    app: AppHandle<R>,
    request: DispatchNotificationRequest,
) -> Result<DispatchNotificationResult, IpcError> {
    run_blocking(move || Ok(service_for(&app).dispatch(&request))).await
}

/// Atomically drain every pending notification-click activation.
///
/// Called by the frontend on startup and on each `notification_activated` wake
/// signal. Each queued target is returned by exactly one drain, so the caller
/// re-selects each originating pane once and never double-routes focus.
#[tauri::command]
pub async fn cmd_notification_take_activations(
    activations: State<'_, Arc<NotificationActivations>>,
) -> Result<Vec<NotificationTarget>, IpcError> {
    Ok(activations.drain())
}

/// Read the current authoritative permission status.
#[tauri::command]
pub async fn cmd_notification_get_permission_status<R: Runtime>(
    app: AppHandle<R>,
) -> Result<NotificationPermissionStatusDto, IpcError> {
    run_blocking(move || Ok(service_for(&app).permission_status())).await
}

/// Request notification authorization.
///
/// Only ever invoked from an explicit user action (master switch, test
/// notification, or "Allow notifications") - never at startup.
#[tauri::command]
pub async fn cmd_notification_request_permission<R: Runtime>(
    app: AppHandle<R>,
) -> Result<NotificationPermissionRequestDto, IpcError> {
    run_blocking(move || Ok(service_for(&app).request_permission())).await
}

/// Report notification readiness, optionally sending a visible test.
///
/// Never claims a banner was seen: Focus/Do-Not-Disturb suppression is
/// invisible to the application.
#[tauri::command]
pub async fn cmd_notification_probe_delivery<R: Runtime>(
    app: AppHandle<R>,
    send_test: Option<bool>,
    sound: Option<NotificationSound>,
) -> Result<NotificationProbeResult, IpcError> {
    let send_test = send_test.unwrap_or(false);
    run_blocking(move || Ok(service_for(&app).probe_delivery(send_test, sound.unwrap_or_default()))).await
}

/// Open the OS notification settings page for rorca.
#[tauri::command]
pub async fn cmd_notification_open_system_settings() -> Result<OpenSystemSettingsResult, IpcError> {
    run_blocking(|| Ok(open_system_notification_settings())).await
}

/// Play a custom notification sound.
///
/// A failure here is a structured result rather than an IPC error: the
/// notification itself may already have been delivered successfully.
#[tauri::command]
pub async fn cmd_notification_play_sound(
    player: State<'_, Arc<NotificationAudioPlayer>>,
    path: PathBuf,
    volume: Option<f32>,
    force: Option<bool>,
) -> Result<PlaySoundResult, IpcError> {
    let player = Arc::clone(&player);
    let volume = volume.unwrap_or(100.0);
    let force = force.unwrap_or(false);
    run_blocking(move || Ok(player.play(&path, volume, force))).await
}

/// Pick a custom notification sound through the native file dialog.
///
/// Returns only the path and display name; the file is never read into the
/// WebView, and no general filesystem access is exposed.
#[tauri::command]
pub async fn cmd_notification_pick_audio<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Option<PickedAudioFile>, IpcError> {
    let (tx, rx) = tokio::sync::oneshot::channel();

    app.dialog()
        .file()
        .add_filter("Audio", SUPPORTED_AUDIO_EXTENSIONS)
        .pick_file(move |selected| {
            let _ = tx.send(selected);
        });

    let selected = rx
        .await
        .map_err(|_| IpcError::internal("audio file dialog closed unexpectedly"))?;

    // User cancelled the dialog.
    let Some(selected) = selected else {
        return Ok(None);
    };

    let path = selected
        .into_path()
        .map_err(|error| IpcError::internal(format!("invalid audio selection: {error}")))?;

    Ok(Some(picked_audio_file(path)))
}

/// Set or clear the application badge count on the host platform.
///
/// Accepts a numerical unread count derived by the frontend. Formats the badge
/// as a decimal label when count > 0, and clears it when count == 0. On unsupported
/// platforms (Windows / Linux), returns a structured `supported = false` outcome
/// without failing. User-controlled strings are never accepted over IPC.
#[tauri::command]
pub async fn cmd_notification_set_badge_count<R: Runtime>(
    app: AppHandle<R>,
    count: u32,
) -> Result<SetBadgeCountResult, IpcError> {
    #[cfg(target_os = "macos")]
    {
        let label = format_badge_label(count);
        let label_clone = label.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();

        app.run_on_main_thread(move || {
            let res = crate::notification::badge::macos_impl::apply_dock_badge_label(
                label_clone.as_deref(),
            );
            let _ = tx.send(res);
        })
        .map_err(|error| {
            IpcError::internal(format!(
                "failed to schedule badge update on main thread: {error}"
            ))
        })?;

        let outcome = rx
            .await
            .map_err(|_| IpcError::internal("main-thread badge update channel dropped"))?;

        match outcome {
            Ok(()) => Ok(SetBadgeCountResult::macos(count, label)),
            Err(error) => {
                tracing::warn!("failed to apply macOS dock badge: {error}");
                Err(IpcError::internal(format!(
                    "failed to set macOS dock badge: {error}"
                )))
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        Ok(SetBadgeCountResult::unsupported(count))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notification::{
        is_supported_audio_extension, NotificationAuthorization, NotificationDispatchReason,
        NotificationPlatform, NotificationProbeOutcome, NotificationSource, PlaySoundReason,
    };
    use crate::notification::NotificationTarget;
    use serde_json::json;

    /// Mock app managing the same audio and activation state `lib.rs` registers.
    fn mock_app() -> tauri::App<tauri::test::MockRuntime> {
        tauri::test::mock_builder()
            .manage(Arc::new(NotificationAudioPlayer::new()))
            .manage(Arc::new(NotificationActivations::new()))
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app")
    }

    #[tokio::test]
    async fn take_activations_drains_queued_targets_exactly_once() {
        let app = mock_app();
        let activations = app.state::<Arc<NotificationActivations>>();
        activations.push(NotificationTarget {
            workspace_id: "ws-1".into(),
            session_id: "fe-1".into(),
        });
        activations.push(NotificationTarget {
            workspace_id: "ws-2".into(),
            session_id: "fe-2".into(),
        });

        let drained = cmd_notification_take_activations(app.state())
            .await
            .expect("drain resolves");
        assert_eq!(
            drained.iter().map(|t| t.session_id.as_str()).collect::<Vec<_>>(),
            vec!["fe-1", "fe-2"]
        );

        // A second drain with no new click yields nothing: routed exactly once.
        let empty = cmd_notification_take_activations(app.state())
            .await
            .expect("second drain resolves");
        assert!(empty.is_empty());
    }

    #[tokio::test]
    async fn take_activations_on_empty_queue_returns_empty() {
        let app = mock_app();
        let drained = cmd_notification_take_activations(app.state())
            .await
            .expect("drain resolves");
        assert!(drained.is_empty());
    }

    #[tokio::test]
    async fn play_sound_command_reports_missing_file_without_erroring() {
        let app = mock_app();
        let player = app.state::<Arc<NotificationAudioPlayer>>();

        let result = cmd_notification_play_sound(
            player,
            PathBuf::from("/nonexistent/orca-ding.wav"),
            Some(80.0),
            Some(true),
        )
        .await
        .expect("command must resolve, not fail");

        assert!(!result.played);
        assert_eq!(result.reason, Some(PlaySoundReason::NotFound));
    }

    #[tokio::test]
    async fn play_sound_command_rejects_unsupported_formats() {
        let app = mock_app();
        let player = app.state::<Arc<NotificationAudioPlayer>>();
        let dir = tempfile::TempDir::new().expect("tempdir");
        let path = dir.path().join("payload.sh");
        std::fs::write(&path, b"#!/bin/sh\necho nope").expect("write file");

        let result = cmd_notification_play_sound(player, path, Some(50.0), Some(true))
            .await
            .expect("command resolves");

        assert_eq!(result.reason, Some(PlaySoundReason::UnsupportedFormat));
    }

    #[tokio::test]
    async fn play_sound_command_tolerates_omitted_optional_arguments() {
        let app = mock_app();
        let player = app.state::<Arc<NotificationAudioPlayer>>();
        let dir = tempfile::TempDir::new().expect("tempdir");
        let path = dir.path().join("missing.wav");

        let result = cmd_notification_play_sound(player, path, None, None)
            .await
            .expect("command resolves");

        assert!(!result.played);
    }

    #[tokio::test]
    async fn open_system_settings_command_returns_a_structured_result() {
        let result = cmd_notification_open_system_settings()
            .await
            .expect("command resolves");

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            assert!(!result.opened);
            assert_eq!(result.reason.as_deref(), Some("unsupported"));
        }
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            // Opening can legitimately fail in a headless session; only the
            // structured shape is guaranteed.
            assert!(result.opened || result.reason.is_some());
        }
    }

    #[test]
    fn dispatch_command_payload_matches_the_frontend_contract() {
        // Mirrors what ui/src/lib/tauri.ts sends through invokeCommand.
        let request: DispatchNotificationRequest = serde_json::from_value(json!({
            "source": "agentTaskComplete",
            "notificationId": "agent-42",
            "workspaceLabel": "orca-lite",
            "worktreeLabel": "orca/ws-1/feat-login",
            "terminalTitle": "claude",
            "agentLabel": "Claude",
        }))
        .expect("payload parses");

        assert_eq!(request.source, NotificationSource::AgentTaskComplete);
        assert_eq!(request.agent_label.as_deref(), Some("Claude"));
        assert_eq!(request.notification_id.as_deref(), Some("agent-42"));
    }

    #[test]
    fn dispatch_result_round_trips_across_the_ipc_boundary() {
        let result =
            DispatchNotificationResult::rejected(NotificationDispatchReason::PermissionRequired);

        let encoded = serde_json::to_string(&result).expect("encode");
        let decoded: DispatchNotificationResult = serde_json::from_str(&encoded).expect("decode");

        assert_eq!(decoded, result);
    }

    #[test]
    fn permission_status_round_trips_across_the_ipc_boundary() {
        let status = NotificationPermissionStatusDto {
            platform: NotificationPlatform::Macos,
            supported: true,
            authorization: NotificationAuthorization::Denied,
            alerts_enabled: Some(false),
            sounds_enabled: Some(true),
            requested: true,
            authoritative: true,
            can_open_settings: true,
        };

        let encoded = serde_json::to_string(&status).expect("encode");
        let decoded: NotificationPermissionStatusDto =
            serde_json::from_str(&encoded).expect("decode");

        assert_eq!(decoded, status);
        assert!(decoded.is_blocked());
    }

    #[test]
    fn permission_request_result_round_trips_across_the_ipc_boundary() {
        let dto = NotificationPermissionRequestDto {
            granted: false,
            status: NotificationPermissionStatusDto::non_authoritative(
                NotificationPlatform::Windows,
                true,
            ),
            error: Some("authorization request timed out".into()),
        };

        let encoded = serde_json::to_string(&dto).expect("encode");
        let decoded: NotificationPermissionRequestDto =
            serde_json::from_str(&encoded).expect("decode");

        assert_eq!(decoded, dto);
    }

    #[test]
    fn probe_result_round_trips_across_the_ipc_boundary() {
        let probe = NotificationProbeResult {
            outcome: NotificationProbeOutcome::Submitted,
            status: NotificationPermissionStatusDto::non_authoritative(
                NotificationPlatform::Linux,
                false,
            ),
            test_submitted: true,
        };

        let encoded = serde_json::to_string(&probe).expect("encode");
        let decoded: NotificationProbeResult = serde_json::from_str(&encoded).expect("decode");

        assert_eq!(decoded, probe);
    }

    #[test]
    fn picked_audio_result_round_trips_including_cancellation() {
        let cancelled: Option<PickedAudioFile> =
            serde_json::from_str("null").expect("null decodes as cancellation");
        assert!(cancelled.is_none());

        let picked = Some(picked_audio_file(PathBuf::from("/tmp/ding.wav")));
        let encoded = serde_json::to_string(&picked).expect("encode");
        let decoded: Option<PickedAudioFile> = serde_json::from_str(&encoded).expect("decode");

        assert_eq!(decoded, picked);
    }

    #[test]
    fn picker_filters_only_offer_decodable_formats() {
        // The dialog filter must not advertise formats rodio cannot decode.
        for extension in SUPPORTED_AUDIO_EXTENSIONS {
            assert!(
                is_supported_audio_extension(std::path::Path::new(&format!("sound.{extension}"))),
                "picker filter offers undecodable extension: {extension}"
            );
        }
    }

    #[tokio::test]
    async fn set_badge_count_command_returns_structured_result() {
        // Given: a mock app and an unread count
        let app = mock_app();
        let count = 42;

        // When: invoking the badge count command with AppHandle
        let result = cmd_notification_set_badge_count(app.handle().clone(), count).await;

        // Then: the structured result reflects the host platform capability and main-thread safety
        #[cfg(target_os = "macos")]
        {
            match result {
                Ok(outcome) => {
                    assert!(outcome.supported);
                    assert_eq!(outcome.count, 42);
                    assert_eq!(outcome.badge_label.as_deref(), Some("42"));
                }
                Err(error) => {
                    // In mock test runner threads, MockRuntime dispatches off the OS main thread.
                    // Accurate error reporting is verified.
                    assert!(
                        error
                            .to_string()
                            .contains("must be called on the macOS main thread")
                            || error
                                .to_string()
                                .contains("failed to schedule badge update")
                    );
                }
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let outcome = result.expect("command resolves on non-macos as unsupported no-op");
            assert!(!outcome.supported);
            assert_eq!(outcome.count, 42);
            assert!(outcome.badge_label.is_none());
        }
    }

    #[tokio::test]
    async fn set_badge_count_command_clears_badge_at_zero() {
        // Given: a mock app and count = 0 to clear
        let app = mock_app();
        let count = 0;

        // When: invoking the badge count command with AppHandle
        let result = cmd_notification_set_badge_count(app.handle().clone(), count).await;

        // Then: badge_label is None when successful, or structured error if off main thread
        #[cfg(target_os = "macos")]
        {
            match result {
                Ok(outcome) => {
                    assert!(outcome.supported);
                    assert_eq!(outcome.count, 0);
                    assert!(outcome.badge_label.is_none());
                }
                Err(error) => {
                    assert!(
                        error
                            .to_string()
                            .contains("must be called on the macOS main thread")
                            || error
                                .to_string()
                                .contains("failed to schedule badge update")
                    );
                }
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let outcome = result.expect("command resolves on non-macos as unsupported no-op");
            assert!(!outcome.supported);
            assert_eq!(outcome.count, 0);
            assert!(outcome.badge_label.is_none());
        }
    }

    #[test]
    fn set_badge_count_result_round_trips_across_ipc_boundary() {
        // Given: a macOS badge result with positive count
        let result = crate::notification::SetBadgeCountResult::macos(7, Some("7".into()));

        // When: encoding and decoding through JSON
        let encoded = serde_json::to_string(&result).expect("encode");
        let decoded: crate::notification::SetBadgeCountResult =
            serde_json::from_str(&encoded).expect("decode");

        // Then: deserialized matches original
        assert_eq!(decoded, result);
        assert_eq!(
            serde_json::to_value(&result).unwrap(),
            json!({
                "supported": true,
                "count": 7,
                "badgeLabel": "7"
            })
        );

        // Given: a cleared badge result
        let cleared = crate::notification::SetBadgeCountResult::macos(0, None);
        let encoded_cleared = serde_json::to_string(&cleared).expect("encode");
        let decoded_cleared: crate::notification::SetBadgeCountResult =
            serde_json::from_str(&encoded_cleared).expect("decode");
        assert_eq!(decoded_cleared, cleared);
        assert_eq!(
            serde_json::to_value(&cleared).unwrap(),
            json!({
                "supported": true,
                "count": 0
            })
        );
    }
}
