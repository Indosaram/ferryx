use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AndroidVideoCodec {
    Hevc,
    Avc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AndroidMediaCodecConfig {
    pub codec: AndroidVideoCodec,
    pub width: u32,
    pub height: u32,
    pub low_latency_enabled: bool,
    pub surface_attached: bool,
}

impl Default for AndroidMediaCodecConfig {
    fn default() -> Self {
        Self {
            codec: AndroidVideoCodec::Hevc,
            width: 1920,
            height: 1080,
            low_latency_enabled: true,
            surface_attached: false,
        }
    }
}

#[derive(Debug, Error, PartialEq)]
pub enum AndroidMediaError {
    #[error("surface is not attached to MediaCodec")]
    SurfaceMissing,
    #[error("unsupported video resolution: {0}x{1}")]
    InvalidResolution(u32, u32),
    #[error("audio track initialization error: {0}")]
    AudioInit(String),
    #[error("native Android MediaCodec/AudioTrack backend is not available")]
    BackendUnavailable,
}

#[derive(Debug, PartialEq)]
pub struct AndroidMediaCodecDecoder {
    config: AndroidMediaCodecConfig,
    frames_decoded: u64,
}

impl AndroidMediaCodecDecoder {
    pub fn new(config: AndroidMediaCodecConfig) -> Result<Self, AndroidMediaError> {
        if config.width == 0 || config.height == 0 {
            return Err(AndroidMediaError::InvalidResolution(
                config.width,
                config.height,
            ));
        }
        if !config.surface_attached {
            return Err(AndroidMediaError::SurfaceMissing);
        }
        Ok(Self {
            config,
            frames_decoded: 0,
        })
    }

    pub fn attach_surface(&mut self) {
        self.config.surface_attached = true;
    }

    pub fn detach_surface(&mut self) {
        self.config.surface_attached = false;
    }

    pub fn decode_access_unit(&mut self, payload: &[u8]) -> Result<bool, AndroidMediaError> {
        if !self.config.surface_attached {
            return Err(AndroidMediaError::SurfaceMissing);
        }
        if payload.is_empty() {
            return Ok(false);
        }
        Err(AndroidMediaError::BackendUnavailable)
    }

    pub fn frames_decoded(&self) -> u64 {
        self.frames_decoded
    }
}

#[derive(Debug, Clone)]
pub struct AndroidAudioTrackPlayer {
    pub sample_rate: u32,
    pub channels: u16,
    pub buffer_frames: u32,
    pub samples_written: u64,
}

impl Default for AndroidAudioTrackPlayer {
    fn default() -> Self {
        Self {
            sample_rate: 48_000,
            channels: 2,
            buffer_frames: 1024,
            samples_written: 0,
        }
    }
}

impl AndroidAudioTrackPlayer {
    pub fn new(sample_rate: u32, channels: u16) -> Result<Self, AndroidMediaError> {
        if sample_rate == 0 || channels == 0 {
            return Err(AndroidMediaError::AudioInit(
                "invalid sample rate or channels".into(),
            ));
        }
        Ok(Self {
            sample_rate,
            channels,
            buffer_frames: 1024,
            samples_written: 0,
        })
    }

    pub fn write_pcm(&mut self, pcm: &[f32]) -> Result<usize, AndroidMediaError> {
        if pcm.is_empty() {
            return Ok(0);
        }
        Err(AndroidMediaError::BackendUnavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mediacodec_rejects_missing_surface() {
        let config = AndroidMediaCodecConfig {
            surface_attached: false,
            ..Default::default()
        };
        assert_eq!(
            AndroidMediaCodecDecoder::new(config),
            Err(AndroidMediaError::SurfaceMissing)
        );
    }

    #[test]
    fn mediacodec_rejects_missing_native_backend_with_attached_surface() {
        let config = AndroidMediaCodecConfig {
            surface_attached: true,
            ..Default::default()
        };
        let mut decoder = AndroidMediaCodecDecoder::new(config).unwrap();

        let dummy_nal = [0x00, 0x00, 0x00, 0x01, 0x40, 0x01];
        assert_eq!(
            decoder.decode_access_unit(&dummy_nal),
            Err(AndroidMediaError::BackendUnavailable)
        );
        assert_eq!(decoder.frames_decoded(), 0);

        decoder.detach_surface();
        assert_eq!(
            decoder.decode_access_unit(&dummy_nal),
            Err(AndroidMediaError::SurfaceMissing)
        );
    }

    #[test]
    fn audiotrack_rejects_missing_native_backend() {
        let mut audio = AndroidAudioTrackPlayer::new(48000, 2).unwrap();
        let samples = vec![0.0f32; 960];
        let res = audio.write_pcm(&samples);
        assert_eq!(res, Err(AndroidMediaError::BackendUnavailable));
        assert_eq!(audio.samples_written, 0);
    }
}
