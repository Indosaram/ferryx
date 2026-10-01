//! Helper-owned DAG subscription streaming.
//!
//! Change detection is owned by the helper process: one watcher thread per
//! subscription scans the project's journal directory, compares full checkpoint
//! *content* hashes (so a rewrite that preserves mtime is still an update), and
//! parks on a condvar between scans. Consumers block in `dag.next` and are woken
//! by that watcher, so the desktop never drives a periodic RPC poll and never
//! resends its known-run set: dedup state lives here, per subscription.
//!
//! Subscriptions are strictly independent of PTY sessions. A watcher thread
//! never reads or mutates `Runtime::sessions`, never owns a PTY, and its exit
//! has no effect on any running child process.
use serde_json::{json, Value};
use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};

use super::dag_journal::parse_run_checkpoint;

/// Maximum concurrently open subscriptions across the whole helper runtime.
pub const MAX_SUBSCRIPTIONS: usize = 16;
/// Maximum coalesced pending checkpoint files before the subscription resyncs.
pub const MAX_PENDING_FILES: usize = 64;
/// Maximum `dropped` reports carried by one frame; the rest stay pending.
pub const MAX_DROPPED_PER_FRAME: usize = 16;
/// Payload budget for one delivered frame batch (512 KiB).
/// Note: caller wrappers (such as `dag.subscribe` in helper which stamps `projectId`)
/// fit well within the 1 MiB wire limit.
pub const MAX_BATCH_BYTES: usize = 512 * 1024;
/// Raw checkpoint file size limit at input boundary (e.g. bridge max 1 MiB).
/// Checkpoints exceeding this are refused immediately before reading into memory.
pub const MAX_INPUT_FILE_BYTES: u64 = 1024 * 1024;
/// Maximum allowed serialized size of a single projected snapshot (512 KiB).
/// Projected snapshots exceeding this budget are explicitly reported in dropped.
pub const MAX_PROJECTED_SNAPSHOT_BYTES: usize = 512 * 1024;
/// Per-checkpoint limit retained for backward compatibility / tests.
pub const MAX_SNAPSHOT_BYTES: u64 = MAX_INPUT_FILE_BYTES;
/// Upper bound on a consumer's blocking wait.
pub const MAX_WAIT_MS: u64 = 10_000;
/// A subscription with no consumer call within this window releases itself.
/// This is the backstop for a consumer that dies while parked in `dag.next`:
/// connection EOF is only observed once the blocked call returns (at most
/// `MAX_WAIT_MS` later), and this TTL covers any consumer that stops calling
/// without closing its connection at all.
pub const IDLE_TTL: Duration = Duration::from_secs(60);
/// Watcher rescan interval. The consumer is woken immediately on change, so this
/// only bounds detection latency inside the helper, never client round trips.
const WATCH_INTERVAL: Duration = Duration::from_millis(120);

/// Connection id used by callers that are not bound to a live connection
/// (in-process tests); such subscriptions are released only by explicit
/// `dag.unsubscribe`.
pub const DETACHED_CONNECTION: u64 = 0;

struct State {
    known: HashMap<String, u64>,
    pending: BTreeSet<String>,
    resync: bool,
    /// Last filename enumerated by an in-progress resync. A resync larger than
    /// `MAX_PENDING_FILES` continues from here instead of enumerating the whole
    /// directory into memory at once.
    resync_cursor: Option<String>,
    sequence: u64,
    closed: bool,
    waiters: usize,
    last_activity: std::time::Instant,
}

struct Subscription {
    runs_dir: PathBuf,
    owner: u64,
    state: Mutex<State>,
    signal: Condvar,
}

pub struct DagStreams {
    subscriptions: Mutex<HashMap<String, Arc<Subscription>>>,
}

