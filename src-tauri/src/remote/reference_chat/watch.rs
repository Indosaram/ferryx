//! Bounded, TTL-expiring metadata watches for recently read native transcripts.
//!
//! A watch is a hint only. The owning host stats the resolved transcript, or the database
//! plus its WAL for a store-backed reader, and emits one coalesced `referenceHistoryChanged`
//! frame per target per window. A missed frame changes nothing: the client still polls and
//! re-reads. Relay hosts do not build these frames; they forward socket bytes opaquely, and a
//! paired owner emits from its own gateway.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, UNIX_EPOCH};

use parking_lot::Mutex;

use super::history::{ReferenceHistoryLocation, ReferenceHistoryStream};
use super::types::{ReferenceHistoryEvent, ReferenceTargetRef};
use crate::agent_control::events::AgentEventBus;

/// How many targets one host watches at once. A read past the cap evicts the soonest expiry.
pub const REFERENCE_HISTORY_WATCH_CAP: usize = 64;

/// A target that is not read again within this long is dropped. Active clients refresh it.
pub const REFERENCE_HISTORY_WATCH_TTL: Duration = Duration::from_secs(60);

/// At most one frame per target per window. A change inside the window stays pending.
pub const REFERENCE_HISTORY_WATCH_COALESCE: Duration = Duration::from_secs(1);

/// Cross-platform poll. There is no inotify-only path; tests drive the same observe step.
pub const REFERENCE_HISTORY_WATCH_POLL: Duration = Duration::from_secs(1);

/// Whether this gateway may watch `target`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryWatchAdmit {
    /// This process is the owning host.
    Local,
    /// A paired daemon owns the session and emits its own frame.
    PairedOwner,
    /// The target names another host or owner. This gateway does not invent a watch.
    Foreign,
}

/// `paired_peer` wins: that daemon emits, even if the target string looks local.
pub fn admit_reference_history_watch(
    target: &ReferenceTargetRef,
    local_host: &str,
    local_owner: &str,
    paired_peer: bool,
) -> HistoryWatchAdmit {
    if paired_peer {
        return HistoryWatchAdmit::PairedOwner;
    }
    if target.target.host_id != local_host || target.target.owner_id != local_owner {
        return HistoryWatchAdmit::Foreign;
    }
    HistoryWatchAdmit::Local
}

/// Files whose metadata a resolved stream is watched through. No path is put on the wire.
pub fn reference_history_watch_paths(stream: &ReferenceHistoryStream) -> Vec<PathBuf> {
    match &stream.location {
        ReferenceHistoryLocation::File(path) if is_database(path) => database_watch_paths(path),
        ReferenceHistoryLocation::File(path) => vec![path.clone()],
        ReferenceHistoryLocation::Rows { store, .. } => database_watch_paths(store),
        ReferenceHistoryLocation::Scrollback | ReferenceHistoryLocation::NotStarted => Vec::new(),
    }
}

fn is_database(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("db")
}

/// SQLite names the WAL by appending `-wal` to the database filename (`store.db-wal`).
fn database_watch_paths(store: &Path) -> Vec<PathBuf> {
    vec![store.to_path_buf(), wal_sibling(store)]
}

fn wal_sibling(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push("-wal");
    PathBuf::from(name)
}

/// Metadata that means "this store may have changed". Content is never read.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FileStamp {
    exists: bool,
    len: u64,
    mtime_ns: Option<u128>,
    /// `(device, inode)` where the platform exposes both. Absent is not a change by itself.
    inode: Option<(u64, u64)>,
}

impl FileStamp {
    fn missing() -> Self {
        Self { exists: false, len: 0, mtime_ns: None, inode: None }
    }

    fn from_meta(meta: &std::fs::Metadata) -> Self {
        Self {
            exists: true,
            len: meta.len(),
            mtime_ns: meta.modified().ok().and_then(|modified| {
                modified
                    .duration_since(UNIX_EPOCH)
                    .ok()
                    .map(|elapsed| elapsed.as_nanos())
            }),
            inode: file_identity(meta),
        }
    }
}

