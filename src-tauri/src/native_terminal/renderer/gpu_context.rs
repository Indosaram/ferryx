//! WGPU device, queue, and GPU context management for terminal rendering.

use parking_lot::Mutex;
use std::sync::{Arc, OnceLock};

use super::atlas::BASE_DIMENSION as ATLAS_BASE_DIMENSION;
use super::pipeline::{GlyphInstance, ScreenUniform};
use crate::native_terminal::error::NativeTerminalError;

// Serialize independent instance initialization and final release, not GPU work.
static INSTANCE_LIFECYCLE: Mutex<()> = Mutex::new(());

#[cfg(test)]
thread_local! {
    static FORCE_ADAPTER_UNAVAILABLE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Test seam: while the returned guard lives, adapter requests on this thread fail as if the
/// machine had no GPU adapter.
#[cfg(test)]
pub(crate) struct ForcedAdapterFailure(());

#[cfg(test)]
impl Drop for ForcedAdapterFailure {
    fn drop(&mut self) {
        FORCE_ADAPTER_UNAVAILABLE.with(|forced| forced.set(false));
    }
}

/// Debug builds honor `FERRYX_FORCE_NO_GPU_ADAPTER` so the DOM fallback can be exercised on a
/// machine that does have a GPU. Release builds ignore it.
fn adapter_forced_unavailable() -> bool {
    #[cfg(test)]
    if FORCE_ADAPTER_UNAVAILABLE.with(|forced| forced.get()) {
        return true;
    }
    cfg!(debug_assertions) && std::env::var_os("FERRYX_FORCE_NO_GPU_ADAPTER").is_some()
}

fn forced_adapter_error() -> NativeTerminalError {
    NativeTerminalError::GpuAdapterUnavailable(
        "GPU adapter disabled by FERRYX_FORCE_NO_GPU_ADAPTER or the test seam".into(),
    )
}

// Latched the first time the GPU proves it cannot render for this launch: a window surface
// reports no usable format for the shared adapter, or the device is lost or raises an uncaptured
// error. The shared device is cached for the process lifetime, so the result holds for this launch.
// Tests keep the latch per thread, cleared by a guard, so one test cannot affect another.
#[cfg(not(test))]
static RENDERER_UNAVAILABLE: OnceLock<NativeTerminalError> = OnceLock::new();

#[cfg(test)]
thread_local! {
    static RENDERER_UNAVAILABLE: std::cell::RefCell<Option<NativeTerminalError>> = const { std::cell::RefCell::new(None) };
}

/// Test seam: clears this thread's surface-presentation latch when dropped.
#[cfg(test)]
pub(crate) struct SurfacePresentationLatchReset(());

#[cfg(test)]
impl Drop for SurfacePresentationLatchReset {
    fn drop(&mut self) {
        RENDERER_UNAVAILABLE.with(|latch| latch.borrow_mut().take());
    }
}

#[cfg(not(test))]
fn latch_renderer_unavailable(error: NativeTerminalError) {
    let _ = RENDERER_UNAVAILABLE.set(error);
}

#[cfg(test)]
fn latch_renderer_unavailable(error: NativeTerminalError) {
    RENDERER_UNAVAILABLE.with(|latch| {
        latch.borrow_mut().get_or_insert(error);
    });
}

#[cfg(not(test))]
fn renderer_unavailable() -> Option<NativeTerminalError> {
    RENDERER_UNAVAILABLE.get().cloned()
}

#[cfg(test)]
fn renderer_unavailable() -> Option<NativeTerminalError> {
    RENDERER_UNAVAILABLE.with(|latch| latch.borrow().clone())
}

/// Device limits to request: the WebGL2 downlevel floor that GL, software (llvmpipe) and VM
/// adapters can grant, raised to the adapter's own texture resolution so large windows still fit.
fn requested_device_limits(adapter_limits: wgpu::Limits) -> wgpu::Limits {
    wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter_limits)
}

