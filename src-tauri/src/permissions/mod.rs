use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionStatus {
    Granted,
    Denied,
    NotDetermined,
    Unsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionItemStatus {
    pub status: PermissionStatus,
    pub granted: bool,
    pub can_request: bool,
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

pub fn get_system_permissions_status() -> SystemPermissionsStatus {
    crate::notification::invalidate_permission_cache();

    let fda_status = check_full_disk_access();
    let ax_status = check_accessibility();

    let notif_raw = crate::notification::platform_permission_provider().status();
    let (notif_status, notif_granted, notif_can_request) = match notif_raw.authorization {
        crate::notification::NotificationAuthorization::Authorized
        | crate::notification::NotificationAuthorization::Provisional => {
            (PermissionStatus::Granted, true, false)
        }
        crate::notification::NotificationAuthorization::Denied => {
            (PermissionStatus::Denied, false, false)
        }
        crate::notification::NotificationAuthorization::NotDetermined => {
            (PermissionStatus::NotDetermined, false, true)
        }
        crate::notification::NotificationAuthorization::Unknown => {
            (PermissionStatus::NotDetermined, false, true)
        }
    };

    let fda_granted = fda_status == PermissionStatus::Granted;
    let ax_granted = ax_status == PermissionStatus::Granted;

    #[cfg(target_os = "macos")]
    let all_granted = fda_granted && ax_granted && notif_granted;
    #[cfg(not(target_os = "macos"))]
    let all_granted = notif_granted;

    #[cfg(target_os = "macos")]
    let platform = "macos".to_string();
    #[cfg(target_os = "windows")]
    let platform = "windows".to_string();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let platform = "linux".to_string();

    SystemPermissionsStatus {
        platform,
        full_disk_access: PermissionItemStatus {
            status: fda_status,
            granted: fda_granted,
            can_request: false,
            description: "Allows terminal subagents, worktrees, and git tools to read project files without macOS Photo Library or folder access prompts.".to_string(),
        },
        accessibility: PermissionItemStatus {
            status: ax_status,
            granted: ax_granted,
            can_request: true,
            description: "Allows global keyboard shortcuts, native terminal focus management, and automation.".to_string(),
        },
        notifications: PermissionItemStatus {
            status: notif_status,
            granted: notif_granted,
            can_request: notif_can_request,
            description: "Allows desktop alerts for agent task completions, background builds, and version updates.".to_string(),
        },
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
