//! Wayland-safe Linux input injection through `/dev/uinput`.
//!
//! The kernel sees the generated keyboard and absolute pointer as ordinary
//! input devices, so injection works under Hyprland without compositor-specific
//! virtual-input protocols.
//!
//! ## Permissions (Arch/Omarchy)
//!
//! Create a dedicated `uinput` group and grant it the device with udev:
//!
//! ```text
//! # /etc/udev/rules.d/70-mahord-uinput.rules
//! KERNEL=="uinput", GROUP="uinput", MODE="0660", OPTIONS+="static_node=uinput"
//! ```
//!
//! Then run `sudo groupadd -f uinput`, add the host user with
//! `sudo usermod -aG uinput $USER`, load `uinput`, reload udev rules, and log
//! out/in. The process must never run setuid or as root merely for injection.

use std::{
    collections::HashSet,
    io,
    process::{Command, Stdio},
    sync::{
        mpsc::{sync_channel, SyncSender, TrySendError},
        Arc, Condvar, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use evdev::{
    uinput::VirtualDevice, AbsInfo, AbsoluteAxisCode, AbsoluteAxisEvent, AttributeSet, EventType,
    InputEvent, KeyCode, RelativeAxisCode, UinputAbsSetup,
};
use ferryx_rd_proto::{InputEvent as WireInputEvent, InputEventType, Modifiers};

/// Derives evdev absolute axis max limits for uinput setup.
///
/// Linux evdev absolute axes are inclusive `[minimum, maximum]`. For a desktop
/// of width `W` (pixels `0..W-1`), the maximum coordinate index is `W - 1`. Setting
/// `maximum = W` creates `W + 1` discrete values, causing compositors/libinput to
/// scale incoming coordinates by `(W - 1) / W` and introducing a systematic -1 pixel boundary error.
pub fn evdev_abs_axis_max(dimension: u32) -> i32 {
    dimension.saturating_sub(1) as i32
}

/// Target output position and dimensions in compositor/global logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    /// Signed origin of the logical desktop. Outputs left of or above the
    /// primary display make this negative.
    pub desktop_x: i32,
    pub desktop_y: i32,
    pub desktop_width: u32,
    pub desktop_height: u32,
}

impl OutputGeometry {
    pub fn single_output(width: u32, height: u32) -> Self {
        Self {
            x: 0,
            y: 0,
            width,
            height,
            desktop_x: 0,
            desktop_y: 0,
            desktop_width: width,
            desktop_height: height,
        }
    }
}

/// Pure coordinate mapping used by the uinput backend.
pub fn map_normalized_to_output(
    normalized_x: f32,
    normalized_y: f32,
    geometry: OutputGeometry,
) -> (u32, u32) {
    let x = normalized_x.clamp(0.0, 1.0) * geometry.width.saturating_sub(1) as f32;
    // Protocol convention inverts Y (1.0 - y) for legacy macOS compatibility.
    // Invert it back so (0,0) is top-left on Linux.
    let y = (1.0 - normalized_y).clamp(0.0, 1.0) * geometry.height.saturating_sub(1) as f32;
    // The uinput absolute axes are unsigned, so shift by the signed desktop
    // origin instead of clamping negative multi-monitor offsets to zero.
    (
        (i64::from(geometry.x) - i64::from(geometry.desktop_x) + x.round() as i64).max(0) as u32,
        (i64::from(geometry.y) - i64::from(geometry.desktop_y) + y.round() as i64).max(0) as u32,
    )
}

/// Maps the v3/macOS virtual-key convention to Linux evdev keys.
///
/// The wire key code intentionally remains the existing macOS key code so all
/// hosts interoperate with the current clients.
pub fn macos_keycode_to_evdev(key_code: u16) -> Option<KeyCode> {
    use ferryx_rd_proto::extended_keys::*;
    Some(match key_code {
        MEDIA_PLAY_PAUSE => KeyCode::KEY_PLAYPAUSE,
        MEDIA_NEXT => KeyCode::KEY_NEXTSONG,
        MEDIA_PREVIOUS => KeyCode::KEY_PREVIOUSSONG,
        MEDIA_STOP => KeyCode::KEY_STOPCD,
        BROWSER_BACK => KeyCode::KEY_BACK,
        BROWSER_FORWARD => KeyCode::KEY_FORWARD,
        BROWSER_REFRESH => KeyCode::KEY_REFRESH,
        BROWSER_STOP => KeyCode::KEY_STOP,
        BROWSER_SEARCH => KeyCode::KEY_SEARCH,
        BROWSER_FAVORITES => KeyCode::KEY_BOOKMARKS,
        BROWSER_HOME => KeyCode::KEY_HOMEPAGE,
        F21 => KeyCode::KEY_F21,
        F22 => KeyCode::KEY_F22,
        F23 => KeyCode::KEY_F23,
        F24 => KeyCode::KEY_F24,
        0 => KeyCode::KEY_A,
        1 => KeyCode::KEY_S,
        2 => KeyCode::KEY_D,
        3 => KeyCode::KEY_F,
        4 => KeyCode::KEY_H,
        5 => KeyCode::KEY_G,
        6 => KeyCode::KEY_Z,
        7 => KeyCode::KEY_X,
        8 => KeyCode::KEY_C,
        9 => KeyCode::KEY_V,
        11 => KeyCode::KEY_B,
        12 => KeyCode::KEY_Q,
        13 => KeyCode::KEY_W,
        14 => KeyCode::KEY_E,
        15 => KeyCode::KEY_R,
        16 => KeyCode::KEY_Y,
        17 => KeyCode::KEY_T,
        18 => KeyCode::KEY_1,
        19 => KeyCode::KEY_2,
        20 => KeyCode::KEY_3,
        21 => KeyCode::KEY_4,
        22 => KeyCode::KEY_6,
        23 => KeyCode::KEY_5,
        24 => KeyCode::KEY_EQUAL,
        25 => KeyCode::KEY_9,
        26 => KeyCode::KEY_7,
        27 => KeyCode::KEY_MINUS,
        28 => KeyCode::KEY_8,
        29 => KeyCode::KEY_0,
        30 => KeyCode::KEY_RIGHTBRACE,
        31 => KeyCode::KEY_O,
        32 => KeyCode::KEY_U,
        33 => KeyCode::KEY_LEFTBRACE,
        34 => KeyCode::KEY_I,
        35 => KeyCode::KEY_P,
        36 => KeyCode::KEY_ENTER,
        37 => KeyCode::KEY_L,
        38 => KeyCode::KEY_J,
        39 => KeyCode::KEY_APOSTROPHE,
        40 => KeyCode::KEY_K,
        41 => KeyCode::KEY_SEMICOLON,
        42 => KeyCode::KEY_BACKSLASH,
        43 => KeyCode::KEY_COMMA,
        44 => KeyCode::KEY_SLASH,
        45 => KeyCode::KEY_N,
        46 => KeyCode::KEY_M,
        47 => KeyCode::KEY_DOT,
        48 => KeyCode::KEY_TAB,
        49 => KeyCode::KEY_SPACE,
        50 => KeyCode::KEY_GRAVE,
        51 => KeyCode::KEY_BACKSPACE,
        53 => KeyCode::KEY_ESC,
        54 => KeyCode::KEY_RIGHTMETA,
        55 => KeyCode::KEY_LEFTMETA,
        56 => KeyCode::KEY_LEFTSHIFT,
        57 => KeyCode::KEY_CAPSLOCK,
        58 => KeyCode::KEY_LEFTALT,
        59 => KeyCode::KEY_LEFTCTRL,
        60 => KeyCode::KEY_RIGHTSHIFT,
        61 => KeyCode::KEY_RIGHTALT,
        62 => KeyCode::KEY_RIGHTCTRL,
        63 => KeyCode::KEY_FN,
        64 => KeyCode::KEY_F17,
        65 => KeyCode::KEY_KPDOT,
        67 => KeyCode::KEY_KPASTERISK,
        69 => KeyCode::KEY_KPPLUS,
        71 => KeyCode::KEY_NUMLOCK,
        72 => KeyCode::KEY_VOLUMEUP,
        73 => KeyCode::KEY_VOLUMEDOWN,
        74 => KeyCode::KEY_MUTE,
        75 => KeyCode::KEY_KPSLASH,
        76 => KeyCode::KEY_KPENTER,
        78 => KeyCode::KEY_KPMINUS,
        79 => KeyCode::KEY_F18,
        80 => KeyCode::KEY_F19,
        81 => KeyCode::KEY_KPEQUAL,
        82 => KeyCode::KEY_KP0,
        83 => KeyCode::KEY_KP1,
        84 => KeyCode::KEY_KP2,
        85 => KeyCode::KEY_KP3,
        86 => KeyCode::KEY_KP4,
        87 => KeyCode::KEY_KP5,
        88 => KeyCode::KEY_KP6,
        89 => KeyCode::KEY_KP7,
        90 => KeyCode::KEY_F20,
        91 => KeyCode::KEY_KP8,
        92 => KeyCode::KEY_KP9,
        96 => KeyCode::KEY_F5,
        97 => KeyCode::KEY_F6,
        98 => KeyCode::KEY_F7,
        99 => KeyCode::KEY_F3,
        100 => KeyCode::KEY_F8,
        101 => KeyCode::KEY_F9,
        103 => KeyCode::KEY_F11,
        105 => KeyCode::KEY_F13,
        106 => KeyCode::KEY_F16,
        107 => KeyCode::KEY_F14,
        109 => KeyCode::KEY_F10,
        111 => KeyCode::KEY_F12,
        113 => KeyCode::KEY_F15,
        114 => KeyCode::KEY_INSERT,
        115 => KeyCode::KEY_HOME,
        116 => KeyCode::KEY_PAGEUP,
        117 => KeyCode::KEY_DELETE,
        118 => KeyCode::KEY_F4,
        119 => KeyCode::KEY_END,
        120 => KeyCode::KEY_F2,
        121 => KeyCode::KEY_PAGEDOWN,
        122 => KeyCode::KEY_F1,
        123 => KeyCode::KEY_LEFT,
        124 => KeyCode::KEY_RIGHT,
        125 => KeyCode::KEY_DOWN,
        126 => KeyCode::KEY_UP,
        _ => return None,
    })
}

/// Maps an ASCII character to an evdev KeyCode and whether Shift is required.
pub fn ascii_to_evdev(ch: char) -> Option<(KeyCode, bool)> {
    match ch {
        'a'..='z' => {
            let key = match ch {
                'a' => KeyCode::KEY_A,
                'b' => KeyCode::KEY_B,
                'c' => KeyCode::KEY_C,
                'd' => KeyCode::KEY_D,
                'e' => KeyCode::KEY_E,
                'f' => KeyCode::KEY_F,
                'g' => KeyCode::KEY_G,
                'h' => KeyCode::KEY_H,
                'i' => KeyCode::KEY_I,
                'j' => KeyCode::KEY_J,
                'k' => KeyCode::KEY_K,
                'l' => KeyCode::KEY_L,
                'm' => KeyCode::KEY_M,
                'n' => KeyCode::KEY_N,
                'o' => KeyCode::KEY_O,
                'p' => KeyCode::KEY_P,
                'q' => KeyCode::KEY_Q,
                'r' => KeyCode::KEY_R,
                's' => KeyCode::KEY_S,
                't' => KeyCode::KEY_T,
                'u' => KeyCode::KEY_U,
                'v' => KeyCode::KEY_V,
                'w' => KeyCode::KEY_W,
                'x' => KeyCode::KEY_X,
                'y' => KeyCode::KEY_Y,
                'z' => KeyCode::KEY_Z,
                _ => unreachable!(),
            };
            Some((key, false))
        }
        'A'..='Z' => {
            let lower = ch.to_ascii_lowercase();
            ascii_to_evdev(lower).map(|(k, _)| (k, true))
        }
        '0'..='9' => {
            let key = match ch {
                '0' => KeyCode::KEY_0,
                '1' => KeyCode::KEY_1,
                '2' => KeyCode::KEY_2,
                '3' => KeyCode::KEY_3,
                '4' => KeyCode::KEY_4,
                '5' => KeyCode::KEY_5,
                '6' => KeyCode::KEY_6,
                '7' => KeyCode::KEY_7,
                '8' => KeyCode::KEY_8,
                '9' => KeyCode::KEY_9,
                _ => unreachable!(),
            };
            Some((key, false))
        }
        ' ' => Some((KeyCode::KEY_SPACE, false)),
        '\n' | '\r' => Some((KeyCode::KEY_ENTER, false)),
        '\t' => Some((KeyCode::KEY_TAB, false)),
        '-' => Some((KeyCode::KEY_MINUS, false)),
        '_' => Some((KeyCode::KEY_MINUS, true)),
        '=' => Some((KeyCode::KEY_EQUAL, false)),
        '+' => Some((KeyCode::KEY_EQUAL, true)),
        '[' => Some((KeyCode::KEY_LEFTBRACE, false)),
        '{' => Some((KeyCode::KEY_LEFTBRACE, true)),
        ']' => Some((KeyCode::KEY_RIGHTBRACE, false)),
        '}' => Some((KeyCode::KEY_RIGHTBRACE, true)),
        ';' => Some((KeyCode::KEY_SEMICOLON, false)),
        ':' => Some((KeyCode::KEY_SEMICOLON, true)),
        '\'' => Some((KeyCode::KEY_APOSTROPHE, false)),
        '"' => Some((KeyCode::KEY_APOSTROPHE, true)),
        '`' => Some((KeyCode::KEY_GRAVE, false)),
        '~' => Some((KeyCode::KEY_GRAVE, true)),
        '\\' => Some((KeyCode::KEY_BACKSLASH, false)),
        '|' => Some((KeyCode::KEY_BACKSLASH, true)),
        ',' => Some((KeyCode::KEY_COMMA, false)),
        '<' => Some((KeyCode::KEY_COMMA, true)),
        '.' => Some((KeyCode::KEY_DOT, false)),
        '>' => Some((KeyCode::KEY_DOT, true)),
        '/' => Some((KeyCode::KEY_SLASH, false)),
        '?' => Some((KeyCode::KEY_SLASH, true)),
        '!' => Some((KeyCode::KEY_1, true)),
        '@' => Some((KeyCode::KEY_2, true)),
        '#' => Some((KeyCode::KEY_3, true)),
        '$' => Some((KeyCode::KEY_4, true)),
        '%' => Some((KeyCode::KEY_5, true)),
        '^' => Some((KeyCode::KEY_6, true)),
        '&' => Some((KeyCode::KEY_7, true)),
        '*' => Some((KeyCode::KEY_8, true)),
        '(' => Some((KeyCode::KEY_9, true)),
        ')' => Some((KeyCode::KEY_0, true)),
        _ => None,
    }
}

fn modifier_key(modifier: Modifiers) -> Option<KeyCode> {
    if modifier == Modifiers::SHIFT {
        Some(KeyCode::KEY_LEFTSHIFT)
    } else if modifier == Modifiers::CONTROL {
        Some(KeyCode::KEY_LEFTCTRL)
    } else if modifier == Modifiers::OPTION {
        Some(KeyCode::KEY_LEFTALT)
    } else if modifier == Modifiers::COMMAND {
        Some(KeyCode::KEY_LEFTMETA)
    } else if modifier == Modifiers::CAPS_LOCK {
        Some(KeyCode::KEY_CAPSLOCK)
    } else {
        None
    }
}

fn modifier_events(previous: Modifiers, current: Modifiers) -> Vec<InputEvent> {
    [
        Modifiers::SHIFT,
        Modifiers::CONTROL,
        Modifiers::OPTION,
        Modifiers::COMMAND,
        Modifiers::CAPS_LOCK,
    ]
    .into_iter()
    .filter_map(|modifier| {
        let was_down = previous.contains(modifier);
        let is_down = current.contains(modifier);
        (was_down != is_down).then(|| {
            InputEvent::new(
                EventType::KEY.0,
                modifier_key(modifier).expect("known modifier").code(),
                i32::from(is_down),
            )
        })
    })
    .collect()
}

pub struct LinuxInputInjector {
    gamepads: crate::gamepad_linux::Gamepads,
    pointer: VirtualDevice,
    wheel: VirtualDevice,
    keyboard: VirtualDevice,
    geometry: OutputGeometry,
    modifiers: Modifiers,
    held_keys: HashSet<KeyCode>,
    pending_surrogate: Option<u16>,
    unicode_worker: UnicodeWorker,
    unicode_batch: UnicodeBatch,
    vertical_wheel: WheelAccumulator,
    horizontal_wheel: WheelAccumulator,
}

impl LinuxInputInjector {
    pub fn new(geometry: OutputGeometry) -> io::Result<Self> {
        if geometry.width == 0
            || geometry.height == 0
            || geometry.desktop_width == 0
            || geometry.desktop_height == 0
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "output and desktop dimensions must be non-zero",
            ));
        }

        let pointer_keys =
            AttributeSet::from_iter([KeyCode::BTN_LEFT, KeyCode::BTN_RIGHT, KeyCode::BTN_MIDDLE]);
        let props = AttributeSet::from_iter([evdev::PropType::DIRECT]);
        let max_x = evdev_abs_axis_max(geometry.desktop_width);
        let max_y = evdev_abs_axis_max(geometry.desktop_height);
        let abs_info_x = AbsInfo::new(-1, 0, max_x, 0, 0, 28);
        let abs_info_y = AbsInfo::new(-1, 0, max_y, 0, 0, 28);
        let abs_x = UinputAbsSetup::new(AbsoluteAxisCode::ABS_X, abs_info_x);
        let abs_y = UinputAbsSetup::new(AbsoluteAxisCode::ABS_Y, abs_info_y);
        let pointer = VirtualDevice::builder()?
            .name("MahoRD Virtual Pointer")
            .with_properties(&props)?
            .with_keys(&pointer_keys)?
            .with_absolute_axis(&abs_x)?
            .with_absolute_axis(&abs_y)?
            .build()?;

        let relative_axes = AttributeSet::from_iter([
            RelativeAxisCode::REL_WHEEL,
            RelativeAxisCode::REL_HWHEEL,
            RelativeAxisCode::REL_WHEEL_HI_RES,
            RelativeAxisCode::REL_HWHEEL_HI_RES,
            RelativeAxisCode::REL_X,
            RelativeAxisCode::REL_Y,
        ]);
        let wheel = VirtualDevice::builder()?
            .name("MahoRD Virtual Wheel")
            .with_relative_axes(&relative_axes)?
            .build()?;

        let mut keyboard_keys = AttributeSet::<KeyCode>::new();
        for code in (0..=126).chain(0x100..=0x10e) {
            if let Some(key) = macos_keycode_to_evdev(code) {
                keyboard_keys.insert(key);
            }
        }
        for key in [
            KeyCode::KEY_LEFTSHIFT,
            KeyCode::KEY_LEFTCTRL,
            KeyCode::KEY_LEFTALT,
            KeyCode::KEY_LEFTMETA,
            KeyCode::KEY_CAPSLOCK,
        ] {
            keyboard_keys.insert(key);
        }
        let keyboard = VirtualDevice::builder()?
            .name("MahoRD Virtual Keyboard")
            .with_keys(&keyboard_keys)?
            .build()?;

        Ok(Self {
            gamepads: crate::gamepad_linux::Gamepads::default(),
            pointer,
            wheel,
            keyboard,
            geometry,
            modifiers: Modifiers::empty(),
            held_keys: HashSet::new(),
            pending_surrogate: None,
            unicode_worker: UnicodeWorker::spawn(inject_wayland_unicode),
            unicode_batch: UnicodeBatch::default(),
            vertical_wheel: WheelAccumulator::default(),
            horizontal_wheel: WheelAccumulator::default(),
        })
    }

    pub fn update_geometry(&mut self, geometry: OutputGeometry) -> io::Result<()> {
        if self.geometry.desktop_width != geometry.desktop_width
            || self.geometry.desktop_height != geometry.desktop_height
        {
            *self = Self::new(geometry)?;
            return Ok(());
        }
        self.geometry = geometry;
        Ok(())
    }

    /// Flush coalesced non-ASCII text once no more arrived for
    /// `UNICODE_BATCH_IDLE`. The session loop calls this on every iteration.
    pub fn flush_idle_text(&mut self, now: Instant) {
        if let Some(text) = self.unicode_batch.take_if_idle(now) {
            self.type_unicode(&text);
        }
    }

    fn flush_pending_text(&mut self) {
        if let Some(text) = self.unicode_batch.take_pending() {
            self.type_unicode(&text);
        }
    }

    /// Hands text to the ordered `wtype` worker; never blocks the input thread.
    fn type_unicode(&mut self, text: &str) {
        self.unicode_worker.submit(text.to_owned());
    }

    pub fn inject(&mut self, event: &WireInputEvent) -> io::Result<()> {
        if !event.x.is_finite()
            || !event.y.is_finite()
            || !event.scroll_dx.is_finite()
            || !event.scroll_dy.is_finite()
        {
            return Ok(());
        }
        // Text queued before any key, pointer, or Reset event must land first.
        if flushes_pending_text(event.event_type) {
            self.flush_pending_text();
        }
        // `wtype` runs on a worker; uinput keyboard events must still land
        // after text queued before them. Pointer, wheel, and gamepad events
        // are independent devices and never wait.
        if matches!(
            event.event_type,
            InputEventType::KeyDown
                | InputEventType::KeyUp
                | InputEventType::FlagsChanged
                | InputEventType::Reset
        ) {
            self.unicode_worker.wait_idle();
        }

        match event.event_type {
            InputEventType::MouseMove
            | InputEventType::LeftMouseDragged
            | InputEventType::RightMouseDragged
            | InputEventType::LeftMouseDown
            | InputEventType::LeftMouseUp
            | InputEventType::RightMouseDown
            | InputEventType::RightMouseUp
            | InputEventType::MiddleMouseDown
            | InputEventType::MiddleMouseUp
            | InputEventType::PenMove
            | InputEventType::PenDown
            | InputEventType::PenUp => {
                let (pixel_x, pixel_y) = map_normalized_to_output(event.x, event.y, self.geometry);
                // Fixed-size batch: pointer moves are the hottest input path.
                let mut events = [
                    *AbsoluteAxisEvent::new(AbsoluteAxisCode::ABS_X, pixel_x as i32),
                    *AbsoluteAxisEvent::new(AbsoluteAxisCode::ABS_Y, pixel_y as i32),
                    InputEvent::new(EventType::SYNCHRONIZATION.0, 0, 0),
                ];
                let mut len = 2;
                let button = match event.event_type {
                    InputEventType::LeftMouseDown | InputEventType::PenDown => {
                        Some((KeyCode::BTN_LEFT, 1))
                    }
                    InputEventType::LeftMouseUp | InputEventType::PenUp => {
                        Some((KeyCode::BTN_LEFT, 0))
                    }
                    InputEventType::RightMouseDown => Some((KeyCode::BTN_RIGHT, 1)),
                    InputEventType::RightMouseUp => Some((KeyCode::BTN_RIGHT, 0)),
                    InputEventType::MiddleMouseDown => Some((KeyCode::BTN_MIDDLE, 1)),
                    InputEventType::MiddleMouseUp => Some((KeyCode::BTN_MIDDLE, 0)),
                    _ => None,
                };
                if let Some((key, value)) = button {
                    events[len] = InputEvent::new(EventType::KEY.0, key.code(), value);
                    len += 1;
                }
                self.pointer.emit(&events[..len])
            }
            InputEventType::GamepadAxis
            | InputEventType::GamepadButtonDown
            | InputEventType::GamepadButtonUp => self.gamepads.inject(event),
            InputEventType::RelativeMove => {
                let dx = event.scroll_dx.round() as i32;
                let dy = event.scroll_dy.round() as i32;
                let mut events = Vec::with_capacity(2);
                if dx != 0 {
                    events.push(InputEvent::new(
                        EventType::RELATIVE.0,
                        RelativeAxisCode::REL_X.0,
                        dx,
                    ));
                }
                if dy != 0 {
                    events.push(InputEvent::new(
                        EventType::RELATIVE.0,
                        RelativeAxisCode::REL_Y.0,
                        dy,
                    ));
                }
                if events.is_empty() {
                    Ok(())
                } else {
                    self.wheel.emit(&events)
                }
            }
            InputEventType::Reset => {
                self.gamepads.reset();
                let pointer_events = vec![
                    InputEvent::new(EventType::KEY.0, KeyCode::BTN_LEFT.code(), 0),
                    InputEvent::new(EventType::KEY.0, KeyCode::BTN_RIGHT.code(), 0),
                    InputEvent::new(EventType::KEY.0, KeyCode::BTN_MIDDLE.code(), 0),
                ];
                let _ = self.pointer.emit(&pointer_events);
                let mut kb_events = Vec::new();
                for key in self.held_keys.drain() {
                    kb_events.push(InputEvent::new(EventType::KEY.0, key.code(), 0));
                }
                for key in [
                    KeyCode::KEY_LEFTSHIFT,
                    KeyCode::KEY_RIGHTSHIFT,
                    KeyCode::KEY_LEFTCTRL,
                    KeyCode::KEY_RIGHTCTRL,
                    KeyCode::KEY_LEFTALT,
                    KeyCode::KEY_RIGHTALT,
                    KeyCode::KEY_LEFTMETA,
                    KeyCode::KEY_RIGHTMETA,
                    KeyCode::KEY_CAPSLOCK,
                ] {
                    kb_events.push(InputEvent::new(EventType::KEY.0, key.code(), 0));
                }
                self.modifiers = Modifiers::empty();
                self.pending_surrogate = None;
                self.vertical_wheel = WheelAccumulator::default();
                self.horizontal_wheel = WheelAccumulator::default();
                if !kb_events.is_empty() {
                    let _ = self.keyboard.emit(&kb_events);
                }
                Ok(())
            }
            InputEventType::ScrollWheel => {
                let (vertical, vertical_notches) = self.vertical_wheel.convert(-event.scroll_dy);
                let (horizontal, horizontal_notches) =
                    self.horizontal_wheel.convert(event.scroll_dx);
                let mut events = Vec::with_capacity(4);
                if vertical_notches != 0 {
                    events.push(InputEvent::new(
                        EventType::RELATIVE.0,
                        RelativeAxisCode::REL_WHEEL.0,
                        vertical_notches,
                    ));
                }
                if vertical != 0 {
                    events.push(InputEvent::new(
                        EventType::RELATIVE.0,
                        RelativeAxisCode::REL_WHEEL_HI_RES.0,
                        vertical,
                    ));
                }
                if horizontal_notches != 0 {
                    events.push(InputEvent::new(
                        EventType::RELATIVE.0,
                        RelativeAxisCode::REL_HWHEEL.0,
                        horizontal_notches,
                    ));
                }
                if horizontal != 0 {
                    events.push(InputEvent::new(
                        EventType::RELATIVE.0,
                        RelativeAxisCode::REL_HWHEEL_HI_RES.0,
                        horizontal,
                    ));
                }
                if events.is_empty() {
                    Ok(())
                } else {
                    self.wheel.emit(&events)
                }
            }
            InputEventType::KeyDown | InputEventType::KeyUp => {
                let is_down = event.event_type == InputEventType::KeyDown;
                if let Some(key) = macos_keycode_to_evdev(event.key_code) {
                    let mut events = Vec::with_capacity(6);
                    // Only sync modifiers if not a modifier key itself
                    let is_mod_key = matches!(
                        key,
                        KeyCode::KEY_LEFTSHIFT
                            | KeyCode::KEY_RIGHTSHIFT
                            | KeyCode::KEY_LEFTCTRL
                            | KeyCode::KEY_RIGHTCTRL
                            | KeyCode::KEY_LEFTALT
                            | KeyCode::KEY_RIGHTALT
                            | KeyCode::KEY_LEFTMETA
                            | KeyCode::KEY_RIGHTMETA
                            | KeyCode::KEY_CAPSLOCK
                    );

                    if !is_mod_key {
                        let mod_changes = modifier_events(self.modifiers, event.modifiers);
                        events.extend(mod_changes);
                        self.modifiers = event.modifiers;
                    }

                    events.push(InputEvent::new(
                        EventType::KEY.0,
                        key.code(),
                        i32::from(is_down),
                    ));
                    self.keyboard.emit(&events)?;
                    if is_down {
                        self.held_keys.insert(key);
                    } else {
                        self.held_keys.remove(&key);
                    }
                }
                Ok(())
            }
            InputEventType::FlagsChanged => {
                let events = modifier_events(self.modifiers, event.modifiers);
                self.modifiers = event.modifiers;
                if events.is_empty() {
                    Ok(())
                } else {
                    self.keyboard.emit(&events)
                }
            }
            InputEventType::UnicodeChar => {
                if let Some(ch) = decode_utf16_unit(&mut self.pending_surrogate, event.key_code)? {
                    match text_input_route(ch) {
                        TextInputRoute::Evdev(key, needs_shift) => {
                            self.flush_pending_text();
                            self.unicode_worker.wait_idle();
                            // Separate SYN reports preserve each key transition.
                            if needs_shift {
                                self.keyboard.emit(&[InputEvent::new(
                                    EventType::KEY.0,
                                    KeyCode::KEY_LEFTSHIFT.code(),
                                    1,
                                )])?;
                            }
                            self.keyboard.emit(&[InputEvent::new(
                                EventType::KEY.0,
                                key.code(),
                                1,
                            )])?;
                            self.keyboard.emit(&[InputEvent::new(
                                EventType::KEY.0,
                                key.code(),
                                0,
                            )])?;
                            if needs_shift {
                                self.keyboard.emit(&[InputEvent::new(
                                    EventType::KEY.0,
                                    KeyCode::KEY_LEFTSHIFT.code(),
                                    0,
                                )])?;
                            }
                        }
                        TextInputRoute::Unicode => {
                            if let Some(text) = self.unicode_batch.push(ch, Instant::now()) {
                                self.type_unicode(&text);
                            }
                        }
                    }
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum TextInputRoute {
    Evdev(KeyCode, bool),
    Unicode,
}

fn text_input_route(ch: char) -> TextInputRoute {
    match ascii_to_evdev(ch) {
        Some((key, shift)) => TextInputRoute::Evdev(key, shift),
        None => TextInputRoute::Unicode,
    }
}

/// Idle gap after which queued non-ASCII text is typed.
const UNICODE_BATCH_IDLE: Duration = Duration::from_millis(8);
/// Upper bound per `wtype` invocation so one spawn stays inside its 100 ms bound.
const UNICODE_BATCH_MAX_CHARS: usize = 32;

/// Every event except `UnicodeChar` flushes queued text first; ASCII
/// `UnicodeChar` flushes on its own route before reaching uinput.
fn flushes_pending_text(event_type: InputEventType) -> bool {
    event_type != InputEventType::UnicodeChar
}

/// Coalesces consecutive non-ASCII characters into one `wtype` invocation.
#[derive(Default)]
struct UnicodeBatch {
    text: String,
    chars: usize,
    last: Option<Instant>,
}

impl UnicodeBatch {
    /// Queue `ch`; returns the batch when it reached its size cap.
    fn push(&mut self, ch: char, now: Instant) -> Option<String> {
        self.text.push(ch);
        self.chars += 1;
        self.last = Some(now);
        (self.chars >= UNICODE_BATCH_MAX_CHARS).then(|| self.take())
    }

    fn take_if_idle(&mut self, now: Instant) -> Option<String> {
        let last = self.last?;
        (now.saturating_duration_since(last) >= UNICODE_BATCH_IDLE).then(|| self.take())
    }

    fn take_pending(&mut self) -> Option<String> {
        (!self.text.is_empty()).then(|| self.take())
    }

    fn take(&mut self) -> String {
        self.chars = 0;
        self.last = None;
        std::mem::take(&mut self.text)
    }
}

/// Queue depth for text waiting on `wtype`; beyond it text is dropped so a
/// wedged compositor cannot grow memory without bound.
const UNICODE_QUEUE_DEPTH: usize = 64;

/// Runs blocking Unicode insertions on one FIFO thread so batches keep their
/// order while the input loop stays free.
struct UnicodeWorker {
    sender: Option<SyncSender<String>>,
    in_flight: Arc<(Mutex<usize>, Condvar)>,
    warned: bool,
    handle: Option<thread::JoinHandle<()>>,
}

impl UnicodeWorker {
    fn spawn<F>(backend: F) -> Self
    where
        F: Fn(&str) -> io::Result<()> + Send + 'static,
    {
        let (sender, receiver) = sync_channel::<String>(UNICODE_QUEUE_DEPTH);
        let in_flight = Arc::new((Mutex::new(0_usize), Condvar::new()));
        let worker_state = Arc::clone(&in_flight);
        let handle = thread::Builder::new()
            .name("maho-unicode-input".into())
            .spawn(move || {
                let mut warned = false;
                while let Ok(text) = receiver.recv() {
                    if let Err(error) = backend(&text) {
                        if !warned {
                            tracing::warn!(%error, "Wayland Unicode backend unavailable; dropping text");
                            warned = true;
                        }
                    }
                    let (count, idle) = &*worker_state;
                    let mut count = count.lock().unwrap_or_else(|e| e.into_inner());
                    *count -= 1;
                    if *count == 0 {
                        idle.notify_all();
                    }
                }
            })
            .ok();
        Self {
            sender: handle.as_ref().and(Some(sender)),
            in_flight,
            warned: false,
            handle,
        }
    }

    fn submit(&mut self, text: String) {
        let Some(sender) = &self.sender else {
            self.warn_once("Unicode worker unavailable; dropping text");
            return;
        };
        let (count, _) = &*self.in_flight;
        *count.lock().unwrap_or_else(|e| e.into_inner()) += 1;
        match sender.try_send(text) {
            Ok(()) => {}
            Err(error) => {
                let (count, idle) = &*self.in_flight;
                let mut count = count.lock().unwrap_or_else(|e| e.into_inner());
                *count -= 1;
                if *count == 0 {
                    idle.notify_all();
                }
                drop(count);
                self.warn_once(match error {
                    TrySendError::Full(_) => "Unicode input queue full; dropping text",
                    TrySendError::Disconnected(_) => "Unicode worker stopped; dropping text",
                });
            }
        }
    }

    fn warn_once(&mut self, message: &str) {
        if !self.warned {
            tracing::warn!("{message}");
            self.warned = true;
        }
    }

    /// Blocks until every queued insertion finished (each bounded to 100 ms).
    fn wait_idle(&self) {
        let (count, idle) = &*self.in_flight;
        let mut count = count.lock().unwrap_or_else(|e| e.into_inner());
        while *count > 0 {
            count = idle.wait(count).unwrap_or_else(|e| e.into_inner());
        }
    }
}

impl Drop for UnicodeWorker {
    fn drop(&mut self) {
        self.sender = None;
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn inject_wayland_unicode(text: &str) -> io::Result<()> {
    // Bound each insertion to 100 ms. Consecutive non-ASCII characters are
    // coalesced by `UnicodeBatch`, so an IME burst costs one spawn, not one
    // per character. Kill and reap on timeout; ASCII never starts a subprocess.
    let deadline = Instant::now() + Duration::from_millis(100);
    let mut command = Command::new("wtype");
    command
        .env("LC_ALL", "C.UTF-8")
        .args(["--", text])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for name in ["WAYLAND_DISPLAY", "XDG_RUNTIME_DIR"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    let mut child = command.spawn()?;
    let result = loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return if status.success() {
                    Ok(())
                } else {
                    Err(io::Error::other(format!("wtype failed: {status}")))
                };
            }
            Ok(None) => {}
            Err(error) => break Err(error),
        }
        if Instant::now() >= deadline {
            break Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "wtype exceeded 100 ms",
            ));
        }
        thread::sleep(Duration::from_millis(1));
    };
    child.kill()?;
    child.wait()?;
    result
}

/// Decode the wire's UTF-16 units without losing supplementary characters.
fn decode_utf16_unit(pending: &mut Option<u16>, unit: u16) -> io::Result<Option<char>> {
    if (0xD800..=0xDBFF).contains(&unit) {
        *pending = Some(unit);
        return Ok(None);
    }
    let scalar = if (0xDC00..=0xDFFF).contains(&unit) {
        let high = pending.take().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "unpaired UTF-16 low surrogate")
        })?;
        0x10000 + ((u32::from(high) - 0xD800) << 10) + u32::from(unit) - 0xDC00
    } else {
        *pending = None;
        u32::from(unit)
    };
    char::from_u32(scalar)
        .map(Some)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid Unicode scalar"))
}