struct Probe {
    hash: u64,
    len: u64,
    content: Option<Vec<u8>>,
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn project_checkpoint(bytes: &[u8]) -> Result<(Value, usize), String> {
    let raw_str = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
    let snapshot = parse_run_checkpoint(raw_str).map_err(|e| e.to_string())?;
    let value = serde_json::to_value(&snapshot).map_err(|e| e.to_string())?;
    let serialized_len = serde_json::to_vec(&value).map_err(|e| e.to_string())?.len();
    Ok((value, serialized_len))
}

fn build_frame(
    id: &str,
    sequence: u64,
    runs: &[Value],
    dropped: &[Value],
    resync: bool,
    more: bool,
) -> Value {
    json!({
        "subscriptionId": id,
        "sequence": sequence,
        "runs": runs,
        "dropped": dropped,
        "resync": resync,
        "more": more,
        "closed": false,
    })
}

fn frame_size_valid(
    id: &str,
    sequence: u64,
    runs: &[Value],
    dropped: &[Value],
    resync: bool,
    more: bool,
) -> bool {
    let frame = build_frame(id, sequence, runs, dropped, resync, more);
    serde_json::to_vec(&frame).map(|b| b.len() <= MAX_BATCH_BYTES).unwrap_or(false)
}

// Content hashing (not mtime) is what makes a same-timestamp rewrite visible.
// Oversized files are never read into memory; their identity is derived from
// length and mtime so they are reported once instead of on every scan.
fn probe(path: &Path) -> Option<Probe> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let len = meta.len();
    if len > MAX_INPUT_FILE_BYTES {
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        let mut seed = len.to_be_bytes().to_vec();
        seed.extend_from_slice(&mtime.to_be_bytes());
        return Some(Probe {
            hash: fnv1a(&seed),
            len,
            content: None,
        });
    }
    let content = std::fs::read(path).ok()?;
    Some(Probe {
        hash: fnv1a(&content),
        len,
        content: Some(content),
    })
}

fn is_checkpoint(path: &Path) -> bool {
    path.is_file() && path.extension().is_some_and(|ext| ext == "json")
}

impl DagStreams {
    pub fn new() -> Self {
        Self {
            subscriptions: Mutex::new(HashMap::new()),
        }
    }

    fn get(&self, id: &str) -> Result<Arc<Subscription>, String> {
        self.subscriptions
            .lock()
            .map_err(|e| e.to_string())?
            .get(id)
            .cloned()
            .ok_or_else(|| "NOT_FOUND".to_string())
    }

    /// Opens a subscription on `runs_dir` and returns the first frame, which
    /// carries the current inventory (bounded; `more` signals a continuation).
    pub fn subscribe(&self, runs_dir: PathBuf, owner: u64) -> Result<Value, String> {
        let id = uuid::Uuid::new_v4().to_string();
        let subscription = Arc::new(Subscription {
            runs_dir,
            owner,
            state: Mutex::new(State {
                known: HashMap::new(),
                pending: BTreeSet::new(),
                // Seeded as a resync so the first frame is a full inventory.
                resync: true,
                resync_cursor: None,
                sequence: 0,
                closed: false,
                waiters: 0,
                last_activity: std::time::Instant::now(),
            }),
            signal: Condvar::new(),
        });

        {
            let mut subscriptions = self.subscriptions.lock().map_err(|e| e.to_string())?;
            // Reap anything the watcher closed on its own (idle TTL) so an
            // abandoned subscription never consumes a slot.
            subscriptions.retain(|_, s| !s.state.lock().map(|st| st.closed).unwrap_or(true));
            if subscriptions.len() >= MAX_SUBSCRIPTIONS {
                return Err("RESOURCE_EXHAUSTED: dag subscription limit reached".into());
            }
            subscriptions.insert(id.clone(), subscription.clone());
        }

        let watched = subscription.clone();
        std::thread::spawn(move || watch(watched));

        deliver(&subscription, &id)
    }

