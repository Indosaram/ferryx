use std::{
    collections::{HashMap, VecDeque},
    time::{Duration, Instant},
};

use ferryx_rd_proto::TimestampStats;
use serde::{Deserialize, Serialize};

use crate::{LatencyRecorder, LatencyStats};

#[cfg(test)]
mod input_latency_tests {
    use super::*;

    #[test]
    fn bounded_ring_and_reordered_echo_percentiles() {
        let now = Instant::now();
        let mut stats = ReceiverStats::default();
        for sequence in 1..=300 {
            stats.input_sent(sequence, 1000);
        }
        assert_eq!(stats.input_sends.len(), 256);
        assert_eq!(stats.input_sends.front().unwrap().0, 45);
        for (index, duration) in [100, 200, 300, 400, 500].into_iter().enumerate() {
            let id = index as u32;
            let echo = ferryx_rd_proto::InputEcho {
                frame_id: id,
                last_applied_input_seq: 45 + index as u64,
                injection_timestamp_us: 1,
            };
            // Alternate ordering to cover metadata before and after draw submission.
            if index % 2 == 0 {
                stats.frame_arrived(id, 1000 + duration, now);
                stats.present(
                    id,
                    now,
                    1100 + duration,
                    &crate::clock::ClockCorrelation::default(),
                );
                stats.input_echo(echo, now);
            } else {
                stats.input_echo(echo, now);
                stats.frame_arrived(id, 1000 + duration, now);
                stats.present(
                    id,
                    now,
                    1100 + duration,
                    &crate::clock::ClockCorrelation::default(),
                );
            }
            stats.input_echo(echo, now); // Duplicate echo cannot add another sample.
        }
        let snapshot = stats.snapshot(now);
        let received = snapshot.input_to_frame_received_us.unwrap();
        assert_eq!(received.frames, 5);
        assert_eq!(received.p50_us, 300);
        assert_eq!(received.p95_us, 500);
        let draw = snapshot.input_to_draw_submit_us.unwrap();
        assert_eq!(draw.frames, 5);
        assert_eq!(draw.p50_us, 400);
        assert_eq!(draw.p95_us, 600);
    }
}

/// Inclusive sample age window, evaluated on every snapshot (including idle reads).
pub const RECEIVER_STATS_WINDOW: Duration = Duration::from_secs(5);
/// Missing positions finalize at this age, or earlier on hard-window eviction.
pub const RECEIVER_REORDER_GRACE: Duration = Duration::from_millis(100);
/// Maximum entries per new sample ring, serial window, or observation collection.
pub const RECEIVER_TELEMETRY_CAPACITY: usize = 4096;

/// Receiver-local observations; unavailable or expired measurements serialize as null.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ReceiverSnapshot {
    /// Receiver-owned ABR target, absent until the host confirms stream bitrate.
    pub abr_target_bitrate: Option<i32>,
    /// Saturating session-lifetime count after successful UDP authentication.
    pub udp_authenticated_datagrams: u64,
    /// Session-lifetime UDP datagram bytes, including header/nonce/tag, not IP overhead.
    pub udp_authenticated_bytes: u64,
    /// Retained interval bytes * 8 / positive first-to-last arrival seconds.
    /// Bytes at the first timestamp establish the baseline and are excluded.
    pub udp_receive_bps: Option<f64>,
    /// Missing / expected over retained finalized sequence positions, in 0..=1.
    /// None until there are finalized observations; unrelated to legacy ABR loss.
    pub packet_loss_ratio: Option<f64>,
    pub loss_expected_packets: u64,
    pub loss_missing_packets: u64,
    /// Checked host encode_start - capture-ready; not capture-operation duration.
    pub host_ready_to_encode_us: Option<LatencyStats>,
    /// Checked host encode_end - encode_start; encoder pipeline residence.
    pub host_encode_us: Option<LatencyStats>,
    /// Checked host send_complete - encode_end; not network transit time.
    pub host_encode_to_send_complete_us: Option<LatencyStats>,
    /// First authenticated fragment receive to successful assembly completion.
    /// Latest received audio minus video source capture time, on the shared host clock.
    /// This is a source A/V offset report, not a network latency estimate.
    pub audio_video_source_offset_us: Option<i64>,
    pub receive_assembly_us: Option<LatencyStats>,
    pub input_to_frame_received_us: Option<LatencyStats>,
    pub input_to_draw_submit_us: Option<LatencyStats>,
    pub capture_to_present_us: Option<LatencyStats>,
    pub network_us: Option<LatencyStats>,
}

