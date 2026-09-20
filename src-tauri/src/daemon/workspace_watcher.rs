//! Subscription-owned native watches: one bounded wakeup, no idle Git scans.
use super::DaemonWorkspaceService;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};

pub(crate) struct WorkspaceWatch {
    watcher: RecommendedWatcher,
    paths: BTreeMap<PathBuf, bool>,
    changed: tokio::sync::mpsc::Receiver<()>,
    debounce: Option<tokio::time::Instant>,
    // The worker, not its cancellable caller, owns refresh admission.
    refresh_slot: Option<tokio::sync::OwnedSemaphorePermit>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::{machine_protocol::Availability, workspace_catalog::CatalogRow};

    #[tokio::test]
    async fn parent_watch_promoted_to_project_observes_nested_file() {
        // Given a parent watched nonrecursively for one registered child.
        let (root, service, parent, file) = crate::ipc::run_blocking(|| {
            let root = tempfile::tempdir().unwrap();
            let parent = root.path().join("parent");
            std::fs::create_dir_all(parent.join("child")).unwrap();
            std::fs::create_dir_all(parent.join("other/deep")).unwrap();
            let parent = std::fs::canonicalize(parent).unwrap();
            let file = parent.join("other/deep/proof");
            std::fs::write(&file, b"before").unwrap();
            let service = Arc::new(DaemonWorkspaceService::new(
                crate::worktree::WorkspaceRegistry::new(),
                root.path().join("data/catalog"),
            ));
            service.catalog.lock().as_mut().unwrap().workspaces.insert(
                "child".into(),
                CatalogRow {
                    repo_root: parent.join("child"),
                    mirror_exposed: false,
                    availability: Availability::Ready,
                },
            );
            Ok((root, service, parent, file))
        })
        .await
        .unwrap();
        let (observed, mut events) = tokio::sync::mpsc::unbounded_channel();
        let mut watcher = WorkspaceWatch::new().unwrap();
        watcher.watcher = notify::recommended_watcher(move |event| {
            let _ = observed.send(event);
        })
        .unwrap();
        let watcher = watcher.refresh(service.clone()).await.unwrap();
        service.catalog.lock().as_mut().unwrap().workspaces.insert(
            "parent".into(),
            CatalogRow {
                repo_root: parent,
                mirror_exposed: false,
                availability: Availability::Ready,
            },
        );
        // When that existing parent becomes a project, then a nested file changes.
        let watcher = watcher.refresh(service.clone()).await.unwrap();
        let changed_file = file.clone();
        crate::ipc::run_blocking(move || {
            std::fs::write(changed_file, b"after").unwrap();
            Ok(())
        })
        .await
        .unwrap();
        let result = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(Ok(event)) = events.recv().await {
                    if event.paths.contains(&file) {
                        break;
                    }
                }
            }
        })
        .await;
        // Then real native notification reaches the observer; clean up before asserting.
        drop(watcher);
        drop(service);
        crate::ipc::run_blocking(move || {
            root.close().unwrap();
            Ok(())
        })
        .await
        .unwrap();
        eprintln!(
            "A12_WATCH_PROMOTION root_removed=true watcher_dropped=true observed={}",
            result.is_ok()
        );
        assert!(
            result.is_ok(),
            "promoted project watch missed nested file change"
        );
    }
}
impl WorkspaceWatch {
    pub(crate) fn new() -> Result<Self, String> {
        let (sender, changed) = tokio::sync::mpsc::channel(1);
        let watcher =
            notify::recommended_watcher(move |event: Result<notify::Event, notify::Error>| {
                match event {
                    Ok(event) if matches!(event.kind, notify::EventKind::Access(_)) => return,
                    Err(error) => tracing::warn!(%error, "Workspace watch failed"),
                    _ => {}
                }
                // Full means an invalidation is already pending; closed means unsubscribe.
                let _ = sender.try_send(());
            })
            .map_err(|_| "MACHINE_SERVICE_UNAVAILABLE")?;
        Ok(Self {
            watcher,
            paths: BTreeMap::new(),
            changed,
            debounce: None,
            refresh_slot: None,
        })
    }
    pub(crate) async fn refresh_bounded(
        mut self,
        service: Arc<DaemonWorkspaceService>,
        slot: tokio::sync::OwnedSemaphorePermit,
    ) -> Result<Self, String> {
        self.refresh_slot = Some(slot);
        let mut watcher = self.refresh(service).await?;
        watcher.refresh_slot = None;
        Ok(watcher)
    }
    pub(crate) async fn refresh(
        mut self,
        service: Arc<DaemonWorkspaceService>,
    ) -> Result<Self, String> {
        crate::ipc::run_blocking(move || {
            #[cfg(test)]
            if let Some(probe) = service.transaction_probe.read().clone() {
                probe("watchRefreshRequested");
            }
            let catalog = service.catalog().map_err(crate::ipc::IpcError::internal)?;
            let mut paths = BTreeMap::new();
            for row in catalog.workspaces.values() {
                if let Some(parent) = row.repo_root.parent() {
                    paths.entry(parent.to_owned()).or_insert(false);
                }
                if row.repo_root.is_dir() {
                    paths.insert(row.repo_root.clone(), true);
                }
            }
            if paths.len() > 2048 {
                return Err(crate::ipc::IpcError::internal("CAPACITY_EXCEEDED"));
            }
            for (path, recursive) in &self.paths {
                if paths.get(path) != Some(recursive) {
                    self.watcher.unwatch(path).map_err(|_| {
                        crate::ipc::IpcError::internal("MACHINE_SERVICE_UNAVAILABLE")
                    })?;
                }
            }
            for (path, recursive) in &paths {
                if self.paths.get(path) != Some(recursive) {
                    self.watcher
                        .watch(
                            path,
                            if *recursive {
                                RecursiveMode::Recursive
                            } else {
                                RecursiveMode::NonRecursive
                            },
                        )
                        .map_err(|_| {
                            crate::ipc::IpcError::internal("MACHINE_SERVICE_UNAVAILABLE")
                        })?;
                }
            }
            self.paths = paths;
            Ok(self)
        })
        .await
        .map_err(|_| "MACHINE_SERVICE_UNAVAILABLE".into())
    }
    pub(crate) async fn changed(&mut self) {
        if self.debounce.is_none() {
            if self.changed.recv().await.is_none() {
                std::future::pending::<()>().await;
            }
            self.debounce = Some(tokio::time::Instant::now() + Duration::from_millis(100));
        }
        // Retain the deadline across select cancellation by another event.
        tokio::time::sleep_until(self.debounce.expect("pending invalidation")).await;
        while self.changed.try_recv().is_ok() {}
        self.debounce = None;
    }
}
