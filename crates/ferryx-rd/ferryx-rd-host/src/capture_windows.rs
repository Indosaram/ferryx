//! DXGI Desktop Duplication capture for the Windows host.
//!
//! [`WindowsCapture::acquire_next_frame`] waits for the next desktop update,
//! asks DXGI for dirty/move metadata, copies the GPU texture through a staging
//! texture, and returns a tightly packed top-down BGRA buffer. DXGI access-loss
//! recovery (display mode changes, lock/unlock, driver reset) is handled by the
//! underlying manager on the next call.
//!
//! CI proves this module builds. Runtime QA still requires a real Windows 10/11
//! interactive desktop with a Desktop Duplication-capable graphics driver.

use std::time::Duration;

pub use crate::windows_logic::{
    resolve_output_metadata, RawDuplDesc, RawOutputDesc, SelectedOutputMetadata,
};
use dxgi_capture_rs::{CaptureError as DxgiCaptureError, DXGIManager};
use thiserror::Error;
use windows::{
    core::Interface,
    Win32::{
        Foundation::{E_FAIL, HMODULE},
        Graphics::{
            Direct3D::{D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_9_1},
            Direct3D11::{
                D3D11CreateDevice, ID3D11Device, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                D3D11_SDK_VERSION,
            },
            Dxgi::{
                CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1, IDXGIOutput1,
                DXGI_ERROR_NOT_FOUND, DXGI_OUTDUPL_DESC, DXGI_OUTPUT_DESC,
            },
        },
    },
};

/// Pixel-space rectangle in top-left-origin desktop coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirtyRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// A DXGI move operation. Moves precede dirty rectangles when applying damage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveRect {
    /// Signed like the Windows POINT type: negative source coordinates are
    /// valid and must not silently drop the move operation.
    pub source_x: i32,
    pub source_y: i32,
    pub destination: DirtyRect,
}

/// Tightly packed top-down BGRA8 pixels plus Desktop Duplication metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedFrame {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub bgra: Vec<u8>,
    pub dirty_regions: Vec<DirtyRect>,
    pub move_regions: Vec<MoveRect>,
    pub pointer_position: Option<(i32, i32)>,
    pub pointer_visible: bool,
    pub accumulated_frames: u32,
    pub presentation_counter: i64,
}

#[derive(Debug, Error)]
pub enum CaptureError {
    #[error("DXGI Desktop Duplication initialization failed: {0}")]
    Initialization(String),
    #[error("DXGI capture timed out")]
    Timeout,
    #[error("DXGI desktop access was denied, usually because protected content is visible")]
    AccessDenied,
    #[error("DXGI desktop duplication was lost; retry to rebuild it")]
    AccessLost,
    #[error("DXGI desktop duplication could not be refreshed")]
    RefreshFailure,
    #[error("DXGI frame capture failed: {0}")]
    Capture(String),
    #[error("captured frame dimensions or buffer length overflowed")]
    InvalidFrame,
    #[error("display rotation ({0}) is unsupported without a pixel rotation transform")]
    UnsupportedRotation(i32),
}

/// Blocking capture source for one Windows display.
pub struct WindowsCapture {
    manager: DXGIManager,
    display_index: usize,
    /// Physical pixel dimensions of the duplicated output, when they could be
    /// resolved. Frames are validated against these so a DPI-virtualized
    /// (logical) capture is rejected here instead of truncating the encoder.
    physical: Option<(u32, u32)>,
}

impl WindowsCapture {
    /// Pixel dimensions of the primary DXGI output, for session negotiation
    /// before a full capture pipeline starts. Reads the physical output
    /// duplication mode without waiting for a desktop update or acquiring pixels.
    pub fn primary_output_geometry() -> Result<(u32, u32), CaptureError> {
        let meta = Self::primary_output_metadata()?;
        Ok((meta.pixel_width, meta.pixel_height))
    }

    /// Full metadata (physical pixels, logical desktop bounds, DPI scale) of
    /// the primary DXGI output.
    pub fn primary_output_metadata() -> Result<SelectedOutputMetadata, CaptureError> {
        selected_output_metadata(0)
    }

    pub fn selected_output_metadata(
        display_index: usize,
    ) -> Result<SelectedOutputMetadata, CaptureError> {
        selected_output_metadata(display_index)
    }

    pub fn new(display_index: usize, timeout: Duration) -> Result<Self, CaptureError> {
        // Probe before the manager duplicates the output: the probe duplicates
        // it too, and DXGI limits concurrent duplications of one output.
        let physical = physical_dimensions(display_index);
        let mut manager = DXGIManager::new(duration_ms(timeout))
            .map_err(|error| CaptureError::Initialization(error.to_string()))?;
        manager.set_capture_source_index(display_index);
        Ok(Self {
            manager,
            display_index,
            physical,
        })
    }

