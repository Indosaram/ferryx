//! Remote-desktop session telemetry contract shared by the GUI client and the frontend.
//!
//! The engine side (`ferryx_rd_app`) owns the raw per-frame recorders; this module folds the
//! client-side stages, the receiver snapshot and the presentation counters into one
//! `camelCase` serializable value that the web UI reads through IPC. Every method on
//! [`RdTelemetry`] takes `&mut self`, so callers share it as
//! `Arc<parking_lot::Mutex<RdTelemetry>>`: the async bridge and the std viewer/decode threads
//! serialize their updates through the mutex and each critical section is short and non-blocking.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use ferryx_rd_app::{LatencyRecorder, LatencyStats, ReceiverSnapshot, RECEIVER_STATS_WINDOW};
use serde::{Serialize, Serializer};

/// Sliding window over presented-frame timestamps used to derive `present_fps`.
const PRESENT_WINDOW: Duration = Duration::from_secs(1);

/// Hard bound on retained presentation timestamps; one second at any plausible frame rate fits.
const PRESENT_WINDOW_CAPACITY: usize = 4096;

/// One session's remote-desktop telemetry as the frontend consumes it.
///
/// Enumerated string fields carry a fixed vocabulary so the UI can switch on them:
/// `codec` is `"hevc" | "h264" | "unknown"`, `hw_accel` is
/// `"videotoolbox" | "d3d11va" | "vaapi" | "software"`, `present_backend` is
/// `"wgpu" | "canvas2d"`, `transport_mode` is `"quic_datagram" | "ws_direct" | "ws_relay"`
/// and `playout_mode` is `"immediate" | "latest_wins" | "adaptive"`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RdSessionStats {
    pub rd_session_id: String,
    pub target_fps: u32,
    pub present_fps: f64,
    pub dropped_frames: u64,
    pub codec: String,
    pub hw_accel: String,
    pub present_backend: String,
    pub transport_mode: String,
    pub playout_mode: String,
    pub frame_width: u32,
    pub frame_height: u32,
    pub scale_permille: u16,
    pub audio_active: bool,
    pub audio_underruns: u64,
    pub audio_drops: u64,
    #[serde(serialize_with = "serialize_optional_latency")]
    pub decode_us: Option<LatencyStats>,
    #[serde(serialize_with = "serialize_optional_latency")]
    pub present_us: Option<LatencyStats>,
    #[serde(serialize_with = "serialize_optional_latency")]
    pub total_client_us: Option<LatencyStats>,
    pub udp_receive_bps: Option<f64>,
    pub packet_loss_ratio: Option<f64>,
    pub abr_target_bitrate: Option<i32>,
    #[serde(serialize_with = "serialize_optional_latency")]
    pub host_encode_us: Option<LatencyStats>,
    #[serde(serialize_with = "serialize_optional_latency")]
    pub network_us: Option<LatencyStats>,
    #[serde(serialize_with = "serialize_optional_latency")]
    pub capture_to_present_us: Option<LatencyStats>,
    #[serde(serialize_with = "serialize_optional_latency")]
    pub input_to_draw_submit_us: Option<LatencyStats>,
    pub audio_video_source_offset_us: Option<i64>,
}

/// `LatencyStats` in the contract's `camelCase` spelling (`p95Us`, not `p95_us`).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LatencyStatsView {
    frames: u64,
    p50_us: u64,
    p95_us: u64,
    p99_us: u64,
    max_us: u64,
}

impl From<LatencyStats> for LatencyStatsView {
    fn from(stats: LatencyStats) -> Self {
        Self {
            frames: stats.frames,
            p50_us: stats.p50_us,
            p95_us: stats.p95_us,
            p99_us: stats.p99_us,
            max_us: stats.max_us,
        }
    }
}

fn serialize_optional_latency<S>(
    stats: &Option<LatencyStats>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match stats {
        Some(stats) => LatencyStatsView::from(*stats).serialize(serializer),
        None => serializer.serialize_none(),
    }
}

