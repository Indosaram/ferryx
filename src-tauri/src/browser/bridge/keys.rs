//! Single source of truth for app-chrome ("a-chrome") browser shortcuts, ported from the
//! page key table that used to live in the guest bridge (design 2.1). Native key sources
//! (macOS NSEvent monitor, Linux GtkWidget key-press-event, Windows AcceleratorKeyPressed)
//! translate their event into a [`Chord`] and call [`chord_to_action`]. Native events cannot
//! be synthesized by page script, which is what closes REVIEW M2 for shortcuts.
//!
//! Self-scoped actions (back, forward, reload, reload-hard) are never returned here: they
//! stay on the page route because the page can already do them itself.
//! Native sources must not call this for IME composition events (keyCode 229 equivalent).

use std::collections::HashMap;
use std::sync::{PoisonError, RwLock};

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

#[cfg(test)]
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
#[cfg(test)]
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

/// A user binding as the UI shortcut registry stores it (ui/src/lib/shortcutOverrides.ts):
/// `mod` is Cmd on macOS and Ctrl elsewhere, `control` is the literal Control key.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct NativeBinding {
    pub key: String,
    #[serde(default, rename = "mod")]
    pub primary: bool,
    #[serde(default)]
    pub control: bool,
    #[serde(default)]
    pub alt: bool,
    #[serde(default)]
    pub shift: bool,
}

/// Sparse user overrides keyed by TS action id (ui/src/lib/shortcuts.ts `ShortcutActionId`).
/// A binding replaces every default chord of the action (aliases included); `None` unbinds it,
/// so its chord is no longer intercepted and reaches the webview/PTY. Absent ids keep the
/// default chords, which equal the legacy hardcoded predicates.
pub type NativeOverrides = HashMap<String, Option<NativeBinding>>;

/// Read by every native key source, including the Windows accelerator callback thread.
#[cfg(not(test))]
static NATIVE_OVERRIDES: RwLock<Option<NativeOverrides>> = RwLock::new(None);

// Unit tests run in parallel threads and many assert the default table through
// [`chord_to_action`]; a per-thread table keeps a test that pushes overrides from leaking
// into them.
#[cfg(test)]
thread_local! {
    static NATIVE_OVERRIDES: RwLock<Option<NativeOverrides>> = const { RwLock::new(None) };
}

fn with_overrides_lock<T>(use_lock: impl FnOnce(&RwLock<Option<NativeOverrides>>) -> T) -> T {
    #[cfg(not(test))]
    {
        use_lock(&NATIVE_OVERRIDES)
    }
    #[cfg(test)]
    {
        NATIVE_OVERRIDES.with(use_lock)
    }
}

/// Replaces the user overrides; `None` or an empty map restores the legacy defaults.
pub fn set_native_overrides(table: Option<NativeOverrides>) {
    let table = table.filter(|table| !table.is_empty());
    with_overrides_lock(|lock| {
        *lock.write().unwrap_or_else(PoisonError::into_inner) = table;
    });
}

fn with_native_overrides<T>(read: impl FnOnce(Option<&NativeOverrides>) -> T) -> T {
    with_overrides_lock(|lock| {
        let guard = lock.read().unwrap_or_else(PoisonError::into_inner);
        read(guard.as_ref())
    })
}

