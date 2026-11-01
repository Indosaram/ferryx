use ferryx_rd_proto::InputEventType;
use std::ffi::{c_char, c_void, CStr};

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FerryxRdMobileStatus {
    Ok = 0,
    InvalidParam = 1,
    SessionNotFound = 2,
    NetworkError = 3,
    InternalError = 4,
    BackendUnavailable = 5,
}

pub struct MobileSessionHandle {
    pub host: String,
    pub port: u16,
}

/// Creates a mobile session allocation.
///
/// # Safety
/// - `out_handle` must be a valid, aligned, non-null pointer to a writable `*mut c_void` slot.
/// - If non-null, `host` must point to a valid null-terminated C string that lives at least for the duration of this call.
/// - The caller assumes ownership of the returned handle on success and must eventually destroy it exactly once via `ferryx_rd_mobile_destroy`.
#[no_mangle]
pub unsafe extern "C" fn ferryx_rd_mobile_create(
    host: *const c_char,
    port: u16,
    out_handle: *mut *mut c_void,
) -> FerryxRdMobileStatus {
    if out_handle.is_null() {
        return FerryxRdMobileStatus::InvalidParam;
    }

    // SAFETY: out_handle is non-null and the caller contract requires a writable pointer slot.
    unsafe {
        *out_handle = std::ptr::null_mut();
    }

    if host.is_null() || port == 0 {
        return FerryxRdMobileStatus::InvalidParam;
    }

    // SAFETY: host is non-null and the caller contract requires a valid null-terminated C string.
    let c_str = unsafe { CStr::from_ptr(host) };
    let host_str = match c_str.to_str() {
        Ok(s) => s,
        Err(_) => return FerryxRdMobileStatus::InvalidParam,
    };

    if host_str.trim().is_empty() {
        return FerryxRdMobileStatus::InvalidParam;
    }

    let session = Box::new(MobileSessionHandle {
        host: host_str.to_string(),
        port,
    });

    // SAFETY: out_handle was verified non-null and initialized.
    unsafe {
        *out_handle = Box::into_raw(session) as *mut c_void;
    }
    FerryxRdMobileStatus::Ok
}

/// Destroys a previously allocated mobile session handle.
///
/// # Safety
/// - If non-null, `handle` must be an exclusively owned pointer returned by `ferryx_rd_mobile_create` that has not been previously destroyed.
/// - After destruction, the handle must never be accessed or destroyed again.
#[no_mangle]
pub unsafe extern "C" fn ferryx_rd_mobile_destroy(handle: *mut c_void) -> FerryxRdMobileStatus {
    if handle.is_null() {
        return FerryxRdMobileStatus::SessionNotFound;
    }
    // SAFETY: handle was produced by Box::into_raw in ferryx_rd_mobile_create and caller guarantees single destruction.
    let _ = unsafe { Box::from_raw(handle as *mut MobileSessionHandle) };
    FerryxRdMobileStatus::Ok
}

/// Sends normalized touch input through a mobile session handle.
///
/// # Safety
/// - `handle` must point to an active, valid `MobileSessionHandle` returned by `ferryx_rd_mobile_create` that has not been destroyed.
/// - Concurrent access to the same handle is not synchronized across C ABI callers.
#[no_mangle]
pub unsafe extern "C" fn ferryx_rd_mobile_send_touch(
    handle: *mut c_void,
    event_type: u8,
    norm_x: f32,
    norm_y: f32,
) -> FerryxRdMobileStatus {
    if handle.is_null() {
        return FerryxRdMobileStatus::SessionNotFound;
    }
    if !norm_x.is_finite()
        || !norm_y.is_finite()
        || !(0.0..=1.0).contains(&norm_x)
        || !(0.0..=1.0).contains(&norm_y)
    {
        return FerryxRdMobileStatus::InvalidParam;
    }

    if InputEventType::try_from(event_type).is_err() {
        return FerryxRdMobileStatus::InvalidParam;
    }

    FerryxRdMobileStatus::BackendUnavailable
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn c_abi_null_pointer_safety() {
        unsafe {
            assert_eq!(
                ferryx_rd_mobile_create(std::ptr::null(), 19730, std::ptr::null_mut()),
                FerryxRdMobileStatus::InvalidParam
            );
            assert_eq!(
                ferryx_rd_mobile_destroy(std::ptr::null_mut()),
                FerryxRdMobileStatus::SessionNotFound
            );
            assert_eq!(
                ferryx_rd_mobile_send_touch(std::ptr::null_mut(), 1, 0.5, 0.5),
                FerryxRdMobileStatus::SessionNotFound
            );
        }
    }

    #[test]
    fn c_abi_lifecycle_and_backend_unavailability() {
        let host = CString::new("127.0.0.1").unwrap();
        let mut handle = std::ptr::null_mut();

        unsafe {
            let status = ferryx_rd_mobile_create(host.as_ptr(), 19730, &mut handle);
            assert_eq!(status, FerryxRdMobileStatus::Ok);
            assert!(!handle.is_null());

            let touch_status =
                ferryx_rd_mobile_send_touch(handle, InputEventType::LeftMouseDown as u8, 0.5, 0.5);
            assert_eq!(touch_status, FerryxRdMobileStatus::BackendUnavailable);

            let invalid_coord_status =
                ferryx_rd_mobile_send_touch(handle, InputEventType::LeftMouseDown as u8, -1.0, 0.5);
            assert_eq!(invalid_coord_status, FerryxRdMobileStatus::InvalidParam);

            let destroy_status = ferryx_rd_mobile_destroy(handle);
            assert_eq!(destroy_status, FerryxRdMobileStatus::Ok);
        }
    }
}
