//! Application badge domain logic and native Dock tile updates.
//!
//! Exposes typed badge count synchronization for the macOS Dock tile, and
//! records the Windows taskbar overlay count without claiming it was drawn.
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

    /// Windows taskbar overlay badge outcome.
    ///
    /// The taskbar overlay icon is drawn by the shell COM interface
    /// `ITaskbarList3::SetOverlayIcon`. That interface lives behind the
    /// `windows-sys` `Win32_UI_Shell` feature, which this crate does not enable,
    /// so nothing can be drawn from here today. This constructor therefore stays
    /// honest rather than optimistic: it records the count and the label the
    /// taskbar would show, and reports `supported: false` instead of a success
    /// that never reached the shell.
    ///
    /// TODO(windows-badge): enable `Win32_UI_Shell` in the `windows-sys` feature
    /// list in `src-tauri/Cargo.toml`, then create the taskbar instance with
    /// `CoCreateInstance(CLSID_TaskbarList, IID_ITaskbarList3)` against the main
    /// window `HWND`, build the overlay `HICON` from `badge_label` (passing
    /// `None` to clear the overlay), call
    /// `ITaskbarList3::SetOverlayIcon(hwnd, icon, description)`, and only then
    /// flip `supported` to `true`.
    pub fn windows(count: u32, badge_label: Option<String>) -> Self {
        Self {
            supported: false,
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
    fn windows_badge_records_count_and_label_without_claiming_support() {
        // Given: an unread count on Windows
        // When: building the Windows taskbar overlay outcome
        let recorded = SetBadgeCountResult::windows(4, format_badge_label(4));

        // Then: the count and label are recorded for the pending shell call,
        // and support is not claimed because no overlay icon was drawn
        assert!(!recorded.supported);
        assert_eq!(recorded.count, 4);
        assert_eq!(recorded.badge_label.as_deref(), Some("4"));
        assert_eq!(
            serde_json::to_value(&recorded).expect("serialize"),
            json!({
                "supported": false,
                "count": 4,
                "badgeLabel": "4"
            })
        );

        // Given: the full u32 range and a cleared count
        // When: building the Windows taskbar overlay outcome
        let widest = SetBadgeCountResult::windows(u32::MAX, format_badge_label(u32::MAX));
        let cleared = SetBadgeCountResult::windows(0, format_badge_label(0));

        // Then: the recorded label is never truncated, and zero clears it
        assert_eq!(widest.badge_label.as_deref(), Some("4294967295"));
        assert!(!widest.supported);
        assert_eq!(cleared.count, 0);
        assert_eq!(cleared.badge_label, None);
        assert!(!cleared.supported);
    }

    #[test]
    fn badge_constructor_selection_tracks_the_host_platform() {
        // Given: the unread count the IPC command receives
        let count = 3;
        let label = format_badge_label(count);

        // When: selecting the constructor for this host
        #[cfg(target_os = "macos")]
        let selected = SetBadgeCountResult::macos(count, label.clone());
        #[cfg(not(target_os = "macos"))]
        let selected = SetBadgeCountResult::windows(count, label.clone());

        // Then: only macOS claims a native badge; every other host records
        // the count while reporting the structured unsupported outcome
        #[cfg(target_os = "macos")]
        assert!(selected.supported);
        #[cfg(not(target_os = "macos"))]
        assert!(!selected.supported);
        assert_eq!(selected.count, count);
        assert_eq!(selected.badge_label, label);

        // And: the generic unsupported constructor never carries a label
        assert!(SetBadgeCountResult::unsupported(count).badge_label.is_none());
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
