//! Shared bounded encoded-frame ordering and recovery for CLI and desktop.
use anyhow::{bail, Result};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

/// Legacy default soft capacity; `FrameQueue::new` keeps its exact behavior.
pub const FRAME_QUEUE_CAPACITY: usize = 4;
/// Upper bound for FPS-scaled capacity.
pub const FRAME_QUEUE_MAX_CAPACITY: usize = 16;
/// Preserve the receive instant across asynchronous decoding.
pub type QueuedFrame = (crate::AssembledFrame, Instant);

/// Presentation policy. All modes keep encoded dependency safety: a frame is
/// only ever discarded together with every queued frame before a keyframe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayoutMode {
    /// Deliver in arrival order with no pacing (legacy behavior).
    #[default]
    Immediate,
    /// Hold each frame 1-2 frame intervals, scaled by inter-arrival jitter.
    Adaptive,
    /// No pacing; skip a stale backlog up to the newest queued keyframe.
    LatestWins,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameQueueConfig {
    /// Soft cap: backlog beyond this skips to an already queued keyframe;
    /// otherwise the transient overage is retained.
    pub capacity: usize,
    /// Hard cap; never below `capacity`.
    pub max_backlog: usize,
    /// When false (legacy), reaching the hard cap clears the backlog and
    /// requests an IDR immediately. When true, the decodable queued prefix is
    /// kept, the incoming tail is treated as lost (dependents suppressed until
    /// a keyframe) and the IDR request is deferred by `idr_defer`.
    pub retain_prefix: bool,
    pub idr_defer: Duration,
    /// Nominal frame interval used by adaptive playout.
    pub frame_interval: Duration,
    pub playout: PlayoutMode,
}

impl Default for FrameQueueConfig {
    fn default() -> Self {
        Self {
            capacity: FRAME_QUEUE_CAPACITY,
            max_backlog: FRAME_QUEUE_CAPACITY,
            retain_prefix: false,
            idr_defer: Duration::ZERO,
            frame_interval: Duration::from_micros(16_667),
            playout: PlayoutMode::Immediate,
        }
    }
}

impl FrameQueueConfig {
    /// Soft cap covers about 25 ms of frames plus one (2..=16), hard cap is
    /// twice that, and an overload IDR is deferred 100 ms.
    pub fn for_fps(fps: u32, playout: PlayoutMode) -> Self {
        let fps = fps.clamp(1, 1000);
        let capacity = (fps as usize * 25 / 1000 + 1).clamp(2, FRAME_QUEUE_MAX_CAPACITY);
        Self {
            capacity,
            max_backlog: capacity * 2,
            retain_prefix: true,
            idr_defer: Duration::from_millis(100),
            frame_interval: Duration::from_secs(1) / fps,
            playout,
        }
    }

    fn normalized(mut self) -> Self {
        self.capacity = self.capacity.max(1);
        self.max_backlog = self.max_backlog.max(self.capacity);
        self
    }
}

pub struct FrameQueue {
    state: std::sync::Mutex<FrameQueueState>,
    ready: std::sync::Condvar,
    trace: crate::ReceiverTrace,
}

struct FrameQueueState {
    config: FrameQueueConfig,
    frames: std::collections::VecDeque<QueuedFrame>,
    last_frame_id: Option<u32>,
    recovering: bool,
    stopped: bool,
    /// Receive instant at which overload recovery began; the keyframe
    /// request fires once recovery has lasted `idr_defer`.
    pending_request_since: Option<Instant>,
    /// Previous admitted (receive instant, host timestamp_ms) for jitter.
    last_arrival: Option<(Instant, u32)>,
    /// RFC 3550 style smoothed inter-arrival jitter, in seconds.
    jitter: f64,
}

/// Outcome of a non-blocking take: a frame, or how long until the head is due.
enum Take {
    Frame(QueuedFrame),
    Wait(Duration),
    Empty,
}

