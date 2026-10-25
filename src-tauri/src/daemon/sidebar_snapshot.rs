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
        if presence.epoch != admitted_epoch
            && !(admitted_live
                && presence.streams == 0
                && presence.epoch == admitted_epoch.wrapping_add(1))
        {
            return Err(SidebarRejection::StaleLifetime);
        }
        if presence.streams == 0 || !admitted_live {
            strip_live(&mut model);
        }
        // Last-close needs writer, so it cannot invalidate this decision before commit/event.
        // First-open may advance the epoch, but a model admitted without presence is stripped.
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
        crate::remote::auth::write_private_json(&self.path, &model)?;
        *self.current.write() = Some(Arc::new(model));
        announce(events, revision);
        Ok(revision)
    }

    pub fn model(&self) -> Option<Arc<Value>> {
        self.current.read().clone()
    }

    /// Opening never acquires writer, which may be held across disk I/O by a close or publish.
    pub fn desktop_stream_opened(&self) {
        let mut presence = self.presence.lock();
        if presence.streams == 0 {
            presence.epoch = presence.epoch.wrapping_add(1);
        }
        presence.streams += 1;
        self.desktop_stream_count.send_replace(presence.streams);
    }

    pub fn desktop_stream_count(&self) -> tokio::sync::watch::Receiver<usize> {
        self.desktop_stream_count.subscribe()
    }

    /// Blocking: serializes final-close invalidation and its event with publishes. Memory and
    /// readers are invalidated before persistence, even if the write fails. Open never waits on I/O.
    pub fn desktop_stream_closed(
        &self,
        events: &crate::remote::machine_events::MachineEvents,
    ) -> Result<(), SidebarRejection> {
        let _writer = self.writer.lock();
        let mut presence = self.presence.lock();
        presence.streams = presence.streams.saturating_sub(1);
        if presence.streams != 0 {
            self.desktop_stream_count.send_replace(presence.streams);
            return Ok(());
        }
        presence.epoch = presence.epoch.wrapping_add(1);
        let Some(current) = self.model() else {
            self.desktop_stream_count.send_replace(0);
            return Ok(());
        };
        let mut model = (*current).clone();
        if !strip_live(&mut model) {
            self.desktop_stream_count.send_replace(0);
            return Ok(());
        }
        let revision = current["revision"]
            .as_u64()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(SidebarRejection::RevisionExhausted)?;
        model["revision"] = Value::from(revision);
        *self.current.write() = Some(Arc::new(model.clone()));
        announce(events, revision);
        self.desktop_stream_count.send_replace(0);
        drop(presence);
        #[cfg(test)]
        self.probe("dropLivePersist")?;
        crate::remote::auth::write_private_json(&self.path, &model)?;
        Ok(())
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
