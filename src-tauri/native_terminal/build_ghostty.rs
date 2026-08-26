use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const EXPECTED_GHOSTTY_SHA: &str = "6a508fd5e34c7e222c052a6d00bb3891ff3feace";
pub const REQUIRED_ZIG_VERSION: &str = "0.16.0";

/// Result of build environment verification and libghostty-vt compilation.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GhosttyBuildConfig {
    pub zig_binary: String,
    pub zig_version: String,
    pub submodule_dir: PathBuf,
    pub resolved_sha: String,
    pub out_dir: PathBuf,
    pub lib_dir: PathBuf,
    pub include_dir: PathBuf,
    pub static_lib_path: PathBuf,
    pub link_lib_stem: String,
    pub target: String,
    pub zig_target: String,
}

pub fn verify_zig(zig_binary: &str) -> Result<String, String> {
    let output = Command::new(zig_binary)
        .arg("version")
        .output()
        .map_err(|e| {
            format!(
                "Missing Zig toolchain: failed to execute '{zig_binary}' ({e}). \\\n\
Zig {REQUIRED_ZIG_VERSION} is required to build libghostty-vt. \\\n\
Please install Zig {REQUIRED_ZIG_VERSION} (e.g. `brew install zig` or from https://ziglang.org/download/) \\\n\
or set the ZIG environment variable."
            )
        })?;

    if !output.status.success() {
        return Err(format!(
            "Failed to query zig version from '{zig_binary}': exited with status {:?}",
            output.status
        ));
    }

    let version_raw = String::from_utf8_lossy(&output.stdout);
    let version = version_raw.trim().to_string();

    if !version.starts_with(REQUIRED_ZIG_VERSION) {
        return Err(format!(
            "Incompatible Zig version '{version}': libghostty-vt requires Zig {REQUIRED_ZIG_VERSION}. \\\n\
Found: '{version}'."
        ));
    }

    Ok(version)
}

pub fn verify_submodule_sha(submodule_dir: &Path) -> Result<String, String> {
    if !submodule_dir.exists() {
        return Err(format!(
            "Ghostty submodule directory not found at '{}'. \\\n\
Run 'git submodule update --init --recursive' to initialize the vendored Ghostty source at commit {EXPECTED_GHOSTTY_SHA}. \\\n\
Silent or automatic upstream network downloads during build are disallowed.",
            submodule_dir.display()
        ));
    }

    let build_zig = submodule_dir.join("build.zig");
    if !build_zig.is_file() {
        return Err(format!(
            "Ghostty source at '{}' appears uninitialized (missing build.zig). \\\n\
Run 'git submodule update --init --recursive' to populate the submodule at commit {EXPECTED_GHOSTTY_SHA}.",
            submodule_dir.display()
        ));
    }

    let submodule_str = submodule_dir.to_str().ok_or_else(|| {
        format!(
            "Submodule path contains invalid UTF-8: {}",
            submodule_dir.display()
        )
    })?;

    let output = Command::new("git")
        .args(["-C", submodule_str, "rev-parse", "HEAD"])
        .output()
        .map_err(|e| {
            format!(
                "Failed to execute git in submodule directory '{}' ({e}). \\\n\
Ensure git is installed and submodule is accessible.",
                submodule_dir.display()
            )
        })?;

    if !output.status.success() {
        return Err(format!(
            "Failed to resolve git HEAD in submodule '{}': git rev-parse returned {:?}",
            submodule_dir.display(),
            output.status
        ));
    }

    let resolved_sha = String::from_utf8_lossy(&output.stdout).trim().to_string();

    if resolved_sha != EXPECTED_GHOSTTY_SHA {
        return Err(format!(
            "Ghostty submodule revision mismatch at '{}': found '{resolved_sha}', expected '{EXPECTED_GHOSTTY_SHA}'. \\\n\
Please checkout the exact pinned revision: (cd '{}' && git checkout {EXPECTED_GHOSTTY_SHA}). \\\n\
Silent or automatic upstream network downloads during build are disallowed.",
            submodule_dir.display(),
            submodule_dir.display()
        ));
    }

    Ok(resolved_sha)
}

