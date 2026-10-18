#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct OwnedProcessIdentity {
    pub(super) executable: String,
    pub(super) process_start: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum IdentityUnavailable {
    InvalidPid,
    UnsupportedPlatform,
    Platform(String),
    ProcessStartChanged,
    InvalidIdentity,
}

impl std::fmt::Display for IdentityUnavailable {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPid => formatter.write_str("invalid exact PID"),
            Self::UnsupportedPlatform => formatter.write_str("unsupported platform exact process identity"),
            Self::Platform(reason) => formatter.write_str(reason),
            Self::ProcessStartChanged => formatter.write_str("process start identity changed"),
            Self::InvalidIdentity => formatter.write_str("empty executable or process start token"),
        }
    }
}

impl From<String> for IdentityUnavailable {
    fn from(reason: String) -> Self { Self::Platform(reason) }
}

impl From<&str> for IdentityUnavailable {
    fn from(reason: &str) -> Self { Self::Platform(reason.into()) }
}

#[cfg(target_os = "linux")]
mod platform {
    fn start(pid: u32) -> Result<String, super::IdentityUnavailable> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .map_err(|error| super::IdentityUnavailable::Platform(error.to_string()))?;
        let (_, rest) = stat.rsplit_once(')').ok_or(super::IdentityUnavailable::InvalidIdentity)?;
        let mut fields = rest.split_whitespace();
        let state = fields.next().ok_or(super::IdentityUnavailable::InvalidIdentity)?;
        if matches!(state, "Z" | "X" | "x") { return Err(super::IdentityUnavailable::Platform("process exited".into())); }
        let token = fields.nth(18).ok_or(super::IdentityUnavailable::InvalidIdentity)?;
        token.parse::<u64>().map_err(|error| super::IdentityUnavailable::Platform(error.to_string()))?;
        Ok(token.to_string())
    }

    pub(super) fn capture(pid: u32) -> Result<(String, String), super::IdentityUnavailable> {
        if pid == 0 { return Err(super::IdentityUnavailable::InvalidPid); }
        let before = start(pid)?;
        let executable = std::fs::canonicalize(format!("/proc/{pid}/exe"))
            .map_err(|error| super::IdentityUnavailable::Platform(error.to_string()))?
            .into_os_string().into_string().map_err(|_| super::IdentityUnavailable::InvalidIdentity)?;
        if before != start(pid)? { return Err(super::IdentityUnavailable::ProcessStartChanged); }
        Ok((executable, before))
    }
}

#[cfg(windows)]
mod platform {
    pub(super) fn capture(pid: u32) -> Result<(String, String), super::IdentityUnavailable> {
        if pid == 0 { return Err(super::IdentityUnavailable::InvalidPid); }
        let script = format!(
            "$p=Get-Process -Id {pid} -ErrorAction Stop; @{{executable=$p.Path;processStart=$p.StartTime.ToUniversalTime().Ticks.ToString()}}|ConvertTo-Json -Compress"
        );
        let output = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .output().map_err(|error| error.to_string())?;
        if !output.status.success() {
            return Err(super::IdentityUnavailable::Platform(format!("exact PID probe failed: {}", String::from_utf8_lossy(&output.stderr))));
        }
        let value: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|error| super::IdentityUnavailable::Platform(error.to_string()))?;
        let executable = value["executable"].as_str().filter(|value| !value.is_empty())
            .ok_or(super::IdentityUnavailable::Platform("exact PID executable unavailable".into()))?;
        let start = value["processStart"].as_str().filter(|value| !value.is_empty())
            .ok_or(super::IdentityUnavailable::Platform("exact PID start token unavailable".into()))?;
        start.parse::<u64>().map_err(|error| super::IdentityUnavailable::Platform(error.to_string()))?;
        Ok((executable.into(), start.into()))
    }
}

#[cfg(target_os = "macos")]
mod platform {
    fn start(pid: u32) -> Result<String, super::IdentityUnavailable> {
        // SAFETY: proc_bsdinfo is a C POD; the writable buffer is aligned and
        // sized for that type, and is read only after an exact-size result.
        let mut info = unsafe { std::mem::zeroed::<libc::proc_bsdinfo>() };
        let size = std::mem::size_of_val(&info) as i32;
        let read = unsafe {
            libc::proc_pidinfo(pid as i32, libc::PROC_PIDTBSDINFO, 0,
                (&mut info as *mut libc::proc_bsdinfo).cast(), size)
        };
        if read != size { return Err(super::IdentityUnavailable::Platform(std::io::Error::last_os_error().to_string())); }
        if info.pbi_pid != pid || info.pbi_status == libc::SZOMB as u32 {
            return Err(super::IdentityUnavailable::Platform("exact process exited or changed PID".into()));
        }
        darwin_start_token(info.pbi_start_tvsec, info.pbi_start_tvusec).map_err(super::IdentityUnavailable::Platform)
    }

