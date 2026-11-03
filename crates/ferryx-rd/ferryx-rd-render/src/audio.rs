use std::sync::{
    atomic::{fence, AtomicBool, AtomicU32, AtomicU64, Ordering},
    Arc, Mutex,
};

#[cfg(any(feature = "cpal-output", test))]
use std::{sync::atomic::AtomicI32, time::Instant};

#[cfg(any(feature = "cpal-output", test))]
use std::sync::mpsc::SyncSender;

#[cfg(feature = "cpal-output")]
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    BufferSize, Device, FromSample, SampleFormat, SizedSample, Stream, StreamConfig,
    SupportedBufferSize, SupportedStreamConfigRange,
};
use thiserror::Error;

/// Wire PCM sample rate (little-endian f32 stereo).
pub const AUDIO_SAMPLE_RATE: u32 = 48_000;
/// Wire PCM channel count.
pub const AUDIO_CHANNELS: u16 = 2;
/// 100 ms of interleaved 48 kHz stereo PCM, measured in samples (not frames).
/// This is the hard safety bound on queued audio; steady-state occupancy is
/// held near [`AUDIO_TARGET_OCCUPANCY_FRAMES`] by the output renderer.
pub const AUDIO_QUEUE_CAPACITY: usize = AUDIO_SAMPLE_RATE as usize * AUDIO_CHANNELS as usize / 10;
/// Default steady-state queue occupancy (30 ms of wire frames) the drift
/// controller steers towards.
pub const AUDIO_TARGET_OCCUPANCY_FRAMES: usize = AUDIO_SAMPLE_RATE as usize * 30 / 1000;

/// Physical ring size in samples (power of two, much larger than the logical
/// capacity so a reader can always detect that a slot was recycled).
const RING_SLOTS: usize = 1 << 15;
const RING_MASK: u64 = RING_SLOTS as u64 - 1;
const _: () = assert!(RING_SLOTS >= AUDIO_QUEUE_CAPACITY * 2);

/// Maximum rate trim applied by the drift controller (1 %).
const MAX_DRIFT_ADJUSTMENT: f64 = 0.01;
/// Smoothing applied to the per-callback occupancy measurement.
const OCCUPANCY_SMOOTHING: f64 = 0.1;

/// Shared live PCM queue: a bounded lock-free ring (one producer at a time, one
/// real-time consumer). The consumer path never takes a lock; gain, mute and
/// counters are atomics. Overflow discards only the oldest complete stereo
/// frames. Inputs must be finite and frame-aligned; invalid input never changes
/// the queue (validation happens before the ring is touched).
#[derive(Debug, Clone)]
pub struct AudioQueue {
    inner: Arc<Ring>,
}

#[derive(Debug)]
struct Ring {
    slots: Box<[AtomicU32]>,
    /// Sample index announced before slot writes begin (seqlock sequence).
    reserve: AtomicU64,
    /// Sample index published after slot writes complete.
    write: AtomicU64,
    /// Next sample the consumer will read.
    read: AtomicU64,
    /// Samples below this index are logically discarded (overflow or clear).
    discard: AtomicU64,
    gain: AtomicU32,
    muted: AtomicBool,
    underrun_samples: AtomicU64,
    dropped_samples: AtomicU64,
    /// Serialises producers only; the consumer never touches it.
    producer: Mutex<()>,
}

impl Default for AudioQueue {
    fn default() -> Self {
        Self {
            inner: Arc::new(Ring {
                slots: (0..RING_SLOTS).map(|_| AtomicU32::new(0)).collect(),
                reserve: AtomicU64::new(0),
                write: AtomicU64::new(0),
                read: AtomicU64::new(0),
                discard: AtomicU64::new(0),
                gain: AtomicU32::new(1.0f32.to_bits()),
                muted: AtomicBool::new(false),
                underrun_samples: AtomicU64::new(0),
                dropped_samples: AtomicU64::new(0),
                producer: Mutex::new(()),
            }),
        }
    }
}

impl Ring {
    fn queued(&self) -> u64 {
        let write = self.write.load(Ordering::Acquire);
        let start = self
            .read
            .load(Ordering::Acquire)
            .max(self.discard.load(Ordering::Acquire));
        write.saturating_sub(start)
    }

    fn gain(&self) -> f32 {
        if self.muted.load(Ordering::Relaxed) {
            0.0
        } else {
            f32::from_bits(self.gain.load(Ordering::Relaxed))
        }
    }

    /// Pop one wire frame, applying volume/mute. Wait-free for the consumer
    /// except for a retry when the producer recycled the slot being read.
    fn pop_frame(&self) -> Option<[f32; 2]> {
        loop {
            let write = self.write.load(Ordering::Acquire);
            let read = self
                .read
                .load(Ordering::Relaxed)
                .max(self.discard.load(Ordering::Acquire));
            if write.saturating_sub(read) < 2 {
                return None;
            }
            let left = self.slots[(read & RING_MASK) as usize].load(Ordering::Relaxed);
            let right = self.slots[((read + 1) & RING_MASK) as usize].load(Ordering::Relaxed);
            fence(Ordering::Acquire);
            if self.reserve.load(Ordering::Relaxed).saturating_sub(read) > RING_SLOTS as u64 {
                // Slot may have been recycled mid-read; discard has moved past
                // `read`, so the retry makes progress.
                continue;
            }
            self.read.store(read + 2, Ordering::Release);
            let gain = self.gain();
            return Some([f32::from_bits(left) * gain, f32::from_bits(right) * gain]);
        }
    }

    fn append(&self, samples: impl ExactSizeIterator<Item = f32>) -> Result<(), AudioError> {
        let _producer = self.producer.lock().map_err(|_| AudioError::Poisoned)?;
        let skip = samples.len().saturating_sub(AUDIO_QUEUE_CAPACITY);
        let retained = (samples.len() - skip) as u64;
        let write = self.write.load(Ordering::Relaxed);
        let new_write = write + retained;
        let bound = new_write.saturating_sub(AUDIO_QUEUE_CAPACITY as u64);
        let live_start = self
            .read
            .load(Ordering::Acquire)
            .max(self.discard.load(Ordering::Acquire));
        let dropped = bound.saturating_sub(live_start) + skip as u64;
        if dropped > 0 {
            self.dropped_samples.fetch_add(dropped, Ordering::Relaxed);
        }
        self.discard.fetch_max(bound, Ordering::AcqRel);
        self.reserve.store(new_write, Ordering::Relaxed);
        fence(Ordering::Release);
        for (offset, sample) in samples.skip(skip).enumerate() {
            self.slots[((write + offset as u64) & RING_MASK) as usize]
                .store(sample.to_bits(), Ordering::Relaxed);
        }
        self.write.store(new_write, Ordering::Release);
        Ok(())
    }
}