/// Maps a Cargo target triple to the corresponding Zig target triple string.
pub fn cargo_target_to_zig_target(cargo_target: &str) -> Result<&'static str, String> {
    match cargo_target {
        // Windows targets
        "x86_64-pc-windows-msvc" => Ok("x86_64-windows-msvc"),
        "x86_64-pc-windows-gnu" => Ok("x86_64-windows-gnu"),
        "aarch64-pc-windows-msvc" => Ok("aarch64-windows-msvc"),
        "aarch64-pc-windows-gnu" | "aarch64-pc-windows-gnullvm" => Ok("aarch64-windows-gnu"),

        // Linux targets
        "x86_64-unknown-linux-gnu" => Ok("x86_64-linux-gnu"),
        "x86_64-unknown-linux-musl" => Ok("x86_64-linux-musl"),
        "aarch64-unknown-linux-gnu" => Ok("aarch64-linux-gnu"),
        "aarch64-unknown-linux-musl" => Ok("aarch64-linux-musl"),

        // Apple / macOS targets
        "x86_64-apple-darwin" => Ok("x86_64-macos"),
        "aarch64-apple-darwin" | "arm64-apple-darwin" => Ok("aarch64-macos"),

        _ => Err(format!(
            "Unsupported or unmapped Cargo target triple: '{cargo_target}'. \\\n\
No corresponding Zig target mapping is defined for this target."
        )),
    }
}

/// Computes the link stem for a given static library artifact filename.
pub fn compute_link_lib_stem(filename: &str) -> Result<String, String> {
    if let Some(stem) = filename.strip_suffix(".lib") {
        if stem.is_empty() {
            return Err(format!("Invalid library filename: '{filename}'"));
        }
        Ok(stem.to_string())
    } else if let Some(stripped) = filename.strip_prefix("lib") {
        if let Some(stem) = stripped.strip_suffix(".a") {
            if stem.is_empty() {
                return Err(format!("Invalid library filename: '{filename}'"));
            }
            Ok(stem.to_string())
        } else {
            Err(format!(
                "Unrecognized static library extension in filename '{filename}' (expected .a or .lib)"
            ))
        }
    } else {
        Err(format!(
            "Unrecognized static library naming convention for '{filename}' (expected lib*.a or *.lib)"
        ))
    }
}

/// Resolves the platform-correct static library artifact in `lib_dir` for `cargo_target`.
pub fn resolve_static_lib(lib_dir: &Path, cargo_target: &str) -> Result<(PathBuf, String), String> {
    let candidates: &[&str] = if cargo_target.contains("windows") {
        &["ghostty-vt-static.lib", "ghostty-vt.lib", "libghostty-vt.a"]
    } else {
        &["libghostty-vt.a", "ghostty-vt-static.lib", "ghostty-vt.lib"]
    };

    for &candidate in candidates {
        let candidate_path = lib_dir.join(candidate);
        if candidate_path.is_file() {
            let stem = compute_link_lib_stem(candidate)?;
            return Ok((candidate_path, stem));
        }
    }

    Err(format!(
        "libghostty-vt build completed successfully, but expected static library was not found in '{}' for target '{}'. \\\n\
Checked candidates: {}",
        lib_dir.display(),
        cargo_target,
        candidates.join(", ")
    ))
}

