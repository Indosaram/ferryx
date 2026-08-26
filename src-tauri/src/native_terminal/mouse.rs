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
pub struct MousePosition {
    pub x: f32,
    pub y: f32,
}

/// Renderer dimensions and cell geometry context for mouse coordinate calculation.
#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
pub struct MouseEvent {
    pub action: MouseAction,
    pub button: Option<MouseButton>,
    pub position: MousePosition,
    pub modifiers: KeyModifiers,
    pub size: Option<MouseRendererSize>,
}
