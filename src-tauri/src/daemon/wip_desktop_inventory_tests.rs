async fn desktop_fixture() -> (tempfile::TempDir, DaemonServer, CreateSessionRequest) {
    eprintln!("[desktop_fixture] entering desktop_fixture");
    tokio::task::spawn_blocking(|| {
        eprintln!("[desktop_fixture] creating tempdir");
        let root = tempfile::tempdir().unwrap();
        let project_dir = root.path().join("project");
        std::fs::create_dir_all(&project_dir).unwrap();
        eprintln!("[desktop_fixture] project_dir: {:?}", project_dir);

        eprintln!("[desktop_fixture] run_git init");
        crate::worktree::git::run_git(&project_dir, &["init"]).unwrap();
        eprintln!("[desktop_fixture] run_git config name");
        crate::worktree::git::run_git(&project_dir, &["config", "user.name", "Test"]).unwrap();
        eprintln!("[desktop_fixture] run_git config email");
        crate::worktree::git::run_git(&project_dir, &["config", "user.email", "test@example.com"]).unwrap();
        eprintln!("[desktop_fixture] write init.txt");
        std::fs::write(project_dir.join("init.txt"), b"init").unwrap();
        eprintln!("[desktop_fixture] run_git add");
        crate::worktree::git::run_git(&project_dir, &["add", "init.txt"]).unwrap();
        eprintln!("[desktop_fixture] run_git commit");
        crate::worktree::git::run_git(&project_dir, &["commit", "--no-gpg-sign", "-m", "init"]).unwrap();

        eprintln!("[desktop_fixture] creating DaemonServer");
        let owner = DaemonServer::new_with_paths(
            Some(root.path().join("config")),
            Some(root.path().join("auth")),
        );

        let ws_id = "desktop-project".to_string();
        let project_str = project_dir.to_str().unwrap();
        eprintln!("[desktop_fixture] registering workspace: {}", ws_id);
        owner
            .session_service
            .workspace_service
            .register(&ws_id, project_str)
            .expect("desktop workspace registration");

        eprintln!("[desktop_fixture] asserting repo_root");
        assert_eq!(
            owner
                .session_service
                .workspace_service
                .registry
                .repo_root(&ws_id)
                .unwrap(),
            std::fs::canonicalize(&project_dir).unwrap()
        );

        eprintln!("[desktop_fixture] ready");
        (root, owner, request(ws_id))
    })
    .await
    .unwrap()
}

