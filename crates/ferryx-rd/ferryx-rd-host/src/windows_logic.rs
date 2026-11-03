//! Windows host pure logic kept target-independent for unit tests.

use ferryx_rd_proto::Modifiers;

#[path = "windows_freshness.rs"]
pub mod freshness;

/// Wire audio format produced by every host: interleaved f32 LE stereo at 48 kHz,
/// matching the client's cpal output contract in `ferryx-rd-render::audio`.
pub const WIRE_AUDIO_CHANNELS: usize = 2;
pub const WIRE_AUDIO_SAMPLE_RATE: u32 = 48_000;

/// Sample layout of a WASAPI loopback mix format relevant to wire conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceAudioFormat {
    pub channels: usize,
    pub sample_rate: u32,
    /// `false` for 16-bit PCM, `true` for IEEE f32 (shared-mode mix format norm).
    pub is_float: bool,
}

impl Default for SourceAudioFormat {
    fn default() -> Self {
        Self {
            channels: WIRE_AUDIO_CHANNELS,
            sample_rate: WIRE_AUDIO_SAMPLE_RATE,
            is_float: true,
        }
    }
}

/// Converts one WASAPI loopback packet to the wire format: interleaved f32 LE
/// stereo at 48 kHz. Downmixes multi-channel by keeping the first two channels,
/// duplicates mono, converts 16-bit PCM, and linearly resamples other rates.
pub fn convert_to_wire_audio(pcm: &[u8], source: &SourceAudioFormat) -> Vec<u8> {
    if source.channels == 0 {
        return Vec::new();
    }
    let bytes_per_sample = if source.is_float { 4 } else { 2 };
    let frame_bytes = bytes_per_sample * source.channels;
    let frame_count = pcm.len() / frame_bytes.max(1);
    if frame_count == 0 {
        return Vec::new();
    }

    let mut samples = Vec::with_capacity(frame_count * WIRE_AUDIO_CHANNELS);
    for frame in 0..frame_count {
        let read_sample = |channel: usize| -> f32 {
            let offset = frame * frame_bytes + channel * bytes_per_sample;
            if source.is_float {
                f32::from_le_bytes(pcm[offset..offset + 4].try_into().expect("4 bytes"))
            } else {
                i16::from_le_bytes(pcm[offset..offset + 2].try_into().expect("2 bytes")) as f32
                    / 32_768.0
            }
        };
        if source.channels == 1 {
            let mono = read_sample(0);
            samples.push(mono);
            samples.push(mono);
        } else {
            samples.push(read_sample(0));
            samples.push(read_sample(1));
        }
    }
    // `samples` is now frame-sequential stereo at the source rate.
    let output = if source.sample_rate == WIRE_AUDIO_SAMPLE_RATE {
        samples
    } else {
        let source_frames = frame_count;
        let output_frames = (source_frames as u64 * u64::from(WIRE_AUDIO_SAMPLE_RATE)
            / u64::from(source.sample_rate)) as usize;
        let step = f64::from(source.sample_rate) / f64::from(WIRE_AUDIO_SAMPLE_RATE);
        let mut resampled = Vec::with_capacity(output_frames * WIRE_AUDIO_CHANNELS);
        for frame in 0..output_frames {
            let position = frame as f64 * step;
            let index = position.floor() as usize;
            let next = (index + 1).min(source_frames - 1);
            let frac = (position - index as f64) as f32;
            for channel in 0..WIRE_AUDIO_CHANNELS {
                let left = samples[index * WIRE_AUDIO_CHANNELS + channel];
                let right = samples[next * WIRE_AUDIO_CHANNELS + channel];
                resampled.push(left + (right - left) * frac);
            }
        }
        resampled
    };
    let mut bytes = Vec::with_capacity(output.len() * 4);
    for sample in output {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

const ABSOLUTE_AXIS_MAX: f64 = 65_535.0;

/// Windows virtual desktop bounds in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VirtualDesktop {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Target display bounds within the virtual desktop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetDisplay {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl TargetDisplay {
    pub fn from_virtual_desktop(desktop: VirtualDesktop) -> Self {
        Self {
            x: desktop.x,
            y: desktop.y,
            width: desktop.width,
            height: desktop.height,
        }
    }
}

/// Maps normalized protocol coordinates into the Windows absolute-input range.
pub fn normalize_absolute_pointer(
    normalized_x: f32,
    normalized_y: f32,
    target: TargetDisplay,
    desktop: VirtualDesktop,
) -> (i32, i32) {
    if target.width == 0 || target.height == 0 || desktop.width <= 1 || desktop.height <= 1 {
        return (0, 0);
    }
    let local_x = finite_unit(normalized_x) * target.width.saturating_sub(1) as f64;
    // Protocol convention inverts Y (1.0 - y) for legacy macOS compatibility.
    // Invert it back so (0,0) is top-left on Windows.
    let local_y = (1.0 - finite_unit(normalized_y)) * target.height.saturating_sub(1) as f64;
    let desktop_x = (target.x as f64 + local_x - desktop.x as f64)
        .clamp(0.0, desktop.width.saturating_sub(1) as f64);
    let desktop_y = (target.y as f64 + local_y - desktop.y as f64)
        .clamp(0.0, desktop.height.saturating_sub(1) as f64);
    (
        (desktop_x * ABSOLUTE_AXIS_MAX / desktop.width.saturating_sub(1) as f64).round() as i32,
        (desktop_y * ABSOLUTE_AXIS_MAX / desktop.height.saturating_sub(1) as f64).round() as i32,
    )
}

fn finite_unit(value: f32) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0) as f64
    } else {
        0.0
    }
}