/// Accumulates browser pixels into integer high-resolution units and notches.
#[derive(Default)]
struct WheelAccumulator {
    fractional_units: f64,
    notch_units: i32,
}

impl WheelAccumulator {
    fn convert(&mut self, pixels: f32) -> (i32, i32) {
        if !pixels.is_finite() {
            return (0, 0);
        }
        let units = (f64::from(pixels) * 1.2 + self.fractional_units).clamp(-32767.0, 32767.0);
        let high_res = units.trunc() as i32;
        self.fractional_units = units - f64::from(high_res);
        self.notch_units += high_res;
        let notches = self.notch_units / 120;
        self.notch_units %= 120;
        (high_res, notches)
    }
}

impl Drop for LinuxInputInjector {
    fn drop(&mut self) {
        let reset = WireInputEvent {
            event_type: InputEventType::Reset,
            x: 0.0,
            y: 0.0,
            key_code: 0,
            modifiers: Modifiers::empty(),
            scroll_dx: 0.0,
            scroll_dy: 0.0,
        };
        let _ = self.inject(&reset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regression_evdev_abs_axis_max_matches_pixel_index_boundary() {
        assert_eq!(evdev_abs_axis_max(6400), 6399);
        assert_eq!(evdev_abs_axis_max(1600), 1599);
        assert_eq!(evdev_abs_axis_max(3840), 3839);
        assert_eq!(evdev_abs_axis_max(1), 0);
        assert_eq!(evdev_abs_axis_max(0), 0);
    }

    #[test]
    fn maps_normalized_coordinates_into_target_output() {
        let geometry = OutputGeometry {
            x: 1920,
            y: 100,
            width: 2560,
            height: 1440,
            desktop_x: 0,
            desktop_y: 0,
            desktop_width: 4480,
            desktop_height: 1540,
        };
        for ((local_x, local_y), expected) in [
            ((0.0, 0.0), (1920, 100)),
            ((2560.0, 1440.0), (4479, 1539)),
            ((-5120.0, 2880.0), (1920, 1539)),
        ] {
            let (wire_x, wire_y) =
                ferryx_rd_proto::normalize_client_coordinates(local_x, local_y, 2560.0, 1440.0);
            assert_eq!(map_normalized_to_output(wire_x, wire_y, geometry), expected);
        }
        assert_eq!(map_normalized_to_output(0.0, 0.0, geometry), (1920, 1539));
        assert_eq!(map_normalized_to_output(-2.0, 2.0, geometry), (1920, 100));
    }

    #[test]
    fn maps_outputs_left_of_and_above_the_primary_display() {
        // 1920x1080 output placed left of and above a 2560x1440 primary.
        let geometry = OutputGeometry {
            x: -1920,
            y: -1080,
            width: 1920,
            height: 1080,
            desktop_x: -1920,
            desktop_y: -1080,
            desktop_width: 4480,
            desktop_height: 2520,
        };
        // Top-left of that output maps to the desktop origin, not a clamp.
        assert_eq!(map_normalized_to_output(0.0, 1.0, geometry), (0, 0));
        // Bottom-right stays inside the same output instead of collapsing to x=0.
        assert_eq!(map_normalized_to_output(1.0, 0.0, geometry), (1919, 1079));
        assert_eq!(map_normalized_to_output(0.5, 0.5, geometry), (960, 540));
    }

    #[test]
    fn maps_v3_keycodes_and_modifiers() {
        assert_eq!(macos_keycode_to_evdev(0), Some(KeyCode::KEY_A));
        assert_eq!(macos_keycode_to_evdev(36), Some(KeyCode::KEY_ENTER));
        assert_eq!(macos_keycode_to_evdev(123), Some(KeyCode::KEY_LEFT));
        assert_eq!(macos_keycode_to_evdev(u16::MAX), None);

        let flags = Modifiers::SHIFT | Modifiers::COMMAND | Modifiers::CAPS_LOCK;
        let events = modifier_events(Modifiers::empty(), flags);
        assert_eq!(events.len(), 3);
        assert!(events.iter().all(|event| event.value() == 1));
        let releases = modifier_events(flags, Modifiers::empty());
        assert!(releases.iter().all(|event| event.value() == 0));
    }

    #[test]
    fn maps_middle_click_and_reset_button_codes() {
        assert_eq!(KeyCode::BTN_MIDDLE.code(), 0x112);
        assert_eq!(KeyCode::BTN_LEFT.code(), 0x110);
        assert_eq!(KeyCode::BTN_RIGHT.code(), 0x111);
    }

    #[test]
    fn decodes_unicode_bmp_and_supplementary_text() {
        let mut pending = None;
        assert_eq!(decode_utf16_unit(&mut pending, 0x00E9).unwrap(), Some('é'));
        assert_eq!(decode_utf16_unit(&mut pending, 0xD55C).unwrap(), Some('한'));
        assert_eq!(decode_utf16_unit(&mut pending, 0xD83D).unwrap(), None);
        assert_eq!(decode_utf16_unit(&mut pending, 0xDE80).unwrap(), Some('🚀'));
        assert!(decode_utf16_unit(&mut pending, 0xDE80).is_err());
    }

    #[test]
    fn ascii_text_routes_directly_to_evdev() {
        assert_eq!(
            text_input_route('a'),
            TextInputRoute::Evdev(KeyCode::KEY_A, false)
        );
        assert_eq!(
            text_input_route('A'),
            TextInputRoute::Evdev(KeyCode::KEY_A, true)
        );
        assert_eq!(
            text_input_route('!'),
            TextInputRoute::Evdev(KeyCode::KEY_1, true)
        );
        assert_eq!(
            text_input_route('\n'),
            TextInputRoute::Evdev(KeyCode::KEY_ENTER, false)
        );
    }

    #[test]
    fn unmapped_text_routes_to_unicode_backend() {
        for ch in ['é', '한', '🚀'] {
            assert_eq!(text_input_route(ch), TextInputRoute::Unicode);
        }
    }

    #[test]
    fn unicode_batch_coalesces_until_idle_or_cap() {
        let start = Instant::now();
        let mut batch = UnicodeBatch::default();
        assert_eq!(batch.take_if_idle(start), None);
        assert_eq!(batch.push('한', start), None);
        assert_eq!(batch.push('글', start + Duration::from_millis(5)), None);
        // Idle is measured from the last character, not the first.
        assert_eq!(batch.take_if_idle(start + Duration::from_millis(12)), None);
        assert_eq!(
            batch.take_if_idle(start + Duration::from_millis(13)),
            Some("한글".into())
        );
        assert_eq!(batch.take_pending(), None);

        assert_eq!(batch.push('é', start), None);
        assert_eq!(batch.take_pending(), Some("é".into()));
        assert_eq!(batch.take_if_idle(start + Duration::from_secs(1)), None);

        for _ in 1..UNICODE_BATCH_MAX_CHARS {
            assert_eq!(batch.push('é', start), None);
        }
        let full = batch.push('é', start).unwrap();
        assert_eq!(full.chars().count(), UNICODE_BATCH_MAX_CHARS);
        assert_eq!(batch.take_pending(), None);
    }

    #[test]
    fn unicode_worker_is_ordered_nonblocking_and_drains() {
        use std::sync::mpsc;
        let (gate_tx, gate_rx) = mpsc::channel::<()>();
        let gate_rx = Mutex::new(gate_rx);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        let mut worker = UnicodeWorker::spawn(move |text| {
            gate_rx.lock().unwrap().recv().unwrap();
            sink.lock().unwrap().push(text.to_owned());
            Ok(())
        });
        // Submitting while the backend is blocked must return immediately.
        worker.submit("a".into());
        worker.submit("b".into());
        worker.submit("c".into());
        assert!(seen.lock().unwrap().is_empty());
        for _ in 0..3 {
            gate_tx.send(()).unwrap();
        }
        worker.wait_idle();
        assert_eq!(*seen.lock().unwrap(), ["a", "b", "c"]);
    }

    #[test]
    fn unicode_worker_drops_when_queue_is_full_without_hanging() {
        use std::sync::mpsc;
        let (gate_tx, gate_rx) = mpsc::channel::<()>();
        let gate_rx = Mutex::new(gate_rx);
        let mut worker = UnicodeWorker::spawn(move |_| {
            let _ = gate_rx.lock().unwrap().recv();
            Ok(())
        });
        for _ in 0..UNICODE_QUEUE_DEPTH * 2 {
            worker.submit("x".into());
        }
        drop(gate_tx);
        worker.wait_idle();
    }

    #[test]
    fn non_text_events_flush_pending_unicode() {
        assert!(!flushes_pending_text(InputEventType::UnicodeChar));
        for event_type in [
            InputEventType::KeyDown,
            InputEventType::KeyUp,
            InputEventType::FlagsChanged,
            InputEventType::MouseMove,
            InputEventType::LeftMouseDown,
            InputEventType::ScrollWheel,
            InputEventType::Reset,
        ] {
            assert!(flushes_pending_text(event_type));
        }
    }

    #[test]
    fn wheel_pixels_have_correct_sign_and_magnitude() {
        let mut vertical = WheelAccumulator::default();
        let mut horizontal = WheelAccumulator::default();
        assert_eq!(vertical.convert(-100.0), (-120, -1));
        assert_eq!(horizontal.convert(100.0), (120, 1));
        assert_eq!(vertical.convert(200.0), (240, 2));
    }

    #[test]
    fn wheel_accumulates_subpixel_and_subnotch_remainders() {
        let mut wheel = WheelAccumulator::default();
        assert_eq!(wheel.convert(0.5), (0, 0));
        assert_eq!(wheel.convert(0.5), (1, 0));
        assert_eq!(wheel.convert(49.0), (59, 0));
        assert_eq!(wheel.convert(50.0), (60, 1));
        assert_eq!(wheel.convert(-50.0), (-60, 0));
        assert_eq!(wheel.convert(50.0), (60, 0));
    }

    #[test]
    fn maps_volume_and_extended_function_keys() {
        for (index, key) in [
            KeyCode::KEY_PLAYPAUSE,
            KeyCode::KEY_NEXTSONG,
            KeyCode::KEY_PREVIOUSSONG,
            KeyCode::KEY_STOPCD,
            KeyCode::KEY_BACK,
            KeyCode::KEY_FORWARD,
            KeyCode::KEY_REFRESH,
            KeyCode::KEY_STOP,
            KeyCode::KEY_SEARCH,
            KeyCode::KEY_BOOKMARKS,
            KeyCode::KEY_HOMEPAGE,
            KeyCode::KEY_F21,
            KeyCode::KEY_F22,
            KeyCode::KEY_F23,
            KeyCode::KEY_F24,
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(macos_keycode_to_evdev(0x100 + index as u16), Some(key));
        }
        for (code, key) in [
            (72, KeyCode::KEY_VOLUMEUP),
            (73, KeyCode::KEY_VOLUMEDOWN),
            (74, KeyCode::KEY_MUTE),
            (90, KeyCode::KEY_F20),
        ] {
            assert_eq!(macos_keycode_to_evdev(code), Some(key));
        }
    }

    #[test]
    fn ascii_to_evdev_maps_alphanumerics_and_symbols() {
        assert_eq!(ascii_to_evdev('a'), Some((KeyCode::KEY_A, false)));
        assert_eq!(ascii_to_evdev('A'), Some((KeyCode::KEY_A, true)));
        assert_eq!(ascii_to_evdev('1'), Some((KeyCode::KEY_1, false)));
        assert_eq!(ascii_to_evdev('!'), Some((KeyCode::KEY_1, true)));
        assert_eq!(ascii_to_evdev(' '), Some((KeyCode::KEY_SPACE, false)));
        assert_eq!(ascii_to_evdev('\n'), Some((KeyCode::KEY_ENTER, false)));
        assert_eq!(ascii_to_evdev('한'), None);
    }
}
