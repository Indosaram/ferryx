use super::*;
use crate::worktree::{WorkspaceRegistry, WorktreeManager, WorktreeError, CreateWorktreeOptions};
use serde::Deserialize;
#[cfg(test)]
#[path = "worktree_authority_tests.rs"]
mod authority_tests;
#[cfg(all(test, unix))]
#[path = "worktree_wire_proof_tests.rs"]
mod wire_proof_tests;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Mutation {
    request_id: String,
    workspace_id: String,
    worktree: WorktreeIdentity,
    base_ref: Option<String>,
    delete_branch: Option<bool>,
    expected_revision: Option<crate::scoped_contracts::Epoch>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Query { workspace_id: String, ws_id: Option<String>, slug: Option<String> }
fn domain(error: WorktreeError) -> String {
    match error {
        WorktreeError::ParseError(code) if matches!(code.as_str(), "TIMEOUT" | "UNAUTHORIZED" | "OUTPUT_LIMIT_EXCEEDED") => code,
        WorktreeError::WorktreeNotFound { .. } => "WORKTREE_NOT_FOUND".into(),
        WorktreeError::NotAGitRepository { .. } => "NOT_A_GIT_REPOSITORY".into(),
        WorktreeError::DirtyWorktree { .. } => "DIRTY_WORKTREE".into(),
        WorktreeError::WriterAlreadyActive { .. } => "WORKTREE_BUSY".into(),
        WorktreeError::UnmergedBranch { .. } => "UNMERGED_BRANCH".into(),
        WorktreeError::WorktreeAlreadyExists { .. } => "WORKTREE_EXISTS".into(),
        _ => "INVALID_WORKTREE".into(),
    }
}
pub(crate) fn commit_legacy_revision(service: &crate::daemon::workspace_service::DaemonWorkspaceService, workspace: &str, removed: bool, deadline: Instant, worktree: &Worktree) -> Result<(), String> {
    let digest = observation_digest(workspace, &service.worktree_manager(workspace, false)?)?;
    let _publication = service.mutation_gate.try_lock_until(deadline).ok_or("TIMEOUT")?;
    let mut catalog = service.catalog()?;
    catalog.worktree_observations.insert(workspace.into(), digest);
    catalog.revision.0 = catalog.revision.0.checked_add(1).ok_or("CAPACITY_EXCEEDED")?;
    if super::super::workspace_catalog::persist(&service.catalog_path, &catalog).is_err() {
        *service.catalog.lock() = Err("MACHINE_SERVICE_UNAVAILABLE".into());
        return Err("OPERATION_OUTCOME_UNKNOWN".into());
    }
    let revision = catalog.revision;
    *service.catalog.lock() = Ok(catalog);
    service.worktree_committed(workspace, removed, revision, worktree);
    Ok(())
}
fn budget(context: &RequestContext) -> crate::worktree::git::GitBudget {
    crate::worktree::git::GitBudget { deadline: context.deadline, revoked: context.revoked.clone(), cancelled: context.cancelled.clone(), cancellation: Some(context.cancellation.clone()) }
}
fn identity(workspace: &str, identity: &WorktreeIdentity) -> Result<(), String> {
    if workspace != identity.ws_id { return Err("WORKSPACE_ID_MISMATCH".into()); }
    if identity.slug.trim() != identity.slug || workspace.trim() != workspace { return Err("INVALID_REQUEST".into()); }
    WorktreeManager::format_branch_name(workspace, &identity.slug).map_err(domain)?;
    Ok(())
}
fn rows(workspace: &str, manager: &WorktreeManager) -> Result<Vec<Worktree>, String> {
    if !manager.is_git_backed() { return Err("NOT_A_GIT_REPOSITORY".into()); }
    // NUL-separated porcelain preserves quotes, whitespace and Unicode paths.
    // Keep the legacy parser unchanged outside the machine projection.
    let output = crate::worktree::run_git(manager.repo_root(), &["worktree", "list", "--porcelain", "-z"]).map_err(domain)?;
    let mut listed = Vec::new();
    let mut current: Option<crate::worktree::Worktree> = None;
    for field in output.split('\0') {
        if let Some(path) = field.strip_prefix("worktree ") {
            if let Some(row) = current.take() { listed.push(row); }
            current = Some(crate::worktree::Worktree { path: crate::worktree::native_git_path(path), head: String::new(), branch: None, bare: false, detached: false, locked: None, prunable: None });
        } else if let Some(row) = &mut current {
            if let Some(value) = field.strip_prefix("HEAD ") { row.head = value.into(); }
            else if let Some(value) = field.strip_prefix("branch ") { row.branch = Some(value.into()); }
            else if field == "bare" { row.bare = true; }
            else if field == "detached" { row.detached = true; }
            else if field == "locked" || field.starts_with("locked ") { row.locked = Some(field.strip_prefix("locked ").unwrap_or("locked").into()); }
            else if field == "prunable" || field.starts_with("prunable ") { row.prunable = Some(field.strip_prefix("prunable ").unwrap_or("prunable").into()); }
        }
    }
    if let Some(row) = current { listed.push(row); }
    listed.into_iter().map(|row| {
        let info = row.orca_info();
        let identity = info.filter(|i| i.ws_id == workspace && row.path != manager.repo_root())
            .filter(|i| manager.worktree_path_for(&i.ws_id, &i.slug).ok().as_ref() == Some(&row.path))
            .map(|i| WorktreeIdentity { ws_id: i.ws_id, slug: i.slug });
        Ok(Worktree { workspace_id: workspace.into(), managed: identity.is_some(), identity,
            path: row.path.to_str().ok_or("INVALID_PATH")?.into(), head: row.head, branch: row.branch,
            bare: row.bare, detached: row.detached, locked: row.locked, prunable: row.prunable })
    }).collect()
}
fn project_worktree(workspace: &str, manager: &WorktreeManager, row: &crate::worktree::Worktree) -> Result<Worktree, String> {
    let identity = row.orca_info().filter(|i| i.ws_id == workspace && row.path != manager.repo_root())
        .filter(|i| manager.worktree_path_for(&i.ws_id, &i.slug).ok().as_ref() == Some(&row.path))
        .map(|i| WorktreeIdentity { ws_id: i.ws_id, slug: i.slug });
    Ok(Worktree { workspace_id: workspace.into(), managed: identity.is_some(), identity,
        path: row.path.to_str().ok_or("INVALID_PATH")?.into(), head: row.head.clone(), branch: row.branch.clone(),
        bare: row.bare, detached: row.detached, locked: row.locked.clone(), prunable: row.prunable.clone() })
}
fn target(workspace: &str, id: &WorktreeIdentity, manager: &WorktreeManager) -> Result<Worktree, String> {
    identity(workspace, id)?;
    let path = manager.worktree_path_for(workspace, &id.slug).map_err(domain)?;
    let canonical = manager.canonical_allowed_path(&path).map_err(domain)?;
    if canonical != path || canonical == manager.repo_root() { return Err("INVALID_WORKTREE".into()); }
    rows(workspace, manager)?.into_iter().find(|row| row.identity.as_ref() == Some(id)).ok_or("WORKTREE_NOT_FOUND".into())
}
fn observed_revision(service: &crate::daemon::workspace_service::DaemonWorkspaceService, workspace: &str, manager: &WorktreeManager, context: &RequestContext) -> Result<crate::scoped_contracts::Epoch, String> {
    service.observe_worktrees(workspace, observation_digest(workspace, manager)?, context.deadline)
}
fn observation_digest(workspace: &str, manager: &WorktreeManager) -> Result<String, String> {
    let rows = rows(workspace, manager)?;
    let mut observed = Vec::new();
    for row in &rows {
        if row.managed && row.prunable.is_none() {
            let path = std::path::Path::new(&row.path);
            if manager.canonical_allowed_path(path).ok().as_deref() == Some(path) {
                observed.push((row.path.clone(), manager.check_dirty(path).map_err(domain)?));
            }
        }
    }
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(&(rows, observed)).map_err(|_| "INVALID_WORKTREE")?)))
}
fn sessions(state: &RemoteGatewayState, path: &std::path::Path) -> Result<Vec<String>, String> {
    let backend = &state.machine_services.as_ref().expect("services").sessions;
    tokio::runtime::Handle::current().block_on(async {
        let mut live = Vec::new();
        for id in backend.list_sessions().await {
            let details = backend.describe_session(&id).await.map_err(|_| "MACHINE_SERVICE_UNAVAILABLE")?;
            if details.running && details.worktree_path.as_ref().is_some_and(|cwd| cwd.starts_with(path)) { live.push(id); }
        }
        live.sort(); Ok(live)
    })
}
pub async fn read(state: Arc<RemoteGatewayState>, headers: HeaderMap, query: Option<String>, preview: bool) -> Response {
    let id = uuid::Uuid::new_v4().to_string();
    execute(state, headers, false, id.clone(), move |state, headers, context| {
        let result = crate::worktree::git::with_git_budget(budget(context), || {
            authorize(state, headers)?;
            let uri: axum::http::Uri = format!("/?{}", query.as_deref().unwrap_or("")).parse().map_err(|_| "INVALID_REQUEST")?;
            let axum::extract::Query(query) = axum::extract::Query::<Query>::try_from_uri(&uri).map_err(|_| "INVALID_REQUEST")?;
            let service = &state.machine_services.as_ref().expect("services").workspaces;
            let gate = service.worktree_gate(&query.workspace_id);
            let _gate = gate.try_lock_until(context.deadline).ok_or("TIMEOUT")?;
            context.check()?;
            let manager = service.worktree_manager(&query.workspace_id, false)?;
            let revision = observed_revision(service, &query.workspace_id, &manager, context)?;
            if !preview { return Ok(serde_json::to_value(Worktrees { revision, worktrees: rows(&query.workspace_id, &manager)? }).expect("rows")); }
            let identity = WorktreeIdentity { ws_id: query.ws_id.ok_or("INVALID_REQUEST")?, slug: query.slug.ok_or("INVALID_REQUEST")? };
            self::identity(&query.workspace_id, &identity)?;
            let row = rows(&query.workspace_id, &manager)?.into_iter()
                .find(|row| row.identity.as_ref() == Some(&identity)).ok_or("WORKTREE_NOT_FOUND")?;
            let path = std::path::Path::new(&row.path);
            // Unlike DELETE, metadata can describe an absent checkout. Walk only
            // the derived managed path and reject symlinks before following any
            // component, including dangling links and aliases inside the jail.
            let relative = path.strip_prefix(manager.repo_root()).map_err(|_| "INVALID_WORKTREE")?;
            let mut ancestor = manager.repo_root().to_path_buf();
            let mut missing = false;
            for component in relative.components() {
                if !matches!(component, std::path::Component::Normal(_)) { return Err("INVALID_WORKTREE".into()); }
                ancestor.push(component);
                match std::fs::symlink_metadata(&ancestor) {
                    Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                        if manager.canonical_allowed_path(&ancestor).map_err(domain)? != ancestor { return Err("INVALID_WORKTREE".into()); }
                    }
                    Ok(_) => return Err("INVALID_WORKTREE".into()),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => { missing = true; break; }
                    Err(_) => return Err("INVALID_WORKTREE".into()),
                }
            }
            if missing || row.prunable.is_some() {
                let branch_name = WorktreeManager::format_branch_name(&identity.ws_id, &identity.slug).map_err(domain)?;
                let (branch, branch_error) = match manager.branch_deletion_preview_for_ref(&branch_name) {
                    Ok(branch) => (Some(branch), None),
                    Err(error) => {
                        let code = domain(error);
                        if matches!(code.as_str(), "TIMEOUT" | "UNAUTHORIZED" | "OUTPUT_LIMIT_EXCEEDED") { return Err(code); }
                        (None, Some(code))
                    }
                };
                return Ok(serde_json::json!({"revision":revision, "dirtyInspection":"unavailable", "dirtyFiles":null, "dirtyCount":null,
                    "branchDeletion":branch, "branchDeletionError":branch_error, "locked":row.locked, "prunable":row.prunable, "liveSessionIds":sessions(state, path)?}));
            }
            let dirty = manager.check_dirty(path).map_err(domain)?;
            let branch = manager.branch_deletion_preview(path).map_err(domain)?;
            Ok::<_, String>(serde_json::json!({"revision":revision, "dirtyFiles":dirty.files, "dirtyCount":dirty.files.len(), "branchDeletion":branch, "locked":row.locked, "prunable":row.prunable, "liveSessionIds":sessions(state, path)?}))
        });
        match result {
            Ok(value) => response(200, value),

            Err(code) => failure(&code, &id),
        }
    }).await
}

