//! Daemon-held copy of the desktop sidebar model (schemaVersion 1), served to remote clients.
//!
//! The model is opaque here: the desktop owns its shape. The daemon only bounds it, assigns a
//! monotonically increasing revision and persists it next to the workspace catalog.
//!
//! The model's `signals` member is the desktop's live state (activity, Inbox rows, bell marks). It
//! is only true while that desktop is connected, so it is dropped when the desktop disconnects and
//! never restored from disk; the structure (projects, order, collapsed set) persists.
use parking_lot::{Mutex, RwLock};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

pub const SIDEBAR_MODEL_MAX_BYTES: usize = 512 * 1024;
const SIDEBAR_SCHEMA_VERSION: u64 = 1;
/// The live-only member of a published model.
const LIVE_SIGNALS_KEY: &str = "signals";

#[derive(Debug, thiserror::Error)]
pub enum SidebarRejection {
    #[error("sidebar model exceeds {SIDEBAR_MODEL_MAX_BYTES} bytes")]
    TooLarge,
    #[error("sidebar model must be an object with schemaVersion {SIDEBAR_SCHEMA_VERSION}")]
    UnsupportedSchema,
    #[error("sidebar revision exhausted")]
    RevisionExhausted,
    #[error("sidebar publish belongs to a stale desktop lifetime")]
    StaleLifetime,
    #[error("sidebar snapshot could not be persisted: {0}")]
    Persist(#[from] std::io::Error),
}

impl SidebarRejection {
    /// Wire code, parsed by the desktop as a structured IpcError code.
    pub fn code(&self) -> &'static str {
        match self {
            Self::TooLarge => "PAYLOAD_TOO_LARGE",
            Self::UnsupportedSchema => "INVALID_REQUEST",
            Self::RevisionExhausted => "CAPACITY_EXCEEDED",
            Self::StaleLifetime => "STALE_EPOCH",
            Self::Persist(_) => "MACHINE_SERVICE_UNAVAILABLE",
        }
    }
}

#[cfg(test)]
type LifecycleProbe = Arc<dyn Fn(&str) -> std::io::Result<()> + Send + Sync>;

#[derive(Default)]
struct WriterState {
    /// Last model written to disk, used to skip redundant atomic writes when only `signals`
    /// (and the daemon-assigned `revision`) changed.
    last_persisted: Option<Arc<Value>>,
}

pub struct SidebarSnapshotStore {
    path: PathBuf,
    /// Serializes revision assignment and the disk write; readers never wait on disk I/O.
    writer: Mutex<WriterState>,
    current: RwLock<Option<Arc<Value>>>,
    /// Never held over persistence. Closing takes writer before presence; opening only presence.
    presence: Mutex<DesktopPresence>,
    desktop_stream_count: tokio::sync::watch::Sender<usize>,
    desktop_epoch: tokio::sync::watch::Sender<u64>,
    #[cfg(test)]
    pub(super) lifecycle_probe: RwLock<Option<LifecycleProbe>>,
}

#[derive(Default)]
struct DesktopPresence {
    streams: usize,
    epoch: u64,
}

