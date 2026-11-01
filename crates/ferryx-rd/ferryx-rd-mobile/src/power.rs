#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThermalState {
    Nominal,
    Fair,
    Serious,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowerBudgetConfig {
    pub default_bitrate_kbps: u32,
    pub default_fps: u32,
    pub battery_saver_bitrate_kbps: u32,
    pub battery_saver_fps: u32,
    pub thermal_throttle_bitrate_kbps: u32,
    pub thermal_throttle_fps: u32,
}

impl Default for PowerBudgetConfig {
    fn default() -> Self {
        Self {
            default_bitrate_kbps: 15_000,
            default_fps: 60,
            battery_saver_bitrate_kbps: 4_000,
            battery_saver_fps: 30,
            thermal_throttle_bitrate_kbps: 2_500,
            thermal_throttle_fps: 30,
        }
    }
}

pub struct PowerPolicyManager {
    config: PowerBudgetConfig,
    battery_saver_active: bool,
    data_saver_active: bool,
    thermal_state: ThermalState,
}

impl Default for PowerPolicyManager {
    fn default() -> Self {
        Self {
            config: PowerBudgetConfig::default(),
            battery_saver_active: false,
            data_saver_active: false,
            thermal_state: ThermalState::Nominal,
        }
    }
}

impl PowerPolicyManager {
    pub fn new(config: PowerBudgetConfig) -> Self {
        Self {
            config,
            battery_saver_active: false,
            data_saver_active: false,
            thermal_state: ThermalState::Nominal,
        }
    }

    pub fn set_battery_saver(&mut self, active: bool) {
        self.battery_saver_active = active;
    }

    pub fn set_data_saver(&mut self, active: bool) {
        self.data_saver_active = active;
    }

    pub fn set_thermal_state(&mut self, state: ThermalState) {
        self.thermal_state = state;
    }

    pub fn target_limits(&self) -> (u32, u32) {
        let mut target_bitrate = self.config.default_bitrate_kbps;
        let mut target_fps = self.config.default_fps;

        if self.battery_saver_active || self.data_saver_active {
            target_bitrate = target_bitrate.min(self.config.battery_saver_bitrate_kbps);
            target_fps = target_fps.min(self.config.battery_saver_fps);
        }

        match self.thermal_state {
            ThermalState::Nominal => {}
            ThermalState::Fair => {
                let q = target_bitrate / 5;
                let r = target_bitrate % 5;
                let scaled = q * 4 + (r * 4) / 5;
                target_bitrate = scaled.max(1_000).min(target_bitrate);
            }
            ThermalState::Serious => {
                target_bitrate = target_bitrate.min(self.config.thermal_throttle_bitrate_kbps);
                target_fps = target_fps.min(self.config.thermal_throttle_fps);
            }
            ThermalState::Critical => {
                target_bitrate = target_bitrate.min(1_000);
                target_fps = target_fps.min(15);
            }
        }

        (target_bitrate, target_fps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nominal_defaults_apply() {
        let manager = PowerPolicyManager::default();
        let (bitrate, fps) = manager.target_limits();
        assert_eq!(bitrate, 15_000);
        assert_eq!(fps, 60);
    }

    #[test]
    fn battery_saver_caps_bitrate_and_fps() {
        let mut manager = PowerPolicyManager::default();
        manager.set_battery_saver(true);
        let (bitrate, fps) = manager.target_limits();
        assert_eq!(bitrate, 4_000);
        assert_eq!(fps, 30);
    }

    #[test]
    fn critical_thermal_throttles_aggressively() {
        let mut manager = PowerPolicyManager::default();
        manager.set_thermal_state(ThermalState::Critical);
        let (bitrate, fps) = manager.target_limits();
        assert_eq!(bitrate, 1_000);
        assert_eq!(fps, 15);
    }

    #[test]
    fn fair_thermal_never_exceeds_low_cap() {
        let config = PowerBudgetConfig {
            default_bitrate_kbps: 800,
            default_fps: 30,
            battery_saver_bitrate_kbps: 400,
            battery_saver_fps: 15,
            thermal_throttle_bitrate_kbps: 300,
            thermal_throttle_fps: 15,
        };
        let mut manager = PowerPolicyManager::new(config);
        manager.set_thermal_state(ThermalState::Fair);
        let (bitrate, _) = manager.target_limits();
        assert!(bitrate <= 800);
    }

    #[test]
    fn fair_thermal_handles_u32_max_without_overflow() {
        let config = PowerBudgetConfig {
            default_bitrate_kbps: u32::MAX,
            default_fps: 60,
            battery_saver_bitrate_kbps: u32::MAX,
            battery_saver_fps: 60,
            thermal_throttle_bitrate_kbps: u32::MAX,
            thermal_throttle_fps: 60,
        };
        let mut manager = PowerPolicyManager::new(config);
        manager.set_thermal_state(ThermalState::Fair);
        let (bitrate, _) = manager.target_limits();
        assert_eq!(bitrate, 3_435_973_836);
    }
}
