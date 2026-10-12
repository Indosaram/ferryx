//! Decides which pane a native left click focuses.
//!
//! The platform mouse monitors (AppKit local monitor, Win32 low-level hook) see button
//! press and release separately. Focusing on release alone moved keyboard focus to whatever
//! pane sat under the pointer when a text-selection drag ended, so dragging out of one pane
//! and letting go over its neighbour silently redirected the next keystrokes there. A click
//! focuses a pane only when the press and the release both land on that same pane.

use std::sync::Mutex;

#[derive(Debug, Default)]
pub struct PressReleaseFocusGate {
    pressed: Mutex<Option<String>>,
}

impl PressReleaseFocusGate {
    pub const fn new() -> Self {
        Self { pressed: Mutex::new(None) }
    }

    /// Records the pane under the pointer when the left button goes down; `None` when the
    /// press is outside every terminal surface.
    pub fn press(&self, session_id: Option<String>) {
        if let Ok(mut pressed) = self.pressed.lock() {
            *pressed = session_id;
        }
    }

    /// Returns the pane to focus for this release: the released-over pane, only when the
    /// same pane received the press. Each press is consumed by one release.
    pub fn release(&self, session_id: Option<String>) -> Option<String> {
        let pressed = self.pressed.lock().ok()?.take();
        match (pressed, session_id) {
            (Some(pressed), Some(released)) if pressed == released => Some(released),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PressReleaseFocusGate;

    fn id(value: &str) -> Option<String> {
        Some(value.to_string())
    }

    #[test]
    fn a_click_inside_one_pane_focuses_that_pane() {
        let gate = PressReleaseFocusGate::new();
        gate.press(id("left"));
        assert_eq!(gate.release(id("left")), id("left"));
    }

    #[test]
    fn a_drag_released_over_a_neighbouring_pane_focuses_nothing() {
        let gate = PressReleaseFocusGate::new();
        gate.press(id("right"));
        assert_eq!(gate.release(id("left")), None);
    }

    #[test]
    fn a_release_without_a_terminal_press_focuses_nothing() {
        let gate = PressReleaseFocusGate::new();
        gate.press(None);
        assert_eq!(gate.release(id("left")), None);
        assert_eq!(gate.release(id("left")), None, "a release with no recorded press never focuses");
    }

    #[test]
    fn each_press_is_consumed_by_one_release() {
        let gate = PressReleaseFocusGate::new();
        gate.press(id("left"));
        assert_eq!(gate.release(id("left")), id("left"));
        assert_eq!(gate.release(id("left")), None);
    }
}
