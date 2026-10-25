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
use std::{path::PathBuf, sync::Arc};

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

pub struct SidebarSnapshotStore {
    path: PathBuf,
    /// Serializes revision assignment and the disk write; readers never wait on disk I/O.
    writer: Mutex<()>,
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
            writer: Mutex::new(()),
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
        let _writer = self.writer.lock();
        let presence = self.presence.lock();
        if presence.epoch != admitted_epoch {
            return Err(SidebarRejection::StaleLifetime);
        }
        if presence.streams == 0 || !admitted_live {
            strip_live(&mut model);
        }
        drop(presence);
        if serde_json::to_vec(&model)
            .map_err(std::io::Error::from)?
            .len()
            > SIDEBAR_MODEL_MAX_BYTES
        {
            return Err(SidebarRejection::TooLarge);
        }
        let previous = self
            .model()
            .and_then(|current| current["revision"].as_u64())
            .unwrap_or(0);
        let revision = previous
            .checked_add(1)
            .ok_or(SidebarRejection::RevisionExhausted)?;
        model["revision"] = Value::from(revision);
        #[cfg(test)]
        self.probe("publishPersist")?;
        crate::remote::auth::write_private_json(&self.path, &model)?;
        // Open must not wait on writer/disk. Fence its invalidation again at memory commit.
        let presence = self.presence.lock();
        if presence.epoch != admitted_epoch {
            return Err(SidebarRejection::StaleLifetime);
        }
        *self.current.write() = Some(Arc::new(model));
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
        let _writer = self.writer.lock();
        let mut presence = self.presence.lock();
        presence.streams = presence.streams.saturating_sub(1);
        presence.epoch = presence.epoch.wrapping_add(1);
        let invalidated = self.invalidate(events);
        self.desktop_epoch.send_replace(presence.epoch);
        self.desktop_stream_count.send_replace(presence.streams);
        drop(presence);
        let Some(model) = invalidated? else { return Ok(()); };
        #[cfg(test)]
        self.probe("dropLivePersist")?;
        crate::remote::auth::write_private_json(&self.path, &*model)?;
        Ok(())
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

#[cfg(test)]
#[path = "sidebar_snapshot_tests.rs"]
mod tests;
