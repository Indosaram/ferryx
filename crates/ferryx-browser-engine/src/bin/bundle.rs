//! Builds the engine and lays it out as the on-demand download Ferryx installs:
//! `ferryx_engine.app` on macOS, a directory with `ferryx_engine.exe` on Windows, and on Linux
//! a directory with `ferryx_engine`, `libcef.so`, the resources and `locales/`, zipped as
//! `engine-linux-<arch>.zip` beside it.
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

/// Linux. CEF's `chrome-sandbox` only works setuid root, which a download into the user's data
/// dir can never be, and a present but unprivileged helper makes Chromium refuse to start, so it
/// is left out: the engine uses the namespace sandbox (see `engine/linux_sandbox.rs`).
#[cfg(target_os = "linux")]
fn bundle(output: &std::path::Path) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let name = format!("engine-linux-{}", std::env::consts::ARCH);
    let dir = output.join(&name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    std::fs::create_dir_all(&dir)?;
    cef::build_util::linux::build_bundle(&dir, "ferryx_engine", true)?;
    let sandbox_helper = dir.join("chrome-sandbox");
    if sandbox_helper.exists() {
        std::fs::remove_file(&sandbox_helper)?;
    }
    let archive = output.join(format!("{name}.zip"));
    zip_dir(&dir, &archive)?;
    Ok(archive)
}

/// Zips `dir`'s contents (entry names relative to it) keeping each file's Unix mode, which the
/// Ferryx installer restores.
#[cfg(target_os = "linux")]
fn zip_dir(dir: &std::path::Path, archive: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;
    let mut files = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        for entry in std::fs::read_dir(&current)? {
            let path = entry?.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files.sort();
    let mut writer = zip::ZipWriter::new(std::fs::File::create(archive)?);
    for path in files {
        let name = path.strip_prefix(dir)?.to_string_lossy().replace('\\', "/");
        let mode = std::fs::metadata(&path)?.permissions().mode() & 0o777;
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .large_file(true)
            .unix_permissions(mode);
        writer.start_file(name, options)?;
        std::io::copy(&mut std::fs::File::open(&path)?, &mut writer)?;
    }
    writer.finish()?;
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn bundle(_output: &std::path::Path) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    Err("the engine is bundled on macOS, Windows and Linux only".into())
}
