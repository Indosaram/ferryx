use super::*;
use futures_util::FutureExt;
use crate::daemon::{server::DaemonServer, protocol::{DaemonRequest, DaemonResponse}};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

#[tokio::test]
async fn followthrough_prune_partial_replays_after_restart() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("data/config");
    let auth = root.path().join("data/auth");
    let (owner, workspace, token) = tokio::task::spawn_blocking({
        let repo = root.path().join("repo"); let config = config.clone(); let auth = auth.clone();
        move || {
            std::fs::create_dir(&repo).unwrap();
            crate::worktree::run_git(&repo, &["init", "--quiet"]).unwrap();
            crate::worktree::run_git(&repo, &["-c", "user.name=A08", "-c", "user.email=a08@example.invalid", "commit", "--allow-empty", "-m", "base"]).unwrap();
            let owner = DaemonServer::new_with_paths(Some(config), Some(auth));
            let state = owner.remote_state();
            let workspace = state.machine_services.as_ref().unwrap().workspaces.register_machine(repo.to_str().unwrap()).unwrap();
            let pin = state.auth_manager.create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine).unwrap();
            let token = state.auth_manager.exchange_pairing_code(&pin, "prune").unwrap().0;
            (owner, workspace, token)
        }
    }).await.unwrap();
    let result = std::panic::AssertUnwindSafe(async {
        let mut headers = HeaderMap::new(); headers.insert("authorization", format!("Bearer {token}").parse().unwrap());
        let state = owner.remote_state().clone();
        let created = mutate_worktree(state.clone(), headers.clone(), Bytes::from(serde_json::json!({"requestId":uuid::Uuid::new_v4().to_string(),"workspaceId":workspace,"worktree":{"wsId":workspace,"slug":"partial-prune"}}).to_string()), false).await;
        assert_eq!(created.status().as_u16(), 201);
        let service = &state.machine_services.as_ref().unwrap().workspaces;
        *service.transaction_probe.write() = Some(Arc::new(|phase| if phase == "worktreeQueued" {
            crate::worktree::manager::PRUNE_PROBE.with(|probe| *probe.borrow_mut() = Some(Box::new(|repo, before| {
                // Real Git exits 128 only for prune; restore before observation/publication.
                let config = repo.join(".git/config");
                if before {
                    let original = std::fs::read(&config).unwrap();
                    std::fs::write(repo.join("prune-config-backup"), &original).unwrap();
                    let text = String::from_utf8(original).unwrap().replace("repositoryformatversion = 0", "repositoryformatversion = 999");
                    std::fs::write(config, text).unwrap();
                } else {
                    let backup = repo.join("prune-config-backup");
                    std::fs::write(config, std::fs::read(&backup).unwrap()).unwrap();
                    std::fs::remove_file(backup).unwrap();
                }
            })));
        }));
        let request = uuid::Uuid::new_v4().to_string();
        let body = Bytes::from(serde_json::json!({"requestId":request,"workspaceId":workspace,"worktree":{"wsId":workspace,"slug":"partial-prune"},"deleteBranch":true,"expectedRevision":service.catalog().unwrap().revision}).to_string());
        let removed = mutate_worktree(state.clone(), headers.clone(), body.clone(), true).await;
        *service.transaction_probe.write() = None;
        let status = removed.status().as_u16();
        let bytes = axum::body::to_bytes(removed.into_body(), 65536).await.unwrap();
        assert_eq!(status, 409, "partial prune must not acknowledge 204: {}", String::from_utf8_lossy(&bytes));
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["error"]["code"], "WORKTREE_REMOVED_PRUNE_FAILED");
        assert_eq!(value["error"]["details"]["worktreeRemoved"], true);
        assert_eq!(value["error"]["details"]["pruned"], false);
        for restart in [false, true] {
            let restarted = if restart { Some(tokio::task::spawn_blocking({ let config = config.clone(); let auth = auth.clone(); move || DaemonServer::new_with_paths(Some(config), Some(auth)) }).await.unwrap()) } else { None };
            let replay_state = restarted.as_ref().map(|o| o.remote_state().clone()).unwrap_or_else(|| state.clone());
            let replay = mutate_worktree(replay_state, headers.clone(), body.clone(), true).await;
            assert_eq!(replay.status().as_u16(), 409);
            assert_eq!(axum::body::to_bytes(replay.into_body(), 65536).await.unwrap(), bytes);
        }
        eprintln!("A08 followthrough prune request={request} workspace={workspace} original_result_replayed=true restart=true");
    }).catch_unwind().await;
    drop(owner);
    tokio::task::spawn_blocking(move || root.close().unwrap()).await.unwrap();
    eprintln!("A08 followthrough prune workers_awaited=true private_root_removed=true");
    result.unwrap();
}

