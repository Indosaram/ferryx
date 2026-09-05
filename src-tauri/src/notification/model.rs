//! Pure notification domain types and content policy.
//!
//! Nothing in this module touches the OS. Content formatting, sanitization,
//! permission normalization, and audio request validation all live here so
//! they can be unit tested without displaying real notifications or opening
//! an audio device.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Maximum rendered length of a notification title.
pub const MAX_TITLE_LEN: usize = 80;
/// Maximum rendered length of a notification body.
pub const MAX_BODY_LEN: usize = 160;
/// Upper bound for a custom notification sound file.
pub const MAX_AUDIO_FILE_BYTES: u64 = 20 * 1024 * 1024;
/// Audio extensions matching the enabled `rodio` decoder feature set.
pub const SUPPORTED_AUDIO_EXTENSIONS: &[&str] =
    &["wav", "mp3", "flac", "ogg", "oga", "m4a", "mp4", "aac"];

/// Frontend pane a notification originated from.
///
/// Both IDs are the frontend's own identifiers (the workspace and the frontend
/// session id), never a backend/leaf id. They travel unchanged from dispatch,
/// through the OS payload, and back on click so the UI can re-select the exact
/// originating pane instead of guessing the most recent one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationTarget {
    pub workspace_id: String,
    pub session_id: String,
}

/// Application event that asked for a notification.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NotificationSource {
    #[serde(alias = "agent-task-complete")]
    AgentTaskComplete,
    #[serde(alias = "terminal-bell")]
    TerminalBell,
    #[default]
    Test,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NotificationSound {
    #[default]
    System,
    Silent,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NotificationAttentionReason {
    Waiting,
    #[default]
    Done,
}

/// Why a dispatch did not result in a submitted notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NotificationDispatchReason {
    PermissionRequired,
    BlockedBySystem,
    Unsupported,
    BackendError,
}

/// Frontend request to raise a native notification.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchNotificationRequest {
    pub source: NotificationSourceField,
    #[serde(default)]
    pub sound: NotificationSound,
    #[serde(default)]
    pub attention_reason: NotificationAttentionReason,
    #[serde(default)]
    pub notification_id: Option<String>,
    #[serde(default)]
    pub workspace_label: Option<String>,
    #[serde(default)]
    pub worktree_label: Option<String>,
    #[serde(default)]
    pub terminal_title: Option<String>,
    #[serde(default)]
    pub agent_label: Option<String>,
    /// Pane to re-select when the user clicks the delivered notification.
    ///
    /// `None` for test and id-less notifications, which must not route focus.
    #[serde(default)]
    pub target: Option<NotificationTarget>,
}

/// `source` defaults to `test` so a malformed payload cannot silently
/// impersonate an agent-completion notification.
pub type NotificationSourceField = NotificationSource;

/// Outcome of a dispatch attempt.
///
/// `submitted` means the request passed preflight and was handed to the
/// native backend. It never claims the user actually saw a banner: Focus
/// modes and notification-center policy are outside rorca's visibility.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchNotificationResult {
    pub submitted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<NotificationDispatchReason>,
}

impl DispatchNotificationResult {
    pub fn submitted() -> Self {
        Self {
            submitted: true,
            reason: None,
        }
    }

    pub fn rejected(reason: NotificationDispatchReason) -> Self {
        Self {
            submitted: false,
            reason: Some(reason),
        }
    }
}

/// Rendered, sanitized notification text ready for the native backend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationContent {
    pub title: String,
    pub body: String,
    pub sound: NotificationSound,
    /// Carried verbatim to the native payload; echoed back on click.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<NotificationTarget>,
}

/// Host platform, reported so the UI can explain platform differences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NotificationPlatform {
    Macos,
    Windows,
    Linux,
    Other,
}

impl NotificationPlatform {
    pub fn current() -> Self {
        #[cfg(target_os = "macos")]
        {
            Self::Macos
        }
        #[cfg(target_os = "windows")]
        {
            Self::Windows
        }
        #[cfg(target_os = "linux")]
        {
            Self::Linux
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            Self::Other
        }
    }
}

/// Normalized authorization state across platforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NotificationAuthorization {
    NotDetermined,
    Authorized,
    Denied,
    Provisional,
    Unknown,
}

/// Cross-platform permission status.
///
/// `authoritative` distinguishes a real OS query (macOS `UNNotificationSettings`)
/// from a best-effort assumption. Tauri's desktop `permission_state()` always
/// returns `Granted`, so it is never used as the source of truth here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationPermissionStatusDto {
    pub platform: NotificationPlatform,
    pub supported: bool,
    pub authorization: NotificationAuthorization,
    pub alerts_enabled: Option<bool>,
    pub sounds_enabled: Option<bool>,
    pub requested: bool,
    pub authoritative: bool,
    pub can_open_settings: bool,
}

