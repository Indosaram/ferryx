use std::process::Command;

// Compile-time dependencies must live in the source tree, not agent evidence.
const EDGE_WRAPPER: &str = include_str!("../../scripts/fixtures/run-edge-probes.mjs");
const EDGE_DRIVER: &str = include_str!("../../scripts/fixtures/probe-daemon-edges.mjs");

#[test]
fn edge_wrapper_stages_and_cleans_its_protocol_driver() {
    assert!(!EDGE_WRAPPER.is_empty());
    assert!(!EDGE_DRIVER.is_empty());
    let wrapper = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../scripts/fixtures/run-edge-probes.mjs");
    let output = Command::new("node")
        .arg(wrapper)
        .output()
        .expect("execute portable owned edge fixture with Node");
    assert!(
        output.status.success(),
        "fixture failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "EDGE_FIXTURE_STAGED_EXECUTED_CLEANED"
    );
}
