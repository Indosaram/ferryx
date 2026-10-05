use super::{Observation, Process, SuspensionError, SuspensionTarget};

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use crate::terminal::suspension::ProcessIdentity;
    use std::fs;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

    pub(super) struct PinnedProcess {
        pid: u32,
        fd: OwnedFd,
    }

    fn failure(operation: &'static str, pid: u32, source: std::io::Error) -> SuspensionError {
        match source.raw_os_error() {
            Some(libc::ESRCH) | Some(libc::ENOENT) => SuspensionError::ProcessGone { pid },
            Some(libc::ENOSYS) => SuspensionError::UnsupportedPlatform("Linux pidfd support is required"),
            _ => SuspensionError::PlatformCallFailure { operation, source },
        }
    }

    fn read(path: &str, pid: u32) -> Result<String, SuspensionError> {
        fs::read_to_string(path).map_err(|error| failure("read process identity", pid, error))
    }

    fn malformed(operation: &'static str) -> SuspensionError {
        SuspensionError::PlatformCallFailure {
            operation,
            source: std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid kernel process data"),
        }
    }

    fn parse_stat(stat: &str) -> Result<(u64, bool), SuspensionError> {
        let (_, rest) = stat.rsplit_once(')').ok_or_else(|| malformed("parse /proc stat"))?;
        let fields: Vec<_> = rest.split_whitespace().collect();
        let state = fields.first().ok_or_else(|| malformed("parse process state"))?;
        if matches!(*state, "Z" | "X" | "x") {
            return Err(malformed("observe exited process"));
        }
        let start = fields.get(19).ok_or_else(|| malformed("parse process start"))?
            .parse::<u64>().map_err(|_| malformed("parse process start"))?;
        Ok((start, *state == "T"))
    }

    fn parse_incarnation(environment: &[u8]) -> Result<Option<String>, SuspensionError> {
        let prefix = b"FERRYX_PTY_INCARNATION=";
        let mut found = None;
        for entry in environment.split(|byte| *byte == 0) {
            if let Some(value) = entry.strip_prefix(prefix) {
                if found.is_some() {
                    return Err(malformed("duplicate process incarnation"));
                }
                found = Some(std::str::from_utf8(value).map_err(|_| malformed("parse process incarnation"))?.to_owned());
            }
        }
        Ok(found)
    }

    impl PinnedProcess {
        fn ensure_alive(&self) -> Result<(), SuspensionError> {
            let mut descriptor = libc::pollfd { fd: self.fd.as_raw_fd(), events: libc::POLLIN, revents: 0 };
            let result = unsafe { libc::poll(&mut descriptor, 1, 0) };
            if result < 0 {
                return Err(failure("poll pidfd", self.pid, std::io::Error::last_os_error()));
            }
            if result != 0 { return Err(SuspensionError::ProcessGone { pid: self.pid }); }
            Ok(())
        }

        fn signal(&self, signal: i32) -> Result<(), SuspensionError> {
            let result = unsafe {
                libc::syscall(libc::SYS_pidfd_send_signal, self.fd.as_raw_fd(), signal, std::ptr::null::<libc::siginfo_t>(), 0u32)
            };
            if result != 0 {
                return Err(failure("pidfd_send_signal", self.pid, std::io::Error::last_os_error()));
            }
            Ok(())
        }

        fn wait_stop(&self, nonblocking: bool) -> Result<libc::siginfo_t, SuspensionError> {
            let mut info = unsafe { std::mem::zeroed::<libc::siginfo_t>() };
            // P_PIDFD is Linux idtype 3. WNOWAIT preserves the daemon's child events.
            let flags = libc::WSTOPPED | libc::WNOWAIT | if nonblocking { libc::WNOHANG } else { 0 };
            let result = unsafe { libc::waitid(3 as libc::idtype_t, self.fd.as_raw_fd() as libc::id_t, &mut info, flags) };
            if result != 0 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() == Some(libc::ECHILD) {
                    return Err(SuspensionError::UnsupportedPlatform("stop acknowledgement requires a direct child owned by this daemon"));
                }
                if error.raw_os_error() == Some(libc::EINVAL) {
                    return Err(SuspensionError::UnsupportedPlatform("waitid P_PIDFD support is required"));
                }
                return Err(failure("waitid stop", self.pid, error));
            }
            Ok(info)
        }
    }

    impl Process for PinnedProcess {
        fn observe(&self) -> Result<Observation, SuspensionError> {
            self.ensure_alive()?;
            let path = format!("/proc/{}/stat", self.pid);
            let (start_token, stopped) = parse_stat(&read(&path, self.pid)?)?;
            let boot = read("/proc/stat", self.pid)?.lines()
                .find_map(|line| line.strip_prefix("btime ").and_then(|value| value.parse::<u64>().ok()))
                .ok_or_else(|| malformed("parse boot time"))?;
            let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
            if ticks <= 0 { return Err(malformed("read clock ticks")); }
            let started_at_unix_ms = boot.checked_mul(1000)
                .and_then(|base| start_token.checked_mul(1000).and_then(|value| base.checked_add(value / ticks as u64)))
                .ok_or_else(|| malformed("convert process start time"))?;
            let incarnation = match fs::read(format!("/proc/{}/environ", self.pid)) {
                Ok(environment) => parse_incarnation(&environment)?,
                Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => None,
                Err(error) => return Err(failure("read process environment", self.pid, error)),
            };
            let (confirmed_start, confirmed_stop) = parse_stat(&read(&path, self.pid)?)?;
            self.ensure_alive()?;
            if confirmed_start != start_token || confirmed_stop != stopped {
                return Err(SuspensionError::IdentityMismatch { pid: self.pid });
            }
            Ok(Observation { identity: ProcessIdentity { started_at_unix_ms, start_token, incarnation }, stopped })
        }

        fn stop(&self) -> Result<(), SuspensionError> {
            // Validate the wait relationship before signalling; non-children are untouched.
            let pending = self.wait_stop(true)?;
            if unsafe { pending.si_pid() } != 0 {
                return Err(SuspensionError::NotOwned { pid: self.pid });
            }
            self.signal(libc::SIGSTOP)?;
            let acknowledgement = self.wait_stop(false)?;
            if acknowledgement.si_code != libc::CLD_STOPPED
                || unsafe { acknowledgement.si_pid() } != self.pid as libc::pid_t
                || unsafe { acknowledgement.si_status() } != libc::SIGSTOP
            {
                return Err(malformed("acknowledge SIGSTOP"));
            }
            Ok(())
        }

        fn resume(&self) -> Result<(), SuspensionError> {
            self.signal(libc::SIGCONT)
        }
    }

    pub(super) fn open(target: &SuspensionTarget) -> Result<PinnedProcess, SuspensionError> {
        if target.pid == 0 || target.pid > i32::MAX as u32 || target.incarnation.is_empty() || target.started_at_unix_ms.is_none() {
            return Err(SuspensionError::IdentityMismatch { pid: target.pid });
        }
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, target.pid as libc::pid_t, 0u32) };
        if fd < 0 { return Err(failure("pidfd_open", target.pid, std::io::Error::last_os_error())); }
        Ok(PinnedProcess { pid: target.pid, fd: unsafe { OwnedFd::from_raw_fd(fd as i32) } })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn stat_parser_handles_parentheses_and_exact_start_ticks() {
            let stat = "42 (a name) with)paren) T 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 987";
            assert_eq!(parse_stat(stat).expect("stat"), (987, true));
            assert!(parse_stat("malformed").is_err());
        }

        #[test]
        fn incarnation_uses_the_actual_pty_environment_key() {
            assert_eq!(parse_incarnation(b"OTHER=x\0FERRYX_PTY_INCARNATION=pane-a\0").expect("environment"), Some("pane-a".into()));
            assert_eq!(parse_incarnation(b"OTHER=x\0").expect("missing"), None);
            assert!(parse_incarnation(b"FERRYX_PTY_INCARNATION=a\0FERRYX_PTY_INCARNATION=b\0").is_err());
        }

        #[test]
        fn bare_pid_is_rejected_before_any_platform_call() {
            let target = SuspensionTarget { pid: 42, incarnation: String::new(), started_at_unix_ms: None };
            assert!(matches!(open(&target), Err(SuspensionError::IdentityMismatch { .. })));
        }
    }
}

