use serde_json::Value;

const TAURI_CONF: &str = include_str!("../tauri.conf.json");
const DEV_RUNNER: &str = include_str!("../../scripts/macos-dev-runner.sh");

#[test]
fn development_runner_launches_named_macos_app_bundle() {
    let config: Value = serde_json::from_str(TAURI_CONF).expect("parse tauri.conf.json");

    assert_eq!(config["build"]["runner"], "../scripts/macos-dev-runner.sh");
    assert!(
        DEV_RUNNER.contains("APP_DIR=\"$TARGET_DIR/Ferryx.app\""),
        "the development runner must create a Ferryx.app bundle for the Dock"
    );
    assert!(
        DEV_RUNNER.contains("cp -f \"$TARGET_DIR/ferryx\" \"$MACOS_DIR/ferryx\""),
        "the development runner must copy the binary inside Ferryx.app"
    );
    assert!(
        DEV_RUNNER.contains("exec \"$MACOS_DIR/ferryx\""),
        "the development runner must launch from the named app bundle"
    );
}