impl FrameQueueState {
    fn playout_delay(&self) -> Duration {
        let interval = self.config.frame_interval;
        let extra = Duration::from_secs_f64((self.jitter * 2.0).min(interval.as_secs_f64()));
        interval + extra
    }

    fn observe_arrival(&mut self, received: Instant, timestamp_ms: u32) {
        if let Some((prev, prev_ts)) = self.last_arrival {
            let arrival = if received >= prev {
                received.duration_since(prev).as_secs_f64()
            } else {
                -prev.duration_since(received).as_secs_f64()
            };
            let sent = f64::from(timestamp_ms.wrapping_sub(prev_ts) as i32) / 1000.0;
            self.jitter += ((arrival - sent).abs() - self.jitter) / 16.0;
        }
        self.last_arrival = Some((received, timestamp_ms));
    }

    /// Drop everything before the newest queued keyframe at index > 0.
    fn skip_to_latest_key(&mut self) -> usize {
        match self.frames.iter().rposition(|f| f.0.header.is_key_frame) {
            Some(key) if key > 0 => {
                self.frames.drain(..key);
                key
            }
            _ => 0,
        }
    }

    fn take(&mut self, now: Instant) -> Take {
        if self.config.playout == PlayoutMode::LatestWins {
            self.skip_to_latest_key();
        }
        let Some(head) = self.frames.front() else {
            return Take::Empty;
        };
        if self.config.playout == PlayoutMode::Adaptive && self.frames.len() <= 2 {
            let due = head.1 + self.playout_delay();
            if now < due {
                return Take::Wait(due - now);
            }
        }
        Take::Frame(self.frames.pop_front().expect("head checked"))
    }
}

impl FrameQueue {
    pub fn new() -> Self {
        Self::with_receiver_trace(crate::ReceiverTrace::default())
    }

    pub fn with_receiver_trace(trace: crate::ReceiverTrace) -> Self {
        Self::with_config(FrameQueueConfig::default(), trace)
    }

    pub fn with_config(config: FrameQueueConfig, trace: crate::ReceiverTrace) -> Self {
        let config = config.normalized();
        Self {
            state: std::sync::Mutex::new(FrameQueueState {
                frames: std::collections::VecDeque::with_capacity(config.max_backlog),
                config,
                last_frame_id: None,
                recovering: false,
                stopped: false,
                pending_request_since: None,
                last_arrival: None,
                jitter: 0.0,
            }),
            ready: std::sync::Condvar::new(),
            trace,
        }
    }

    pub fn config(&self) -> Result<FrameQueueConfig> {
        Ok(self.lock()?.config)
    }

    /// Apply a new configuration (e.g. negotiated FPS) without losing frames.
    pub fn reconfigure(&self, config: FrameQueueConfig) -> Result<()> {
        self.lock()?.config = config.normalized();
        self.ready.notify_all();
        Ok(())
    }

    /// Switch presentation policy at runtime (user setting).
    pub fn set_playout_mode(&self, mode: PlayoutMode) -> Result<()> {
        self.lock()?.config.playout = mode;
        self.ready.notify_all();
        Ok(())
    }