async fn desktop_gui_session_inventory_and_attach_validation() {
    eprintln!("[inventory_test] start");
    let (root, owner, request) = desktop_fixture().await;
    eprintln!("[inventory_test] fixture ready");
    let service = owner.session_service.clone();
    let epoch = Epoch(owner.epoch());

    // Spawn a desktop GUI session (machine is None)
    eprintln!("[inventory_test] admit_spawn");
    let session_id = service
        .handle_spawn(
            "desktop-gui-req-1",
            &request.workspace_id,
            None,
            None,
            80,
            24,
            None,
            None,
            #[cfg(test)]
            None,
        )
        .await
        .expect("desktop GUI spawn");
    eprintln!("[inventory_test] spawned session_id: {}", session_id);

    // 1. machine_sessions_routed lists the desktop GUI session
    eprintln!("[inventory_test] machine_sessions_routed");
    let inventory = service.machine_sessions_routed(epoch).await.unwrap();
    let found = inventory
        .sessions
        .iter()
        .find(|s| s.target.session_id == session_id);
    assert!(found.is_some(), "Desktop GUI session must appear in routed machine sessions");
    let found = found.unwrap();
    assert_eq!(found.workspace_id, request.workspace_id);
    assert!(found.running);
    assert_eq!(found.target.daemon_epoch, epoch);
    assert_eq!(found.target.machine_id, crate::remote::auth::load_or_generate_machine_identity(&crate::remote::auth::canonical_identity_dir().unwrap()).unwrap().machine_id);
    assert_eq!(found.cwd, ".");

    // 2. validate_machine_target succeeds for exact target
    eprintln!("[inventory_test] validate_machine_target");
    let target = found.target.clone();
    let validated = service.validate_machine_target(&target).await.unwrap();
    assert_eq!(validated.target, target);
    assert_eq!(validated.workspace_id, request.workspace_id);
    assert!(validated.running);
    assert_eq!(validated.cwd, ".");

    // 3. Stale epoch is rejected with STALE_EPOCH
    eprintln!("[inventory_test] stale_epoch");
    let mut stale_target = target.clone();
    stale_target.daemon_epoch = Epoch(epoch.0 + 999);
    let stale_err = service.validate_machine_target(&stale_target).await.unwrap_err();
    assert_eq!(stale_err, "STALE_EPOCH");

    // 4. Wrong machine_id is rejected with STALE_EPOCH
    eprintln!("[inventory_test] wrong_machine_id");
    let mut wrong_mach_target = target.clone();
    wrong_mach_target.machine_id = "wrong-machine-id".into();
    let wrong_mach_err = service.validate_machine_target(&wrong_mach_target).await.unwrap_err();
    assert_eq!(wrong_mach_err, "STALE_EPOCH");

    // 5. Remote SSH / :: proxy workspace session is rejected
    eprintln!("[inventory_test] ssh reject");
    let (ssh_session_id, _) = service.terminal_service.spawn_shell(80, 24).unwrap();
    service.session_metadata.write().insert(
        ssh_session_id.clone(),
        StoredSessionMeta {
            client_request_id: "ssh-req".into(),
            machine_session: None,
            workspace_id: "ssh:host::project".into(),
            worktree: None,
            cwd: root.path().join("project"),
            provider_claim: None,
            spawn_fingerprint: SpawnRequestFingerprint {
                workspace_id: "ssh:host::project".into(),
                worktree: None,
                cwd: None,
                cols: 80,
                rows: 24,
                shell: None,
                provider_claim: None,
                startup: None,
                requested_session_id: None,
            },
        },
    );
    let inventory_after = service.machine_sessions_routed(epoch).await.unwrap();
    assert!(
        inventory_after.sessions.iter().all(|s| s.target.session_id != ssh_session_id),
        "SSH session must not appear in machine sessions"
    );
    let ssh_target = RemoteTerminalTarget {
        machine_id: crate::remote::auth::load_or_generate_machine_identity(&crate::remote::auth::canonical_identity_dir().unwrap()).unwrap().machine_id,
        daemon_epoch: epoch,
        session_id: ssh_session_id.clone(),
    };
    let ssh_err = service.validate_machine_target(&ssh_target).await.unwrap_err();
    // main serves remote/SSH sessions through the remote runtime projector instead of refusing
    // them; a target with no remote runtime details is still rejected, never validated.
    assert!(matches!(ssh_err.as_str(), "SESSION_OWNERSHIP_CHANGED" | "SESSION_NOT_FOUND"), "{ssh_err}");

    // Clean up
    eprintln!("[inventory_test] closing sessions");
    service.terminal_service.close_session(&session_id).await.expect("close desktop GUI session");
    service.wait_machine_lifecycle(&session_id).await.expect("desktop GUI lifecycle completion");
    service.terminal_service.close_session(&ssh_session_id).await.expect("close SSH session");
    drop(service);
    drop(owner);
    eprintln!("[inventory_test] cleaning root");
    crate::ipc::run_blocking(move || {
        root.close().expect("tempdir root cleanup");
        Ok(())
    })
    .await
    .unwrap();
    eprintln!("[inventory_test] complete");
}

async fn desktop_gui_session_managed_worktree_projection() {
    eprintln!("[managed_test] start");
    let (root, owner, request) = desktop_fixture().await;
    eprintln!("[managed_test] fixture ready");
    let service = owner.session_service.clone();
    let epoch = Epoch(owner.epoch());

    let project_dir = root.path().join("project");
    let ws_id = request.workspace_id.clone();
    let wt_slug = "feature-task";
    eprintln!("[managed_test] create worktree");
    let (wt_path, wt_identity) = crate::ipc::run_blocking(move || {
        let manager = crate::worktree::WorktreeManager::new(&project_dir);
        let path = manager.worktree_path_for(&ws_id, wt_slug).unwrap();
        manager
            .create_worktree(crate::worktree::CreateWorktreeOptions::new(
                &ws_id,
                wt_slug,
                &path,
            ))
            .unwrap();

        let ident = crate::worktree::model::WorktreeIdentity {
            ws_id: ws_id.clone(),
            slug: wt_slug.to_string(),
        };
        Ok((path, ident))
    })
    .await
    .unwrap();
    eprintln!("[managed_test] worktree ready: {:?}", wt_path);

    eprintln!("[managed_test] spawn shell");
    let (session_id, _) = service.terminal_service.spawn_shell(80, 24).unwrap();
    service.session_metadata.write().insert(
        session_id.clone(),
        StoredSessionMeta {
            client_request_id: "wt-req".into(),
            machine_session: None,
            workspace_id: request.workspace_id.clone(),
            worktree: Some(wt_identity.clone()),
            cwd: wt_path.clone(),
            provider_claim: None,
            spawn_fingerprint: SpawnRequestFingerprint {
                workspace_id: request.workspace_id.clone(),
                worktree: Some(wt_identity.clone()),
                cwd: None,
                cols: 80,
                rows: 24,
                shell: None,
                provider_claim: None,
                startup: None,
                requested_session_id: None,
            },
        },
    );

    eprintln!("[managed_test] check inventory");
    let inventory = service.machine_sessions_routed(epoch).await.unwrap();
    let found = inventory
        .sessions
        .iter()
        .find(|s| s.target.session_id == session_id)
        .expect("worktree session in inventory");
    assert_eq!(found.workspace_id, request.workspace_id);
    assert_eq!(found.worktree.as_ref().map(|w| w.slug.as_str()), Some(wt_slug));
    assert_eq!(found.cwd, ".");

    eprintln!("[managed_test] validate target");
    let validated = service.validate_machine_target(&found.target).await.unwrap();
    assert_eq!(validated.cwd, ".");
    assert_eq!(validated.worktree.as_ref().map(|w| w.slug.as_str()), Some(wt_slug));

    eprintln!("[managed_test] close session");
    service.terminal_service.close_session(&session_id).await.expect("close managed worktree session");
    service.wait_machine_lifecycle(&session_id).await.expect("managed worktree lifecycle completion");
    drop(service);
    drop(owner);
    eprintln!("[managed_test] clean root");
    crate::ipc::run_blocking(move || {
        root.close().expect("tempdir root cleanup");
        Ok(())
    })
    .await
    .unwrap();
    eprintln!("[managed_test] complete");
}

