use serde_json::Value;

const TAURI_CONF: &str = include_str!("../tauri.conf.json");

#[test]
fn clean_dev_builds_bundled_remote_assets_before_starting_vite() {
    let config: Value = serde_json::from_str(TAURI_CONF).expect("parse tauri.conf.json");

    assert_eq!(
        config["build"]["beforeDevCommand"],
        "bun scripts/dev-frontend.mjs",
        "clean dev must use the owned frontend runner before tauri-build validates bundled remote assets"
    );
    assert_eq!(
        config["bundle"]["resources"]["../ui/dist"],
        "ui/dist",
        "release bundles must continue shipping the remote SPA"
    );
}
