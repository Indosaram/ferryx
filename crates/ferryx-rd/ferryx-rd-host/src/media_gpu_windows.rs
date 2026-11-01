use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use super::*;
use crate::capture_windows::CaptureError;

#[path = "capture_windows_gpu.rs"]
mod capture_windows_gpu;
#[path = "encode_windows_hw.rs"]
mod encode_windows_hw;

use capture_windows_gpu::{GpuCapture, GpuFrame};
use encode_windows_hw::{HwMftEncoder, SchedulerAction};

pub(super) fn gpu_source_with_options(
    cpu: WindowsMediaSource,
    target: Option<(u32, u32)>,
    codec: VideoCodec,
) -> Arc<dyn MediaSource> {
    Arc::new(WindowsGpuMediaSource { cpu, target, codec })
}

struct WindowsGpuMediaSource {
    cpu: WindowsMediaSource,
    target: Option<(u32, u32)>,
    codec: VideoCodec,
}

impl MediaSource for WindowsGpuMediaSource {
    fn supports_cpu_scaling(&self) -> bool {
        true
    }

    fn output_target(&self) -> Option<TargetDisplay> {
        self.cpu.output_target()
    }

    fn start_with_capabilities(
        &self,
        sender: SyncSender<MediaEvent>,
        peer: Capabilities,
    ) -> Result<Box<dyn MediaHandle>, SessionError> {
        let gpu_capture = match GpuCapture::new_scaled(
            self.cpu.display_index,
            self.cpu.width,
            self.cpu.height,
            self.target.map_or(0, |v| v.0),
            self.target.map_or(0, |v| v.1),
        ) {
            Ok(capture) => capture,
            Err(error) => {
                crate::host_log(&format!("media pipeline: cpu fallback: dxgi init: {error}"));
                if self.target.is_some() {
                    return Err(SessionError::MediaStopped);
                }
                return self.cpu.start_with_capabilities(sender, peer);
            }
        };

        let adapter_name = gpu_capture.adapter_name().to_string();
        let adapter_luid = gpu_capture.adapter_luid();
        let d3d11_device = gpu_capture.d3d11_device();
        let (actual_w, actual_h) = gpu_capture.dimensions();

        let (init_tx, init_rx) = mpsc::sync_channel::<Result<VideoCodec, String>>(1);

        let mut workers = native_pipeline::Workers::<GpuFrame>::new();
        let first_keyframe_emitted = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let frame_dims = Arc::new(AtomicU64::new(
            (u64::from(actual_w) << 32) | u64::from(actual_h),
        ));

        {
            let sender = sender.clone();
            let first_keyframe_emitted = Arc::clone(&first_keyframe_emitted);
            let frame_dims = Arc::clone(&frame_dims);
            let display_index = self.cpu.display_index;
            let fps = self.cpu.fps.max(1);
            let mon_w = self.cpu.width;
            let mon_h = self.cpu.height;
            let target = self.target;

            workers.spawn("maho-win-capture", true, move |handoff| {
                let _mmcss = crate::windows_logic::MultimediaTask::register(false);
                struct KeepAwake;
                impl Drop for KeepAwake {
                    fn drop(&mut self) {
                        display_power::set_keep_awake(false);
                    }
                }
                display_power::set_keep_awake(true);
                let _awake = KeepAwake;

                struct TimerResolution {
                    active: bool,
                }
                impl TimerResolution {
                    fn new() -> Self {
                        // SAFETY: timeBeginPeriod requests a minimum timer resolution of 1 ms
                        // for the duration of the capture loop and has no pointer invariants.
                        let ret = unsafe { windows::Win32::Media::timeBeginPeriod(1) };
                        Self {
                            active: ret == windows::Win32::Media::TIMERR_NOERROR,
                        }
                    }
                }
                impl Drop for TimerResolution {
                    fn drop(&mut self) {
                        if self.active {
                            // SAFETY: timeEndPeriod clears the matching 1 ms period requested
                            // during initialization when timeBeginPeriod returned TIMERR_NOERROR.
                            unsafe {
                                windows::Win32::Media::timeEndPeriod(1);
                            }
                        }
                    }
                }
                let _timer_res = TimerResolution::new();

                // Initialization success releases capture; dropping the sender cancels it.
                if ready_rx.recv().is_err() { return; }

                let interval = Duration::from_micros(1_000_000 / u64::from(fps));
                let mut capture = gpu_capture;
                let mut capture_id = 0_u64;
                // This lease excludes the idle-refresh surface from available_slot().
                let mut last_frame: Option<GpuFrame> = None;
                let mut last_emit = Instant::now();
                let mut consecutive_failures = 0_u32;
                let mut jiggle_flip = false;
                let mut last_jiggle = Instant::now() - Duration::from_millis(700);

                let mut next_deadline = Instant::now() + interval;
                while !handoff.is_stopped() {
                    let started = Instant::now();
                    if let Some(trace) = host_trace::enabled() {
                        trace.record(host_trace::Record {
                            event: 9,
                            frame: capture_id + 1,
                            ..Default::default()
                        });
                    }

                    let Some(slot) = capture.available_slot() else {
                        handoff.pace_until(next_deadline);
                        next_deadline = Instant::now() + interval;
                        continue;
                    };
                    match capture.acquire_and_blt(interval, slot) {
                        Ok(_metadata) => {
                            consecutive_failures = 0;
                            let (w, h) = capture.dimensions();
                            frame_dims.store((u64::from(w) << 32) | u64::from(h), Ordering::Relaxed);

                            if let Some(trace) = host_trace::enabled() {
                                trace.record(host_trace::Record {
                                    event: 10,
                                    frame: capture_id + 1,
                                    value: trace.time(started),
                                    ..Default::default()
                                });
                            }

                            let captured_at = Instant::now();
                            capture_id += 1;
                            let published_at = Instant::now();
                            let frame = GpuFrame {
                                texture: capture.texture(slot),
                                captured_at,
                                capture_id,
                            };

                            if let Some(trace) = host_trace::enabled() {
                                trace.record(host_trace::Record {
                                    event: 11,
                                    frame: capture_id,
                                    value: trace.time(captured_at),
                                    ..Default::default()
                                });
                                trace.record(host_trace::Record {
                                    event: 12,
                                    frame: capture_id,
                                    value: trace.time(published_at),
                                    ..Default::default()
                                });
                            }

                            last_frame = Some(frame.clone());
                            last_emit = Instant::now();
                            if !handoff.publish(frame) {
                                break;
                            }
                        }
                        Err(CaptureError::Timeout) => {
                            if (!first_keyframe_emitted.load(Ordering::Relaxed)
                                || capture_id == 0)
                                && last_jiggle.elapsed() >= Duration::from_millis(300)
                            {
                                last_jiggle = started;
                                jiggle_flip = !jiggle_flip;
                                cursor_jiggle::nudge(jiggle_flip);
                            }

                            if crate::windows_logic::idle_refresh_due(
                                last_emit,
                                Instant::now(),
                                first_keyframe_emitted.load(Ordering::Relaxed),
                            ) {
                                if let Some(frame) = &last_frame {
                                    // Cloning preserves the captured surface lease and identity.
                                    // The unused slot selected above cannot alias this texture.
                                    if !handoff.publish(frame.clone()) {
                                        break;
                                    }
                                    last_emit = Instant::now();
                                }
                            }
                        }
                        Err(error) => {
                            use crate::windows_session::{capture_recovery, CaptureRecovery};

                            let failure = capture_failure_from_label(match &error {
                                CaptureError::AccessLost => "access_lost",
                                CaptureError::AccessDenied => "access_denied",
                                CaptureError::RefreshFailure => "refresh_failure",
                                _ => "capture",
                            });
                            tracing::warn!(%error, consecutive_failures, "GPU DXGI capture interrupted");

                            while !handoff.is_stopped() {
                                match capture_recovery(failure, consecutive_failures) {
                                    CaptureRecovery::Reacquire(delay) => {
                                        thread::sleep(delay);
                                        if handoff.is_stopped() {
                                            break;
                                        }
                                        let rebuilt = GpuCapture::new_scaled(
                                            display_index,
                                            mon_w,
                                            mon_h,
                                            target.map_or(0, |v| v.0),
                                            target.map_or(0, |v| v.1),
                                        );
                                        consecutive_failures =
                                            consecutive_failures.saturating_add(1);
                                        match rebuilt {
                                            Ok(rebuilt) => {
                                                last_frame = None;
                                                capture = rebuilt;
                                                break;
                                            }
                                            Err(error) => {
                                                tracing::warn!(%error, consecutive_failures, "GPU DXGI reacquire failed");
                                            }
                                        }
                                    }
                                    CaptureRecovery::Abort => {
                                        native_media_error(
                                            &handoff,
                                            &sender,
                                            format!("dxgi gpu: {error}"),
                                        );
                                        return;
                                    }
                                }
                            }
                            continue;
                        }
                    }

                    handoff.pace_until(next_deadline);
                    next_deadline += interval;
                    if Instant::now() > next_deadline + interval {
                        next_deadline = Instant::now() + interval;
                    }
                }
            })?;
        }

        if self.cpu.capture_audio {
            let sender = sender.clone();
            workers.spawn("maho-win-audio", false, move |handoff| {
                let _mmcss = crate::windows_logic::MultimediaTask::register(true);
                let mut capture = match crate::audio_windows::WindowsAudioCapture::new() {
                    Ok(capture) => capture,
                    Err(error) => {
                        tracing::warn!(%error, "Windows audio loopback unavailable; continuing without audio");
                        return;
                    }
                };
                tracing::info!("Windows audio loopback started");
                while !handoff.is_stopped() {
                    let pcm = capture.poll();
                    if pcm.is_empty() {
                        continue;
                    }
                    let _ = sender.send(MediaEvent::TimedAudio { pcm, captured_at: Instant::now() });
                }
            })?;
        }

        let encoder_config = EncoderConfig {
            width: actual_w,
            height: actual_h,
            bitrate: self.cpu.bitrate,
            fps: self.cpu.fps.max(1),
            keyframe_interval: 0,
            preferred_codec: self.codec,
        };

        {
            let sender = sender.clone();
            let first_keyframe_emitted = Arc::clone(&first_keyframe_emitted);

            workers.spawn("maho-win-encode", true, move |handoff| {
                let _mmcss = crate::windows_logic::MultimediaTask::register(false);
                let encode_thread = thread::current();
                let event_thread = encode_thread.clone();
                let mut encoder = match HwMftEncoder::new_with_notifier(
                    &d3d11_device,
                    adapter_luid,
                    encoder_config,
                    Arc::new(move || event_thread.unpark()),
                ) {
                    Ok(enc) => {
                        let _ = init_tx.send(Ok(enc.codec()));
                        enc
                    }
                    Err(error) => {
                        let msg = format!("hwmft init: {error}");
                        let _ = init_tx.send(Err(msg.clone()));
                        native_media_error(&handoff, &sender, msg);
                        return;
                    }
                };

                let (work_tx, work_rx) = mpsc::sync_channel(1);
                let bridge_handoff = Arc::clone(&handoff);
                let work_bridge = thread::spawn(move || {
                    while let Some(work) = bridge_handoff.next() {
                        if work_tx.send(work).is_err() {
                            break;
                        }
                        encode_thread.unpark();
                    }
                    encode_thread.unpark();
                });
                let mut pending_frame: Option<GpuFrame> = None;
                let mut last_submitted: Option<GpuFrame> = None;
                let mut encode_started_at = Instant::now();

                while !handoff.is_stopped() {
                    if let Err(error) = encoder.pump_events() {
                        native_media_error(&handoff, &sender, format!("mf pump: {error}"));
                        break;
                    }

                    match encoder.scheduler.decide(pending_frame.is_some()) {
                        SchedulerAction::ProcessOutput => {
                            match encoder.take_output() {
                                Ok(Some(encoded)) => {
                                    let is_key = encoded.is_key_frame;
                                    let trace = host_trace::enabled();
                                    let started = trace.map(|_| Instant::now());
                                    if let Some(trace) = trace {
                                        trace.record(host_trace::Record {
                                            event: 23,
                                            keyframe: is_key,
                                            size: encoded.data.len(),
                                            ..Default::default()
                                        });
                                    }

                                    let video_frame = VideoFrame {
                                        data: encoded.data,
                                        is_key_frame: encoded.is_key_frame,
                                        capture_at: encoded.capture_at,
                                        encode_started_at: encoded.encode_started_at,
                                        encode_completed_at: encoded.encode_completed_at,
                                    };

                                    let sent = sender.send(MediaEvent::Video(video_frame)).is_ok();
                                    if let (Some(trace), Some(started)) = (trace, started) {
                                        trace.record(host_trace::Record {
                                            event: 24,
                                            keyframe: is_key,
                                            value: u64::try_from(started.elapsed().as_micros())
                                                .unwrap_or(u64::MAX),
                                            repeat: !sent,
                                            ..Default::default()
                                        });
                                    }

                                    if sent && is_key {
                                        first_keyframe_emitted.store(true, Ordering::Relaxed);
                                    }
                                    if !sent {
                                        handoff.stop();
                                        break;
                                    }
                                }
                                Ok(None) => {}
                                Err(error) => {
                                    native_media_error(
                                        &handoff,
                                        &sender,
                                        format!("mf output: {error}"),
                                    );
                                    break;
                                }
                            }
                            continue;
                        }
                        SchedulerAction::SubmitInput => {
                            if let Some(frame) = pending_frame.take() {
                                last_submitted = Some(frame.clone());
                                if let Err(error) = encoder.submit_frame(&frame, encode_started_at)
                                {
                                    native_media_error(
                                        &handoff,
                                        &sender,
                                        format!("mf submit: {error}"),
                                    );
                                    break;
                                }
                            }
                            continue;
                        }
                        SchedulerAction::Drained => {
                            break;
                        }
                        SchedulerAction::Wait => {}
                    }

                    if let Ok(mut work) = work_rx.try_recv() {
                        while let Ok(newer) = work_rx.try_recv() {
                            if newer.frame.is_some() {
                                work.frame = newer.frame;
                            }
                            work.controls.force_keyframe |= newer.controls.force_keyframe;
                            if newer.controls.bitrate.is_some() {
                                work.controls.bitrate = newer.controls.bitrate;
                            }
                        }
                        handoff.reselect(&mut work);
                        if work.controls.force_keyframe
                            && encoder.force_key_frame()
                            && pending_frame.is_none()
                        {
                            pending_frame = last_submitted.clone();
                        }
                        if let Some(bitrate) = work.controls.bitrate {
                            if let Err(error) = encoder.update_bitrate(bitrate) {
                                native_media_error(
                                    &handoff,
                                    &sender,
                                    format!("mf bitrate: {error}"),
                                );
                            }
                        }
                        if let Some(frame) = work.frame {
                            encode_started_at = Instant::now();
                            pending_frame = Some(frame);
                        }
                    } else {
                        thread::park();
                    }
                }
                handoff.stop();
                drop(work_rx);
                if work_bridge.join().is_err() {
                    tracing::warn!("encoder handoff bridge panicked");
                }

                encoder.drain();
                let drain_deadline = Instant::now() + Duration::from_millis(60);
                while Instant::now() < drain_deadline && !encoder.scheduler.drain_complete {
                    let pump_res = encoder.pump_events();
                    if pump_res.is_err() {
                        break;
                    }
                    if encoder.scheduler.has_output > 0 {
                        match encoder.take_output() {
                            Ok(Some(encoded)) => {
                                let video_frame = VideoFrame {
                                    data: encoded.data,
                                    is_key_frame: encoded.is_key_frame,
                                    capture_at: encoded.capture_at,
                                    encode_started_at: encoded.encode_started_at,
                                    encode_completed_at: encoded.encode_completed_at,
                                };
                                if sender.send(MediaEvent::Video(video_frame)).is_err() {
                                    break;
                                }
                            }
                            Ok(None) => {}
                            Err(_) => break,
                        }
                    } else {
                        thread::park_timeout(
                            drain_deadline.saturating_duration_since(Instant::now()),
                        );
                    }
                }
            })?;
        }

        spawn_windows_cursor(&mut workers, sender.clone(), &self.cpu)?;
        workers.handoff.activate();

        match init_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(codec)) => {
                let _ = ready_tx.send(());
                crate::host_log(&format!(
                    "media pipeline: gpu d3d11-vp+hwmft {:?} {}",
                    codec, adapter_name
                ));
                Ok(Box::new(WindowsGpuMediaHandle { workers, codec }))
            }
            Ok(Err(error)) => {
                drop(ready_tx);
                workers.stop();
                crate::host_log(&format!("media pipeline: cpu fallback: {error}"));
                if self.target.is_some() {
                    Err(SessionError::MediaStopped)
                } else {
                    self.cpu.start_with_capabilities(sender, peer)
                }
            }
            Err(_) => {
                drop(ready_tx);
                workers.stop();
                crate::host_log("media pipeline: cpu fallback: encoder init timeout");
                if self.target.is_some() {
                    Err(SessionError::MediaStopped)
                } else {
                    self.cpu.start_with_capabilities(sender, peer)
                }
            }
        }
    }
}

