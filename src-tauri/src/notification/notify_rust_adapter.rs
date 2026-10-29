//! Windows/Linux native notification click routing via `notify-rust`.
//!
//! `tauri-plugin-notification`'s desktop path shows a toast and discards the
//! `NotificationHandle`, so the user's click never reaches us. `notify-rust`
//! 4.18.0 exposes `wait_for_response`, which resolves when the user activates
//! (or closes) the toast. We register a default action, show the notification,
//! then wait on a dedicated OS thread; a default activation enqueues the
//! originating pane target through the same [`route_activation`] seam the macOS
//! delegate uses.
//!
//! The waiter runs on its own `std::thread`, never a Tokio worker, so blocking
//! on the D-Bus/WinRT response cannot starve the async runtime. The thread's
//! lifetime is bounded by the notification: `wait_for_response` returns on
//! activation, dismissal, or timeout/close.

use super::activation::{route_activation, ActivationAction, NotificationActivations};
use super::model::{
    NotificationContent, NotificationPlatform, NotificationSound, NotificationTarget,
};
use notify_rust::{Notification, NotificationResponse};
use std::sync::Arc;

/// Canonical default-action identifier registered on every routed notification.
const DEFAULT_ACTION_ID: &str = "default";

/// Sound name that asks Windows for the platform default alert.
///
/// `tauri-winrt-notification` parses it into `Sound::Default`, which leaves the
/// toast without an `<audio>` element and so plays the system alert. An unset
/// name renders `<audio silent="true"/>` instead, so the default choice has to
/// name it explicitly.
pub(crate) const DEFAULT_SOUND_NAME: &str = "Default";

/// Alert fields a platform backend needs to honour the user's sound setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SoundPolicy {
    /// Sound name to request; `None` leaves the platform default in place.
    sound_name: Option<&'static str>,
    /// XDG `suppress-sound` hint.
    suppress: bool,
}

/// Map the user's sound setting onto the Windows/Linux alert fields.
///
/// Windows renders an unset sound name as a *silent* toast, so the default
/// `System` choice has to name the platform default explicitly; `Silent` stays
/// unset, which is already silent there. Linux alerts by default and expresses
/// `Silent` through the XDG `suppress-sound` hint, and is never sent a sound
/// name because `"Default"` is not a valid freedesktop sound name.
fn sound_policy(sound: NotificationSound, platform: NotificationPlatform) -> SoundPolicy {
    match (sound, platform) {
        (NotificationSound::System, NotificationPlatform::Windows) => SoundPolicy {
            sound_name: Some(DEFAULT_SOUND_NAME),
            suppress: false,
        },
        (NotificationSound::Silent, NotificationPlatform::Linux) => SoundPolicy {
            sound_name: None,
            suppress: true,
        },
        // `Silent` on Windows rides the unset name and `System` on Linux the
        // server default; macOS applies its own policy in `macos_submission`.
        _ => SoundPolicy {
            sound_name: None,
            suppress: false,
        },
    }
}

/// Does `dir` look like a cargo dev output directory (`target/debug` or
/// `target/release`)?
///
/// Only dev runs skip the AppUserModelID: a `cargo run` binary has no
/// Start-Menu shortcut, so applying an unregistered identifier there would
/// only lose the toast.
#[cfg(any(target_os = "windows", test))]
fn is_dev_output_dir(dir: &std::path::Path) -> bool {
    dir.ends_with("target/debug") || dir.ends_with("target/release")
}

/// `<Application Id>` in `src-tauri/windows/msix/AppxManifest.xml`; a Store
/// install's AUMID is `<PackageFamilyName>!<Application Id>`.
#[cfg(any(target_os = "windows", test))]
const MSIX_APPLICATION_ID: &str = "Ferryx";

/// Which AppUserModelID a Windows toast must carry for this install.
#[cfg(any(target_os = "windows", test))]
#[derive(Debug, PartialEq, Eq)]
enum ToastIdentity {
    /// MSIX (Store): the package AUMID; the config identifier is unknown to Windows there.
    Packaged(String),
    /// NSIS/MSI: Tauri's installer templates stamp the config identifier on the
    /// Start-Menu shortcut as `System.AppUserModel.ID`.
    Installed(String),
    /// `cargo run`: no shortcut exists, so no identifier is applied.
    Dev,
}

#[cfg(any(target_os = "windows", test))]
fn select_toast_identity(
    package_family_name: Option<&str>,
    exe_dir: Option<&std::path::Path>,
    identifier: &str,
) -> ToastIdentity {
    if let Some(family) = package_family_name.filter(|family| !family.is_empty()) {
        return ToastIdentity::Packaged(format!("{family}!{MSIX_APPLICATION_ID}"));
    }
    if exe_dir.is_some_and(is_dev_output_dir) {
        return ToastIdentity::Dev;
    }
    ToastIdentity::Installed(identifier.to_string())
}