fn file_identity(meta: &std::fs::Metadata) -> Option<(u64, u64)> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some((meta.dev(), meta.ino()))
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        Some((u64::from(meta.volume_serial_number()?), meta.file_index()?))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = meta;
        None
    }
}

/// `None` means the stat failed for a reason other than absence. Callers keep the baseline.
fn stat_file(path: &Path) -> Option<FileStamp> {
    match std::fs::metadata(path) {
        Ok(meta) => Some(FileStamp::from_meta(&meta)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(FileStamp::missing()),
        Err(_) => None,
    }
}

async fn stat_aligned(paths: &[PathBuf]) -> Option<Vec<Option<FileStamp>>> {
    let paths = paths.to_vec();
    crate::ipc::run_blocking(move || {
        Ok::<_, crate::ipc::error::IpcError>(paths.iter().map(|path| stat_file(path)).collect())
    })
        .await
        .ok()
}

async fn stat_map(paths: Vec<PathBuf>) -> Option<HashMap<PathBuf, Option<FileStamp>>> {
    crate::ipc::run_blocking(move || {
        let mut stamps = HashMap::with_capacity(paths.len());
        for path in paths {
            stamps.insert(path.clone(), stat_file(&path));
        }
        Ok::<_, crate::ipc::error::IpcError>(stamps)
    })
    .await
    .ok()
}

fn target_key(target: &ReferenceTargetRef) -> String {
    format!(
        "{}|{}|{}|{}|{}",
        target.target.host_id,
        target.target.owner_id,
        target.target.epoch.0,
        target.target.backend_session_id,
        target.provider_session_id.as_deref().unwrap_or("")
    )
}

struct Entry {
    target: ReferenceTargetRef,
    paths: Vec<PathBuf>,
    baseline: Vec<Option<FileStamp>>,
    expires_at: Instant,
    last_emit: Option<Instant>,
    pending: bool,
    registered_seq: u64,
}

struct Registry {
    entries: HashMap<String, Entry>,
    cap: usize,
    ttl: Duration,
    coalesce: Duration,
    next_seq: u64,
    next_registration: u64,
}

impl Registry {
    fn new(cap: usize, ttl: Duration, coalesce: Duration) -> Self {
        Self {
            entries: HashMap::new(),
            cap: cap.max(1),
            ttl,
            coalesce,
            next_seq: 0,
            next_registration: 0,
        }
    }

    fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    fn contains(&self, target: &ReferenceTargetRef) -> bool {
        self.entries.contains_key(&target_key(target))
    }

    fn forget(&mut self, target: &ReferenceTargetRef) {
        self.entries.remove(&target_key(target));
    }

    fn watched_paths(&self) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        for entry in self.entries.values() {
            paths.extend(entry.paths.iter().cloned());
        }
        paths.sort();
        paths.dedup();
        paths
    }

    fn refresh(
        &mut self,
        target: ReferenceTargetRef,
        paths: Vec<PathBuf>,
        baseline: Vec<Option<FileStamp>>,
        now: Instant,
    ) {
        let key = target_key(&target);
        if paths.is_empty() {
            self.entries.remove(&key);
            return;
        }
        if !self.entries.contains_key(&key) && self.entries.len() >= self.cap {
            self.evict_one();
        }
        let (registered_seq, last_emit) = match self.entries.get(&key) {
            Some(existing) => (existing.registered_seq, existing.last_emit),
            None => {
                self.next_registration = self.next_registration.saturating_add(1);
                (self.next_registration, None)
            }
        };
        // The client just read this page, so a change already covered by that read is not pending.
        self.entries.insert(
            key,
            Entry {
                target,
                paths,
                baseline,
                expires_at: now + self.ttl,
                last_emit,
                pending: false,
                registered_seq,
            },
        );
    }

    fn evict_one(&mut self) {
        let victim = self
            .entries
            .iter()
            .min_by_key(|(_, entry)| (entry.expires_at, entry.registered_seq))
            .map(|(key, _)| key.clone());
        if let Some(key) = victim {
            self.entries.remove(&key);
        }
    }

    /// Drop expired targets, then emit at most one frame per target whose metadata moved and
    /// whose coalesce window has elapsed. `None` stamps are failed stats and do not count.
    fn observe(
        &mut self,
        now: Instant,
        stamps: &HashMap<PathBuf, Option<FileStamp>>,
    ) -> Vec<ReferenceHistoryEvent> {
        let expired: Vec<String> = self
            .entries
            .iter()
            .filter(|(_, entry)| now >= entry.expires_at)
            .map(|(key, _)| key.clone())
            .collect();
        for key in expired {
            self.entries.remove(&key);
        }
        let coalesce = self.coalesce;
        let mut next_seq = self.next_seq;
        let mut events = Vec::new();
        let keys: Vec<String> = self.entries.keys().cloned().collect();
        for key in keys {
            let Some(entry) = self.entries.get_mut(&key) else {
                continue;
            };
            let mut changed = false;
            for (index, path) in entry.paths.iter().enumerate() {
                let Some(Some(stamp)) = stamps.get(path).cloned() else {
                    continue;
                };
                let previous = entry.baseline.get(index).cloned().flatten();
                if previous.as_ref() != Some(&stamp) {
                    changed = true;
                    if entry.baseline.len() <= index {
                        entry.baseline.resize(index + 1, None);
                    }
                    entry.baseline[index] = Some(stamp);
                }
            }
            if changed {
                entry.pending = true;
            }
            let window_open = entry
                .last_emit
                .is_none_or(|emitted| now.saturating_duration_since(emitted) >= coalesce);
            if entry.pending && window_open {
                entry.pending = false;
                entry.last_emit = Some(now);
                next_seq = next_seq.saturating_add(1);
                events.push(ReferenceHistoryEvent::ReferenceHistoryChanged {
                    target: entry.target.clone(),
                    change_seq: next_seq.to_string(),
                });
            }
        }
        self.next_seq = next_seq;
        events
    }
}