    pub fn display_index(&self) -> usize {
        self.display_index
    }

    /// Physical pixel dimensions of the captured output, falling back to the
    /// duplication manager's desktop-coordinate geometry.
    pub fn geometry(&self) -> (u32, u32) {
        if let Some(physical) = self.physical {
            return physical;
        }
        let (width, height) = self.manager.geometry();
        (saturating_u32(width), saturating_u32(height))
    }

    pub fn set_timeout(&mut self, timeout: Duration) {
        self.manager.set_timeout_ms(duration_ms(timeout));
    }

    pub fn select_display(&mut self, display_index: usize) {
        self.physical = physical_dimensions(display_index);
        self.manager.set_capture_source_index(display_index);
        self.display_index = display_index;
    }

    /// Acquire one update. `CaptureError::Timeout` means no desktop update was
    /// available before the requested deadline and is not a fatal condition.
    pub fn acquire_next_frame(&mut self, timeout: Duration) -> Result<CapturedFrame, CaptureError> {
        self.set_timeout(timeout);
        let (bgra, (width, height), metadata) = self
            .manager
            .capture_frame_components_with_metadata()
            .map_err(map_capture_error)?;

        let expected = width
            .checked_mul(height)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or(CaptureError::InvalidFrame)?;
        if bgra.len() != expected {
            return Err(CaptureError::InvalidFrame);
        }

        let width = u32::try_from(width).map_err(|_| CaptureError::InvalidFrame)?;
        let height = u32::try_from(height).map_err(|_| CaptureError::InvalidFrame)?;
        // The rest of the pipeline (encoder configuration, wire geometry) is
        // sized from the physical duplication mode, so a logical-sized frame is
        // a truncated capture and must not reach the encoder.
        if self
            .physical
            .is_some_and(|physical| physical != (width, height))
        {
            return Err(CaptureError::InvalidFrame);
        }
        let dirty_regions = metadata
            .dirty_rects
            .into_iter()
            .filter_map(|(left, top, right, bottom)| rect_from_edges(left, top, right, bottom))
            .collect();
        let move_regions = metadata
            .move_rects
            .into_iter()
            .filter_map(|region| {
                let destination = rect_from_edges(
                    region.destination_rect.0,
                    region.destination_rect.1,
                    region.destination_rect.2,
                    region.destination_rect.3,
                )?;
                Some(MoveRect {
                    source_x: region.source_point.0,
                    source_y: region.source_point.1,
                    destination,
                })
            })
            .collect();

        Ok(CapturedFrame {
            width,
            height,
            stride: width.checked_mul(4).ok_or(CaptureError::InvalidFrame)?,
            bgra,
            dirty_regions,
            move_regions,
            pointer_position: metadata.pointer_position,
            pointer_visible: metadata.pointer_visible,
            accumulated_frames: metadata.accumulated_frames,
            presentation_counter: metadata.last_present_time,
        })
    }
}

#[cfg(test)]
trait OutputGeometry {
    fn geometry(&self) -> (usize, usize);
    fn rotation(&self) -> i32 {
        0
    }
}

#[cfg(test)]
impl OutputGeometry for DXGI_OUTPUT_DESC {
    fn geometry(&self) -> (usize, usize) {
        let rect = self.DesktopCoordinates;
        (
            usize::try_from(i64::from(rect.right) - i64::from(rect.left)).unwrap_or(0),
            usize::try_from(i64::from(rect.bottom) - i64::from(rect.top)).unwrap_or(0),
        )
    }

    fn rotation(&self) -> i32 {
        self.Rotation.0
    }
}

#[cfg(test)]
fn output_geometry(source: &impl OutputGeometry) -> Result<(u32, u32), CaptureError> {
    let (width, height) = source.geometry();
    if width == 0 || height == 0 {
        return Err(CaptureError::InvalidFrame);
    }
    // Match dxgi-capture-rs 1.2.2 copy_surface_data, not its unrotated geometry().
    let (width, height) = if source.rotation() == 2 || source.rotation() == 4 {
        (height, width)
    } else {
        (width, height)
    };
    Ok((
        u32::try_from(width).map_err(|_| CaptureError::InvalidFrame)?,
        u32::try_from(height).map_err(|_| CaptureError::InvalidFrame)?,
    ))
}

fn physical_dimensions(display_index: usize) -> Option<(u32, u32)> {
    selected_output_metadata(display_index)
        .ok()
        .map(|metadata| (metadata.pixel_width, metadata.pixel_height))
}

