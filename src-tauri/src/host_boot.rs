//! Host boot time, used as positive exit evidence for persisted local sessions.
//!
//! A persisted session's `daemonEpoch` is the owning daemon's start wall-clock in ms.
//! If that epoch predates the host boot time, the daemon (and every PTY it owned)
//! cannot still be alive.

/// Host boot time in milliseconds since the UNIX epoch, or `None` when unavailable.
pub fn host_boot_time_ms() -> Option<u64> {
    imp::host_boot_time_ms()
}

/// Parses the `btime <secs>` line of `/proc/stat` and returns it in milliseconds.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
pub(crate) fn parse_proc_stat_btime(contents: &str) -> Option<u64> {
    contents.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        if fields.next()? != "btime" {
            return None;
        }
        let secs: u64 = fields.next()?.parse().ok()?;
        if fields.next().is_some() {
            return None;
        }
        secs.checked_mul(1000)
    })
}

#[cfg(target_os = "macos")]
mod imp {
    pub(super) fn host_boot_time_ms() -> Option<u64> {
        let mut mib = [libc::CTL_KERN, libc::KERN_BOOTTIME];
        let mut boot = libc::timeval {
            tv_sec: 0,
            tv_usec: 0,
        };
        let mut size = std::mem::size_of::<libc::timeval>();
        // SAFETY: `mib` names a valid sysctl, `boot` is a properly sized and aligned
        // out-buffer whose size is passed in `size`; no new value is written.
        let rc = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                mib.len() as libc::c_uint,
                (&mut boot as *mut libc::timeval).cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if rc != 0 || size != std::mem::size_of::<libc::timeval>() || boot.tv_sec <= 0 {
            return None;
        }
        let secs = u64::try_from(boot.tv_sec).ok()?;
        let usec = u64::try_from(boot.tv_usec).ok()?;
        secs.checked_mul(1000)?.checked_add(usec / 1000)
    }
}

#[cfg(target_os = "linux")]
mod imp {
    pub(super) fn host_boot_time_ms() -> Option<u64> {
        let contents = std::fs::read_to_string("/proc/stat").ok()?;
        super::parse_proc_stat_btime(&contents)
    }
}

#[cfg(target_os = "windows")]
mod imp {
    use std::time::{SystemTime, UNIX_EPOCH};

    pub(super) fn host_boot_time_ms() -> Option<u64> {
        let now_ms = u64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_millis()).ok()?;
        // SAFETY: GetTickCount64 takes no arguments and has no preconditions.
        let uptime_ms = unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() };
        Some(now_ms.saturating_sub(uptime_ms))
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
mod imp {
    pub(super) fn host_boot_time_ms() -> Option<u64> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROC_STAT_SAMPLE: &str = "cpu  10132153 290696 3084719 46828483 16683 0 25195 0 0 0
cpu0 1393280 32966 572056 13343292 6130 0 17875 0 0 0
cpu1 1335860 31930 474920 13466355 4227 0 2402 0 0 0
intr 1462898 41 9 0 0 0 0 0 0 1 0 0 0 0 0 0 0
ctxt 115315
btime 1769212345
processes 2289
procs_running 1
procs_blocked 0
softirq 1277516 0 362035 0 104 46281 0 2 340871 0 528223
";

    #[test]
    fn parses_btime_from_realistic_proc_stat() {
        assert_eq!(parse_proc_stat_btime(PROC_STAT_SAMPLE), Some(1_769_212_345_000));
    }

    #[test]
    fn missing_btime_yields_none() {
        let sample = "cpu  1 2 3 4\nctxt 115315\nprocesses 2289\n";
        assert_eq!(parse_proc_stat_btime(sample), None);
    }

    #[test]
    fn malformed_btime_yields_none() {
        assert_eq!(parse_proc_stat_btime("btime notanumber\n"), None);
        assert_eq!(parse_proc_stat_btime("btime\n"), None);
        assert_eq!(parse_proc_stat_btime("btime -5\n"), None);
        assert_eq!(parse_proc_stat_btime("btime 12 34\n"), None);
    }

    /// Cross-checks the sysctl read against the independent `sysctl -n kern.boottime` text.
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_boot_time_matches_sysctl_command() {
        let output = std::process::Command::new("/usr/sbin/sysctl")
            .args(["-n", "kern.boottime"])
            .output()
            .expect("run sysctl");
        let text = String::from_utf8_lossy(&output.stdout);
        let secs = crate::daemon::resource_usage::parse_boottime_secs(&text).expect("parse kern.boottime");
        let boot = host_boot_time_ms().expect("boot time");
        assert_eq!(boot / 1000, secs, "sysctl read {boot} ms vs kern.boottime {secs} s");
    }

    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    #[test]
    fn host_boot_time_is_recent_past() {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_millis() as u64;
        let boot = host_boot_time_ms().expect("boot time available on this platform");
        assert!(boot < now_ms, "boot {boot} must precede now {now_ms}");
        let four_hundred_days_ms = 400 * 24 * 60 * 60 * 1000;
        assert!(
            boot > now_ms - four_hundred_days_ms,
            "boot {boot} older than 400 days before now {now_ms}"
        );
    }
}