/// Convert a Windows VK to the protocol's macOS physical-key code.
pub fn vk_to_macos_keycode(vk: u16) -> Option<u16> {
    KEY_MAP
        .iter()
        .find_map(|&(macos, windows)| (windows == vk).then_some(macos))
}

/// Convert a protocol/macOS physical-key code to a Windows VK.
pub fn macos_keycode_to_vk(key_code: u16) -> Option<u16> {
    KEY_MAP
        .iter()
        .find_map(|&(macos, windows)| (macos == key_code).then_some(windows))
}

pub fn vk_is_extended(vk: u16) -> bool {
    matches!(vk, 0x21..=0x28 | 0x2D | 0x2E | 0x5B | 0x5C | 0x6F | 0x90 | 0xA3 | 0xA5 | 0xA6..=0xB3)
}

pub fn macos_keycode_to_windows_key(key_code: u16) -> Option<(u16, bool)> {
    macos_keycode_to_vk(key_code).map(|vk| (vk, key_code == 76 || vk_is_extended(vk)))
}

/// Maps v3 modifier bits to the left-side Win32 VKs used by the injector.
pub fn modifier_vks(modifiers: Modifiers) -> Vec<u16> {
    [
        (Modifiers::SHIFT, 0xA0),
        (Modifiers::CONTROL, 0xA2),
        (Modifiers::OPTION, 0xA4),
        (Modifiers::COMMAND, 0x5B),
        (Modifiers::CAPS_LOCK, 0x14),
    ]
    .into_iter()
    .filter_map(|(modifier, vk)| modifiers.contains(modifier).then_some(vk))
    .collect()
}

/// Returns Win32 MOUSEEVENTF flags for mouse button down/up events.
pub fn mouse_button_flags(event_type: ferryx_rd_proto::InputEventType) -> u32 {
    const MOUSEEVENTF_LEFTDOWN: u32 = 0x0002;
    const MOUSEEVENTF_LEFTUP: u32 = 0x0004;
    const MOUSEEVENTF_RIGHTDOWN: u32 = 0x0008;
    const MOUSEEVENTF_RIGHTUP: u32 = 0x0010;
    const MOUSEEVENTF_MIDDLEDOWN: u32 = 0x0020;
    const MOUSEEVENTF_MIDDLEUP: u32 = 0x0040;

    match event_type {
        ferryx_rd_proto::InputEventType::LeftMouseDown => MOUSEEVENTF_LEFTDOWN,
        ferryx_rd_proto::InputEventType::LeftMouseUp => MOUSEEVENTF_LEFTUP,
        ferryx_rd_proto::InputEventType::RightMouseDown => MOUSEEVENTF_RIGHTDOWN,
        ferryx_rd_proto::InputEventType::RightMouseUp => MOUSEEVENTF_RIGHTUP,
        ferryx_rd_proto::InputEventType::MiddleMouseDown => MOUSEEVENTF_MIDDLEDOWN,
        ferryx_rd_proto::InputEventType::MiddleMouseUp => MOUSEEVENTF_MIDDLEUP,
        _ => 0,
    }
}

/// Browser wheel pixels that make up one Win32 wheel notch (`WHEEL_DELTA`).
const PIXELS_PER_WHEEL_NOTCH: f32 = 100.0;
const WHEEL_DELTA: f32 = 120.0;

/// Converts wire scroll deltas (browser `WheelEvent` pixels: +dy scrolls down,
/// +dx scrolls right) to `SendInput` wheel data `(vertical, horizontal)`.
/// Win32 `MOUSEEVENTF_WHEEL` is positive when the wheel rotates forward (scroll
/// up), so the vertical axis is negated; `MOUSEEVENTF_HWHEEL` is positive to the
/// right and keeps its sign. The client has already applied its own scroll
/// preference, but Windows applies the host user's "reverse scrolling" setting
/// to injected wheel input as well, so `host_reversed` pre-flips both axes to
/// cancel it. A non-zero delta always yields at least one unit so small trackpad
/// deltas are not dropped, and units are clamped to the 16-bit range the wheel
/// message carries so large deltas cannot wrap into the opposite direction.
pub fn wheel_data_from_scroll(scroll_dx: f32, scroll_dy: f32, host_reversed: bool) -> (i32, i32) {
    let sign = if host_reversed { -1.0 } else { 1.0 };
    (
        pixels_to_wheel_units(-scroll_dy * sign),
        pixels_to_wheel_units(scroll_dx * sign),
    )
}

