//! Pure notification domain tests.
//!
//! Nothing here opens an audio device or displays a real notification.

use super::model::*;
use serde_json::json;
use std::path::{Path, PathBuf};

#[test]
fn notification_wire_preserves_sound_and_attention_reason() {
    let parsed: DispatchNotificationRequest = serde_json::from_value(json!({
        "source": "agent-task-complete", "sound": "silent", "attentionReason": "waiting"
    })).unwrap();
    let wire = serde_json::to_value(&parsed).unwrap();
    assert_eq!(wire["sound"], "silent");
    assert_eq!(wire["attentionReason"], "waiting");
    let content = serde_json::to_value(format_notification(&parsed)).unwrap();
    assert_eq!(content["sound"], "silent");
}

#[test]
fn notification_fallback_does_not_claim_permission() {
    use super::permission::{DesktopFallbackPermissionProvider, NotificationPermissionProvider};
    let status = DesktopFallbackPermissionProvider.status();
    assert_eq!(status.authorization, NotificationAuthorization::Unknown);
    assert_eq!(status.alerts_enabled, None);
    assert_eq!(status.sounds_enabled, None);
}

fn request(source: NotificationSource) -> DispatchNotificationRequest {
    DispatchNotificationRequest {
        source,
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Sanitization
// ---------------------------------------------------------------------------

#[test]
fn sanitize_strips_control_characters_and_escape_sequences() {
    // A terminal title can carry escape sequences; none may survive into
    // OS notification text.
    let raw = "build\u{1b}[31m done\u{7}\u{0}";
    assert_eq!(sanitize_text(raw), "build [31m done");
}

#[test]
fn sanitize_collapses_newlines_and_pathological_whitespace() {
    let raw = "  line one\n\n\n\tline    two  \r\n";
    assert_eq!(sanitize_text(raw), "line one line two");
}

#[test]
fn sanitize_removes_unicode_line_separators_and_bom() {
    let raw = "alpha\u{2028}beta\u{2029}gamma\u{feff}delta";
    assert_eq!(sanitize_text(raw), "alpha beta gamma delta");
}

#[test]
fn sanitize_preserves_ordinary_unicode_text() {
    assert_eq!(sanitize_text("café ✅ 日本語"), "café ✅ 日本語");
}

#[test]
fn sanitize_of_only_control_characters_is_empty() {
    assert_eq!(sanitize_text("\u{0}\u{1}\n\t  "), "");
}

// ---------------------------------------------------------------------------
// Truncation
// ---------------------------------------------------------------------------

#[test]
fn truncate_leaves_short_text_untouched() {
    assert_eq!(truncate_text("short", 10), "short");
    assert_eq!(truncate_text("exactly10!", 10), "exactly10!");
}

#[test]
fn truncate_appends_ellipsis_and_respects_the_limit() {
    let truncated = truncate_text("abcdefghijklmnop", 10);

    assert_eq!(truncated.chars().count(), 10);
    assert!(truncated.ends_with('\u{2026}'));
}

#[test]
fn truncate_splits_on_character_boundaries_not_bytes() {
    // Multi-byte characters must not be cut in half.
    let truncated = truncate_text("日本語のテキストです", 5);

    assert_eq!(truncated.chars().count(), 5);
    assert!(truncated.is_char_boundary(truncated.len()));
}

// ---------------------------------------------------------------------------
// Content formatting
// ---------------------------------------------------------------------------

#[test]
fn agent_completion_uses_agent_name_and_worktree_label() {
    let content = format_notification(&DispatchNotificationRequest {
        source: NotificationSource::AgentTaskComplete,
        agent_label: Some("Claude".into()),
        worktree_label: Some("orca/ws-1/feat-login".into()),
        ..Default::default()
    });

    assert_eq!(content.title, "Claude finished");
    assert_eq!(content.body, "orca/ws-1/feat-login");
}

#[test]
fn agent_completion_falls_back_when_labels_are_missing() {
    let content = format_notification(&request(NotificationSource::AgentTaskComplete));

    assert_eq!(content.title, "Agent finished");
    assert_eq!(content.body, "Task complete");
}

#[test]
fn agent_completion_falls_back_to_workspace_label() {
    let content = format_notification(&DispatchNotificationRequest {
        source: NotificationSource::AgentTaskComplete,
        agent_label: Some("Codex".into()),
        workspace_label: Some("orca-lite".into()),
        ..Default::default()
    });

    assert_eq!(content.body, "orca-lite");
}

#[test]
fn terminal_bell_joins_location_and_terminal_title() {
    let content = format_notification(&DispatchNotificationRequest {
        source: NotificationSource::TerminalBell,
        worktree_label: Some("feat/login".into()),
        terminal_title: Some("npm test".into()),
        ..Default::default()
    });

    assert_eq!(content.title, "Terminal needs attention");
    assert_eq!(content.body, "feat/login \u{b7} npm test");
}

#[test]
fn terminal_bell_without_any_label_still_has_a_body() {
    let content = format_notification(&request(NotificationSource::TerminalBell));

    assert_eq!(content.body, "A terminal rang the bell");
}

#[test]
fn test_notification_has_fixed_content() {
    let content = format_notification(&request(NotificationSource::Test));

    assert_eq!(content.title, "Ferryx notifications are working");
    assert_eq!(content.body, "Test notification");
}

#[test]
fn formatted_content_never_exceeds_length_bounds() {
    let content = format_notification(&DispatchNotificationRequest {
        source: NotificationSource::TerminalBell,
        worktree_label: Some("w".repeat(500)),
        terminal_title: Some("t".repeat(500)),
        ..Default::default()
    });

    assert!(content.title.chars().count() <= MAX_TITLE_LEN);
    assert!(content.body.chars().count() <= MAX_BODY_LEN);
}

#[test]
fn formatted_content_is_sanitized_against_injected_control_characters() {
    let content = format_notification(&DispatchNotificationRequest {
        source: NotificationSource::AgentTaskComplete,
        agent_label: Some("Claude\n\u{1b}[2J".into()),
        worktree_label: Some("feat\u{0}/login".into()),
        ..Default::default()
    });

    assert!(!content.title.contains('\n'));
    assert!(!content.title.contains('\u{1b}'));
    assert!(!content.body.contains('\u{0}'));
}

#[test]
fn whitespace_only_labels_are_treated_as_absent() {
    let content = format_notification(&DispatchNotificationRequest {
        source: NotificationSource::AgentTaskComplete,
        agent_label: Some("   \n  ".into()),
        worktree_label: Some("\t".into()),
        ..Default::default()
    });

    assert_eq!(content.title, "Agent finished");
    assert_eq!(content.body, "Task complete");
}

// ---------------------------------------------------------------------------
// Serialization contract with the frontend
// ---------------------------------------------------------------------------

#[test]
fn notification_source_uses_camel_case_wire_values() {
    assert_eq!(
        serde_json::to_value(NotificationSource::AgentTaskComplete).unwrap(),
        json!("agentTaskComplete")
    );
    assert_eq!(
        serde_json::to_value(NotificationSource::TerminalBell).unwrap(),
        json!("terminalBell")
    );
    assert_eq!(
        serde_json::to_value(NotificationSource::Test).unwrap(),
        json!("test")
    );
}

#[test]
fn dispatch_request_deserializes_from_camel_case_payload() {
    let parsed: DispatchNotificationRequest = serde_json::from_value(json!({
        "source": "terminalBell",
        "notificationId": "bell-1",
        "workspaceLabel": "orca-lite",
        "worktreeLabel": "feat/login",
        "terminalTitle": "vim",
        "agentLabel": null,
    }))
    .expect("parse dispatch request");

    assert_eq!(parsed.source, NotificationSource::TerminalBell);
    assert_eq!(parsed.notification_id.as_deref(), Some("bell-1"));
    assert_eq!(parsed.terminal_title.as_deref(), Some("vim"));
    assert!(parsed.agent_label.is_none());
}

#[test]
fn dispatch_request_deserializes_from_kebab_case_payload() {
    let parsed_agent: DispatchNotificationRequest = serde_json::from_value(json!({
        "source": "agent-task-complete",
        "agentLabel": "claude",
    }))
    .expect("parse kebab-case agent-task-complete");
    assert_eq!(parsed_agent.source, NotificationSource::AgentTaskComplete);
    assert_eq!(parsed_agent.agent_label.as_deref(), Some("claude"));

    let parsed_bell: DispatchNotificationRequest = serde_json::from_value(json!({
        "source": "terminal-bell",
        "worktreeLabel": "main",
    }))
    .expect("parse kebab-case terminal-bell");
    assert_eq!(parsed_bell.source, NotificationSource::TerminalBell);
    assert_eq!(parsed_bell.worktree_label.as_deref(), Some("main"));
}

#[test]
fn dispatch_request_tolerates_a_minimal_payload() {
    let parsed: DispatchNotificationRequest =
        serde_json::from_value(json!({ "source": "test" })).expect("parse minimal request");

    assert_eq!(parsed.source, NotificationSource::Test);
    assert!(parsed.worktree_label.is_none());
}

#[test]
fn dispatch_reasons_serialize_as_kebab_case() {
    assert_eq!(
        serde_json::to_value(NotificationDispatchReason::PermissionRequired).unwrap(),
        json!("permission-required")
    );
    assert_eq!(
        serde_json::to_value(NotificationDispatchReason::BlockedBySystem).unwrap(),
        json!("blocked-by-system")
    );
    assert_eq!(
        serde_json::to_value(NotificationDispatchReason::BackendError).unwrap(),
        json!("backend-error")
    );
}

#[test]
fn successful_dispatch_result_omits_the_reason_field() {
    let value = serde_json::to_value(DispatchNotificationResult::submitted()).unwrap();

    assert_eq!(value, json!({ "submitted": true }));
}

#[test]
fn rejected_dispatch_result_carries_its_reason() {
    let value = serde_json::to_value(DispatchNotificationResult::rejected(
        NotificationDispatchReason::BlockedBySystem,
    ))
    .unwrap();

    assert_eq!(
        value,
        json!({ "submitted": false, "reason": "blocked-by-system" })
    );
}

#[test]
fn permission_status_dto_matches_the_documented_wire_shape() {
    let status = NotificationPermissionStatusDto {
        platform: NotificationPlatform::Macos,
        supported: true,
        authorization: NotificationAuthorization::NotDetermined,
        alerts_enabled: Some(true),
        sounds_enabled: None,
        requested: false,
        authoritative: true,
        can_open_settings: true,
    };

    assert_eq!(
        serde_json::to_value(&status).unwrap(),
        json!({
            "platform": "macos",
            "supported": true,
            "authorization": "not-determined",
            "alertsEnabled": true,
            "soundsEnabled": null,
            "requested": false,
            "authoritative": true,
            "canOpenSettings": true,
        })
    );
}

#[test]
fn permission_request_dto_serializes_with_nested_status() {
    let dto = NotificationPermissionRequestDto {
        granted: true,
        status: NotificationPermissionStatusDto::non_authoritative(
            NotificationPlatform::Linux,
            false,
        ),
        error: None,
    };

    let value = serde_json::to_value(&dto).unwrap();

    assert_eq!(value["granted"], json!(true));
    assert_eq!(value["status"]["platform"], json!("linux"));
    // A successful request carries no error key at all.
    assert!(value.get("error").is_none());
}

#[test]
fn probe_result_serializes_outcome_and_submission_flag() {
    let value = serde_json::to_value(NotificationProbeResult {
        outcome: NotificationProbeOutcome::PermissionRequired,
        status: NotificationPermissionStatusDto::unsupported(NotificationPlatform::Other),
        test_submitted: false,
    })
    .unwrap();

    assert_eq!(value["outcome"], json!("permission-required"));
    assert_eq!(value["testSubmitted"], json!(false));
    assert_eq!(value["status"]["supported"], json!(false));
}

#[test]
fn play_sound_result_serializes_kebab_case_reasons() {
    assert_eq!(
        serde_json::to_value(PlaySoundResult::failed(PlaySoundReason::NoOutputDevice)).unwrap(),
        json!({ "played": false, "reason": "no-output-device" })
    );
    assert_eq!(
        serde_json::to_value(PlaySoundResult::played()).unwrap(),
        json!({ "played": true })
    );
}

#[test]
fn picked_audio_file_serializes_path_and_display_name() {
    let value = serde_json::to_value(PickedAudioFile {
        path: PathBuf::from("/tmp/ding.wav"),
        display_name: "ding.wav".into(),
    })
    .unwrap();

    assert_eq!(value["displayName"], json!("ding.wav"));
    assert_eq!(value["path"], json!("/tmp/ding.wav"));
}

// ---------------------------------------------------------------------------
// Permission status helpers
// ---------------------------------------------------------------------------

#[test]
fn only_authoritative_denied_status_counts_as_blocked() {
    let mut denied =
        NotificationPermissionStatusDto::non_authoritative(NotificationPlatform::Macos, true);
    denied.authorization = NotificationAuthorization::Denied;

    // Not authoritative yet -> not blocked.
    assert!(!denied.is_blocked());

    denied.authoritative = true;
    assert!(denied.is_blocked());
}

#[test]
fn only_authoritative_not_determined_requires_a_permission_request() {
    let mut status =
        NotificationPermissionStatusDto::non_authoritative(NotificationPlatform::Windows, true);
    status.authorization = NotificationAuthorization::NotDetermined;
    assert!(!status.needs_permission_request());

    status.authoritative = true;
    assert!(status.needs_permission_request());
}

#[test]
fn unsupported_status_reports_no_capabilities() {
    let status = NotificationPermissionStatusDto::unsupported(NotificationPlatform::Other);

    assert!(!status.supported);
    assert!(!status.can_open_settings);
    assert_eq!(status.authorization, NotificationAuthorization::Unknown);
}

// ---------------------------------------------------------------------------
// Volume normalization
// ---------------------------------------------------------------------------

#[test]
fn volume_percentage_maps_to_linear_gain() {
    assert_eq!(normalize_volume(0.0), 0.0);
    assert_eq!(normalize_volume(50.0), 0.5);
    assert_eq!(normalize_volume(100.0), 1.0);
}

#[test]
fn out_of_range_volume_is_clamped() {
    assert_eq!(normalize_volume(-25.0), 0.0);
    assert_eq!(normalize_volume(1000.0), 1.0);
    assert_eq!(normalize_volume(f32::INFINITY), 1.0);
    assert_eq!(normalize_volume(f32::NEG_INFINITY), 0.0);
}

#[test]
fn nan_volume_is_silent_rather_than_undefined() {
    assert_eq!(normalize_volume(f32::NAN), 0.0);
}

// ---------------------------------------------------------------------------
// Audio path validation
// ---------------------------------------------------------------------------

#[test]
fn supported_extensions_are_recognized_case_insensitively() {
    assert!(is_supported_audio_extension(Path::new("a.wav")));
    assert!(is_supported_audio_extension(Path::new("a.MP3")));
    assert!(is_supported_audio_extension(Path::new("a.Flac")));
    assert!(is_supported_audio_extension(Path::new("a.ogg")));
    assert!(is_supported_audio_extension(Path::new("a.m4a")));
}

#[test]
fn unsupported_and_extensionless_paths_are_rejected() {
    assert!(!is_supported_audio_extension(Path::new("a.txt")));
    assert!(!is_supported_audio_extension(Path::new("a.exe")));
    assert!(!is_supported_audio_extension(Path::new("noextension")));
    assert!(!is_supported_audio_extension(Path::new("")));
}

#[test]
fn validate_rejects_unsupported_format_before_touching_the_filesystem() {
    // The file does not exist, but the format check must fire first.
    let reason = validate_audio_path(Path::new("/nonexistent/file.txt")).unwrap_err();

    assert_eq!(reason, PlaySoundReason::UnsupportedFormat);
}

#[test]
fn validate_rejects_a_missing_file() {
    let reason = validate_audio_path(Path::new("/nonexistent/orca/ding.wav")).unwrap_err();

    assert_eq!(reason, PlaySoundReason::NotFound);
}

#[test]
fn validate_rejects_a_directory_masquerading_as_audio() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let fake = dir.path().join("bundle.wav");
    std::fs::create_dir(&fake).expect("create dir");

    assert_eq!(
        validate_audio_path(&fake).unwrap_err(),
        PlaySoundReason::NotFound
    );
}

#[test]
fn validate_rejects_files_over_the_size_bound() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let path = dir.path().join("huge.wav");
    let file = std::fs::File::create(&path).expect("create file");
    file.set_len(MAX_AUDIO_FILE_BYTES + 1).expect("grow file");

    assert_eq!(
        validate_audio_path(&path).unwrap_err(),
        PlaySoundReason::FileTooLarge
    );
}