/// Authenticated local IPC uses the same daemon gate and catalog publication as HTTP.
/// These messages are never automatically resent after ambiguous delivery.
pub(crate) async fn owner_mutation(state: Arc<RemoteGatewayState>, workspace: String, id: crate::worktree::WorktreeIdentity, base: Option<String>, delete: Option<(bool, bool)>) -> Result<crate::daemon::protocol::DaemonResponse, String> {
    let id = WorktreeIdentity { ws_id: id.ws_id, slug: id.slug };
    let deadline = Instant::now() + Duration::from_secs(40);
    let cancelled = CancelWork(Arc::new(AtomicBool::new(false)), Arc::new(tokio::sync::Notify::new()));
    let (grant, revoked) = tokio::sync::watch::channel(false);
    let context = RequestContext { deadline, revoked, cancelled: cancelled.0.clone(), cancellation: cancelled.1.clone() };
    let worker = crate::ipc::run_blocking(move || Ok(crate::worktree::git::with_git_budget(budget(&context), || {
        let _grant = grant;
        let service = &state.machine_services.as_ref().ok_or("MACHINE_SERVICE_UNAVAILABLE")?.workspaces;
        let _permit = service.project_mutations.clone().try_acquire_owned().map_err(|_| "CAPACITY_EXCEEDED")?;
        let gate = service.worktree_gate(&workspace);
        #[cfg(test)]
        probe(&state, "ownerWorktreeQueued");
        let _gate = gate.try_lock_until(deadline).ok_or("TIMEOUT")?;
        context.check()?;
        // Keep domain errors typed for Local callers; machine HTTP projection is separate.
        let typed = |error| crate::daemon::protocol::DaemonResponse::WorktreeError { error: crate::ipc::IpcError::from(error) };
        if let Err(error) = WorkspaceRegistry::validate_workspace_id(&workspace) { return Ok(typed(error)); }
        // Local registry keys and branch namespaces are independent identities.
        // Machine HTTP requests retain their stricter identity() contract.
        if let Err(error) = WorktreeManager::format_branch_name(&id.ws_id, &id.slug) { return Ok(typed(error)); }
        if !service.catalog()?.workspaces.contains_key(&workspace) { return Ok(typed(WorktreeError::WorkspaceNotFound { workspace_id: workspace })); }
        let manager = service.worktree_manager(&workspace, false)?;
        let path = match manager.worktree_path_for(&id.ws_id, &id.slug) { Ok(path) => path, Err(error) => return Ok(typed(error)) };
        if let Some((delete_branch, destructive)) = delete {
            // Preserve the preexisting registry's identity-missing error details.
            // Deletion accepts a jailed prunable record even when its checkout
            // is missing; ordinary lookup deliberately excludes those records.
            match manager.deletion_record(&path) {
                Err(WorktreeError::WorktreeNotFound { .. }) => return Ok(typed(WorktreeError::WorktreeIdentityNotFound { workspace_id: workspace, ws_id: id.ws_id, slug: id.slug })),
                Err(error) => return Ok(typed(error)),
                Ok(_) => {}
            }
            // Porcelain listing retains lock/prunable metadata that inspecting
            // the worktree's HEAD alone does not provide.
            let row = rows(&workspace, &manager)?.into_iter()
                .find(|row| std::path::Path::new(&row.path) == path)
                .ok_or("WORKTREE_NOT_FOUND")?;
            if let Some(reason) = row.locked.clone() { return Ok(typed(WorktreeError::WorktreeLocked { path, reason })); }
            let live_session_ids = sessions(&state, &path)?;
            if !live_session_ids.is_empty() { return Ok(typed(WorktreeError::WorktreeBusy { path, live_session_ids })); }
            let result = manager.delete_worktree_and_branch_with_prune_status(&path, delete_branch, destructive);
            // A branch cleanup error is not a rollback of successful removal.
            if result.is_ok() || !path.exists() { commit_legacy_revision(service, &workspace, true, deadline, &row)?; }
            match result {
                Ok(pruned) => Ok(crate::daemon::protocol::DaemonResponse::DeleteWorktreeOk { pruned }),
                Err(error @ WorktreeError::WorktreeRemovedPruneFailed { .. }) => Ok(typed(error)),
                Err(source) if !path.exists() => Ok(typed(WorktreeError::WorktreeRemovedBranchRetained {
                    path, branch: WorktreeManager::format_branch_name(&id.ws_id, &id.slug).map_err(domain)?, source: Box::new(source),
                })),
                Err(error) => Ok(typed(error)),
            }
        } else {
            let created = match manager.create_worktree(CreateWorktreeOptions { ws_id: id.ws_id, slug: id.slug, path, base_ref: base }) {
                Ok(created) => created,
                Err(error) => return Ok(typed(error)),
            };
            commit_legacy_revision(service, &workspace, false, deadline, &project_worktree(&workspace, &manager, &created)?)?;
            Ok(crate::daemon::protocol::DaemonResponse::CreateWorktreeOk { worktree: created })
        }
    })));
    tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), worker).await
        .map_err(|_| "TIMEOUT".to_owned())?
        .map_err(|_| "MACHINE_SERVICE_UNAVAILABLE".to_owned())?
}