/// The package family name when this process runs from an MSIX package.
#[cfg(target_os = "windows")]
fn current_package_family_name() -> Option<String> {
    use windows_sys::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS};
    use windows_sys::Win32::Storage::Packaging::Appx::GetCurrentPackageFamilyName;
    static FAMILY: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    FAMILY
        .get_or_init(|| {
            let mut length: u32 = 0;
            // SAFETY:
            // Category: Foreign Function Interface (FFI).
            // Invariant: a null buffer with length 0 is the documented size query; the call
            // only writes the required length into `length`. An unpackaged process gets
            // APPMODEL_ERROR_NO_PACKAGE instead.
            let code = unsafe { GetCurrentPackageFamilyName(&mut length, std::ptr::null_mut()) };
            if code != ERROR_INSUFFICIENT_BUFFER || length == 0 {
                return None;
            }
            let mut buffer = vec![0u16; length as usize];
            // SAFETY:
            // Category: Foreign Function Interface (FFI).
            // Invariant: `buffer` is writable for `length` UTF-16 units, the size the
            // previous call reported, and outlives the call.
            let code = unsafe { GetCurrentPackageFamilyName(&mut length, buffer.as_mut_ptr()) };
            if code != ERROR_SUCCESS {
                return None;
            }
            let end = buffer.iter().position(|&unit| unit == 0).unwrap_or(buffer.len());
            String::from_utf16(&buffer[..end]).ok()
        })
        .clone()
}

