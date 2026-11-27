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
    open_system_notification_settings, open_system_notification_settings_with_launcher,
    picked_audio_file, DispatchNotificationRequest, DispatchNotificationResult,
    NativeNotificationBackend, NotificationActivations, NotificationAudioPlayer,
    NotificationContent, NotificationOrderFence, NotificationPermissionRequestDto,
    NotificationPermissionStatusDto, NotificationPlatform, NotificationProbeResult,
    NotificationService, NotificationSound, NotificationTarget,
    OpenSystemSettingsResult, PickedAudioFile, PlaySoundResult, SetBadgeCountResult,
    SUPPORTED_AUDIO_EXTENSIONS,
};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use tauri::{AppHandle, Runtime, State};
#[cfg(any(target_os = "windows", target_os = "linux"))]
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;
#[cfg(not(target_os = "macos"))]
use tauri_plugin_notification::NotificationExt;

/// Native backend built on `tauri-plugin-notification`.
pub struct TauriNotificationBackend<R: Runtime> {
    #[cfg(not(target_os = "macos"))]
    app: AppHandle<R>,
    #[cfg(target_os = "macos")]
    _app: AppHandle<R>,
}

impl<R: Runtime> TauriNotificationBackend<R> {
    pub fn new(app: AppHandle<R>) -> Self {
        #[cfg(target_os = "macos")]
        {
            Self { _app: app }
        }
        #[cfg(not(target_os = "macos"))]
        {
            Self { app }
        }
    }
}

impl<R: Runtime> NativeNotificationBackend for TauriNotificationBackend<R> {
    fn submit(&self, content: &NotificationContent) -> Result<(), String> {
        self.submit_inner(content, &|| false)
    }

    /// A fenced dispatch: the `notify-rust` path re-checks the order fence after its banner slot
    /// exists, so a withdrawal that landed before that registration still closes the banner.
    fn submit_fenced(
        &self,
        content: &NotificationContent,
        superseded: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<(), String> {
        self.submit_inner(content, superseded)
    }

    /// Remove the delivered notification for `target`, if the platform can.
    ///
    /// macOS addresses the delivered request by its stable pane identifier.
    /// Windows/Linux route through the `notify-rust` waiter registry, where a
    /// withdrawal closes the banner still waiting for that pane and routes no
    /// activation, so a superseded notification cannot steal focus later.
    fn withdraw(&self, target: &NotificationTarget) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            crate::notification::permission::macos::withdraw_notification(target)
        }
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        {
            crate::notification::notify_rust_adapter::withdraw_routed_notification(target)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            let _ = target;
            Ok(())
        }
    }
}

