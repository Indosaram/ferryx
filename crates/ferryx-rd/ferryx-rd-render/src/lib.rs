//! MahoRD presentation primitives.
//!
//! The default `cpal-output` feature provides native CPAL device/stream output.
//! Disable default features to use the same bounded [`AudioQueue`], PCM checks,
//! gain/mute controls, and output event/status payloads with a caller-owned sink.
//! No alternative sink or format conversion is supplied by this crate.

mod audio;

pub use audio::*;

/// Cursor overlay state consumed by the platform renderer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CursorOverlay {
    pub x: f32,
    pub y: f32,
    pub cursor_type: u8,
}

impl Default for CursorOverlay {
    fn default() -> Self {
        Self {
            x: -1.0,
            y: -1.0,
            cursor_type: 0,
        }
    }
}

impl CursorOverlay {
    pub fn update(&mut self, x: f32, y: f32, cursor_type: u8) {
        self.x = x.clamp(0.0, 1.0);
        self.y = y.clamp(0.0, 1.0);
        self.cursor_type = cursor_type;
    }
}
