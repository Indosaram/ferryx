//! Native APNs registration for the iOS shell.
//!
//! An iOS WKWebView has no `PushManager`, so the remote UI cannot use Web Push on the installed
//! app. This module registers with APNs and hands the device token to the web shell.

use std::sync::Mutex;
use std::sync::OnceLock;

#[derive(Debug, Clone, Default)]
pub enum Registration {
    #[default]
    Idle,
    Registered(String),
    Failed(String),
}

/// A token plus the APNs environment it was minted for. A development build's token is only
/// valid on the sandbox host, so the gateway must be told which one to post to.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PushRegistration {
    pub token: String,
    pub sandbox: bool,
}

static STATE: OnceLock<Mutex<Registration>> = OnceLock::new();

fn state() -> &'static Mutex<Registration> {
    STATE.get_or_init(|| Mutex::new(Registration::Idle))
}

pub fn set_registered(token: String) {
    *state().lock().expect("the APNs state lock is never poisoned") = Registration::Registered(token);
}

pub fn set_failed(reason: String) {
    *state().lock().expect("the APNs state lock is never poisoned") = Registration::Failed(reason);
}

pub fn current() -> Registration {
    state().lock().expect("the APNs state lock is never poisoned").clone()
}

/// APNs hands the shell 32 raw bytes; the gateway validates exactly 64 lowercase hex characters.
pub fn hex_token(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
    })
}

/// The environment is decided here, not by the caller, because a mismatch is fatal: a token
/// minted for one environment answers `BadDeviceToken` on the other, and the gateway treats that
/// as a dead token and drops it.
///
/// What actually decides it is the signing profile: the `aps-environment` entitlement iOS embeds
/// comes from the profile, not from the plist this project ships. A development-signed build gets
/// the sandbox; Apple re-signs App Store and TestFlight builds with a distribution profile whose
/// entitlement is `production`. `debug_assertions` tracks that split for every build this repo
/// produces, and `PUSH_SANDBOX` overrides it for the one case it does not: a release build signed
/// with a development profile.
fn apns_sandbox() -> bool {
    resolve_sandbox(std::env::var("FERRYX_APNS_SANDBOX").ok().as_deref(), cfg!(debug_assertions))
}

/// Pure resolution, so the split can be tested without mutating process-global env in a suite
/// that runs its cases in parallel.
fn resolve_sandbox(override_value: Option<&str>, debug_build: bool) -> bool {
    match override_value {
        Some("1") | Some("true") => true,
        Some("0") | Some("false") => false,
        _ => debug_build,
    }
}

/// Asks for alert permission, registers with APNs, and returns the registered token and the
/// APNs environment that token belongs to.
///
/// Must run from a user gesture: iOS shows the prompt only then.
#[tauri::command]
pub async fn request_push_permission(
    app: tauri::AppHandle,
) -> Result<Option<PushRegistration>, String> {
    #[cfg(target_os = "ios")]
    {
        ios::install_delegate_methods(app.clone());
        ios::request(&app).await?;
        let sandbox = apns_sandbox();
        ios::await_token().await.map(|token| Some(PushRegistration { token, sandbox }))
    }
    #[cfg(not(target_os = "ios"))]
    {
        let _ = app;
        Ok(None)
    }
}

#[cfg(target_os = "ios")]
mod ios {
    use super::{hex_token, set_failed, set_registered, Registration};
    use std::sync::mpsc::{channel, Sender};
    use std::sync::Mutex;

    static PENDING: Mutex<Option<Sender<()>>> = Mutex::new(None);

    fn notify_waiter() {
        if let Some(sender) = PENDING.lock().expect("the APNs pending lock is never poisoned").take() {
            let _ = sender.send(());
        }
    }

    pub(super) fn handle_token(bytes: &[u8]) {
        set_registered(hex_token(bytes));
        notify_waiter();
    }

    pub(super) fn handle_failure(reason: String) {
        set_failed(reason);
        notify_waiter();
    }