    /// Blocks until the helper's watcher reports a change, the wait elapses, or
    /// the subscription is released.
    pub fn next(&self, id: &str, wait_ms: u64) -> Result<Value, String> {
        let subscription = self.get(id)?;
        let wait = Duration::from_millis(wait_ms.min(MAX_WAIT_MS));
        {
            let mut state = subscription.state.lock().map_err(|e| e.to_string())?;
            state.waiters += 1;
            state.last_activity = std::time::Instant::now();
            subscription.signal.notify_all();
            let (mut state, _) = subscription
                .signal
                .wait_timeout_while(state, wait, |s| {
                    s.pending.is_empty() && !s.resync && !s.closed
                })
                .map_err(|e| e.to_string())?;
            state.waiters -= 1;
            state.last_activity = std::time::Instant::now();
            if state.closed {
                let sequence = state.sequence;
                drop(state);
                // Terminal frame: the id is retired, so the next call is
                // NOT_FOUND rather than an endless closed-frame loop.
                if let Ok(mut subscriptions) = self.subscriptions.lock() {
                    subscriptions.remove(id);
                }
                return Ok(json!({
                    "subscriptionId": id,
                    "sequence": sequence,
                    "runs": [],
                    "dropped": [],
                    "resync": false,
                    "more": false,
                    "closed": true,
                }));
            }
        }
        deliver(&subscription, id)
    }

    pub fn unsubscribe(&self, id: &str) -> Result<Value, String> {
        let removed = self
            .subscriptions
            .lock()
            .map_err(|e| e.to_string())?
            .remove(id);
        let subscription = removed.ok_or_else(|| "NOT_FOUND".to_string())?;
        close(&subscription);
        Ok(json!({ "subscriptionId": id, "closed": true }))
    }

    /// Releases every subscription owned by a connection that went away.
    pub fn close_connection(&self, owner: u64) {
        if owner == DETACHED_CONNECTION {
            return;
        }
        let mut closing = Vec::new();
        if let Ok(mut subscriptions) = self.subscriptions.lock() {
            subscriptions.retain(|_, subscription| {
                if subscription.owner == owner {
                    closing.push(subscription.clone());
                    false
                } else {
                    true
                }
            });
        }
        for subscription in closing {
            close(&subscription);
        }
    }

    #[cfg(test)]
    pub fn subscription_count(&self) -> usize {
        self.subscriptions.lock().map(|s| s.len()).unwrap_or(0)
    }

    /// Test-only: reproduces the watcher/delivery overlap deterministically by
    /// re-enqueuing a name that delivery has already recorded.
    #[cfg(test)]
    pub fn force_pending(&self, id: &str, name: &str) -> bool {
        let Ok(subscription) = self.get(id) else {
            return false;
        };
        let Ok(mut state) = subscription.state.lock() else {
            return false;
        };
        state.pending.insert(name.to_string());
        subscription.signal.notify_all();
        true
    }

    /// Test-only: blocks until a consumer is parked inside `next` for `id`.
    /// Event-driven (condvar), never a sleep.
    #[cfg(test)]
    pub fn wait_until_waiting(&self, id: &str, timeout: Duration) -> bool {
        let Ok(subscription) = self.get(id) else {
            return false;
        };
        let state = subscription.state.lock().expect("dag state mutex poisoned");
        let (state, _) = subscription
            .signal
            .wait_timeout_while(state, timeout, |s| s.waiters == 0)
            .expect("dag state mutex poisoned");
        state.waiters > 0
    }
}

#[cfg(test)]
#[path = "dag_stream_tests.rs"]
mod dag_stream_tests;

impl Default for DagStreams {
    fn default() -> Self {
        Self::new()
    }
}

fn close(subscription: &Arc<Subscription>) {
    if let Ok(mut state) = subscription.state.lock() {
        state.closed = true;
    }
    subscription.signal.notify_all();
}