#[derive(Default)]
pub(crate) struct ReceiverStats {
    input_presented: HashMap<u32, (u64, Instant)>,
    input_arrived: HashMap<u32, (u64, Instant)>,
    input_sends: VecDeque<(u64, u64)>,
    input_frames: HashMap<u32, (u64, Instant)>,
    input_echoes: HashMap<u32, (u64, Instant)>,
    input_received: LatencyRecorder,
    input_draw: LatencyRecorder,
    last_echo_sequence: u64,
    datagrams: u64,
    bytes: u64,
    arrivals: VecDeque<(Instant, u64)>,
    loss: PacketLoss,
    host_ids: HashMap<u32, Instant>,
    newest_host_id: Option<(u32, Instant)>,
    ready_to_encode: LatencyRecorder,
    encode: LatencyRecorder,
    encode_to_send: LatencyRecorder,
    audio_source_us: Option<u64>,
    video_source_us: Option<u64>,
    assembly: LatencyRecorder,
    capture_to_present: LatencyRecorder,
    network: LatencyRecorder,
    frame_times: HashMap<u32, (TimestampStats, Instant)>,
    frame_arrivals: HashMap<u32, (u64, Instant)>,
}

impl ReceiverStats {
    pub(crate) fn input_sent(&mut self, sequence: u64, client_us: u64) {
        if self.input_sends.len() == 256 {
            self.input_sends.pop_front();
        }
        self.input_sends.push_back((sequence, client_us));
    }

    pub(crate) fn input_echo(&mut self, echo: ferryx_rd_proto::InputEcho, now: Instant) {
        let sequence = echo.last_applied_input_seq;
        if sequence <= self.last_echo_sequence {
            return;
        }
        let Some((_, sent)) = self
            .input_sends
            .iter()
            .find(|(id, _)| *id == sequence)
            .copied()
        else {
            return;
        };
        self.last_echo_sequence = sequence;
        self.input_frames.retain(|_, (_, at)| !expired(*at, now));
        if self.input_frames.len() == 256 {
            self.input_frames.clear();
        }
        if let Some((presented, at)) = self.input_presented.remove(&echo.frame_id) {
            if !expired(at, now) {
                if let Some(duration) = presented.checked_sub(sent) {
                    self.input_draw.record_us_at(duration, now);
                }
            }
        } else {
            self.input_frames.insert(echo.frame_id, (sent, now));
        }
        if let Some((arrived, _)) = self.input_arrived.get(&echo.frame_id) {
            if let Some(duration) = arrived.checked_sub(sent) {
                self.input_received.record_us_at(duration, now);
            }
        } else {
            self.input_echoes.retain(|_, (_, at)| !expired(*at, now));
            if self.input_echoes.len() == 256 {
                self.input_echoes.clear();
            }
            self.input_echoes.insert(echo.frame_id, (sent, now));
        }
    }

    pub(crate) fn record_audio_source(&mut self, capture_us: Option<u64>) {
        self.audio_source_us = capture_us;
    }

    pub(crate) fn present(
        &mut self,
        id: u32,
        now: Instant,
        client_us: u64,
        clock: &crate::clock::ClockCorrelation,
    ) {
        self.input_presented.retain(|_, (_, at)| !expired(*at, now));
        if self.input_presented.len() == 256 {
            self.input_presented.clear();
        }
        self.input_presented.insert(id, (client_us, now));
        if let Some((sent, received)) = self.input_frames.remove(&id) {
            if !expired(received, now) {
                if let Some(duration) = client_us.checked_sub(sent) {
                    self.input_draw.record_us_at(duration, now);
                }
            }
        }
        let Some((host, received)) = self.frame_times.remove(&id) else {
            return;
        };
        if expired(received, now) {
            return;
        }
        if let Some(capture) = clock.client_time(host.capture_us) {
            if let Some(duration) = client_us.checked_sub(capture) {
                self.capture_to_present.record_us_at(duration, now);
            }
        }
    }

    pub(crate) fn record_network(
        &mut self,
        stats: TimestampStats,
        now: Instant,
        _client_us: u64,
        clock: &crate::clock::ClockCorrelation,
    ) {
        self.frame_times.retain(|_, (_, at)| !expired(*at, now));
        if self.frame_times.len() >= RECEIVER_TELEMETRY_CAPACITY {
            self.frame_times.clear();
        }
        self.frame_times.insert(stats.frame_id, (stats, now));
        if let (Some(sent), Some((arrived, _))) = (
            clock.client_time(stats.send_us),
            self.frame_arrivals.remove(&stats.frame_id),
        ) {
            if let Some(duration) = arrived.checked_sub(sent) {
                self.network.record_us_at(duration, now);
            }
        }
    }

