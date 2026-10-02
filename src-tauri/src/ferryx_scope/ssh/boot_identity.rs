#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BootIdSource {
    System,
    Injected(String),
}

impl Default for BootIdSource {
    fn default() -> Self {
        Self::System
    }
}

impl BootIdSource {
    pub fn resolve(&self) -> Result<String, String> {
        match self {
            Self::System => detect_os_boot_id(),
            Self::Injected(id) => validate_boot_id(id),
        }
    }
}

pub fn validate_boot_id(id: &str) -> Result<String, String> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        return Err("BOOT_ID_INVALID: boot id cannot be empty".into());
    }
    if trimmed.len() > 256 {
        return Err("BOOT_ID_INVALID: boot id exceeds maximum length".into());
    }
    if !trimmed.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b':' | b'.')) {
        return Err("BOOT_ID_INVALID: boot id contains illegal characters".into());
    }
    Ok(trimmed.to_string())
}

pub fn detect_os_boot_id() -> Result<String, String> {
    #[cfg(target_os = "linux")]
    {
        detect_linux_boot_id()
    }
    #[cfg(target_os = "macos")]
    {
        detect_macos_boot_id()
    }
    #[cfg(windows)]
    {
        detect_windows_boot_id()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        Err("BOOT_ID_UNAVAILABLE: platform not supported for OS boot identity".into())
    }
}

#[cfg(target_os = "linux")]
fn detect_linux_boot_id() -> Result<String, String> {
    let content = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .map_err(|e| format!("BOOT_ID_UNAVAILABLE: cannot read Linux boot UUID: {e}"))?;
    validate_boot_id(&content)
}

#[cfg(target_os = "macos")]
fn detect_macos_boot_id() -> Result<String, String> {
    let mut buffer = [0u8; 128];
    let mut size = buffer.len();
    // SAFETY: the name is NUL-terminated; sysctl receives a live writable buffer with its exact capacity.
    let res = unsafe {
        libc::sysctlbyname(
            b"kern.bootsessionuuid\0".as_ptr().cast(),
            buffer.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if res != 0 {
        return Err("BOOT_ID_UNAVAILABLE: sysctl kern.bootsessionuuid failed".into());
    }
    if size == 0 || size > buffer.len() {
        return Err("BOOT_ID_UNAVAILABLE: invalid boot UUID length".into());
    }
    let id = std::str::from_utf8(&buffer[..size]).map_err(|e| e.to_string())?;
    validate_boot_id(id.trim_end_matches('\0'))
}

#[cfg(windows)]
fn detect_windows_boot_id() -> Result<String, String> {
    #[repr(C)]
    struct SystemBootEnvironmentInformation {
        boot_identifier: [u8; 16],
        firmware_type: u32,
        boot_flags: u64,
    }

    #[link(name = "ntdll")]
    extern "system" {
        fn NtQuerySystemInformation(
            class: u32,
            information: *mut std::ffi::c_void,
            length: u32,
            return_length: *mut u32,
        ) -> i32;
    }

    let mut boot_env = SystemBootEnvironmentInformation {
        boot_identifier: [0u8; 16],
        firmware_type: 0,
        boot_flags: 0,
    };
    let mut ret_len = 0u32;
    // SAFETY: this initialized repr(C) buffer matches SYSTEM_BOOT_ENVIRONMENT_INFORMATION and is exclusively borrowed.
    let status = unsafe {
        NtQuerySystemInformation(
            90,
            &mut boot_env as *mut _ as *mut std::ffi::c_void,
            std::mem::size_of::<SystemBootEnvironmentInformation>() as u32,
            &mut ret_len,
        )
    };
    if status == 0 && ret_len as usize == std::mem::size_of::<SystemBootEnvironmentInformation>()
        && boot_env.boot_identifier != [0u8; 16]
    {
        let b = boot_env.boot_identifier;
        let id_str = format!(
            "win-boot-{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            b[3], b[2], b[1], b[0], b[5], b[4], b[7], b[6], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
        );
        return validate_boot_id(&id_str);
    }

    Err(format!(
        "BOOT_ID_UNAVAILABLE: kernel BootIdentifier query failed (status={status:#x}, length={ret_len})"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boot_id_injected_source_resolves() {
        let source = BootIdSource::Injected("boot-uuid-1234".into());
        assert_eq!(source.resolve().unwrap(), "boot-uuid-1234");
    }

    #[test]
    fn boot_id_injected_empty_fails() {
        let source = BootIdSource::Injected("   ".into());
        assert!(source.resolve().is_err());
    }

    #[test]
    fn boot_id_validation_invariants() {
        assert!(validate_boot_id("valid-boot-id.123_456:789").is_ok());
        assert!(validate_boot_id("").is_err());
        assert!(validate_boot_id("   ").is_err());
        assert!(validate_boot_id("bad;command").is_err());
        assert!(validate_boot_id("bad\nnewline").is_err());
        assert!(validate_boot_id("bad/path").is_err());
        assert!(validate_boot_id(&"a".repeat(257)).is_err());
    }

    #[test]
    fn current_platform_boot_id_resolves_or_fails_closed() {
        let result = detect_os_boot_id();
        assert!(result.is_ok(), "OS boot id should resolve on host: {:?}", result);
        let id = result.unwrap();
        assert!(validate_boot_id(&id).is_ok());
    }
}
