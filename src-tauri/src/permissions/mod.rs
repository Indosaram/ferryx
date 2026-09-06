use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionStatus {
    Granted,
    Denied,
    NotDetermined,
    Unsupported,
    /// The OS manages this externally; Ferryx cannot query it authoritatively.
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionItemStatus {
    pub status: PermissionStatus,
    pub granted: bool,
    pub can_request: bool,
    pub can_open_settings: bool,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemPermissionsStatus {
    pub platform: String,
    pub full_disk_access: PermissionItemStatus,
    pub accessibility: PermissionItemStatus,
    pub notifications: PermissionItemStatus,
    pub all_granted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenPermissionsSettingsResult {
    pub opened: bool,
    pub target: String,
    pub reason: Option<String>,
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

#[cfg(target_os = "macos")]
pub fn check_full_disk_access() -> PermissionStatus {
    let Some(home) = home_dir() else {
        return PermissionStatus::NotDetermined;
    };
    let safari_dir = home.join("Library/Safari");
    if safari_dir.exists() {
        match std::fs::read_dir(&safari_dir) {
            Ok(_) => PermissionStatus::Granted,
            Err(err) if err.raw_os_error() == Some(libc::EPERM) => PermissionStatus::Denied,
            Err(_) => PermissionStatus::NotDetermined,
        }
    } else {
        let suggestions = home.join("Library/Suggestions");
        if suggestions.exists() {
            match std::fs::read_dir(&suggestions) {
                Ok(_) => PermissionStatus::Granted,
                Err(err) if err.raw_os_error() == Some(libc::EPERM) => PermissionStatus::Denied,
                Err(_) => PermissionStatus::NotDetermined,
            }
        } else {
            PermissionStatus::NotDetermined
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn check_full_disk_access() -> PermissionStatus {
    PermissionStatus::Unsupported
}

#[cfg(target_os = "macos")]
pub fn check_accessibility() -> PermissionStatus {
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
    }
    if unsafe { AXIsProcessTrusted() } {
        PermissionStatus::Granted
    } else {
        PermissionStatus::Denied
    }
}

#[cfg(not(target_os = "macos"))]
pub fn check_accessibility() -> PermissionStatus {
    PermissionStatus::Unsupported
}

#[cfg(target_os = "macos")]
pub fn request_accessibility() -> bool {
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrustedWithOptions(options: *const std::ffi::c_void) -> bool;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFBooleanTrue: *const std::ffi::c_void;
        fn CFDictionaryCreate(
            allocator: *const std::ffi::c_void,
            keys: *const *const std::ffi::c_void,
            values: *const *const std::ffi::c_void,
            numValues: isize,
            keyCallBacks: *const std::ffi::c_void,
            valueCallBacks: *const std::ffi::c_void,
        ) -> *const std::ffi::c_void;
        fn CFRelease(cf: *const std::ffi::c_void);
        fn CFStringCreateWithCString(
            allocator: *const std::ffi::c_void,
            cStr: *const std::os::raw::c_char,
            encoding: u32,
        ) -> *const std::ffi::c_void;
        static kCFTypeDictionaryKeyCallBacks: std::ffi::c_void;
        static kCFTypeDictionaryValueCallBacks: std::ffi::c_void;
    }

    let Ok(key_name) = std::ffi::CString::new("AXTrustedCheckOptionPrompt") else {
        return false;
    };
    unsafe {
        let key = CFStringCreateWithCString(std::ptr::null(), key_name.as_ptr(), 0x0800_0100);
        if key.is_null() {
            return false;
        }
        let value = kCFBooleanTrue;
        let keys = [key];
        let values = [value];
        let dict = CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            1,
            &kCFTypeDictionaryKeyCallBacks,
            &kCFTypeDictionaryValueCallBacks,
        );
        let trusted = AXIsProcessTrustedWithOptions(dict);
        if !dict.is_null() {
            CFRelease(dict);
        }
        CFRelease(key);
        trusted
    }
}

#[cfg(not(target_os = "macos"))]
pub fn request_accessibility() -> bool {
    true
}

/// Map a notification permission DTO to a UI-facing permission item.
///
/// Off-macOS the provider is non-authoritative: the OS manages notification
/// access externally, so we report `Unknown` (never a fake `can_request` that
/// would render a dead "Enable Notifications" button) while still exposing an
/// "open settings" affordance where one exists.
fn notification_item(
    platform: &str,
    raw: &crate::notification::NotificationPermissionStatusDto,
) -> PermissionItemStatus {
    const DEFAULT_DESC: &str = "Allows desktop alerts for agent task completions, background builds, and version updates.";

    if raw.authoritative {
        return match raw.authorization {
            crate::notification::NotificationAuthorization::Authorized
            | crate::notification::NotificationAuthorization::Provisional => PermissionItemStatus {
                status: PermissionStatus::Granted,
                granted: true,
                can_request: false,
                can_open_settings: true,
                description: DEFAULT_DESC.to_string(),
            },
            crate::notification::NotificationAuthorization::Denied => PermissionItemStatus {
                status: PermissionStatus::Denied,
                granted: false,
                can_request: false,
                can_open_settings: true,
                description: DEFAULT_DESC.to_string(),
            },
            crate::notification::NotificationAuthorization::NotDetermined
            | crate::notification::NotificationAuthorization::Unknown => PermissionItemStatus {
                status: PermissionStatus::NotDetermined,
                granted: false,
                can_request: true,
                can_open_settings: true,
                description: DEFAULT_DESC.to_string(),
            },
        };
    }

    let description = match platform {
        "windows" => {
            "Windows manages per-app notification access in Settings > System > Notifications."
        }
        "linux" => {
            "Desktop notifications are managed by the desktop environment; most setups need no per-app grant."
        }
        _ => DEFAULT_DESC,
    };

    PermissionItemStatus {
        status: PermissionStatus::Unknown,
        granted: false,
        can_request: false,
        can_open_settings: raw.can_open_settings,
        description: description.to_string(),
    }
}

pub fn get_system_permissions_status() -> SystemPermissionsStatus {
    crate::notification::invalidate_permission_cache();

    let fda_status = check_full_disk_access();
    let ax_status = check_accessibility();

    #[cfg(target_os = "macos")]
    let platform = "macos".to_string();
    #[cfg(target_os = "windows")]
    let platform = "windows".to_string();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let platform = "linux".to_string();

    let notif_raw = crate::notification::platform_permission_provider().status();
    let notifications = notification_item(&platform, &notif_raw);

    let fda_granted = fda_status == PermissionStatus::Granted;
    let ax_granted = ax_status == PermissionStatus::Granted;
    let notif_granted = notifications.granted;

    #[cfg(target_os = "macos")]
    let all_granted = fda_granted && ax_granted && notif_granted;
    #[cfg(not(target_os = "macos"))]
    let all_granted = notif_granted;

    #[cfg(target_os = "macos")]
    let deep_links = true;
    #[cfg(not(target_os = "macos"))]
    let deep_links = false;

    SystemPermissionsStatus {
        platform,
        full_disk_access: PermissionItemStatus {
            status: fda_status,
            granted: fda_granted,
            can_request: false,
            can_open_settings: deep_links,
            description: "Allows terminal subagents, worktrees, and git tools to read project files without macOS Photo Library or folder access prompts.".to_string(),
        },
        accessibility: PermissionItemStatus {
            status: ax_status,
            granted: ax_granted,
            can_request: true,
            can_open_settings: deep_links,
            description: "Allows global keyboard shortcuts, native terminal focus management, and automation.".to_string(),
        },
        notifications,
        all_granted,
    }
}

pub fn open_system_settings_for_target(target: &str) -> OpenPermissionsSettingsResult {
    #[cfg(target_os = "macos")]
    {
        let url = match target {
            "full_disk_access" | "fullDiskAccess" => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles"
            }
            "accessibility" => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
            }
            "notifications" => "x-apple.systempreferences:com.apple.preference.notifications",
            _ => {
                return OpenPermissionsSettingsResult {
                    opened: false,
                    target: target.to_string(),
                    reason: Some(format!("unknown settings target: {target}")),
                };
            }
        };

        let opened = crate::util::no_window_command("open")
            .arg(url)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        OpenPermissionsSettingsResult {
            opened,
            target: target.to_string(),
            reason: if opened {
                None
            } else {
                Some("failed to launch open command".to_string())
            },
        }
    }

    #[cfg(target_os = "windows")]
    {
        let uri = match target {
            "notifications" => Some("ms-settings:notifications"),
            _ => None,
        };
        if let Some(uri) = uri {
            let opened = crate::util::no_window_command("cmd")
                .args(["/C", "start", "", uri])
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            OpenPermissionsSettingsResult {
                opened,
                target: target.to_string(),
                reason: if opened {
                    None
                } else {
                    Some("failed to launch Windows settings".to_string())
                },
            }
        } else {
            OpenPermissionsSettingsResult {
                opened: false,
                target: target.to_string(),
                reason: Some("target unsupported on Windows".to_string()),
            }
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        OpenPermissionsSettingsResult {
            opened: false,
            target: target.to_string(),
            reason: Some("target unsupported on Linux".to_string()),
        }
    }
}

#[cfg(test)]
mod notification_item_tests {
    use super::*;
    use crate::notification::{
        NotificationAuthorization, NotificationPermissionStatusDto, NotificationPlatform,
    };

    fn authoritative(auth: NotificationAuthorization) -> NotificationPermissionStatusDto {
        NotificationPermissionStatusDto {
            platform: NotificationPlatform::Macos,
            supported: true,
            authorization: auth,
            alerts_enabled: None,
            sounds_enabled: None,
            badges_enabled: None,
            requested: auth != NotificationAuthorization::NotDetermined,
            authoritative: true,
            can_open_settings: true,
        }
    }

    #[test]
    fn authoritative_authorized_is_granted() {
        let item = notification_item(
            "macos",
            &authoritative(NotificationAuthorization::Authorized),
        );
        assert_eq!(item.status, PermissionStatus::Granted);
        assert!(item.granted);
        assert!(!item.can_request);
        assert!(item.can_open_settings);
    }

    #[test]
    fn authoritative_provisional_is_granted() {
        let item = notification_item(
            "macos",
            &authoritative(NotificationAuthorization::Provisional),
        );
        assert_eq!(item.status, PermissionStatus::Granted);
        assert!(item.granted);
        assert!(!item.can_request);
        assert!(item.can_open_settings);
    }

    #[test]
    fn authoritative_denied_is_denied() {
        let item =
            notification_item("macos", &authoritative(NotificationAuthorization::Denied));
        assert_eq!(item.status, PermissionStatus::Denied);
        assert!(!item.granted);
        assert!(!item.can_request);
        assert!(item.can_open_settings);
    }

    #[test]
    fn authoritative_not_determined_can_request() {
        let item = notification_item(
            "macos",
            &authoritative(NotificationAuthorization::NotDetermined),
        );
        assert_eq!(item.status, PermissionStatus::NotDetermined);
        assert!(!item.granted);
        assert!(item.can_request);
        assert!(item.can_open_settings);
    }

    #[test]
    fn non_authoritative_windows_is_unknown_with_settings() {
        let raw = NotificationPermissionStatusDto::non_authoritative(
            NotificationPlatform::Windows,
            true,
        );
        let item = notification_item("windows", &raw);
        assert_eq!(item.status, PermissionStatus::Unknown);
        assert!(!item.granted);
        assert!(!item.can_request);
        assert!(item.can_open_settings);
        assert!(item.description.contains("Windows manages"));
    }

    #[test]
    fn non_authoritative_linux_is_unknown_no_settings() {
        let raw = NotificationPermissionStatusDto::non_authoritative(
            NotificationPlatform::Linux,
            false,
        );
        let item = notification_item("linux", &raw);
        assert_eq!(item.status, PermissionStatus::Unknown);
        assert!(!item.granted);
        assert!(!item.can_request);
        assert!(!item.can_open_settings);
        assert!(item.description.contains("desktop environment"));
    }
}