impl NotificationPermissionStatusDto {
    /// Non-authoritative status for platforms without a real permission query.
    pub fn non_authoritative(platform: NotificationPlatform, can_open_settings: bool) -> Self {
        Self {
            platform,
            supported: true,
            authorization: NotificationAuthorization::Unknown,
            alerts_enabled: None,
            sounds_enabled: None,
            requested: false,
            authoritative: false,
            can_open_settings,
        }
    }

    pub fn unsupported(platform: NotificationPlatform) -> Self {
        Self {
            platform,
            supported: false,
            authorization: NotificationAuthorization::Unknown,
            alerts_enabled: None,
            sounds_enabled: None,
            requested: false,
            authoritative: false,
            can_open_settings: false,
        }
    }

    /// True when an authoritative query proves the OS will drop notifications.
    pub fn is_blocked(&self) -> bool {
        self.authoritative && self.authorization == NotificationAuthorization::Denied
    }

    /// True when macOS has never been asked and no implicit prompt may occur.
    pub fn needs_permission_request(&self) -> bool {
        self.authoritative && self.authorization == NotificationAuthorization::NotDetermined
    }
}

/// Result of an explicit permission request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationPermissionRequestDto {
    /// Whether the platform reported the request itself as granted.
    pub granted: bool,
    /// Status re-queried after the request resolved.
    pub status: NotificationPermissionStatusDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Probe outcome describing notification readiness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NotificationProbeOutcome {
    Submitted,
    Ready,
    PermissionRequired,
    BlockedBySystem,
    Unsupported,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationProbeResult {
    pub outcome: NotificationProbeOutcome,
    pub status: NotificationPermissionStatusDto,
    /// True only when a visible test notification was actually submitted.
    pub test_submitted: bool,
}

/// Why a custom sound did not play.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlaySoundReason {
    NotFound,
    UnsupportedFormat,
    FileTooLarge,
    DecodeFailed,
    NoOutputDevice,
    Deduped,
    BackendError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaySoundResult {
    pub played: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<PlaySoundReason>,
}

impl PlaySoundResult {
    pub fn played() -> Self {
        Self {
            played: true,
            reason: None,
        }
    }

    pub fn failed(reason: PlaySoundReason) -> Self {
        Self {
            played: false,
            reason: Some(reason),
        }
    }
}

/// Audio file chosen through the native picker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PickedAudioFile {
    pub path: PathBuf,
    pub display_name: String,
}

/// Result of asking the OS to open its notification settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenSystemSettingsResult {
    pub opened: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

// ---------------------------------------------------------------------------
// Content policy
// ---------------------------------------------------------------------------

/// Strip control characters and collapse pathological whitespace.
///
/// Terminal titles and agent labels are attacker-adjacent input: they can
/// carry escape sequences, newlines, or padding designed to spoof notification
/// text. Everything below `0x20` plus DEL and the Unicode line separators is
/// replaced by a space, then runs of whitespace collapse to one space.
pub fn sanitize_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut pending_space = false;

    for ch in raw.chars() {
        let is_blank = ch.is_control()
            || ch.is_whitespace()
            || matches!(ch, '\u{2028}' | '\u{2029}' | '\u{feff}');

        if is_blank {
            pending_space = !out.is_empty();
            continue;
        }

        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        out.push(ch);
    }

    out
}

/// Truncate on a character boundary, appending an ellipsis when shortened.
pub fn truncate_text(text: &str, max_len: usize) -> String {
    if text.chars().count() <= max_len {
        return text.to_string();
    }
    if max_len == 0 {
        return String::new();
    }

    // Reserve one character for the ellipsis.
    let keep = max_len.saturating_sub(1);
    let mut out: String = text.chars().take(keep).collect();
    while out.ends_with(' ') {
        out.pop();
    }
    out.push('\u{2026}');
    out
}