#[test]
fn validate_accepts_a_file_at_the_size_bound_and_returns_a_canonical_path() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let path = dir.path().join("ok.wav");
    let file = std::fs::File::create(&path).expect("create file");
    file.set_len(MAX_AUDIO_FILE_BYTES).expect("grow file");

    let canonical = validate_audio_path(&path).expect("accepted");

    assert!(canonical.is_absolute());
    assert_eq!(canonical, path.canonicalize().unwrap());
}

#[test]
fn validate_resolves_indirect_paths_to_a_canonical_location() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let path = dir.path().join("ding.wav");
    std::fs::write(&path, b"placeholder").expect("write file");

    // A path containing traversal segments resolves to the real location.
    let indirect = dir.path().join("sub").join("..").join("ding.wav");
    std::fs::create_dir_all(dir.path().join("sub")).expect("create sub");

    let canonical = validate_audio_path(&indirect).expect("accepted");

    assert_eq!(canonical, path.canonicalize().unwrap());
}

#[test]
fn display_name_uses_the_file_name() {
    assert_eq!(
        audio_display_name(Path::new("/Users/x/Music/alert tone.mp3")),
        "alert tone.mp3"
    );
}

// ---------------------------------------------------------------------------
// System settings opener
// ---------------------------------------------------------------------------