    pub(crate) fn frame_arrived(&mut self, id: u32, client_us: u64, now: Instant) {
        self.input_arrived.retain(|_, (_, at)| !expired(*at, now));
        if self.input_arrived.len() == 256 {
            self.input_arrived.clear();
        }
        self.input_arrived.insert(id, (client_us, now));
        if let Some((sent, received)) = self.input_echoes.remove(&id) {
            if !expired(received, now) {
                if let Some(duration) = client_us.checked_sub(sent) {
                    self.input_received.record_us_at(duration, now);
                }
            }
        }
        self.frame_arrivals.retain(|_, (_, at)| !expired(*at, now));
        if self.frame_arrivals.len() >= RECEIVER_TELEMETRY_CAPACITY {
            self.frame_arrivals.clear();
        }
        self.frame_arrivals.insert(id, (client_us, now));
    }

    pub(crate) fn set_trace(&mut self, trace: crate::ReceiverTrace) {
        self.loss.trace = trace;
    }
    pub(crate) fn observe_datagram(&mut self, sequence: u32, bytes: usize, now: Instant) {
        self.datagrams = self.datagrams.saturating_add(1);
        self.bytes = self.bytes.saturating_add(bytes as u64);
        while self
            .arrivals
            .front()
            .is_some_and(|(at, _)| expired(*at, now))
        {
            self.arrivals.pop_front();
        }
        if self.arrivals.len() == RECEIVER_TELEMETRY_CAPACITY {
            self.arrivals.pop_front();
        }
        self.arrivals.push_back((now, bytes as u64));
        self.loss.observe(sequence, now);
    }

    pub(crate) fn record_host(&mut self, stats: TimestampStats, now: Instant) {
        self.video_source_us = Some(stats.capture_us);
        let (Some(ready), Some(encode), Some(send)) = (
            stats.encode_start_us.checked_sub(stats.capture_us),
            stats.encode_end_us.checked_sub(stats.encode_start_us),
            stats.send_us.checked_sub(stats.encode_end_us),
        ) else {
            return;
        };

        if self.newest_host_id.is_some_and(|(_, at)| expired(at, now)) {
            self.newest_host_id = None;
            self.host_ids.clear();
        }
        let newest = match self.newest_host_id {
            None => stats.frame_id,
            Some((id, _)) if ahead(stats.frame_id, id) => stats.frame_id,
            Some((id, _)) => {
                // Old/ambiguous serial IDs cannot re-enter after hard-window eviction.
                if id.wrapping_sub(stats.frame_id) >= RECEIVER_TELEMETRY_CAPACITY as u32 {
                    return;
                }
                id
            }
        };
        self.host_ids.retain(|id, at| {
            newest.wrapping_sub(*id) < RECEIVER_TELEMETRY_CAPACITY as u32 && !expired(*at, now)
        });
        if self.host_ids.contains_key(&stats.frame_id) {
            return;
        }
        self.host_ids.insert(stats.frame_id, now);
        self.newest_host_id = Some((newest, now));
        self.ready_to_encode.record_us_at(ready, now);
        self.encode.record_us_at(encode, now);
        self.encode_to_send.record_us_at(send, now);
    }

    pub(crate) fn record_assembly(&mut self, start: Instant, end: Instant) {
        // Reversed supplied clocks are absent, not fabricated zero-duration samples.
        if let Some(duration) = end.checked_duration_since(start) {
            self.assembly.record_duration_at(duration, end);
        }
    }

    pub(crate) fn snapshot(&mut self, now: Instant) -> ReceiverSnapshot {
        let (expected, missing) = self.loss.snapshot(now);
        ReceiverSnapshot {
            input_to_frame_received_us: self.input_received.stats_at(now, RECEIVER_STATS_WINDOW),
            input_to_draw_submit_us: self.input_draw.stats_at(now, RECEIVER_STATS_WINDOW),
            capture_to_present_us: self.capture_to_present.stats_at(now, RECEIVER_STATS_WINDOW),
            network_us: self.network.stats_at(now, RECEIVER_STATS_WINDOW),
            abr_target_bitrate: None,
            udp_authenticated_datagrams: self.datagrams,
            udp_authenticated_bytes: self.bytes,
            udp_receive_bps: self.receive_bps(now),
            packet_loss_ratio: (expected != 0).then(|| missing as f64 / expected as f64),
            loss_expected_packets: expected,
            loss_missing_packets: missing,
            host_ready_to_encode_us: self.ready_to_encode.stats_at(now, RECEIVER_STATS_WINDOW),
            host_encode_us: self.encode.stats_at(now, RECEIVER_STATS_WINDOW),
            host_encode_to_send_complete_us: self
                .encode_to_send
                .stats_at(now, RECEIVER_STATS_WINDOW),
            audio_video_source_offset_us: self.audio_source_us.zip(self.video_source_us).map(
                |(audio, video)| {
                    (i128::from(audio) - i128::from(video))
                        .clamp(i128::from(i64::MIN), i128::from(i64::MAX))
                        as i64
                },
            ),
            receive_assembly_us: self.assembly.stats_at(now, RECEIVER_STATS_WINDOW),
        }
    }

