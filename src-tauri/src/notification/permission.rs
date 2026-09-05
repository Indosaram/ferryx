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

/// Invalidate any cached permission status so next query is fresh.
#[cfg(target_os = "macos")]
pub fn invalidate_permission_cache() {
    macos::invalidate_status_cache();
}

#[cfg(not(target_os = "macos"))]
pub fn invalidate_permission_cache() {}

/// Non-authoritative provider for platforms without a real permission query.
#[derive(Debug, Default, Clone, Copy)]
pub struct DesktopFallbackPermissionProvider;

impl NotificationPermissionProvider for DesktopFallbackPermissionProvider {
    fn status(&self) -> NotificationPermissionStatusDto {
        let platform = NotificationPlatform::current();
        let can_open_settings = matches!(platform, NotificationPlatform::Windows);
        NotificationPermissionStatusDto::non_authoritative(platform, can_open_settings)
    }

    fn request(&self) -> NotificationPermissionRequestDto {
        NotificationPermissionRequestDto {
            granted: false,
            status: self.status(),
            error: None,
        }
    }
}

#[cfg(target_os = "macos")]
pub mod macos {
    pub use super::super::macos_submission::submit_notification;
    use super::*;
    use block2::RcBlock;
    use objc2_foundation::{NSBundle, NSError};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNAuthorizationStatus, UNNotificationSetting, UNNotificationSettings,
        UNUserNotificationCenter,
    };
    use std::ptr::NonNull;
    use std::sync::mpsc;
    use std::time::Duration;

    const CALLBACK_TIMEOUT: Duration = Duration::from_secs(5);
    const STATUS_CACHE_TTL: Duration = Duration::from_secs(10);
    static STATUS_CACHE: std::sync::Mutex<
        Option<(NotificationPermissionStatusDto, std::time::Instant)>,
    > = std::sync::Mutex::new(None);

    pub fn invalidate_status_cache() {
        if let Ok(mut guard) = STATUS_CACHE.lock() {
            *guard = None;
        }
    }

    /// Exact `UNAuthorizationOptions` bit set passed to
    /// `requestAuthorizationWithOptions_completionHandler` by [`MacosPermissionProvider::request`].
    ///
    /// Exposed so tests can assert the real machine-consumed option bits (not source prose).
    pub fn authorization_options() -> UNAuthorizationOptions {
        UNAuthorizationOptions::Alert
            | UNAuthorizationOptions::Sound
            | UNAuthorizationOptions::Badge
    }

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
        if has_bundle_identity() {
            NotificationPermissionStatusDto::non_authoritative(NotificationPlatform::Macos, true)
        } else {
            NotificationPermissionStatusDto::unsupported(NotificationPlatform::Macos)
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
            if let Ok(guard) = STATUS_CACHE.lock() {
                if let Some((cached, instant)) = guard.as_ref() {
                    if instant.elapsed() < STATUS_CACHE_TTL {
                        return cached.clone();
                    }
                }
            }

            let fresh = query_settings().unwrap_or_else(dev_fallback_status);
            if let Ok(mut guard) = STATUS_CACHE.lock() {
                *guard = Some((fresh.clone(), std::time::Instant::now()));
            }
            fresh
        }

        fn request(&self) -> NotificationPermissionRequestDto {
            if !has_bundle_identity() {
                return NotificationPermissionRequestDto {
                    granted: false,
                    status: dev_fallback_status(),
                    error: Some("notifications require a bundled .app".into()),
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

            let options = authorization_options();
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

            // Invalidate permission cache so next query reads authoritative OS status
            if let Ok(mut guard) = STATUS_CACHE.lock() {
                *guard = None;
            }

            NotificationPermissionRequestDto {
                granted,
                status,
                error,
            }
        }
    }
}
