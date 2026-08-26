//! Cursor types and visual styles for the terminal engine.

use std::ffi::c_int;

use super::error::NativeTerminalError;

/// Visual representation of the terminal cursor.
#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CursorVisualStyle {
    Bar,
    Block,
    Underline,
    BlockHollow,
}

impl TryFrom<c_int> for CursorVisualStyle {
    type Error = NativeTerminalError;

    fn try_from(val: c_int) -> Result<Self, Self::Error> {
        match val {
            0 => Ok(Self::Bar),
            1 => Ok(Self::Block),
            2 => Ok(Self::Underline),
            3 => Ok(Self::BlockHollow),
            other => Err(NativeTerminalError::ForeignErrorCode(other)),
        }
    }
}

/// Dynamic cursor state queried from the native terminal engine.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CursorState {
    pub x: u16,
    pub y: u16,
    pub visible: bool,
    pub pending_wrap: bool,
}

/// Cursor information captured in a render snapshot.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CursorSnapshot {
    pub x: u16,
    pub y: u16,
    pub visible: bool,
    pub blinking: bool,
    pub wide_tail: bool,
    pub visual_style: CursorVisualStyle,
}
