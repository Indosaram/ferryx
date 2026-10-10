use super::*;
use crate::{daemon::server::DaemonServer, remote::machine_protocol::*, scoped_contracts::Epoch};
use futures_util::FutureExt;

#[test]
fn served_session_cwd_falls_back_when_the_stored_value_is_probe_output() {
    let poisoned = std::path::PathBuf::from("cwd|rtd info error: No such file or directory");
    assert_eq!(
        serveable_local_cwd(&poisoned, Some("/repo".to_string())),
        Some("/repo".to_string())
    );
    assert_eq!(serveable_local_cwd(&poisoned, None), None);
    assert_eq!(
        serveable_local_cwd(std::path::Path::new("/repo/sub"), Some("/repo".to_string())),
        Some("/repo/sub".to_string())
    );
}

#[cfg(target_os = "macos")]
#[path = "session_service_crash_tests.rs"]
mod crash;

#[cfg(unix)]
#[tokio::test]
async fn provider_resume_child() {
    let Some(root) = std::env::var_os("A09_PRIVATE_AGENT_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let owner = DaemonServer::new_with_paths(Some(root.join("config")), Some(root.join("auth")));
    let service = owner.session_service.clone();
    let workspace = service
        .workspace_service
        .register_machine(root.join("project").to_str().unwrap())
        .unwrap();
    let mut request = request(workspace);
    request.startup = Startup::AgentResume {
        agent_type: "omo".into(),
        provider_session: crate::daemon::protocol::AgentProviderSession {
            key: crate::daemon::protocol::AgentProviderSessionKey::SessionId,
            id: "existing-provider-id".into(),
            transcript_path: None,
        },
    };
    let result = std::panic::AssertUnwindSafe(async {
        let id = service.spawn_machine(request.clone(), "device".into(), "digest".into(), target(), Instant::now() + Duration::from_secs(10), Arc::new(|| Ok(()))).await.unwrap();
        let (mut bytes, mut output) = service.terminal_service.attach(&id).unwrap();
        tokio::time::timeout(Duration::from_secs(10), async {
            while !String::from_utf8_lossy(&bytes).contains("A09_ARGV:--session:existing-provider-id:END") {
                bytes.extend(output.recv().await.unwrap());
            }
        }).await.unwrap();
        let mut duplicate = request.clone(); duplicate.request_id = uuid::Uuid::new_v4().to_string();
        let duplicate = service.spawn_machine(duplicate, "other-device".into(), "other-digest".into(), target(), Instant::now() + Duration::from_secs(10), Arc::new(|| Ok(()))).await;
        assert_eq!(duplicate.unwrap_err(), "AGENT_SESSION_CONFLICT");
        assert_eq!(service.terminal_service.list_sessions(), vec![id]);
        let mut invalid = request.clone(); invalid.request_id = uuid::Uuid::new_v4().to_string();
        if let Startup::AgentResume { provider_session, .. } = &mut invalid.startup { provider_session.key = crate::daemon::protocol::AgentProviderSessionKey::ConversationId; }
        assert_eq!(service.spawn_machine(invalid, "device".into(), "invalid".into(), target(), Instant::now() + Duration::from_secs(10), Arc::new(|| Ok(()))).await.unwrap_err(), "AGENT_RESUME_INVALID");
        eprintln!("A09 provider: remote executable fixture consumed --session/existing-provider-id; transcript CWD verified; cross-device claim conflict; typed key rejected; no generated provider ID");
    }).catch_unwind().await;
    for id in service.terminal_service.list_sessions() {
        service.terminal_service.close_session(&id).await.unwrap();
        service.wait_machine_lifecycle(&id).await.unwrap();
    }
    drop(service);
    drop(owner);
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

#[cfg(unix)]
#[tokio::test]
async fn provider_resume_uses_only_remote_validated_identity() {
    use std::os::unix::fs::PermissionsExt;
    let root = tokio::task::spawn_blocking(|| {
        let root = tempfile::tempdir().unwrap();
        for path in ["project", "bin", "omo/sessions/project"] { std::fs::create_dir_all(root.path().join(path)).unwrap(); }
        let script = root.path().join("bin/omo");
        std::fs::write(&script, "#!/bin/sh\nprintf 'A09_ARGV:%s:%s:END\\n' \"$1\" \"$2\"\nexec /bin/cat\n").unwrap();
        std::fs::set_permissions(script, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(root.path().join("omo/sessions/project/fixture_existing-provider-id.jsonl"), serde_json::json!({"type":"session","id":"existing-provider-id","cwd":std::fs::canonicalize(root.path().join("project")).unwrap()}).to_string()).unwrap();
        root
    }).await.unwrap();
    let mut child = tokio::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "daemon::session_service::machine_tests::provider_resume_child",
            "--nocapture",
        ])
        .env("A09_PRIVATE_AGENT_ROOT", root.path())
        .env("OMO_CODING_AGENT_DIR", root.path().join("omo"))
        .env("HOME", root.path())
        .env("SHELL", "/bin/sh")
        .env(
            "PATH",
            std::env::join_paths([
                root.path().join("bin"),
                PathBuf::from("/usr/bin"),
                PathBuf::from("/bin"),
            ])
            .unwrap(),
        )
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(30), child.wait()).await;
    if result.is_err() {
        child.kill().await.unwrap();
        child.wait().await.unwrap();
    }
    tokio::task::spawn_blocking(move || root.close().unwrap())
        .await
        .unwrap();
    assert!(result.unwrap().unwrap().success());
    eprintln!("A09 provider fixture cleanup: private child waited; PTY lifecycle joined; private executable/transcript/root removed");
}

