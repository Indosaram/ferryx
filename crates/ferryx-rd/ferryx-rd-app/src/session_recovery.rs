use super::*;

impl ClientSession {
    /// Reports the same recovered-plus-unrecovered loss used by ABR, so XOR
    /// repair never hides congestion from the sender's redundancy controller.
    /// NACKs and periodic loss-only reports use the sole GCM sequence owner.
    pub(super) fn send_recovery_feedback(
        &self,
        now: std::time::Instant,
    ) -> Result<(), SessionError> {
        let report = {
            let mut state = self.state.lock().map_err(|_| SessionError::Poisoned)?;
            if state.state != SessionState::Ready
                || !self.config.capabilities.contains(Capabilities::FEC_NACK)
                || !state
                    .server
                    .as_ref()
                    .is_some_and(|s| s.capabilities.contains(Capabilities::FEC_NACK))
            {
                return Ok(());
            }
            let loss = state.frames.congestion_loss_ratio(now);
            let rtt = state.parity.stats.rtt_smoothed_ms.and_then(|ms| {
                (ms.is_finite() && ms >= 0.0).then(|| Duration::from_secs_f64(ms / 1000.0))
            });
            if let Some(nack) =
                rtt.and_then(|rtt| state.frames.recovery_nack_with_rtt(now, loss, rtt))
            {
                state.recovery_feedback_at = Some(now);
                Some(nack)
            } else if state.recovery_feedback_at.map_or(true, |last| {
                now.saturating_duration_since(last) >= Duration::from_millis(200)
            }) {
                state.recovery_feedback_at = Some(now);
                Some(ferryx_rd_proto::FrameNack {
                    frame_id: 0,
                    loss_permille: (loss.clamp(0.0, 1.0) * 1000.0).round() as u16,
                    rtt_ms: rtt.map_or(u16::MAX, |rtt| {
                        rtt.as_micros().div_ceil(1000).min(u128::from(u16::MAX)) as u16
                    }),
                    missing: Vec::new(),
                })
            } else {
                None
            }
        };
        if let Some(report) = report {
            self.send_udp(PacketType::FrameNack, &report.encode()?)?;
        }
        Ok(())
    }
}