fn deliver(subscription: &Arc<Subscription>, id: &str) -> Result<Value, String> {
    let mut resync = false;
    {
        let mut state = subscription.state.lock().map_err(|e| e.to_string())?;
        if state.resync {
            resync = true;
            state.resync = false;
            if state.resync_cursor.is_none() {
                // First pass of a fresh resync: drop all derived state.
                state.pending.clear();
                state.known.clear();
            }

            // Enumerate in filename order and bounded, continuing from the
            // cursor. A directory larger than one batch keeps `resync` set so
            // the caller drains it across frames instead of materializing the
            // whole listing.
            let mut names: Vec<String> = Vec::new();
            if let Ok(entries) = std::fs::read_dir(&subscription.runs_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !is_checkpoint(&path) {
                        continue;
                    }
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if state
                            .resync_cursor
                            .as_deref()
                            .is_none_or(|cursor| name > cursor)
                        {
                            names.push(name.to_string());
                        }
                    }
                }
            }
            names.sort();
            let truncated = names.len() > MAX_PENDING_FILES;
            names.truncate(MAX_PENDING_FILES);
            if let Some(last) = names.last().cloned() {
                state.resync_cursor = Some(last);
            }
            for name in names {
                state.pending.insert(name);
            }
            if truncated {
                state.resync = true;
            } else {
                state.resync_cursor = None;
            }
        }
    }

    let mut runs: Vec<Value> = Vec::new();
    let mut dropped: Vec<Value> = Vec::new();

    loop {
        let name = {
            let mut state = subscription.state.lock().map_err(|e| e.to_string())?;
            match state.pending.iter().next().cloned() {
                Some(name) => {
                    state.pending.remove(&name);
                    name
                }
                None => break,
            }
        };

        let path = subscription.runs_dir.join(&name);
        let Some(probed) = probe(&path) else {
            // Vanished or unreadable: no tombstone protocol exists, so simply
            // forget it; a recreated file hashes as new.
            let mut state = subscription.state.lock().map_err(|e| e.to_string())?;
            state.known.remove(&name);
            continue;
        };

        {
            // The watcher may re-enqueue a name in the window between this loop
            // popping it and recording its hash. Re-check against `known` after
            // probing so an unchanged checkpoint is never emitted twice in one
            // frame (or across frames).
            let state = subscription.state.lock().map_err(|e| e.to_string())?;
            if state.known.get(&name) == Some(&probed.hash) {
                continue;
            }
        }

        let next_seq = subscription
            .state
            .lock()
            .map_err(|e| e.to_string())?
            .sequence
            + 1;

        let Some(content) = probed.content else {
            if dropped.len() >= MAX_DROPPED_PER_FRAME {
                let mut state = subscription.state.lock().map_err(|e| e.to_string())?;
                state.pending.insert(name);
                break;
            }
            let drop_entry = json!({
                "file": name,
                "error": "SNAPSHOT_TOO_LARGE",
                "bytes": probed.len,
            });
            let mut candidate_dropped = dropped.clone();
            candidate_dropped.push(drop_entry.clone());
            if !frame_size_valid(id, next_seq, &runs, &candidate_dropped, resync, true) {
                if !runs.is_empty() || !dropped.is_empty() {
                    let mut state = subscription.state.lock().map_err(|e| e.to_string())?;
                    state.pending.insert(name);
                    break;
                }
            }
            dropped.push(drop_entry);
            let mut state = subscription.state.lock().map_err(|e| e.to_string())?;
            state.known.insert(name, probed.hash);
            continue;
        };

        match project_checkpoint(&content) {
            Ok((projected_val, projected_bytes)) => {
                if projected_bytes > MAX_PROJECTED_SNAPSHOT_BYTES {
                    if dropped.len() >= MAX_DROPPED_PER_FRAME {
                        let mut state = subscription.state.lock().map_err(|e| e.to_string())?;
                        state.pending.insert(name);
                        break;
                    }
                    let drop_entry = json!({
                        "file": name.clone(),
                        "error": "SNAPSHOT_TOO_LARGE",
                        "bytes": projected_bytes as u64,
                    });
                    let mut candidate_dropped = dropped.clone();
                    candidate_dropped.push(drop_entry.clone());
                    if !frame_size_valid(id, next_seq, &runs, &candidate_dropped, resync, true) {
                        if !runs.is_empty() || !dropped.is_empty() {
                            let mut state = subscription.state.lock().map_err(|e| e.to_string())?;
                            state.pending.insert(name);
                            break;
                        }
                    }
                    dropped.push(drop_entry);
                    let mut state = subscription.state.lock().map_err(|e| e.to_string())?;
                    state.known.insert(name, probed.hash);
                    continue;
                }

                let mut candidate_runs = runs.clone();
                candidate_runs.push(projected_val.clone());
                if !frame_size_valid(id, next_seq, &candidate_runs, &dropped, resync, true) {
                    if !runs.is_empty() || !dropped.is_empty() {
                        // Batched: defer to subsequent frame so current frame stays within 512KiB
                        let mut state = subscription.state.lock().map_err(|e| e.to_string())?;
                        state.pending.insert(name);
                        break;
                    }
                    // Single run alone exceeds frame budget (with envelope wrapper) -> report drop
                    if dropped.len() >= MAX_DROPPED_PER_FRAME {
                        let mut state = subscription.state.lock().map_err(|e| e.to_string())?;
                        state.pending.insert(name);
                        break;
                    }
                    dropped.push(json!({
                        "file": name.clone(),
                        "error": "SNAPSHOT_TOO_LARGE",
                        "bytes": projected_bytes as u64,
                    }));
                    let mut state = subscription.state.lock().map_err(|e| e.to_string())?;
                    state.known.insert(name, probed.hash);
                    continue;
                }

                runs.push(projected_val);
            }
            Err(error) => {
                if dropped.len() >= MAX_DROPPED_PER_FRAME {
                    let mut state = subscription.state.lock().map_err(|e| e.to_string())?;
                    state.pending.insert(name);
                    break;
                }
                let drop_entry = json!({
                    "file": name.clone(),
                    "error": "INVALID_CHECKPOINT_JSON",
                    "bytes": probed.len,
                    "detail": error,
                });
                let mut candidate_dropped = dropped.clone();
                candidate_dropped.push(drop_entry.clone());
                if !frame_size_valid(id, next_seq, &runs, &candidate_dropped, resync, true) {
                    if !runs.is_empty() || !dropped.is_empty() {
                        let mut state = subscription.state.lock().map_err(|e| e.to_string())?;
                        state.pending.insert(name);
                        break;
                    }
                }
                dropped.push(drop_entry);
            }
        }
        let mut state = subscription.state.lock().map_err(|e| e.to_string())?;
        state.known.insert(name, probed.hash);
    }

    let mut state = subscription.state.lock().map_err(|e| e.to_string())?;
    state.sequence += 1;
    state.last_activity = std::time::Instant::now();
    let more = !state.pending.is_empty() || state.resync;
    Ok(build_frame(id, state.sequence, &runs, &dropped, resync, more))
}

