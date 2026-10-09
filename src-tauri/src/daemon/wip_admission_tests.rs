#[test]
fn local_split_reliability_provider_reservation_failure_and_uncertainty() {
    let claim = ProviderSessionClaimKey { agent_type: "omo".into(),
        provider_key: AgentProviderSessionKey::SessionId, provider_id: "provider".into(), transcript_path: None };
    let live = Arc::new(Mutex::new(HashMap::new()));
    let pending = Arc::new(Mutex::new(HashMap::new()));
    let capacity = Arc::new(Mutex::new(HashSet::new()));
    let reserve = |id: &str| SpawnReservation::reserve(id.into(), Some(claim.clone()), true,
        live.clone(), pending.clone(), capacity.clone(), 1);
    drop(reserve("failed-before-child").unwrap());
    assert!(live.lock().is_empty());
    assert!(pending.lock().is_empty());
    assert!(capacity.lock().is_empty());
    let mut uncertain = reserve("child-exists").unwrap();
    uncertain.published = true;
    uncertain.retain_capacity = true;
    drop(uncertain);
    assert!(matches!(reserve("replacement"), Err(SpawnError::AgentSessionConflict { .. })));
    assert_eq!(live.lock().get(&claim).map(String::as_str), Some("child-exists"));
    assert!(capacity.lock().contains("child-exists"));
}

#[test]
fn local_split_reliability_admission_clock_does_not_rollback() {
    let mut clock = AdmissionClock::default();
    let observed = clock.now();
    clock.logical_ms = observed + 600_000;
    let advanced = clock.now();
    assert!(advanced >= observed + 600_000);
    assert!(clock.now() >= advanced);
}

