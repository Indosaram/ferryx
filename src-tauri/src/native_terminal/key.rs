//! Safe Ferryx-owned keyboard event types.

/// Action performed on a keyboard key.
#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum KeyAction {
    Release = 0,
    Press = 1,
    Repeat = 2,
}

/// Keyboard modifier keys bitmask representation.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyModifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub super_key: bool,
    pub caps_lock: bool,
    pub num_lock: bool,
}

impl KeyModifiers {
    pub fn to_raw_mods(self) -> u16 {
        let mut raw = 0u16;
        if self.shift {
            raw |= 1 << 0;
        }
        if self.ctrl {
            raw |= 1 << 1;
        }
        if self.alt {
            raw |= 1 << 2;
        }
        if self.super_key {
            raw |= 1 << 3;
        }
        if self.caps_lock {
            raw |= 1 << 4;
        }
        if self.num_lock {
            raw |= 1 << 5;
        }
        raw
    }
}

/// Key code abstraction for common keyboard keys.
#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum KeyCode {
    Unidentified,
    Character(char),
    Enter,
    Tab,
    Backspace,
    Escape,
    Space,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
}

/// A complete keyboard input event.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct KeyEvent {
    pub key: KeyCode,
    pub action: KeyAction,
    pub modifiers: KeyModifiers,
    pub utf8: Option<String>,
}

impl KeyEvent {
    pub fn new(key: KeyCode, action: KeyAction) -> Self {
        Self {
            key,
            action,
            modifiers: KeyModifiers::default(),
            utf8: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_modifiers_deserialize_from_camel_case_json() {
        let json = r#"{
            "shift": true,
            "ctrl": false,
            "alt": true,
            "superKey": true,
            "capsLock": false,
            "numLock": false
        }"#;
        let mods: KeyModifiers =
            serde_json::from_str(json).expect("deserialize camelCase key modifiers");
        assert!(mods.shift);
        assert!(!mods.ctrl);
        assert!(mods.alt);
        assert!(mods.super_key);
        assert!(!mods.caps_lock);
        assert!(!mods.num_lock);
        assert_eq!(mods.to_raw_mods(), (1 << 0) | (1 << 2) | (1 << 3));
    }

    #[test]
    fn key_event_deserialize_from_json() {
        let json = r#"{
            "key": "Enter",
            "action": "Press",
            "modifiers": {
                "shift": false,
                "ctrl": true,
                "alt": false,
                "superKey": false,
                "capsLock": false,
                "numLock": false
            },
            "utf8": null
        }"#;
        let ev: Result<KeyEvent, _> = serde_json::from_str(json);
        println!("ev result: {:?}", ev);
        ev.expect("deserialize key event");
    }
}
