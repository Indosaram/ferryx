//! macOS `UNUserNotificationCenter` click delegate.
//!
//! The notification center holds its delegate *weakly*, so the app keeps the
//! sole strong reference alive for the process lifetime through managed state.
//! Delegate callbacks arrive on a background GCD queue, so this class is not
//! main-thread-only; it only touches the thread-safe activation queue and the
//! completion handler, never AppKit.

use super::activation::{route_activation, ActivationAction, NotificationActivations};
use super::macos_submission::TARGET_USER_INFO_KEY;
use block2::DynBlock;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_foundation::{NSObject, NSObjectProtocol, NSString};
use objc2_user_notifications::{
    UNNotificationContent, UNNotificationDefaultActionIdentifier, UNNotificationPresentationOptions,
    UNNotificationResponse, UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};
use std::sync::Arc;

pub struct DelegateIvars {
    activations: Arc<NotificationActivations>,
}

define_class!(
    // SAFETY:
    // - NSObject has no subclassing requirements.
    // - FerryxNotificationDelegate does not implement Drop.
    // - Callbacks run on a background queue and touch only the thread-safe
    //   activation queue, so the class is intentionally not MainThreadOnly.
    #[unsafe(super(NSObject))]
    #[name = "FerryxNotificationDelegate"]
    #[ivars = DelegateIvars]
    pub struct FerryxNotificationDelegate;

    unsafe impl NSObjectProtocol for FerryxNotificationDelegate {}

    unsafe impl UNUserNotificationCenterDelegate for FerryxNotificationDelegate {
        // Preserve the "no automatic foreground banners" policy: when the app
        // is frontmost the frontend already shows in-app state, so present
        // nothing. Always invoke the completion handler.
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &objc2_user_notifications::UNNotification,
            completion_handler: &DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            completion_handler.call((UNNotificationPresentationOptions::empty(),));
        }

        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive_response(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            completion_handler: &DynBlock<dyn Fn()>,
        ) {
            self.handle_response(response);
            // The OS requires the completion handler be called exactly once,
            // even when we ignore the response.
            completion_handler.call(());
        }
    }
);

impl FerryxNotificationDelegate {
    pub fn new(activations: Arc<NotificationActivations>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(DelegateIvars { activations });
        // SAFETY: `this` is a freshly allocated, ivars-initialized instance of
        // this class; `init` on NSObject is always valid and returns the same
        // (now initialized) instance.
        unsafe { msg_send![super(this), init] }
    }

    /// Classify the response and route its carried target. Delegates the whole
    /// extraction+decode+enqueue path to [`route_from_content`] so the same
    /// production code is exercised by tests that build a real content object.
    fn handle_response(&self, response: &UNNotificationResponse) {
        let action = classify_action(&response.actionIdentifier());
        let content = response.notification().request().content();
        route_from_content(self.ivars().activations.as_ref(), action, &content);
    }
}

/// Map an action identifier to our default-vs-dismiss decision. Only the system
/// default-action identifier routes focus; the dismiss identifier and any
/// custom action are treated as non-routing.
fn classify_action(identifier: &NSString) -> ActivationAction {
    // SAFETY: `UNNotificationDefaultActionIdentifier` is an immutable framework
    // global string constant, valid for the process lifetime; `isEqual:`
    // borrows it only for the comparison.
    let default_id = unsafe { UNNotificationDefaultActionIdentifier };
    if identifier.isEqual(Some(default_id)) {
        ActivationAction::Default
    } else {
        ActivationAction::Dismiss
    }
}

/// Production routing path shared by the delegate callback and its tests.
///
/// Reads the canonical userInfo key off a real `UNNotificationContent`, decodes
/// it with [`route_activation`], and enqueues only on a default action
/// with a well-formed target. Returns `true` iff a target was enqueued.
fn route_from_content(
    queue: &NotificationActivations,
    action: ActivationAction,
    content: &UNNotificationContent,
) -> bool {
    if action != ActivationAction::Default {
        return false;
    }
    let key = NSString::from_str(TARGET_USER_INFO_KEY);
    let raw = content
        .userInfo()
        .objectForKey(&key)
        .and_then(|obj| obj.downcast::<NSString>().ok())
        .map(|s| s.to_string());
    route_activation(queue, action, raw.as_deref())
}

/// Install the delegate on the shared notification center and keep the strong
/// reference alive by handing it back to the caller (managed app state).
///
/// The center retains the delegate only weakly, so dropping the returned value
/// would silently disable click routing.
pub fn install_delegate(
    activations: Arc<NotificationActivations>,
) -> Retained<FerryxNotificationDelegate> {
    let delegate = FerryxNotificationDelegate::new(activations);
    let proto = ProtocolObject::from_ref(&*delegate);
    if objc2::exception::catch(std::panic::AssertUnwindSafe(|| {
        UNUserNotificationCenter::currentNotificationCenter().setDelegate(Some(proto));
    }))
    .is_err()
    {
        // A failure here means clicks will never route; make it visible rather
        // than silently losing every activation.
        tracing::error!(
            "failed to install UNUserNotificationCenter delegate; notification clicks will not route"
        );
    }
    delegate
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notification::model::{
        format_notification, DispatchNotificationRequest, NotificationSource, NotificationTarget,
    };
    use crate::notification::macos_submission::test_support::native_content_for_test;

    fn content_with_target(target: Option<NotificationTarget>) -> Retained<UNNotificationContent> {
        // Build a real content object through the production encoder so the test
        // exercises userInfo write + read + decode end to end.
        let request = DispatchNotificationRequest {
            source: NotificationSource::AgentTaskComplete,
            agent_label: Some("Claude".into()),
            target,
            ..Default::default()
        };
        let mutable = native_content_for_test(&format_notification(&request));
        // Upcast the mutable subclass to the immutable base the delegate sees.
        Retained::into_super(mutable)
    }

    #[test]
    fn classify_action_matches_the_system_default_identifier() {
        // SAFETY: framework global constant string, valid for the process.
        let default_id = unsafe { UNNotificationDefaultActionIdentifier };
        assert_eq!(classify_action(default_id), ActivationAction::Default);
        assert_eq!(
            classify_action(&NSString::from_str("some.custom.action")),
            ActivationAction::Dismiss
        );
    }

    #[test]
    fn default_action_with_valid_target_reads_user_info_and_enqueues() {
        let queue = NotificationActivations::new();
        let content = content_with_target(Some(NotificationTarget {
            workspace_id: "ws".into(),
            session_id: "fe".into(),
        }));

        assert!(route_from_content(&queue, ActivationAction::Default, &content));
        let drained = queue.drain();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].workspace_id, "ws");
        assert_eq!(drained[0].session_id, "fe");
    }

    #[test]
    fn dismiss_action_never_routes_even_with_a_valid_target() {
        let queue = NotificationActivations::new();
        let content = content_with_target(Some(NotificationTarget {
            workspace_id: "ws".into(),
            session_id: "fe".into(),
        }));
        assert!(!route_from_content(&queue, ActivationAction::Dismiss, &content));
        assert_eq!(queue.len(), 0);
    }

    #[test]
    fn default_action_without_a_target_key_does_not_route() {
        // Test / id-less notifications carry no userInfo target.
        let queue = NotificationActivations::new();
        let content = content_with_target(None);
        assert!(!route_from_content(&queue, ActivationAction::Default, &content));
        assert_eq!(queue.len(), 0);
    }
}
