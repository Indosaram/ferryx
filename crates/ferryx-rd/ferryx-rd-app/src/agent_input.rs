use std::{
    collections::{HashMap, HashSet},
    fmt,
    io::Cursor,
    time::{Duration, Instant},
};

use base64::Engine;
use image::{codecs::jpeg::JpegEncoder, codecs::png::PngEncoder, ColorType, ImageEncoder};
use ferryx_rd_proto::{InputEvent, InputEventType, Modifiers};
use serde::{Deserialize, Serialize};

use crate::input::InputKey;

fn default_click_count() -> u32 {
    1
}

fn default_drag_steps() -> u32 {
    10
}

fn default_drag_duration() -> u64 {
    200
}

fn default_key_hold_ms() -> u64 {
    50
}

fn default_type_delay_ms() -> u64 {
    20
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MonitorInfo {
    pub id: u32,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub is_primary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenInfo {
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    #[serde(default)]
    pub logical_width: Option<u32>,
    #[serde(default)]
    pub logical_height: Option<u32>,
    #[serde(default)]
    pub monitors: Vec<MonitorInfo>,
    pub connected_host: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ScreenshotFormat {
    #[default]
    Png,
    Jpeg,
}

pub fn nv12_to_rgb(width: u32, height: u32, nv12_buf: &[u8]) -> Option<Vec<u8>> {
    let w = width as usize;
    let h = height as usize;
    // The tightly packed NV12 contract assumes even dimensions: odd width has
    // no defined chroma stride, and odd height would need rounded-up chroma
    // extents. Checked arithmetic keeps extreme dimensions from overflowing
    // the advertised-length comparison or the RGB allocation.
    if w == 0 || h == 0 || w % 2 != 0 || h % 2 != 0 {
        return None;
    }
    let luma = w.checked_mul(h)?;
    let expected_len = luma.checked_mul(3)? / 2;
    if nv12_buf.len() < expected_len {
        return None;
    }

    let mut rgb = Vec::with_capacity(luma.checked_mul(3)?);
    let uv_plane_start = w * h;

    for y in 0..h {
        let y_row_start = y * w;
        let uv_row_start = uv_plane_start + (y / 2) * w;
        for x in 0..w {
            let y_val = nv12_buf[y_row_start + x] as f32;
            let uv_offset = uv_row_start + (x / 2) * 2;
            let u_val = nv12_buf[uv_offset] as f32 - 128.0;
            let v_val = nv12_buf[uv_offset + 1] as f32 - 128.0;

            let r = (y_val + 1.5748 * v_val).clamp(0.0, 255.0) as u8;
            let g = (y_val - 0.1873 * u_val - 0.4681 * v_val).clamp(0.0, 255.0) as u8;
            let b = (y_val + 1.8556 * u_val).clamp(0.0, 255.0) as u8;

            rgb.push(r);
            rgb.push(g);
            rgb.push(b);
        }
    }

    Some(rgb)
}

pub fn encode_nv12_screenshot(
    width: u32,
    height: u32,
    nv12_buf: &[u8],
    format: ScreenshotFormat,
) -> Result<String, String> {
    let rgb = nv12_to_rgb(width, height, nv12_buf)
        .ok_or_else(|| "invalid nv12 buffer size or zero dimensions".to_string())?;

    let mut output = Vec::new();
    let mut cursor = Cursor::new(&mut output);

    match format {
        ScreenshotFormat::Png => {
            let encoder = PngEncoder::new(&mut cursor);
            encoder
                .write_image(&rgb, width, height, ColorType::Rgb8.into())
                .map_err(|e| format!("failed to encode png: {e}"))?;
        }
        ScreenshotFormat::Jpeg => {
            let mut encoder = JpegEncoder::new_with_quality(&mut cursor, 80);
            encoder
                .encode(&rgb, width, height, ColorType::Rgb8.into())
                .map_err(|e| format!("failed to encode jpeg: {e}"))?;
        }
    }

    Ok(base64::prelude::BASE64_STANDARD.encode(&output))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameMetadata {
    pub frame_id: u64,
    pub timestamp_ms: u64,
    pub age_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum MouseButton {
    #[default]
    Left,
    Right,
    Middle,
}

impl MouseButton {
    pub fn to_down_event_type(self) -> InputEventType {
        match self {
            Self::Left => InputEventType::LeftMouseDown,
            Self::Right => InputEventType::RightMouseDown,
            Self::Middle => InputEventType::MiddleMouseDown,
        }
    }

    pub fn to_up_event_type(self) -> InputEventType {
        match self {
            Self::Left => InputEventType::LeftMouseUp,
            Self::Right => InputEventType::RightMouseUp,
            Self::Middle => InputEventType::MiddleMouseUp,
        }
    }

    pub fn to_drag_event_type(self) -> InputEventType {
        match self {
            Self::Left => InputEventType::LeftMouseDragged,
            Self::Right => InputEventType::RightMouseDragged,
            Self::Middle => InputEventType::LeftMouseDragged,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum AgentAction {
    MouseMove {
        x: f32,
        y: f32,
        #[serde(default)]
        normalized: bool,
    },
    MouseDown {
        #[serde(default)]
        button: MouseButton,
    },
    MouseUp {
        #[serde(default)]
        button: MouseButton,
    },
    Click {
        x: f32,
        y: f32,
        #[serde(default)]
        button: MouseButton,
        #[serde(default = "default_click_count")]
        count: u32,
        #[serde(default)]
        normalized: bool,
    },
    Drag {
        start_x: f32,
        start_y: f32,
        end_x: f32,
        end_y: f32,
        #[serde(default)]
        button: MouseButton,
        #[serde(default = "default_drag_steps")]
        steps: u32,
        #[serde(default = "default_drag_duration")]
        duration_ms: u64,
        #[serde(default)]
        normalized: bool,
    },
    Scroll {
        dx: f32,
        dy: f32,
        #[serde(default)]
        x: Option<f32>,
        #[serde(default)]
        y: Option<f32>,
        #[serde(default)]
        normalized: bool,
    },
    KeyDown {
        key: String,
    },
    KeyUp {
        key: String,
    },
    KeyPress {
        key: String,
        #[serde(default = "default_key_hold_ms")]
        hold_ms: u64,
    },
    Hotkey {
        keys: Vec<String>,
    },
    TypeText {
        text: String,
        #[serde(default = "default_type_delay_ms")]
        delay_ms: u64,
        #[serde(default)]
        paste_mode: bool,
    },
    ReleaseAll,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AgentInputError {
    UnknownKey(String),
    UnmappableKey(String),
    EmptyHotkey,
    InvalidCoordinates { x: f32, y: f32 },
    ActionTooLarge { max_events: usize },
}

impl fmt::Display for AgentInputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownKey(k) => write!(f, "unknown key name: '{k}'"),
            Self::UnmappableKey(k) => write!(f, "key has no host keycode mapping: '{k}'"),
            Self::EmptyHotkey => write!(f, "hotkey must contain at least one key"),
            Self::InvalidCoordinates { x, y } => {
                write!(f, "invalid non-finite coordinates ({x}, {y})")
            }
            Self::ActionTooLarge { max_events } => {
                write!(f, "action exceeds the {max_events}-event work limit")
            }
        }
    }
}

impl std::error::Error for AgentInputError {}

pub fn parse_key_name(name: &str) -> Result<(InputKey, Modifiers), AgentInputError> {
    let trimmed = name.trim();
    let lower = trimmed.to_ascii_lowercase();

    match lower.as_str() {
        "shift" | "leftshift" | "rightshift" => {
            return Ok((InputKey::WindowsVirtualKey(0x10), Modifiers::SHIFT));
        }
        "ctrl" | "control" | "leftctrl" | "rightctrl" => {
            return Ok((InputKey::WindowsVirtualKey(0x11), Modifiers::CONTROL));
        }
        "alt" | "option" | "leftalt" | "rightalt" => {
            return Ok((InputKey::WindowsVirtualKey(0x12), Modifiers::OPTION));
        }
        "super" | "win" | "windows" | "cmd" | "command" | "meta" => {
            return Ok((InputKey::WindowsVirtualKey(0x5B), Modifiers::COMMAND));
        }
        "capslock" => {
            return Ok((InputKey::WindowsVirtualKey(0x14), Modifiers::CAPS_LOCK));
        }
        _ => {}
    }

    let vk = match lower.as_str() {
        "enter" | "return" => 0x0D,
        "esc" | "escape" => 0x1B,
        "tab" => 0x09,
        "space" | " " => 0x20,
        "backspace" => 0x08,
        "delete" | "del" => 0x2E,
        "insert" | "ins" => 0x2D,
        "home" => 0x24,
        "end" => 0x23,
        "pageup" | "pgup" => 0x21,
        "pagedown" | "pgdn" => 0x22,
        "arrowup" | "up" => 0x26,
        "arrowdown" | "down" => 0x28,
        "arrowleft" | "left" => 0x25,
        "arrowright" | "right" => 0x27,
        "f1" => 0x70,
        "f2" => 0x71,
        "f3" => 0x72,
        "f4" => 0x73,
        "f5" => 0x74,
        "f6" => 0x75,
        "f7" => 0x76,
        "f8" => 0x77,
        "f9" => 0x78,
        "f10" => 0x79,
        "f11" => 0x7A,
        "f12" => 0x7B,
        "-" | "minus" => 0xBD,
        "=" | "equal" => 0xBB,
        "[" | "bracketleft" => 0xDB,
        "]" | "bracketright" => 0xDD,
        "\\" | "backslash" => 0xDC,
        ";" | "semicolon" => 0xBA,
        "'" | "quote" => 0xDE,
        "," | "comma" => 0xBC,
        "." | "period" => 0xBE,
        "/" | "slash" => 0xBF,
        "`" | "grave" => 0xC0,
        _ => {
            if trimmed.len() == 1 {
                let ch = trimmed.chars().next().unwrap();
                if ch.is_ascii_alphabetic() {
                    let upper = ch.to_ascii_uppercase() as u16;
                    let is_upper = ch.is_ascii_uppercase();
                    let mods = if is_upper {
                        Modifiers::SHIFT
                    } else {
                        Modifiers::empty()
                    };
                    return Ok((InputKey::WindowsVirtualKey(upper), mods));
                } else if ch.is_ascii_digit() {
                    return Ok((InputKey::WindowsVirtualKey(ch as u16), Modifiers::empty()));
                }
            }
            return Err(AgentInputError::UnknownKey(name.to_string()));
        }
    };

    Ok((InputKey::WindowsVirtualKey(vk), Modifiers::empty()))
}

pub fn parse_hotkey_string(hotkey: &str) -> Result<(InputKey, Modifiers), AgentInputError> {
    let parts: Vec<&str> = hotkey
        .split('+')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        return Err(AgentInputError::EmptyHotkey);
    }

    let mut combined_modifiers = Modifiers::empty();
    let mut target_key = None;

    for (i, part) in parts.iter().enumerate() {
        let is_last = i == parts.len() - 1;
        let (key, mods) = parse_key_name(part)?;
        if is_last {
            target_key = Some(key);
            combined_modifiers |= mods;
        } else if mods == Modifiers::empty() {
            combined_modifiers |= match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => Modifiers::CONTROL,
                "alt" | "opt" | "option" => Modifiers::OPTION,
                "shift" => Modifiers::SHIFT,
                "super" | "win" | "cmd" | "command" => Modifiers::COMMAND,
                _ => return Err(AgentInputError::UnknownKey(part.to_string())),
            };
        } else {
            combined_modifiers |= mods;
        }
    }

    let key = target_key.ok_or(AgentInputError::EmptyHotkey)?;
    Ok((key, combined_modifiers))
}

/// Resolves a parsed key to its host keycode, failing explicitly when no mapping exists.
///
/// A missing mapping must never fall back to `0`: that is the macOS keycode for `A`.
fn require_macos_keycode(key: InputKey, name: &str) -> Result<u16, AgentInputError> {
    crate::input::InputKeyMap::to_macos(key)
        .ok_or_else(|| AgentInputError::UnmappableKey(name.to_string()))
}

pub fn normalize_agent_coordinates(
    x: f32,
    y: f32,
    normalized: bool,
    host_width: f32,
    host_height: f32,
) -> Result<(f32, f32), AgentInputError> {
    if !x.is_finite() || !y.is_finite() {
        return Err(AgentInputError::InvalidCoordinates { x, y });
    }

    if normalized {
        let norm_x = x.clamp(0.0, 1.0);
        let norm_y = (1.0 - y).clamp(0.0, 1.0);
        Ok((norm_x, norm_y))
    } else {
        if host_width <= 0.0 || host_height <= 0.0 {
            return Ok((0.0, 0.0));
        }
        let norm_x = (x / host_width).clamp(0.0, 1.0);
        let norm_y = (1.0 - (y / host_height)).clamp(0.0, 1.0);
        Ok((norm_x, norm_y))
    }
}

pub fn synthesize_ascii_char(ch: char) -> Option<(InputKey, Modifiers)> {
    if ch.is_ascii_alphabetic() {
        let vk = ch.to_ascii_uppercase() as u16;
        let mods = if ch.is_ascii_uppercase() {
            Modifiers::SHIFT
        } else {
            Modifiers::empty()
        };
        Some((InputKey::WindowsVirtualKey(vk), mods))
    } else if ch.is_ascii_digit() {
        Some((InputKey::WindowsVirtualKey(ch as u16), Modifiers::empty()))
    } else {
        match ch {
            ' ' => Some((InputKey::WindowsVirtualKey(0x20), Modifiers::empty())),
            '\n' | '\r' => Some((InputKey::WindowsVirtualKey(0x0D), Modifiers::empty())),
            '\t' => Some((InputKey::WindowsVirtualKey(0x09), Modifiers::empty())),
            '-' => Some((InputKey::WindowsVirtualKey(0xBD), Modifiers::empty())),
            '_' => Some((InputKey::WindowsVirtualKey(0xBD), Modifiers::SHIFT)),
            '=' => Some((InputKey::WindowsVirtualKey(0xBB), Modifiers::empty())),
            '+' => Some((InputKey::WindowsVirtualKey(0xBB), Modifiers::SHIFT)),
            '[' => Some((InputKey::WindowsVirtualKey(0xDB), Modifiers::empty())),
            '{' => Some((InputKey::WindowsVirtualKey(0xDB), Modifiers::SHIFT)),
            ']' => Some((InputKey::WindowsVirtualKey(0xDD), Modifiers::empty())),
            '}' => Some((InputKey::WindowsVirtualKey(0xDD), Modifiers::SHIFT)),
            '\\' => Some((InputKey::WindowsVirtualKey(0xDC), Modifiers::empty())),
            '|' => Some((InputKey::WindowsVirtualKey(0xDC), Modifiers::SHIFT)),
            ';' => Some((InputKey::WindowsVirtualKey(0xBA), Modifiers::empty())),
            ':' => Some((InputKey::WindowsVirtualKey(0xBA), Modifiers::SHIFT)),
            '\'' => Some((InputKey::WindowsVirtualKey(0xDE), Modifiers::empty())),
            '"' => Some((InputKey::WindowsVirtualKey(0xDE), Modifiers::SHIFT)),
            ',' => Some((InputKey::WindowsVirtualKey(0xBC), Modifiers::empty())),
            '<' => Some((InputKey::WindowsVirtualKey(0xBC), Modifiers::SHIFT)),
            '.' => Some((InputKey::WindowsVirtualKey(0xBE), Modifiers::empty())),
            '>' => Some((InputKey::WindowsVirtualKey(0xBE), Modifiers::SHIFT)),
            '/' => Some((InputKey::WindowsVirtualKey(0xBF), Modifiers::empty())),
            '?' => Some((InputKey::WindowsVirtualKey(0xBF), Modifiers::SHIFT)),
            '`' => Some((InputKey::WindowsVirtualKey(0xC0), Modifiers::empty())),
            '~' => Some((InputKey::WindowsVirtualKey(0xC0), Modifiers::SHIFT)),
            '!' => Some((InputKey::WindowsVirtualKey(0x31), Modifiers::SHIFT)),
            '@' => Some((InputKey::WindowsVirtualKey(0x32), Modifiers::SHIFT)),
            '#' => Some((InputKey::WindowsVirtualKey(0x33), Modifiers::SHIFT)),
            '$' => Some((InputKey::WindowsVirtualKey(0x34), Modifiers::SHIFT)),
            '%' => Some((InputKey::WindowsVirtualKey(0x35), Modifiers::SHIFT)),
            '^' => Some((InputKey::WindowsVirtualKey(0x36), Modifiers::SHIFT)),
            '&' => Some((InputKey::WindowsVirtualKey(0x37), Modifiers::SHIFT)),
            '*' => Some((InputKey::WindowsVirtualKey(0x38), Modifiers::SHIFT)),
            '(' => Some((InputKey::WindowsVirtualKey(0x39), Modifiers::SHIFT)),
            ')' => Some((InputKey::WindowsVirtualKey(0x30), Modifiers::SHIFT)),
            _ => None,
        }
    }
}

pub struct InputStateTracker {
    active_buttons: HashSet<MouseButton>,
    active_keys: HashSet<u16>,
    /// Modifier bits each held key contributed, so releases can recompute the active set.
    key_modifiers: HashMap<u16, Modifiers>,
    active_modifiers: Modifiers,
    last_action_at: Instant,
    hold_timeout: Duration,
}

impl Default for InputStateTracker {
    fn default() -> Self {
        Self {
            active_buttons: HashSet::new(),
            active_keys: HashSet::new(),
            key_modifiers: HashMap::new(),
            active_modifiers: Modifiers::empty(),
            last_action_at: Instant::now(),
            hold_timeout: Duration::from_millis(5000),
        }
    }
}

impl InputStateTracker {
    pub fn new(hold_timeout: Duration) -> Self {
        Self {
            hold_timeout,
            ..Default::default()
        }
    }

    pub fn record_button_down(&mut self, button: MouseButton) {
        self.active_buttons.insert(button);
        self.last_action_at = Instant::now();
    }

    pub fn record_button_up(&mut self, button: MouseButton) {
        self.active_buttons.remove(&button);
        self.last_action_at = Instant::now();
    }

    pub fn record_key_down(&mut self, key_code: u16, modifiers: Modifiers) {
        self.active_keys.insert(key_code);
        *self
            .key_modifiers
            .entry(key_code)
            .or_insert(Modifiers::empty()) |= modifiers;
        self.active_modifiers |= modifiers;
        self.last_action_at = Instant::now();
    }

    pub fn record_key_up(&mut self, key_code: u16) {
        self.active_keys.remove(&key_code);
        self.key_modifiers.remove(&key_code);
        self.active_modifiers = self
            .key_modifiers
            .values()
            .fold(Modifiers::empty(), |acc, mods| acc | *mods);
        self.last_action_at = Instant::now();
    }

    pub fn is_empty(&self) -> bool {
        self.active_buttons.is_empty()
            && self.active_keys.is_empty()
            && self.active_modifiers == Modifiers::empty()
    }

    pub fn clear(&mut self) {
        self.active_buttons.clear();
        self.active_keys.clear();
        self.key_modifiers.clear();
        self.active_modifiers = Modifiers::empty();
        self.last_action_at = Instant::now();
    }

    pub fn release_all(&mut self, current_x: f32, current_y: f32) -> Vec<InputEvent> {
        let mut events = Vec::new();

        for button in self.active_buttons.drain() {
            events.push(InputEvent {
                event_type: button.to_up_event_type(),
                x: current_x,
                y: current_y,
                key_code: 0,
                modifiers: self.active_modifiers,
                scroll_dx: 0.0,
                scroll_dy: 0.0,
            });
        }

        for key_code in self.active_keys.drain() {
            events.push(InputEvent {
                event_type: InputEventType::KeyUp,
                x: current_x,
                y: current_y,
                key_code,
                modifiers: self.active_modifiers,
                scroll_dx: 0.0,
                scroll_dy: 0.0,
            });
        }

        self.key_modifiers.clear();
        self.active_modifiers = Modifiers::empty();

        events.push(InputEvent {
            event_type: InputEventType::Reset,
            x: current_x,
            y: current_y,
            key_code: 0,
            modifiers: Modifiers::empty(),
            scroll_dx: 0.0,
            scroll_dy: 0.0,
        });

        self.last_action_at = Instant::now();
        events
    }

    pub fn check_timeout(&mut self, current_x: f32, current_y: f32) -> Option<Vec<InputEvent>> {
        if self.is_timed_out() {
            Some(self.release_all(current_x, current_y))
        } else {
            None
        }
    }

    /// Non-destructive timeout probe, so detection can be separated from the release itself.
    pub fn is_timed_out(&self) -> bool {
        !self.is_empty() && self.last_action_at.elapsed() > self.hold_timeout
    }

    /// Backdates the activity clock so a failed release is retried on the next watchdog tick
    /// instead of waiting a full hold timeout again.
    pub fn mark_release_failed(&mut self) {
        self.last_action_at = Instant::now()
            .checked_sub(self.hold_timeout + Duration::from_millis(1))
            .unwrap_or_else(Instant::now);
    }
}

pub fn convert_agent_action_to_events(
    action: &AgentAction,
    tracker: &mut InputStateTracker,
    current_pos: &mut (f32, f32),
    host_width: f32,
    host_height: f32,
) -> Result<Vec<InputEvent>, AgentInputError> {
    const MAX_ACTION_EVENTS: usize = 4096;
    let event_bound = match action {
        AgentAction::Click { count, .. } => u64::from((*count).max(1)) * 2 + 1,
        AgentAction::Drag { steps, .. } => u64::from((*steps).max(1)) + 3,
        AgentAction::TypeText { text, .. } => {
            text.chars().take(MAX_ACTION_EVENTS / 2 + 1).count() as u64 * 2
        }
        _ => 0,
    };
    if event_bound > MAX_ACTION_EVENTS as u64 {
        return Err(AgentInputError::ActionTooLarge {
            max_events: MAX_ACTION_EVENTS,
        });
    }
    let mut events = Vec::new();

    match action {
        AgentAction::MouseMove { x, y, normalized } => {
            let (wire_x, wire_y) =
                normalize_agent_coordinates(*x, *y, *normalized, host_width, host_height)?;
            *current_pos = (wire_x, wire_y);
            events.push(InputEvent {
                event_type: InputEventType::MouseMove,
                x: wire_x,
                y: wire_y,
                key_code: 0,
                modifiers: tracker.active_modifiers,
                scroll_dx: 0.0,
                scroll_dy: 0.0,
            });
        }
        AgentAction::MouseDown { button } => {
            tracker.record_button_down(*button);
            events.push(InputEvent {
                event_type: button.to_down_event_type(),
                x: current_pos.0,
                y: current_pos.1,
                key_code: 0,
                modifiers: tracker.active_modifiers,
                scroll_dx: 0.0,
                scroll_dy: 0.0,
            });
        }
        AgentAction::MouseUp { button } => {
            tracker.record_button_up(*button);
            events.push(InputEvent {
                event_type: button.to_up_event_type(),
                x: current_pos.0,
                y: current_pos.1,
                key_code: 0,
                modifiers: tracker.active_modifiers,
                scroll_dx: 0.0,
                scroll_dy: 0.0,
            });
        }
        AgentAction::Click {
            x,
            y,
            button,
            count,
            normalized,
        } => {
            let (wire_x, wire_y) =
                normalize_agent_coordinates(*x, *y, *normalized, host_width, host_height)?;
            *current_pos = (wire_x, wire_y);
            events.push(InputEvent {
                event_type: InputEventType::MouseMove,
                x: wire_x,
                y: wire_y,
                key_code: 0,
                modifiers: tracker.active_modifiers,
                scroll_dx: 0.0,
                scroll_dy: 0.0,
            });
            for _ in 0..*count {
                events.push(InputEvent {
                    event_type: button.to_down_event_type(),
                    x: wire_x,
                    y: wire_y,
                    key_code: 0,
                    modifiers: tracker.active_modifiers,
                    scroll_dx: 0.0,
                    scroll_dy: 0.0,
                });
                events.push(InputEvent {
                    event_type: button.to_up_event_type(),
                    x: wire_x,
                    y: wire_y,
                    key_code: 0,
                    modifiers: tracker.active_modifiers,
                    scroll_dx: 0.0,
                    scroll_dy: 0.0,
                });
            }
        }
        AgentAction::Drag {
            start_x,
            start_y,
            end_x,
            end_y,
            button,
            steps,
            normalized,
            ..
        } => {
            let (sx, sy) = normalize_agent_coordinates(
                *start_x,
                *start_y,
                *normalized,
                host_width,
                host_height,
            )?;
            let (ex, ey) =
                normalize_agent_coordinates(*end_x, *end_y, *normalized, host_width, host_height)?;

            *current_pos = (sx, sy);
            events.push(InputEvent {
                event_type: InputEventType::MouseMove,
                x: sx,
                y: sy,
                key_code: 0,
                modifiers: tracker.active_modifiers,
                scroll_dx: 0.0,
                scroll_dy: 0.0,
            });

            tracker.record_button_down(*button);
            events.push(InputEvent {
                event_type: button.to_down_event_type(),
                x: sx,
                y: sy,
                key_code: 0,
                modifiers: tracker.active_modifiers,
                scroll_dx: 0.0,
                scroll_dy: 0.0,
            });

            let step_count = (*steps).max(1);
            let drag_event_type = button.to_drag_event_type();
            for i in 1..=step_count {
                let t = i as f32 / step_count as f32;
                let cur_x = sx + (ex - sx) * t;
                let cur_y = sy + (ey - sy) * t;
                *current_pos = (cur_x, cur_y);
                events.push(InputEvent {
                    event_type: drag_event_type,
                    x: cur_x,
                    y: cur_y,
                    key_code: 0,
                    modifiers: tracker.active_modifiers,
                    scroll_dx: 0.0,
                    scroll_dy: 0.0,
                });
            }

            tracker.record_button_up(*button);
            events.push(InputEvent {
                event_type: button.to_up_event_type(),
                x: ex,
                y: ey,
                key_code: 0,
                modifiers: tracker.active_modifiers,
                scroll_dx: 0.0,
                scroll_dy: 0.0,
            });
        }
        AgentAction::Scroll {
            dx,
            dy,
            x,
            y,
            normalized,
        } => {
            if let (Some(px), Some(py)) = (x, y) {
                let (wx, wy) =
                    normalize_agent_coordinates(*px, *py, *normalized, host_width, host_height)?;
                *current_pos = (wx, wy);
            }
            events.push(InputEvent {
                event_type: InputEventType::ScrollWheel,
                x: current_pos.0,
                y: current_pos.1,
                key_code: 0,
                modifiers: tracker.active_modifiers,
                scroll_dx: *dx,
                scroll_dy: *dy,
            });
        }
        AgentAction::KeyDown { key } => {
            let (k, mods) = parse_key_name(key)?;
            let macos_code = require_macos_keycode(k, key)?;
            tracker.record_key_down(macos_code, mods);
            events.push(InputEvent {
                event_type: InputEventType::KeyDown,
                x: current_pos.0,
                y: current_pos.1,
                key_code: macos_code,
                modifiers: tracker.active_modifiers,
                scroll_dx: 0.0,
                scroll_dy: 0.0,
            });
        }
        AgentAction::KeyUp { key } => {
            let (k, _) = parse_key_name(key)?;
            let macos_code = require_macos_keycode(k, key)?;
            tracker.record_key_up(macos_code);
            events.push(InputEvent {
                event_type: InputEventType::KeyUp,
                x: current_pos.0,
                y: current_pos.1,
                key_code: macos_code,
                modifiers: tracker.active_modifiers,
                scroll_dx: 0.0,
                scroll_dy: 0.0,
            });
        }
        AgentAction::KeyPress { key, .. } => {
            let (k, mods) = parse_key_name(key)?;
            let macos_code = require_macos_keycode(k, key)?;
            events.push(InputEvent {
                event_type: InputEventType::KeyDown,
                x: current_pos.0,
                y: current_pos.1,
                key_code: macos_code,
                modifiers: mods,
                scroll_dx: 0.0,
                scroll_dy: 0.0,
            });
            events.push(InputEvent {
                event_type: InputEventType::KeyUp,
                x: current_pos.0,
                y: current_pos.1,
                key_code: macos_code,
                modifiers: mods,
                scroll_dx: 0.0,
                scroll_dy: 0.0,
            });
        }
        AgentAction::Hotkey { keys } => {
            let combined = keys.join("+");
            let (k, mods) = parse_hotkey_string(&combined)?;
            let macos_code = require_macos_keycode(k, &combined)?;
            events.push(InputEvent {
                event_type: InputEventType::KeyDown,
                x: current_pos.0,
                y: current_pos.1,
                key_code: macos_code,
                modifiers: mods,
                scroll_dx: 0.0,
                scroll_dy: 0.0,
            });
            events.push(InputEvent {
                event_type: InputEventType::KeyUp,
                x: current_pos.0,
                y: current_pos.1,
                key_code: macos_code,
                modifiers: mods,
                scroll_dx: 0.0,
                scroll_dy: 0.0,
            });
        }
        AgentAction::TypeText { text, .. } => {
            for ch in text.chars() {
                if let Some((k, mods)) = synthesize_ascii_char(ch) {
                    if let Some(macos_code) = crate::input::InputKeyMap::to_macos(k) {
                        events.push(InputEvent {
                            event_type: InputEventType::KeyDown,
                            x: current_pos.0,
                            y: current_pos.1,
                            key_code: macos_code,
                            modifiers: mods,
                            scroll_dx: 0.0,
                            scroll_dy: 0.0,
                        });
                        events.push(InputEvent {
                            event_type: InputEventType::KeyUp,
                            x: current_pos.0,
                            y: current_pos.1,
                            key_code: macos_code,
                            modifiers: mods,
                            scroll_dx: 0.0,
                            scroll_dy: 0.0,
                        });
                    }
                } else {
                    // Non-ASCII character: convert to UTF-16 code units and emit UnicodeChar events
                    let mut utf16_buf = [0u16; 2];
                    let len = ch.encode_utf16(&mut utf16_buf).len();
                    for code_unit in &utf16_buf[..len] {
                        events.push(InputEvent::unicode_char(
                            *code_unit,
                            current_pos.0,
                            current_pos.1,
                        ));
                    }
                }
            }
        }
        AgentAction::ReleaseAll => {
            events.extend(tracker.release_all(current_pos.0, current_pos.1));
        }
    }

    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_release_is_retried_on_the_next_timeout_check() {
        let mut tracker = InputStateTracker::new(Duration::from_secs(60));
        tracker.record_key_down(0x3b, Modifiers::CONTROL);
        assert!(!tracker.is_timed_out(), "fresh hold is not timed out");
        // A failed release keeps the held state and demands an immediate retry.
        tracker.mark_release_failed();
        assert!(
            tracker.is_timed_out(),
            "a failed release must be retried without waiting another hold timeout"
        );
        assert!(!tracker.is_empty());
    }

    #[test]
    fn function_keys_map_to_distinct_host_keycodes() {
        let a_keycode = crate::input::InputKeyMap::to_macos(InputKey::WindowsVirtualKey(0x41))
            .expect("letter A must map");
        assert_eq!(a_keycode, 0x00);

        let (f5_key, _) = parse_key_name("f5").unwrap();
        let f5_code = require_macos_keycode(f5_key, "f5").expect("f5 must map");
        assert_ne!(
            f5_code, a_keycode,
            "F5 must not collapse onto the macOS 'A' keycode"
        );
        assert_eq!(f5_code, 0x60);

        let mut seen = HashSet::new();
        for (name, expected) in [
            ("f1", 0x7au16),
            ("f2", 0x78),
            ("f3", 0x63),
            ("f4", 0x76),
            ("f5", 0x60),
            ("f6", 0x61),
            ("f7", 0x62),
            ("f8", 0x64),
            ("f9", 0x65),
            ("f10", 0x6d),
            ("f11", 0x67),
            ("f12", 0x6f),
        ] {
            let (key, _) = parse_key_name(name).unwrap();
            let code = require_macos_keycode(key, name).expect("function key must map");
            assert_eq!(code, expected, "{name} mapped to unexpected keycode");
            assert!(
                seen.insert(code),
                "{name} keycode collides with another key"
            );
        }
    }

    #[test]
    fn key_up_clears_only_that_keys_modifiers() {
        let mut tracker = InputStateTracker::default();
        let mut position = (0.5, 0.5);

        let down = convert_agent_action_to_events(
            &AgentAction::KeyDown { key: "ctrl".into() },
            &mut tracker,
            &mut position,
            800.0,
            600.0,
        )
        .unwrap();
        assert!(down[0].modifiers.contains(Modifiers::CONTROL));

        convert_agent_action_to_events(
            &AgentAction::KeyUp { key: "ctrl".into() },
            &mut tracker,
            &mut position,
            800.0,
            600.0,
        )
        .unwrap();
        assert_eq!(tracker.active_modifiers, Modifiers::empty());

        let c_down = convert_agent_action_to_events(
            &AgentAction::KeyDown { key: "c".into() },
            &mut tracker,
            &mut position,
            800.0,
            600.0,
        )
        .unwrap();
        assert!(
            !c_down[0].modifiers.contains(Modifiers::CONTROL),
            "released ctrl must not leak into later key events"
        );
    }

    #[test]
    fn key_up_retains_modifiers_owned_by_other_held_keys() {
        let mut tracker = InputStateTracker::default();
        tracker.record_key_down(0x3b, Modifiers::CONTROL);
        tracker.record_key_down(0x38, Modifiers::SHIFT);
        tracker.record_key_up(0x3b);
        assert_eq!(tracker.active_modifiers, Modifiers::SHIFT);
        tracker.record_key_up(0x38);
        assert_eq!(tracker.active_modifiers, Modifiers::empty());
        assert!(tracker.is_empty());
    }

    #[test]
    fn parses_standard_key_names_and_modifiers() {
        let (key, mods) = parse_key_name("Enter").unwrap();
        assert_eq!(key, InputKey::WindowsVirtualKey(0x0D));
        assert_eq!(mods, Modifiers::empty());

        let (key, mods) = parse_key_name("Ctrl").unwrap();
        assert_eq!(key, InputKey::WindowsVirtualKey(0x11));
        assert_eq!(mods, Modifiers::CONTROL);

        let (key, mods) = parse_key_name("a").unwrap();
        assert_eq!(key, InputKey::WindowsVirtualKey(0x41));
        assert_eq!(mods, Modifiers::empty());

        let (key, mods) = parse_key_name("A").unwrap();
        assert_eq!(key, InputKey::WindowsVirtualKey(0x41));
        assert_eq!(mods, Modifiers::SHIFT);
    }

    #[test]
    fn parses_hotkey_combinations() {
        let (key, mods) = parse_hotkey_string("Ctrl+Shift+T").unwrap();
        assert_eq!(key, InputKey::WindowsVirtualKey(0x54));
        assert!(mods.contains(Modifiers::CONTROL));
        assert!(mods.contains(Modifiers::SHIFT));

        let (key, mods) = parse_hotkey_string("Super+Return").unwrap();
        assert_eq!(key, InputKey::WindowsVirtualKey(0x0D));
        assert!(mods.contains(Modifiers::COMMAND));

        assert!(matches!(
            parse_hotkey_string(""),
            Err(AgentInputError::EmptyHotkey)
        ));
        assert!(matches!(
            parse_hotkey_string("InvalidKey123"),
            Err(AgentInputError::UnknownKey(_))
        ));
    }

    #[test]
    fn normalizes_coordinates_with_y_flip() {
        let (x, y) = normalize_agent_coordinates(0.5, 0.25, true, 1920.0, 1080.0).unwrap();
        assert_eq!((x, y), (0.5, 0.75));

        let (x, y) = normalize_agent_coordinates(960.0, 270.0, false, 1920.0, 1080.0).unwrap();
        assert_eq!((x, y), (0.5, 0.75));

        assert!(normalize_agent_coordinates(f32::NAN, 0.0, true, 1920.0, 1080.0).is_err());
    }

    #[test]
    fn synthesizes_ascii_characters() {
        let (key, mods) = synthesize_ascii_char('H').unwrap();
        assert_eq!(key, InputKey::WindowsVirtualKey(0x48));
        assert_eq!(mods, Modifiers::SHIFT);

        let (key, mods) = synthesize_ascii_char('e').unwrap();
        assert_eq!(key, InputKey::WindowsVirtualKey(0x45));
        assert_eq!(mods, Modifiers::empty());

        let (key, mods) = synthesize_ascii_char('@').unwrap();
        assert_eq!(key, InputKey::WindowsVirtualKey(0x32));
        assert_eq!(mods, Modifiers::SHIFT);
    }

    #[test]
    fn tracker_records_and_releases_all() {
        let mut tracker = InputStateTracker::new(Duration::from_millis(50));
        assert!(tracker.is_empty());

        tracker.record_button_down(MouseButton::Left);
        tracker.record_key_down(0x41, Modifiers::CONTROL);
        assert!(!tracker.is_empty());

        let release_events = tracker.release_all(0.5, 0.5);
        assert!(tracker.is_empty());
        assert_eq!(release_events.len(), 3);
        assert!(release_events
            .iter()
            .any(|e| e.event_type == InputEventType::LeftMouseUp));
        assert!(release_events
            .iter()
            .any(|e| e.event_type == InputEventType::KeyUp));
        assert!(release_events
            .iter()
            .any(|e| e.event_type == InputEventType::Reset));
    }

    #[test]
    fn converts_actions_to_wire_input_events() {
        let mut tracker = InputStateTracker::default();
        let mut current_pos = (0.0, 0.0);

        let click = AgentAction::Click {
            x: 100.0,
            y: 200.0,
            button: MouseButton::Left,
            count: 1,
            normalized: false,
        };
        let events =
            convert_agent_action_to_events(&click, &mut tracker, &mut current_pos, 1000.0, 1000.0)
                .unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].event_type, InputEventType::MouseMove);
        assert_eq!(events[1].event_type, InputEventType::LeftMouseDown);
        assert_eq!(events[2].event_type, InputEventType::LeftMouseUp);

        let hotkey = AgentAction::Hotkey {
            keys: vec!["Ctrl".into(), "c".into()],
        };
        let events =
            convert_agent_action_to_events(&hotkey, &mut tracker, &mut current_pos, 1000.0, 1000.0)
                .unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, InputEventType::KeyDown);
        assert_eq!(events[1].event_type, InputEventType::KeyUp);
        assert!(events[0].modifiers.contains(Modifiers::CONTROL));

        let type_text = AgentAction::TypeText {
            text: "ls".into(),
            delay_ms: 0,
            paste_mode: false,
        };
        let events = convert_agent_action_to_events(
            &type_text,
            &mut tracker,
            &mut current_pos,
            1000.0,
            1000.0,
        )
        .unwrap();
        assert_eq!(events.len(), 4);
    }

    fn assert_action_budget_rejected(action: AgentAction) {
        let mut tracker = InputStateTracker::default();
        tracker.record_key_down(56, Modifiers::SHIFT);
        let before_keys = tracker.active_keys.clone();
        let before_buttons = tracker.active_buttons.clone();
        let before_modifiers = tracker.active_modifiers;
        let before_action_at = tracker.last_action_at;
        let mut position = (0.25, 0.75);
        let result =
            convert_agent_action_to_events(&action, &mut tracker, &mut position, 800.0, 600.0);
        eprintln!(
            "over-budget action returned event count: {:?}",
            result.as_ref().map(Vec::len)
        );
        assert!(
            result.is_err(),
            "oversized action must fail before expansion"
        );
        assert_eq!(position, (0.25, 0.75));
        assert_eq!(tracker.active_keys, before_keys);
        assert_eq!(tracker.active_buttons, before_buttons);
        assert_eq!(tracker.active_modifiers, before_modifiers);
        assert_eq!(tracker.last_action_at, before_action_at);
    }

    #[test]
    fn action_budget_rejects_click_expansion() {
        assert_action_budget_rejected(AgentAction::Click {
            x: 200.0,
            y: 150.0,
            button: MouseButton::Left,
            count: 2048,
            normalized: false,
        });
    }

    #[test]
    fn action_budget_rejects_drag_expansion() {
        assert_action_budget_rejected(AgentAction::Drag {
            start_x: 100.0,
            start_y: 100.0,
            end_x: 700.0,
            end_y: 500.0,
            button: MouseButton::Left,
            steps: 4094,
            duration_ms: 400,
            normalized: false,
        });
    }

    #[test]
    fn action_budget_rejects_text_expansion() {
        assert_action_budget_rejected(AgentAction::TypeText {
            text: "a".repeat(2049),
            delay_ms: 0,
            paste_mode: false,
        });
    }

    #[test]
    fn action_budget_accepts_maximum_text_and_drag() {
        let mut tracker = InputStateTracker::default();
        let mut position = (0.0, 0.0);
        for action in [
            AgentAction::TypeText {
                text: "A".repeat(2048),
                delay_ms: 0,
                paste_mode: false,
            },
            AgentAction::Drag {
                start_x: 0.0,
                start_y: 0.0,
                end_x: 799.0,
                end_y: 599.0,
                button: MouseButton::Left,
                steps: 4093,
                duration_ms: 400,
                normalized: false,
            },
        ] {
            assert_eq!(
                convert_agent_action_to_events(&action, &mut tracker, &mut position, 800.0, 600.0,)
                    .unwrap()
                    .len(),
                4096
            );
        }
    }

    #[test]
    fn nv12_to_rgb_rejects_odd_and_underflowing_dimensions() {
        // Advertised length passes the old size check but the first pixel read
        // its V component past the end of the buffer.
        assert_eq!(nv12_to_rgb(2, 1, &[16, 16, 128]), None);
        // Odd width has no defined chroma stride in this packed contract.
        assert_eq!(nv12_to_rgb(3, 2, &[16; 9]), None);
        // Real even dimensions still convert: BT.601 with neutral chroma maps
        // Y=16 to RGB 16 on every channel.
        let rgb = nv12_to_rgb(2, 2, &[16, 16, 16, 16, 128, 128]).unwrap();
        assert_eq!(rgb, vec![16; 12]);
    }

    #[test]
    fn encodes_nv12_screenshot_to_valid_png_and_jpeg() {
        let width = 64u32;
        let height = 64u32;
        let nv12_len = (width * height * 3 / 2) as usize;
        let nv12_buf = vec![128u8; nv12_len];

        let png_base64 =
            encode_nv12_screenshot(width, height, &nv12_buf, ScreenshotFormat::Png).unwrap();
        let png_bytes = base64::prelude::BASE64_STANDARD
            .decode(&png_base64)
            .unwrap();
        assert_eq!(&png_bytes[0..4], &[0x89, 0x50, 0x4E, 0x47]);

        let jpeg_base64 =
            encode_nv12_screenshot(width, height, &nv12_buf, ScreenshotFormat::Jpeg).unwrap();
        let jpeg_bytes = base64::prelude::BASE64_STANDARD
            .decode(&jpeg_base64)
            .unwrap();
        assert_eq!(&jpeg_bytes[0..2], &[0xFF, 0xD8]);
    }

    #[test]
    fn right_mouse_drag_emits_right_dragged_event() {
        let mut tracker = InputStateTracker::default();
        let mut current_pos = (0.0, 0.0);

        let drag = AgentAction::Drag {
            start_x: 100.0,
            start_y: 200.0,
            end_x: 300.0,
            end_y: 400.0,
            button: MouseButton::Right,
            steps: 5,
            duration_ms: 200,
            normalized: false,
        };
        let events =
            convert_agent_action_to_events(&drag, &mut tracker, &mut current_pos, 1000.0, 1000.0)
                .unwrap();
        assert!(events.len() >= 7); // MouseMove, MouseDown, 5 drags, MouseUp
        let has_right_dragged = events
            .iter()
            .any(|e| e.event_type == InputEventType::RightMouseDragged);
        assert!(
            has_right_dragged,
            "drag with right button must emit RightMouseDragged events"
        );
        let has_right_down = events
            .iter()
            .any(|e| e.event_type == InputEventType::RightMouseDown);
        assert!(has_right_down);
        let has_right_up = events
            .iter()
            .any(|e| e.event_type == InputEventType::RightMouseUp);
        assert!(has_right_up);
    }

    #[test]
    fn types_multilingual_text_with_unicode_char_events() {
        let mut tracker = InputStateTracker::default();
        let mut current_pos = (0.0, 0.0);

        let type_text = AgentAction::TypeText {
            text: "Hello 세계 🚀".to_string(),
            delay_ms: 0,
            paste_mode: false,
        };
        let events = convert_agent_action_to_events(
            &type_text,
            &mut tracker,
            &mut current_pos,
            1000.0,
            1000.0,
        )
        .unwrap();

        // "Hello" = 10 events (5 chars × 2 for KeyDown/KeyUp)
        // " " (space) = 2 events
        // "세" (Korean) = 1 or more UnicodeChar events
        // "계" (Korean) = 1 or more UnicodeChar events
        // " " (space) = 2 events
        // "🚀" (emoji, multi-code-unit) = 2 UnicodeChar events (surrogate pair)
        let has_unicode_events = events
            .iter()
            .any(|e| e.event_type == InputEventType::UnicodeChar);
        assert!(
            has_unicode_events,
            "TypeText with non-ASCII characters must generate UnicodeChar events"
        );
        // Verify no silent drops: events should include both ASCII and non-ASCII
        let has_ascii_events = events
            .iter()
            .any(|e| e.event_type == InputEventType::KeyDown);
        assert!(
            has_ascii_events,
            "TypeText must synthesize ASCII characters with KeyDown/KeyUp"
        );
    }

    #[test]
    fn left_mouse_drag_emits_left_dragged_event() {
        let mut tracker = InputStateTracker::default();
        let mut current_pos = (0.0, 0.0);

        let drag = AgentAction::Drag {
            start_x: 100.0,
            start_y: 200.0,
            end_x: 300.0,
            end_y: 400.0,
            button: MouseButton::Left,
            steps: 5,
            duration_ms: 200,
            normalized: false,
        };
        let events =
            convert_agent_action_to_events(&drag, &mut tracker, &mut current_pos, 1000.0, 1000.0)
                .unwrap();
        assert!(events.len() >= 7); // MouseMove, MouseDown, 5 drags, MouseUp
        let has_left_dragged = events
            .iter()
            .any(|e| e.event_type == InputEventType::LeftMouseDragged);
        assert!(
            has_left_dragged,
            "drag with left button must emit LeftMouseDragged events"
        );
        let has_left_down = events
            .iter()
            .any(|e| e.event_type == InputEventType::LeftMouseDown);
        assert!(has_left_down);
        let has_left_up = events
            .iter()
            .any(|e| e.event_type == InputEventType::LeftMouseUp);
        assert!(has_left_up);
    }

    #[test]
    fn serializes_screen_info_with_logical_dimensions_and_monitors() {
        let monitor1 = MonitorInfo {
            id: 1,
            name: "HDMI-1".to_string(),
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            scale: 1.0,
            is_primary: true,
        };
        let monitor2 = MonitorInfo {
            id: 2,
            name: "DP-1".to_string(),
            x: 1920,
            y: 0,
            width: 2560,
            height: 1440,
            scale: 2.0,
            is_primary: false,
        };
        let screen = ScreenInfo {
            width: 3840,
            height: 1440,
            scale: 1.5,
            logical_width: Some(2560),
            logical_height: Some(960),
            monitors: vec![monitor1, monitor2],
            connected_host: "test-host".to_string(),
        };
        let json = serde_json::to_value(&screen).unwrap();
        assert_eq!(json["width"], 3840);
        assert_eq!(json["height"], 1440);
        assert_eq!(json["scale"], 1.5);
        assert_eq!(json["logical_width"], 2560);
        assert_eq!(json["logical_height"], 960);
        assert_eq!(json["monitors"].as_array().unwrap().len(), 2);
        assert_eq!(json["monitors"][0]["id"], 1);
        assert_eq!(json["monitors"][0]["name"], "HDMI-1");
        assert_eq!(json["monitors"][0]["is_primary"], true);
        assert_eq!(json["monitors"][1]["scale"], 2.0);
    }

    #[test]
    fn deserializes_screen_info_with_backward_compatibility() {
        // Old JSON without logical_width, logical_height, and monitors
        let old_json = r#"{
            "width": 1920,
            "height": 1080,
            "scale": 1.0,
            "connected_host": "legacy-host"
        }"#;
        let screen: ScreenInfo = serde_json::from_str(old_json).unwrap();
        assert_eq!(screen.width, 1920);
        assert_eq!(screen.height, 1080);
        assert_eq!(screen.scale, 1.0);
        assert_eq!(screen.logical_width, None);
        assert_eq!(screen.logical_height, None);
        assert_eq!(screen.monitors, Vec::new());
        assert_eq!(screen.connected_host, "legacy-host");
    }

    #[test]
    fn deserializes_screen_info_with_new_fields() {
        let new_json = r#"{
            "width": 3840,
            "height": 1440,
            "scale": 2.0,
            "logical_width": 1920,
            "logical_height": 720,
            "monitors": [
                {
                    "id": 1,
                    "name": "HDMI",
                    "x": 0,
                    "y": 0,
                    "width": 1920,
                    "height": 1080,
                    "scale": 1.0,
                    "is_primary": true
                }
            ],
            "connected_host": "new-host"
        }"#;
        let screen: ScreenInfo = serde_json::from_str(new_json).unwrap();
        assert_eq!(screen.width, 3840);
        assert_eq!(screen.logical_width, Some(1920));
        assert_eq!(screen.logical_height, Some(720));
        assert_eq!(screen.monitors.len(), 1);
        assert_eq!(screen.monitors[0].id, 1);
        assert_eq!(screen.monitors[0].name, "HDMI");
        assert!(screen.monitors[0].is_primary);
    }

    #[test]
    fn normalizes_coordinates_from_physical_pixels() {
        // Physical pixel 960, 270 on 1920x1080 display = normalized 0.5, 0.75
        let (x, y) = normalize_agent_coordinates(960.0, 270.0, false, 1920.0, 1080.0).unwrap();
        assert_eq!((x, y), (0.5, 0.75));

        // Left edge: physical 0, 0 = normalized 0.0, 1.0
        let (x, y) = normalize_agent_coordinates(0.0, 0.0, false, 1920.0, 1080.0).unwrap();
        assert_eq!((x, y), (0.0, 1.0));

        // Right edge: physical 1920, 1080 = normalized 1.0, 0.0
        let (x, y) = normalize_agent_coordinates(1920.0, 1080.0, false, 1920.0, 1080.0).unwrap();
        assert_eq!((x, y), (1.0, 0.0));

        // Out of bounds: physical 2400, 1500 clamped to 1.0, 0.0
        let (x, y) = normalize_agent_coordinates(2400.0, 1500.0, false, 1920.0, 1080.0).unwrap();
        assert!((x - 1.0).abs() < 1e-6);
        assert!((y - 0.0).abs() < 1e-6);

        // Negative physical: clamped to 0.0, 1.0
        let (x, y) = normalize_agent_coordinates(-100.0, -100.0, false, 1920.0, 1080.0).unwrap();
        assert_eq!((x, y), (0.0, 1.0));
    }

    #[test]
    fn normalizes_already_normalized_coordinates() {
        // When normalized=true, expects 0.0-1.0 range, applies clamping and Y flip
        let (x, y) = normalize_agent_coordinates(0.5, 0.25, true, 1920.0, 1080.0).unwrap();
        assert_eq!((x, y), (0.5, 0.75));

        let (x, y) = normalize_agent_coordinates(0.0, 0.0, true, 1920.0, 1080.0).unwrap();
        assert_eq!((x, y), (0.0, 1.0));

        let (x, y) = normalize_agent_coordinates(1.0, 1.0, true, 1920.0, 1080.0).unwrap();
        assert_eq!((x, y), (1.0, 0.0));
    }

    #[test]
    fn rejects_non_finite_coordinates() {
        assert!(normalize_agent_coordinates(f32::NAN, 0.0, true, 1920.0, 1080.0).is_err());
        assert!(normalize_agent_coordinates(0.0, f32::NAN, true, 1920.0, 1080.0).is_err());
        assert!(normalize_agent_coordinates(f32::INFINITY, 0.0, false, 1920.0, 1080.0).is_err());
        assert!(
            normalize_agent_coordinates(0.0, f32::NEG_INFINITY, false, 1920.0, 1080.0).is_err()
        );
    }

    #[test]
    fn monitor_info_equality() {
        let m1 = MonitorInfo {
            id: 1,
            name: "HDMI".to_string(),
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            scale: 1.0,
            is_primary: true,
        };
        let m2 = MonitorInfo {
            id: 1,
            name: "HDMI".to_string(),
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            scale: 1.0,
            is_primary: true,
        };
        let m3 = MonitorInfo {
            id: 2,
            name: "DP".to_string(),
            x: 1920,
            y: 0,
            width: 2560,
            height: 1440,
            scale: 2.0,
            is_primary: false,
        };
        assert_eq!(m1, m2);
        assert_ne!(m1, m3);
    }
}