    /// Current adaptive hold time: one frame interval plus twice the jitter,
    /// capped at two frame intervals.
    pub fn playout_delay(&self) -> Result<Duration> {
        Ok(self.lock()?.playout_delay())
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, FrameQueueState>> {
        self.state
            .lock()
            .map_err(|_| anyhow::anyhow!("frame queue poisoned"))
    }

    /// Admit a frame without blocking the producer. True requests one keyframe.
    pub fn push(&self, frame: QueuedFrame) -> Result<bool> {
        let mut state = self.lock()?;
        if state.stopped {
            bail!("frame queue stopped");
        }
        let key = frame.0.header.is_key_frame;
        let id = frame.0.header.frame_id;
        let received = frame.1;
        let now = Instant::now();
        let mut record = crate::ReceiverTraceRecord::new(crate::ReceiverTraceEvent::QueueAdmission);
        record.frame = Some(id);
        record.bytes = frame.0.data.len() as u64;
        let mut discontinuity = false;
        if let Some(last) = state.last_frame_id {
            let advance = id.wrapping_sub(last);
            // Half-range serial ordering rejects late frames across u32 wrap.
            if advance == 0 || advance >= (1 << 31) {
                record.event = crate::ReceiverTraceEvent::QueueLate;
                self.trace.record(now, record);
                return Ok(false);
            }
            discontinuity = advance != 1;
        }
        state.last_frame_id = Some(id);
        state.observe_arrival(received, frame.0.timestamp_ms);
        let mut drop_backlog = discontinuity;
        let mut request_keyframe = false;
        if !discontinuity && state.frames.len() >= state.config.capacity {
            let mut skip =
                crate::ReceiverTraceRecord::new(crate::ReceiverTraceEvent::QueueOverflow);
            skip.frame = Some(id);
            skip.count = if key {
                // A new independent picture supersedes the whole backlog.
                let count = state.frames.len();
                state.frames.clear();
                count
            } else {
                // Only skip to a queued keyframe; never drop a lone reference.
                state.skip_to_latest_key()
            } as u64;
            if skip.count > 0 {
                self.trace.record(now, skip);
            }
            if state.frames.len() >= state.config.max_backlog {
                if state.config.retain_prefix {
                    // Keep the decodable prefix; this frame is the lost tail.
                    record.event = crate::ReceiverTraceEvent::QueueOverflow;
                    record.count = state.frames.len() as u64;
                    if !state.recovering {
                        state.recovering = true;
                        state.pending_request_since = Some(received);
                    }
                } else {
                    drop_backlog = true;
                }
            }
        }
        if drop_backlog {
            record.event = if discontinuity {
                crate::ReceiverTraceEvent::QueueDiscontinuity
            } else {
                crate::ReceiverTraceEvent::QueueOverflow
            };
            record.count = state.frames.len() as u64;
            record.result = i32::from(!state.recovering && !key);
            self.trace.record(now, record);
            // Dropping a reference invalidates the entire pending chain.
            state.frames.clear();
            request_keyframe = !state.recovering && !key;
            state.recovering = true;
            state.pending_request_since = None;
        }
        if let Some(since) = state.pending_request_since {
            if !key && received.saturating_duration_since(since) >= state.config.idr_defer {
                state.pending_request_since = None;
                request_keyframe = true;
            }
        }
        if key {
            state.pending_request_since = None;
        }
        if state.recovering && !key {
            // A retained-prefix lost tail stays attributed as overflow; the
            // legacy drop path already traced its overflow record.
            if drop_backlog || record.event != crate::ReceiverTraceEvent::QueueOverflow {
                record.event = crate::ReceiverTraceEvent::QueueDependent;
            }
            record.result = i32::from(request_keyframe);
            self.trace.record(now, record);
            return Ok(request_keyframe);
        }
        record.event = if state.recovering {
            crate::ReceiverTraceEvent::QueueRecovered
        } else {
            crate::ReceiverTraceEvent::QueueAdmission
        };
        self.trace.record(now, record);
        state.recovering = false;
        state.frames.push_back(frame);
        self.ready.notify_one();
        Ok(request_keyframe)
    }

    pub fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> std::result::Result<QueuedFrame, mpsc::RecvTimeoutError> {
        let deadline = Instant::now().checked_add(timeout);
        let mut state = self
            .state
            .lock()
            .map_err(|_| mpsc::RecvTimeoutError::Disconnected)?;
        loop {
            if state.stopped {
                return Err(mpsc::RecvTimeoutError::Disconnected);
            }
            let now = Instant::now();
            let hint = match state.take(now) {
                Take::Frame(frame) => return Ok(frame),
                Take::Wait(wait) => Some(wait),
                Take::Empty => None,
            };
            let remaining = match deadline {
                Some(deadline) if now >= deadline => return Err(mpsc::RecvTimeoutError::Timeout),
                Some(deadline) => deadline - now,
                None => Duration::MAX,
            };
            let wait = hint.map_or(remaining, |hint| hint.min(remaining));
            state = self
                .ready
                .wait_timeout(state, wait)
                .map_err(|_| mpsc::RecvTimeoutError::Disconnected)?
                .0;
        }
    }

    /// Invalidate dependents of a failed in-flight decode. An already queued
    /// independent keyframe and its successors remain usable.
    pub fn decode_failed(&self) -> Result<bool> {
        let mut state = self.lock()?;
        if state.stopped {
            return Ok(false);
        }
        if let Some(key) = state
            .frames
            .iter()
            .position(|frame| frame.0.header.is_key_frame)
        {
            let mut record =
                crate::ReceiverTraceRecord::new(crate::ReceiverTraceEvent::DecodeFailed);
            record.count = key as u64;
            self.trace.record(Instant::now(), record);
            state.frames.drain(..key);
            state.recovering = false;
            state.pending_request_since = None;
            return Ok(false);
        }
        state.frames.clear();
        // A deferred overload request is due now: no decodable picture remains.
        let request = !state.recovering || state.pending_request_since.take().is_some();
        let mut record = crate::ReceiverTraceRecord::new(crate::ReceiverTraceEvent::DecodeFailed);
        record.result = i32::from(request);
        self.trace.record(Instant::now(), record);
        state.recovering = true;
        Ok(request)
    }

    /// Wait for a due frame or explicit stop, without polling the decoder worker.
    pub fn recv(&self) -> std::result::Result<QueuedFrame, mpsc::RecvError> {
        let mut state = self.state.lock().map_err(|_| mpsc::RecvError)?;
        loop {
            if state.stopped {
                return Err(mpsc::RecvError);
            }
            state = match state.take(Instant::now()) {
                Take::Frame(frame) => return Ok(frame),
                Take::Wait(wait) => {
                    self.ready
                        .wait_timeout(state, wait)
                        .map_err(|_| mpsc::RecvError)?
                        .0
                }
                Take::Empty => self.ready.wait(state).map_err(|_| mpsc::RecvError)?,
            };
        }
    }

    pub fn stop(&self) -> Result<()> {
        let mut state = self.lock()?;
        state.stopped = true;
        state.frames.clear();
        self.ready.notify_all();
        Ok(())
    }
}

impl Default for FrameQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn frame(id: u32, key: bool) -> QueuedFrame {
        (
            crate::AssembledFrame {
                header: ferryx_rd_proto::FrameHeader {
                    frame_id: id,
                    width: 2,
                    height: 2,
                    is_key_frame: key,
                    total_chunks: 1,
                    total_size: 1,
                },
                data: vec![1],
                timestamp_ms: id,
            },
            Instant::now(),
        )
    }

