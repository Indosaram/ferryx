//! Private raw ABI definitions and safe query helpers for libghostty-vt build info.

extern crate ferryx_lib as _;

use std::ffi::{c_int, c_void};
use std::fmt;

macro_rules! impl_error_display {
    ($err:ident, $msg:literal) => {
        #[derive(Copy, Clone, Debug, PartialEq, Eq)]
        pub struct $err(pub c_int);
        impl fmt::Display for $err {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!($msg, ": {}"), self.0)
            }
        }
        impl std::error::Error for $err {}
    };
}

#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum GhosttyResult {
    Success = 0,
    OutOfMemory = -1,
    InvalidValue = -2,
    OutOfSpace = -3,
    NoValue = -4,
    IoError = -5,
    LimitExceeded = -6,
    Rejected = -7,
}
impl_error_display!(UnknownResultCodeError, "unknown ghostty result code");

impl TryFrom<c_int> for GhosttyResult {
    type Error = UnknownResultCodeError;
    fn try_from(raw: c_int) -> Result<Self, Self::Error> {
        match raw {
            0 => Ok(Self::Success),
            -1 => Ok(Self::OutOfMemory),
            -2 => Ok(Self::InvalidValue),
            -3 => Ok(Self::OutOfSpace),
            -4 => Ok(Self::NoValue),
            -5 => Ok(Self::IoError),
            -6 => Ok(Self::LimitExceeded),
            -7 => Ok(Self::Rejected),
            other => Err(UnknownResultCodeError(other)),
        }
    }
}

#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum GhosttyOptimizeMode {
    Debug = 0,
    ReleaseSafe = 1,
    ReleaseSmall = 2,
    ReleaseFast = 3,
}
impl_error_display!(UnknownOptimizeModeError, "unknown ghostty optimize mode");

impl TryFrom<c_int> for GhosttyOptimizeMode {
    type Error = UnknownOptimizeModeError;
    fn try_from(raw: c_int) -> Result<Self, Self::Error> {
        match raw {
            0 => Ok(Self::Debug),
            1 => Ok(Self::ReleaseSafe),
            2 => Ok(Self::ReleaseSmall),
            3 => Ok(Self::ReleaseFast),
            other => Err(UnknownOptimizeModeError(other)),
        }
    }
}

#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum GhosttyBuildInfo {
    Invalid = 0,
    Simd = 1,
    KittyGraphics = 2,
    TmuxControlMode = 3,
    Optimize = 4,
    VersionString = 5,
    VersionMajor = 6,
    VersionMinor = 7,
    VersionPatch = 8,
    VersionPre = 9,
    VersionBuild = 10,
}
impl_error_display!(UnknownBuildInfoKeyError, "unknown ghostty build info key");

impl GhosttyBuildInfo {
    #[inline]
    pub fn as_raw_c_int(self) -> c_int {
        self as c_int
    }
}

