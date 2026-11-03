use std::time::{Duration, Instant};

/// Loss above this ratio (frame or chunk) is congestion.
const LOSS_THRESHOLD: f64 = 0.05;
/// One-way delay growing faster than this (ms of queueing per second) is overuse.
const DELAY_GRADIENT_OVERUSE_MS_PER_S: f64 = 10.0;
/// Delay growing faster than this blocks increases without forcing a decrease.
const DELAY_GRADIENT_HOLD_MS_PER_S: f64 = 2.0;
/// Multiplicative fast decrease applied to the current target.
const REDUCTION_FACTOR: f64 = 0.75;
/// Under overuse the target never exceeds this fraction of the measured receive rate.
const RECEIVE_RATE_DECREASE_FACTOR: f64 = 0.85;
/// A receive-rate cap never cuts deeper than this fraction of the current
/// target in one step, so an app-limited (idle screen) measurement cannot
/// collapse the bitrate.
const MAX_SINGLE_DECREASE: f64 = 0.5;
/// The last-known-good ceiling sits this far below the rate that congested.
const LAST_KNOWN_GOOD_FACTOR: f64 = 0.9;
/// Additive-ish recovery: ~5% per second of uncongested signals.
const RECOVERY_FACTOR: f64 = 1.05;
const RECOVERY_INTERVAL: Duration = Duration::from_secs(1);
/// A previously congested rate is only a temporary ceiling. Probe beyond it
/// after ten seconds of continuously clean signals, still receive-rate bounded.
const PROBE_AFTER: Duration = Duration::from_secs(10);
/// Increases never outrun what the network demonstrably delivers by more than this.
const RECEIVE_RATE_INCREASE_HEADROOM: f64 = 1.5;
/// Minimum spacing between two fast decreases, so one congestion episode seen
/// through a multi-second loss window is not punished repeatedly.
const DECREASE_HOLD: Duration = Duration::from_secs(1);

fn overused(loss_ratio: f64, delay_gradient_ms_per_s: f64) -> bool {
    loss_ratio > LOSS_THRESHOLD || delay_gradient_ms_per_s > DELAY_GRADIENT_OVERUSE_MS_PER_S
}

/// Adaptive bitrate controller: loss and delay-gradient fast decrease,
/// ~5%/second recovery bounded by receive rate and a last-known-good ceiling.
#[derive(Debug, Clone)]
pub struct AbrController {
    current_bitrate: i32,
    min_bitrate: i32,
    max_bitrate: i32,
    last_known_good: i32,
    stable_since: Option<Instant>,
    last_change: Option<Instant>,
    last_decrease: Option<Instant>,
}

impl AbrController {
    pub fn new(initial_bitrate: i32, min_bitrate: i32, max_bitrate: i32) -> Self {
        assert!(min_bitrate > 0);
        assert!(min_bitrate <= initial_bitrate && initial_bitrate <= max_bitrate);
        Self {
            current_bitrate: initial_bitrate,
            min_bitrate,
            max_bitrate,
            last_known_good: initial_bitrate,
            stable_since: None,
            last_change: None,
            last_decrease: None,
        }
    }

    pub fn current_bitrate(&self) -> i32 {
        self.current_bitrate
    }

    /// Ceiling that recovery climbs back to before probing higher.
    pub fn last_known_good(&self) -> i32 {
        self.last_known_good
    }

    /// Loss-only evaluation: `evaluate_signals(loss_ratio, 0.0, None, now)`.
    pub fn evaluate(&mut self, loss_ratio: f64, now: Instant) -> Option<i32> {
        self.evaluate_signals(loss_ratio, 0.0, None, now)
    }

    /// Returns a new target bitrate when a control update must be emitted.
    ///
    /// - `loss_ratio`: worst of frame and chunk loss, `0.0..=1.0`.
    /// - `delay_gradient_ms_per_s`: one-way delay slope; positive means queues
    ///   are building. Pass `0.0` when unknown.
    /// - `receive_rate_bps`: measured media receive rate in bits per second.
    pub fn evaluate_signals(
        &mut self,
        loss_ratio: f64,
        delay_gradient_ms_per_s: f64,
        receive_rate_bps: Option<f64>,
        now: Instant,
    ) -> Option<i32> {
        let receive_rate_bps = receive_rate_bps.filter(|rate| rate.is_finite() && *rate >= 0.0);
        if overused(loss_ratio, delay_gradient_ms_per_s) {
            self.stable_since = None;
            return self.decrease(receive_rate_bps, now);
        }
        if delay_gradient_ms_per_s > DELAY_GRADIENT_HOLD_MS_PER_S {
            self.stable_since = None;
            return None;
        }

        // Recover conservatively first, then probe toward the user cap once
        // the path has stayed clean long enough to outlive a congestion burst.
        let stable_since = *self.stable_since.get_or_insert(now);
        let since = self
            .last_change
            .map_or(stable_since, |c| c.max(stable_since));
        if now.saturating_duration_since(since) < RECOVERY_INTERVAL {
            return None;
        }
        let mut ceiling = if now.saturating_duration_since(stable_since) >= PROBE_AFTER {
            self.max_bitrate
        } else {
            self.last_known_good.min(self.max_bitrate)
        };
        if let Some(rate) = receive_rate_bps {
            let headroom = (rate * RECEIVE_RATE_INCREASE_HEADROOM).min(i32::MAX as f64);
            ceiling = ceiling.min((headroom as i32).max(self.current_bitrate));
        }
        if self.current_bitrate >= ceiling {
            return None;
        }
        let step = ((self.current_bitrate as f64 * RECOVERY_FACTOR).ceil() as i32)
            .max(self.current_bitrate.saturating_add(1));
        self.update(step.min(ceiling), now)
    }