    fn frame_at(id: u32, key: bool, at: Instant) -> QueuedFrame {
        let mut f = frame(id, key);
        f.1 = at;
        f
    }

    fn ids(queue: &FrameQueue) -> Vec<u32> {
        std::iter::from_fn(|| queue.recv_timeout(Duration::ZERO).ok())
            .map(|f| f.0.header.frame_id)
            .collect()
    }

    fn deferred(capacity: usize, playout: PlayoutMode) -> FrameQueue {
        FrameQueue::with_config(
            FrameQueueConfig {
                capacity,
                max_backlog: capacity * 2,
                retain_prefix: true,
                idr_defer: Duration::from_millis(100),
                frame_interval: Duration::from_millis(10),
                playout,
            },
            crate::ReceiverTrace::default(),
        )
    }

    #[test]
    fn capacity_scales_with_fps_and_default_is_legacy() {
        assert_eq!(FrameQueueConfig::default().capacity, FRAME_QUEUE_CAPACITY);
        assert!(!FrameQueueConfig::default().retain_prefix);
        let c60 = FrameQueueConfig::for_fps(60, PlayoutMode::Immediate);
        let c240 = FrameQueueConfig::for_fps(240, PlayoutMode::Immediate);
        assert_eq!((c60.capacity, c60.max_backlog), (2, 4));
        assert_eq!((c240.capacity, c240.max_backlog), (7, 14));
        assert_eq!(
            FrameQueueConfig::for_fps(u32::MAX, PlayoutMode::Immediate).capacity,
            FRAME_QUEUE_MAX_CAPACITY
        );
        assert_eq!(c240.idr_defer, Duration::from_millis(100));
    }