#[tokio::test]
async fn followthrough_write_failures_non_head_and_prunable_preview() {
    for journal_failure in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("data/config"); let auth = root.path().join("data/auth");
        let (owner, workspace, token, base) = tokio::task::spawn_blocking({
            let repo = root.path().join("repo"); let config = config.clone(); let auth = auth.clone();
            move || {
                std::fs::create_dir(&repo).unwrap();
                crate::worktree::run_git(&repo, &["init", "--quiet"]).unwrap();
                for message in ["base", "head"] {
                    crate::worktree::run_git(&repo, &["-c", "user.name=A08", "-c", "user.email=a08@example.invalid", "commit", "--allow-empty", "-m", message]).unwrap();
                }
                let base = crate::worktree::run_git(&repo, &["rev-parse", "HEAD~1"]).unwrap().trim().to_owned();
                let owner = DaemonServer::new_with_paths(Some(config), Some(auth));
                let state = owner.remote_state();
                let workspace = state.machine_services.as_ref().unwrap().workspaces.register_machine(repo.to_str().unwrap()).unwrap();
                let pin = state.auth_manager.create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine).unwrap();
                let token = state.auth_manager.exchange_pairing_code(&pin, "write-failure").unwrap().0;
                (owner, workspace, token, base)
            }
        }).await.unwrap();
        let result = std::panic::AssertUnwindSafe(async {
            let mut headers = HeaderMap::new(); headers.insert("authorization", format!("Bearer {token}").parse().unwrap());
            let state = owner.remote_state().clone(); let service = &state.machine_services.as_ref().unwrap().workspaces;
            // Given: a subscribed publication observer and the durable baseline.
            let baseline = crate::remote::workspace_catalog::load(&service.catalog_path).unwrap();
            let failure_path = if journal_failure { service.catalog_path.with_file_name("machine-operations.v1.json") } else { service.catalog_path.clone() };
            let backup = failure_path.with_extension("saved");
            let injected_path = failure_path.clone(); let injected_backup = backup.clone();
            // Receipt acquisition refreshes the journal; obstruct only completion
            // for the post-catalog failure, not that earlier admission read.
            let injection_phase = if journal_failure { "worktreeJournalCompletion" } else { "worktreePublication" };
            *service.transaction_probe.write() = Some(Arc::new(move |phase| if phase == injection_phase {
                std::fs::rename(&injected_path, &injected_backup).unwrap();
                std::fs::create_dir(&injected_path).unwrap();
            }));
            let request = uuid::Uuid::new_v4().to_string();
            let body = Bytes::from(serde_json::json!({"requestId":request,"workspaceId":workspace,"worktree":{"wsId":workspace,"slug":"write-failure"},"baseRef":"HEAD~1"}).to_string());
            let mut events = service.subscribe_worktree_changes();
            // When: real Git succeeds but one durable publication store fails.
            let response = mutate_worktree(state.clone(), headers.clone(), body.clone(), false).await;
            *service.transaction_probe.write() = None;
            let status = response.status().as_u16();
            let bytes = axum::body::to_bytes(response.into_body(), 65536).await.unwrap();
            let (head, path, durable) = tokio::task::spawn_blocking({ let service = service.clone(); let workspace = workspace.clone(); move || {
                // Catalog may be fenced: inspect the actual private Git target directly.
                let catalog = crate::remote::workspace_catalog::load(if journal_failure { &service.catalog_path } else { &backup }).unwrap();
                let repo = &catalog.workspaces[&workspace].repo_root;
                let path = repo.join(".orca-worktrees").join(&workspace).join("write-failure");
                let head = crate::worktree::run_git(&path, &["rev-parse", "HEAD"]).unwrap().trim().to_owned();
                std::fs::remove_dir(&failure_path).unwrap();
                std::fs::rename(backup, failure_path).unwrap();
                (head, path, catalog)
            }}).await.unwrap();
            // Then: disk publication, event identity and restart replay agree.
            eprintln!("A08 publication journal_failure={journal_failure} baseline={} durable={} receipt={:?}", baseline.revision.0, durable.revision.0, durable.transaction.as_ref().map(|receipt| &receipt.request_id));
            assert_eq!(head, base, "successful explicit non-HEAD Git side effect");
            assert_eq!(status, if journal_failure {503} else {409}, "{}", String::from_utf8_lossy(&bytes));
            let committed = events.try_recv();
            assert_eq!(committed.is_ok(), journal_failure, "only durable catalog publication emits an event");
            if let Ok(event) = committed {
                assert_eq!(event.workspace_id, workspace);
                assert_eq!(event.revision, durable.revision);
                assert!(!event.removed);
            }
            if journal_failure {
                assert_eq!(durable.revision.0, baseline.revision.0 + 1);
                let receipt = durable.transaction.as_ref().unwrap();
                assert_eq!(receipt.request_id, request);
                assert_eq!(receipt.status, 201);
                assert!(matches!(&receipt.operation, crate::remote::machine_protocol::Operation::Completed { outcome: crate::remote::machine_protocol::OperationOutcome::Worktree { worktree }, .. } if worktree.head == base));
            } else {
                assert_eq!(durable.revision, baseline.revision);
                assert_eq!(durable.worktree_observations, baseline.worktree_observations);
                assert!(durable.transaction.is_none());
            }
            assert!(matches!(events.try_recv(), Err(tokio::sync::broadcast::error::TryRecvError::Empty)));
            let restarted = tokio::task::spawn_blocking({ let config = config.clone(); let auth = auth.clone(); move || DaemonServer::new_with_paths(Some(config), Some(auth)) }).await.unwrap();
            let restarted_state = restarted.remote_state().clone();
            let replay_service = &restarted_state.machine_services.as_ref().unwrap().workspaces;
            let mut replay_events = replay_service.subscribe_worktree_changes();
            let replay_revision = replay_service.catalog().unwrap().revision;
            let replay = mutate_worktree(restarted_state.clone(), headers.clone(), body, false).await;
            assert_eq!(replay.status().as_u16(), if journal_failure {201} else {409});
            let replay: serde_json::Value = serde_json::from_slice(&axum::body::to_bytes(replay.into_body(), 65536).await.unwrap()).unwrap();
            if journal_failure { assert_eq!(replay["head"], base); } else { assert_eq!(replay["error"]["code"], "OPERATION_OUTCOME_UNKNOWN"); }
            assert_eq!(replay_service.catalog().unwrap().revision, replay_revision);
            assert!(matches!(replay_events.try_recv(), Err(tokio::sync::broadcast::error::TryRecvError::Empty)));
            tokio::task::spawn_blocking(move || std::fs::remove_dir_all(path).unwrap()).await.unwrap();
            let preview = read(restarted_state, headers, Some(format!("workspaceId={workspace}&wsId={workspace}&slug=write-failure")), true).await;
            assert_eq!(preview.status().as_u16(), 200, "prunable preview must explicitly report unavailable dirty inspection");
            let preview: serde_json::Value = serde_json::from_slice(&axum::body::to_bytes(preview.into_body(), 65536).await.unwrap()).unwrap();
            assert_eq!(preview["dirtyInspection"], "unavailable");
            assert!(preview.get("dirtyFiles").unwrap().is_null());
            assert!(preview.get("dirtyCount").unwrap().is_null());
            assert_eq!(preview["branchDeletion"]["head"], base);
            assert_eq!(preview["branchDeletion"]["merged"], true);
            assert!(preview["prunable"].is_string());
            assert!(preview["revision"].is_string());
            eprintln!("A08 followthrough journal_failure={journal_failure} request={request} workspace={workspace} non_head={base} restart_no_repeat=true prunable_preview=200 dirty_inspection=unavailable");
        }).catch_unwind().await;
        drop(owner);
        tokio::task::spawn_blocking(move || root.close().unwrap()).await.unwrap();
        eprintln!("A08 followthrough write failure private_root_removed=true");
        result.unwrap();
    }
}

