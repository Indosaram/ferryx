const APP_LIB: &str = include_str!("../src/lib.rs");

#[test]
fn native_application_menu_is_macos_only() {
    assert!(
        APP_LIB.contains("#[cfg(target_os = \"macos\")]\nfn install_app_menu"),
        "the native application menu must only be compiled for macOS"
    );
    assert!(
        APP_LIB.contains(
            "#[cfg(target_os = \"macos\")]\n            install_app_menu(app)?;"
        ),
        "Windows and Linux must not attach the macOS-style menu to their windows"
    );
}
