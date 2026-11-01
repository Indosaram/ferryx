//! Builds the engine and lays it out as the on-demand download Ferryx installs:
//! `ferryx_engine.app` on macOS, a directory with `ferryx_engine.exe` on Windows.
//!
//! `cargo run --bin bundle-engine -- <output dir> [--release]` (needs `CEF_PATH` or the CEF
//! download the `cef` build script does). `--release` builds and bundles optimized binaries;
//! `scripts/browser-engine/release-engine.mjs` uses it for published engines.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut output = None;
    let mut release = false;
    for arg in std::env::args_os().skip(1) {
        if arg == "--release" {
            release = true;
        } else if output.is_none() {
            output = Some(std::path::PathBuf::from(arg));
        } else {
            return Err("usage: bundle-engine <output dir> [--release]".into());
        }
    }
    let output = output.ok_or("usage: bundle-engine <output dir> [--release]")?;
    std::fs::create_dir_all(&output)?;
    let path = bundle(&output, release)?;
    println!("{}", path.display());
    Ok(())
}

#[cfg(target_os = "macos")]
fn bundle(output: &std::path::Path, release: bool) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    use cef::build_util::mac::{build_bundle, bundle as bundle_app, BundleInfo};
    let version = semver::Version::parse(env!("CARGO_PKG_VERSION"))?;
    let info = BundleInfo::new("ferryx_engine", "app.ferryx.browser-engine", "Ferryx Browser Engine", "English", version);
    if !release {
        return Ok(build_bundle(output, "ferryx_engine", info)?);
    }
    // cef's build_bundle always builds debug binaries; a release bundle builds them here.
    let metadata = cef::build_util::metadata::get_cargo_metadata()?;
    let bundle_metadata = metadata.parse_bundle_metadata("ferryx_engine")?;
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = std::process::Command::new(cargo)
        .args(["build", "--release", "--bin", "ferryx_engine", "--bin", &bundle_metadata.helper_name])
        .status()?;
    if !status.success() {
        return Err(format!("cargo build --release failed: {status}").into());
    }
    Ok(bundle_app(
        output,
        &metadata.target_directory().join("release"),
        "ferryx_engine",
        &bundle_metadata.helper_name,
        bundle_metadata.resources_path,
        info,
    )?)
}

#[cfg(target_os = "windows")]
fn bundle(output: &std::path::Path, release: bool) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    Ok(cef::build_util::win::build_bundle(output, "ferryx_engine", release)?)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn bundle(_output: &std::path::Path, _release: bool) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    Err("the engine is bundled on macOS and Windows only".into())
}
