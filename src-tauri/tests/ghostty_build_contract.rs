//! Ghostty Source, Build, and FFI Boundary Contract Tests.
//! Validates pinned submodule state, source lock, Zig toolchain requirements, cross-target mappings, and static FFI linkage.

use std::ffi::c_void;
use std::fs;
use std::path::Path;
use std::process::Command;

#[allow(dead_code)]
#[path = "../native_terminal/build_ghostty.rs"]
mod build_ghostty;

#[path = "ghostty_build_info_ffi.rs"]
mod ghostty_build_info_ffi;
use ghostty_build_info_ffi::*;

fn assert_err(res: Result<String, String>, subs: &[&str]) {
    let err = res.unwrap_err();
    for sub in subs {
        assert!(err.contains(sub), "missing '{sub}' in '{err}'");
    }
}

#[test]
fn test_ghostty_source_lock_matches_build_contract() {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("native_terminal/ghostty_source_lock.json");
    assert!(p.is_file(), "lock must exist at {}", p.display());
    let j: serde_json::Value = serde_json::from_str(&fs::read_to_string(&p).unwrap()).unwrap();
    assert_eq!(
        j["commit"].as_str().unwrap(),
        build_ghostty::EXPECTED_GHOSTTY_SHA
    );
    assert_eq!(
        j["zig_version"].as_str().unwrap(),
        build_ghostty::REQUIRED_ZIG_VERSION
    );
    assert_eq!(j["artifact"].as_str().unwrap(), "libghostty-vt.a");
    assert_eq!(j["artifacts"]["unix"].as_str().unwrap(), "libghostty-vt.a");
    assert_eq!(
        j["artifacts"]["windows_static"].as_str().unwrap(),
        "ghostty-vt-static.lib"
    );
    assert_eq!(
        j["artifacts"]["windows_import"].as_str().unwrap(),
        "ghostty-vt.lib"
    );
}

#[test]
fn test_ghostty_submodule_pinned_sha_and_clean_state() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/ghostty");
    let sha = build_ghostty::verify_submodule_sha(&dir).expect("SHA check failed");
    assert_eq!(sha, build_ghostty::EXPECTED_GHOSTTY_SHA, "pinned SHA match");
    let st = Command::new("git")
        .arg("-C")
        .arg(&dir)
        .args(["status", "--porcelain"])
        .output()
        .unwrap();
    assert!(st.status.success() && String::from_utf8_lossy(&st.stdout).trim().is_empty());
}

#[test]
fn test_ghostty_zig_version_verification() {
    let zig = std::env::var("ZIG").unwrap_or_else(|_| "zig".to_string());
    assert!(build_ghostty::verify_zig(&zig)
        .unwrap()
        .starts_with(build_ghostty::REQUIRED_ZIG_VERSION));
}

#[test]
fn test_cargo_target_to_zig_target_mapping() {
    let expected_mappings = [
        ("x86_64-pc-windows-msvc", "x86_64-windows-msvc"),
        ("x86_64-pc-windows-gnu", "x86_64-windows-gnu"),
        ("aarch64-pc-windows-msvc", "aarch64-windows-msvc"),
        ("aarch64-pc-windows-gnu", "aarch64-windows-gnu"),
        ("aarch64-pc-windows-gnullvm", "aarch64-windows-gnu"),
        ("x86_64-unknown-linux-gnu", "x86_64-linux-gnu"),
        ("x86_64-unknown-linux-musl", "x86_64-linux-musl"),
        ("aarch64-unknown-linux-gnu", "aarch64-linux-gnu"),
        ("aarch64-unknown-linux-musl", "aarch64-linux-musl"),
        ("x86_64-apple-darwin", "x86_64-macos"),
        ("aarch64-apple-darwin", "aarch64-macos"),
        ("arm64-apple-darwin", "aarch64-macos"),
    ];

    for (cargo_target, expected_zig_target) in expected_mappings {
        let actual = build_ghostty::cargo_target_to_zig_target(cargo_target)
            .unwrap_or_else(|e| panic!("Failed to map target {cargo_target}: {e}"));
        assert_eq!(
            actual, expected_zig_target,
            "Target mapping mismatch for '{cargo_target}'"
        );
    }
}