/// The single mapping between TS action ids and the ids [`chord_to_action`] returns.
pub const BROWSER_ACTION_IDS: [(&str, &str); 33] = [
    ("tab.newBrowser", "tab-new-browser"),
    ("tab.close", "tab-close"),
    ("commandPalette.open", "command-palette"),
    ("sidebar.left.toggle", "sidebar-toggle"),
    ("settings.toggle", "settings-toggle"),
    ("terminal.splitRight", "split-right"),
    ("terminal.splitDown", "split-down"),
    ("zoom.in", "zoom-in"),
    ("zoom.out", "zoom-out"),
    ("zoom.reset", "zoom-reset"),
    ("browser.focusAddress", "focus-address"),
    ("browser.find", "find"),
    ("tab.reopenClosed", "tab-reopen-closed"),
    ("tab.next", "tab-next"),
    ("tab.previous", "tab-previous"),
    ("workspace.select1", "workspace-select-1"),
    ("workspace.select2", "workspace-select-2"),
    ("workspace.select3", "workspace-select-3"),
    ("workspace.select4", "workspace-select-4"),
    ("workspace.select5", "workspace-select-5"),
    ("workspace.select6", "workspace-select-6"),
    ("workspace.select7", "workspace-select-7"),
    ("workspace.select8", "workspace-select-8"),
    ("workspace.select9", "workspace-select-9"),
    ("tab.select1", "tab-select-1"),
    ("tab.select2", "tab-select-2"),
    ("tab.select3", "tab-select-3"),
    ("tab.select4", "tab-select-4"),
    ("tab.select5", "tab-select-5"),
    ("tab.select6", "tab-select-6"),
    ("tab.select7", "tab-select-7"),
    ("tab.select8", "tab-select-8"),
    ("tab.select9", "tab-select-9"),
];