#[tokio::test]
async fn followthrough_delete_publication_blocks_spawn() {
    let root = tempfile::tempdir().unwrap();
    let owner = tokio::task::spawn_blocking({ let root = root.path().to_owned(); move || {
        let repo = root.join("repo"); std::fs::create_dir(&repo).unwrap();
        crate::worktree::run_git(&repo, &["init", "--quiet"]).unwrap();
        crate::worktree::run_git(&repo, &["-c", "user.name=A08", "-c", "user.email=a08@example.invalid", "commit", "--allow-empty", "-m", "base"]).unwrap();
        let owner = Arc::new(DaemonServer::new_with_paths(Some(root.join("data/config")), Some(root.join("data/auth"))));
        let service = &owner.remote_state().machine_services.as_ref().unwrap().workspaces;
        service.register("barrier", repo.to_str().unwrap()).unwrap();
        let manager = service.worktree_manager("barrier", false).unwrap();
        manager.create_worktree(CreateWorktreeOptions::new("barrier", "target", manager.worktree_path_for("barrier", "target").unwrap())).unwrap();
        owner
    }}).await.unwrap();
    let state = owner.remote_state().clone(); let service = state.machine_services.as_ref().unwrap().workspaces.clone();

    let pin = state.auth_manager.create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine).unwrap();
    let token = state.auth_manager.exchange_pairing_code(&pin, "barrier").unwrap().0;
    let mut headers = HeaderMap::new(); headers.insert("authorization", format!("Bearer {token}").parse().unwrap());
    // Establish the observation before the deletion publication barrier.
    let preview = read(state.clone(), headers.clone(), Some("workspaceId=barrier&wsId=barrier&slug=target".into()), true).await;
    assert_eq!(preview.status().as_u16(), 200);
    let (published, mut publication_rx) = tokio::sync::mpsc::unbounded_channel();
    let (release, released) = std::sync::mpsc::channel();
    let released = std::sync::Mutex::new(released);
    *service.transaction_probe.write() = Some(Arc::new(move |phase| {
        let _ = published.send(phase.to_owned());
        if phase == "worktreePublication" { released.lock().unwrap().recv_timeout(Duration::from_secs(15)).unwrap(); }
    }));
    let request = uuid::Uuid::new_v4().to_string();
    let body = Bytes::from(serde_json::json!({"requestId":request,"workspaceId":"barrier","worktree":{"wsId":"barrier","slug":"target"},"deleteBranch":false,"expectedRevision":service.catalog().unwrap().revision}).to_string());
    let delete = tokio::spawn(mutate_worktree(state.clone(), headers, body, true));
    let reached = tokio::time::timeout(Duration::from_secs(10), async { while publication_rx.recv().await.as_deref() != Some("worktreePublication") {} }).await;
    let spawn_owner = owner.clone();
    let spawn = tokio::spawn(async move { ipc(spawn_owner, DaemonRequest::Spawn { client_request_id: uuid::Uuid::new_v4().to_string(), workspace_id: "barrier".into(), worktree: Some(crate::worktree::WorktreeIdentity { ws_id: "barrier".into(), slug: "target".into() }), cwd: None, cols:80, rows:24, shell:None, startup:None }).await });
    let queued = tokio::time::timeout(Duration::from_secs(10), async { while publication_rx.recv().await.as_deref() != Some("workspaceGateRequested") {} }).await;
    let blocked = !spawn.is_finished();
    let _ = release.send(());
    let deletion = delete.await; let spawned = spawn.await;
    *service.transaction_probe.write() = None;
    // A regression creating a shell is still explicitly torn down.
    if let Ok(DaemonResponse::SpawnOk { session_id, .. }) = &spawned { owner.terminal_service().close_session(session_id).await.unwrap(); }

    drop(service); drop(state); drop(owner);
    tokio::task::spawn_blocking(move || root.close().unwrap()).await.unwrap();
    assert!(reached.is_ok() && queued.is_ok() && blocked);
    assert_eq!(deletion.unwrap().status().as_u16(), 204);
    assert!(matches!(spawned.unwrap(), DaemonResponse::Error { .. }));
    eprintln!("A08 delete-vs-spawn request={request} spawn_observed_at_shared_gate=true blocked_until_delete_publication=true spawn_refused=true workers_joined=true root_removed=true");
}

async fn ipc(owner: Arc<DaemonServer>, request: DaemonRequest) -> DaemonResponse {
    let (mut client, stream) = tokio::io::duplex(65536);
    let worker = tokio::spawn(owner.handle_client(stream));
    client.write_all(format!("{}\n", serde_json::to_string(&request).unwrap()).as_bytes()).await.unwrap();
    let mut reader = tokio::io::BufReader::new(client);
    let mut line = String::new();
    let result = tokio::time::timeout(Duration::from_secs(15), reader.read_line(&mut line)).await;
    drop(reader);
    worker.await.unwrap();
    result.unwrap().unwrap();
    serde_json::from_str(&line).unwrap()
}

