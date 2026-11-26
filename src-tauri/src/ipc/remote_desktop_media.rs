//! Viewer-side audio, cursor framing and synchronous clipboard adapters.
//!
//! Clipboard calls perform OS I/O (bounded subprocesses on Linux). Call them
//! from a std worker thread or a blocking task, never a Tokio reactor thread.
#![cfg(feature = "remote-desktop")]

use base64::{engine::general_purpose::STANDARD, Engine as _};
use ferryx_rd_app::{
    ClipboardDecision, ClipboardError, ClipboardSynchronizer, PlatformClipboard,
};
#[cfg(any(test, target_os = "windows", target_os = "linux"))]
use ferryx_rd_app::ClipboardSnapshot;
use ferryx_rd_render::{AudioError, AudioQueue, CpalAudioOutput};
use serde::{Deserialize, Serialize};

pub struct RdAudioPlayer {
    queue: AudioQueue,
    output: Option<CpalAudioOutput>,
    unavailable: bool,
    muted: bool,
    volume: f32,
}

impl Default for RdAudioPlayer {
    fn default() -> Self {
        Self {
            queue: AudioQueue::default(),
            output: None,
            unavailable: false,
            muted: false,
            volume: 1.0,
        }
    }
}

impl RdAudioPlayer {
    #[cfg(test)]
    pub(crate) fn new_headless_for_test() -> Self {
        Self { unavailable: true, ..Self::default() }
    }

    #[cfg(test)]
    pub(crate) fn queued_samples(&self) -> usize {
        self.queue.queued_samples().expect("audio queue sample count")
    }

    pub fn push_pcm(&mut self, pcm_f32_le: &[u8]) {
        self.push_pcm_with_start(pcm_f32_le, CpalAudioOutput::start);
    }

    // The injected starter lets tests exercise the real failure path without
    // probing an OS device. SessionEvent::Audio is already decoded stereo PCM.
    fn push_pcm_with_start(
        &mut self,
        pcm_f32_le: &[u8],
        start: impl FnOnce(AudioQueue) -> Result<CpalAudioOutput, AudioError>,
    ) {
        if self.output.is_none() && !self.unavailable {
            match start(self.queue.clone()) {
                Ok(output) => self.output = Some(output),
                Err(err) => {
                    self.unavailable = true;
                    tracing::warn!(%err, "remote desktop audio output unavailable; continuing without audio");
                }
            }
        }
        if let Err(err) = self.queue.push_pcm_bytes(pcm_f32_le) {
            tracing::warn!(%err, "remote desktop rejected invalid PCM");
        }
    }

    pub fn set_audio(&mut self, muted: bool, volume: f32) {
        let clamped = if volume.is_finite() {
            volume.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.queue
            .set_volume(clamped)
            .expect("finite, clamped audio volume");
        self.queue
            .set_muted(muted)
            .expect("setting mute is infallible");
        self.volume = clamped;
        self.muted = muted;
    }

    pub fn stats(&self) -> (bool, u64, u64) {
        (
            self.output.is_some() && !self.unavailable,
            self.queue.underrun_samples().unwrap_or(0),
            self.queue.dropped_samples(),
        )
    }

    pub fn stop(&mut self) {
        let _ = self.queue.clear();
        self.output = None;
    }
}

pub const FRAME_KIND_CURSOR: u8 = 0x03;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RdCursorPayload {
    pub x: f32,
    pub y: f32,
    pub cursor_type: u8,
    pub visible: bool,
    pub shape_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rgba_base64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hotspot_x: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hotspot_y: Option<u16>,
}

#[derive(Default)]
pub struct RdCursorEncoder {
    last_sent_shape_id: Option<u32>,
}

impl RdCursorEncoder {
    pub fn encode(
        &mut self,
        cursor: &ferryx_rd_app::CursorState,
        shape_lookup: impl FnOnce(u32) -> Option<ferryx_rd_proto::CursorShape>,
    ) -> Vec<u8> {
        // The engine uses zero, not Option, for the legacy/no-shape sentinel.
        let shape_id = (cursor.shape_id != 0).then_some(cursor.shape_id);
        let mut payload = RdCursorPayload {
            x: cursor.x,
            y: cursor.y,
            cursor_type: cursor.cursor_type,
            visible: cursor.x >= 0.0 && cursor.y >= 0.0,
            shape_id,
            rgba_base64: None,
            width: None,
            height: None,
            hotspot_x: None,
            hotspot_y: None,
        };
        if shape_id != self.last_sent_shape_id {
            if let Some(id) = shape_id {
                if let Some(shape) = shape_lookup(id) {
                    payload.rgba_base64 = Some(STANDARD.encode(&shape.rgba));
                    payload.width = Some(shape.width);
                    payload.height = Some(shape.height);
                    payload.hotspot_x = Some(shape.hotspot_x);
                    payload.hotspot_y = Some(shape.hotspot_y);
                    self.last_sent_shape_id = Some(id);
                }
                // Do not mark a cache miss sent: a later update must retry it.
            } else {
                self.last_sent_shape_id = None;
            }
        }
        let mut frame = vec![FRAME_KIND_CURSOR];
        serde_json::to_writer(&mut frame, &payload).expect("cursor payload serializes to JSON");
        frame
    }
}

pub struct RdClipboardBridge {
    clipboard: Box<dyn PlatformClipboard>,
    synchronizer: ClipboardSynchronizer,
}

impl Default for RdClipboardBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl RdClipboardBridge {
    /// Blocking: construct and use on a clipboard worker, not a Tokio reactor.
    pub fn new() -> Self {
        Self::with_clipboard(platform::Clipboard::default())
    }

