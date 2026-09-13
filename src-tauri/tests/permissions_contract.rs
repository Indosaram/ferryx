use ferryx_lib::permissions::{
    get_system_permissions_status, open_system_settings_for_target_with_launcher,
};
#[cfg(not(target_os = "macos"))]
use ferryx_lib::permissions::{request_accessibility, PermissionStatus};

#[test]
fn permissions_contract_reports_consistent_aggregate_status() {
    let status = get_system_permissions_status();
    #[cfg(target_os = "macos")]
    {
        assert_eq!(status.platform, "macos");
        assert!(status.accessibility.can_request);
        assert_eq!(
            status.all_granted,
            status.full_disk_access.granted
                && status.accessibility.granted
                && status.notifications.granted
        );
    }
    #[cfg(not(target_os = "macos"))]
    {
        assert_eq!(
            status.platform,
            if cfg!(target_os = "windows") {
                "windows"
            } else {
                "linux"
            }
        );
        assert_eq!(status.all_granted, status.notifications.granted);
        for item in [&status.full_disk_access, &status.accessibility] {
            assert_eq!(item.status, PermissionStatus::Unsupported);
            assert!(!item.granted);
            assert!(!item.can_open_settings);
            assert!(!item.can_request);
        }
        assert_eq!(status.notifications.status, PermissionStatus::Unknown);
        assert!(!status.notifications.granted);
        assert!(!status.notifications.can_request);
        assert_eq!(
            status.notifications.can_open_settings,
            cfg!(target_os = "windows")
        );
    }
}

#[cfg(not(target_os = "macos"))]
#[test]
fn unsupported_accessibility_request_does_not_report_success() {
    assert!(!request_accessibility());
}

#[test]
fn permissions_unknown_settings_target_never_launches() {
    for target in [
        "",
        "malformed_target_injection",
        "notifications;echo marker",
    ] {
        let result = open_system_settings_for_target_with_launcher(target, |_, _| {
            panic!("unsupported target must not launch")
        });
        assert!(!result.opened);
        assert_eq!(result.target, target);
        assert!(result.reason.is_some());
    }
}

#[test]
fn permissions_known_settings_targets_preserve_launch_contract() {
    for target in [
        "full_disk_access",
        "fullDiskAccess",
        "accessibility",
        "notifications",
    ] {
        let expected: Option<(&str, Vec<&str>)> = if cfg!(target_os = "macos") {
            let uri = match target {
                "full_disk_access" | "fullDiskAccess" => {
                    "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles"
                }
                "accessibility" => {
                    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
                }
                "notifications" => "x-apple.systempreferences:com.apple.preference.notifications",
                _ => unreachable!(),
            };
            Some(("open", vec![uri]))
        } else if cfg!(target_os = "windows") && target == "notifications" {
            Some(("cmd", vec!["/C", "start", "", "ms-settings:notifications"]))
        } else {
            None
        };
        for succeeded in [false, true] {
            let mut calls = Vec::new();
            let result = open_system_settings_for_target_with_launcher(target, |program, args| {
                calls.push((
                    program.to_string(),
                    args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>(),
                ));
                succeeded
            });
            let expected_calls: Vec<_> = expected
                .iter()
                .map(|(program, args)| {
                    (
                        program.to_string(),
                        args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>(),
                    )
                })
                .collect();
            assert_eq!(calls, expected_calls);
            assert_eq!(result.target, target);
            assert_eq!(result.opened, expected.is_some() && succeeded);
            assert_eq!(result.reason.is_some(), !result.opened);
        }
    }
}
