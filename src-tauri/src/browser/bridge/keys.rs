//! Single source of truth for app-chrome ("a-chrome") browser shortcuts, ported from the
//! page key table that used to live in the guest bridge (design 2.1). Native key sources
//! (macOS NSEvent monitor, Linux GtkWidget key-press-event, Windows AcceleratorKeyPressed)
//! translate their event into a [`Chord`] and call [`chord_to_action`]. Native events cannot
//! be synthesized by page script, which is what closes REVIEW M2 for shortcuts.
//!
//! Self-scoped actions (back, forward, reload, reload-hard) are never returned here: they
//! stay on the page route because the page can already do them itself.
//! Native sources must not call this for IME composition events (keyCode 229 equivalent).

/// Platform-neutral key identity: the physical key a chord names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyId {
    /// ASCII letter, lowercase.
    Letter(char),
    /// Top-row digit 0..=9.
    Digit(u8),
    Comma,
    Equal,
    Minus,
    BracketLeft,
    BracketRight,
    Tab,
    PageUp,
    PageDown,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    pub key: KeyId,
    pub ctrl: bool,
    pub meta: bool,
    pub alt: bool,
    pub shift: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyPlatform {
    Mac,
    Windows,
    Linux,
}

impl KeyPlatform {
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            KeyPlatform::Mac
        } else if cfg!(target_os = "windows") {
            KeyPlatform::Windows
        } else {
            KeyPlatform::Linux
        }
    }
}

const WORKSPACE_SELECT: [&str; 9] = [
    "workspace-select-1",
    "workspace-select-2",
    "workspace-select-3",
    "workspace-select-4",
    "workspace-select-5",
    "workspace-select-6",
    "workspace-select-7",
    "workspace-select-8",
    "workspace-select-9",
];
const TAB_SELECT: [&str; 9] = [
    "tab-select-1",
    "tab-select-2",
    "tab-select-3",
    "tab-select-4",
    "tab-select-5",
    "tab-select-6",
    "tab-select-7",
    "tab-select-8",
    "tab-select-9",
];

/// Maps a native chord to the app-chrome action the guest key table used to emit, or None.
pub fn chord_to_action(chord: Chord, platform: KeyPlatform) -> Option<&'static str> {
    let Chord {
        key,
        ctrl,
        meta,
        alt,
        shift,
    } = chord;
    let mac = platform == KeyPlatform::Mac;
    let primary = if mac { meta && !ctrl } else { ctrl && !meta };

    if primary && !alt {
        let action = if !shift {
            match key {
                KeyId::Letter('t') => Some("tab-new-browser"),
                KeyId::Letter('w') => Some("tab-close"),
                KeyId::Letter('k') => Some("command-palette"),
                KeyId::Letter('b') => Some("sidebar-toggle"),
                KeyId::Comma => Some("settings-toggle"),
                KeyId::Letter('d') => Some("split-right"),
                KeyId::Equal => Some("zoom-in"),
                KeyId::Minus => Some("zoom-out"),
                KeyId::Digit(0) => Some("zoom-reset"),
                KeyId::Letter('l') => Some("focus-address"),
                KeyId::Letter('f') => Some("find"),
                _ => None,
            }
        } else {
            match key {
                KeyId::Letter('t') => Some("tab-reopen-closed"),
                KeyId::Letter('d') => Some("split-down"),
                KeyId::Equal => Some("zoom-in"),
                KeyId::BracketRight => Some("tab-next"),
                KeyId::BracketLeft => Some("tab-previous"),
                _ => None,
            }
        };
        if action.is_some() {
            return action;
        }
    }

    if let KeyId::Digit(digit @ 1..=9) = key {
        let index = usize::from(digit - 1);
        let workspace_mod = if mac {
            meta && !ctrl && !alt
        } else {
            alt && !ctrl && !meta
        };
        if !shift && workspace_mod {
            return Some(WORKSPACE_SELECT[index]);
        }
        if ctrl && !meta && !alt && !shift {
            return Some(TAB_SELECT[index]);
        }
    }

    if ctrl && !meta && !alt {
        match key {
            KeyId::Tab => return Some(if shift { "tab-previous" } else { "tab-next" }),
            KeyId::PageDown => return Some("tab-next"),
            KeyId::PageUp => return Some("tab-previous"),
            _ => {}
        }
    }
    None
}

