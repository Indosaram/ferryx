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
    let app = if release {
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
        bundle_app(
            output,
            &metadata.target_directory().join("release"),
            "ferryx_engine",
            &bundle_metadata.helper_name,
            bundle_metadata.resources_path,
            info,
        )?
    } else {
        build_bundle(output, "ferryx_engine", info)?
    };
    // cef writes the executable name as the camera and microphone usage text; macOS shows it
    // in the permission dialog, so it has to say why.
    let plist = app.join("Contents").join("Info.plist");
    let text = std::fs::read_to_string(&plist)?;
    let text = set_plist_string(&text, "NSCameraUsageDescription", CAMERA_USAGE)
        .and_then(|text| set_plist_string(&text, "NSMicrophoneUsageDescription", MICROPHONE_USAGE))
        .ok_or("the engine Info.plist lacks the camera or microphone usage key")?;
    std::fs::write(&plist, text)?;
    Ok(app)
}

#[cfg(any(target_os = "macos", test))]
const CAMERA_USAGE: &str =
    "Websites you open in Ferryx's built-in browser can use the camera after you allow them.";
#[cfg(any(target_os = "macos", test))]
const MICROPHONE_USAGE: &str =
    "Websites you open in Ferryx's built-in browser can use the microphone after you allow them.";

/// Replaces the `<string>` value that follows `<key>key</key>` in an XML plist.
#[cfg(any(target_os = "macos", test))]
fn set_plist_string(plist: &str, key: &str, value: &str) -> Option<String> {
    let key_tag = format!("<key>{key}</key>");
    let after_key = plist.find(&key_tag)? + key_tag.len();
    let open = after_key + plist[after_key..].find("<string>")? + "<string>".len();
    let close = open + plist[open..].find("</string>")?;
    if !plist[after_key..open - "<string>".len()].trim().is_empty() {
        return None;
    }
    let escaped = value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    Some(format!("{}{escaped}{}", &plist[..open], &plist[close..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_strings_replace_only_their_key() {
        let plist = "<dict>\n\t<key>NSCameraUsageDescription</key>\n\t<string>ferryx_engine</string>\n\t<key>NSMicrophoneUsageDescription</key>\n\t<string>ferryx_engine</string>\n</dict>";
        let updated = set_plist_string(plist, "NSCameraUsageDescription", CAMERA_USAGE).expect("camera key");
        assert!(updated.contains(&format!("<key>NSCameraUsageDescription</key>\n\t<string>{CAMERA_USAGE}</string>")));
        assert!(updated.contains("<key>NSMicrophoneUsageDescription</key>\n\t<string>ferryx_engine</string>"));
        assert_eq!(set_plist_string(plist, "NSMissing", "x"), None);
        assert_eq!(set_plist_string("<key>A</key><key>B</key><string>x</string>", "A", "y"), None);
    }
}

#[cfg(target_os = "windows")]
fn bundle(output: &std::path::Path, release: bool) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    Ok(cef::build_util::win::build_bundle(output, "ferryx_engine", release)?)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn bundle(_output: &std::path::Path, _release: bool) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    Err("the engine is bundled on macOS and Windows only".into())
}
