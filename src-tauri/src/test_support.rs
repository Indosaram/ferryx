//! Test-only helpers shared across suites.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Path to a `ferryx-cli` built from this checkout, building it on first use.
///
/// `cargo test --lib` does not build the crate's binaries, so suites that drive the
/// real CLI cannot assume one already sits in the target directory. The build reuses
/// the cargo and target directory that produced this test binary, so it is incremental
/// and the CLI always matches the code under test.
pub(crate) fn ferryx_cli_binary() -> &'static Path {
    static CLI: OnceLock<PathBuf> = OnceLock::new();
    CLI.get_or_init(build_ferryx_cli)
}

fn build_ferryx_cli() -> PathBuf {
    let exe = std::env::current_exe().expect("test binary path");
    // <target>/<profile>/deps/<test-binary>
    let profile_dir = exe
        .parent()
        .and_then(Path::parent)
        .expect("test binary lives in <target>/<profile>/deps");
    let target_dir = profile_dir.parent().expect("profile directory has a target parent");
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = std::process::Command::new(cargo)
        .args(["build", "--bin", "ferryx-cli", "--manifest-path"])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
        .arg("--target-dir")
        .arg(target_dir)
        .status()
        .expect("spawn cargo build for ferryx-cli");
    assert!(status.success(), "cargo build --bin ferryx-cli failed: {status}");
    let binary = profile_dir.join(format!("ferryx-cli{}", std::env::consts::EXE_SUFFIX));
    assert!(binary.is_file(), "ferryx-cli missing after build at {}", binary.display());
    binary
}