pub fn build_ghostty_vt_with_target_and_env(
    manifest_dir: &Path,
    out_dir: &Path,
    cargo_target: &str,
    zig_override: Option<&str>,
) -> Result<GhosttyBuildConfig, String> {
    let zig_binary = zig_override
        .map(|s| s.to_string())
        .or_else(|| env::var("ZIG").ok())
        .unwrap_or_else(|| "zig".to_string());

    let zig_version = verify_zig(&zig_binary)?;

    let submodule_dir = manifest_dir.join("vendor/ghostty");
    let resolved_sha = verify_submodule_sha(&submodule_dir)?;

    let zig_target = cargo_target_to_zig_target(cargo_target)?;

    let ghostty_prefix = out_dir.join("ghostty_vt");
    let zig_cache_dir = out_dir.join("ghostty_zig_cache");
    let zig_global_cache_dir = out_dir.join("ghostty_zig_global_cache");

    fs::create_dir_all(&ghostty_prefix).map_err(|e| {
        format!(
            "Failed to create Ghostty prefix directory '{}': {e}",
            ghostty_prefix.display()
        )
    })?;
    fs::create_dir_all(&zig_cache_dir).map_err(|e| {
        format!(
            "Failed to create Ghostty Zig cache directory '{}': {e}",
            zig_cache_dir.display()
        )
    })?;
    fs::create_dir_all(&zig_global_cache_dir).map_err(|e| {
        format!(
            "Failed to create Ghostty Zig global cache directory '{}': {e}",
            zig_global_cache_dir.display()
        )
    })?;

    let ghostty_prefix_str = ghostty_prefix.to_str().ok_or_else(|| {
        format!(
            "Ghostty prefix path is not valid UTF-8: {}",
            ghostty_prefix.display()
        )
    })?;
    let zig_cache_dir_str = zig_cache_dir.to_str().ok_or_else(|| {
        format!(
            "Ghostty Zig cache path is not valid UTF-8: {}",
            zig_cache_dir.display()
        )
    })?;
    let zig_global_cache_dir_str = zig_global_cache_dir.to_str().ok_or_else(|| {
        format!(
            "Ghostty Zig global cache path is not valid UTF-8: {}",
            zig_global_cache_dir.display()
        )
    })?;

    let target_flag = format!("-Dtarget={zig_target}");

    let mut cmd = Command::new(&zig_binary);
    cmd.current_dir(&submodule_dir);
    cmd.args([
        "build",
        "-Demit-lib-vt=true",
        "-Demit-xcframework=false",
        "-Demit-docs=false",
        "-Demit-exe=false",
        "-Demit-test-exe=false",
        "-Doptimize=ReleaseFast",
        &target_flag,
        "-p",
        ghostty_prefix_str,
        "--cache-dir",
        zig_cache_dir_str,
        "--global-cache-dir",
        zig_global_cache_dir_str,
    ]);

    let output = cmd.output().map_err(|e| {
        format!("Failed to spawn `zig build` for libghostty-vt using binary '{zig_binary}': {e}")
    })?;

    if !output.status.success() {
        return Err(format!(
            "`zig build` for libghostty-vt failed with status {:?}:\nStdout:\n{}\nStderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let lib_dir = ghostty_prefix.join("lib");
    let include_dir = ghostty_prefix.join("include");
    let (static_lib_path, link_lib_stem) = resolve_static_lib(&lib_dir, cargo_target)?;

    Ok(GhosttyBuildConfig {
        zig_binary,
        zig_version,
        submodule_dir,
        resolved_sha,
        out_dir: out_dir.to_path_buf(),
        lib_dir,
        include_dir,
        static_lib_path,
        link_lib_stem,
        target: cargo_target.to_string(),
        zig_target: zig_target.to_string(),
    })
}

#[allow(dead_code)]
#[cfg(all(target_arch = "aarch64", target_os = "macos"))]
pub const DEFAULT_HOST_TARGET: &str = "aarch64-apple-darwin";
#[allow(dead_code)]
#[cfg(all(target_arch = "x86_64", target_os = "macos"))]
pub const DEFAULT_HOST_TARGET: &str = "x86_64-apple-darwin";
#[allow(dead_code)]
#[cfg(all(target_arch = "x86_64", target_os = "linux", target_env = "gnu"))]
pub const DEFAULT_HOST_TARGET: &str = "x86_64-unknown-linux-gnu";
#[allow(dead_code)]
#[cfg(all(target_arch = "aarch64", target_os = "linux", target_env = "gnu"))]
pub const DEFAULT_HOST_TARGET: &str = "aarch64-unknown-linux-gnu";
#[allow(dead_code)]
#[cfg(all(target_arch = "x86_64", target_os = "windows", target_env = "msvc"))]
pub const DEFAULT_HOST_TARGET: &str = "x86_64-pc-windows-msvc";
#[allow(dead_code)]
#[cfg(all(target_arch = "x86_64", target_os = "windows", target_env = "gnu"))]
pub const DEFAULT_HOST_TARGET: &str = "x86_64-pc-windows-gnu";
#[allow(dead_code)]
#[cfg(not(any(
    all(target_arch = "aarch64", target_os = "macos"),
    all(target_arch = "x86_64", target_os = "macos"),
    all(target_arch = "x86_64", target_os = "linux", target_env = "gnu"),
    all(target_arch = "aarch64", target_os = "linux", target_env = "gnu"),
    all(target_arch = "x86_64", target_os = "windows", target_env = "msvc"),
    all(target_arch = "x86_64", target_os = "windows", target_env = "gnu"),
)))]
pub const DEFAULT_HOST_TARGET: &str = "unknown";