/// Mutable client-side telemetry accumulator for one remote-desktop session.
///
/// Recommended sharing shape: `Arc<parking_lot::Mutex<RdTelemetry>>`, since updates arrive from
/// the async bridge and from the std viewer/decode threads.
pub struct RdTelemetry {
    rd_session_id: String,
    target_fps: u32,
    dropped_frames: u64,
    codec: String,
    hw_accel: String,
    present_backend: String,
    transport_mode: String,
    playout_mode: String,
    frame_width: u32,
    frame_height: u32,
    scale_permille: u16,
    audio_active: bool,
    audio_underruns: u64,
    audio_drops: u64,
    present_times: VecDeque<Instant>,
    decode: LatencyRecorder,
    present: LatencyRecorder,
    total_client: LatencyRecorder,
}

impl RdTelemetry {
    pub fn new(rd_session_id: impl Into<String>, target_fps: u32) -> Self {
        Self {
            rd_session_id: rd_session_id.into(),
            target_fps,
            dropped_frames: 0,
            codec: "unknown".to_string(),
            hw_accel: "software".to_string(),
            present_backend: "canvas2d".to_string(),
            transport_mode: "ws_direct".to_string(),
            playout_mode: "immediate".to_string(),
            frame_width: 0,
            frame_height: 0,
            scale_permille: 1000,
            audio_active: false,
            audio_underruns: 0,
            audio_drops: 0,
            present_times: VecDeque::new(),
            decode: LatencyRecorder::new(),
            present: LatencyRecorder::new(),
            total_client: LatencyRecorder::new(),
        }
    }

    /// Records one presented frame: its client stages, the negotiated frame size and the
    /// presentation instant that feeds the sliding `present_fps` window.
    pub fn record_frame_at(
        &mut self,
        decode_us: u64,
        present_us: u64,
        total_client_us: u64,
        width: u32,
        height: u32,
        now: Instant,
    ) {
        prune_present_times(&mut self.present_times, now);
        if self.present_times.len() >= PRESENT_WINDOW_CAPACITY {
            self.present_times.pop_front();
        }
        self.present_times.push_back(now);
        self.decode.record_us_at(decode_us, now);
        self.present.record_us_at(present_us, now);
        self.total_client.record_us_at(total_client_us, now);
        self.frame_width = width;
        self.frame_height = height;
    }

    pub fn record_dropped(&mut self, count: u64) {
        self.dropped_frames = self.dropped_frames.saturating_add(count);
    }

    pub fn set_target_fps(&mut self, fps: u32) {
        self.target_fps = fps;
    }

    pub fn set_codec_and_accel(&mut self, codec: &str, hw_accel: &str) {
        self.codec = codec.to_string();
        self.hw_accel = hw_accel.to_string();
    }

    pub fn set_present_backend(&mut self, backend: &str) {
        self.present_backend = backend.to_string();
    }

    pub fn set_transport_mode(&mut self, mode: &str) {
        self.transport_mode = mode.to_string();
    }

    pub fn set_playout_mode(&mut self, mode: &str) {
        self.playout_mode = mode.to_string();
    }

    pub fn set_scale_permille(&mut self, scale_permille: u16) {
        self.scale_permille = scale_permille;
    }

    pub fn set_audio_stats(&mut self, active: bool, underruns: u64, drops: u64) {
        self.audio_active = active;
        self.audio_underruns = underruns;
        self.audio_drops = drops;
    }