    pub fn with_clipboard(clipboard: impl PlatformClipboard) -> Self {
        let initial_count = match clipboard.snapshot() {
            Ok(snapshot) => snapshot.change_count,
            Err(err) => {
                tracing::debug!(%err, "remote desktop clipboard initially unavailable");
                0
            }
        };
        Self {
            clipboard: Box::new(clipboard),
            synchronizer: ClipboardSynchronizer::new(initial_count),
        }
    }

    pub fn apply_remote_text(&mut self, text: &str, allow_clipboard: bool) {
        if !allow_clipboard {
            return;
        }
        if text.len() > ferryx_rd_proto::MAX_CLIPBOARD_TEXT_BYTES {
            tracing::warn!("remote desktop clipboard text exceeds the protocol limit");
            return;
        }
        match self.clipboard.set_text(text) {
            Ok(change_count) => self.synchronizer.record_remote_write(text, change_count),
            Err(err) => tracing::debug!(%err, "remote desktop clipboard write unavailable"),
        }
    }

    pub fn poll_local_text(&mut self, allow_clipboard: bool) -> Option<String> {
        if !allow_clipboard {
            return None;
        }
        let snapshot = match self.clipboard.snapshot() {
            Ok(snapshot) => snapshot,
            Err(err) => {
                tracing::debug!(%err, "remote desktop clipboard read unavailable");
                return None;
            }
        };
        let types: Vec<&str> = snapshot.types.iter().map(String::as_str).collect();
        match self.synchronizer.poll(
            snapshot.change_count,
            snapshot.text.as_deref().unwrap_or_default(),
            &types,
        ) {
            ClipboardDecision::Send(text) => Some(text),
            _ => None,
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    pub use ferryx_rd_app::platform::SystemClipboard as Clipboard;
}

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use windows_sys::Win32::{
        Foundation::GlobalFree,
        System::{
            DataExchange::{
                CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardSequenceNumber,
                IsClipboardFormatAvailable, OpenClipboard, SetClipboardData,
            },
            Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE},
        },
        UI::WindowsAndMessaging::GetDesktopWindow,
    };

    const CF_UNICODETEXT: u32 = 13;

    #[derive(Default)]
    pub struct Clipboard;

    struct OpenGuard;

    impl OpenGuard {
        fn open() -> Result<Self, ClipboardError> {
            // A non-null HWND is required for EmptyClipboard/SetClipboardData.
            if unsafe { OpenClipboard(GetDesktopWindow()) } == 0 {
                return Err(unavailable("OpenClipboard failed"));
            }
            Ok(Self)
        }
    }

    impl Drop for OpenGuard {
        fn drop(&mut self) {
            unsafe {
                CloseClipboard();
            }
        }
    }

    fn unavailable(message: &str) -> ClipboardError {
        ClipboardError::Unavailable(message.to_owned())
    }