    pub(super) fn darwin_start_token(seconds: u64, microseconds: u64) -> Result<String, String> {
        if microseconds >= 1_000_000 { return Err("invalid Darwin start microseconds".into()); }
        Ok(format!("{seconds}:{microseconds:06}"))
    }

    pub(super) fn capture(pid: u32) -> Result<(String, String), super::IdentityUnavailable> {
        if pid == 0 || pid > i32::MAX as u32 { return Err(super::IdentityUnavailable::InvalidPid); }
        let before = start(pid)?;
        let mut buffer = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        // SAFETY: libproc receives a live byte buffer with its actual capacity;
        // no pointer survives this call and returned bytes are bounds checked.
        let count = unsafe { libc::proc_pidpath(pid as i32, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
        if count <= 0 { return Err(super::IdentityUnavailable::Platform(std::io::Error::last_os_error().to_string())); }
        let bytes = buffer.get(..count as usize).ok_or(super::IdentityUnavailable::InvalidIdentity)?;
        let end = bytes.iter().position(|byte| *byte == 0).unwrap_or(bytes.len());
        let path = std::str::from_utf8(&bytes[..end]).map_err(|error| super::IdentityUnavailable::Platform(error.to_string()))?;
        if path.is_empty() { return Err(super::IdentityUnavailable::InvalidIdentity); }
        let executable = std::fs::canonicalize(path).map_err(|error| super::IdentityUnavailable::Platform(error.to_string()))?
            .into_os_string().into_string().map_err(|_| super::IdentityUnavailable::InvalidIdentity)?;
        if before != start(pid)? { return Err(super::IdentityUnavailable::ProcessStartChanged); }
        Ok((executable, before))
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn full_start_token_preserves_microseconds() {
            assert_eq!(super::darwin_start_token(1700000000, 7).expect("token"), "1700000000:000007");
            assert_ne!(super::darwin_start_token(1, 7).expect("token"), super::darwin_start_token(1, 8).expect("token"));
            assert!(super::darwin_start_token(1, 1_000_000).is_err());
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod platform {
    pub(super) fn capture(_pid: u32) -> Result<(String, String), super::IdentityUnavailable> {
        Err(super::IdentityUnavailable::UnsupportedPlatform)
    }
}

pub(super) fn capture(pid: u32) -> Result<OwnedProcessIdentity, IdentityUnavailable> {
    capture_with(pid, platform::capture)
}

fn capture_with(
    pid: u32,
    read: impl FnOnce(u32) -> Result<(String, String), IdentityUnavailable>,
) -> Result<OwnedProcessIdentity, IdentityUnavailable> {
    if pid == 0 { return Err(IdentityUnavailable::InvalidPid); }
    let (executable, process_start) = read(pid)?;
    validate_identity(OwnedProcessIdentity { executable, process_start })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn injected_identity_requires_nonempty_exact_fields() {
        let identity = OwnedProcessIdentity {
            executable: "/owned/shell".into(),
            process_start: "1700000000:000007".into(),
        };
        assert!(validate_identity(identity.clone()).is_ok());
        assert_eq!(identity.executable, "/owned/shell");
        assert_eq!(identity.process_start, "1700000000:000007");
        assert_eq!(validate_identity(OwnedProcessIdentity { executable: String::new(), ..identity.clone() }), Err(IdentityUnavailable::InvalidIdentity));
        assert_eq!(validate_identity(OwnedProcessIdentity { process_start: String::new(), ..identity }), Err(IdentityUnavailable::InvalidIdentity));
    }

    #[test]
    fn capture_forwards_only_the_exact_spawn_pid() {
        let mut received_pid = None;
        let identity = capture_with(73, |pid| {
            received_pid = Some(pid);
            Ok(("/owned/shell".into(), "1700000000:000007".into()))
        }).expect("injected exact identity");
        assert_eq!(received_pid, Some(73));
        assert_eq!(identity.executable, "/owned/shell");
        assert_eq!(identity.process_start, "1700000000:000007");
        assert_eq!(capture_with(0, |_| panic!("invalid PID must not invoke reader")), Err(IdentityUnavailable::InvalidPid));
        assert_eq!(capture_with(73, |_| Err(IdentityUnavailable::UnsupportedPlatform)), Err(IdentityUnavailable::UnsupportedPlatform));
    }
}

fn validate_identity(identity: OwnedProcessIdentity) -> Result<OwnedProcessIdentity, IdentityUnavailable> {
    if !std::path::Path::new(&identity.executable).is_absolute() || identity.process_start.is_empty() {
        Err(IdentityUnavailable::InvalidIdentity)
    } else {
        Ok(identity)
    }
}
