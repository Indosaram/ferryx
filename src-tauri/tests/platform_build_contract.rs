#[path = "../build_platform.rs"]
mod build_platform;

#[test]
fn windows_gnu_cdylib_disables_only_automatic_exports() {
    for target in ["x86_64-pc-windows-gnu", "i686-pc-windows-gnu"] {
        assert_eq!(build_platform::cdylib_export_link_arg(target), Some("-Wl,--exclude-all-symbols"));
    }
}

#[test]
fn other_linkers_are_unchanged() {
    for target in ["x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc", "aarch64-pc-windows-gnullvm", "x86_64-unknown-linux-gnu", "aarch64-apple-darwin", "x86_64-apple-darwin"] {
        assert_eq!(build_platform::cdylib_export_link_arg(target), None, "{target}");
    }
}

#[test]
fn export_policy_is_scoped_to_cdylib_not_executables() {
    let source = include_str!("../build.rs");
    assert!(source.contains("build_platform::cdylib_export_link_arg(&build_target)"));
    assert!(source.contains("cargo:rustc-link-arg-cdylib={arg}"));
    assert!(!source.contains("cargo:rustc-link-arg={arg}"));
}

#[test]
fn windows_msvc_preserves_debug_info_with_larger_pdb_pages() {
    for target in ["x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc", "i686-pc-windows-msvc"] {
        assert_eq!(build_platform::pdb_page_size_link_arg(target), Some("/PDBPAGESIZE:8192"));
    }
}

#[test]
fn pdb_page_size_does_not_affect_other_linkers() {
    for target in ["x86_64-pc-windows-gnu", "aarch64-pc-windows-gnullvm", "x86_64-unknown-linux-gnu", "aarch64-apple-darwin"] {
        assert_eq!(build_platform::pdb_page_size_link_arg(target), None, "{target}");
    }
}

#[test]
fn pdb_capacity_applies_to_library_and_executable_links() {
    let source = include_str!("../build.rs");
    assert!(source.contains("build_platform::pdb_page_size_link_arg(&build_target)"));
    assert!(source.contains("cargo:rustc-link-arg={pdb_arg}"));
}
