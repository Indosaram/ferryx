use evdev::{
    uinput::VirtualDevice, AbsInfo, AbsoluteAxisCode, AttributeSet, EventType, InputEvent, KeyCode,
    UinputAbsSetup,
};
use ferryx_rd_proto::{InputEvent as WireEvent, InputEventType};
use std::{collections::BTreeMap, io};

#[derive(Default)]
pub(crate) struct Gamepads(BTreeMap<u8, VirtualDevice>);

const BUTTONS: [KeyCode; 15] = [
    KeyCode::BTN_SOUTH,
    KeyCode::BTN_EAST,
    KeyCode::BTN_WEST,
    KeyCode::BTN_NORTH,
    KeyCode::BTN_TL,
    KeyCode::BTN_TR,
    KeyCode::BTN_SELECT,
    KeyCode::BTN_START,
    KeyCode::BTN_MODE,
    KeyCode::BTN_THUMBL,
    KeyCode::BTN_THUMBR,
    KeyCode::BTN_DPAD_UP,
    KeyCode::BTN_DPAD_DOWN,
    KeyCode::BTN_DPAD_LEFT,
    KeyCode::BTN_DPAD_RIGHT,
];
const AXES: [AbsoluteAxisCode; 6] = [
    AbsoluteAxisCode::ABS_X,
    AbsoluteAxisCode::ABS_Y,
    AbsoluteAxisCode::ABS_RX,
    AbsoluteAxisCode::ABS_RY,
    AbsoluteAxisCode::ABS_Z,
    AbsoluteAxisCode::ABS_RZ,
];

pub(crate) fn available() -> bool {
    std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/uinput")
        .is_ok()
}

fn events(event: &WireEvent) -> io::Result<Vec<InputEvent>> {
    let control = usize::from(event.key_code & 255);
    let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "unsupported gamepad control");
    match event.event_type {
        InputEventType::GamepadAxis => {
            let (a, b) = match control {
                0 => (0, 1),
                1 => (2, 3),
                2 => (4, 5),
                _ => return Err(invalid()),
            };
            if !event.x.is_finite() || !event.y.is_finite() {
                return Err(invalid());
            }
            Ok(vec![
                InputEvent::new(
                    EventType::ABSOLUTE.0,
                    AXES[a].0,
                    (event.x.clamp(-1.0, 1.0) * 32767.0).round() as i32,
                ),
                InputEvent::new(
                    EventType::ABSOLUTE.0,
                    AXES[b].0,
                    (event.y.clamp(-1.0, 1.0) * 32767.0).round() as i32,
                ),
            ])
        }
        InputEventType::GamepadButtonDown | InputEventType::GamepadButtonUp => {
            let button = BUTTONS.get(control).ok_or_else(invalid)?;
            Ok(vec![InputEvent::new(
                EventType::KEY.0,
                button.code(),
                i32::from(event.event_type == InputEventType::GamepadButtonDown),
            )])
        }
        _ => Err(invalid()),
    }
}

impl Gamepads {
    pub(crate) fn inject(&mut self, event: &WireEvent) -> io::Result<()> {
        let events = events(event)?;
        let id = (event.key_code >> 8) as u8;
        if id >= 4 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "at most four gamepads",
            ));
        }
        if let std::collections::btree_map::Entry::Vacant(entry) = self.0.entry(id) {
            let keys = AttributeSet::from_iter(BUTTONS);
            let mut builder = VirtualDevice::builder()?
                .name("MahoRD Virtual Gamepad")
                .with_keys(&keys)?;
            for axis in AXES {
                builder = builder.with_absolute_axis(&UinputAbsSetup::new(
                    axis,
                    AbsInfo::new(0, -32767, 32767, 16, 128, 0),
                ))?;
            }
            entry.insert(builder.build()?);
        }
        self.0
            .get_mut(&id)
            .ok_or_else(|| io::Error::other("gamepad missing"))?
            .emit(&events)
    }

    pub(crate) fn reset(&mut self) {
        self.0.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn axis_maps_pair_and_clamps() {
        let event = WireEvent::gamepad_axis(0, 1, 2.0, -2.0);
        let emitted = events(&event).unwrap();
        assert_eq!(emitted[0].code(), AbsoluteAxisCode::ABS_RX.0);
        assert_eq!(emitted[0].value(), 32767);
        assert_eq!(emitted[1].value(), -32767);
    }
    #[test]
    fn unknown_button_is_rejected() {
        assert!(events(&WireEvent::gamepad_button(0, 255, true)).is_err());
    }
}
