const LIB_SOURCE: &str = include_str!("../src/lib.rs");

const DEAD_MENU_ITEM_IDS: [&str; 2] = ["sidebar.left.toggle", "commandPalette.open"];
const DEAD_MENU_EVENTS: [&str; 2] = ["menu_toggle_sidebar", "menu_command_palette"];
const LIVE_MENU_EVENTS: [&str; 2] = ["menu_new_terminal_tab", "menu_close_tab"];

#[test]
fn app_menu_is_compiled_only_for_macos() {
    let definition = "#[cfg(target_os = \"macos\")]\nfn install_app_menu<R:";
    let installation = "#[cfg(target_os = \"macos\")]\n            install_app_menu(app)?;";

    assert!(LIB_SOURCE.contains(definition));
    assert!(LIB_SOURCE.contains(installation));
    assert!(!LIB_SOURCE.contains("#[cfg(desktop)]\nfn install_app_menu"));
}

#[test]
fn app_menu_builds_its_own_root_instead_of_extending_the_default_menu() {
    assert!(
        !LIB_SOURCE.contains("Menu::default("),
        "Menu::default already provides App/Edit/Window submenus, so appending our own \
         duplicates Edit and Window in the menu bar"
    );
}

#[test]
fn app_menu_never_claims_an_accelerator_without_a_frontend_listener() {
    for id in DEAD_MENU_ITEM_IDS {
        assert!(
            !LIB_SOURCE.contains(id),
            "menu item {id} owns its accelerator on macOS, which stops the webview from \
             ever receiving the keydown that ui/src/lib/shortcuts.ts binds"
        );
    }
    for event in DEAD_MENU_EVENTS {
        assert!(
            !LIB_SOURCE.contains(event),
            "{event} has no listener in ui/src, so emitting it silently drops the shortcut"
        );
    }
    for event in LIVE_MENU_EVENTS {
        assert!(
            LIB_SOURCE.contains(event),
            "{event} is consumed by ui/src/App.tsx and must keep its menu item"
        );
    }
}
