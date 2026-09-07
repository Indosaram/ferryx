use super::error::NativeTerminalError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SurfaceFrameAction {
    Drop,
}

pub(crate) fn classify_surface_error(
    status: &wgpu::CurrentSurfaceTexture,
) -> Result<SurfaceFrameAction, NativeTerminalError> {
    match status {
        wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
            Ok(SurfaceFrameAction::Drop)
        }
        wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => Err(
            NativeTerminalError::GpuPipelineError("Native terminal surface recovery failed".into()),
        ),
        wgpu::CurrentSurfaceTexture::Validation => {
            Err(NativeTerminalError::GpuPipelineError("Surface validation error".into()))
        }
        wgpu::CurrentSurfaceTexture::Success(_) | wgpu::CurrentSurfaceTexture::Suboptimal(_) => {
            Ok(SurfaceFrameAction::Drop)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{classify_surface_error, SurfaceFrameAction};

    #[test]
    fn native_surface_timeout_drops_frame_without_terminal_failure() {
        assert_eq!(
            classify_surface_error(&wgpu::CurrentSurfaceTexture::Timeout),
            Ok(SurfaceFrameAction::Drop)
        );
    }

    #[test]
    fn native_surface_occluded_drops_frame_without_terminal_failure() {
        assert_eq!(
            classify_surface_error(&wgpu::CurrentSurfaceTexture::Occluded),
            Ok(SurfaceFrameAction::Drop)
        );
    }
}