    #[test]
    fn reconfigure_on_stream_confirm_keeps_frames_and_playout_mode() {
        let queue = FrameQueue::with_config(
            FrameQueueConfig::for_fps(60, PlayoutMode::LatestWins),
            crate::ReceiverTrace::default(),
        );
        assert_eq!(queue.config().unwrap().capacity, 2);
        queue.push(frame(0, true)).unwrap();
        queue.push(frame(1, false)).unwrap();
        // The confirmed stream is 240 fps: capacity grows, queued frames survive,
        // and the user's playout mode is carried over by the caller.
        let playout = queue.config().unwrap().playout;
        queue
            .reconfigure(FrameQueueConfig::for_fps(240, playout))
            .unwrap();
        let config = queue.config().unwrap();
        assert_eq!((config.capacity, config.max_backlog), (7, 14));
        assert_eq!(config.playout, PlayoutMode::LatestWins);
        assert_eq!(config.frame_interval, Duration::from_secs(1) / 240);
        assert_eq!(ids(&queue), [0, 1]);
    }

    #[test]
    fn transient_soft_overage_is_retained_without_request() {
        let queue = deferred(2, PlayoutMode::Immediate);
        for id in 0..4 {
            assert!(!queue.push(frame(id, id == 0)).unwrap());
        }
        assert_eq!(ids(&queue), [0, 1, 2, 3]);
    }

    #[test]
    fn overflow_skips_only_to_queued_keyframe() {
        // Legacy default config also benefits: no IDR when a key is queued.
        let queue = FrameQueue::new();
        for (id, key) in [(0, true), (1, false), (2, true), (3, false)] {
            assert!(!queue.push(frame(id, key)).unwrap());
        }
        assert!(!queue.push(frame(4, false)).unwrap());
        assert_eq!(ids(&queue), [2, 3, 4]);
    }

    #[test]
    fn hard_cap_keeps_prefix_and_defers_keyframe_request() {
        let t0 = Instant::now();
        let queue = deferred(2, PlayoutMode::Immediate);
        for id in 0..4 {
            assert!(!queue.push(frame_at(id, id == 0, t0)).unwrap());
        }
        // Hard cap: frame 4 is the lost tail; dependents are suppressed.
        assert!(!queue.push(frame_at(4, false, t0)).unwrap());
        assert!(!queue
            .push(frame_at(5, false, t0 + Duration::from_millis(50)))
            .unwrap());
        // Deferral expires exactly once.
        assert!(queue
            .push(frame_at(6, false, t0 + Duration::from_millis(100)))
            .unwrap());
        assert!(!queue
            .push(frame_at(7, false, t0 + Duration::from_millis(200)))
            .unwrap());
        assert_eq!(ids(&queue), [0, 1, 2, 3]);
        assert!(!queue.push(frame_at(8, true, t0)).unwrap());
        assert!(!queue.push(frame_at(9, false, t0)).unwrap());
        assert_eq!(ids(&queue), [8, 9]);
    }

    #[test]
    fn keyframe_before_deferral_cancels_request() {
        let t0 = Instant::now();
        let queue = deferred(2, PlayoutMode::Immediate);
        for id in 0..5 {
            queue.push(frame_at(id, id == 0, t0)).unwrap();
        }
        assert!(!queue.push(frame_at(5, true, t0)).unwrap());
        assert!(!queue
            .push(frame_at(6, false, t0 + Duration::from_secs(1)))
            .unwrap());
        assert_eq!(ids(&queue), [5, 6]);
    }