    /// Short-window throughput for congestion control, not the five-second UI average.
    pub(crate) fn abr_receive_bps(&self, now: Instant) -> Option<f64> {
        let mut samples = self.arrivals.iter().filter(|(at, _)| {
            now.checked_duration_since(*at)
                .is_some_and(|age| age <= Duration::from_millis(500))
        });
        let &(first, _) = samples.next()?;
        let mut last = first;
        let mut bytes = 0_u64;
        for &(at, count) in samples {
            if at > first {
                bytes += count;
                last = last.max(at);
            }
        }
        let elapsed = last.checked_duration_since(first)?;
        (elapsed >= Duration::from_millis(100)).then(|| bytes as f64 * 8.0 / elapsed.as_secs_f64())
    }

    fn receive_bps(&self, now: Instant) -> Option<f64> {
        let mut times = self
            .arrivals
            .iter()
            .filter(|(at, _)| in_window(*at, now))
            .map(|(at, _)| *at);
        let first = times.next()?;
        let (first, last) = times.fold((first, first), |(first, last), at| {
            (first.min(at), last.max(at))
        });
        let interval = last.checked_duration_since(first)?;
        if interval.is_zero() {
            return None;
        }
        // Arrival at the first timestamp establishes a baseline, not interval bytes.
        let bytes: u64 = self
            .arrivals
            .iter()
            .filter(|(at, _)| *at > first && in_window(*at, now))
            .map(|(_, bytes)| bytes)
            .sum();
        Some(bytes as f64 * 8.0 / interval.as_secs_f64())
    }
}

const fn ahead(sequence: u32, previous: u32) -> bool {
    let delta = sequence.wrapping_sub(previous);
    delta != 0 && delta < (1 << 31)
}

fn expired(at: Instant, now: Instant) -> bool {
    now.checked_duration_since(at)
        .is_some_and(|age| age > RECEIVER_STATS_WINDOW)
}

fn in_window(at: Instant, now: Instant) -> bool {
    now.checked_duration_since(at)
        .is_some_and(|age| age <= RECEIVER_STATS_WINDOW)
}

struct Position {
    received: bool,
    observed_at: Instant,
}

/// A contiguous suffix ending at the highest observed sequence, never beyond it.
/// Finalized positions are immutable; sequence distance, not frame IDs or nonce
/// counters, determines missing positions. Every operation is O(hard window).
#[derive(Default)]
struct PacketLoss {
    trace: crate::ReceiverTrace,
    highest: Option<u32>,
    pending: VecDeque<Position>,
    finalized: VecDeque<(Instant, u64, u64)>,
}

impl PacketLoss {
    fn observe(&mut self, sequence: u32, now: Instant) {
        self.finalize_due(now);
        let Some(highest) = self.highest else {
            self.highest = Some(sequence);
            self.pending.push_back(Position {
                received: true,
                observed_at: now,
            });
            return;
        };
        if !ahead(sequence, highest) {
            let behind = highest.wrapping_sub(sequence) as usize;
            if behind < self.pending.len() {
                let index = self.pending.len() - 1 - behind;
                self.pending[index].received = true;
            } else {
                let mut record =
                    crate::ReceiverTraceRecord::new(crate::ReceiverTraceEvent::LateOutsidePending);
                record.sequence = Some(sequence);
                self.trace.record(now, record);
            }
            return;
        }
        let distance = sequence.wrapping_sub(highest) as usize;
        let overflow = (self.pending.len() + distance).saturating_sub(RECEIVER_TELEMETRY_CAPACITY);
        let old = overflow.min(self.pending.len());
        if distance > 1 {
            let mut record =
                crate::ReceiverTraceRecord::new(crate::ReceiverTraceEvent::GapDiscovered);
            record.sequence = Some(highest.wrapping_add(1));
            record.count = (distance - 1) as u64;
            self.trace.record(now, record);
        }
        let first = highest
            .wrapping_sub(self.pending.len() as u32)
            .wrapping_add(1);
        for (index, position) in self.pending.iter().take(old).enumerate() {
            if !position.received {
                let mut record =
                    crate::ReceiverTraceRecord::new(crate::ReceiverTraceEvent::GapEvicted);
                record.sequence = Some(first.wrapping_add(index as u32));
                self.trace.record(now, record);
            }
        }
        let missing = self
            .pending
            .drain(..old)
            .filter(|position| !position.received)
            .count() as u64;
        // A huge gap evicts its unmaterialized prefix arithmetically, not per packet.
        let skipped = overflow - old;
        if skipped > 0 {
            let mut record = crate::ReceiverTraceRecord::new(crate::ReceiverTraceEvent::GapEvicted);
            record.sequence = Some(highest.wrapping_add(1));
            record.count = skipped as u64;
            self.trace.record(now, record);
        }
        self.record_finalized(now, overflow as u64, missing + skipped as u64);
        let retained = distance - skipped;
        for index in 0..retained {
            self.pending.push_back(Position {
                received: index + 1 == retained,
                observed_at: now,
            });
        }
        self.highest = Some(sequence);
    }

