//! Durable delivery receipts for owner conversation turns. A written prompt is never retried.
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::{ConversationScope, SupervisorFault};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    scope: ConversationScope,
    message: String,
    written: bool,
}

pub struct PendingTurn {
    path: PathBuf,
    scope: ConversationScope,
    message: String,
    written: AtomicBool,
}

impl PendingTurn {
    pub async fn create(
        root: &Path,
        scope: &ConversationScope,
        message: &str,
    ) -> Result<Arc<Self>, SupervisorFault> {
        let dir = root.join("mhc-supervisor-pending");
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|error| SupervisorFault::launch(&format!("pending turn directory: {error}")))?;
        let pending = Arc::new(Self {
            path: dir.join(format!("{}.json", uuid::Uuid::new_v4())),
            scope: scope.clone(),
            message: message.to_owned(),
            written: AtomicBool::new(false),
        });
        pending.persist(false).await?;
        Ok(pending)
    }

    async fn persist(&self, written: bool) -> Result<(), SupervisorFault> {
        use tokio::io::AsyncWriteExt;
        let bytes = serde_json::to_vec(&Record {
            scope: self.scope.clone(),
            message: self.message.clone(),
            written,
        })
        .map_err(|error| SupervisorFault::protocol(&format!("pending turn: {error}")))?;
        let temporary = self.path.with_extension("tmp");
        let mut options = tokio::fs::OpenOptions::new();
        options.create(true).truncate(true).write(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options.open(&temporary).await
            .map_err(|e| SupervisorFault::launch(&format!("pending turn open: {e}")))?;
        file.write_all(&bytes).await
            .map_err(|e| SupervisorFault::launch(&format!("pending turn write: {e}")))?;
        file.sync_all().await
            .map_err(|e| SupervisorFault::launch(&format!("pending turn sync: {e}")))?;
        tokio::fs::rename(&temporary, &self.path).await
            .map_err(|e| SupervisorFault::launch(&format!("pending turn rename: {e}")))
    }

    /// Called by the sole stdin writer immediately before attempting any prompt bytes.
    pub async fn before_write(&self) -> Result<(), SupervisorFault> {
        self.persist(true).await?;
        self.written.store(true, Ordering::SeqCst);
        Ok(())
    }

    pub fn written(&self) -> bool {
        self.written.load(Ordering::SeqCst)
    }

    pub async fn complete(&self) -> Result<(), SupervisorFault> {
        match tokio::fs::remove_file(&self.path).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(SupervisorFault::launch(&format!("pending turn remove: {error}"))),
        }
    }

    #[cfg(test)]
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

/// A persisted pending record loaded from disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingRecord {
    pub path: PathBuf,
    pub scope: ConversationScope,
    pub message: String,
    pub written: bool,
}

/// A persisted turn classified on supervisor start or relaunch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconciledTurn {
    /// The prompt bytes were never written: safe to resend to the relaunched engine.
    Recoverable {
        path: PathBuf,
        scope: ConversationScope,
        message: String,
    },
    /// The prompt bytes were written: outcome is unknown, must never be resent.
    Interrupted {
        path: PathBuf,
        scope: ConversationScope,
        message: String,
        notice: String,
    },
}

impl ReconciledTurn {
    pub fn path(&self) -> &Path {
        match self {
            Self::Recoverable { path, .. } | Self::Interrupted { path, .. } => path,
        }
    }

    pub fn scope(&self) -> &ConversationScope {
        match self {
            Self::Recoverable { scope, .. } | Self::Interrupted { scope, .. } => scope,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::Recoverable { message, .. } | Self::Interrupted { message, .. } => message,
        }
    }

    pub fn is_recoverable(&self) -> bool {
        matches!(self, Self::Recoverable { .. })
    }

    pub fn is_interrupted(&self) -> bool {
        matches!(self, Self::Interrupted { .. })
    }

    pub async fn clear(&self) -> Result<(), SupervisorFault> {
        match tokio::fs::remove_file(self.path()).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(SupervisorFault::launch(&format!("pending turn remove: {e}"))),
        }
    }
}

/// Store for managing and reconciling persisted pending conversation turns.
pub struct PendingStore {
    dir: PathBuf,
}