    impl PlatformClipboard for Clipboard {
        fn snapshot(&self) -> Result<ClipboardSnapshot, ClipboardError> {
            let _guard = OpenGuard::open()?;
            let change_count = unsafe { GetClipboardSequenceNumber() } as u64;
            if unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT) } == 0 {
                return Ok(ClipboardSnapshot { change_count, ..Default::default() });
            }
            unsafe {
                let handle = GetClipboardData(CF_UNICODETEXT);
                if handle.is_null() {
                    return Err(unavailable("GetClipboardData failed"));
                }
                let units = GlobalSize(handle) / std::mem::size_of::<u16>();
                if units == 0 {
                    return Err(unavailable("clipboard allocation is empty"));
                }
                let ptr = GlobalLock(handle) as *const u16;
                if ptr.is_null() {
                    return Err(unavailable("GlobalLock failed"));
                }
                // Bound scanning even if another process supplied malformed data.
                let limit = units.min(ferryx_rd_proto::MAX_CLIPBOARD_TEXT_BYTES + 1);
                let slice = std::slice::from_raw_parts(ptr, limit);
                let end = slice.iter().position(|unit| *unit == 0);
                let text = end.map(|end| String::from_utf16(&slice[..end]));
                GlobalUnlock(handle);
                let text = text.ok_or_else(|| unavailable("clipboard text exceeds the limit or lacks NUL"))?
                    .map_err(|_| unavailable("clipboard text is not valid UTF-16"))?;
                if text.len() > ferryx_rd_proto::MAX_CLIPBOARD_TEXT_BYTES {
                    return Err(unavailable("clipboard text exceeds the protocol limit"));
                }
                Ok(ClipboardSnapshot {
                    change_count,
                    text: Some(text),
                    types: Vec::new(),
                })
            }
        }

        fn set_text(&self, text: &str) -> Result<u64, ClipboardError> {
            if text.contains('\0') {
                return Err(unavailable("clipboard text contains NUL"));
            }
            let _guard = OpenGuard::open()?;
            let units: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
            unsafe {
                let handle = GlobalAlloc(GMEM_MOVEABLE, units.len() * std::mem::size_of::<u16>());
                if handle.is_null() {
                    return Err(unavailable("GlobalAlloc failed"));
                }
                let ptr = GlobalLock(handle) as *mut u16;
                if ptr.is_null() {
                    GlobalFree(handle);
                    return Err(unavailable("GlobalLock failed"));
                }
                std::ptr::copy_nonoverlapping(units.as_ptr(), ptr, units.len());
                GlobalUnlock(handle);
                if EmptyClipboard() == 0 || SetClipboardData(CF_UNICODETEXT, handle).is_null() {
                    GlobalFree(handle);
                    return Err(unavailable("Windows rejected clipboard text"));
                }
                // SetClipboardData transferred allocation ownership to Windows.
                Ok(GetClipboardSequenceNumber() as u64)
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use std::{
        env,
        io::{Read, Write},
        os::{fd::AsRawFd, unix::fs::PermissionsExt},
        process::{Child, Command, Stdio},
        sync::Mutex,
        time::{Duration, Instant},
    };

    #[derive(Default)]
    pub struct Clipboard {
        observed: Mutex<(u64, Option<String>)>,
    }

    #[derive(Clone, Copy)]
    enum Backend {
        Wayland,
        X11,
    }

    fn unavailable(message: impl ToString) -> ClipboardError {
        ClipboardError::Unavailable(message.to_string())
    }

    fn installed(name: &str) -> bool {
        env::var_os("PATH").is_some_and(|path| {
            env::split_paths(&path).any(|dir| {
                dir.join(name).metadata().is_ok_and(|meta| {
                    meta.is_file() && meta.permissions().mode() & 0o111 != 0
                })
            })
        })
    }

    fn backend() -> Result<Backend, ClipboardError> {
        if env::var_os("WAYLAND_DISPLAY").is_some() && installed("wl-copy") && installed("wl-paste") {
            Ok(Backend::Wayland)
        } else if env::var_os("DISPLAY").is_some() && installed("xclip") {
            Ok(Backend::X11)
        } else {
            Err(unavailable("install wl-clipboard for Wayland or xclip for X11 clipboard support"))
        }
    }

    // Kill/reap on every error, including pipe setup and I/O failures.
    struct ChildGuard(Child);

    impl Drop for ChildGuard {
        fn drop(&mut self) {
            if !matches!(self.0.try_wait(), Ok(Some(_))) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
    }

    fn nonblocking(fd: i32) -> Result<(), ClipboardError> {
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(unavailable(std::io::Error::last_os_error()));
        }
        Ok(())
    }

    /// Bound stdin, stdout and process exit by one deadline. Do not wait for
    /// inherited pipes after exit: wl-copy/xclip fork selection-owner daemons.
    fn command(
        program: &str,
        args: &[&str],
        input: Option<&[u8]>,
        limit: usize,
    ) -> Result<Vec<u8>, ClipboardError> {
        let mut child = ChildGuard(
            Command::new(program)
                .args(args)
                .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
                .stdout(if input.is_some() { Stdio::null() } else { Stdio::piped() })
                .stderr(Stdio::null())
                .spawn()
                .map_err(unavailable)?,
        );
        let deadline = Instant::now() + Duration::from_secs(1);
        let mut stdin = child.0.stdin.take();
        let mut stdout = child.0.stdout.take();
        if let Some(pipe) = &stdin {
            nonblocking(pipe.as_raw_fd())?;
        }
        if let Some(pipe) = &stdout {
            nonblocking(pipe.as_raw_fd())?;
        }
        let mut offset = 0;
        let mut bytes = Vec::new();
        let mut eof = input.is_some();
        loop {
            if Instant::now() >= deadline {
                return Err(unavailable("clipboard command timed out"));
            }
            if let Some(pipe) = stdin.as_mut() {
                let input = input.expect("stdin exists only for writes");
                match pipe.write(&input[offset..]) {
                    Ok(0) if offset < input.len() => return Err(unavailable("clipboard stdin closed")),
                    Ok(count) => offset += count,
                    Err(err) if matches!(err.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted) => {}
                    Err(err) => return Err(unavailable(err)),
                }
                if offset == input.len() {
                    stdin = None;
                }
            }
            if let Some(pipe) = stdout.as_mut() {
                let mut chunk = [0; 1024];
                match pipe.read(&mut chunk) {
                    Ok(0) => {
                        eof = true;
                        stdout = None;
                    }
                    Ok(count) => {
                        bytes.extend_from_slice(&chunk[..count]);
                        if bytes.len() > limit {
                            return Err(unavailable("clipboard output exceeds the limit"));
                        }
                        // Drain available output before waiting for more.
                        continue;
                    }
                    Err(err) if matches!(err.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted) => {}
                    Err(err) => return Err(unavailable(err)),
                }
            }
            if let Some(status) = child.0.try_wait().map_err(unavailable)? {
                if !status.success() || stdin.is_some() {
                    return Err(unavailable(format!("{program} failed: {status}")));
                }
                if eof {
                    return Ok(bytes);
                }
            }
            let mut fds = Vec::new();
            if let Some(pipe) = &stdin {
                fds.push(libc::pollfd { fd: pipe.as_raw_fd(), events: libc::POLLOUT, revents: 0 });
            }
            if let Some(pipe) = &stdout {
                fds.push(libc::pollfd { fd: pipe.as_raw_fd(), events: libc::POLLIN, revents: 0 });
            }
            let timeout = deadline.saturating_duration_since(Instant::now()).as_millis().min(20) as i32;
            if unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout) } < 0 {
                let err = std::io::Error::last_os_error();
                if err.kind() != std::io::ErrorKind::Interrupted {
                    return Err(unavailable(err));
                }
            }
        }
    }

