//! Daemon-owned supervisors. Disk operations run on blocking workers.
use super::{runtime, Approvals, Manifest};
use crate::daemon::{pane_grants::PaneGrants, protocol::DaemonRequest};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, future::Future, path::PathBuf, sync::Arc};
use tokio::sync::{watch, Mutex};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Status { pub manifest: Manifest, pub enabled: bool }

struct Running { stop: watch::Sender<bool>, task: tokio::task::JoinHandle<Result<(), String>> }

pub const PLUGIN_APPROVAL_REQUIRED: &str = "PLUGIN_APPROVAL_REQUIRED";

#[derive(Debug, PartialEq, Eq)]
pub enum ApprovalError { IdentityChanged, Failed(String) }

impl ApprovalError {
    pub fn code(&self) -> &'static str {
        match self { Self::IdentityChanged => PLUGIN_APPROVAL_REQUIRED, Self::Failed(_) => "PLUGIN_APPROVAL_FAILED" }
    }
}

impl std::fmt::Display for ApprovalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IdentityChanged => write!(f, "{PLUGIN_APPROVAL_REQUIRED}: plugin identity changed; review it again"),
            Self::Failed(message) => f.write_str(message),
        }
    }
}

pub const MAX_PLUGIN_VIEWS: usize = 16;
pub const MAX_PLUGIN_VIEW_BYTES: usize = 512 * 1024;
pub const MAX_TOTAL_VIEWS: usize = 128;
pub const MAX_TOTAL_VIEW_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub struct ViewBudgetExceeded;

impl std::fmt::Display for ViewBudgetExceeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PLUGIN_VIEW_BUDGET_EXCEEDED")
    }
}

#[derive(Default)]
pub struct Views { documents: BTreeMap<(String, String), String> }

impl Views {
    pub fn insert(&mut self, plugin: String, view: String, document: String) -> Result<(), ViewBudgetExceeded> {
        let key = (plugin.clone(), view);
        let size = |key: &(String, String), value: &String| key.0.len() + key.1.len() + value.len();
        let old = self.documents.get(&key);
        let added_count = usize::from(old.is_none());
        let old_bytes = old.map_or(0, |value| size(&key, value));
        let new_bytes = size(&key, &document);
        let plugin_entries: Vec<_> = self.documents.iter().filter(|(key, _)| key.0 == plugin).collect();
        let plugin_bytes: usize = plugin_entries.iter().map(|(key, value)| size(key, value)).sum();
        let total_bytes: usize = self.documents.iter().map(|(key, value)| size(key, value)).sum();
        if plugin_entries.len() + added_count > MAX_PLUGIN_VIEWS
            || self.documents.len() + added_count > MAX_TOTAL_VIEWS
            || plugin_bytes - old_bytes + new_bytes > MAX_PLUGIN_VIEW_BYTES
            || total_bytes - old_bytes + new_bytes > MAX_TOTAL_VIEW_BYTES {
            return Err(ViewBudgetExceeded);
        }
        self.documents.insert(key, document);
        Ok(())
    }

    pub fn get(&self, key: &(String, String)) -> Option<&String> { self.documents.get(key) }
    pub fn clear_plugin(&mut self, plugin: &str) { self.documents.retain(|(name, _), _| name != plugin); }
}

pub struct Registry {
    pub approvals: Arc<parking_lot::Mutex<Approvals>>,
    path: PathBuf,
    consent: Mutex<()>,
    running: Mutex<BTreeMap<String, Running>>,
    pub views: Arc<parking_lot::RwLock<Views>>,
}

impl Registry {
    pub fn new(path: PathBuf) -> Self {
        Self { path, approvals: Arc::new(parking_lot::Mutex::new(Approvals::default())),
            consent: Mutex::new(()), running: Mutex::new(BTreeMap::new()), views: Arc::new(parking_lot::RwLock::new(Views::default())) }
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
                        let manifest = match Manifest::load(&root, &name) {
                            Ok(manifest) => manifest,
                            Err(error) if error.starts_with("PLUGIN_NATIVE_UNSUPPORTED") => {
                                tracing::warn!(plugin = %name, %error, "Native plugin rejected; explicit Wasm migration required");
                                continue;
                            },
                            Err(error) => return Err(crate::ipc::IpcError::internal(error)),
                        };
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

    pub async fn approve(&self, name: String, enabled: bool, expected: Option<Manifest>) -> Result<(), ApprovalError> {
        self.approve_from(super::config_root(), name, enabled, expected).await
    }

    pub(super) async fn approve_from(&self, root: Option<PathBuf>, name: String, enabled: bool, expected: Option<Manifest>) -> Result<(), ApprovalError> {
        let _consent = self.consent.lock().await;
        let stopped_name = name.clone();
        let path = self.path.clone();
        let approvals = Arc::clone(&self.approvals);
        tokio::task::spawn_blocking(move || {
            let mut current = approvals.lock();
            let mut next = current.clone();
            next.manifests.retain(|m| m.name != name);
            if enabled {
                let expected = expected.ok_or(ApprovalError::IdentityChanged)?;
                let root = root.ok_or_else(|| ApprovalError::Failed("plugin config root unavailable".into()))?;
                let manifest = Manifest::load(&root, &name).map_err(ApprovalError::Failed)?;
                if manifest != expected { return Err(ApprovalError::IdentityChanged); }
                manifest.approved_bytes().map_err(ApprovalError::Failed)?;
                next.manifests.push(manifest);
            }
            next.save(&path).map_err(ApprovalError::Failed)?;
            *current = next;
            Ok(())
        }).await.map_err(|e| ApprovalError::Failed(e.to_string()))??;
        self.stop(&stopped_name).await.map_err(ApprovalError::Failed)?;
        Ok(())
    }

    pub async fn start<F, Fut>(&self, manifest: Manifest, grants: Arc<PaneGrants>, dispatch: F) -> Result<(), String>
    where F: FnMut(DaemonRequest) -> Fut + Send + 'static,
          Fut: Future<Output = Result<serde_json::Value, String>> + Send + 'static,
    {
        let mut running = self.running.lock().await;
        let approvals = self.approvals.lock().clone();
        if !approvals.enabled(&manifest) { return Err("plugin is not enabled".into()); }
        if running.contains_key(&manifest.name) { return Ok(()); }
        let name = manifest.name.clone();
        let observer_name = name.clone();
        let task_name = name.clone();
        let views = Arc::clone(&self.views);
        let restart_views = Arc::clone(&views);
        let restart_name = name.clone();
        let (stop, stopped) = watch::channel(false);
        let (events, mut observed) = tokio::sync::mpsc::unbounded_channel();
        let task = tokio::spawn(async move {
            let observer = tokio::spawn(async move {
                while let Some(event) = observed.recv().await {
                    tracing::info!(plugin = %observer_name, ?event, "plugin lifecycle");
                }
            });
            let result = runtime::supervise_with_cleanup(manifest, &approvals, grants, stopped, events, dispatch,
                move || restart_views.write().clear_plugin(&restart_name)).await;
            views.write().clear_plugin(&task_name);
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
            let exhausted = running.task.is_finished();
            let _ = running.stop.send(true);
            match running.task.await {
                Err(error) => Err(error.to_string()),
                Ok(Err(error)) if exhausted => {
                    tracing::warn!(plugin = name, %error, "Removing exhausted plugin supervisor");
                    Ok(())
                },
                Ok(result) => result,
            }
        } else { Ok(()) };
        self.views.write().clear_plugin(name);
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