fn pixels_to_wheel_units(pixels: f32) -> i32 {
    if !pixels.is_finite() || pixels == 0.0 {
        return 0;
    }
    let units = (pixels * WHEEL_DELTA / PIXELS_PER_WHEEL_NOTCH)
        .round()
        .clamp(-(i16::MAX as f32), i16::MAX as f32) as i32;
    if units == 0 {
        pixels.signum() as i32
    } else {
        units
    }
}

/// Stable FNV-1a clipboard hash used for content-level dedup.
pub fn clipboard_content_hash(text: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ClipboardEchoSuppressor {
    last_sequence: Option<u32>,
    last_written_sequence: Option<u32>,
    last_hash: Option<u64>,
}

impl ClipboardEchoSuppressor {
    pub fn start(&mut self, sequence: u32) {
        self.last_sequence = Some(sequence);
        self.last_written_sequence = None;
        self.last_hash = None;
    }

    pub fn record_remote_write(&mut self, sequence: u32, text: &str) {
        self.last_sequence = Some(sequence);
        self.last_written_sequence = Some(sequence);
        self.last_hash = Some(clipboard_content_hash(text));
    }

    pub fn observe_sequence(&mut self, sequence: u32) -> bool {
        if self.last_sequence == Some(sequence) {
            return false;
        }
        self.last_sequence = Some(sequence);
        self.last_written_sequence != Some(sequence)
    }

    pub fn observe_text(&mut self, text: &str) -> bool {
        let hash = clipboard_content_hash(text);
        if self.last_hash == Some(hash) {
            return false;
        }
        self.last_hash = Some(hash);
        true
    }
}

/// Apple ANSI physical key codes mirrored to Win32 virtual keys.
const KEY_MAP: &[(u16, u16)] = &[
    (0, 0x41),
    (1, 0x53),
    (2, 0x44),
    (3, 0x46),
    (4, 0x48),
    (5, 0x47),
    (6, 0x5A),
    (7, 0x58),
    (8, 0x43),
    (9, 0x56),
    (11, 0x42),
    (12, 0x51),
    (13, 0x57),
    (14, 0x45),
    (15, 0x52),
    (16, 0x59),
    (17, 0x54),
    (18, 0x31),
    (19, 0x32),
    (20, 0x33),
    (21, 0x34),
    (22, 0x36),
    (23, 0x35),
    (24, 0xBB),
    (25, 0x39),
    (26, 0x37),
    (27, 0xBD),
    (28, 0x38),
    (29, 0x30),
    (30, 0xDD),
    (31, 0x4F),
    (32, 0x55),
    (33, 0xDB),
    (34, 0x49),
    (35, 0x50),
    (36, 0x0D),
    (37, 0x4C),
    (38, 0x4A),
    (39, 0xDE),
    (40, 0x4B),
    (41, 0xBA),
    (42, 0xDC),
    (43, 0xBC),
    (44, 0xBF),
    (45, 0x4E),
    (46, 0x4D),
    (47, 0xBE),
    (48, 0x09),
    (49, 0x20),
    (50, 0xC0),
    (51, 0x08),
    (53, 0x1B),
    (54, 0x5C),
    (55, 0x5B),
    (56, 0xA0),
    (57, 0x14),
    (58, 0xA4),
    (59, 0xA2),
    (60, 0xA1),
    (61, 0xA5),
    (62, 0xA3),
    (64, 0x80),
    (65, 0x6E),
    (67, 0x6A),
    (69, 0x6B),
    (71, 0x90),
    (72, 0xAF),
    (73, 0xAE),
    (74, 0xAD),
    (75, 0x6F),
    (76, 0x0D),
    (78, 0x6D),
    (79, 0x81),
    (80, 0x82),
    (81, 0x92),
    (82, 0x60),
    (83, 0x61),
    (84, 0x62),
    (85, 0x63),
    (86, 0x64),
    (87, 0x65),
    (88, 0x66),
    (89, 0x67),
    (90, 0x83),
    (91, 0x68),
    (92, 0x69),
    (96, 0x74),
    (97, 0x75),
    (98, 0x76),
    (99, 0x72),
    (100, 0x77),
    (101, 0x78),
    (103, 0x7A),
    (105, 0x7C),
    (106, 0x7F),
    (107, 0x7D),
    (109, 0x79),
    (111, 0x7B),
    (113, 0x7E),
    (114, 0x2D),
    (115, 0x24),
    (116, 0x21),
    (117, 0x2E),
    (118, 0x73),
    (119, 0x23),
    (120, 0x71),
    (121, 0x22),
    (122, 0x70),
    (123, 0x25),
    (124, 0x27),
    (125, 0x28),
    (126, 0x26),
    (ferryx_rd_proto::extended_keys::MEDIA_PLAY_PAUSE, 0xB3),
    (ferryx_rd_proto::extended_keys::MEDIA_NEXT, 0xB0),
    (ferryx_rd_proto::extended_keys::MEDIA_PREVIOUS, 0xB1),
    (ferryx_rd_proto::extended_keys::MEDIA_STOP, 0xB2),
    (ferryx_rd_proto::extended_keys::BROWSER_BACK, 0xA6),
    (ferryx_rd_proto::extended_keys::BROWSER_FORWARD, 0xA7),
    (ferryx_rd_proto::extended_keys::BROWSER_REFRESH, 0xA8),
    (ferryx_rd_proto::extended_keys::BROWSER_STOP, 0xA9),
    (ferryx_rd_proto::extended_keys::BROWSER_SEARCH, 0xAA),
    (ferryx_rd_proto::extended_keys::BROWSER_FAVORITES, 0xAB),
    (ferryx_rd_proto::extended_keys::BROWSER_HOME, 0xAC),
    (ferryx_rd_proto::extended_keys::F21, 0x84),
    (ferryx_rd_proto::extended_keys::F22, 0x85),
    (ferryx_rd_proto::extended_keys::F23, 0x86),
    (ferryx_rd_proto::extended_keys::F24, 0x87),
];

#[cfg(test)]
mod input_identity_tests {
    use super::*;
    #[test]
    fn maho_extended_keys_round_trip() {
        for (index, vk) in [
            0xB3, 0xB0, 0xB1, 0xB2, 0xA6, 0xA7, 0xA8, 0xA9, 0xAA, 0xAB, 0xAC, 0x84, 0x85, 0x86,
            0x87,
        ]
        .into_iter()
        .enumerate()
        {
            let wire = 0x100 + index as u16;
            assert_eq!(macos_keycode_to_vk(wire), Some(vk));
            assert_eq!(vk_to_macos_keycode(vk), Some(wire));
        }
    }

    #[test]
    fn keypad_enter_preserves_extended_identity() {
        assert_eq!(macos_keycode_to_windows_key(36), Some((0x0D, false)));
        assert_eq!(macos_keycode_to_windows_key(76), Some((0x0D, true)));
        assert_eq!(macos_keycode_to_windows_key(75), Some((0x6F, true)));
        assert_eq!(macos_keycode_to_windows_key(81), Some((0x92, false)));
        assert_eq!(macos_keycode_to_windows_key(24), Some((0xBB, false)));
    }

    #[test]
    fn keypad_digits_do_not_collapse_into_main_row() {
        for (wire, vk) in [
            (82, 0x60),
            (83, 0x61),
            (84, 0x62),
            (85, 0x63),
            (86, 0x64),
            (87, 0x65),
            (88, 0x66),
            (89, 0x67),
            (91, 0x68),
            (92, 0x69),
        ] {
            assert_eq!(macos_keycode_to_windows_key(wire), Some((vk, false)));
        }
        assert_eq!(macos_keycode_to_windows_key(18), Some((0x31, false)));
    }

    #[test]
    fn maps_volume_and_f13_through_f20() {
        for (wire, vk) in [
            (72, 0xAF),
            (73, 0xAE),
            (74, 0xAD),
            (105, 0x7C),
            (107, 0x7D),
            (113, 0x7E),
            (106, 0x7F),
            (64, 0x80),
            (79, 0x81),
            (80, 0x82),
            (90, 0x83),
        ] {
            assert_eq!(macos_keycode_to_vk(wire), Some(vk));
        }
    }
}

/// Raw descriptor information extracted from DXGI output enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawOutputDesc {
    pub desktop_left: i32,
    pub desktop_top: i32,
    pub desktop_right: i32,
    pub desktop_bottom: i32,
    pub rotation: i32,
}