impl SidebarSnapshotStore {
    /// Blocking: reads the persisted snapshot. A missing or invalid file starts empty.
    /// The live signals of a previous desktop session are never reloaded.
    pub fn load(path: PathBuf) -> Self {
        let current = match std::fs::read(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                tracing::warn!(%error, "Desktop sidebar snapshot unreadable; starting empty");
                None
            }
            Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
                Ok(mut model) if accepts(&model) && model["revision"].is_u64() => {
                    strip_live(&mut model);
                    Some(Arc::new(model))
                }
                Ok(_) => {
                    tracing::warn!("Desktop sidebar snapshot invalid; starting empty");
                    None
                }
                Err(error) => {
                    tracing::warn!(%error, "Desktop sidebar snapshot corrupt; starting empty");
                    None
                }
            },
        };
        Self {
            path,
            writer: Mutex::new(WriterState {
                last_persisted: current.clone(),
            }),
            current: RwLock::new(current),
            presence: Mutex::new(DesktopPresence::default()),
            desktop_stream_count: tokio::sync::watch::channel(0).0,
            desktop_epoch: tokio::sync::watch::channel(0).0,
            #[cfg(test)]
            lifecycle_probe: RwLock::new(None),
        }
    }

    /// Blocking: validates, assigns the next revision and persists atomically (temp + rename).
    /// A publish admitted in an older desktop lifetime cannot restore that lifetime's signals.
    pub fn store(
        &self,
        mut model: Value,
        events: &crate::remote::machine_events::MachineEvents,
    ) -> Result<u64, SidebarRejection> {
        let (admitted_live, admitted_epoch) = {
            let presence = self.presence.lock();
            (presence.streams > 0, presence.epoch)
        };
        #[cfg(test)]
        self.probe("presenceCaptured")?;
        if !accepts(&model) {
            return Err(SidebarRejection::UnsupportedSchema);
        }
        let mut writer = self.writer.lock();
        let presence = self.presence.lock();
        if presence.epoch != admitted_epoch {
            return Err(SidebarRejection::StaleLifetime);
        }
        if presence.streams == 0 || !admitted_live {
            strip_live(&mut model);
        }
        drop(presence);
        let previous = self
            .model()
            .and_then(|current| current["revision"].as_u64())
            .unwrap_or(0);
        let revision = previous
            .checked_add(1)
            .ok_or(SidebarRejection::RevisionExhausted)?;
        model["revision"] = Value::from(revision);
        // Serialize once for both the size bound and the disk payload.
        let serialized = serde_json::to_vec(&model).map_err(std::io::Error::from)?;
        if serialized.len() > SIDEBAR_MODEL_MAX_BYTES {
            return Err(SidebarRejection::TooLarge);
        }
        let needs_persist = !writer
            .last_persisted
            .as_deref()
            .is_some_and(|prior| same_structure(prior, &model));
        if needs_persist {
            #[cfg(test)]
            self.probe("publishPersist")?;
            write_private_bytes(&self.path, &serialized)?;
        }
        // Open must not wait on writer/disk. Fence its invalidation again at memory commit.
        let presence = self.presence.lock();
        if presence.epoch != admitted_epoch {
            if needs_persist {
                // Disk now holds a fenced write; force the next valid publish to rewrite it.
                writer.last_persisted = None;
            }
            return Err(SidebarRejection::StaleLifetime);
        }
        let model = Arc::new(model);
        if needs_persist {
            writer.last_persisted = Some(Arc::clone(&model));
        }
        *self.current.write() = Some(model);
        announce(events, revision);
        drop(presence);
        Ok(revision)
    }

    pub fn model(&self) -> Option<Arc<Value>> {
        self.current.read().clone()
    }

    /// Opening never acquires writer, which may be held across disk I/O by a close or publish.
    pub fn desktop_stream_opened(&self, events: &crate::remote::machine_events::MachineEvents) {
        let mut presence = self.presence.lock();
        presence.epoch = presence.epoch.wrapping_add(1);
        presence.streams += 1;
        if let Err(error) = self.invalidate(events) {
            tracing::warn!(%error, "Desktop sidebar open invalidation failed");
        }
        self.desktop_epoch.send_replace(presence.epoch);
        self.desktop_stream_count.send_replace(presence.streams);
    }

    pub fn desktop_epoch(&self) -> tokio::sync::watch::Receiver<u64> {
        self.desktop_epoch.subscribe()
    }

    pub fn desktop_stream_count(&self) -> tokio::sync::watch::Receiver<usize> {
        self.desktop_stream_count.subscribe()
    }

    /// Blocking: serializes close invalidation and its event with publishes. Memory and
    /// readers are invalidated before persistence, even if the write fails. Open never waits on I/O.
    pub fn desktop_stream_closed(
        &self,
        events: &crate::remote::machine_events::MachineEvents,
    ) -> Result<(), SidebarRejection> {
        let mut writer = self.writer.lock();
        let mut presence = self.presence.lock();
        presence.streams = presence.streams.saturating_sub(1);
        presence.epoch = presence.epoch.wrapping_add(1);
        let invalidated = self.invalidate(events);
        self.desktop_epoch.send_replace(presence.epoch);
        drop(presence);
        let result = (|| {
            if let Some(model) = invalidated? {
                #[cfg(test)]
                self.probe("dropLivePersist")?;
                let serialized = serde_json::to_vec(&*model).map_err(std::io::Error::from)?;
                write_private_bytes(&self.path, &serialized)?;
                writer.last_persisted = Some(model);
            }
            Ok(())
        })();
        // Count observers may tear down the service's storage after the final close.
        // Notify only after persistence finishes, including the error path. Re-read
        // presence because a new stream may have opened while disk I/O was in flight.
        let presence = self.presence.lock();
        self.desktop_stream_count.send_replace(presence.streams);
        result
    }

    /// Called under presence: bounded memory/event work only, never filesystem operations.
    fn invalidate(
        &self,
        events: &crate::remote::machine_events::MachineEvents,
    ) -> Result<Option<Arc<Value>>, SidebarRejection> {
        let Some(current) = self.model() else { return Ok(None); };
        let mut model = (*current).clone();
        strip_live(&mut model);
        let revision = current["revision"]
            .as_u64()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(SidebarRejection::RevisionExhausted)?;
        model["revision"] = Value::from(revision);
        let model = Arc::new(model);
        *self.current.write() = Some(Arc::clone(&model));
        announce(events, revision);
        Ok(Some(model))
    }

    #[cfg(test)]
    fn probe(&self, stage: &str) -> std::io::Result<()> {
        let probe = self.lifecycle_probe.read().clone();
        match probe {
            Some(probe) => probe(stage),
            None => Ok(()),
        }
    }
}