fn watch(subscription: Arc<Subscription>) {
    loop {
        {
            let Ok(state) = subscription.state.lock() else {
                return;
            };
            if state.closed {
                return;
            }
        }

        let mut observed: Vec<(String, u64)> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&subscription.runs_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !is_checkpoint(&path) {
                    continue;
                }
                let Some(name) = path.file_name().and_then(|n| n.to_str()).map(String::from) else {
                    continue;
                };
                if let Some(probed) = probe(&path) {
                    observed.push((name, probed.hash));
                }
            }
        }

        {
            let Ok(mut state) = subscription.state.lock() else {
                return;
            };
            if state.closed {
                return;
            }
            if state.waiters == 0 && state.last_activity.elapsed() >= IDLE_TTL {
                // Abandoned subscription: release the watcher rather than scan
                // a directory nobody is reading.
                state.closed = true;
                subscription.signal.notify_all();
                return;
            }
            let mut changed = false;
            for (name, hash) in observed {
                if state.known.get(&name) == Some(&hash) || state.pending.contains(&name) {
                    continue;
                }
                if state.pending.len() >= MAX_PENDING_FILES {
                    // Bounded queue: collapse to a resync instead of growing.
                    state.pending.clear();
                    state.resync = true;
                    state.resync_cursor = None;
                    changed = true;
                    break;
                }
                state.pending.insert(name);
                changed = true;
            }
            if changed {
                subscription.signal.notify_all();
            }
        }

        let Ok(state) = subscription.state.lock() else {
            return;
        };
        if subscription
            .signal
            .wait_timeout_while(state, WATCH_INTERVAL, |s| !s.closed)
            .map(|(s, _)| s.closed)
            .unwrap_or(true)
        {
            return;
        }
    }
}