    fn finalize_due(&mut self, now: Instant) {
        while let Some(position) = self.pending.front() {
            let Some(age) = now.checked_duration_since(position.observed_at) else {
                break;
            };
            if age < RECEIVER_REORDER_GRACE {
                break;
            }
            let at = position.observed_at + RECEIVER_REORDER_GRACE;
            let missing = u64::from(!position.received);
            if missing != 0 {
                let mut record =
                    crate::ReceiverTraceRecord::new(crate::ReceiverTraceEvent::GapGrace);
                record.sequence = self.highest.map(|highest| {
                    highest
                        .wrapping_sub(self.pending.len() as u32)
                        .wrapping_add(1)
                });
                self.trace.record(at, record);
            }
            self.pending.pop_front();
            // Reads after idle use the actual deadline, not a fresh snapshot timestamp.
            self.record_finalized(at, 1, missing);
        }
    }

    fn record_finalized(&mut self, at: Instant, expected: u64, missing: u64) {
        if expected == 0 {
            return;
        }
        if let Some((last_at, last_expected, last_missing)) = self.finalized.back_mut() {
            if *last_at == at {
                *last_expected = last_expected.saturating_add(expected);
                *last_missing = last_missing.saturating_add(missing);
                return;
            }
        }
        if self.finalized.len() == RECEIVER_TELEMETRY_CAPACITY {
            self.finalized.pop_front();
        }
        self.finalized.push_back((at, expected, missing));
    }

    fn snapshot(&mut self, now: Instant) -> (u64, u64) {
        self.finalize_due(now);
        self.finalized
            .iter()
            .filter(|(at, _, _)| in_window(*at, now))
            .fold(
                (0_u64, 0_u64),
                |(expected, missing), (_, next_expected, next_missing)| {
                    (
                        expected.saturating_add(*next_expected),
                        missing.saturating_add(*next_missing),
                    )
                },
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    include!("receiver_trace_tests.rs");

    #[test]
    fn abr_rate_uses_recent_traffic_and_expires_when_idle() {
        let start = Instant::now();
        let mut stats = ReceiverStats::default();
        stats.observe_datagram(0, 100_000, start);
        assert_eq!(stats.abr_receive_bps(start), None);
        stats.observe_datagram(1, 1_000, start + Duration::from_secs(1));
        stats.observe_datagram(2, 1_000, start + Duration::from_millis(1100));
        assert_eq!(
            stats.abr_receive_bps(start + Duration::from_millis(1100)),
            Some(80_000.0)
        );
        assert_eq!(stats.abr_receive_bps(start + Duration::from_secs(2)), None);
    }

    #[test]
    fn receiver_trace_retains_first_fixed_capacity_and_counts_overflow() {
        use crate::{
            ReceiverTrace, ReceiverTraceEvent, ReceiverTraceRecord, RECEIVER_TRACE_CAPACITY,
        };
        let dir = tempfile::tempdir().unwrap();
        let trace = ReceiverTrace::at_path(dir.path().join("trace.json"));
        let now = Instant::now();
        for sequence in 0..RECEIVER_TRACE_CAPACITY + 3 {
            let mut record = ReceiverTraceRecord::new(ReceiverTraceEvent::Authenticated);
            record.sequence = Some(sequence as u32);
            record.kind = Some(3);
            record.bytes = 123;
            record.frame = Some(42);
            trace.record(now, record);
        }
        let snapshot = trace.snapshot().unwrap().unwrap();
        assert_eq!(snapshot.records.len(), RECEIVER_TRACE_CAPACITY);
        assert_eq!(snapshot.overflow, 3);
        assert_eq!(snapshot.records[0].sequence, Some(0));
        assert_eq!(snapshot.records.last().unwrap().sequence, Some(65535));
        assert_eq!(snapshot.records[0].frame, Some(42));
        assert_eq!(snapshot.records[0].bytes, 123);
        assert_eq!(snapshot.records[0].kind, Some(3));
        assert!(!dir.path().join("trace.json").exists());
        trace.write(&crate::ReceiverSnapshot::default()).unwrap();
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.path().join("trace.json")).unwrap()).unwrap();
        assert_eq!(value[0]["overflow"], 3);
        assert_eq!(value[0]["records"].as_array().unwrap().len(), 65536);
        assert!(ReceiverTrace::default().snapshot().unwrap().is_none());
    }