    #[test]
    fn decode_failure_during_deferral_requests_immediately() {
        let t0 = Instant::now();
        let queue = deferred(2, PlayoutMode::Immediate);
        for id in 0..5 {
            queue.push(frame_at(id, id == 0, t0)).unwrap();
        }
        queue.recv().unwrap(); // The decoder's failed in-flight keyframe.
        assert!(queue.decode_failed().unwrap());
        assert!(!queue.decode_failed().unwrap());
        assert_eq!(ids(&queue), Vec::<u32>::new());
    }

    #[test]
    fn latest_wins_skips_stale_backlog_only_to_keyframe() {
        let queue = deferred(8, PlayoutMode::LatestWins);
        for (id, key) in [(0, true), (1, false), (2, false), (3, true), (4, false)] {
            queue.push(frame(id, key)).unwrap();
        }
        assert_eq!(ids(&queue), [3, 4]);
        // Without a queued keyframe every dependent P frame is delivered.
        for id in 5..8 {
            queue.push(frame(id, false)).unwrap();
        }
        assert_eq!(ids(&queue), [5, 6, 7]);
    }

    #[test]
    fn adaptive_playout_holds_shallow_queue_and_tracks_jitter() {
        let queue = deferred(8, PlayoutMode::Adaptive);
        assert_eq!(queue.playout_delay().unwrap(), Duration::from_millis(10));
        let t0 = Instant::now() + Duration::from_secs(60);
        // Host spacing 10 ms, arrival spacing 0 and 30 ms: jitter grows.
        let mut a = frame_at(0, true, t0);
        a.0.timestamp_ms = 0;
        let mut b = frame_at(1, false, t0);
        b.0.timestamp_ms = 10;
        let mut c = frame_at(2, false, t0 + Duration::from_millis(30));
        c.0.timestamp_ms = 20;
        for f in [a, b, c] {
            queue.push(f).unwrap();
        }
        let delay = queue.playout_delay().unwrap();
        assert!(delay > Duration::from_millis(10) && delay <= Duration::from_millis(20));
        // Depth 3 > 2: backlog drains without hold, then the future-stamped
        // shallow tail is held.
        assert_eq!(
            queue
                .recv_timeout(Duration::ZERO)
                .unwrap()
                .0
                .header
                .frame_id,
            0
        );
        assert_eq!(
            queue.recv_timeout(Duration::ZERO),
            Err(mpsc::RecvTimeoutError::Timeout)
        );
        // Switching to immediate releases held frames at once.
        queue.set_playout_mode(PlayoutMode::Immediate).unwrap();
        assert_eq!(ids(&queue), [1, 2]);
    }