impl<R: Runtime> TauriNotificationBackend<R> {
    fn submit_inner(
        &self,
        content: &NotificationContent,
        superseded: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<(), String> {
        // macOS refuses to serve one process over both notification APIs, and our permission
        // query already registers us as a modern UNUserNotificationCenter client, so the plugin's
        // legacy bridge would be denied. Submit over the same API we query.
        #[cfg(target_os = "macos")]
        {
            // macOS delivers through UNUserNotificationCenter, which has no order fence to
            // re-check after its banner slot exists: the withdrawal path closes the banner itself.
            // Named so a macOS build is warning-free under `-D warnings`.
            let _ = superseded;
            crate::notification::permission::macos::submit_notification(content)
        }
        // Windows/Linux: route clicks through notify-rust when the notification
        // carries a destination. notify-rust's `wait_for_response` gives us the
        // real activation the tauri plugin drops. Test / id-less notifications
        // (no target) still show through the plugin on Windows.
        //
        // Linux shows every notification through notify-rust on the calling thread
        // (callers run in `run_blocking`). The plugin's desktop `show()` spawns
        // notify-rust onto a Tokio worker, and on Linux notify-rust calls
        // `zbus::block_on`, which (zbus's `tokio` feature is on through ashpd) starts
        // its own runtime there and panics with "Cannot start a runtime from within a
        // runtime": the page notification never reached D-Bus (linux UI audit run
        // 41846200603, daily G6).
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        {
            if shows_through_notify_rust(NotificationPlatform::current(), content.target.is_some()) {
                let app_id = self.app.config().identifier.clone();
                return crate::notification::notify_rust_adapter::submit_with_click_routing(
                    content,
                    &app_id,
                    Arc::clone(self.app.state::<Arc<NotificationActivations>>().inner()),
                    superseded,
                );
            }
            let builder = self
                .app
                .notification()
                .builder()
                .title(&content.title)
                .body(&content.body);
            // Windows renders an unset sound name as a silent toast, so the
            // default `System` choice has to name the platform default
            // explicitly; `Silent` stays unset, which is already silent there.
            // Linux is untouched: `"Default"` is not a freedesktop sound name,
            // and the plugin builder exposes no suppress-sound hint.
            #[cfg(target_os = "windows")]
            let builder = if content.sound == NotificationSound::System {
                builder.sound(crate::notification::notify_rust_adapter::DEFAULT_SOUND_NAME)
            } else {
                builder
            };
            builder.show().map_err(|error| error.to_string())
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            // No platform-specific fence on this target either, so name the closure rather than
            // leave the parameter unused.
            let _ = superseded;
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

/// Whether a Windows/Linux notification is shown through `notify-rust` on the calling thread
/// rather than the plugin's `show()`: always on Linux (the plugin's Tokio-spawned `show()`
/// panics there, see `submit`), and on Windows only when a click target has to be routed.
#[cfg_attr(not(any(target_os = "windows", target_os = "linux")), allow(dead_code))]
fn shows_through_notify_rust(platform: NotificationPlatform, has_target: bool) -> bool {
    platform == NotificationPlatform::Linux || has_target
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

/// The one ordering fence every dispatch and withdrawal shares.
///
/// The sequence allocator and the per-pane watermarks have to be the same
/// objects on both paths, or a recall can never be recognised as newer than the
/// prompt it supersedes.
static ORDER_FENCE: OnceLock<NotificationOrderFence> = OnceLock::new();

fn notification_order_fence() -> &'static NotificationOrderFence {
    ORDER_FENCE.get_or_init(NotificationOrderFence::default)
}

/// Raise one native notification for an application event.
#[tauri::command]
pub async fn cmd_notification_dispatch<R: Runtime>(
    app: AppHandle<R>,
    request: DispatchNotificationRequest,
) -> Result<DispatchNotificationResult, IpcError> {
    // Stamped here, before the blocking hop: workers start and finish in any
    // order, so a sequence taken inside `run_blocking` would order by thread
    // scheduling instead of by the order the frontend asked.
    let seq = notification_order_fence().next_seq();
    run_blocking(move || {
        Ok(service_for(&app).dispatch_sequenced(&request, notification_order_fence(), seq))
    })
    .await
}

/// Remove the delivered notification for `target`.
///
/// Called when the pane leaves the attention state the banner announced (the
/// user is back on it) or when the frontend acknowledges the session. No
/// permission preflight runs: the banner is already on screen, and revoking
/// alerts later does not make it disappear. The sequence is stamped
/// synchronously for the same reason dispatch stamps its own - the recall is
/// ordered against the prompt it supersedes, not against whichever blocking
/// worker happens to finish first.
#[tauri::command]
pub async fn cmd_notification_withdraw<R: Runtime>(
    app: AppHandle<R>,
    target: NotificationTarget,
) -> Result<(), IpcError> {
    let seq = notification_order_fence().next_seq();
    run_blocking(move || {
        service_for(&app)
            .withdraw_sequenced(&target, notification_order_fence(), seq)
            .map_err(IpcError::internal)
    })
    .await
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

/// Posts the authorization *start* onto the main thread and returns immediately.
#[cfg(target_os = "macos")]
struct AppMainThread<R: Runtime>(AppHandle<R>);

#[cfg(target_os = "macos")]
impl<R: Runtime> crate::notification::MainThreadScheduler for AppMainThread<R> {
    fn schedule(&self, work: Box<dyn FnOnce() + Send>) {
        let _ = self.0.run_on_main_thread(work);
    }
}

/// Request notification authorization.
///
/// Only ever invoked from an explicit user action (master switch, test
/// notification, or "Allow notifications") - never at startup.
/// The macOS prompt is started on the main thread; the wait stays off it.
#[tauri::command]
pub async fn cmd_notification_request_permission<R: Runtime>(
    app: AppHandle<R>,
) -> Result<NotificationPermissionRequestDto, IpcError> {
    #[cfg(target_os = "macos")]
    {
        let scheduler = AppMainThread(app);
        run_blocking(move || {
            let (granted, error) = crate::notification::request_authorization_with(
                &scheduler,
                crate::notification::permission::macos::begin_authorization_request,
                crate::notification::permission::authorization_wait(),
            );
            crate::notification::invalidate_permission_cache();
            let status = crate::notification::platform_permission_provider().status();
            Ok(NotificationPermissionRequestDto {
                granted,
                status,
                error,
            })
        })
        .await
    }
    #[cfg(not(target_os = "macos"))]
    {
        run_blocking(move || Ok(service_for(&app).request_permission())).await
    }
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
    run_blocking(move || Ok(service_for(&app).probe_delivery(send_test, sound.unwrap_or_default())))
        .await
}

/// Open the OS notification settings page for rorca.
#[tauri::command]
pub async fn cmd_notification_open_system_settings() -> Result<OpenSystemSettingsResult, IpcError> {
    run_blocking(|| Ok(open_system_notification_settings())).await
}

pub async fn cmd_notification_open_system_settings_with_launcher(
    launch: impl FnOnce(&str, &[&str]) -> bool + Send + 'static,
) -> Result<OpenSystemSettingsResult, IpcError> {
    run_blocking(|| Ok(open_system_notification_settings_with_launcher(launch))).await
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
    use crate::notification::service::test_support::{macos_status, service_with, FakeBackend};
    use crate::notification::NotificationTarget;
    use crate::notification::{
        is_supported_audio_extension, NotificationAuthorization, NotificationDispatchReason,
        NotificationPlatform, NotificationProbeOutcome, NotificationSource, PlaySoundReason,
    };
    use serde_json::json;
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    use tauri::Manager;

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
            drained
                .iter()
                .map(|t| t.session_id.as_str())
                .collect::<Vec<_>>(),
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
        let result = cmd_notification_open_system_settings_with_launcher(|_program, _args| true)
            .await
            .expect("command resolves");

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            assert!(!result.opened);
            assert_eq!(result.reason.as_deref(), Some("unsupported"));
        }
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            assert!(result.opened);
            assert!(result.reason.is_none());
        }

        let failed = cmd_notification_open_system_settings_with_launcher(|_program, _args| false)
            .await
            .expect("command resolves");

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            assert!(!failed.opened);
            assert_eq!(failed.reason.as_deref(), Some("unsupported"));
        }
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            assert!(!failed.opened);
            assert!(failed.reason.is_some());
        }
    }

