use ferryx_rd_proto::{InputEvent, InputEventType, Modifiers};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImeComposition {
    pub text: String,
    pub cursor_position: usize,
    pub is_composing: bool,
}

impl ImeComposition {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update_composition(&mut self, text: &str, cursor_position: usize) {
        self.text = text.to_string();
        self.cursor_position = cursor_position.min(text.len());
        self.is_composing = !text.is_empty();
    }

    pub fn commit(&mut self) -> String {
        let committed = std::mem::take(&mut self.text);
        self.cursor_position = 0;
        self.is_composing = false;
        committed
    }

    pub fn cancel(&mut self) {
        self.text.clear();
        self.cursor_position = 0;
        self.is_composing = false;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MobileAccessoryKey {
    Escape,
    Tab,
    Control,
    Alt,
    Meta,
    Shift,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
}

#[derive(Debug, Clone, Default)]
pub struct MobileModifierBar {
    pub modifiers: Modifiers,
}

impl MobileModifierBar {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn toggle_modifier(&mut self, key: MobileAccessoryKey) -> Modifiers {
        let mask = match key {
            MobileAccessoryKey::Shift => Modifiers::SHIFT,
            MobileAccessoryKey::Control => Modifiers::CONTROL,
            MobileAccessoryKey::Alt => Modifiers::OPTION,
            MobileAccessoryKey::Meta => Modifiers::COMMAND,
            _ => Modifiers::empty(),
        };

        if self.modifiers.contains(mask) {
            let cleared_bits = self.modifiers.bits() & !mask.bits();
            self.modifiers = Modifiers::from_bits_retain(cleared_bits);
        } else {
            self.modifiers |= mask;
        }
        self.modifiers
    }

    pub fn accessory_key_to_input_events(
        &self,
        key: MobileAccessoryKey,
        is_down: bool,
    ) -> Option<InputEvent> {
        let key_code = match key {
            MobileAccessoryKey::Escape => 0x35,
            MobileAccessoryKey::Tab => 0x30,
            MobileAccessoryKey::ArrowUp => 0x7e,
            MobileAccessoryKey::ArrowDown => 0x7d,
            MobileAccessoryKey::ArrowLeft => 0x7b,
            MobileAccessoryKey::ArrowRight => 0x7c,
            _ => return None,
        };

        Some(InputEvent {
            event_type: if is_down {
                InputEventType::KeyDown
            } else {
                InputEventType::KeyUp
            },
            x: 0.0,
            y: 0.0,
            key_code,
            modifiers: self.modifiers,
            scroll_dx: 0.0,
            scroll_dy: 0.0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ime_composition_lifecycle() {
        let mut ime = ImeComposition::new();
        assert!(!ime.is_composing);

        ime.update_composition("안녕", 2);
        assert!(ime.is_composing);
        assert_eq!(ime.text, "안녕");
        assert_eq!(ime.cursor_position, 2);

        let committed = ime.commit();
        assert_eq!(committed, "안녕");
        assert!(!ime.is_composing);
        assert!(ime.text.is_empty());
    }

    #[test]
    fn ime_composition_cancellation() {
        let mut ime = ImeComposition::new();
        ime.update_composition("test", 4);
        ime.cancel();
        assert!(!ime.is_composing);
        assert!(ime.text.is_empty());
    }

    #[test]
    fn modifier_bar_toggles_bits() {
        let mut bar = MobileModifierBar::new();
        assert_eq!(bar.modifiers, Modifiers::empty());

        bar.toggle_modifier(MobileAccessoryKey::Control);
        assert!(bar.modifiers.contains(Modifiers::CONTROL));

        bar.toggle_modifier(MobileAccessoryKey::Alt);
        assert!(bar.modifiers.contains(Modifiers::CONTROL));
        assert!(bar.modifiers.contains(Modifiers::OPTION));

        bar.toggle_modifier(MobileAccessoryKey::Control);
        assert!(!bar.modifiers.contains(Modifiers::CONTROL));
        assert!(bar.modifiers.contains(Modifiers::OPTION));
    }

    #[test]
    fn accessory_navigation_key_events() {
        let mut bar = MobileModifierBar::new();
        bar.toggle_modifier(MobileAccessoryKey::Shift);

        let esc_down = bar
            .accessory_key_to_input_events(MobileAccessoryKey::Escape, true)
            .unwrap();
        assert_eq!(esc_down.event_type, InputEventType::KeyDown);
        assert_eq!(esc_down.key_code, 0x35);
        assert!(esc_down.modifiers.contains(Modifiers::SHIFT));

        let esc_up = bar
            .accessory_key_to_input_events(MobileAccessoryKey::Escape, false)
            .unwrap();
        assert_eq!(esc_up.event_type, InputEventType::KeyUp);
    }
}