#[test]
fn test_unmapped_cargo_target_returns_structured_err() {
    let unmapped_targets = [
        "sparc64-unknown-linux-gnu",
        "riscv64gc-unknown-none-elf",
        "wasm32-unknown-unknown",
        "mips-unknown-linux-gnu",
    ];

    for target in unmapped_targets {
        let err = build_ghostty::cargo_target_to_zig_target(target).unwrap_err();
        assert!(
            err.contains("Unsupported or unmapped Cargo target triple"),
            "Expected unmapped target message, got: '{err}'"
        );
        assert!(
            err.contains(target),
            "Expected error message to contain '{target}', got: '{err}'"
        );
    }
}

#[test]
fn test_compute_link_lib_stem() {
    assert_eq!(
        build_ghostty::compute_link_lib_stem("libghostty-vt.a").unwrap(),
        "ghostty-vt"
    );
    assert_eq!(
        build_ghostty::compute_link_lib_stem("ghostty-vt-static.lib").unwrap(),
        "ghostty-vt-static"
    );
    assert_eq!(
        build_ghostty::compute_link_lib_stem("ghostty-vt.lib").unwrap(),
        "ghostty-vt"
    );

    assert!(build_ghostty::compute_link_lib_stem("").is_err());
    assert!(build_ghostty::compute_link_lib_stem("lib.a").is_err());
    assert!(build_ghostty::compute_link_lib_stem(".lib").is_err());
    assert!(build_ghostty::compute_link_lib_stem("ghostty-vt.so").is_err());
    assert!(build_ghostty::compute_link_lib_stem("ghostty-vt.dylib").is_err());
}

#[test]
fn test_resolve_static_lib_windows_and_unix() {
    let temp_dir = tempfile::tempdir().unwrap();
    let lib_dir = temp_dir.path().join("lib");
    fs::create_dir_all(&lib_dir).unwrap();

    // 1. When ghostty-vt-static.lib exists, windows target prefers it
    let static_lib_win = lib_dir.join("ghostty-vt-static.lib");
    fs::write(&static_lib_win, b"fake_static_lib").unwrap();

    let (path, stem) =
        build_ghostty::resolve_static_lib(&lib_dir, "x86_64-pc-windows-gnu").unwrap();
    assert_eq!(path, static_lib_win);
    assert_eq!(stem, "ghostty-vt-static");

    // 2. When only ghostty-vt.lib exists on windows
    fs::remove_file(&static_lib_win).unwrap();
    let import_lib_win = lib_dir.join("ghostty-vt.lib");
    fs::write(&import_lib_win, b"fake_import_lib").unwrap();

    let (path, stem) =
        build_ghostty::resolve_static_lib(&lib_dir, "x86_64-pc-windows-msvc").unwrap();
    assert_eq!(path, import_lib_win);
    assert_eq!(stem, "ghostty-vt");

    // 3. For unix target with libghostty-vt.a
    let unix_a = lib_dir.join("libghostty-vt.a");
    fs::write(&unix_a, b"fake_unix_a").unwrap();

    let (path, stem) =
        build_ghostty::resolve_static_lib(&lib_dir, "x86_64-unknown-linux-gnu").unwrap();
    assert_eq!(path, unix_a);
    assert_eq!(stem, "ghostty-vt");

    // 4. Missing artifacts in empty directory returns structured error
    fs::remove_file(&unix_a).unwrap();
    fs::remove_file(&import_lib_win).unwrap();

    let err = build_ghostty::resolve_static_lib(&lib_dir, "aarch64-apple-darwin").unwrap_err();
    assert!(err.contains("expected static library was not found"));
    assert!(err.contains("aarch64-apple-darwin"));
    assert!(err.contains("libghostty-vt.a"));
}

#[test]
fn test_negative_submodule_verification_nonexistent_directory() {
    let p = Path::new("/tmp/deliberately_missing_ghostty_path_contract_test");
    assert_err(
        build_ghostty::verify_submodule_sha(p),
        &[
            "Ghostty submodule directory not found at",
            build_ghostty::EXPECTED_GHOSTTY_SHA,
            "git submodule update --init --recursive",
        ],
    );
}