impl AudioQueue {
    /// Enqueue little-endian f32 stereo PCM, validating the entire input first.
    pub fn push_pcm_bytes(&self, bytes: &[u8]) -> Result<(), AudioError> {
        if bytes.len() % (std::mem::size_of::<f32>() * AUDIO_CHANNELS as usize) != 0 {
            return Err(AudioError::MisalignedPcm(bytes.len()));
        }
        let samples = bytes
            .chunks_exact(4)
            .map(|sample| f32::from_le_bytes(sample.try_into().expect("four-byte chunk")));
        if samples.clone().any(|sample| !sample.is_finite()) {
            return Err(AudioError::NonFinitePcm);
        }
        // Immutable bytes allow a validation pass followed by copying only the
        // newest bounded tail straight into the ring.
        self.inner.append(samples)
    }

    pub fn push_samples(&self, samples: impl IntoIterator<Item = f32>) -> Result<(), AudioError> {
        // Stage at most 100 ms, outside the ring. Every input sample is
        // validated (invalid input never changes the queue), but once staging
        // is full each new sample overwrites the oldest slot in place; a single
        // rotation afterwards restores newest-wins order.
        let mut pending: Vec<f32> = Vec::new();
        let mut ring = 0usize;
        let mut overflowed = false;
        let mut staged_dropped = 0u64;
        let mut input = samples.into_iter();
        while let Some(left) = input.next() {
            let right = input.next().ok_or(AudioError::MisalignedSamples)?;
            if !left.is_finite() || !right.is_finite() {
                return Err(AudioError::NonFinitePcm);
            }
            if pending.len() < AUDIO_QUEUE_CAPACITY {
                pending.push(left);
                pending.push(right);
            } else {
                pending[ring] = left;
                ring = (ring + 1) % AUDIO_QUEUE_CAPACITY;
                pending[ring] = right;
                ring = (ring + 1) % AUDIO_QUEUE_CAPACITY;
                overflowed = true;
                staged_dropped += AUDIO_CHANNELS as u64;
            }
        }
        if overflowed {
            pending.rotate_left(ring);
        }
        self.inner.append(pending.into_iter())?;
        if staged_dropped > 0 {
            self.inner
                .dropped_samples
                .fetch_add(staged_dropped, Ordering::Relaxed);
        }
        Ok(())
    }

    /// Cumulative output slots rendered without queued PCM (underruns).
    pub fn underrun_samples(&self) -> Result<u64, AudioError> {
        Ok(self.inner.underrun_samples.load(Ordering::Relaxed))
    }

    /// Cumulative samples discarded by overflow (oldest-first).
    pub fn dropped_samples(&self) -> u64 {
        self.inner.dropped_samples.load(Ordering::Relaxed)
    }

    pub fn queued_samples(&self) -> Result<usize, AudioError> {
        Ok(self.inner.queued() as usize)
    }

    /// Discard buffered PCM across all clones, preserving volume and mute.
    /// Producers must be stopped separately before session teardown.
    pub fn clear(&self) -> Result<(), AudioError> {
        let write = self.inner.write.load(Ordering::Acquire);
        self.inner.discard.fetch_max(write, Ordering::AcqRel);
        Ok(())
    }

    /// Set a finite linear gain in 0..=1. Invalid values leave the gain unchanged.
    pub fn set_volume(&self, volume: f32) -> Result<(), AudioError> {
        if !volume.is_finite() || !(0.0..=1.0).contains(&volume) {
            return Err(AudioError::InvalidVolume(volume));
        }
        self.inner.gain.store(volume.to_bits(), Ordering::Relaxed);
        Ok(())
    }

    pub fn set_muted(&self, muted: bool) -> Result<(), AudioError> {
        self.inner.muted.store(muted, Ordering::Relaxed);
        Ok(())
    }

    /// Fill complete stereo frames, zero-filling underrun. Mute still consumes PCM.
    /// Returns samples consumed; underrun slots are counted in
    /// [`AudioQueue::underrun_samples`]. An odd-sized output is rejected
    /// without mutation.
    pub fn drain_into(&self, output: &mut [f32]) -> Result<usize, AudioError> {
        if output.len() % AUDIO_CHANNELS as usize != 0 {
            return Err(AudioError::MisalignedSamples);
        }
        let mut consumed = 0;
        for frame in output.chunks_exact_mut(AUDIO_CHANNELS as usize) {
            if let Some([left, right]) = self.inner.pop_frame() {
                frame[0] = left;
                frame[1] = right;
                consumed += 2;
            } else {
                frame.fill(0.0);
                self.inner
                    .underrun_samples
                    .fetch_add(AUDIO_CHANNELS as u64, Ordering::Relaxed);
            }
        }
        Ok(consumed)
    }

    fn queued_frames(&self) -> usize {
        self.inner.queued() as usize / AUDIO_CHANNELS as usize
    }

    fn record_underrun(&self, slots: u64) {
        if slots > 0 {
            self.inner
                .underrun_samples
                .fetch_add(slots, Ordering::Relaxed);
        }
    }
}

/// Per-render accounting returned by [`AudioRenderer::render`].
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct RenderStats {
    /// Wire samples removed from the queue.
    pub consumed_samples: usize,
    /// Output slots filled with silence because the queue was empty.
    pub underrun_samples: usize,
    /// Current drift correction applied to the resampling ratio, in ppm
    /// (positive = consuming wire audio faster than nominal).
    pub drift_ppm: i32,
}

