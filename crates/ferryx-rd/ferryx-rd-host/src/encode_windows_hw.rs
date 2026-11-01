use std::collections::BTreeMap;
use std::mem::ManuallyDrop;
use std::ptr;
use std::slice;
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::Instant;

use windows::core::Interface;
use windows::Win32::Foundation::{LUID, S_OK};
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11Texture2D};
use windows::Win32::Media::MediaFoundation::{
    eAVScenarioInfo_DisplayRemoting, CODECAPI_AVEncVideoForceKeyFrame,
    CODECAPI_AVEncVideoGradualIntraRefresh, CODECAPI_AVScenarioInfo, ICodecAPI, IMFActivate,
    IMFAttributes, IMFDXGIDeviceManager, IMFMediaEventGenerator, IMFShutdown, IMFTransform,
    METransformDrainComplete, METransformHaveOutput, METransformNeedInput, MFCreateAttributes,
    MFCreateDXGIDeviceManager, MFCreateDXGISurfaceBuffer, MFCreateMediaType, MFCreateMemoryBuffer,
    MFCreateSample, MFMediaType_Video, MFSampleExtension_CleanPoint, MFTEnum2, MFTEnumEx,
    MFVideoFormat_NV12, MFVideoInterlace_Progressive, MFT_CATEGORY_VIDEO_ENCODER,
    MFT_ENUM_ADAPTER_LUID, MFT_ENUM_FLAG, MFT_ENUM_FLAG_HARDWARE, MFT_ENUM_FLAG_SORTANDFILTER,
    MFT_MESSAGE_COMMAND_DRAIN, MFT_MESSAGE_COMMAND_FLUSH, MFT_MESSAGE_NOTIFY_BEGIN_STREAMING,
    MFT_MESSAGE_NOTIFY_END_OF_STREAM, MFT_MESSAGE_NOTIFY_START_OF_STREAM,
    MFT_MESSAGE_SET_D3D_MANAGER, MFT_OUTPUT_DATA_BUFFER, MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES,
    MFT_OUTPUT_STREAM_PROVIDES_SAMPLES, MFT_REGISTER_TYPE_INFO, MF_E_TRANSFORM_STREAM_CHANGE,
    MF_LOW_LATENCY, MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE,
    MF_MT_VIDEO_NOMINAL_RANGE, MF_MT_YUV_MATRIX, MF_TRANSFORM_ASYNC_UNLOCK,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Variant::VARIANT;

use super::capture_windows_gpu::GpuFrame;
use crate::encode_windows::{
    apply_bitrate_properties, apply_codec_properties, media_type_parameter_sets, nalu_type,
    negotiate_output_type, parameter_sets, parse_access_unit, sample_bytes, write_avcc,
    EncodeError, EncoderConfig, MediaFoundationRuntime, VideoCodec,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerAction {
    SubmitInput,
    ProcessOutput,
    Wait,
    Drained,
}

#[derive(Default)]
pub struct MftScheduler {
    pub need_input: u32,
    pub has_output: u32,
    pub draining: bool,
    pub drain_complete: bool,
    pub frame_duration_hns: i64,
    pub frame_counter: u64,
    pub in_flight: BTreeMap<i64, (Instant, Instant)>,
}

impl MftScheduler {
    pub fn new(frame_duration_hns: i64) -> Self {
        Self {
            need_input: 0,
            has_output: 0,
            draining: false,
            drain_complete: false,
            frame_duration_hns,
            frame_counter: 0,
            in_flight: BTreeMap::new(),
        }
    }

    pub fn on_need_input(&mut self) {
        self.need_input = self.need_input.saturating_add(1);
    }

    pub fn on_have_output(&mut self) {
        self.has_output = self.has_output.saturating_add(1);
    }

    pub fn on_drain_complete(&mut self) {
        self.drain_complete = true;
    }

    pub fn next_sample_time(&mut self) -> i64 {
        let time = self.frame_counter as i64 * self.frame_duration_hns;
        self.frame_counter += 1;
        time
    }

    pub fn register_input(
        &mut self,
        timestamp: i64,
        capture_at: Instant,
        encode_started_at: Instant,
    ) {
        self.need_input = self.need_input.saturating_sub(1);
        self.in_flight
            .insert(timestamp, (capture_at, encode_started_at));
    }

    pub fn take_output_timing(&mut self, timestamp: i64) -> Option<(Instant, Instant)> {
        self.in_flight.remove(&timestamp).or_else(|| {
            let oldest_key = self.in_flight.keys().next().copied()?;
            self.in_flight.remove(&oldest_key)
        })
    }

    pub fn decide(&self, has_pending_frame: bool) -> SchedulerAction {
        if self.has_output > 0 {
            SchedulerAction::ProcessOutput
        } else if self.need_input > 0 && has_pending_frame && !self.draining {
            SchedulerAction::SubmitInput
        } else if self.draining && self.drain_complete {
            SchedulerAction::Drained
        } else {
            SchedulerAction::Wait
        }
    }
}

// SAFETY: asynchronous hardware MFT event generators are free-threaded. The
// event worker is the sole GetEvent consumer; shutdown remains on the owner.
struct MftEventSource(IMFMediaEventGenerator);
unsafe impl Send for MftEventSource {}
impl MftEventSource {
    fn next(&self) -> windows::core::Result<u32> {
        // SAFETY: this wrapper is used by exactly one event consumer.
        unsafe {
            let event = self.0.GetEvent(
                windows::Win32::Media::MediaFoundation::MEDIA_EVENT_GENERATOR_GET_EVENT_FLAGS(0),
            )?;
            event.GetStatus()?.ok()?;
            event.GetType()
        }
    }
}

/// Failed codec negotiation must shut down the activation before trying fallback.
struct NegotiatingTransform(Option<(IMFTransform, IMFActivate)>);
impl Drop for NegotiatingTransform {
    fn drop(&mut self) {
        if let Some((transform, activate)) = &self.0 {
            // SAFETY: no worker has taken ownership during negotiation.
            unsafe {
                if let Ok(shutdown) = transform.cast::<IMFShutdown>() {
                    let _ = shutdown.Shutdown();
                }
                let _ = activate.ShutdownObject();
            }
        }
    }
}

pub struct HwMftEncoder {
    transform: IMFTransform,
    activate: IMFActivate,
    _dxgi_manager: IMFDXGIDeviceManager,
    events: mpsc::Receiver<windows::core::Result<u32>>,
    event_thread: Option<JoinHandle<()>>,
    in_flight_frames: BTreeMap<i64, GpuFrame>,
    codec_api: Option<ICodecAPI>,
    config: EncoderConfig,
    parameter_sets: Vec<Vec<u8>>,
    pub scheduler: MftScheduler,
    force_keyframe: bool,
    _runtime: MediaFoundationRuntime,
}

impl HwMftEncoder {
    pub fn new_with_notifier(
        d3d11_device: &ID3D11Device,
        adapter_luid: LUID,
        config: EncoderConfig,
        notify: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self, EncodeError> {
        let config = config.validate()?;
        let mut candidates = vec![config.preferred_codec];
        if config.preferred_codec != VideoCodec::H264 {
            candidates.push(VideoCodec::H264);
        }
        let mut last_error = EncodeError::TransformUnavailable(config.preferred_codec);
        for codec in candidates {
            match Self::new_codec(
                d3d11_device,
                adapter_luid,
                EncoderConfig {
                    preferred_codec: codec,
                    ..config
                },
                Arc::clone(&notify),
            ) {
                Ok(encoder) => return Ok(encoder),
                Err(error) => {
                    tracing::warn!(?codec, %error, "hardware codec negotiation failed");
                    last_error = error;
                }
            }
        }
        Err(last_error)
    }

    fn new_codec(
        d3d11_device: &ID3D11Device,
        adapter_luid: LUID,
        config: EncoderConfig,
        notify: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self, EncodeError> {
        let runtime = MediaFoundationRuntime::start()?;
        let duration_hns = 10_000_000_i64 / i64::from(config.fps.max(1));

        let mut negotiation = NegotiatingTransform(Some(enumerate_hw_mft(
            adapter_luid,
            config.preferred_codec,
        )?));
        let (transform, _) = negotiation.0.as_ref().unwrap();

        // SAFETY: GetAttributes returns the MFT attribute store.
        let attributes: IMFAttributes = unsafe { transform.GetAttributes()? };
        // SAFETY: MF_TRANSFORM_ASYNC_UNLOCK enables asynchronous MFT processing.
        unsafe {
            attributes.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1)?;
        }

        let mut reset_token = 0;
        let mut dxgi_manager: Option<IMFDXGIDeviceManager> = None;
        // SAFETY: MFCreateDXGIDeviceManager creates DXGI device manager for MFT hardware acceleration.
        unsafe {
            MFCreateDXGIDeviceManager(&mut reset_token, &mut dxgi_manager)?;
        }
        let dxgi_manager = dxgi_manager.ok_or_else(|| {
            EncodeError::MediaFoundation(windows::core::Error::from_hresult(
                windows::Win32::Foundation::E_FAIL,
            ))
        })?;
        // SAFETY: ResetDevice binds the capture D3D11 device to the DXGI device manager.
        unsafe {
            dxgi_manager.ResetDevice(d3d11_device, reset_token)?;
        }
        let manager_unknown: windows::core::IUnknown = dxgi_manager.cast()?;
        // SAFETY: transmute_copy borrows the raw IUnknown pointer as usize parameter for ProcessMessage.
        let param = unsafe {
            core::mem::transmute_copy::<windows::core::IUnknown, usize>(&manager_unknown)
        };
        // SAFETY: ProcessMessage passes D3D device manager interface to the hardware MFT.
        unsafe {
            transform.ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, param)?;
        }

        let codec_api: Option<ICodecAPI> = transform.cast().ok();
        // Recovery is always ForceKeyFrame (IDR); see docs/encoder-recovery.md.
        // CODECAPI_AVEncVideoGradualIntraRefresh is declared only as UINT32 and
        // its value meaning is undocumented, so it is observed read-only and
        // logged raw: never written, decoded, or used to choose recovery.
        if let Some(api) = &codec_api {
            log_gradual_intra_refresh_probe(api);
        } else {
            tracing::info!("recovery: on-demand IDR; encoder exposes no ICodecAPI");
        }
        if let Some(api) = &codec_api {
            apply_codec_properties(api, config);
            apply_bitrate_properties(api, config.bitrate)?;
            let scenario_val = VARIANT::from(eAVScenarioInfo_DisplayRemoting.0 as u32);
            // SAFETY: SetValue on ICodecAPI sets low-latency display remoting profile.
            unsafe {
                let _ = api.SetValue(&CODECAPI_AVScenarioInfo, &scenario_val);
            }
        }

        // SAFETY: MF_LOW_LATENCY enables zero-buffering low latency encoding mode.
        unsafe {
            let _ = attributes.SetUINT32(&MF_LOW_LATENCY, 1);
        }

        let output_type = negotiate_output_type(
            transform,
            codec_api.as_ref(),
            config.preferred_codec,
            config,
        )?;

        // SAFETY: MFCreateMediaType creates an unconfigured media type descriptor.
        let input_type = unsafe { MFCreateMediaType()? };
        // SAFETY: SetGUID / SetUINT64 / SetUINT32 configure input media type properties for NV12 video.
        unsafe {
            input_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            input_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
            input_type.SetUINT64(
                &windows::Win32::Media::MediaFoundation::MF_MT_FRAME_SIZE,
                (u64::from(config.width) << 32) | u64::from(config.height),
            )?;
            input_type.SetUINT64(
                &windows::Win32::Media::MediaFoundation::MF_MT_FRAME_RATE,
                (u64::from(config.fps) << 32) | 1,
            )?;
            input_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            input_type.SetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE, 1)?;
            input_type.SetUINT32(&MF_MT_YUV_MATRIX, 2)?;
            transform.SetInputType(0, &input_type, 0)?;
        }

        let parameter_sets = media_type_parameter_sets(&output_type, config.preferred_codec);

        // SAFETY: ProcessMessage notifies encoder streaming lifecycle.
        unsafe {
            transform.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
            transform.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;
        }

        let event_generator = MftEventSource(transform.cast()?);
        let (event_tx, events) = mpsc::channel();
        // Blocking GetEvent runs off the encode worker. Shutdown wakes it before join.
        let event_thread = std::thread::spawn(move || {
            let _com = match MediaFoundationRuntime::start() {
                Ok(runtime) => runtime,
                Err(error) => {
                    if let EncodeError::MediaFoundation(error) = error {
                        let _ = event_tx.send(Err(error));
                    }
                    notify();
                    return;
                }
            };
            loop {
                // SAFETY: the asynchronous MFT supports a single blocking event consumer.
                let event = event_generator.next();
                let failed = event.is_err();
                if event_tx.send(event).is_err() {
                    break;
                }
                notify();
                if failed {
                    break;
                }
            }
        });

        let (transform, activate) = negotiation.0.take().unwrap();
        Ok(Self {
            transform,
            activate,
            _dxgi_manager: dxgi_manager,
            events,
            event_thread: Some(event_thread),
            in_flight_frames: BTreeMap::new(),
            codec_api,
            config,
            parameter_sets,
            scheduler: MftScheduler::new(duration_hns),
            force_keyframe: true,
            _runtime: runtime,
        })
    }

    pub fn codec(&self) -> VideoCodec {
        self.config.preferred_codec
    }

    pub fn submit_frame(
        &mut self,
        frame: &GpuFrame,
        encode_started_at: Instant,
    ) -> Result<(), EncodeError> {
        tracing::trace!(
            capture_id = frame.capture_id,
            "submitting GPU capture surface"
        );
        let sample_time = self.scheduler.next_sample_time();
        let duration = self.scheduler.frame_duration_hns;

        // SAFETY: MFCreateDXGISurfaceBuffer creates a media buffer wrapping an ID3D11Texture2D.
        let media_buffer = unsafe {
            MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, frame.texture.as_ref(), 0, false)?
        };

        // SAFETY: MFCreateSample allocates an IMFMediaSample.
        let sample = unsafe { MFCreateSample()? };

        // SAFETY: Adding surface buffer and setting sample timestamp/duration.
        unsafe {
            sample.AddBuffer(&media_buffer)?;
            sample.SetSampleTime(sample_time)?;
            sample.SetSampleDuration(duration)?;
        }

        let is_key = self.force_keyframe;
        if is_key {
            if let Some(api) = &self.codec_api {
                let value = VARIANT::from(1_u32);
                // SAFETY: SetValue on ICodecAPI forces keyframe on next submitted frame.
                unsafe {
                    api.SetValue(&CODECAPI_AVEncVideoForceKeyFrame, &value)?;
                }
            }
        }

        // SAFETY: ProcessInput submits the DXGI surface sample into MFT stream 0.
        unsafe {
            self.transform.ProcessInput(0, &sample, 0)?;
        }

        if is_key {
            self.force_keyframe = false;
        }
        self.in_flight_frames.insert(sample_time, frame.clone());
        self.scheduler
            .register_input(sample_time, frame.captured_at, encode_started_at);
        Ok(())
    }

    pub fn take_output(
        &mut self,
    ) -> Result<Option<crate::encode_windows::EncodedFrame>, EncodeError> {
        // SAFETY: GetOutputStreamInfo queries whether the MFT provides output samples.
        let stream_info = unsafe { self.transform.GetOutputStreamInfo(0)? };
        let provides_sample = stream_info.dwFlags
            & (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32
                | MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES.0 as u32)
            != 0;

        let sample = if provides_sample {
            None
        } else {
            // SAFETY: Allocate sample buffer if MFT does not provide its own.
            let sample = unsafe { MFCreateSample()? };
            let buffer = unsafe { MFCreateMemoryBuffer(stream_info.cbSize.max(1))? };
            unsafe { sample.AddBuffer(&buffer)? };
            Some(sample)
        };

        let mut output_buffer = [MFT_OUTPUT_DATA_BUFFER {
            dwStreamID: 0,
            pSample: ManuallyDrop::new(sample),
            dwStatus: 0,
            pEvents: ManuallyDrop::new(None),
        }];

        let mut status = 0;
        // SAFETY: ProcessOutput retrieves encoded sample from the hardware MFT.
        let res = unsafe {
            self.transform
                .ProcessOutput(0, &mut output_buffer, &mut status)
        };
        self.scheduler.has_output = self.scheduler.has_output.saturating_sub(1);

        // SAFETY: ManuallyDrop::take extracts owned COM pointers from the output struct.
        let sample = unsafe { ManuallyDrop::take(&mut output_buffer[0].pSample) };
        let _events = unsafe { ManuallyDrop::take(&mut output_buffer[0].pEvents) };

        match res {
            Ok(()) => {}
            Err(error) => {
                if error.code() == MF_E_TRANSFORM_STREAM_CHANGE {
                    // SAFETY: Re-query output type on dynamic stream format change.
                    let available_type = unsafe { self.transform.GetOutputAvailableType(0, 0)? };
                    unsafe {
                        self.transform.SetOutputType(0, &available_type, 0)?;
                    }
                    return Ok(None);
                }
                return Err(error.into());
            }
        }

        let Some(sample) = sample else {
            return Ok(None);
        };

        // SAFETY: GetUINT32 checks clean point keyframe flag.
        let sample_clean_point =
            unsafe { sample.GetUINT32(&MFSampleExtension_CleanPoint).unwrap_or(0) } == 1;

        // SAFETY: GetSampleTime retrieves presentation timestamp.
        let timestamp_hns = unsafe { sample.GetSampleTime()? };
        // B-frames are disabled, so output releases the corresponding input surface.
        self.in_flight_frames.remove(&timestamp_hns);
        let timing = self.scheduler.take_output_timing(timestamp_hns);
        let (capture_at, encode_started_at) = timing.unwrap_or((Instant::now(), Instant::now()));

        let bytes = sample_bytes(&sample)?;
        let mut nalus = parse_access_unit(&bytes)?;
        let is_key_frame = sample_clean_point
            || nalus.iter().any(|nalu| {
                matches!(
                    (
                        self.config.preferred_codec,
                        nalu_type(nalu, self.config.preferred_codec)
                    ),
                    (VideoCodec::H264, Some(5)) | (VideoCodec::Hevc, Some(16..=21))
                )
            });

        if is_key_frame {
            let fresh = parameter_sets(&nalus, self.config.preferred_codec);
            if !fresh.is_empty() {
                self.parameter_sets = fresh;
            }
            if !self.parameter_sets.is_empty() {
                let present_types: Vec<u8> = nalus
                    .iter()
                    .filter_map(|nalu| nalu_type(nalu, self.config.preferred_codec))
                    .collect();
                let mut prefixed = Vec::new();
                for parameter_set in &self.parameter_sets {
                    let Some(kind) = nalu_type(parameter_set, self.config.preferred_codec) else {
                        continue;
                    };
                    if !present_types.contains(&kind) {
                        prefixed.push(parameter_set.as_slice());
                    }
                }
                prefixed.append(&mut nalus);
                nalus = prefixed;
            }
        }

        let data = write_avcc(&nalus)?;

        Ok(Some(crate::encode_windows::EncodedFrame {
            data,
            is_key_frame,
            timestamp_hns,
            codec: self.config.preferred_codec,
            capture_at,
            encode_started_at,
            encode_completed_at: Instant::now(),
        }))
    }

    pub fn force_key_frame(&mut self) -> bool {
        // Startup/restarts bypass the session's reactive IDR budget.
        self.force_keyframe = true;
        true
    }

    pub fn update_bitrate(&mut self, bitrate: u32) -> Result<(), EncodeError> {
        if bitrate == 0 {
            return Err(EncodeError::InvalidConfiguration);
        }
        if bitrate == self.config.bitrate {
            return Ok(());
        }
        let api = self
            .codec_api
            .as_ref()
            .ok_or(EncodeError::BitrateControlUnavailable)?;
        apply_bitrate_properties(api, bitrate)?;
        self.config.bitrate = bitrate;
        Ok(())
    }

    pub fn pump_events(&mut self) -> Result<bool, EncodeError> {
        let mut any_event = false;
        for event in self.events.try_iter() {
            any_event = true;
            let event_type = event?;
            if event_type == METransformNeedInput.0 as u32 {
                self.scheduler.on_need_input();
            } else if event_type == METransformHaveOutput.0 as u32 {
                self.scheduler.on_have_output();
            } else if event_type == METransformDrainComplete.0 as u32 {
                self.scheduler.on_drain_complete();
            }
        }
        Ok(any_event)
    }

    pub fn drain(&mut self) {
        self.scheduler.draining = true;
        // SAFETY: ProcessMessage notifies end of stream and commands drain.
        unsafe {
            let _ = self
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0);
            let _ = self.transform.ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0);
        }
    }
}

