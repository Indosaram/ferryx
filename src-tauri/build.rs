#[path = "native_terminal/build_ghostty.rs"]
mod build_ghostty;
mod build_platform;

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

fn validate_bundled_helpers_gate(repo_root: &std::path::Path) -> Result<(), String> {
    let manifest_path = repo_root.join("src-tauri/resources/helpers/manifest.json");
    if !manifest_path.is_file() {
        // Unlike the JS validator, which stays development-permissive, this gate is only
        // reached for release or FERRYX_ENFORCE_HELPER_GATE builds: those must ship staged
        // helper assets, so an absent manifest is a hard error, not a reason to skip.
        return Err(format!(
            "missing bundled helper manifest at {}; stage remote helpers before release packaging",
            manifest_path.display()
        ));
    }

    let script_path = repo_root.join("scripts/build-remote-helpers.mjs");
    if !script_path.is_file() {
        return Err(format!("missing validation script at {}", script_path.display()));
    }

    let (binary, args) = if std::process::Command::new("bun").arg("--version").output().is_ok() {
        ("bun", vec![script_path.to_string_lossy().to_string(), "--check-bundled".to_string(), repo_root.to_string_lossy().to_string()])
    } else {
        ("node", vec![script_path.to_string_lossy().to_string(), "--check-bundled".to_string(), repo_root.to_string_lossy().to_string()])
    };

    let output = std::process::Command::new(binary)
        .args(&args)
        .output()
        .map_err(|e| format!("failed to invoke {binary} helper freshness gate: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let msg = if !stderr.trim().is_empty() { stderr } else { stdout };
        return Err(format!("{msg}"));
    }

    Ok(())
}

fn main() {
    // Unit-test CLI builds must preserve Cargo's explicit target layout.
    let build_target = std::env::var("TARGET").expect("Cargo provides TARGET");
    if let Some(arg) = build_platform::cdylib_export_link_arg(&build_target) {
        println!("cargo:rustc-link-arg-cdylib={arg}");
    }
    println!("cargo::rustc-env=FERRYX_BUILD_TARGET={build_target}");
    println!("cargo::rustc-check-cfg=cfg(a10_wire_red)");
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    let repo_root = manifest_dir
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| manifest_dir.clone());

    let profile = std::env::var("PROFILE").unwrap_or_default();
    let is_release = profile == "release"
        || std::env::var("FERRYX_ENFORCE_HELPER_GATE").as_deref() == Ok("1");

    if is_release {
        if let Err(err) = validate_bundled_helpers_gate(&repo_root) {
            eprintln!("\n[helper asset freshness error] {err}\n");
            std::process::exit(1);
        }
    }

    // Windows test binaries do not receive tauri-build's embedded application
    // manifest, so the loader binds comctl32 v5 from System32 and fails to
    // launch with STATUS_ENTRYPOINT_NOT_FOUND (TaskDialogIndirect). Embed the
    // Common-Controls v6 side-by-side dependency into test targets explicitly.
    // Gated to the test profile: bin builds already carry tauri-build's own
    // manifest resource, and a linker-generated one would duplicate it
    // (CVT1100). Non-test builds are unaffected.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        println!("cargo:rustc-link-arg-tests=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-tests=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' publicKeyToken='6595b64144ccf1df' language='*' processorArchitecture='*'"
        );
    }
    // The remote gateway needs real VT parsing even without native GPU presentation.
    if let Err(err) = build_ghostty::build_ghostty_vt() {
        eprintln!("\n[ghostty build error] {err}\n");
        std::process::exit(1);
    }

    // tauri-build reads `DEP_TAURI_DEV`, which only exists when the `tauri` crate is linked, so
    // the desktop-free CLI build (`--no-default-features`) skips it entirely.
    if std::env::var_os("CARGO_FEATURE_DESKTOP").is_none() {
        return;
    }

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
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
