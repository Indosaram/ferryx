//! Verifies the bundled helper manifest through the production resolver.
fn main() {
    let base = std::env::args()
        .nth(1)
        .expect("usage: helper_manifest_check <helpers dir>");
    let targets = [
        ("x86_64-unknown-linux-gnu", ferryx_lib::ssh::helper_assets::HelperTarget::LinuxX86_64),
        ("aarch64-unknown-linux-gnu", ferryx_lib::ssh::helper_assets::HelperTarget::LinuxAarch64),
        ("aarch64-apple-darwin", ferryx_lib::ssh::helper_assets::HelperTarget::DarwinAarch64),
        ("x86_64-apple-darwin", ferryx_lib::ssh::helper_assets::HelperTarget::DarwinX86_64),
        ("x86_64-pc-windows-msvc", ferryx_lib::ssh::helper_assets::HelperTarget::WindowsX64),
    ];
    let mut failures = 0;
    for (label, target) in targets {
        match ferryx_lib::ssh::helper_assets::resolve_helper_asset(std::path::Path::new(&base), target) {
            Ok(resolved) => println!(
                "OK   {label} -> {} ({} bytes, sha256 {})",
                resolved.binary_path.display(),
                resolved.byte_length,
                &resolved.sha256[..16]
            ),
            Err(error) => {
                failures += 1;
                println!("FAIL {label} -> {}", error.message);
            }
        }
    }
    println!("HELPER_MANIFEST_CHECK failures={failures}");
    if failures > 0 {
        std::process::exit(1);
    }
}