impl Drop for HwMftEncoder {
    fn drop(&mut self) {
        // SAFETY: Flushes transform, cleanly shuts down MFT via IMFShutdown, detaches
        // DXGI device manager, and shuts down the activation object to release driver worker threads.
        unsafe {
            let _ = self.transform.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0);
            if let Ok(shutdown) = self.transform.cast::<IMFShutdown>() {
                let _ = shutdown.Shutdown();
            }
            let _ = self
                .transform
                .ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, 0);
            let _ = self.activate.ShutdownObject();
        }
        if let Some(thread) = self.event_thread.take() {
            if thread.join().is_err() {
                tracing::warn!("MFT event thread panicked");
            }
        }
    }
}

/// Logs what the encoder reports for `CODECAPI_AVEncVideoGradualIntraRefresh`
/// using only query calls. Values are raw UINT32s: their meaning is not
/// documented by Microsoft, so nothing here interprets them.
fn log_gradual_intra_refresh_probe(api: &ICodecAPI) {
    let key = &CODECAPI_AVEncVideoGradualIntraRefresh;
    // SAFETY: query-only ICodecAPI calls on the owning thread.
    let supported = unsafe { api.IsSupported(key) }.is_ok();
    let modifiable = supported && unsafe { api.IsModifiable(key) } == S_OK;
    let raw_values = if supported {
        gradual_intra_refresh_raw_values(api)
    } else {
        None
    };
    tracing::info!(
        supported,
        modifiable,
        raw_values = ?raw_values,
        "recovery: on-demand IDR; CODECAPI_AVEncVideoGradualIntraRefresh probed read-only, not enabled"
    );
}