struct WindowsGpuMediaHandle {
    codec: VideoCodec,
    workers: native_pipeline::Workers<GpuFrame>,
}

impl MediaHandle for WindowsGpuMediaHandle {
    fn codec(&self) -> StreamCodecPreference {
        match self.codec {
            VideoCodec::H264 => StreamCodecPreference::H264,
            VideoCodec::Hevc => StreamCodecPreference::Hevc,
        }
    }
    fn force_key_frame(&self) -> Result<(), SessionError> {
        self.workers
            .handoff
            .control(true, None)
            .then_some(())
            .ok_or(SessionError::MediaStopped)
    }

    fn update_bitrate(&self, bitrate: u32) -> Result<(), SessionError> {
        self.workers
            .handoff
            .control(false, Some(bitrate))
            .then_some(())
            .ok_or(SessionError::MediaStopped)
    }

    fn stop(&mut self) {
        self.workers.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fallback_decision_logic() {
        let (init_tx, init_rx) = mpsc::sync_channel::<Result<(), String>>(1);
        init_tx.send(Err("device lost".into())).unwrap();
        let res = init_rx.recv_timeout(Duration::from_millis(50));
        assert!(matches!(res, Ok(Err(msg)) if msg == "device lost"));

        let (tx2, rx2) = mpsc::sync_channel::<Result<(), String>>(1);
        drop(tx2);
        let res2 = rx2.recv();
        assert!(matches!(res2, Err(mpsc::RecvError)));
    }

    #[test]
    fn test_worker_activation_before_init_rx_resolves() {
        // Given: workers spawned with native_pipeline::Workers and a fake encoder factory
        let mut workers = native_pipeline::Workers::<u32>::new();
        let (init_tx, init_rx) = mpsc::sync_channel::<Result<(), String>>(1);

        // A fake encoder factory closure that reports Ok upon worker thread entry
        let fake_encoder_factory = move || -> Result<(), String> { Ok(()) };

        workers
            .spawn("test-fake-encoder", true, move |_handoff| {
                let res = fake_encoder_factory();
                let _ = init_tx.send(res);
            })
            .unwrap();

        // When: handoff is activated (matching start() fix)
        workers.handoff.activate();

        // Then: init_rx receives Ok within bounded duration without deadlocking
        let res = init_rx.recv_timeout(Duration::from_secs(1));
        assert!(matches!(res, Ok(Ok(()))));
        workers.stop();
    }
}