/// TS action id -> the shortcut_dispatch event (and digit payload) the macOS key monitor
/// emits. Order is the legacy predicate order, which decides ties between candidate keys.
pub const MONITOR_ACTION_IDS: [(&str, &str, Option<u8>); 27] = [
    ("tab.close", "menu_close_tab", None),
    ("workspace.select1", "menu_select_worktree", Some(1)),
    ("workspace.select2", "menu_select_worktree", Some(2)),
    ("workspace.select3", "menu_select_worktree", Some(3)),
    ("workspace.select4", "menu_select_worktree", Some(4)),
    ("workspace.select5", "menu_select_worktree", Some(5)),
    ("workspace.select6", "menu_select_worktree", Some(6)),
    ("workspace.select7", "menu_select_worktree", Some(7)),
    ("workspace.select8", "menu_select_worktree", Some(8)),
    ("workspace.select9", "menu_select_worktree", Some(9)),
    ("tab.select1", "menu_select_tab", Some(1)),
    ("tab.select2", "menu_select_tab", Some(2)),
    ("tab.select3", "menu_select_tab", Some(3)),
    ("tab.select4", "menu_select_tab", Some(4)),
    ("tab.select5", "menu_select_tab", Some(5)),
    ("tab.select6", "menu_select_tab", Some(6)),
    ("tab.select7", "menu_select_tab", Some(7)),
    ("tab.select8", "menu_select_tab", Some(8)),
    ("tab.select9", "menu_select_tab", Some(9)),
    ("tab.next", "menu_next_tab", None),
    ("tab.previous", "menu_prev_tab", None),
    ("tab.newTerminal", "menu_new_terminal_tab", None),
    ("terminal.splitRight", "menu_split_right", None),
    ("terminal.splitDown", "menu_split_down", None),
    ("commandPalette.open", "menu_command_palette", None),
    ("sidebar.left.toggle", "menu_toggle_sidebar", None),
    ("settings.toggle", "menu_open_settings", None),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativeConsumer {
    /// [`chord_to_action`]: guest browser key sources on every platform.
    BrowserGuest,
    /// The macOS NSEvent monitor on the main window.
    MacMonitor,
}

fn primary_chord(platform: KeyPlatform, key: KeyId, shift: bool) -> Chord {
    let mac = platform == KeyPlatform::Mac;
    Chord { key, ctrl: !mac, meta: mac, alt: false, shift }
}

fn ctrl_chord(key: KeyId, shift: bool) -> Chord {
    Chord { key, ctrl: true, meta: false, alt: false, shift }
}

fn select_digit(id: &str, prefix: &str) -> Option<u8> {
    match id.strip_prefix(prefix)?.as_bytes() {
        [digit @ b'1'..=b'9'] => Some(digit - b'0'),
        _ => None,
    }
}

/// The chords each consumer intercepted before user bindings existed.
fn default_chords(consumer: NativeConsumer, id: &str, platform: KeyPlatform) -> Vec<Chord> {
    use KeyId::{BracketLeft, BracketRight, Comma, Digit, Equal, Letter, Minus, PageDown, PageUp, Tab};
    let p = |key, shift| primary_chord(platform, key, shift);
    let browser = consumer == NativeConsumer::BrowserGuest;
    if let Some(digit) = select_digit(id, "workspace.select") {
        let mac = platform == KeyPlatform::Mac;
        return vec![Chord { key: Digit(digit), ctrl: false, meta: mac, alt: !mac, shift: false }];
    }
    if let Some(digit) = select_digit(id, "tab.select") {
        return vec![ctrl_chord(Digit(digit), false)];
    }
    match id {
        "tab.newBrowser" if browser => vec![p(Letter('t'), false)],
        "tab.newTerminal" if !browser => vec![p(Letter('t'), false)],
        "tab.close" => vec![p(Letter('w'), false)],
        "commandPalette.open" => vec![p(Letter('k'), false)],
        "sidebar.left.toggle" => vec![p(Letter('b'), false)],
        "settings.toggle" => vec![p(Comma, false)],
        "terminal.splitRight" => vec![p(Letter('d'), false)],
        "terminal.splitDown" => vec![p(Letter('d'), true)],
        "zoom.in" if browser => vec![p(Equal, false), p(Equal, true)],
        "zoom.out" if browser => vec![p(Minus, false)],
        "zoom.reset" if browser => vec![p(Digit(0), false)],
        "browser.focusAddress" if browser => vec![p(Letter('l'), false)],
        "browser.find" if browser => vec![p(Letter('f'), false)],
        "tab.reopenClosed" if browser => vec![p(Letter('t'), true)],
        // The legacy guest table ignored Shift on Ctrl+PageUp/PageDown.
        "tab.next" if browser => vec![
            p(BracketRight, true),
            ctrl_chord(Tab, false),
            ctrl_chord(PageDown, false),
            ctrl_chord(PageDown, true),
        ],
        "tab.previous" if browser => vec![
            p(BracketLeft, true),
            ctrl_chord(Tab, true),
            ctrl_chord(PageUp, false),
            ctrl_chord(PageUp, true),
        ],
        "tab.next" => vec![ctrl_chord(Tab, false), p(BracketRight, true)],
        "tab.previous" => vec![ctrl_chord(Tab, true), p(BracketLeft, true)],
        _ => Vec::new(),
    }
}

/// The physical key a UI binding `key` names (KeyboardEvent.key spelling).
fn binding_key(key: &str) -> Option<KeyId> {
    let mut chars = key.chars();
    if let (Some(single), None) = (chars.next(), chars.next()) {
        if single.is_ascii_alphabetic() {
            return Some(KeyId::Letter(single.to_ascii_lowercase()));
        }
        if let Some(digit) = single.to_digit(10) {
            return Some(KeyId::Digit(digit as u8));
        }
        return match single {
            ',' => Some(KeyId::Comma),
            '=' | '+' => Some(KeyId::Equal),
            '-' => Some(KeyId::Minus),
            '[' => Some(KeyId::BracketLeft),
            ']' => Some(KeyId::BracketRight),
            _ => None,
        };
    }
    match key {
        "Tab" => Some(KeyId::Tab),
        "PageUp" => Some(KeyId::PageUp),
        "PageDown" => Some(KeyId::PageDown),
        _ => None,
    }
}

/// The concrete chord a binding means on `platform`; None when the key is not one a native
/// source can report (such a binding is never intercepted natively).
fn binding_chord(binding: &NativeBinding, platform: KeyPlatform) -> Option<Chord> {
    let key = binding_key(&binding.key)?;
    Some(if platform == KeyPlatform::Mac {
        Chord { key, ctrl: binding.control, meta: binding.primary, alt: binding.alt, shift: binding.shift }
    } else {
        Chord { key, ctrl: binding.primary || binding.control, meta: false, alt: binding.alt, shift: binding.shift }
    })
}

/// First action (in `ids` order, which is the legacy predicate order) bound to any candidate
/// chord. Overridden actions are checked before defaults, so a chord the user moved onto an
/// action wins over a default it shadows.
fn lookup(
    ids: impl Iterator<Item = &'static str> + Clone,
    consumer: NativeConsumer,
    overrides: Option<&NativeOverrides>,
    candidates: &[Chord],
    platform: KeyPlatform,
) -> Option<&'static str> {
    if let Some(overrides) = overrides {
        let overridden = ids.clone().find(|id| match overrides.get(*id) {
            Some(Some(binding)) => {
                binding_chord(binding, platform).is_some_and(|chord| candidates.contains(&chord))
            }
            _ => false,
        });
        if overridden.is_some() {
            return overridden;
        }
    }
    ids.filter(|id| !overrides.is_some_and(|overrides| overrides.contains_key(*id)))
        .find(|id| {
            default_chords(consumer, id, platform)
                .iter()
                .any(|chord| candidates.contains(chord))
        })
}

