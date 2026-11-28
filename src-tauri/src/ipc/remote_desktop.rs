//! Remote desktop viewer: connects the ferryx-rd `ClientSession` to a paired host through
//! the Ferryx secure transport and streams decoded frames to the WebView.
//!
//! Flow per session: `POST /api/v1/remote-desktop/pair` once (record cached in a file
//! store under the Ferryx data dir), open the `/api/v1/remote-desktop/stream` socket,
//! bridge it to a loopback TCP listener and UDP socket, and run `ClientSession` against
//! that loopback endpoint on a dedicated std thread. TLS-PSK and the UDP AEAD stay end to
//! end between the session and the host; the bridge only carries bytes.

/// Whether this build carries the remote desktop client.
#[tauri::command]
pub fn cmd_rd_supported() -> bool {
    cfg!(feature = "remote-desktop")
}

#[cfg(feature = "remote-desktop")]
pub mod wire {
    //! Pure encodings shared by the bridge, the frame channel and the input path.
    use std::collections::BTreeSet;

    use ferryx_rd_proto::{InputEvent, InputEventType, Modifiers, StreamCodecPreference};
    use serde::{Deserialize, Serialize};

    /// Frame channel message kinds (byte 0).
    pub const FRAME_KIND_VIDEO: u8 = 0x01;
    pub const FRAME_KIND_STATUS: u8 = 0x02;
    /// `0x03` then UTF-8 JSON [`RdSessionMeta`].
    pub const FRAME_KIND_META: u8 = 0x03;
    /// `0x04, u32 sampleRate, u16 channels` (little endian) then interleaved f32 LE PCM.
    pub const FRAME_KIND_AUDIO: u8 = 0x04;
    /// `0x05` then UTF-8 JSON `{"text"}`.
    pub const FRAME_KIND_CLIPBOARD: u8 = 0x05;
    /// `0x06` then the UTF-8 JSON cursor payload built by `RdCursorEncoder`.
    pub const FRAME_KIND_CURSOR: u8 = crate::ipc::remote_desktop_media::FRAME_KIND_CURSOR;
    /// `0x07` then `u32 width, u32 height` (little endian): the native surface presented a
    /// frame of a new size, so the renderer drops its canvas and maps input to that size.
    pub const FRAME_KIND_NATIVE_PRESENTED: u8 = 0x07;
    /// Stream bridge channels (byte 0) and the TCP chunk cap of the gateway contract.
    pub const BRIDGE_TCP: u8 = 0x01;
    pub const BRIDGE_UDP: u8 = 0x02;
    pub const MAX_TCP_CHUNK: usize = 60_000;
    pub const MAX_CLIPBOARD_BYTES: usize = 1_048_576;

    pub const AUDIO_SAMPLE_RATE: u32 = 48_000;
    pub const AUDIO_CHANNELS: u16 = 2;

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct RdDisplayInfo {
        pub index: u8,
        pub name: String,
        pub width: u32,
        pub height: u32,
        pub primary: bool,
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct RdSessionMeta {
        pub route: String,
        pub host_name: String,
        pub displays: Vec<RdDisplayInfo>,
        pub active_display: u8,
        pub supports_stream_options: bool,
        pub width: u32,
        pub height: u32,
        pub codec: Option<String>,
        pub target_fps: u16,
        pub scale_permille: u16,
        pub jpeg_quality: u8,
        pub max_present_fps: u32,
        pub fps: u32,
        pub rtt_ms: Option<f64>,
        pub packet_loss_ratio: Option<f64>,
        pub clipboard_allowed: bool,
        pub audio_available: bool,
        pub audio_muted: bool,
    }

    #[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct RdStreamOptions {
        pub display_index: Option<u8>,
        pub scale_permille: Option<u16>,
        pub fps: Option<u16>,
        pub bitrate: Option<u32>,
        pub jpeg_quality: Option<u8>,
        pub max_present_fps: Option<u32>,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub enum RdSpecialKeyAction {
        CtrlAltDel,
        AltTab,
        CmdTab,
        Meta,
        CmdW,
        CtrlW,
        AltF4,
        Escape,
        PrintScreen,
    }

    pub fn codec_label(codec: StreamCodecPreference) -> &'static str {
        match codec {
            StreamCodecPreference::Auto => "auto",
            StreamCodecPreference::H264 => "h264",
            StreamCodecPreference::Hevc => "hevc",
        }
    }

    /// Maps a special key toolbar action to an atomic down/up key sequence.
    /// `CtrlAltDel` returns `None` because it routes through native Secure Attention (SAS).
    pub fn special_key_sequence(action: RdSpecialKeyAction) -> Option<Vec<RdInputEvent>> {
        let key = |code: &str, down: bool| RdInputEvent::Key {
            code: code.to_owned(),
            down,
        };
        let chord = |mods: &[&str], main: &str| {
            let mut events = Vec::with_capacity(mods.len() * 2 + 2);
            for m in mods {
                events.push(key(m, true));
            }
            events.push(key(main, true));
            events.push(key(main, false));
            for m in mods.iter().rev() {
                events.push(key(m, false));
            }
            events
        };
        Some(match action {
            RdSpecialKeyAction::CtrlAltDel => return None,
            RdSpecialKeyAction::AltTab => chord(&["AltLeft"], "Tab"),
            RdSpecialKeyAction::CmdTab => chord(&["MetaLeft"], "Tab"),
            RdSpecialKeyAction::Meta => vec![key("MetaLeft", true), key("MetaLeft", false)],
            RdSpecialKeyAction::CmdW => chord(&["MetaLeft"], "KeyW"),
            RdSpecialKeyAction::CtrlW => chord(&["ControlLeft"], "KeyW"),
            RdSpecialKeyAction::AltF4 => chord(&["AltLeft"], "F4"),
            RdSpecialKeyAction::Escape => vec![key("Escape", true), key("Escape", false)],
            RdSpecialKeyAction::PrintScreen => {
                vec![key("PrintScreen", true), key("PrintScreen", false)]
            }
        })
    }

    /// `0x01, u32 width, u32 height, u64 frameId` (little endian), then the JPEG bytes.
    pub fn encode_video_message(width: u32, height: u32, frame_id: u64, jpeg: &[u8]) -> Vec<u8> {
        let mut message = Vec::with_capacity(17 + jpeg.len());
        message.push(FRAME_KIND_VIDEO);
        message.extend_from_slice(&width.to_le_bytes());
        message.extend_from_slice(&height.to_le_bytes());
        message.extend_from_slice(&frame_id.to_le_bytes());
        message.extend_from_slice(jpeg);
        message
    }

    /// Exactly 9 bytes: `0x07, u32 width, u32 height` (little endian).
    pub fn encode_native_presented_frame(width: u32, height: u32) -> Vec<u8> {
        let mut message = Vec::with_capacity(9);
        message.push(FRAME_KIND_NATIVE_PRESENTED);
        message.extend_from_slice(&width.to_le_bytes());
        message.extend_from_slice(&height.to_le_bytes());
        message
    }

    /// `0x02` then UTF-8 JSON `{"state", "reason"}`.
    pub fn encode_status_message(state: &str, reason: Option<&str>) -> Vec<u8> {
        let mut message = vec![FRAME_KIND_STATUS];
        message.extend_from_slice(
            serde_json::json!({ "state": state, "reason": reason })
                .to_string()
                .as_bytes(),
        );
        message
    }

    /// `0x03` then UTF-8 JSON [`RdSessionMeta`].
    pub fn encode_meta_message(meta: &RdSessionMeta) -> Vec<u8> {
        let mut message = vec![FRAME_KIND_META];
        if let Ok(bytes) = serde_json::to_vec(meta) {
            message.extend_from_slice(&bytes);
        }
        message
    }

    /// `0x04, u32 sampleRate, u16 channels` (little endian), then interleaved f32 LE PCM bytes.
    pub fn encode_audio_message(sample_rate: u32, channels: u16, pcm: &[u8]) -> Vec<u8> {
        let mut message = Vec::with_capacity(7 + pcm.len());
        message.push(FRAME_KIND_AUDIO);
        message.extend_from_slice(&sample_rate.to_le_bytes());
        message.extend_from_slice(&channels.to_le_bytes());
        message.extend_from_slice(pcm);
        message
    }

    /// `0x05` then UTF-8 JSON `{"text"}`.
    pub fn encode_clipboard_message(text: &str) -> Vec<u8> {
        let mut message = vec![FRAME_KIND_CLIPBOARD];
        message.extend_from_slice(
            serde_json::json!({ "text": text })
                .to_string()
                .as_bytes(),
        );
        message
    }

    pub fn encode_bridge_frame(channel: u8, payload: &[u8]) -> Vec<u8> {
        let mut frame = Vec::with_capacity(payload.len() + 1);
        frame.push(channel);
        frame.extend_from_slice(payload);
        frame
    }

    /// `None` for an empty frame, an unknown channel or an oversized TCP chunk.
    pub fn decode_bridge_frame(frame: &[u8]) -> Option<(u8, &[u8])> {
        let (&channel, payload) = frame.split_first()?;
        match channel {
            BRIDGE_TCP if payload.len() <= MAX_TCP_CHUNK => Some((BRIDGE_TCP, payload)),
            BRIDGE_UDP => Some((BRIDGE_UDP, payload)),
            _ => None,
        }
    }

    #[derive(Debug, Clone, PartialEq, Deserialize)]
    #[serde(tag = "kind", rename_all = "camelCase")]
    pub enum RdInputEvent {
        MouseMove { x: f64, y: f64 },
        MouseButton { button: u8, down: bool },
        Wheel { dx: f64, dy: f64 },
        Key { code: String, down: bool },
        /// Pointer-locked motion: raw pixel deltas, no absolute position.
        RelativeMove { dx: f64, dy: f64 },
    }

    /// DOM `KeyboardEvent.code` to the macOS virtual key code the protocol carries.
    pub fn dom_code_to_key(code: &str) -> Option<u16> {
        use ferryx_rd_proto::extended_keys::*;
        Some(match code {
            "KeyA" => 0,
            "KeyS" => 1,
            "KeyD" => 2,
            "KeyF" => 3,
            "KeyH" => 4,
            "KeyG" => 5,
            "KeyZ" => 6,
            "KeyX" => 7,
            "KeyC" => 8,
            "KeyV" => 9,
            "IntlBackslash" => 10,
            "KeyB" => 11,
            "KeyQ" => 12,
            "KeyW" => 13,
            "KeyE" => 14,
            "KeyR" => 15,
            "KeyY" => 16,
            "KeyT" => 17,
            "Digit1" => 18,
            "Digit2" => 19,
            "Digit3" => 20,
            "Digit4" => 21,
            "Digit6" => 22,
            "Digit5" => 23,
            "Equal" => 24,
            "Digit9" => 25,
            "Digit7" => 26,
            "Minus" => 27,
            "Digit8" => 28,
            "Digit0" => 29,
            "BracketRight" => 30,
            "KeyO" => 31,
            "KeyU" => 32,
            "BracketLeft" => 33,
            "KeyI" => 34,
            "KeyP" => 35,
            "Enter" => 36,
            "KeyL" => 37,
            "KeyJ" => 38,
            "Quote" => 39,
            "KeyK" => 40,
            "Semicolon" => 41,
            "Backslash" => 42,
            "Comma" => 43,
            "Slash" => 44,
            "KeyN" => 45,
            "KeyM" => 46,
            "Period" => 47,
            "Tab" => 48,
            "Space" => 49,
            "Backquote" => 50,
            "Backspace" => 51,
            "Escape" => 53,
            "MetaRight" => 54,
            "MetaLeft" => 55,
            "ShiftLeft" => 56,
            "CapsLock" => 57,
            "AltLeft" => 58,
            "ControlLeft" => 59,
            "ShiftRight" => 60,
            "AltRight" => 61,
            "ControlRight" => 62,
            "F17" => 64,
            "NumpadDecimal" => 65,
            "NumpadMultiply" => 67,
            "NumpadAdd" => 69,
            "NumLock" => 71,
            "AudioVolumeUp" => 72,
            "AudioVolumeDown" => 73,
            "AudioVolumeMute" => 74,
            "NumpadDivide" => 75,
            "NumpadEnter" => 76,
            "NumpadSubtract" => 78,
            "F18" => 79,
            "F19" => 80,
            "NumpadEqual" => 81,
            "Numpad0" => 82,
            "Numpad1" => 83,
            "Numpad2" => 84,
            "Numpad3" => 85,
            "Numpad4" => 86,
            "Numpad5" => 87,
            "Numpad6" => 88,
            "Numpad7" => 89,
            "F20" => 90,
            "Numpad8" => 91,
            "Numpad9" => 92,
            "F5" => 96,
            "F6" => 97,
            "F7" => 98,
            "F3" => 99,
            "F8" => 100,
            "F9" => 101,
            "F11" => 103,
            "F13" | "PrintScreen" => 105,
            "F16" => 106,
            "F14" => 107,
            "F10" => 109,
            "ContextMenu" => 110,
            "F12" => 111,
            "F15" => 113,
            "Insert" => 114,
            "Home" => 115,
            "PageUp" => 116,
            "Delete" => 117,
            "F4" => 118,
            "End" => 119,
            "F2" => 120,
            "PageDown" => 121,
            "F1" => 122,
            "ArrowLeft" => 123,
            "ArrowRight" => 124,
            "ArrowDown" => 125,
            "ArrowUp" => 126,
            "F21" => F21,
            "F22" => F22,
            "F23" => F23,
            "F24" => F24,
            "MediaPlayPause" => MEDIA_PLAY_PAUSE,
            "MediaTrackNext" => MEDIA_NEXT,
            "MediaTrackPrevious" => MEDIA_PREVIOUS,
            "MediaStop" => MEDIA_STOP,
            "BrowserBack" => BROWSER_BACK,
            "BrowserForward" => BROWSER_FORWARD,
            "BrowserRefresh" => BROWSER_REFRESH,
            "BrowserStop" => BROWSER_STOP,
            "BrowserSearch" => BROWSER_SEARCH,
            "BrowserFavorites" => BROWSER_FAVORITES,
            "BrowserHome" => BROWSER_HOME,
            _ => return None,
        })
    }

    fn modifier_of(key: u16) -> Option<Modifiers> {
        match key {
            56 | 60 => Some(Modifiers::SHIFT),
            59 | 62 => Some(Modifiers::CONTROL),
            58 | 61 => Some(Modifiers::OPTION),
            55 | 54 => Some(Modifiers::COMMAND),
            _ => None,
        }
    }

    /// Per-session input state: pointer position, held buttons and modifiers.
    #[derive(Debug, Default)]
    pub struct InputMapper {
        x: f32,
        y: f32,
        left: bool,
        right: bool,
        caps_lock: bool,
        held_modifiers: BTreeSet<u16>,
    }

    impl InputMapper {
        fn modifiers(&self) -> Modifiers {
            let mut modifiers = Modifiers::empty();
            for key in &self.held_modifiers {
                if let Some(bit) = modifier_of(*key) {
                    modifiers |= bit;
                }
            }
            if self.caps_lock {
                modifiers |= Modifiers::CAPS_LOCK;
            }
            modifiers
        }

        fn event(&self, event_type: InputEventType, key_code: u16, dx: f32, dy: f32) -> InputEvent {
            InputEvent {
                event_type,
                x: self.x,
                y: self.y,
                key_code,
                modifiers: self.modifiers(),
                scroll_dx: dx,
                scroll_dy: dy,
            }
        }

