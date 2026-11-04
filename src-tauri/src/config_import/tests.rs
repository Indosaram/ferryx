use super::*;

#[test]
fn ghostty_maps_font_theme_cursor() {
    let p = parse("ghostty", br##"
font-family = JetBrains Mono
font-size = 15
background = #102030
foreground = #fafafa
cursor-style = bar
palette = 1=#ab1234
command = touch /never-run
keybind = ctrl+x=close_surface
config-file = /never-read
theme = external-theme
scrollback-limit = 123456
"##).unwrap();
    println!("{}", serde_json::to_string(&p.settings).unwrap());
    assert_eq!(p.settings.font_family.as_deref(), Some("JetBrains Mono"));
    assert_eq!(p.settings.font_size, Some(15.0));
    assert_eq!(p.settings.cursor_style.as_deref(), Some("bar"));
    assert_eq!(p.settings.theme["background"], "#102030");
    assert_eq!(p.settings.theme["red"], "#ab1234");
    assert_eq!(p.settings.scrollback, None);
    assert_eq!(p.skipped, ["command", "keybind", "config-file", "theme", "scrollback-limit"]);
}

fn iterm_fixture() -> plist::Value {
    plist::Value::from_reader_xml(br#"<?xml version="1.0"?><plist version="1.0"><dict>
<key>New Bookmarks</key><array><dict>
<key>Normal Font</key><string>Menlo 16</string>
<key>Cursor Type</key><integer>2</integer>
<key>Scrollback Lines</key><integer>12000</integer>
<key>Background Color</key><dict>
<key>Red Component</key><real>0.0</real>
<key>Green Component</key><real>0.5</real>
<key>Blue Component</key><real>1.0</real></dict>
<key>Command</key><string>touch /never-run</string>
</dict></array></dict></plist>"#.as_slice()).unwrap()
}

#[test]
fn iterm2_xml_and_binary_profiles() {
    let fixture = iterm_fixture();
    for binary in [false, true] {
        let mut bytes = Vec::new();
        if binary { fixture.to_writer_binary(&mut bytes).unwrap(); }
        else { fixture.to_writer_xml(&mut bytes).unwrap(); }
        let p = parse("iterm2", &bytes).unwrap();
        assert_eq!(p.settings.font_family.as_deref(), Some("Menlo"));
        assert_eq!(p.settings.font_size, Some(16.0));
        assert_eq!(p.settings.cursor_style.as_deref(), Some("underline"));
        assert_eq!(p.settings.scrollback, Some(12000));
        assert_eq!(p.settings.theme["background"], "#0080ff");
        assert!(p.skipped.contains(&"Command".into()));
    }
}

#[test]
fn malformed_iterm2_is_parse_error() {
    for bytes in [b"<plist><dict>".as_slice(), b"not a plist", b"<plist><dict/></plist>"] {
        assert_eq!(parse("iterm2", bytes).unwrap_err(), "IMPORT_PARSE_ERROR");
    }
}

#[test]
fn wezterm_static_table_maps_settings() {
    let p = parse("wezterm", br##"return {
font = 'Menlo', font_size = 16, scrollback_lines = 20000,
default_cursor_style = 'SteadyUnderline',
colors = { background = '#102030', ansi = {'#000000', '#ff0000'} },
}"##).unwrap();
    assert_eq!(p.settings.font_family.as_deref(), Some("Menlo"));
    assert_eq!(p.settings.font_size, Some(16.0));
    assert_eq!(p.settings.scrollback, Some(20000));
    assert_eq!(p.settings.cursor_style.as_deref(), Some("underline"));
    assert_eq!(p.settings.theme["background"], "#102030");
    assert_eq!(p.settings.theme["red"], "#ff0000");
}

#[test]
fn wezterm_os_execute_is_skipped_without_creating_canary() {
    let dir = tempfile::tempdir().unwrap();
    let canary = dir.path().join("canary");
    let source = format!("os.execute('touch {}'); return {{font_size=16}}", canary.display());
    let p = parse("wezterm", source.as_bytes()).unwrap();
    assert_eq!(p.settings, Settings::default());
    assert!(!p.skipped.is_empty());
    assert!(!canary.exists());
    for source in ["return {font = wezterm.font('Menlo')}", "while true do end", "return {font_size=(function() return 16 end)()}", "return {font_size=12+4}"] {
        let p = parse("wezterm", source.as_bytes()).unwrap();
        assert_eq!(p.settings, Settings::default());
        assert!(!p.skipped.is_empty());
    }
}

#[test]
fn import_rejects_oversized_and_invalid_inputs() {
    assert!(parse("ghostty", &vec![b' '; 1024 * 1024 + 1]).is_err());
    assert!(parse("ghostty", &[0xff]).is_err());
    assert!(parse("ghostty", b"broken assignment").is_err());
    let p = parse("ghostty", b"font-size = NaN\ncursor-style = unknown\nbackground = not-a-color").unwrap();
    assert_eq!(p.settings, Settings::default());
    assert_eq!(p.skipped.len(), 3);
}
