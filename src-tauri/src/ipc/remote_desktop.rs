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

    use ferryx_rd_proto::{InputEvent, InputEventType, Modifiers};
    use serde::Deserialize;

    /// Frame channel message kinds (byte 0).
    pub const FRAME_KIND_VIDEO: u8 = 0x01;
    pub const FRAME_KIND_STATUS: u8 = 0x02;
    /// Stream bridge channels (byte 0) and the TCP chunk cap of the gateway contract.
    pub const BRIDGE_TCP: u8 = 0x01;
    pub const BRIDGE_UDP: u8 = 0x02;
    pub const MAX_TCP_CHUNK: usize = 60_000;

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
            "F13" => 105,
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
        }
    }
}

// ---- session runtime ----

#[cfg(feature = "remote-desktop")]
mod viewer {
    //! The per-session std thread: `ClientSession`, decode, JPEG and the frame channel.
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use ferryx_rd_app::{ClientSession, PairingRecord, ReconnectState, SessionEvent};
    use ferryx_rd_decode::{detect_codec, CodecKind, HevcDecoder};
    use ferryx_rd_proto::{ControlMessage, StreamCodecPreference, StreamOptionsRequest};
    use tauri::ipc::{Channel, Response};

    use super::wire;

    /// Upper bound on JPEG frames pushed to the WebView.
    const MAX_PRESENT_FPS: u32 = 30;
    const JPEG_QUALITY: u8 = 70;

    pub struct ViewerConfig {
        pub session: ClientSession,
        pub pairing: PairingRecord,
        pub loopback_tcp: u16,
        pub loopback_udp: u16,
        pub frames: Channel<Response>,
        pub stop: Arc<AtomicBool>,
        /// Called once when the thread ends, so the registry and bridge tear down.
        pub on_exit: Box<dyn FnOnce() + Send>,
    }

    fn status(frames: &Channel<Response>, state: &str, reason: Option<&str>) {
        let _ = frames.send(Response::new(wire::encode_status_message(state, reason)));
    }

    pub fn run(config: ViewerConfig) {
        let ViewerConfig {
            session,
            pairing,
            loopback_tcp,
            loopback_udp,
            frames,
            stop,
            on_exit,
        } = config;
        let reason = run_session(
            &session,
            pairing,
            loopback_tcp,
            loopback_udp,
            &frames,
            &stop,
        );
        let _ = session.disconnect();
        status(&frames, "ended", reason.as_deref());
        on_exit();
    }

