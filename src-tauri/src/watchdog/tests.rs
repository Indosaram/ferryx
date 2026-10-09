use super::collector::exercise_evidence_collector_seam;
use super::*;
use std::sync::Mutex;

struct MockClock {
    now: Arc<Mutex<(Instant, SystemTime)>>,
}

impl MockClock {
    fn new() -> Self {
        Self {
            now: Arc::new(Mutex::new((Instant::now(), SystemTime::now()))),
        }
    }

    fn advance(&self, duration: Duration) {
        let mut guard = self.now.lock().unwrap();
        guard.0 += duration;
        guard.1 += duration;
    }
}

impl WatchdogClock for MockClock {
    fn now_monotonic(&self) -> Instant {
        self.now.lock().unwrap().0
    }

    fn now_system(&self) -> SystemTime {
        self.now.lock().unwrap().1
    }
}

struct MockDispatcher {
    callbacks: Arc<Mutex<Vec<Box<dyn FnOnce() + Send>>>>,
    total_dispatched: Arc<AtomicU64>,
}

impl MockDispatcher {
    fn new() -> Self {
        Self {
            callbacks: Arc::new(Mutex::new(Vec::new())),
            total_dispatched: Arc::new(AtomicU64::new(0)),
        }
    }

    fn drain(&self) {
        let mut list = self.callbacks.lock().unwrap();
        for cb in list.drain(..) {
            cb();
        }
    }

    fn pending_count(&self) -> usize {
        self.callbacks.lock().unwrap().len()
    }

    fn total_count(&self) -> u64 {
        self.total_dispatched.load(Ordering::SeqCst)
    }
}

impl MainThreadDispatcher for MockDispatcher {
    fn dispatch(&self, task: Box<dyn FnOnce() + Send>) -> Result<(), String> {
        self.total_dispatched.fetch_add(1, Ordering::SeqCst);
        self.callbacks.lock().unwrap().push(task);
        Ok(())
    }
}

struct MockSink {
    hangs: Arc<Mutex<Vec<(u64, u64)>>>,
    recoveries: Arc<Mutex<Vec<(f64, u64)>>>,
}

