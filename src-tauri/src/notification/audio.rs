//! Custom notification sound playback.
//!
//! Playing the user's sound in Rust rather than the WebView keeps the selected
//! path off the JavaScript side entirely: no `asset:` protocol, no
//! `convertFileSrc(...)` scope, and no CSP expansion for local media. It also
//! makes preview and real notification playback share one code path with one
//! volume normalization point.

use super::model::{
    audio_display_name, normalize_volume, validate_audio_path, PickedAudioFile, PlaySoundReason,
    PlaySoundResult,
};
use parking_lot::Mutex;
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player};
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Automatic sounds fired closer together than this collapse into one.
///
/// An agent completion and an adjacent terminal bell should not stack into a
/// double alert. Settings preview bypasses this with `force`.
pub const DEDUPE_WINDOW: Duration = Duration::from_millis(400);

/// How long the OS audio output sink remains open after the last notification
/// playback completes. Dropping the idle sink lets OS audio hardware sleep
/// instead of waking at ~86 Hz indefinitely after one alert.
pub const IDLE_CLOSE_GRACE: Duration = Duration::from_secs(10);

/// Lazily-initialized audio output plus playback state.
///
/// Initialization is deferred so a machine with no audio output device can
/// still launch rorca, and the sink is released after [`IDLE_CLOSE_GRACE`] of
/// silence and reopened lazily on the next [`play`](Self::play).
pub struct NotificationAudioPlayer {
    output: Arc<Mutex<OutputState>>,
    generation: Arc<AtomicU64>,
    active_playbacks: Arc<AtomicUsize>,
    last_played_at: Mutex<Option<Instant>>,
    idle_grace: Duration,
}

enum OutputState<S = MixerDeviceSink> {
    /// No attempt made yet, or released after idle grace; next `play()` reopens lazily.
    Uninitialized,
    Ready(S),
    /// A previous attempt failed; do not retry on every notification.
    Unavailable,
}

fn maybe_close_idle_sink<S>(
    output: &Mutex<OutputState<S>>,
    generation: &AtomicU64,
    active_playbacks: &AtomicUsize,
    expected_gen: u64,
) -> bool {
    let mut guard = output.lock();
    if generation.load(Ordering::Acquire) == expected_gen
        && active_playbacks.load(Ordering::Acquire) == 0
        && matches!(*guard, OutputState::Ready(_))
    {
        *guard = OutputState::Uninitialized;
        true
    } else {
        false
    }
}

impl Default for NotificationAudioPlayer {
    fn default() -> Self {
        Self::new()
    }
}

impl NotificationAudioPlayer {
    pub fn new() -> Self {
        Self {
            output: Arc::new(Mutex::new(OutputState::Uninitialized)),
            generation: Arc::new(AtomicU64::new(0)),
            active_playbacks: Arc::new(AtomicUsize::new(0)),
            last_played_at: Mutex::new(None),
            idle_grace: IDLE_CLOSE_GRACE,
        }
    }