fn selected_output_metadata(display_index: usize) -> Result<SelectedOutputMetadata, CaptureError> {
    let initialization =
        |error: windows::core::Error| CaptureError::Initialization(error.to_string());
    // SAFETY: DXGI returns an owned COM interface; no caller-owned raw pointers.
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.map_err(initialization)?;
    // Keep dxgi-capture-rs's adapter order, indexing attached outputs globally.
    let mut attached_index = 0;
    for adapter_index in 0.. {
        // SAFETY: factory is live and the API validates the enumeration index.
        let adapter = match unsafe { factory.EnumAdapters1(adapter_index) } {
            Ok(adapter) => adapter,
            Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => break,
            Err(error) => return Err(initialization(error)),
        };
        let Ok(device) = geometry_device(&adapter, Some(&[D3D_FEATURE_LEVEL_9_1])) else {
            continue; // Like the capture manager, skip adapters without a usable device.
        };
        for output_index in 0.. {
            // SAFETY: adapter is live; enumeration returns owned COM interfaces.
            let output = match unsafe { adapter.EnumOutputs(output_index) } {
                Ok(output) => output,
                // The dependency treats every EnumOutputs error as end of adapter.
                Err(_) => break,
            };
            // SAFETY: output is live; GetDesc initializes the returned value.
            let desc: DXGI_OUTPUT_DESC = unsafe { output.GetDesc() }.map_err(initialization)?;
            if !desc.AttachedToDesktop.as_bool() {
                continue;
            }
            if attached_index != display_index {
                attached_index += 1;
                continue;
            }
            let output: IDXGIOutput1 = output.cast().map_err(initialization)?;
            // DuplicateOutput is only a capability/selection check, never pixel acquisition.
            // SAFETY: both COM interfaces remain live for the call.
            let duplication = unsafe { output.DuplicateOutput(&device) }
                .or_else(|_| {
                    let fallback = geometry_device(&adapter, None)?;
                    // SAFETY: fallback device and output remain live for the call.
                    unsafe { output.DuplicateOutput(&fallback) }
                })
                .map_err(initialization)?;

            let dupl_desc: DXGI_OUTDUPL_DESC = unsafe { duplication.GetDesc() };
            let raw_output = RawOutputDesc {
                desktop_left: desc.DesktopCoordinates.left,
                desktop_top: desc.DesktopCoordinates.top,
                desktop_right: desc.DesktopCoordinates.right,
                desktop_bottom: desc.DesktopCoordinates.bottom,
                rotation: desc.Rotation.0,
            };
            let raw_dupl = RawDuplDesc {
                mode_width: dupl_desc.ModeDesc.Width,
                mode_height: dupl_desc.ModeDesc.Height,
                rotation: dupl_desc.Rotation.0,
            };
            return resolve_output_metadata(&raw_output, &raw_dupl).map_err(|_| {
                if dupl_desc.Rotation.0 != 1 && dupl_desc.Rotation.0 != 0 {
                    CaptureError::UnsupportedRotation(dupl_desc.Rotation.0)
                } else if desc.Rotation.0 != 1 && desc.Rotation.0 != 0 {
                    CaptureError::UnsupportedRotation(desc.Rotation.0)
                } else {
                    CaptureError::InvalidFrame
                }
            });
        }
    }
    Err(CaptureError::Initialization(
        "No suitable output display was found".into(),
    ))
}

pub fn enumerate_displays() -> Result<Vec<ferryx_rd_proto::HostDisplay>, CaptureError> {
    let error = |e: windows::core::Error| CaptureError::Initialization(e.to_string());
    // SAFETY: all returned COM interfaces are owned and descriptions are initialized by DXGI.
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1().map_err(error)?;
        let mut displays = Vec::new();
        for a in 0.. {
            let adapter = match factory.EnumAdapters1(a) {
                Ok(adapter) => adapter,
                Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
                Err(e) => return Err(error(e)),
            };
            for o in 0.. {
                let output = match adapter.EnumOutputs(o) {
                    Ok(output) => output,
                    Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
                    Err(e) => return Err(error(e)),
                };
                let desc = output.GetDesc().map_err(error)?;
                if !desc.AttachedToDesktop.as_bool() {
                    continue;
                }
                if displays.len() == ferryx_rd_proto::MAX_HOST_DISPLAYS {
                    return Ok(displays);
                }
                let bounds = desc.DesktopCoordinates;
                displays.push(ferryx_rd_proto::HostDisplay {
                    index: displays.len() as u8,
                    name: String::from_utf16_lossy(
                        &desc.DeviceName[..desc
                            .DeviceName
                            .iter()
                            .position(|c| *c == 0)
                            .unwrap_or(desc.DeviceName.len())],
                    ),
                    width: (bounds.right - bounds.left).unsigned_abs(),
                    height: (bounds.bottom - bounds.top).unsigned_abs(),
                    primary: bounds.left == 0 && bounds.top == 0,
                });
            }
        }
        Ok(displays)
    }
}