impl PendingStore {
    pub fn new(root: &Path) -> Self {
        Self {
            dir: root.join("mhc-supervisor-pending"),
        }
    }

    /// Loads all persisted pending records from disk.
    pub async fn load_all(&self) -> Result<Vec<PendingRecord>, SupervisorFault> {
        let Ok(mut entries) = tokio::fs::read_dir(&self.dir).await else {
            return Ok(Vec::new());
        };
        let mut records = Vec::new();
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let Ok(bytes) = tokio::fs::read(&path).await else {
                continue;
            };
            if let Ok(record) = serde_json::from_slice::<Record>(&bytes) {
                records.push(PendingRecord {
                    path,
                    scope: record.scope,
                    message: record.message,
                    written: record.written,
                });
            }
        }
        records.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(records)
    }

    /// Classifies persisted pending turns on startup:
    /// - `written == false`: recoverable (never written to engine stdin, can be resent)
    /// - `written == true`: interrupted ("엔진이 재시작되어 작업이 중단됐습니다", outcome unknown, never resent)
    pub async fn reconcile_on_start(
        &self,
        reason: &str,
    ) -> Result<Vec<ReconciledTurn>, SupervisorFault> {
        let records = self.load_all().await?;
        let reconciled = records
            .into_iter()
            .map(|record| match record.written {
                true => ReconciledTurn::Interrupted {
                    path: record.path,
                    scope: record.scope,
                    message: record.message,
                    notice: format!("엔진이 재시작되어 작업이 중단됐습니다: {reason}"),
                },
                false => ReconciledTurn::Recoverable {
                    path: record.path,
                    scope: record.scope,
                    message: record.message,
                },
            })
            .collect();
        Ok(reconciled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[tokio::test]
    async fn pending_turn_lifecycle_creates_updates_and_completes() {
        let root = std::env::temp_dir().join(format!("mhc-pending-test-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&root).await.unwrap();
        let _guard = TempDir(root.clone());
        let scope = ConversationScope {
            project_id: None,
            channel_key: "test:channel".to_owned(),
        };
        let turn = PendingTurn::create(&root, &scope, "hello").await.unwrap();
        assert!(!turn.written());
        assert!(turn.path().exists());

        let raw = tokio::fs::read_to_string(turn.path()).await.unwrap();
        assert!(raw.contains(r#""written":false"#));
        assert!(raw.contains("hello"));

        turn.before_write().await.unwrap();
        assert!(turn.written());
        let raw2 = tokio::fs::read_to_string(turn.path()).await.unwrap();
        assert!(raw2.contains(r#""written":true"#));

        turn.complete().await.unwrap();
        assert!(!turn.path().exists());
    }

    #[tokio::test]
    async fn pending_store_loads_and_reconciles_records() {
        let root = std::env::temp_dir().join(format!("mhc-store-test-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&root).await.unwrap();
        let _guard = TempDir(root.clone());
        let scope = ConversationScope {
            project_id: None,
            channel_key: "test:scope".to_owned(),
        };

        let _turn1 = PendingTurn::create(&root, &scope, "unwritten message").await.unwrap();
        let turn2 = PendingTurn::create(&root, &scope, "written message").await.unwrap();
        turn2.before_write().await.unwrap();

        let store = PendingStore::new(&root);
        let records = store.load_all().await.unwrap();
        assert_eq!(records.len(), 2);

        let reconciled = store.reconcile_on_start("host reboot").await.unwrap();
        assert_eq!(reconciled.len(), 2);

        let recoverable = reconciled.iter().find(|t| t.is_recoverable()).unwrap();
        assert_eq!(recoverable.message(), "unwritten message");

        let interrupted = reconciled.iter().find(|t| t.is_interrupted()).unwrap();
        assert_eq!(interrupted.message(), "written message");
        if let ReconciledTurn::Interrupted { notice, .. } = interrupted {
            assert!(notice.contains("엔진이 재시작되어 작업이 중단됐습니다: host reboot"));
        } else {
            panic!("expected Interrupted");
        }

        recoverable.clear().await.unwrap();
        interrupted.clear().await.unwrap();
        assert!(store.load_all().await.unwrap().is_empty());
    }
}