    #[test]
    fn audio_video_offset_uses_shared_source_clock_and_legacy_is_absent() {
        let now = Instant::now();
        let mut stats = ReceiverStats::default();
        stats.record_host(host(1), now);
        assert_eq!(stats.snapshot(now).audio_video_source_offset_us, None);
        stats.record_audio_source(Some(140));
        assert_eq!(stats.snapshot(now).audio_video_source_offset_us, Some(40));
        stats.record_audio_source(Some(80));
        assert_eq!(stats.snapshot(now).audio_video_source_offset_us, Some(-20));
        stats.record_audio_source(None);
        assert_eq!(stats.snapshot(now).audio_video_source_offset_us, None);
    }

    fn host(frame_id: u32) -> TimestampStats {
        TimestampStats {
            frame_id,
            capture_us: 100,
            encode_start_us: 110,
            encode_end_us: 130,
            send_us: 160,
        }
    }

    #[test]
    fn receiver_host_checked_stages_duplicates_wrap_reorder_and_idle_expiry() {
        let now = Instant::now();
        let mut stats = ReceiverStats::default();
        for id in [u32::MAX - 1, 0, u32::MAX, 0, u32::MAX - 1] {
            stats.record_host(host(id), now);
        }
        let snapshot = stats.snapshot(now);
        assert_eq!(snapshot.host_ready_to_encode_us.unwrap().frames, 3);
        assert_eq!(snapshot.host_ready_to_encode_us.unwrap().p50_us, 10);
        assert_eq!(snapshot.host_encode_us.unwrap().p50_us, 20);
        assert_eq!(snapshot.host_encode_to_send_complete_us.unwrap().p50_us, 30);
        assert!(stats
            .snapshot(now + RECEIVER_STATS_WINDOW)
            .host_encode_us
            .is_some());
        let later = now + RECEIVER_STATS_WINDOW + Duration::from_nanos(1);
        assert_eq!(stats.snapshot(later).host_encode_us, None);
        stats.record_host(host(0), later);
        assert_eq!(stats.snapshot(later).host_encode_us.unwrap().frames, 1);
    }

    #[test]
    fn receiver_invalid_host_order_does_not_poison_identity_or_admit_partial_stages() {
        let now = Instant::now();
        let mut stats = ReceiverStats::default();
        for invalid in [
            TimestampStats {
                encode_start_us: 99,
                ..host(1)
            },
            TimestampStats {
                encode_end_us: 109,
                ..host(1)
            },
            TimestampStats {
                send_us: 129,
                ..host(1)
            },
        ] {
            stats.record_host(invalid, now);
            assert_eq!(stats.snapshot(now), ReceiverSnapshot::default());
        }
        stats.record_host(host(1), now);
        assert_eq!(stats.snapshot(now).host_encode_us.unwrap().frames, 1);
        stats.record_host(
            TimestampStats {
                frame_id: 2,
                capture_us: u64::MAX,
                encode_start_us: u64::MAX,
                encode_end_us: u64::MAX,
                send_us: u64::MAX,
            },
            now,
        );
        assert_eq!(stats.snapshot(now).host_encode_us.unwrap().frames, 2);
    }

    #[test]
    fn receiver_throughput_uses_only_positive_observed_interval_and_expires() {
        let now = Instant::now();
        let mut stats = ReceiverStats::default();
        stats.observe_datagram(1, 100, now);
        stats.observe_datagram(1, 200, now);
        assert_eq!(stats.snapshot(now).udp_receive_bps, None);
        stats.observe_datagram(2, 300, now + Duration::from_secs(1));
        let snapshot = stats.snapshot(now + Duration::from_secs(1));
        assert_eq!(snapshot.udp_authenticated_datagrams, 3);
        assert_eq!(snapshot.udp_authenticated_bytes, 600);
        // The first timestamp is the baseline: only bytes strictly after it count.
        assert_eq!(snapshot.udp_receive_bps, Some(2400.0));
        assert_eq!(
            stats
                .snapshot(now - Duration::from_nanos(1))
                .udp_receive_bps,
            None
        );
        assert_eq!(
            stats.snapshot(now + Duration::from_secs(7)).udp_receive_bps,
            None
        );
    }