fn request(workspace: String) -> CreateSessionRequest {
    CreateSessionRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        workspace_id: workspace,
        worktree: None,
        cols: 80,
        rows: 24,
        inherit_from_session_id: None,
        cwd_relative: None,
        startup: Startup::Shell,
    }
}
fn target() -> RemoteTerminalTarget {
    RemoteTerminalTarget {
        machine_id: "private-machine".into(),
        daemon_epoch: Epoch(123),
        session_id: uuid::Uuid::new_v4().to_string(),
    }
}
async fn fixture() -> (tempfile::TempDir, DaemonServer, CreateSessionRequest) {
    tokio::task::spawn_blocking(|| {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("project")).unwrap();
        let owner = DaemonServer::new_with_paths(
            Some(root.path().join("config")),
            Some(root.path().join("auth")),
        );
        let workspace = owner
            .session_service
            .workspace_service
            .register_machine(root.path().join("project").to_str().unwrap())
            .unwrap();
        (root, owner, request(workspace))
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn queued_revocation_rechecks_before_journal_or_pty() {
    let (root, owner, request) = fixture().await;
    let service = owner.session_service.clone();
    let gate = service.workspace_service.worktree_gate(&request.workspace_id);
    let held = gate.lock();
    let (queued, observed) = tokio::sync::oneshot::channel();
    let queued = Mutex::new(Some(queued));
    *service.workspace_service.transaction_probe.write() = Some(Arc::new(move |phase| {
        if phase == "sessionBeforeWorktreeGate" {
            if let Some(tx) = queued.lock().take() {
                tx.send(()).unwrap();
            }
        }
    }));
    let (grant, revoked) = tokio::sync::watch::channel(false);
    let check = Arc::new(move || {
        if *revoked.borrow() {
            Err("UNAUTHORIZED".into())
        } else {
            Ok(())
        }
    });
    let worker_service = service.clone();
    let request_id = request.request_id.clone();
    let worker = tokio::spawn(async move {
        worker_service
            .spawn_machine(
                request,
                "device".into(),
                "digest".into(),
                target(),
                Instant::now() + Duration::from_secs(10),
                check,
            )
            .await
    });
    let observed = tokio::time::timeout(Duration::from_secs(5), observed).await;
    grant.send(true).unwrap();
    drop(held);
    let result = tokio::time::timeout(Duration::from_secs(10), worker)
        .await
        .unwrap()
        .unwrap();
    *service.workspace_service.transaction_probe.write() = None;
    let journal = service
        .workspace_service
        .journal
        .reconcile("device", &request_id)
        .unwrap();
    let sessions = service.terminal_service.list_sessions();
    drop(service);
    drop(owner);
    tokio::task::spawn_blocking(move || root.close().unwrap())
        .await
        .unwrap();
    observed.unwrap().unwrap();
    assert_eq!(result.unwrap_err(), "UNAUTHORIZED");
    assert!(journal.is_none());
    assert!(sessions.is_empty());
    eprintln!("A09 queued revocation: subscribed admission, no intent, no PTY, root removed");
}

#[tokio::test]
async fn dropped_http_reply_replays_original_process_and_controller_fences_close() {
    use tokio::io::AsyncWriteExt;
    let (root, owner, request) = fixture().await;
    let state = owner.remote_state().clone();
    let service = owner.session_service.clone();
    let pin = state
        .auth_manager
        .create_scoped_pairing_code(
            crate::remote::DevicePermission::Control,
            crate::remote::DeviceAccessScope::Machine,
        )
        .unwrap();
    let (token, device) = state
        .auth_manager
        .exchange_pairing_code(&pin, "lost-reply")
        .unwrap();
    let (committed, observed) = tokio::sync::oneshot::channel();
    let committed = Mutex::new(Some(committed));
    let (release, released) = std::sync::mpsc::channel();
    let released = Mutex::new(released);
    let mut release = Some(release);
    *service.workspace_service.transaction_probe.write() = Some(Arc::new(move |phase| {
        if phase == "sessionBeforeResponse" {
            if let Some(committed) = committed.lock().take() {
                committed.send(()).unwrap();
                released
                    .lock()
                    .recv_timeout(Duration::from_secs(10))
                    .unwrap();
            }
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let router = crate::remote::server::create_remote_router(state.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    let result = std::panic::AssertUnwindSafe(async {
        let body = serde_json::to_string(&request).unwrap();
        let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
        socket.write_all(format!("POST /api/v1/sessions HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {token}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        tokio::time::timeout(Duration::from_secs(10), observed).await.unwrap().unwrap();
        let record = service.workspace_service.journal.reconcile(&device.id, &request.request_id).unwrap().unwrap();
        let Operation::Completed { outcome: OperationOutcome::Session { session }, .. } = record.operation else { panic!("not committed"); };
        let pty = service.terminal_service.get_session(&session.target.session_id).unwrap();
        let original_pid = pty.pid();
        // The server is held before response serialization; no response bytes are consumed.
        drop(socket);
        release.take().unwrap().send(()).unwrap();
        let replay = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(10)).build().unwrap()
            .post(format!("http://{address}/api/v1/sessions")).bearer_auth(&token).body(body).send().await.unwrap();
        assert_eq!(replay.status().as_u16(), 201);
        let replay: Session = serde_json::from_str(&replay.text().await.unwrap()).unwrap();
        assert_eq!(replay, session);
        assert_eq!(service.terminal_service.list_sessions(), vec![session.target.session_id.clone()]);
        assert_eq!(service.terminal_service.get_session(&session.target.session_id).unwrap().pid(), original_pid);
        service.machine_controllers.lock().await.insert(session.target.session_id.clone(), "another-controller".into());
        let close_id = uuid::Uuid::new_v4().to_string();
        assert_eq!(service.close_machine(&device.id, &close_id, "close", &session.target.session_id, session.target.daemon_epoch, session.target.daemon_epoch, Arc::new(|| Ok(()))).await.unwrap_err(), "CONTROL_CONFLICT");
        assert!(service.workspace_service.journal.reconcile(&device.id, &close_id).unwrap().is_none());
        service.machine_controllers.lock().await.clear();
        eprintln!("A09 dropped actual HTTP reply: target={} original_pid={original_pid:?} same-PID-replay=true controller-conflict-before-intent=true", session.target.session_id);
    }).catch_unwind().await;
    if let Some(release) = release {
        let _ = release.send(());
    }
    *service.workspace_service.transaction_probe.write() = None;
    for id in service.terminal_service.list_sessions() {
        service.terminal_service.close_session(&id).await.unwrap();
        service.wait_machine_lifecycle(&id).await.unwrap();
    }
    stop.send(()).unwrap();
    server.await.unwrap();
    drop(state);
    drop(service);
    drop(owner);
    tokio::task::spawn_blocking(move || root.close().unwrap())
        .await
        .unwrap();
    eprintln!("A09 lost-reply cleanup: listener joined; private PTY reaped; lifecycle writer joined; root removed");
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

#[tokio::test]
async fn interrupted_spawn_never_repeats_preallocated_intent() {
    for phase in ["sessionIntent", "sessionSpawned"] {
        let (root, owner, request) = fixture().await;
        let service = owner.session_service.clone();
        let target = target();
        *service.workspace_service.transaction_probe.write() = Some(Arc::new(move |event| {
            if event == phase {
                panic!("A09 controlled interruption at {phase}");
            }
        }));
        let result = std::panic::AssertUnwindSafe(async {
            let result = service.spawn_machine(request.clone(), "device".into(), "digest".into(), target.clone(), Instant::now() + Duration::from_secs(10), Arc::new(|| Ok(()))).await;
            assert!(result.is_err());
            *service.workspace_service.transaction_probe.write() = None;
            let record = service.workspace_service.journal.reconcile("device", &request.request_id).unwrap().unwrap();
            assert_eq!(serde_json::from_str::<RemoteTerminalTarget>(&record.resource).unwrap(), target);
            assert!(matches!(record.operation, Operation::OutcomeUnknown { .. }));
            let pty = service.terminal_service.get_session(&target.session_id);
            assert_eq!(pty.is_some(), phase == "sessionSpawned");
            let pid = pty.as_ref().and_then(|p| p.pid());
            let retried = service.spawn_machine(request.clone(), "device".into(), "digest".into(), super::machine_tests::target(), Instant::now() + Duration::from_secs(10), Arc::new(|| Ok(()))).await;
            assert_eq!(retried.unwrap_err(), "OPERATION_OUTCOME_UNKNOWN");
            assert_eq!(service.terminal_service.list_sessions().len(), usize::from(pty.is_some()));
            assert_eq!(service.terminal_service.get_session(&target.session_id).and_then(|p| p.pid()), pid);
            assert!(service.machine_only(&target.session_id));
            eprintln!("A09 interruption={phase} target={} original_pid={pid:?} replay=outcomeUnknown no-repeat=true", target.session_id);
        }).catch_unwind().await;
        *service.workspace_service.transaction_probe.write() = None;
        for id in service.terminal_service.list_sessions() {
            let pty = service.terminal_service.get_session(&id).unwrap();
            service.terminal_service.close_session(&id).await.unwrap();
            assert!(pty.is_reaped());
        }
        drop(service);
        drop(owner);
        tokio::task::spawn_blocking(move || root.close().unwrap())
            .await
            .unwrap();
        eprintln!("A09 interrupted owner cleanup: every owned PTY reaped and private root removed");
        if let Err(panic) = result {
            std::panic::resume_unwind(panic);
        }
    }
}

#[tokio::test]
async fn machine_mutation_slots_are_shared_and_bounded() {
    let (root, owner, _) = fixture().await;
    let state = owner.remote_state().clone();
    let pin = state
        .auth_manager
        .create_scoped_pairing_code(
            crate::remote::DevicePermission::Control,
            crate::remote::DeviceAccessScope::Machine,
        )
        .unwrap();
    let (token, _) = state
        .auth_manager
        .exchange_pairing_code(&pin, "slots")
        .unwrap();
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("authorization", format!("Bearer {token}").parse().unwrap());
    let mut slots = Vec::new();
    for _ in 0..8 {
        slots.push(
            owner
                .session_service
                .workspace_service
                .project_mutations
                .clone()
                .try_acquire_owned()
                .unwrap(),
        );
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let router = crate::remote::server::create_remote_router(state.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    let ninth = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap()
        .post(format!("http://{address}/api/v1/sessions"))
        .headers(headers)
        .body("{}")
        .send()
        .await;
    stop.send(()).unwrap();
    server.await.unwrap();
    assert_eq!(ninth.unwrap().status().as_u16(), 429);
    drop(slots);
    drop(state);
    drop(owner);
    tokio::task::spawn_blocking(move || root.close().unwrap())
        .await
        .unwrap();
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
async fn machine_session_capacity_limit_is_configurable() {
    let (_root, owner, _template) = fixture().await;
    let service = owner.session_service.clone();

    // Default in test mode is 64
    std::env::remove_var("FERRYX_MAX_MACHINE_SESSIONS");
    assert_eq!(service.max_machine_sessions(), 64);

    // Configurable via environment variable
    std::env::set_var("FERRYX_MAX_MACHINE_SESSIONS", "128");
    assert_eq!(service.max_machine_sessions(), 128);

    std::env::set_var("FERRYX_MAX_MACHINE_SESSIONS", "256");
    assert_eq!(service.max_machine_sessions(), 256);

    std::env::remove_var("FERRYX_MAX_MACHINE_SESSIONS");
    assert_eq!(service.max_machine_sessions(), 64);
}

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

#[tokio::test]
async fn desktop_gui_session_inventory_and_attach_validation() {
    eprintln!("[inventory_test] start");
    let (root, owner, request) = desktop_fixture().await;
    eprintln!("[inventory_test] fixture ready");
    let service = owner.session_service.clone();
    let epoch = Epoch(service.epoch);

    // Spawn a desktop GUI session (machine is None)
    eprintln!("[inventory_test] admit_spawn");
    let session_id = service
        .admit_spawn(
            "desktop-gui-req-1",
            &request.workspace_id,
            None,
            None,
            80,
            24,
            None,
            None,
            None,
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
    assert_eq!(found.target.machine_id, *service.machine_id.as_ref().unwrap());
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
        machine_id: service.machine_id.as_ref().unwrap().clone(),
        daemon_epoch: epoch,
        session_id: ssh_session_id.clone(),
    };
    let ssh_err = service.validate_machine_target(&ssh_target).await.unwrap_err();
    assert_eq!(ssh_err, "SESSION_OWNERSHIP_CHANGED");

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

#[tokio::test]
async fn desktop_gui_session_managed_worktree_projection() {
    eprintln!("[managed_test] start");
    let (root, owner, request) = desktop_fixture().await;
    eprintln!("[managed_test] fixture ready");
    let service = owner.session_service.clone();
    let epoch = Epoch(service.epoch);

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

#[tokio::test]
async fn desktop_gui_session_jail_and_missing_path_rejections() {
    eprintln!("[jail_test] start");
    let (root, owner, request) = desktop_fixture().await;
    eprintln!("[jail_test] fixture ready");
    let service = owner.session_service.clone();
    let epoch = Epoch(service.epoch);

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
        machine_id: service.machine_id.as_ref().unwrap().clone(),
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
        machine_id: service.machine_id.as_ref().unwrap().clone(),
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

#[tokio::test]
async fn desktop_gui_session_lifecycle_machine_events() {
    eprintln!("[events_test] start");
    let (root, owner, request) = desktop_fixture().await;
    eprintln!("[events_test] fixture ready");
    let service = owner.session_service.clone();
    let epoch = Epoch(service.epoch);

    // Subscribe to machine_events BEFORE triggering spawn
    let mut events_rx = service.workspace_service.machine_events.subscribe();

    // 1. Spawn desktop GUI session
    eprintln!("[events_test] admit_spawn");
    let session_id = service
        .admit_spawn(
            "desktop-gui-event-req",
            &request.workspace_id,
            None,
            None,
            80,
            24,
            None,
            None,
            None,
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