/// Raw descriptor information extracted from DXGI desktop duplication mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawDuplDesc {
    pub mode_width: u32,
    pub mode_height: u32,
    pub rotation: i32,
}

/// Consolidated output geometry describing both physical capture pixels and logical desktop space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectedOutputMetadata {
    pub desktop_x: i32,
    pub desktop_y: i32,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub logical_width: u32,
    pub logical_height: u32,
    pub scale_factor_milli: u32,
}

impl SelectedOutputMetadata {
    pub fn scale_factor(&self) -> f32 {
        self.scale_factor_milli as f32 / 1_000.0
    }
}

pub fn resolve_output_metadata(
    output: &RawOutputDesc,
    dupl: &RawDuplDesc,
) -> Result<SelectedOutputMetadata, &'static str> {
    if dupl.mode_width == 0 || dupl.mode_height == 0 {
        return Err("physical output mode dimensions must be positive");
    }
    // Rotation values: 0 = Unspecified, 1 = Identity, 2 = Rotate90, 3 = Rotate180, 4 = Rotate270.
    // The DXGI duplication surface always arrives in unrotated native raster orientation.
    // Without a GPU shader or CPU transposition pass, rotated displays cannot be displayed correctly.
    if (dupl.rotation != 1 && dupl.rotation != 0) || (output.rotation != 1 && output.rotation != 0)
    {
        return Err("display rotation is unsupported without pixel rotation transform");
    }

    // Preserve the unrotated physical raster directly from the DXGI duplication mode.
    let pixel_width = dupl.mode_width;
    let pixel_height = dupl.mode_height;

    let raw_log_w = (output.desktop_right - output.desktop_left).unsigned_abs();
    let raw_log_h = (output.desktop_bottom - output.desktop_top).unsigned_abs();
    if raw_log_w == 0 || raw_log_h == 0 {
        return Err("desktop coordinate dimensions must be positive");
    }
    let logical_width = raw_log_w;
    let logical_height = raw_log_h;

    let scale_factor_milli = if logical_width > 0 && pixel_width > 0 {
        ((u64::from(pixel_width) * 1_000) / u64::from(logical_width)).clamp(1_000, 10_000) as u32
    } else {
        1_000
    };

    Ok(SelectedOutputMetadata {
        desktop_x: output.desktop_left,
        desktop_y: output.desktop_top,
        pixel_width,
        pixel_height,
        logical_width,
        logical_height,
        scale_factor_milli,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_data_inverts_vertical_and_scales_pixels_to_notches() {
        // One browser notch down (+100 px) is one Win32 notch backward (-120).
        assert_eq!(wheel_data_from_scroll(0.0, 100.0, false), (-120, 0));
        assert_eq!(wheel_data_from_scroll(0.0, -100.0, false), (120, 0));
        // Horizontal keeps its sign: +dx scrolls right, as does +HWHEEL.
        assert_eq!(wheel_data_from_scroll(50.0, 0.0, false), (0, 60));
        // Tiny trackpad deltas still move one unit in the right direction.
        assert_eq!(wheel_data_from_scroll(-0.2, 0.3, false), (-1, -1));
        assert_eq!(wheel_data_from_scroll(0.0, 0.0, false), (0, 0));
        assert_eq!(
            wheel_data_from_scroll(f32::NAN, f32::INFINITY, false),
            (0, 0)
        );
    }

    #[test]
    fn wheel_data_cancels_host_reverse_scrolling_and_never_wraps() {
        // Windows flips injected wheel data when the host user enabled reverse
        // scrolling, so both axes are pre-flipped to keep the client's intent.
        assert_eq!(wheel_data_from_scroll(0.0, 100.0, true), (120, 0));
        assert_eq!(wheel_data_from_scroll(50.0, 0.0, true), (0, -60));
        // Huge deltas saturate instead of wrapping the 16-bit wheel delta.
        assert_eq!(wheel_data_from_scroll(0.0, 1.0e6, false), (-32767, 0));
        assert_eq!(wheel_data_from_scroll(-1.0e6, 0.0, false), (0, -32767));
    }

    #[test]
    fn maps_alphabet_digits_navigation_and_modifiers() {
        assert_eq!(macos_keycode_to_vk(0), Some(0x41));
        assert_eq!(macos_keycode_to_vk(18), Some(0x31));
        assert_eq!(macos_keycode_to_vk(36), Some(0x0D));
        assert_eq!(macos_keycode_to_vk(49), Some(0x20));
        assert_eq!(macos_keycode_to_vk(53), Some(0x1B));
        assert_eq!(macos_keycode_to_vk(123), Some(0x25));
        assert_eq!(macos_keycode_to_vk(126), Some(0x26));
        assert_eq!(macos_keycode_to_vk(u16::MAX), None);
        assert_eq!(vk_to_macos_keycode(0x41), Some(0));
        assert_eq!(vk_to_macos_keycode(0x25), Some(123));
        assert_eq!(vk_to_macos_keycode(0xFF), None);

        assert_eq!(
            modifier_vks(
                Modifiers::SHIFT | Modifiers::CONTROL | Modifiers::OPTION | Modifiers::COMMAND
            ),
            vec![0xA0, 0xA2, 0xA4, 0x5B]
        );

        assert_eq!(
            mouse_button_flags(ferryx_rd_proto::InputEventType::LeftMouseDown),
            0x0002
        );
        assert_eq!(
            mouse_button_flags(ferryx_rd_proto::InputEventType::LeftMouseUp),
            0x0004
        );
        assert_eq!(
            mouse_button_flags(ferryx_rd_proto::InputEventType::RightMouseDown),
            0x0008
        );
        assert_eq!(
            mouse_button_flags(ferryx_rd_proto::InputEventType::RightMouseUp),
            0x0010
        );
        assert_eq!(
            mouse_button_flags(ferryx_rd_proto::InputEventType::MiddleMouseDown),
            0x0020
        );
        assert_eq!(
            mouse_button_flags(ferryx_rd_proto::InputEventType::MiddleMouseUp),
            0x0040
        );
        assert_eq!(mouse_button_flags(ferryx_rd_proto::InputEventType::MouseMove), 0);
    }

    #[test]
    fn normalizes_target_monitor_into_virtual_desktop() {
        let desktop = VirtualDesktop {
            x: -1920,
            y: 0,
            width: 4480,
            height: 1440,
        };
        let target = TargetDisplay {
            x: 0,
            y: 0,
            width: 2560,
            height: 1440,
        };
        // Wire coordinates: (0.0, 1.0) is top-left, (1.0, 0.0) is bottom-right
        let top_left = normalize_absolute_pointer(0.0, 1.0, target, desktop);
        let bottom_right = normalize_absolute_pointer(1.0, 0.0, target, desktop);
        assert!(top_left.0 > 0);
        assert_eq!(top_left.1, 0);
        assert_eq!(bottom_right.1, 65_535);
        assert!(bottom_right.0 > top_left.0);
        assert_eq!(
            normalize_absolute_pointer(f32::NAN, f32::INFINITY, target, desktop),
            normalize_absolute_pointer(0.0, 0.0, target, desktop)
        );
    }

    #[test]
    fn hashes_and_suppresses_clipboard_echo() {
        assert_eq!(
            clipboard_content_hash("hello"),
            clipboard_content_hash("hello")
        );
        assert_ne!(
            clipboard_content_hash("hello"),
            clipboard_content_hash("hello!")
        );

        let mut state = ClipboardEchoSuppressor::default();
        state.start(10);
        assert!(!state.observe_sequence(10));
        assert!(state.observe_sequence(11));
        assert!(state.observe_text("local"));
        assert!(!state.observe_text("local"));

        state.record_remote_write(12, "remote");
        assert!(!state.observe_sequence(12));
        assert!(!state.observe_text("remote"));
        assert!(state.observe_sequence(13));
        assert!(state.observe_text("other"));
    }

    #[test]
    fn clipboard_hash_is_utf8_byte_based() {
        assert_eq!("é".repeat(2048).len(), 4096);
        assert_eq!(clipboard_content_hash("é"), 0x0ac2_1707_b718_1e01);
    }

    fn read_f32_le(pcm: &[u8]) -> Vec<f32> {
        pcm.chunks_exact(4)
            .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
            .collect()
    }

    #[test]
    fn stereo_48k_float_passes_through() {
        let source = SourceAudioFormat::default();
        let pcm: Vec<u8> = [0.25_f32, -0.5]
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        let wire = convert_to_wire_audio(&pcm, &source);
        assert_eq!(read_f32_le(&wire), vec![0.25, -0.5]);
    }

    #[test]
    fn mono_is_duplicated_and_s16_converted() {
        let source = SourceAudioFormat {
            channels: 1,
            is_float: false,
            ..Default::default()
        };
        let pcm: Vec<u8> = [16384_i16, -16384]
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        let wire = convert_to_wire_audio(&pcm, &source);
        let out = read_f32_le(&wire);
        // Interleaved stereo: [L0, R0, L1, R1] with L == R == mono sample.
        assert_eq!(out.len(), 4);
        assert!((out[0] - 0.5).abs() < 1e-6 && (out[1] - 0.5).abs() < 1e-6);
        assert!((out[2] + 0.5).abs() < 1e-6 && (out[3] + 0.5).abs() < 1e-6);
    }

    #[test]
    fn multichannel_keeps_front_pair_and_partial_frames_are_dropped() {
        let source = SourceAudioFormat {
            channels: 6,
            ..Default::default()
        };
        let frame: Vec<u8> = [0.1_f32, 0.9, 0.0, 0.0, 0.0, 0.0]
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        let mut pcm = frame.clone();
        pcm.extend_from_slice(&[0xAB]); // trailing partial sample
        let out = read_f32_le(&convert_to_wire_audio(&pcm, &source));
        assert_eq!(out, vec![0.1, 0.9]);
    }

    #[test]
    fn resampling_scales_frame_count_toward_48k() {
        let source = SourceAudioFormat {
            sample_rate: 24_000,
            ..Default::default()
        };
        // Two stereo frames: (0,0) then (1,1).
        let pcm: Vec<u8> = [0.0_f32, 0.0, 1.0, 1.0]
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        let out = read_f32_le(&convert_to_wire_audio(&pcm, &source));
        assert_eq!(out.len(), 8); // 2 stereo frames @24k -> 4 stereo frames @48k
        assert_eq!(out[0], 0.0); // frame 0 -> source frame 0
        assert_eq!(out[2], 0.5); // frame 1 -> halfway 0..1
        assert!((out[4] - 1.0).abs() < 1e-6); // frame 2 -> source frame 1
    }

    #[test]
    fn windows_geometry_dpi_scaling_resolves_physical_and_logical() {
        // Given a 3840x1600 physical monitor virtualized to 3072x1280 (125% DPI scale).
        let output = RawOutputDesc {
            desktop_left: 0,
            desktop_top: 0,
            desktop_right: 3072,
            desktop_bottom: 1280,
            rotation: 1, // IDENTITY
        };
        let dupl = RawDuplDesc {
            mode_width: 3840,
            mode_height: 1600,
            rotation: 1, // IDENTITY
        };

        // When resolving metadata.
        let meta = resolve_output_metadata(&output, &dupl).unwrap();

        // Then physical pixel dimensions match DXGI capture texture (3840x1600),
        // preventing the 5,898,240 vs 9,216,000 NV12 length mismatch.
        assert_eq!(meta.desktop_x, 0);
        assert_eq!(meta.desktop_y, 0);
        assert_eq!(meta.pixel_width, 3840);
        assert_eq!(meta.pixel_height, 1600);
        assert_eq!(meta.logical_width, 3072);
        assert_eq!(meta.logical_height, 1280);
        assert_eq!(meta.scale_factor_milli, 1250);
        assert!((meta.scale_factor() - 1.25).abs() < 1e-6);

        // Verify buffer length calculation:
        let physical_nv12_len = (meta.pixel_width as usize) * (meta.pixel_height as usize) * 3 / 2;
        assert_eq!(physical_nv12_len, 9_216_000);
        let flawed_logical_nv12_len =
            (meta.logical_width as usize) * (meta.logical_height as usize) * 3 / 2;
        assert_eq!(flawed_logical_nv12_len, 5_898_240);
        // Flawed logical length fails against physical frame size.
        assert_ne!(flawed_logical_nv12_len, physical_nv12_len);
    }

    #[test]
    fn windows_geometry_ordinary_100_percent_scaling() {
        // Given standard 1080p display with 100% scaling.
        let output = RawOutputDesc {
            desktop_left: 0,
            desktop_top: 0,
            desktop_right: 1920,
            desktop_bottom: 1080,
            rotation: 1,
        };
        let dupl = RawDuplDesc {
            mode_width: 1920,
            mode_height: 1080,
            rotation: 1,
        };

        let meta = resolve_output_metadata(&output, &dupl).unwrap();
        assert_eq!(meta.desktop_x, 0);
        assert_eq!(meta.desktop_y, 0);
        assert_eq!(meta.pixel_width, 1920);
        assert_eq!(meta.pixel_height, 1080);
        assert_eq!(meta.logical_width, 1920);
        assert_eq!(meta.logical_height, 1080);
        assert_eq!(meta.scale_factor_milli, 1000);
        assert_eq!(meta.scale_factor(), 1.0);
    }

    #[test]
    fn windows_geometry_rotated_display_rejected_without_pixel_transform() {
        // Given 90-degree and 270-degree rotated display configurations.
        let output_90 = RawOutputDesc {
            desktop_left: 0,
            desktop_top: 0,
            desktop_right: 1280,
            desktop_bottom: 3072,
            rotation: 2, // ROTATE90
        };
        let dupl_90 = RawDuplDesc {
            mode_width: 3840,
            mode_height: 1600,
            rotation: 2, // ROTATE90
        };
        // When/Then: explicitly rejected rather than shipping mismatched input/advertisement.
        assert!(resolve_output_metadata(&output_90, &dupl_90).is_err());

        let output_270 = RawOutputDesc {
            desktop_left: 0,
            desktop_top: 0,
            desktop_right: 1280,
            desktop_bottom: 3072,
            rotation: 4, // ROTATE270
        };
        let dupl_270 = RawDuplDesc {
            mode_width: 3840,
            mode_height: 1600,
            rotation: 4, // ROTATE270
        };
        assert!(resolve_output_metadata(&output_270, &dupl_270).is_err());
    }

    #[test]
    fn windows_geometry_invalid_dimensions_rejected() {
        assert!(resolve_output_metadata(
            &RawOutputDesc {
                desktop_left: 0,
                desktop_top: 0,
                desktop_right: 0,
                desktop_bottom: 0,
                rotation: 1
            },
            &RawDuplDesc {
                mode_width: 1920,
                mode_height: 1080,
                rotation: 1
            },
        )
        .is_err());
        assert!(resolve_output_metadata(
            &RawOutputDesc {
                desktop_left: 0,
                desktop_top: 0,
                desktop_right: 1920,
                desktop_bottom: 1080,
                rotation: 1
            },
            &RawDuplDesc {
                mode_width: 0,
                mode_height: 0,
                rotation: 1
            },
        )
        .is_err());
    }

    #[test]
    fn windows_geometry_multimonitor_offset() {
        let output = RawOutputDesc {
            desktop_left: 1920,
            desktop_top: -200,
            desktop_right: 4480,
            desktop_bottom: 1240,
            rotation: 1,
        };
        let dupl = RawDuplDesc {
            mode_width: 2560,
            mode_height: 1440,
            rotation: 1,
        };
        let meta = resolve_output_metadata(&output, &dupl).unwrap();
        assert_eq!(meta.desktop_x, 1920);
        assert_eq!(meta.desktop_y, -200);
        assert_eq!(meta.pixel_width, 2560);
        assert_eq!(meta.pixel_height, 1440);
        assert_eq!(meta.logical_width, 2560);
        assert_eq!(meta.logical_height, 1440);
    }
}

/// Reversible MMCSS registration. No priority boost beyond the task default.
#[cfg(target_os = "windows")]
pub struct MultimediaTask {
    _registration: crate::mmcss::Registration<windows::Win32::Foundation::HANDLE>,
}

#[cfg(target_os = "windows")]
impl MultimediaTask {
    pub fn register(audio: bool) -> Option<Self> {
        use windows::core::w;
        #[link(name = "avrt")]
        unsafe extern "system" {
            fn AvSetMmThreadCharacteristicsW(
                task: windows::core::PCWSTR,
                index: *mut u32,
            ) -> windows::Win32::Foundation::HANDLE;
        }
        crate::mmcss::Registration::acquire(
            || {
                let mut index = 0;
                // SAFETY: static UTF-16 task names and a valid output index.
                let handle = unsafe {
                    AvSetMmThreadCharacteristicsW(
                        if audio { w!("Audio") } else { w!("Capture") },
                        &mut index,
                    )
                };
                if handle.is_invalid() {
                    tracing::warn!(
                        "MMCSS registration failed: {}",
                        std::io::Error::last_os_error()
                    );
                    None
                } else {
                    Some(handle)
                }
            },
            revert_multimedia_task,
        )
        .map(|registration| Self {
            _registration: registration,
        })
    }
}

#[cfg(target_os = "windows")]
fn revert_multimedia_task(handle: windows::Win32::Foundation::HANDLE) {
    #[link(name = "avrt")]
    unsafe extern "system" {
        fn AvRevertMmThreadCharacteristics(
            handle: windows::Win32::Foundation::HANDLE,
        ) -> windows::core::BOOL;
    }
    // SAFETY: registration belongs to this worker and is reverted on that worker.
    if !unsafe { AvRevertMmThreadCharacteristics(handle) }.as_bool() {
        tracing::warn!(
            "MMCSS unregister failed: {}",
            std::io::Error::last_os_error()
        );
    }
}

/// Pool ownership includes the pool itself; any other lease prevents reuse.
pub fn available_texture_slot<T>(pool: &[std::sync::Arc<T>]) -> Option<usize> {
    pool.iter()
        .position(|texture| std::sync::Arc::strong_count(texture) == 1)
}

/// Zero explicitly requests an on-demand-only GOP, per the MFT convention.
pub fn encoder_gop_size(interval: u32) -> u32 {
    if interval == 0 {
        u32::MAX
    } else {
        interval
    }
}

#[cfg(test)]
mod gpu_regression_tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn retained_capture_and_encoder_leases_block_pool_reuse() {
        let pool = [Arc::new(0), Arc::new(1)];
        let capture = Arc::clone(&pool[0]);
        let encoder = Arc::clone(&pool[1]);
        assert_eq!(available_texture_slot(&pool), None);
        drop(capture);
        assert_eq!(available_texture_slot(&pool), Some(0));
        let queued = Arc::clone(&pool[0]);
        drop(encoder);
        assert_eq!(available_texture_slot(&pool), Some(1));
        drop(queued);
    }

    #[test]
    fn zero_gop_means_no_periodic_idr() {
        assert_eq!(encoder_gop_size(0), u32::MAX);
        assert_eq!(encoder_gop_size(600), 600);
    }
}

