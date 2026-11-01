//! Asks the OS to confirm the user before a saved password is shown or copied.
//!
//! macOS: LocalAuthentication (Touch ID, Apple Watch or the login password,
//! LAPolicyDeviceOwnerAuthentication). Windows: Windows Hello (UserConsentVerifier: face,
//! fingerprint or the account PIN); when Hello is not set up, the request is refused with that
//! reason rather than allowed. Linux: polkit's `pkcheck` with an auth dialog for the session
//! user; without polkit the request is refused with the reason.
//!
//! Blocking: run it through `crate::ipc::run_blocking`.

/// `Ok(())` when the user confirmed; `Err(reason)` when they cancelled or the OS cannot ask.
pub fn confirm_user(reason: &str) -> Result<(), String> {
    platform::confirm_user(reason)
}

#[cfg(target_os = "macos")]
mod platform {
    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Bool};
    use objc2::msg_send;
    use objc2_foundation::{NSError, NSString};

    /// LAPolicyDeviceOwnerAuthentication: biometrics, watch or the account password.
    const DEVICE_OWNER_AUTHENTICATION: isize = 2;

    #[link(name = "LocalAuthentication", kind = "framework")]
    extern "C" {}

    pub(super) fn confirm_user(reason: &str) -> Result<(), String> {
        let class = AnyClass::get(c"LAContext").ok_or("LocalAuthentication is unavailable")?;
        // SAFETY: LAContext is an NSObject subclass; `new` returns a +1 instance.
        let context: Option<Retained<AnyObject>> = unsafe { msg_send![class, new] };
        let context = context.ok_or("could not create an authentication context")?;
        let (sender, receiver) = std::sync::mpsc::sync_channel::<Result<(), String>>(1);
        let reply = RcBlock::new(move |success: Bool, error: *mut NSError| {
            let outcome = if success.as_bool() {
                Ok(())
            } else if error.is_null() {
                Err("authentication failed".to_string())
            } else {
                // SAFETY: LocalAuthentication passes a valid NSError when it fails.
                Err(unsafe { (*error).localizedDescription() }.to_string())
            };
            let _ = sender.try_send(outcome);
        });
        let reason = NSString::from_str(&format!("Ferryx wants to {reason}."));
        // SAFETY: documented LAContext selector; the block is copied by the callee.
        unsafe {
            let _: () = msg_send![
                &*context,
                evaluatePolicy: DEVICE_OWNER_AUTHENTICATION,
                localizedReason: &*reason,
                reply: &*reply
            ];
        }
        receiver.recv().map_err(|_| "authentication was interrupted".to_string())?
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use windows::core::HSTRING;
    use windows::Security::Credentials::UI::{
        UserConsentVerificationResult, UserConsentVerifier, UserConsentVerifierAvailability,
    };

    pub(super) fn confirm_user(reason: &str) -> Result<(), String> {
        let available = UserConsentVerifier::CheckAvailabilityAsync()
            .and_then(|operation| operation.get())
            .map_err(|error| format!("Windows Hello is unavailable: {error}"))?;
        if available != UserConsentVerifierAvailability::Available {
            return Err("set up Windows Hello (PIN, face or fingerprint) to show saved passwords".into());
        }
        let result = UserConsentVerifier::RequestVerificationAsync(&HSTRING::from(format!("Ferryx wants to {reason}.")))
            .and_then(|operation| operation.get())
            .map_err(|error| format!("Windows Hello failed: {error}"))?;
        if result == UserConsentVerificationResult::Verified {
            Ok(())
        } else {
            Err("Windows Hello did not confirm the user".into())
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    /// A polkit action every desktop ships that requires the user's own password
    /// (auth_self) for an active session.
    const ACTION: &str = "org.freedesktop.policykit.exec";

    pub(super) fn confirm_user(_reason: &str) -> Result<(), String> {
        let pid = std::process::id().to_string();
        let status = std::process::Command::new("pkcheck")
            .args(["--action-id", ACTION, "--process", &pid, "--allow-user-interaction"])
            .status()
            .map_err(|error| format!("polkit (pkcheck) is unavailable to confirm the user: {error}"))?;
        if status.success() {
            Ok(())
        } else {
            Err("the system did not confirm the user".into())
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
mod platform {
    pub(super) fn confirm_user(_reason: &str) -> Result<(), String> {
        Err("this platform cannot confirm the user".into())
    }
}
