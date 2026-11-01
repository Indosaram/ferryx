use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use ffmpeg_next as ffmpeg;
use thiserror::Error;

use crate::capture_macos::{CaptureFrame, LatestSlot, SurfaceFrame, Wakeup};

pub const DEFAULT_BITRATE: u32 = 8_000_000;
pub const MIN_LAN_BITRATE: u32 = 50_000_000;
pub const MAX_LAN_BITRATE: u32 = 150_000_000;
/// Lowest and highest rate the host accepts in a stream configuration. The
/// encoder clamps to exactly this range, so a setting the session validated is
/// never silently raised to a different floor.
pub const MIN_ENCODER_BITRATE: u32 = 100_000;
pub const MAX_ENCODER_BITRATE: u32 = 300_000_000;
pub const ENCODE_QUEUE_DEPTH: usize = 3;
/// Compressed packets that may wait for the session. Together with the one
/// frame being encoded this is the whole latency budget of the encoder stage;
/// raw frames beyond it are replaced, never queued.
const OUTPUT_QUEUE_DEPTH: usize = 3;
/// Backstop for waits that are always ended by an explicit event.
const LIVENESS_BACKSTOP: Duration = Duration::from_secs(1);

pub fn clamp_bitrate(bitrate: u32) -> u32 {
    bitrate.clamp(MIN_ENCODER_BITRATE, MAX_ENCODER_BITRATE)
}

/// Codec produced by VideoToolbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VtCodec {
    #[default]
    Hevc,
    H264,
}

impl VtCodec {
    pub fn encoder_name(self) -> &'static str {
        match self {
            Self::Hevc => "hevc_videotoolbox",
            Self::H264 => "h264_videotoolbox",
        }
    }

    fn profile(self) -> &'static str {
        match self {
            Self::Hevc => "main",
            Self::H264 => "main",
        }
    }

    fn nalu_type(self, nalu: &[u8]) -> Option<u8> {
        nalu.first().map(|byte| match self {
            Self::Hevc => (byte >> 1) & 0x3f,
            Self::H264 => byte & 0x1f,
        })
    }

    /// VPS/SPS/PPS for HEVC, SPS/PPS for H.264.
    fn is_parameter_set(self, kind: u8) -> bool {
        match self {
            Self::Hevc => matches!(kind, 32..=34),
            Self::H264 => matches!(kind, 7 | 8),
        }
    }
}

/// How frames reach the encoder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputKind {
    /// Copied BGRA frames ([`CaptureFrame`]). Pays a full-frame CPU copy.
    #[default]
    Bgra,
    /// Retained NV12 (420f full range, BT.601) CoreVideo surfaces
    /// ([`SurfaceFrame`]). No CPU copy between capture and the encoder.
    Nv12Surface,
}

/// Encoder choices that do not change the wire config.
#[derive(Clone, Default)]
pub struct EncoderOptions {
    pub codec: VtCodec,
    pub input: InputKind,
    /// Refuse to run without a hardware encoder instead of degrading to
    /// software. When false, a failed hardware open is retried in software and
    /// reported through [`VideoToolboxEncoder::is_hardware`] and a warning.
    pub require_hardware: bool,
    /// Wake this handle when compressed output is available. Share the
    /// capture's [`Wakeup`] so one wait covers frames, audio, errors and output.
    pub wakeup: Option<Arc<Wakeup>>,
}

/// Compressed output queue: bounded, order preserving, with explicit wakeups in
/// both directions instead of timed polling.
struct OutputShared {
    state: Mutex<OutputState>,
    space: Condvar,
    ready: Arc<Wakeup>,
}

struct OutputState {
    queue: VecDeque<Result<EncodedFrame, EncodeError>>,
    stopped: bool,
    receiver_dropped: bool,
}

fn output_channel_with(ready: Arc<Wakeup>) -> (OutputSender, EncodedReceiver) {
    let shared = Arc::new(OutputShared {
        state: Mutex::new(OutputState {
            queue: VecDeque::with_capacity(OUTPUT_QUEUE_DEPTH),
            stopped: false,
            receiver_dropped: false,
        }),
        space: Condvar::new(),
        ready,
    });
    (
        OutputSender {
            shared: Arc::clone(&shared),
        },
        EncodedReceiver { shared },
    )
}

#[cfg(test)]
fn output_channel() -> (OutputSender, EncodedReceiver) {
    output_channel_with(Arc::new(Wakeup::default()))
}

struct OutputSender {
    shared: Arc<OutputShared>,
}

impl OutputSender {
    fn cancel(&self) {
        self.shared.state.lock().expect("output poisoned").stopped = true;
        self.shared.space.notify_all();
    }

    fn is_stopped(&self) -> bool {
        self.shared.state.lock().expect("output poisoned").stopped
    }

    /// Blocks until the queue has room. `false` means stop or a gone receiver:
    /// nothing more may be produced.
    fn wait_for_space_or_controls(&self, shared: &Shared) -> bool {
        let mut state = self.shared.state.lock().expect("output poisoned");
        loop {
            if state.stopped || state.receiver_dropped {
                return false;
            }
            let controls = shared.controls.lock().expect("controls poisoned");
            if state.queue.len() < OUTPUT_QUEUE_DEPTH
                || controls.force_key_frame
                || controls.bitrate.is_some()
            {
                return true;
            }
            drop(controls);
            state = self.shared.space.wait(state).expect("output poisoned");
        }
    }

    fn has_space(&self) -> bool {
        self.shared
            .state
            .lock()
            .expect("output poisoned")
            .queue
            .len()
            < OUTPUT_QUEUE_DEPTH
    }

    #[cfg(test)]
    fn wait_for_space(&self) -> bool {
        let mut state = self.shared.state.lock().expect("output poisoned");
        loop {
            if state.stopped || state.receiver_dropped {
                return false;
            }
            if state.queue.len() < OUTPUT_QUEUE_DEPTH {
                return true;
            }
            state = self.shared.space.wait(state).expect("output poisoned");
        }
    }

    /// Retains this exact compressed frame until space opens; only stop or a
    /// disconnected receiver may abandon it, so references are never skipped.
    fn send(
        &self,
        frame: Result<EncodedFrame, EncodeError>,
    ) -> Result<(), Result<EncodedFrame, EncodeError>> {
        let mut state = self.shared.state.lock().expect("output poisoned");
        loop {
            if state.stopped || state.receiver_dropped {
                return Err(frame);
            }
            if state.queue.len() < OUTPUT_QUEUE_DEPTH {
                state.queue.push_back(frame);
                drop(state);
                self.shared.ready.notify();
                return Ok(());
            }
            state = self.shared.space.wait(state).expect("output poisoned");
        }
    }
}

/// Receiving end of the encoder's compressed output.
pub struct EncodedReceiver {
    shared: Arc<OutputShared>,
}

impl EncodedReceiver {
    pub fn try_recv(&self) -> Result<Result<EncodedFrame, EncodeError>, mpsc::TryRecvError> {
        let mut state = self.shared.state.lock().expect("output poisoned");
        match state.queue.pop_front() {
            Some(frame) => {
                drop(state);
                self.shared.space.notify_all();
                Ok(frame)
            }
            None if state.stopped => Err(mpsc::TryRecvError::Disconnected),
            None => Err(mpsc::TryRecvError::Empty),
        }
    }

    pub fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> Result<Result<EncodedFrame, EncodeError>, mpsc::RecvTimeoutError> {
        let deadline = Instant::now() + timeout;
        loop {
            match self.try_recv() {
                Ok(frame) => return Ok(frame),
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err(mpsc::RecvTimeoutError::Disconnected)
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(mpsc::RecvTimeoutError::Timeout);
            }
            self.shared.ready.wait(remaining.min(LIVENESS_BACKSTOP));
        }
    }