        /// Maps one renderer event; unknown keys or buttons and non-finite values map to
        /// nothing. Coordinates are normalized to the remote frame; the protocol's y axis
        /// runs bottom-up, so y is inverted here as every ferryx-rd client does.
        pub fn map(&mut self, input: &RdInputEvent) -> Option<InputEvent> {
            match input {
                RdInputEvent::MouseMove { x, y } => {
                    if !x.is_finite() || !y.is_finite() {
                        return None;
                    }
                    self.x = x.clamp(0.0, 1.0) as f32;
                    self.y = 1.0 - y.clamp(0.0, 1.0) as f32;
                    let event_type = if self.left {
                        InputEventType::LeftMouseDragged
                    } else if self.right {
                        InputEventType::RightMouseDragged
                    } else {
                        InputEventType::MouseMove
                    };
                    Some(self.event(event_type, 0, 0.0, 0.0))
                }
                RdInputEvent::MouseButton { button, down } => {
                    let event_type = match (button, down) {
                        (0, true) => InputEventType::LeftMouseDown,
                        (0, false) => InputEventType::LeftMouseUp,
                        (1, true) => InputEventType::MiddleMouseDown,
                        (1, false) => InputEventType::MiddleMouseUp,
                        (2, true) => InputEventType::RightMouseDown,
                        (2, false) => InputEventType::RightMouseUp,
                        _ => return None,
                    };
                    match button {
                        0 => self.left = *down,
                        2 => self.right = *down,
                        _ => {}
                    }
                    Some(self.event(event_type, 0, 0.0, 0.0))
                }
                RdInputEvent::Wheel { dx, dy } => {
                    if !dx.is_finite() || !dy.is_finite() {
                        return None;
                    }
                    Some(self.event(InputEventType::ScrollWheel, 0, *dx as f32, *dy as f32))
                }
                RdInputEvent::RelativeMove { dx, dy } => {
                    // Hosts carry relative deltas in the scroll fields and reject non-finite
                    // values; an f64 beyond f32 range would arrive as infinity.
                    let (dx, dy) = (*dx as f32, *dy as f32);
                    if !dx.is_finite() || !dy.is_finite() {
                        return None;
                    }
                    Some(self.event(InputEventType::RelativeMove, 0, dx, dy))
                }
                RdInputEvent::Key { code, down } => {
                    let key = dom_code_to_key(code)?;
                    if key == 57 {
                        // Caps Lock toggles on press; the release carries no state.
                        if !down {
                            return None;
                        }
                        self.caps_lock = !self.caps_lock;
                        return Some(self.event(InputEventType::FlagsChanged, key, 0.0, 0.0));
                    }
                    if modifier_of(key).is_some() {
                        let changed = if *down {
                            self.held_modifiers.insert(key)
                        } else {
                            self.held_modifiers.remove(&key)
                        };
                        return changed
                            .then(|| self.event(InputEventType::FlagsChanged, key, 0.0, 0.0));
                    }
                    let event_type = if *down {
                        InputEventType::KeyDown
                    } else {
                        InputEventType::KeyUp
                    };
                    Some(self.event(event_type, key, 0.0, 0.0))
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn video_message_is_little_endian_header_then_jpeg() {
            let message = encode_video_message(1920, 1080, 0x0102_0304_0506_0708, &[0xFF, 0xD8]);
            assert_eq!(message[0], FRAME_KIND_VIDEO);
            assert_eq!(&message[1..5], &1920_u32.to_le_bytes());
            assert_eq!(&message[5..9], &1080_u32.to_le_bytes());
            assert_eq!(&message[9..17], &0x0102_0304_0506_0708_u64.to_le_bytes());
            assert_eq!(&message[17..], &[0xFF, 0xD8]);
        }

        #[test]
        fn native_presented_message_is_exactly_nine_little_endian_bytes() {
            let message = encode_native_presented_frame(1920, 1080);
            assert_eq!(message.len(), 9);
            assert_eq!(message[0], FRAME_KIND_NATIVE_PRESENTED);
            assert_eq!(FRAME_KIND_NATIVE_PRESENTED, 0x07);
            let width = u32::from_le_bytes(message[1..5].try_into().unwrap());
            let height = u32::from_le_bytes(message[5..9].try_into().unwrap());
            assert_eq!((width, height), (1920, 1080));
            let large = encode_native_presented_frame(u32::MAX, 0x0102_0304);
            assert_eq!(&large[1..5], &[0xFF; 4]);
            assert_eq!(&large[5..9], &[0x04, 0x03, 0x02, 0x01]);
            assert_eq!(FRAME_KIND_CURSOR, 0x06);
        }

        #[test]
        fn relative_move_maps_finite_deltas_and_rejects_non_finite() {
            let parsed: RdInputEvent = serde_json::from_value(
                serde_json::json!({ "kind": "relativeMove", "dx": 3.5, "dy": -2.0 }),
            )
            .unwrap();
            assert_eq!(parsed, RdInputEvent::RelativeMove { dx: 3.5, dy: -2.0 });
            let mut mapper = InputMapper::default();
            mapper.map(&RdInputEvent::MouseMove { x: 0.25, y: 0.75 });
            let moved = mapper.map(&parsed).unwrap();
            assert_eq!(moved.event_type, InputEventType::RelativeMove);
            assert_eq!((moved.scroll_dx, moved.scroll_dy), (3.5, -2.0));
            assert_eq!((moved.x, moved.y), (0.25, 0.25));
            for (dx, dy) in [
                (f64::NAN, 0.0),
                (0.0, f64::NAN),
                (f64::INFINITY, 0.0),
                (0.0, f64::NEG_INFINITY),
                (f64::MAX, 0.0),
            ] {
                assert!(mapper.map(&RdInputEvent::RelativeMove { dx, dy }).is_none());
            }
        }

        #[test]
        fn status_message_carries_state_and_nullable_reason() {
            let message = encode_status_message("ended", None);
            assert_eq!(message[0], FRAME_KIND_STATUS);
            let body: serde_json::Value = serde_json::from_slice(&message[1..]).unwrap();
            assert_eq!(
                body,
                serde_json::json!({ "state": "ended", "reason": null })
            );
            let body: serde_json::Value =
                serde_json::from_slice(&encode_status_message("ended", Some("gone"))[1..]).unwrap();
            assert_eq!(body["reason"], "gone");
        }

        #[test]
        fn bridge_frames_round_trip_and_reject_bad_input() {
            let tcp = encode_bridge_frame(BRIDGE_TCP, b"hello");
            assert_eq!(decode_bridge_frame(&tcp), Some((BRIDGE_TCP, &b"hello"[..])));
            let udp = encode_bridge_frame(BRIDGE_UDP, &[1, 2, 3]);
            assert_eq!(
                decode_bridge_frame(&udp),
                Some((BRIDGE_UDP, &[1, 2, 3][..]))
            );
            let largest = encode_bridge_frame(BRIDGE_TCP, &vec![0; MAX_TCP_CHUNK]);
            assert!(decode_bridge_frame(&largest).is_some());
            let oversized = encode_bridge_frame(BRIDGE_TCP, &vec![0; MAX_TCP_CHUNK + 1]);
            assert!(decode_bridge_frame(&oversized).is_none());
            assert!(decode_bridge_frame(&[]).is_none());
            assert!(decode_bridge_frame(&[0x03, 1]).is_none());
        }

        #[test]
        fn input_events_deserialize_from_the_renderer_shape() {
            let events: Vec<RdInputEvent> = serde_json::from_value(serde_json::json!([
                { "kind": "mouseMove", "x": 0.5, "y": 0.25 },
                { "kind": "mouseButton", "button": 2, "down": true },
                { "kind": "wheel", "dx": 0.0, "dy": -3.0 },
                { "kind": "key", "code": "KeyA", "down": false },
            ]))
            .unwrap();
            assert_eq!(events[0], RdInputEvent::MouseMove { x: 0.5, y: 0.25 });
            assert_eq!(
                events[3],
                RdInputEvent::Key {
                    code: "KeyA".into(),
                    down: false
                }
            );
        }

        #[test]
        fn pointer_maps_to_normalized_bottom_up_coordinates_and_drags() {
            let mut mapper = InputMapper::default();
            let moved = mapper
                .map(&RdInputEvent::MouseMove { x: 0.25, y: 0.75 })
                .unwrap();
            assert_eq!(moved.event_type, InputEventType::MouseMove);
            assert_eq!((moved.x, moved.y), (0.25, 0.25));
            let down = mapper
                .map(&RdInputEvent::MouseButton {
                    button: 0,
                    down: true,
                })
                .unwrap();
            assert_eq!(down.event_type, InputEventType::LeftMouseDown);
            assert_eq!((down.x, down.y), (0.25, 0.25));
            let drag = mapper
                .map(&RdInputEvent::MouseMove { x: 2.0, y: -1.0 })
                .unwrap();
            assert_eq!(drag.event_type, InputEventType::LeftMouseDragged);
            assert_eq!((drag.x, drag.y), (1.0, 1.0));
            let middle = mapper
                .map(&RdInputEvent::MouseButton {
                    button: 1,
                    down: true,
                })
                .unwrap();
            assert_eq!(middle.event_type, InputEventType::MiddleMouseDown);
            assert!(mapper
                .map(&RdInputEvent::MouseButton {
                    button: 7,
                    down: true
                })
                .is_none());
            assert!(mapper
                .map(&RdInputEvent::MouseMove {
                    x: f64::NAN,
                    y: 0.0
                })
                .is_none());
        }

        #[test]
        fn wheel_passes_dom_pixel_deltas_through() {
            let mut mapper = InputMapper::default();
            let wheel = mapper
                .map(&RdInputEvent::Wheel {
                    dx: 4.0,
                    dy: -120.0,
                })
                .unwrap();
            assert_eq!(wheel.event_type, InputEventType::ScrollWheel);
            assert_eq!((wheel.scroll_dx, wheel.scroll_dy), (4.0, -120.0));
        }

        #[test]
        fn keys_map_to_mac_codes_with_tracked_modifiers() {
            let mut mapper = InputMapper::default();
            let key = |code: &str, down| RdInputEvent::Key {
                code: code.into(),
                down,
            };
            let shift = mapper.map(&key("ShiftLeft", true)).unwrap();
            assert_eq!(shift.event_type, InputEventType::FlagsChanged);
            assert_eq!(shift.key_code, 56);
            assert!(shift.modifiers.contains(Modifiers::SHIFT));
            assert!(
                mapper.map(&key("ShiftLeft", true)).is_none(),
                "auto-repeat is not a change"
            );
            let a = mapper.map(&key("KeyA", true)).unwrap();
            assert_eq!((a.event_type, a.key_code), (InputEventType::KeyDown, 0));
            assert!(a.modifiers.contains(Modifiers::SHIFT));
            let released = mapper.map(&key("ShiftLeft", false)).unwrap();
            assert!(!released.modifiers.contains(Modifiers::SHIFT));
            let up = mapper.map(&key("KeyA", false)).unwrap();
            assert_eq!(up.event_type, InputEventType::KeyUp);
            assert_eq!(up.modifiers, Modifiers::empty());
            let caps = mapper.map(&key("CapsLock", true)).unwrap();
            assert!(caps.modifiers.contains(Modifiers::CAPS_LOCK));
            assert!(mapper.map(&key("CapsLock", false)).is_none());
            assert!(mapper.map(&key("Unidentified", true)).is_none());
            assert_eq!(dom_code_to_key("ArrowUp"), Some(126));
            assert_eq!(dom_code_to_key("NumpadEnter"), Some(76));
            assert_eq!(
                dom_code_to_key("F24"),
                Some(ferryx_rd_proto::extended_keys::F24)
            );
            assert_eq!(dom_code_to_key("PrintScreen"), Some(105));
        }

        #[test]
        fn meta_audio_and_clipboard_messages_encode_wire_headers() {
            let meta = RdSessionMeta {
                route: "direct".to_owned(),
                host_name: "Studio".to_owned(),
                displays: vec![RdDisplayInfo {
                    index: 0,
                    name: "Display 1".to_owned(),
                    width: 2560,
                    height: 1440,
                    primary: true,
                }],
                active_display: 0,
                supports_stream_options: true,
                width: 2560,
                height: 1440,
                codec: Some("h264".to_owned()),
                target_fps: 60,
                scale_permille: 1000,
                jpeg_quality: 72,
                max_present_fps: 30,
                fps: 29,
                rtt_ms: Some(4.5),
                packet_loss_ratio: Some(0.0),
                clipboard_allowed: true,
                audio_available: true,
                audio_muted: false,
            };
            let encoded_meta = encode_meta_message(&meta);
            assert_eq!(encoded_meta[0], FRAME_KIND_META);
            let decoded: RdSessionMeta =
                serde_json::from_slice(&encoded_meta[1..]).expect("meta json");
            assert_eq!(decoded, meta);

            let pcm = [0u8, 1, 2, 3, 4, 5, 6, 7];
            let encoded_audio = encode_audio_message(48_000, 2, &pcm);
            assert_eq!(encoded_audio[0], FRAME_KIND_AUDIO);
            assert_eq!(
                u32::from_le_bytes(encoded_audio[1..5].try_into().unwrap()),
                48_000
            );
            assert_eq!(
                u16::from_le_bytes(encoded_audio[5..7].try_into().unwrap()),
                2
            );
            assert_eq!(&encoded_audio[7..], &pcm);

            let encoded_clip = encode_clipboard_message("hello remote");
            assert_eq!(encoded_clip[0], FRAME_KIND_CLIPBOARD);
            let clip_json: serde_json::Value =
                serde_json::from_slice(&encoded_clip[1..]).expect("clipboard json");
            assert_eq!(clip_json["text"], "hello remote");
        }

        #[test]
        fn special_key_sequences_keep_ctrl_alt_del_native_and_balance_chords() {
            assert!(special_key_sequence(RdSpecialKeyAction::CtrlAltDel).is_none());
            let mut mapper = InputMapper::default();
            for action in [
                RdSpecialKeyAction::AltTab,
                RdSpecialKeyAction::CmdTab,
                RdSpecialKeyAction::Meta,
                RdSpecialKeyAction::CmdW,
                RdSpecialKeyAction::CtrlW,
                RdSpecialKeyAction::AltF4,
                RdSpecialKeyAction::Escape,
                RdSpecialKeyAction::PrintScreen,
            ] {
                let seq = special_key_sequence(action).expect("key sequence");
                assert!(seq.len() >= 2);
                let mapped: Vec<_> = seq.iter().filter_map(|ev| mapper.map(ev)).collect();
                assert_eq!(mapped.len(), seq.len(), "all events for {action:?} must map");
                assert_eq!(
                    mapped.last().unwrap().modifiers,
                    Modifiers::empty(),
                    "modifiers must be clean after {action:?}"
                );
            }
        }
    }
}

// ---- session runtime ----

#[cfg(feature = "remote-desktop")]
pub mod viewer {
    //! The per-session media pipeline. `ferryx-rd-recv` owns `ClientSession` I/O (control
    //! events, UDP media, audio, cursor and clipboard) and admits encoded frames into the
    //! shared frame queue; `ferryx-rd-decode` pops them, decodes and presents through the
    //! native surface when one is attached, else as JPEG on the frame channel. Neither thread
    //! waits on the other except through the queue, so a slow decode never stalls receive.
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use ferryx_rd_app::frame_queue::{FrameQueue, FrameQueueConfig, PlayoutMode, QueuedFrame};
    use ferryx_rd_app::{ClientSession, PairingRecord, ReconnectState, SessionEvent};
    use ferryx_rd_decode::{
        detect_codec, CodecKind, DecodedVideoFrame, HardwareAcceleration, HevcDecoder,
    };
    use ferryx_rd_proto::{ControlMessage, StreamCodecPreference, StreamOptionsRequest};
    use parking_lot::Mutex;
    use serde::{Deserialize, Serialize};
    use tauri::ipc::{Channel, Response};

    use super::wire::{self, RdDisplayInfo, RdSessionMeta};
    use crate::ipc::remote_desktop_codec::{CodecAction, RdCodecPolicy};
    use crate::ipc::remote_desktop_media::{RdAudioPlayer, RdClipboardBridge, RdCursorEncoder};
    use crate::ipc::remote_desktop_stats::RdTelemetry;
    use crate::ipc::remote_desktop_surface::RdNativeSurface;

    /// JPEG quality the frame channel starts with, and the default presentation cap
    /// (`0` follows the window refresh rate instead of a fixed ceiling).
    pub const DEFAULT_JPEG_QUALITY: u32 = 70;
    pub const DEFAULT_MAX_PRESENT_FPS: u32 = 0;
    /// Stream option requests remembered for a host reject; older ids are forgotten.
    const MAX_PENDING_REQUESTS: usize = 16;
    /// Adaptive bitrate bounds handed to the session's own controller.
    const ABR_MIN_BITRATE: i32 = 1_500_000;
    const ABR_MAX_BITRATE: i32 = 100_000_000;

    /// Route, host name and the frame channel for one session: the meta emitter shared by
    /// the Tauri commands and the bridge's route callback. Everything else it reports comes
    /// from [`ViewerShared`], so commands and the pipeline never hold two copies of a value.
    pub struct ViewerRuntime {
        route: Mutex<&'static str>,
        host_name: Mutex<String>,
        frames: Channel<Response>,
        shared: ViewerShared,
    }

    impl ViewerRuntime {
        pub fn new(
            initial_route: &'static str,
            frames: Channel<Response>,
            shared: ViewerShared,
        ) -> Self {
            Self {
                route: Mutex::new(initial_route),
                host_name: Mutex::new(String::new()),
                frames,
                shared,
            }
        }

        pub fn set_route(&self, route: &'static str) {
            *self.route.lock() = route;
        }

        pub fn build_meta(&self, session: &ClientSession) -> RdSessionMeta {
            let stream_state = session.stream_state().unwrap_or_default();
            let stats = session.session_stats().ok();
            let (scale_permille, requested_codec) = {
                let stream = self.shared.stream.lock();
                (stream.scale_permille(), Some(wire::codec_label(stream.codec()).to_owned()))
            };
            let (last_w, last_h) = *self.shared.last_frame_size.lock();
            let (width, height, active_display, codec, target_fps) = match stream_state.active.as_ref()
            {
                Some(active) => (
                    if last_w > 0 { last_w } else { active.width },
                    if last_h > 0 { last_h } else { active.height },
                    active.display_index,
                    Some(wire::codec_label(active.codec).to_owned()),
                    active.fps,
                ),
                None => (
                    last_w,
                    last_h,
                    0,
                    requested_codec,
                    self.shared.target_fps().min(u32::from(u16::MAX)) as u16,
                ),
            };
            let displays = stream_state
                .displays
                .iter()
                .map(|d| RdDisplayInfo {
                    index: d.index,
                    name: d.name.clone(),
                    width: d.width,
                    height: d.height,
                    primary: d.primary,
                })
                .collect();
            let rtt_ms = stats
                .as_ref()
                .and_then(|s| s.rtt_smoothed_ms.or(s.rtt_latest_ms));
            let packet_loss_ratio = stats.as_ref().and_then(|s| s.packet_loss_ratio);
            let presented_fps = self
                .shared
                .telemetry
                .lock()
                .snapshot(None, Instant::now())
                .present_fps;
            let audio_muted = self.shared.audio.lock().is_muted();
            RdSessionMeta {
                route: (*self.route.lock()).to_owned(),
                host_name: self.host_name.lock().clone(),
                displays,
                active_display,
                supports_stream_options: stream_state.host_supports_stream_options,
                width,
                height,
                codec,
                target_fps,
                scale_permille,
                jpeg_quality: self.shared.jpeg_quality.load(Ordering::Relaxed).clamp(30, 95) as u8,
                max_present_fps: self.shared.max_present_fps.load(Ordering::Relaxed),
                fps: presented_fps.round() as u32,
                rtt_ms,
                packet_loss_ratio,
                clipboard_allowed: session.clipboard_allowed().unwrap_or(false),
                audio_available: self.shared.audio_available.load(Ordering::Relaxed),
                audio_muted,
            }
        }

        /// Sends the current meta and returns it, so a command that answers with meta does
        /// not build it twice.
        pub fn emit_meta(&self, session: &ClientSession) -> RdSessionMeta {
            let meta = self.build_meta(session);
            let _ = self.frames.send(Response::new(wire::encode_meta_message(&meta)));
            meta
        }
    }

    pub struct ViewerConfig {
        pub session: ClientSession,
        pub pairing: PairingRecord,
        pub loopback_tcp: u16,
        pub loopback_udp: u16,
        pub runtime: Arc<ViewerRuntime>,
        pub stop: Arc<AtomicBool>,
        /// Called once when the thread ends, so the registry and bridge tear down.
        pub on_exit: Box<dyn FnOnce() + Send>,
    }

    /// Per-session tuning chosen by the caller.
    #[derive(Debug, Clone, Copy, Default)]
    pub struct ViewerOptions {
        /// An explicit codec choice; `None` lets the session pick and downgrade on its own.
        pub codec_override: Option<StreamCodecPreference>,
    }

    /// The last `(width, height)` announced to the renderer for the native surface. A
    /// `0x07` message goes out on the first GPU frame and whenever the size changes; a
    /// reset (attach, reattach, reconnect, fallback) makes the next frame announce again.
    #[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
    pub struct PresentedDims(Option<(u32, u32)>);

    impl PresentedDims {
        /// True when `(width, height)` differs from the last announced size.
        pub fn observe(&mut self, width: u32, height: u32) -> bool {
            if self.0 == Some((width, height)) {
                return false;
            }
            self.0 = Some((width, height));
            true
        }

        pub fn reset(&mut self) {
            self.0 = None;
        }

        pub fn last(&self) -> Option<(u32, u32)> {
            self.0
        }
    }

    /// The attached native presenter and the size last announced for it.
    #[derive(Default)]
    pub struct SurfaceSlot {
        pub surface: Option<RdNativeSurface>,
        pub presented: PresentedDims,
    }

    struct FrameQueueSlotState {
        queue: Arc<FrameQueue>,
        config: FrameQueueConfig,
        closed: bool,
    }

    /// The decode queue, replaceable as a whole. The host restarts frame ids on reconnect
    /// and `FrameQueue` rejects any id behind the last one as late, so a reconnect swaps in
    /// a fresh queue (same config) and stops the old one, which wakes a parked decoder onto
    /// the new queue. `close` is terminal and wakes the decoder for good.
    pub struct FrameQueueSlot {
        state: Mutex<FrameQueueSlotState>,
    }

    impl FrameQueueSlot {
        pub fn new(config: FrameQueueConfig) -> Self {
            Self {
                state: Mutex::new(FrameQueueSlotState {
                    queue: Arc::new(FrameQueue::with_config(
                        config,
                        ferryx_rd_app::ReceiverTrace::default(),
                    )),
                    config,
                    closed: false,
                }),
            }
        }

        /// The live queue, or `None` once closed.
        pub fn current(&self) -> Option<Arc<FrameQueue>> {
            let state = self.state.lock();
            (!state.closed).then(|| Arc::clone(&state.queue))
        }

        pub fn config(&self) -> FrameQueueConfig {
            self.state.lock().config
        }

        /// Admits one frame; `Some(true)` asks for a keyframe, `None` means closed. A push
        /// that races a reset lands on the stopped queue and is simply dropped.
        pub fn push(&self, frame: QueuedFrame) -> Option<bool> {
            match self.current()?.push(frame) {
                Ok(request_keyframe) => Some(request_keyframe),
                Err(_) => (!self.state.lock().closed).then_some(false),
            }
        }

        pub fn set_playout_mode(&self, mode: PlayoutMode) {
            let mut state = self.state.lock();
            state.config.playout = mode;
            let _ = state.queue.set_playout_mode(mode);
        }

        /// Resizes for a confirmed stream rate, keeping queued frames and the playout mode.
        pub fn reconfigure_fps(&self, fps: u32) {
            let mut state = self.state.lock();
            state.config = FrameQueueConfig::for_fps(fps, state.config.playout);
            let _ = state.queue.reconfigure(state.config);
        }

        /// Swaps in an empty queue with the current config. False once closed.
        pub fn reset(&self) -> bool {
            let mut state = self.state.lock();
            if state.closed {
                return false;
            }
            let fresh = Arc::new(FrameQueue::with_config(
                state.config,
                ferryx_rd_app::ReceiverTrace::default(),
            ));
            // Replace before stopping: a decoder woken by the stop re-reads the slot.
            let old = std::mem::replace(&mut state.queue, fresh);
            let _ = old.stop();
            true
        }

        pub fn close(&self) {
            let mut state = self.state.lock();
            state.closed = true;
            let _ = state.queue.stop();
        }

        /// Blocks until a frame is due; `None` once closed (or if the queue is poisoned).
        pub fn next_frame(&self) -> Option<(Arc<FrameQueue>, QueuedFrame)> {
            let mut queue = self.current()?;
            loop {
                match queue.recv() {
                    Ok(frame) => return Some((queue, frame)),
                    Err(_) => {
                        let next = self.current()?;
                        if Arc::ptr_eq(&next, &queue) {
                            return None;
                        }
                        queue = next;
                    }
                }
            }
        }
    }

    /// State shared by the receive thread, the decode thread and the session's commands.
    #[derive(Clone)]
    pub struct ViewerShared {
        pub telemetry: Arc<Mutex<RdTelemetry>>,
        /// Requested scale, viewport, codec and the request-id / reject-fallback state.
        pub stream: Arc<Mutex<StreamController>>,
        pub audio: Arc<Mutex<RdAudioPlayer>>,
        pub surface_slot: Arc<Mutex<SurfaceSlot>>,
        pub frame_queue: Arc<FrameQueueSlot>,
        /// Set while connected, so commands can query stats or send control messages.
        pub session_handle: Arc<Mutex<Option<Arc<ClientSession>>>>,
        /// Built lazily on a std thread; clipboard calls block on OS I/O.
        pub clipboard_bridge: Arc<Mutex<Option<RdClipboardBridge>>>,
        /// The viewing window's refresh rate; changes live with the monitor.
        target_fps: Arc<AtomicU32>,
        /// JPEG quality for the canvas path and the optional presentation cap. A cap of
        /// `0` follows [`Self::target_fps`] instead of a fixed ceiling.
        pub jpeg_quality: Arc<AtomicU32>,
        pub max_present_fps: Arc<AtomicU32>,
        /// True once the host sent PCM, so the meta reports audio as available.
        pub audio_available: Arc<AtomicBool>,
        /// DOM presentation modes use JPEG frames rather than the native aspect-fit surface.
        pub canvas_presentation: Arc<AtomicBool>,
        pub retry_renderer: Arc<AtomicBool>,
        /// Last decoded frame size, which outruns the control channel's stream state.
        pub last_frame_size: Arc<Mutex<(u32, u32)>>,
    }

    impl ViewerShared {
        pub fn new(rd_session_id: &str, target_fps: u32, playout: PlayoutMode) -> Self {
            let target_fps = target_fps.max(1);
            let mut telemetry = RdTelemetry::new(rd_session_id, target_fps);
            telemetry.set_playout_mode(playout_name(playout));
            Self {
                telemetry: Arc::new(Mutex::new(telemetry)),
                stream: Arc::new(Mutex::new(StreamController::new(
                    RdScaleMode::Fit,
                    (0, 0),
                    StreamCodecPreference::Auto,
                ))),
                audio: Arc::new(Mutex::new(RdAudioPlayer::default())),
                surface_slot: Arc::new(Mutex::new(SurfaceSlot::default())),
                frame_queue: Arc::new(FrameQueueSlot::new(FrameQueueConfig::for_fps(
                    target_fps, playout,
                ))),
                session_handle: Arc::new(Mutex::new(None)),
                clipboard_bridge: Arc::new(Mutex::new(None)),
                target_fps: Arc::new(AtomicU32::new(target_fps)),
                jpeg_quality: Arc::new(AtomicU32::new(DEFAULT_JPEG_QUALITY)),
                max_present_fps: Arc::new(AtomicU32::new(DEFAULT_MAX_PRESENT_FPS)),
                audio_available: Arc::new(AtomicBool::new(false)),
                canvas_presentation: Arc::new(AtomicBool::new(false)),
                retry_renderer: Arc::new(AtomicBool::new(false)),
                last_frame_size: Arc::new(Mutex::new((0, 0))),
            }
        }

        pub fn target_fps(&self) -> u32 {
            self.target_fps.load(Ordering::SeqCst)
        }

        /// The presentation rate: the explicit cap when one is set, else the window's
        /// refresh rate, so a 75 Hz window presents at 75 fps without extra plumbing.
        pub fn present_fps(&self) -> u32 {
            let cap = self.max_present_fps.load(Ordering::Relaxed);
            if cap == 0 {
                self.target_fps()
            } else {
                cap.clamp(1, 240)
            }
        }

        /// Adopts a new window refresh rate as the presentation target. False when the
        /// rate is unchanged, so callers only renegotiate the stream on a real transition.
        pub fn apply_refresh(&self, refresh_hz: u32) -> bool {
            let fps = refresh_hz.max(1);
            if self.target_fps.swap(fps, Ordering::SeqCst) == fps {
                return false;
            }
            self.frame_queue.reconfigure_fps(fps);
            self.telemetry.lock().set_target_fps(fps);
            true
        }

        pub fn set_playout_mode(&self, mode: PlayoutMode) {
            self.frame_queue.set_playout_mode(mode);
            self.telemetry.lock().set_playout_mode(playout_name(mode));
        }

        /// Installs a native presenter; the next presented frame announces its size.
        pub fn attach_surface(&self, surface: RdNativeSurface) {
            let previous = {
                let mut slot = self.surface_slot.lock();
                slot.presented.reset();
                slot.surface.replace(surface)
            };
            // RdNativeSurface drops its GPU handles before its native target; do it
            // outside the lock so a main-thread teardown never waits on the decoder.
            drop(previous);
        }

        /// Removes the native presenter; frames go back to the JPEG channel.
        pub fn detach_surface(&self) {
            let previous = {
                let mut slot = self.surface_slot.lock();
                slot.presented.reset();
                slot.surface.take()
            };
            drop(previous);
            self.telemetry.lock().set_present_backend("canvas2d");
        }
    }

    pub fn playout_name(mode: PlayoutMode) -> &'static str {
        match mode {
            PlayoutMode::Immediate => "immediate",
            PlayoutMode::Adaptive => "adaptive",
            PlayoutMode::LatestWins => "latest_wins",
        }
    }

    /// How the host should scale its capture relative to the viewer.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub enum RdScaleMode {
        /// Downscale so the stream never exceeds the viewport's physical pixels.
        Fit,
        Native100,
        Scale75,
        Scale50,
    }

