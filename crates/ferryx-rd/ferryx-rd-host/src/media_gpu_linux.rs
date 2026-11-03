#[path = "capture_linux_dmabuf.rs"]
pub mod capture_linux_dmabuf;
#[path = "cuda_linux.rs"]
mod cuda_linux;
#[path = "vaapi_linux.rs"]
pub mod vaapi_linux;

use std::{
    os::fd::AsRawFd,
    sync::mpsc::{self, SyncSender},
    sync::Arc,
    time::{Duration, Instant},
};

use ffmpeg_next as ffmpeg;

use self::capture_linux_dmabuf::{DmabufCapturedFrame, LinuxDmabufCapture};
use self::vaapi_linux::{LibVa, VaapiPipeline};
use super::*;
use crate::encode_linux::{
    EncoderConfig as LinuxEncoderConfig, LinuxVideoEncoder, VideoCodec as LinuxVideoCodec,
};

pub struct GpuMediaSource {
    cpu: LinuxMediaSource,
}

impl GpuMediaSource {
    pub fn new(cpu: LinuxMediaSource) -> Self {
        Self { cpu }
    }
}

fn initialized_or_fallback<T>(
    result: Result<T, String>,
    fallback: impl FnOnce() -> Result<T, SessionError>,
) -> Result<T, SessionError> {
    match result {
        Ok(value) => Ok(value),
        Err(reason) => {
            crate::host_log(&format!("media pipeline: cpu fallback: {reason}"));
            fallback()
        }
    }
}

impl MediaSource for GpuMediaSource {
    fn start_with_capabilities(
        &self,
        sender: SyncSender<MediaEvent>,
        peer: Capabilities,
    ) -> Result<Box<dyn MediaHandle>, SessionError> {
        initialized_or_fallback(try_start_gpu(&self.cpu, sender.clone()), || {
            self.cpu.start_with_capabilities(sender, peer)
        })
    }
}

pub(super) fn gpu_source(cpu: LinuxMediaSource) -> Arc<dyn MediaSource> {
    Arc::new(GpuMediaSource::new(cpu))
}

struct GpuMediaHandle {
    codec: LinuxVideoCodec,
    workers: native_pipeline::Workers<DmabufCapturedFrame>,
}