async fn desktop_gui_session_jail_and_missing_path_rejections() {
    eprintln!("[jail_test] start");
    let (root, owner, request) = desktop_fixture().await;
    eprintln!("[jail_test] fixture ready");
    let service = owner.session_service.clone();
    let epoch = Epoch(owner.epoch());

    // Retain outside_jail_dir guard alive in scope until function end
    let outside_jail_dir = tempfile::tempdir().unwrap();

    // Session whose cwd escapes root jail (outside repo_root)
    eprintln!("[jail_test] spawn shell escape");
    let (jail_escape_id, _) = service.terminal_service.spawn_shell(80, 24).unwrap();
    service.session_metadata.write().insert(
        jail_escape_id.clone(),
        StoredSessionMeta {
            client_request_id: "escape-req".into(),
            machine_session: None,
            workspace_id: request.workspace_id.clone(),
            worktree: None,
            cwd: outside_jail_dir.path().to_path_buf(), // outside project repo_root
            provider_claim: None,
            spawn_fingerprint: SpawnRequestFingerprint {
                workspace_id: request.workspace_id.clone(),
                worktree: None,
                cwd: None,
                cols: 80,
                rows: 24,
                shell: None,
                provider_claim: None,
                startup: None,
                requested_session_id: None,
            },
        },
    );
    let target = RemoteTerminalTarget {
        machine_id: crate::remote::auth::load_or_generate_machine_identity(&crate::remote::auth::canonical_identity_dir().unwrap()).unwrap().machine_id,
        daemon_epoch: epoch,
        session_id: jail_escape_id.clone(),
    };
    eprintln!("[jail_test] validate escape target");
    let err = service.validate_machine_target(&target).await.unwrap_err();
    assert_eq!(err, "SESSION_OWNERSHIP_CHANGED");

    // Session whose cwd does not exist (missing path / broken symlink)
    eprintln!("[jail_test] spawn shell missing path");
    let (missing_path_id, _) = service.terminal_service.spawn_shell(80, 24).unwrap();
    service.session_metadata.write().insert(
        missing_path_id.clone(),
        StoredSessionMeta {
            client_request_id: "missing-req".into(),
            machine_session: None,
            workspace_id: request.workspace_id.clone(),
            worktree: None,
            cwd: root.path().join("project").join("nonexistent_subdir_xyz"),
            provider_claim: None,
            spawn_fingerprint: SpawnRequestFingerprint {
                workspace_id: request.workspace_id.clone(),
                worktree: None,
                cwd: None,
                cols: 80,
                rows: 24,
                shell: None,
                provider_claim: None,
                startup: None,
                requested_session_id: None,
            },
        },
    );
    let target_missing = RemoteTerminalTarget {
        machine_id: crate::remote::auth::load_or_generate_machine_identity(&crate::remote::auth::canonical_identity_dir().unwrap()).unwrap().machine_id,
        daemon_epoch: epoch,
        session_id: missing_path_id.clone(),
    };
    eprintln!("[jail_test] validate missing path target");
    let err_missing = service.validate_machine_target(&target_missing).await.unwrap_err();
    assert_eq!(err_missing, "INVALID_PATH");

    // Clean up
    eprintln!("[jail_test] close sessions");
    service.terminal_service.close_session(&jail_escape_id).await.expect("close jail escape session");
    service.terminal_service.close_session(&missing_path_id).await.expect("close missing path session");
    drop(outside_jail_dir);
    drop(service);
    drop(owner);
    eprintln!("[jail_test] clean root");
    crate::ipc::run_blocking(move || {
        root.close().expect("tempdir root cleanup");
        Ok(())
    })
    .await
    .unwrap();
    eprintln!("[jail_test] complete");
}

