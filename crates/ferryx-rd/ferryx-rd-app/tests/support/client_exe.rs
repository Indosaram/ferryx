//! Locates the `ferryx-rd-client` executable for the CLI integration tests.
//!
//! `env!("CARGO_BIN_EXE_...")` turns a missing variable into a compile error. On the Linux
//! build host the default toolchain compiled these tests without the variable (Rust 1.95
//! on the same tree defined it), so the whole `cargo test --workspace` run failed to build.
//! Resolve it at compile time when present, else at run time (Cargo also exports it to the
//! test process), else from the target layout, where the binary sits beside `deps/`.
use std::path::PathBuf;

pub fn client_exe() -> PathBuf {
    if let Some(path) = option_env!("CARGO_BIN_EXE_ferryx-rd-client") {
        return PathBuf::from(path);
    }
    if let Some(path) = std::env::var_os("CARGO_BIN_EXE_ferryx-rd-client") {
        return PathBuf::from(path);
    }
    let test_exe = std::env::current_exe().expect("test executable path");
    let profile_dir = test_exe
        .parent()
        .and_then(|deps| deps.parent())
        .expect("test executable lives in <profile>/deps");
    profile_dir.join(format!("ferryx-rd-client{}", std::env::consts::EXE_SUFFIX))
}