fn gradual_intra_refresh_raw_values(api: &ICodecAPI) -> Option<Vec<u32>> {
    let mut values: *mut VARIANT = ptr::null_mut();
    let mut count = 0_u32;
    // SAFETY: on success the caller owns `count` CoTaskMem-allocated VARIANTs.
    unsafe {
        api.GetParameterValues(
            &CODECAPI_AVEncVideoGradualIntraRefresh,
            &mut values,
            &mut count,
        )
    }
    .ok()?;
    if values.is_null() {
        return None;
    }
    // SAFETY: `values` points to `count` initialized VARIANTs; each is cleared
    // via drop_in_place (VariantClear) before the array itself is freed.
    unsafe {
        let entries = slice::from_raw_parts_mut(values, count as usize);
        let raw = entries
            .iter()
            .filter_map(|value| u32::try_from(value).ok())
            .collect();
        for entry in entries.iter_mut() {
            ptr::drop_in_place(entry);
        }
        CoTaskMemFree(Some(values.cast()));
        Some(raw)
    }
}

fn enumerate_hw_mft(
    adapter_luid: LUID,
    codec: VideoCodec,
) -> Result<(IMFTransform, IMFActivate), EncodeError> {
    let input_type = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_NV12,
    };
    let output_type = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: codec.media_subtype(),
    };

    let mut attrs = None;
    // SAFETY: MFCreateAttributes allocates attribute store for MFTEnum2 filtering.
    unsafe {
        MFCreateAttributes(&mut attrs, 1)?;
    }
    let attrs = attrs.unwrap();
    // SAFETY: Converting LUID reference to byte slice.
    let luid_slice = unsafe {
        slice::from_raw_parts(
            &adapter_luid as *const LUID as *const u8,
            core::mem::size_of::<LUID>(),
        )
    };
    // SAFETY: Setting adapter LUID blob filter on attribute store.
    unsafe {
        attrs.SetBlob(&MFT_ENUM_ADAPTER_LUID, luid_slice)?;
    }

    let flags = MFT_ENUM_FLAG(MFT_ENUM_FLAG_HARDWARE.0 | MFT_ENUM_FLAG_SORTANDFILTER.0);
    let mut activations: *mut Option<IMFActivate> = ptr::null_mut();
    let mut count = 0;

    // SAFETY: MFTEnum2 enumerates matching hardware encoder transforms.
    let res = unsafe {
        MFTEnum2(
            MFT_CATEGORY_VIDEO_ENCODER,
            flags,
            Some(&input_type),
            Some(&output_type),
            &attrs,
            &mut activations,
            &mut count,
        )
    };

    let mut selected = None;
    if res.is_ok() && count > 0 && !activations.is_null() {
        // SAFETY: Creating mutable slice over CoTaskMem-allocated IMFActivate array.
        let entries = unsafe { slice::from_raw_parts_mut(activations, count as usize) };
        for entry in entries.iter_mut() {
            if let Some(activate) = entry.take() {
                if selected.is_none() {
                    // SAFETY: ActivateObject instantiates the hardware MFT transform.
                    if let Ok(transform) = unsafe { activate.ActivateObject::<IMFTransform>() } {
                        selected = Some((transform, activate));
                    }
                }
            }
        }
        // SAFETY: Freeing CoTaskMem-allocated activation array.
        unsafe {
            CoTaskMemFree(Some(activations.cast()));
        }
    }

    if let Some((transform, activate)) = selected {
        return Ok((transform, activate));
    }

    let mut activations_ex: *mut Option<IMFActivate> = ptr::null_mut();
    let mut count_ex = 0;
    // SAFETY: Fallback MFTEnumEx without adapter LUID filter if MFTEnum2 returned none.
    unsafe {
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_ENCODER,
            flags,
            Some(&input_type),
            Some(&output_type),
            &mut activations_ex,
            &mut count_ex,
        )?;
    }
    if count_ex > 0 && !activations_ex.is_null() {
        // SAFETY: Creating mutable slice over CoTaskMem-allocated IMFActivate array.
        let entries = unsafe { slice::from_raw_parts_mut(activations_ex, count_ex as usize) };
        for entry in entries.iter_mut() {
            if let Some(activate) = entry.take() {
                if selected.is_none() {
                    // SAFETY: ActivateObject instantiates the hardware MFT transform.
                    if let Ok(transform) = unsafe { activate.ActivateObject::<IMFTransform>() } {
                        selected = Some((transform, activate));
                    }
                }
            }
        }
        // SAFETY: Freeing CoTaskMem-allocated activation array.
        unsafe {
            CoTaskMemFree(Some(activations_ex.cast()));
        }
    }

    selected.ok_or(EncodeError::TransformUnavailable(codec))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn scheduler_initial_state_and_wait() {
        let scheduler = MftScheduler::new(166_666);
        assert_eq!(scheduler.need_input, 0);
        assert_eq!(scheduler.has_output, 0);
        assert_eq!(scheduler.decide(false), SchedulerAction::Wait);
        assert_eq!(scheduler.decide(true), SchedulerAction::Wait);
    }

    #[test]
    fn scheduler_need_input_triggers_submission() {
        let mut scheduler = MftScheduler::new(166_666);
        scheduler.on_need_input();
        assert_eq!(scheduler.need_input, 1);
        assert_eq!(scheduler.decide(false), SchedulerAction::Wait);
        assert_eq!(scheduler.decide(true), SchedulerAction::SubmitInput);
    }

    #[test]
    fn scheduler_output_preempts_input() {
        let mut scheduler = MftScheduler::new(166_666);
        scheduler.on_need_input();
        scheduler.on_have_output();
        assert_eq!(scheduler.decide(true), SchedulerAction::ProcessOutput);
        assert_eq!(scheduler.decide(false), SchedulerAction::ProcessOutput);
    }

    #[test]
    fn scheduler_timestamp_registration_and_fallback() {
        let mut scheduler = MftScheduler::new(166_666);
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_millis(1);
        let s0 = scheduler.next_sample_time();
        assert_eq!(s0, 0);
        let s1 = scheduler.next_sample_time();
        assert_eq!(s1, 166_666);

        scheduler.on_need_input();
        assert_eq!(scheduler.need_input, 1);
        scheduler.register_input(s0, t0, t1);
        assert_eq!(scheduler.need_input, 0);
        assert_eq!(scheduler.in_flight.len(), 1);

        scheduler.on_have_output();
        assert_eq!(scheduler.has_output, 1);
        let timing = scheduler.take_output_timing(s0);
        assert_eq!(timing, Some((t0, t1)));
        assert_eq!(scheduler.in_flight.len(), 0);

        scheduler.register_input(s1, t0, t1);
        let fallback_timing = scheduler.take_output_timing(999_999);
        assert_eq!(fallback_timing, Some((t0, t1)));
        assert_eq!(scheduler.in_flight.len(), 0);
    }

    #[test]
    fn scheduler_drain_lifecycle() {
        let mut scheduler = MftScheduler::new(166_666);
        scheduler.draining = true;
        assert_eq!(scheduler.decide(true), SchedulerAction::Wait);
        scheduler.on_drain_complete();
        assert_eq!(scheduler.decide(true), SchedulerAction::Drained);
    }

    #[test]
    fn scheduler_counts_multiple_need_input_events() {
        let mut scheduler = MftScheduler::new(166_666);
        let now = Instant::now();
        scheduler.on_need_input();
        scheduler.on_need_input();
        assert_eq!(scheduler.need_input, 2);

        assert_eq!(scheduler.decide(true), SchedulerAction::SubmitInput);
        let s0 = scheduler.next_sample_time();
        scheduler.register_input(s0, now, now);
        assert_eq!(scheduler.need_input, 1);

        assert_eq!(scheduler.decide(true), SchedulerAction::SubmitInput);
        let s1 = scheduler.next_sample_time();
        scheduler.register_input(s1, now, now);
        assert_eq!(scheduler.need_input, 0);

        assert_eq!(scheduler.decide(true), SchedulerAction::Wait);
    }

    #[test]
    fn scheduler_counts_multiple_have_output_events() {
        let mut scheduler = MftScheduler::new(166_666);
        let now = Instant::now();
        let s0 = scheduler.next_sample_time();
        let s1 = scheduler.next_sample_time();
        scheduler.register_input(s0, now, now);
        scheduler.register_input(s1, now, now);

        scheduler.on_have_output();
        scheduler.on_have_output();
        assert_eq!(scheduler.has_output, 2);

        assert_eq!(scheduler.decide(true), SchedulerAction::ProcessOutput);
        scheduler.has_output = scheduler.has_output.saturating_sub(1);
        assert_eq!(scheduler.take_output_timing(s0), Some((now, now)));
        assert_eq!(scheduler.has_output, 1);

        assert_eq!(scheduler.decide(true), SchedulerAction::ProcessOutput);
        scheduler.has_output = scheduler.has_output.saturating_sub(1);
        assert_eq!(scheduler.take_output_timing(s1), Some((now, now)));
        assert_eq!(scheduler.has_output, 0);

        assert_eq!(scheduler.decide(true), SchedulerAction::Wait);
    }
}