    /// Host capture scale in permille for a viewport given in physical pixels.
    pub fn compute_scale_permille(
        host_w: u32,
        host_h: u32,
        viewport_phys_w: u32,
        viewport_phys_h: u32,
        mode: RdScaleMode,
    ) -> u16 {
        match mode {
            RdScaleMode::Native100 => 1000,
            RdScaleMode::Scale75 => 750,
            RdScaleMode::Scale50 => 500,
            RdScaleMode::Fit => {
                if host_w == 0 || host_h == 0 || viewport_phys_w == 0 || viewport_phys_h == 0 {
                    return 1000;
                }
                if host_w <= viewport_phys_w && host_h <= viewport_phys_h {
                    return 1000;
                }
                let ratio = (f64::from(viewport_phys_w) / f64::from(host_w))
                    .min(f64::from(viewport_phys_h) / f64::from(host_h));
                ((ratio * 1000.0).floor() as u16).clamp(250, 1000)
            }
        }
    }

    /// Builds stream option requests with monotonic ids, suppresses duplicates, and
    /// falls back once to native scale when the host rejects a scaled request.
    #[derive(Debug)]
    pub struct StreamController {
        next_request_id: u32,
        scale_mode: RdScaleMode,
        viewport: (u32, u32),
        codec: StreamCodecPreference,
        /// Set after a rejected scaled request; cleared when the user picks a new mode.
        native_fallback: bool,
        last_sent: Option<(u16, u16, StreamCodecPreference, u8, u32)>,
        pending: VecDeque<StreamOptionsRequest>,
        /// An explicit scale/bitrate/display from the toolbar, which outranks the mode.
        scale_override: Option<u16>,
        bitrate: u32,
        display_override: Option<u8>,
    }

    impl StreamController {
        pub fn new(
            scale_mode: RdScaleMode,
            viewport: (u32, u32),
            codec: StreamCodecPreference,
        ) -> Self {
            Self {
                next_request_id: 1,
                scale_mode,
                viewport,
                codec,
                native_fallback: false,
                last_sent: None,
                pending: VecDeque::new(),
                scale_override: None,
                bitrate: 0,
                display_override: None,
            }
        }

        pub fn set_scale_mode(&mut self, mode: RdScaleMode) {
            if mode != self.scale_mode {
                self.scale_mode = mode;
                self.native_fallback = false;
                // An explicit mode choice replaces a pinned scale from the toolbar.
                self.scale_override = None;
            }
        }

        pub fn viewport(&self) -> (u32, u32) {
            self.viewport
        }

        pub fn set_viewport(&mut self, width: u32, height: u32) {
            self.viewport = (width, height);
        }

        pub fn codec(&self) -> StreamCodecPreference {
            self.codec
        }

        pub fn set_codec(&mut self, codec: StreamCodecPreference) {
            self.codec = codec;
        }

        /// The scale last negotiated, for the meta the renderer reads back.
        pub fn scale_permille(&self) -> u16 {
            self.last_sent.map_or(1000, |sent| sent.0)
        }

        /// Pins an explicit host scale, as a toolbar `scalePermille` choice does. It is
        /// reported by the next request and survives until another scale is chosen.
        pub fn set_scale_permille(&mut self, permille: u16) {
            self.native_fallback = false;
            self.scale_override = Some(permille);
        }

        pub fn set_bitrate(&mut self, bitrate: u32) {
            self.bitrate = bitrate;
        }

        pub fn set_display_override(&mut self, display_index: Option<u8>) {
            self.display_override = display_index;
        }

        /// The next request for the current settings, or `None` when it would repeat the
        /// last one sent (unless `force`).
        pub fn next_request(
            &mut self,
            host_w: u32,
            host_h: u32,
            display_index: u8,
            target_fps: u32,
            force: bool,
        ) -> Option<StreamOptionsRequest> {
            let display_index = self.display_override.unwrap_or(display_index);
            let scale_permille = if let Some(permille) = self.scale_override {
                permille
            } else if self.native_fallback {
                1000
            } else {
                let (viewport_w, viewport_h) = self.viewport;
                compute_scale_permille(host_w, host_h, viewport_w, viewport_h, self.scale_mode)
            };
            let fps = stream_fps(target_fps);
            if !force
                && self.last_sent == Some((scale_permille, fps, self.codec, display_index, self.bitrate))
            {
                return None;
            }
            Some(self.issue(StreamOptionsRequest {
                request_id: 0,
                scale_permille,
                fps,
                codec: self.codec,
                display_index,
                bitrate: self.bitrate,
            }))
        }

        fn issue(&mut self, mut request: StreamOptionsRequest) -> StreamOptionsRequest {
            request.request_id = self.next_request_id;
            self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
            self.last_sent = Some((
                request.scale_permille,
                request.fps,
                request.codec,
                request.display_index,
                request.bitrate,
            ));
            if self.pending.len() >= MAX_PENDING_REQUESTS {
                self.pending.pop_front();
            }
            self.pending.push_back(request);
            request
        }

        /// The single native-scale retry for a rejected scaled request; `None` for an
        /// unknown id, a rejected native request or a second scaled reject.
        pub fn on_reject(&mut self, request_id: u32) -> Option<StreamOptionsRequest> {
            let index = self
                .pending
                .iter()
                .position(|request| request.request_id == request_id)?;
            let rejected = self.pending.remove(index)?;
            if rejected.scale_permille >= 1000 || self.native_fallback {
                return None;
            }
            self.native_fallback = true;
            // The rejected scale came from the toolbar; drop it so the retry sticks.
            self.scale_override = None;
            Some(self.issue(StreamOptionsRequest {
                scale_permille: 1000,
                ..rejected
            }))
        }
    }

    /// Sends the current stream options when the host supports them. Returns true when a
    /// request went out. Blocks briefly on the control socket; call off the async reactor.
    pub fn send_stream_options(session: &ClientSession, shared: &ViewerShared, force: bool) -> bool {
        let Ok(state) = session.stream_state() else {
            return false;
        };
        if !state.host_supports_stream_options {
            return false;
        }
        let display_index = state.active.map_or(0, |active| active.display_index);
        let (host_w, host_h) = state
            .displays
            .iter()
            .find(|display| display.index == display_index)
            .map_or((0, 0), |display| (display.width, display.height));
        let request = shared.stream.lock().next_request(
            host_w,
            host_h,
            display_index,
            shared.target_fps(),
            force,
        );
        let Some(request) = request else {
            return false;
        };
        shared
            .telemetry
            .lock()
            .set_scale_permille(request.scale_permille);
        session.request_stream_options(request).is_ok()
    }

    /// `"hevc" | "h264"`, the telemetry vocabulary.
    fn codec_name(codec: CodecKind) -> String {
        format!("{codec:?}").to_ascii_lowercase()
    }

    /// `"software" | "videotoolbox" | "d3d11va" | "vaapi"`; the variants are
    /// feature-gated per platform, so the Debug name avoids a cfg-gated match.
    fn accel_name(accel: HardwareAcceleration) -> String {
        format!("{accel:?}").to_ascii_lowercase()
    }

    fn stream_fps(target_fps: u32) -> u16 {
        target_fps.clamp(1, 1000) as u16
    }

    fn micros(duration: Duration) -> u64 {
        u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
    }

    fn status(frames: &Channel<Response>, state: &str, reason: Option<&str>) {
        let _ = frames.send(Response::new(wire::encode_status_message(state, reason)));
    }

    /// Flags crossing from the receive thread to the decode thread and back.
    struct PipelineSignals {
        /// Codec, size or host session changed: rebuild the decoder from the next keyframe.
        reset_decoder: AtomicBool,
        /// The frame channel is gone; the receive loop ends the session.
        viewer_closed: AtomicBool,
    }

    /// Runs with the runtime's own shared state (no native surface, default refresh rate).
    pub fn run(config: ViewerConfig) {
        let shared = config.runtime.shared.clone();
        run_with(config, shared, ViewerOptions::default());
    }

    /// The `ferryx-rd-recv` body: spawns `ferryx-rd-decode`, runs the session until it
    /// ends or `stop` is set, then closes the queue and joins the decoder before exiting.
    pub fn run_with(config: ViewerConfig, shared: ViewerShared, options: ViewerOptions) {
        let ViewerConfig {
            session,
            pairing,
            loopback_tcp,
            loopback_udp,
            runtime,
            stop,
            on_exit,
        } = config;
        let frames = runtime.frames.clone();
        let session = Arc::new(session);
        let codec_policy = Arc::new(Mutex::new(RdCodecPolicy::new(options.codec_override)));
        let signals = Arc::new(PipelineSignals {
            reset_decoder: AtomicBool::new(false),
            viewer_closed: AtomicBool::new(false),
        });
        let decode = {
            let context = DecodeContext {
                session: Arc::clone(&session),
                frames: frames.clone(),
                shared: shared.clone(),
                signals: Arc::clone(&signals),
                codec_policy: Arc::clone(&codec_policy),
            };
            std::thread::Builder::new()
                .name("ferryx-rd-decode".to_owned())
                .spawn(move || decode_loop(context))
        };
        let reason = match decode {
            Ok(decode) => {
                let reason = run_session(
                    &session,
                    pairing,
                    loopback_tcp,
                    loopback_udp,
                    &runtime,
                    &stop,
                    &shared,
                    &signals,
                    &codec_policy,
                );
                shared.frame_queue.close();
                if decode.join().is_err() {
                    tracing::warn!("remote desktop decode thread panicked");
                }
                reason
            }
            Err(error) => Some(format!("Remote desktop decoder could not start: {error}")),
        };
        shared.frame_queue.close();
        shared.session_handle.lock().take();
        let _ = session.disconnect();
        shared.audio.lock().stop();
        status(&frames, "ended", reason.as_deref());
        on_exit();
    }

    fn apply_remote_clipboard(session: &ClientSession, shared: &ViewerShared, text: &str) {
        let allow = session.clipboard_allowed().unwrap_or(false);
        if !allow {
            return;
        }
        shared
            .clipboard_bridge
            .lock()
            .get_or_insert_with(RdClipboardBridge::new)
            .apply_remote_text(text, allow);
    }

    /// Restarts the decode chain after a host session change: fresh queue (the host
    /// restarts frame ids), fresh decoder, fresh codec latch and a size re-announce.
    fn restart_pipeline(
        shared: &ViewerShared,
        signals: &PipelineSignals,
        policy: &Mutex<RdCodecPolicy>,
    ) {
        shared.frame_queue.reset();
        signals.reset_decoder.store(true, Ordering::SeqCst);
        policy.lock().reset_for_reconnect();
        shared.surface_slot.lock().presented.reset();
    }