    fn run_session(
        session: &ClientSession,
        pairing: PairingRecord,
        loopback_tcp: u16,
        loopback_udp: u16,
        frames: &Channel<Response>,
        stop: &AtomicBool,
    ) -> Option<String> {
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
        let mut tcp_runtime = match session.spawn_tcp_runtime() {
            Ok(runtime) => runtime,
            Err(error) => return Some(format!("Remote desktop control channel failed: {error}")),
        };
        if let Err(error) = session.set_udp_read_timeout(Some(Duration::from_millis(20))) {
            let _ = tcp_runtime.stop();
            return Some(format!("Remote desktop media channel failed: {error}"));
        }
        let host_supports_options = session
            .stream_state()
            .map(|state| state.host_supports_stream_options)
            .unwrap_or(false);
        if host_supports_options {
            // The software decode path is fastest on H.264; the host falls back on its own.
            let _ = session.request_stream_options(StreamOptionsRequest {
                request_id: 1,
                scale_permille: 1000,
                fps: 60,
                codec: StreamCodecPreference::H264,
                display_index: 0,
                bitrate: 0,
            });
        }
        let _ = session.send_control(ControlMessage::RequestKeyFrame);
        status(frames, "connected", None);

        let min_interval = Duration::from_secs(1) / MAX_PRESENT_FPS;
        let mut last_present: Option<Instant> = None;
        let mut decoder: Option<(CodecKind, HevcDecoder)> = None;
        let mut announced_ready = false;
        let mut reconnecting = false;
        let mut ended: Option<String> = None;

        while !stop.load(Ordering::SeqCst) && ended.is_none() {
            while let Ok(event) = tcp_runtime.events().try_recv() {
                match event {
                    Ok(SessionEvent::StreamConfig(ControlMessage::SessionEnded(end))) => {
                        ended = Some(format!("The host ended the session ({:?})", end.reason));
                    }
                    Ok(SessionEvent::StreamConfig(ControlMessage::StreamOptionsActive(_))) => {
                        // Codec or size changed: the next keyframe builds a fresh decoder.
                        decoder = None;
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
                if !reconnecting {
                    decoder = None;
                    let _ = session.send_control(ControlMessage::RequestKeyFrame);
                }
            }
            let frame = match session.receive_udp_event() {
                Ok(SessionEvent::Frame(frame)) => frame,
                Ok(_) => continue,
                Err(ferryx_rd_app::SessionError::Io(error))
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) =>
                {
                    continue
                }
                Err(error) => {
                    if reconnecting || ferryx_rd_app::should_reconnect(&error) {
                        // The TCP runtime owns recovery; wait for it instead of failing.
                        std::thread::sleep(Duration::from_millis(50));
                        continue;
                    }
                    ended = Some(format!("Remote desktop media failed: {error}"));
                    break;
                }
            };
            let key = frame.header.is_key_frame;
            let codec_changed = key
                && decoder.as_ref().is_some_and(|(kind, _)| {
                    detect_codec(&frame.data).is_ok_and(|detected| detected != *kind)
                });
            if decoder.is_none() || codec_changed {
                match HevcDecoder::from_keyframe_auto(&frame.data) {
                    Ok(built) => decoder = Some(built),
                    Err(_) => {
                        let _ = session.send_control(ControlMessage::RequestKeyFrame);
                        continue;
                    }
                }
            }
            let Some((_, dec)) = decoder.as_mut() else {
                continue;
            };
            let decoded = match dec.decode(&frame.data, i64::from(frame.timestamp_ms)) {
                Ok(decoded) => decoded,
                Err(error) => {
                    tracing::debug!(%error, "remote desktop decode failed");
                    decoder = None;
                    let _ = session.send_control(ControlMessage::RequestKeyFrame);
                    continue;
                }
            };
            let Some(picture) = decoded.last() else {
                continue;
            };
            if !announced_ready {
                announced_ready = true;
                session.video_ready();
            }
            let due = last_present.is_none_or(|at| at.elapsed() >= min_interval);
            if !due {
                continue;
            }
            match ferryx_rd_app::nv12_frame_to_jpeg(picture, JPEG_QUALITY) {
                Ok(jpeg) => {
                    let message = wire::encode_video_message(
                        picture.width,
                        picture.height,
                        u64::from(frame.header.frame_id),
                        &jpeg,
                    );
                    if frames.send(Response::new(message)).is_err() {
                        ended = Some("The viewer closed".to_owned());
                        break;
                    }
                    last_present = Some(Instant::now());
                    let _ = session.frame_presented(frame.header.frame_id);
                }
                Err(error) => tracing::debug!(%error, "remote desktop jpeg encode failed"),
            }
        }
        let _ = tcp_runtime.stop();
        ended
    }
}

/// Registered in `lib.rs` by full path so tauri's per-command macros resolve.
#[cfg(feature = "remote-desktop")]
pub mod commands {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use ferryx_rd_app::{ClientSession, PairingStore, SessionConfig};
    use ferryx_rd_proto::InputEvent;
    use parking_lot::Mutex;
    use serde::Serialize;
    use tauri::ipc::{Channel, Response};

    use super::transport::{self, rd_error, RD_CONNECT_FAILED, RD_NOT_FOUND};
    use super::viewer;
    use super::wire::{InputMapper, RdInputEvent};
    use crate::ipc::error::IpcError;
    use crate::paired_host::client::MachineClient;
    use crate::paired_host::inventory::{AuthStatus, GrantScope};
    use crate::paired_host::service::PairedHostService;

    const PAIRING_FILE: &str = "remote-desktop-pairings.json";
    const MAX_EVENTS_PER_CALL: usize = 512;

    struct RdSession {
        session: ClientSession,
        mapper: Mutex<InputMapper>,
        stop: Arc<AtomicBool>,
        bridge_stop: tokio::sync::watch::Sender<bool>,
    }

    impl RdSession {
        fn shutdown(&self) {
            self.stop.store(true, Ordering::SeqCst);
            self.session.interrupt();
            self.bridge_stop.send_replace(true);
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

    #[tauri::command]
    pub async fn cmd_rd_connect(
        host_id: String,
        frames: Channel<Response>,
    ) -> Result<RdConnectResponse, IpcError> {
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
                transport::RD_PERMISSION_REQUIRED,
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
            .remote_desktop_stream(&service, host_id.clone(), host.generation)
            .await
            .map_err(|error| transport::map_client_error(&error))?;
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
        let entry = Arc::new(RdSession {
            session: session.clone(),
            mapper: Mutex::new(InputMapper::default()),
            stop: Arc::clone(&stop),
            bridge_stop,
        });
        SESSIONS
            .lock()
            .get_or_insert_with(HashMap::new)
            .insert(rd_session_id.clone(), Arc::clone(&entry));

        tokio::spawn(transport::run_bridge(
            listener,
            udp,
            first,
            transport::StreamSource {
                client,
                service,
                host_id,
                generation: host.generation,
            },
            bridge_stopped,
        ));
        let exit_id = rd_session_id.clone();
        let spawned = std::thread::Builder::new()
            .name("ferryx-rd-viewer".to_owned())
            .spawn(move || {
                viewer::run(viewer::ViewerConfig {
                    session,
                    pairing,
                    loopback_tcp,
                    loopback_udp,
                    frames,
                    stop,
                    on_exit: Box::new(move || {
                        if let Some(entry) = remove_session(&exit_id) {
                            entry.shutdown();
                        }
                    }),
                })
            });
        if let Err(error) = spawned {
            if let Some(entry) = remove_session(&rd_session_id) {
                entry.shutdown();
            }
            return Err(rd_error(
                RD_CONNECT_FAILED,
                format!("Could not start the remote desktop session: {error}"),
            ));
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
                        transport::RD_PERMISSION_REQUIRED,
                        "The host does not allow input from this session",
                    ),
                    other => rd_error(RD_CONNECT_FAILED, other.to_string()),
                })
        })
        .await
    }

    #[tauri::command]
    pub async fn cmd_rd_disconnect(rd_session_id: String) -> Result<(), IpcError> {
        let entry = remove_session(&rd_session_id)
            .ok_or_else(|| rd_error(RD_NOT_FOUND, "That remote desktop session has ended"))?;
        entry.shutdown();
        Ok(())
    }
}