#[tokio::test]
async fn local_worktree_errors_survive_owner_wire_and_native_adapter() {
    let (root, owner, path) = tokio::task::spawn_blocking(|| {
        let root = tempfile::tempdir().unwrap(); let repo = root.path().join("repo"); std::fs::create_dir(&repo).unwrap();
        crate::worktree::run_git(&repo, &["init", "--quiet"]).unwrap();
        crate::worktree::run_git(&repo, &["-c", "user.name=A08", "-c", "user.email=a08@example.invalid", "commit", "--allow-empty", "-m", "base"]).unwrap();
        let owner = Arc::new(DaemonServer::new_with_paths(Some(root.path().join("data/config")), Some(root.path().join("data/auth"))));
        let service = &owner.remote_state().machine_services.as_ref().unwrap().workspaces;
        service.register("local", repo.to_str().unwrap()).unwrap();
        let manager = service.worktree_manager("local", false).unwrap();
        let path = manager.worktree_path_for("local", "dirty").unwrap();
        manager.create_worktree(CreateWorktreeOptions::new("local", "dirty", path.clone())).unwrap();
        std::fs::write(path.join("dirty.txt"), "dirty").unwrap();
        (root, owner, path)
    }).await.unwrap();
    let response = ipc(owner.clone(), DaemonRequest::DeleteWorktree { workspace_id: "local".into(), worktree: crate::worktree::WorktreeIdentity { ws_id: "local".into(), slug: "dirty".into() }, delete_branch: false, destructive: false }).await;
    let actual = crate::ipc::worktree::worktree_response(response).unwrap_err();
    let expected = crate::ipc::IpcError::from(WorktreeError::DirtyWorktree { path: path.clone(), count: 1, files: vec!["dirty.txt".into()] });
    let missing = ipc(owner.clone(), DaemonRequest::DeleteWorktree { workspace_id: "local".into(), worktree: crate::worktree::WorktreeIdentity { ws_id: "local".into(), slug: "missing".into() }, delete_branch: false, destructive: false }).await;
    let missing = crate::ipc::worktree::worktree_response(missing).unwrap_err();
    let missing_expected = crate::ipc::IpcError::from(WorktreeError::WorktreeIdentityNotFound { workspace_id: "local".into(), ws_id: "local".into(), slug: "missing".into() });
    let preserved = tokio::task::spawn_blocking(move || path.join("dirty.txt").is_file()).await.unwrap();
    drop(owner);
    tokio::task::spawn_blocking(move || root.close().unwrap()).await.unwrap();
    assert!(preserved);
    assert_eq!(actual, expected);
    assert_eq!(missing, missing_expected);
    eprintln!("A08 typed dirty delete: serialized owner -> native adapter exact code/message/path/count/files; target preserved; worker joined/root removed");
}