    /// Compressed packets currently waiting.
    pub fn len(&self) -> usize {
        self.shared
            .state
            .lock()
            .expect("output poisoned")
            .queue
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Drop for EncodedReceiver {
    fn drop(&mut self) {
        self.shared
            .state
            .lock()
            .expect("output poisoned")
            .receiver_dropped = true;
        self.shared.space.notify_all();
    }
}

#[derive(Debug, Clone, Copy)]
pub struct EncoderConfig {
    pub width: u32,
    pub height: u32,
    pub frames_per_second: u32,
    pub bitrate: u32,
    pub key_frame_interval: u32,
}

impl EncoderConfig {
    pub fn compatibility(width: u32, height: u32, frames_per_second: u32) -> Self {
        Self {
            width,
            height,
            frames_per_second,
            bitrate: DEFAULT_BITRATE,
            key_frame_interval: frames_per_second.max(1),
        }
    }

    pub fn lan(width: u32, height: u32, frames_per_second: u32, bitrate: u32) -> Self {
        Self {
            width,
            height,
            frames_per_second,
            bitrate: bitrate.clamp(MIN_LAN_BITRATE, MAX_LAN_BITRATE),
            key_frame_interval: frames_per_second.max(1),
        }
    }
}

#[derive(Debug)]
pub struct EncodedFrame {
    pub data: Vec<u8>,
    pub is_key_frame: bool,
    /// When the picture was captured by ScreenCaptureKit (monotonic clock).
    pub capture_at: Instant,
    /// When the capture callback handed the picture over.
    pub callback_at: Instant,
    pub encode_started_at: Instant,
    pub encode_completed_at: Instant,
}

#[derive(Debug, Error)]
pub enum EncodeError {
    #[error("FFmpeg initialization failed: {0}")]
    Ffmpeg(#[from] ffmpeg::Error),
    #[error("VideoToolbox encoder is unavailable")]
    EncoderUnavailable,
    #[error("no hardware VideoToolbox encoder is available for this codec")]
    HardwareUnavailable,
    #[error("invalid encoder dimensions or frame rate")]
    InvalidConfiguration,
    #[error("encoder worker stopped")]
    WorkerStopped,
    #[error("encoder queue is full; frame dropped")]
    QueueFull,
    #[error("captured frame dimensions or stride do not match the encoder")]
    FrameShape,
    #[error("frame kind does not match the encoder input kind")]
    FrameKind,
    #[error("encoder returned malformed HEVC data")]
    MalformedHevc,
    #[error("bitrate change was not applied: {0}")]
    BitrateNotApplied(String),
}

/// What the encoder actually runs on, known once it opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncoderInfo {
    pub codec: VtCodec,
    pub hardware: bool,
}

/// Opens a tiny hardware encoder to learn whether `codec` has hardware
/// support on this Mac. Use it to choose the codec to advertise before a
/// stream starts, instead of discovering a software fallback mid-session.
pub fn hardware_encoder_available(codec: VtCodec) -> bool {
    let options = EncoderOptions {
        codec,
        require_hardware: true,
        ..EncoderOptions::default()
    };
    EncoderWorker::new(EncoderConfig::compatibility(640, 360, 30), &options).is_ok()
}

pub(crate) enum Command {
    Frame(CaptureFrame),
    ForceKeyFrame,
    UpdateBitrate(u32),
    /// Nothing to apply: the worker re-reads the shared slot and controls.
    Wake,
    Stop,
}

enum FrameInput {
    Bgra(CaptureFrame),
    Surface(SurfaceFrame),
}

impl FrameInput {
    fn dimensions(&self) -> (u32, u32) {
        match self {
            Self::Bgra(frame) => (frame.width, frame.height),
            Self::Surface(frame) => (frame.width, frame.height),
        }
    }
}

type BitrateReply = SyncSender<Result<u32, EncodeError>>;

#[derive(Default)]
struct Controls {
    force_key_frame: bool,
    bitrate: Option<u32>,
    replies: Vec<BitrateReply>,
}

/// State shared between the owner thread and the encoder worker.
struct Shared {
    /// Replace-latest raw frame: a stale picture is never encoded when a newer
    /// one exists.
    frame: LatestSlot<FrameInput>,
    controls: Mutex<Controls>,
    applied_bitrate: AtomicU32,
    bitrate_error: Mutex<Option<String>>,
    hardware: AtomicBool,
}

pub struct VideoToolboxEncoder {
    pub(crate) sender: SyncSender<Command>,
    shared: Arc<Shared>,
    output: OutputSender,
    wakeup: Arc<Wakeup>,
    info: EncoderInfo,
    worker: Option<thread::JoinHandle<()>>,
}

fn open_hardware_fallback<T>(
    codec: VtCodec,
    mut open: impl FnMut(VtCodec) -> Result<T, EncodeError>,
) -> Result<T, EncodeError> {
    match open(codec) {
        Err(error) if codec == VtCodec::Hevc => {
            tracing::warn!(%error, "HEVC hardware unavailable; trying H.264 hardware");
            open(VtCodec::H264)
        }
        result => result,
    }
}

impl VideoToolboxEncoder {
    pub fn start_hardware_fallback(
        config: EncoderConfig,
        mut options: EncoderOptions,
    ) -> Result<(Self, EncodedReceiver), EncodeError> {
        options.require_hardware = true;
        open_hardware_fallback(options.codec, |codec| {
            Self::start_with(
                config,
                EncoderOptions {
                    codec,
                    ..options.clone()
                },
            )
        })
    }

    pub fn start(config: EncoderConfig) -> Result<(Self, EncodedReceiver), EncodeError> {
        Self::start_with(config, EncoderOptions::default())
    }

    pub fn start_with(
        mut config: EncoderConfig,
        options: EncoderOptions,
    ) -> Result<(Self, EncodedReceiver), EncodeError> {
        if config.frames_per_second == 0 {
            config.frames_per_second = 60;
        }
        if config.key_frame_interval == 0 {
            config.key_frame_interval = config.frames_per_second;
        }
        if config.width == 0 || config.height == 0 {
            return Err(EncodeError::InvalidConfiguration);
        }
        config.bitrate = clamp_bitrate(config.bitrate);
        ffmpeg::init()?;
        if ffmpeg::encoder::find_by_name(options.codec.encoder_name()).is_none() {
            return Err(EncodeError::EncoderUnavailable);
        }
        // Open on the caller's thread so an unsupported configuration is a
        // start error, not an asynchronous failure after the stream began.
        let worker_state = EncoderWorker::new(config, &options)?;
        let info = worker_state.info;

        let wakeup = options
            .wakeup
            .clone()
            .unwrap_or_else(|| Arc::new(Wakeup::default()));
        let (command_tx, command_rx) = mpsc::sync_channel(ENCODE_QUEUE_DEPTH);
        let (output_tx, output_rx) = output_channel_with(Arc::clone(&wakeup));
        let shared = Arc::new(Shared {
            frame: LatestSlot::new(Arc::new(Wakeup::default())),
            controls: Mutex::new(Controls::default()),
            applied_bitrate: AtomicU32::new(config.bitrate),
            bitrate_error: Mutex::new(None),
            hardware: AtomicBool::new(info.hardware),
        });
        let worker_shared = Arc::clone(&shared);
        let worker_output = OutputSender {
            shared: Arc::clone(&output_tx.shared),
        };
        let worker = thread::Builder::new()
            .name("ferryx-rd-host-videotoolbox".into())
            .spawn(move || {
                let result = run_worker(worker_state, &command_rx, &worker_shared, &worker_output);
                if let Err(error) = result {
                    let _ = worker_output.send(Err(error));
                }
            })
            .map_err(|_| EncodeError::WorkerStopped)?;
        Ok((
            Self {
                sender: command_tx,
                shared,
                output: output_tx,
                wakeup,
                info,
                worker: Some(worker),
            },
            output_rx,
        ))
    }

    /// Codec and hardware status the encoder opened with.
    pub fn info(&self) -> EncoderInfo {
        self.info
    }

    /// False only when hardware was unavailable and software was explicitly
    /// accepted (`require_hardware == false`).
    pub fn is_hardware(&self) -> bool {
        self.shared.hardware.load(Ordering::Acquire)
    }

    /// Notified on compressed output, and by whoever shares it via
    /// [`EncoderOptions::wakeup`].
    pub fn wakeup(&self) -> &Arc<Wakeup> {
        &self.wakeup
    }

    fn wake_worker(&self) -> Result<(), EncodeError> {
        // Pair with the output-space wait: controls must wake a congested
        // encoder as well as one sleeping on the command channel.
        {
            let _state = self.output.shared.state.lock().expect("output poisoned");
            self.output.shared.space.notify_all();
        }
        // A full command queue means the worker is already about to run; the
        // shared slot and controls are re-read on every pass.
        match self.sender.try_send(Command::Wake) {
            Ok(()) | Err(TrySendError::Full(_)) => Ok(()),
            Err(TrySendError::Disconnected(_)) => Err(EncodeError::WorkerStopped),
        }
    }

    /// Hands over the newest copied frame, replacing any unencoded one.
    pub fn submit(&self, frame: CaptureFrame) -> Result<(), EncodeError> {
        self.shared.frame.publish(FrameInput::Bgra(frame));
        self.wake_worker()
    }

    /// Hands over the newest GPU surface, replacing (and releasing) any
    /// unencoded one. Never blocks and never copies pixels.
    pub fn submit_surface(&self, frame: SurfaceFrame) -> Result<(), EncodeError> {
        self.shared.frame.publish(FrameInput::Surface(frame));
        self.wake_worker()
    }

    /// Raw frames evicted before encoding because the encoder was behind.
    pub fn replaced_frames(&self) -> u64 {
        self.shared.frame.replaced()
    }

    /// Requests an IDR. Never blocks, even when the command queue is full.
    pub fn force_key_frame(&self) -> Result<(), EncodeError> {
        self.shared
            .controls
            .lock()
            .map_err(|_| EncodeError::WorkerStopped)?
            .force_key_frame = true;
        self.wake_worker()
    }

    /// Requests a bitrate change; the result is observable through
    /// [`Self::applied_bitrate`] / [`Self::take_bitrate_error`], or use
    /// [`Self::update_bitrate_confirmed`] to wait for it.
    pub fn update_bitrate(&self, bitrate: u32) -> Result<(), EncodeError> {
        self.shared
            .controls
            .lock()
            .map_err(|_| EncodeError::WorkerStopped)?
            .bitrate = Some(bitrate);
        self.wake_worker()
    }

    /// Applies `bitrate` and returns the rate the encoder actually runs at
    /// (clamped to the validated range), or why it could not be applied.
    pub fn update_bitrate_confirmed(
        &self,
        bitrate: u32,
        timeout: Duration,
    ) -> Result<u32, EncodeError> {
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        {
            let mut controls = self
                .shared
                .controls
                .lock()
                .map_err(|_| EncodeError::WorkerStopped)?;
            controls.bitrate = Some(bitrate);
            controls.replies.push(reply_tx);
        }
        self.wake_worker()?;
        reply_rx
            .recv_timeout(timeout)
            .map_err(|_| EncodeError::WorkerStopped)?
    }

    /// Rate the encoder currently runs at: updated only after a successful
    /// apply, so it never claims a rate that failed.
    pub fn applied_bitrate(&self) -> u32 {
        self.shared.applied_bitrate.load(Ordering::Acquire)
    }

    /// The most recent bitrate-apply failure, if any, cleared on read.
    pub fn take_bitrate_error(&self) -> Option<String> {
        self.shared
            .bitrate_error
            .lock()
            .ok()
            .and_then(|mut error| error.take())
    }

    /// Cancel pending output and join, even if the output receiver is full.
    /// Compressed output is never discarded while the encoder is active.
    pub fn stop(self) {
        drop(self);
    }
}

impl Drop for VideoToolboxEncoder {
    fn drop(&mut self) {
        self.output.cancel();
        // Wake an idle worker, but never wait for space in a full command queue.
        let _ = self.sender.try_send(Command::Stop);
        if let Some(worker) = self.worker.take() {
            if let Err(error) = worker.join() {
                tracing::error!(?error, "VideoToolbox worker panicked");
            }
        }
    }
}

fn run_worker(
    mut worker: EncoderWorker,
    commands: &mpsc::Receiver<Command>,
    shared: &Shared,
    output: &OutputSender,
) -> Result<(), EncodeError> {
    let mut stop = false;
    let mut pending = VecDeque::new();
    while !stop {
        while !pending.is_empty() {
            if !output.wait_for_space_or_controls(shared) {
                return Ok(());
            }
            service_controls(&mut worker, shared, &mut pending)?;
            if output.has_space() {
                let frame = pending.pop_front().expect("pending packet exists");
                if output.send(frame).is_err() {
                    return Ok(());
                }
            }
        }
        // Event driven: sleep until a frame, control or stop is signalled.
        match commands.recv() {
            Ok(command) => stop |= absorb(command, shared),
            Err(_) => break,
        }
        // Drain whatever else arrived so the newest frame and every control
        // are seen before encoding anything.
        while let Ok(command) = commands.try_recv() {
            stop |= absorb(command, shared);
        }
        if output.is_stopped() {
            return Ok(());
        }
        service_controls(&mut worker, shared, &mut pending)?;
        // Do not start a picture the session cannot take yet: wait for output
        // space first, then encode the *newest* frame, so congestion costs
        // dropped raw frames (chain safe) and never stale pictures.
        if shared.frame.is_empty() {
            continue;
        }
        loop {
            if !output.wait_for_space_or_controls(shared) {
                return Ok(());
            }
            service_controls(&mut worker, shared, &mut pending)?;
            if output.has_space() {
                break;
            }
        }
        let Some(frame) = shared.frame.take() else {
            continue;
        };
        match worker.encode(frame) {
            Ok(frames) => {
                pending.extend(frames.into_iter().map(Ok));
            }
            Err(error) => {
                pending.push_back(Err(error));
            }
        }
        stop |= output.is_stopped();
    }
    worker.flush(output)
}

/// Legacy per-command entry points; returns true to stop.
fn absorb(command: Command, shared: &Shared) -> bool {
    match command {
        Command::Frame(frame) => shared.frame.publish(FrameInput::Bgra(frame)),
        Command::ForceKeyFrame => {
            shared
                .controls
                .lock()
                .expect("controls poisoned")
                .force_key_frame = true
        }
        Command::UpdateBitrate(bitrate) => {
            shared.controls.lock().expect("controls poisoned").bitrate = Some(bitrate)
        }
        Command::Wake => {}
        Command::Stop => return true,
    }
    false
}

fn service_controls(
    worker: &mut EncoderWorker,
    shared: &Shared,
    pending: &mut VecDeque<Result<EncodedFrame, EncodeError>>,
) -> Result<(), EncodeError> {
    let (force, bitrate, replies) = {
        let mut controls = shared.controls.lock().expect("controls poisoned");
        (
            std::mem::take(&mut controls.force_key_frame),
            controls.bitrate.take(),
            std::mem::take(&mut controls.replies),
        )
    };
    if force {
        worker.force_key_frame = true;
    }
    if let Some(requested) = bitrate {
        match worker.apply_bitrate(requested) {
            Ok((applied, drained)) => {
                shared.applied_bitrate.store(applied, Ordering::Release);
                shared
                    .hardware
                    .store(worker.info.hardware, Ordering::Release);
                *shared.bitrate_error.lock().expect("bitrate error poisoned") = None;
                for reply in replies {
                    let _ = reply.send(Ok(applied));
                }
                // The old encoder's remaining packets precede the new IDR.
                pending.extend(drained.into_iter().map(Ok));
            }
            Err(error) => {
                let message = error.to_string();
                tracing::warn!(requested, %message, "VideoToolbox bitrate change not applied");
                *shared.bitrate_error.lock().expect("bitrate error poisoned") =
                    Some(message.clone());
                for reply in replies {
                    let _ = reply.send(Err(EncodeError::BitrateNotApplied(message.clone())));
                }
            }
        }
    } else {
        for reply in replies {
            let _ = reply.send(Ok(worker.config.bitrate));
        }
    }
    Ok(())
}

/// Device and frames contexts that let FFmpeg accept `AV_PIX_FMT_VIDEOTOOLBOX`
/// frames wrapping our own CVPixelBuffers.
struct HwFrames {
    device: *mut ffmpeg::sys::AVBufferRef,
    frames: *mut ffmpeg::sys::AVBufferRef,
}

// SAFETY: AVBufferRef is atomically refcounted; the worker owns this value.
unsafe impl Send for HwFrames {}

impl HwFrames {
    fn new(width: u32, height: u32) -> Result<Self, EncodeError> {
        use ffmpeg::sys::*;
        // SAFETY: plain FFmpeg allocation calls; every failure path releases
        // what was created before it, and the result owns both references.
        unsafe {
            let mut device = std::ptr::null_mut();
            let status = av_hwdevice_ctx_create(
                &mut device,
                AVHWDeviceType::AV_HWDEVICE_TYPE_VIDEOTOOLBOX,
                std::ptr::null(),
                std::ptr::null_mut(),
                0,
            );
            if status < 0 {
                return Err(EncodeError::Ffmpeg(ffmpeg::Error::from(status)));
            }
            let mut frames = av_hwframe_ctx_alloc(device);
            if frames.is_null() {
                av_buffer_unref(&mut device);
                return Err(EncodeError::EncoderUnavailable);
            }
            let context = (*frames).data.cast::<AVHWFramesContext>();
            (*context).format = AVPixelFormat::AV_PIX_FMT_VIDEOTOOLBOX;
            (*context).sw_format = AVPixelFormat::AV_PIX_FMT_NV12;
            (*context).width = width as i32;
            (*context).height = height as i32;
            // AVVTFramesContext is a single `enum AVColorRange color_range`:
            // 420f full range, matching what ScreenCaptureKit delivers.
            *(*context).hwctx.cast::<i32>() = AVColorRange::AVCOL_RANGE_JPEG as i32;
            let status = av_hwframe_ctx_init(frames);
            if status < 0 {
                av_buffer_unref(&mut frames);
                av_buffer_unref(&mut device);
                return Err(EncodeError::Ffmpeg(ffmpeg::Error::from(status)));
            }
            Ok(Self { device, frames })
        }
    }
}

impl Drop for HwFrames {
    fn drop(&mut self) {
        // SAFETY: both references are owned by this value.
        unsafe {
            ffmpeg::sys::av_buffer_unref(&mut self.frames);
            ffmpeg::sys::av_buffer_unref(&mut self.device);
        }
    }
}

/// Free callback of the AVBuffer that owns a wrapped CVPixelBuffer.
unsafe extern "C" fn release_pixel_buffer(_opaque: *mut std::ffi::c_void, data: *mut u8) {
    if let Some(ptr) = std::ptr::NonNull::new(data.cast()) {
        // SAFETY: `data` is the +1 reference handed to `av_buffer_create`.
        drop(crate::capture_macos::PixelBuffer::from_raw(ptr));
    }
}

struct EncoderWorker {
    encoder: ffmpeg::encoder::Video,
    config: EncoderConfig,
    options: EncoderOptions,
    info: EncoderInfo,
    hw: Option<HwFrames>,
    next_pts: i64,
    force_key_frame: bool,
    timing_by_pts: HashMap<i64, FrameTiming>,
    parameter_sets: Vec<Vec<u8>>,
}

#[derive(Clone, Copy)]
struct FrameTiming {
    captured_at: Instant,
    callback_at: Instant,
    encode_started_at: Instant,
}

impl EncoderWorker {
    fn new(config: EncoderConfig, options: &EncoderOptions) -> Result<Self, EncodeError> {
        match Self::open(config, options, false) {
            Ok(worker) => Ok(worker),
            Err(error) if options.require_hardware => {
                tracing::error!(?error, codec = ?options.codec, "hardware VideoToolbox encoder unavailable");
                Err(EncodeError::HardwareUnavailable)
            }
            Err(error) => {
                // Explicit, logged degradation: the session can see it through
                // `is_hardware()` and decide whether to keep streaming.
                tracing::warn!(
                    ?error,
                    codec = ?options.codec,
                    "hardware VideoToolbox encoder unavailable; falling back to software"
                );
                Self::open(config, options, true)
            }
        }
    }

    fn open(
        config: EncoderConfig,
        options: &EncoderOptions,
        allow_software: bool,
    ) -> Result<Self, EncodeError> {
        let codec = ffmpeg::encoder::find_by_name(options.codec.encoder_name())
            .ok_or(EncodeError::EncoderUnavailable)?;
        let mut encoder = ffmpeg::codec::context::Context::new_with_codec(codec)
            .encoder()
            .video()?;
        encoder.set_width(config.width);
        encoder.set_height(config.height);
        let hw = match options.input {
            InputKind::Bgra => {
                // h264_videotoolbox accepts no BGRA; it gets NV12 converted by
                // `bgra_to_nv12_frame`. The zero-copy surface path avoids both.
                encoder.set_format(match options.codec {
                    VtCodec::Hevc => ffmpeg::format::Pixel::BGRA,
                    VtCodec::H264 => ffmpeg::format::Pixel::NV12,
                });
                None
            }
            InputKind::Nv12Surface => {
                encoder.set_format(ffmpeg::format::Pixel::VIDEOTOOLBOX);
                let hw = HwFrames::new(config.width, config.height)?;
                // SAFETY: the codec context takes its own reference; `hw` keeps
                // the original until the worker drops.
                unsafe {
                    let context = encoder.as_mut_ptr();
                    (*context).hw_frames_ctx = ffmpeg::sys::av_buffer_ref(hw.frames);
                    if (*context).hw_frames_ctx.is_null() {
                        return Err(EncodeError::EncoderUnavailable);
                    }
                }
                encoder.set_colorspace(ffmpeg::color::Space::SMPTE170M);
                Some(hw)
            }
        };
        encoder.set_color_range(ffmpeg::color::Range::JPEG);
        encoder.set_time_base((1, config.frames_per_second as i32));
        encoder.set_frame_rate(Some((config.frames_per_second as i32, 1)));
        encoder.set_gop(config.key_frame_interval);
        encoder.set_max_b_frames(0);
        encoder.set_bit_rate(config.bitrate as usize);
        encoder.set_max_bit_rate(config.bitrate as usize);
        encoder.set_tolerance(config.bitrate as usize);
        encoder.set_flags(ffmpeg::codec::Flags::LOW_DELAY);

        let mut dictionary = ffmpeg::Dictionary::new();
        dictionary.set("realtime", "1");
        // Without allow_sw FFmpeg requires a hardware encoder and the open
        // fails when none exists, which is how hardware use is guaranteed.
        if allow_software {
            dictionary.set("allow_sw", "1");
        }
        dictionary.set("profile", options.codec.profile());
        dictionary.set("max_ref_frames", "1");
        dictionary.set("prio_speed", "1");
        dictionary.set("constant_bit_rate", "0");
        let encoder = encoder.open_as_with(codec, dictionary)?;

        Ok(Self {
            encoder,
            config,
            options: options.clone(),
            info: EncoderInfo {
                codec: options.codec,
                hardware: !allow_software,
            },
            hw,
            next_pts: 0,
            force_key_frame: true,
            timing_by_pts: HashMap::new(),
            parameter_sets: Vec::new(),
        })
    }

    fn encode(&mut self, input: FrameInput) -> Result<Vec<EncodedFrame>, EncodeError> {
        let (width, height) = input.dimensions();
        match (&input, self.options.input) {
            (FrameInput::Bgra(_), InputKind::Bgra)
            | (FrameInput::Surface(_), InputKind::Nv12Surface) => {}
            _ => return Err(EncodeError::FrameKind),
        }
        if let FrameInput::Bgra(frame) = &input {
            if frame.bytes_per_row < frame.width as usize * 4
                || frame.bgra.len() < frame.bytes_per_row * frame.height as usize
            {
                return Err(EncodeError::FrameShape);
            }
        }
        let mut output = Vec::new();
        if width != self.config.width || height != self.config.height {
            // Desktop resolution changed: rebuild the encoder instead of
            // failing the worker. The fresh encoder forces a keyframe so the
            // client receives parameter sets for the new size. Packets the old
            // encoder still owes go out first, in order.
            let fresh = EncoderWorker::new(
                EncoderConfig {
                    width,
                    height,
                    ..self.config
                },
                &self.options,
            )?;
            output = self.swap_in(fresh)?;
        }
        let encode_started_at = Instant::now();
        let pts = self.next_pts;
        self.next_pts += 1;

        let (captured_at, callback_at, av_frame) = match input {
            FrameInput::Bgra(frame) if self.info.codec == VtCodec::H264 => {
                let av_frame = self.bgra_to_nv12_frame(&frame)?;
                (frame.captured_at, frame.captured_at, av_frame)
            }
            FrameInput::Bgra(frame) => {
                let mut av_frame = ffmpeg::frame::Video::new(
                    ffmpeg::format::Pixel::BGRA,
                    self.config.width,
                    self.config.height,
                );
                av_frame.set_color_range(ffmpeg::color::Range::JPEG);
                let destination_stride = av_frame.stride(0);
                for row in 0..self.config.height as usize {
                    let source_start = row * frame.bytes_per_row;
                    let destination_start = row * destination_stride;
                    let row_bytes = self.config.width as usize * 4;
                    av_frame.data_mut(0)[destination_start..destination_start + row_bytes]
                        .copy_from_slice(&frame.bgra[source_start..source_start + row_bytes]);
                }
                (frame.captured_at, frame.captured_at, av_frame)
            }
            FrameInput::Surface(frame) => {
                let av_frame = self.wrap_surface(frame.buffer)?;
                (frame.captured_at, frame.callback_at, av_frame)
            }
        };
        let mut av_frame = av_frame;
        av_frame.set_pts(Some(pts));
        if self.force_key_frame {
            av_frame.set_kind(ffmpeg::picture::Type::I);
            self.force_key_frame = false;
        }
        self.timing_by_pts.insert(
            pts,
            FrameTiming {
                captured_at,
                callback_at,
                encode_started_at,
            },
        );
        match self.encoder.send_frame(&av_frame) {
            Ok(()) => {}
            Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::error::EAGAIN => {
                output.extend(self.receive_available()?);
                self.encoder.send_frame(&av_frame)?;
                output.extend(self.receive_available()?);
                return Ok(output);
            }
            Err(error) => return Err(EncodeError::Ffmpeg(error)),
        }
        output.extend(self.receive_available()?);
        Ok(output)
    }

    /// Compatibility conversion for H.264 from copied BGRA frames.
    fn bgra_to_nv12_frame(
        &self,
        frame: &CaptureFrame,
    ) -> Result<ffmpeg::frame::Video, EncodeError> {
        let mut source = ffmpeg::frame::Video::new(
            ffmpeg::format::Pixel::BGRA,
            self.config.width,
            self.config.height,
        );
        let stride = source.stride(0);
        let row_bytes = self.config.width as usize * 4;
        for row in 0..self.config.height as usize {
            let from = row * frame.bytes_per_row;
            source.data_mut(0)[row * stride..row * stride + row_bytes]
                .copy_from_slice(&frame.bgra[from..from + row_bytes]);
        }
        let mut scaler = ffmpeg::software::scaling::Context::get(
            ffmpeg::format::Pixel::BGRA,
            self.config.width,
            self.config.height,
            ffmpeg::format::Pixel::NV12,
            self.config.width,
            self.config.height,
            ffmpeg::software::scaling::Flags::BILINEAR,
        )?;
        let mut nv12 = ffmpeg::frame::Video::empty();
        scaler.run(&source, &mut nv12)?;
        nv12.set_color_range(ffmpeg::color::Range::JPEG);
        Ok(nv12)
    }

    /// Wraps a retained CVPixelBuffer as an FFmpeg VideoToolbox frame. The
    /// frame owns the +1 reference and releases it when FFmpeg drops it; no
    /// pixel is read or copied.
    fn wrap_surface(
        &self,
        buffer: crate::capture_macos::PixelBuffer,
    ) -> Result<ffmpeg::frame::Video, EncodeError> {
        use ffmpeg::sys::*;
        let hw = self.hw.as_ref().ok_or(EncodeError::FrameKind)?;
        let mut frame = ffmpeg::frame::Video::empty();
        let raw = buffer.into_raw();
        // SAFETY: `raw` is a +1 CVPixelBufferRef whose ownership moves into the
        // AVBuffer; the free callback releases it exactly once, including on
        // the failure paths below via the frame's own cleanup.
        unsafe {
            let owner = av_buffer_create(
                raw.as_ptr().cast(),
                1,
                Some(release_pixel_buffer),
                std::ptr::null_mut(),
                0,
            );
            if owner.is_null() {
                drop(crate::capture_macos::PixelBuffer::from_raw(raw));
                return Err(EncodeError::EncoderUnavailable);
            }
            let target = frame.as_mut_ptr();
            (*target).format = AVPixelFormat::AV_PIX_FMT_VIDEOTOOLBOX as i32;
            (*target).width = self.config.width as i32;
            (*target).height = self.config.height as i32;
            (*target).data[3] = raw.as_ptr().cast();
            (*target).buf[0] = owner;
            (*target).hw_frames_ctx = av_buffer_ref(hw.frames);
            (*target).color_range = AVColorRange::AVCOL_RANGE_JPEG;
            (*target).colorspace = AVColorSpace::AVCOL_SPC_SMPTE170M;
            if (*target).hw_frames_ctx.is_null() {
                return Err(EncodeError::EncoderUnavailable);
            }
        }
        Ok(frame)
    }

    /// Replaces this encoder with `fresh` after draining everything the old one
    /// still holds, so compressed order is preserved across the switch.
    fn swap_in(&mut self, mut fresh: EncoderWorker) -> Result<Vec<EncodedFrame>, EncodeError> {
        self.encoder.send_eof()?;
        let drained = self.receive_available()?;
        // The fresh encoder opens with a forced IDR and the same pts clock.
        fresh.next_pts = self.next_pts;
        std::mem::swap(self, &mut fresh);
        Ok(drained)
    }

    /// Applies a bitrate. FFmpeg's VideoToolbox wrapper reads the rate only when
    /// the session is created, so a live encoder cannot be retargeted; instead
    /// the encoder is rebuilt at the new rate behind an IDR. On success the
    /// applied rate is persisted in `config`, so later size-triggered rebuilds
    /// keep it. On failure the running encoder and its config are untouched.
    fn apply_bitrate(&mut self, requested: u32) -> Result<(u32, Vec<EncodedFrame>), EncodeError> {
        let applied = clamp_bitrate(requested);
        if applied == self.config.bitrate {
            return Ok((applied, Vec::new()));
        }
        let fresh = EncoderWorker::new(
            EncoderConfig {
                bitrate: applied,
                ..self.config
            },
            &self.options,
        )
        .map_err(|error| EncodeError::BitrateNotApplied(error.to_string()))?;
        let drained = self.swap_in(fresh)?;
        Ok((applied, drained))
    }

    fn receive_available(&mut self) -> Result<Vec<EncodedFrame>, EncodeError> {
        let mut output = Vec::new();
        loop {
            match self.receive_one() {
                Ok(frame) => output.push(frame),
                Err(EncodeError::Ffmpeg(ffmpeg::Error::Other { errno }))
                    if errno == ffmpeg::error::EAGAIN =>
                {
                    break
                }
                Err(EncodeError::Ffmpeg(ffmpeg::Error::Eof)) => break,
                Err(error) => return Err(error),
            }
        }
        Ok(output)
    }

    fn receive_one(&mut self) -> Result<EncodedFrame, EncodeError> {
        let mut packet = ffmpeg::Packet::empty();
        self.encoder.receive_packet(&mut packet)?;
        let packet_pts = packet.pts().unwrap_or(self.next_pts - 1);
        let now = Instant::now();
        let timing = self
            .timing_by_pts
            .remove(&packet_pts)
            .unwrap_or(FrameTiming {
                captured_at: now,
                callback_at: now,
                encode_started_at: now,
            });
        let is_key_frame = packet.is_key();
        let raw = packet.data().ok_or(EncodeError::MalformedHevc)?;
        let data = normalize_packet(self.info.codec, raw, is_key_frame, &mut self.parameter_sets)?;
        Ok(EncodedFrame {
            data,
            is_key_frame,
            capture_at: timing.captured_at,
            callback_at: timing.callback_at,
            encode_started_at: timing.encode_started_at,
            encode_completed_at: Instant::now(),
        })
    }

    fn flush(&mut self, output: &OutputSender) -> Result<(), EncodeError> {
        self.encoder.send_eof()?;
        for frame in self.receive_available()? {
            if output.send(Ok(frame)).is_err() {
                break;
            }
        }
        Ok(())
    }
}

/// FFmpeg's VideoToolbox encoder may expose Annex-B or 4-byte length-prefixed
/// output depending on its build. The v3 wire always carries AVCC-style NALUs,
/// for HEVC (VPS/SPS/PPS) and H.264 (SPS/PPS) alike.
fn normalize_packet(
    codec: VtCodec,
    packet: &[u8],
    is_key_frame: bool,
    parameter_sets: &mut Vec<Vec<u8>>,
) -> Result<Vec<u8>, EncodeError> {
    let nalus = split_nalus(packet)?;
    let is_ps = |nalu: &[u8]| {
        codec
            .nalu_type(nalu)
            .is_some_and(|kind| codec.is_parameter_set(kind))
    };
    for nalu in &nalus {
        let kind = codec.nalu_type(nalu).ok_or(EncodeError::MalformedHevc)?;
        if !codec.is_parameter_set(kind) {
            continue;
        }
        // Replace the cached set of this type instead of appending. A re-issued
        // parameter set supersedes the previous one; keeping both would grow the
        // cache for the session's lifetime and prepend conflicting parameter
        // sets with identical ids into every keyframe access unit.
        match parameter_sets
            .iter_mut()
            .find(|known| codec.nalu_type(known) == Some(kind))
        {
            Some(stored) => {
                if stored.as_slice() != *nalu {
                    *stored = nalu.to_vec();
                }
            }
            None => parameter_sets.push(nalu.to_vec()),
        }
    }

    let mut output = Vec::with_capacity(packet.len() + 256);
    if is_key_frame {
        for parameter_set in parameter_sets.iter().filter(|nalu| is_ps(nalu)) {
            append_length_prefixed(&mut output, parameter_set)?;
        }
    }
    for nalu in nalus {
        if is_key_frame && is_ps(nalu) {
            continue;
        }
        append_length_prefixed(&mut output, nalu)?;
    }
    Ok(output)
}

fn split_nalus(packet: &[u8]) -> Result<Vec<&[u8]>, EncodeError> {
    if let Ok(nalus) = split_avcc(packet) {
        return Ok(nalus);
    }
    split_annex_b(packet)
}

fn split_avcc(packet: &[u8]) -> Result<Vec<&[u8]>, EncodeError> {
    let mut offset = 0;
    let mut nalus = Vec::new();
    while offset < packet.len() {
        if packet.len() - offset < 4 {
            return Err(EncodeError::MalformedHevc);
        }
        let length = u32::from_be_bytes(packet[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        if length < 2 || packet.len() - offset < length {
            return Err(EncodeError::MalformedHevc);
        }
        nalus.push(&packet[offset..offset + length]);
        offset += length;
    }
    if nalus.is_empty() {
        return Err(EncodeError::MalformedHevc);
    }
    Ok(nalus)
}

fn split_annex_b(packet: &[u8]) -> Result<Vec<&[u8]>, EncodeError> {
    let mut starts = Vec::new();
    let mut index = 0;
    while index + 3 <= packet.len() {
        if packet[index..].starts_with(&[0, 0, 0, 1]) {
            starts.push((index, 4));
            index += 4;
        } else if packet[index..].starts_with(&[0, 0, 1]) {
            starts.push((index, 3));
            index += 3;
        } else {
            index += 1;
        }
    }
    if starts.is_empty() {
        return Err(EncodeError::MalformedHevc);
    }
    let mut nalus = Vec::new();
    for (position, (start, start_len)) in starts.iter().copied().enumerate() {
        let end = starts
            .get(position + 1)
            .map_or(packet.len(), |(next, _)| *next);
        let mut nalu_data = &packet[start + start_len..end];
        // Annex B start codes are frequently preceded by trailing_zero_8bits
        // or alignment padding; those zero bytes are not part of the NAL unit
        // and break decoder parameter-set parsing when copied into AVCC.
        while let Some((&0, rest)) = nalu_data.split_last() {
            nalu_data = rest;
        }
        if !nalu_data.is_empty() {
            nalus.push(nalu_data);
        }
    }
    if nalus.is_empty() {
        return Err(EncodeError::MalformedHevc);
    }
    Ok(nalus)
}

fn append_length_prefixed(output: &mut Vec<u8>, nalu: &[u8]) -> Result<(), EncodeError> {
    let length = u32::try_from(nalu.len()).map_err(|_| EncodeError::MalformedHevc)?;
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(nalu);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(index: u8) -> EncodedFrame {
        let now = Instant::now();
        EncodedFrame {
            data: vec![index],
            is_key_frame: index == 0,
            capture_at: now,
            callback_at: now,
            encode_started_at: now,
            encode_completed_at: now,
        }
    }

    fn fill(sender: &OutputSender) {
        for index in 0..OUTPUT_QUEUE_DEPTH {
            sender.send(Ok(frame(index as u8))).unwrap();
        }
    }

    #[test]
    fn output_queue_bounds_retention_without_dropping_references() {
        let (sender, receiver) = output_channel();
        fill(&sender);
        assert_eq!(receiver.len(), OUTPUT_QUEUE_DEPTH);
        let retained: Vec<_> = std::iter::from_fn(|| receiver.try_recv().ok())
            .map(|result| result.unwrap().data[0])
            .collect();
        assert_eq!(retained, vec![0, 1, 2]);
    }

    #[test]
    fn full_output_blocks_the_sender_until_space_opens_without_polling() {
        let (sender, receiver) = output_channel();
        fill(&sender);
        let (done_tx, done_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            // Blocks on the condvar; consuming one packet must wake it.
            let sent = sender.send(Ok(frame(9))).is_ok();
            done_tx.send(sent).unwrap();
            sender
        });
        assert!(matches!(done_rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
        assert_eq!(receiver.try_recv().unwrap().unwrap().data[0], 0);
        assert!(done_rx.recv_timeout(Duration::from_secs(2)).unwrap());
        let _sender = worker.join().unwrap();
        // Order is preserved: the retained packet landed after its predecessors.
        let rest: Vec<_> = std::iter::from_fn(|| receiver.try_recv().ok())
            .map(|result| result.unwrap().data[0])
            .collect();
        assert_eq!(rest, vec![1, 2, 9]);
    }

    #[test]
    fn dropping_output_receiver_releases_publisher() {
        let (sender, receiver) = output_channel();
        fill(&sender);
        let (ready_tx, ready_rx) = mpsc::channel();
        let (exit_tx, exit_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            ready_tx.send(()).unwrap();
            let disconnected = sender.send(Ok(frame(3))).is_err();
            exit_tx.send(disconnected).unwrap();
        });
        ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        drop(receiver);
        assert!(exit_rx.recv_timeout(Duration::from_secs(2)).unwrap());
        worker.join().unwrap();
    }

    #[test]
    fn cancel_releases_a_sender_blocked_on_a_full_queue_with_receiver_retained() {
        let (sender, receiver) = output_channel();
        fill(&sender);
        let sender = Arc::new(sender);
        let blocked = Arc::clone(&sender);
        let (done_tx, done_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            done_tx.send(blocked.send(Ok(frame(3))).is_err()).unwrap();
        });
        sender.cancel();
        let released = done_rx.recv_timeout(Duration::from_secs(2));
        // Release even on failure so no worker is left blocked.
        drop(receiver);
        worker.join().unwrap();
        assert_eq!(released, Ok(true), "cancel did not wake the blocked sender");
        assert!(!sender.wait_for_space());
    }

    #[test]
    fn wait_for_space_wakes_when_the_consumer_pops() {
        let (sender, receiver) = output_channel();
        fill(&sender);
        let (done_tx, done_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            done_tx.send(sender.wait_for_space()).unwrap();
        });
        assert!(matches!(done_rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
        receiver.try_recv().unwrap().unwrap();
        assert!(done_rx.recv_timeout(Duration::from_secs(2)).unwrap());
        worker.join().unwrap();
    }

    #[test]
    fn hardware_codec_selection_falls_back_only_from_hevc() {
        let mut attempts = Vec::new();
        let selected = open_hardware_fallback(VtCodec::Hevc, |codec| {
            attempts.push(codec);
            if codec == VtCodec::Hevc {
                Err(EncodeError::HardwareUnavailable)
            } else {
                Ok(codec)
            }
        })
        .unwrap();
        assert_eq!(selected, VtCodec::H264);
        assert_eq!(attempts, vec![VtCodec::Hevc, VtCodec::H264]);
        assert!(open_hardware_fallback::<()>(VtCodec::H264, |_| Err(
            EncodeError::HardwareUnavailable
        ))
        .is_err());
        assert_eq!(
            open_hardware_fallback(VtCodec::Hevc, Ok).unwrap(),
            VtCodec::Hevc
        );
    }

    #[test]
    fn full_output_services_pending_controls_without_waiting_for_consumer() {
        let (sender, _receiver) = output_channel();
        fill(&sender);
        let shared = Shared {
            frame: LatestSlot::new(Arc::new(Wakeup::default())),
            controls: Mutex::new(Controls {
                force_key_frame: true,
                ..Default::default()
            }),
            applied_bitrate: AtomicU32::new(DEFAULT_BITRATE),
            bitrate_error: Mutex::new(None),
            hardware: AtomicBool::new(true),
        };
        let (done_tx, done_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            done_tx
                .send(sender.wait_for_space_or_controls(&shared))
                .unwrap();
        });
        assert_eq!(done_rx.recv_timeout(Duration::from_secs(2)), Ok(true));
        worker.join().unwrap();
    }

    #[test]
    fn recv_timeout_is_woken_by_output_not_by_polling() {
        let (sender, receiver) = output_channel();
        let (release_tx, release_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            sender.send(Ok(frame(5))).unwrap();
            sender
        });
        release_tx.send(()).unwrap();
        let got = receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        assert_eq!(got.data[0], 5);
        let _sender = worker.join().unwrap();
    }

    fn nalu(kind: u8, payload: u8) -> Vec<u8> {
        vec![kind << 1, 1, payload]
    }

    fn annex_b(units: &[&Vec<u8>]) -> Vec<u8> {
        let mut out = Vec::new();
        for unit in units {
            out.extend_from_slice(&[0, 0, 0, 1]);
            out.extend_from_slice(unit);
        }
        out
    }

    #[test]
    fn annex_b_trailing_zero_padding_is_trimmed() {
        // Given Annex B NALUs separated by trailing_zero_8bits padding.
        let packet = [0, 0, 0, 1, 0x40, 1, 2, 0, 0, 0, 0, 1, 0x26, 3];
        // When splitting, the padding must not join the preceding NAL unit.
        let nalus = split_annex_b(&packet).unwrap();
        assert_eq!(nalus, vec![&[0x40u8, 1, 2][..], &[0x26u8, 3][..]]);
    }

    #[test]
    fn nalu_splitting_borrows_packet_storage() {
        let packet = [0, 0, 0, 3, 2, 1, 99];
        let nalus = split_avcc(&packet).unwrap();
        assert_eq!(nalus[0].as_ptr(), packet[4..].as_ptr());
    }

    #[test]
    fn annex_b_is_converted_to_avcc_and_parameter_sets_prepend_keyframes() {
        let vps = nalu(32, 1);
        let sps = nalu(33, 2);
        let pps = nalu(34, 3);
        let idr = nalu(19, 4);
        let mut parameters = Vec::new();
        let encoded = normalize_packet(
            VtCodec::Hevc,
            &annex_b(&[&vps, &sps, &pps, &idr]),
            true,
            &mut parameters,
        )
        .unwrap();
        assert_eq!(split_avcc(&encoded).unwrap(), vec![vps, sps, pps, idr]);
    }

    #[test]
    fn reissued_parameter_sets_replace_the_cached_set() {
        let vps = nalu(32, 1);
        let sps = nalu(33, 2);
        let pps = nalu(34, 3);
        let idr = nalu(19, 4);
        let mut parameters = Vec::new();
        normalize_packet(
            VtCodec::Hevc,
            &annex_b(&[&vps, &sps, &pps, &idr]),
            true,
            &mut parameters,
        )
        .unwrap();
        let new_sps = nalu(33, 7);
        let new_pps = nalu(34, 8);
        let encoded = normalize_packet(
            VtCodec::Hevc,
            &annex_b(&[&new_sps, &new_pps, &idr]),
            true,
            &mut parameters,
        )
        .unwrap();
        assert_eq!(
            parameters,
            vec![vps.clone(), new_sps.clone(), new_pps.clone()]
        );
        assert_eq!(
            split_avcc(&encoded).unwrap(),
            vec![vps, new_sps, new_pps, idr]
        );
    }

    #[test]
    fn h264_packets_use_h264_nalu_types_not_hevc_ones() {
        // H.264 NAL header: type in the low 5 bits. SPS=7, PPS=8, IDR=5.
        let sps = vec![0x67, 1, 2];
        let pps = vec![0x68, 3];
        let idr = vec![0x65, 4];
        let p_slice = vec![0x41, 5];
        let mut parameters = Vec::new();
        // The IDR packet carries only SPS/PPS inline in Annex B; a following P
        // frame must not gain parameter sets.
        let key = normalize_packet(
            VtCodec::H264,
            &annex_b(&[&sps, &pps, &idr]),
            true,
            &mut parameters,
        )
        .unwrap();
        assert_eq!(
            split_avcc(&key).unwrap(),
            vec![sps.clone(), pps.clone(), idr]
        );
        assert_eq!(parameters, vec![sps, pps]);
        let delta =
            normalize_packet(VtCodec::H264, &annex_b(&[&p_slice]), false, &mut parameters).unwrap();
        assert_eq!(split_avcc(&delta).unwrap(), vec![p_slice]);
        // Read as HEVC the same bytes would be misclassified; the codec decides.
        assert_ne!(
            VtCodec::H264.nalu_type(&[0x67, 1]),
            VtCodec::Hevc.nalu_type(&[0x67, 1])
        );
    }

    #[test]
    fn h264_keyframes_without_inline_sets_reuse_the_cached_ones() {
        let sps = vec![0x67, 1, 2];
        let pps = vec![0x68, 3];
        let idr = vec![0x65, 4];
        let mut parameters = Vec::new();
        normalize_packet(
            VtCodec::H264,
            &annex_b(&[&sps, &pps, &idr]),
            true,
            &mut parameters,
        )
        .unwrap();
        let again =
            normalize_packet(VtCodec::H264, &annex_b(&[&idr]), true, &mut parameters).unwrap();
        assert_eq!(split_avcc(&again).unwrap(), vec![sps, pps, idr]);
    }

    #[test]
    fn codec_selects_the_matching_videotoolbox_encoder() {
        assert_eq!(VtCodec::Hevc.encoder_name(), "hevc_videotoolbox");
        assert_eq!(VtCodec::H264.encoder_name(), "h264_videotoolbox");
        assert_eq!(VtCodec::default(), VtCodec::Hevc);
    }

    #[test]
    fn lan_bitrate_is_clamped_to_requested_range() {
        assert_eq!(EncoderConfig::lan(1, 1, 60, 1).bitrate, MIN_LAN_BITRATE);
        assert_eq!(
            EncoderConfig::lan(1, 1, 60, u32::MAX).bitrate,
            MAX_LAN_BITRATE
        );
        assert_eq!(
            EncoderConfig::compatibility(1, 1, 60).bitrate,
            DEFAULT_BITRATE
        );
    }

    #[test]
    fn encoder_floor_matches_the_session_validated_minimum() {
        // The session accepts 100 kbit/s..300 Mbit/s; the encoder must neither
        // raise the floor (the old 1 Mbit/s) nor exceed the ceiling.
        assert_eq!(clamp_bitrate(100_000), 100_000);
        assert_eq!(clamp_bitrate(500_000), 500_000);
        assert_eq!(clamp_bitrate(1), MIN_ENCODER_BITRATE);
        assert_eq!(clamp_bitrate(u32::MAX), MAX_ENCODER_BITRATE);
    }

    #[test]
    fn newest_frame_replaces_an_unencoded_one_when_the_encoder_is_behind() {
        let slot = LatestSlot::new(Arc::new(Wakeup::default()));
        let now = Instant::now();
        for index in 0..5_u8 {
            slot.publish(FrameInput::Bgra(CaptureFrame {
                width: 1,
                height: 1,
                bytes_per_row: 4,
                bgra: vec![index; 4],
                captured_at: now,
            }));
        }
        match slot.take() {
            Some(FrameInput::Bgra(frame)) => assert_eq!(frame.bgra[0], 4),
            _ => panic!("expected the newest frame"),
        }
        assert_eq!(slot.replaced(), 4);
    }

    #[test]
    fn controls_are_coalesced_and_absorbed_without_a_queue_slot() {
        let shared = Shared {
            frame: LatestSlot::new(Arc::new(Wakeup::default())),
            controls: Mutex::new(Controls::default()),
            applied_bitrate: AtomicU32::new(0),
            bitrate_error: Mutex::new(None),
            hardware: AtomicBool::new(true),
        };
        for rate in 1..=1000 {
            assert!(!absorb(Command::UpdateBitrate(rate), &shared));
            assert!(!absorb(Command::ForceKeyFrame, &shared));
        }
        assert!(absorb(Command::Stop, &shared));
        let controls = shared.controls.lock().unwrap();
        assert!(controls.force_key_frame);
        assert_eq!(controls.bitrate, Some(1000));
    }

    /// A real NV12 420f CVPixelBuffer, allocated by CoreVideo (no capture, no
    /// permission): the same pixel format and ownership as an SCK surface.
    fn nv12_surface(width: u32, height: u32) -> SurfaceFrame {
        use objc2_core_video::{
            kCVPixelFormatType_420YpCbCr8BiPlanarFullRange, CVPixelBufferCreate,
        };
        let mut raw = std::ptr::null_mut();
        // SAFETY: valid out-pointer; null attributes request defaults.
        let status = unsafe {
            CVPixelBufferCreate(
                None,
                width as usize,
                height as usize,
                kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
                None,
                std::ptr::NonNull::new(&mut raw).unwrap(),
            )
        };
        assert_eq!(status, 0);
        // SAFETY: Create returns a +1 reference.
        let buffer = unsafe {
            crate::capture_macos::PixelBuffer::from_raw(std::ptr::NonNull::new(raw).unwrap())
        };
        let now = Instant::now();
        SurfaceFrame {
            width,
            height,
            buffer,
            captured_at: now,
            callback_at: now,
        }
    }

    fn bgra_frame(width: u32, height: u32) -> CaptureFrame {
        CaptureFrame {
            width,
            height,
            bytes_per_row: width as usize * 4,
            bgra: vec![90; width as usize * height as usize * 4],
            captured_at: Instant::now(),
        }
    }

    /// VideoToolbox may hold a picture until the next arrives, so keep feeding
    /// fresh frames until one packet comes out.
    fn encode_one(
        encoder: &VideoToolboxEncoder,
        receiver: &EncodedReceiver,
        mut next: impl FnMut() -> FrameInput,
    ) -> EncodedFrame {
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            match next() {
                FrameInput::Bgra(frame) => encoder.submit(frame).unwrap(),
                FrameInput::Surface(frame) => encoder.submit_surface(frame).unwrap(),
            }
            if let Ok(result) = receiver.recv_timeout(Duration::from_millis(100)) {
                return result.expect("encode error");
            }
        }
        panic!("encoder produced no output");
    }

    #[test]
    fn surface_frames_encode_through_videotoolbox_without_a_cpu_copy() {
        let options = EncoderOptions {
            input: InputKind::Nv12Surface,
            require_hardware: false,
            ..EncoderOptions::default()
        };
        let (encoder, receiver) =
            VideoToolboxEncoder::start_with(EncoderConfig::compatibility(640, 360, 30), options)
                .expect("surface encoder opens");
        let surface = || FrameInput::Surface(nv12_surface(640, 360));
        let first = encode_one(&encoder, &receiver, surface);
        assert!(first.is_key_frame && !first.data.is_empty());
        let second = encode_one(&encoder, &receiver, surface);
        assert!(!second.data.is_empty());
        encoder.stop();
    }

    #[test]
    fn h264_videotoolbox_produces_avcc_with_sps_pps_on_keyframes() {
        let options = EncoderOptions {
            codec: VtCodec::H264,
            ..EncoderOptions::default()
        };
        let (encoder, receiver) =
            VideoToolboxEncoder::start_with(EncoderConfig::compatibility(320, 240, 30), options)
                .expect("h264 encoder opens");
        let key = encode_one(&encoder, &receiver, || {
            FrameInput::Bgra(bgra_frame(320, 240))
        });
        assert!(key.is_key_frame);
        let kinds: Vec<u8> = split_avcc(&key.data)
            .unwrap()
            .iter()
            .map(|nalu| nalu[0] & 0x1f)
            .collect();
        assert!(kinds.contains(&7) && kinds.contains(&8), "got {kinds:?}");
        encoder.stop();
    }

    #[test]
    fn rebuild_controls_complete_while_output_is_full() {
        let options = EncoderOptions::default();
        let mut worker =
            EncoderWorker::new(EncoderConfig::compatibility(320, 240, 30), &options).unwrap();
        let shared = Shared {
            frame: LatestSlot::new(Arc::new(Wakeup::default())),
            controls: Mutex::new(Controls::default()),
            applied_bitrate: AtomicU32::new(DEFAULT_BITRATE),
            bitrate_error: Mutex::new(None),
            hardware: AtomicBool::new(true),
        };
        let (output, receiver) = output_channel();
        fill(&output);
        let mut pending = VecDeque::from([Ok(frame(91))]);
        for rate in [500_000, 600_000] {
            let (reply, result) = mpsc::sync_channel(1);
            *shared.controls.lock().unwrap() = Controls {
                force_key_frame: true,
                bitrate: Some(rate),
                replies: vec![reply],
            };
            service_controls(&mut worker, &shared, &mut pending).unwrap();
            assert_eq!(
                result
                    .recv_timeout(Duration::from_secs(2))
                    .unwrap()
                    .unwrap(),
                rate
            );
        }
        assert_eq!(receiver.len(), OUTPUT_QUEUE_DEPTH);
        assert_eq!(pending.pop_front().unwrap().unwrap().data, vec![91]);
        assert!(worker.force_key_frame);
    }

    #[test]
    fn bitrate_change_is_confirmed_persisted_and_survives_a_resize() {
        let (encoder, receiver) =
            VideoToolboxEncoder::start(EncoderConfig::compatibility(320, 240, 30)).unwrap();
        let bgra = |w, h| move || FrameInput::Bgra(bgra_frame(w, h));
        encode_one(&encoder, &receiver, bgra(320, 240));
        // A sub-1-Mbit setting is applied as asked, not raised to a 1 Mbit floor.
        let applied = encoder
            .update_bitrate_confirmed(500_000, Duration::from_secs(10))
            .unwrap();
        assert_eq!(applied, 500_000);
        assert_eq!(encoder.applied_bitrate(), 500_000);
        // The rebuild restarts on an IDR; packets the old encoder still owed
        // are delivered first, in order.
        let mut after = encode_one(&encoder, &receiver, bgra(320, 240));
        for _ in 0..8 {
            if after.is_key_frame {
                break;
            }
            after = encode_one(&encoder, &receiver, bgra(320, 240));
        }
        assert!(after.is_key_frame);
        // A size change rebuilds again but keeps the applied rate.
        let mut resized = encode_one(&encoder, &receiver, bgra(160, 120));
        for _ in 0..8 {
            if resized.is_key_frame {
                break;
            }
            resized = encode_one(&encoder, &receiver, bgra(160, 120));
        }
        assert!(resized.is_key_frame);
        assert_eq!(encoder.applied_bitrate(), 500_000);
        encoder.stop();
    }
}
