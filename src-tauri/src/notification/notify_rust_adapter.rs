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
use super::model::{NotificationContent, NotificationTarget};
use notify_rust::{Notification, NotificationResponse};
use std::sync::Arc;

/// Canonical default-action identifier registered on every routed notification.
const DEFAULT_ACTION_ID: &str = "default";

/// Show a notification and, when it carries a routing target, wait for the
/// user's click on a dedicated thread and route it.
///
/// `app_id` preserves the Windows AppUserModelID the app already registers so
/// toasts keep their identity; it is ignored on Linux.
pub fn submit_with_click_routing(
    content: &NotificationContent,
    _app_id: &str,
    activations: Arc<NotificationActivations>,
) -> Result<(), String> {
    let mut builder = Notification::new();
    builder.summary(&content.title).body(&content.body);
    #[cfg(target_os = "windows")]
    {
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        let dev_output = exe.parent().is_some_and(|dir| {
            dir.ends_with("target/debug") || dir.ends_with("target/release")
        });
        if !dev_output {
            builder.app_id(_app_id);
        }
    }
    #[cfg(target_os = "linux")]
    builder.appname("Ferryx");

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
    }).to_string();
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
}