impl MediaHandle for GpuMediaHandle {
    fn take_audio_error(&self) -> Option<String> {
        self.workers.handoff.take_audio_error()
    }
    fn codec(&self) -> StreamCodecPreference {
        match self.codec {
            LinuxVideoCodec::Hevc => StreamCodecPreference::Hevc,
            LinuxVideoCodec::H264 => StreamCodecPreference::H264,
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

struct LinuxSessionGpuEncoder {
    encoder: LinuxVideoEncoder,
    vpp: VaapiPipeline,
    frames_buf: crate::encode_linux::HwBufferRef,
    device_buf: crate::encode_linux::HwBufferRef,
    imported_slots: Vec<u32>,
    generation: Option<u64>,
    times: native_pipeline::FrameTimes,
}

impl LinuxSessionGpuEncoder {
    fn refresh_context(&mut self) -> Result<(), String> {
        let (device, frames) = self
            .encoder
            .vaapi_hwctx()
            .ok_or("encoder missing VAAPI context")?;
        // av_buffer_ref creates a distinct AVBufferRef wrapper around shared data.
        // Compare the underlying HWFramesContext, not wrapper addresses.
        if unsafe { (*frames).data != (*self.frames_buf.as_ptr()).data } {
            let vpp = VaapiPipeline::new(
                LibVa::load().map_err(|e| e.to_string())?,
                device,
                self.encoder.dimensions().0,
                self.encoder.dimensions().1,
            )
            .map_err(|e| e.to_string())?;
            // Build before replacing: old VPP retains its device through teardown.
            self.vpp = vpp;
            self.frames_buf = unsafe { crate::encode_linux::HwBufferRef::retain(frames) }
                .map_err(|e| e.to_string())?;
            self.device_buf = unsafe { crate::encode_linux::HwBufferRef::retain(device) }
                .map_err(|e| e.to_string())?;
            self.imported_slots.fill(vaapi_linux::VA_INVALID_ID);
        }
        Ok(())
    }

    fn output(
        &mut self,
        frames: Vec<crate::encode_linux::EncodedFrame>,
    ) -> Result<Vec<VideoFrame>, String> {
        let encode_completed_at = Instant::now();
        frames
            .into_iter()
            .map(|encoded| {
                let (capture_at, encode_started_at) =
                    self.times.take(encoded.pts).ok_or_else(|| {
                        format!("encoder output has unknown input PTS {}", encoded.pts)
                    })?;
                Ok(VideoFrame {
                    data: encoded.data,
                    is_key_frame: encoded.is_key_frame,
                    capture_at,
                    encode_started_at,
                    encode_completed_at,
                })
            })
            .collect()
    }
}

impl native_pipeline::Encoder<DmabufCapturedFrame> for LinuxSessionGpuEncoder {
    type Output = VideoFrame;
    type Error = String;

    fn force_keyframe(&mut self) {
        self.encoder.force_key_frame();
    }

    fn bitrate(&mut self, bitrate: u32) -> Result<Vec<VideoFrame>, String> {
        let frames = self
            .encoder
            .update_bitrate(bitrate as usize)
            .map_err(|error| format!("encoder bitrate: {error}"))?;
        let output = self.output(frames)?;
        self.refresh_context()?;
        Ok(output)
    }

    fn encode(&mut self, frame: DmabufCapturedFrame) -> Result<Vec<VideoFrame>, String> {
        let pending = self
            .encoder
            .apply_pending_bitrate()
            .map_err(|e| e.to_string())?;
        let mut prior = self.output(pending)?;
        self.refresh_context()?;
        if self.generation != Some(frame.generation) {
            self.vpp = VaapiPipeline::new(
                LibVa::load().map_err(|e| e.to_string())?,
                self.device_buf.as_ptr(),
                self.encoder.dimensions().0,
                self.encoder.dimensions().1,
            )
            .map_err(|e| e.to_string())?;
            self.imported_slots.fill(vaapi_linux::VA_INVALID_ID);
            self.generation = Some(frame.generation);
        }
        let ring_idx = frame.ring_index;
        if self.imported_slots[ring_idx] == vaapi_linux::VA_INVALID_ID {
            self.imported_slots[ring_idx] = self
                .vpp
                .import_drm_bo(
                    frame.buffer.fd.as_raw_fd(),
                    frame.buffer.stride,
                    frame.buffer.offset,
                    frame.offer.format,
                    frame.offer.width,
                    frame.offer.height,
                )
                .map_err(|e| format!("import drm bo: {e}"))?;
        }

        let mut dst_frame = ffmpeg::frame::Video::empty();
        // SAFETY: frames_buf is the valid AVBufferRef allocated for hw_frames_ctx, and dst_frame is a valid empty frame.
        let status = unsafe {
            ffmpeg::ffi::av_hwframe_get_buffer(self.frames_buf.as_ptr(), dst_frame.as_mut_ptr(), 0)
        };
        if status < 0 {
            return Err(format!(
                "av_hwframe_get_buffer failed: {}",
                ffmpeg::Error::from(status)
            ));
        }

        // SAFETY: dst_frame is a successfully allocated hardware frame; data[3] holds the VASurfaceID integer.
        let dst_surface = unsafe { (*dst_frame.as_ptr()).data[3] as usize as u32 };
        if let Err(e) = self.vpp.process_vpp(
            self.imported_slots[ring_idx],
            dst_surface,
            frame.offer.width,
            frame.offer.height,
            frame.y_inverted,
        ) {
            return Err(format!("process_vpp failed: {e}"));
        }

        // The lease releases only after VPP has synchronized the destination.
        let captured_at = frame.captured_at;
        drop(frame);

        self.times.submitted(captured_at, Instant::now());
        let encoded = self
            .encoder
            .encode_vaapi_frame(dst_frame)
            .map_err(|e| format!("encode_vaapi_frame: {e}"))?;

        prior.extend(self.output(encoded)?);
        Ok(prior)
    }
}

enum SessionGpuEncoder {
    Vaapi(LinuxSessionGpuEncoder),
    Cuda {
        encoder: LinuxVideoEncoder,
        pipeline: cuda_linux::CudaPipeline,
        times: native_pipeline::FrameTimes,
    },
}

impl SessionGpuEncoder {
    fn codec(&self) -> LinuxVideoCodec {
        match self {
            Self::Vaapi(e) => e.encoder.codec(),
            Self::Cuda { encoder, .. } => encoder.codec(),
        }
    }
}

impl native_pipeline::Encoder<DmabufCapturedFrame> for SessionGpuEncoder {
    type Output = VideoFrame;
    type Error = String;
    fn force_keyframe(&mut self) {
        match self {
            Self::Vaapi(e) => e.force_keyframe(),
            Self::Cuda { encoder, .. } => encoder.force_key_frame(),
        }
    }
    fn bitrate(&mut self, bitrate: u32) -> Result<Vec<VideoFrame>, String> {
        match self {
            Self::Vaapi(e) => e.bitrate(bitrate),
            Self::Cuda { encoder, times, .. } => {
                let packets = encoder
                    .update_bitrate(bitrate as usize)
                    .map_err(|e| e.to_string())?;
                cuda_output(times, packets)
            }
        }
    }
    fn encode(&mut self, frame: DmabufCapturedFrame) -> Result<Vec<VideoFrame>, String> {
        match self {
            Self::Vaapi(e) => e.encode(frame),
            Self::Cuda {
                encoder,
                pipeline,
                times,
            } => {
                let packets = encoder.apply_pending_bitrate().map_err(|e| e.to_string())?;
                let mut output = cuda_output(times, packets)?;
                let hardware = pipeline.import(&frame)?;
                times.submitted(frame.captured_at, Instant::now());
                let packets = encoder
                    .encode_vaapi_frame(hardware)
                    .map_err(|e| e.to_string())?;
                // Keep the capture slot leased through filter/transfer/encoder submission.
                drop(frame);
                output.extend(cuda_output(times, packets)?);
                Ok(output)
            }
        }
    }
}

fn cuda_output(
    times: &mut native_pipeline::FrameTimes,
    packets: Vec<crate::encode_linux::EncodedFrame>,
) -> Result<Vec<VideoFrame>, String> {
    packets
        .into_iter()
        .map(|encoded| {
            let (capture_at, encode_started_at) =
                times.take(encoded.pts).ok_or("unknown CUDA input PTS")?;
            Ok(VideoFrame {
                data: encoded.data,
                is_key_frame: encoded.is_key_frame,
                capture_at,
                encode_started_at,
                encode_completed_at: Instant::now(),
            })
        })
        .collect()
}

fn try_start_gpu(
    source: &LinuxMediaSource,
    sender: SyncSender<MediaEvent>,
) -> Result<Box<dyn MediaHandle>, String> {
    let effective = reprobe_output_name(source.output_name.as_deref());
    let fps = source.fps.max(1);

    let (capture_ready_tx, capture_ready_rx) = mpsc::sync_channel::<Result<(), String>>(1);
    let (encode_ready_tx, encode_ready_rx) =
        mpsc::sync_channel::<Result<LinuxVideoCodec, String>>(1);

    let mut workers = native_pipeline::Workers::<DmabufCapturedFrame>::new();

    {
        let sender = sender.clone();
        let effective_name = effective.clone();
        workers
            .spawn("maho-linux-dmabuf-capture", true, move |handoff| {
                let interval = Duration::from_micros(1_000_000 / u64::from(fps));
                let mut capture = match LinuxDmabufCapture::connect(
                    effective_name.as_deref(),
                    handoff.cancellation(),
                    Instant::now() + Duration::from_secs(5),
                    handoff.readiness_fd(),
                ) {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = capture_ready_tx.send(Err(format!("dmabuf capture init: {e}")));
                        return;
                    }
                };

                let _ = capture_ready_tx.send(Ok(()));

                while !handoff.is_stopped() {
                    let started = Instant::now();
                    let slot = loop {
                        if handoff.is_stopped() {
                            return;
                        }
                        if let Some(slot) = capture.next_free_slot() {
                            break slot;
                        }
                        handoff.pace_until(Instant::now() + Duration::from_millis(2));
                    };

                    match capture.capture_frame(
                        slot,
                        handoff.cancellation(),
                        Duration::from_millis(50),
                        handoff.readiness_fd(),
                    ) {
                        Ok(frame) => {
                            if !handoff.publish(frame) {
                                break;
                            }
                            handoff.pace_until(started + interval);
                        }
                        Err(e) => {
                            capture.release_slot(slot);
                            if !handoff.is_stopped() {
                                native_media_error(&handoff, &sender, format!("capture: {e}"));
                            }
                            return;
                        }
                    }
                }
            })
            .map_err(|e| format!("spawn capture: {e}"))?;
    }

    let config = LinuxEncoderConfig {
        width: source.width,
        height: source.height,
        bitrate: source.bitrate as usize,
        fps,
        keyframe_interval: 0,
        preferred_codec: source.codec,
    };

    {
        let sender = sender.clone();
        workers
            .spawn("maho-linux-dmabuf-encode", true, move |handoff| {
                let init_res = (|| -> Result<SessionGpuEncoder, String> {
                    if cuda_linux::is_nvidia(&cuda_linux::render_node())? {
                        let mut pipeline = cuda_linux::CudaPipeline::new(config)?;
                        let mut encoder = LinuxVideoEncoder::new_cuda(config, pipeline.frames())
                            .map_err(|e| e.to_string())?;
                        // Validate actual descriptor import, shaders and CUDA transfer before
                        // advertising ready. Errors still reach GpuMediaSource's CPU fallback.
                        let first = handoff
                            .next()
                            .and_then(|work| work.frame)
                            .ok_or("capture stopped before CUDA import probe")?;
                        let hardware = pipeline.import(&first)?;
                        let mut times = native_pipeline::FrameTimes::new();
                        times.submitted(first.captured_at, Instant::now());
                        let packets = encoder
                            .encode_vaapi_frame(hardware)
                            .map_err(|e| e.to_string())?;
                        drop(first);
                        for output in cuda_output(&mut times, packets)? {
                            sender
                                .send(MediaEvent::Video(output))
                                .map_err(|e| e.to_string())?;
                        }
                        return Ok(SessionGpuEncoder::Cuda {
                            encoder,
                            pipeline,
                            times,
                        });
                    }
                    let libva = LibVa::load().map_err(|e| format!("libva load: {e}"))?;
                    let encoder = LinuxVideoEncoder::new_vaapi(config)
                        .map_err(|e| format!("encoder init: {e}"))?;
                    let (device_buf, frames_buf) = encoder
                        .vaapi_hwctx()
                        .ok_or_else(|| "encoder missing vaapi hwctx".to_string())?;

                    let vpp = VaapiPipeline::new(libva, device_buf, config.width, config.height)
                        .map_err(|e| format!("vpp init: {e}"))?;

                    Ok(SessionGpuEncoder::Vaapi(LinuxSessionGpuEncoder {
                        encoder,
                        vpp,
                        frames_buf: unsafe { crate::encode_linux::HwBufferRef::retain(frames_buf) }
                            .map_err(|e| e.to_string())?,
                        device_buf: unsafe { crate::encode_linux::HwBufferRef::retain(device_buf) }
                            .map_err(|e| e.to_string())?,
                        imported_slots: vec![
                            vaapi_linux::VA_INVALID_ID;
                            capture_linux_dmabuf::RING_CAPACITY
                        ],
                        generation: None,
                        times: native_pipeline::FrameTimes::new(),
                    }))
                })();

                let session_encoder = match init_res {
                    Ok(enc) => {
                        let _ = encode_ready_tx.send(Ok(enc.codec()));
                        enc
                    }
                    Err(e) => {
                        let _ = encode_ready_tx.send(Err(e));
                        return;
                    }
                };

                let result = native_pipeline::run_encoder(
                    &handoff,
                    || Ok(session_encoder),
                    |frame| sender.send(MediaEvent::Video(frame)).is_ok(),
                );
                if let Err(error) = result {
                    native_media_error(&handoff, &sender, error);
                }
            })
            .map_err(|e| format!("spawn encode: {e}"))?;
    }

    if source.capture_audio {
        let sender = sender.clone();
        workers
            .spawn("maho-linux-dmabuf-audio", false, move |handoff| {
                run_linux_audio(handoff, sender);
            })
            .map_err(|e| format!("spawn audio: {e}"))?;
    }

    workers.handoff.activate();

    match capture_ready_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            workers.stop();
            return Err(e);
        }
        Err(_) => {
            workers.stop();
            return Err("capture init timeout".into());
        }
    }

