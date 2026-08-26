use serde_json::Value;

const TAURI_CONF: &str = include_str!("../tauri.conf.json");
const DEFAULT_CAPABILITY: &str = include_str!("../capabilities/default.json");
const CARGO_MANIFEST: &str = include_str!("../Cargo.toml");
const LIB_SOURCE: &str = include_str!("../src/lib.rs");

// Public half of the release signing key. The private half never enters the repository; CI receives
// it through the TAURI_SIGNING_PRIVATE_KEY secret.
const UPDATER_PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDEwQzk5REI1QzI1QzY3Q0IKUldUTFoxekN0WjNKRU8zWGhqWlo2VXEzclF0RXoyRmJCY2Z4eGwvK2FGbE5LSmVwcW9RTmoyWm0K";

const UPDATER_ENDPOINT: &str =
    "https://github.com/Indosaram/ferryx/releases/latest/download/latest.json";

fn tauri_config() -> Value {
    serde_json::from_str(TAURI_CONF).expect("parse tauri.conf.json")
}

#[test]
fn updater_plugin_config_pins_endpoint_and_signing_key() {
    let config = tauri_config();
    let updater = &config["plugins"]["updater"];

    assert!(
        updater.is_object(),
        "tauri.conf.json must configure plugins.updater"
    );
    assert_eq!(
        updater["pubkey"], UPDATER_PUBKEY,
        "plugins.updater.pubkey must be the release signing public key"
    );

    let endpoints = updater["endpoints"]
        .as_array()
        .expect("plugins.updater.endpoints must be an array");
    assert_eq!(
        endpoints.len(),
        1,
        "exactly one update manifest endpoint is published"
    );
    assert_eq!(endpoints[0], UPDATER_ENDPOINT);
}

#[test]
fn updater_installs_without_prompting_on_windows() {
    let config = tauri_config();
    assert_eq!(
        config["plugins"]["updater"]["windows"]["installMode"], "passive",
        "the Windows installer runs passively so an update needs no extra user interaction"
    );
}

#[test]
fn bundler_emits_updater_artifacts() {
    let config = tauri_config();
    assert_eq!(
        config["bundle"]["createUpdaterArtifacts"], true,
        "bundle.createUpdaterArtifacts must be true or no signed .sig artifacts are produced"
    );
}

#[test]
fn default_capability_grants_updater_commands() {
    let capability: Value =
        serde_json::from_str(DEFAULT_CAPABILITY).expect("parse capabilities/default.json");
    let permissions = capability["permissions"]
        .as_array()
        .expect("capabilities/default.json must list permissions");

    assert!(
        permissions.iter().any(|value| value == "updater:default"),
        "the main webview needs updater:default to call the updater commands, got {permissions:?}"
    );
    assert!(
        permissions.iter().any(|value| value == "process:default"),
        "the main webview needs process:default to relaunch after installing an update, got {permissions:?}"
    );
}

#[test]
fn updater_plugin_is_a_dependency_and_is_registered() {
    assert!(
        CARGO_MANIFEST.contains("tauri-plugin-updater"),
        "src-tauri/Cargo.toml must depend on tauri-plugin-updater"
    );
    assert!(
        LIB_SOURCE.contains("tauri_plugin_updater"),
        "src-tauri/src/lib.rs must register the updater plugin on the Tauri builder"
    );
}
