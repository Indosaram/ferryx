//! Modern macOS notification submission and its completion contract.

use super::model::{NotificationContent, NotificationSound};
use block2::{Block, RcBlock};
use objc2::rc::Retained;
use objc2_foundation::{NSError, NSString};
use objc2_user_notifications::{UNMutableNotificationContent, UNNotificationRequest, UNNotificationSound, UNUserNotificationCenter};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::Duration;

static SUBMISSION_COUNTER: AtomicU64 = AtomicU64::new(0);

fn native_content(content: &NotificationContent) -> Retained<UNMutableNotificationContent> {
    let native = UNMutableNotificationContent::new();
    native.setTitle(&NSString::from_str(&content.title));
    native.setBody(&NSString::from_str(&content.body));
    match content.sound {
        NotificationSound::System => native.setSound(Some(&UNNotificationSound::defaultSound())),
        NotificationSound::Silent => native.setSound(None),
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
mod tests {
    use super::*;
    use crate::notification::model::{format_notification, DispatchNotificationRequest};

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