/// macOS virtual keyCode (ANSI layout position, independent of the input source) to
/// [`KeyId`]. Pure data so it is testable on every OS; used by the macOS key monitor.
pub fn mac_key_id(key_code: u16) -> KeyId {
    const LETTERS: [(u16, char); 26] = [
        (0, 'a'), (11, 'b'), (8, 'c'), (2, 'd'), (14, 'e'), (3, 'f'), (5, 'g'), (4, 'h'),
        (34, 'i'), (38, 'j'), (40, 'k'), (37, 'l'), (46, 'm'), (45, 'n'), (31, 'o'),
        (35, 'p'), (12, 'q'), (15, 'r'), (1, 's'), (17, 't'), (32, 'u'), (9, 'v'),
        (13, 'w'), (7, 'x'), (16, 'y'), (6, 'z'),
    ];
    const DIGITS: [u16; 10] = [29, 18, 19, 20, 21, 23, 22, 26, 28, 25];
    if let Some((_, letter)) = LETTERS.iter().find(|(code, _)| *code == key_code) {
        return KeyId::Letter(*letter);
    }
    if let Some(digit) = DIGITS.iter().position(|code| *code == key_code) {
        return KeyId::Digit(digit as u8);
    }
    match key_code {
        43 => KeyId::Comma,
        24 => KeyId::Equal,
        27 => KeyId::Minus,
        33 => KeyId::BracketLeft,
        30 => KeyId::BracketRight,
        48 => KeyId::Tab,
        116 => KeyId::PageUp,
        121 => KeyId::PageDown,
        _ => KeyId::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLATFORMS: [KeyPlatform; 3] = [KeyPlatform::Mac, KeyPlatform::Windows, KeyPlatform::Linux];
    const SELF_SCOPED: [&str; 4] = ["back", "forward", "reload", "reload-hard"];

    fn chord(key: KeyId, ctrl: bool, meta: bool, alt: bool, shift: bool) -> Chord {
        Chord { key, ctrl, meta, alt, shift }
    }

    /// The platform's primary modifier: Cmd on macOS, Ctrl elsewhere.
    fn primary(platform: KeyPlatform, key: KeyId, shift: bool) -> Chord {
        let mac = platform == KeyPlatform::Mac;
        chord(key, !mac, mac, false, shift)
    }

    #[test]
    fn primary_chords_map_identically_on_every_platform() {
        let table: [(KeyId, bool, &str); 16] = [
            (KeyId::Letter('t'), false, "tab-new-browser"),
            (KeyId::Letter('w'), false, "tab-close"),
            (KeyId::Letter('k'), false, "command-palette"),
            (KeyId::Letter('b'), false, "sidebar-toggle"),
            (KeyId::Comma, false, "settings-toggle"),
            (KeyId::Letter('d'), false, "split-right"),
            (KeyId::Equal, false, "zoom-in"),
            (KeyId::Minus, false, "zoom-out"),
            (KeyId::Digit(0), false, "zoom-reset"),
            (KeyId::Letter('l'), false, "focus-address"),
            (KeyId::Letter('f'), false, "find"),
            (KeyId::Letter('t'), true, "tab-reopen-closed"),
            (KeyId::Letter('d'), true, "split-down"),
            (KeyId::Equal, true, "zoom-in"),
            (KeyId::BracketRight, true, "tab-next"),
            (KeyId::BracketLeft, true, "tab-previous"),
        ];
        for platform in PLATFORMS {
            for (key, shift, action) in table {
                assert_eq!(
                    chord_to_action(primary(platform, key, shift), platform),
                    Some(action),
                    "{platform:?} {key:?} shift={shift}"
                );
            }
        }
    }

    #[test]
    fn workspace_and_tab_selection_follow_platform_modifiers() {
        for digit in 1..=9u8 {
            let key = KeyId::Digit(digit);
            let ws = format!("workspace-select-{digit}");
            let tab = format!("tab-select-{digit}");
            assert_eq!(chord_to_action(chord(key, false, true, false, false), KeyPlatform::Mac), Some(ws.as_str()));
            for platform in [KeyPlatform::Windows, KeyPlatform::Linux] {
                assert_eq!(chord_to_action(chord(key, false, false, true, false), platform), Some(ws.as_str()));
                // Cmd/Meta+digit is not a workspace chord off macOS.
                assert_eq!(chord_to_action(chord(key, false, true, false, false), platform), None);
            }
            for platform in PLATFORMS {
                assert_eq!(chord_to_action(chord(key, true, false, false, false), platform), Some(tab.as_str()));
            }
        }
        // Alt+digit is not a workspace chord on macOS (it types characters there).
        assert_eq!(chord_to_action(chord(KeyId::Digit(1), false, false, true, false), KeyPlatform::Mac), None);
    }

    #[test]
    fn ctrl_tab_and_page_keys_cycle_tabs_everywhere() {
        for platform in PLATFORMS {
            assert_eq!(chord_to_action(chord(KeyId::Tab, true, false, false, false), platform), Some("tab-next"));
            assert_eq!(chord_to_action(chord(KeyId::Tab, true, false, false, true), platform), Some("tab-previous"));
            assert_eq!(chord_to_action(chord(KeyId::PageDown, true, false, false, false), platform), Some("tab-next"));
            assert_eq!(chord_to_action(chord(KeyId::PageUp, true, false, false, false), platform), Some("tab-previous"));
        }
    }

    #[test]
    fn non_primary_and_unmodified_keys_map_to_nothing() {
        for platform in PLATFORMS {
            for key in [KeyId::Letter('t'), KeyId::Letter('w'), KeyId::Tab, KeyId::Digit(1), KeyId::Other] {
                assert_eq!(chord_to_action(chord(key, false, false, false, false), platform), None);
                assert_eq!(chord_to_action(chord(key, false, false, false, true), platform), None);
            }
            // Primary + Alt is a different chord.
            let mut alt = primary(platform, KeyId::Letter('w'), false);
            alt.alt = true;
            assert_eq!(chord_to_action(alt, platform), None);
        }
        // The other platform's primary modifier does not count.
        assert_eq!(chord_to_action(chord(KeyId::Letter('w'), true, false, false, false), KeyPlatform::Mac), None);
        assert_eq!(chord_to_action(chord(KeyId::Letter('w'), false, true, false, false), KeyPlatform::Linux), None);
    }

    #[test]
    fn self_scoped_actions_are_never_returned() {
        let keys = [
            KeyId::Letter('r'), KeyId::BracketLeft, KeyId::BracketRight, KeyId::Other,
        ];
        for platform in PLATFORMS {
            for key in keys {
                for bits in 0u8..16 {
                    let c = chord(key, bits & 1 != 0, bits & 2 != 0, bits & 4 != 0, bits & 8 != 0);
                    if let Some(action) = chord_to_action(c, platform) {
                        assert!(!SELF_SCOPED.contains(&action), "{c:?} -> {action}");
                    }
                }
            }
        }
    }

    #[test]
    fn mac_key_codes_map_to_physical_keys() {
        assert_eq!(mac_key_id(13), KeyId::Letter('w'));
        assert_eq!(mac_key_id(17), KeyId::Letter('t'));
        assert_eq!(mac_key_id(37), KeyId::Letter('l'));
        assert_eq!(mac_key_id(3), KeyId::Letter('f'));
        assert_eq!(mac_key_id(29), KeyId::Digit(0));
        assert_eq!(mac_key_id(18), KeyId::Digit(1));
        assert_eq!(mac_key_id(25), KeyId::Digit(9));
        assert_eq!(mac_key_id(43), KeyId::Comma);
        assert_eq!(mac_key_id(24), KeyId::Equal);
        assert_eq!(mac_key_id(27), KeyId::Minus);
        assert_eq!(mac_key_id(33), KeyId::BracketLeft);
        assert_eq!(mac_key_id(30), KeyId::BracketRight);
        assert_eq!(mac_key_id(48), KeyId::Tab);
        assert_eq!(mac_key_id(116), KeyId::PageUp);
        assert_eq!(mac_key_id(121), KeyId::PageDown);
        assert_eq!(mac_key_id(53), KeyId::Other);
        // Cmd+L on a guest becomes focus-address through the single table.
        let chord = Chord { key: mac_key_id(37), ctrl: false, meta: true, alt: false, shift: false };
        assert_eq!(chord_to_action(chord, KeyPlatform::Mac), Some("focus-address"));
    }
}