impl MockSink {
    fn new() -> Self {
        Self {
            hangs: Arc::new(Mutex::new(Vec::new())),
            recoveries: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn hang_count(&self) -> usize {
        self.hangs.lock().unwrap().len()
    }

    fn recovery_count(&self) -> usize {
        self.recoveries.lock().unwrap().len()
    }
}

impl EvidenceSink for MockSink {
    fn record_hang(&self, hang_duration_ms: u64, threshold_ms: u64, _custom_dir: Option<&Path>) {
        self.hangs
            .lock()
            .unwrap()
            .push((hang_duration_ms, threshold_ms));
    }

    fn record_recovery(&self, wall_time_ms: f64, hang_duration_ms: u64) {
        self.recoveries
            .lock()
            .unwrap()
            .push((wall_time_ms, hang_duration_ms));
    }
}

#[test]
fn test_healthy_loop_acknowledges_heartbeats() {
    let clock = MockClock::new();
    let dispatcher = MockDispatcher::new();
    let sink = MockSink::new();
    let mut engine = WatchdogEngine::new(WatchdogConfig::default());

    assert_eq!(engine.current_state(), WatchdogState::Healthy);
    assert!(!engine.is_pending());

    engine.tick(&clock, &dispatcher, &sink);
    assert_eq!(dispatcher.total_count(), 1);
    assert_eq!(dispatcher.pending_count(), 1);
    assert!(engine.is_pending());

    dispatcher.drain();
    assert_eq!(dispatcher.pending_count(), 0);

    clock.advance(Duration::from_secs(1));
    engine.tick(&clock, &dispatcher, &sink);

    assert_eq!(engine.current_state(), WatchdogState::Healthy);
    assert_eq!(dispatcher.total_count(), 2);
    assert_eq!(sink.hang_count(), 0);
}

#[test]
fn test_at_most_one_pending_heartbeat_enforced_when_hung() {
    let clock = MockClock::new();
    let dispatcher = MockDispatcher::new();
    let sink = MockSink::new();
    let mut config = WatchdogConfig::default();
    config.hang_threshold = Duration::from_secs(5);
    config.cooldown_interval = Duration::from_secs(60);
    let mut engine = WatchdogEngine::new(config);

    engine.tick(&clock, &dispatcher, &sink);
    assert_eq!(dispatcher.total_count(), 1);
    assert_eq!(dispatcher.pending_count(), 1);

    for i in 1..=4 {
        clock.advance(Duration::from_secs(1));
        engine.tick(&clock, &dispatcher, &sink);
        assert_eq!(dispatcher.total_count(), 1);
        assert_eq!(dispatcher.pending_count(), 1);
        if i >= 1 {
            assert_eq!(engine.current_state(), WatchdogState::Slow);
        }
        assert_eq!(sink.hang_count(), 0);
    }

    clock.advance(Duration::from_secs(1));
    engine.tick(&clock, &dispatcher, &sink);

    assert_eq!(dispatcher.total_count(), 1);
    assert_eq!(dispatcher.pending_count(), 1);
    assert_eq!(engine.current_state(), WatchdogState::Hung);
    assert_eq!(sink.hang_count(), 1);
    let (hang_dur, thresh) = sink.hangs.lock().unwrap()[0];
    assert_eq!(thresh, 5000);
    assert!(hang_dur >= 5000);
}

#[test]
fn test_repeated_sleep_gaps_preserves_pending_seq_and_keeps_queue_length_one() {
    let clock = MockClock::new();
    let dispatcher = MockDispatcher::new();
    let sink = MockSink::new();
    let mut config = WatchdogConfig::default();
    config.heartbeat_interval = Duration::from_secs(1);
    config.sleep_gap_tolerance = Duration::from_secs(3);
    config.hang_threshold = Duration::from_secs(5);
    let mut engine = WatchdogEngine::new(config);

    engine.tick(&clock, &dispatcher, &sink);
    assert_eq!(dispatcher.total_count(), 1);
    assert_eq!(dispatcher.pending_count(), 1);

    clock.advance(Duration::from_secs(20));
    engine.tick(&clock, &dispatcher, &sink);
    assert_eq!(dispatcher.total_count(), 1);
    assert_eq!(dispatcher.pending_count(), 1);
    assert_eq!(sink.hang_count(), 0);
    assert_eq!(engine.current_state(), WatchdogState::Healthy);
    assert!(engine.is_pending());

    clock.advance(Duration::from_secs(45));
    engine.tick(&clock, &dispatcher, &sink);
    assert_eq!(dispatcher.total_count(), 1);
    assert_eq!(dispatcher.pending_count(), 1);
    assert_eq!(sink.hang_count(), 0);
    assert_eq!(engine.current_state(), WatchdogState::Healthy);
    assert!(engine.is_pending());

    clock.advance(Duration::from_secs(15));
    engine.tick(&clock, &dispatcher, &sink);
    assert_eq!(dispatcher.total_count(), 1);
    assert_eq!(dispatcher.pending_count(), 1);
    assert_eq!(sink.hang_count(), 0);
    assert_eq!(engine.current_state(), WatchdogState::Healthy);
    assert!(engine.is_pending());

    for _ in 0..5 {
        clock.advance(Duration::from_secs(1));
        engine.tick(&clock, &dispatcher, &sink);
    }
    assert_eq!(dispatcher.total_count(), 1);
    assert_eq!(dispatcher.pending_count(), 1);
    assert_eq!(engine.current_state(), WatchdogState::Hung);
    assert_eq!(sink.hang_count(), 1);
}

#[test]
fn test_cooldown_prevents_repeated_capture_during_continuous_hang() {
    let clock = MockClock::new();
    let dispatcher = MockDispatcher::new();
    let sink = MockSink::new();
    let mut config = WatchdogConfig::default();
    config.hang_threshold = Duration::from_secs(5);
    config.cooldown_interval = Duration::from_secs(60);
    let mut engine = WatchdogEngine::new(config);

    engine.tick(&clock, &dispatcher, &sink);

    for _ in 0..5 {
        clock.advance(Duration::from_secs(1));
        engine.tick(&clock, &dispatcher, &sink);
    }
    assert_eq!(sink.hang_count(), 1);

    for _ in 0..59 {
        clock.advance(Duration::from_secs(1));
        engine.tick(&clock, &dispatcher, &sink);
        assert_eq!(sink.hang_count(), 1);
        assert_eq!(dispatcher.total_count(), 1);
    }

    clock.advance(Duration::from_secs(1));
    engine.tick(&clock, &dispatcher, &sink);
    assert_eq!(sink.hang_count(), 2);
    assert_eq!(dispatcher.total_count(), 1);
}

#[test]
fn test_recovery_after_hang() {
    let clock = MockClock::new();
    let dispatcher = MockDispatcher::new();
    let sink = MockSink::new();
    let mut config = WatchdogConfig::default();
    config.hang_threshold = Duration::from_secs(5);
    let mut engine = WatchdogEngine::new(config);

    engine.tick(&clock, &dispatcher, &sink);
    for _ in 0..5 {
        clock.advance(Duration::from_secs(1));
        engine.tick(&clock, &dispatcher, &sink);
    }
    assert_eq!(engine.current_state(), WatchdogState::Hung);

    dispatcher.drain();
    clock.advance(Duration::from_secs(1));
    engine.tick(&clock, &dispatcher, &sink);

    assert_eq!(engine.current_state(), WatchdogState::Healthy);
    assert_eq!(sink.recovery_count(), 1);
    let (_, rec_dur) = sink.recoveries.lock().unwrap()[0];
    assert!(rec_dur >= 5000);
}

#[test]
fn test_condvar_interruptible_shutdown() {
    let shutdown = Arc::new(WatchdogShutdown {
        stopped: Mutex::new(false),
        condvar: Condvar::new(),
    });
    let handle = WatchdogHandle {
        shutdown: Arc::clone(&shutdown),
    };

    let thread_shutdown = Arc::clone(&shutdown);
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();

    let thread = std::thread::spawn(move || {
        let guard = thread_shutdown.stopped.lock().unwrap();
        ready_tx.send(()).unwrap();
        let (guard, _) = thread_shutdown
            .condvar
            .wait_timeout(guard, Duration::from_secs(60))
            .unwrap();
        let was_stopped = *guard;
        let _ = done_tx.send(was_stopped);
    });

    ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(!handle.is_stopped());
    handle.stop();
    assert!(handle.is_stopped());

    let received = done_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("thread woke up immediately via condvar interrupt");
    assert!(received);
    thread.join().expect("thread joined cleanly");
}

#[test]
fn test_evidence_collector_isolated_seam() {
    let temp_dir = tempfile::tempdir().expect("created temp dir");
    let current_pid = std::process::id();

    let res = exercise_evidence_collector_seam(current_pid, temp_dir.path(), 6200);
    assert!(res.is_ok(), "Collector seam must succeed without hanging");
    let evidence = res.unwrap();

    assert_eq!(evidence.pid, current_pid);
    assert_eq!(evidence.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(evidence.snapshot_tag, env!("CARGO_PKG_VERSION"));
    assert_eq!(evidence.hang_duration_ms, 6200);
    assert_eq!(evidence.threshold_ms, 5000);

    let meta_path = evidence.metadata_path.expect("metadata path present");
    assert!(Path::new(&meta_path).exists(), "Metadata file must exist");

    let meta_raw = std::fs::read_to_string(&meta_path).expect("read metadata json");
    let parsed: serde_json::Value = serde_json::from_str(&meta_raw).expect("parsed metadata json");

    assert_eq!(parsed["pid"].as_u64().unwrap(), current_pid as u64);
    assert_eq!(parsed["hangDurationMs"].as_u64().unwrap(), 6200);
    assert_eq!(
        parsed["snapshotTag"].as_str().unwrap(),
        env!("CARGO_PKG_VERSION")
    );
    assert_eq!(
        parsed["metadataPath"].as_str().expect("metadataPath not null in on-disk json"),
        meta_path
    );

    let sample_path = evidence.sample_output_path.expect("sample path present");
    assert!(Path::new(&sample_path).exists(), "Sample file must exist");
}

#[test]
fn test_evidence_collector_fails_truthfully_on_invalid_dir() {
    let temp_file = tempfile::NamedTempFile::new().expect("created temp file");
    let invalid_dir = temp_file.path().join("impossible_sub");
    let current_pid = std::process::id();

    let res = exercise_evidence_collector_seam(current_pid, &invalid_dir, 6200);
    assert!(res.is_err(), "Collector seam must return Err when writing fails");
}

#[test]
fn test_single_capture_in_flight_guard() {
    let sink = ProductionEvidenceSink::new(
        std::process::id(),
        env!("CARGO_PKG_VERSION").to_string(),
        env!("CARGO_PKG_VERSION").to_string(),
    );

    assert!(!sink.capture_in_flight.load(Ordering::SeqCst));

    sink.capture_in_flight.store(true, Ordering::SeqCst);
    sink.record_hang(6000, 5000, None);

    assert!(sink.capture_in_flight.load(Ordering::SeqCst));
    sink.capture_in_flight.store(false, Ordering::SeqCst);
    assert!(!sink.capture_in_flight.load(Ordering::SeqCst));
}
