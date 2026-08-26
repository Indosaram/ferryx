use super::error::NativeTerminalError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SurfaceFrameAction {
    Drop,
}

pub(crate) fn classify_surface_error(
    error: wgpu::SurfaceError,
) -> Result<SurfaceFrameAction, NativeTerminalError> {
    match error {
        wgpu::SurfaceError::Timeout => Ok(SurfaceFrameAction::Drop),
        wgpu::SurfaceError::OutOfMemory => Err(NativeTerminalError::OutOfMemory),
        wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated => Err(
            NativeTerminalError::GpuPipelineError("Native terminal surface recovery failed".into()),
        ),
        other => Err(NativeTerminalError::GpuPipelineError(other.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::{classify_surface_error, SurfaceFrameAction};

    #[test]
    fn native_surface_timeout_drops_frame_without_terminal_failure() {
        assert_eq!(
            classify_surface_error(wgpu::SurfaceError::Timeout),
            Ok(SurfaceFrameAction::Drop)
        );
    }
}
