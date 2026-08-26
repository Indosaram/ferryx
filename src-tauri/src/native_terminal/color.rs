//! Color types for terminal cells and rendering.

use super::sys::types::GhosttyColorRgb;

/// 24-bit RGB color.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ColorRgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl ColorRgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Parses a 3-, 4-, 6-, or 8-digit hexadecimal color string into RGB.
    pub fn from_hex(s: &str) -> Option<Self> {
        let hex = s.trim().trim_start_matches('#');
        if hex.len() >= 6 {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some(Self { r, g, b })
        } else if hex.len() >= 3 {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()?;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()?;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()?;
            Some(Self {
                r: r * 17,
                g: g * 17,
                b: b * 17,
            })
        } else {
            None
        }
    }

    pub fn to_f32_rgba(&self, a: f32) -> [f32; 4] {
        [
            self.r as f32 / 255.0,
            self.g as f32 / 255.0,
            self.b as f32 / 255.0,
            a,
        ]
    }
}

impl From<GhosttyColorRgb> for ColorRgb {
    fn from(c: GhosttyColorRgb) -> Self {
        Self {
            r: c.r,
            g: c.g,
            b: c.b,
        }
    }
}

impl From<ColorRgb> for GhosttyColorRgb {
    fn from(c: ColorRgb) -> Self {
        Self {
            r: c.r,
            g: c.g,
            b: c.b,
        }
    }
}
