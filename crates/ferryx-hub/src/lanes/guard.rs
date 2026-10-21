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
    std::env::var(primary)
        .or_else(|_| std::env::var(legacy))
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
    pub fn admit(self, thresholds: &GuardThresholds) -> bool {
        self.active_lanes < thresholds.lanes_max
            && self.load < thresholds.load_alert
            && self.mem_free_pct >= thresholds.mem_free_min_pct
            && self.disk_free_gb >= thresholds.disk_free_min_gb
    }
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
}
