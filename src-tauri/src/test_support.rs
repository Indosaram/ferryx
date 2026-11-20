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
/// write descriptor on it.
///
/// `std::fs::copy` creates the target in-process. It closes that descriptor
/// before returning, but a parallel test's `fork` can inherit it while it is
/// open; until the inheriting child execs, the inode has an open-for-write
/// descriptor, and `exec(destination)` fails with `ETXTBSY` ("Text file busy",
/// `ExecutableFileBusy`). The flake therefore tracks machine load and thread
/// count rather than the code under test, and closing the descriptor on our
/// side cannot fix it: the one that blocks the exec lives in another process.
///
/// A hard link reuses the source's already-closed inode and copies nothing.
/// Cross-device staging (the usual case when `TMPDIR` is a tmpfs and the build
/// tree is not) falls back to a child process that writes the bytes and exits
/// before the destination is executed, so no concurrent fork of this process
/// can inherit its descriptor.
#[cfg(unix)]
pub(crate) fn stage_executable(source: &Path, destination: &Path) {
    use std::os::unix::fs::PermissionsExt;

    if std::fs::hard_link(source, destination).is_ok() {
        return;
    }

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
}