/// Show a notification and, when it carries a routing target, wait for the
/// user's click on a dedicated thread and route it.
///
/// The user's sound setting is applied before showing, since `notify-rust`
/// leaves the alert entirely to the platform default otherwise.
///
/// `identifier` is the Tauri config identifier, ignored on Linux. On Windows
/// the toast's AppUserModelID is chosen per install: the package AUMID for an
/// MSIX install, the identifier for NSIS/MSI (whose Start-Menu shortcut
/// carries it), and none for a dev run.
pub fn submit_with_click_routing(
    content: &NotificationContent,
    identifier: &str,
    activations: Arc<NotificationActivations>,
) -> Result<(), String> {
    let mut builder = Notification::new();
    builder.summary(&content.title).body(&content.body);
    #[cfg(target_os = "windows")]
    {
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        let family = current_package_family_name();
        match select_toast_identity(family.as_deref(), exe.parent(), identifier) {
            ToastIdentity::Packaged(app_id) => {
                builder.app_id(&app_id);
            }
            ToastIdentity::Installed(app_id) => {
                // Delivery depends on the installer's shortcut still existing; Windows drops a
                // toast for an AUMID no shortcut registers without reporting an error.
                static WARNED: std::sync::Once = std::sync::Once::new();
                WARNED.call_once(|| {
                    tracing::warn!(
                        app_id = %app_id,
                        "Windows toasts need the Start-Menu shortcut the installer registers with this AppUserModelID; without it they are dropped"
                    );
                });
                builder.app_id(&app_id);
            }
            ToastIdentity::Dev => {}
        }
    }
    #[cfg(not(target_os = "windows"))]
    let _ = identifier;
    #[cfg(target_os = "linux")]
    builder.appname("Ferryx");

    // The user's sound setting reaches neither notify-rust nor the tauri plugin
    // on its own (macOS applies it in `macos_submission`), so apply it here.
    let policy = sound_policy(content.sound, NotificationPlatform::current());
    if let Some(name) = policy.sound_name {
        #[cfg(target_os = "windows")]
        builder.sound_name(name);
        // Linux never carries a name: `"Default"` is not a freedesktop sound.
        #[cfg(not(target_os = "windows"))]
        let _ = name;
    }
    if policy.suppress {
        // Windows expresses `Silent` by leaving the sound name unset instead.
        #[cfg(target_os = "linux")]
        builder.hint(notify_rust::Hint::SuppressSound(true));
    }

    // Only notifications with a real destination need a click handler; test /
    // id-less notifications show without one and route nothing.
    let target = content.target.clone();
    if target.is_some() {
        // Registering the default action makes a body click resolve as
        // `NotificationResponse::Default` on both XDG and Windows.
        builder.action(DEFAULT_ACTION_ID, "Open");
    }

    let handle = builder.show().map_err(|error| error.to_string())?;

    let Some(target) = target else {
        return Ok(());
    };

    std::thread::Builder::new()
        .name("ferryx-notify-click".into())
        .spawn(move || {
            if let Err(error) = handle.wait_for_response(move |response: &NotificationResponse| {
                route_response(response, target, activations.as_ref());
            }) {
                tracing::error!("notification click response failed: {error}");
            }
        })
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// Apply the shared default-vs-dismiss policy to a `notify-rust` response.
///
/// The target is already typed here (it was carried in-process, not through the
/// OS payload), so this decides only whether the response is a default
/// activation and, if so, enqueues. Returns `true` iff a target was enqueued.
fn route_response(
    response: &NotificationResponse,
    target: NotificationTarget,
    activations: &NotificationActivations,
) -> bool {
    let action = match response {
        NotificationResponse::Default => ActivationAction::Default,
        NotificationResponse::Action(id) if id == DEFAULT_ACTION_ID => ActivationAction::Default,
        // Named non-default actions, inline replies, and closes never route.
        _ => ActivationAction::Dismiss,
    };
    let json = serde_json::json!({
        "workspaceId": target.workspace_id, "sessionId": target.session_id,
    })
    .to_string();
    route_activation(activations, action, Some(&json))
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify_rust::CloseReason;

    fn target() -> NotificationTarget {
        NotificationTarget {
            workspace_id: "ws".into(),
            session_id: "fe".into(),
        }
    }

    #[test]
    fn default_response_routes_the_target() {
        let queue = NotificationActivations::new();
        assert!(route_response(
            &NotificationResponse::Default,
            target(),
            &queue
        ));
        assert_eq!(queue.drain().len(), 1);
    }

    #[test]
    fn default_named_action_routes_the_target() {
        let queue = NotificationActivations::new();
        assert!(route_response(
            &NotificationResponse::Action(DEFAULT_ACTION_ID.into()),
            target(),
            &queue
        ));
        assert_eq!(queue.drain().len(), 1);
    }

    #[test]
    fn close_and_other_actions_do_not_route() {
        let queue = NotificationActivations::new();
        assert!(!route_response(
            &NotificationResponse::Closed(CloseReason::Dismissed),
            target(),
            &queue
        ));
        assert!(!route_response(
            &NotificationResponse::Action("some-other".into()),
            target(),
            &queue
        ));
        assert_eq!(queue.len(), 0);
    }

    #[test]
    fn system_sound_names_the_windows_default_only() {
        let windows = sound_policy(NotificationSound::System, NotificationPlatform::Windows);
        assert_eq!(windows.sound_name, Some(DEFAULT_SOUND_NAME));
        assert!(!windows.suppress);

        // Linux alerts by default and rejects `"Default"` as a sound name.
        let linux = sound_policy(NotificationSound::System, NotificationPlatform::Linux);
        assert_eq!(linux.sound_name, None);
        assert!(!linux.suppress);
    }

    #[test]
    fn silent_sound_suppresses_on_linux_only() {
        let linux = sound_policy(NotificationSound::Silent, NotificationPlatform::Linux);
        assert!(linux.suppress);
        assert_eq!(linux.sound_name, None);

        // Windows renders an unset sound name as a silent toast already.
        let windows = sound_policy(NotificationSound::Silent, NotificationPlatform::Windows);
        assert_eq!(windows.sound_name, None);
        assert!(!windows.suppress);
    }

    #[test]
    fn toast_identity_selects_packaged_installed_or_dev() {
        let installed_dir = std::path::Path::new(r"C:\Users\dev\AppData\Local\Ferryx");
        let dev_dir = std::path::Path::new("/repo/src-tauri/target/debug");
        assert_eq!(
            select_toast_identity(Some("ProjectMaho.Ferryx_abc123"), Some(installed_dir), "com.ferryx.app"),
            ToastIdentity::Packaged("ProjectMaho.Ferryx_abc123!Ferryx".into())
        );
        assert_eq!(
            select_toast_identity(None, Some(installed_dir), "com.ferryx.app"),
            ToastIdentity::Installed("com.ferryx.app".into())
        );
        assert_eq!(
            select_toast_identity(Some(""), Some(installed_dir), "com.ferryx.app"),
            ToastIdentity::Installed("com.ferryx.app".into())
        );
        assert_eq!(select_toast_identity(None, Some(dev_dir), "com.ferryx.app"), ToastIdentity::Dev);
    }

    #[test]
    fn packaged_application_id_matches_the_msix_manifest() {
        let manifest = include_str!("../../windows/msix/AppxManifest.xml");
        assert!(manifest.contains(&format!("<Application Id=\"{MSIX_APPLICATION_ID}\"")));
    }

    #[test]
    fn dev_output_detection_classifies_target_and_installed_dirs() {
        assert!(is_dev_output_dir(std::path::Path::new(
            "/repo/src-tauri/target/debug"
        )));
        assert!(is_dev_output_dir(std::path::Path::new(
            "/repo/src-tauri/target/release"
        )));
        // A packaged install runs from its own directory, not a cargo target.
        assert!(!is_dev_output_dir(std::path::Path::new(
            r"C:\Users\dev\AppData\Local\Ferryx"
        )));
    }
}