#[cfg(target_os = "linux")]
pub(super) fn open(target: &SuspensionTarget) -> Result<impl Process, SuspensionError> {
    linux::open(target)
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use super::super::{verify, ProcessIdentity};

    /// macOS has no pidfd: kernel start time is the identity authority. Verification
    /// immediately before kill bounds, but cannot eliminate, the PID-reuse window.
    /// Every post-actuation identity failure returns an error, never a receipt.
    pub(super) struct VerifiedProcess {
        target: SuspensionTarget,
        identity: ProcessIdentity,
    }

    fn failure(operation: &'static str, pid: u32, source: std::io::Error) -> SuspensionError {
        match source.raw_os_error() {
            Some(libc::ESRCH) | Some(libc::ENOENT) => SuspensionError::ProcessGone { pid },
            _ => SuspensionError::PlatformCallFailure { operation, source },
        }
    }

    fn invalid(operation: &'static str) -> SuspensionError {
        SuspensionError::PlatformCallFailure {
            operation,
            source: std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid macOS process data"),
        }
    }

    fn proc_state(pid: u32) -> Result<u32, SuspensionError> {
        // Darwin arm64/x86_64 kinfo_proc: 648 bytes; extern_proc.p_stat at byte 36.
        let mut info = [0u8; 648];
        let mut len = info.len();
        let mut mib = [libc::CTL_KERN, libc::KERN_PROC, libc::KERN_PROC_PID, pid as i32];
        let result = unsafe {
            libc::sysctl(mib.as_mut_ptr(), 4, info.as_mut_ptr().cast(), &mut len, std::ptr::null_mut(), 0)
        };
        if result != 0 { return Err(failure("sysctl process state", pid, std::io::Error::last_os_error())); }
        state_from_entry(pid, (len != 0).then_some((len, u32::from(info[36]))))
    }

    fn state_from_entry(pid: u32, entry: Option<(usize, u32)>) -> Result<u32, SuspensionError> {
        match entry {
            None => Err(SuspensionError::ProcessGone { pid }),
            Some((648, state)) if state == libc::SZOMB => Err(SuspensionError::ProcessGone { pid }),
            Some((648, state)) => Ok(state),
            _ => Err(invalid("decode kinfo_proc")),
        }
    }

    fn start_identity(seconds: u64, microseconds: u64, incarnation: Option<String>) -> Result<ProcessIdentity, SuspensionError> {
        if microseconds >= 1_000_000 { return Err(invalid("decode process start time")); }
        let start_token = seconds.checked_mul(1_000_000).and_then(|value| value.checked_add(microseconds))
            .ok_or_else(|| invalid("convert process start time"))?;
        Ok(ProcessIdentity { started_at_unix_ms: start_token / 1000, start_token, incarnation })
    }

    fn start_time(pid: u32) -> Result<(u64, u64), SuspensionError> {
        let mut info = unsafe { std::mem::zeroed::<libc::proc_bsdinfo>() };
        let size = std::mem::size_of_val(&info) as i32;
        let read = unsafe { libc::proc_pidinfo(pid as i32, libc::PROC_PIDTBSDINFO, 0, (&mut info as *mut libc::proc_bsdinfo).cast(), size) };
        if read != size { return Err(failure("proc_pidinfo start time", pid, std::io::Error::last_os_error())); }
        if info.pbi_pid != pid { return Err(SuspensionError::IdentityMismatch { pid }); }
        Ok((info.pbi_start_tvsec, info.pbi_start_tvusec))
    }

    fn parse_environment(buffer: &[u8]) -> Result<Option<String>, SuspensionError> {
        let argc_bytes: [u8; 4] = buffer.get(..4).ok_or_else(|| invalid("decode procargs argc"))?
            .try_into().map_err(|_| invalid("decode procargs argc"))?;
        let argc = i32::from_ne_bytes(argc_bytes);
        if argc < 0 { return Err(invalid("decode procargs argc")); }
        let mut rest = &buffer[4..];
        let executable_end = rest.iter().position(|byte| *byte == 0).ok_or_else(|| invalid("decode procargs executable"))?;
        rest = &rest[executable_end + 1..];
        while rest.first() == Some(&0) { rest = &rest[1..]; }
        for _ in 0..argc {
            let end = rest.iter().position(|byte| *byte == 0).ok_or_else(|| invalid("decode procargs argv"))?;
            rest = &rest[end + 1..];
        }
        let mut incarnation = None;
        for entry in rest.split(|byte| *byte == 0) {
            if let Some(value) = entry.strip_prefix(b"FERRYX_PTY_INCARNATION=") {
                if incarnation.is_some() { return Err(invalid("duplicate process incarnation")); }
                incarnation = Some(std::str::from_utf8(value).map_err(|_| invalid("decode process incarnation"))?.to_owned());
            }
        }
        Ok(incarnation)
    }

    fn environment(pid: u32) -> Result<Option<String>, SuspensionError> {
        let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid as i32];
        let mut len = 0;
        let result = unsafe { libc::sysctl(mib.as_mut_ptr(), 3, std::ptr::null_mut(), &mut len, std::ptr::null_mut(), 0) };
        if result != 0 {
            let error = std::io::Error::last_os_error();
            if matches!(error.raw_os_error(), Some(libc::EPERM) | Some(libc::EACCES)) { return Ok(None); }
            return Err(failure("size process environment", pid, error));
        }
        if len == 0 { return Err(SuspensionError::ProcessGone { pid }); }
        let mut buffer = vec![0u8; len];
        let result = unsafe { libc::sysctl(mib.as_mut_ptr(), 3, buffer.as_mut_ptr().cast(), &mut len, std::ptr::null_mut(), 0) };
        if result != 0 {
            let error = std::io::Error::last_os_error();
            if matches!(error.raw_os_error(), Some(libc::EPERM) | Some(libc::EACCES)) { return Ok(None); }
            return Err(failure("read process environment", pid, error));
        }
        buffer.truncate(len);
        parse_environment(&buffer)
    }

    fn observe(pid: u32) -> Result<Observation, SuspensionError> {
        proc_state(pid)?;
        let before = start_time(pid)?;
        let incarnation = environment(pid)?;
        let state = proc_state(pid)?;
        if start_time(pid)? != before { return Err(SuspensionError::IdentityMismatch { pid }); }
        Ok(Observation { identity: start_identity(before.0, before.1, incarnation)?, stopped: state == libc::SSTOP })
    }

    impl VerifiedProcess {
        fn verified_observation(&self) -> Result<Observation, SuspensionError> {
            let observed = observe(self.target.pid)?;
            verify(&self.target, &observed.identity)?;
            if observed.identity != self.identity { return Err(SuspensionError::IdentityMismatch { pid: self.target.pid }); }
            Ok(observed)
        }

        fn signal(&self, signal: i32) -> Result<(), SuspensionError> {
            let observed = self.verified_observation()?;
            if signal == libc::SIGSTOP && observed.stopped { return Err(SuspensionError::NotOwned { pid: self.target.pid }); }
            if signal == libc::SIGCONT && !observed.stopped { return Err(SuspensionError::NotOwned { pid: self.target.pid }); }
            if unsafe { libc::kill(self.target.pid as i32, signal) } != 0 {
                return Err(failure("kill suspension signal", self.target.pid, std::io::Error::last_os_error()));
            }
            Ok(())
        }

        fn wait_stop(&self, nonblocking: bool) -> Result<libc::siginfo_t, SuspensionError> {
            let mut info = unsafe { std::mem::zeroed::<libc::siginfo_t>() };
            let flags = libc::WSTOPPED | libc::WNOWAIT | if nonblocking { libc::WNOHANG } else { 0 };
            if unsafe { libc::waitid(libc::P_PID, self.target.pid, &mut info, flags) } != 0 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() == Some(libc::ECHILD) {
                    return Err(SuspensionError::UnsupportedPlatform("macOS stop acknowledgement requires a direct child"));
                }
                return Err(failure("waitid stop", self.target.pid, error));
            }
            Ok(info)
        }
    }

    impl Process for VerifiedProcess {
        fn observe(&self) -> Result<Observation, SuspensionError> { self.verified_observation() }
        fn stop(&self) -> Result<(), SuspensionError> {
            if self.wait_stop(true)?.si_pid != 0 { return Err(SuspensionError::NotOwned { pid: self.target.pid }); }
            self.signal(libc::SIGSTOP)?;
            let info = self.wait_stop(false)?;
            if info.si_code != libc::CLD_STOPPED || info.si_pid != self.target.pid as i32 || info.si_status != libc::SIGSTOP {
                return Err(invalid("acknowledge SIGSTOP"));
            }
            if !self.verified_observation()?.stopped { return Err(invalid("confirm stopped state")); }
            Ok(())
        }
        fn resume(&self) -> Result<(), SuspensionError> {
            self.signal(libc::SIGCONT)?;
            self.verified_observation()?;
            Ok(())
        }
    }

    pub(super) fn open(target: &SuspensionTarget) -> Result<VerifiedProcess, SuspensionError> {
        if target.pid == 0 || target.pid > i32::MAX as u32 || target.incarnation.is_empty() || target.started_at_unix_ms.is_none() {
            return Err(SuspensionError::IdentityMismatch { pid: target.pid });
        }
        let observed = observe(target.pid)?;
        verify(target, &observed.identity)?;
        Ok(VerifiedProcess { target: target.clone(), identity: observed.identity })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn target() -> SuspensionTarget {
            SuspensionTarget { pid: 42, incarnation: "pane-a".into(), started_at_unix_ms: Some(100_123) }
        }

        #[test]
        fn start_time_match_and_mismatch() {
            let identity = start_identity(100, 123_456, Some("pane-a".into())).expect("identity");
            assert_eq!(identity.start_token, 100_123_456);
            assert!(verify(&target(), &identity).is_ok());
            let mismatch = start_identity(101, 123_456, Some("pane-a".into())).expect("identity");
            assert!(matches!(verify(&target(), &mismatch), Err(SuspensionError::IdentityMismatch { pid: 42 })));
        }

        #[test]
        fn incarnation_mismatch_is_refused() {
            let identity = start_identity(100, 123_456, Some("pane-b".into())).expect("identity");
            assert!(matches!(verify(&target(), &identity), Err(SuspensionError::IdentityMismatch { pid: 42 })));
        }

        #[test]
        fn missing_proc_entry_is_process_gone() {
            assert!(matches!(state_from_entry(42, None), Err(SuspensionError::ProcessGone { pid: 42 })));
            assert!(state_from_entry(42, Some((36, libc::SSTOP))).is_err());
        }

        #[test]
        fn procargs_environment_excludes_argv_lookalikes() {
            let mut buffer = 2i32.to_ne_bytes().to_vec();
            buffer.extend_from_slice(b"/bin/sh\0\0sh\0FERRYX_PTY_INCARNATION=wrong\0FERRYX_PTY_INCARNATION=pane-a\0");
            assert_eq!(parse_environment(&buffer).expect("environment"), Some("pane-a".into()));
            assert!(parse_environment(&[0, 0]).is_err());
        }
    }
}

#[cfg(target_os = "macos")]
pub(super) fn open(target: &SuspensionTarget) -> Result<impl Process, SuspensionError> {
    macos::open(target)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
struct UnsupportedProcess;

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
impl Process for UnsupportedProcess {
    fn observe(&self) -> Result<Observation, SuspensionError> { Err(unsupported()) }
    fn stop(&self) -> Result<(), SuspensionError> { Err(unsupported()) }
    fn resume(&self) -> Result<(), SuspensionError> { Err(unsupported()) }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn unsupported() -> SuspensionError {
    SuspensionError::UnsupportedPlatform("this Unix platform has no identity-pinned signal backend")
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(super) fn open(_target: &SuspensionTarget) -> Result<impl Process, SuspensionError> {
    Err::<UnsupportedProcess, _>(unsupported())
}

#[cfg(all(test, not(any(target_os = "linux", target_os = "macos"))))]
mod tests {
    use super::*;
    #[test]
    fn unsupported_unix_never_returns_a_receipt() {
        let target = SuspensionTarget { pid: 42, incarnation: "pane-a".into(), started_at_unix_ms: Some(100) };
        assert!(matches!(open(&target), Err(SuspensionError::UnsupportedPlatform(_))));
    }
}