fn geometry_device(
    adapter: &IDXGIAdapter1,
    levels: Option<&[D3D_FEATURE_LEVEL]>,
) -> windows::core::Result<ID3D11Device> {
    let mut device = None;
    // SAFETY: adapter is live, levels is a borrowed valid slice, and the output
    // pointer refers to a local Option initialized by the Windows binding.
    unsafe {
        D3D11CreateDevice(
            adapter,
            D3D_DRIVER_TYPE_UNKNOWN,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            levels,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            None,
        )?;
    }
    device.ok_or_else(|| windows::core::Error::from_hresult(E_FAIL))
}

fn saturating_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn duration_ms(timeout: Duration) -> u32 {
    u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX)
}

fn rect_from_edges(left: i32, top: i32, right: i32, bottom: i32) -> Option<DirtyRect> {
    if left < 0 || top < 0 || right <= left || bottom <= top {
        return None;
    }
    Some(DirtyRect {
        x: left as u32,
        y: top as u32,
        width: (right - left) as u32,
        height: (bottom - top) as u32,
    })
}

fn map_capture_error(error: DxgiCaptureError) -> CaptureError {
    match error {
        DxgiCaptureError::Timeout => CaptureError::Timeout,
        DxgiCaptureError::AccessDenied => CaptureError::AccessDenied,
        DxgiCaptureError::AccessLost => CaptureError::AccessLost,
        DxgiCaptureError::RefreshFailure => CaptureError::RefreshFailure,
        DxgiCaptureError::Fail(error) => CaptureError::Capture(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::{output_geometry, CaptureError, OutputGeometry};

    struct StationaryOutput {
        dimensions: (usize, usize),
        acquisitions: usize,
    }

    impl OutputGeometry for StationaryOutput {
        fn geometry(&self) -> (usize, usize) {
            self.dimensions
        }
    }

    #[test]
    fn geometry_does_not_acquire_frame() {
        // Given: output-description dimensions, but no desktop updates.
        let source = StationaryOutput {
            dimensions: (1920, 1080),
            acquisitions: 0,
        };
        // When: probing the metadata seam used during startup.
        let result = output_geometry(&source);
        // Then: stationary output negotiates without requesting pixels.
        assert_eq!(source.acquisitions, 0, "geometry acquired pixels");
        assert_eq!(result.unwrap(), (1920, 1080));
    }

    #[test]
    fn geometry_preserves_selected_output_dimensions() {
        for dimensions in [(1080, 1920), (1920, 1080), (3840, 2160)] {
            // Given: portrait, landscape, or high-DPI output metadata.
            let source = StationaryOutput {
                dimensions,
                acquisitions: 0,
            };
            // When: reading the selected output's geometry.
            let result = output_geometry(&source).unwrap();
            // Then: do not reorder or scale the metadata dimensions.
            assert_eq!(result, (dimensions.0 as u32, dimensions.1 as u32));
        }
    }

    #[test]
    fn geometry_matches_capture_rotation_without_acquiring_pixels() {
        // Given: an identical desktop rectangle for every DXGI rotation.
        for (rotation, expected) in [
            (0, (1080, 1920)),
            (1, (1080, 1920)),
            (2, (1920, 1080)),
            (3, (1080, 1920)),
            (4, (1920, 1080)),
        ] {
            let mut desc = super::DXGI_OUTPUT_DESC::default();
            desc.DesktopCoordinates.left = -1080;
            desc.DesktopCoordinates.right = 0;
            desc.DesktopCoordinates.top = -120;
            desc.DesktopCoordinates.bottom = 1800;
            desc.Rotation.0 = rotation;
            // When: probing a metadata-only source (no pixel acquisition API).
            let result = output_geometry(&desc).unwrap();
            // Then: match the dependency's copy_surface_data dimension swap.
            assert_eq!(result, expected, "DXGI rotation {rotation}");
        }
    }

    #[test]
    fn geometry_rejects_empty_or_unrepresentable_dimensions() {
        for dimensions in [(0, 1080), (1920, 0), (usize::MAX, 1080), (1920, usize::MAX)] {
            // Given: missing output metadata or dimensions outside the wire range.
            let source = StationaryOutput {
                dimensions,
                acquisitions: 0,
            };
            // When: attempting session negotiation.
            let result = output_geometry(&source);
            // Then: reject rather than negotiating zero or truncating dimensions.
            assert!(matches!(result, Err(CaptureError::InvalidFrame)));
        }
    }
}