    impl Clipboard {
        fn observe(&self, text: Option<String>) -> Result<u64, ClipboardError> {
            let mut observed = self.observed.lock().map_err(unavailable)?;
            if observed.1 != text {
                observed.0 = observed.0.wrapping_add(1);
                observed.1 = text;
            }
            Ok(observed.0)
        }
    }

    impl PlatformClipboard for Clipboard {
        fn snapshot(&self) -> Result<ClipboardSnapshot, ClipboardError> {
            let mut types = Vec::new();
            let bytes = match backend()? {
                Backend::Wayland => {
                    let listed = command("wl-paste", &["--list-types"], None, 64 * 1024)?;
                    types = String::from_utf8(listed).map_err(unavailable)?
                        .lines().map(str::to_owned).collect();
                    if types.iter().any(|ty| ferryx_rd_app::CONCEALED_CLIPBOARD_TYPES.contains(&ty.as_str())) {
                        return Ok(ClipboardSnapshot {
                            change_count: self.observe(None)?, text: None, types,
                        });
                    }
                    command("wl-paste", &["--no-newline", "--type", "text/plain;charset=utf-8"], None,
                        ferryx_rd_proto::MAX_CLIPBOARD_TEXT_BYTES)
                        .or_else(|_| command("wl-paste", &["--no-newline", "--type", "text/plain"], None,
                            ferryx_rd_proto::MAX_CLIPBOARD_TEXT_BYTES))?
                }
                Backend::X11 => command("xclip", &["-selection", "clipboard", "-out"], None,
                    ferryx_rd_proto::MAX_CLIPBOARD_TEXT_BYTES)?,
            };
            let text = String::from_utf8(bytes).map_err(unavailable)?;
            let change_count = self.observe(Some(text.clone()))?;
            Ok(ClipboardSnapshot { change_count, text: Some(text), types })
        }