    #[allow(clippy::too_many_arguments)]
    fn run_session(
        session: &Arc<ClientSession>,
        pairing: PairingRecord,
        loopback_tcp: u16,
        loopback_udp: u16,
        runtime: &ViewerRuntime,
        stop: &AtomicBool,
        shared: &ViewerShared,
        signals: &PipelineSignals,
        codec_policy: &Mutex<RdCodecPolicy>,
    ) -> Option<String> {
        let frames = &runtime.frames;
        status(frames, "connecting", None);
        let ready = match session.connect_with_pairing_via(
            pairing,
            "127.0.0.1",
            loopback_tcp,
            loopback_udp,
        ) {
            Ok(ready) => ready,
            Err(error) => return Some(format!("Remote desktop could not connect: {error}")),
        };
        tracing::info!(host = %ready.server.name, "remote desktop session ready");
        *runtime.host_name.lock() = ready.server.name.clone();
        let mut tcp_runtime = match session.spawn_tcp_runtime() {
            Ok(runtime) => runtime,
            Err(error) => return Some(format!("Remote desktop control channel failed: {error}")),
        };
        if let Err(error) = session.set_udp_read_timeout(Some(Duration::from_millis(20))) {
            let _ = tcp_runtime.stop();
            return Some(format!("Remote desktop media channel failed: {error}"));
        }
        // The session drives ABR from authenticated receive ingress; only its bounds are set.
        if let Err(error) = session.configure_abr(ABR_MIN_BITRATE, ABR_MAX_BITRATE) {
            tracing::debug!(%error, "remote desktop ABR bounds rejected");
        }
        *shared.session_handle.lock() = Some(Arc::clone(session));
        let initial_codec = codec_policy.lock().initial_preference();
        if initial_codec != StreamCodecPreference::Auto {
            shared.stream.lock().set_codec(initial_codec);
        }
        send_stream_options(session, shared, true);
        let _ = session.send_control(ControlMessage::RequestKeyFrame);
        status(frames, "connected", None);
        runtime.emit_meta(session);

        let mut cursor_encoder = RdCursorEncoder::default();
        let mut last_frame_id: Option<u32> = None;
        let mut last_meta_emit = Instant::now();
        let mut reconnecting = false;
        let mut ended: Option<String> = None;

        while !stop.load(Ordering::SeqCst) && ended.is_none() {
            if signals.viewer_closed.load(Ordering::SeqCst) {
                ended = Some("The viewer closed".to_owned());
                break;
            }
            let mut meta_dirty = false;
            while let Ok(event) = tcp_runtime.events().try_recv() {
                match event {
                    Ok(SessionEvent::StreamConfig(ControlMessage::SessionEnded(end))) => {
                        ended = Some(format!("The host ended the session ({:?})", end.reason));
                    }
                    Ok(SessionEvent::StreamConfig(ControlMessage::StreamOptionsActive(active))) => {
                        // Codec or size changed: the next keyframe builds a fresh decoder.
                        signals.reset_decoder.store(true, Ordering::SeqCst);
                        shared.frame_queue.reconfigure_fps(u32::from(active.fps));
                        meta_dirty = true;
                    }
                    Ok(SessionEvent::StreamConfig(ControlMessage::StreamConfigReject(reject))) => {
                        let retry = shared.stream.lock().on_reject(reject.request_id);
                        match retry {
                            Some(request) => {
                                tracing::info!(
                                    request_id = reject.request_id,
                                    reason = ?reject.reason,
                                    "remote desktop host rejected scaling; retrying at native scale"
                                );
                                shared.telemetry.lock().set_scale_permille(1000);
                                let _ = session.request_stream_options(request);
                            }
                            None => tracing::debug!(
                                request_id = reject.request_id,
                                reason = ?reject.reason,
                                "remote desktop stream options rejected"
                            ),
                        }
                    }
                    Ok(SessionEvent::StreamConfig(
                        ControlMessage::HostDisplayList(_)
                        | ControlMessage::GuestPermissions(_)
                        | ControlMessage::HostPolicy(_),
                    )) => {
                        // Host size is known now, so a Fit scale can be computed.
                        send_stream_options(session, shared, false);
                        meta_dirty = true;
                    }
                    Ok(SessionEvent::Clipboard(text)) => {
                        apply_remote_clipboard(session, shared, &text);
                        if frames
                            .send(Response::new(wire::encode_clipboard_message(&text)))
                            .is_err()
                        {
                            ended = Some("The viewer closed".to_owned());
                        }
                    }
                    Ok(_) => {}
                    Err(error) => ended = Some(format!("Remote desktop connection lost: {error}")),
                }
            }
            let now_reconnecting = matches!(
                session.session_info().map(|info| info.reconnect),
                Ok(ReconnectState::Reconnecting { .. })
            );
            if now_reconnecting != reconnecting {
                reconnecting = now_reconnecting;
                status(
                    frames,
                    if reconnecting {
                        "reconnecting"
                    } else {
                        "connected"
                    },
                    None,
                );
                restart_pipeline(shared, signals, codec_policy);
                last_frame_id = None;
                if !reconnecting {
                    // A new host session starts from its defaults; restate ours.
                    send_stream_options(session, shared, true);
                    let _ = session.send_control(ControlMessage::RequestKeyFrame);
                    meta_dirty = true;
                }
            }
            if meta_dirty || last_meta_emit.elapsed() >= Duration::from_secs(2) {
                runtime.emit_meta(session);
                last_meta_emit = Instant::now();
            }

            match session.receive_udp_event() {
                Ok(SessionEvent::Frame(frame)) => {
                    let id = frame.header.frame_id;
                    let advance = last_frame_id.map(|last| id.wrapping_sub(last));
                    if advance.is_none_or(|advance| advance != 0 && advance < (1 << 31)) {
                        if let Some(gap) = advance.filter(|advance| *advance > 1) {
                            shared.telemetry.lock().record_dropped(u64::from(gap - 1));
                        }
                        last_frame_id = Some(id);
                    }
                    match shared.frame_queue.push((frame, Instant::now())) {
                        Some(true) => {
                            let _ = session.send_control(ControlMessage::RequestKeyFrame);
                        }
                        Some(false) => {}
                        None => break,
                    }
                }
                Ok(SessionEvent::Audio(pcm)) => {
                    let (active, underruns, drops, muted) = {
                        let mut audio = shared.audio.lock();
                        audio.push_pcm(&pcm);
                        let (active, underruns, drops) = audio.stats();
                        (active, underruns, drops, audio.is_muted())
                    };
                    shared
                        .telemetry
                        .lock()
                        .set_audio_stats(active, underruns, drops);
                    if !shared.audio_available.swap(true, Ordering::Relaxed) {
                        runtime.emit_meta(session);
                    }
                    // The local sink owns playback once the device opened; a headless host
                    // has no sink, so the same PCM rides the frame channel to the WebView.
                    if !active && !muted && !pcm.is_empty() {
                        let message = wire::encode_audio_message(
                            wire::AUDIO_SAMPLE_RATE,
                            wire::AUDIO_CHANNELS,
                            &pcm,
                        );
                        if frames.send(Response::new(message)).is_err() {
                            ended = Some("The viewer closed".to_owned());
                        }
                    }
                }
                Ok(SessionEvent::Cursor(cursor)) => {
                    let message = cursor_encoder
                        .encode(&cursor, |id| session.cursor_shape(id).ok().flatten());
                    if frames.send(Response::new(message)).is_err() {
                        ended = Some("The viewer closed".to_owned());
                    }
                }
                Ok(_) => {}
                Err(ferryx_rd_app::SessionError::Io(error))
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) => {}
                Err(error) => {
                    if reconnecting || ferryx_rd_app::should_reconnect(&error) {
                        // The TCP runtime owns recovery; wait for it instead of failing.
                        std::thread::sleep(Duration::from_millis(50));
                        continue;
                    }
                    ended = Some(format!("Remote desktop media failed: {error}"));
                }
            }
        }
        let _ = tcp_runtime.stop();
        ended
    }

    struct DecodeContext {
        session: Arc<ClientSession>,
        frames: Channel<Response>,
        shared: ViewerShared,
        signals: Arc<PipelineSignals>,
        codec_policy: Arc<Mutex<RdCodecPolicy>>,
    }

    enum Presented {
        /// Drawn natively or sent as JPEG.
        Yes,
        /// JPEG pacing skipped this picture.
        Skipped,
        ChannelClosed,
    }

    enum NativeOutcome {
        NotAttached,
        Presented(Option<(u32, u32)>),
        Failed,
    }

    /// Drops dependents of a failed decode and asks for a keyframe when none is queued.
    fn recover(session: &ClientSession, queue: &FrameQueue) {
        if !matches!(queue.decode_failed(), Ok(false)) {
            let _ = session.send_control(ControlMessage::RequestKeyFrame);
        }
    }

    fn present_native(shared: &ViewerShared, picture: &DecodedVideoFrame) -> NativeOutcome {
        let (outcome, failed) = {
            let mut guard = shared.surface_slot.lock();
            let slot = &mut *guard;
            let Some(surface) = slot.surface.as_mut() else {
                return NativeOutcome::NotAttached;
            };
            match surface.present_frame(picture) {
                Ok((width, height)) => {
                    let announce = slot.presented.observe(width, height);
                    (NativeOutcome::Presented(announce.then_some((width, height))), None)
                }
                Err(error) => {
                    tracing::warn!(%error, "remote desktop native surface failed; using canvas");
                    slot.presented.reset();
                    (NativeOutcome::Failed, slot.surface.take())
                }
            }
        };
        drop(failed);
        outcome
    }

    fn decode_loop(context: DecodeContext) {
        let DecodeContext {
            session,
            frames,
            shared,
            signals,
            codec_policy,
        } = context;
        let mut decoder: Option<(CodecKind, HevcDecoder)> = None;
        let mut reported_backend: Option<(CodecKind, HardwareAcceleration)> = None;
        let mut announced_ready = false;
        let mut native_backend = false;
        let mut last_jpeg: Option<Instant> = None;

        while let Some((queue, (frame, received))) = shared.frame_queue.next_frame() {
            if signals.reset_decoder.swap(false, Ordering::SeqCst) || shared.retry_renderer.swap(false, Ordering::SeqCst) {
                decoder = None;
            }
            let codec_changed = frame.header.is_key_frame
                && decoder.as_ref().is_some_and(|(kind, _)| {
                    detect_codec(&frame.data).is_ok_and(|detected| detected != *kind)
                });
            if decoder.is_none() || codec_changed {
                match HevcDecoder::from_keyframe_auto(&frame.data) {
                    Ok(built) => decoder = Some(built),
                    Err(_) => {
                        recover(&session, &queue);
                        continue;
                    }
                }
            }
            let Some((codec, dec)) = decoder.as_mut() else {
                continue;
            };
            let codec = *codec;
            let decode_started = Instant::now();
            let (decoded, accel) =
                match dec.decode_with_color(&frame.data, i64::from(frame.timestamp_ms)) {
                    Ok(decoded) => (decoded, dec.acceleration()),
                    Err(error) => {
                        tracing::debug!(%error, "remote desktop decode failed");
                        decoder = None;
                        recover(&session, &queue);
                        continue;
                    }
                };
            let decode_us = micros(decode_started.elapsed());
            let Some(picture) = decoded.into_iter().last() else {
                continue;
            };
            *shared.last_frame_size.lock() =
                (picture.frame.width, picture.frame.height);
            if reported_backend != Some((codec, accel)) {
                reported_backend = Some((codec, accel));
                let accel = accel_name(accel);
                let _ = session.set_decoder_backend(accel.clone());
                shared
                    .telemetry
                    .lock()
                    .set_codec_and_accel(&codec_name(codec), &accel);
            }
            let action = codec_policy.lock().observe_decoded(codec, accel);
            if action == CodecAction::DowngradeToH264 {
                let downgrade = {
                    let mut stream = shared.stream.lock();
                    let auto = stream.codec() == StreamCodecPreference::Auto;
                    if auto {
                        stream.set_codec(StreamCodecPreference::H264);
                    }
                    auto
                };
                if downgrade {
                    send_stream_options(&session, &shared, false);
                }
            }
            if !announced_ready {
                announced_ready = true;
                session.video_ready();
            }

            let present_started = Instant::now();
            let mut presented = None;
            match if shared.canvas_presentation.load(Ordering::SeqCst) { NativeOutcome::NotAttached } else { present_native(&shared, &picture) } {
                NativeOutcome::Presented(announce) => {
                    if !native_backend {
                        native_backend = true;
                        shared.telemetry.lock().set_present_backend("wgpu");
                    }
                    presented = Some(match announce {
                        Some((width, height)) => {
                            let message = wire::encode_native_presented_frame(width, height);
                            if frames.send(Response::new(message)).is_err() {
                                Presented::ChannelClosed
                            } else {
                                Presented::Yes
                            }
                        }
                        None => Presented::Yes,
                    });
                }
                NativeOutcome::Failed => {
                    native_backend = false;
                    shared.telemetry.lock().set_present_backend("canvas2d");
                }
                NativeOutcome::NotAttached => {
                    if native_backend {
                        native_backend = false;
                        shared.telemetry.lock().set_present_backend("canvas2d");
                    }
                }
            }
            let presented = presented.unwrap_or_else(|| {
                // JPEG pacing follows the window's live refresh rate.
                let jpeg_interval = Duration::from_secs(1) / shared.present_fps();
                if last_jpeg.is_some_and(|at| at.elapsed() < jpeg_interval) {
                    return Presented::Skipped;
                }
                match ferryx_rd_app::nv12_frame_to_jpeg(
                        &picture.frame,
                        shared.jpeg_quality.load(Ordering::Relaxed).clamp(30, 95) as u8,
                    ) {
                    Ok(jpeg) => {
                        let message = wire::encode_video_message(
                            picture.frame.width,
                            picture.frame.height,
                            u64::from(frame.header.frame_id),
                            &jpeg,
                        );
                        if frames.send(Response::new(message)).is_err() {
                            return Presented::ChannelClosed;
                        }
                        last_jpeg = Some(Instant::now());
                        Presented::Yes
                    }
                    Err(error) => {
                        tracing::debug!(%error, "remote desktop jpeg encode failed");
                        Presented::Skipped
                    }
                }
            });
            match presented {
                Presented::Yes => {
                    let now = Instant::now();
                    shared.telemetry.lock().record_frame_at(
                        decode_us,
                        micros(now.saturating_duration_since(present_started)),
                        micros(now.saturating_duration_since(received)),
                        picture.frame.width,
                        picture.frame.height,
                        now,
                    );
                    let _ = session.frame_presented(frame.header.frame_id);
                }
                Presented::Skipped => {}
                Presented::ChannelClosed => {
                    signals.viewer_closed.store(true, Ordering::SeqCst);
                    break;
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::sync::mpsc;

        fn frame(id: u32, key: bool) -> QueuedFrame {
            (
                ferryx_rd_app::AssembledFrame {
                    header: ferryx_rd_proto::FrameHeader {
                        frame_id: id,
                        width: 2,
                        height: 2,
                        is_key_frame: key,
                        total_chunks: 1,
                        total_size: 1,
                    },
                    data: vec![1],
                    timestamp_ms: id,
                },
                Instant::now(),
            )
        }

        fn next_id(slot: &FrameQueueSlot) -> u32 {
            let (_, (frame, _)) = slot.next_frame().expect("a queued frame");
            frame.header.frame_id
        }

        #[test]
        fn queue_slot_reconfigures_playout_and_survives_reconnect_reset() {
            let slot = FrameQueueSlot::new(FrameQueueConfig::for_fps(75, PlayoutMode::Immediate));
            assert_eq!(slot.push(frame(0, true)), Some(false));
            assert_eq!(next_id(&slot), 0);

            slot.set_playout_mode(PlayoutMode::Adaptive);
            assert_eq!(slot.config().playout, PlayoutMode::Adaptive);
            let live = slot.current().unwrap();
            assert_eq!(live.config().unwrap().playout, PlayoutMode::Adaptive);

            slot.set_playout_mode(PlayoutMode::LatestWins);
            slot.reconfigure_fps(240);
            let config = live.config().unwrap();
            assert_eq!(config.playout, PlayoutMode::LatestWins);
            assert_eq!(config.frame_interval, Duration::from_secs(1) / 240);
            // Latest-wins skips the stale P frame up to the newest keyframe.
            for (id, key) in [(1, false), (2, true), (3, false)] {
                assert_eq!(slot.push(frame(id, key)), Some(false));
            }
            assert_eq!(next_id(&slot), 2);
            assert_eq!(next_id(&slot), 3);

            // A reconnected host restarts its ids; the old queue would call them late.
            assert!(!live.push(frame(1, true)).unwrap());
            assert!(live.recv_timeout(Duration::ZERO).is_err());
            assert!(slot.reset());
            assert!(live.push(frame(4, false)).is_err(), "old queue is stopped");
            assert_eq!(slot.push(frame(1, true)), Some(false));
            assert_eq!(next_id(&slot), 1);
            assert_eq!(slot.config().playout, PlayoutMode::LatestWins);
        }

        #[test]
        fn closing_unblocks_an_idle_decoder_within_200ms() {
            let slot = Arc::new(FrameQueueSlot::new(FrameQueueConfig::for_fps(
                75,
                PlayoutMode::Immediate,
            )));
            let (entered_tx, entered_rx) = mpsc::channel();
            let (done_tx, done_rx) = mpsc::channel();
            let decoder = {
                let slot = Arc::clone(&slot);
                std::thread::spawn(move || {
                    entered_tx.send(()).unwrap();
                    done_tx.send(slot.next_frame().is_none()).unwrap();
                })
            };
            entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            slot.close();
            assert_eq!(done_rx.recv_timeout(Duration::from_millis(200)), Ok(true));
            decoder.join().unwrap();
            assert!(slot.current().is_none());
            assert_eq!(slot.push(frame(0, true)), None);
            assert!(!slot.reset(), "a closed slot stays closed");
            assert!(slot.next_frame().is_none());
        }

        #[test]
        fn reset_moves_a_parked_decoder_onto_the_fresh_queue() {
            let slot = Arc::new(FrameQueueSlot::new(FrameQueueConfig::for_fps(
                75,
                PlayoutMode::Immediate,
            )));
            let (entered_tx, entered_rx) = mpsc::channel();
            let (done_tx, done_rx) = mpsc::channel();
            let decoder = {
                let slot = Arc::clone(&slot);
                std::thread::spawn(move || {
                    entered_tx.send(()).unwrap();
                    let id = slot.next_frame().map(|(_, frame)| frame.0.header.frame_id);
                    done_tx.send(id).unwrap();
                })
            };
            entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            assert!(slot.reset());
            assert_eq!(slot.push(frame(7, true)), Some(false));
            assert_eq!(done_rx.recv_timeout(Duration::from_secs(2)), Ok(Some(7)));
            decoder.join().unwrap();
            slot.close();
        }

        #[test]
        fn native_presented_size_is_announced_on_first_frame_change_and_reattach() {
            let mut slot = SurfaceSlot::default();
            assert_eq!(slot.presented.last(), None);
            assert!(slot.presented.observe(1920, 1080));
            assert!(!slot.presented.observe(1920, 1080), "same size is suppressed");
            assert!(slot.presented.observe(1280, 720), "a live rescale announces");
            assert!(!slot.presented.observe(1280, 720));
            // Reattach / reconnect reset the latch, so the same size announces again.
            slot.presented.reset();
            assert!(slot.presented.observe(1280, 720));
            assert_eq!(slot.presented.last(), Some((1280, 720)));
            assert!(slot.surface.is_none());
        }

        #[test]
        fn telemetry_names_match_the_stats_vocabulary() {
            assert_eq!(codec_name(CodecKind::Hevc), "hevc");
            assert_eq!(codec_name(CodecKind::H264), "h264");
            assert_eq!(accel_name(HardwareAcceleration::Software), "software");
            assert_eq!(playout_name(PlayoutMode::LatestWins), "latest_wins");
            assert_eq!(stream_fps(75), 75);
            assert_eq!(stream_fps(0), 1);
        }

        #[test]
        fn scale_permille_follows_mode_and_fits_the_viewport() {
            assert_eq!(compute_scale_permille(5120, 2880, 1920, 1080, RdScaleMode::Fit), 375);
            assert_eq!(compute_scale_permille(1920, 1080, 2560, 1440, RdScaleMode::Fit), 1000);
            assert_eq!(compute_scale_permille(0, 0, 1920, 1080, RdScaleMode::Fit), 1000);
            assert_eq!(compute_scale_permille(1920, 1080, 0, 0, RdScaleMode::Fit), 1000);
            // A tiny viewport is clamped to the host's lower bound.
            assert_eq!(compute_scale_permille(7680, 4320, 640, 360, RdScaleMode::Fit), 250);
            assert_eq!(compute_scale_permille(5120, 2880, 1920, 1080, RdScaleMode::Scale75), 750);
            assert_eq!(compute_scale_permille(5120, 2880, 1920, 1080, RdScaleMode::Scale50), 500);
            assert_eq!(compute_scale_permille(5120, 2880, 1920, 1080, RdScaleMode::Native100), 1000);
            let modes: Vec<RdScaleMode> =
                serde_json::from_value(serde_json::json!(["fit", "native100", "scale75", "scale50"]))
                    .unwrap();
            assert_eq!(
                modes,
                [RdScaleMode::Fit, RdScaleMode::Native100, RdScaleMode::Scale75, RdScaleMode::Scale50]
            );
        }

        #[test]
        fn a_rejected_scaled_request_retries_once_at_native_scale() {
            let mut stream =
                StreamController::new(RdScaleMode::Scale50, (1920, 1080), StreamCodecPreference::Auto);
            let first = stream.next_request(3840, 2160, 0, 75, true).unwrap();
            assert_eq!(first.scale_permille, 500);
            let retry = stream.on_reject(first.request_id).expect("one native retry");
            assert_eq!(retry.scale_permille, 1000);
            assert!(retry.request_id > first.request_id);
            assert_eq!((retry.fps, retry.codec), (first.fps, first.codec));
            // The native request is rejected too: no further retry, no loop.
            assert_eq!(stream.on_reject(retry.request_id), None);
            assert_eq!(stream.on_reject(retry.request_id), None);
            assert_eq!(stream.on_reject(9999), None, "unknown ids are ignored");
            // The fallback sticks until the user picks another mode.
            let next = stream.next_request(3840, 2160, 0, 75, true).unwrap();
            assert_eq!(next.scale_permille, 1000);
            assert_eq!(stream.on_reject(next.request_id), None);
            stream.set_scale_mode(RdScaleMode::Scale75);
            assert_eq!(stream.next_request(3840, 2160, 0, 75, false).unwrap().scale_permille, 750);
        }

        #[test]
        fn unchanged_options_are_not_resent_unless_forced() {
            let mut stream =
                StreamController::new(RdScaleMode::Fit, (1920, 1080), StreamCodecPreference::Auto);
            let first = stream.next_request(3840, 2160, 0, 60, false).unwrap();
            assert_eq!(first.scale_permille, 500);
            assert_eq!(stream.next_request(3840, 2160, 0, 60, false), None);
            let forced = stream.next_request(3840, 2160, 0, 60, true).unwrap();
            assert_eq!(forced.request_id, first.request_id + 1);
            stream.set_viewport(3840, 2160);
            assert_eq!(stream.next_request(3840, 2160, 0, 60, false).unwrap().scale_permille, 1000);
        }

        #[test]
        fn refresh_change_reconfigures_the_queue_and_requests_the_new_rate() {
            let shared = ViewerShared::new("s", 60, PlayoutMode::Immediate);
            assert_eq!(shared.target_fps(), 60);
            let at_60 = shared
                .stream
                .lock()
                .next_request(1920, 1080, 0, shared.target_fps(), true)
                .unwrap();
            assert_eq!(at_60.fps, 60);

            assert!(shared.apply_refresh(75));
            assert!(!shared.apply_refresh(75), "an unchanged rate is not a transition");
            assert_eq!(shared.target_fps(), 75);
            let interval = shared.frame_queue.config().frame_interval;
            assert_eq!(interval.as_micros(), 13_333);
            assert_eq!(
                shared.frame_queue.current().unwrap().config().unwrap().frame_interval,
                interval
            );
            assert_eq!(shared.telemetry.lock().snapshot(None, Instant::now()).target_fps, 75);
            let at_75 = shared
                .stream
                .lock()
                .next_request(1920, 1080, 0, shared.target_fps(), false)
                .expect("a new rate is a new request");
            assert_eq!(at_75.fps, 75);
            assert!(at_75.request_id > at_60.request_id);
        }
    }
}

/// Registered in `lib.rs` by full path so tauri's per-command macros resolve.
#[cfg(feature = "remote-desktop")]
pub mod commands {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use ferryx_rd_app::frame_queue::PlayoutMode;
    use ferryx_rd_app::{ClientSession, PairingStore, SessionConfig};
    use ferryx_rd_proto::{ControlMessage, InputEvent, StreamCodecPreference};
    use parking_lot::Mutex;
    use serde::{Deserialize, Serialize};
    use tauri::ipc::{Channel, Response};

    use super::transport::{
        self, rd_error, RD_CONNECT_FAILED, RD_NOT_FOUND, RD_PERMISSION_REQUIRED, RD_UNSUPPORTED,
    };
    use super::viewer::{self, RdScaleMode, ViewerRuntime};
    use super::wire::{
        self, InputMapper, RdInputEvent, RdSessionMeta, RdSpecialKeyAction, RdStreamOptions,
    };
    use crate::ipc::display_refresh::{self, DisplayRefreshInfo};
    use crate::ipc::error::IpcError;
    use crate::ipc::remote_desktop_stats::RdSessionStats;
    use crate::ipc::remote_desktop_surface::RdNativeSurface;
    use crate::paired_host::client::MachineClient;
    use crate::paired_host::inventory::{AuthStatus, GrantScope};
    use crate::paired_host::service::PairedHostService;

    const PAIRING_FILE: &str = "remote-desktop-pairings.json";
    const MAX_EVENTS_PER_CALL: usize = 512;
    const RD_INVALID_OPTIONS: &str = "RD_INVALID_OPTIONS";

    /// Optional `cmd_rd_connect` settings; every field may be omitted.
    #[derive(Debug, Clone, Default, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct RdConnectOptions {
        pub playout_mode: Option<PlayoutMode>,
        pub scale_mode: Option<RdScaleMode>,
        pub viewport_width: Option<u32>,
        pub viewport_height: Option<u32>,
        pub audio_muted: Option<bool>,
        pub audio_volume: Option<f32>,
    }

    /// `cmd_rd_set_options` changes; omitted fields keep their current value.
    #[derive(Debug, Clone, Default, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct RdStreamOptionsUpdate {
        pub playout_mode: Option<PlayoutMode>,
        pub scale_mode: Option<RdScaleMode>,
        pub viewport_width: Option<u32>,
        pub viewport_height: Option<u32>,
        /// `"auto" | "hevc" | "h264"`.
        pub codec: Option<String>,
        pub clipboard_enabled: Option<bool>,
        pub canvas_presentation: Option<bool>,
        pub retry_renderer: Option<bool>,
    }

    /// The renderer's `RdLogicalBounds` (`LogicalBounds` itself has no serde shape).
    #[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct RdLogicalBounds {
        pub x: f64,
        pub y: f64,
        pub width: f64,
        pub height: f64,
        pub scale_factor: f64,
    }