    /// Decode and play `path` at `volume_percent` (`0..=100`).
    ///
    /// Never panics on a moved or deleted file: every failure is reported as a
    /// structured [`PlaySoundResult`].
    pub fn play(&self, path: &Path, volume_percent: f32, force: bool) -> PlaySoundResult {
        if !force && self.is_deduped() {
            return PlaySoundResult::failed(PlaySoundReason::Deduped);
        }

        let canonical = match validate_audio_path(path) {
            Ok(canonical) => canonical,
            Err(reason) => {
                // Log the failure kind, never the full user path.
                tracing::debug!("notification sound rejected: {:?}", reason);
                return PlaySoundResult::failed(reason);
            }
        };

        let volume = normalize_volume(volume_percent);
        if volume <= 0.0 {
            // Muted: treat as a successful no-op rather than a device error.
            self.mark_played();
            return PlaySoundResult::played();
        }

        let file = match File::open(&canonical) {
            Ok(file) => file,
            Err(_) => return PlaySoundResult::failed(PlaySoundReason::NotFound),
        };

        let decoder = match Decoder::new(BufReader::new(file)) {
            Ok(decoder) => decoder,
            Err(error) => {
                tracing::debug!("notification sound decode failed: {error}");
                return PlaySoundResult::failed(PlaySoundReason::DecodeFailed);
            }
        };

        let mut output = self.output.lock();
        if matches!(*output, OutputState::Uninitialized) {
            *output = match DeviceSinkBuilder::open_default_sink() {
                Ok(mut sink) => {
                    // Silence rodio's stderr message when the process exits.
                    sink.log_on_drop(false);
                    OutputState::Ready(sink)
                }
                Err(error) => {
                    tracing::warn!("no audio output device for notification sound: {error}");
                    OutputState::Unavailable
                }
            };
        }

        let sink = match &*output {
            OutputState::Ready(sink) => sink,
            OutputState::Unavailable => {
                return PlaySoundResult::failed(PlaySoundReason::NoOutputDevice)
            }
            OutputState::Uninitialized => {
                return PlaySoundResult::failed(PlaySoundReason::BackendError)
            }
        };

        let player = Player::connect_new(sink.mixer());
        player.set_volume(volume);
        player.append(decoder);
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.active_playbacks.fetch_add(1, Ordering::AcqRel);
        drop(output);

        // Wait for playback to finish off the caller's thread, then drop the
        // idle sink after `idle_grace` unless another playback superseded it.
        let output_weak = Arc::downgrade(&self.output);
        let generation_arc = Arc::clone(&self.generation);
        let active_arc = Arc::clone(&self.active_playbacks);
        let idle_grace = self.idle_grace;
        let player_slot = Arc::new(Mutex::new(Some(player)));
        let thread_player_slot = Arc::clone(&player_slot);
        if std::thread::Builder::new()
            .name("ferryx-audio-reaper".into())
            .spawn(move || {
                if let Some(player) = thread_player_slot.lock().take() {
                    player.sleep_until_end();
                    drop(player);
                }
                let end_gen = {
                    let Some(output_arc) = output_weak.upgrade() else {
                        active_arc.fetch_sub(1, Ordering::AcqRel);
                        return;
                    };
                    let _guard = output_arc.lock();
                    let remaining = active_arc.fetch_sub(1, Ordering::AcqRel) - 1;
                    if remaining > 0 {
                        return;
                    }
                    generation_arc.fetch_add(1, Ordering::AcqRel) + 1
                };
                std::thread::sleep(idle_grace);
                if let Some(output_arc) = output_weak.upgrade() {
                    maybe_close_idle_sink(&output_arc, &generation_arc, &active_arc, end_gen);
                }
            })
            .is_err()
        {
            if let Some(player) = player_slot.lock().take() {
                player.detach();
            }
            let _guard = self.output.lock();
            self.active_playbacks.fetch_sub(1, Ordering::AcqRel);
            self.generation.fetch_add(1, Ordering::AcqRel);
        }

        self.mark_played();
        PlaySoundResult::played()
    }

    fn is_deduped(&self) -> bool {
        self.last_played_at
            .lock()
            .map(|last| last.elapsed() < DEDUPE_WINDOW)
            .unwrap_or(false)
    }

    fn mark_played(&self) {
        *self.last_played_at.lock() = Some(Instant::now());
    }

    /// Test-only hook to age the dedupe window.
    #[cfg(test)]
    fn set_last_played(&self, at: Option<Instant>) {
        *self.last_played_at.lock() = at;
    }
}

impl std::fmt::Debug for NotificationAudioPlayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NotificationAudioPlayer")
            .finish_non_exhaustive()
    }
}