#[cfg(unix)]
#[tokio::test]
async fn typed_owner_repair_private_uds_and_native_adapter() {
    use crate::daemon::client::DaemonClient;
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::Builder::new().prefix("a08-typed-owner-").tempdir_in("/tmp").unwrap();
    let (owner, repo) = tokio::task::spawn_blocking({ let root = root.path().to_owned(); move || {
        let repo = root.join("repo"); std::fs::create_dir(&repo).unwrap();
        crate::worktree::run_git(&repo, &["init", "--quiet"]).unwrap();
        crate::worktree::run_git(&repo, &["-c", "user.name=A08", "-c", "user.email=a08@example.invalid", "commit", "--allow-empty", "-m", "base"]).unwrap();
        let owner = Arc::new(DaemonServer::new_with_paths(Some(root.join("data/config")), Some(root.join("data/auth"))));
        owner.remote_state().machine_services.as_ref().unwrap().workspaces.register("typed", repo.to_str().unwrap()).unwrap();
        (owner, std::fs::canonicalize(repo).unwrap())
    }}).await.unwrap();
    let socket = root.path().join("owner.sock");
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let server = owner.clone();
    let listener_task = tokio::spawn(async move {
        let mut clients = tokio::task::JoinSet::new();
        loop { let (stream, _) = listener.accept().await.unwrap(); clients.spawn(server.clone().handle_client(stream)); }
    });
    let client = DaemonClient::new_with_socket(socket.clone());
    let mut session = None;
    let mut pid = None;
    let outcome = std::panic::AssertUnwindSafe(async {
        let mut results = Vec::new();
        for slug in ["locked", "partial", "busy"] {
            let identity = crate::worktree::WorktreeIdentity { ws_id: "typed".into(), slug: slug.into() };
            let created = tokio::time::timeout(Duration::from_secs(40), client.send_request(DaemonRequest::CreateWorktree { workspace_id: "typed".into(), worktree: identity.clone(), base_ref: None })).await.unwrap().unwrap();
            assert!(matches!(created, DaemonResponse::CreateWorktreeOk { .. }));
            let path = repo.join(".orca-worktrees/typed").join(slug);
            let branch = format!("orca/typed/{slug}");
            if slug == "busy" {
                let spawned = client.send_request(DaemonRequest::Spawn { client_request_id: uuid::Uuid::new_v4().to_string(), workspace_id: "typed".into(), worktree: Some(identity.clone()), cwd: None, cols:80, rows:24, shell:None, startup:None }).await.unwrap();
                let DaemonResponse::SpawnOk { session_id, .. } = spawned else { panic!("{spawned:?}") };
                session = Some(session_id.clone());
                let (_, mut output) = owner.terminal_service().attach(&session_id).unwrap();
                owner.terminal_service().write_input(&session_id, b"printf '\\10108_TYPED pid=%s cwd=%s\\n' \"$$\" \"$PWD\"\n").unwrap();
                let marker = tokio::time::timeout(Duration::from_secs(10), async {
                    let mut bytes = Vec::new();
                    loop {
                        bytes.extend(output.recv().await.unwrap());
                        let text = String::from_utf8_lossy(&bytes);
                        if text.contains("A08_TYPED pid=") && text.contains(&format!("cwd={}\r", path.display())) {
                            break text.split("A08_TYPED pid=").nth(1).unwrap().lines().next().unwrap().to_owned();
                        }
                    }
                }).await.unwrap();
                pid = Some(marker.split_whitespace().next().unwrap().parse::<libc::pid_t>().unwrap());
                eprintln!("A08_TYPED_ORIGINAL pid={marker} session={session_id}");
            } else {
                tokio::task::spawn_blocking({ let repo = repo.clone(); let path = path.clone(); let branch = branch.clone(); move || {
                    if slug == "locked" { crate::worktree::run_git(&repo, &["worktree", "lock", "--reason", "typed fixture lock", path.to_str().unwrap()]).unwrap(); }
                    else { std::fs::write(repo.join(".git/refs/heads").join(format!("{branch}.lock")), "owned lock").unwrap(); }
                }}).await.unwrap();
            }
            let service = &owner.remote_state().machine_services.as_ref().unwrap().workspaces;
            let before = service.catalog().unwrap().revision;
            let mut events = service.subscribe_worktree_changes();
            let response = tokio::time::timeout(Duration::from_secs(40), client.send_request(DaemonRequest::DeleteWorktree { workspace_id: "typed".into(), worktree: identity, delete_branch:true, destructive:true })).await.unwrap().unwrap();
            let wire = serde_json::to_value(&response).unwrap();
            let native = crate::ipc::worktree::worktree_response(response).unwrap_err();
            let native = serde_json::to_value(native).unwrap();
            eprintln!("A08_TYPED_RESULT case={slug} wire={wire} native={native}");
            let after = service.catalog().unwrap().revision;
            if slug == "partial" {
                let event = tokio::time::timeout(Duration::from_secs(5), events.recv()).await.unwrap().unwrap();
                assert!(event.removed); assert_eq!(after.0, before.0 + 1); assert_eq!(event.revision, after);
            } else { assert_eq!(after, before); assert!(events.try_recv().is_err()); }
            let exists = tokio::fs::try_exists(&path).await.unwrap();
            assert_eq!(exists, slug != "partial");
            tokio::task::spawn_blocking({ let repo = repo.clone(); let branch = branch.clone(); move || crate::worktree::run_git(repo, &["show-ref", "--verify", &format!("refs/heads/{branch}")]).unwrap() }).await.unwrap();
            if let Some(pid) = pid { assert_eq!(unsafe { libc::kill(pid, 0) }, 0); }
            results.push((slug, path, branch, wire, native));
        }
        // Collect all three behavioral failures before asserting, so RED records every missing contract.
        for (slug, path, branch, wire, native) in &results {
            let code = match *slug { "busy" => "WORKTREE_BUSY", "locked" => "WORKTREE_LOCKED", _ => "WORKTREE_REMOVED_BRANCH_RETAINED" };
            assert_eq!(wire["type"], "worktreeError", "{slug}: {wire}");
            assert_eq!(native, &wire["error"]);
            assert_eq!(native["code"], code);
            assert_eq!(native["details"]["path"], path.to_str().unwrap());
            match *slug {
                "busy" => assert_eq!(native["details"]["liveSessionIds"], serde_json::json!([session.as_ref().unwrap()])),
                "locked" => assert_eq!(native["details"]["reason"], "typed fixture lock"),
                _ => {
                    assert_eq!(native["details"]["worktreeRemoved"], true);
                    assert_eq!(native["details"]["branchDeleted"], false);
                    assert_eq!(native["details"]["branch"], branch.as_str());
                    assert_eq!(native["details"]["cause"]["code"], "GIT_ERROR");
                    assert!(native["details"]["cause"]["details"]["exitCode"].as_i64().unwrap() != 0);
                    assert!(native["details"]["cause"]["details"]["command"].as_str().unwrap().contains("branch"));
                    assert!(native["details"]["cause"]["details"]["stderr"].as_str().unwrap().contains(&format!("{branch}.lock")));
                }
            }
        }
    }).catch_unwind().await;
    if let Some(session) = session { owner.terminal_service().close_session(&session).await.unwrap(); }
    if let Some(pid) = pid {
        let mut status = 0;
        assert_eq!(unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) }, -1);
        assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(libc::ECHILD));
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
        assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
        eprintln!("A08_TYPED_PTY_CLEANUP pid={pid} absent=true already_reaped=true");
    }
    drop(client); listener_task.abort(); assert!(listener_task.await.unwrap_err().is_cancelled()); drop(owner);
    tokio::fs::remove_file(&socket).await.unwrap();
    assert!(tokio::net::UnixStream::connect(&socket).await.is_err());
    let receipt = root.path().to_owned();
    tokio::task::spawn_blocking(move || root.close().unwrap()).await.unwrap();
    eprintln!("A08_TYPED_CLEANUP listener_joined=true socket_refused=true root={} absent={} failed={}", receipt.display(), !receipt.exists(), outcome.is_err());
    outcome.unwrap();
}

#[test]
fn local_worktree_error_wire_roundtrip() {
    for domain in [
        WorktreeError::DirtyWorktree { path: PathBuf::from("/repo/feature"), count: 1, files: vec!["untracked.txt".into()] },
        WorktreeError::UnmergedBranch { branch: "orca/local/feature".into(), head: "abc123".into() },
        WorktreeError::WorktreeNotFound { path: PathBuf::from("/repo/missing") },
        WorktreeError::WorktreeAlreadyExists { path: PathBuf::from("/repo/existing") },
        WorktreeError::InvalidNamespace { reason: "invalid slug".into() },
    ] {
        let expected = crate::ipc::IpcError::from(domain);
        let wire = serde_json::to_vec(&DaemonResponse::WorktreeError { error: expected.clone() }).unwrap();
        let response = serde_json::from_slice(&wire).unwrap();
        assert_eq!(crate::ipc::worktree::worktree_response(response).unwrap_err(), expected);
    }
}

