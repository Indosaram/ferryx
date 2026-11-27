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
//! lifetime is bounded by the notification: the platform wait returns on
//! activation, dismissal, or timeout/close.
//!
//! On Linux a newer notification for the same pane replaces the old banner
//! instead of stacking a second one: the waiter registry signals the previous
//! waiter, which closes its own banner over D-Bus and routes nothing. Windows
//! toasts cannot be withdrawn; see [`withdraw_routed_notification`].

use super::activation::{route_activation, ActivationAction, NotificationActivations};
use super::model::{
    NotificationContent, NotificationPlatform, NotificationSound, NotificationTarget,
};
use notify_rust::{Notification, NotificationResponse};
use std::sync::Arc;

#[cfg(target_os = "linux")]
#[path = "waiter_registry.rs"]
mod waiter_registry;
#[cfg(target_os = "linux")]
use std::sync::OnceLock;

/// Canonical default-action identifier registered on every routed notification.
const DEFAULT_ACTION_ID: &str = "default";

/// Process-wide registry of live Linux click waiters.
///
/// One registry per process, so a dispatch for a pane can withdraw the banner a
/// previous dispatch left waiting for that same pane.
#[cfg(target_os = "linux")]
fn linux_registry() -> &'static waiter_registry::WaiterRegistry {
    static REGISTRY: OnceLock<waiter_registry::WaiterRegistry> = OnceLock::new();
    REGISTRY.get_or_init(waiter_registry::WaiterRegistry::new)
}

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
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn is_dev_output_dir(dir: &std::path::Path) -> bool {
    dir.ends_with("target/debug") || dir.ends_with("target/release")
}

/// Show a notification and, when it carries a routing target, wait for the
/// user's click on a dedicated thread and route it.
///
/// The user's sound setting is applied before showing, since `notify-rust`
/// leaves the alert entirely to the platform default otherwise.
///
/// `app_id` is the Windows AppUserModelID applied to the toast; it is ignored
/// on Linux. Windows only shows the toast when that identifier matches the
/// `System.AppUserModel.ID` of an installed Start-Menu shortcut, and this repo
/// registers none yet, so the NSIS/MSI packaging must register `app_id` before
/// packaged Windows toasts can be relied on.
///
/// `superseded` answers whether the order fence already withdrew this dispatch. On Linux the
/// waiter slot is registered *before* the banner is shown, so the check runs after that
/// registration and before `show()`: a withdrawal that landed in between found no slot to close,
/// and this is where the fresh banner is refused instead of outliving the fence that recorded it.
pub fn submit_with_click_routing(
    content: &NotificationContent,
    _app_id: &str,
    activations: Arc<NotificationActivations>,
    superseded: &(dyn Fn() -> bool + Send + Sync),
) -> Result<(), String> {
    let mut builder = Notification::new();
    builder.summary(&content.title).body(&content.body);
    #[cfg(target_os = "windows")]
    {
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        let dev_output = exe.parent().is_some_and(is_dev_output_dir);
        if !dev_output {
            // Nothing in this repo registers the identifier, and Windows only
            // delivers a toast whose AppUserModelID matches the installed
            // Start-Menu shortcut.
            tracing::warn!(
                app_id = %_app_id,
                "Windows toast delivery requires a registered AppUserModelID matching this identifier; an unpackaged or shortcut-less install may silently drop the toast"
            );
            builder.app_id(_app_id);
        }
    }
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

    // Register the Linux waiter *before* showing: an immediate withdrawal (a
    // newer banner for the same pane, or a closed pane) must find the slot in
    // place, or it would race `show()` and the banner would outlive it.
    #[cfg(target_os = "linux")]
    let registration = target.as_ref().map(|target| {
        let key = (target.workspace_id.clone(), target.session_id.clone());
        let (token, cancel_rx) = linux_registry().register(key.clone());
        (key, token, cancel_rx)
    });

    // The slot exists now, so a withdrawal that settled between the fence claim and that
    // registration has nothing left to close: release the slot and show nothing.
    if superseded() {
        #[cfg(target_os = "linux")]
        if let Some((key, token, _)) = &registration {
            linux_registry().finish_token(key, *token);
        }
        return Ok(());
    }

    let handle = match builder.show() {
        Ok(handle) => handle,
        Err(error) => {
            // Nothing was shown, so release the slot instead of leaving a dead one.
            #[cfg(target_os = "linux")]
            if let Some((key, token, _)) = &registration {
                linux_registry().finish_token(key, *token);
            }
            return Err(error.to_string());
        }
    };

    let Some(target) = target else {
        return Ok(());
    };

    #[cfg(target_os = "linux")]
    let Some((key, token, mut cancel_rx)) = registration else {
        return Ok(());
    };

    // A withdrawal that landed while `show()` was in flight already fired the
    // cancel: close the fresh banner now and route nothing.
    #[cfg(target_os = "linux")]
    if !matches!(
        cancel_rx.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    ) {
        handle.close();
        linux_registry().finish_token(&key, token);
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    let spawned = std::thread::Builder::new()
        .name("ferryx-notify-click".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    // Nothing can wait without a runtime; release the slot so a
                    // later withdrawal is not a no-op on a dead registration.
                    linux_registry().finish_token(&key, token);
                    tracing::error!("notification click waiter runtime failed: {error}");
                    return;
                }
            };
            // Both futures hold a shared borrow of the handle rather than the
            // handle itself: the platform wait only needs `&self`, and the
            // response is borrowed for its callback only, so the classified
            // action is copied out and never outlives the reference.
            let wait_handle = &handle;
            let close_handle = &handle;
            let wait_fut = async move {
                let mut captured = None;
                wait_handle
                    .wait_for_action_async(|response| {
                        captured = Some(classify_action(response));
                    })
                    .await;
                captured
            };
            let close_fut = move || async move {
                close_handle.close_async().await;
            };
            let outcome = runtime.block_on(waiter_registry::drive_cancellable_waiter(
                linux_registry(),
                key.clone(),
                token,
                wait_fut,
                close_fut,
                cancel_rx,
            ));
            // A withdrawn waiter has already closed its own banner and routes
            // nothing; only a surviving default activation is routable.
            if let waiter_registry::WaiterOutcome::Action(Some(action)) = outcome {
                route_action(action, &target, activations.as_ref());
            }
        });

    #[cfg(target_os = "windows")]
    let spawned = std::thread::Builder::new()
        .name("ferryx-notify-click".into())
        .spawn(move || {
            if let Err(error) = handle.wait_for_response(move |response: &NotificationResponse| {
                route_action(classify_action(response), &target, activations.as_ref());
            }) {
                tracing::error!("notification click response failed: {error}");
            }
        });

    spawned.map(|_| ()).map_err(|error| error.to_string())
}

