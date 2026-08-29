#[path = "native_terminal/build_ghostty.rs"]
mod build_ghostty;

// Mirrors tauri-build 2.x's windows-app-manifest.xml. Embedded via the linker
// for EVERY executable target (see below) instead of only bin targets.
const WINDOWS_APP_MANIFEST: &str = r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity
        type="win32"
        name="Microsoft.Windows.Common-Controls"
        version="6.0.0.0"
        processorArchitecture="*"
        publicKeyToken="6595b64144ccf1df"
        language="*"
      />
    </dependentAssembly>
  </dependency>
</assembly>
"#;

fn main() {
    // Windows test binaries do not receive tauri-build's embedded application
    // manifest, so the loader binds comctl32 v5 from System32 and fails to
    // launch with STATUS_ENTRYPOINT_NOT_FOUND (TaskDialogIndirect). Embed the
    // Common-Controls v6 side-by-side dependency into test targets explicitly.
    // Gated to the test profile: bin builds already carry tauri-build's own
    // manifest resource, and a linker-generated one would duplicate it
    // (CVT1100). Non-test builds are unaffected.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-arg-tests=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-tests=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' publicKeyToken='6595b64144ccf1df' language='*' processorArchitecture='*'"
        );
    }
    if std::env::var_os("CARGO_FEATURE_NATIVE_TERMINAL").is_some() {
        if let Err(err) = build_ghostty::build_ghostty_vt() {
            eprintln!("\n[ghostty build error] {err}\n");
            std::process::exit(1);
        }
    }

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // Embed ONE application manifest for EVERY executable target (bins and
        // unit-test executables alike). tauri-build's resource manifest only
        // reaches bin targets; without it, unit-test executables bind comctl32
        // v5 from System32 and die at load with STATUS_ENTRYPOINT_NOT_FOUND
        // (TaskDialogIndirect, pulled in by muda). We therefore build
        // tauri-build WITHOUT its own manifest resource (which would duplicate
        // the linker-generated one, CVT1100) and embed the same manifest
        // content through the linker for all targets.
        let manifest_path = std::path::Path::new(&std::env::var("OUT_DIR").expect("OUT_DIR"))
            .join("ferryx-app-manifest.xml");
        std::fs::write(&manifest_path, WINDOWS_APP_MANIFEST).expect("write app manifest");
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg=/MANIFESTINPUT:{}",
            manifest_path.display()
        );
        let attrs = tauri_build::Attributes::new()
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
        tauri_build::try_build(attrs).expect("tauri build failed");
    } else {
        tauri_build::build();
    }
}
