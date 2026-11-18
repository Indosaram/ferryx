//! Test-only helpers shared across suites.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub(crate) fn ferryx_cli_binary() -> &'static Path {
    static CLI: OnceLock<PathBuf> = OnceLock::new();
    CLI.get_or_init(|| {
        let exe = std::env::current_exe().expect("test executable path");
        let binary = std::env::var_os("CARGO_BIN_EXE_ferryx-cli")
            .map(PathBuf::from)
            .or_else(|| cli_next_to_test(&exe))
            .expect("unknown Cargo test layout: pass the Cargo-built CLI in CARGO_BIN_EXE_ferryx-cli");
        assert!(binary.is_file(), "missing {}: run cargo test --lib --test cli_help_version to build the CLI for this profile", binary.display());
        binary
    })
}

fn cli_next_to_test(exe: &Path) -> Option<PathBuf> {
    let dir = exe.parent()?;
    let profile = if dir.ends_with("deps") {
        dir.parent()?
    } else {
        if !dir.ends_with("out") { return None; }
        let build = dir.ancestors().nth(3)?;
        if !build.ends_with("build") { return None; }
        build.parent()?
    };
    Some(profile.join(format!("ferryx-cli{}", std::env::consts::EXE_SUFFIX)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_layout(exe: &str, profile: &str) {
        let cli = Path::new(profile).join(format!("ferryx-cli{}", std::env::consts::EXE_SUFFIX));
        assert_eq!(cli_next_to_test(Path::new(exe)), Some(cli));
    }

    #[test]
    fn native_debug_reuses_the_existing_target_root() {
        assert_layout("target/debug/deps/tests", "target/debug");
    }

    #[test]
    fn cargo_build_tree_does_not_treat_the_fingerprint_as_a_profile() {
        assert_layout("shared/triple/debug/build/ferryx/3c561bacd2034271/out/tests", "shared/triple/debug");
    }

    #[test]
    fn explicit_target_does_not_nest_or_rebuild_as_host() {
        assert_layout("target/aarch64-apple-darwin/debug/deps/tests", "target/aarch64-apple-darwin/debug");
    }

    #[test]
    fn explicit_release_preserves_profile_and_binary_location() {
        assert_layout("shared/aarch64-apple-darwin/release/deps/tests", "shared/aarch64-apple-darwin/release");
    }

    #[test]
    fn custom_profile_and_target_directory_names_are_not_guessed_as_triples() {
        assert_layout("my-target-dir/custom/deps/tests", "my-target-dir/custom");
        assert!(cli_next_to_test(Path::new("foreign/custom/out/tests")).is_none());
    }
}