fn clean_field(value: Option<&str>) -> Option<String> {
    let cleaned = sanitize_text(value?);
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

/// Render final notification text for a dispatch request.
///
/// Deliberately conservative: no shell output, no agent prompt text, and no
/// absolute paths. Native notifications can appear on a lock screen, so only
/// short display labels the user already chose are included.
pub fn format_notification(request: &DispatchNotificationRequest) -> NotificationContent {
    let agent = clean_field(request.agent_label.as_deref());
    let worktree = clean_field(request.worktree_label.as_deref());
    let workspace = clean_field(request.workspace_label.as_deref());
    let terminal = clean_field(request.terminal_title.as_deref());

    let (title, body) = match request.source {
        NotificationSource::AgentTaskComplete => {
            let (action, fallback) = match request.attention_reason {
                NotificationAttentionReason::Waiting => ("needs input", "Input or approval needed"),
                NotificationAttentionReason::Done => ("finished", "Task complete"),
            };
            let title = format!("{} {action}", agent.as_deref().unwrap_or("Agent"));
            let body = worktree
                .or(workspace)
                .unwrap_or_else(|| fallback.to_string());
            (title, body)
        }
        NotificationSource::TerminalBell => {
            let title = "Terminal needs attention".to_string();
            let location = worktree.or(workspace);
            let body = match (location, terminal) {
                (Some(location), Some(terminal)) => format!("{location} \u{b7} {terminal}"),
                (Some(location), None) => location,
                (None, Some(terminal)) => terminal,
                (None, None) => "A terminal rang the bell".to_string(),
            };
            (title, body)
        }
        NotificationSource::Test => (
            "Ferryx notifications are working".to_string(),
            "Test notification".to_string(),
        ),
    };

    NotificationContent {
        title: truncate_text(&title, MAX_TITLE_LEN),
        body: truncate_text(&body, MAX_BODY_LEN),
        sound: request.sound,
        target: request.target.clone(),
    }
}

// ---------------------------------------------------------------------------
// Audio validation
// ---------------------------------------------------------------------------

/// Convert a frontend volume percentage to a linear gain.
///
/// Accepts `0..=100`; anything outside (including NaN) clamps into range so a
/// malformed setting can never produce a deafening or negative gain.
pub fn normalize_volume(percent: f32) -> f32 {
    if percent.is_nan() {
        return 0.0;
    }
    (percent / 100.0).clamp(0.0, 1.0)
}

/// True when the extension matches an enabled decoder.
pub fn is_supported_audio_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            let lower = ext.to_ascii_lowercase();
            SUPPORTED_AUDIO_EXTENSIONS.contains(&lower.as_str())
        })
        .unwrap_or(false)
}

/// Validate a custom sound path before any decoding is attempted.
///
/// Returns the canonicalized path so playback never follows a path that
/// changed between validation and open.
pub fn validate_audio_path(path: &Path) -> Result<PathBuf, PlaySoundReason> {
    if !is_supported_audio_extension(path) {
        return Err(PlaySoundReason::UnsupportedFormat);
    }

    let canonical = path.canonicalize().map_err(|_| PlaySoundReason::NotFound)?;

    let metadata = canonical
        .metadata()
        .map_err(|_| PlaySoundReason::NotFound)?;

    if !metadata.is_file() {
        return Err(PlaySoundReason::NotFound);
    }
    if metadata.len() > MAX_AUDIO_FILE_BYTES {
        return Err(PlaySoundReason::FileTooLarge);
    }

    Ok(canonical)
}

/// Display name for a picked file, falling back to the full path string.
pub fn audio_display_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string())
}

#[cfg(test)]
mod target_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn notification_target_uses_camel_case_frontend_ids() {
        let target = NotificationTarget {
            workspace_id: "ws-1".into(),
            session_id: "fe-session-7".into(),
        };
        let wire = serde_json::to_value(&target).unwrap();
        assert_eq!(wire, json!({ "workspaceId": "ws-1", "sessionId": "fe-session-7" }));

        let decoded: NotificationTarget = serde_json::from_value(wire).unwrap();
        assert_eq!(decoded, target);
    }

    #[test]
    fn dispatch_request_target_defaults_to_none() {
        // A payload without `target` (test / id-less notifications) must not
        // carry a routing destination.
        let request: DispatchNotificationRequest = serde_json::from_value(json!({
            "source": "test"
        }))
        .unwrap();
        assert_eq!(request.target, None);
    }

    #[test]
    fn dispatch_request_parses_camel_case_target() {
        let request: DispatchNotificationRequest = serde_json::from_value(json!({
            "source": "agentTaskComplete",
            "target": { "workspaceId": "ws-9", "sessionId": "fe-42" }
        }))
        .unwrap();
        assert_eq!(
            request.target,
            Some(NotificationTarget {
                workspace_id: "ws-9".into(),
                session_id: "fe-42".into(),
            })
        );
    }

    #[test]
    fn format_notification_carries_target_through() {
        let request = DispatchNotificationRequest {
            source: NotificationSource::AgentTaskComplete,
            agent_label: Some("Claude".into()),
            target: Some(NotificationTarget {
                workspace_id: "ws-3".into(),
                session_id: "fe-99".into(),
            }),
            ..Default::default()
        };
        let content = format_notification(&request);
        assert_eq!(content.target, request.target);
    }

    #[test]
    fn format_notification_test_source_has_no_target() {
        let request = DispatchNotificationRequest {
            source: NotificationSource::Test,
            ..Default::default()
        };
        assert_eq!(format_notification(&request).target, None);
    }
}