#[test]
fn open_system_settings_result_serializes_for_the_frontend() {
    let value = serde_json::to_value(OpenSystemSettingsResult {
        opened: false,
        reason: Some("unsupported".into()),
    })
    .unwrap();

    assert_eq!(value, json!({ "opened": false, "reason": "unsupported" }));
}

#[test]
fn current_platform_is_detected() {
    let platform = NotificationPlatform::current();

    #[cfg(target_os = "macos")]
    assert_eq!(platform, NotificationPlatform::Macos);
    #[cfg(target_os = "linux")]
    assert_eq!(platform, NotificationPlatform::Linux);
    #[cfg(target_os = "windows")]
    assert_eq!(platform, NotificationPlatform::Windows);
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    assert_eq!(platform, NotificationPlatform::Other);
}

#[cfg(target_os = "macos")]
#[test]
fn macos_authorization_request_includes_badge_alert_and_sound() {
    use objc2_user_notifications::UNAuthorizationOptions;

    // Assert the exact option bits actually handed to requestAuthorizationWithOptions, not source
    // text. Badge was omitted, silently stripping dock-badge permission from the one explicit
    // authorization prompt.
    let options = crate::notification::permission::macos::authorization_options();

    assert!(
        options.contains(UNAuthorizationOptions::Badge),
        "authorization options must request Badge"
    );
    assert!(
        options.contains(UNAuthorizationOptions::Alert),
        "authorization options must retain Alert"
    );
    assert!(
        options.contains(UNAuthorizationOptions::Sound),
        "authorization options must retain Sound"
    );
    // No implicit extras: exactly the explicit alert/sound/badge trio.
    assert_eq!(
        options,
        UNAuthorizationOptions::Alert
            | UNAuthorizationOptions::Sound
            | UNAuthorizationOptions::Badge,
    );
}

#[cfg(target_os = "macos")]
#[test]
fn macos_submits_through_user_notifications_not_the_legacy_bridge() {
    // usernoted refuses to serve one process over both APIs: querying UNUserNotificationCenter for
    // permission makes us a "modern" client, so submitting over the legacy NSUserNotification
    // bridge is denied with "You can't mix modern clients with legacy clients". Submission must
    // therefore use UNUserNotificationCenter too.
    let content = NotificationContent {
        title: "Agent finished".into(),
        body: "omo completed a task".into(),
        sound: NotificationSound::System,
    };

    let outcome = crate::notification::permission::macos::submit_notification(&content);

    // An unbundled test binary has no bundle proxy, so UNUserNotificationCenter would raise
    // NSInternalInconsistencyException. The submitter must refuse cleanly instead of aborting.
    assert_eq!(
        outcome,
        Err("notifications require a bundled .app".to_string()),
        "unbundled submit must fail cleanly, not raise"
    );
}
