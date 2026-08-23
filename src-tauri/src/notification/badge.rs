//! Application badge domain logic and native Dock tile updates.
//!
//! Exposes typed badge count synchronization for the macOS Dock tile.
//! On non-macOS platforms, operations return a structured unsupported outcome
//! without failing. Never accepts arbitrary user-controlled strings over IPC.

use serde::{Deserialize, Serialize};

/// Structured result of an application badge count update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetBadgeCountResult {
    pub supported: bool,
    pub count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub badge_label: Option<String>,
}

impl SetBadgeCountResult {
    pub fn macos(count: u32, badge_label: Option<String>) -> Self {
        Self {
            supported: true,
            count,
            badge_label,
        }
    }

    pub fn unsupported(count: u32) -> Self {
        Self {
            supported: false,
            count,
            badge_label: None,
        }
    }
}

/// Format an unread count into an application badge label.
///
/// Returns `Some(decimal_string)` when count > 0, and `None` when count == 0.
pub fn format_badge_label(count: u32) -> Option<String> {
    if count == 0 {
        None
    } else {
        Some(count.to_string())
    }
}

#[cfg(target_os = "macos")]
pub mod macos_impl {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;
    use objc2_foundation::NSString;

    /// Apply badge label to macOS dock tile.
    ///
    /// Must be invoked on the macOS main thread where `MainThreadMarker` can be acquired.
    pub fn apply_dock_badge_label(label: Option<&str>) -> Result<(), String> {
        let label_ns = label.map(NSString::from_str);

        let res = objc2::exception::catch(std::panic::AssertUnwindSafe(|| {
            let Some(mtm) = MainThreadMarker::new() else {
                return Err("must be called on the macOS main thread".to_string());
            };
            let app = NSApplication::sharedApplication(mtm);
            let dock_tile = app.dockTile();
            dock_tile.setBadgeLabel(label_ns.as_deref());
            Ok(())
        }));

        match res {
            Ok(inner) => inner,
            Err(_) => Err("objc exception raised while setting Dock badge".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn format_badge_label_when_count_is_positive_returns_decimal_string() {
        // Given: positive counts
        // When: formatting the badge label
        // Then: exact decimal string is returned
        assert_eq!(format_badge_label(1), Some("1".to_string()));
        assert_eq!(format_badge_label(42), Some("42".to_string()));
        assert_eq!(format_badge_label(999), Some("999".to_string()));
        assert_eq!(format_badge_label(10_000), Some("10000".to_string()));
    }

    #[test]
    fn format_badge_label_when_count_is_zero_returns_none() {
        // Given: count = 0
        // When: formatting the badge label
        // Then: None is returned to clear the badge
        assert_eq!(format_badge_label(0), None);
    }

    #[test]
    fn set_badge_count_result_constructors() {
        // Given: macOS result
        let macos_res = SetBadgeCountResult::macos(5, Some("5".into()));
        assert!(macos_res.supported);
        assert_eq!(macos_res.count, 5);
        assert_eq!(macos_res.badge_label.as_deref(), Some("5"));

        // Given: unsupported platform result
        let unsupp_res = SetBadgeCountResult::unsupported(5);
        assert!(!unsupp_res.supported);
        assert_eq!(unsupp_res.count, 5);
        assert!(unsupp_res.badge_label.is_none());
    }

    #[test]
    fn set_badge_count_result_serializes_with_camel_case() {
        // Given: positive count result
        let result = SetBadgeCountResult::macos(3, Some("3".into()));

        // When: serializing to JSON
        let value = serde_json::to_value(&result).expect("serialize");

        // Then: camelCase properties with badgeLabel present
        assert_eq!(
            value,
            json!({
                "supported": true,
                "count": 3,
                "badgeLabel": "3"
            })
        );
    }

    #[test]
    fn set_badge_count_result_cleared_omits_none_badge_label() {
        // Given: cleared count result
        let result = SetBadgeCountResult::macos(0, None);

        // When: serializing to JSON
        let value = serde_json::to_value(&result).expect("serialize");

        // Then: badgeLabel is omitted
        assert_eq!(
            value,
            json!({
                "supported": true,
                "count": 0
            })
        );
    }

    #[test]
    fn set_badge_count_result_unsupported_serializes_cleanly() {
        // Given: unsupported platform result
        let result = SetBadgeCountResult::unsupported(12);

        // When: serializing to JSON
        let value = serde_json::to_value(&result).expect("serialize");

        // Then: supported is false and badgeLabel is omitted
        assert_eq!(
            value,
            json!({
                "supported": false,
                "count": 12
            })
        );
    }

    #[test]
    fn set_badge_count_result_deserializes_from_json() {
        // Given: JSON payloads
        let payload_with_badge = json!({
            "supported": true,
            "count": 9,
            "badgeLabel": "9"
        });
        let payload_without_badge = json!({
            "supported": false,
            "count": 9
        });

        // When: deserializing
        let res1: SetBadgeCountResult =
            serde_json::from_value(payload_with_badge).expect("deserialize");
        let res2: SetBadgeCountResult =
            serde_json::from_value(payload_without_badge).expect("deserialize");

        // Then: typed values match
        assert_eq!(res1, SetBadgeCountResult::macos(9, Some("9".into())));
        assert_eq!(res2, SetBadgeCountResult::unsupported(9));
    }
}