#[cfg(feature = "remote-desktop")]
mod transport {
    //! Pairing over the paired-host secure transport and the loopback byte bridge.
    use std::net::SocketAddr;
    use std::sync::Arc;

    use base64::Engine as _;
    use ferryx_rd_app::PairingRecord;
    use futures_util::{SinkExt, StreamExt};
    use serde::Deserialize;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream, UdpSocket};
    use tokio::sync::watch;
    use tokio_tungstenite::tungstenite::Message;

    use super::wire;
    use crate::ipc::error::{IpcError, IpcErrorCode};
    use crate::paired_host::client::{ClientError, MachineClient};
    use crate::paired_host::secure_http::PairedSocket;
    use crate::paired_host::service::PairedHostService;
    use crate::scoped_contracts::Epoch;

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
    }

    /// Serves the loopback endpoint `ClientSession` connects to. Every accepted TCP
    /// connection rides its own host stream socket (the first one is opened by the
    /// command so it can report errors); the session's UDP socket is whichever peer last
    /// sent a datagram. Runs until `stop` is set.
    pub async fn run_bridge(
        listener: TcpListener,
        udp: UdpSocket,
        first: PairedSocket,
        source: StreamSource,
        mut stop: watch::Receiver<bool>,
    ) {
        let mut next_socket = Some(first);
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
            let socket = match next_socket.take() {
                Some(socket) => socket,
                None => {
                    let opened = tokio::select! {
                        biased;
                        _ = stopped(&mut stop) => return,
                        opened = source.client.remote_desktop_stream(
                            &source.service,
                            source.host_id.clone(),
                            source.generation,
                        ) => opened,
                    };
                    match opened {
                        Ok(socket) => socket,
                        Err(error) => {
                            // Dropping the connection lets the session's reconnect retry.
                            tracing::debug!(code = %error.code, "remote desktop stream reopen failed");
                            continue;
                        }
                    }
                }
            };
            if pump(tcp, socket, &udp, &mut udp_peer, &mut stop).await {
                return;
            }
        }
    }

    /// Moves bytes between one loopback TCP connection, the UDP socket and one stream
    /// socket. Returns true when `stop` ended it.
    async fn pump(
        tcp: TcpStream,
        socket: PairedSocket,
        udp: &UdpSocket,
        udp_peer: &mut Option<SocketAddr>,
        stop: &mut watch::Receiver<bool>,
    ) -> bool {
        let _ = tcp.set_nodelay(true);
        let (mut tcp_read, mut tcp_write) = tcp.into_split();
        let (mut ws_tx, mut ws_rx) = socket.split();
        let mut tcp_buf = vec![0_u8; wire::MAX_TCP_CHUNK];
        let mut udp_buf = vec![0_u8; 64 * 1024];
        loop {
            tokio::select! {
                biased;
                _ = stopped(stop) => {
                    let _ = ws_tx.send(Message::Close(None)).await;
                    return true;
                }
                message = ws_rx.next() => match message {
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
                read = tcp_read.read(&mut tcp_buf) => match read {
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
                received = udp.recv_from(&mut udp_buf) => {
                    if let Ok((n, from)) = received {
                        *udp_peer = Some(from);
                        let frame = wire::encode_bridge_frame(wire::BRIDGE_UDP, &udp_buf[..n]);
                        if ws_tx.send(Message::Binary(frame.into())).await.is_err() {
                            return false;
                        }
                    }
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

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
    }
}

/// Live end-to-end session: real host, gateway routes and client bridge (ignored; needs a
/// capturable display).
#[cfg(all(test, feature = "remote-desktop"))]
#[path = "remote_desktop_live_tests.rs"]
mod live_tests;