    impl From<RdLogicalBounds> for crate::native_terminal::LogicalBounds {
        fn from(bounds: RdLogicalBounds) -> Self {
            Self {
                x: bounds.x,
                y: bounds.y,
                width: bounds.width,
                height: bounds.height,
                scale_factor: bounds.scale_factor,
            }
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct RdSurfaceAttachResponse {
        /// `"wgpu" | "canvas2d"`.
        pub backend: String,
        pub reason: Option<String>,
    }

    fn parse_codec(codec: &str) -> Result<StreamCodecPreference, IpcError> {
        match codec {
            "auto" => Ok(StreamCodecPreference::Auto),
            "hevc" => Ok(StreamCodecPreference::Hevc),
            "h264" => Ok(StreamCodecPreference::H264),
            other => Err(rd_error(
                RD_INVALID_OPTIONS,
                format!("Unknown remote desktop codec: {other}"),
            )),
        }
    }

    fn session_or_not_found(id: &str) -> Result<Arc<RdSession>, IpcError> {
        find_session(id)
            .ok_or_else(|| rd_error(RD_NOT_FOUND, "That remote desktop session has ended"))
    }

    struct RdSession {
        session: ClientSession,
        mapper: Mutex<InputMapper>,
        runtime: Arc<ViewerRuntime>,
        stop: Arc<AtomicBool>,
        bridge_stop: tokio::sync::watch::Sender<bool>,
        /// Telemetry, audio, native surface, frame queue, session handle and clipboard,
        /// shared with the `ferryx-rd-recv` / `ferryx-rd-decode` threads.
        shared: viewer::ViewerShared,
        /// `ferryx-rd-recv`; it joins `ferryx-rd-decode` itself before exiting.
        recv_thread: Mutex<Option<std::thread::JoinHandle<()>>>,
    }

    impl RdSession {
        /// Stops both pipeline threads and releases the presenter and audio. Blocks while
        /// the receive thread winds down, so async callers run it on a blocking thread.
        fn shutdown(&self) {
            self.stop.store(true, Ordering::SeqCst);
            // Wakes a decoder parked on an empty queue.
            self.shared.frame_queue.close();
            self.session.interrupt();
            self.bridge_stop.send_replace(true);
            let recv = self.recv_thread.lock().take();
            if let Some(recv) = recv {
                // on_exit runs on the receive thread itself; it must not join itself.
                if recv.thread().id() != std::thread::current().id()
                    && recv.join().is_err()
                {
                    tracing::warn!("remote desktop receive thread panicked");
                }
            }
            self.shared.detach_surface();
            self.shared.audio.lock().stop();
        }
    }

    static SESSIONS: Mutex<Option<HashMap<String, Arc<RdSession>>>> = Mutex::new(None);

    fn remove_session(id: &str) -> Option<Arc<RdSession>> {
        SESSIONS
            .lock()
            .as_mut()
            .and_then(|sessions| sessions.remove(id))
    }

    fn find_session(id: &str) -> Option<Arc<RdSession>> {
        SESSIONS
            .lock()
            .as_ref()
            .and_then(|sessions| sessions.get(id).cloned())
    }

    /// The GUI reads the same paired-host inventory the daemon owns, under the same data dir.
    fn data_dir() -> Result<PathBuf, IpcError> {
        crate::remote::auth::canonical_identity_dir()
            .map_err(|message| rd_error(RD_CONNECT_FAILED, message))
    }

    fn now_unix_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis() as u64)
            .unwrap_or(0)
    }

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct RdConnectResponse {
        pub rd_session_id: String,
    }

    /// Re-sends the stream options of a connected session on a blocking thread (the
    /// control socket write may wait). No-op before the session is connected.
    async fn renegotiate(shared: &viewer::ViewerShared, force: bool) -> Result<(), IpcError> {
        let session = shared.session_handle.lock().clone();
        let Some(session) = session else {
            return Ok(());
        };
        let shared = shared.clone();
        crate::ipc::run_blocking(move || {
            viewer::send_stream_options(&session, &shared, force);
            Ok(())
        })
        .await
    }

    /// Follows the viewing window's monitor: a refresh transition retargets presentation
    /// and asks the host for the new rate. Ends with the session's bridge stop signal.
    fn spawn_refresh_watcher(
        window_label: String,
        shared: viewer::ViewerShared,
        mut stopped: tokio::sync::watch::Receiver<bool>,
    ) {
        let mut changes = display_refresh::global_refresh_hub().subscribe();
        tokio::spawn(async move {
            use tokio::sync::broadcast::error::RecvError;
            loop {
                if *stopped.borrow_and_update() {
                    return;
                }
                let change = tokio::select! {
                    biased;
                    changed = stopped.changed() => {
                        if changed.is_err() {
                            return;
                        }
                        continue;
                    }
                    change = changes.recv() => change,
                };
                let change = match change {
                    Ok(change) => change,
                    Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => return,
                };
                if change.window_label != window_label || !shared.apply_refresh(change.refresh_hz) {
                    continue;
                }
                tracing::info!(refresh_hz = change.refresh_hz, "remote desktop follows refresh change");
                if let Err(error) = renegotiate(&shared, false).await {
                    tracing::debug!(%error, "remote desktop refresh renegotiation failed");
                }
            }
        });
    }

    #[tauri::command]
    pub async fn cmd_rd_connect<R: tauri::Runtime>(
        window: tauri::WebviewWindow<R>,
        host_id: String,
        options: Option<RdConnectOptions>,
        frames: Channel<Response>,
    ) -> Result<RdConnectResponse, IpcError> {
        let options = options.unwrap_or_default();
        // The AppKit/GTK query may wait on the main thread; keep it off the reactor.
        let refresh_window = window.clone();
        let refresh = crate::ipc::run_blocking(move || {
            Ok(display_refresh::query_window_refresh_rate(&refresh_window))
        })
        .await?;
        // Seeds the hub so the next monitor switch is a transition the watcher sees.
        if let Some(change) =
            display_refresh::global_refresh_hub().observe(window.label(), refresh.clone())
        {
            use tauri::Emitter as _;
            let _ = window.emit("rd-display-refresh-changed", change);
        }
        let dir = data_dir()?;
        let service = PairedHostService::open(dir.clone());
        let host = service
            .list()
            .await
            .map_err(|error| rd_error(RD_CONNECT_FAILED, error.message))?
            .into_iter()
            .find(|host| host.host_id == host_id)
            .ok_or_else(|| rd_error(RD_NOT_FOUND, "That paired host is not known"))?;
        if host.grant_scope != GrantScope::Machine || host.auth_status != AuthStatus::Paired {
            return Err(rd_error(
                RD_PERMISSION_REQUIRED,
                "Remote desktop needs a current Machine pairing with control access",
            ));
        }
        let client = Arc::new(MachineClient::new());
        let body = client
            .remote_desktop_pair(&service, host_id.clone(), host.generation)
            .await
            .map_err(|error| transport::map_client_error(&error))?;
        let pairing = transport::pairing_record(&body, now_unix_ms())?;
        let store_path = dir.join(PAIRING_FILE);
        let record = pairing.clone();
        let path = store_path.clone();
        crate::ipc::run_blocking(move || {
            PairingStore::new(path).save(record).map_err(|error| {
                rd_error(
                    RD_CONNECT_FAILED,
                    format!("Could not store the remote desktop pairing: {error}"),
                )
            })
        })
        .await?;

        let first = client
            .remote_desktop_stream_with_transport(&service, host_id.clone(), host.generation)
            .await
            .map_err(|error| transport::map_client_error(&error))?;
        let initial_route = first.transport.route().as_str();
        let local = |error: std::io::Error| {
            rd_error(
                RD_CONNECT_FAILED,
                format!("Remote desktop loopback failed: {error}"),
            )
        };
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(local)?;
        let udp = tokio::net::UdpSocket::bind(("127.0.0.1", 0))
            .await
            .map_err(local)?;
        let loopback_tcp = listener.local_addr().map_err(local)?.port();
        let loopback_udp = udp.local_addr().map_err(local)?.port();

        let mut config = SessionConfig::direct("127.0.0.1", "Ferryx");
        config.tcp_port = loopback_tcp;
        config.udp_port = loopback_udp;
        config.pairing_store_path = Some(store_path);
        let session = ClientSession::new(config)
            .map_err(|error| rd_error(RD_CONNECT_FAILED, error.to_string()))?;

        let rd_session_id = uuid::Uuid::new_v4().to_string();
        let (bridge_stop, bridge_stopped) = tokio::sync::watch::channel(false);
        let stop = Arc::new(AtomicBool::new(false));
        let shared = viewer::ViewerShared::new(
            &rd_session_id,
            refresh.refresh_hz,
            options.playout_mode.unwrap_or(PlayoutMode::Immediate),
        );
        {
            let mut stream = shared.stream.lock();
            if let Some(mode) = options.scale_mode {
                stream.set_scale_mode(mode);
            }
            if let (Some(width), Some(height)) = (options.viewport_width, options.viewport_height) {
                stream.set_viewport(width, height);
            }
        }
        if options.audio_muted.is_some() || options.audio_volume.is_some() {
            shared.audio.lock().set_audio(
                options.audio_muted.unwrap_or(false),
                options.audio_volume.unwrap_or(1.0),
            );
        }
        spawn_refresh_watcher(window.label().to_owned(), shared.clone(), bridge_stop.subscribe());
        let runtime = Arc::new(ViewerRuntime::new(initial_route, frames, shared.clone()));
        let entry = Arc::new(RdSession {
            session: session.clone(),
            mapper: Mutex::new(InputMapper::default()),
            runtime: Arc::clone(&runtime),
            stop: Arc::clone(&stop),
            bridge_stop,
            shared: shared.clone(),
            recv_thread: Mutex::new(None),
        });
        SESSIONS
            .lock()
            .get_or_insert_with(HashMap::new)
            .insert(rd_session_id.clone(), Arc::clone(&entry));

        let route_runtime = Arc::clone(&runtime);
        let route_session = session.clone();
        tokio::spawn(transport::start_bridge(
            listener,
            udp,
            first,
            transport::StreamSource {
                client,
                service,
                host_id,
                generation: host.generation,
                on_route: Some(Arc::new(move |route| {
                    route_runtime.set_route(route.as_str());
                    route_runtime.emit_meta(&route_session);
                })),
            },
            bridge_stopped,
            Some(Arc::clone(&shared.telemetry)),
        ));
        let exit_id = rd_session_id.clone();
        let spawned = std::thread::Builder::new()
            .name("ferryx-rd-recv".to_owned())
            .spawn(move || {
                viewer::run_with(
                    viewer::ViewerConfig {
                        session,
                        pairing,
                        loopback_tcp,
                        loopback_udp,
                        runtime,
                        stop,
                        on_exit: Box::new(move || {
                            if let Some(entry) = remove_session(&exit_id) {
                                entry.shutdown();
                            }
                        }),
                    },
                    shared,
                    viewer::ViewerOptions::default(),
                )
            });
        match spawned {
            Ok(recv) => *entry.recv_thread.lock() = Some(recv),
            Err(error) => {
                if let Some(entry) = remove_session(&rd_session_id) {
                    entry.shutdown();
                }
                return Err(rd_error(
                    RD_CONNECT_FAILED,
                    format!("Could not start the remote desktop session: {error}"),
                ));
            }
        }
        Ok(RdConnectResponse { rd_session_id })
    }

    #[tauri::command]
    pub async fn cmd_rd_input(
        rd_session_id: String,
        events: Vec<RdInputEvent>,
    ) -> Result<(), IpcError> {
        let entry = find_session(&rd_session_id)
            .ok_or_else(|| rd_error(RD_NOT_FOUND, "That remote desktop session has ended"))?;
        let mapped: Vec<InputEvent> = {
            let mut mapper = entry.mapper.lock();
            events
                .iter()
                .take(MAX_EVENTS_PER_CALL)
                .filter_map(|event| mapper.map(event))
                .collect()
        };
        if mapped.is_empty() {
            return Ok(());
        }
        crate::ipc::run_blocking(move || {
            entry
                .session
                .send_inputs(&mapped)
                .map_err(|error| match error {
                    ferryx_rd_app::SessionError::PermissionDenied => rd_error(
                        RD_PERMISSION_REQUIRED,
                        "The host does not allow input from this session",
                    ),
                    other => rd_error(RD_CONNECT_FAILED, other.to_string()),
                })
        })
        .await
    }

    #[tauri::command]
    pub async fn cmd_rd_stats(rd_session_id: String) -> Result<RdSessionStats, IpcError> {
        let entry = session_or_not_found(&rd_session_id)?;
        let receiver = entry.session.receiver_snapshot().ok();
        let stats = entry
            .shared
            .telemetry
            .lock()
            .snapshot(receiver, std::time::Instant::now());
        Ok(stats)
    }

    #[tauri::command]
    pub async fn cmd_rd_display_refresh<R: tauri::Runtime>(
        window: tauri::WebviewWindow<R>,
    ) -> Result<DisplayRefreshInfo, IpcError> {
        crate::ipc::run_blocking(move || Ok(display_refresh::query_window_refresh_rate(&window)))
            .await
    }