/// Maps a native chord to the app-chrome action for a focused browser guest, or None.
/// Follows the user's bindings pushed through [`set_native_overrides`].
pub fn chord_to_action(chord: Chord, platform: KeyPlatform) -> Option<&'static str> {
    with_native_overrides(|overrides| chord_to_action_in(overrides, chord, platform))
}

pub fn chord_to_action_in(
    overrides: Option<&NativeOverrides>,
    chord: Chord,
    platform: KeyPlatform,
) -> Option<&'static str> {
    let ids = BROWSER_ACTION_IDS.iter().map(|(ts, _)| *ts);
    let ts = lookup(ids, NativeConsumer::BrowserGuest, overrides, &[chord], platform)?;
    BROWSER_ACTION_IDS
        .iter()
        .find(|(id, _)| *id == ts)
        .map(|(_, native)| *native)
}

/// The shortcut_dispatch event (and digit) the macOS main-window monitor emits for any of
/// `candidates` (one physical key may be reported by keyCode and by character), or None.
pub fn monitor_action(candidates: &[Chord]) -> Option<(&'static str, Option<u8>)> {
    with_native_overrides(|overrides| monitor_action_in(overrides, candidates))
}

pub fn monitor_action_in(
    overrides: Option<&NativeOverrides>,
    candidates: &[Chord],
) -> Option<(&'static str, Option<u8>)> {
    let ids = MONITOR_ACTION_IDS.iter().map(|(ts, ..)| *ts);
    let ts = lookup(ids, NativeConsumer::MacMonitor, overrides, candidates, KeyPlatform::Mac)?;
    MONITOR_ACTION_IDS
        .iter()
        .find(|(id, ..)| *id == ts)
        .map(|(_, event, digit)| (*event, *digit))
}

/// The menu accelerator for a TS action: `default` unless overridden; None when unbound.
pub fn menu_accelerator(ts_id: &str, default: &str) -> Option<String> {
    with_native_overrides(|overrides| menu_accelerator_in(overrides, ts_id, default))
}

pub fn menu_accelerator_in(
    overrides: Option<&NativeOverrides>,
    ts_id: &str,
    default: &str,
) -> Option<String> {
    match overrides.and_then(|overrides| overrides.get(ts_id)) {
        None => Some(default.to_string()),
        Some(None) => None,
        Some(Some(binding)) => accelerator_string(binding),
    }
}

/// Tauri/muda accelerator syntax for a binding (`CmdOrCtrl` is the `mod` key).
fn accelerator_string(binding: &NativeBinding) -> Option<String> {
    let key = match binding_key(&binding.key)? {
        KeyId::Letter(letter) => letter.to_ascii_uppercase().to_string(),
        KeyId::Digit(digit) => digit.to_string(),
        KeyId::Comma => "Comma".to_string(),
        KeyId::Equal => "Equal".to_string(),
        KeyId::Minus => "Minus".to_string(),
        KeyId::BracketLeft => "BracketLeft".to_string(),
        KeyId::BracketRight => "BracketRight".to_string(),
        KeyId::Tab => "Tab".to_string(),
        KeyId::PageUp => "PageUp".to_string(),
        KeyId::PageDown => "PageDown".to_string(),
        KeyId::Other => return None,
    };
    let mut parts: Vec<&str> = Vec::new();
    if binding.primary {
        parts.push("CmdOrCtrl");
    }
    if binding.control {
        parts.push("Ctrl");
    }
    if binding.alt {
        parts.push("Alt");
    }
    if binding.shift {
        parts.push("Shift");
    }
    parts.push(&key);
    Some(parts.join("+"))
}

