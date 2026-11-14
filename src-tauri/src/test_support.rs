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
    let profile_dir = test_profile_dir(exe);
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
    // `cargo test` gives this process its package environment (CARGO_MANIFEST_DIR,
    // CARGO_PKG_*, OUT_DIR, ...). Build scripts such as ring's track those variables with
    // rerun-if-env-changed, so a nested cargo that inherits them sees them "changed"
    // and rebuilds ring and everything above it, and the next ordinary cargo run in the
    // same target dir rebuilds it all again. Cargo sets them per package itself.
    for (key, _) in std::env::vars_os() {
        if key.to_str().is_some_and(is_cargo_package_env) {
            command.env_remove(key);
        }
    }
    command
        // A plain `cargo build --bin` resolves features without dev-dependencies, so tokio
        // loses the `test-util` feature the test build has. That is a different tokio, and
        // every crate above it, ferryx_lib included, compiles a second time here; the next
        // test or example build in the same target dir then rebuilds its own units again.
        // Naming an example as well (one `cargo test` already compiles) resolves features
        // the way the test build does, so the CLI reuses the units the tests were built from.
        .args(["build", "--bin", "ferryx-cli", "--example", CLI_FEATURE_PEER_EXAMPLE])
        .arg("--manifest-path")
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

/// Cargo can execute tests from `deps`, or from a package's build output
/// directory. Do not mistake the package fingerprint for a Cargo profile.
fn test_profile_dir(exe: &Path) -> &Path {
    let directory = exe.parent().expect("test executable has a parent");
    if directory.file_name() == Some(std::ffi::OsStr::new("deps")) {
        return directory.parent().expect("deps lies under the profile directory");
    }
    if directory.file_name() == Some(std::ffi::OsStr::new("out")) {
        // Both Cargo layouts occur: build/<package>-<hash>/out and
        // build/<package>/<hash>/out. Match the build directory structurally,
        // not a hyphen pattern in arbitrary custom profile/target names.
        for depth in [2, 3] {
            if let Some(build) = directory.ancestors().nth(depth) {
                if build.file_name() == Some(std::ffi::OsStr::new("build")) {
                    return build.parent().expect("build lies under the profile directory");
                }
            }
        }
    }
    panic!("unsupported Cargo test executable layout: {}", exe.display());
}

/// A small example with no required features, built next to the CLI so feature
/// resolution includes dev-dependencies (see `cli_build_command`).
const CLI_FEATURE_PEER_EXAMPLE: &str = "remote_grid_frame_dump";

/// Variables cargo sets for the package it builds or runs, never for cargo itself.
fn is_cargo_package_env(key: &str) -> bool {
    key.starts_with("CARGO_PKG_")
        || matches!(
            key,
            "CARGO_MANIFEST_DIR"
                | "CARGO_MANIFEST_PATH"
                | "CARGO_MANIFEST_LINKS"
                | "CARGO_CRATE_NAME"
                | "CARGO_BIN_NAME"
                | "CARGO_PRIMARY_PACKAGE"
                | "CARGO_TARGET_TMPDIR"
                | "CARGO_RUSTC_CURRENT_DIR"
                | "OUT_DIR"
        )
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
            "build", "--bin", "ferryx-cli", "--example", "remote_grid_frame_dump",
            "--manifest-path", "checkout/src-tauri/Cargo.toml",
            "--target-dir", expected_root, "--profile", expected_profile,
        ];
        if explicit { expected.extend(["--target", triple]); }
        let mut expected: Vec<std::ffi::OsString> = expected.into_iter().map(Into::into).collect();
        expected[6] = Path::new("checkout/src-tauri").join("Cargo.toml").into_os_string();
        assert_eq!(command.get_args().collect::<Vec<_>>(),
            expected.iter().map(std::ffi::OsString::as_os_str).collect::<Vec<_>>());
        let mut expected_binary_dir = PathBuf::from(expected_root);
        if explicit { expected_binary_dir.push(triple); }
        expected_binary_dir.push(if expected_profile == "dev" { "debug" } else { expected_profile });
        assert_eq!(binary, expected_binary_dir
            .join(format!("ferryx-cli{}", std::env::consts::EXE_SUFFIX)));
        assert_eq!(command.get_envs().any(|(key, value)| key == "CARGO_BUILD_TARGET" && value.is_none()), !explicit);
    }

    #[test]
    fn native_debug_reuses_the_existing_target_root() {
        assert_layout("target/debug/deps/tests", "target", "dev", false);
    }

    #[test]
    fn staged_package_output_reuses_target_profile_not_fingerprint() {
        assert_layout(
            "target/aarch64-apple-darwin/debug/build/ferryx/93ea01d10e1900a3/out/tests",
            "target", "dev", true,
        );
    }

    #[test]
    fn legacy_staged_output_and_custom_profiles_are_supported() {
        assert_layout("target/debug/build/ferryx-abcd/out/tests", "target", "dev", false);
        assert_layout(
            "shared/aarch64-apple-darwin/custom/build/ferryx/abcd/out/tests",
            "shared", "custom", true,
        );
    }

    #[test]
    #[should_panic(expected = "unsupported Cargo test executable layout")]
    fn unknown_layout_is_rejected_instead_of_building_into_an_arbitrary_directory() {
        test_profile_dir(Path::new("unrelated/random/test-binary"));
    }

    #[test]
    fn nested_cargo_drops_the_inherited_package_environment() {
        // This test binary runs under cargo test, so the package variables are set.
        assert!(std::env::var_os("CARGO_MANIFEST_DIR").is_some());
        let (command, _) = cli_build_command(
            std::ffi::OsStr::new("cargo"),
            Path::new("target/debug/deps/tests"),
            Path::new("checkout/src-tauri"),
            "aarch64-apple-darwin",
        );
        let removed: Vec<_> = command
            .get_envs()
            .filter(|(_, value)| value.is_none())
            .map(|(key, _)| key.to_string_lossy().into_owned())
            .collect();
        for key in ["CARGO_MANIFEST_DIR", "CARGO_PKG_NAME", "CARGO_PKG_VERSION", "OUT_DIR"] {
            assert!(removed.iter().any(|k| k == key), "{key} must not reach the nested cargo");
        }
        assert!(removed.iter().all(|k| is_cargo_package_env(k) || k == "CARGO_BUILD_TARGET"));
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