    #[tauri::command]
    pub async fn cmd_rd_set_options(
        rd_session_id: String,
        options: RdStreamOptionsUpdate,
    ) -> Result<(), IpcError> {
        let entry = session_or_not_found(&rd_session_id)?;
        let codec = options.codec.as_deref().map(parse_codec).transpose()?;
        let shared = &entry.shared;
        if options.retry_renderer == Some(true) {
            shared.retry_renderer.store(true, Ordering::SeqCst);
        }
        if let Some(canvas) = options.canvas_presentation {
            shared.canvas_presentation.store(canvas, Ordering::SeqCst);
        }
        if let Some(mode) = options.playout_mode {
            shared.set_playout_mode(mode);
        }
        {
            let mut stream = shared.stream.lock();
            if let Some(mode) = options.scale_mode {
                stream.set_scale_mode(mode);
            }
            if options.viewport_width.is_some() || options.viewport_height.is_some() {
                let (width, height) = stream.viewport();
                stream.set_viewport(
                    options.viewport_width.unwrap_or(width),
                    options.viewport_height.unwrap_or(height),
                );
            }
            if let Some(codec) = codec {
                stream.set_codec(codec);
            }
        }
        if let Some(enabled) = options.clipboard_enabled {
            let session = entry.session.clone();
            crate::ipc::run_blocking(move || {
                session.set_clipboard_enabled(enabled)
                    .map_err(|error| rd_error(RD_CONNECT_FAILED, error.to_string()))
            }).await?;
        }
        // Unchanged settings produce no request; changed ones get the next request id.
        renegotiate(shared, false).await
    }

    #[tauri::command]
    pub async fn cmd_rd_set_audio(
        rd_session_id: String,
        muted: bool,
        volume: f32,
    ) -> Result<(), IpcError> {
        let entry = session_or_not_found(&rd_session_id)?;
        let (active, underruns, drops) = {
            let mut audio = entry.shared.audio.lock();
            audio.set_audio(muted, volume);
            audio.stats()
        };
        entry
            .shared
            .telemetry
            .lock()
            .set_audio_stats(active, underruns, drops);
        entry.runtime.emit_meta(&entry.session);
        Ok(())
    }

    #[tauri::command]
    pub async fn cmd_rd_list_audio_devices() -> Result<Vec<crate::ipc::remote_desktop_media::RdAudioDeviceInfo>, IpcError> {
        crate::ipc::run_blocking(|| {
            crate::ipc::remote_desktop_media::list_audio_output_devices()
                .map_err(|error| rd_error(RD_CONNECT_FAILED, error.to_string()))
        })
        .await
    }

    #[tauri::command]
    pub async fn cmd_rd_set_audio_device(
        rd_session_id: String,
        device_name: Option<String>,
    ) -> Result<(), IpcError> {
        let entry = session_or_not_found(&rd_session_id)?;
        crate::ipc::run_blocking(move || {
            let mut audio = entry.shared.audio.lock();
            audio
                .set_output_device(device_name)
                .map_err(|error| rd_error(RD_INVALID_OPTIONS, error.to_string()))
        })
        .await
    }

    pub(crate) fn parse_mac_address(mac: &str) -> Result<[u8; 6], IpcError> {
        let hex: String = mac
            .trim()
            .chars()
            .filter(|ch| *ch != ':' && *ch != '-' && *ch != '.')
            .collect();
        if hex.len() != 12 || !hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return Err(rd_error(
                RD_INVALID_OPTIONS,
                "Enter a valid 6-byte MAC address (for example AA:BB:CC:DD:EE:FF)",
            ));
        }
        let mut out = [0u8; 6];
        for i in 0..6 {
            out[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
                .map_err(|_| rd_error(RD_INVALID_OPTIONS, "Invalid MAC address"))?;
        }
        Ok(out)
    }

    pub(crate) fn build_wol_magic_packet(mac: [u8; 6]) -> [u8; 102] {
        let mut packet = [0u8; 102];
        packet[..6].fill(0xFF);
        for i in 0..16 {
            packet[6 + i * 6..6 + (i + 1) * 6].copy_from_slice(&mac);
        }
        packet
    }

