use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Fixed-capacity lock-free or single-threaded ring buffer for low-latency per-frame measurement.
#[derive(Debug, Clone)]
pub struct LatencyRecorder {
    samples: Vec<(u64, Option<Instant>)>,
    capacity: usize,
    index: usize,
    count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LatencyStats {
    pub frames: u64,
    pub p50_us: u64,
    pub p95_us: u64,
    pub p99_us: u64,
    pub max_us: u64,
}

impl Default for LatencyRecorder {
    fn default() -> Self {
        Self::new()
    }
}

impl LatencyRecorder {
    pub const DEFAULT_CAPACITY: usize = 4096;

    pub fn new() -> Self {
        Self::with_capacity(Self::DEFAULT_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            samples: Vec::with_capacity(capacity),
            capacity,
            index: 0,
            count: 0,
        }
    }

    /// Records a per-frame sample from capture timestamp (if present, else receive Instant),
    /// decode timestamp, and present timestamp.
    pub fn record_sample(
        &mut self,
        capture_ts: Option<Instant>,
        decode_ts: Instant,
        present_ts: Instant,
    ) {
        let start = capture_ts.unwrap_or(decode_ts);
        let latency = present_ts.saturating_duration_since(start);
        self.record_duration(latency);
    }

    /// Ingests a frame latency as a `Duration`.
    pub fn record_duration(&mut self, latency: Duration) {
        let us = latency.as_micros().min(u64::MAX as u128) as u64;
        self.record_us(us);
    }

    /// Ingests a latency sample directly in microseconds.
    /// Legacy samples have no observation time and are excluded from `stats_at`.
    pub fn record_us(&mut self, latency_us: u64) {
        self.push_sample(latency_us, None);
    }

    fn push_sample(&mut self, latency_us: u64, observed_at: Option<Instant>) {
        if self.samples.len() < self.capacity {
            self.samples.push((latency_us, observed_at));
        } else {
            self.samples[self.index] = (latency_us, observed_at);
        }
        self.index = (self.index + 1) % self.capacity;
        self.count = self.count.saturating_add(1);
    }

    /// Records a latency with an explicit observation time on the query's clock.
    /// Timed and legacy samples share the same capacity and lifetime count.
    /// Eviction follows insertion order, not observation time; out-of-order
    /// observations are allowed and queries exclude observations in their future.
    pub fn record_us_at(&mut self, latency_us: u64, observed_at: Instant) {
        self.push_sample(latency_us, Some(observed_at));
    }

    /// Records a duration at an explicit observation time, with the same
    /// microsecond truncation and saturation as `record_duration`.
    pub fn record_duration_at(&mut self, latency: Duration, observed_at: Instant) {
        let us = latency.as_micros().min(u64::MAX as u128) as u64;
        self.record_us_at(us, observed_at);
    }

    /// Records a same-clock interval observed at its completion. Returns false
    /// without recording or evicting anything if completion precedes the start.
    /// Equal instants are a valid zero-duration sample.
    #[must_use]
    pub fn record_interval(&mut self, started_at: Instant, completed_at: Instant) -> bool {
        let Some(latency) = completed_at.checked_duration_since(started_at) else {
            return false;
        };
        self.record_duration_at(latency, completed_at);
        true
    }

    /// Summarizes retained timed samples with inclusive ages `0..=window`.
    /// `frames` counts only matching samples, not lifetime writes. Empty,
    /// untimed, expired and future-only windows return `None`, not zero latency.
    /// Filtering on every read expires idle data without mutating the ring, so
    /// earlier or wider queries can still inspect retained observations.
    pub fn stats_at(&self, now: Instant, window: Duration) -> Option<LatencyStats> {
        let samples: Vec<u64> = self
            .samples
            .iter()
            .filter_map(|&(latency_us, observed_at)| {
                let age = now.checked_duration_since(observed_at?)?;
                (age <= window).then_some(latency_us)
            })
            .collect();
        let frames = samples.len() as u64;
        (!samples.is_empty()).then(|| Self::stats_from_samples(samples, frames))
    }