#[allow(dead_code)]
pub fn build_ghostty_vt_with_env(
    manifest_dir: &Path,
    out_dir: &Path,
    zig_override: Option<&str>,
) -> Result<GhosttyBuildConfig, String> {
    let cargo_target = env::var("TARGET")
        .or_else(|_| env::var("HOST"))
        .unwrap_or_else(|_| DEFAULT_HOST_TARGET.to_string());

    build_ghostty_vt_with_target_and_env(manifest_dir, out_dir, &cargo_target, zig_override)
}

pub fn build_ghostty_vt() -> Result<GhosttyBuildConfig, String> {
    let manifest_dir = PathBuf::from(
        env::var("CARGO_MANIFEST_DIR")
            .map_err(|e| format!("CARGO_MANIFEST_DIR environment variable is not set: {e}"))?,
    );
    let out_dir = PathBuf::from(
        env::var("OUT_DIR").map_err(|e| format!("OUT_DIR environment variable is not set: {e}"))?,
    );
    let cargo_target =
        env::var("TARGET").map_err(|e| format!("TARGET environment variable is not set: {e}"))?;

    let config =
        build_ghostty_vt_with_target_and_env(&manifest_dir, &out_dir, &cargo_target, None)?;

    // Export link search path and library
    println!(
        "cargo:rustc-link-search=native={}",
        config.lib_dir.display()
    );
    println!("cargo:rustc-link-lib=static={}", config.link_lib_stem);
    println!("cargo:include={}", config.include_dir.display());
    println!("cargo:ghostty_lib_dir={}", config.lib_dir.display());
    println!("cargo:ghostty_include_dir={}", config.include_dir.display());
    println!("cargo:ghostty_sha={}", config.resolved_sha);
    println!("cargo:ghostty_zig_version={}", config.zig_version);
    println!("cargo:ghostty_target={}", config.target);
    println!("cargo:ghostty_zig_target={}", config.zig_target);
    println!(
        "cargo:ghostty_artifact={}",
        config.static_lib_path.display()
    );

    // Watch triggers for Cargo
    println!("cargo:rerun-if-changed=native_terminal/build_ghostty.rs");
    println!("cargo:rerun-if-changed=native_terminal/ghostty_source_lock.json");
    println!("cargo:rerun-if-changed=vendor/ghostty/build.zig");
    println!("cargo:rerun-if-changed=vendor/ghostty/src");
    println!("cargo:rerun-if-changed=vendor/ghostty/include");
    println!("cargo:rerun-if-env-changed=TARGET");
    println!("cargo:rerun-if-env-changed=HOST");
    println!("cargo:rerun-if-env-changed=ZIG");

    Ok(config)
}
