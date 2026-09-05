//! Modern macOS notification submission and its completion contract.

use super::model::{NotificationContent, NotificationSound};
#[cfg(test)]
use super::model::NotificationTarget;
use block2::{Block, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::{NSCopying, NSDictionary, NSError, NSString};
use objc2_user_notifications::{UNMutableNotificationContent, UNNotificationRequest, UNNotificationSound, UNUserNotificationCenter};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::Duration;

static SUBMISSION_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Canonical, ferryx-namespaced userInfo key carrying the click-routing target.
///
/// The target is stored as one JSON `NSString` under this single key so the
/// delegate can round-trip it back to a typed [`NotificationTarget`] on click.
pub(crate) const TARGET_USER_INFO_KEY: &str = "ferryx.notification.target";

#[cfg(test)]
pub(crate) fn decode_target(raw: &str) -> Option<NotificationTarget> {
    serde_json::from_str::<NotificationTarget>(raw).ok()
}

fn native_content(content: &NotificationContent) -> Retained<UNMutableNotificationContent> {
    let native = UNMutableNotificationContent::new();
    native.setTitle(&NSString::from_str(&content.title));
    native.setBody(&NSString::from_str(&content.body));
    match content.sound {
        NotificationSound::System => native.setSound(Some(&UNNotificationSound::defaultSound())),
        NotificationSound::Silent => native.setSound(None),
    }
    if let Some(target) = &content.target {
        let json = serde_json::json!({
            "workspaceId": target.workspace_id, "sessionId": target.session_id,
        }).to_string();
        let value = NSString::from_str(&json);
        let key = NSString::from_str(TARGET_USER_INFO_KEY);
        let key_copying = ProtocolObject::<dyn NSCopying>::from_ref(&*key);
        // SAFETY: Both NSStrings are live and retained by the immutable dictionary.
        let user_info: Retained<NSDictionary> =
            unsafe { NSDictionary::dictionaryWithObject_forKey(&value, key_copying) };
        // SAFETY: userInfo accepts this immutable NSString-to-NSString dictionary.
        unsafe { native.setUserInfo(&user_info) };
    }
    native
}

fn await_submission(
    submit: impl FnOnce(&Block<dyn Fn(*mut NSError)>) -> Result<(), String>,
) -> Result<(), String> {
    let (tx, rx) = mpsc::sync_channel(1);
    let handler = RcBlock::new(move |error: *mut NSError| {
        let result = if error.is_null() {
            Ok(())
        } else {
            // SAFETY: Apple keeps a non-null NSError valid for the callback duration; only its owned description crosses threads.
            Err(unsafe { (*error).localizedDescription() }.to_string())
        };
        let _ = tx.try_send(result);
    });
    submit(&handler)?;
    drop(handler);
    rx.recv_timeout(Duration::from_secs(5))
        .map_err(|_| "UNUserNotificationCenter submission completion unavailable".to_string())?
}

pub fn submit_notification(content: &NotificationContent) -> Result<(), String> {
    if !super::permission::macos::has_bundle_identity() {
        return Err("notifications require a bundled .app".into());
    }
    let identifier = format!("ferryx-{}-{}", std::process::id(), SUBMISSION_COUNTER.fetch_add(1, Ordering::Relaxed));
    await_submission(|handler| {
        objc2::exception::catch(std::panic::AssertUnwindSafe(|| {
            let native = native_content(content);
            let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
                &NSString::from_str(&identifier), &native, None);
            UNUserNotificationCenter::currentNotificationCenter()
                .addNotificationRequest_withCompletionHandler(&request, Some(handler));
        })).map_err(|_| "UNUserNotificationCenter submission raised an exception".to_string())
    })
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    /// Build the production native content (userInfo included) for tests in
    /// sibling modules so they exercise the real encoder, not a copy.
    pub(crate) fn native_content_for_test(
        content: &NotificationContent,
    ) -> Retained<UNMutableNotificationContent> {
        native_content(content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notification::model::{format_notification, DispatchNotificationRequest};

    fn user_info_target_json(
        content: &objc2_user_notifications::UNMutableNotificationContent,
    ) -> Option<String> {
        let key = NSString::from_str(TARGET_USER_INFO_KEY);
        content
            .userInfo()
            .objectForKey(&key)
            .and_then(|obj| obj.downcast::<NSString>().ok())
            .map(|s| s.to_string())
    }

    #[test]
    fn native_content_encodes_target_into_user_info_and_round_trips() {
        let request = DispatchNotificationRequest {
            source: crate::notification::model::NotificationSource::AgentTaskComplete,
            agent_label: Some("Claude".into()),
            target: Some(NotificationTarget {
                workspace_id: "ws-5".into(),
                session_id: "fe-11".into(),
            }),
            ..Default::default()
        };
        let native = native_content(&format_notification(&request));
        let raw = user_info_target_json(&native).expect("userInfo carries the target key");
        assert_eq!(decode_target(&raw), request.target);
    }

    #[test]
    fn native_content_without_target_omits_the_user_info_key() {
        // Test / id-less notifications must not carry a routing key.
        let request: DispatchNotificationRequest = serde_json::from_value(serde_json::json!({
            "source": "test"
        }))
        .unwrap();
        let native = native_content(&format_notification(&request));
        assert_eq!(user_info_target_json(&native), None);
    }

    #[test]
    fn decode_target_rejects_malformed_payloads() {
        assert!(decode_target("not json").is_none());
        assert!(decode_target(r#"{\"workspaceId\":\"ws\"}"#).is_none());
        assert_eq!(
            decode_target(r#"{"workspaceId":"w","sessionId":"s"}"#),
            Some(NotificationTarget {
                workspace_id: "w".into(),
                session_id: "s".into()
            })
        );
    }

    #[test]
    fn native_content_uses_only_the_selected_sound() {
        for (sound, expected) in [("system", true), ("silent", false)] {
            let request: DispatchNotificationRequest = serde_json::from_value(serde_json::json!({
                "source": "test", "sound": sound
            })).unwrap();
            assert_eq!(native_content(&format_notification(&request)).sound().is_some(), expected);
        }
    }

    #[test]
    fn submission_waits_for_the_registered_completion() {
        assert!(await_submission(|handler| {
            handler.call((std::ptr::null_mut(),));
            Ok(())
        }).is_ok());
    }

    #[test]
    fn submission_propagates_native_rejection() {
        // SAFETY: The domain NSString is valid for the call and NSError retains it; no userInfo is supplied.
        let error = unsafe { objc2_foundation::NSError::errorWithDomain_code_userInfo(
            &objc2_foundation::NSString::from_str("FerryxNotificationTest"), 17, None) };
        assert!(await_submission(|handler| {
            handler.call((objc2::rc::Retained::as_ptr(&error).cast_mut(),));
            Ok(())
        }).is_err());
    }

    #[test]
    fn submission_does_not_report_success_without_a_callback() {
        assert!(await_submission(|_| Ok(())).is_err());
    }
}
