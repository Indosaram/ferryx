//! Safe Ferryx-owned mouse event and geometry types.

use super::key::KeyModifiers;

/// Mouse event action.
#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MouseAction {
    Press = 0,
    Release = 1,
    Motion = 2,
}

/// Mouse button identity.
#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MouseButton {
    Left = 1,
    Right = 2,
    Middle = 3,
    Four = 4,
    Five = 5,
    Six = 6,
    Seven = 7,
    Eight = 8,
    Nine = 9,
    Ten = 10,
    Eleven = 11,
}

/// Mouse surface-space position in pixels.
#[derive(Copy, Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MousePosition {
    pub x: f32,
    pub y: f32,
}

/// Renderer dimensions and cell geometry context for mouse coordinate calculation.
#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MouseRendererSize {
    pub screen_width: u32,
    pub screen_height: u32,
    pub cell_width: u32,
    pub cell_height: u32,
    pub padding_top: u32,
    pub padding_bottom: u32,
    pub padding_right: u32,
    pub padding_left: u32,
}

/// A complete mouse input event.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MouseEvent {
    pub action: MouseAction,
    pub button: Option<MouseButton>,
    pub position: MousePosition,
    pub modifiers: KeyModifiers,
    pub size: Option<MouseRendererSize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mouse_event_deserializes_from_camel_case_wire_json() {
        let json = r#"{
            "action": "Press",
            "button": "Left",
            "position": { "x": 12.5, "y": 6.0 },
            "modifiers": { "shift": false, "ctrl": false, "alt": false, "superKey": true, "capsLock": false, "numLock": false },
            "size": { "screenWidth": 1000, "screenHeight": 500, "cellWidth": 10, "cellHeight": 20, "paddingTop": 0, "paddingBottom": 0, "paddingRight": 0, "paddingLeft": 0 }
        }"#;

        let event: MouseEvent =
            serde_json::from_str(json).expect("deserialize camelCase mouse event");
        assert_eq!(event.action, MouseAction::Press);
        assert_eq!(event.button, Some(MouseButton::Left));
        assert_eq!(event.position.x, 12.5);
        assert_eq!(event.modifiers.super_key, true);
        let size = event.size.expect("mouse renderer size");
        assert_eq!(size.screen_width, 1000);
        assert_eq!(size.cell_height, 20);
    }
}