/// Fails when a granted limit is below what the terminal pipelines, glyph atlas and instance
/// buffers use, so the pane falls back to the DOM renderer instead of rendering incorrectly.
fn check_renderer_limits(
    granted: &wgpu::Limits,
    adapter_info: &wgpu::AdapterInfo,
) -> Result<(), NativeTerminalError> {
    let needs: [(&str, u64, u64); 11] = [
        (
            "max_texture_dimension_2d",
            ATLAS_BASE_DIMENSION.into(),
            granted.max_texture_dimension_2d.into(),
        ),
        // Screen uniforms plus the glyph atlas (or inline image) textures.
        ("max_bind_groups", 2, granted.max_bind_groups.into()),
        (
            "max_bind_groups_plus_vertex_buffers",
            3,
            granted.max_bind_groups_plus_vertex_buffers.into(),
        ),
        (
            "max_uniform_buffers_per_shader_stage",
            1,
            granted.max_uniform_buffers_per_shader_stage.into(),
        ),
        (
            "max_uniform_buffer_binding_size",
            std::mem::size_of::<ScreenUniform>() as u64,
            granted.max_uniform_buffer_binding_size,
        ),
        (
            "max_sampled_textures_per_shader_stage",
            2,
            granted.max_sampled_textures_per_shader_stage.into(),
        ),
        (
            "max_samplers_per_shader_stage",
            1,
            granted.max_samplers_per_shader_stage.into(),
        ),
        ("max_vertex_buffers", 1, granted.max_vertex_buffers.into()),
        (
            "max_vertex_attributes",
            4,
            granted.max_vertex_attributes.into(),
        ),
        (
            "max_vertex_buffer_array_stride",
            std::mem::size_of::<GlyphInstance>() as u64,
            granted.max_vertex_buffer_array_stride.into(),
        ),
        // Glyph pass varyings: uv, color and the color-glyph flag.
        (
            "max_inter_stage_shader_variables",
            3,
            granted.max_inter_stage_shader_variables.into(),
        ),
    ];
    let shortfalls: Vec<String> = needs
        .iter()
        .filter(|(_, need, have)| have < need)
        .map(|(name, need, have)| format!("{name} {have} < {need}"))
        .collect();
    if shortfalls.is_empty() {
        return Ok(());
    }
    Err(NativeTerminalError::GpuDeviceUnavailable(format!(
        "GPU limits granted by adapter {:?} ({:?}) are below the native terminal renderer's needs: {}",
        adapter_info.name,
        adapter_info.backend,
        shortfalls.join(", "),
    )))
}

/// A device lost after creation leaves every later frame blank. `Destroyed` is the device's own
/// teardown, not a failure.
fn device_lost_error(reason: wgpu::DeviceLostReason, message: &str) -> Option<NativeTerminalError> {
    match reason {
        wgpu::DeviceLostReason::Destroyed => None,
        wgpu::DeviceLostReason::Unknown => Some(NativeTerminalError::GpuDeviceUnavailable(
            format!("GPU device lost: {message}"),
        )),
    }
}