    #[test]
    fn receiver_loss_reorders_wraps_deduplicates_and_never_guesses_edges() {
        let now = Instant::now();
        let mut stats = ReceiverStats::default();
        assert_eq!(stats.snapshot(now).packet_loss_ratio, None);
        for seq in [u32::MAX - 1, 1, u32::MAX, 0, 0] {
            stats.observe_datagram(seq, 1, now);
        }
        assert_eq!(stats.snapshot(now).packet_loss_ratio, None);
        let snapshot = stats.snapshot(now + RECEIVER_REORDER_GRACE);
        assert_eq!(
            (
                snapshot.loss_expected_packets,
                snapshot.loss_missing_packets
            ),
            (4, 0)
        );
        assert_eq!(snapshot.packet_loss_ratio, Some(0.0));
        // Older leading traffic and a late duplicate cannot change finalized counts.
        stats.observe_datagram(u32::MAX - 2, 1, now + RECEIVER_REORDER_GRACE);
        stats.observe_datagram(0, 1, now + RECEIVER_REORDER_GRACE);
        assert_eq!(
            stats
                .snapshot(now + RECEIVER_REORDER_GRACE)
                .loss_expected_packets,
            4
        );
    }

    #[test]
    fn receiver_loss_grace_boundary_late_arrival_and_idle_expiry() {
        let now = Instant::now();
        let mut stats = ReceiverStats::default();
        stats.observe_datagram(10, 1, now);
        stats.observe_datagram(12, 1, now);
        let before = now + RECEIVER_REORDER_GRACE - Duration::from_nanos(1);
        assert_eq!(stats.snapshot(before).packet_loss_ratio, None);
        let boundary = now + RECEIVER_REORDER_GRACE;
        stats.observe_datagram(11, 1, boundary);
        let snapshot = stats.snapshot(boundary);
        assert_eq!(
            (
                snapshot.loss_expected_packets,
                snapshot.loss_missing_packets
            ),
            (3, 1)
        );
        assert_eq!(snapshot.packet_loss_ratio, Some(1.0 / 3.0));
        assert_eq!(
            stats
                .snapshot(boundary + RECEIVER_STATS_WINDOW)
                .loss_expected_packets,
            3
        );
        assert_eq!(
            stats
                .snapshot(boundary + RECEIVER_STATS_WINDOW + Duration::from_nanos(1))
                .packet_loss_ratio,
            None
        );
        // First query after a long idle must not give old gaps a fresh finalization time.
        let mut idle = ReceiverStats::default();
        idle.observe_datagram(10, 1, now);
        idle.observe_datagram(12, 1, now);
        assert_eq!(
            idle.snapshot(now + Duration::from_secs(10))
                .packet_loss_ratio,
            None
        );
    }

    #[test]
    fn receiver_huge_forward_gap_is_arithmetic_and_half_range_is_ambiguous() {
        let now = Instant::now();
        let mut stats = ReceiverStats::default();
        stats.observe_datagram(0, 1, now);
        stats.observe_datagram(1 << 31, 1, now);
        stats.observe_datagram((1 << 31) - 1, 1, now);
        let forced = stats.snapshot(now);
        assert_eq!(
            forced.loss_expected_packets,
            (1_u64 << 31) - RECEIVER_TELEMETRY_CAPACITY as u64
        );
        assert_eq!(
            forced.loss_missing_packets,
            forced.loss_expected_packets - 1
        );
        let finalized = stats.snapshot(now + RECEIVER_REORDER_GRACE);
        assert_eq!(finalized.loss_expected_packets, 1_u64 << 31);
        assert_eq!(finalized.loss_missing_packets, (1_u64 << 31) - 2);
    }

    #[test]
    fn frame_latency_requires_correlated_clock_and_matching_identity() {
        let now = Instant::now();
        let mut stats = ReceiverStats::default();
        let mut clock = crate::clock::ClockCorrelation::default();
        clock.observe(
            ferryx_rd_proto::ClockProbe {
                client_send_us: 0,
                host_receive_us: 1010,
                host_send_us: 1010,
            },
            20,
        );
        let mut frame = host(42);
        frame.capture_us = 1100;
        frame.send_us = 1200;
        stats.frame_arrived(42, 250, now);
        stats.record_network(frame, now, 250, &clock);
        stats.present(41, now, 300, &clock);
        assert_eq!(stats.snapshot(now).capture_to_present_us, None);
        stats.present(42, now, 300, &clock);
        let snapshot = stats.snapshot(now);
        assert_eq!(snapshot.capture_to_present_us.unwrap().p50_us, 200);
        assert_eq!(snapshot.network_us.unwrap().p50_us, 50);
    }