async fn desktop_gui_session_lifecycle_machine_events() {
    eprintln!("[events_test] start");
    let (root, owner, request) = desktop_fixture().await;
    eprintln!("[events_test] fixture ready");
    let service = owner.session_service.clone();
    let epoch = Epoch(owner.epoch());

    // Subscribe to machine_events BEFORE triggering spawn
    let mut events_rx = service.workspace_service.machine_events.subscribe();

    // 1. Spawn desktop GUI session
    eprintln!("[events_test] admit_spawn");
    let session_id = service
        .handle_spawn(
            "desktop-gui-event-req",
            &request.workspace_id,
            None,
            None,
            80,
            24,
            None,
            None,
            #[cfg(test)]
            None,
        )
        .await
        .expect("desktop GUI spawn");
    eprintln!("[events_test] spawned session_id: {}", session_id);

    // 2. Receive sessionStarted event
    eprintln!("[events_test] wait sessionStarted");
    let start_event = tokio::time::timeout(std::time::Duration::from_secs(5), events_rx.recv())
        .await
        .expect("sessionStarted timeout")
        .expect("sessionStarted receive");
    assert_eq!(start_event["type"], "sessionStarted");
    assert_eq!(start_event["sessionId"], session_id);
    assert_eq!(start_event["workspaceId"], request.workspace_id);

    // 3. Verify session appears in inventory
    eprintln!("[events_test] check inventory");
    let inventory = service.machine_sessions_routed(epoch).await.unwrap();
    assert!(inventory.sessions.iter().any(|s| s.target.session_id == session_id));

    // 4. Close session and receive sessionExited event
    eprintln!("[events_test] close session");
    service.terminal_service.close_session(&session_id).await.unwrap();
    eprintln!("[events_test] wait sessionExited");
    let exit_event = tokio::time::timeout(std::time::Duration::from_secs(5), events_rx.recv())
        .await
        .expect("sessionExited timeout")
        .expect("sessionExited receive");
    assert_eq!(exit_event["type"], "sessionExited");
    assert_eq!(exit_event["sessionId"], session_id);
    assert_eq!(exit_event["workspaceId"], request.workspace_id);
    service.wait_machine_lifecycle(&session_id).await.expect("desktop GUI lifecycle completion");

    // 5. Verify session no longer in inventory as running
    eprintln!("[events_test] check inventory after exit");
    let inventory_after = service.machine_sessions_routed(epoch).await.unwrap();
    let found = inventory_after.sessions.iter().find(|s| s.target.session_id == session_id);
    assert!(found.is_none() || !found.unwrap().running);

    drop(service);
    drop(owner);
    eprintln!("[events_test] clean root");
    crate::ipc::run_blocking(move || {
        root.close().expect("tempdir root cleanup");
        Ok(())
    })
    .await
    .unwrap();
    eprintln!("[events_test] complete");
}

#[tokio::test]
async fn desktop_inventory_isolated_child() {
    if std::env::var_os("FERRYX_PRIVATE_INVENTORY_CHILD").is_none() { return; }
    desktop_gui_session_inventory_and_attach_validation().await;
    desktop_gui_session_managed_worktree_projection().await;
    desktop_gui_session_jail_and_missing_path_rejections().await;
    desktop_gui_session_lifecycle_machine_events().await;
    eprintln!("FERRYX_PRIVATE_INVENTORY_OK");
}

#[tokio::test]
async fn desktop_inventory_isolated_parent() {
    let root = tempfile::tempdir().unwrap();
    let output = tokio::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "daemon::session_service::machine_tests::desktop_inventory_isolated_child", "--nocapture"])
        .env("FERRYX_PRIVATE_INVENTORY_CHILD", "1")
        .env("FERRYX_DATA_DIR", root.path())
        .env("FERRYX_RUNTIME_DIR", root.path())
        .env("HOME", root.path())
        .env("USERPROFILE", root.path())
        .env("SHELL", if cfg!(windows) { "powershell.exe" } else { "/bin/sh" })
        .kill_on_drop(true)
        .output();
    let output = tokio::time::timeout(Duration::from_secs(120), output).await.unwrap().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(String::from_utf8_lossy(&output.stderr).contains("FERRYX_PRIVATE_INVENTORY_OK"));
}
