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

/// Stages an executable at `destination` without this process ever holding a
/// write descriptor on it and without sharing an inode with `source`.
///
/// `std::fs::copy` creates the target in-process. It closes that descriptor
/// before returning, but a parallel test's `fork` can inherit it while it is
/// open; until the inheriting child execs, the inode has an open-for-write
/// descriptor, and `exec(destination)` fails with `ETXTBSY` ("Text file busy",
/// `ExecutableFileBusy`). The flake therefore tracks machine load and thread
/// count rather than the code under test, and closing the descriptor on our
/// side cannot fix it: the one that blocks the exec lives in another process.
///
/// Always staging through a short-lived child `cp` process both prevents any
/// concurrent fork of this process from inheriting a write descriptor AND keeps
/// the staged destination on its own inode, so a test that `chmod`s or mutates
/// `destination` can never alter a shared Cargo build artifact.
#[cfg(unix)]
pub(crate) fn stage_executable(source: &Path, destination: &Path) {
    use std::os::unix::fs::PermissionsExt;

    // `sh -c 'script' name "$1" "$2"`: `name` becomes $0 and the paths arrive as
    // positional parameters, so nothing in them is parsed as shell syntax.
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg("cp \"$1\" \"$2\"")
        .arg("stage-executable")
        .arg(source)
        .arg(destination)
        .status()
        .unwrap_or_else(|error| {
            panic!(
                "stage {} -> {}: spawn cp: {error}",
                source.display(),
                destination.display()
            )
        });
    assert!(
        status.success(),
        "stage {} -> {}: cp exited with {status}",
        source.display(),
        destination.display()
    );
    let mode = std::fs::metadata(source)
        .unwrap_or_else(|error| panic!("stat {}: {error}", source.display()))
        .permissions()
        .mode()
        & 0o777;
    std::fs::set_permissions(destination, std::fs::Permissions::from_mode(mode))
        .unwrap_or_else(|error| panic!("chmod {}: {error}", destination.display()));
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

    #[cfg(unix)]
    #[test]
    fn stage_executable_does_not_share_inode_with_source() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let dir = tempfile::tempdir().expect("tempdir");
        let src = dir.path().join("src-bin");
        let dst = dir.path().join("dst-bin");
        std::fs::write(&src, b"#!/bin/sh\nexit 0\n").expect("write src");
        std::fs::set_permissions(&src, std::fs::Permissions::from_mode(0o755)).expect("chmod src");

        stage_executable(&src, &dst);

        let src_meta = std::fs::metadata(&src).expect("stat src");
        let dst_meta = std::fs::metadata(&dst).expect("stat dst");
        assert_ne!(
            (src_meta.dev(), src_meta.ino()),
            (dst_meta.dev(), dst_meta.ino()),
            "staged copy must not hard-link the source build artifact"
        );
        assert_eq!(dst_meta.permissions().mode() & 0o777, 0o755);
    }
}
