//! Daemon-held copy of the desktop sidebar model (schemaVersion 1), served to remote clients.
//!
//! The model is opaque here: the desktop owns its shape. The daemon only bounds it, assigns a
//! monotonically increasing revision and persists it next to the workspace catalog.
use parking_lot::{Mutex, RwLock};
use serde_json::Value;
use std::{path::PathBuf, sync::Arc};

pub const SIDEBAR_MODEL_MAX_BYTES: usize = 512 * 1024;
const SIDEBAR_SCHEMA_VERSION: u64 = 1;

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
    pub fn load(path: PathBuf) -> Self {
        let current = match std::fs::read(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                tracing::warn!(%error, "Desktop sidebar snapshot unreadable; starting empty");
                None
            }
            Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
                Ok(model) if accepts(&model) && model["revision"].is_u64() => Some(Arc::new(model)),
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
        Self { path, writer: Mutex::new(()), current: RwLock::new(current) }
    }

    /// Blocking: validates, assigns the next revision and persists atomically (temp + rename).
    pub fn store(&self, mut model: Value) -> Result<u64, SidebarRejection> {
        if !accepts(&model) {
            return Err(SidebarRejection::UnsupportedSchema);
        }
        if serde_json::to_vec(&model).map_err(std::io::Error::from)?.len() > SIDEBAR_MODEL_MAX_BYTES {
            return Err(SidebarRejection::TooLarge);
        }
        let _writer = self.writer.lock();
        let previous = self.model().and_then(|current| current["revision"].as_u64()).unwrap_or(0);
        let revision = previous.checked_add(1).ok_or(SidebarRejection::RevisionExhausted)?;
        model["revision"] = Value::from(revision);
        crate::remote::auth::write_private_json(&self.path, &model)?;
        *self.current.write() = Some(Arc::new(model));
        Ok(revision)
    }

    pub fn model(&self) -> Option<Arc<Value>> {
        self.current.read().clone()
    }
}

fn accepts(model: &Value) -> bool {
    model.get("schemaVersion").and_then(Value::as_u64) == Some(SIDEBAR_SCHEMA_VERSION)
}

#[cfg(test)]
#[path = "sidebar_snapshot_tests.rs"]
mod tests;