/// Converts wire PCM (48 kHz stereo f32) to an arbitrary output rate and
/// channel count, steering queue occupancy towards a target by trimming the
/// resampling ratio (linear interpolation). Volume/mute are applied by the
/// queue before this conversion. Intended to live inside the device callback:
/// it never locks or allocates.
#[derive(Debug)]
pub struct AudioRenderer {
    queue: AudioQueue,
    channels: usize,
    nominal_step: f64,
    target_frames: f64,
    current: [f32; 2],
    next: [f32; 2],
    have_current: bool,
    have_next: bool,
    phase: f64,
    primed: bool,
    smoothed_occupancy: f64,
    adjustment: f64,
}

impl AudioRenderer {
    pub fn new(queue: AudioQueue, output_sample_rate: u32, output_channels: u16) -> Self {
        Self {
            queue,
            channels: usize::from(output_channels.max(1)),
            nominal_step: f64::from(AUDIO_SAMPLE_RATE) / f64::from(output_sample_rate.max(1)),
            target_frames: AUDIO_TARGET_OCCUPANCY_FRAMES as f64,
            current: [0.0; 2],
            next: [0.0; 2],
            have_current: false,
            have_next: false,
            phase: 0.0,
            primed: false,
            smoothed_occupancy: 0.0,
            adjustment: 0.0,
        }
    }

    /// Override the drift controller target (wire frames). `0` disables drift
    /// correction, leaving only the nominal rate conversion.
    pub fn with_target_occupancy_frames(mut self, frames: usize) -> Self {
        self.target_frames = frames as f64;
        self
    }

    fn update_drift(&mut self) {
        if self.target_frames <= 0.0 {
            self.adjustment = 0.0;
            return;
        }
        let occupancy = self.queue.queued_frames() as f64;
        if !self.primed {
            // Correct only once the stream has built up; starved/starting
            // streams just play what arrives.
            if occupancy >= self.target_frames / 2.0 {
                self.primed = true;
                self.smoothed_occupancy = occupancy;
            }
            self.adjustment = 0.0;
            return;
        }
        self.smoothed_occupancy += OCCUPANCY_SMOOTHING * (occupancy - self.smoothed_occupancy);
        let error = self.smoothed_occupancy - self.target_frames;
        let dead_band = self.target_frames / 2.0;
        self.adjustment = if error.abs() <= dead_band {
            0.0
        } else {
            ((error - dead_band * error.signum()) / self.target_frames * MAX_DRIFT_ADJUSTMENT)
                .clamp(-MAX_DRIFT_ADJUSTMENT, MAX_DRIFT_ADJUSTMENT)
        };
    }

    fn fetch(&mut self, consumed: &mut usize) -> Option<[f32; 2]> {
        let frame = self.queue.inner.pop_frame()?;
        *consumed += AUDIO_CHANNELS as usize;
        Some(frame)
    }

    fn write_frame(&self, frame: [f32; 2], out: &mut [f32]) {
        match out.len() {
            1 => out[0] = (frame[0] + frame[1]) * 0.5,
            _ => {
                out[0] = frame[0];
                out[1] = frame[1];
                out[2..].fill(0.0);
            }
        }
    }

    /// Fill `output` (interleaved, `output_channels` wide; a trailing partial
    /// frame is zeroed) and report consumption.
    pub fn render(&mut self, output: &mut [f32]) -> RenderStats {
        self.update_drift();
        let step = self.nominal_step * (1.0 + self.adjustment);
        let mut consumed = 0usize;
        let mut underrun_frames = 0usize;
        let mut frames = output.chunks_exact_mut(self.channels);
        for out in &mut frames {
            if !self.have_current {
                match self.fetch(&mut consumed) {
                    Some(frame) => {
                        self.current = frame;
                        self.have_current = true;
                    }
                    None => {
                        out.fill(0.0);
                        underrun_frames += 1;
                        self.primed = false;
                        continue;
                    }
                }
            }
            let frame = if self.phase == 0.0 {
                self.current
            } else {
                if !self.have_next {
                    match self.fetch(&mut consumed) {
                        Some(frame) => {
                            self.next = frame;
                            self.have_next = true;
                        }
                        None => {
                            out.fill(0.0);
                            underrun_frames += 1;
                            self.primed = false;
                            continue;
                        }
                    }
                }
                let t = self.phase as f32;
                [
                    self.current[0] + (self.next[0] - self.current[0]) * t,
                    self.current[1] + (self.next[1] - self.current[1]) * t,
                ]
            };
            self.write_frame(frame, out);
            self.phase += step;
            while self.phase >= 1.0 {
                self.phase -= 1.0;
                if self.have_next {
                    self.current = self.next;
                    self.have_next = false;
                } else if let Some(frame) = self.fetch(&mut consumed) {
                    self.current = frame;
                } else {
                    self.have_current = false;
                    self.phase = 0.0;
                    break;
                }
            }
        }
        frames.into_remainder().fill(0.0);
        let underrun_samples = underrun_frames * self.channels;
        self.queue.record_underrun(underrun_samples as u64);
        RenderStats {
            consumed_samples: consumed,
            underrun_samples,
            drift_ppm: (self.adjustment * 1e6).round() as i32,
        }
    }
}

#[derive(Debug, Error)]
pub enum AudioError {
    #[error("no default audio output device")]
    NoOutputDevice,
    #[cfg(feature = "cpal-output")]
    #[error("default output uses {0:?}, which the audio path cannot convert from f32")]
    UnsupportedSampleFormat(SampleFormat),
    #[error("output device offers no usable PCM output configuration")]
    UnsupportedOutputConfig,
    #[error("PCM byte count {0} is not aligned to stereo f32 frames (8 bytes)")]
    MisalignedPcm(usize),
    #[error("sample count is not aligned to stereo frames")]
    MisalignedSamples,
    #[error("PCM contains a non-finite sample")]
    NonFinitePcm,
    #[error("volume must be finite and in 0..=1, got {0}")]
    InvalidVolume(f32),
    #[error("audio queue lock was poisoned")]
    Poisoned,
    #[error("audio device error: {0}")]
    Device(String),
}

/// An enumerated device handle, not a name-based identifier. Keep this handle for
/// selection: names may collide, and enumeration indices are not persistent IDs.
#[cfg(feature = "cpal-output")]
#[derive(Clone)]
pub struct AudioOutputDevice {
    device: Device,
}

#[cfg(feature = "cpal-output")]
impl AudioOutputDevice {
    pub fn name(&self) -> Result<String, AudioError> {
        self.device
            .description()
            .map(|description| description.name().to_owned())
            .map_err(|error| AudioError::Device(error.to_string()))
    }

