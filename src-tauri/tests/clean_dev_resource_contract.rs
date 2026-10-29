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
        config["bundle"]["resources"]["../ui/dist"], "ui/dist",
        "release bundles must continue shipping the remote SPA"
    );
}

#[test]
fn every_package_bundles_the_fetched_mhc_payload() {
    let config: Value = serde_json::from_str(TAURI_CONF).expect("parse tauri.conf.json");

    assert_eq!(
        config["bundle"]["resources"]["resources/mhc/payload"], "mhc",
        "the hub resolves the bundled mhc from the `mhc` resource dir"
    );
    let before_build = config["build"]["beforeBuildCommand"].as_str().unwrap_or_default();
    assert!(
        before_build.starts_with("bun scripts/fetch-mhc.mjs --from-env --prune && "),
        "release builds fetch and verify the pinned mhc for their target first: {before_build}"
    );
    // The resource dir must exist in a fresh clone, or tauri-build fails before anything is fetched.
    let payload = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/mhc/payload/.gitkeep");
    assert!(payload.is_file(), "{} must be committed", payload.display());
}