#[test]
fn test_negative_submodule_verification_uninitialized_directory() {
    let dir = std::env::temp_dir().join("ghostty_uninitialized_submodule_test");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let res = build_ghostty::verify_submodule_sha(&dir);
    let _ = fs::remove_dir_all(&dir);
    assert_err(
        res,
        &[
            "appears uninitialized (missing build.zig)",
            "git submodule update --init --recursive",
        ],
    );
}

#[test]
fn test_negative_submodule_verification_mismatched_sha() {
    let dir = std::env::temp_dir().join("ghostty_contract_test_mismatched_sha");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let git = |a: &[&str]| {
        assert!(Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(a)
            .status()
            .unwrap()
            .success())
    };
    git(&["init"]);
    fs::write(dir.join("build.zig"), "// dummy").unwrap();
    git(&["add", "build.zig"]);
    git(&[
        "-c",
        "user.name=T",
        "-c",
        "user.email=t@e.com",
        "commit",
        "-m",
        "init",
    ]);
    let res = build_ghostty::verify_submodule_sha(&dir);
    let _ = fs::remove_dir_all(&dir);
    assert_err(
        res,
        &[
            "Ghostty submodule revision mismatch at",
            build_ghostty::EXPECTED_GHOSTTY_SHA,
            "git checkout",
        ],
    );
}

#[test]
fn test_negative_zig_verification_missing_binary() {
    assert_err(
        build_ghostty::verify_zig("/tmp/deliberately_missing_zig_binary_path"),
        &[
            "Missing Zig toolchain: failed to execute",
            "Zig 0.16.0 is required to build libghostty-vt",
            "ZIG",
        ],
    );
}

#[test]
fn test_negative_zig_verification_incompatible_version() {
    let dir = std::env::temp_dir().join("ghostty_incompatible_zig_test");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let fake_zig = dir.join("fake_zig.sh");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::write(&fake_zig, "#!/bin/sh\necho \"0.15.0\"\n").unwrap();
        let mut p = fs::metadata(&fake_zig).unwrap().permissions();
        p.set_mode(0o755);
        fs::set_permissions(&fake_zig, p).unwrap();
    }
    let res = build_ghostty::verify_zig(fake_zig.to_str().unwrap());
    let _ = fs::remove_dir_all(&dir);
    assert_err(
        res,
        &[
            "Incompatible Zig version '0.15.0'",
            "libghostty-vt requires Zig 0.16.0",
        ],
    );
}

#[test]
fn test_negative_build_ghostty_vt_with_invalid_source_root() {
    let p = Path::new("/tmp/missing_manifest_ghostty_contract_test");
    let out = std::env::temp_dir().join("ghostty_neg_out");
    let err = build_ghostty::build_ghostty_vt_with_env(p, &out, None).unwrap_err();
    assert!(err.contains("Ghostty submodule directory not found at"));
}

#[test]
fn test_negative_build_ghostty_vt_with_unmapped_target() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = std::env::temp_dir().join("ghostty_neg_target_out");
    let err = build_ghostty::build_ghostty_vt_with_target_and_env(
        manifest_dir,
        &out,
        "sparc64-unknown-linux-gnu",
        None,
    )
    .unwrap_err();
    assert!(err.contains("Unsupported or unmapped Cargo target triple"));
    assert!(err.contains("sparc64-unknown-linux-gnu"));
}

#[test]
fn test_negative_ffi_invalid_key_returns_invalid_value() {
    // SAFETY: 1. Pinned static link; 2. Key passed as raw c_int 0; 3. Null pointer safe for invalid key; 4. No unwind.
    unsafe {
        let raw = ghostty_build_info(
            GhosttyBuildInfo::Invalid.as_raw_c_int(),
            std::ptr::null_mut(),
        );
        assert_eq!(
            GhosttyResult::try_from(raw),
            Ok(GhosttyResult::InvalidValue)
        );
    }
}

/// # Safety
/// Callers must pass an output type `T` and pointer `out` matching the expected C output type contract
/// specified for `key` in `include/ghostty/vt/build_info.h` (e.g. `u8` for bools, `c_int` for Optimize).
unsafe fn query_ok<T>(key: GhosttyBuildInfo, out: &mut T) {
    let raw = ghostty_build_info(key.as_raw_c_int(), out as *mut T as *mut c_void);
    assert_eq!(GhosttyResult::try_from(raw), Ok(GhosttyResult::Success));
}