/// The hardcoded predicate this table replaced; kept as the parity oracle for the default table.
#[cfg(test)]
fn legacy_chord_to_action(chord: Chord, platform: KeyPlatform) -> Option<&'static str> {
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
    fn binding(key: &str, primary: bool, control: bool, alt: bool, shift: bool) -> NativeBinding {
        NativeBinding { key: key.to_string(), primary, control, alt, shift }
    }

    fn overrides(entries: &[(&str, Option<NativeBinding>)]) -> NativeOverrides {
        entries.iter().map(|(id, b)| (id.to_string(), b.clone())).collect()
    }

    const ALL_KEYS: [KeyId; 22] = [
        KeyId::Letter('t'), KeyId::Letter('w'), KeyId::Letter('k'), KeyId::Letter('b'),
        KeyId::Letter('d'), KeyId::Letter('l'), KeyId::Letter('f'), KeyId::Letter('r'),
        KeyId::Letter('x'), KeyId::Digit(0), KeyId::Digit(1), KeyId::Digit(5), KeyId::Digit(9),
        KeyId::Comma, KeyId::Equal, KeyId::Minus, KeyId::BracketLeft, KeyId::BracketRight,
        KeyId::Tab, KeyId::PageUp, KeyId::PageDown, KeyId::Other,
    ];

    fn every_chord() -> impl Iterator<Item = Chord> {
        ALL_KEYS.into_iter().flat_map(|key| {
            (0u8..16).map(move |bits| chord(key, bits & 1 != 0, bits & 2 != 0, bits & 4 != 0, bits & 8 != 0))
        })
    }

    #[test]
    fn default_table_matches_legacy_predicates() {
        for platform in PLATFORMS {
            for c in every_chord() {
                assert_eq!(
                    chord_to_action_in(None, c, platform),
                    legacy_chord_to_action(c, platform),
                    "{platform:?} {c:?}"
                );
                // An empty pushed table is the same as none.
                assert_eq!(
                    chord_to_action_in(Some(&NativeOverrides::new()), c, platform),
                    legacy_chord_to_action(c, platform),
                    "{platform:?} {c:?}"
                );
            }
        }
        // The menu keeps its legacy accelerators when nothing is overridden.
        assert_eq!(menu_accelerator_in(None, "tab.close", "CmdOrCtrl+W").as_deref(), Some("CmdOrCtrl+W"));
    }

    #[test]
    fn rebound_close_tab_is_intercepted_on_new_chord_only() {
        let table = overrides(&[("tab.close", Some(binding("q", true, false, false, true)))]);
        let mac = |key, shift| chord(key, false, true, false, shift);
        // macOS main-window monitor.
        assert_eq!(monitor_action_in(None, &[mac(KeyId::Letter('w'), false)]), Some(("menu_close_tab", None)));
        assert_eq!(monitor_action_in(Some(&table), &[mac(KeyId::Letter('w'), false)]), None);
        assert_eq!(monitor_action_in(Some(&table), &[mac(KeyId::Letter('q'), true)]), Some(("menu_close_tab", None)));
        assert_eq!(monitor_action_in(Some(&table), &[mac(KeyId::Letter('q'), false)]), None);
        // Guest browser sources: mod is Cmd on macOS and Ctrl elsewhere.
        for platform in PLATFORMS {
            assert_eq!(chord_to_action_in(Some(&table), primary(platform, KeyId::Letter('w'), false), platform), None);
            assert_eq!(
                chord_to_action_in(Some(&table), primary(platform, KeyId::Letter('q'), true), platform),
                Some("tab-close")
            );
        }
        // Other actions keep their defaults.
        assert_eq!(monitor_action_in(Some(&table), &[mac(KeyId::Letter('t'), false)]), Some(("menu_new_terminal_tab", None)));
        // The menu accelerator follows the new chord.
        assert_eq!(menu_accelerator_in(Some(&table), "tab.close", "CmdOrCtrl+W").as_deref(), Some("CmdOrCtrl+Shift+Q"));
        assert_eq!(menu_accelerator_in(Some(&table), "tab.newTerminal", "CmdOrCtrl+T").as_deref(), Some("CmdOrCtrl+T"));
    }

    #[test]
    fn unbound_action_is_not_intercepted() {
        let table = overrides(&[("tab.close", None), ("tab.next", None), ("workspace.select1", None)]);
        let mac = |key, shift| chord(key, false, true, false, shift);
        assert_eq!(monitor_action_in(Some(&table), &[mac(KeyId::Letter('w'), false)]), None);
        assert_eq!(monitor_action_in(Some(&table), &[mac(KeyId::Digit(1), false)]), None);
        assert_eq!(monitor_action_in(Some(&table), &[mac(KeyId::Digit(2), false)]), Some(("menu_select_worktree", Some(2))));
        // Unbinding drops every alias of a multi-chord action.
        assert_eq!(monitor_action_in(Some(&table), &[chord(KeyId::Tab, true, false, false, false)]), None);
        assert_eq!(monitor_action_in(Some(&table), &[mac(KeyId::BracketRight, true)]), None);
        assert_eq!(
            monitor_action_in(Some(&table), &[chord(KeyId::Tab, true, false, false, true)]),
            Some(("menu_prev_tab", None))
        );
        for platform in PLATFORMS {
            assert_eq!(chord_to_action_in(Some(&table), primary(platform, KeyId::Letter('w'), false), platform), None);
            assert_eq!(chord_to_action_in(Some(&table), chord(KeyId::PageDown, true, false, false, false), platform), None);
        }
        assert_eq!(menu_accelerator_in(Some(&table), "tab.close", "CmdOrCtrl+W"), None);
    }

    #[test]
    fn browser_chord_lookup_uses_the_table() {
        let mac_l = chord(KeyId::Letter('l'), false, true, false, false);
        assert_eq!(chord_to_action(mac_l, KeyPlatform::Mac), Some("focus-address"));
        set_native_overrides(Some(overrides(&[
            ("browser.focusAddress", Some(binding("g", true, false, false, false))),
            ("zoom.in", Some(binding("=", false, true, false, false))),
        ])));
        assert_eq!(chord_to_action(mac_l, KeyPlatform::Mac), None);
        assert_eq!(
            chord_to_action(chord(KeyId::Letter('g'), false, true, false, false), KeyPlatform::Mac),
            Some("focus-address")
        );
        assert_eq!(
            chord_to_action(chord(KeyId::Letter('g'), true, false, false, false), KeyPlatform::Linux),
            Some("focus-address")
        );
        // An overridden multi-chord action loses both default chords.
        for shift in [false, true] {
            assert_eq!(chord_to_action(primary(KeyPlatform::Mac, KeyId::Equal, shift), KeyPlatform::Mac), None);
        }
        assert_eq!(
            chord_to_action(chord(KeyId::Equal, true, false, false, false), KeyPlatform::Mac),
            Some("zoom-in")
        );
        // Untouched multi-chord actions keep their aliases.
        assert_eq!(
            chord_to_action(chord(KeyId::Tab, true, false, false, false), KeyPlatform::Windows),
            Some("tab-next")
        );
        set_native_overrides(None);
        assert_eq!(chord_to_action(mac_l, KeyPlatform::Mac), Some("focus-address"));
    }

    #[test]
    fn every_native_id_maps_to_a_ts_action() {
        let ts_source = include_str!("../../../../ui/src/lib/shortcuts.ts");
        let ts_declared = |id: &str| ts_source.contains(&format!("id: \"{id}\""));
        for platform in PLATFORMS {
            for c in every_chord() {
                if let Some(native) = legacy_chord_to_action(c, platform) {
                    assert!(
                        BROWSER_ACTION_IDS.iter().any(|(_, n)| *n == native),
                        "{native} has no TS counterpart"
                    );
                }
            }
        }
        for (ts, native) in BROWSER_ACTION_IDS {
            assert!(ts_declared(ts), "{ts} (for {native}) is not a TS ShortcutActionId");
        }
        for (ts, event, _) in MONITOR_ACTION_IDS {
            assert!(ts_declared(ts), "{ts} (for {event}) is not a TS ShortcutActionId");
        }
    }
}