    let codec = match encode_ready_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(codec)) => codec,
        Ok(Err(e)) => {
            workers.stop();
            return Err(e);
        }
        Err(_) => {
            workers.stop();
            return Err("encode init timeout".into());
        }
    };

    Ok(Box::new(GpuMediaHandle { workers, codec }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_init_failure_falls_back_once_and_success_never_falls_back() {
        let called = std::cell::Cell::new(0);
        assert_eq!(
            initialized_or_fallback(Err("CUDA mapping unavailable".into()), || {
                called.set(called.get() + 1);
                Ok(42)
            })
            .unwrap(),
            42
        );
        assert_eq!(called.get(), 1);
        assert_eq!(
            initialized_or_fallback(Ok(7), || {
                called.set(called.get() + 1);
                Ok(0)
            })
            .unwrap(),
            7
        );
        assert_eq!(called.get(), 1);
        assert!(matches!(
            initialized_or_fallback::<()>(Err("init failed".into()), || Err(
                SessionError::MediaStopped
            )),
            Err(SessionError::MediaStopped)
        ));
    }

    #[test]
    fn workers_activation_and_init_handshake_contract() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let mut workers = native_pipeline::Workers::<DmabufCapturedFrame>::new();
        let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<(), String>>(1);
        let exited = Arc::new(AtomicBool::new(false));

        let exited_clone = Arc::clone(&exited);
        workers
            .spawn("test-init", true, move |_handoff| {
                let _ = ready_tx.send(Ok(()));
                exited_clone.store(true, Ordering::Release);
            })
            .unwrap();

        assert!(matches!(
            ready_rx.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));

        workers.handoff.activate();
        assert!(ready_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .is_ok());

        workers.stop();
        assert!(exited.load(Ordering::Acquire), "worker must exit upon stop");
    }

    #[test]
    fn workers_failure_stops_and_joins_without_leaks() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let mut workers = native_pipeline::Workers::<DmabufCapturedFrame>::new();
        let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<(), String>>(1);
        let exited = Arc::new(AtomicBool::new(false));

        let exited_clone = Arc::clone(&exited);
        workers
            .spawn("test-fail", true, move |_handoff| {
                let _ = ready_tx.send(Err("device failure".into()));
                exited_clone.store(true, Ordering::Release);
            })
            .unwrap();

        workers.handoff.activate();
        let err = ready_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap_err();
        assert_eq!(err, "device failure");

        workers.stop();
        assert!(
            exited.load(Ordering::Acquire),
            "worker must exit upon failure stop"
        );
    }
}