#[tokio::test]
async fn sixty_four_live_machine_sessions_are_the_admission_limit() {
    use futures_util::StreamExt;
    let (root, owner, template) = fixture().await;
    let service = owner.session_service.clone();
    let result = std::panic::AssertUnwindSafe(async {
        for _ in 0..63 {
            let mut request = template.clone();
            request.request_id = uuid::Uuid::new_v4().to_string();
            service
                .spawn_machine(
                    request,
                    "device".into(),
                    "digest".into(),
                    target(),
                    Instant::now() + Duration::from_secs(30),
                    Arc::new(|| Ok(())),
                )
                .await
                .unwrap();
        }
        let workspaces = service.workspace_service.clone();
        let second_path = root.path().join("second-project");
        let second_workspace = crate::ipc::run_blocking(move || {
            std::fs::create_dir(&second_path).unwrap();
            Ok(workspaces.register_machine(second_path.to_str().unwrap()).unwrap())
        }).await.unwrap();
        let (entered, observed) = tokio::sync::oneshot::channel();
        let (release, released) = std::sync::mpsc::channel();
        let entered = Mutex::new(Some(entered));
        let released = Mutex::new(released);
        *service.split_probe.write() = Some(Arc::new(move |_, phase| {
            if phase == "preparation" {
                if let Some(tx) = entered.lock().take() {
                    tx.send(()).unwrap();
                    let _ = released.lock().recv_timeout(Duration::from_secs(5));
                }
            }
        }));
        let mut first_request = template.clone();
        first_request.request_id = uuid::Uuid::new_v4().to_string();
        let first_service = service.clone();
        let first = tokio::spawn(async move { first_service.spawn_machine(first_request, "device".into(), "digest".into(), target(),
            Instant::now() + Duration::from_secs(30), Arc::new(|| Ok(()))).await });
        tokio::time::timeout(Duration::from_secs(5), observed).await.unwrap().unwrap();
        let mut second_request = template.clone();
        second_request.request_id = uuid::Uuid::new_v4().to_string();
        second_request.workspace_id = second_workspace;
        let second = service.spawn_machine(second_request, "device".into(), "digest".into(), target(),
            Instant::now() + Duration::from_secs(30), Arc::new(|| Ok(()))).await;
        let _ = release.send(());
        let first = first.await.unwrap();
        *service.split_probe.write() = None;
        first.unwrap();
        assert_eq!(second.unwrap_err(), "CAPACITY_EXCEEDED");
        assert_eq!(service.terminal_service.list_sessions().len(), 64);
        let result = service
            .spawn_machine(
                template.clone(),
                "device".into(),
                "digest".into(),
                target(),
                Instant::now() + Duration::from_secs(30),
                Arc::new(|| Ok(())),
            )
            .await;
        assert_eq!(result.unwrap_err(), "CAPACITY_EXCEEDED");
        assert!(service
            .workspace_service
            .journal
            .reconcile("device", &template.request_id)
            .unwrap()
            .is_none());
        eprintln!("A09 capacity: 64 real PTYs live, 65th refused before intent");
    })
    .catch_unwind()
    .await;
    let sessions = service.terminal_service.list_sessions();
    let results: Vec<_> = futures_util::stream::iter(sessions.into_iter().map(|id| {
        let service = service.clone();
        async move {
            let pty = service.terminal_service.get_session(&id).unwrap();
            let result = service.terminal_service.close_session(&id).await;
            let lifecycle = service.wait_machine_lifecycle(&id).await;
            (result, lifecycle, pty.is_reaped())
        }
    }))
    .buffer_unordered(4)
    .collect()
    .await;
    drop(service);
    drop(owner);
    tokio::task::spawn_blocking(move || root.close().unwrap())
        .await
        .unwrap();
    for (result, lifecycle, reaped) in results {
        result.unwrap();
        lifecycle.unwrap();
        assert!(reaped);
    }
    eprintln!(
        "A09 capacity cleanup: all 64 PTYs reaped; all lifecycle writers completed; root removed"
    );
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

#[tokio::test]
async fn local_split_reliability_journal_failure_retains_original_target() {
    let (root, owner, request) = fixture().await;
    let service = owner.session_service.clone();
    let journal_path = root.path().join("machine-operations.v1.json");
    let failed_path = journal_path.clone();
    *service.workspace_service.transaction_probe.write() = Some(Arc::new(move |phase| {
        if phase == "sessionSpawned" {
            std::fs::remove_file(&failed_path).unwrap();
            std::fs::create_dir(&failed_path).unwrap();
        }
    }));
    let original = target();
    let result = std::panic::AssertUnwindSafe(async {
        let created = service.spawn_machine(request.clone(), "device".into(), "digest".into(), original.clone(),
            Instant::now() + Duration::from_secs(10), Arc::new(|| Ok(()))).await;
        assert_eq!(created.unwrap_err(), "OPERATION_OUTCOME_UNKNOWN");
        *service.workspace_service.transaction_probe.write() = None;
        assert!(service.terminal_service.get_session(&original.session_id).is_some());
        let retry = service.spawn_machine(request.clone(), "device".into(), "digest".into(), target(),
            Instant::now() + Duration::from_secs(10), Arc::new(|| Ok(()))).await;
        assert_eq!(retry.unwrap_err(), "OPERATION_OUTCOME_UNKNOWN");
        assert_eq!(service.terminal_service.list_sessions(), vec![original.session_id.clone()]);
        assert!(service.machine_capacity.lock().contains(&original.session_id));
    }).catch_unwind().await;
    *service.workspace_service.transaction_probe.write() = None;
    for id in service.terminal_service.list_sessions() {
        let mut lifecycle = service.machine_lifecycles.lock().get(&id).cloned();
        service.terminal_service.close_session(&id).await.unwrap();
        if let Some(ref mut lifecycle) = lifecycle {
            tokio::time::timeout(Duration::from_secs(5), async {
                while !*lifecycle.borrow() { lifecycle.changed().await.unwrap(); }
            }).await.unwrap();
        }
    }
    drop(service);
    drop(owner);
    crate::ipc::run_blocking(move || { root.close().unwrap(); Ok(()) }).await.unwrap();
    if let Err(panic) = result { std::panic::resume_unwind(panic); }
}

#[tokio::test]
async fn local_split_reliability_journal_begin_failure_has_no_child() {
    let (root, owner, request) = fixture().await;
    let service = owner.session_service.clone();
    let path = root.path().join("machine-operations.v1.json");
    *service.workspace_service.transaction_probe.write() = Some(Arc::new(move |phase| {
        if phase == "sessionBeforeIntent" { std::fs::create_dir(&path).unwrap(); }
    }));
    let outcome = service.spawn_machine(request, "device".into(), "digest".into(), target(),
        Instant::now() + Duration::from_secs(10), Arc::new(|| Ok(()))).await;
    *service.workspace_service.transaction_probe.write() = None;
    let sessions = service.terminal_service.list_sessions();
    let retained = service.machine_capacity.lock().len();
    drop(service);
    drop(owner);
    crate::ipc::run_blocking(move || { root.close().unwrap(); Ok(()) }).await.unwrap();
    assert_eq!(outcome.unwrap_err(), "OPERATION_OUTCOME_UNKNOWN");
    assert!(sessions.is_empty());
    assert_eq!(retained, 1);
}

#[tokio::test]
async fn local_split_reliability_journal_exit_failure_retains_uncertain_capacity() {
    let (root, owner, request) = fixture().await;
    let service = owner.session_service.clone();
    let id = service.spawn_machine(request, "device".into(), "digest".into(), target(),
        Instant::now() + Duration::from_secs(10), Arc::new(|| Ok(()))).await.unwrap();
    let path = root.path().join("machine-operations.v1.json");
    crate::ipc::run_blocking(move || { std::fs::remove_file(&path).unwrap(); std::fs::create_dir(path).unwrap(); Ok(()) }).await.unwrap();
    let mut lifecycle = service.machine_lifecycles.lock().get(&id).unwrap().clone();
    let pty = service.terminal_service.get_session(&id).unwrap();
    service.terminal_service.close_session(&id).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !*lifecycle.borrow() { lifecycle.changed().await.unwrap(); }
    }).await.unwrap();
    let retained = service.machine_capacity.lock().contains(&id);
    let metadata = service.session_metadata.read().contains_key(&id);
    let reaped = pty.is_reaped();
    drop(pty);
    drop(service);
    drop(owner);
    crate::ipc::run_blocking(move || { root.close().unwrap(); Ok(()) }).await.unwrap();
    assert!(retained && metadata && reaped);
}