    fn decrease(&mut self, receive_rate_bps: Option<f64>, now: Instant) -> Option<i32> {
        if self
            .last_decrease
            .is_some_and(|at| now.saturating_duration_since(at) < DECREASE_HOLD)
        {
            return None;
        }
        let current = self.current_bitrate as f64;
        let mut target = current * REDUCTION_FACTOR;
        let mut delivered = current;
        if let Some(rate) = receive_rate_bps {
            delivered = delivered.min(rate);
            target = target
                .min((rate * RECEIVE_RATE_DECREASE_FACTOR).max(current * MAX_SINGLE_DECREASE));
        }
        let target = (target as i32).max(self.min_bitrate);
        self.last_known_good =
            ((delivered * LAST_KNOWN_GOOD_FACTOR) as i32).clamp(target, self.max_bitrate);
        self.last_decrease = Some(now);
        self.update(target, now)
    }

    fn update(&mut self, target: i32, now: Instant) -> Option<i32> {
        if target == self.current_bitrate {
            return None;
        }
        self.current_bitrate = target;
        self.last_change = Some(now);
        Some(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const M: i32 = 1_000_000;

    #[test]
    fn delay_gradient_overuse_triggers_fast_decrease_once_per_hold() {
        let start = Instant::now();
        let mut abr = AbrController::new(8 * M, M, 20 * M);
        assert_eq!(abr.evaluate_signals(0.0, 25.0, None, start), Some(6 * M));
        let half = start + Duration::from_millis(500);
        assert_eq!(abr.evaluate_signals(0.0, 25.0, None, half), None);
        let later = start + Duration::from_secs(1);
        assert_eq!(
            abr.evaluate_signals(0.0, 25.0, None, later),
            Some(4_500_000)
        );
    }

    #[test]
    fn decrease_uses_receive_rate_and_sets_last_known_good() {
        let start = Instant::now();
        let mut abr = AbrController::new(8 * M, M, 20 * M);
        assert_eq!(
            abr.evaluate_signals(0.2, 0.0, Some(5_000_000.0), start),
            Some(4_250_000)
        );
        assert_eq!(abr.last_known_good(), 4_500_000);

        // An app-limited near-zero rate cannot cut below half in one step.
        let mut abr = AbrController::new(8 * M, M, 20 * M);
        assert_eq!(
            abr.evaluate_signals(0.2, 0.0, Some(10_000.0), start),
            Some(4 * M)
        );
    }

    #[test]
    fn recovery_climbs_five_percent_per_second_to_last_known_good() {
        let start = Instant::now();
        let mut abr = AbrController::new(8 * M, M, 20 * M);
        assert_eq!(
            abr.evaluate_signals(0.5, 0.0, Some(8e6), start),
            Some(6 * M)
        );
        assert_eq!(abr.last_known_good(), 7_200_000);
        let at = |s| start + Duration::from_secs(s);
        assert_eq!(abr.evaluate_signals(0.0, 0.0, None, at(1)), None);
        assert_eq!(abr.evaluate_signals(0.0, 0.0, None, at(2)), Some(6_300_000));
        assert_eq!(abr.evaluate_signals(0.0, 0.0, None, at(3)), Some(6_615_000));
        assert_eq!(abr.evaluate_signals(0.0, 0.0, None, at(4)), Some(6_945_750));
        assert_eq!(abr.evaluate_signals(0.0, 0.0, None, at(5)), Some(7_200_000));
        assert_eq!(abr.evaluate_signals(0.0, 0.0, None, at(6)), None);
        assert_eq!(abr.evaluate_signals(0.0, 0.0, None, at(10)), None);
        assert_eq!(
            abr.evaluate_signals(0.0, 0.0, None, at(11)),
            Some(7_560_000)
        );
        for second in 12..60 {
            abr.evaluate_signals(0.0, 0.0, None, at(second));
        }
        assert_eq!(abr.current_bitrate(), 20 * M);
    }

    #[test]
    fn hold_zone_blocks_recovery_without_decreasing() {
        let start = Instant::now();
        let mut abr = AbrController::new(8 * M, M, 20 * M);
        assert_eq!(abr.evaluate(0.5, start), Some(6 * M));
        for second in 1..10 {
            let now = start + Duration::from_secs(second);
            assert_eq!(abr.evaluate_signals(0.0, 5.0, None, now), None);
        }
        assert_eq!(abr.current_bitrate(), 6 * M);
    }

    #[test]
    fn recovery_is_capped_by_receive_rate_headroom() {
        let start = Instant::now();
        let mut abr = AbrController::new(8 * M, M, 20 * M);
        assert_eq!(abr.evaluate(0.5, start), Some(6 * M));
        let rate = Some(4_100_000.0);
        abr.evaluate_signals(0.0, 0.0, rate, start + Duration::from_secs(1));
        assert_eq!(
            abr.evaluate_signals(0.0, 0.0, rate, start + Duration::from_secs(2)),
            Some(6_150_000)
        );
        assert_eq!(
            abr.evaluate_signals(0.0, 0.0, rate, start + Duration::from_secs(3)),
            None
        );
    }
}