pub async fn legacy(state: Arc<RemoteGatewayState>, headers: HeaderMap, body: Bytes, delete: bool, deadline: Instant) -> Response {
    let cancelled = CancelWork(Arc::new(AtomicBool::new(false)), Arc::new(tokio::sync::Notify::new()));
    let cancel = cancelled.0.clone();
    let cancellation = cancelled.1.clone();
    let result = crate::ipc::run_blocking(move || Ok((|| -> Result<Response, String> {
        let token = headers.get("authorization").and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer ")).ok_or("UNAUTHORIZED")?;
        let device = state.auth_manager.validate_token(token).map_err(|_| "UNAUTHORIZED")?;
        if device.permission != DevicePermission::Control || device.access_scope != DeviceAccessScope::Mirror { return Err("MACHINE_ACCESS_REQUIRED".into()); }
        let services = state.machine_services.as_ref().ok_or("MACHINE_SERVICE_UNAVAILABLE")?;
        let service = &services.workspaces;
        let _permit = service.project_mutations.clone().try_acquire_owned().map_err(|_| "CAPACITY_EXCEEDED")?;
        let revoked = state.auth_manager.device_revocation(&device.id).map_err(|_| "UNAUTHORIZED")?;
        let context = RequestContext { cancelled: cancel, cancellation, deadline, revoked };
        crate::worktree::git::with_git_budget(budget(&context), || {
            let (workspace, identity, base, delete_branch) = if delete {
                let p: super::super::protocol::RemoteDeleteWorktreeRequest = serde_json::from_slice(&body).map_err(|_| "INVALID_REQUEST")?;
                (p.workspace_id, p.worktree, None, p.delete_branch.unwrap_or(false))
            } else {
                let p: super::super::protocol::RemoteCreateWorktreeRequest = serde_json::from_slice(&body).map_err(|_| "INVALID_REQUEST")?;
                (p.workspace_id, p.worktree, p.base_ref, false)
            };
            let gate = service.worktree_gate(&workspace);
            let _gate = gate.try_lock_until(deadline).ok_or("TIMEOUT")?;
            context.check()?;
            let manager = service.worktree_manager(&workspace, true)?;
            let path = manager.worktree_path_for(&identity.ws_id, &identity.slug).map_err(domain)?;
            if delete {
                let canonical = manager.canonical_allowed_path(&path).map_err(domain)?;
                if canonical != path || canonical == manager.repo_root() { return Err("INVALID_WORKTREE".into()); }
                if !sessions(&state, &path)?.is_empty() { return Err("WORKTREE_BUSY".into()); }
                let row = crate::worktree::git_worktree_list(manager.repo_root()).map_err(domain)?.into_iter().find(|row| row.path == path).ok_or("WORKTREE_NOT_FOUND")?;
                let event_row = project_worktree(&workspace, &manager, &row)?;
                if row.locked.is_some() { return Err("WORKTREE_LOCKED".into()); }
                let removed = manager.delete_worktree_and_branch(&path, delete_branch);
                if let Err(error) = removed {
                    if path.exists() { return Err(domain(error)); }
                    commit_legacy_revision(service, &workspace, true, deadline, &event_row)?;
                    let prune_failed = matches!(error, WorktreeError::WorktreeRemovedPruneFailed { .. });
                    let mut partial = super::error(if prune_failed { "WORKTREE_REMOVED_PRUNE_FAILED" } else { "WORKTREE_REMOVED_BRANCH_RETAINED" }, &uuid::Uuid::new_v4().to_string());
                    if prune_failed { partial.details.insert("pruned".into(), serde_json::json!(false)); }
                    partial.details.insert("worktreeRemoved".into(), serde_json::json!(true));
                    partial.details.insert("branchDeleted".into(), serde_json::json!(false));
                    return Ok(response(409, ErrorEnvelope { error: partial }));
                }
                commit_legacy_revision(service, &workspace, true, deadline, &event_row)?;
                Ok((StatusCode::NO_CONTENT, [("cache-control", "no-store")]).into_response())
            } else {
                let created = manager.create_worktree(CreateWorktreeOptions { ws_id: identity.ws_id, slug: identity.slug, path, base_ref: base }).map_err(domain)?;
                commit_legacy_revision(service, &workspace, false, deadline, &project_worktree(&workspace, &manager, &created)?)?;
                Ok(response(200, super::super::protocol::RemoteWorktreeInfo { worktree_slug: created.orca_info().map(|info| info.slug), worktree_label: created.branch_short_name().map(str::to_owned), attention: None }))
            }
        })
    })()));
    match tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), result).await {
        Ok(Ok(Ok(response))) => response,
        Ok(Ok(Err(code))) => failure(&code, &uuid::Uuid::new_v4().to_string()),
        Ok(Err(_)) => failure("MACHINE_SERVICE_UNAVAILABLE", &uuid::Uuid::new_v4().to_string()),
        Err(_) => failure("TIMEOUT", &uuid::Uuid::new_v4().to_string()),
    }
}
#[cfg(test)]
mod publication_tests {
    use super::*;
    #[tokio::test]
    async fn cancelled_publication_preserves_unknown_after_git() {
        for revoke in [false, true] {
            let (root, server, workspace) = tokio::task::spawn_blocking(|| {
                let root = tempfile::tempdir().unwrap();
                let repo = root.path().join("repo");
                std::fs::create_dir(&repo).unwrap();
                crate::worktree::run_git(&repo, &["init", "--quiet"]).unwrap();
                crate::worktree::run_git(&repo, &["-c", "user.name=A08", "-c", "user.email=a08@example.invalid", "commit", "--allow-empty", "-m", "base"]).unwrap();
                let server = crate::daemon::server::DaemonServer::new_with_paths(Some(root.path().join("config")), Some(root.path().join("auth")));
                let workspace = server.remote_state().machine_services.as_ref().unwrap().workspaces.register_machine(repo.to_str().unwrap()).unwrap();
                (root, server, workspace)
            }).await.unwrap();
            let state = server.remote_state().clone();
            let service = &state.machine_services.as_ref().unwrap().workspaces;
            let pin = state.auth_manager.create_scoped_pairing_code(super::super::super::auth::DevicePermission::Control, super::super::super::auth::DeviceAccessScope::Machine).unwrap();
            let (token, device) = state.auth_manager.exchange_pairing_code(&pin, "publication").unwrap();
            let mut headers = HeaderMap::new();
            headers.insert("authorization", format!("Bearer {token}").parse().unwrap());
            let revision = service.catalog().unwrap().revision;
            let gate = service.mutation_gate.lock();
            let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
            let entered_tx = std::sync::Mutex::new(Some(entered_tx));
            *service.transaction_probe.write() = Some(Arc::new(move |phase| {
                if phase == "worktreePublication" { entered_tx.lock().unwrap().take().unwrap().send(()).unwrap(); }
            }));
            let request = uuid::Uuid::new_v4().to_string();
            let payload = Bytes::from(serde_json::json!({"requestId":request,"workspaceId":workspace,"worktree":{"wsId":workspace,"slug":"fenced"}}).to_string());
            let task = tokio::spawn(mutate_worktree(state.clone(), headers.clone(), payload.clone(), false));
            let entered = tokio::time::timeout(Duration::from_secs(10), entered_rx).await;
            if revoke { assert!(state.auth_manager.revoke_device(&device.id)); } else { task.abort(); }
            let response = task.await;
            drop(gate);
            let slots = tokio::time::timeout(Duration::from_secs(10), service.project_mutations.clone().acquire_many_owned(8)).await.unwrap().unwrap();
            *service.transaction_probe.write() = None;
            let current = service.catalog().unwrap().revision;
            let record = service.journal.reconcile(&device.id, &request).unwrap().unwrap();
            let manager = service.worktree_manager(&workspace, false).unwrap();
            let target = manager.worktree_path_for(&workspace, "fenced").unwrap();
            let present = tokio::task::spawn_blocking(move || target.exists()).await.unwrap();
            drop(slots);
            // An identical request must replay unknown, not repeat completed Git.
            if !revoke {
                let replay = mutate_worktree(state.clone(), headers, payload, false).await;
                assert_eq!(replay.status().as_u16(), 409);
            }
            drop(state); drop(server);
            tokio::task::spawn_blocking(move || root.close().unwrap()).await.unwrap();
            assert!(entered.unwrap().is_ok());
            if revoke { assert_eq!(response.unwrap().status().as_u16(), 401); } else { assert!(response.unwrap_err().is_cancelled()); }
            assert_eq!(current, revision);
            assert!(present);
            assert!(matches!(record.operation, Operation::OutcomeUnknown { .. }));
            eprintln!("A08 publication revoke={revoke} git_target_exists=true catalog_unchanged=true outcome_unknown=true workers_drained=true private_root_removed=true");
        }
    }
}
pub async fn mutate_worktree(state: Arc<RemoteGatewayState>, headers: HeaderMap, body: Bytes, delete: bool) -> Response {
    let fallback = uuid::Uuid::new_v4().to_string();
    execute(state, headers, true, fallback.clone(), move |state, headers, context| {
        let mut request_id = fallback.clone();
        let result = crate::worktree::git::with_git_budget(budget(context), || {
            let device = authorize(state, headers)?;
            let payload: Mutation = serde_json::from_slice(&body).map_err(|_| "INVALID_REQUEST")?;
            uuid::Uuid::parse_str(&payload.request_id).map_err(|_| "INVALID_REQUEST")?;
            request_id = payload.request_id.clone();
            identity(&payload.workspace_id, &payload.worktree)?;
            if delete && (payload.expected_revision.is_none() || payload.delete_branch.is_none()) { return Err("INVALID_REQUEST".into()); }
            let kind = if delete { "removeWorktree" } else { "createWorktree" };
            let digest = format!("{:x}", Sha256::digest(serde_json::to_vec(&(kind, &payload.workspace_id, &payload.worktree, &payload.base_ref, payload.delete_branch, payload.expected_revision)).expect("digest")));
            let service = &state.machine_services.as_ref().expect("services").workspaces;
            if let Some(record) = service.journal.reconcile(&device, &request_id)? {
                if record.digest != digest || record.kind != kind { return Err("REQUEST_CONFLICT".into()); }
                return Ok(record);
            }
            let gate = service.worktree_gate(&payload.workspace_id);
            #[cfg(test)]
            probe(state, "worktreeQueued");
            let _worktree_gate = gate.try_lock_until(context.deadline).ok_or("TIMEOUT")?;
            // Local spawn and unregister acquire this same workspace fence.
            // The global publication gate is not held during Git execution.
            context.check()?; authorize(state, headers)?;
            let manager = service.worktree_manager(&payload.workspace_id, false)?;
            if delete { observed_revision(service, &payload.workspace_id, &manager, context)?; }
            let path = manager.worktree_path_for(&payload.workspace_id, &payload.worktree.slug).map_err(domain)?;
            let resource = serde_json::to_string(&(&payload.workspace_id, &payload.worktree, &path)).expect("identity");
            if let Begin::Existing(record) = service.journal.begin(&device, &request_id, kind, &digest, &resource)? { return Ok(record); }
            let mut side_effect_started = false;
            let outcome = (|| {
                let catalog = service.catalog()?;
                let event_row;
                let outcome = if delete {
                    if payload.expected_revision != Some(catalog.revision) { return Err("STALE_REVISION".into()); }
                    let row = target(&payload.workspace_id, &payload.worktree, &manager)?;
                    event_row = row.clone();
                    if row.locked.is_some() { return Err("WORKTREE_LOCKED".into()); }
                    if !sessions(state, &path)?.is_empty() { return Err("WORKTREE_BUSY".into()); }
                    let dirty = manager.check_dirty(&path).map_err(domain)?;
                    if dirty.is_dirty { return Err("DIRTY_WORKTREE".into()); }
                    if payload.delete_branch == Some(true) && !manager.branch_deletion_preview(&path).map_err(domain)?.merged { return Err("UNMERGED_BRANCH".into()); }
                    context.check()?; authorize(state, headers)?;
                    side_effect_started = true;
                    let removal = manager.safe_delete(&path);
                    if matches!(removal, Err(WorktreeError::WorktreeRemovedPruneFailed { .. })) {
                        let mut partial = error("WORKTREE_REMOVED_PRUNE_FAILED", &request_id);
                        partial.details.insert("worktreeRemoved".into(), serde_json::json!(true));
                        partial.details.insert("pruned".into(), serde_json::json!(false));
                        partial.details.insert("branchDeleted".into(), serde_json::json!(false));
                        OperationOutcome::Error { error: partial }
                    } else {
                    removal.map_err(domain)?;
                    if payload.delete_branch == Some(true) {
                        let branch = WorktreeManager::format_branch_name(&payload.workspace_id, &payload.worktree.slug).map_err(domain)?;
                        if crate::worktree::git_branch_delete(manager.repo_root(), &branch, false).is_err() {
                            let mut partial = error("WORKTREE_REMOVED_BRANCH_RETAINED", &request_id);
                            partial.details.insert("worktreeRemoved".into(), serde_json::json!(true));
                            partial.details.insert("branchDeleted".into(), serde_json::json!(false));
                            partial.details.insert("branch".into(), serde_json::json!(branch));
                            OperationOutcome::Error { error: partial }
                        } else { OperationOutcome::NoContent }
                    } else { OperationOutcome::NoContent }
                    }
                } else {
                    if !manager.is_git_backed() { return Err("NOT_A_GIT_REPOSITORY".into()); }
                    if path.exists() { return Err("WORKTREE_EXISTS".into()); }
                    let base = payload.base_ref.as_deref().unwrap_or("HEAD");
                    if base.is_empty() || base.starts_with('-') || base.chars().any(char::is_control) { return Err("INVALID_BASE_REF".into()); }
                    let commit = crate::worktree::run_git(manager.repo_root(), &["rev-parse", "--verify", "--end-of-options", &format!("{base}^{{commit}}")]).map_err(|error| match error { WorktreeError::GitError { .. } => "BASE_REF_UNAVAILABLE".into(), error => domain(error) })?;
                    context.check()?; authorize(state, headers)?;
                    side_effect_started = true;
                    manager.create_worktree(CreateWorktreeOptions { ws_id: payload.workspace_id.clone(), slug: payload.worktree.slug.clone(), path: path.clone(), base_ref: Some(commit.trim().into()) }).map_err(domain)?;
                    event_row = target(&payload.workspace_id, &payload.worktree, &manager)?;
                    OperationOutcome::Worktree { worktree: event_row.clone() }
                };
                let observation = observation_digest(&payload.workspace_id, &manager)?;
                #[cfg(test)]
                probe(state, "worktreePublication");
                let _publication = service.mutation_gate.try_lock_until(context.deadline).ok_or("TIMEOUT")?;
                context.check()?; authorize(state, headers)?;
                // Other workspaces may have committed while Git was running.
                let mut catalog = service.catalog()?;
                catalog.revision.0 = catalog.revision.0.checked_add(1).ok_or("CAPACITY_EXCEEDED")?;
                catalog.worktree_observations.insert(payload.workspace_id.clone(), observation);
                let status = match outcome { OperationOutcome::Error { .. } => 409, OperationOutcome::NoContent => 204, _ => 201 };
                catalog.transaction = Some(service.journal.catalog_receipt(&device, &request_id, status, outcome.clone())?);
                if super::super::workspace_catalog::persist(&service.catalog_path, &catalog).is_err() {
                    *service.catalog.lock() = Err("MACHINE_SERVICE_UNAVAILABLE".into());
                    return Err("OPERATION_OUTCOME_UNKNOWN".into());
                }
                let revision = catalog.revision;
                *service.catalog.lock() = Ok(catalog);
                service.worktree_committed(&payload.workspace_id, delete, revision, &event_row);
                Ok::<_, String>((status, outcome))
            })();
            let (status, outcome) = match outcome {
                Ok(value) => value,
                Err(code) if code == "OPERATION_OUTCOME_UNKNOWN" || side_effect_started => {
                    service.journal.mark_unknown(&device, &request_id)?;
                    return Err("OPERATION_OUTCOME_UNKNOWN".into());
                }
                Err(code) => (status(&code), OperationOutcome::Error { error: error(&code, &request_id) }),
            };
            #[cfg(test)]
            probe(state, "worktreeJournalCompletion");
            service.journal.complete(&device, &request_id, status, outcome)
        });
        match result { Ok(record) => replay(record), Err(code) => failure(&code, &request_id) }
    }).await
}
