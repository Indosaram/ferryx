use crate::ipc::{run_blocking, IpcError};
use crate::worktree::{
    BranchDeletionPreview, DirtyState, WorkspaceRegistry, Worktree,
    WorktreeIdentity,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use crate::daemon::{DaemonClient, protocol::{DaemonRequest, DaemonResponse}};
use std::sync::Arc;

pub const WORKTREE_CHANGED_EVENT: &str = "worktree_changed";

pub(crate) fn worktree_response(response: DaemonResponse) -> Result<DaemonResponse, IpcError> {
    match response {
        DaemonResponse::WorktreeError { error } => Err(error),
        DaemonResponse::Error { message } => Err(IpcError::internal(message)),
        response => Ok(response),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateWorktreeRequest {
    pub workspace_id: String,
    pub worktree: WorktreeIdentity,
    pub base_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeleteWorktreeRequest {
    pub workspace_id: String,
    pub worktree: WorktreeIdentity,
    pub delete_branch: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorktreeStatusRequest {
    pub workspace_id: String,
    pub worktree: WorktreeIdentity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorktreeChangeKind {
    Created,
    Deleted,
    DestructivelyDeleted,
    #[serde(rename = "dirtyChanged")]
    DirtyChanged,
    Pruned,
    Updated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeChangedPayload {
    pub workspace_id: String,
    pub worktree: WorktreeIdentity,
    pub kind: WorktreeChangeKind,
}

pub(crate) fn emit_worktree_changed<R: Runtime>(
    app: &AppHandle<R>,
    workspace_id: String,
    worktree: WorktreeIdentity,
    kind: WorktreeChangeKind,
) -> Result<(), IpcError> {
    app.emit(
        WORKTREE_CHANGED_EVENT,
        WorktreeChangedPayload {
            workspace_id,
            worktree,
            kind,
        },
    )
    .map_err(|error| IpcError::internal(format!("failed to emit worktree_changed: {error}")))
}

#[tauri::command]
pub async fn cmd_worktree_list<R: Runtime>(
    app: AppHandle<R>,
    registry: State<'_, WorkspaceRegistry>,
    workspace_id: String,
) -> Result<Vec<Worktree>, IpcError> {
    if crate::ssh::projects::is_remote(&workspace_id) {
        let store = crate::ipc::ssh::get_ssh_store_path(&app)?;
        let id = workspace_id.clone();
        let (project, host) =
            run_blocking(move || crate::ssh::projects::resolve(&store, &id)).await?;
        let environment = crate::ssh::runtime::detect(&host).await?;
        let remote_worktrees =
            crate::ssh::worktree::list_remote(&host, &environment, &project.repo_root).await?;
        let worktrees = remote_worktrees
            .into_iter()
            .map(|wt| Worktree {
                path: std::path::PathBuf::from(wt.path),
                head: wt.head.unwrap_or_default(),
                branch: wt.branch,
                bare: wt.bare,
                detached: wt.detached,
                locked: None,
                prunable: None,
            })
            .collect();
        return Ok(worktrees);
    }

    let registry = (*registry).clone();
    run_blocking(move || {
        let manager = registry.manager(&workspace_id).map_err(IpcError::from)?;
        if !manager.is_git_backed() {
            return Ok(Vec::new());
        }
        manager.list_worktrees().map_err(IpcError::from)
    })
    .await
}

#[tauri::command]
pub async fn cmd_worktree_create<R: Runtime>(
    app: AppHandle<R>,
    registry: State<'_, WorkspaceRegistry>,
    request: CreateWorktreeRequest,
) -> Result<Worktree, IpcError> {
    let registry = (*registry).clone();
    let workspace_id = request.workspace_id.clone();
    let identity = request.worktree.clone();
    let event_workspace_id = workspace_id.clone();
    let event_identity = identity.clone();

    WorkspaceRegistry::validate_workspace_id(&workspace_id).map_err(IpcError::from)?;
    let created = match worktree_response(app.state::<Arc<DaemonClient>>().send_request(DaemonRequest::CreateWorktree {
        workspace_id, worktree: identity, base_ref: request.base_ref,
    }).await?)? {
        DaemonResponse::CreateWorktreeOk { worktree } => worktree,
        _ => return Err(IpcError::internal("Unexpected worktree response")),
    };
    registry.bump_revision();

    emit_worktree_changed(
        &app,
        event_workspace_id,
        event_identity,
        WorktreeChangeKind::Created,
    )?;
    Ok(created)
}

async fn delete_worktree<R: Runtime>(
    app: AppHandle<R>,
    registry: State<'_, WorkspaceRegistry>,
    request: DeleteWorktreeRequest,
    destructive: bool,
) -> Result<(), IpcError> {
    let registry = (*registry).clone();
    let workspace_id = request.workspace_id.clone();
    let identity = request.worktree.clone();
    let delete_branch = request.delete_branch.unwrap_or(false);
    let event_workspace_id = workspace_id.clone();
    let event_identity = identity.clone();

    WorkspaceRegistry::validate_workspace_id(&workspace_id).map_err(IpcError::from)?;
    // Capture the managed path before the daemon removes the checkout/record.
    let deleted_path = registry
        .manager(&workspace_id)
        .map_err(IpcError::from)?
        .worktree_path_for(&identity.ws_id, &identity.slug)
        .map_err(IpcError::from)?;
    let pruned = match worktree_response(app.state::<Arc<DaemonClient>>().send_request(DaemonRequest::DeleteWorktree {
        workspace_id, worktree: identity, delete_branch, destructive,
    }).await?)? {
        DaemonResponse::DeleteWorktreeOk { pruned } => pruned,
        _ => return Err(IpcError::internal("Unexpected worktree response")),
    };
    registry.bump_revision();

    // The disk-scan cache is optional app state: a host that never managed it
    // (a test harness, or a future embedder) must still be able to delete a
    // worktree. `state()` panics when unmanaged, so ask for it fallibly the way
    // every other optional-state call site in this crate does.
    if let Some(scans) = app.try_state::<crate::ipc::worktree_disk::WorktreeDiskScans>() {
        if let Some(snapshot) = scans.remove_deleted(&event_workspace_id, &deleted_path) {
            let _ = app.emit(
                crate::ipc::worktree_disk::WORKTREE_DISK_SCAN_PROGRESS_EVENT,
                snapshot,
            );
        }
    }

    emit_worktree_changed(
        &app,
        event_workspace_id.clone(),
        event_identity.clone(),
        if destructive {
            WorktreeChangeKind::DestructivelyDeleted
        } else {
            WorktreeChangeKind::Deleted
        },
    )?;
    if pruned {
        emit_worktree_changed(
            &app,
            event_workspace_id,
            event_identity,
            WorktreeChangeKind::Pruned,
        )?;
    }
    Ok(())
}

#[tauri::command]
pub async fn cmd_worktree_delete<R: Runtime>(
    app: AppHandle<R>,
    registry: State<'_, WorkspaceRegistry>,
    request: DeleteWorktreeRequest,
) -> Result<(), IpcError> {
    delete_worktree(app, registry, request, false).await
}

#[tauri::command]
pub async fn cmd_worktree_delete_destructive<R: Runtime>(
    app: AppHandle<R>,
    registry: State<'_, WorkspaceRegistry>,
    request: DeleteWorktreeRequest,
) -> Result<(), IpcError> {
    delete_worktree(app, registry, request, true).await
}

#[tauri::command]
pub async fn cmd_worktree_delete_preview(
    registry: State<'_, WorkspaceRegistry>,
    request: WorktreeStatusRequest,
) -> Result<BranchDeletionPreview, IpcError> {
    let registry = (*registry).clone();
    run_blocking(move || {
        let (manager, worktree) = registry
            .resolve_deletion_worktree(&request.workspace_id, &request.worktree)
            .map_err(IpcError::from)?;
        manager
            .branch_deletion_preview(&worktree.path)
            .map_err(IpcError::from)
    })
    .await
}

#[tauri::command]
pub async fn cmd_worktree_status<R: Runtime>(
    app: AppHandle<R>,
    registry: State<'_, WorkspaceRegistry>,
    request: WorktreeStatusRequest,
) -> Result<DirtyState, IpcError> {
    let registry = (*registry).clone();
    let workspace_id = request.workspace_id.clone();
    let identity = request.worktree.clone();
    let event_workspace_id = workspace_id.clone();
    let event_identity = identity.clone();

    let (status, changed) = run_blocking(move || {
        let (manager, worktree) = registry
            .resolve_worktree(&workspace_id, &identity)
            .map_err(IpcError::from)?;
        manager
            .observe_dirty_state(&worktree.path)
            .map_err(IpcError::from)
    })
    .await?;

    if changed {
        emit_worktree_changed(
            &app,
            event_workspace_id,
            event_identity,
            WorktreeChangeKind::DirtyChanged,
        )?;
    }
    Ok(status)
}

#[cfg(test)]
mod deletion_repair_tests {
    use super::*;
    use crate::ipc::worktree_disk::WorktreeDiskScans;
    use crate::worktree::{run_git, CreateWorktreeOptions};
    use tauri::{Listener, Manager};

    // Copy existing objects; never create commits or mutate the source repository.
    fn fixture() -> (
        tempfile::TempDir,
        WorkspaceRegistry,
        WorktreeIdentity,
        Worktree,
    ) {
        let target_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
        let dir = std::env::var("TMPDIR")
            .ok()
            .and_then(|t| tempfile::tempdir_in(t).ok())
            .or_else(|| tempfile::tempdir_in(&target_dir).ok())
            .unwrap_or_else(|| tempfile::tempdir().unwrap());
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        run_git(
            dir.path(),
            &[
                "clone",
                "--no-hardlinks",
                "--no-checkout",
                source.to_str().unwrap(),
                "repo",
            ],
        )
        .unwrap();
        let repo = dir.path().join("repo");
        let tip = run_git(&repo, &["rev-parse", "HEAD"]).unwrap();
        run_git(&repo, &["checkout", "--detach", "HEAD~1"]).unwrap();
        let registry = WorkspaceRegistry::new();
        registry.register("repair", &repo).unwrap();
        let manager = registry.manager("repair").unwrap();
        let identity = WorktreeIdentity {
            ws_id: "repair".into(),
            slug: "target".into(),
        };
        let path = manager
            .worktree_path_for(&identity.ws_id, &identity.slug)
            .unwrap();
        let wt = manager
            .create_worktree(
                CreateWorktreeOptions::new("repair", "target", path).with_base_ref(tip.trim()),
            )
            .unwrap();
        (dir, registry, identity, wt)
    }

    async fn deletion_daemon(
        root: &std::path::Path,
        registry: &WorkspaceRegistry,
    ) -> (Arc<DaemonClient>, tokio::sync::oneshot::Sender<()>, tokio::task::JoinHandle<()>) {
        let owner = Arc::new(crate::daemon::server::DaemonServer::new_with_paths(
            Some(root.join("config.json")),
            Some(root.join("auth.json")),
        ));
        owner.remote_state().machine_services.as_ref().unwrap().workspaces
            .register("repair", registry.manager("repair").unwrap().repo_root().to_str().unwrap())
            .unwrap();
        let socket = root.join("repair.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        let (stop, mut stopped) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let mut clients = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    _ = &mut stopped => break,
                    accepted = listener.accept() => {
                        let (stream, _) = accepted.unwrap();
                        clients.spawn(owner.clone().handle_client(stream));
                    }
                }
            }
            clients.shutdown().await;
        });
        (Arc::new(DaemonClient::new_with_socket(socket)), stop, task)
    }

    async fn stop_deletion_daemon(stop: tokio::sync::oneshot::Sender<()>, mut task: tokio::task::JoinHandle<()>) {
        stop.send(()).unwrap();
        match tokio::time::timeout(std::time::Duration::from_secs(10), &mut task).await {
            Ok(result) => result.unwrap(),
            Err(_) => {
                task.abort();
                assert!(task.await.unwrap_err().is_cancelled());
                panic!("deletion daemon failed to stop");
            }
        }
    }

    #[tokio::test]
    async fn deletion_repair_preview_reports_current_dirty_and_unmerged_loss() {
        let (_dir, registry, identity, wt) = fixture();
        let app = tauri::test::mock_builder()
            .manage(registry)
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let request = WorktreeStatusRequest {
            workspace_id: "repair".into(),
            worktree: identity,
        };
        let clean = cmd_worktree_delete_preview(app.state(), request.clone())
            .await
            .unwrap();
        assert!(!clean.merged);
        let clean = serde_json::to_value(clean).unwrap();
        assert_eq!(
            clean["dirtyState"]["isDirty"], false,
            "preview must include current dirty state"
        );
        assert_eq!(clean["missing"], false);
        std::fs::write(wt.path.join("repair-untracked.txt"), b"current loss").unwrap();
        let dirty = serde_json::to_value(
            cmd_worktree_delete_preview(app.state(), request)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(dirty["merged"], false);
        assert_eq!(dirty["dirtyState"]["isDirty"], true);
        assert_eq!(
            dirty["dirtyState"]["files"],
            serde_json::json!([{ "statusCode": "??", "path": "repair-untracked.txt" }])
        );
    }

    #[tokio::test]
    async fn deletion_repair_missing_record_preview_and_targeted_cleanup() {
        let (_dir, registry, identity, wt) = fixture();
        let manager = registry.manager("repair").unwrap();
        let other = manager.worktree_path_for("repair", "other").unwrap();
        manager
            .create_worktree(CreateWorktreeOptions::new("repair", "other", &other))
            .unwrap();
        let outside = manager.repo_root().parent().unwrap().join("outside");
        run_git(
            manager.repo_root(),
            &[
                "worktree",
                "add",
                "--detach",
                outside.to_str().unwrap(),
                "HEAD",
            ],
        )
        .unwrap();
        for path in [&wt.path, &other, &outside] {
            std::fs::remove_dir_all(path).unwrap();
        }
        let (client, stop, daemon) = deletion_daemon(_dir.path(), &registry).await;
        let app = tauri::test::mock_builder()
            .manage(client)
            .manage(registry)
            .manage(WorktreeDiskScans::default())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let preview = cmd_worktree_delete_preview(
            app.state(),
            WorktreeStatusRequest {
                workspace_id: "repair".into(),
                worktree: identity.clone(),
            },
        )
        .await;
        assert!(
            preview.is_ok(),
            "missing managed Git record must resolve for preview: {preview:?}"
        );
        assert_eq!(
            serde_json::to_value(preview.unwrap()).unwrap()["missing"],
            true
        );
        cmd_worktree_delete_destructive(
            app.handle().clone(),
            app.state(),
            DeleteWorktreeRequest {
                workspace_id: "repair".into(),
                worktree: identity,
                delete_branch: Some(true),
            },
        )
        .await
        .unwrap();
        let records = crate::worktree::git_worktree_list(manager.repo_root()).unwrap();
        assert!(!records.iter().any(|r| r.path == wt.path));
        assert!(records.iter().any(|r| r.path == other));
        assert!(records.iter().any(|r| r.path == outside));
        assert!(manager.canonical_allowed_path(&outside).is_err());
        assert!(manager
            .find_worktree_by_slug("repair", "other")
            .unwrap()
            .is_none());
        stop_deletion_daemon(stop, daemon).await;
    }

    #[tokio::test]
    async fn deletion_repair_success_removes_cached_row_and_blocks_stale_worker() {
        let (_dir, registry, identity, wt) = fixture();
        let scans = WorktreeDiskScans::default();
        let row = crate::worktree::disk::WorktreeDiskRow {
            worktree: wt.clone(),
            size_bytes: Some(1),
            last_commit_at: None,
            is_dirty: Some(false),
            dirty_files: vec![],
            error: None,
        };
        let (initial, _) = scans.begin("repair", false);
        scans.finish("repair", &initial.scan_id, Ok(vec![row.clone()]));
        let (worker, _) = scans.begin("repair", true);
        let (client, stop, daemon) = deletion_daemon(_dir.path(), &registry).await;
        let app = tauri::test::mock_builder()
            .manage(client)
            .manage(registry)
            .manage(scans.clone())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let (scan_event_tx, mut scan_event_rx) = tokio::sync::mpsc::unbounded_channel();
        app.listen(crate::ipc::worktree_disk::WORKTREE_DISK_SCAN_PROGRESS_EVENT, move |event: tauri::Event| {
            let snapshot: crate::ipc::worktree_disk::DiskScanSnapshot =
                serde_json::from_str(event.payload()).unwrap();
            scan_event_tx.send(snapshot).unwrap();
        });
        cmd_worktree_delete_destructive(
            app.handle().clone(),
            app.state(),
            DeleteWorktreeRequest {
                workspace_id: "repair".into(),
                worktree: identity,
                delete_branch: Some(true),
            },
        )
        .await
        .unwrap();
        let cancelled_event = tokio::time::timeout(std::time::Duration::from_secs(5), scan_event_rx.recv())
            .await
            .expect("delete must emit scan cancellation event")
            .expect("event received");
        assert_eq!(cancelled_event.status, crate::ipc::worktree_disk::DiskScanStatus::Cancelled);
        assert_eq!(cancelled_event.scan_id, worker.scan_id);
        assert!(!wt.path.exists());
        assert!(
            scans
                .finish("repair", &worker.scan_id, Ok(vec![row]))
                .is_none(),
            "pre-delete worker must not resurrect deleted row"
        );
        assert!(scans.result("repair").unwrap().rows.is_empty());
        let (cached, _) = scans.begin("repair", false);
        assert!(cached.rows.is_empty(), "reopen must not return deleted row");
        stop_deletion_daemon(stop, daemon).await;
    }
}