        fn set_text(&self, text: &str) -> Result<u64, ClipboardError> {
            match backend()? {
                Backend::Wayland => command("wl-copy", &["--type", "text/plain;charset=utf-8"], Some(text.as_bytes()), 0)?,
                Backend::X11 => command("xclip", &["-selection", "clipboard", "-in"], Some(text.as_bytes()), 0)?,
            };
            self.observe(Some(text.to_owned()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn pcm(samples: &[f32]) -> Vec<u8> {
        samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect()
    }

    fn headless_player() -> RdAudioPlayer {
        RdAudioPlayer {
            unavailable: true,
            ..RdAudioPlayer::default()
        }
    }

    #[test]
    fn decoded_pcm_is_queued_without_opening_hardware() {
        let mut player = headless_player();
        player.push_pcm(&pcm(&[0.25, -0.5, 0.75, -1.0]));
        assert_eq!(player.queue.queued_samples().unwrap(), 4);
        let mut output = [0.0; 4];
        assert_eq!(player.queue.drain_into(&mut output).unwrap(), 4);
        assert_eq!(output, [0.25, -0.5, 0.75, -1.0]);
    }

    #[test]
    fn audio_controls_clamp_before_setting_the_queue() {
        let mut player = headless_player();
        player.set_audio(false, 1.5);
        assert_eq!(player.volume, 1.0);
        assert!(!player.muted);
        player.push_pcm(&pcm(&[1.0, -1.0]));
        let mut output = [0.0; 2];
        player.queue.drain_into(&mut output).unwrap();
        assert_eq!(output, [1.0, -1.0]);

        player.set_audio(true, -0.5);
        assert_eq!(player.volume, 0.0);
        assert!(player.muted);
        player.push_pcm(&pcm(&[1.0, -1.0]));
        player.queue.drain_into(&mut output).unwrap();
        assert_eq!(output, [0.0; 2]);

        // Test mute separately from zero gain: muted playback still consumes PCM.
        player.set_audio(true, 1.0);
        player.push_pcm(&pcm(&[1.0, -1.0]));
        assert_eq!(player.queue.drain_into(&mut output).unwrap(), 2);
        assert_eq!(output, [0.0; 2]);
        assert_eq!(player.queue.queued_samples().unwrap(), 0);
    }

    #[test]
    fn nonfinite_volume_is_silenced() {
        let mut player = headless_player();
        for volume in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            player.set_audio(false, volume);
            assert_eq!(player.volume, 0.0);
            player.push_pcm(&pcm(&[1.0, 1.0]));
            let mut output = [1.0; 2];
            player.queue.drain_into(&mut output).unwrap();
            assert_eq!(output, [0.0; 2]);
        }
    }

    #[test]
    fn failed_device_start_is_latched_without_losing_pcm() {
        let mut player = RdAudioPlayer::default();
        let bytes = pcm(&[0.5, -0.5]);
        let mut attempts = 0;
        player.push_pcm_with_start(&bytes, |_| {
            attempts += 1;
            Err(ferryx_rd_render::AudioError::NoOutputDevice)
        });
        assert_eq!(attempts, 1);
        assert!(player.unavailable);
        assert!(player.output.is_none());
        player.push_pcm_with_start(&bytes, |_| panic!("device start must not be retried"));
        assert_eq!(player.queue.queued_samples().unwrap(), 4);
        assert_eq!(player.stats(), (false, 0, 0));
    }

    #[test]
    fn stop_clears_pcm_and_preserves_failure_latch() {
        let mut player = headless_player();
        player.push_pcm(&pcm(&[0.5, -0.5]));
        player.stop();
        assert_eq!(player.queue.queued_samples().unwrap(), 0);
        assert!(player.output.is_none());
        assert!(player.unavailable);
    }

    fn shape(shape_id: u32, rgba: [u8; 4]) -> ferryx_rd_proto::CursorShape {
        ferryx_rd_proto::CursorShape {
            shape_id,
            width: 1,
            height: 1,
            hotspot_x: 0,
            hotspot_y: 0,
            rgba: rgba.to_vec(),
        }
    }

    fn decode_cursor(frame: &[u8]) -> serde_json::Value {
        assert_eq!(frame[0], FRAME_KIND_CURSOR);
        serde_json::from_slice(&frame[1..]).unwrap()
    }

    #[test]
    fn cursor_images_are_framed_and_deduplicated_by_shape_id() {
        let mut encoder = RdCursorEncoder::default();
        let mut cursor = ferryx_rd_app::CursorState {
            x: 0.25,
            y: 0.75,
            cursor_type: 0,
            shape_id: 7,
        };
        let first = decode_cursor(&encoder.encode(&cursor, |id| Some(shape(id, [1, 2, 3, 255]))));
        assert_eq!(first["shapeId"], 7);
        assert_eq!(first["rgbaBase64"], "AQID/w==");
        assert_eq!(first["width"], 1);
        assert_eq!(first["height"], 1);
        assert_eq!(first["hotspotX"], 0);
        assert_eq!(first["hotspotY"], 0);
        assert_eq!(first["visible"], true);
        let second = decode_cursor(&encoder.encode(&cursor, |_| panic!("shape already sent")));
        assert!(second.get("rgbaBase64").is_none());
        assert!(second.get("width").is_none());
        assert!(second.get("hotspotX").is_none());
        cursor.shape_id = 8;
        let third = decode_cursor(&encoder.encode(&cursor, |id| Some(shape(id, [4, 5, 6, 255]))));
        assert_eq!(third["shapeId"], 8);
        assert_eq!(third["rgbaBase64"], "BAUG/w==");
    }

    #[test]
    fn missing_shapes_are_retried_when_the_cache_catches_up() {
        let mut encoder = RdCursorEncoder::default();
        let cursor = ferryx_rd_app::CursorState {
            shape_id: 7,
            ..Default::default()
        };
        let missing = decode_cursor(&encoder.encode(&cursor, |_| None));
        assert!(missing.get("rgbaBase64").is_none());
        let available = decode_cursor(&encoder.encode(&cursor, |id| Some(shape(id, [0; 4]))));
        assert_eq!(available["rgbaBase64"], "AAAAAA==");
    }

    #[test]
    fn legacy_cursor_has_no_shape_and_hidden_sentinel_is_invisible() {
        let mut encoder = RdCursorEncoder::default();
        let frame = encoder.encode(&ferryx_rd_app::CursorState::default(), |_| panic!("no shape"));
        let payload: RdCursorPayload = serde_json::from_slice(&frame[1..]).unwrap();
        assert_eq!(payload.shape_id, None);
        assert!(!payload.visible);
    }

    #[derive(Clone, Default)]
    struct MockClipboard {
        state: Arc<Mutex<(ClipboardSnapshot, usize)>>,
    }

    impl PlatformClipboard for MockClipboard {
        fn snapshot(&self) -> Result<ClipboardSnapshot, ClipboardError> {
            Ok(self.state.lock().unwrap().0.clone())
        }

        fn set_text(&self, text: &str) -> Result<u64, ClipboardError> {
            let mut state = self.state.lock().unwrap();
            state.0.change_count += 1;
            state.0.text = Some(text.to_owned());
            state.1 += 1;
            Ok(state.0.change_count)
        }
    }

    #[test]
    fn clipboard_remote_echo_is_suppressed_but_local_changes_are_sent() {
        let clipboard = MockClipboard::default();
        let mut bridge = RdClipboardBridge::with_clipboard(clipboard.clone());
        bridge.apply_remote_text("hello", true);
        assert_eq!(clipboard.state.lock().unwrap().1, 1);
        assert_eq!(bridge.poll_local_text(true), None);
        assert_eq!(bridge.poll_local_text(true), None);
        clipboard.set_text("world").unwrap();
        assert_eq!(bridge.poll_local_text(true), Some("world".to_owned()));
        assert_eq!(bridge.poll_local_text(true), None);
    }

    #[test]
    fn clipboard_permission_denial_does_not_write_or_consume_changes() {
        let clipboard = MockClipboard::default();
        let mut bridge = RdClipboardBridge::with_clipboard(clipboard.clone());
        bridge.apply_remote_text("x", false);
        assert_eq!(clipboard.state.lock().unwrap().1, 0);
        assert_eq!(clipboard.snapshot().unwrap().text, None);
        clipboard.set_text("world").unwrap();
        assert_eq!(bridge.poll_local_text(false), None);
        assert_eq!(bridge.poll_local_text(true), Some("world".to_owned()));
    }
}
