use serde_json::Value;

const TAURI_CONF: &str = include_str!("../tauri.conf.json");
const TAURI_MACOS_CONF: &str = include_str!("../tauri.macos.conf.json");
const DEV_RUNNER: &str = include_str!("../../scripts/macos-dev-runner.sh");

#[test]
fn base_tauri_config_omits_platform_specific_runner() {
    let config: Value = serde_json::from_str(TAURI_CONF).expect("parse tauri.conf.json");

    assert!(
        config["build"].get("runner").is_none(),
        "the base config must not run a macOS shell script on Windows or Linux"
    );
}

#[test]
fn development_runner_launches_named_macos_app_bundle() {
    let macos_config: Value =
        serde_json::from_str(TAURI_MACOS_CONF).expect("parse tauri.macos.conf.json");

    assert_eq!(
        macos_config["build"]["runner"],
        "../scripts/macos-dev-runner.sh"
    );
    assert!(
        DEV_RUNNER.contains("APP_DIR=\"$TARGET_DIR/Ferryx.app\""),
        "the development runner must create a Ferryx.app bundle for the Dock"
    );
    assert!(
        DEV_RUNNER.contains("install_atomic \"$TARGET_DIR/ferryx\" \"$MACOS_DIR/ferryx\""),
        "the development runner must place the binary inside Ferryx.app"
    );
    assert!(
        !DEV_RUNNER.contains("cp -f \"$TARGET_DIR/ferryx\" \"$MACOS_DIR/ferryx\""),
        "the binary must not be rewritten in place: mutating a running executable's pages makes the \
         kernel invalidate its code signature and SIGKILL the running app"
    );
    assert!(
        DEV_RUNNER.contains("mv -f \"$tmp\" \"$dest\""),
        "install_atomic must publish through a rename so a running Ferryx keeps its inode"
    );
    assert!(
        DEV_RUNNER.contains("exec \"$MACOS_DIR/ferryx\""),
        "the development runner must launch from the named app bundle"
    );
}