    /// Builds the IPC value for `now`, pruning IDLE data instead of replaying stale samples.
    ///
    /// The three client stages are queried over [`RECEIVER_STATS_WINDOW`] like the receiver
    /// stages, so a silent session reports `None` rather than the last frame's latency.
    pub fn snapshot(&mut self, receiver: Option<ReceiverSnapshot>, now: Instant) -> RdSessionStats {
        prune_present_times(&mut self.present_times, now);
        RdSessionStats {
            rd_session_id: self.rd_session_id.clone(),
            target_fps: self.target_fps,
            present_fps: present_fps(&self.present_times),
            dropped_frames: self.dropped_frames,
            codec: self.codec.clone(),
            hw_accel: self.hw_accel.clone(),
            present_backend: self.present_backend.clone(),
            transport_mode: self.transport_mode.clone(),
            playout_mode: self.playout_mode.clone(),
            frame_width: self.frame_width,
            frame_height: self.frame_height,
            scale_permille: self.scale_permille,
            audio_active: self.audio_active,
            audio_underruns: self.audio_underruns,
            audio_drops: self.audio_drops,
            decode_us: self.decode.stats_at(now, RECEIVER_STATS_WINDOW),
            present_us: self.present.stats_at(now, RECEIVER_STATS_WINDOW),
            total_client_us: self.total_client.stats_at(now, RECEIVER_STATS_WINDOW),
            udp_receive_bps: receiver.as_ref().and_then(|r| r.udp_receive_bps),
            packet_loss_ratio: receiver.as_ref().and_then(|r| r.packet_loss_ratio),
            abr_target_bitrate: receiver.as_ref().and_then(|r| r.abr_target_bitrate),
            host_encode_us: receiver.as_ref().and_then(|r| r.host_encode_us),
            network_us: receiver.as_ref().and_then(|r| r.network_us),
            capture_to_present_us: receiver.as_ref().and_then(|r| r.capture_to_present_us),
            input_to_draw_submit_us: receiver
                .as_ref()
                .and_then(|r| r.input_to_draw_submit_us),
            audio_video_source_offset_us: receiver
                .as_ref()
                .and_then(|r| r.audio_video_source_offset_us),
        }
    }
}

/// Drops timestamps older than [`PRESENT_WINDOW`]; future timestamps are left alone.
fn prune_present_times(times: &mut VecDeque<Instant>, now: Instant) {
    while let Some(front) = times.front() {
        match now.checked_duration_since(*front) {
            Some(age) if age > PRESENT_WINDOW => {
                times.pop_front();
            }
            _ => break,
        }
    }
}

