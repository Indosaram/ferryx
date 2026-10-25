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
            Self::Persist(_) => "MACHINE_SERVICE_UNAVAILABLE",
        }
    }
}

pub struct SidebarSnapshotStore {
    path: PathBuf,
    /// Serializes revision assignment and the disk write; readers never wait on disk I/O.
    writer: Mutex<()>,
    current: RwLock<Option<Arc<Value>>>,
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
        }
    }

    /// Blocking: validates, assigns the next revision and persists atomically (temp + rename).
    /// Without `live` (no desktop connected) the model's live signals are not kept.
    pub fn store(&self, mut model: Value, live: bool) -> Result<u64, SidebarRejection> {
        if !accepts(&model) {
            return Err(SidebarRejection::UnsupportedSchema);
        }
        if !live {
            strip_live(&mut model);
        }
        if serde_json::to_vec(&model)
            .map_err(std::io::Error::from)?
            .len()
            > SIDEBAR_MODEL_MAX_BYTES
        {
            return Err(SidebarRejection::TooLarge);
        }
        let _writer = self.writer.lock();
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
        Ok(revision)
    }

    pub fn model(&self) -> Option<Arc<Value>> {
        self.current.read().clone()
    }

    /// Blocking: removes the live signals once their publisher is gone, under a new revision.
    /// Returns that revision, or None when there was nothing live to drop.
    pub fn drop_live(&self) -> Result<Option<u64>, SidebarRejection> {
        let _writer = self.writer.lock();
        let Some(current) = self.model() else {
            return Ok(None);
        };
        let mut model = (*current).clone();
        if !strip_live(&mut model) {
            return Ok(None);
        }
        let revision = current["revision"]
            .as_u64()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(SidebarRejection::RevisionExhausted)?;
        model["revision"] = Value::from(revision);
        crate::remote::auth::write_private_json(&self.path, &model)?;
        *self.current.write() = Some(Arc::new(model));
        Ok(Some(revision))
    }
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