    /// True when the device offers any configuration the output path can
    /// convert to (any channel count, rate and common sample format).
    pub fn supports_pcm(&self) -> Result<bool, AudioError> {
        Ok(self.output_plan()?.is_some())
    }

    fn output_plan(&self) -> Result<Option<OutputPlan>, AudioError> {
        Ok(select_output_plan(
            self.device
                .supported_output_configs()
                .map_err(|error| AudioError::Device(error.to_string()))?,
        ))
    }
}

/// Device configuration chosen for playback.
#[cfg(feature = "cpal-output")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OutputPlan {
    channels: u16,
    sample_rate: u32,
    format: SampleFormat,
    /// Requested low-latency buffer in frames, when the device reports a range.
    buffer_frames: Option<u32>,
}

/// Preference rank of a convertible sample format (lower is better).
#[cfg(feature = "cpal-output")]
fn format_rank(format: SampleFormat) -> Option<u8> {
    match format {
        SampleFormat::F32 => Some(0),
        SampleFormat::I32 => Some(1),
        SampleFormat::I16 => Some(2),
        SampleFormat::F64 => Some(3),
        SampleFormat::U16 => Some(4),
        SampleFormat::U32 => Some(5),
        SampleFormat::I8 => Some(6),
        SampleFormat::U8 => Some(7),
        _ => None,
    }
}

/// Sort key for candidate configurations (lower is better).
#[cfg(feature = "cpal-output")]
type PlanKey = (bool, bool, u8, u16, u32);

/// Pick the best supported configuration: exact 48 kHz (no resampling) first,
/// then stereo, then the preferred sample format, then fewest channels, then
/// the nearest rate. Requests a ~10 ms buffer when the device reports a range.
#[cfg(feature = "cpal-output")]
fn select_output_plan(
    ranges: impl IntoIterator<Item = SupportedStreamConfigRange>,
) -> Option<OutputPlan> {
    let mut best: Option<(PlanKey, OutputPlan)> = None;
    for range in ranges {
        let Some(rank) = format_rank(range.sample_format()) else {
            continue;
        };
        let (min, max) = (range.min_sample_rate(), range.max_sample_rate());
        let rate = AUDIO_SAMPLE_RATE.clamp(min, max);
        let key = (
            rate != AUDIO_SAMPLE_RATE,
            range.channels() != AUDIO_CHANNELS,
            rank,
            range.channels(),
            rate.abs_diff(AUDIO_SAMPLE_RATE),
        );
        if best.as_ref().map_or(true, |(current, _)| key < *current) {
            let buffer_frames = match range.buffer_size() {
                SupportedBufferSize::Range { min, max } => Some((rate / 100).clamp(*min, *max)),
                SupportedBufferSize::Unknown => None,
            };
            best = Some((
                key,
                OutputPlan {
                    channels: range.channels(),
                    sample_rate: rate,
                    format: range.sample_format(),
                    buffer_frames,
                },
            ));
        }
    }
    best.map(|(_, plan)| plan)
}

/// Best-effort notifications on a caller-owned bounded channel. A full or closed
/// channel never blocks playback; cumulative status/errors remain in `status()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioOutputEvent {
    Callback {
        consumed_samples: usize,
        total_consumed_samples: u64,
    },
    Error(String),
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AudioOutputStatus {
    pub callback_count: u64,
    pub consumed_samples: u64,
    pub underrun_samples: u64,
    pub error_count: u64,
    pub last_error: Option<String>,
    /// Negotiated device rate / channels (0 until a stream is configured).
    pub output_sample_rate: u32,
    pub output_channels: u16,
    /// Requested device buffer in frames (0 = device default).
    pub buffer_frames: u32,
    /// Mean/maximum time between device callbacks, microseconds.
    pub callback_interval_avg_us: u64,
    pub callback_interval_max_us: u64,
    /// Longest time spent rendering inside one callback, microseconds.
    pub callback_duration_max_us: u64,
    /// Wire samples currently queued.
    pub queue_occupancy_samples: u64,
    /// Samples discarded by queue overflow since start.
    pub dropped_samples: u64,
    /// Drift correction currently applied to the resampling ratio, ppm.
    pub drift_correction_ppm: i32,
}

/// Lock-free callback telemetry: everything the real-time callback writes is an
/// atomic. Only the (non-real-time) stream error path takes `last_error`.
#[cfg(any(feature = "cpal-output", test))]
struct CallbackState {
    epoch: Instant,
    last_callback_ns: AtomicU64,
    callback_count: AtomicU64,
    consumed_samples: AtomicU64,
    error_count: AtomicU64,
    interval_sum_us: AtomicU64,
    interval_max_us: AtomicU64,
    duration_max_us: AtomicU64,
    drift_ppm: AtomicI32,
    output_sample_rate: AtomicU32,
    output_channels: AtomicU32,
    buffer_frames: AtomicU32,
    last_error: Mutex<Option<String>>,
    events: Option<SyncSender<AudioOutputEvent>>,
}

#[cfg(any(feature = "cpal-output", test))]
impl CallbackState {
    fn new(events: Option<SyncSender<AudioOutputEvent>>) -> Self {
        Self {
            epoch: Instant::now(),
            last_callback_ns: AtomicU64::new(0),
            callback_count: AtomicU64::new(0),
            consumed_samples: AtomicU64::new(0),
            error_count: AtomicU64::new(0),
            interval_sum_us: AtomicU64::new(0),
            interval_max_us: AtomicU64::new(0),
            duration_max_us: AtomicU64::new(0),
            drift_ppm: AtomicI32::new(0),
            output_sample_rate: AtomicU32::new(0),
            output_channels: AtomicU32::new(0),
            buffer_frames: AtomicU32::new(0),
            last_error: Mutex::new(None),
            events,
        }
    }

    fn notify(&self, event: AudioOutputEvent) {
        if let Some(events) = &self.events {
            // Lossy by contract: a stalled UI must not stall the audio callback.
            let _ = events.try_send(event);
        }
    }