/// Static desktops still refresh video; before decoder readiness use fast startup cadence.
pub fn idle_refresh_due(
    last_emit: std::time::Instant,
    now: std::time::Instant,
    first_keyframe_emitted: bool,
) -> bool {
    let interval = if first_keyframe_emitted { 500 } else { 33 };
    now.saturating_duration_since(last_emit) >= std::time::Duration::from_millis(interval)
}

#[cfg(test)]
mod idle_refresh_tests {
    use super::*;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    #[test]
    fn idle_refresh_cadence_has_exact_startup_and_ready_boundaries() {
        let start = Instant::now();
        for (ready, millis) in [(false, 33), (true, 500)] {
            assert!(!idle_refresh_due(
                start,
                start + Duration::from_millis(millis - 1),
                ready
            ));
            assert!(idle_refresh_due(
                start,
                start + Duration::from_millis(millis),
                ready
            ));
        }
    }

    #[test]
    fn repeated_last_frame_never_becomes_available_for_capture() {
        let pool = [Arc::new(0), Arc::new(1)];
        let last_frame = Arc::clone(&pool[0]);
        let repeated = Arc::clone(&last_frame);
        assert_eq!(available_texture_slot(&pool), Some(1));
        drop(repeated);
        assert_eq!(available_texture_slot(&pool), Some(1));
        drop(last_frame);
        assert_eq!(available_texture_slot(&pool), Some(0));
    }
}