#[tokio::test]
async fn owner_http_gate_and_eight_admissions() {
    let (root, owner, workspace, token, device) = tokio::task::spawn_blocking(|| {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo"); std::fs::create_dir(&repo).unwrap();
        crate::worktree::run_git(&repo, &["init", "--quiet"]).unwrap();
        crate::worktree::run_git(&repo, &["-c", "user.name=A08", "-c", "user.email=a08@example.invalid", "commit", "--allow-empty", "-m", "base"]).unwrap();
        let owner = Arc::new(DaemonServer::new_with_paths(Some(root.path().join("data/config")), Some(root.path().join("data/auth"))));
        let state = owner.remote_state();
        let workspace = state.machine_services.as_ref().unwrap().workspaces.register_machine(repo.to_str().unwrap()).unwrap();
        let pin = state.auth_manager.create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine).unwrap();
        let (token, device) = state.auth_manager.exchange_pairing_code(&pin, "authority").unwrap();
        (root, owner, workspace, token, device.id)
    }).await.unwrap();
    let state = owner.remote_state().clone();
    let service = state.machine_services.as_ref().unwrap().workspaces.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let router = crate::remote::server::create_remote_router(state.clone());
    let mut gateway = tokio::spawn(async move { axum::serve(listener, router).with_graceful_shutdown(async { let _ = stopped.await; }).await.unwrap(); });
    let fence = service.worktree_gate(&workspace);
    let mut held = Some(fence.lock());
    let mut jobs = tokio::task::JoinSet::new();
    let outcome = std::panic::AssertUnwindSafe(async {
    let (entered, mut entries) = tokio::sync::mpsc::unbounded_channel();
    *service.transaction_probe.write() = Some(Arc::new(move |phase| { if phase == "worktreeQueued" || phase == "ownerWorktreeQueued" { entered.send(phase.to_owned()).unwrap(); } }));
    let client = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(40)).build().unwrap();
    let endpoint = format!("http://{address}/api/v1/workspace/worktrees");
    // One local request owns one admission slot; seven HTTP requests own the rest.
    let local_owner = owner.clone(); let local_workspace = workspace.clone();
    jobs.spawn(async move { match ipc(local_owner, DaemonRequest::CreateWorktree { workspace_id: local_workspace.clone(), worktree: crate::worktree::WorktreeIdentity { ws_id: local_workspace, slug: "ipc".into() }, base_ref: Some("HEAD".into()) }).await { DaemonResponse::CreateWorktreeOk { .. } => 201, response => panic!("{response:?}") } });
    for n in 0..7 {
        let call = client.post(&endpoint).bearer_auth(&token).body(serde_json::json!({"requestId":uuid::Uuid::new_v4().to_string(),"workspaceId":workspace,"worktree":{"wsId":workspace,"slug":format!("http-{n}")}}).to_string());
        jobs.spawn(async move { call.send().await.unwrap().status().as_u16() });
    }
        let mut local_seen = false;
        for _ in 0..8 { let phase = tokio::time::timeout(Duration::from_secs(10), entries.recv()).await.unwrap().unwrap(); local_seen |= phase == "ownerWorktreeQueued"; }
        assert!(local_seen);
        assert_eq!(service.project_mutations.available_permits(), 0);
        let ninth = client.post(&endpoint).bearer_auth(&token).body("{}").send().await.unwrap();
        assert_eq!(ninth.status().as_u16(), 429);
        // Revoke while HTTP jobs wait; no queued Git may be authorized later.
        assert!(state.auth_manager.revoke_device(&device));
    drop(held.take());
    let mut statuses = Vec::new();
    while let Some(joined) = tokio::time::timeout(Duration::from_secs(45), jobs.join_next()).await.unwrap() { statuses.push(joined.unwrap()); }
    *service.transaction_probe.write() = None;
    // A fresh grant queues behind the same gate while the registered root is
    // replaced by a symlink. Revalidation must happen after gate acquisition.
    #[cfg(unix)] {
        let auth = state.auth_manager.clone();
        let fresh = tokio::task::spawn_blocking(move || {
            let pin = auth.create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine).unwrap();
            auth.exchange_pairing_code(&pin, "replacement").unwrap().0
        }).await.unwrap();
        for managed_child in [false, true] {
        held = Some(fence.lock());
        let (entered, reached) = tokio::sync::oneshot::channel();
        let entered = std::sync::Mutex::new(Some(entered));
        *service.transaction_probe.write() = Some(Arc::new(move |phase| { if phase == "worktreeQueued" { entered.lock().unwrap().take().unwrap().send(()).unwrap(); } }));
        let call = if managed_child {
            client.delete(&endpoint).bearer_auth(&fresh).body(serde_json::json!({"requestId":uuid::Uuid::new_v4().to_string(),"workspaceId":workspace,"worktree":{"wsId":workspace,"slug":"ipc"},"deleteBranch":false,"expectedRevision":service.catalog().unwrap().revision}).to_string())
        } else {
            client.post(&endpoint).bearer_auth(&fresh).body(serde_json::json!({"requestId":uuid::Uuid::new_v4().to_string(),"workspaceId":workspace,"worktree":{"wsId":workspace,"slug":"replaced"}}).to_string())
        };
        jobs.spawn(async move { call.send().await.unwrap().status().as_u16() });
        let ready = tokio::time::timeout(Duration::from_secs(10), reached).await;
        let repo = if managed_child { root.path().join("repo/.orca-worktrees").join(&workspace).join("ipc") } else { root.path().join("repo") };
        let original = repo.with_extension("original"); let outside = root.path().join(if managed_child {"outside-child"} else {"outside"});
        let paths = (repo.clone(), original.clone(), outside.clone());
        tokio::task::spawn_blocking(move || { std::fs::rename(&paths.0, &paths.1).unwrap(); std::fs::create_dir(&paths.2).unwrap(); std::fs::write(paths.2.join("sentinel"), "untouched").unwrap(); std::os::unix::fs::symlink(&paths.2, &paths.0).unwrap(); }).await.unwrap();
        drop(held.take());
        let status = tokio::time::timeout(Duration::from_secs(45), jobs.join_next()).await.unwrap().unwrap().unwrap();
        tokio::task::spawn_blocking(move || { assert_eq!(std::fs::read_to_string(outside.join("sentinel")).unwrap(), "untouched"); std::fs::remove_file(&repo).unwrap(); std::fs::rename(original, repo).unwrap(); }).await.unwrap();
        *service.transaction_probe.write() = None;
        assert!(ready.unwrap().is_ok()); assert_eq!(status, if managed_child {409} else {400});
        eprintln!("A08 queued symlink replacement managed_child={managed_child} HTTP={status} outside_sentinel_untouched=true");
        }
    }
    let disk_service = service.clone(); let disk_workspace = workspace.clone();
    let rows = tokio::task::spawn_blocking(move || rows(&disk_workspace, &disk_service.worktree_manager(&disk_workspace, false).unwrap()).unwrap()).await.unwrap();
    assert_eq!(statuses.iter().filter(|&&status| status == 201).count(), 1);
    assert_eq!(statuses.iter().filter(|&&status| status == 401).count(), 7);
    assert_eq!(rows.iter().filter(|row| row.managed).count(), 1);
    }).catch_unwind().await;

    // Cleanup cannot assert until every owned resource has been dealt with.
    drop(held.take());
    *service.transaction_probe.write() = None;
    let mut cleanup_errors = Vec::new();
    loop {
        match tokio::time::timeout(Duration::from_secs(45), jobs.join_next()).await {
            Ok(Some(Ok(_))) => {},
            Ok(Some(Err(error))) => cleanup_errors.push(format!("request worker: {error}")),
            Ok(None) => break,
            Err(error) => {
                cleanup_errors.push(format!("request drain: {error}"));
                jobs.shutdown().await;
                break;
            }
        }
    }
    // HTTP cancellation may finish before its blocking worker. Waiting for all
    // admission permits observes their exit, not just the request wrappers.
    let drained = tokio::time::timeout(Duration::from_secs(45), service.project_mutations.clone().acquire_many_owned(8)).await;
    if !matches!(drained, Ok(Ok(_))) { cleanup_errors.push(format!("domain drain: {drained:?}")); }
    let repo = root.path().join("repo");
    let original = root.path().join("original");
    let restored = tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        if original.try_exists()? {
            match std::fs::symlink_metadata(&repo) {
                Ok(metadata) if metadata.file_type().is_symlink() => std::fs::remove_file(&repo)?,
                Ok(_) => return Err(std::io::Error::other("replacement is not the fixture symlink")),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
                Err(error) => return Err(error),
            }
            std::fs::rename(original, repo)?;
        }
        Ok(())
    }).await;
    if !matches!(restored, Ok(Ok(()))) { cleanup_errors.push(format!("root restore: {restored:?}")); }
    if stop.send(()).is_err() { cleanup_errors.push("gateway shutdown receiver closed".into()); }
    match tokio::time::timeout(Duration::from_secs(45), &mut gateway).await {
        Ok(Ok(())) => {},
        Ok(Err(error)) => cleanup_errors.push(format!("gateway: {error}")),
        Err(error) => {
            cleanup_errors.push(format!("gateway shutdown: {error}"));
            gateway.abort();
            let reaped = gateway.await;
            if !matches!(reaped, Err(ref error) if error.is_cancelled()) { cleanup_errors.push(format!("gateway abort: {reaped:?}")); }
        }
    }
    drop(drained);
    drop(service); drop(state); drop(owner);
    let closed = tokio::task::spawn_blocking(move || root.close()).await;
    if !matches!(closed, Ok(Ok(()))) { cleanup_errors.push(format!("root close: {closed:?}")); }
    eprintln!("A08 gate fixture cleanup completed; errors={cleanup_errors:?}");
    assert!(cleanup_errors.is_empty(), "{cleanup_errors:?}");
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
    eprintln!("A08 shared owner IPC + seven HTTP slots=8 ninth=429 queued revoke=401x7 local=201 only_one_git_target=true listeners_workers_joined=true private_root_removed=true");
}

