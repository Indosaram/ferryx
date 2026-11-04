//! Daemon-owned supervisors. Disk operations run on blocking workers.
use super::{runtime, Approvals, Manifest};
use crate::daemon::{pane_grants::PaneGrants, protocol::DaemonRequest};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, future::Future, path::PathBuf, sync::Arc};
use tokio::sync::{watch, Mutex};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Status { pub manifest: Manifest, pub enabled: bool }

struct Running { stop: watch::Sender<bool>, task: tokio::task::JoinHandle<Result<(), String>> }

pub struct Registry {
    pub approvals: Arc<parking_lot::Mutex<Approvals>>,
    path: PathBuf,
    running: Mutex<BTreeMap<String, Running>>,
    pub views: Arc<parking_lot::RwLock<BTreeMap<(String, String), String>>>,
}

impl Registry {
    pub fn new(path: PathBuf) -> Self {
        Self { path, approvals: Arc::new(parking_lot::Mutex::new(Approvals::default())),
            running: Mutex::new(BTreeMap::new()), views: Arc::new(parking_lot::RwLock::new(BTreeMap::new())) }
    }

    pub async fn restore(&self) -> Result<(), String> {
        let path = self.path.clone();
        let approvals = crate::ipc::run_blocking(move || Approvals::load(&path)
            .map_err(crate::ipc::IpcError::internal)).await.map_err(|e| e.to_string())?;
        *self.approvals.lock() = approvals;
        Ok(())
    }

    pub async fn list(&self) -> Result<Vec<Status>, String> {
        let approvals = self.approvals.lock().clone();
        crate::ipc::run_blocking(move || {
            let mut statuses = Vec::new();
            if let Some(root) = super::config_root() {
                match std::fs::read_dir(&root) {
                    Ok(entries) => for entry in entries {
                        let entry = entry.map_err(crate::ipc::IpcError::internal)?;
                        let name = entry.file_name().to_string_lossy().into_owned();
                        let manifest = Manifest::load(&root, &name).map_err(crate::ipc::IpcError::internal)?;
                        statuses.push(Status { enabled: approvals.enabled(&manifest), manifest });
                    },
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
                    Err(e) => return Err(crate::ipc::IpcError::internal(e)),
                }
            }
            statuses.sort_by(|a, b| a.manifest.name.cmp(&b.manifest.name));
            Ok(statuses)
        }).await.map_err(|e| e.to_string())
    }

    pub async fn approve(&self, name: String, enabled: bool) -> Result<(), String> {
        let stopped_name = name.clone();
        let path = self.path.clone();
        let approvals = Arc::clone(&self.approvals);
        crate::ipc::run_blocking(move || {
            let mut current = approvals.lock();
            let mut next = current.clone();
            next.manifests.retain(|m| m.name != name);
            if enabled {
                let root = super::config_root().ok_or_else(|| crate::ipc::IpcError::internal("plugin config root unavailable"))?;
                next.manifests.push(Manifest::load(&root, &name).map_err(crate::ipc::IpcError::internal)?);
            }
            next.save(&path).map_err(crate::ipc::IpcError::internal)?;
            *current = next;
            Ok(())
        }).await.map_err(|e| e.to_string())?;
        self.stop(&stopped_name).await?;
        Ok(())
    }

    pub async fn start<F, Fut>(&self, manifest: Manifest, grants: Arc<PaneGrants>, dispatch: F) -> Result<(), String>
    where F: FnMut(DaemonRequest) -> Fut + Send + 'static,
          Fut: Future<Output = Result<serde_json::Value, String>> + Send + 'static,
    {
        let mut running = self.running.lock().await;
        if running.contains_key(&manifest.name) { return Ok(()); }
        let approvals = self.approvals.lock().clone();
        if !approvals.enabled(&manifest) { return Err("plugin is not enabled".into()); }
        let name = manifest.name.clone();
        let observer_name = name.clone();
        let task_name = name.clone();
        let (stop, stopped) = watch::channel(false);
        let (events, mut observed) = tokio::sync::mpsc::unbounded_channel();
        let task = tokio::spawn(async move {
            let observer = tokio::spawn(async move {
                while let Some(event) = observed.recv().await { tracing::info!(plugin = %observer_name, ?event, "plugin lifecycle"); }
            });
            let result = runtime::supervise(manifest, &approvals, grants, stopped, events, dispatch).await;
            observer.await.map_err(|e| e.to_string())?;
            if let Err(error) = &result { tracing::error!(plugin = %task_name, %error, "plugin supervisor stopped"); }
            result
        });
        running.insert(name, Running { stop, task });
        Ok(())
    }

    pub async fn stop(&self, name: &str) -> Result<(), String> {
        let mut supervisors = self.running.lock().await;
        let result = if let Some(running) = supervisors.remove(name) {
            let _ = running.stop.send(true);
            running.task.await.map_err(|e| e.to_string()).and_then(|result| result)
        } else { Ok(()) };
        self.views.write().retain(|(plugin, _), _| plugin != name);
        result
    }

    pub async fn stop_all(&self) -> Result<(), String> {
        let names: Vec<_> = self.running.lock().await.keys().cloned().collect();
        let mut errors = Vec::new();
        for name in names {
            if let Err(error) = self.stop(&name).await { errors.push(format!("{name}: {error}")); }
        }
        if errors.is_empty() { Ok(()) } else { Err(errors.join("; ")) }
    }
}
