//! Serial audio continuity, including u32 wrap and rejection of late duplicates.
#[derive(Default)]
pub(crate) struct AudioContinuity {
    last: Option<u32>,
    pcm_bytes: usize,
}

impl AudioContinuity {
    /// None is a duplicate/late packet; large discontinuities restart rather than allocate unbounded silence.
    pub(crate) fn missing(&mut self, id: u32, bytes: usize) -> Option<(u32, usize)> {
        let missing = if let Some(last) = self.last {
            let delta = id.wrapping_sub(last);
            if delta == 0 || delta >= 1 << 31 {
                return None;
            }
            delta - 1
        } else {
            0
        };
        let previous_bytes = self.pcm_bytes;
        self.last = Some(id);
        self.pcm_bytes = bytes;
        Some((missing.min(10), previous_bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn skips_preserve_duration_and_wrap_is_contiguous() {
        let mut state = AudioContinuity::default();
        assert_eq!(state.missing(u32::MAX - 1, 3840), Some((0, 0)));
        assert_eq!(state.missing(1, 3840), Some((2, 3840)));
        assert_eq!(state.missing(1, 3840), None);
        assert_eq!(state.missing(0, 3840), None);
        assert_eq!(state.missing(2, 3840), Some((0, 3840)));
    }
    #[test]
    fn malicious_gap_has_bounded_concealment() {
        let mut state = AudioContinuity::default();
        state.missing(1, 3840);
        assert_eq!(state.missing(1_000_000, 3840), Some((10, 3840)));
    }
}
