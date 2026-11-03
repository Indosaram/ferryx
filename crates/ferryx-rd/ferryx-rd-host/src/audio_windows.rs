//! WASAPI loopback capture producing the protocol wire audio format
//! (interleaved f32 LE stereo at 48 kHz, matching `ferryx-rd-render::audio`).
//!
//! The capture taps the default render endpoint in shared mode, so whatever
//! the Windows desktop plays (apps, browsers, system sounds) is what streams
//! to the client. Shared-mode loopback wakes the worker through a WASAPI
//! event rather than polling the endpoint.

#[cfg(target_os = "windows")]
mod imp {
    use crate::windows_logic::{convert_to_wire_audio, SourceAudioFormat};
    use windows::core::GUID;
    use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_FAILED};
    use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

    struct CaptureEvent(HANDLE);
    impl Drop for CaptureEvent {
        fn drop(&mut self) {
            // SAFETY: this guard uniquely owns the event handle.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
    use windows::Win32::Media::Audio::{
        eMultimedia, eRender, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator,
        MMDeviceEnumerator, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
        AUDCLNT_STREAMFLAGS_LOOPBACK, WAVEFORMATEX, WAVEFORMATEXTENSIBLE,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED,
    };

    /// KSDATAFORMAT_SUBTYPE_IEEE_FLOAT.
    const IEEE_FLOAT_SUBFORMAT: GUID = GUID::from_u128(0x0000_0003_0000_0010_8000_00aa_0038_9b71);
    const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
    const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;
    const WAVE_FORMAT_PCM: u16 = 1;
    const AUDCLNT_BUFFERFLAGS_SILENT: u32 = 0x2;

    pub struct WindowsAudioCapture {
        audio_client: IAudioClient,
        capture_client: IAudioCaptureClient,
        source: SourceAudioFormat,
        event: CaptureEvent,
    }

    // SAFETY: COM interface pointers; used from the owning worker thread only.
    unsafe impl Send for WindowsAudioCapture {}

    impl WindowsAudioCapture {
        /// Initializes loopback capture on the default render endpoint.
        pub fn new() -> Result<Self, String> {
            unsafe {
                // The audio worker owns its thread; initialize COM for it.
                // Already-initialized threads (any apartment) are fine here.
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                let enumerator: IMMDeviceEnumerator =
                    CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                        .map_err(|error| format!("MMDeviceEnumerator: {error}"))?;
                let device = enumerator
                    .GetDefaultAudioEndpoint(eRender, eMultimedia)
                    .map_err(|error| format!("default render endpoint: {error}"))?;
                let audio_client: IAudioClient = device
                    .Activate(CLSCTX_ALL, None)
                    .map_err(|error| format!("IAudioClient activate: {error}"))?;
                let format_ptr: *mut WAVEFORMATEX = audio_client
                    .GetMixFormat()
                    .map_err(|error| format!("GetMixFormat: {error}"))?;
                if format_ptr.is_null() {
                    return Err("mix format was null".into());
                }
                let source = match parse_mix_format(format_ptr) {
                    Ok(source) => source,
                    Err(message) => {
                        CoTaskMemFree(Some(format_ptr.cast()));
                        return Err(message);
                    }
                };
                if let Err(error) = audio_client.Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                    2_000_000, // 200 ms buffer, drained far faster
                    0,
                    format_ptr,
                    None,
                ) {
                    CoTaskMemFree(Some(format_ptr.cast()));
                    return Err(format!("loopback initialize: {error}"));
                }
                CoTaskMemFree(Some(format_ptr.cast()));
                let capture_client: IAudioCaptureClient = audio_client
                    .GetService()
                    .map_err(|error| format!("IAudioCaptureClient: {error}"))?;
                let event = CaptureEvent(
                    CreateEventW(None, false, false, None)
                        .map_err(|error| format!("audio event: {error}"))?,
                );
                audio_client
                    .SetEventHandle(event.0)
                    .map_err(|error| format!("audio SetEventHandle: {error}"))?;
                audio_client
                    .Start()
                    .map_err(|error| format!("capture start: {error}"))?;
                Ok(Self {
                    audio_client,
                    capture_client,
                    source,
                    event,
                })
            }
        }

        /// Drains every queued loopback packet, returning wire-format f32 LE
        /// stereo bytes. Empty when the endpoint produced nothing (silence).
        pub fn poll(&mut self) -> Vec<u8> {
            // Bounded for cancellation during silence; packet readiness, not a timer,
            // drives normal capture. Existing callers retain the poll API.
            if unsafe { WaitForSingleObject(self.event.0, 100) } == WAIT_FAILED {
                tracing::warn!("WASAPI readiness wait failed");
                return Vec::new();
            }
            let mut raw = Vec::new();
            loop {
                let packet_bytes = match unsafe { self.capture_client.GetNextPacketSize() } {
                    Ok(frames) => frames,
                    Err(error) => {
                        tracing::debug!("audio packet size failed: {error}");
                        break;
                    }
                };
                if packet_bytes == 0 {
                    break;
                }
                let mut data: *mut u8 = std::ptr::null_mut();
                let mut frames = 0_u32;
                let mut flags = 0_u32;
                if let Err(error) = unsafe {
                    self.capture_client
                        .GetBuffer(&mut data, &mut frames, &mut flags, None, None)
                } {
                    tracing::debug!("audio GetBuffer failed: {error}");
                    break;
                }
                let frame_bytes = self.source.channels * if self.source.is_float { 4 } else { 2 };
                if flags & AUDCLNT_BUFFERFLAGS_SILENT != 0 {
                    // Buffer contents are undefined while the flag is set.
                    raw.resize(raw.len() + frames as usize * frame_bytes, 0);
                } else if !data.is_null() {
                    let slice =
                        unsafe { std::slice::from_raw_parts(data, frames as usize * frame_bytes) };
                    raw.extend_from_slice(slice);
                }
                if let Err(error) = unsafe { self.capture_client.ReleaseBuffer(frames) } {
                    tracing::debug!("audio ReleaseBuffer failed: {error}");
                    break;
                }
            }
            if raw.is_empty() {
                Vec::new()
            } else {
                convert_to_wire_audio(&raw, &self.source)
            }
        }
    }

    impl Drop for WindowsAudioCapture {
        fn drop(&mut self) {
            unsafe {
                let _ = self.audio_client.Stop();
            }
        }
    }

    fn parse_mix_format(format: *mut WAVEFORMATEX) -> Result<SourceAudioFormat, String> {
        // SAFETY: the mix format pointer is a valid WAVEFORMATEX from WASAPI.
        let header = unsafe { &*format };
        let is_float = match header.wFormatTag {
            WAVE_FORMAT_IEEE_FLOAT => true,
            WAVE_FORMAT_EXTENSIBLE => {
                // Packed layout: never form references to its fields.
                // SAFETY: WASAPI guarantees an extensible tag carries a
                // WAVEFORMATEXTENSIBLE layout.
                let extensible = format.cast::<WAVEFORMATEXTENSIBLE>();
                let sub_format =
                    unsafe { core::ptr::addr_of!((*extensible).SubFormat).read_unaligned() };
                sub_format == IEEE_FLOAT_SUBFORMAT
            }
            WAVE_FORMAT_PCM => false,
            tag => return Err(format!("unsupported mix format tag: {tag:#x}")),
        };
        // `convert_to_wire_audio` assumes 32-bit float or 16-bit int frames;
        // any other width would desynchronize the wire samples.
        let expected_bits = if is_float { 32 } else { 16 };
        // WAVEFORMATEX is packed: copy fields to locals before formatting them.
        let bits_per_sample = header.wBitsPerSample;
        if bits_per_sample != expected_bits {
            return Err(format!(
                "unsupported sample width: {bits_per_sample} bits (expected {expected_bits})"
            ));
        }
        let channels = header.nChannels as usize;
        if channels == 0 {
            return Err("mix format has no channels".into());
        }
        if header.nSamplesPerSec == 0 {
            return Err("mix format has no sample rate".into());
        }
        Ok(SourceAudioFormat {
            channels,
            sample_rate: header.nSamplesPerSec,
            is_float,
        })
    }
}

#[cfg(target_os = "windows")]
pub use imp::WindowsAudioCapture;
