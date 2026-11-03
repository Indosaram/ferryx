use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceOrientation {
    Portrait,
    PortraitUpsideDown,
    LandscapeLeft,
    LandscapeRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppLifecycleState {
    ForegroundActive,
    ForegroundInactive,
    Background,
    Suspended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkInterfaceType {
    Wifi,
    Cellular,
    Ethernet,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct MobileLifecycleManager {
    orientation: DeviceOrientation,
    state: AppLifecycleState,
    network_type: NetworkInterfaceType,
    last_state_change: Instant,
    needs_keyframe_on_resume: bool,
    reconnect_attempts: u32,
    max_reconnect_attempts: u32,
}

impl Default for MobileLifecycleManager {
    fn default() -> Self {
        Self {
            orientation: DeviceOrientation::Portrait,
            state: AppLifecycleState::ForegroundActive,
            network_type: NetworkInterfaceType::Wifi,
            last_state_change: Instant::now(),
            needs_keyframe_on_resume: false,
            reconnect_attempts: 0,
            max_reconnect_attempts: 5,
        }
    }
}

impl MobileLifecycleManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn orientation(&self) -> DeviceOrientation {
        self.orientation
    }

    pub fn set_orientation(&mut self, orientation: DeviceOrientation) {
        self.orientation = orientation;
    }

    pub fn state(&self) -> AppLifecycleState {
        self.state
    }

    pub fn enter_background(&mut self) {
        self.state = AppLifecycleState::Background;
        self.last_state_change = Instant::now();
        self.needs_keyframe_on_resume = true;
    }

    pub fn enter_foreground(&mut self) -> bool {
        self.state = AppLifecycleState::ForegroundActive;
        self.last_state_change = Instant::now();
        let keyframe_needed = self.needs_keyframe_on_resume;
        self.needs_keyframe_on_resume = false;
        keyframe_needed
    }

    pub fn suspend(&mut self) {
        self.state = AppLifecycleState::Suspended;
        self.last_state_change = Instant::now();
        self.needs_keyframe_on_resume = true;
    }

    pub fn on_network_change(&mut self, new_type: NetworkInterfaceType) -> bool {
        let changed = self.network_type != new_type;
        self.network_type = new_type;
        if changed {
            self.reconnect_attempts = 0;
        }
        changed
    }

    pub fn should_attempt_reconnect(&mut self) -> bool {
        if self.reconnect_attempts < self.max_reconnect_attempts {
            self.reconnect_attempts += 1;
            true
        } else {
            false
        }
    }

    pub fn reset_reconnect_budget(&mut self) {
        self.reconnect_attempts = 0;
    }

    pub fn background_duration(&self) -> Duration {
        if self.state == AppLifecycleState::Background || self.state == AppLifecycleState::Suspended
        {
            self.last_state_change.elapsed()
        } else {
            Duration::ZERO
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_preserves_active_state() {
        let mut manager = MobileLifecycleManager::new();
        assert_eq!(manager.state(), AppLifecycleState::ForegroundActive);

        manager.set_orientation(DeviceOrientation::LandscapeLeft);
        assert_eq!(manager.orientation(), DeviceOrientation::LandscapeLeft);
        assert_eq!(manager.state(), AppLifecycleState::ForegroundActive);
    }

    #[test]
    fn background_and_resume_requests_keyframe() {
        let mut manager = MobileLifecycleManager::new();

        manager.enter_background();
        assert_eq!(manager.state(), AppLifecycleState::Background);

        let request_idr = manager.enter_foreground();
        assert!(request_idr);
        assert_eq!(manager.state(), AppLifecycleState::ForegroundActive);

        let second_call = manager.enter_foreground();
        assert!(!second_call);
    }

    #[test]
    fn network_handover_and_reconnect_budget() {
        let mut manager = MobileLifecycleManager::new();

        assert!(manager.on_network_change(NetworkInterfaceType::Cellular));
        assert!(!manager.on_network_change(NetworkInterfaceType::Cellular));

        for _ in 0..5 {
            assert!(manager.should_attempt_reconnect());
        }
        assert!(!manager.should_attempt_reconnect());

        manager.reset_reconnect_budget();
        assert!(manager.should_attempt_reconnect());
    }
}
