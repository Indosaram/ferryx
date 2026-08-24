use parking_lot::Mutex;
use std::collections::{HashMap, VecDeque};
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const HISTOGRAM_CAPACITY: usize = 4096;
const DUMP_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy)]
struct PtyReadMark {
    bytes: usize,
    unix_micros: u64,
}

struct LatencyHistogram {
    samples_ms: VecDeque<f64>,
    last_dump: Instant,
}

impl LatencyHistogram {
    fn new() -> Self {
        Self {
            samples_ms: VecDeque::with_capacity(HISTOGRAM_CAPACITY),
            last_dump: Instant::now(),
        }
    }

    fn record(&mut self, value_ms: f64) {
        if self.samples_ms.len() == HISTOGRAM_CAPACITY {
            self.samples_ms.pop_front();
        }
        self.samples_ms.push_back(value_ms);
    }

    fn should_dump(&self) -> bool {
        self.last_dump.elapsed() >= DUMP_INTERVAL && !self.samples_ms.is_empty()
    }

    fn snapshot_and_mark_dumped(&mut self) -> (usize, f64, f64, f64) {
        let mut sorted: Vec<f64> = self.samples_ms.iter().copied().collect();
        sorted.sort_by(f64::total_cmp);
        self.last_dump = Instant::now();
        let count = sorted.len();
        let p50 = percentile(&sorted, 0.50);
        let p95 = percentile(&sorted, 0.95);
        let max = sorted.last().copied().unwrap_or(0.0);
        (count, p50, p95, max)
    }
}

static METRICS_ENABLED: OnceLock<bool> = OnceLock::new();
static PTY_READ_MARKS: Mutex<Option<HashMap<String, VecDeque<PtyReadMark>>>> = Mutex::new(None);
static PENDING_BATCH_READS: Mutex<Option<HashMap<String, u64>>> = Mutex::new(None);
static READ_TO_CHANNEL_SEND: OnceLock<Mutex<LatencyHistogram>> = OnceLock::new();

pub(crate) fn terminal_metrics_enabled() -> bool {
    *METRICS_ENABLED.get_or_init(|| {
        cfg!(debug_assertions)
            && std::env::var("FERRYX_TERMINAL_METRICS")
                .map(|value| value == "1")
                .unwrap_or(false)
    })
}

pub(crate) fn record_pty_read(session_id: &str, bytes: usize) {
    if !terminal_metrics_enabled() {
        return;
    }
    let Some(unix_micros) = unix_micros_now() else {
        return;
    };

    let mut guard = PTY_READ_MARKS.lock();
    let sessions = guard.get_or_insert_with(HashMap::new);
    sessions
        .entry(session_id.to_string())
        .or_default()
        .push_back(PtyReadMark { bytes, unix_micros });
}

pub(crate) fn take_pty_read_timestamp(session_id: &str, bytes: usize) -> Option<u64> {
    if !terminal_metrics_enabled() {
        return None;
    }

    let mut guard = PTY_READ_MARKS.lock();
    let sessions = guard.as_mut()?;
    let marks = sessions.get_mut(session_id)?;
    let mark = marks.pop_front()?;
    if marks.is_empty() {
        sessions.remove(session_id);
    }

    if mark.bytes != bytes {
        eprintln!(
            "[terminal-metrics] PTY read mark length mismatch session={session_id} read={} received={bytes}",
            mark.bytes
        );
    }
    Some(mark.unix_micros)
}

pub(crate) fn clear_pty_read_timestamps(session_id: &str) {
    if !terminal_metrics_enabled() {
        return;
    }
    if let Some(sessions) = PTY_READ_MARKS.lock().as_mut() {
        sessions.remove(session_id);
    }
}

pub(crate) fn note_batch_read_timestamp(session_id: &str, read_unix_micros: Option<u64>) {
    if !terminal_metrics_enabled() {
        return;
    }
    let Some(read_unix_micros) = read_unix_micros else {
        return;
    };

    let mut guard = PENDING_BATCH_READS.lock();
    let sessions = guard.get_or_insert_with(HashMap::new);
    sessions
        .entry(session_id.to_string())
        .and_modify(|existing| *existing = (*existing).min(read_unix_micros))
        .or_insert(read_unix_micros);
}

pub(crate) fn record_channel_send_for_session(session_id: &str) {
    if !terminal_metrics_enabled() {
        return;
    }
    let read_unix_micros = PENDING_BATCH_READS
        .lock()
        .as_mut()
        .and_then(|sessions| sessions.remove(session_id));
    record_read_to_channel_send(read_unix_micros);
}

pub(crate) fn clear_pending_batch_read(session_id: &str) {
    if !terminal_metrics_enabled() {
        return;
    }
    if let Some(sessions) = PENDING_BATCH_READS.lock().as_mut() {
        sessions.remove(session_id);
    }
}

fn record_read_to_channel_send(read_unix_micros: Option<u64>) {
    if !terminal_metrics_enabled() {
        return;
    }
    let Some(read_unix_micros) = read_unix_micros else {
        return;
    };
    let Some(now_unix_micros) = unix_micros_now() else {
        return;
    };
    if now_unix_micros < read_unix_micros {
        return;
    }

    let delta_ms = (now_unix_micros - read_unix_micros) as f64 / 1000.0;
    let histogram = READ_TO_CHANNEL_SEND.get_or_init(|| Mutex::new(LatencyHistogram::new()));
    let mut histogram = histogram.lock();
    histogram.record(delta_ms);
    if histogram.should_dump() {
        let (count, p50, p95, max) = histogram.snapshot_and_mark_dumped();
        eprintln!(
            "[terminal-metrics] read->channel-send count={count} p50_ms={p50:.3} p95_ms={p95:.3} max_ms={max:.3}"
        );
    }
}

fn unix_micros_now() -> Option<u64> {
    let micros = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_micros();
    u64::try_from(micros).ok()
}

fn percentile(sorted: &[f64], percentile: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let index = ((sorted.len() - 1) as f64 * percentile).ceil() as usize;
    sorted[index.min(sorted.len() - 1)]
}