#[test]
fn test_ghostty_vt_static_link_and_ffi_call() {
    // SAFETY:
    // 1. Linked to static libghostty-vt.a from pinned commit.
    // 2. ABI types match build_info.h: bool->u8, optimize->c_int, versions->usize/GhosttyString.
    // 3. Output pointers are valid, aligned stack allocations; strings borrow static data; no unwinding.
    unsafe {
        let (mut raw_simd, mut raw_kitty, mut raw_tmux, mut raw_opt) = (255u8, 255u8, 255u8, -1i32);
        let (mut major, mut minor, mut patch) = (0usize, 0usize, 0usize);
        let (mut v_str, mut v_pre, mut v_build) = (
            GhosttyString::NULL,
            GhosttyString::NULL,
            GhosttyString::NULL,
        );

        query_ok(GhosttyBuildInfo::Simd, &mut raw_simd);
        query_ok(GhosttyBuildInfo::KittyGraphics, &mut raw_kitty);
        query_ok(GhosttyBuildInfo::TmuxControlMode, &mut raw_tmux);
        query_ok(GhosttyBuildInfo::Optimize, &mut raw_opt);
        query_ok(GhosttyBuildInfo::VersionMajor, &mut major);
        query_ok(GhosttyBuildInfo::VersionMinor, &mut minor);
        query_ok(GhosttyBuildInfo::VersionPatch, &mut patch);
        query_ok(GhosttyBuildInfo::VersionString, &mut v_str);
        query_ok(GhosttyBuildInfo::VersionPre, &mut v_pre);
        query_ok(GhosttyBuildInfo::VersionBuild, &mut v_build);

        let (simd, kitty, tmux) = (
            try_bool_from_c_u8(raw_simd).unwrap(),
            try_bool_from_c_u8(raw_kitty).unwrap(),
            try_bool_from_c_u8(raw_tmux).unwrap(),
        );
        assert!(kitty);
        assert_eq!(
            GhosttyOptimizeMode::try_from(raw_opt),
            Ok(GhosttyOptimizeMode::ReleaseFast)
        );
        assert!(!v_str.ptr.is_null() && v_str.len > 0);
        let (s, pre, build) = (
            v_str.as_str().unwrap(),
            v_pre.as_str().unwrap(),
            v_build.as_str().unwrap(),
        );
        println!("Linked: {s} (pre='{pre}', build='{build}'), ver={major}.{minor}.{patch}, simd={simd}, kitty={kitty}, tmux={tmux}");
    }
}

#[test]
fn test_safe_ghostty_build_info_helpers() {
    let simd = safe_ghostty_build_info_bool(GhosttyBuildInfo::Simd).unwrap();
    let kitty = safe_ghostty_build_info_bool(GhosttyBuildInfo::KittyGraphics).unwrap();
    let tmux = safe_ghostty_build_info_bool(GhosttyBuildInfo::TmuxControlMode).unwrap();
    assert!(kitty);
    println!("Safe bools: simd={simd}, kitty={kitty}, tmux={tmux}");
    assert_eq!(
        safe_ghostty_build_info_optimize().unwrap(),
        GhosttyOptimizeMode::ReleaseFast
    );

    let (maj, min, pat) = (
        safe_ghostty_build_info_usize(GhosttyBuildInfo::VersionMajor).unwrap(),
        safe_ghostty_build_info_usize(GhosttyBuildInfo::VersionMinor).unwrap(),
        safe_ghostty_build_info_usize(GhosttyBuildInfo::VersionPatch).unwrap(),
    );
    assert_eq!((maj, min, pat), (0, 1, 0));

    let (s, pre, build) = (
        safe_ghostty_build_info_string(GhosttyBuildInfo::VersionString).unwrap(),
        safe_ghostty_build_info_string(GhosttyBuildInfo::VersionPre).unwrap(),
        safe_ghostty_build_info_string(GhosttyBuildInfo::VersionBuild).unwrap(),
    );
    assert_eq!(s, "0.1.0-dev");
    assert_eq!(pre, "dev");
    println!("Safe strings: ver={s}, pre={pre}, build={build}");
}