    fn record_error(&self, error: String) {
        tracing::error!(%error, "CPAL output failed");
        self.error_count.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut last) = self.last_error.lock() {
            *last = Some(error.clone());
        }
        self.notify(AudioOutputEvent::Error(error));
    }

    fn render(&self, renderer: &mut AudioRenderer, output: &mut [f32]) {
        let started = Instant::now();
        let now_ns = started.duration_since(self.epoch).as_nanos() as u64 + 1;
        let previous_ns = self.last_callback_ns.swap(now_ns, Ordering::Relaxed);
        if previous_ns != 0 {
            let interval_us = now_ns.saturating_sub(previous_ns) / 1_000;
            self.interval_sum_us
                .fetch_add(interval_us, Ordering::Relaxed);
            self.interval_max_us
                .fetch_max(interval_us, Ordering::Relaxed);
        }
        let stats = renderer.render(output);
        self.drift_ppm.store(stats.drift_ppm, Ordering::Relaxed);
        self.callback_count.fetch_add(1, Ordering::Relaxed);
        let total_consumed_samples = self
            .consumed_samples
            .fetch_add(stats.consumed_samples as u64, Ordering::Relaxed)
            + stats.consumed_samples as u64;
        self.duration_max_us
            .fetch_max(started.elapsed().as_micros() as u64, Ordering::Relaxed);
        self.notify(AudioOutputEvent::Callback {
            consumed_samples: stats.consumed_samples,
            total_consumed_samples,
        });
    }

    fn snapshot(&self, queue: &AudioQueue) -> AudioOutputStatus {
        let callback_count = self.callback_count.load(Ordering::Relaxed);
        AudioOutputStatus {
            callback_count,
            consumed_samples: self.consumed_samples.load(Ordering::Relaxed),
            underrun_samples: queue.underrun_samples().unwrap_or(0),
            error_count: self.error_count.load(Ordering::Relaxed),
            last_error: self.last_error.lock().ok().and_then(|last| last.clone()),
            output_sample_rate: self.output_sample_rate.load(Ordering::Relaxed),
            output_channels: self.output_channels.load(Ordering::Relaxed) as u16,
            buffer_frames: self.buffer_frames.load(Ordering::Relaxed),
            callback_interval_avg_us: self
                .interval_sum_us
                .load(Ordering::Relaxed)
                .checked_div(callback_count.saturating_sub(1))
                .unwrap_or(0),
            callback_interval_max_us: self.interval_max_us.load(Ordering::Relaxed),
            callback_duration_max_us: self.duration_max_us.load(Ordering::Relaxed),
            queue_occupancy_samples: queue.inner.queued(),
            dropped_samples: queue.dropped_samples(),
            drift_correction_ppm: self.drift_ppm.load(Ordering::Relaxed),
        }
    }
}

/// Build a stream for one concrete device sample type; wire PCM is rendered to
/// f32 in the device layout/rate and converted per sample.
#[cfg(feature = "cpal-output")]
fn build_typed_stream<T>(
    device: &Device,
    config: &StreamConfig,
    queue: &AudioQueue,
    callbacks: &Arc<CallbackState>,
) -> Result<Stream, cpal::BuildStreamError>
where
    T: SizedSample + FromSample<f32>,
{
    let mut renderer = AudioRenderer::new(queue.clone(), config.sample_rate, config.channels);
    let mut scratch = vec![0.0f32; 8192];
    let render_callbacks = callbacks.clone();
    let error_callbacks = callbacks.clone();
    device.build_output_stream(
        config,
        move |output: &mut [T], _| {
            if scratch.len() < output.len() {
                scratch.resize(output.len(), 0.0);
            }
            let rendered = &mut scratch[..output.len()];
            render_callbacks.render(&mut renderer, rendered);
            for (destination, sample) in output.iter_mut().zip(rendered.iter()) {
                *destination = T::from_sample(*sample);
            }
        },
        move |error| error_callbacks.record_error(error.to_string()),
        None,
    )
}

#[cfg(feature = "cpal-output")]
fn build_stream(
    device: &Device,
    config: &StreamConfig,
    format: SampleFormat,
    queue: &AudioQueue,
    callbacks: &Arc<CallbackState>,
) -> Result<Stream, AudioError> {
    let built = match format {
        SampleFormat::F32 => build_typed_stream::<f32>(device, config, queue, callbacks),
        SampleFormat::I32 => build_typed_stream::<i32>(device, config, queue, callbacks),
        SampleFormat::I16 => build_typed_stream::<i16>(device, config, queue, callbacks),
        SampleFormat::F64 => build_typed_stream::<f64>(device, config, queue, callbacks),
        SampleFormat::U16 => build_typed_stream::<u16>(device, config, queue, callbacks),
        SampleFormat::U32 => build_typed_stream::<u32>(device, config, queue, callbacks),
        SampleFormat::I8 => build_typed_stream::<i8>(device, config, queue, callbacks),
        SampleFormat::U8 => build_typed_stream::<u8>(device, config, queue, callbacks),
        other => return Err(AudioError::UnsupportedSampleFormat(other)),
    };
    built.map_err(|error| AudioError::Device(error.to_string()))
}

/// Active CPAL output stream backed by [`AudioQueue`]. Wire PCM (48 kHz stereo
/// f32) is adapted to whatever channel layout, sample format and rate the
/// device supports. Drop stops/releases the stream before clearing buffered
/// PCM. Stop producers first, and use a fresh queue per session; clones remain
/// writable after drop.
#[cfg(feature = "cpal-output")]
pub struct CpalAudioOutput {
    queue: AudioQueue,
    stream: Option<Stream>,
    callbacks: Arc<CallbackState>,
}

#[cfg(feature = "cpal-output")]
impl CpalAudioOutput {
    pub fn output_devices() -> Result<Vec<AudioOutputDevice>, AudioError> {
        Ok(cpal::default_host()
            .output_devices()
            .map_err(|error| AudioError::Device(error.to_string()))?
            .map(|device| AudioOutputDevice { device })
            .collect())
    }

    pub fn default_output_device() -> Result<AudioOutputDevice, AudioError> {
        cpal::default_host()
            .default_output_device()
            .map(|device| AudioOutputDevice { device })
            .ok_or(AudioError::NoOutputDevice)
    }

    /// Compatibility entry point: start the current default output.
    pub fn start(queue: AudioQueue) -> Result<Self, AudioError> {
        Self::start_on_device(queue, None)
    }

