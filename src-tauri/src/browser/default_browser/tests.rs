use super::*;

#[test]
fn inbound_url_accepts_one_http_or_https_argument() {
    assert_eq!(
        inbound_url_from_args(&["ferryx", "https://example.com/a?b=1"]).as_deref(),
        Some("https://example.com/a?b=1")
    );
    assert_eq!(
        inbound_url_from_args(&["ferryx", "-psn_0_123", "http://example.com"]).as_deref(),
        Some("http://example.com/")
    );
}

#[test]
fn inbound_url_ignores_commands_and_other_schemes() {
    assert_eq!(inbound_url_from_args(&["ferryx"]), None);
    assert_eq!(inbound_url_from_args(&["ferryx", "browser", "list"]), None);
    assert_eq!(
        inbound_url_from_args(&["ferryx", "file:///etc/passwd"]),
        None
    );
    assert_eq!(
        inbound_url_from_args(&["ferryx", "javascript:alert(1)"]),
        None
    );
    assert_eq!(
        inbound_url_from_args(&["ferryx", "https://a.com", "https://b.com"]),
        None
    );
    assert_eq!(
        inbound_url_from_args(&["ferryx", "--daemon", "https://a.com"]),
        None
    );
    assert_eq!(classify_inbound_url("https://"), None);
}

#[test]
fn windows_registration_lists_http_and_https_under_hkcu_capabilities() {
    let values = windows_registry_values(r"C:\Program Files\Ferryx\ferryx.exe");
    let find = |key: &str, name: Option<&str>| {
        values
            .iter()
            .find(|v| v.key == key && v.name.as_deref() == name)
            .map(|v| v.data.as_str())
    };
    let assoc = r"Software\Clients\StartMenuInternet\Ferryx\Capabilities\URLAssociations";
    assert_eq!(find(assoc, Some("http")), Some("FerryxURL"));
    assert_eq!(find(assoc, Some("https")), Some("FerryxURL"));
    assert_eq!(
        find(r"Software\RegisteredApplications", Some("Ferryx")),
        Some(r"Software\Clients\StartMenuInternet\Ferryx\Capabilities")
    );
    assert_eq!(
        find(r"Software\Classes\FerryxURL\shell\open\command", None),
        Some(r#""C:\Program Files\Ferryx\ferryx.exe" "%1""#)
    );
}

#[test]
fn reg_add_args_write_named_and_default_values() {
    let named = RegistryValue {
        key: r"Software\X".into(),
        name: Some("http".into()),
        data: "P".into(),
    };
    assert_eq!(
        windows_reg_add_args(&named),
        [
            "add",
            r"HKCU\Software\X",
            "/v",
            "http",
            "/t",
            "REG_SZ",
            "/d",
            "P",
            "/f"
        ]
    );
    let default = RegistryValue {
        key: r"Software\X".into(),
        name: None,
        data: "".into(),
    };
    assert_eq!(
        windows_reg_add_args(&default),
        [
            "add",
            r"HKCU\Software\X",
            "/ve",
            "/t",
            "REG_SZ",
            "/d",
            "",
            "/f"
        ]
    );
}

#[test]
fn reg_query_output_yields_the_prog_id() {
    let output =
        "\r\nHKEY_CURRENT_USER\\...\\UserChoice\r\n    ProgId    REG_SZ    ChromeHTML\r\n\r\n";
    assert_eq!(
        windows_prog_id_from_reg_query(output).as_deref(),
        Some("ChromeHTML")
    );
    assert_eq!(windows_prog_id_from_reg_query("ERROR: not found"), None);
    assert_eq!(
        windows_status_from_prog_ids(Some("FerryxURL"), Some("ferryxurl")),
        DefaultBrowserStatus::Default
    );
    assert_eq!(
        windows_status_from_prog_ids(Some("FerryxURL"), Some("ChromeHTML")),
        DefaultBrowserStatus::NotDefault {
            current: Some("ChromeHTML".into())
        }
    );
}

#[test]
fn linux_desktop_entry_claims_http_and_https_handlers() {
    let entry = linux_desktop_entry("/opt/Ferryx App/ferryx");
    assert!(entry.contains("MimeType=x-scheme-handler/http;x-scheme-handler/https;"));
    assert!(entry.contains("Exec=/opt/Ferryx\\sApp/ferryx %u\n"));
    let commands = linux_registration_commands();
    assert_eq!(commands[0].0, "xdg-settings");
    assert_eq!(
        commands[0].1,
        ["set", "default-web-browser", LINUX_DESKTOP_FILE]
    );
    assert_eq!(
        linux_status_from_xdg("ferryx-browser.desktop\n"),
        DefaultBrowserStatus::Default
    );
    assert_eq!(
        linux_status_from_xdg("firefox.desktop\n"),
        DefaultBrowserStatus::NotDefault {
            current: Some("firefox.desktop".into())
        }
    );
}

#[test]
fn macos_status_needs_both_schemes() {
    assert_eq!(
        macos_status_from_handlers(
            "com.ferryx.app",
            Some("com.Ferryx.App"),
            Some("com.ferryx.app")
        ),
        DefaultBrowserStatus::Default
    );
    assert_eq!(
        macos_status_from_handlers(
            "com.ferryx.app",
            Some("com.ferryx.app"),
            Some("com.apple.safari")
        ),
        DefaultBrowserStatus::NotDefault {
            current: Some("com.apple.safari".into())
        }
    );
    assert_eq!(
        macos_status_from_handlers("com.ferryx.app", None, None),
        DefaultBrowserStatus::NotDefault { current: None }
    );
}

#[test]
fn inbound_queue_drains_once() {
    queue_inbound_url("https://queued.example/".into());
    let drained = take_inbound_urls();
    assert!(drained.contains(&"https://queued.example/".to_string()));
    assert!(!take_inbound_urls().contains(&"https://queued.example/".to_string()));
}

#[test]
fn status_serializes_for_the_settings_row() {
    assert_eq!(
        serde_json::to_value(DefaultBrowserStatus::Default).unwrap(),
        serde_json::json!({ "state": "default" })
    );
    assert_eq!(
        serde_json::to_value(DefaultBrowserStatus::NotDefault { current: None }).unwrap(),
        serde_json::json!({ "state": "notDefault", "current": null })
    );
    assert_eq!(
        serde_json::to_value(MakeDefaultOutcome::OpenedSystemSettings).unwrap(),
        serde_json::json!({ "outcome": "openedSystemSettings" })
    );
}
