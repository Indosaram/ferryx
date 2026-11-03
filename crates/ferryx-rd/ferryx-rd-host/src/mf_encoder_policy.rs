//! Platform-independent Media Foundation encoder negotiation policy.
//!
//! Kept free of `windows` types so the decisions are unit-testable on any host;
//! `encode_windows*.rs` maps them onto `ICodecAPI` / `IMFMediaType` calls.

/// H.264 profiles MahoRD negotiates, in `eAVEncH264VProfile` numbering
/// (equal to the H.264 `profile_idc`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum H264Profile {
    Main,
    High,
}

impl H264Profile {
    /// Value written to `MF_MT_MPEG2_PROFILE` (`eAVEncH264VProfile_*`).
    pub(crate) fn mf_value(self) -> u32 {
        match self {
            Self::Main => 77,
            Self::High => 100,
        }
    }

    /// Main and High both permit CABAC; only Baseline-class profiles forbid it.
    fn allows_cabac(self) -> bool {
        matches!(self, Self::Main | Self::High)
    }
}

/// Output-type negotiation order: High first, Main when the MFT rejects High.
const H264_PROFILE_ORDER: [H264Profile; 2] = [H264Profile::High, H264Profile::Main];

/// Whether `CODECAPI_AVEncH264CABACEnable` should be written, and with what.
/// `None` means leave the encoder default untouched (property unsupported).
pub(crate) fn cabac_setting(supported: bool, profile: H264Profile) -> Option<bool> {
    supported.then(|| profile.allows_cabac())
}

/// Drives the High -> Main fallback. `try_profile` attempts `SetOutputType`
/// with the given profile; the first success wins. Returns the last error when
/// every profile is rejected.
pub(crate) fn negotiate_h264_profile<T, E>(
    mut try_profile: impl FnMut(H264Profile) -> Result<T, E>,
) -> Result<(H264Profile, T), E> {
    let mut last_error = None;
    for profile in H264_PROFILE_ORDER {
        match try_profile(profile) {
            Ok(value) => return Ok((profile, value)),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.expect("H264_PROFILE_ORDER is non-empty"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_values_match_eavench264vprofile() {
        assert_eq!(H264Profile::High.mf_value(), 100);
        assert_eq!(H264Profile::Main.mf_value(), 77);
    }

    #[test]
    fn high_is_preferred_when_accepted() {
        let mut tried = Vec::new();
        let result = negotiate_h264_profile(|profile| {
            tried.push(profile);
            Ok::<_, ()>(profile.mf_value())
        });
        assert_eq!(result, Ok((H264Profile::High, 100)));
        assert_eq!(tried, [H264Profile::High]);
    }

    #[test]
    fn falls_back_to_main_when_high_rejected() {
        let mut tried = Vec::new();
        let result = negotiate_h264_profile(|profile| {
            tried.push(profile);
            if profile == H264Profile::High {
                Err("high rejected")
            } else {
                Ok(())
            }
        });
        assert_eq!(result, Ok((H264Profile::Main, ())));
        assert_eq!(tried, [H264Profile::High, H264Profile::Main]);
    }

    #[test]
    fn reports_last_error_when_all_profiles_rejected() {
        let result: Result<(H264Profile, ()), _> =
            negotiate_h264_profile(|profile| Err(profile.mf_value()));
        assert_eq!(result, Err(77));
    }

    #[test]
    fn cabac_enabled_only_when_supported() {
        assert_eq!(cabac_setting(false, H264Profile::High), None);
        assert_eq!(cabac_setting(true, H264Profile::High), Some(true));
        assert_eq!(cabac_setting(true, H264Profile::Main), Some(true));
    }
}
