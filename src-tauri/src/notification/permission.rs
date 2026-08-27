//! Authoritative notification permission queries.

use super::model::{
    NotificationAuthorization, NotificationPermissionRequestDto, NotificationPermissionStatusDto,
    NotificationPlatform,
};

/// Seam over the OS permission backend.
pub trait NotificationPermissionProvider: Send + Sync {
    fn status(&self) -> NotificationPermissionStatusDto;
    fn request(&self) -> NotificationPermissionRequestDto;
}

/// Provider selected for the current build target.
pub fn platform_permission_provider() -> Box<dyn NotificationPermissionProvider> {
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::MacosPermissionProvider)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Box::new(DesktopFallbackPermissionProvider)
    }
}

/// Non-authoritative provider for platforms without a real permission query.
#[derive(Debug, Default, Clone, Copy)]
pub struct DesktopFallbackPermissionProvider;

impl NotificationPermissionProvider for DesktopFallbackPermissionProvider {
    fn status(&self) -> NotificationPermissionStatusDto {
        let platform = NotificationPlatform::current();
        let can_open_settings = matches!(platform, NotificationPlatform::Windows);
        NotificationPermissionStatusDto {
            platform,
            supported: true,
            authorization: NotificationAuthorization::Authorized,
            alerts_enabled: Some(true),
            sounds_enabled: Some(true),
            requested: true,
            authoritative: false,
            can_open_settings,
        }
    }

    fn request(&self) -> NotificationPermissionRequestDto {
        NotificationPermissionRequestDto {
            granted: true,
            status: self.status(),
            error: None,
        }
    }
}