/// Stops the poller when the gateway drops. The poller holds the watch, not this guard.
pub struct HistoryWatchStop(tokio::sync::watch::Sender<bool>);

impl Drop for HistoryWatchStop {
    fn drop(&mut self) {
        let _ = self.0.send(true);
    }
}

/// Owning-host registry. Disk IO goes through [`crate::ipc::run_blocking`].
pub struct ReferenceHistoryWatch {
    registry: Mutex<Registry>,
    bus: Arc<AgentEventBus>,
    stop: tokio::sync::watch::Sender<bool>,
    poller: Mutex<Option<tokio::task::JoinHandle<()>>>,
    manual: AtomicBool,
}

impl ReferenceHistoryWatch {
    pub fn new(bus: Arc<AgentEventBus>) -> (Arc<Self>, HistoryWatchStop) {
        let (stop, _) = tokio::sync::watch::channel(false);
        let watch = Arc::new(Self {
            registry: Mutex::new(Registry::new(
                REFERENCE_HISTORY_WATCH_CAP,
                REFERENCE_HISTORY_WATCH_TTL,
                REFERENCE_HISTORY_WATCH_COALESCE,
            )),
            bus,
            stop: stop.clone(),
            poller: Mutex::new(None),
            manual: AtomicBool::new(false),
        });
        (watch, HistoryWatchStop(stop))
    }

    /// Test seam: do not spawn the interval poller. [`Self::manual_tick`] drives observe.
    pub fn disarm_poller(&self) {
        self.manual.store(true, Ordering::Release);
    }

    pub fn registration_count(&self) -> usize {
        self.registry.lock().len()
    }

    pub fn forget(&self, target: &ReferenceTargetRef) {
        self.registry.lock().forget(target);
    }

    /// Refresh the TTL for a local read, or drop a target this host must not watch.
    ///
    /// An empty path list is a resolved stream with no file (scrollback, not started). That
    /// read still counts: it clears a watch that no longer names a store.
    pub async fn note_read(
        self: &Arc<Self>,
        admit: HistoryWatchAdmit,
        target: ReferenceTargetRef,
        paths: Vec<PathBuf>,
    ) -> bool {
        if admit != HistoryWatchAdmit::Local || paths.is_empty() {
            self.forget(&target);
            return false;
        }
        let Some(baseline) = stat_aligned(&paths).await else {
            return false;
        };
        self.registry
            .lock()
            .refresh(target, paths, baseline, Instant::now());
        self.ensure_poller();
        true
    }

