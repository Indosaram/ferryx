//! Native notification domain.
//!
//! Rust decides *how* to talk to the OS safely; the frontend decides *whether*
//! an application event should notify. React state, active-tab knowledge, and
//! user-event dedupe deliberately stay out of this module.

pub mod activation;
#[cfg(feature = "desktop")]
pub mod audio;
pub mod badge;
#[cfg(target_os = "macos")]
pub mod macos_delegate;
#[cfg(target_os = "macos")]
mod macos_submission;
pub mod model;
#[cfg(any(target_os = "windows", target_os = "linux"))]
pub mod notify_rust_adapter;
pub mod permission;
pub mod service;

pub use activation::*;
#[cfg(feature = "desktop")]
pub use audio::*;
pub use badge::*;
pub use model::*;
pub use permission::*;
pub use service::*;

/// Open the OS notification settings for rorca.
///
/// Deliberately narrow: the target is fixed per platform and never accepted
/// from the frontend, so this cannot become a general URI-open primitive.
/// On macOS 13 and later the target is the app's own Notifications pane entry,
/// keyed by the bundle identifier this process is actually running under; the
/// identifier is read from `NSBundle`, never from IPC.
pub fn open_system_notification_settings() -> OpenSystemSettingsResult {
    open_system_notification_settings_with_launcher(|program, args| {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            run_opener(program, args)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = (program, args);
            false
        }
    })
}

/// Notifications pane target used by macOS 12 and earlier, where the pane is
/// not addressable per application.
pub const MACOS_LEGACY_NOTIFICATION_SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.preference.notifications";

/// Notification settings deep link for a given macOS version and bundle identity.
///
/// macOS 13 (Ventura) moved notification settings into System Settings, where
/// each app's pane is addressable by bundle identifier. Earlier releases only
/// expose the shared legacy pane, so they keep the fallback target.
pub fn macos_notification_settings_url(major_version: isize, bundle_id: Option<&str>) -> String {
    if major_version >= 13 {
        if let Some(id) = bundle_id.map(str::trim).filter(|id| !id.is_empty()) {
            return format!(
                "x-apple.systempreferences:com.apple.Notifications-Settings.extension?id={id}"
            );
        }
    }
    MACOS_LEGACY_NOTIFICATION_SETTINGS_URL.to_string()
}

pub fn open_system_notification_settings_with_launcher(
    launch: impl FnOnce(&str, &[&str]) -> bool,
) -> OpenSystemSettingsResult {
    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::{NSBundle, NSProcessInfo};

        let major_version = NSProcessInfo::processInfo()
            .operatingSystemVersion()
            .majorVersion;
        // An unbundled binary (dev run, `cargo test`) has no bundle identity and
        // must not claim an app-specific pane it cannot own.
        let bundle_id = permission::macos::has_bundle_identity()
            .then(|| {
                NSBundle::mainBundle()
                    .bundleIdentifier()
                    .map(|id| id.to_string())
            })
            .flatten();

        open_macos_notification_settings_for(major_version, bundle_id.as_deref(), launch)
    }

    #[cfg(target_os = "windows")]
    {
        if launch("cmd", &["/C", "start", "", "ms-settings:notifications"]) {
            return OpenSystemSettingsResult {
                opened: true,
                reason: None,
            };
        }
        OpenSystemSettingsResult {
            opened: false,
            reason: Some("could not open Windows notification settings".into()),
        }
    }

    // Linux desktop environments have no single notification settings target.
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = launch;
        OpenSystemSettingsResult {
            opened: false,
            reason: Some("unsupported".into()),
        }
    }
}

/// Launch the macOS notification settings pane for an already-resolved
/// version/bundle pair.
///
/// Kept separate from [`open_system_notification_settings_with_launcher`] so the
/// version gate and the exact `open` invocation can be exercised by a recording
/// launcher instead of the real `open` binary.
pub fn open_macos_notification_settings_for(
    major_version: isize,
    bundle_id: Option<&str>,
    launch: impl FnOnce(&str, &[&str]) -> bool,
) -> OpenSystemSettingsResult {
    let url = macos_notification_settings_url(major_version, bundle_id);
    if launch("open", &[url.as_str()]) {
        return OpenSystemSettingsResult {
            opened: true,
            reason: None,
        };
    }
    OpenSystemSettingsResult {
        opened: false,
        reason: Some("could not open macOS notification settings".into()),
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[allow(dead_code)]
pub fn run_opener(program: &str, args: &[&str]) -> bool {
    crate::util::no_window_command(program)
        .args(args)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests;