#[cfg(target_os = "macos")]
pub mod macos {
    use super::*;
    use block2::RcBlock;
    use objc2_foundation::{NSBundle, NSError};
    use super::super::model::NotificationContent;
    use objc2_foundation::NSString;
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNAuthorizationStatus, UNMutableNotificationContent,
        UNNotificationRequest, UNNotificationSetting, UNNotificationSettings,
        UNUserNotificationCenter,
    };
    use std::ptr::NonNull;
    use std::sync::mpsc;
    use std::time::Duration;

    const CALLBACK_TIMEOUT: Duration = Duration::from_secs(5);

    pub fn map_authorization_status(status: UNAuthorizationStatus) -> NotificationAuthorization {
        match status {
            UNAuthorizationStatus::NotDetermined => NotificationAuthorization::NotDetermined,
            UNAuthorizationStatus::Denied => NotificationAuthorization::Denied,
            UNAuthorizationStatus::Authorized => NotificationAuthorization::Authorized,
            UNAuthorizationStatus::Provisional => NotificationAuthorization::Provisional,
            UNAuthorizationStatus::Ephemeral => NotificationAuthorization::Provisional,
            _ => NotificationAuthorization::Unknown,
        }
    }

    pub fn map_notification_setting(setting: UNNotificationSetting) -> Option<bool> {
        match setting {
            UNNotificationSetting::Enabled => Some(true),
            UNNotificationSetting::Disabled => Some(false),
            _ => None,
        }
    }

    /// Submit one notification through `UNUserNotificationCenter`.
    ///
    /// `usernoted` refuses to serve a single process over both notification APIs: querying
    /// authorization already registers us as a modern client, so a legacy `NSUserNotification`
    /// submission is denied ("You can't mix modern clients with legacy clients"). Submitting here
    /// keeps the process on one API.
    pub fn submit_notification(content: &NotificationContent) -> Result<(), String> {
        if !has_bundle_identity() {
            return Err("notifications require a bundled .app".into());
        }

        let identifier = format!("ferryx-{}", std::process::id());
        let dispatched = objc2::exception::catch(std::panic::AssertUnwindSafe(|| {
            let native = unsafe { UNMutableNotificationContent::new() };
            unsafe {
                native.setTitle(&NSString::from_str(&content.title));
                native.setBody(&NSString::from_str(&content.body));
            }
            let request = unsafe {
                UNNotificationRequest::requestWithIdentifier_content_trigger(
                    &NSString::from_str(&identifier),
                    &native,
                    None,
                )
            };
            let center = UNUserNotificationCenter::currentNotificationCenter();
            unsafe { center.addNotificationRequest_withCompletionHandler(&request, None) };
        }));

        if dispatched.is_err() {
            return Err("UNUserNotificationCenter submission raised an exception".into());
        }
        Ok(())
    }

    pub fn has_bundle_identity() -> bool {
        // Only consider it a real bundled macOS app if the bundle identifier is present
        // AND the bundle path is actually inside a .app wrapper directory.
        if let Some(bundle_id) = NSBundle::mainBundle().bundleIdentifier() {
            if bundle_id.to_string().is_empty() {
                return false;
            }
            let path_str = NSBundle::mainBundle().bundlePath().to_string();
            return path_str.ends_with(".app") || path_str.contains(".app/");
        }
        false
    }

    pub fn dev_fallback_status() -> NotificationPermissionStatusDto {
        NotificationPermissionStatusDto {
            platform: NotificationPlatform::Macos,
            supported: true,
            authorization: NotificationAuthorization::Authorized,
            alerts_enabled: Some(true),
            sounds_enabled: Some(true),
            requested: true,
            authoritative: false,
            can_open_settings: true,
        }
    }

    fn read_settings(settings: &UNNotificationSettings) -> NotificationPermissionStatusDto {
        let authorization = map_authorization_status(settings.authorizationStatus());
        NotificationPermissionStatusDto {
            platform: NotificationPlatform::Macos,
            supported: true,
            authorization,
            alerts_enabled: map_notification_setting(settings.alertSetting()),
            sounds_enabled: map_notification_setting(settings.soundSetting()),
            requested: authorization != NotificationAuthorization::NotDetermined,
            authoritative: true,
            can_open_settings: true,
        }
    }

    fn query_settings() -> Option<NotificationPermissionStatusDto> {
        if !has_bundle_identity() {
            return None;
        }

        let (tx, rx) = mpsc::sync_channel::<NotificationPermissionStatusDto>(1);
        let handler = RcBlock::new(move |settings: NonNull<UNNotificationSettings>| {
            let status = read_settings(unsafe { settings.as_ref() });
            let _ = tx.try_send(status);
        });

        let dispatched = objc2::exception::catch(std::panic::AssertUnwindSafe(|| {
            let center = UNUserNotificationCenter::currentNotificationCenter();
            center.getNotificationSettingsWithCompletionHandler(&handler);
        }));

        if dispatched.is_err() {
            tracing::warn!("UNUserNotificationCenter settings query raised an exception");
            return None;
        }

        match rx.recv_timeout(CALLBACK_TIMEOUT) {
            Ok(status) => Some(status),
            Err(_) => {
                tracing::warn!("UNUserNotificationCenter settings query timed out");
                None
            }
        }
    }

    #[derive(Debug, Default, Clone, Copy)]
    pub struct MacosPermissionProvider;

    impl NotificationPermissionProvider for MacosPermissionProvider {
        fn status(&self) -> NotificationPermissionStatusDto {
            query_settings().unwrap_or_else(dev_fallback_status)
        }

        fn request(&self) -> NotificationPermissionRequestDto {
            if !has_bundle_identity() {
                // In dev mode / unbundled mode, grant immediately so dev testing works
                return NotificationPermissionRequestDto {
                    granted: true,
                    status: dev_fallback_status(),
                    error: None,
                };
            }

            let (tx, rx) = mpsc::sync_channel::<Result<bool, String>>(1);
            let handler =
                RcBlock::new(move |granted: objc2::runtime::Bool, error: *mut NSError| {
                    let outcome = if error.is_null() {
                        Ok(granted.as_bool())
                    } else {
                        Err(unsafe { (*error).localizedDescription() }.to_string())
                    };
                    let _ = tx.try_send(outcome);
                });

            let options = UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound;
            let dispatched = objc2::exception::catch(std::panic::AssertUnwindSafe(|| {
                let center = UNUserNotificationCenter::currentNotificationCenter();
                center.requestAuthorizationWithOptions_completionHandler(options, &handler);
            }));

            if dispatched.is_err() {
                return NotificationPermissionRequestDto {
                    granted: false,
                    status: dev_fallback_status(),
                    error: Some("notification authorization request failed".into()),
                };
            }

            let (granted, error) = match rx.recv_timeout(CALLBACK_TIMEOUT) {
                Ok(Ok(granted)) => (granted, None),
                Ok(Err(message)) => (false, Some(message)),
                Err(_) => (false, Some("authorization request timed out".to_string())),
            };

            let status = query_settings().unwrap_or_else(dev_fallback_status);

            NotificationPermissionRequestDto {
                granted,
                status,
                error,
            }
        }
    }
}