    /// One poll step. Production and tests share it; tests call it instead of waiting.
    pub async fn manual_tick(&self) -> Vec<ReferenceHistoryEvent> {
        self.poll_once().await
    }

    fn ensure_poller(self: &Arc<Self>) {
        if self.manual.load(Ordering::Acquire) {
            return;
        }
        let mut slot = self.poller.lock();
        if slot.as_ref().is_some_and(|handle| !handle.is_finished()) {
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let watch = Arc::clone(self);
        let mut stop = self.stop.subscribe();
        if *stop.borrow() {
            return;
        }
        *slot = Some(runtime.spawn(async move {
            let mut interval = tokio::time::interval(REFERENCE_HISTORY_WATCH_POLL);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    biased;
                    changed = stop.changed() => {
                        if changed.is_err() || *stop.borrow() {
                            break;
                        }
                    }
                    _ = interval.tick() => {
                        watch.poll_once().await;
                    }
                }
            }
        }));
    }

    async fn poll_once(&self) -> Vec<ReferenceHistoryEvent> {
        let paths = self.registry.lock().watched_paths();
        if paths.is_empty() {
            return Vec::new();
        }
        let Some(stamps) = stat_map(paths).await else {
            return Vec::new();
        };
        let events = self.registry.lock().observe(Instant::now(), &stamps);
        for event in &events {
            self.bus.publish_reference_history(event);
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::{FileStamp, Registry};
    use super::super::history::{ReferenceHistoryLocation, ReferenceHistoryStream};
    use super::super::types::{ReferenceHistoryEvent, ReferenceTargetRef};
    use crate::agent_control::events::AgentEventBus;
    use crate::scoped_contracts::{Epoch, TargetRef};
    use serde_json::Value;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    fn target(host: &str, owner: &str, session: &str) -> ReferenceTargetRef {
        ReferenceTargetRef::without_provider_session(TargetRef {
            host_id: host.into(),
            owner_id: owner.into(),
            epoch: Epoch(1),
            backend_session_id: session.into(),
        })
    }

    fn registry(cap: usize, ttl: Duration, coalesce: Duration) -> Registry {
        Registry::new(cap, ttl, coalesce)
    }

    fn stamp(len: u64) -> FileStamp {
        FileStamp { exists: true, len, mtime_ns: Some(1), inode: Some((1, 1)) }
    }

    fn changed(path: &Path, len: u64) -> HashMap<PathBuf, Option<FileStamp>> {
        let mut stamps = HashMap::new();
        stamps.insert(path.to_path_buf(), Some(stamp(len)));
        stamps
    }

    fn assert_no_path(frame: &str, path: &Path) {
        let rendered = path.to_string_lossy();
        assert!(!frame.contains(rendered.as_ref()), "{frame}");
        let value: Value = serde_json::from_str(frame).expect("frame");
        let mut keys: Vec<_> = value.as_object().expect("object").keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["changeSeq", "target", "type"]);
        assert!(value.get("path").is_none());
        assert!(value["target"].get("path").is_none());
        assert!(value["target"].get("model").is_none());
        assert_eq!(value["type"], "referenceHistoryChanged");
    }

    #[test]
    fn paired_and_foreign_targets_are_not_this_owners_watch() {
        let local = target("host", "owner", "sess");
        assert_eq!(
            admit_reference_history_watch(&local, "host", "owner", false),
            HistoryWatchAdmit::Local
        );
        assert_eq!(
            admit_reference_history_watch(&local, "host", "owner", true),
            HistoryWatchAdmit::PairedOwner
        );
        assert_eq!(
            admit_reference_history_watch(&target("other", "owner", "sess"), "host", "owner", false),
            HistoryWatchAdmit::Foreign
        );
        assert_eq!(
            admit_reference_history_watch(&local, "host", "other-owner", false),
            HistoryWatchAdmit::Foreign
        );
    }

    #[test]
    fn a_store_watch_includes_the_database_and_its_wal() {
        let store = PathBuf::from("/isolated/opencode.db");
        let stream = ReferenceHistoryStream {
            source: super::super::types::ReferenceHistorySource::OpencodeTranscript,
            kind: super::super::types::ReferenceNativeHistoryKind::Opencode,
            availability: super::super::types::ReferenceHistoryAvailability::Native,
            stream_id: "stream".into(),
            location: ReferenceHistoryLocation::Rows {
                store: store.clone(),
                session_id: "ses".into(),
            },
            unavailable_reason: None,
            owner: target("host", "owner", "sess"),
        };
        assert_eq!(
            reference_history_watch_paths(&stream),
            vec![store.clone(), PathBuf::from("/isolated/opencode.db-wal")]
        );
        let transcript = ReferenceHistoryStream {
            location: ReferenceHistoryLocation::File(PathBuf::from("/isolated/session.jsonl")),
            source: super::super::types::ReferenceHistorySource::ClaudeTranscript,
            kind: super::super::types::ReferenceNativeHistoryKind::Claude,
            ..stream
        };
        assert_eq!(
            reference_history_watch_paths(&transcript),
            vec![PathBuf::from("/isolated/session.jsonl")]
        );
        let cursor = ReferenceHistoryStream {
            location: ReferenceHistoryLocation::File(PathBuf::from("/isolated/store.db")),
            source: super::super::types::ReferenceHistorySource::CursorTranscript,
            kind: super::super::types::ReferenceNativeHistoryKind::Cursor,
            ..transcript
        };
        assert_eq!(
            reference_history_watch_paths(&cursor),
            vec![
                PathBuf::from("/isolated/store.db"),
                PathBuf::from("/isolated/store.db-wal"),
            ]
        );
    }

    #[test]
    fn an_unrefreshed_target_expires_and_a_read_refreshes_the_ttl() {
        let ttl = Duration::from_secs(30);
        let mut registry = registry(4, ttl, Duration::ZERO);
        let now = Instant::now();
        let path = PathBuf::from("/isolated/session.jsonl");
        let watched = target("host", "owner", "live");
        registry.refresh(watched.clone(), vec![path.clone()], vec![Some(stamp(1))], now);
        let stale = target("host", "owner", "stale");
        registry.refresh(stale.clone(), vec![path.clone()], vec![Some(stamp(1))], now);

        registry.refresh(
            watched.clone(),
            vec![path.clone()],
            vec![Some(stamp(1))],
            now + ttl,
        );
        let events = registry.observe(now + ttl, &changed(&path, 1));
        assert!(events.is_empty(), "expiry is silent, and the refreshed target is unchanged");
        assert!(!registry.contains(&stale));
        assert!(registry.contains(&watched));

        let events = registry.observe(now + ttl, &changed(&path, 3));
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            ReferenceHistoryEvent::ReferenceHistoryChanged {
                target: watched,
                change_seq: "1".into(),
            }
        );
    }

    #[test]
    fn the_cap_evicts_the_least_recently_read_target() {
        let mut registry = registry(2, Duration::from_secs(60), Duration::ZERO);
        let now = Instant::now();
        let path = PathBuf::from("/isolated/session.jsonl");
        for session in ["s1", "s2", "s3"] {
            registry.refresh(
                target("host", "owner", session),
                vec![path.clone()],
                vec![Some(stamp(1))],
                now,
            );
        }
        assert_eq!(registry.len(), 2);
        assert!(!registry.contains(&target("host", "owner", "s1")));
        let events = registry.observe(now, &changed(&path, 9));
        let sessions: Vec<_> = events
            .iter()
            .map(|event| match event {
                ReferenceHistoryEvent::ReferenceHistoryChanged { target, .. } => {
                    target.target.backend_session_id.clone()
                }
            })
            .collect();
        assert!(!sessions.iter().any(|session| session == "s1"));
        assert_eq!(sessions.len(), 2);
    }

    #[test]
    fn one_window_emits_one_frame_and_an_unchanged_tick_is_silent() {
        let mut registry = registry(4, Duration::from_secs(60), Duration::from_secs(10));
        let now = Instant::now();
        let path = PathBuf::from("/isolated/session.jsonl");
        let watched = target("host", "owner", "sess");
        registry.refresh(watched.clone(), vec![path.clone()], vec![Some(stamp(1))], now);

        assert!(registry.observe(now, &changed(&path, 1)).is_empty());
        let first = registry.observe(now, &changed(&path, 2));
        assert_eq!(first.len(), 1);
        let second_inside = registry.observe(now + Duration::from_secs(1), &changed(&path, 3));
        assert!(second_inside.is_empty(), "a second change inside the window does not emit yet");
        let second = registry.observe(now + Duration::from_secs(10), &HashMap::new());
        assert_eq!(second.len(), 1, "the pending change emits once when the window elapses");
        let frame = serde_json::to_string(&second[0]).expect("frame");
        assert_no_path(&frame, &path);
    }

    #[tokio::test]
    async fn a_manual_tick_emits_once_for_a_stat_change_and_stays_silent_without_one() {
        let root = tempfile::tempdir().expect("root");
        let path = root.path().join("session.jsonl");
        std::fs::write(&path, b"one\n").expect("transcript");
        let bus = Arc::new(AgentEventBus::new(8));
        let (watch, _stop) = ReferenceHistoryWatch::new(Arc::clone(&bus));
        watch.disarm_poller();
        let watched = target("host", "owner", "sess");
        let mut frames = bus.subscribe_history();
        assert!(
            watch
                .note_read(HistoryWatchAdmit::Local, watched, vec![path.clone()])
                .await
        );
        assert!(watch.manual_tick().await.is_empty(), "registration is not a change");
        assert!(frames.try_recv().is_err());

        std::fs::write(&path, b"one\ntwo\n").expect("append");
        let emitted = watch.manual_tick().await;
        assert_eq!(emitted.len(), 1);
        let frame = tokio::time::timeout(Duration::from_secs(5), frames.recv())
            .await
            .expect("bounded")
            .expect("frame");
        assert_no_path(&frame, &path);
        assert_eq!(
            serde_json::from_str::<ReferenceHistoryEvent>(&frame).expect("dto"),
            emitted[0]
        );
        assert!(watch.manual_tick().await.is_empty());
        assert!(frames.try_recv().is_err(), "no change stays silent");
    }

    #[tokio::test]
    async fn a_wal_metadata_change_emits_once_and_a_foreign_read_does_not_register() {
        let root = tempfile::tempdir().expect("root");
        let store = root.path().join("opencode.db");
        let wal = wal_sibling(&store);
        std::fs::write(&store, b"db").expect("db");
        std::fs::write(&wal, b"wal").expect("wal");
        let bus = Arc::new(AgentEventBus::new(8));
        let (watch, _stop) = ReferenceHistoryWatch::new(Arc::clone(&bus));
        watch.disarm_poller();
        let mut frames = bus.subscribe_history();
        assert!(
            watch
                .note_read(
                    HistoryWatchAdmit::Local,
                    target("host", "owner", "sess"),
                    vec![store.clone(), wal.clone()],
                )
                .await
        );
        assert!(
            !watch
                .note_read(
                    HistoryWatchAdmit::PairedOwner,
                    target("host", "owner", "paired"),
                    vec![store.clone()],
                )
                .await
        );
        assert!(
            !watch
                .note_read(
                    HistoryWatchAdmit::Foreign,
                    target("other", "owner", "foreign"),
                    vec![store.clone()],
                )
                .await
        );
        assert_eq!(watch.registration_count(), 1);

        std::fs::write(&wal, b"wal-grew").expect("wal change");
        let emitted = watch.manual_tick().await;
        assert_eq!(emitted.len(), 1);
        let frame = tokio::time::timeout(Duration::from_secs(5), frames.recv())
            .await
            .expect("bounded")
            .expect("frame");
        assert_no_path(&frame, &store);
        assert_no_path(&frame, &wal);
        assert!(watch.manual_tick().await.is_empty());
        let _ = emitted;
    }
}
