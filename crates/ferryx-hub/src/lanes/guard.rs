//! Admission gate for launching another lane pane. Metrics are supplied by the caller.
//! The gateway's SSH metric probe is not ported.

pub const DEFAULT_LOAD_ALERT: f64 = 150.0;
pub const DEFAULT_LOAD_OK: f64 = 80.0;
pub const DEFAULT_MEM_FREE_MIN_PCT: u32 = 20;
pub const DEFAULT_DISK_FREE_MIN_GB: u64 = 50;
pub const DEFAULT_LANES_MAX: u64 = 6;

#[derive(Debug, Clone, PartialEq)]
pub struct GuardThresholds {
    pub load_alert: f64,
    pub mem_free_min_pct: u32,
    pub disk_free_min_gb: u64,
    pub lanes_max: u64,
}

impl Default for GuardThresholds {
    fn default() -> Self {
        Self::from_env()
    }
}

impl GuardThresholds {
    pub fn from_env() -> Self {
        Self {
            load_alert: env_f64(
                "FERRYX_HUB_GUARD_LOAD_ALERT",
                "OMON_GUARD_LOAD_ALERT",
                DEFAULT_LOAD_ALERT,
            ),
            mem_free_min_pct: env_u32(
                "FERRYX_HUB_GUARD_MEM_FREE_MIN_PCT",
                "OMON_GUARD_MEM_FREE_MIN_PCT",
                DEFAULT_MEM_FREE_MIN_PCT,
            ),
            disk_free_min_gb: env_u64(
                "FERRYX_HUB_GUARD_DISK_FREE_MIN_GB",
                "OMON_GUARD_DISK_FREE_MIN_GB",
                DEFAULT_DISK_FREE_MIN_GB,
            ),
            lanes_max: env_u64(
                "FERRYX_HUB_GUARD_LANES_MAX",
                "OMON_GUARD_LANES_MAX",
                DEFAULT_LANES_MAX,
            ),
        }
    }
}

fn env_f64(primary: &str, legacy: &str, default: f64) -> f64 {
    crate::hub_env::var(primary)
        .or_else(|_| crate::hub_env::var(legacy))
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn env_u32(primary: &str, legacy: &str, default: u32) -> u32 {
    env_f64(primary, legacy, f64::from(default)) as u32
}

fn env_u64(primary: &str, legacy: &str, default: u64) -> u64 {
    env_f64(primary, legacy, default as f64) as u64
}

#[derive(Debug, Clone, Copy)]
pub struct HostLoad {
    pub load: f64,
    pub mem_free_pct: u32,
    pub disk_free_gb: u64,
    pub active_lanes: u64,
}

impl HostLoad {
    /// Reads `/proc` on Linux and `statvfs` everywhere else it is available.
    /// A metric that cannot be read admits, so a missing proc file does not stall lanes.
    pub fn sample(active_lanes: u64) -> Self {
        Self {
            load: read_load().unwrap_or(0.0),
            mem_free_pct: read_mem_free_pct().unwrap_or(100),
            disk_free_gb: read_disk_free_gb().unwrap_or(100),
            active_lanes,
        }
    }

    pub fn admit(self, thresholds: &GuardThresholds) -> bool {
        self.active_lanes < thresholds.lanes_max
            && self.load < thresholds.load_alert
            && self.mem_free_pct >= thresholds.mem_free_min_pct
            && self.disk_free_gb >= thresholds.disk_free_min_gb
    }
}

pub fn parse_loadavg(text: &str) -> Option<f64> {
    text.split_whitespace().next()?.parse().ok()
}

pub fn parse_meminfo(text: &str) -> Option<u32> {
    let mut total = None;
    let mut available = None;
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let Some(key) = parts.next() else { continue };
        let Some(value) = parts.next().and_then(|value| value.parse::<u64>().ok()) else {
            continue;
        };
        match key {
            "MemTotal:" => total = Some(value),
            "MemAvailable:" => available = Some(value),
            _ => {}
        }
    }
    let (total, available) = (total?, available?);
    if total == 0 {
        return None;
    }
    Some(((available.saturating_mul(100)) / total) as u32)
}

fn read_load() -> Option<f64> {
    let text = std::fs::read_to_string("/proc/loadavg").ok()?;
    parse_loadavg(&text)
}

fn read_mem_free_pct() -> Option<u32> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    parse_meminfo(&text)
}

fn read_disk_free_gb() -> Option<u64> {
    let path = std::ffi::CString::new("/").ok()?;
    let (_total, free) = crate::readiness::disk_sample(&path)?;
    Some(free / 1_000_000_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_lane_budget_or_a_hot_host_is_refused() {
        let thresholds = GuardThresholds {
            load_alert: 10.0,
            mem_free_min_pct: 20,
            disk_free_min_gb: 5,
            lanes_max: 2,
        };
        let ok = HostLoad {
            load: 1.0,
            mem_free_pct: 40,
            disk_free_gb: 20,
            active_lanes: 1,
        };
        assert!(ok.admit(&thresholds));
        assert!(!HostLoad {
            active_lanes: 2,
            ..ok
        }
        .admit(&thresholds));
        assert!(!HostLoad { load: 10.0, ..ok }.admit(&thresholds));
        assert!(!HostLoad {
            mem_free_pct: 19,
            ..ok
        }
        .admit(&thresholds));
    }

    #[test]
    fn proc_parsers_read_load_and_available_memory() {
        assert_eq!(parse_loadavg("1.25 0.9 0.8 1/2 3"), Some(1.25));
        let meminfo = "MemTotal: 1000 kB\nMemAvailable: 250 kB\n";
        assert_eq!(parse_meminfo(meminfo), Some(25));
        let sample = HostLoad::sample(0);
        assert!(sample.mem_free_pct <= 100);
    }
}