fn announce(events: &crate::remote::machine_events::MachineEvents, revision: u64) {
    events.publish("sidebarChanged", None, None, serde_json::json!({"revision": revision}));
}

/// Removes the live signals; true when the model carried any.
fn strip_live(model: &mut Value) -> bool {
    model
        .as_object_mut()
        .is_some_and(|map| map.remove(LIVE_SIGNALS_KEY).is_some())
}

fn accepts(model: &Value) -> bool {
    model.get("schemaVersion").and_then(Value::as_u64) == Some(SIDEBAR_SCHEMA_VERSION)
}

/// True when two sidebar models carry identical durable structure (every top-level member except
/// the ephemeral `signals` object and the daemon-assigned `revision`).
fn same_structure(a: &Value, b: &Value) -> bool {
    let (Some(a), Some(b)) = (a.as_object(), b.as_object()) else {
        return false;
    };
    let is_durable = |key: &str| key != LIVE_SIGNALS_KEY && key != "revision";
    let a_count = a.keys().filter(|key| is_durable(key)).count();
    let b_count = b.keys().filter(|key| is_durable(key)).count();
    if a_count != b_count {
        return false;
    }
    a.iter()
        .filter(|(key, _)| is_durable(key))
        .all(|(key, value)| b.get(key) == Some(value))
}

/// Atomically writes already-serialized JSON bytes with the same cross-process flock, owner-only
/// permissions, temp-file fsync, atomic rename, and parent-directory fsync semantics as
/// `crate::remote::auth::write_private_json`.
fn write_private_bytes(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    let _lock = {
        let mut options = std::fs::OpenOptions::new();
        options.create(true).truncate(false).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let file = options.open(path.with_extension("lock"))?;
        file.lock()?;
        file
    };
    let temp = path.with_extension(format!("tmp.{}", std::process::id()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    {
        use std::io::Write as _;
        let mut file = options.open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(&temp, path)?;
    if let Some(parent) = path.parent() {
        if let Ok(dir) = std::fs::File::open(parent) {
            let _ = dir.sync_all();
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "sidebar_snapshot_tests.rs"]
mod tests;

#[cfg(test)]
mod persist_dedup_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn store_skips_disk_rewrite_when_only_live_signals_change() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("data/desktop-sidebar.v1.json");
        let store = SidebarSnapshotStore::load(path.clone());
        let events = crate::remote::machine_events::MachineEvents::default();
        store.desktop_stream_opened(&events);

        let persist_calls = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&persist_calls);
        *store.lifecycle_probe.write() = Some(Arc::new(move |stage| {
            if stage == "publishPersist" {
                counter.fetch_add(1, Ordering::Relaxed);
            }
            Ok(())
        }));

        let base = |unread: bool, label: &str| {
            serde_json::json!({
                "schemaVersion": 1,
                "revision": 0,
                "projects": [{"groupId": "g1", "label": label}],
                "signals": {"unread": unread}
            })
        };

        // First publish writes disk.
        assert_eq!(store.store(base(false, "proj-a"), &events).unwrap(), 1);
        assert_eq!(persist_calls.load(Ordering::Relaxed), 1);

        // Signal-only change updates in-memory model & revision, without touching disk.
        assert_eq!(store.store(base(true, "proj-a"), &events).unwrap(), 2);
        assert_eq!(persist_calls.load(Ordering::Relaxed), 1);
        assert_eq!(store.model().unwrap()["signals"]["unread"], true);
        assert_eq!(store.model().unwrap()["revision"], 2);

        // Structural change writes disk again.
        assert_eq!(store.store(base(true, "proj-b"), &events).unwrap(), 3);
        assert_eq!(persist_calls.load(Ordering::Relaxed), 2);
        let reloaded = SidebarSnapshotStore::load(path);
        assert_eq!(reloaded.model().unwrap()["projects"][0]["label"], "proj-b");
    }
}
