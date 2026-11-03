//! Builds the engine and lays it out as the on-demand download Ferryx installs:
//! `ferryx_engine.app` on macOS, a directory with `ferryx_engine.exe` on Windows.
//!
//! `cargo run --bin bundle-engine -- <output dir>` (needs `CEF_PATH` or the CEF download the
//! `cef` build script does).

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args_os()
        .nth(1)
        .map(std::path::PathBuf::from)
        .ok_or("usage: bundle-engine <output dir>")?;
    std::fs::create_dir_all(&output)?;
    let path = bundle(&output)?;
    println!("{}", path.display());
    Ok(())
}

#[cfg(target_os = "macos")]
fn bundle(output: &std::path::Path) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    use cef::build_util::mac::{build_bundle, BundleInfo};
    let version = semver::Version::parse(env!("CARGO_PKG_VERSION"))?;
    let info = BundleInfo::new("ferryx_engine", "app.ferryx.browser-engine", "Ferryx Browser Engine", "English", version);
    Ok(build_bundle(output, "ferryx_engine", info)?)
}

#[cfg(target_os = "windows")]
fn bundle(output: &std::path::Path) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    Ok(cef::build_util::win::build_bundle(output, "ferryx_engine", true)?)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn bundle(_output: &std::path::Path) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    Err("the engine is bundled on macOS and Windows only".into())
}