    /// Adds the APNs callbacks to the app delegate `tao` already registered, rather than
    /// installing a second delegate or replacing an existing method.
    pub(super) fn install_delegate_methods(app: tauri::AppHandle) {
        use objc2::runtime::{AnyClass, AnyObject, Sel};
        use std::ffi::CStr;
        use std::sync::Once;

        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            let _ = app.run_on_main_thread(|| unsafe {
                let Some(class) = AnyClass::get(c"AppDelegate") else {
                    set_failed("the app delegate was not registered".into());
                    return;
                };
                let raw = (&*class) as *const AnyClass as *mut AnyClass;

                extern "C-unwind" fn did_register(
                    _this: &AnyObject,
                    _cmd: Sel,
                    device_token: *mut AnyObject,
                ) {
                    if device_token.is_null() {
                        handle_failure("APNs returned an empty device token".into());
                        return;
                    }
                    // SAFETY: the delegate passes a live NSData for the token.
                    let data = unsafe { &*(device_token as *const objc2_foundation::NSData) };
                    handle_token(&nsdata_bytes(data));
                }
                extern "C-unwind" fn did_fail(
                    _this: &AnyObject,
                    _cmd: Sel,
                    error: *mut AnyObject,
                ) {
                    let reason = if error.is_null() {
                        "APNs registration failed".to_string()
                    } else {
                        // SAFETY: the delegate passes a live NSError on failure.
                        let error = unsafe { &*(error as *const objc2_foundation::NSError) };
                        format!("APNs registration failed: {}", error.localizedDescription())
                    };
                    handle_failure(reason);
                }

                const VOID_OBJECT: &CStr = c"v@:@";
                let registered_sel =
                    Sel::register(c"application:didRegisterForRemoteNotificationsWithDeviceToken:");
                let failed_sel = Sel::register(c"application:didFailToRegisterForRemoteNotificationsWithError:");
                // `class_addMethod` answers false when the selector already exists, and it then
                // leaves the existing implementation in place. Swallowing that would surface only
                // as a 30-second wait with no token, so report it as the setup failure it is.
                let added_registered = objc2::ffi::class_addMethod(
                    raw,
                    registered_sel,
                    std::mem::transmute::<
                        extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject),
                        objc2::runtime::Imp,
                    >(did_register),
                    VOID_OBJECT.as_ptr(),
                );
                let added_failed = objc2::ffi::class_addMethod(
                    raw,
                    failed_sel,
                    std::mem::transmute::<
                        extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject),
                        objc2::runtime::Imp,
                    >(did_fail),
                    VOID_OBJECT.as_ptr(),
                );
                if !added_registered || !added_failed {
                    set_failed(
                        "another framework already handles the APNs delegate callbacks"
                            .to_string(),
                    );
                }
            });
        });
    }

    fn nsdata_bytes(data: &objc2_foundation::NSData) -> Vec<u8> {
        use std::ptr::NonNull;
        let length = data.length();
        if length == 0 {
            return Vec::new();
        }
        let mut bytes = vec![0u8; length];
        let Some(buffer) = NonNull::new(bytes.as_mut_ptr().cast()) else {
            return Vec::new();
        };
        // SAFETY: `getBytes:length:` copies at most `length` bytes into a buffer of that size.
        unsafe { data.getBytes_length(buffer, length) };
        bytes
    }

    /// Requests alert permission, then registers with APNs from the main thread.
    pub(super) async fn request(app: &tauri::AppHandle) -> Result<(), String> {
        use objc2::runtime::Bool;
        use objc2_user_notifications::{UNAuthorizationOptions, UNUserNotificationCenter};

        let (sender, receiver) = channel::<Result<(), String>>();
        let handle = app.clone();
        let submitted = app.run_on_main_thread(move || {
            let center = UNUserNotificationCenter::currentNotificationCenter();
            let options = UNAuthorizationOptions::Alert
                | UNAuthorizationOptions::Badge
                | UNAuthorizationOptions::Sound;
            let handler = block2::RcBlock::new(
                move |granted: Bool, error: *mut objc2_foundation::NSError| {
                    if !granted.as_bool() {
                        let reason = if error.is_null() {
                            "notifications were not allowed".to_string()
                        } else {
                            // SAFETY: a non-null error is the object the callback passed.
                            let error = unsafe { &*error };
                            format!(
                                "notifications were not allowed: {}",
                                error.localizedDescription()
                            )
                        };
                        let _ = sender.send(Err(reason));
                        return;
                    }
                    // `registerForRemoteNotifications` is main-thread only, and this callback is not.
                    let inner = sender.clone();
                    let _ = handle.run_on_main_thread(move || {
                        use objc2_ui_kit::UIApplication;
                        unsafe {
                            UIApplication::sharedApplication().registerForRemoteNotifications();
                        }
                        let _ = inner.send(Ok(()));
                    });
                },
            );
            center.requestAuthorizationWithOptions_completionHandler(options, &handler);
        });
        submitted.map_err(|error| format!("the permission prompt could not be shown: {error}"))?;
        tauri::async_runtime::spawn_blocking(move || {
            receiver
                .recv_timeout(std::time::Duration::from_secs(120))
                .unwrap_or_else(|_| Err("the notification permission prompt did not answer".into()))
        })
        .await
        .map_err(|error| format!("the permission result was lost: {error}"))?
    }

    /// Waits for the delegate callback that carries the token.
    pub(super) async fn await_token() -> Result<String, String> {
        let (sender, receiver) = channel::<()>();
        // Install the waiter before reading the state: APNs can answer between the permission
        // grant and this call, and checking first would then wait the full timeout for an
        // event that already happened.
        *PENDING.lock().expect("the APNs pending lock is never poisoned") = Some(sender);
        if !matches!(super::current(), Registration::Idle) {
            // The callback already ran; drop the waiter it never saw.
            let _ = PENDING.lock().expect("the APNs pending lock is never poisoned").take();
        } else {
            tauri::async_runtime::spawn_blocking(move || {
                let _ = receiver.recv_timeout(std::time::Duration::from_secs(30));
            })
            .await
            .map_err(|error| format!("the APNs token wait was lost: {error}"))?;
        }
        match super::current() {
            Registration::Registered(token) => Ok(token),
            Registration::Failed(reason) => Err(reason),
            Registration::Idle => Err("APNs registered no device token".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_token_is_lowercase_and_doubles_the_length() {
        assert_eq!(hex_token(&[0x00, 0x0f, 0xa1, 0xff]), "000fa1ff");
        assert_eq!(hex_token(&[0u8; 32]).len(), 64);
    }

    #[test]
    fn a_late_token_replaces_an_earlier_failure() {
        set_failed("not asked".into());
        assert!(matches!(current(), Registration::Failed(_)));
        set_registered(hex_token(&[0xab; 32]));
        match current() {
            Registration::Registered(token) => assert_eq!(token.len(), 64),
            other => panic!("expected a registered token, got {other:?}"),
        }
    }

    // A build/env mismatch is not recoverable at runtime: the wrong host answers BadDeviceToken
    // and the gateway drops the token, so the split has to be right before anything is sent.
    #[test]
    fn the_sandbox_flag_follows_the_build_and_the_override() {
        assert!(resolve_sandbox(None, true), "a debug build registers on the sandbox");
        assert!(!resolve_sandbox(None, false), "a release build registers on production");
        assert!(resolve_sandbox(Some("1"), false));
        assert!(resolve_sandbox(Some("true"), false));
        assert!(!resolve_sandbox(Some("0"), true));
        assert!(!resolve_sandbox(Some("false"), true));
        // An unrecognised value must not silently pick the wrong host.
        assert!(resolve_sandbox(Some("yes please"), true));
        assert!(!resolve_sandbox(Some(""), false));
    }
}