    /// `None` selects the current default. An explicit device never silently falls back.
    pub fn start_on_device(
        queue: AudioQueue,
        device: Option<&AudioOutputDevice>,
    ) -> Result<Self, AudioError> {
        Self::start_inner(queue, device, None)
    }

    /// Construct the channel before calling this method so the first callback is
    /// observable. Stream errors are also retained by `status()` if events are lost.
    pub fn start_with_events(
        queue: AudioQueue,
        device: Option<&AudioOutputDevice>,
        events: SyncSender<AudioOutputEvent>,
    ) -> Result<Self, AudioError> {
        Self::start_inner(queue, device, Some(events))
    }

    fn start_inner(
        queue: AudioQueue,
        selected: Option<&AudioOutputDevice>,
        events: Option<SyncSender<AudioOutputEvent>>,
    ) -> Result<Self, AudioError> {
        activate_ios_audio_session()?;
        let device = match selected {
            Some(device) => device.clone(),
            None => Self::default_output_device()?,
        };
        let plan = device
            .output_plan()?
            .ok_or(AudioError::UnsupportedOutputConfig)?;
        let callbacks = Arc::new(CallbackState::new(events));
        let config_for = |buffer_size| StreamConfig {
            channels: plan.channels,
            sample_rate: plan.sample_rate,
            buffer_size,
        };
        // Prefer the negotiated low-latency buffer; devices that reject the
        // fixed size fall back to their default.
        let mut buffer_frames = plan.buffer_frames.unwrap_or(0);
        let stream = match plan.buffer_frames {
            Some(frames) => build_stream(
                &device.device,
                &config_for(BufferSize::Fixed(frames)),
                plan.format,
                &queue,
                &callbacks,
            )
            .or_else(|_| {
                buffer_frames = 0;
                build_stream(
                    &device.device,
                    &config_for(BufferSize::Default),
                    plan.format,
                    &queue,
                    &callbacks,
                )
            })?,
            None => build_stream(
                &device.device,
                &config_for(BufferSize::Default),
                plan.format,
                &queue,
                &callbacks,
            )?,
        };
        callbacks
            .output_sample_rate
            .store(plan.sample_rate, Ordering::Relaxed);
        callbacks
            .output_channels
            .store(u32::from(plan.channels), Ordering::Relaxed);
        callbacks
            .buffer_frames
            .store(buffer_frames, Ordering::Relaxed);
        // Own the stream before play: a start failure follows the same cleanup path.
        let output = Self {
            queue,
            stream: Some(stream),
            callbacks,
        };
        output
            .stream()
            .play()
            .map_err(|error| AudioError::Device(error.to_string()))?;
        Ok(output)
    }

    pub fn queue(&self) -> &AudioQueue {
        &self.queue
    }

    pub fn stream(&self) -> &Stream {
        self.stream
            .as_ref()
            .expect("stream exists until output is dropped")
    }

    /// Snapshot of cumulative counters; never blocks the audio callback.
    pub fn status(&self) -> Result<AudioOutputStatus, AudioError> {
        Ok(self.callbacks.snapshot(&self.queue))
    }

    pub fn clear(&self) -> Result<(), AudioError> {
        self.queue.clear()
    }
}

#[cfg(feature = "cpal-output")]
impl Drop for CpalAudioOutput {
    fn drop(&mut self) {
        if let Some(stream) = self.stream.take() {
            if let Err(error) = stream.pause() {
                self.callbacks.record_error(error.to_string());
            }
            drop(stream);
        }
        if let Err(error) = self.queue.clear() {
            self.callbacks.record_error(error.to_string());
        }
    }
}

#[cfg(target_os = "ios")]
pub fn activate_ios_audio_session() -> Result<(), AudioError> {
    use std::ffi::{c_char, c_void};

    #[link(name = "objc", kind = "dylib")]
    #[link(name = "AVFoundation", kind = "framework")]
    extern "C" {
        fn objc_getClass(name: *const c_char) -> *mut c_void;
        fn sel_registerName(name: *const c_char) -> *mut c_void;
        fn objc_msgSend();
    }

    unsafe {
        let cls = objc_getClass(b"AVAudioSession\0".as_ptr() as *const c_char);
        if cls.is_null() {
            return Err(AudioError::Device("AVAudioSession class not found".into()));
        }
        let sel_shared = sel_registerName(b"sharedInstance\0".as_ptr() as *const c_char);
        let msg_send_cls: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
            std::mem::transmute(objc_msgSend as *const ());
        let session = msg_send_cls(cls, sel_shared);
        if session.is_null() {
            return Err(AudioError::Device(
                "AVAudioSession sharedInstance is null".into(),
            ));
        }

        let nsstring_cls = objc_getClass(b"NSString\0".as_ptr() as *const c_char);
        let sel_string_with_utf8 =
            sel_registerName(b"stringWithUTF8String:\0".as_ptr() as *const c_char);
        let msg_send_str: unsafe extern "C" fn(
            *mut c_void,
            *mut c_void,
            *const c_char,
        ) -> *mut c_void = std::mem::transmute(objc_msgSend as *const ());
        let category = msg_send_str(
            nsstring_cls,
            sel_string_with_utf8,
            b"AVAudioSessionCategoryPlayback\0".as_ptr() as *const c_char,
        );

        let sel_set_category = sel_registerName(b"setCategory:error:\0".as_ptr() as *const c_char);
        let mut err: *mut c_void = std::ptr::null_mut();
        let msg_send_set_cat: unsafe extern "C" fn(
            *mut c_void,
            *mut c_void,
            *mut c_void,
            *mut *mut c_void,
        ) -> bool = std::mem::transmute(objc_msgSend as *const ());
        let cat_ok = msg_send_set_cat(session, sel_set_category, category, &mut err);
        if !cat_ok {
            return Err(AudioError::Device(
                "Failed to set AVAudioSessionCategoryPlayback".into(),
            ));
        }

        let sel_set_active = sel_registerName(b"setActive:error:\0".as_ptr() as *const c_char);
        let msg_send_set_active: unsafe extern "C" fn(
            *mut c_void,
            *mut c_void,
            bool,
            *mut *mut c_void,
        ) -> bool = std::mem::transmute(objc_msgSend as *const ());
        let active_ok = msg_send_set_active(session, sel_set_active, true, &mut err);
        if !active_ok {
            return Err(AudioError::Device(
                "Failed to activate AVAudioSession".into(),
            ));
        }
    }
    Ok(())
}

