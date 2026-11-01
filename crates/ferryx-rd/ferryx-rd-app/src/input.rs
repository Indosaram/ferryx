use ferryx_rd_proto::{InputEvent, InputEventType, Modifiers};

/// Input key identifier accepted by the cross-platform key mapper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKey {
    WindowsVirtualKey(u16),
}

pub struct InputKeyMap;

impl InputKeyMap {
    /// Maps Windows virtual-key values to the macOS virtual key codes consumed by the host.
    pub fn to_macos(key: InputKey) -> Option<u16> {
        use ferryx_rd_proto::extended_keys::*;
        let InputKey::WindowsVirtualKey(key) = key;
        Some(match key {
            0x41 => 0x00,
            0x53 => 0x01,
            0x44 => 0x02,
            0x46 => 0x03,
            0x48 => 0x04,
            0x47 => 0x05,
            0x5a => 0x06,
            0x58 => 0x07,
            0x43 => 0x08,
            0x56 => 0x09,
            0x42 => 0x0b,
            0x51 => 0x0c,
            0x57 => 0x0d,
            0x45 => 0x0e,
            0x52 => 0x0f,
            0x59 => 0x10,
            0x54 => 0x11,
            0x31 => 0x12,
            0x32 => 0x13,
            0x33 => 0x14,
            0x34 => 0x15,
            0x36 => 0x16,
            0x35 => 0x17,
            0x3d => 0x18,
            0x39 => 0x19,
            0x37 => 0x1a,
            0x2d => 0x1b,
            0x38 => 0x1c,
            0x30 => 0x1d,
            0xdd => 0x1e,
            0x4f => 0x1f,
            0x55 => 0x20,
            0xdb => 0x21,
            0x49 => 0x22,
            0x50 => 0x23,
            0x0d => 0x24,
            0x4c => 0x25,
            0x4a => 0x26,
            0xde => 0x27,
            0x4b => 0x28,
            0xba => 0x29,
            0xdc => 0x2a,
            0xbc => 0x2b,
            0xbf => 0x2c,
            0x4e => 0x2d,
            0x4d => 0x2e,
            0xbe => 0x2f,
            0x09 => 0x30,
            0x20 => 0x31,
            0xc0 => 0x32,
            0x08 => 0x33,
            0x1b => 0x35,
            0x5b | 0x5c => 0x37,
            0x10 | 0xa0 => 0x38,
            0x14 => 0x39,
            0x12 | 0xa4 => 0x3a,
            0x11 | 0xa2 => 0x3b,
            0xa1 => 0x3c,
            0xa5 => 0x3d,
            0xa3 => 0x3e,
            0x70 => 0x7a,
            0x71 => 0x78,
            0x72 => 0x63,
            0x73 => 0x76,
            0x74 => 0x60,
            0x75 => 0x61,
            0x76 => 0x62,
            0x77 => 0x64,
            0x78 => 0x65,
            0x79 => 0x6d,
            0x7a => 0x67,
            0x7b => 0x6f,
            0x7c => 0x69,
            0x7d => 0x6b,
            0x7e => 0x71,
            0x7f => 0x6a,
            0x80 => 0x40,
            0x81 => 0x4f,
            0x82 => 0x50,
            0x83 => 0x5a,
            0x84 => F21,
            0x85 => F22,
            0x86 => F23,
            0x87 => F24,
            0xb3 => MEDIA_PLAY_PAUSE,
            0xb0 => MEDIA_NEXT,
            0xb1 => MEDIA_PREVIOUS,
            0xb2 => MEDIA_STOP,
            0xa6 => BROWSER_BACK,
            0xa7 => BROWSER_FORWARD,
            0xa8 => BROWSER_REFRESH,
            0xa9 => BROWSER_STOP,
            0xaa => BROWSER_SEARCH,
            0xab => BROWSER_FAVORITES,
            0xac => BROWSER_HOME,
            0xad => 0x4a,
            0xae => 0x49,
            0xaf => 0x48,
            0x60 => 0x52,
            0x61 => 0x53,
            0x62 => 0x54,
            0x63 => 0x55,
            0x64 => 0x56,
            0x65 => 0x57,
            0x66 => 0x58,
            0x67 => 0x59,
            0x68 => 0x5b,
            0x69 => 0x5c,
            0x6a => 0x43,
            0x6b => 0x45,
            0x6d => 0x4e,
            0x6e => 0x41,
            0x6f => 0x4b,
            0x90 => 0x47,
            0x92 => 0x51,
            0x100d => 0x4c,
            0x25 => 0x7b,
            0x27 => 0x7c,
            0x28 => 0x7d,
            0x26 => 0x7e,
            0x24 => 0x73,
            0x23 => 0x77,
            0x21 => 0x74,
            0x22 => 0x79,
            0x2e => 0x75,
            _ => return None,
        })
    }
}

/// Normalizes client view coordinates and flips Y for the host wire coordinate system.
pub fn normalize_pointer(x: f32, y: f32, view_width: f32, view_height: f32) -> Option<(f32, f32)> {
    if !view_width.is_finite()
        || !view_height.is_finite()
        || view_width <= 0.0
        || view_height <= 0.0
    {
        return None;
    }
    let normalized_x = (x / view_width).clamp(0.0, 1.0);
    let normalized_y = 1.0 - (y / view_height).clamp(0.0, 1.0);
    Some((normalized_x, normalized_y))
}

#[allow(clippy::too_many_arguments)]
pub fn pointer_event(
    event_type: InputEventType,
    x: f32,
    y: f32,
    view_width: f32,
    view_height: f32,
    modifiers: Modifiers,
    scroll_dx: f32,
    scroll_dy: f32,
) -> Option<InputEvent> {
    let (x, y) = normalize_pointer(x, y, view_width, view_height)?;
    Some(InputEvent {
        event_type,
        x,
        y,
        key_code: 0,
        modifiers,
        scroll_dx,
        scroll_dy,
    })
}

pub fn key_event(
    event_type: InputEventType,
    key: InputKey,
    modifiers: Modifiers,
) -> Option<InputEvent> {
    Some(InputEvent {
        event_type,
        x: 0.0,
        y: 0.0,
        key_code: InputKeyMap::to_macos(key)?,
        modifiers,
        scroll_dx: 0.0,
        scroll_dy: 0.0,
    })
}

#[cfg(test)]
mod extended_tests {
    use super::*;
    #[test]
    fn keypad_enter_volume_and_extended_functions_preserve_wire_identity() {
        for (vk, wire) in [
            (0x0d, 36),
            (0x100d, 76),
            (0x60, 82),
            (0xaf, 72),
            (0x83, 90),
            (0x7c, 105),
        ] {
            assert_eq!(
                InputKeyMap::to_macos(InputKey::WindowsVirtualKey(vk)),
                Some(wire)
            );
        }
        for vk in 0xa6..=0xac {
            assert_eq!(
                InputKeyMap::to_macos(InputKey::WindowsVirtualKey(vk)),
                Some(0x104 + vk - 0xa6)
            );
        }
        for vk in 0xb0..=0xb3 {
            assert_eq!(
                InputKeyMap::to_macos(InputKey::WindowsVirtualKey(vk)),
                Some([0x101, 0x102, 0x103, 0x100][(vk - 0xb0) as usize])
            );
        }
        for vk in 0x84..=0x87 {
            assert_eq!(
                InputKeyMap::to_macos(InputKey::WindowsVirtualKey(vk)),
                Some(0x10b + vk - 0x84)
            );
        }
    }
}
