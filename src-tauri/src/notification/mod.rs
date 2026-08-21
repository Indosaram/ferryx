//! Native notification domain.
//!
//! Rust decides *how* to talk to the OS safely; the frontend decides *whether*
//! an application event should notify. React state, active-tab knowledge, and
//! user-event dedupe deliberately stay out of this module.

pub mod audio;
pub mod model;
pub mod permission;
pub mod service;

pub use audio::*;
pub use model::*;
pub use permission::*;
pub use service::*;

/// Open the OS notification settings for rorca.
///
/// Deliberately narrow: the target is fixed per platform and never accepted
/// from the frontend, so this cannot become a general URI-open primitive.
pub fn open_system_notification_settings() -> OpenSystemSettingsResult {
    #[cfg(target_os = "macos")]
    {
        const NOTIFICATION_PANE: &str =
            "x-apple.systempreferences:com.apple.preference.notifications";

        if run_opener("open", &[NOTIFICATION_PANE]) {
            return OpenSystemSettingsResult {
                opened: true,
                reason: None,
            };
        }
        // Fall back to the System Settings app if the pane URI is rejected.
        if run_opener("open", &["-b", "com.apple.systempreferences"]) {
            return OpenSystemSettingsResult {
                opened: true,
                reason: None,
            };
        }
        OpenSystemSettingsResult {
            opened: false,
            reason: Some("could not open System Settings".into()),
        }
    }

    #[cfg(target_os = "windows")]
    {
        if run_opener("cmd", &["/C", "start", "", "ms-settings:notifications"]) {
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
        OpenSystemSettingsResult {
            opened: false,
            reason: Some("unsupported".into()),
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn run_opener(program: &str, args: &[&str]) -> bool {
    std::process::Command::new(program)
        .args(args)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests;