    #[test]
    fn receiver_assembly_observes_completion_and_rejects_reverse_time() {
        let now = Instant::now();
        let mut stats = ReceiverStats::default();
        stats.record_assembly(now, now + Duration::from_micros(1234));
        assert_eq!(stats.snapshot(now).receive_assembly_us, None);
        stats.record_assembly(now, now - Duration::from_nanos(1));
        let sample = stats
            .snapshot(now + Duration::from_micros(1234))
            .receive_assembly_us
            .unwrap();
        assert_eq!(sample.frames, 1);
        assert_eq!(sample.p50_us, 1234);
    }

    #[test]
    fn receiver_samples_and_identity_window_have_hard_caps() {
        let now = Instant::now();
        let mut stats = ReceiverStats::default();
        for id in 0..(RECEIVER_TELEMETRY_CAPACITY * 2) as u32 {
            stats.record_host(host(id), now);
            stats.record_assembly(now, now);
            stats.observe_datagram(id, 1, now);
        }
        // An evicted old ID must not be re-admitted while the serial window is active.
        stats.record_host(
            TimestampStats {
                encode_end_us: 999,
                send_us: 999,
                ..host(0)
            },
            now,
        );
        let snapshot = stats.snapshot(now);
        assert_eq!(
            snapshot.host_encode_us.unwrap().frames,
            RECEIVER_TELEMETRY_CAPACITY as u64
        );
        assert_eq!(snapshot.host_encode_us.unwrap().max_us, 20);
        assert_eq!(
            snapshot.receive_assembly_us.unwrap().frames,
            RECEIVER_TELEMETRY_CAPACITY as u64
        );
        assert_eq!(stats.host_ids.len(), RECEIVER_TELEMETRY_CAPACITY);
        assert_eq!(stats.arrivals.len(), RECEIVER_TELEMETRY_CAPACITY);
        assert_eq!(stats.loss.pending.len(), RECEIVER_TELEMETRY_CAPACITY);
    }

    #[test]
    fn receiver_loss_matches_explicit_positions_for_every_small_gap_mask() {
        let now = Instant::now();
        for start in [0_u32, u32::MAX - 3] {
            for mask in 0_u32..64 {
                let mut stats = ReceiverStats::default();
                stats.observe_datagram(start, 1, now);
                stats.observe_datagram(start.wrapping_add(7), 1, now);
                for offset in (1..7).rev() {
                    if mask & (1 << (offset - 1)) != 0 {
                        stats.observe_datagram(start.wrapping_add(offset), 1, now);
                        stats.observe_datagram(start.wrapping_add(offset), 1, now);
                    }
                }
                let snapshot = stats.snapshot(now + RECEIVER_REORDER_GRACE);
                assert_eq!(snapshot.loss_expected_packets, 8);
                assert_eq!(
                    snapshot.loss_missing_packets,
                    u64::from(6 - mask.count_ones())
                );
            }
        }
    }

    #[test]
    fn receiver_gap_grace_starts_when_gap_is_observed_and_finalized_history_is_bounded() {
        let now = Instant::now();
        let mut loss = PacketLoss::default();
        loss.observe(10, now);
        loss.observe(12, now + Duration::from_millis(10));
        assert_eq!(loss.snapshot(now + RECEIVER_REORDER_GRACE), (1, 0));
        loss.observe(11, now + RECEIVER_REORDER_GRACE + Duration::from_millis(9));
        assert_eq!(
            loss.snapshot(now + RECEIVER_REORDER_GRACE + Duration::from_millis(10)),
            (3, 0)
        );
        for seq in 13..(RECEIVER_TELEMETRY_CAPACITY * 2) as u32 {
            loss.observe(seq, now + Duration::from_millis(u64::from(seq) * 100));
        }
        assert_eq!(loss.finalized.len(), RECEIVER_TELEMETRY_CAPACITY);
        assert!(loss.pending.len() <= RECEIVER_TELEMETRY_CAPACITY);
    }

    #[test]
    fn receiver_future_host_and_assembly_samples_are_absent_and_reset_discards_all_state() {
        let now = Instant::now();
        let mut stats = ReceiverStats::default();
        stats.record_host(host(1), now + Duration::from_nanos(1));
        stats.record_assembly(now, now + Duration::from_nanos(1));
        assert_eq!(stats.snapshot(now), ReceiverSnapshot::default());
        stats.observe_datagram(0, 1, now);
        stats.observe_datagram(2, 1, now);
        stats = ReceiverStats::default();
        assert_eq!(
            stats.snapshot(now + RECEIVER_REORDER_GRACE),
            ReceiverSnapshot::default()
        );
        stats.record_host(host(1), now);
        assert_eq!(stats.snapshot(now).host_encode_us.unwrap().frames, 1);
        stats.observe_datagram(99, 1, now);
        assert_eq!(
            stats
                .snapshot(now + RECEIVER_REORDER_GRACE)
                .loss_expected_packets,
            1
        );
    }
}