    #[test]
    fn adaptive_hold_wakes_on_stop() {
        let queue = Arc::new(deferred(8, PlayoutMode::Adaptive));
        queue
            .push(frame_at(
                0,
                true,
                Instant::now() + Duration::from_secs(3600),
            ))
            .unwrap();
        let consumer = queue.clone();
        let (entered_tx, entered_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let state = consumer.state.lock().unwrap();
            entered_tx.send(()).unwrap();
            drop(state);
            consumer.recv()
        });
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        queue.stop().unwrap();
        assert_eq!(worker.join().unwrap(), Err(mpsc::RecvError));
    }

    #[test]
    fn receiver_trace_attributes_queue_overload_and_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let trace = crate::ReceiverTrace::at_path(dir.path().join("trace.json"));
        let queue = FrameQueue::with_receiver_trace(trace.clone());
        for id in 0..6 {
            queue.push(frame(id, false)).unwrap();
        }
        queue.push(frame(6, true)).unwrap();
        let records = trace.snapshot().unwrap().unwrap().records;
        assert!(records
            .iter()
            .any(|r| r.event == crate::ReceiverTraceEvent::QueueOverflow
                && r.frame == Some(4)
                && r.count == 4
                && r.result == 1));
        assert!(records
            .iter()
            .any(|r| r.event == crate::ReceiverTraceEvent::QueueRecovered && r.frame == Some(6)));
        assert_eq!(
            records
                .iter()
                .filter(|r| r.event == crate::ReceiverTraceEvent::QueueDependent)
                .count(),
            2
        );
    }

    #[test]
    fn four_slots_overflow_then_recover_with_original_receive_instant() {
        let queue = FrameQueue::new();
        for id in 0..4 {
            assert!(!queue.push(frame(id, id == 0)).unwrap());
        }
        assert_eq!(queue.state.lock().unwrap().frames.len(), 4);
        assert!(queue.push(frame(4, false)).unwrap());
        assert!(!queue.push(frame(5, false)).unwrap());
        assert_eq!(
            queue.recv_timeout(Duration::ZERO),
            Err(mpsc::RecvTimeoutError::Timeout)
        );
        let key = frame(6, true);
        queue.push(key.clone()).unwrap();
        assert_eq!(queue.recv().unwrap(), key);
    }

    #[test]
    fn duplicate_and_late_frames_are_rejected_across_wrap() {
        let queue = FrameQueue::new();
        for (id, key) in [
            (u32::MAX, true),
            (0, false),
            (u32::MAX, true),
            (0, false),
            (1, false),
        ] {
            assert!(!queue.push(frame(id, key)).unwrap());
        }
        for id in [u32::MAX, 0, 1] {
            assert_eq!(queue.recv().unwrap().0.header.frame_id, id);
        }
        assert_eq!(
            queue.recv_timeout(Duration::ZERO),
            Err(mpsc::RecvTimeoutError::Timeout)
        );
    }

    #[test]
    fn decode_failure_discards_pending_dependents_and_requests_only_once() {
        let queue = FrameQueue::new();
        queue.push(frame(0, true)).unwrap();
        queue.recv().unwrap();
        queue.push(frame(1, false)).unwrap();
        queue.push(frame(2, false)).unwrap();
        assert!(queue.decode_failed().unwrap());
        assert!(!queue.decode_failed().unwrap());
        assert!(!queue.push(frame(3, false)).unwrap());
        assert_eq!(
            queue.recv_timeout(Duration::ZERO),
            Err(mpsc::RecvTimeoutError::Timeout)
        );
        assert!(!queue.push(frame(4, true)).unwrap());
        assert!(!queue.push(frame(5, false)).unwrap());
        assert_eq!(queue.recv().unwrap().0.header.frame_id, 4);
        assert_eq!(queue.recv().unwrap().0.header.frame_id, 5);
    }

    #[test]
    fn decode_failure_preserves_already_queued_independent_chain() {
        let queue = FrameQueue::new();
        queue.push(frame(0, true)).unwrap();
        queue.recv().unwrap();
        for (id, key) in [(1, false), (2, true), (3, false)] {
            queue.push(frame(id, key)).unwrap();
        }
        assert!(!queue.decode_failed().unwrap());
        assert_eq!(queue.recv().unwrap().0.header.frame_id, 2);
        assert_eq!(queue.recv().unwrap().0.header.frame_id, 3);
    }

    #[test]
    fn stop_wakes_blocking_waiter_and_rejects_producer() {
        let queue = Arc::new(FrameQueue::new());
        let consumer = queue.clone();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            // Signal while holding the queue mutex: stop cannot acquire it
            // until Condvar atomically registers this waiter and unlocks it.
            let state = consumer.state.lock().unwrap();
            entered_tx.send(()).unwrap();
            let state = consumer
                .ready
                .wait_while(state, |state| !state.stopped)
                .unwrap();
            done_tx.send(state.stopped).unwrap();
        });
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        queue.stop().unwrap();
        let completion = done_rx.recv_timeout(Duration::from_secs(2));
        // Release even a broken stop implementation before reporting failure.
        queue.ready.notify_all();
        worker.join().unwrap();
        assert!(completion.unwrap());
        assert_eq!(queue.recv(), Err(mpsc::RecvError));
        assert!(queue.push(frame(0, true)).is_err());
        assert!(!queue.decode_failed().unwrap());
    }
}
