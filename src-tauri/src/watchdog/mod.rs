pub mod collector;
#[cfg(test)]
mod tests;

use collector::collect_hang_evidence;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime};

pub trait MainThreadDispatcher: Send + Sync + 'static {
    fn dispatch(&self, task: Box<dyn FnOnce() + Send>) -> Result<(), String>;
}

pub trait WatchdogClock: Send + Sync + 'static {
    fn now_monotonic(&self) -> Instant;
    fn now_system(&self) -> SystemTime;
}

pub trait EvidenceSink: Send + Sync + 'static {
    fn record_hang(&self, hang_duration_ms: u64, threshold_ms: u64, custom_dir: Option<&Path>);
    fn record_recovery(&self, wall_time_ms: f64, hang_duration_ms: u64);
}

pub struct ProductionClock;

impl WatchdogClock for ProductionClock {
    fn now_monotonic(&self) -> Instant {
        Instant::now()
    }
    fn now_system(&self) -> SystemTime {
        SystemTime::now()
    }
}

pub struct TauriMainDispatcher<R: tauri::Runtime> {
    app_handle: tauri::AppHandle<R>,
}

impl<R: tauri::Runtime> TauriMainDispatcher<R> {
    pub fn new(app_handle: tauri::AppHandle<R>) -> Self {
        Self { app_handle }
    }
}

impl<R: tauri::Runtime> MainThreadDispatcher for TauriMainDispatcher<R> {
    fn dispatch(&self, task: Box<dyn FnOnce() + Send>) -> Result<(), String> {
        self.app_handle
            .run_on_main_thread(task)
            .map_err(|e| e.to_string())
    }
}

pub struct ProductionEvidenceSink {
    pid: u32,
    version: String,
    snapshot_tag: String,
    capture_in_flight: Arc<AtomicBool>,
}

impl ProductionEvidenceSink {
    pub fn new(pid: u32, version: String, snapshot_tag: String) -> Self {
        Self {
            pid,
            version,
            snapshot_tag,
            capture_in_flight: Arc::new(AtomicBool::new(false)),
        }
    }
}

struct CaptureGuard(Arc<AtomicBool>);

impl Drop for CaptureGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

impl EvidenceSink for ProductionEvidenceSink {
    fn record_hang(&self, hang_duration_ms: u64, threshold_ms: u64, custom_dir: Option<&Path>) {
        if self
            .capture_in_flight
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            tracing::warn!(
                pid = self.pid,
                "Watchdog evidence capture already in progress; skipping concurrent capture"
            );
            return;
        }

        let pid = self.pid;
        let version = self.version.clone();
        let snapshot_tag = self.snapshot_tag.clone();
        let custom_dir_buf = custom_dir.map(|p| p.to_path_buf());
        let flag = Arc::clone(&self.capture_in_flight);

        let spawn_res = std::thread::Builder::new()
            .name("ferryx-watchdog-capture".to_string())
            .spawn(move || {
                let _guard = CaptureGuard(flag);
                collect_hang_evidence(
                    pid,
                    &version,
                    &snapshot_tag,
                    hang_duration_ms,
                    threshold_ms,
                    custom_dir_buf.as_deref(),
                );
            });

        if let Err(err) = spawn_res {
            self.capture_in_flight.store(false, Ordering::SeqCst);
            tracing::warn!(?err, pid, "Failed to spawn watchdog capture thread");
        }
    }

    fn record_recovery(&self, wall_time_ms: f64, hang_duration_ms: u64) {
        crate::ipc::debug::log_native_switch_debug(serde_json::json!({
            "event": "app.watchdog.recovered",
            "runId": "watchdog",
            "sequence": 0,
            "wallTimeMs": wall_time_ms,
            "details": {
                "pid": self.pid,
                "recoveredAfterMs": hang_duration_ms,
            }
        }));
    }
}

#[derive(Debug, Clone)]
pub struct WatchdogConfig {
    pub heartbeat_interval: Duration,
    pub hang_threshold: Duration,
    pub cooldown_interval: Duration,
    pub sleep_gap_tolerance: Duration,
    pub snapshot_tag: String,
    pub custom_report_dir: Option<PathBuf>,
}