/// Withdraw the routed notification banner for `target`, if one is live.
///
/// The waiter for that pane stops waiting, closes its own banner, and routes no
/// activation, so a superseded notification can never steal focus later.
pub fn withdraw_routed_notification(target: &NotificationTarget) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        linux_registry().withdraw(&(target.workspace_id.clone(), target.session_id.clone()));
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        // A Windows toast cannot be withdrawn from here: the toast needs a
        // registered AppUserModelID, which this repo does not install yet, and
        // `notify-rust`'s Windows `NotificationHandle` exposes neither `close()`
        // nor `id()`, so there is no handle to address the live toast with. The
        // waiter therefore ends when the user acts or the toast expires.
        let _ = target;
        Ok(())
    }
}

/// Apply the shared default-vs-dismiss policy to a `notify-rust` response.
///
/// The response is borrowed for the platform callback only, so the decision is
/// copied out here rather than the reference being kept alive.
fn classify_action(response: &NotificationResponse) -> ActivationAction {
    match response {
        NotificationResponse::Default => ActivationAction::Default,
        NotificationResponse::Action(id) if id == DEFAULT_ACTION_ID => ActivationAction::Default,
        // Named non-default actions, inline replies, and closes never route.
        _ => ActivationAction::Dismiss,
    }
}

/// Route an already classified action for `target` through the shared seam.
///
/// The target is typed here (it was carried in-process, not through the OS
/// payload), so it only has to be re-encoded for the queue. Returns `true` iff a
/// target was enqueued.
fn route_action(
    action: ActivationAction,
    target: &NotificationTarget,
    activations: &NotificationActivations,
) -> bool {
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

    fn route_response(
        response: &NotificationResponse,
        target: &NotificationTarget,
        activations: &NotificationActivations,
    ) -> bool {
        route_action(classify_action(response), target, activations)
    }

    #[test]
    fn default_response_routes_the_target() {
        let queue = NotificationActivations::new();
        assert!(route_response(
            &NotificationResponse::Default,
            &target(),
            &queue
        ));
        assert_eq!(queue.drain().len(), 1);
    }

    #[test]
    fn default_named_action_routes_the_target() {
        let queue = NotificationActivations::new();
        assert!(route_response(
            &NotificationResponse::Action(DEFAULT_ACTION_ID.into()),
            &target(),
            &queue
        ));
        assert_eq!(queue.drain().len(), 1);
    }

    #[test]
    fn close_and_other_actions_do_not_route() {
        let queue = NotificationActivations::new();
        assert!(!route_response(
            &NotificationResponse::Closed(CloseReason::Dismissed),
            &target(),
            &queue
        ));
        assert!(!route_response(
            &NotificationResponse::Action("some-other".into()),
            &target(),
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
