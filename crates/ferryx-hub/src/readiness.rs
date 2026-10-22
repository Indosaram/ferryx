//! Disk pressure, matching the omo-gateway health classes.
pub const DISK_DEGRADED_PERCENT: f64 = 90.0;
pub const DISK_BYTES_PER_MB: u64 = 1024 * 1024;
pub const DISK_CRITICAL_FREE_FLOOR_MB: u64 = 256;
pub const DISK_CRITICAL_PERCENT_FLOOR: f64 = 95.0;
pub const DISK_CRITICAL_HEADROOM_MB: u64 = 1024;
pub const DISK_ELEVATED_FREE_FLOOR_MB: u64 = 512;
pub const DISK_ELEVATED_PERCENT_FLOOR: f64 = 85.0;
pub const DISK_ELEVATED_HEADROOM_MB: u64 = 4096;

/// `ok`, `elevated`, `critical`, or `unknown`. Unreadable or zero-capacity disks are unknown.
pub fn classify_disk_pressure(total_bytes: Option<u64>, free_bytes: Option<u64>) -> &'static str {
    let (Some(total), Some(free)) = (total_bytes, free_bytes) else {
        return "unknown";
    };
    if total == 0 || free > total {
        return "unknown";
    }
    let total_mb = total / DISK_BYTES_PER_MB;
    let free_mb = free / DISK_BYTES_PER_MB;
    if total_mb == 0 || free_mb > total_mb {
        return "unknown";
    }
    let used_percent = (1.0 - (free_mb as f64 / total_mb as f64)) * 100.0;
    if free_mb < DISK_CRITICAL_FREE_FLOOR_MB
        || (used_percent >= DISK_CRITICAL_PERCENT_FLOOR && free_mb < DISK_CRITICAL_HEADROOM_MB)
    {
        "critical"
    } else if free_mb < DISK_ELEVATED_FREE_FLOOR_MB
        || (used_percent >= DISK_ELEVATED_PERCENT_FLOOR && free_mb < DISK_ELEVATED_HEADROOM_MB)
    {
        "elevated"
    } else {
        "ok"
    }
}

/// `statvfs` is POSIX-only, so non-unix targets report no sample and the caller
/// classifies the disk as `unknown`, matching the `/proc`-based load and memory
/// probes in `lanes::guard`.
#[cfg(not(unix))]
pub fn disk_sample(_path: &std::ffi::CStr) -> Option<(u64, u64)> {
    None
}

#[cfg(unix)]
pub fn disk_sample(path: &std::ffi::CStr) -> Option<(u64, u64)> {
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::zeroed();
    let rc = unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) };
    if rc != 0 {
        return None;
    }
    let stat = unsafe { stat.assume_init() };
    // `fsblkcnt_t` is already `u64` on Linux and narrower on other targets.
    #[allow(clippy::unnecessary_cast)]
    let frsize = stat.f_frsize as u64;
    #[allow(clippy::unnecessary_cast)]
    let total = (stat.f_blocks as u64).saturating_mul(frsize);
    #[allow(clippy::unnecessary_cast)]
    let free = (stat.f_bavail as u64).saturating_mul(frsize);
    Some((total, free))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_pressure_matches_the_gateway_floors() {
        assert_eq!(classify_disk_pressure(None, Some(1)), "unknown");
        assert_eq!(classify_disk_pressure(Some(0), Some(0)), "unknown");
        let critical = 200 * DISK_BYTES_PER_MB;
        let total = 1000 * DISK_BYTES_PER_MB;
        assert_eq!(
            classify_disk_pressure(Some(total), Some(critical)),
            "critical"
        );
        let healthy_free = 50_000 * DISK_BYTES_PER_MB;
        let healthy_total = 100_000 * DISK_BYTES_PER_MB;
        assert_eq!(
            classify_disk_pressure(Some(healthy_total), Some(healthy_free)),
            "ok"
        );
        let _ = DISK_DEGRADED_PERCENT;
        let _ = DISK_ELEVATED_HEADROOM_MB;
    }
}