#[test]
fn interrupted_worktree_owner_child() {
    use std::io::{BufRead, Write};
    let Some(root) = std::env::var_os("A08_INTERRUPTED_ROOT") else { return; };
    let root = PathBuf::from(root);
    let replay = std::env::var_os("A08_INTERRUPTED_REPLAY").is_some();
    let owner = DaemonServer::new_with_paths(Some(root.join("data/config")), Some(root.join("data/auth")));
    let state = owner.remote_state().clone();
    let service = &state.machine_services.as_ref().unwrap().workspaces;
    let (workspace, token, request): (String, String, String) = if replay { serde_json::from_slice(&std::fs::read(root.join("request.json")).unwrap()).unwrap() } else {
        let workspace = service.register_machine(root.join("repo").to_str().unwrap()).unwrap();
        let pin = state.auth_manager.create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine).unwrap();
        let (token, _) = state.auth_manager.exchange_pairing_code(&pin, "interrupted").unwrap();
        let value = (workspace, token, uuid::Uuid::new_v4().to_string());
        crate::remote::auth::write_private_json(&root.join("request.json"), &value).unwrap(); value
    };
    if !replay && std::env::var_os("A08_ACTIVE_GIT").is_none() { *service.transaction_probe.write() = Some(Arc::new(|phase| { if phase == "worktreePublication" {
        // Serial libtest prints its test-name prefix without a newline.
        println!("\nA08_GIT_COMPLETE_BEFORE_PUBLICATION"); std::io::stdout().flush().unwrap();
        let mut line = String::new(); std::io::stdin().lock().read_line(&mut line).unwrap();
        panic!("unexpected crash barrier release");
    }})); }
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let router = crate::remote::server::create_remote_router(state.clone());
        let gateway = tokio::spawn(async move { axum::serve(listener, router).with_graceful_shutdown(async { let _ = stopped.await; }).await.unwrap(); });
        let result = std::panic::AssertUnwindSafe(async {
            let client = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(40)).build().unwrap();
            let response = client.post(format!("http://{address}/api/v1/workspace/worktrees")).bearer_auth(token).body(serde_json::json!({"requestId":request,"workspaceId":workspace,"worktree":{"wsId":workspace,"slug":"interrupted"}}).to_string()).send().await.unwrap();
            assert!(replay); assert_eq!(response.status().as_u16(), 409);
            let body: serde_json::Value = serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
            assert_eq!(body["error"]["code"], "OPERATION_OUTCOME_UNKNOWN");
            let service = state.machine_services.as_ref().unwrap().workspaces.clone();
            let listed = tokio::task::spawn_blocking(move || rows(&workspace, &service.worktree_manager(&workspace, false).unwrap()).unwrap()).await.unwrap();
            assert_eq!(listed.iter().filter(|row| row.managed).count(), 1);
        }).catch_unwind().await;
        stop.send(()).unwrap(); gateway.await.unwrap(); result.unwrap();
        println!("\nA08_RECONCILED_UNKNOWN_NO_REPEAT listener_joined=true");
    });
}

