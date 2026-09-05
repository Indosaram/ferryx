//! Native notification domain.
//!
//! Rust decides *how* to talk to the OS safely; the frontend decides *whether*
//! an application event should notify. React state, active-tab knowledge, and
//! user-event dedupe deliberately stay out of this module.

pub mod activation;
pub mod audio;
pub mod badge;
#[cfg(target_os = "macos")]
pub mod macos_delegate;
pub mod model;
#[cfg(target_os = "macos")]
mod macos_submission;
#[cfg(any(target_os = "windows", target_os = "linux"))]
pub mod notify_rust_adapter;
pub mod permission;
pub mod service;

pub use activation::*;
pub use audio::*;
pub use badge::*;
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
        if run_opener(
            "open",
            &["x-apple.systempreferences:com.apple.preference.notifications"],
        ) {
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
#[allow(dead_code)]
fn run_opener(program: &str, args: &[&str]) -> bool {
    crate::util::no_window_command(program)
        .args(args)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests;
