//! Session-local, explicitly refreshed disk scans. Ordinary worktree listing does
//! not touch this state or perform disk measurement.
use crate::ipc::{run_blocking, IpcError, IpcErrorCode};
use crate::worktree::disk::{collect_workspace, WorkspaceDiskProgress, WorktreeDiskRow};
use crate::worktree::WorkspaceRegistry;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tauri::{AppHandle, Emitter, Runtime, State};

pub const WORKTREE_DISK_SCAN_PROGRESS_EVENT: &str = "worktree_disk_scan_progress";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiskScanStatus {
    Running,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskScanSnapshot {
    pub workspace_id: String,
    pub scan_id: String,
    pub status: DiskScanStatus,
    pub progress: WorkspaceDiskProgress,
    /// Populated only after the entire scan completes, never during progress.
    pub rows: Vec<WorktreeDiskRow>,
    pub error: Option<IpcError>,
}

struct WorkspaceScan {
    current: DiskScanSnapshot,
    cancelled: Arc<AtomicBool>,
    completed: Option<DiskScanSnapshot>,
}

#[derive(Clone, Default)]
pub struct WorktreeDiskScans(Arc<Mutex<HashMap<String, WorkspaceScan>>>);

impl WorktreeDiskScans {
    pub(crate) fn remove_deleted(&self, workspace_id: &str, path: &std::path::Path) {
        let mut scans = self.0.lock();
        let Some(scan) = scans.get_mut(workspace_id) else {
            return;
        };
        // Share publication's lock so an older worker cannot resurrect the row.
        scan.cancelled.store(true, Ordering::Release);
        if scan.current.status == DiskScanStatus::Running {
            scan.current.status = DiskScanStatus::Cancelled;
            scan.current.progress.current_path = None;
        }
        scan.current.rows.retain(|row| row.worktree.path != path);
        if let Some(completed) = &mut scan.completed {
            completed.rows.retain(|row| row.worktree.path != path);
        }
    }

    pub(crate) fn begin(
        &self,
        workspace_id: &str,
        refresh: bool,
    ) -> (DiskScanSnapshot, Option<Arc<AtomicBool>>) {
        let mut scans = self.0.lock();
        if !refresh {
            if let Some(scan) = scans.get(workspace_id) {
                if scan.current.status == DiskScanStatus::Running {
                    return (scan.current.clone(), None);
                }
                if let Some(completed) = &scan.completed {
                    return (completed.clone(), None);
                }
            }
        }
        let completed = scans.get(workspace_id).and_then(|scan| {
            scan.cancelled.store(true, Ordering::Release);
            scan.completed.clone()
        });
        let current = DiskScanSnapshot {
            workspace_id: workspace_id.to_owned(),
            scan_id: uuid::Uuid::new_v4().to_string(),
            status: DiskScanStatus::Running,
            progress: WorkspaceDiskProgress::default(),
            rows: Vec::new(),
            error: None,
        };
        let cancelled = Arc::new(AtomicBool::new(false));
        scans.insert(
            workspace_id.to_owned(),
            WorkspaceScan {
                current: current.clone(),
                cancelled: cancelled.clone(),
                completed,
            },
        );
        (current, Some(cancelled))
    }

    pub(crate) fn result(&self, workspace_id: &str) -> Option<DiskScanSnapshot> {
        self.0
            .lock()
            .get(workspace_id)
            .map(|scan| scan.current.clone())
    }

    fn progress(
        &self,
        workspace_id: &str,
        scan_id: &str,
        progress: WorkspaceDiskProgress,
    ) -> Option<DiskScanSnapshot> {
        let mut scans = self.0.lock();
        let scan = scans.get_mut(workspace_id)?;
        if scan.current.scan_id != scan_id || scan.current.status != DiskScanStatus::Running {
            return None;
        }
        scan.current.progress = progress;
        Some(scan.current.clone())
    }

    pub(crate) fn finish(
        &self,
        workspace_id: &str,
        scan_id: &str,
        result: Result<Vec<WorktreeDiskRow>, IpcError>,
    ) -> Option<DiskScanSnapshot> {
        let mut scans = self.0.lock();
        let scan = scans.get_mut(workspace_id)?;
        // This check and cache publication share the cancellation lock. A worker
        // completing after cancel/refresh can never resurrect its partial result.
        if scan.current.scan_id != scan_id
            || scan.current.status != DiskScanStatus::Running
            || scan.cancelled.load(Ordering::Acquire)
        {
            return None;
        }
        scan.current.progress.current_path = None;
        match result {
            Ok(rows) => {
                scan.current.status = DiskScanStatus::Completed;
                scan.current.rows = rows;
                scan.completed = Some(scan.current.clone());
            }
            Err(error) => {
                scan.current.status = if error.code == IpcErrorCode::ScanCancelled {
                    DiskScanStatus::Cancelled
                } else {
                    DiskScanStatus::Failed
                };
                scan.current.error = Some(error);
            }
        }
        Some(scan.current.clone())
    }

    pub(crate) fn cancel(&self, workspace_id: &str, scan_id: &str) -> Option<DiskScanSnapshot> {
        let mut scans = self.0.lock();
        let scan = scans.get_mut(workspace_id)?;
        if scan.current.scan_id != scan_id || scan.current.status != DiskScanStatus::Running {
            return None;
        }
        scan.cancelled.store(true, Ordering::Release);
        scan.current.status = DiskScanStatus::Cancelled;
        scan.current.progress.current_path = None;
        scan.current.error = Some(IpcError::new(
            IpcErrorCode::ScanCancelled,
            "Disk scan cancelled",
        ));
        Some(scan.current.clone())
    }
}

fn require_local(workspace_id: &str) -> Result<(), IpcError> {
    if crate::ssh::projects::is_remote(workspace_id) {
        Err(IpcError::new(
            IpcErrorCode::Unsupported,
            "Disk scans are only available for local workspaces",
        ))
    } else {
        Ok(())
    }
}

fn emit<R: Runtime>(app: &AppHandle<R>, snapshot: &DiskScanSnapshot) -> Result<(), IpcError> {
    app.emit(WORKTREE_DISK_SCAN_PROGRESS_EVENT, snapshot)
        .map_err(|error| IpcError::internal(format!("failed to emit disk scan progress: {error}")))
}

/// Returns immediately with the scan ID. Subscribe to progress BEFORE invoking.
/// refresh=false reuses running/completed scans; refresh=true cancels and replaces.
#[tauri::command]
pub async fn cmd_worktree_disk_scan_start<R: Runtime>(
    app: AppHandle<R>,
    registry: State<'_, WorkspaceRegistry>,
    scans: State<'_, WorktreeDiskScans>,
    workspace_id: String,
    refresh: bool,
) -> Result<DiskScanSnapshot, IpcError> {
    require_local(&workspace_id)?;
    // Registry lookup only clones the already-validated manager; all filesystem
    // and Git work below is inside run_blocking.
    let manager = registry.manager(&workspace_id).map_err(IpcError::from)?;
    let scans = scans.inner().clone();
    let (snapshot, cancelled) = scans.begin(&workspace_id, refresh);
    let Some(cancelled) = cancelled else {
        return Ok(snapshot);
    };
    if let Err(error) = emit(&app, &snapshot) {
        scans.finish(&workspace_id, &snapshot.scan_id, Err(error.clone()));
        return Err(error);
    }
    let scan_id = snapshot.scan_id.clone();
    tauri::async_runtime::spawn(async move {
        let worker_scans = scans.clone();
        let worker_app = app.clone();
        let worker_workspace = workspace_id.clone();
        let worker_id = scan_id.clone();
        let result = run_blocking(move || {
            collect_workspace(&manager, &cancelled, |progress| {
                if let Some(snapshot) =
                    worker_scans.progress(&worker_workspace, &worker_id, progress)
                {
                    emit(&worker_app, &snapshot)?;
                }
                Ok(())
            })
        })
        .await;
        if let Some(snapshot) = scans.finish(&workspace_id, &scan_id, result) {
            // A failed terminal event remains observable through result lookup.
            // Log delivery failure rather than silently losing it.
            if let Err(error) = emit(&app, &snapshot) {
                tracing::error!(workspace_id, scan_id, error = %error, "disk scan terminal event delivery failed");
            }
        }
    });
    Ok(snapshot)
}

#[tauri::command]
pub async fn cmd_worktree_disk_scan_cancel<R: Runtime>(
    app: AppHandle<R>,
    scans: State<'_, WorktreeDiskScans>,
    workspace_id: String,
    scan_id: String,
) -> Result<bool, IpcError> {
    require_local(&workspace_id)?;
    if let Some(snapshot) = scans.cancel(&workspace_id, &scan_id) {
        emit(&app, &snapshot)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Pure in-memory lookup: never starts or refreshes a scan.
#[tauri::command]
pub async fn cmd_worktree_disk_scan_result(
    scans: State<'_, WorktreeDiskScans>,
    workspace_id: String,
) -> Result<Option<DiskScanSnapshot>, IpcError> {
    require_local(&workspace_id)?;
    Ok(scans.result(&workspace_id))
}