/// Frames presented per second over the retained window.
///
/// Zero retained frames is `0.0`; a single frame is `1.0`; otherwise `(count - 1)` over the
/// shortest-to-longest timestamp span, i.e. the instantaneous rate rather than a lifetime
/// average. Extremes are taken over the whole deque, so out-of-order insertions cannot skew it,
/// and the result is always finite: coincident timestamps fall back to the frame count.
fn present_fps(times: &VecDeque<Instant>) -> f64 {
    let len = times.len();
    if len == 0 {
        return 0.0;
    }
    if len == 1 {
        return 1.0;
    }
    let (Some(first), Some(last)) = (times.iter().min(), times.iter().max()) else {
        return 0.0;
    };
    let rate = match last.checked_duration_since(*first) {
        Some(span) if !span.is_zero() => (len - 1) as f64 / span.as_secs_f64(),
        _ => len as f64,
    };
    if rate.is_finite() {
        rate
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME_SPACING_US: u64 = 13_333;

    fn stats_of(p50_us: u64, p95_us: u64, p99_us: u64, max_us: u64) -> LatencyStats {
        LatencyStats {
            frames: 75,
            p50_us,
            p95_us,
            p99_us,
            max_us,
        }
    }

    /// 75 frames at 13.333 ms spacing: the 75 Hz target the plan measures.
    fn record_75_frames(telemetry: &mut RdTelemetry, start: Instant) -> Instant {
        for index in 0..75_u64 {
            telemetry.record_frame_at(
                1_000 + index,
                2_000 + index,
                3_000 + index,
                1920,
                1080,
                start + Duration::from_micros(index * FRAME_SPACING_US),
            );
        }
        start + Duration::from_micros(74 * FRAME_SPACING_US)
    }

    #[test]
    fn seventy_five_hz_window_reports_target_rate_and_exact_percentiles() {
        let start = Instant::now();
        let mut telemetry = RdTelemetry::new("rd-75hz", 75);
        let last = record_75_frames(&mut telemetry, start);

        let stats = telemetry.snapshot(None, last);

        // 74 spans of 13.333 ms inside the 1 s window.
        assert!(
            (73.0..=77.0).contains(&stats.present_fps),
            "present_fps was {}",
            stats.present_fps
        );
        // Nearest-rank over 75 sorted samples: rank = round(74 * p).
        assert_eq!(stats.decode_us, Some(stats_of(1_037, 1_070, 1_073, 1_074)));
        assert_eq!(stats.present_us, Some(stats_of(2_037, 2_070, 2_073, 2_074)));
        assert_eq!(
            stats.total_client_us,
            Some(stats_of(3_037, 3_070, 3_073, 3_074))
        );
        assert_eq!(stats.frame_width, 1920);
        assert_eq!(stats.frame_height, 1080);
        assert_eq!(stats.target_fps, 75);
        assert_eq!(stats.rd_session_id, "rd-75hz");
    }

    #[test]
    fn snapshot_serializes_camel_case_keys_and_nested_percentiles() {
        let start = Instant::now();
        let mut telemetry = RdTelemetry::new("rd-keys", 75);
        telemetry.set_codec_and_accel("hevc", "videotoolbox");
        telemetry.set_transport_mode("quic_datagram");
        let last = record_75_frames(&mut telemetry, start);
        let receiver = ReceiverSnapshot {
            capture_to_present_us: Some(stats_of(10, 20, 30, 40)),
            ..ReceiverSnapshot::default()
        };

        let value = serde_json::to_value(telemetry.snapshot(Some(receiver), last)).unwrap();

        assert_eq!(value["rdSessionId"], serde_json::json!("rd-keys"));
        assert!(value["presentFps"].as_f64().unwrap() >= 73.0);
        assert_eq!(value["hwAccel"], serde_json::json!("videotoolbox"));
        assert_eq!(value["transportMode"], serde_json::json!("quic_datagram"));
        assert_eq!(value["decodeUs"]["p95Us"], serde_json::json!(1_070));
        assert_eq!(value["decodeUs"]["p50Us"], serde_json::json!(1_037));
        assert_eq!(value["totalClientUs"]["maxUs"], serde_json::json!(3_074));
        assert_eq!(value["captureToPresentUs"]["p95Us"], serde_json::json!(20));
        assert_eq!(value["presentUs"]["p95Us"], serde_json::json!(2_070));
        assert_eq!(value["audioActive"], serde_json::json!(false));
        assert_eq!(value["frameWidth"], serde_json::json!(1920));
        // No snake_case keys survive the rename.
        assert!(value.get("present_us").is_none());
        assert!(value.get("decode_us").is_none());
    }

    #[test]
    fn idle_session_expires_frames_and_client_stages_without_nan_or_stale_data() {
        let start = Instant::now();
        let mut telemetry = RdTelemetry::new("rd-idle", 75);
        let last = record_75_frames(&mut telemetry, start);
        assert!(telemetry.snapshot(None, last).present_fps >= 73.0);

        let stats = telemetry.snapshot(None, last + Duration::from_secs(10));

        assert_eq!(stats.present_fps, 0.0);
        assert!(stats.present_fps.is_finite());
        assert_eq!(stats.decode_us, None);
        assert_eq!(stats.present_us, None);
        assert_eq!(stats.total_client_us, None);
        // Counters keep their lifetime meaning; only windows expire.
        assert_eq!(stats.target_fps, 75);
    }

    #[test]
    fn present_fps_is_one_for_a_single_frame_and_zero_before_any_frame() {
        let start = Instant::now();
        let mut telemetry = RdTelemetry::new("rd-edged", 75);
        let empty = telemetry.snapshot(None, start);
        assert_eq!(empty.present_fps, 0.0);
        assert_eq!(empty.decode_us, None);

        telemetry.record_frame_at(1, 2, 3, 640, 480, start);
        assert_eq!(telemetry.snapshot(None, start).present_fps, 1.0);

        // Coincident timestamps must stay finite instead of dividing by a zero span.
        telemetry.record_frame_at(1, 2, 3, 640, 480, start);
        let burst = telemetry.snapshot(None, start).present_fps;
        assert!(burst.is_finite() && burst > 0.0, "burst rate was {burst}");
    }

    #[test]
    fn receiver_snapshot_fields_merge_verbatim_and_absence_stays_null() {
        let start = Instant::now();
        let mut telemetry = RdTelemetry::new("rd-merge", 75);
        telemetry.record_frame_at(100, 200, 300, 1280, 720, start);
        let receiver = ReceiverSnapshot {
            abr_target_bitrate: Some(4_000_000),
            udp_receive_bps: Some(5_000_000.0),
            packet_loss_ratio: Some(0.25),
            host_encode_us: Some(stats_of(1, 2, 3, 4)),
            network_us: Some(stats_of(5, 6, 7, 8)),
            capture_to_present_us: Some(stats_of(9, 10, 11, 12)),
            input_to_draw_submit_us: Some(stats_of(13, 14, 15, 16)),
            audio_video_source_offset_us: Some(-1_500),
            ..ReceiverSnapshot::default()
        };

        let merged = telemetry.snapshot(Some(receiver), start);

        assert_eq!(merged.abr_target_bitrate, Some(4_000_000));
        assert_eq!(merged.udp_receive_bps, Some(5_000_000.0));
        assert_eq!(merged.packet_loss_ratio, Some(0.25));
        assert_eq!(merged.host_encode_us, Some(stats_of(1, 2, 3, 4)));
        assert_eq!(merged.network_us, Some(stats_of(5, 6, 7, 8)));
        assert_eq!(merged.capture_to_present_us, Some(stats_of(9, 10, 11, 12)));
        assert_eq!(merged.input_to_draw_submit_us, Some(stats_of(13, 14, 15, 16)));
        assert_eq!(merged.audio_video_source_offset_us, Some(-1_500));

        let absent = telemetry.snapshot(None, start);
        assert_eq!(absent.abr_target_bitrate, None);
        assert_eq!(absent.udp_receive_bps, None);
        assert_eq!(absent.packet_loss_ratio, None);
        assert_eq!(absent.host_encode_us, None);
        assert_eq!(absent.network_us, None);
        assert_eq!(absent.capture_to_present_us, None);
        assert_eq!(absent.input_to_draw_submit_us, None);
        assert_eq!(absent.audio_video_source_offset_us, None);
    }

    #[test]
    fn setters_populate_mode_counters_and_defaults() {
        let now = Instant::now();
        let mut telemetry = RdTelemetry::new("rd-setters", 60);
        let fresh = telemetry.snapshot(None, now);
        assert_eq!(fresh.codec, "unknown");
        assert_eq!(fresh.hw_accel, "software");
        assert_eq!(fresh.present_backend, "canvas2d");
        assert_eq!(fresh.transport_mode, "ws_direct");
        assert_eq!(fresh.playout_mode, "immediate");
        assert_eq!(fresh.scale_permille, 1000);
        assert_eq!(fresh.target_fps, 60);
        assert_eq!(fresh.dropped_frames, 0);
        assert!(!fresh.audio_active);

        telemetry.set_target_fps(75);
        telemetry.set_codec_and_accel("h264", "d3d11va");
        telemetry.set_present_backend("wgpu");
        telemetry.set_transport_mode("ws_relay");
        telemetry.set_playout_mode("adaptive");
        telemetry.set_scale_permille(750);
        telemetry.set_audio_stats(true, 2, 3);
        telemetry.record_dropped(4);
        telemetry.record_dropped(u64::MAX);

        let stats = telemetry.snapshot(None, now);
        assert_eq!(stats.target_fps, 75);
        assert_eq!(stats.codec, "h264");
        assert_eq!(stats.hw_accel, "d3d11va");
        assert_eq!(stats.present_backend, "wgpu");
        assert_eq!(stats.transport_mode, "ws_relay");
        assert_eq!(stats.playout_mode, "adaptive");
        assert_eq!(stats.scale_permille, 750);
        assert!(stats.audio_active);
        assert_eq!(stats.audio_underruns, 2);
        assert_eq!(stats.audio_drops, 3);
        assert_eq!(stats.dropped_frames, u64::MAX);
    }
}