/// Runs `attempt` on each candidate in order and returns the first success. Fails with the last
/// attempt's error, or `None` when there was no candidate at all.
fn first_working<C, T>(
    candidates: impl IntoIterator<Item = C>,
    mut attempt: impl FnMut(C) -> Result<T, NativeTerminalError>,
) -> Result<T, Option<NativeTerminalError>> {
    let mut last_error = None;
    for candidate in candidates {
        match attempt(candidate) {
            Ok(working) => return Ok(working),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error)
}

fn uncaptured_device_error(message: &str) -> NativeTerminalError {
    NativeTerminalError::GpuDeviceUnavailable(format!("Uncaptured GPU device error: {message}"))
}

/// Picks the terminal surface format, or reports that this adapter cannot present to a window.
///
/// A driverless host can still hand out a software fallback adapter whose surface capabilities
/// list no formats. Every frame would then fail, leaving a blank pane, so the failure is typed
/// as renderer-unavailable and latched for the probe that gates new attaches.
fn presentable_surface_format(
    formats: &[wgpu::TextureFormat],
    adapter_info: &wgpu::AdapterInfo,
) -> Result<wgpu::TextureFormat, NativeTerminalError> {
    preferred_terminal_surface_format(formats).ok_or_else(|| {
        let message = format!(
            "Surface does not support standard 8-bit RGBA/BGRA formats (adapter {:?}, {:?} on {:?}, {} surface formats)",
            adapter_info.name,
            adapter_info.device_type,
            adapter_info.backend,
            formats.len(),
        );
        let error = NativeTerminalError::GpuSurfaceUnsupported(message);
        latch_renderer_unavailable(error.clone());
        error
    })
}

struct OwnedInstance(Option<wgpu::Instance>);

impl Drop for OwnedInstance {
    fn drop(&mut self) {
        let _guard = INSTANCE_LIFECYCLE.lock();
        // Drop the value here: automatic field drop would run after the guard unlocks.
        drop(self.0.take());
    }
}

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
#[derive(Clone)]
pub struct GpuContext {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
    pub adapter_info: wgpu::AdapterInfo,
    uncaptured_error: Arc<Mutex<Option<String>>>,
    // Last field: all context-owned handles release before the retained instance.
    // Cloned contexts retain the same owner until their last release.
    _owned_instance: Option<Arc<OwnedInstance>>,
}

impl GpuContext {
    #[cfg(test)]
    pub(crate) fn force_adapter_unavailable_for_test() -> ForcedAdapterFailure {
        FORCE_ADAPTER_UNAVAILABLE.with(|forced| forced.set(true));
        ForcedAdapterFailure(())
    }

    /// Reports a forced adapter failure (test seam or debug env override) on the calling thread.
    pub(crate) fn check_forced_adapter_failure() -> Result<(), NativeTerminalError> {
        if adapter_forced_unavailable() {
            Err(forced_adapter_error())
        } else {
            Ok(())
        }
    }

    /// Reports that a window surface already proved the shared adapter cannot present, or that
    /// the device was lost or raised an uncaptured error after creation.
    ///
    /// Checked by the attach probe and by bounds updates, so the pane that hit the failure and
    /// every later pane receive `NATIVE_RENDERER_UNAVAILABLE` instead of a blank surface.
    pub(crate) fn check_surface_presentation() -> Result<(), NativeTerminalError> {
        match renderer_unavailable() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// Test seam: latches "surface cannot present" on this thread, as a format-less surface
    /// would, until the returned guard drops.
    #[cfg(test)]
    pub(crate) fn latch_surface_unsupported_for_test(
        formats: &[wgpu::TextureFormat],
        adapter_info: &wgpu::AdapterInfo,
    ) -> (NativeTerminalError, SurfacePresentationLatchReset) {
        let reset = SurfacePresentationLatchReset(());
        let error = presentable_surface_format(formats, adapter_info)
            .expect_err("test seam expects a surface without standard formats");
        (error, reset)
    }

    // Share one process-lifetime instance across production surfaces only.
    // Independent contexts use separate instances so they do not share one EGL context.
    pub(crate) fn shared_surface_instance() -> wgpu::Instance {
        static INSTANCE: OnceLock<wgpu::Instance> = OnceLock::new();
        INSTANCE.get_or_init(|| {
            wgpu::Instance::new(wgpu::InstanceDescriptor {
                backends: wgpu::Backends::all(),
                ..wgpu::InstanceDescriptor::new_without_display_handle()
            })
        }).clone()
    }

    pub(crate) fn shared_surface_context() -> Result<Self, NativeTerminalError> {
        if adapter_forced_unavailable() {
            return Err(forced_adapter_error());
        }
        static CONTEXT: OnceLock<Result<GpuContext, NativeTerminalError>> = OnceLock::new();
        CONTEXT.get_or_init(|| Self::with_instance(Self::shared_surface_instance())).clone()
    }

    /// Initializes a real wgpu device and queue, selecting the primary native backend.
    pub fn new() -> Result<Self, NativeTerminalError> {
        let (instance, owner) = {
            let _guard = INSTANCE_LIFECYCLE.lock();
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
                backends: wgpu::Backends::all(),
                flags: wgpu::InstanceFlags::default(),
                backend_options: wgpu::BackendOptions::default(),
                ..wgpu::InstanceDescriptor::new_without_display_handle()
            });
            let owner = Arc::new(OwnedInstance(Some(instance.clone())));
            (instance, owner)
        };
        Self::with_owned_instance(Some(owner), instance)
    }

    pub(crate) fn with_instance(instance: wgpu::Instance) -> Result<Self, NativeTerminalError> {
        Self::with_owned_instance(None, instance)
    }

    fn with_owned_instance(
        // Arguments drop in reverse order on failure, after local adapter/device handles.
        owned_instance: Option<Arc<OwnedInstance>>,
        instance: wgpu::Instance,
    ) -> Result<Self, NativeTerminalError> {
        if adapter_forced_unavailable() {
            return Err(forced_adapter_error());
        }
        // wgpu only sorts adapters by power preference, so each request returns an adapter
        // whenever one exists. Every distinct candidate gets a device and the limit check;
        // the first that can run the renderer wins.
        let mut tried = Vec::new();
        let mut adapter_error = None;
        let candidates = [
            (wgpu::PowerPreference::LowPower, false),
            (wgpu::PowerPreference::HighPerformance, false),
            (wgpu::PowerPreference::default(), true),
        ]
        .into_iter()
        .filter_map(|(power_preference, force_fallback_adapter)| {
            let request = instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference,
                compatible_surface: None,
                force_fallback_adapter,
                ..Default::default()
            });
            let adapter = pollster::block_on(request)
                .map_err(|e| adapter_error = Some(e.to_string()))
                .ok()?;
            let info = adapter.get_info();
            if tried.contains(&info) {
                return None;
            }
            tried.push(info);
            Some(adapter)
        });
        let working = first_working(candidates, |adapter| {
            let adapter_info = adapter.get_info();
            let device = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("Ferryx Native Terminal Device"),
                required_features: wgpu::Features::empty(),
                required_limits: requested_device_limits(adapter.limits()),
                memory_hints: wgpu::MemoryHints::Performance,
                ..Default::default()
            }))
            .map_err(|e| NativeTerminalError::GpuDeviceUnavailable(e.to_string()))
            .and_then(|(device, queue)| {
                check_renderer_limits(&device.limits(), &adapter_info)?;
                Ok((device, queue))
            });
            match device {
                Ok((device, queue)) => Ok((adapter, adapter_info, device, queue)),
                Err(error) => {
                    tracing::warn!(
                        adapter = ?adapter_info.name,
                        backend = ?adapter_info.backend,
                        %error,
                        "GPU adapter cannot run the native terminal renderer; trying the next one"
                    );
                    Err(error)
                }
            }
        });
        let (adapter, adapter_info, device, queue) = working.map_err(|last_error| {
            last_error.unwrap_or_else(|| {
                NativeTerminalError::GpuAdapterUnavailable(format!(
                    "No compatible GPU adapter found for native terminal renderer: {}",
                    adapter_error.unwrap_or_default()
                ))
            })
        })?;

        let uncaptured_error = Arc::new(Mutex::new(None));
        let err_slot = Arc::clone(&uncaptured_error);

        // Any later device failure leaves the pane blank or wrong, so it latches the same
        // renderer-unavailable state that hands panes to the DOM renderer.
        device.on_uncaptured_error(Arc::new(move |err| {
            let message = err.to_string();
            latch_renderer_unavailable(uncaptured_device_error(&message));
            let mut slot = err_slot.lock();
            *slot = Some(message);
        }));
        let lost_slot = Arc::clone(&uncaptured_error);
        device.set_device_lost_callback(move |reason, message| {
            if let Some(error) = device_lost_error(reason, &message) {
                *lost_slot.lock() = Some(error.to_string());
                latch_renderer_unavailable(error);
            }
        });

        Ok(Self {
            instance,
            adapter,
            device: Arc::new(device),
            queue: Arc::new(queue),
            adapter_info,
            uncaptured_error,
            _owned_instance: owned_instance,
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
        let format = presentable_surface_format(&cap.formats, &self.adapter_info)?;

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: opaque_composite_alpha_mode(&cap.alpha_modes),
            view_formats: vec![],
            color_space: wgpu::SurfaceColorSpace::Auto,
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

    fn software_adapter_info() -> wgpu::AdapterInfo {
        wgpu::AdapterInfo {
            name: "llvmpipe".into(),
            ..wgpu::AdapterInfo::new(wgpu::DeviceType::Cpu, wgpu::Backend::Gl)
        }
    }

    #[test]
    fn surface_without_standard_formats_is_renderer_unavailable_and_latched() {
        let _reset = SurfacePresentationLatchReset(());
        assert!(GpuContext::check_surface_presentation().is_ok());

        let error = presentable_surface_format(&[], &software_adapter_info())
            .expect_err("a surface with no formats can never present");

        assert!(matches!(error, NativeTerminalError::GpuSurfaceUnsupported(_)));
        assert!(error
            .to_string()
            .contains("Surface does not support standard 8-bit RGBA/BGRA formats"));
        assert!(matches!(
            GpuContext::check_surface_presentation(),
            Err(NativeTerminalError::GpuSurfaceUnsupported(_))
        ));
    }

    #[test]
    fn surface_with_a_standard_format_does_not_latch() {
        let _reset = SurfacePresentationLatchReset(());
        let format = presentable_surface_format(
            &[wgpu::TextureFormat::Rgba8Unorm],
            &software_adapter_info(),
        )
        .unwrap();

        assert_eq!(format, wgpu::TextureFormat::Rgba8Unorm);
        assert!(GpuContext::check_surface_presentation().is_ok());
    }

    fn assert_renderer_unavailable(error: NativeTerminalError) {
        let ipc = crate::ipc::IpcError::from(error);
        println!("ipc error: {}", serde_json::to_string(&ipc).unwrap());
        assert_eq!(
            ipc.code,
            crate::ipc::IpcErrorCode::NativeRendererUnavailable
        );
        assert_eq!(
            ipc.details,
            Some(serde_json::json!({ "source": "wgpu", "kind": "renderer_unavailable" }))
        );
    }

    #[test]
    fn requested_limits_derive_from_the_adapter() {
        // A GL/llvmpipe-class adapter: downlevel limits with its own texture resolution.
        let gl_adapter = wgpu::Limits {
            max_texture_dimension_1d: 16384,
            max_texture_dimension_2d: 16384,
            max_texture_dimension_3d: 2048,
            ..wgpu::Limits::downlevel_webgl2_defaults()
        };
        assert!(
            !wgpu::Limits::default().check_limits(&gl_adapter),
            "the WebGPU defaults are more than this adapter can grant"
        );

        let requested = requested_device_limits(gl_adapter.clone());

        assert_eq!(requested.max_texture_dimension_1d, 16384);
        assert_eq!(requested.max_texture_dimension_2d, 16384);
        assert_eq!(requested.max_texture_dimension_3d, 2048);
        assert!(requested.check_limits(&gl_adapter));
        assert!(check_renderer_limits(&requested, &software_adapter_info()).is_ok());

        // A high-end adapter only raises the resolution, never the downlevel floor.
        let discrete = wgpu::Limits {
            max_texture_dimension_2d: 32768,
            ..wgpu::Limits::default()
        };
        let requested = requested_device_limits(discrete.clone());
        assert_eq!(requested.max_texture_dimension_2d, 32768);
        assert_eq!(requested.max_storage_buffers_per_shader_stage, 0);
        assert!(requested.check_limits(&discrete));
    }

    #[test]
    fn granted_device_limits_are_the_adapter_derived_request() {
        let gpu = match GpuContext::new() {
            Ok(gpu) => gpu,
            Err(_) => return,
        };
        let granted = gpu.device.limits();
        println!(
            "adapter={:?} type={:?} backend={:?} default_limits_grantable={} granted={granted:?}",
            gpu.adapter_info.name,
            gpu.adapter_info.device_type,
            gpu.adapter_info.backend,
            wgpu::Limits::default().check_limits(&gpu.adapter.limits()),
        );

        assert_eq!(granted, requested_device_limits(gpu.adapter.limits()));
        assert!(check_renderer_limits(&granted, &gpu.adapter_info).is_ok());
    }

    #[test]
    fn renderer_needs_above_granted_limits_are_renderer_unavailable() {
        let granted = wgpu::Limits {
            max_bind_groups: 1,
            max_vertex_attributes: 2,
            ..requested_device_limits(wgpu::Limits::downlevel_webgl2_defaults())
        };

        let error = check_renderer_limits(&granted, &software_adapter_info())
            .expect_err("a device below the renderer's needs must not be used");

        assert!(matches!(
            error,
            NativeTerminalError::GpuDeviceUnavailable(_)
        ));
        let message = error.to_string();
        assert!(message.contains("max_bind_groups 1 < 2"), "{message}");
        assert!(message.contains("max_vertex_attributes 2 < 4"), "{message}");
        assert_renderer_unavailable(error);
    }

    #[test]
    fn an_adapter_failing_the_limit_check_falls_through_to_the_next_candidate() {
        let mut tried = Vec::new();

        let working = first_working(["low-power", "high-performance", "fallback"], |name| {
            tried.push(name);
            match name {
                "low-power" => Err(NativeTerminalError::GpuDeviceUnavailable(
                    "max_bind_groups 1 < 2".into(),
                )),
                other => Ok(other),
            }
        });

        assert_eq!(working, Ok("high-performance"));
        assert_eq!(tried, ["low-power", "high-performance"]);
    }

    #[test]
    fn every_candidate_failing_returns_the_last_error() {
        let mut tried = Vec::new();

        let working: Result<(), _> =
            first_working(["low-power", "high-performance", "fallback"], |name| {
                tried.push(name);
                Err(NativeTerminalError::GpuDeviceUnavailable(name.into()))
            });

        assert_eq!(
            working,
            Err(Some(NativeTerminalError::GpuDeviceUnavailable(
                "fallback".into()
            )))
        );
        assert_eq!(tried, ["low-power", "high-performance", "fallback"]);
        assert_eq!(
            first_working(Vec::<&str>::new(), |_| Ok(())),
            Err(None),
            "no adapter at all is reported separately"
        );
    }

    #[test]
    fn device_loss_after_creation_latches_renderer_unavailable() {
        let _reset = SurfacePresentationLatchReset(());
        assert!(device_lost_error(wgpu::DeviceLostReason::Destroyed, "").is_none());

        let error = device_lost_error(wgpu::DeviceLostReason::Unknown, "driver reset")
            .expect("an unexpected device loss is a renderer failure");
        latch_renderer_unavailable(error);

        let error = GpuContext::check_surface_presentation()
            .expect_err("attach and bounds updates report the lost device");
        assert_eq!(
            error,
            NativeTerminalError::GpuDeviceUnavailable("GPU device lost: driver reset".into())
        );
        assert_renderer_unavailable(error);
    }

    #[test]
    fn uncaptured_device_error_latches_renderer_unavailable() {
        let _reset = SurfacePresentationLatchReset(());
        let gpu = match GpuContext::new() {
            Ok(gpu) => gpu,
            Err(_) => return,
        };
        assert!(GpuContext::check_surface_presentation().is_ok());

        // MAP_READ | MAP_WRITE without MAPPABLE_PRIMARY_BUFFERS is a validation error that no
        // error scope captures.
        let _buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("invalid usage"),
            size: 4,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::MAP_WRITE,
            mapped_at_creation: false,
        });

        assert!(gpu.check_error().is_err());
        let error = GpuContext::check_surface_presentation()
            .expect_err("an uncaptured device error hands panes to the DOM renderer");
        assert!(
            error.to_string().contains("Uncaptured GPU device error"),
            "{error}"
        );
        assert_renderer_unavailable(error);
    }
}
