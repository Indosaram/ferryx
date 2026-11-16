//! `ferryx-cli --version` and `--help` are probes, not usage errors: the one-line installer
//! (scripts/install.sh) runs the downloaded binary to prove it executes, and wrappers check the
//! version. Both must exit 0, and the help text must list `up`, the command the installer runs.

use std::process::Command;

fn run(arg: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ferryx-cli"))
        .arg(arg)
        .output()
        .expect("spawn ferryx-cli")
}

#[test]
fn version_flags_print_the_version_and_exit_zero() {
    for flag in ["--version", "-V", "version"] {
        let out = run(flag);
        assert!(
            out.status.success(),
            "{flag} must exit 0, got {:?}",
            out.status
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.starts_with("ferryx-cli "),
            "{flag} printed: {stdout}"
        );
    }
}

#[test]
fn help_flags_print_usage_with_up_and_exit_zero() {
    for flag in ["--help", "-h", "help"] {
        let out = run(flag);
        assert!(
            out.status.success(),
            "{flag} must exit 0, got {:?}",
            out.status
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains("Usage: ferryx-cli <up "),
            "{flag} printed: {stdout}"
        );
    }
}

#[test]
fn no_arguments_is_still_a_usage_error() {
    let out = Command::new(env!("CARGO_BIN_EXE_ferryx-cli"))
        .output()
        .expect("spawn ferryx-cli");
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("Usage: ferryx-cli <up "));
}
