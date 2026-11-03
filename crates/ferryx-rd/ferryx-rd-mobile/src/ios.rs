use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IosPixelFormat {
    Nv12,
    Bgra,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IosVideoToolboxConfig {
    pub width: u32,
    pub height: u32,
    pub real_time_decompression: bool,
    pub pixel_format: IosPixelFormat,
    pub metal_layer_attached: bool,
}

impl Default for IosVideoToolboxConfig {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            real_time_decompression: true,
            pixel_format: IosPixelFormat::Nv12,
            metal_layer_attached: false,
        }
    }
}

#[derive(Debug, Error, PartialEq)]
pub enum IosMediaError {
    #[error("Metal layer or texture target missing")]
    MetalLayerMissing,
    #[error("decompression session initialization failed")]
    DecompressionInitFailed,
    #[error("native iOS VideoToolbox/AudioEngine backend is not available")]
    BackendUnavailable,
}

#[derive(Debug, PartialEq)]
pub struct IosVideoToolboxDecoder {
    config: IosVideoToolboxConfig,
    frames_rendered: u64,
}

impl IosVideoToolboxDecoder {
    pub fn new(config: IosVideoToolboxConfig) -> Result<Self, IosMediaError> {
        if !config.metal_layer_attached {
            return Err(IosMediaError::MetalLayerMissing);
        }
        Ok(Self {
            config,
            frames_rendered: 0,
        })
    }

    pub fn attach_metal_layer(&mut self) {
        self.config.metal_layer_attached = true;
    }

    pub fn detach_metal_layer(&mut self) {
        self.config.metal_layer_attached = false;
    }

    pub fn render_frame(&mut self, payload: &[u8]) -> Result<bool, IosMediaError> {
        if !self.config.metal_layer_attached {
            return Err(IosMediaError::MetalLayerMissing);
        }
        if payload.is_empty() {
            return Ok(false);
        }
        Err(IosMediaError::BackendUnavailable)
    }

    pub fn frames_rendered(&self) -> u64 {
        self.frames_rendered
    }
}

#[derive(Debug, Clone)]
pub struct IosAudioEnginePlayer {
    pub sample_rate: f64,
    pub channels: u32,
    pub is_running: bool,
    pub frames_played: u64,
}

impl Default for IosAudioEnginePlayer {
    fn default() -> Self {
        Self {
            sample_rate: 48000.0,
            channels: 2,
            is_running: false,
            frames_played: 0,
        }
    }
}

impl IosAudioEnginePlayer {
    pub fn start(&mut self) -> Result<(), IosMediaError> {
        self.is_running = false;
        Err(IosMediaError::BackendUnavailable)
    }

    pub fn stop(&mut self) {
        self.is_running = false;
    }

    pub fn render_pcm(&mut self, _samples: &[f32]) -> usize {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn videotoolbox_requires_metal_layer() {
        let config = IosVideoToolboxConfig {
            metal_layer_attached: false,
            ..Default::default()
        };
        assert_eq!(
            IosVideoToolboxDecoder::new(config),
            Err(IosMediaError::MetalLayerMissing)
        );
    }

    #[test]
    fn videotoolbox_rejects_missing_native_backend_with_metal_layer() {
        let config = IosVideoToolboxConfig {
            metal_layer_attached: true,
            ..Default::default()
        };
        let mut decoder = IosVideoToolboxDecoder::new(config).unwrap();

        let dummy_hevc = [0x00, 0x00, 0x00, 0x01, 0x26, 0x01];
        assert_eq!(
            decoder.render_frame(&dummy_hevc),
            Err(IosMediaError::BackendUnavailable)
        );
        assert_eq!(decoder.frames_rendered(), 0);
    }

    #[test]
    fn audio_engine_lifecycle_and_backend_unavailability() {
        let mut engine = IosAudioEnginePlayer::default();
        assert_eq!(engine.render_pcm(&[0.0, 0.0]), 0);

        let res = engine.start();
        assert_eq!(res, Err(IosMediaError::BackendUnavailable));
        assert!(!engine.is_running);
        assert_eq!(engine.render_pcm(&[0.1, -0.1, 0.2, -0.2]), 0);
        assert_eq!(engine.frames_played, 0);

        engine.stop();
        assert_eq!(engine.render_pcm(&[0.1, -0.1]), 0);
    }
}