#[tokio::test]
async fn interrupted_git_transaction_restart_never_repeats() {
    interrupted_fixture(false).await;
}

#[cfg(unix)]
#[tokio::test]
async fn followthrough_owner_crash_while_git_active() {
    interrupted_fixture(true).await;
}

async fn interrupted_fixture(active: bool) {
    let root = tokio::task::spawn_blocking(|| {
        let root = tempfile::tempdir().unwrap(); let repo = root.path().join("repo"); std::fs::create_dir(&repo).unwrap();
        crate::worktree::run_git(&repo, &["init", "--quiet"]).unwrap();
        crate::worktree::run_git(&repo, &["-c", "user.name=A08", "-c", "user.email=a08@example.invalid", "commit", "--allow-empty", "-m", "base"]).unwrap(); root
    }).await.unwrap();
    let ready = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    if active {
        let hook = root.path().join("repo/.git/hooks/post-checkout"); let port = ready.local_addr().unwrap().port();
        tokio::task::spawn_blocking(move || {
            #[cfg(unix)] {
                use std::os::unix::fs::PermissionsExt;
                std::fs::write(&hook, format!("#!/bin/bash\nexec 3<>/dev/tcp/127.0.0.1/{port}\nprintf '%s %s %s\\n' \"$$\" \"$PPID\" \"$PWD\" >&3\nread -r release <&3\n")).unwrap();
                std::fs::set_permissions(hook, std::fs::Permissions::from_mode(0o700)).unwrap();
            }
        }).await.unwrap();
    }
    let outcome = std::panic::AssertUnwindSafe(async {
        for replay in [false, true] {
            let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
            command.args(["--exact", "remote::workspace_api::worktrees::authority_tests::interrupted_worktree_owner_child", "--nocapture"])
                .env("A08_INTERRUPTED_ROOT", root.path()).env("HOME", root.path()).env("FERRYX_DATA_DIR", root.path().join("data")).env("FERRYX_RUNTIME_DIR", root.path().join("runtime"))
                .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).kill_on_drop(true);
            if replay { command.env("A08_INTERRUPTED_REPLAY", "1"); }
            if active { command.env("A08_ACTIVE_GIT", "1"); }
            let mut child = command.spawn().unwrap(); let pid = child.id().unwrap();
            let stdin = child.stdin.take().unwrap();
            let mut lines = tokio::io::BufReader::new(child.stdout.take().unwrap()).lines();
            let mut git_socket = None;
            let mut git_pid = None;
            let signal = tokio::time::timeout(Duration::from_secs(30), async {
                if active && !replay {
                    let (socket, _) = ready.accept().await.unwrap();
                    let mut reader = tokio::io::BufReader::new(socket); let mut line = String::new();
                    reader.read_line(&mut line).await.unwrap();
                    let mut fields = line.split_whitespace(); let hook = fields.next().unwrap();
                    let git: i32 = fields.next().unwrap().parse().unwrap();
                    git_pid = Some(git); git_socket = Some(reader);
                    eprintln!("A08 ACTIVE owner={pid} git={git} hook={hook} cwd={}", fields.collect::<Vec<_>>().join(" "));
                    return true;
                }
                while let Some(line) = lines.next_line().await.unwrap() {
                    if line.starts_with(if replay { "A08_RECONCILED_UNKNOWN_NO_REPEAT" } else { "A08_GIT_COMPLETE_BEFORE_PUBLICATION" }) { return true; }
                } false
            }).await;
            if !replay || !matches!(signal, Ok(true)) { child.start_kill().unwrap(); }
            let status = child.wait().await.unwrap(); drop(stdin);
            #[cfg(unix)]
            if let Some(git) = git_pid {
                let group = unsafe { libc::getpgid(git) };
                let live = unsafe { libc::kill(git, 0) };
                let killed = if group == git { unsafe { libc::kill(-git, libc::SIGKILL) } } else { -1 };
                use tokio::io::AsyncReadExt;
                let mut bytes = Vec::new();
                let eof = tokio::time::timeout(Duration::from_secs(10), git_socket.as_mut().unwrap().read_to_end(&mut bytes)).await;
                assert_eq!(group, git); assert_eq!(live, 0); assert_eq!(killed, 0); assert_eq!(eof.unwrap().unwrap(), 0);
                eprintln!("A08 ACTIVE owner_reaped=true git_group={git} fixture_cleanup_signal=SIGKILL hook_eof=true (nonchildren not waitpid-reapable)");
            }
            eprintln!("A08 interrupted owner pid={pid} replay={replay} status={status} reaped=true");
            assert!(matches!(signal, Ok(true)));
            assert_eq!(status.success(), replay);
        }
    }).catch_unwind().await;
    tokio::task::spawn_blocking(move || root.close().unwrap()).await.unwrap();
    eprintln!("A08 interrupted Git transaction private_root_removed=true");
    outcome.unwrap();
}
