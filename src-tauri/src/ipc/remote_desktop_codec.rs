//! Session-local codec fallback based on the backend that actually decoded a frame.

use ferryx_rd_decode::{CodecKind, HardwareAcceleration};
use ferryx_rd_proto::StreamCodecPreference;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodecAction {
    Keep,
    DowngradeToH264,
}

#[derive(Debug)]
pub struct RdCodecPolicy {
    override_codec: Option<StreamCodecPreference>,
    downgraded: bool,
}

impl RdCodecPolicy {
    pub fn new(override_codec: Option<StreamCodecPreference>) -> Self {
        Self {
            override_codec,
            downgraded: false,
        }
    }

    pub fn initial_preference(&self) -> StreamCodecPreference {
        self.override_codec.unwrap_or(StreamCodecPreference::Auto)
    }

    pub fn observe_decoded(
        &mut self,
        codec: CodecKind,
        accel: HardwareAcceleration,
    ) -> CodecAction {
        if self.override_codec.is_none()
            && codec == CodecKind::Hevc
            && accel == HardwareAcceleration::Software
            && !self.downgraded
        {
            self.downgraded = true;
            CodecAction::DowngradeToH264
        } else {
            CodecAction::Keep
        }
    }

    pub fn reset_for_reconnect(&mut self) {
        self.downgraded = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_auto() {
        let policy = RdCodecPolicy::new(None);
        assert_eq!(policy.initial_preference(), StreamCodecPreference::Auto);
    }

    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    #[test]
    fn hardware_hevc_does_not_downgrade_or_consume_the_latch() {
        #[cfg(target_os = "macos")]
        let acceleration = HardwareAcceleration::VideoToolbox;
        #[cfg(target_os = "windows")]
        let acceleration = HardwareAcceleration::D3d11va;
        #[cfg(target_os = "linux")]
        let acceleration = HardwareAcceleration::Vaapi;

        let mut policy = RdCodecPolicy::new(None);
        assert_eq!(
            policy.observe_decoded(CodecKind::Hevc, acceleration),
            CodecAction::Keep
        );
        assert_eq!(
            policy.observe_decoded(CodecKind::Hevc, HardwareAcceleration::Software),
            CodecAction::DowngradeToH264
        );
    }

    #[test]
    fn software_hevc_downgrades_only_once_per_session() {
        let mut policy = RdCodecPolicy::new(None);
        assert_eq!(
            policy.observe_decoded(CodecKind::Hevc, HardwareAcceleration::Software),
            CodecAction::DowngradeToH264
        );
        assert_eq!(
            policy.observe_decoded(CodecKind::H264, HardwareAcceleration::Software),
            CodecAction::Keep
        );
        assert_eq!(
            policy.observe_decoded(CodecKind::Hevc, HardwareAcceleration::Software),
            CodecAction::Keep
        );
    }

    #[test]
    fn explicit_overrides_are_preserved_and_never_downgrade() {
        for preference in [
            StreamCodecPreference::Hevc,
            StreamCodecPreference::H264,
            StreamCodecPreference::Auto,
        ] {
            let mut policy = RdCodecPolicy::new(Some(preference));
            assert_eq!(policy.initial_preference(), preference);
            for codec in [CodecKind::Hevc, CodecKind::H264] {
                assert_eq!(
                    policy.observe_decoded(codec, HardwareAcceleration::Software),
                    CodecAction::Keep
                );
            }
            policy.reset_for_reconnect();
            assert_eq!(policy.initial_preference(), preference);
            assert_eq!(
                policy.observe_decoded(CodecKind::Hevc, HardwareAcceleration::Software),
                CodecAction::Keep
            );
        }
    }

    #[test]
    fn software_h264_does_not_downgrade_or_consume_the_latch() {
        let mut policy = RdCodecPolicy::new(None);
        assert_eq!(
            policy.observe_decoded(CodecKind::H264, HardwareAcceleration::Software),
            CodecAction::Keep
        );
        assert_eq!(
            policy.observe_decoded(CodecKind::Hevc, HardwareAcceleration::Software),
            CodecAction::DowngradeToH264
        );
    }

    #[test]
    fn reconnect_resets_downgrade_latch() {
        let mut policy = RdCodecPolicy::new(None);
        assert_eq!(
            policy.observe_decoded(CodecKind::Hevc, HardwareAcceleration::Software),
            CodecAction::DowngradeToH264
        );
        policy.reset_for_reconnect();
        assert_eq!(policy.initial_preference(), StreamCodecPreference::Auto);
        assert_eq!(
            policy.observe_decoded(CodecKind::Hevc, HardwareAcceleration::Software),
            CodecAction::DowngradeToH264
        );
        assert_eq!(
            policy.observe_decoded(CodecKind::Hevc, HardwareAcceleration::Software),
            CodecAction::Keep
        );
    }

    #[test]
    fn reconnect_before_downgrade_keeps_policy_clean() {
        let mut policy = RdCodecPolicy::new(None);
        policy.reset_for_reconnect();
        assert_eq!(
            policy.observe_decoded(CodecKind::Hevc, HardwareAcceleration::Software),
            CodecAction::DowngradeToH264
        );
    }
}
