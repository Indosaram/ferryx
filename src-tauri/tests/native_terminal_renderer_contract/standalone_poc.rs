//! Contract test for Phase 2 compiled standalone native-window POC binary command interface.

fn poc_binary_path() -> std::path::PathBuf {
    let bin_path = option_env!("CARGO_BIN_EXE_native_terminal_renderer_poc")
        .or(option_env!("CARGO_BIN_EXE_standalone_renderer_poc"))
        .or(option_env!("CARGO_BIN_EXE_renderer_poc"))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
            let profile = if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            };
            let target_bin = std::env::var_os("CARGO_TARGET_DIR")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| manifest_dir.join("target"))
                .join(profile)
                .join("native_terminal_renderer_poc");
            target_bin
        });

    if !bin_path.exists() {
        panic!(
            "Standalone renderer POC binary target 'native_terminal_renderer_poc' is absent at {:?}",
            bin_path
        );
    }

    bin_path
}

#[test]
fn test_standalone_renderer_poc_binary_help_flags() {
    let bin_path = poc_binary_path();

    let output = std::process::Command::new(&bin_path)
        .arg("--help")
        .output()
        .expect("execute standalone renderer poc binary with --help");

    assert!(
        output.status.success(),
        "binary --help must exit successfully"
    );

    let stdout_and_stderr = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        stdout_and_stderr.contains("--window"),
        "machine-consumed --help output must contain '--window' flag token"
    );
    assert!(
        stdout_and_stderr.contains("--headless"),
        "machine-consumed --help output must contain '--headless' flag token"
    );
    assert!(
        stdout_and_stderr.contains("--output"),
        "machine-consumed --help output must contain '--output' flag token"
    );
}

#[test]
fn test_standalone_renderer_poc_binary_rejects_unknown_option() {
    let bin_path = poc_binary_path();

    let output = std::process::Command::new(&bin_path)
        .arg("--unknown-flag")
        .output()
        .expect("execute standalone renderer poc binary with unknown flag");

    let stdout_and_stderr = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        !output.status.success(),
        "binary must reject unknown option with nonzero exit status, got: {:?}",
        output.status
    );
    assert!(
        stdout_and_stderr.contains("Unknown option"),
        "error output must contain 'Unknown option', got: {}",
        stdout_and_stderr
    );
    assert!(
        !stdout_and_stderr.contains("Ferryx Native WGPU Terminal Renderer POC (Headless)"),
        "binary must not execute headless render when given unknown option"
    );
}