impl Default for WatchdogConfig {
    fn default() -> Self {
        Self {
            heartbeat_interval: Duration::from_secs(1),
            hang_threshold: Duration::from_secs(5),
            cooldown_interval: Duration::from_secs(60),
            sleep_gap_tolerance: Duration::from_secs(3),
            snapshot_tag: env!("CARGO_PKG_VERSION").to_string(),
            custom_report_dir: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchdogState {
    Healthy,
    Slow,
    Hung,
}

#[derive(Debug, Clone)]
pub struct PendingHeartbeat {
    pub seq: u64,
    pub dispatched_monotonic: Instant,
}

pub struct WatchdogEngine {
    pub config: WatchdogConfig,
    pub next_seq: u64,
    pub last_acked_seq: Arc<AtomicU64>,
    pub pending_heartbeat: Option<PendingHeartbeat>,
    pub last_tick_monotonic: Option<Instant>,
    pub last_capture_monotonic: Option<Instant>,
    pub state: WatchdogState,
    pub total_dispatches: u64,
}

impl WatchdogEngine {
    pub fn new(config: WatchdogConfig) -> Self {
        Self {
            config,
            next_seq: 1,
            last_acked_seq: Arc::new(AtomicU64::new(0)),
            pending_heartbeat: None,
            last_tick_monotonic: None,
            last_capture_monotonic: None,
            state: WatchdogState::Healthy,
            total_dispatches: 0,
        }
    }

    pub fn is_pending(&self) -> bool {
        self.pending_heartbeat.is_some()
    }

    pub fn current_state(&self) -> WatchdogState {
        self.state
    }

    pub fn tick<C: WatchdogClock, D: MainThreadDispatcher, S: EvidenceSink>(
        &mut self,
        clock: &C,
        dispatcher: &D,
        sink: &S,
    ) {
        let now_mono = clock.now_monotonic();
        let now_sys = clock.now_system();

        if let Some(last_tick_mono) = self.last_tick_monotonic {
            let elapsed_gap = now_mono.saturating_duration_since(last_tick_mono);
            let expected_max_gap = self.config.heartbeat_interval + self.config.sleep_gap_tolerance;
            if elapsed_gap > expected_max_gap {
                if let Some(ref mut pending) = self.pending_heartbeat {
                    let acked = self.last_acked_seq.load(Ordering::SeqCst);
                    if acked >= pending.seq {
                        self.pending_heartbeat = None;
                        self.state = WatchdogState::Healthy;
                    } else {
                        pending.dispatched_monotonic = now_mono;
                        self.state = WatchdogState::Healthy;
                    }
                } else {
                    self.state = WatchdogState::Healthy;
                }
                self.last_tick_monotonic = Some(now_mono);
                return;
            }
        }

        if let Some(pending) = &self.pending_heartbeat {
            let acked = self.last_acked_seq.load(Ordering::SeqCst);
            if acked >= pending.seq {
                let latency = now_mono.saturating_duration_since(pending.dispatched_monotonic);
                if self.state == WatchdogState::Hung {
                    let wall_ms = now_sys
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .map(|d| d.as_millis() as f64)
                        .unwrap_or(0.0);
                    sink.record_recovery(wall_ms, latency.as_millis() as u64);
                }
                self.state = WatchdogState::Healthy;
                self.pending_heartbeat = None;
            } else {
                let unacked_duration =
                    now_mono.saturating_duration_since(pending.dispatched_monotonic);
                if unacked_duration >= self.config.hang_threshold {
                    self.state = WatchdogState::Hung;
                    let in_cooldown = match self.last_capture_monotonic {
                        Some(last_cap) => {
                            now_mono.saturating_duration_since(last_cap)
                                < self.config.cooldown_interval
                        }
                        None => false,
                    };
                    if !in_cooldown {
                        self.last_capture_monotonic = Some(now_mono);
                        sink.record_hang(
                            unacked_duration.as_millis() as u64,
                            self.config.hang_threshold.as_millis() as u64,
                            self.config.custom_report_dir.as_deref(),
                        );
                    }
                } else if unacked_duration >= self.config.heartbeat_interval {
                    self.state = WatchdogState::Slow;
                }
                self.last_tick_monotonic = Some(now_mono);
                return;
            }
        }

        let seq = self.next_seq;
        self.next_seq += 1;
        let ack_target = Arc::clone(&self.last_acked_seq);
        let dispatch_res = dispatcher.dispatch(Box::new(move || {
            ack_target.store(seq, Ordering::SeqCst);
        }));

        if dispatch_res.is_ok() {
            self.total_dispatches += 1;
            self.pending_heartbeat = Some(PendingHeartbeat {
                seq,
                dispatched_monotonic: now_mono,
            });
        }
        self.last_tick_monotonic = Some(now_mono);
    }
}

struct WatchdogShutdown {
    stopped: Mutex<bool>,
    condvar: Condvar,
}

pub struct WatchdogHandle {
    shutdown: Arc<WatchdogShutdown>,
}

impl WatchdogHandle {
    pub fn stop(&self) {
        let mut guard = self.shutdown.stopped.lock().unwrap();
        *guard = true;
        self.shutdown.condvar.notify_all();
    }

    pub fn is_stopped(&self) -> bool {
        *self.shutdown.stopped.lock().unwrap()
    }
}

impl Drop for WatchdogHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn start_watchdog<R: tauri::Runtime>(
    app_handle: tauri::AppHandle<R>,
    config: WatchdogConfig,
) -> WatchdogHandle {
    let shutdown = Arc::new(WatchdogShutdown {
        stopped: Mutex::new(false),
        condvar: Condvar::new(),
    });
    let handle = WatchdogHandle {
        shutdown: Arc::clone(&shutdown),
    };

    let pid = std::process::id();
    let version = env!("CARGO_PKG_VERSION").to_string();
    let snapshot_tag = config.snapshot_tag.clone();
    let interval = config.heartbeat_interval;

    let dispatcher = TauriMainDispatcher::new(app_handle);
    let clock = ProductionClock;
    let sink = ProductionEvidenceSink::new(pid, version, snapshot_tag);
    let mut engine = WatchdogEngine::new(config);

    let thread_shutdown = Arc::clone(&shutdown);
    let spawn_res = std::thread::Builder::new()
        .name("ferryx-watchdog".to_string())
        .spawn(move || loop {
            let guard = thread_shutdown.stopped.lock().unwrap();
            if *guard {
                break;
            }
            let (guard, _) = thread_shutdown
                .condvar
                .wait_timeout(guard, interval)
                .unwrap();
            if *guard {
                break;
            }
            drop(guard);

            engine.tick(&clock, &dispatcher, &sink);
        });

    if let Err(err) = spawn_res {
        tracing::warn!(?err, "Failed to spawn dedicated watchdog thread");
    }

    handle
}
