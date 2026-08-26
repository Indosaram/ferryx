//! Error types and foreign result conversion for the native terminal adapter.

use std::ffi::c_int;

use super::sys::types::{
    GHOSTTY_INVALID_VALUE, GHOSTTY_IO_ERROR, GHOSTTY_LIMIT_EXCEEDED, GHOSTTY_NO_VALUE,
    GHOSTTY_OUT_OF_MEMORY, GHOSTTY_OUT_OF_SPACE, GHOSTTY_REJECTED, GHOSTTY_SUCCESS,
};

/// Typed errors emitted by the native terminal engine.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NativeTerminalError {
    #[error("Foreign memory allocation failure (OutOfMemory)")]
    OutOfMemory,

    #[error("Invalid value or parameter: {0}")]
    InvalidValue(String),

    #[error("Output buffer out of space")]
    OutOfSpace,

    #[error("Requested value does not exist (NoValue)")]
    NoValue,

    #[error("I/O error across FFI boundary: {0}")]
    IoError(String),

    #[error("Encoded input exceeded configured limit")]
    LimitExceeded,

    #[error("Operation rejected by security policy: {0}")]
    Rejected(String),

    #[error("Invalid terminal dimensions: cols={0}, rows={1} (both must be > 0)")]
    InvalidDimensions(u16, u16),

    #[error("Invalid UTF-8 sequence returned from native terminal: {0}")]
    InvalidUtf8(String),

    #[error("Unexpected foreign enum/error code across ABI: {0}")]
    ForeignErrorCode(i32),

    #[error("GPU adapter request failed: {0}")]
    GpuAdapterUnavailable(String),

    #[error("GPU device request failed: {0}")]
    GpuDeviceUnavailable(String),

    #[error("GPU render pipeline error: {0}")]
    GpuPipelineError(String),

    #[error("GPU buffer or readback error: {0}")]
    GpuBufferError(String),
}

impl NativeTerminalError {
    /// Maps a raw C ABI return code (`c_int`) to `Result<(), NativeTerminalError>`.
    pub fn from_c_result(res: c_int, context: &'static str) -> Result<(), Self> {
        match res {
            GHOSTTY_SUCCESS => Ok(()),
            GHOSTTY_OUT_OF_MEMORY => Err(Self::OutOfMemory),
            GHOSTTY_INVALID_VALUE => Err(Self::InvalidValue(format!("Invalid value in {context}"))),
            GHOSTTY_OUT_OF_SPACE => Err(Self::OutOfSpace),
            GHOSTTY_NO_VALUE => Err(Self::NoValue),
            GHOSTTY_IO_ERROR => Err(Self::IoError(format!("I/O error in {context}"))),
            GHOSTTY_LIMIT_EXCEEDED => Err(Self::LimitExceeded),
            GHOSTTY_REJECTED => Err(Self::Rejected(format!("Rejected in {context}"))),
            other => Err(Self::ForeignErrorCode(other)),
        }
    }

    /// Decodes a raw foreign byte into a safe Rust `bool`, rejecting invalid representations.
    pub fn decode_c_bool(val: u8, context: &'static str) -> Result<bool, Self> {
        match val {
            0 => Ok(false),
            1 => Ok(true),
            other => Err(Self::InvalidValue(format!(
                "Invalid foreign boolean byte {other:#x} in {context}"
            ))),
        }
    }
}