    #[tauri::command]
    pub async fn cmd_rd_wake_on_lan(
        mac_address: String,
        broadcast_addr: Option<String>,
    ) -> Result<(), IpcError> {
        let mac = parse_mac_address(&mac_address)?;
        let target = broadcast_addr
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "255.255.255.255:9".to_owned());
        let packet = build_wol_magic_packet(mac);
        crate::ipc::run_blocking(move || {
            let socket = std::net::UdpSocket::bind("0.0.0.0:0")
                .map_err(|error| rd_error(RD_CONNECT_FAILED, format!("Wake-on-LAN socket bind failed: {error}")))?;
            socket
                .set_broadcast(true)
                .map_err(|error| rd_error(RD_CONNECT_FAILED, format!("Wake-on-LAN broadcast failed: {error}")))?;
            socket
                .send_to(&packet, &target)
                .map_err(|error| rd_error(RD_CONNECT_FAILED, format!("Wake-on-LAN send failed: {error}")))?;
            Ok(())
        })
        .await
    }

    #[tauri::command]
    pub async fn cmd_rd_clipboard_send(rd_session_id: String, text: String) -> Result<(), IpcError> {
        let entry = session_or_not_found(&rd_session_id)?;
        if text.len() > ferryx_rd_proto::MAX_CLIPBOARD_TEXT_BYTES {
            return Err(rd_error(
                RD_INVALID_OPTIONS,
                "The clipboard text is too large for remote desktop",
            ));
        }
        let allowed = entry
            .session
            .stream_state()
            .ok()
            .and_then(|state| state.policy)
            .is_some_and(|policy| policy.allow_clipboard);
        if !allowed {
            return Err(rd_error(
                transport::RD_PERMISSION_REQUIRED,
                "The host does not allow clipboard sharing",
            ));
        }
        crate::ipc::run_blocking(move || {
            // Sends ControlMessage::ClipboardSyncUpdate (client to host).
            entry
                .session
                .send_clipboard_text(text)
                .map_err(|error| rd_error(RD_CONNECT_FAILED, error.to_string()))
        })
        .await
    }

    #[tauri::command]
    pub async fn cmd_rd_special_key(
        rd_session_id: String,
        action: RdSpecialKeyAction,
    ) -> Result<(), IpcError> {
        let entry = find_session(&rd_session_id)
            .ok_or_else(|| rd_error(RD_NOT_FOUND, "That remote desktop session has ended"))?;
        if action == RdSpecialKeyAction::CtrlAltDel {
            return crate::ipc::run_blocking(move || {
                entry
                    .session
                    .request_secure_attention()
                    .map(|_| ())
                    .map_err(|error| match error {
                        ferryx_rd_app::SessionError::Unsupported => rd_error(
                            RD_UNSUPPORTED,
                            "The remote host does not support Secure Attention (Ctrl+Alt+Del)",
                        ),
                        ferryx_rd_app::SessionError::PermissionDenied => rd_error(
                            RD_PERMISSION_REQUIRED,
                            "The host does not allow input from this session",
                        ),
                        other => rd_error(RD_CONNECT_FAILED, other.to_string()),
                    })
            })
            .await;
        }
        let seq = wire::special_key_sequence(action)
            .ok_or_else(|| rd_error(RD_UNSUPPORTED, "Unsupported special key action"))?;
        let mapped: Vec<InputEvent> = {
            let mut mapper = entry.mapper.lock();
            seq.iter().filter_map(|ev| mapper.map(ev)).collect()
        };
        if mapped.is_empty() {
            return Ok(());
        }
        crate::ipc::run_blocking(move || {
            entry
                .session
                .send_inputs(&mapped)
                .map_err(|error| match error {
                    ferryx_rd_app::SessionError::PermissionDenied => rd_error(
                        RD_PERMISSION_REQUIRED,
                        "The host does not allow input from this session",
                    ),
                    other => rd_error(RD_CONNECT_FAILED, other.to_string()),
                })
        })
        .await
    }

    #[tauri::command]
    pub async fn cmd_rd_set_stream_options(
        rd_session_id: String,
        options: RdStreamOptions,
    ) -> Result<RdSessionMeta, IpcError> {
        let entry = session_or_not_found(&rd_session_id)?;
        let shared = entry.shared.clone();
        if let Some(quality) = options.jpeg_quality {
            shared
                .jpeg_quality
                .store(u32::from(quality.clamp(30, 95)), Ordering::Relaxed);
        }
        if let Some(fps) = options.max_present_fps {
            // Zero removes the cap and follows the window refresh rate again.
            shared.max_present_fps.store(fps.min(240), Ordering::Relaxed);
        }
        if let Some(fps) = options.fps {
            // The controller reads its rate from the shared target, so a host-rate change
            // reconfigures the queue and asks for the new rate in one place.
            shared.apply_refresh(u32::from(fps));
        }
        let host_change = options.display_index.is_some()
            || options.scale_permille.is_some()
            || options.fps.is_some()
            || options.bitrate.is_some();
        let session = entry.session.clone();
        let meta = crate::ipc::run_blocking(move || {
            if host_change {
                let state = session
                    .stream_state()
                    .map_err(|error| rd_error(RD_CONNECT_FAILED, error.to_string()))?;
                if !state.host_supports_stream_options {
                    if options.display_index.is_some() {
                        return Err(rd_error(
                            RD_UNSUPPORTED,
                            "The remote host does not support switching displays or stream options",
                        ));
                    }
                } else {
                    if let Some(display_index) = options.display_index {
                        if !state.displays.is_empty()
                            && !state.displays.iter().any(|d| d.index == display_index)
                        {
                            return Err(rd_error(
                                RD_NOT_FOUND,
                                format!("Display {display_index} is not available on the host"),
                            ));
                        }
                    }
                    {
                        // One controller owns the request ids, so a stream-option change
                        // never collides with a scale/refresh renegotiation.
                        let mut stream = shared.stream.lock();
                        if let Some(display_index) = options.display_index {
                            stream.set_display_override(Some(display_index));
                        }
                        if let Some(scale) = options.scale_permille {
                            stream.set_scale_permille(scale.clamp(250, 1000));
                        }
                        if let Some(bitrate) = options.bitrate {
                            stream.set_bitrate(bitrate);
                        }
                    }
                    viewer::send_stream_options(&session, &shared, false);
                    let _ = session.send_control(ControlMessage::RequestKeyFrame);
                }
            }
            let meta = entry.runtime.emit_meta(&entry.session);
            Ok(meta)
        })
        .await?;
        Ok(meta)
    }

    #[tauri::command]
    pub async fn cmd_rd_send_clipboard(
        rd_session_id: String,
        text: String,
    ) -> Result<(), IpcError> {
        let entry = session_or_not_found(&rd_session_id)?;
        if text.len() > wire::MAX_CLIPBOARD_BYTES {
            return Err(rd_error(
                RD_CONNECT_FAILED,
                "Clipboard text exceeds the 1 MiB limit",
            ));
        }
        crate::ipc::run_blocking(move || {
            let allowed = entry.session.clipboard_allowed().unwrap_or(false);
            if !allowed {
                return Err(rd_error(
                    RD_PERMISSION_REQUIRED,
                    "The host does not allow clipboard sharing for this session",
                ));
            }
            entry
                .session
                .send_clipboard_text(text)
                .map_err(|error| match error {
                    ferryx_rd_app::SessionError::PermissionDenied => rd_error(
                        RD_PERMISSION_REQUIRED,
                        "The host does not allow clipboard sharing for this session",
                    ),
                    other => rd_error(RD_CONNECT_FAILED, other.to_string()),
                })
        })
        .await
    }

    #[tauri::command]
    pub async fn cmd_rd_set_audio_muted(
        rd_session_id: String,
        muted: bool,
    ) -> Result<bool, IpcError> {
        let entry = session_or_not_found(&rd_session_id)?;
        let (active, underruns, drops) = {
            // Mute is the same knob `cmd_rd_set_audio` turns; only the volume is kept.
            let mut audio = entry.shared.audio.lock();
            let volume = audio.volume();
            audio.set_audio(muted, volume);
            audio.stats()
        };
        entry
            .shared
            .telemetry
            .lock()
            .set_audio_stats(active, underruns, drops);
        entry.runtime.emit_meta(&entry.session);
        Ok(muted)
    }

    #[tauri::command]
    pub async fn cmd_rd_request_keyframe(rd_session_id: String) -> Result<(), IpcError> {
        let entry = find_session(&rd_session_id)
            .ok_or_else(|| rd_error(RD_NOT_FOUND, "That remote desktop session has ended"))?;
        crate::ipc::run_blocking(move || {
            entry
                .session
                .send_control(ControlMessage::RequestKeyFrame)

                .map_err(|error| rd_error(RD_CONNECT_FAILED, error.to_string()))
        })
        .await
    }

    #[tauri::command]
    pub async fn cmd_rd_surface_attach<R: tauri::Runtime>(
        window: tauri::WebviewWindow<R>,
        rd_session_id: String,
        bounds: RdLogicalBounds,
    ) -> Result<RdSurfaceAttachResponse, IpcError> {
        let entry = session_or_not_found(&rd_session_id)?;
        // Surface creation dispatches synchronously to the UI thread on macOS.
        let target = window.as_ref().window();
        let created = crate::ipc::run_blocking(move || {
            Ok(RdNativeSurface::new(&target).map(|mut surface| {
                surface.update_bounds(Some(bounds.into()), true);
                surface
            }))
        })
        .await?;
        match created {
            Ok(surface) => {
                entry.shared.attach_surface(surface);
                entry.shared.telemetry.lock().set_present_backend("wgpu");
                Ok(RdSurfaceAttachResponse {
                    backend: "wgpu".into(),
                    reason: None,
                })
            }
            Err(error) => {
                tracing::info!(%error, "remote desktop native surface unavailable; using canvas");
                entry.shared.telemetry.lock().set_present_backend("canvas2d");
                Ok(RdSurfaceAttachResponse {
                    backend: "canvas2d".into(),
                    reason: Some(error.to_string()),
                })
            }
        }
    }

    #[tauri::command]
    pub async fn cmd_rd_surface_update_bounds(
        rd_session_id: String,
        bounds: Option<RdLogicalBounds>,
        visible: bool,
    ) -> Result<(), IpcError> {
        let entry = session_or_not_found(&rd_session_id)?;
        let mut slot = entry.shared.surface_slot.lock();
        if let Some(surface) = slot.surface.as_mut() {
            surface.update_bounds(bounds.map(Into::into), visible);
        }
        Ok(())
    }

    #[tauri::command]
    pub async fn cmd_rd_surface_detach(rd_session_id: String) -> Result<(), IpcError> {
        let entry = session_or_not_found(&rd_session_id)?;
        // Releasing the native target may wait on UI-thread teardown; idempotent.
        crate::ipc::run_blocking(move || {
            entry.shared.detach_surface();
            Ok(())
        })
        .await
    }

    #[tauri::command]
    pub async fn cmd_rd_disconnect(rd_session_id: String) -> Result<(), IpcError> {
        let entry = remove_session(&rd_session_id)
            .ok_or_else(|| rd_error(RD_NOT_FOUND, "That remote desktop session has ended"))?;
        // Joining the pipeline threads blocks; keep it off the async reactor.
        crate::ipc::run_blocking(move || {
            entry.shutdown();
            Ok(())
        })
        .await
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn assert_not_found(error: IpcError) {
            assert_eq!(
                error.code,
                crate::ipc::error::IpcErrorCode::Custom(RD_NOT_FOUND.to_owned())
            );
        }

        const MISSING: &str = "missing-rd-session";

        #[test]
        fn client_clipboard_toggle_is_optional_and_codec_is_validated() {
            let unchanged: RdStreamOptionsUpdate = serde_json::from_str("{}").unwrap();
            assert_eq!(unchanged.clipboard_enabled, None);
            let disabled: RdStreamOptionsUpdate = serde_json::from_str(r#"{"clipboardEnabled":false}"#).unwrap();
            assert_eq!(disabled.clipboard_enabled, Some(false));
            assert_eq!(parse_codec("h264").unwrap(), StreamCodecPreference::H264);
            assert_eq!(parse_codec("invalid").unwrap_err().code,
                crate::ipc::error::IpcErrorCode::Custom(RD_INVALID_OPTIONS.into()));
        }

        #[test]
        fn wol_mac_parsing_and_magic_packet_layout() {
            let mac = parse_mac_address("aa:bb:cc:dd:ee:ff").unwrap();
            assert_eq!(mac, [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
            assert_eq!(parse_mac_address("AA-BB-CC-DD-EE-FF").unwrap(), mac);
            assert_eq!(
                parse_mac_address("not-a-mac").unwrap_err().code,
                crate::ipc::error::IpcErrorCode::Custom(RD_INVALID_OPTIONS.into())
            );
            let packet = build_wol_magic_packet(mac);
            assert_eq!(&packet[..6], &[0xFF; 6]);
            for i in 0..16 {
                assert_eq!(&packet[6 + i * 6..6 + (i + 1) * 6], &mac);
            }
        }

        #[tokio::test]
        async fn unknown_sessions_report_rd_not_found() {
            assert_not_found(cmd_rd_stats(MISSING.into()).await.unwrap_err());
            assert_not_found(
                cmd_rd_set_options(MISSING.into(), RdStreamOptionsUpdate::default())
                    .await
                    .unwrap_err(),
            );
            assert_not_found(cmd_rd_set_audio(MISSING.into(), true, 0.5).await.unwrap_err());
            assert_not_found(
                cmd_rd_set_audio_device(MISSING.into(), Some("Built-in".into()))
                    .await
                    .unwrap_err(),
            );
            assert_not_found(
                cmd_rd_clipboard_send(MISSING.into(), "x".into()).await.unwrap_err(),
            );
            assert_not_found(
                cmd_rd_surface_update_bounds(MISSING.into(), None, false)
                    .await
                    .unwrap_err(),
            );
            assert_not_found(cmd_rd_surface_detach(MISSING.into()).await.unwrap_err());
        }

        #[tokio::test]
        async fn surface_attach_reports_rd_not_found_before_touching_the_window() {
            let app = tauri::test::mock_builder()
                .build(tauri::test::mock_context(tauri::test::noop_assets()))
                .unwrap();
            let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
                .build()
                .unwrap();
            let bounds = RdLogicalBounds {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 600.0,
                scale_factor: 2.0,
            };
            assert_not_found(
                cmd_rd_surface_attach(window, MISSING.into(), bounds)
                    .await
                    .unwrap_err(),
            );
        }

        #[test]
        fn option_payloads_match_the_renderer_contract() {
            let connect: RdConnectOptions = serde_json::from_value(serde_json::json!({
                "playoutMode": "latest_wins",
                "scaleMode": "scale75",
                "viewportWidth": 1920,
                "viewportHeight": 1080,
                "audioMuted": true,
                "audioVolume": 0.25,
            }))
            .unwrap();
            assert_eq!(connect.playout_mode, Some(PlayoutMode::LatestWins));
            assert_eq!(connect.scale_mode, Some(RdScaleMode::Scale75));
            assert_eq!((connect.viewport_width, connect.viewport_height), (Some(1920), Some(1080)));
            assert_eq!((connect.audio_muted, connect.audio_volume), (Some(true), Some(0.25)));
            let empty: RdConnectOptions = serde_json::from_value(serde_json::json!({})).unwrap();
            assert!(empty.scale_mode.is_none());
            let update: RdStreamOptionsUpdate =
                serde_json::from_value(serde_json::json!({ "codec": "h264", "scaleMode": "fit" }))
                    .unwrap();
            assert_eq!(parse_codec(update.codec.as_deref().unwrap()).unwrap(), StreamCodecPreference::H264);
            assert!(parse_codec("vp9").is_err());
            let bounds: RdLogicalBounds = serde_json::from_value(serde_json::json!({
                "x": 1.0, "y": 2.0, "width": 3.0, "height": 4.0, "scaleFactor": 2.0,
            }))
            .unwrap();
            assert_eq!(bounds.scale_factor, 2.0);
            assert_eq!(
                serde_json::to_value(RdSurfaceAttachResponse { backend: "canvas2d".into(), reason: None })
                    .unwrap(),
                serde_json::json!({ "backend": "canvas2d", "reason": null })
            );
        }
    }
}

#[cfg(feature = "remote-desktop")]
mod transport {
    //! Pairing over the paired-host secure transport and the loopback byte bridge.
    use std::net::SocketAddr;
    use std::sync::Arc;

    use base64::Engine as _;
    use bytes::Bytes;
    use ferryx_rd_app::PairingRecord;
    use futures_util::{SinkExt, StreamExt};
    use parking_lot::Mutex;
    use serde::Deserialize;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream, UdpSocket};
    use tokio::sync::{mpsc, watch};
    use tokio_tungstenite::tungstenite::Message;

    use super::wire;
    use crate::ipc::error::{IpcError, IpcErrorCode};
    use crate::ipc::remote_desktop_stats::RdTelemetry;
    use crate::paired_host::client::{
        ClientError, MachineClient, RdBridgeTransport, RdStreamConnection, RemoteDesktopRoute,
    };
    use crate::paired_host::direct::datagram::{
        send_datagram_checked, DatagramChannelGuard, DatagramSendOutcome,
    };
    use crate::paired_host::direct_route::GLOBAL_DIRECT_ROUTES;
    use crate::paired_host::secure_http::PairedSocket;
    use crate::paired_host::service::PairedHostService;
    use crate::scoped_contracts::Epoch;

    /// `RdSessionStats::transport_mode` vocabulary.
    const TRANSPORT_QUIC_DATAGRAM: &str = "quic_datagram";
    const TRANSPORT_WS_DIRECT: &str = "ws_direct";
    const TRANSPORT_WS_RELAY: &str = "ws_relay";

    pub const RD_UNSUPPORTED: &str = "RD_UNSUPPORTED";
    pub const RD_HOST_OFFLINE: &str = "RD_HOST_OFFLINE";
    pub const RD_HOST_DISABLED: &str = "RD_HOST_DISABLED";
    pub const RD_PERMISSION_REQUIRED: &str = "RD_PERMISSION_REQUIRED";
    pub const RD_NOT_FOUND: &str = "RD_NOT_FOUND";
    pub const RD_CONNECT_FAILED: &str = "RD_CONNECT_FAILED";

    pub fn rd_error(code: &str, message: impl Into<String>) -> IpcError {
        IpcError::new(IpcErrorCode::Custom(code.to_owned()), message)
    }

    /// Maps a paired-host transport failure to the remote desktop error contract.
    pub fn map_client_error(error: &ClientError) -> IpcError {
        let status = error
            .machine_error
            .as_ref()
            .and_then(|machine| machine.details.get("httpStatus"))
            .and_then(serde_json::Value::as_u64);
        let (code, message) = match (error.code.as_str(), status) {
            ("HOST_UNAVAILABLE", _)
            | ("TIMEOUT", _)
            | ("MACHINE_SERVICE_UNAVAILABLE", _)
            | ("RATE_LIMITED", _) => (RD_HOST_OFFLINE, "The paired host is not reachable"),
            (_, Some(409)) => (
                RD_HOST_DISABLED,
                "Remote desktop hosting is off on that machine",
            ),
            ("NOT_FOUND", _) | ("PAIRED_HOST_CAPABILITY_UNAVAILABLE", _) => (
                RD_UNSUPPORTED,
                "That machine's Ferryx does not offer remote desktop",
            ),
            ("MACHINE_ACCESS_REQUIRED", _)
            | ("PERMISSION_DENIED", _)
            | ("UNAUTHORIZED", _)
            | ("REPAIR_REQUIRED", _)
            | ("PAIRED_HOST_STALE_GENERATION", _) => (
                RD_PERMISSION_REQUIRED,
                "Remote desktop needs a current Machine pairing with control access",
            ),
            _ => (RD_CONNECT_FAILED, "Remote desktop could not connect"),
        };
        rd_error(code, message).with_details(serde_json::json!({ "cause": error.code }))
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct PairGrant {
        pairing_id: String,
        key_b64: String,
        host_name: String,
    }

    /// Builds the client pairing record from a `/remote-desktop/pair` response body.
    pub fn pairing_record(body: &[u8], now_unix_ms: u64) -> Result<PairingRecord, IpcError> {
        let invalid = || {
            rd_error(
                RD_CONNECT_FAILED,
                "The host sent an invalid remote desktop pairing",
            )
        };
        let grant: PairGrant = serde_json::from_slice(body).map_err(|_| invalid())?;
        let key = base64::engine::general_purpose::STANDARD
            .decode(grant.key_b64.trim())
            .map_err(|_| invalid())?;
        if grant.pairing_id.is_empty() || key.len() != 32 {
            return Err(invalid());
        }
        Ok(PairingRecord {
            id: grant.pairing_id,
            name: grant.host_name,
            key,
            added_at_unix_ms: now_unix_ms,
            last_endpoint: None,
            endpoint_aliases: Vec::new(),
            relay_url: None,
            relay_host_id: None,
            trust: ferryx_rd_proto::PairingTrust::Owner,
        })
    }

    async fn stopped(stop: &mut watch::Receiver<bool>) {
        while !*stop.borrow_and_update() {
            if stop.changed().await.is_err() {
                return;
            }
        }
    }

    /// Where the bridge reopens the host stream for a reconnect.
    pub struct StreamSource {
        pub client: Arc<MachineClient>,
        pub service: PairedHostService,
        pub host_id: String,
        pub generation: Epoch,
        pub on_route: Option<Arc<dyn Fn(RemoteDesktopRoute) + Send + Sync>>,
    }

    /// One event out of the `pump` select. Handling happens after the select's futures are
    /// dropped, so the datagram receiver can be replaced when its QUIC connection ends.
    enum PumpEvent {
        Stop,
        Ws(Option<Result<Message, tokio_tungstenite::tungstenite::Error>>),
        Tcp(std::io::Result<usize>),
        Udp(std::io::Result<(usize, SocketAddr)>),
        Datagram(Option<Bytes>),
    }

    async fn recv_datagram(rx: &mut Option<mpsc::Receiver<Bytes>>) -> Option<Bytes> {
        match rx {
            Some(rx) => rx.recv().await,
            None => std::future::pending::<Option<Bytes>>().await,
        }
    }

    /// This `pump`'s view of the direct QUIC datagram path: the channel it registered and
    /// the connection that carries it. All four fields are `None`/unset on the WS paths.
    struct DatagramArm {
        rx: Option<mpsc::Receiver<Bytes>>,
        /// Held only so the channel is deregistered when this `pump` returns.
        _guard: Option<DatagramChannelGuard>,
        connection: Option<quinn::Connection>,
        channel_id: u64,
    }

    impl DatagramArm {
        /// Registers the stream's channel id on the live direct route, if there is one.
        fn open(host_id: &str, transport: RdBridgeTransport) -> Self {
            if let RdBridgeTransport::Direct {
                epoch,
                dgram_channel_id: Some(channel_id),
            } = transport
            {
                if let Some(peer) = GLOBAL_DIRECT_ROUTES.datagram_handle(host_id, epoch) {
                    let (rx, guard) = peer.mux.register(channel_id);
                    return Self {
                        rx: Some(rx),
                        _guard: Some(guard),
                        connection: Some(peer.connection),
                        channel_id,
                    };
                }
            }
            Self {
                rx: None,
                _guard: None,
                connection: None,
                channel_id: 0,
            }
        }

        fn is_open(&self) -> bool {
            self.rx.is_some()
        }

        /// Sends one loopback UDP payload as a QUIC datagram. `false` means the caller must
        /// use the reliable `CHANNEL_UDP` WebSocket frame for this packet.
        fn send(&self, payload: &[u8]) -> bool {
            if !self.is_open() {
                return false;
            }
            self.connection.as_ref().is_some_and(|connection| {
                send_datagram_checked(connection, self.channel_id, payload)
                    == DatagramSendOutcome::Sent
            })
        }
    }

    /// Serves the loopback endpoint `ClientSession` connects to. Every accepted TCP
    /// connection rides its own host stream socket (the first one is opened by the
    /// command so it can report errors); the session's UDP socket is whichever peer last
    /// sent a datagram. Runs until `stop` is set.
    ///
    /// Kept for callers that already hold a bare socket (the live test dials the stream
    /// itself); production uses [`start_bridge`], which also carries the transport.
    #[cfg_attr(not(test), allow(dead_code))]
    pub async fn run_bridge(
        listener: TcpListener,
        udp: UdpSocket,
        first: PairedSocket,
        source: StreamSource,
        stop: watch::Receiver<bool>,
    ) {
        // The caller dialled this socket itself and did not report a transport; the WS path
        // is the honest reading (and the only one the live test takes).
        let first = RdStreamConnection {
            socket: first,
            transport: RdBridgeTransport::Relay,
        };
        run_bridge_with(listener, udp, first, source, stop, None).await;
    }

    /// [`run_bridge`] with the transport the first stream was opened on, plus the shared
    /// telemetry that reports `transport_mode` on every `pump` (re)start.
    pub async fn start_bridge(
        listener: TcpListener,
        udp: UdpSocket,
        first: RdStreamConnection,
        source: StreamSource,
        stop: watch::Receiver<bool>,
        telemetry: Option<Arc<Mutex<RdTelemetry>>>,
    ) {
        run_bridge_with(listener, udp, first, source, stop, telemetry).await;
    }

    async fn run_bridge_with(
        listener: TcpListener,
        udp: UdpSocket,
        first: RdStreamConnection,
        source: StreamSource,
        mut stop: watch::Receiver<bool>,
        telemetry: Option<Arc<Mutex<RdTelemetry>>>,
    ) {
        let mut next_connection = Some(first);
        let mut udp_peer: Option<SocketAddr> = None;
        let mut udp_buf = vec![0_u8; 64 * 1024];
        loop {
            let tcp = tokio::select! {
                biased;
                _ = stopped(&mut stop) => return,
                accepted = listener.accept() => match accepted {
                    Ok((tcp, _)) => tcp,
                    Err(error) => {
                        tracing::warn!(%error, "remote desktop loopback listener failed");
                        return;
                    }
                },
                received = udp.recv_from(&mut udp_buf) => {
                    if let Ok((_, from)) = received {
                        udp_peer = Some(from);
                    }
                    continue;
                }
            };
            let connection = match next_connection.take() {
                Some(connection) => connection,
                None => {
                    let opened = tokio::select! {
                        biased;
                        _ = stopped(&mut stop) => return,
                        opened = source.client.remote_desktop_stream_with_transport(
                            &source.service,
                            source.host_id.clone(),
                            source.generation,
                        ) => opened,
                    };
                    match opened {
                        Ok(connection) => {
                            // A reopen can land on a different route (direct adapter lost or
                            // regained), so the meta is restated for the renderer.
                            if let Some(on_route) = &source.on_route {
                                on_route(connection.transport.route());
                            }
                            connection
                        }
                        Err(error) => {
                            // Dropping the connection lets the session's reconnect retry.
                            tracing::debug!(code = %error.code, "remote desktop stream reopen failed");
                            continue;
                        }
                    }
                }
            };
            let ended = pump(
                tcp,
                connection,
                &udp,
                &mut udp_peer,
                &mut stop,
                &source.host_id,
                telemetry.as_ref(),
            )
            .await;
            if ended {
                return;
            }
        }
    }

    /// Moves bytes between one loopback TCP connection, the UDP socket and one stream
    /// socket. Returns true when `stop` ended it.
    ///
    /// Datagram registration is per invocation: `run_bridge_with` opens a fresh host stream
    /// on every reconnect, so this `pump` registers the channel id of *its* stream and the
    /// guard deregisters it when this `pump` returns.
    async fn pump(
        tcp: TcpStream,
        connection: RdStreamConnection,
        udp: &UdpSocket,
        udp_peer: &mut Option<SocketAddr>,
        stop: &mut watch::Receiver<bool>,
        host_id: &str,
        telemetry: Option<&Arc<Mutex<RdTelemetry>>>,
    ) -> bool {
        let _ = tcp.set_nodelay(true);
        let RdStreamConnection { socket, transport } = connection;
        let (mut tcp_read, mut tcp_write) = tcp.into_split();
        let (mut ws_tx, mut ws_rx) = socket.split();
        let mut tcp_buf = vec![0_u8; wire::MAX_TCP_CHUNK];
        let mut udp_buf = vec![0_u8; 64 * 1024];

        let mut arm = DatagramArm::open(host_id, transport);
        let mode = match (transport, arm.is_open()) {
            (_, true) => TRANSPORT_QUIC_DATAGRAM,
            (RdBridgeTransport::Direct { .. }, false) => TRANSPORT_WS_DIRECT,
            (RdBridgeTransport::Relay, false) => TRANSPORT_WS_RELAY,
        };
        if let Some(telemetry) = telemetry {
            telemetry.lock().set_transport_mode(mode);
        }

        loop {
            let datagram_open = arm.is_open();
            let event = tokio::select! {
                biased;
                _ = stopped(stop) => PumpEvent::Stop,
                datagram = recv_datagram(&mut arm.rx), if datagram_open => {
                    PumpEvent::Datagram(datagram)
                },
                message = ws_rx.next() => PumpEvent::Ws(message),
                read = tcp_read.read(&mut tcp_buf) => PumpEvent::Tcp(read),
                received = udp.recv_from(&mut udp_buf) => PumpEvent::Udp(received),
            };
            match event {
                PumpEvent::Stop => {
                    let _ = ws_tx.send(Message::Close(None)).await;
                    return true;
                }
                PumpEvent::Datagram(Some(payload)) => {
                    if let Some(peer) = *udp_peer {
                        let _ = udp.send_to(&payload[..], peer).await;
                    }
                }
                PumpEvent::Datagram(None) => {
                    // The QUIC connection ended: keep the session on the WebSocket path
                    // instead of tearing down a stream that still carries TCP.
                    arm.rx = None;
                }
                PumpEvent::Ws(message) => match message {
                    Some(Ok(Message::Binary(data))) => match wire::decode_bridge_frame(&data) {
                        Some((wire::BRIDGE_TCP, payload)) => {
                            if tcp_write.write_all(payload).await.is_err() {
                                return false;
                            }
                        }
                        Some((_, payload)) => {
                            if let Some(peer) = *udp_peer {
                                let _ = udp.send_to(payload, peer).await;
                            }
                        }
                        None => return false,
                    },
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return false,
                    Some(Ok(_)) => {}
                },
                PumpEvent::Tcp(read) => match read {
                    Ok(0) | Err(_) => {
                        let _ = ws_tx.send(Message::Close(None)).await;
                        return false;
                    }
                    Ok(n) => {
                        let frame = wire::encode_bridge_frame(wire::BRIDGE_TCP, &tcp_buf[..n]);
                        if ws_tx.send(Message::Binary(frame.into())).await.is_err() {
                            return false;
                        }
                    }
                },
                PumpEvent::Udp(received) => {
                    if let Ok((n, from)) = received {
                        *udp_peer = Some(from);
                        let payload = &udp_buf[..n];
                        // Per packet: only a datagram that was not sent takes the WS path.
                        if !arm.send(payload) {
                            let frame = wire::encode_bridge_frame(wire::BRIDGE_UDP, payload);
                            if ws_tx.send(Message::Binary(frame.into())).await.is_err() {
                                return false;
                            }
                        }
                    }
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::paired_host::direct::datagram::{
            decode_datagram_frame, spawn_datagram_reader, DatagramMux,
        };
        use crate::paired_host::direct::{create_quic_endpoint, generate_ephemeral_cert};
        use crate::paired_host::direct_route::DirectGuard;
        use crate::paired_host::secure_http::PairedStream;
        use crate::remote::remote_desktop::PAIR_ROUTE;
        use crate::remote::DevicePermission;
        use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

        fn http_error(code: &str, status: u16) -> ClientError {
            let mut details = serde_json::Map::new();
            details.insert("httpStatus".into(), serde_json::json!(status));
            ClientError {
                code: code.to_owned(),
                machine_error: Some(Box::new(crate::remote::machine_protocol::MachineError {
                    code: code.to_owned(),
                    message: String::new(),
                    retryable: false,
                    request_id: String::new(),
                    details,
                })),
                request_id: None,
                ambiguous: false,
            }
        }

        fn code_of(error: &IpcError) -> String {
            error.code.to_string()
        }

        /// A loopback WebSocket pair on the shared `PairedSocket` type: the pump takes the
        /// client end, the test plays the host bridge on the server end. Both handshakes run
        /// concurrently, the way a real upgrade does.
        async fn ws_pair() -> (PairedSocket, WebSocketStream<PairedStream>) {
            use tokio_tungstenite::tungstenite::client::IntoClientRequest;
            let (local, gateway) = tokio::io::duplex(256 * 1024);
            let request = "ws://localhost/api/v1/remote-desktop/stream"
                .into_client_request()
                .expect("ws request");
            let (client, server) = tokio::join!(
                tokio_tungstenite::client_async_with_config(
                    request,
                    MaybeTlsStream::Plain(PairedStream::Tunnel(local)),
                    None,
                ),
                tokio_tungstenite::accept_async(PairedStream::Tunnel(gateway)),
            );
            (
                client.expect("client end of the stream socket").0,
                server.expect("host end of the stream socket"),
            )
        }

        struct QuicPair {
            server_endpoint: quinn::Endpoint,
            client_endpoint: quinn::Endpoint,
            /// Played by the host.
            host: quinn::Connection,
            /// The connection the direct route publishes to the client.
            client: quinn::Connection,
        }

        impl Drop for QuicPair {
            fn drop(&mut self) {
                self.server_endpoint.close(0u32.into(), b"done");
                self.client_endpoint.close(0u32.into(), b"done");
            }
        }

        async fn quic_pair() -> QuicPair {
            let server_sock = std::net::UdpSocket::bind("127.0.0.1:0").expect("server udp");
            let client_sock = std::net::UdpSocket::bind("127.0.0.1:0").expect("client udp");
            let server_addr = server_sock.local_addr().expect("server addr");
            let server_cert = generate_ephemeral_cert().expect("server cert");
            let client_cert = generate_ephemeral_cert().expect("client cert");
            let server_endpoint =
                create_quic_endpoint(server_sock, &server_cert, &client_cert.cert_der, true)
                    .expect("server endpoint");
            let client_endpoint =
                create_quic_endpoint(client_sock, &client_cert, &server_cert.cert_der, false)
                    .expect("client endpoint");
            let connect = async {
                client_endpoint
                    .connect(server_addr, "ferryx-direct")
                    .map_err(|e| e.to_string())?
                    .await
                    .map_err(|e| e.to_string())
            };
            let accept = async {
                let incoming = server_endpoint
                    .accept()
                    .await
                    .ok_or_else(|| "server endpoint closed".to_string())?;
                incoming.await.map_err(|e| e.to_string())
            };
            let (client, host) = tokio::try_join!(connect, accept).expect("quic handshake");
            QuicPair {
                server_endpoint,
                client_endpoint,
                host,
                client,
            }
        }

        /// Publishes the direct route the `pump` resolves, and keeps everything it owns alive.
        fn publish_direct(
            host_id: &str,
            quic: &QuicPair,
            mux: &Arc<DatagramMux>,
        ) -> (u64, tokio::task::JoinHandle<()>) {
            let reader = spawn_datagram_reader(quic.client.clone(), Arc::clone(mux));
            let mut guard = DirectGuard::new(quic.client.clone(), quic.client_endpoint.clone())
                .with_datagram_mux(Arc::clone(mux));
            guard.track(reader.abort_handle());
            let epoch = GLOBAL_DIRECT_ROUTES
                .begin_attempt(host_id, std::time::Instant::now())
                .expect("direct attempt");
            assert!(
                GLOBAL_DIRECT_ROUTES.publish(host_id, epoch, "http://127.0.0.1:1".into(), guard),
                "route published"
            );
            (epoch, reader)
        }

        fn unique_host_id(name: &str) -> String {
            format!("rd-pump-{name}-{}", uuid::Uuid::new_v4())
        }

        fn transport_mode(telemetry: &Arc<Mutex<RdTelemetry>>) -> String {
            let mut telemetry = telemetry.lock();
            telemetry
                .snapshot(None, std::time::Instant::now())
                .transport_mode
        }

        /// `(viewer tcp, pump tcp, loopback udp, viewer udp, stop tx, stop rx)` for one pump.
        type Parts = (
            TcpStream,
            TcpStream,
            Arc<UdpSocket>,
            UdpSocket,
            watch::Sender<bool>,
            watch::Receiver<bool>,
        );

        async fn parts() -> Parts {
            let listener = TcpListener::bind("127.0.0.1:0").await.expect("loopback tcp");
            let addr = listener.local_addr().expect("loopback addr");
            let viewer = TcpStream::connect(addr).await.expect("viewer tcp");
            let (pump_tcp, _) = listener.accept().await.expect("pump tcp");
            let udp = Arc::new(UdpSocket::bind("127.0.0.1:0").await.expect("loopback udp"));
            let viewer_udp = UdpSocket::bind("127.0.0.1:0").await.expect("viewer udp");
            let (stop_tx, stop_rx) = watch::channel(false);
            (viewer, pump_tcp, udp, viewer_udp, stop_tx, stop_rx)
        }

        const BOUND: std::time::Duration = std::time::Duration::from_secs(20);
        const MTU_PAYLOAD: usize = 1120;

        /// Loopback direct QUIC: 1120-byte UDP media rides datagrams both ways, no
        /// `CHANNEL_UDP` WebSocket frame is produced, and the channel is deregistered when
        /// `pump` returns.
        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn pump_moves_udp_over_quic_datagrams_and_cleans_up_on_exit() {
            tokio::time::timeout(BOUND, async {
                let host_id = unique_host_id("datagram");
                let quic = quic_pair().await;
                let mux = DatagramMux::new();
                let (epoch, reader) = publish_direct(&host_id, &quic, &mux);
                let channel_id = 0x77_u64;
                let telemetry = Arc::new(Mutex::new(RdTelemetry::new("rd-pump", 60)));
                let (socket, mut host_ws) = ws_pair().await;
                let (mut viewer, pump_tcp, udp, viewer_udp, stop_tx, stop_rx) = parts().await;
                let pump = tokio::spawn({
                    let udp = Arc::clone(&udp);
                    let telemetry = Arc::clone(&telemetry);
                    let host_id = host_id.clone();
                    async move {
                        let mut udp_peer = None;
                        let mut stop = stop_rx;
                        pump(
                            pump_tcp,
                            RdStreamConnection {
                                socket,
                                transport: RdBridgeTransport::Direct {
                                    epoch,
                                    dgram_channel_id: Some(channel_id),
                                },
                            },
                            &udp,
                            &mut udp_peer,
                            &mut stop,
                            &host_id,
                            Some(&telemetry),
                        )
                        .await
                    }
                });

                // Client -> host: the first datagram arriving proves registration preceded
                // the loop, so everything after this is deterministic.
                let payload = vec![0xA5_u8; MTU_PAYLOAD];
                viewer_udp
                    .send_to(&payload, udp.local_addr().expect("udp addr"))
                    .await
                    .expect("viewer send");
                let frame = quic.host.read_datagram().await.expect("host datagram");
                let (id, got) = decode_datagram_frame(frame).expect("valid datagram frame");
                assert_eq!(id, channel_id);
                assert_eq!(&got[..], &payload[..]);
                assert_eq!(transport_mode(&telemetry), TRANSPORT_QUIC_DATAGRAM);

                // Host -> client: the same size rides back on the datagram arm.
                let reply = vec![0x5A_u8; MTU_PAYLOAD];
                assert_eq!(
                    send_datagram_checked(&quic.host, channel_id, &reply),
                    DatagramSendOutcome::Sent
                );
                let mut buf = vec![0_u8; 64 * 1024];
                let (n, from) = viewer_udp.recv_from(&mut buf).await.expect("viewer udp");
                assert_eq!(&buf[..n], &reply[..]);
                assert_eq!(from.port(), udp.local_addr().expect("udp addr").port());

                // Zero `CHANNEL_UDP` WebSocket frames: the next frame the host sees is this
                // ordered TCP marker.
                viewer.write_all(b"marker").await.expect("viewer tcp write");
                let message = host_ws.next().await.expect("open").expect("frame");
                assert_eq!(
                    wire::decode_bridge_frame(&message.into_data()),
                    Some((wire::BRIDGE_TCP, &b"marker"[..]))
                );

                assert_eq!(mux.len(), 1, "registered for the life of this pump");
                stop_tx.send_replace(true);
                assert!(pump.await.expect("pump joins"), "stop ends the pump");
                assert!(mux.is_empty(), "the guard deregisters the channel on exit");
                GLOBAL_DIRECT_ROUTES.reset(&host_id);
                reader.abort();
            })
            .await
            .expect("test bounded");
        }

        /// Each `pump` invocation owns its registration: channel 1 is deregistered when the
        /// first pump ends, and the next pump streams cleanly on a fresh channel 2.
        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn reconnect_deregisters_the_old_channel_and_streams_on_a_fresh_one() {
            tokio::time::timeout(BOUND, async {
                let host_id = unique_host_id("reconnect");
                let quic = quic_pair().await;
                let mux = DatagramMux::new();
                let (epoch, reader) = publish_direct(&host_id, &quic, &mux);
                let telemetry = Arc::new(Mutex::new(RdTelemetry::new("rd-pump", 60)));

                // First pump on channel 1.
                let (socket, host_ws) = ws_pair().await;
                let (viewer, pump_tcp, udp, viewer_udp, _stop_tx, stop_rx) = parts().await;
                let first_pump = tokio::spawn({
                    let udp = Arc::clone(&udp);
                    let telemetry = Arc::clone(&telemetry);
                    let host_id = host_id.clone();
                    async move {
                        let mut udp_peer = None;
                        let mut stop = stop_rx;
                        pump(
                            pump_tcp,
                            RdStreamConnection {
                                socket,
                                transport: RdBridgeTransport::Direct {
                                    epoch,
                                    dgram_channel_id: Some(1),
                                },
                            },
                            &udp,
                            &mut udp_peer,
                            &mut stop,
                            &host_id,
                            Some(&telemetry),
                        )
                        .await
                    }
                });
                viewer_udp
                    .send_to(&[1_u8; MTU_PAYLOAD], udp.local_addr().expect("udp addr"))
                    .await
                    .expect("viewer send");
                let (id, _) = decode_datagram_frame(
                    quic.host.read_datagram().await.expect("host datagram"),
                )
                .expect("valid frame");
                assert_eq!(id, 1);
                assert_eq!(mux.len(), 1);

                // The viewer goes away, so the first pump ends on its own (not via `stop`)
                // and the host end of the stream was never closed underneath it.
                drop(viewer);
                assert!(!first_pump.await.expect("first pump joins"));
                drop(host_ws);
                assert!(mux.is_empty(), "channel 1 deregistered on pump exit");

                // Second pump on channel 2 streams cleanly over the same route.
                let (socket, mut host_ws) = ws_pair().await;
                let (mut viewer, pump_tcp, udp, viewer_udp, stop_tx, stop_rx) = parts().await;
                let second_pump = tokio::spawn({
                    let udp = Arc::clone(&udp);
                    let telemetry = Arc::clone(&telemetry);
                    let host_id = host_id.clone();
                    async move {
                        let mut udp_peer = None;
                        let mut stop = stop_rx;
                        pump(
                            pump_tcp,
                            RdStreamConnection {
                                socket,
                                transport: RdBridgeTransport::Direct {
                                    epoch,
                                    dgram_channel_id: Some(2),
                                },
                            },
                            &udp,
                            &mut udp_peer,
                            &mut stop,
                            &host_id,
                            Some(&telemetry),
                        )
                        .await
                    }
                });
                viewer_udp
                    .send_to(&[2_u8; MTU_PAYLOAD], udp.local_addr().expect("udp addr"))
                    .await
                    .expect("viewer send");
                let (id, got) = decode_datagram_frame(
                    quic.host.read_datagram().await.expect("host datagram"),
                )
                .expect("valid frame");
                assert_eq!(id, 2, "the reconnected stream uses its own channel");
                assert_eq!(&got[..], &[2_u8; MTU_PAYLOAD][..]);
                let reply = [0x42_u8; MTU_PAYLOAD];
                assert_eq!(
                    send_datagram_checked(&quic.host, 2, &reply),
                    DatagramSendOutcome::Sent
                );
                let mut buf = vec![0_u8; 64 * 1024];
                let (n, _) = viewer_udp.recv_from(&mut buf).await.expect("viewer udp");
                assert_eq!(&buf[..n], &reply[..]);
                assert_eq!(transport_mode(&telemetry), TRANSPORT_QUIC_DATAGRAM);

                // `CHANNEL_TCP` still rides the WebSocket on the reconnected stream.
                viewer.write_all(b"done").await.expect("viewer write");
                let message = host_ws.next().await.expect("open").expect("frame");
                assert_eq!(
                    wire::decode_bridge_frame(&message.into_data()),
                    Some((wire::BRIDGE_TCP, &b"done"[..]))
                );
                assert_eq!(mux.len(), 1);
                stop_tx.send_replace(true);
                assert!(second_pump.await.expect("second pump joins"));
                assert!(mux.is_empty());
                GLOBAL_DIRECT_ROUTES.reset(&host_id);
                reader.abort();
            })
            .await
            .expect("test bounded");
        }

        /// Without a datagram channel the pump reports the WebSocket transport and carries
        /// loopback UDP as `CHANNEL_UDP` frames: `ws_direct` for a direct stream the host did
        /// not accept datagrams on, `ws_relay` for a relayed one.
        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn no_datagram_channel_pumps_udp_over_the_websocket() {
            tokio::time::timeout(BOUND, async {
                for (transport, expected) in [
                    (
                        RdBridgeTransport::Direct {
                            epoch: 1,
                            dgram_channel_id: None,
                        },
                        TRANSPORT_WS_DIRECT,
                    ),
                    (RdBridgeTransport::Relay, TRANSPORT_WS_RELAY),
                ] {
                    let host_id = unique_host_id("ws");
                    let telemetry = Arc::new(Mutex::new(RdTelemetry::new("rd-pump", 60)));
                    let (socket, mut host_ws) = ws_pair().await;
                    let (mut viewer, pump_tcp, udp, viewer_udp, stop_tx, stop_rx) = parts().await;
                    let pump = tokio::spawn({
                        let udp = Arc::clone(&udp);
                        let telemetry = Arc::clone(&telemetry);
                        let host_id = host_id.clone();
                        async move {
                            let mut udp_peer = None;
                            let mut stop = stop_rx;
                            pump(
                                pump_tcp,
                                RdStreamConnection { socket, transport },
                                &udp,
                                &mut udp_peer,
                                &mut stop,
                                &host_id,
                                Some(&telemetry),
                            )
                            .await
                        }
                    });

                    let payload = vec![0x11_u8; MTU_PAYLOAD];
                    viewer_udp
                        .send_to(&payload, udp.local_addr().expect("udp addr"))
                        .await
                        .expect("viewer send");
                    let message = host_ws.next().await.expect("open").expect("frame");
                    let data = message.into_data();
                    let (channel, got) =
                        wire::decode_bridge_frame(&data).expect("bridge frame");
                    assert_eq!(channel, wire::BRIDGE_UDP, "UDP rides the WebSocket");
                    assert_eq!(got, &payload[..]);
                    assert_eq!(transport_mode(&telemetry), expected);

                    // Host -> client over the same WS channel reaches the viewer's UDP socket.
                    let frame = wire::encode_bridge_frame(wire::BRIDGE_UDP, b"pong");
                    host_ws
                        .send(Message::Binary(frame.into()))
                        .await
                        .expect("host send");
                    let mut buf = vec![0_u8; 2048];
                    let (n, _) = viewer_udp.recv_from(&mut buf).await.expect("viewer udp");
                    assert_eq!(&buf[..n], b"pong");

                    // `CHANNEL_TCP` stays on the WebSocket in the fallback path too.
                    viewer.write_all(b"tcp").await.expect("viewer write");
                    let message = host_ws.next().await.expect("open").expect("frame");
                    assert_eq!(
                        wire::decode_bridge_frame(&message.into_data()),
                        Some((wire::BRIDGE_TCP, &b"tcp"[..]))
                    );

                    drop(viewer);
                    assert!(!pump.await.expect("pump joins"));
                    drop(stop_tx);
                }
            })
            .await
            .expect("test bounded");
        }

        #[test]
        fn transport_errors_map_to_contract_codes() {
            assert_eq!(
                code_of(&map_client_error(&ClientError::local("HOST_UNAVAILABLE"))),
                RD_HOST_OFFLINE
            );
            assert_eq!(
                code_of(&map_client_error(&http_error("HOST_UNAVAILABLE", 409))),
                RD_HOST_OFFLINE
            );
            assert_eq!(
                code_of(&map_client_error(&http_error(
                    "PAIRED_HOST_REMOTE_ERROR",
                    409
                ))),
                RD_HOST_DISABLED
            );
            assert_eq!(
                code_of(&map_client_error(&http_error("NOT_FOUND", 404))),
                RD_UNSUPPORTED
            );
            assert_eq!(
                code_of(&map_client_error(&ClientError::local(
                    "MACHINE_ACCESS_REQUIRED"
                ))),
                RD_PERMISSION_REQUIRED
            );
            let other = map_client_error(&ClientError::local("PAYLOAD_TOO_LARGE"));
            assert_eq!(code_of(&other), RD_CONNECT_FAILED);
            assert_eq!(other.details.unwrap()["cause"], "PAYLOAD_TOO_LARGE");
        }

        #[test]
        fn pairing_grant_becomes_an_owner_record() {
            let key = base64::engine::general_purpose::STANDARD.encode([7_u8; 32]);
            let body = serde_json::json!({ "pairingId": "p1", "keyB64": key, "hostName": "Desk" });
            let record = pairing_record(body.to_string().as_bytes(), 42).unwrap();
            assert_eq!((record.id.as_str(), record.name.as_str()), ("p1", "Desk"));
            assert_eq!(record.key, vec![7_u8; 32]);
            assert_eq!(record.added_at_unix_ms, 42);
            assert_eq!(record.trust, ferryx_rd_proto::PairingTrust::Owner);
            let short = base64::engine::general_purpose::STANDARD.encode([7_u8; 16]);
            let body =
                serde_json::json!({ "pairingId": "p1", "keyB64": short, "hostName": "Desk" });
            assert!(pairing_record(body.to_string().as_bytes(), 0).is_err());
            assert!(pairing_record(b"not json", 0).is_err());
        }

        /// C3 regression: with remote desktop hosting OFF on the far machine the pane must get a
        /// typed refusal instead of waiting. The gateway answers the pair route with 409 +
        /// RD_HOST_DISABLED before any socket upgrade, and the viewer's transport maps that 409
        /// to the same RD_HOST_DISABLED code the pane renders as "turned off". This drives the
        /// real gateway router and the real MachineClient, so a regression in either the route or
        /// the mapping fails here rather than showing up as a spinner in the GUI.
        #[test]
        fn a_disabled_host_refuses_the_pair_route_before_any_stream() {
            let dir = tempfile::tempdir().expect("tempdir");
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("runtime");
            let state = {
                let _context = rt.enter();
                Arc::new(crate::remote::state::RemoteGatewayState::new_with_paths(
                    Arc::new(crate::terminal::TerminalService::default()),
                    crate::worktree::WorkspaceRegistry::new(),
                    Some(dir.path().join("remote-config.json")),
                    Some(dir.path().join("remote-auth.json")),
                ))
            };
            // The host stays disabled: never call set(), so this is the default state the GUI
            // finds on a machine whose owner turned remote desktop off.
            assert!(!state.remote_desktop.state().enabled);

            let listener = rt
                .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
                .expect("bind gateway");
            let gateway = listener.local_addr().expect("gateway addr");
            let router = crate::remote::create_remote_router(Arc::clone(&state));
            let server = rt.spawn(async move { axum::serve(listener, router).await });

            let code = state
                .auth_manager
                .create_scoped_pairing_code(
                    DevicePermission::Control,
                    crate::remote::auth::DeviceAccessScope::Machine,
                )
                .expect("machine pairing code");
            let (token, _device) = state
                .auth_manager
                .exchange_pairing_code(&code, "c3 disabled host")
                .expect("exchange pairing code");

            let started = std::time::Instant::now();
            let (status, body) = rt.block_on(async {
                let http = reqwest::Client::builder()
                    .no_proxy()
                    .build()
                    .expect("http client");
                let response = http
                    .post(format!("http://{gateway}{PAIR_ROUTE}"))
                    .bearer_auth(&token)
                    .send()
                    .await
                    .expect("pair request");
                (response.status().as_u16(), response.bytes().await.expect("pair body").to_vec())
            });
            let elapsed = started.elapsed();
            assert_eq!(status, 409, "disabled host must answer 409: {}", String::from_utf8_lossy(&body));
            let refusal: serde_json::Value = serde_json::from_slice(&body).expect("refusal JSON");
            assert_eq!(refusal["code"], "RD_HOST_DISABLED", "typed refusal: {refusal}");
            // Well inside the pane's 30 s budget: the refusal is immediate, not a timeout.
            assert!(
                elapsed < std::time::Duration::from_secs(5),
                "refusal took {elapsed:?}, expected an immediate answer"
            );

            // The same refusal travels through the client's error mapping to the pane's code.
            let mut details = serde_json::Map::new();
            details.insert("httpStatus".into(), serde_json::json!(status));
            let client_error = ClientError {
                code: refusal["code"].as_str().unwrap_or_default().to_owned(),
                machine_error: Some(Box::new(crate::remote::machine_protocol::MachineError {
                    code: refusal["code"].as_str().unwrap_or_default().to_owned(),
                    message: refusal["message"].as_str().unwrap_or_default().to_owned(),
                    retryable: false,
                    request_id: String::new(),
                    details,
                })),
                request_id: None,
                ambiguous: false,
            };
            assert_eq!(code_of(&map_client_error(&client_error)), RD_HOST_DISABLED);

            server.abort();
        }
    }
}

/// Live end-to-end session: real host, gateway routes and client bridge (ignored; needs a
/// capturable display).
#[cfg(all(test, feature = "remote-desktop"))]
#[path = "remote_desktop_live_tests.rs"]
mod live_tests;

#[cfg(test)]
#[path = "remote_desktop_contract_tests.rs"]
mod contract_tests;
