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
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let (mut command, binary) = cli_build_command(
        &cargo,
        &exe,
        Path::new(env!("CARGO_MANIFEST_DIR")),
        env!("FERRYX_BUILD_TARGET"),
    );
    let status = command.status().expect("spawn cargo build for ferryx-cli");
    assert!(status.success(), "cargo build --bin ferryx-cli failed: {status}");
    assert!(binary.is_file(), "ferryx-cli missing after build at {}", binary.display());
    binary
}

fn cli_build_command(
    cargo: &std::ffi::OsStr,
    exe: &Path,
    manifest_dir: &Path,
    build_target: &str,
) -> (std::process::Command, PathBuf) {
    // Native: <target-dir>/<profile>/deps/<test-binary>
    // Explicit target: <target-dir>/<triple>/<profile>/deps/<test-binary>
    let profile_dir = exe
        .parent()
        .and_then(Path::parent)
        .expect("test binary lives in <target-dir>/[<triple>/]<profile>/deps");
    let profile_parent = profile_dir.parent().expect("profile directory has a parent");
    // Compare with Cargo's actual compile target, not a guess based on hyphens or
    // architecture names in arbitrary directories. Otherwise --target-dir gains a
    // second triple component and the complete dependency graph builds again.
    let explicit_target = profile_parent.file_name() == Some(std::ffi::OsStr::new(build_target));
    let target_dir = if explicit_target {
        profile_parent.parent().expect("target triple directory has a parent")
    } else {
        profile_parent
    };
    let profile = profile_dir.file_name().expect("profile directory has a name");
    let profile = if profile == "debug" { std::ffi::OsStr::new("dev") } else { profile };
    let mut command = std::process::Command::new(cargo);
    command
        .args(["build", "--bin", "ferryx-cli", "--manifest-path"])
        .arg(manifest_dir.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(target_dir)
        .arg("--profile")
        .arg(profile);
    if explicit_target {
        command.arg("--target").arg(build_target);
    } else {
        // A runtime override must not redirect a native test's helper elsewhere.
        command.env_remove("CARGO_BUILD_TARGET");
    }
    let binary = profile_dir.join(format!("ferryx-cli{}", std::env::consts::EXE_SUFFIX));
    (command, binary)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_layout(exe: &str, expected_root: &str, expected_profile: &str, explicit: bool) {
        let triple = "aarch64-apple-darwin";
        let (command, binary) = cli_build_command(
            std::ffi::OsStr::new("cargo"), Path::new(exe), Path::new("checkout/src-tauri"), triple,
        );
        let mut expected = vec![
            "build", "--bin", "ferryx-cli", "--manifest-path", "checkout/src-tauri/Cargo.toml",
            "--target-dir", expected_root, "--profile", expected_profile,
        ];
        if explicit { expected.extend(["--target", triple]); }
        let mut expected: Vec<std::ffi::OsString> = expected.into_iter().map(Into::into).collect();
        expected[4] = Path::new("checkout/src-tauri").join("Cargo.toml").into_os_string();
        assert_eq!(command.get_args().collect::<Vec<_>>(),
            expected.iter().map(std::ffi::OsString::as_os_str).collect::<Vec<_>>());
        assert_eq!(binary, Path::new(exe).parent().unwrap().parent().unwrap()
            .join(format!("ferryx-cli{}", std::env::consts::EXE_SUFFIX)));
        assert_eq!(command.get_envs().any(|(key, value)| key == "CARGO_BUILD_TARGET" && value.is_none()), !explicit);
    }

    #[test]
    fn native_debug_reuses_the_existing_target_root() {
        assert_layout("target/debug/deps/tests", "target", "dev", false);
    }

    #[test]
    fn explicit_target_does_not_nest_or_rebuild_as_host() {
        assert_layout("target/aarch64-apple-darwin/debug/deps/tests", "target", "dev", true);
    }

    #[test]
    fn explicit_release_preserves_profile_and_binary_location() {
        assert_layout("shared/aarch64-apple-darwin/release/deps/tests", "shared", "release", true);
    }

    #[test]
    fn custom_profile_and_target_directory_names_are_not_guessed_as_triples() {
        assert_layout("my-target-dir/custom/deps/tests", "my-target-dir", "custom", false);
    }
}
