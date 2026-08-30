use serde_json::Value;

const TAURI_CONF: &str = include_str!("../tauri.conf.json");
const TAURI_WINDOWS_CONF: &str = include_str!("../tauri.windows.conf.json");

#[test]
fn windows_main_window_is_explicitly_opaque() {
    let shared_config: Value = serde_json::from_str(TAURI_CONF).expect("parse tauri.conf.json");
    let config: Value =
        serde_json::from_str(TAURI_WINDOWS_CONF).expect("parse tauri.windows.conf.json");
    let main_window = &config["app"]["windows"][0];

    assert_eq!(
        shared_config["app"]["windows"][0]["transparent"], true,
        "the shared configuration must preserve the transparent macOS native-surface cutout"
    );
    assert_eq!(main_window["label"], "main");
    assert_eq!(
        main_window["transparent"], false,
        "Windows must override the shared transparent window so WebView2 cannot reveal the desktop"
    );
}