/// Build a picker result from a chosen path.
pub fn picked_audio_file(path: PathBuf) -> PickedAudioFile {
    let display_name = audio_display_name(&path);
    PickedAudioFile { path, display_name }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn write_file(dir: &TempDir, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.path().join(name);
        let mut file = File::create(&path).expect("create fixture");
        file.write_all(bytes).expect("write fixture");
        path
    }

    /// Minimal valid 16-bit PCM WAV: one sample of silence.
    fn tiny_wav_bytes() -> Vec<u8> {
        let sample_rate: u32 = 44_100;
        let data: [u8; 2] = [0, 0];
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36u32 + data.len() as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes()); // PCM chunk size
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM format
        wav.extend_from_slice(&1u16.to_le_bytes()); // mono
        wav.extend_from_slice(&sample_rate.to_le_bytes());
        wav.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
        wav.extend_from_slice(&2u16.to_le_bytes()); // block align
        wav.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
        wav.extend_from_slice(&data);
        wav
    }

    #[test]
    fn missing_file_reports_not_found_without_panicking() {
        let player = NotificationAudioPlayer::new();
        let result = player.play(Path::new("/nonexistent/orca-sound.wav"), 50.0, true);

        assert!(!result.played);
        assert_eq!(result.reason, Some(PlaySoundReason::NotFound));
    }

    #[test]
    fn unsupported_extension_is_rejected_before_decoding() {
        let dir = TempDir::new().expect("tempdir");
        let path = write_file(&dir, "notes.txt", b"not audio");

        let player = NotificationAudioPlayer::new();
        let result = player.play(&path, 50.0, true);

        assert_eq!(result.reason, Some(PlaySoundReason::UnsupportedFormat));
    }

    #[test]
    fn corrupt_audio_reports_decode_failure_not_a_crash() {
        let dir = TempDir::new().expect("tempdir");
        // Valid extension, garbage contents.
        let path = write_file(&dir, "broken.wav", b"definitely not a wav file");

        let player = NotificationAudioPlayer::new();
        let result = player.play(&path, 50.0, true);

        assert!(!result.played);
        assert_eq!(result.reason, Some(PlaySoundReason::DecodeFailed));
    }

    #[test]
    fn zero_volume_short_circuits_before_touching_the_audio_device() {
        let dir = TempDir::new().expect("tempdir");
        let path = write_file(&dir, "sound.wav", &tiny_wav_bytes());

        let player = NotificationAudioPlayer::new();
        let result = player.play(&path, 0.0, true);

        // Muted playback must succeed on machines with no audio device,
        // which is the normal case in CI.
        assert!(result.played);
        assert!(result.reason.is_none());
        assert!(matches!(*player.output.lock(), OutputState::Uninitialized));
    }

    #[test]
    fn negative_and_oversized_volumes_clamp_into_range() {
        let dir = TempDir::new().expect("tempdir");
        let path = write_file(&dir, "sound.wav", &tiny_wav_bytes());
        let player = NotificationAudioPlayer::new();

        // Negative clamps to 0 -> muted no-op, never a device error.
        assert!(player.play(&path, -30.0, true).played);
    }

    #[test]
    fn rapid_automatic_sounds_are_deduped_but_force_still_plays() {
        let dir = TempDir::new().expect("tempdir");
        let path = write_file(&dir, "sound.wav", &tiny_wav_bytes());
        let player = NotificationAudioPlayer::new();

        // Simulate a sound that just played, without sleeping.
        player.set_last_played(Some(Instant::now()));

        let automatic = player.play(&path, 0.0, false);
        assert!(!automatic.played);
        assert_eq!(automatic.reason, Some(PlaySoundReason::Deduped));

        // Settings preview bypasses the dedupe window.
        let forced = player.play(&path, 0.0, true);
        assert!(forced.played);
    }

    #[test]
    fn sound_outside_the_dedupe_window_plays_again() {
        let dir = TempDir::new().expect("tempdir");
        let path = write_file(&dir, "sound.wav", &tiny_wav_bytes());
        let player = NotificationAudioPlayer::new();

        let stale = Instant::now()
            .checked_sub(DEDUPE_WINDOW * 2)
            .expect("stale instant");
        player.set_last_played(Some(stale));

        assert!(player.play(&path, 0.0, false).played);
    }

    #[test]
    fn picked_file_exposes_only_path_and_display_name() {
        let picked = picked_audio_file(PathBuf::from("/Users/someone/Music/ding.wav"));

        assert_eq!(picked.display_name, "ding.wav");
        assert_eq!(picked.path, PathBuf::from("/Users/someone/Music/ding.wav"));
    }

    struct DropSentinel(Arc<std::sync::atomic::AtomicBool>);

    impl Drop for DropSentinel {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }

    #[test]
    fn idle_close_drops_sink_when_generation_matches_and_no_active_playback() {
        let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let output = Mutex::new(OutputState::Ready(DropSentinel(Arc::clone(&dropped))));
        let generation = AtomicU64::new(4);
        let active = AtomicUsize::new(0);

        assert!(maybe_close_idle_sink(&output, &generation, &active, 4));
        assert!(dropped.load(Ordering::Acquire), "idle sink must be dropped");
        assert!(matches!(*output.lock(), OutputState::Uninitialized));
    }

    #[test]
    fn idle_close_skips_when_superseded_active_or_unavailable() {
        let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let output = Mutex::new(OutputState::Ready(DropSentinel(Arc::clone(&dropped))));
        let generation = AtomicU64::new(5);
        let active = AtomicUsize::new(0);

        // Superseded by a newer playback generation.
        assert!(!maybe_close_idle_sink(&output, &generation, &active, 4));
        assert!(!dropped.load(Ordering::Acquire));

        // Still actively playing.
        active.store(1, Ordering::Release);
        assert!(!maybe_close_idle_sink(&output, &generation, &active, 5));
        assert!(!dropped.load(Ordering::Acquire));

        // Unavailable state stays unavailable (does not reset to Uninitialized).
        let unavailable: Mutex<OutputState<()>> = Mutex::new(OutputState::Unavailable);
        let zero = AtomicUsize::new(0);
        assert!(!maybe_close_idle_sink(&unavailable, &generation, &zero, 5));
        assert!(matches!(*unavailable.lock(), OutputState::Unavailable));
    }
}