    pub fn frame_count(&self) -> u64 {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn stats(&self) -> LatencyStats {
        let samples = self
            .samples
            .iter()
            .map(|&(latency_us, _)| latency_us)
            .collect();
        Self::stats_from_samples(samples, self.count)
    }

    fn stats_from_samples(mut sorted: Vec<u64>, frames: u64) -> LatencyStats {
        if sorted.is_empty() {
            return LatencyStats {
                frames: 0,
                p50_us: 0,
                p95_us: 0,
                p99_us: 0,
                max_us: 0,
            };
        }

        sorted.sort_unstable();

        let len = sorted.len();
        let p50_us = percentile_sorted(&sorted, 0.50);
        let p95_us = percentile_sorted(&sorted, 0.95);
        let p99_us = percentile_sorted(&sorted, 0.99);
        let max_us = sorted[len - 1];

        LatencyStats {
            frames,
            p50_us,
            p95_us,
            p99_us,
            max_us,
        }
    }

    pub fn stats_json(&self) -> String {
        serde_json::to_string(&self.stats()).unwrap_or_else(|_| {
            r#"{"frames":0,"p50_us":0,"p95_us":0,"p99_us":0,"max_us":0}"#.to_string()
        })
    }
}

fn percentile_sorted(sorted: &[u64], percentile: f64) -> u64 {
    let len = sorted.len();
    if len == 0 {
        return 0;
    }
    if len == 1 {
        return sorted[0];
    }
    let rank = ((len - 1) as f64 * percentile.clamp(0.0, 1.0)).round() as usize;
    sorted[rank.min(len - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timed_sample_expires_on_read_without_another_write() {
        // Given a timed sample that initially belongs to the window.
        let observed_at = Instant::now();
        let window = Duration::from_secs(10);
        let mut recorder = LatencyRecorder::new();
        recorder.record_us_at(1234, observed_at);
        assert_eq!(recorder.stats_at(observed_at, window).unwrap().frames, 1);

        // When querying after inactivity, just beyond the inclusive boundary.
        let snapshot = recorder.stats_at(observed_at + window + Duration::from_nanos(1), window);

        // Then stale data is absent, while the legacy lifetime view is unchanged.
        assert_eq!(snapshot, None);
        assert_eq!(recorder.frame_count(), 1);
        assert_eq!(recorder.stats().p50_us, 1234);
    }

    #[test]
    fn timed_window_includes_both_edges_and_uses_only_matching_ranks_and_count() {
        let start = Instant::now();
        let window = Duration::from_secs(10);
        let now = start + window;
        let mut recorder = LatencyRecorder::new();
        // Intentionally not timestamp-sorted; neither excluded edge is a zero.
        recorder.record_us_at(8888, now + Duration::from_nanos(1));
        recorder.record_us_at(10, start);
        recorder.record_us_at(9999, start - Duration::from_nanos(1));
        recorder.record_us_at(30, now);
        recorder.record_us_at(20, start + Duration::from_secs(2));
        recorder.record_us(7777);

        let snapshot = recorder.stats_at(now, window);

        assert_eq!(
            snapshot,
            Some(LatencyStats {
                frames: 3,
                p50_us: 20,
                p95_us: 30,
                p99_us: 30,
                max_us: 30,
            })
        );
        assert_eq!(recorder.frame_count(), 6);
        assert_eq!(recorder.stats().frames, 6);
    }

    #[test]
    fn empty_and_untimed_windows_are_absent() {
        let now = Instant::now();
        let mut recorder = LatencyRecorder::new();
        assert_eq!(recorder.stats_at(now, Duration::MAX), None);
        recorder.record_us(0);
        recorder.record_duration(Duration::ZERO);
        recorder.record_sample(Some(now), now, now);

        let snapshot = recorder.stats_at(now, Duration::MAX);

        assert_eq!(snapshot, None);
        assert_eq!(
            serde_json::to_value(snapshot).unwrap(),
            serde_json::Value::Null
        );
        assert_eq!(recorder.frame_count(), 3);
    }

    #[test]
    fn future_only_window_is_absent_even_with_maximum_duration() {
        let now = Instant::now();
        let mut recorder = LatencyRecorder::new();
        recorder.record_us_at(0, now + Duration::from_nanos(1));

        assert_eq!(recorder.stats_at(now, Duration::MAX), None);
        assert_eq!(recorder.frame_count(), 1);
    }

    #[test]
    fn real_zero_interval_is_present_in_a_zero_width_window() {
        let now = Instant::now();
        let mut recorder = LatencyRecorder::new();

        assert!(recorder.record_interval(now, now));

        let snapshot = recorder.stats_at(now, Duration::ZERO);
        assert_eq!(
            snapshot,
            Some(LatencyStats {
                frames: 1,
                p50_us: 0,
                p95_us: 0,
                p99_us: 0,
                max_us: 0,
            })
        );
        assert_eq!(
            serde_json::to_value(snapshot).unwrap(),
            serde_json::json!({
                "frames": 1, "p50_us": 0, "p95_us": 0, "p99_us": 0, "max_us": 0,
            })
        );
        assert_eq!(
            recorder.stats_at(now + Duration::from_nanos(1), Duration::ZERO),
            None
        );
    }

    #[test]
    fn reversed_interval_does_not_record_or_evict_a_valid_sample() {
        let now = Instant::now();
        let mut recorder = LatencyRecorder::with_capacity(1);
        recorder.record_us_at(42, now);
        let before = recorder.stats();

        let recorded = recorder.record_interval(now + Duration::from_nanos(1), now);

        assert!(!recorded);
        assert_eq!(recorder.stats(), before);
        assert_eq!(recorder.stats_at(now, Duration::ZERO), Some(before));
    }

    #[test]
    fn interval_is_observed_at_completion_not_start() {
        let start = Instant::now();
        let end = start + Duration::from_micros(1234);
        let mut recorder = LatencyRecorder::new();

        assert!(recorder.record_interval(start, end));

        assert_eq!(recorder.stats_at(start, Duration::MAX), None);
        assert_eq!(
            recorder.stats_at(end, Duration::ZERO),
            Some(LatencyStats {
                frames: 1,
                p50_us: 1234,
                p95_us: 1234,
                p99_us: 1234,
                max_us: 1234,
            })
        );
    }

    #[test]
    fn duration_conversion_preserves_legacy_truncation_and_saturation() {
        let now = Instant::now();
        for (duration, expected) in [(Duration::from_nanos(1999), 1), (Duration::MAX, u64::MAX)] {
            let mut recorder = LatencyRecorder::new();
            let mut legacy = LatencyRecorder::new();

            recorder.record_duration_at(duration, now);
            legacy.record_duration(duration);

            assert_eq!(recorder.stats().max_us, expected);
            assert_eq!(recorder.stats_at(now, Duration::ZERO), Some(legacy.stats()));
        }
    }

    #[test]
    fn window_wraparound_retains_only_latest_insertions_with_window_count() {
        let start = Instant::now();
        let mut recorder = LatencyRecorder::with_capacity(4);
        let allocated = recorder.samples.capacity();
        for i in 1..=12 {
            recorder.record_us_at(i, start + Duration::from_secs(i));
        }

        let snapshot = recorder.stats_at(start + Duration::from_secs(12), Duration::from_secs(2));

        assert_eq!(
            snapshot,
            Some(LatencyStats {
                frames: 3,
                p50_us: 11,
                p95_us: 12,
                p99_us: 12,
                max_us: 12,
            })
        );
        assert_eq!(
            recorder.stats(),
            LatencyStats {
                frames: 12,
                p50_us: 11,
                p95_us: 12,
                p99_us: 12,
                max_us: 12,
            }
        );
        assert_eq!(recorder.samples.len(), 4);
        assert_eq!(recorder.samples.capacity(), allocated);
        assert_eq!(recorder.index, 0);
    }

    #[test]
    fn default_capacity_is_a_hard_bound_with_exact_legacy_percentiles() {
        let now = Instant::now();
        let mut recorder = LatencyRecorder::new();
        let allocated = recorder.samples.capacity();
        for i in 1..=8192 {
            recorder.record_us_at(i, now);
        }

        let snapshot = recorder.stats_at(now, Duration::ZERO).unwrap();

        assert_eq!(
            snapshot,
            LatencyStats {
                frames: 4096,
                p50_us: 6145,
                p95_us: 7987,
                p99_us: 8151,
                max_us: 8192,
            }
        );
        assert_eq!(
            recorder.stats(),
            LatencyStats {
                frames: 8192,
                ..snapshot
            }
        );
        assert_eq!(recorder.samples.len(), 4096);
        assert_eq!(recorder.samples.capacity(), allocated);
    }

    #[test]
    fn zero_capacity_still_retains_one_and_legacy_writes_evict_timestamps() {
        let now = Instant::now();
        let mut recorder = LatencyRecorder::with_capacity(0);
        recorder.record_us_at(42, now);
        recorder.record_us_at(43, now);
        assert_eq!(recorder.stats_at(now, Duration::ZERO).unwrap().max_us, 43);

        recorder.record_us(44);

        assert_eq!(recorder.samples.len(), 1);
        assert_eq!(recorder.stats_at(now, Duration::MAX), None);
        assert_eq!(
            recorder.stats(),
            LatencyStats {
                frames: 3,
                p50_us: 44,
                p95_us: 44,
                p99_us: 44,
                max_us: 44,
            }
        );
    }

    #[test]
    fn out_of_order_insertions_evict_by_write_order_not_timestamp() {
        let start = Instant::now();
        let now = start + Duration::from_secs(10);
        let mut recorder = LatencyRecorder::with_capacity(2);
        recorder.record_us_at(9999, now);
        recorder.record_us_at(20, start);

        recorder.record_us_at(30, start + Duration::from_secs(1));

        assert_eq!(
            recorder.stats_at(now, Duration::MAX),
            Some(LatencyStats {
                frames: 2,
                p50_us: 30,
                p95_us: 30,
                p99_us: 30,
                max_us: 30,
            })
        );
        assert_eq!(recorder.stats_at(now, Duration::ZERO), None);
    }

    #[test]
    fn queries_do_not_destroy_retained_data_or_change_legacy_json() {
        let now = Instant::now();
        let mut recorder = LatencyRecorder::new();
        recorder.record_us_at(42, now);
        let before = recorder.stats_json();

        assert_eq!(
            recorder.stats_at(now + Duration::from_secs(100), Duration::ZERO),
            None
        );

        assert_eq!(
            recorder.stats_at(now, Duration::MAX),
            Some(recorder.stats())
        );
        assert_eq!(recorder.stats_json(), before);
        assert_eq!(
            serde_json::from_str::<LatencyStats>(&before)
                .unwrap()
                .frames,
            1
        );
    }

    #[test]
    fn legacy_sample_fallback_and_reversed_chronology_remain_compatible() {
        let start = Instant::now();
        let decode = start + Duration::from_micros(700);
        let end = decode + Duration::from_micros(300);
        let mut recorder = LatencyRecorder::with_capacity(1);
        recorder.record_sample(Some(start), decode, end);
        assert_eq!(recorder.stats().p50_us, 1000);
        recorder.record_sample(None, decode, end);
        assert_eq!(recorder.stats().p50_us, 300);

        recorder.record_sample(Some(end), decode, start);

        assert_eq!(
            recorder.stats(),
            LatencyStats {
                frames: 3,
                p50_us: 0,
                p95_us: 0,
                p99_us: 0,
                max_us: 0,
            }
        );
        assert_eq!(recorder.stats_at(end, Duration::MAX), None);
    }

    #[test]
    fn saturated_lifetime_count_does_not_replace_window_count() {
        let now = Instant::now();
        let mut recorder = LatencyRecorder::with_capacity(1);
        recorder.count = u64::MAX;

        recorder.record_us_at(42, now);

        assert_eq!(recorder.frame_count(), u64::MAX);
        assert_eq!(recorder.stats().frames, u64::MAX);
        assert_eq!(recorder.stats_at(now, Duration::ZERO).unwrap().frames, 1);
    }

    #[test]
    fn empty_recorder_stats() {
        let recorder = LatencyRecorder::new();
        assert_eq!(recorder.frame_count(), 0);
        assert!(recorder.is_empty());
        let stats = recorder.stats();
        assert_eq!(stats.frames, 0);
        assert_eq!(stats.p50_us, 0);
        assert_eq!(stats.p95_us, 0);
        assert_eq!(stats.p99_us, 0);
        assert_eq!(stats.max_us, 0);
        assert_eq!(
            recorder.stats_json(),
            r#"{"frames":0,"p50_us":0,"p95_us":0,"p99_us":0,"max_us":0}"#
        );
    }

    #[test]
    fn single_sample_stats() {
        let mut recorder = LatencyRecorder::new();
        recorder.record_us(1234);
        assert_eq!(recorder.frame_count(), 1);
        assert!(!recorder.is_empty());
        let stats = recorder.stats();
        assert_eq!(stats.frames, 1);
        assert_eq!(stats.p50_us, 1234);
        assert_eq!(stats.p95_us, 1234);
        assert_eq!(stats.p99_us, 1234);
        assert_eq!(stats.max_us, 1234);
        assert_eq!(
            recorder.stats_json(),
            r#"{"frames":1,"p50_us":1234,"p95_us":1234,"p99_us":1234,"max_us":1234}"#
        );
    }

    #[test]
    fn synthetic_1000_sample_distribution_sanity() {
        let mut recorder = LatencyRecorder::with_capacity(1000);
        for i in 1..=1000 {
            recorder.record_us(i);
        }
        assert_eq!(recorder.frame_count(), 1000);
        let stats = recorder.stats();
        assert_eq!(stats.frames, 1000);
        assert!(
            stats.p50_us <= stats.p95_us,
            "p50 ({}) should be <= p95 ({})",
            stats.p50_us,
            stats.p95_us
        );
        assert!(
            stats.p95_us <= stats.p99_us,
            "p95 ({}) should be <= p99 ({})",
            stats.p95_us,
            stats.p99_us
        );
        assert!(
            stats.p99_us <= stats.max_us,
            "p99 ({}) should be <= max ({})",
            stats.p99_us,
            stats.max_us
        );
        assert_eq!(stats.max_us, 1000);
        // With 1..=1000 uniformly spaced, p50 should be around 500, p95 around 950, p99 around 990
        assert!((490..=515).contains(&stats.p50_us));
        assert!((940..=960).contains(&stats.p95_us));
        assert!((980..=995).contains(&stats.p99_us));

        let json = recorder.stats_json();
        let parsed: LatencyStats = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, stats);
    }

    #[test]
    fn ring_buffer_wraparound_maintains_latest_capacity() {
        let mut recorder = LatencyRecorder::with_capacity(10);
        for i in 1..=20 {
            recorder.record_us(i);
        }
        assert_eq!(recorder.frame_count(), 20);
        let stats = recorder.stats();
        assert_eq!(stats.frames, 20);
        // The buffer contains 11..=20
        assert_eq!(stats.max_us, 20);
        assert!(stats.p50_us >= 15);
    }
}
