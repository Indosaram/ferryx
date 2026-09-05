use ferryx_lib::permissions::{
    check_accessibility, check_full_disk_access, get_system_permissions_status,
    open_system_settings_for_target,
};

#[test]
fn permissions_contract_reports_consistent_aggregate_status() {
    let _ = check_full_disk_access();
    let _ = check_accessibility();

    let status = get_system_permissions_status();
    #[cfg(target_os = "macos")]
    assert_eq!(status.platform, "macos");
    #[cfg(target_os = "windows")]
    assert_eq!(status.platform, "windows");
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    assert_eq!(status.platform, "linux");

    #[cfg(target_os = "macos")]
    let expected_all_granted = status.full_disk_access.granted
        && status.accessibility.granted
        && status.notifications.granted;
    #[cfg(not(target_os = "macos"))]
    let expected_all_granted = status.notifications.granted;
    assert_eq!(status.all_granted, expected_all_granted);
}

#[test]
fn permissions_open_settings_handles_known_and_unknown_targets() {
    let invalid = open_system_settings_for_target("malformed_target_injection");
    assert!(!invalid.opened);
    assert!(invalid.reason.is_some());
    assert!(invalid
        .reason
        .as_deref()
        .unwrap_or_default()
        .contains("unknown settings target"));

    #[cfg(target_os = "macos")]
    {
        for target in ["full_disk_access", "accessibility", "notifications"] {
            let res = open_system_settings_for_target(target);
            if let Some(reason) = res.reason.as_deref() {
                assert!(!reason.contains("unknown settings target"));
            }
        }
    }
}
