//! WGPU device, queue, and GPU context management for terminal rendering.

use parking_lot::Mutex;
use std::sync::Arc;

use crate::native_terminal::error::NativeTerminalError;

fn opaque_composite_alpha_mode(
    alpha_modes: &[wgpu::CompositeAlphaMode],
) -> wgpu::CompositeAlphaMode {
    alpha_modes
        .iter()
        .copied()
        .find(|alpha_mode| matches!(alpha_mode, wgpu::CompositeAlphaMode::Opaque))
        .unwrap_or(wgpu::CompositeAlphaMode::Auto)
}

/// Selects a surface format that preserves the renderer's configured terminal color values.
///
/// Theme and cell colors enter the renderer as encoded 8-bit sRGB code values (`u8 / 255`).
/// Rendering those values directly to an `*Srgb` attachment applies a second linear-to-sRGB
/// transfer on store, which visibly lifts dark backgrounds and washes out ANSI colors. Prefer
/// a non-sRGB UNORM attachment so the configured code values reach the display unchanged.
fn preferred_terminal_surface_format(
    formats: &[wgpu::TextureFormat],
) -> Option<wgpu::TextureFormat> {
    [
        wgpu::TextureFormat::Bgra8Unorm,
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ]
    .into_iter()
    .find(|candidate| formats.contains(candidate))
}

/// Encapsulated GPU context managing wgpu Instance, Adapter, Device, and Queue.
pub struct GpuContext {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
    pub adapter_info: wgpu::AdapterInfo,
    uncaptured_error: Arc<Mutex<Option<String>>>,
}

impl GpuContext {
    /// Initializes a real wgpu device and queue, selecting the primary native backend.
    pub fn new() -> Result<Self, NativeTerminalError> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            flags: wgpu::InstanceFlags::default(),
            backend_options: wgpu::BackendOptions::default(),
        });

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .or_else(|| {
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: None,
                force_fallback_adapter: true,
            }))
        })
        .ok_or_else(|| {
            NativeTerminalError::GpuAdapterUnavailable(
                "No compatible GPU adapter found for native terminal renderer".to_string(),
            )
        })?;

        let adapter_info = adapter.get_info();
        let uncaptured_error = Arc::new(Mutex::new(None));
        let err_slot = Arc::clone(&uncaptured_error);

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("Ferryx Native Terminal Device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
            },
            None,
        ))
        .map_err(|e| NativeTerminalError::GpuDeviceUnavailable(e.to_string()))?;

        device.on_uncaptured_error(Box::new(move |err| {
            let mut slot = err_slot.lock();
            *slot = Some(err.to_string());
        }));

        Ok(Self {
            instance,
            adapter,
            device: Arc::new(device),
            queue: Arc::new(queue),
            adapter_info,
            uncaptured_error,
        })
    }

    /// Creates a native window render surface using the same WGPU instance.
    pub fn create_surface<'a>(
        &self,
        target: impl Into<wgpu::SurfaceTarget<'a>>,
    ) -> Result<wgpu::Surface<'a>, NativeTerminalError> {
        self.instance.create_surface(target).map_err(|e| {
            NativeTerminalError::GpuPipelineError(format!("Surface create error: {e}"))
        })
    }

    /// Configures a native surface with exact supported matching format and dimensions.
    pub fn configure_surface(
        &self,
        surface: &wgpu::Surface,
        width: u32,
        height: u32,
    ) -> Result<wgpu::TextureFormat, NativeTerminalError> {
        let cap = surface.get_capabilities(&self.adapter);
        let format = preferred_terminal_surface_format(&cap.formats).ok_or_else(|| {
            NativeTerminalError::GpuPipelineError(
                "Surface does not support standard 8-bit RGBA/BGRA formats".into(),
            )
        })?;

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: opaque_composite_alpha_mode(&cap.alpha_modes),
            view_formats: vec![],
        };
        surface.configure(&self.device, &config);
        Ok(format)
    }

    /// Checks if any uncaptured GPU error occurred and returns a typed error.
    pub fn check_error(&self) -> Result<(), NativeTerminalError> {
        let mut slot = self.uncaptured_error.lock();
        if let Some(err) = slot.take() {
            Err(NativeTerminalError::GpuPipelineError(err))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chooses_opaque_compositing_when_the_surface_supports_it() {
        let alpha_mode = opaque_composite_alpha_mode(&[
            wgpu::CompositeAlphaMode::PreMultiplied,
            wgpu::CompositeAlphaMode::Opaque,
        ]);

        assert_eq!(alpha_mode, wgpu::CompositeAlphaMode::Opaque);
    }

    #[test]
    fn falls_back_to_auto_compositing_when_opaque_is_unavailable() {
        let alpha_mode = opaque_composite_alpha_mode(&[wgpu::CompositeAlphaMode::PreMultiplied]);

        assert_eq!(alpha_mode, wgpu::CompositeAlphaMode::Auto);
    }

    #[test]
    fn prefers_linear_unorm_surface_for_srgb_encoded_terminal_colors() {
        let format = preferred_terminal_surface_format(&[
            wgpu::TextureFormat::Bgra8UnormSrgb,
            wgpu::TextureFormat::Bgra8Unorm,
        ]);

        assert_eq!(format, Some(wgpu::TextureFormat::Bgra8Unorm));
    }

    #[test]
    fn falls_back_to_srgb_surface_only_when_unorm_is_unavailable() {
        let format = preferred_terminal_surface_format(&[
            wgpu::TextureFormat::Rgba16Float,
            wgpu::TextureFormat::Bgra8UnormSrgb,
        ]);

        assert_eq!(format, Some(wgpu::TextureFormat::Bgra8UnormSrgb));
    }
}