    #[test]
    fn dispatch_command_payload_matches_the_frontend_contract() {
        // Mirrors what ui/src/lib/tauri.ts sends through invokeCommand.
        let request: DispatchNotificationRequest = serde_json::from_value(json!({
            "source": "agentTaskComplete",
            "notificationId": "agent-42",
            "workspaceLabel": "orca-lite",
            "worktreeLabel": "ferryx/ws-1/feat-login",
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
            badges_enabled: Some(true),
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

    /// Linux never goes through the plugin's `show()`: it spawns notify-rust onto a Tokio
    /// worker, where zbus's `block_on` panics (linux UI audit run 41846200603, daily G6).
    #[test]
    fn linux_shows_every_notification_through_notify_rust() {
        assert!(shows_through_notify_rust(NotificationPlatform::Linux, false));
        assert!(shows_through_notify_rust(NotificationPlatform::Linux, true));
        // Windows keeps the plugin for target-less notifications.
        assert!(!shows_through_notify_rust(NotificationPlatform::Windows, false));
        assert!(shows_through_notify_rust(NotificationPlatform::Windows, true));
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

    // -----------------------------------------------------------------------
    // Sequenced withdrawal over IPC
    // -----------------------------------------------------------------------

    /// One pane target no other test in this binary touches, so the process-wide
    /// order fence cannot leak state between tests.
    fn ipc_test_target(suffix: &str) -> NotificationTarget {
        NotificationTarget {
            workspace_id: format!("ws-ipc-withdraw-{suffix}"),
            session_id: format!("fe-ipc-withdraw-{suffix}"),
        }
    }

    fn ipc_test_request(target: &NotificationTarget) -> DispatchNotificationRequest {
        DispatchNotificationRequest {
            source: NotificationSource::AgentTaskComplete,
            agent_label: Some("Claude".into()),
            target: Some(target.clone()),
            ..Default::default()
        }
    }

    fn ipc_test_service(backend: &Arc<FakeBackend>) -> NotificationService {
        service_with(
            macos_status(NotificationAuthorization::Authorized),
            Arc::clone(backend),
        )
    }

    #[tokio::test]
    async fn withdraw_allocates_monotonic_sequences_and_delegates_cleanly() {
        // Given: the process-wide fence the commands share and a fake backend.
        let backend = Arc::new(FakeBackend::default());
        let fence = notification_order_fence();

        // When: three withdrawals are stamped before their blocking hop, the way
        // `cmd_notification_withdraw` stamps them.
        let sequences = [fence.next_seq(), fence.next_seq(), fence.next_seq()];

        // Then: every request owns a strictly newer sequence than the last.
        assert!(sequences[0] < sequences[1]);
        assert!(sequences[1] < sequences[2]);

        // And each one still reaches the backend cleanly, one delegation each.
        for (index, seq) in sequences.into_iter().enumerate() {
            let target = ipc_test_target(&format!("monotonic-{index}"));
            let service = ipc_test_service(&backend);
            run_blocking(move || {
                service
                    .withdraw_sequenced(&target, fence, seq)
                    .map_err(IpcError::internal)
            })
            .await
            .expect("withdraw resolves");
        }
        assert_eq!(backend.withdrawn_count(), 3);
        assert_eq!(backend.count(), 0);
    }

    #[tokio::test]
    async fn reverse_order_blocking_completion_preserves_the_newer_dispatch() {
        // Given: one pane and the three intents a fast frontend produces -
        // dispatch, withdrawal, newer dispatch - each stamped synchronously
        // before its blocking task is queued, exactly as the commands do.
        let backend = Arc::new(FakeBackend::default());
        let target = ipc_test_target("reverse");
        let fence = notification_order_fence();
        let stale_dispatch_seq = fence.next_seq();
        let stale_withdraw_seq = fence.next_seq();
        let newest_dispatch_seq = fence.next_seq();

        // When: the blocking workers finish in reverse order, so the newest
        // prompt lands first, then the recall that superseded the first prompt,
        // then the first prompt itself.
        let newest = run_blocking({
            let service = ipc_test_service(&backend);
            let target = target.clone();
            move || {
                Ok(service.dispatch_sequenced(
                    &ipc_test_request(&target),
                    fence,
                    newest_dispatch_seq,
                ))
            }
        })
        .await
        .expect("newest dispatch resolves");

        run_blocking({
            let service = ipc_test_service(&backend);
            let target = target.clone();
            move || {
                service
                    .withdraw_sequenced(&target, fence, stale_withdraw_seq)
                    .map_err(IpcError::internal)
            }
        })
        .await
        .expect("stale withdraw resolves");

        let stale = run_blocking({
            let service = ipc_test_service(&backend);
            move || {
                Ok(service.dispatch_sequenced(
                    &ipc_test_request(&target),
                    fence,
                    stale_dispatch_seq,
                ))
            }
        })
        .await
        .expect("stale dispatch resolves");

        // Then: the newest banner is still on screen, the stale recall did not
        // delete it, and the superseded dispatch never reached the backend.
        assert!(newest.submitted);
        assert_eq!(stale, DispatchNotificationResult::suppressed());
        assert_eq!(backend.count(), 1);
        assert_eq!(backend.withdrawn_count(), 0);
    }
}