impl TryFrom<c_int> for GhosttyBuildInfo {
    type Error = UnknownBuildInfoKeyError;
    fn try_from(raw: c_int) -> Result<Self, Self::Error> {
        match raw {
            0 => Ok(Self::Invalid),
            1 => Ok(Self::Simd),
            2 => Ok(Self::KittyGraphics),
            3 => Ok(Self::TmuxControlMode),
            4 => Ok(Self::Optimize),
            5 => Ok(Self::VersionString),
            6 => Ok(Self::VersionMajor),
            7 => Ok(Self::VersionMinor),
            8 => Ok(Self::VersionPatch),
            9 => Ok(Self::VersionPre),
            10 => Ok(Self::VersionBuild),
            other => Err(UnknownBuildInfoKeyError(other)),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct InvalidCBoolError(pub u8);
impl fmt::Display for InvalidCBoolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid C bool byte: {:#04x}", self.0)
    }
}
impl std::error::Error for InvalidCBoolError {}

pub fn try_bool_from_c_u8(raw: u8) -> Result<bool, InvalidCBoolError> {
    match raw {
        0 => Ok(false),
        1 => Ok(true),
        other => Err(InvalidCBoolError(other)),
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct GhosttyString {
    pub ptr: *const u8,
    pub len: usize,
}

impl GhosttyString {
    pub const NULL: Self = Self {
        ptr: std::ptr::null(),
        len: 0,
    };

    /// # Safety
    /// If `len > 0`, `ptr` must be non-null, have valid pointer provenance, and point to at least
    /// `len` valid, initialized, immutable bytes for lifetime `'a`.
    pub unsafe fn as_str<'a>(&self) -> Result<&'a str, std::str::Utf8Error> {
        if self.len == 0 {
            return Ok("");
        }
        assert!(!self.ptr.is_null(), "GhosttyString len > 0 with null ptr");
        std::str::from_utf8(std::slice::from_raw_parts(self.ptr, self.len))
    }
}

extern "C" {
    pub fn ghostty_build_info(data: c_int, out: *mut c_void) -> c_int;
}

/// # Safety
/// Callers must ensure `T` and `out` match the exact header-mandated output layout in
/// `include/ghostty/vt/build_info.h` for `key` and provide valid, properly aligned, writable storage.
unsafe fn query_field<T>(
    key: GhosttyBuildInfo,
    out: &mut T,
) -> Result<(), Box<dyn std::error::Error>> {
    let code = ghostty_build_info(key.as_raw_c_int(), out as *mut T as *mut c_void);
    if GhosttyResult::try_from(code)? != GhosttyResult::Success {
        return Err(format!("ghostty_build_info({key:?}) failed: {code}").into());
    }
    Ok(())
}

pub fn safe_ghostty_build_info_bool(
    key: GhosttyBuildInfo,
) -> Result<bool, Box<dyn std::error::Error>> {
    let mut raw: u8 = 0;
    // SAFETY: build_info.h mandates bool* output for SIMD/Kitty/Tmux keys; u8 is valid 1-byte storage.
    unsafe { query_field(key, &mut raw)? };
    Ok(try_bool_from_c_u8(raw)?)
}

pub fn safe_ghostty_build_info_optimize() -> Result<GhosttyOptimizeMode, Box<dyn std::error::Error>>
{
    let mut raw: c_int = -1;
    // SAFETY: build_info.h mandates GhosttyOptimizeMode* (c_int) output for GHOSTTY_BUILD_INFO_OPTIMIZE.
    unsafe { query_field(GhosttyBuildInfo::Optimize, &mut raw)? };
    Ok(GhosttyOptimizeMode::try_from(raw)?)
}

pub fn safe_ghostty_build_info_usize(
    key: GhosttyBuildInfo,
) -> Result<usize, Box<dyn std::error::Error>> {
    let mut val: usize = 0;
    // SAFETY: build_info.h mandates size_t* (usize) output for major/minor/patch version keys.
    unsafe { query_field(key, &mut val)? };
    Ok(val)
}

pub fn safe_ghostty_build_info_string(
    key: GhosttyBuildInfo,
) -> Result<&'static str, Box<dyn std::error::Error>> {
    let mut raw = GhosttyString::NULL;
    // SAFETY: build_info.h mandates GhosttyString* output for string keys; memory points to static constants.
    unsafe {
        query_field(key, &mut raw)?;
        Ok(raw.as_str()?)
    }
}

#[cfg(test)]
#[test]
fn test_fallible_abi_conversions_and_boundary() {
    let codes = [
        (0, GhosttyResult::Success),
        (-1, GhosttyResult::OutOfMemory),
        (-2, GhosttyResult::InvalidValue),
        (-3, GhosttyResult::OutOfSpace),
        (-4, GhosttyResult::NoValue),
        (-5, GhosttyResult::IoError),
        (-6, GhosttyResult::LimitExceeded),
        (-7, GhosttyResult::Rejected),
    ];
    for (v, exp) in codes {
        assert_eq!(GhosttyResult::try_from(v), Ok(exp));
    }
    assert!(
        GhosttyResult::try_from(1).is_err()
            && GhosttyResult::try_from(-8).is_err()
            && GhosttyResult::try_from(999).is_err()
    );
    assert_eq!(try_bool_from_c_u8(0), Ok(false));
    assert_eq!(try_bool_from_c_u8(1), Ok(true));
    assert!(try_bool_from_c_u8(2).is_err() && try_bool_from_c_u8(255).is_err());

    let modes = [
        (0, GhosttyOptimizeMode::Debug),
        (1, GhosttyOptimizeMode::ReleaseSafe),
        (2, GhosttyOptimizeMode::ReleaseSmall),
        (3, GhosttyOptimizeMode::ReleaseFast),
    ];
    for (v, exp) in modes {
        assert_eq!(GhosttyOptimizeMode::try_from(v), Ok(exp));
    }
    assert!(
        GhosttyOptimizeMode::try_from(4).is_err() && GhosttyOptimizeMode::try_from(-1).is_err()
    );

    assert_eq!(GhosttyBuildInfo::try_from(0), Ok(GhosttyBuildInfo::Invalid));
    assert_eq!(GhosttyBuildInfo::try_from(1), Ok(GhosttyBuildInfo::Simd));
    assert_eq!(
        GhosttyBuildInfo::try_from(10),
        Ok(GhosttyBuildInfo::VersionBuild)
    );
    assert!(GhosttyBuildInfo::try_from(11).is_err() && GhosttyBuildInfo::try_from(-1).is_err());

    // SAFETY: Raw integer key 0 (GHOSTTY_BUILD_INFO_INVALID) with null out pointer.
    unsafe {
        let raw = ghostty_build_info(
            GhosttyBuildInfo::Invalid.as_raw_c_int(),
            std::ptr::null_mut(),
        );
        assert_eq!(raw, -2);
        assert_eq!(
            GhosttyResult::try_from(raw),
            Ok(GhosttyResult::InvalidValue)
        );
    }
}