#[cfg(not(target_os = "ios"))]
pub fn activate_ios_audio_session() -> Result<(), AudioError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{sync_channel, TryRecvError};

    fn renderer(queue: &AudioQueue) -> AudioRenderer {
        AudioRenderer::new(queue.clone(), AUDIO_SAMPLE_RATE, AUDIO_CHANNELS)
    }

    #[test]
    fn callback_events_report_consumption_and_preserve_errors_when_full() {
        let (sender, receiver) = sync_channel(1);
        let callbacks = CallbackState::new(Some(sender));
        let queue = AudioQueue::default();
        let mut renderer = renderer(&queue);
        queue.push_samples([0.125, -0.25]).unwrap();
        let mut output = [123.0; 4];
        callbacks.render(&mut renderer, &mut output);
        assert_eq!(output, [0.125, -0.25, 0.0, 0.0]);
        callbacks.render(&mut renderer, &mut output); // Full channel must not block.
        callbacks.record_error("test-device-disconnected".into());
        assert_eq!(
            receiver.try_recv().unwrap(),
            AudioOutputEvent::Callback {
                consumed_samples: 2,
                total_consumed_samples: 2,
            }
        );
        assert_eq!(receiver.try_recv(), Err(TryRecvError::Empty));
        let status = callbacks.snapshot(&queue);
        assert_eq!(status.callback_count, 2);
        assert_eq!(status.consumed_samples, 2);
        assert_eq!(status.error_count, 1);
        assert_eq!(
            status.last_error.as_deref(),
            Some("test-device-disconnected")
        );
        drop(receiver);
        callbacks.render(&mut renderer, &mut output); // Disconnected channel also cannot block.
        assert_eq!(callbacks.snapshot(&queue).callback_count, 3);
    }

    #[test]
    fn callback_records_timing_without_locking_status() {
        let callbacks = CallbackState::new(None);
        let queue = AudioQueue::default();
        let mut renderer = renderer(&queue);
        let mut output = [0.0; 4];
        for _ in 0..3 {
            callbacks.render(&mut renderer, &mut output);
        }
        let status = callbacks.snapshot(&queue);
        assert_eq!(status.callback_count, 3);
        assert!(status.callback_interval_max_us >= status.callback_interval_avg_us);
    }

    #[test]
    fn trailing_partial_output_frame_is_silenced() {
        let queue = AudioQueue::default();
        queue.push_samples([0.5, -0.5]).unwrap();
        let mut output = [123.0; 3];
        let stats = renderer(&queue).render(&mut output);
        assert_eq!(output, [0.5, -0.5, 0.0]);
        assert_eq!(stats.consumed_samples, 2);
    }

    #[test]
    fn underrun_slots_are_counted_in_queue_state_and_status() {
        let queue = AudioQueue::default();
        queue.push_samples([0.125, -0.25]).unwrap();
        let mut output = [0.0; 6];
        assert_eq!(queue.drain_into(&mut output).unwrap(), 2);
        assert_eq!(queue.underrun_samples().unwrap(), 4);

        let callbacks = CallbackState::new(None);
        callbacks.render(&mut renderer(&queue), &mut output); // All six slots underrun again.
        let status = callbacks.snapshot(&queue);
        assert_eq!(status.consumed_samples, 0);
        assert_eq!(status.underrun_samples, 10);
    }

    #[test]
    fn mono_output_downmixes_and_wide_output_pads_silence() {
        let queue = AudioQueue::default();
        queue.push_samples([0.5, -0.25, 1.0, 0.0]).unwrap();
        let mut mono = [0.0; 2];
        AudioRenderer::new(queue.clone(), AUDIO_SAMPLE_RATE, 1).render(&mut mono);
        assert_eq!(mono, [0.125, 0.5]);
        queue.push_samples([0.5, -0.25]).unwrap();
        let mut wide = [9.0; 6];
        AudioRenderer::new(queue, AUDIO_SAMPLE_RATE, 6).render(&mut wide);
        assert_eq!(wide, [0.5, -0.25, 0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn volume_and_mute_apply_before_conversion() {
        let queue = AudioQueue::default();
        queue.set_volume(0.5).unwrap();
        queue.push_samples([1.0, 0.5]).unwrap();
        let mut mono = [0.0; 1];
        AudioRenderer::new(queue.clone(), AUDIO_SAMPLE_RATE, 1).render(&mut mono);
        assert_eq!(mono, [0.375]);
        queue.set_muted(true).unwrap();
        queue.push_samples([1.0, 0.5]).unwrap();
        AudioRenderer::new(queue.clone(), AUDIO_SAMPLE_RATE, 1).render(&mut mono);
        assert_eq!(mono, [0.0]);
        assert_eq!(queue.queued_samples().unwrap(), 0);
    }

    #[test]
    fn rate_conversion_consumes_input_in_proportion() {
        // 48 kHz wire -> 24 kHz device consumes two wire frames per output frame.
        let queue = AudioQueue::default();
        let ramp: Vec<f32> = (0..400).flat_map(|i| [i as f32 / 1000.0; 2]).collect();
        queue.push_samples(ramp).unwrap();
        let mut output = vec![0.0; 100 * 2];
        let mut renderer =
            AudioRenderer::new(queue.clone(), 24_000, 2).with_target_occupancy_frames(0);
        let stats = renderer.render(&mut output);
        assert_eq!(output[2], 0.002);
        assert_eq!(stats.underrun_samples, 0);
        assert!(queue.queued_samples().unwrap() <= (400 - 200) * 2);
        // 44.1 kHz device: interpolated output stays between neighbouring inputs.
        let queue = AudioQueue::default();
        queue
            .push_samples((0..200).flat_map(|i| [i as f32 / 200.0; 2]))
            .unwrap();
        let mut output = vec![0.0; 100 * 2];
        let stats = AudioRenderer::new(queue, 44_100, 2)
            .with_target_occupancy_frames(0)
            .render(&mut output);
        assert_eq!(stats.underrun_samples, 0);
        assert!(output.chunks(2).all(|f| f[0] == f[1]));
        assert!(output
            .chunks(2)
            .collect::<Vec<_>>()
            .windows(2)
            .all(|w| w[1][0] > w[0][0]));
    }

    #[test]
    fn drift_controller_sheds_backlog_and_stays_bounded() {
        // Producer runs 0.5% faster than the (nominal 48 kHz) device clock.
        let queue = AudioQueue::default();
        let mut renderer = renderer(&queue);
        let mut output = vec![0.0; 480 * 2];
        let packet: Vec<f32> = vec![0.1; 482 * 2];
        let mut max_drift = 0;
        for _ in 0..3000 {
            queue.push_samples(packet.iter().copied()).unwrap();
            let stats = renderer.render(&mut output);
            max_drift = max_drift.max(stats.drift_ppm);
        }
        assert!(max_drift > 0, "controller never engaged");
        assert_eq!(
            queue.dropped_samples(),
            0,
            "backlog clipped instead of trimmed"
        );
        assert_eq!(queue.underrun_samples().unwrap(), 0);
        assert!(queue.queued_samples().unwrap() < AUDIO_QUEUE_CAPACITY);
    }

    #[test]
    fn ring_overflow_counts_dropped_and_clear_is_immediate() {
        let queue = AudioQueue::default();
        queue
            .push_samples((0..AUDIO_QUEUE_CAPACITY / 2 + 10).flat_map(|i| [i as f32; 2]))
            .unwrap();
        assert_eq!(queue.dropped_samples(), 20);
        queue.clear().unwrap();
        assert_eq!(queue.queued_samples().unwrap(), 0);
        let mut out = [1.0; 2];
        assert_eq!(queue.drain_into(&mut out).unwrap(), 0);
    }

    #[test]
    fn concurrent_producer_and_consumer_never_observe_torn_frames() {
        // Every frame is [n, -n]; recycled/torn slots would break the pairing
        // or the monotonic order.
        let queue = AudioQueue::default();
        let producer = queue.clone();
        let handle = std::thread::spawn(move || {
            for chunk in 0..2000u32 {
                producer
                    .push_samples((0..64).flat_map(|i| {
                        let n = (chunk * 64 + i) as f32;
                        [n, -n]
                    }))
                    .unwrap();
            }
        });
        let mut last = -1.0f32;
        let mut out = [0.0f32; 128];
        while !handle.is_finished() || queue.queued_samples().unwrap() > 0 {
            // One frame per drain so a mid-stream underrun cannot interleave
            // silence with real frames in the checked slice.
            let consumed = queue.drain_into(&mut out[..2]).unwrap();
            for frame in out[..consumed].chunks(2) {
                assert_eq!(frame[0], -frame[1]);
                assert!(frame[0] > last, "{} !> {}", frame[0], last);
                last = frame[0];
            }
        }
        handle.join().unwrap();
    }

    #[cfg(feature = "cpal-output")]
    fn range(
        channels: u16,
        min: u32,
        max: u32,
        format: SampleFormat,
        buffer: SupportedBufferSize,
    ) -> SupportedStreamConfigRange {
        SupportedStreamConfigRange::new(channels, min, max, buffer, format)
    }

    #[cfg(feature = "cpal-output")]
    #[test]
    fn output_plan_prefers_exact_rate_then_stereo_then_f32() {
        let unknown = SupportedBufferSize::Unknown;
        let plan = select_output_plan([
            range(2, 44_100, 44_100, SampleFormat::F32, unknown),
            range(6, 44_100, 96_000, SampleFormat::I16, unknown),
            range(2, 44_100, 96_000, SampleFormat::I16, unknown),
            range(2, 44_100, 96_000, SampleFormat::F32, unknown),
        ])
        .unwrap();
        assert_eq!(
            (plan.channels, plan.sample_rate, plan.format),
            (2, 48_000, SampleFormat::F32)
        );
        // Mono-only / 44.1 kHz-only / 16-bit-only devices are still usable.
        let plan =
            select_output_plan([range(1, 44_100, 44_100, SampleFormat::I16, unknown)]).unwrap();
        assert_eq!(
            (plan.channels, plan.sample_rate, plan.format),
            (1, 44_100, SampleFormat::I16)
        );
        // Rate above the device range clamps to the nearest supported rate.
        let plan =
            select_output_plan([range(2, 8_000, 32_000, SampleFormat::F32, unknown)]).unwrap();
        assert_eq!(plan.sample_rate, 32_000);
        assert!(
            select_output_plan([range(2, 48_000, 48_000, SampleFormat::I64, unknown)]).is_none()
        );
        assert!(select_output_plan([]).is_none());
    }

    #[cfg(feature = "cpal-output")]
    #[test]
    fn output_plan_requests_clamped_low_latency_buffer() {
        let buffer = |min, max| SupportedBufferSize::Range { min, max };
        let plan = select_output_plan([range(
            2,
            48_000,
            48_000,
            SampleFormat::F32,
            buffer(64, 4096),
        )])
        .unwrap();
        assert_eq!(plan.buffer_frames, Some(480));
        let plan = select_output_plan([range(
            2,
            48_000,
            48_000,
            SampleFormat::F32,
            buffer(1024, 4096),
        )])
        .unwrap();
        assert_eq!(plan.buffer_frames, Some(1024));
        let plan = select_output_plan([range(
            2,
            48_000,
            48_000,
            SampleFormat::F32,
            SupportedBufferSize::Unknown,
        )])
        .unwrap();
        assert_eq!(plan.buffer_frames, None);
    }

    #[test]
    fn synthetic_five_second_stream_drains_without_underrun() {
        let queue = AudioQueue::default();
        let sample_count = AUDIO_SAMPLE_RATE as usize * AUDIO_CHANNELS as usize * 5;
        let chunk_samples = 480 * AUDIO_CHANNELS as usize;
        let mut total_consumed = 0;
        for start in (0..sample_count).step_by(chunk_samples) {
            let expected: Vec<_> = (start..start + chunk_samples)
                .map(|sample| (sample as f32 * 0.01).sin())
                .collect();
            queue.push_samples(expected.iter().copied()).unwrap();
            let mut output = vec![0.0; chunk_samples];
            total_consumed += queue.drain_into(&mut output).unwrap();
            assert_eq!(output, expected);
            assert_eq!(queue.queued_samples().unwrap(), 0);
        }
        assert_eq!(total_consumed, sample_count);
    }
}
