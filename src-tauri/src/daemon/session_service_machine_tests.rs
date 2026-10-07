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
    // A plausible absolute cwd is served verbatim rather than replaced by the worktree path. The
    // path is built from the running platform's own root, because `Path::is_absolute` on Windows
    // requires a prefix: a POSIX-shaped `/repo/sub` is rooted but NOT absolute there, so falling
    // back to the worktree path is correct behavior this assertion would otherwise misread as a
    // defect in `serveable_local_cwd`.
    let worktree = if cfg!(windows) { r"C:\repo" } else { "/repo" };
    let nested = std::path::Path::new(worktree).join("sub");
    assert_eq!(
        serveable_local_cwd(&nested, Some(worktree.to_string())),
        Some(nested.to_string_lossy().into_owned())
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
    let held = service.spawn_lock.clone().lock_owned().await;
    let (queued, observed) = tokio::sync::oneshot::channel();
    let queued = Mutex::new(Some(queued));
    *service.workspace_service.transaction_probe.write() = Some(Arc::new(move |phase| {
        if phase == "sessionBeforeSpawnGate" {
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
        let pty = service.terminal_service.get_session(&id);
        service.terminal_service.close_session(&id).await.unwrap();
        // The lifecycle signal is sent only after the PTY exit, the metadata task and the exit
        // record. The bounded wait still fails the test, but it now reports which of those the
        // session shows, so the next run names the stalled stage instead of only its timeout.
        if let Err(error) = service.wait_machine_lifecycle(&id).await {
            let reaped = pty.as_ref().is_some_and(|session| session.is_reaped());
            let exit_recorded = service
                .workspace_service
                .journal
                .session(&id)
                .ok()
                .flatten()
                .is_some_and(|record| record.exit.is_some());
            // Two flags with opposite directions, reported side by side so neither is read as the
            // other: `reader_finished` is read on the PTY session and says the reader thread has
            // not finished - a prediction, never a location, since a false flag cannot say where
            // the thread waits. `hub_holds_session` is read on the output hub and says the pump
            // has not drained the entry. Nothing here reports the metadata task, which has no
            // accessor, so a stall cannot be placed before or after it from these fields alone.
            let reader_finished = pty
                .as_ref()
                .is_some_and(|session| session.is_reader_finished());
            let hub_holds_session = service.terminal_service.output_hub().has_session(&id);
            // The reader's own phase is the locator the boolean cannot be: a parked thread reports
            // the phase it was entering when it stopped (before-read / before-send), which names the
            // blocking call instead of only proving the thread had not finished.
            let reader_phase = pty
                .as_ref()
                .map_or("no-session", |session| session.reader_phase());
            panic!(
                "wait_machine_lifecycle failed for {id}: {error}; pty_reaped={reaped}; exit_recorded={exit_recorded}; reader_finished={reader_finished}; reader_phase={reader_phase}; hub_holds_session={hub_holds_session}"
            );
        }
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
        for _ in 0..64 {
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

/// Closing a session must end its reader thread, and the reader's own phase must reach `finished`.
///
/// This is the close-completion regression for the Windows stall: the wait is the existing
/// `wait_machine_lifecycle` bound (an event, never a sleep) and the failure message carries the
/// reader's phase, so a reader that parks names the blocking call instead of only timing out.
/// It is expected to fail on a host whose reader cannot be stopped - that failure is the evidence,
/// not a reason to omit the coverage.
#[tokio::test]
async fn closing_a_machine_session_ends_its_reader() {
    let (root, owner, template) = fixture().await;
    let service = owner.session_service.clone();
    let mut request = template.clone();
    request.request_id = uuid::Uuid::new_v4().to_string();
    let machine_target = target();
    let session_id = machine_target.session_id.clone();
    let spawned = service
        .spawn_machine(
            request,
            "device".into(),
            "digest".into(),
            machine_target,
            Instant::now() + Duration::from_secs(30),
            Arc::new(|| Ok(())),
        )
        .await
        .unwrap();
    assert_eq!(spawned, session_id);
    let pty = service
        .terminal_service
        .get_session(&session_id)
        .expect("the session this test spawned is live");

    service
        .terminal_service
        .close_session(&session_id)
        .await
        .unwrap();
    let lifecycle = service.wait_machine_lifecycle(&session_id).await;

    assert!(
        lifecycle.is_ok(),
        "closing {session_id} never completed its lifecycle; the reader is in phase {} and \
         pty_reaped={} - a parked reader holds the output sender, so the pump keeps the lifecycle \
         sender and the exit record is never written",
        pty.reader_phase(),
        pty.is_reaped()
    );
    assert_eq!(
        pty.reader_phase(),
        "finished",
        "the reader must have finished once the session was closed"
    );
    eprintln!(
        "A09 close completion: session={session_id} reader_phase={} reaped={}",
        pty.reader_phase(),
        pty.is_reaped()
    );
    drop(pty);
    drop(service);
    drop(owner);
    // The temp root is removed off the runtime, as every other fixture teardown in this file does.
    tokio::task::spawn_blocking(move || root.close().unwrap())
        .await
        .unwrap();
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

// ---------------------------------------------------------------------------------------------
// F1: desktop GUI session projection (project_desktop_gui_session).
//
// The projector resolves this machine's durable identity through canonical_identity_dir(), which
// reads process environment (FERRYX_DATA_DIR, else HOME), and DaemonSessionService exposes no
// injection seam for it. These cases therefore run in a child test process whose identity
// environment points at a private fixture - the same shape as
// provider_resume_uses_only_remote_validated_identity - so the parent process never reads or
// creates the real machine identity, and no process-global env is mutated. Every assertion runs
// against the real projector, the real workspace registry/catalog and a real PTY; nothing is
// mocked away and production semantics are unchanged.
//
// Known gap: the remote/SSH branch of the projector (served from RemoteRuntime::details) is not
// covered here. TerminalService builds its RemoteRuntime with the real SshConnector and exposes
// no seam to seed a session entry, so an authentic no-network SSH fixture does not exist under
// A bound that expires signals the recorded pid only after its kernel image is proved to be
// the launched binary, and a drain a descendant keeps open is cut short with the captured
// stage text preserved; neither outcome is reported as a success it did not observe.

const PROJECTOR_CHILD_ROOT: &str = "FERRYX_PROJECTOR_CHILD_ROOT";

/// Spawns this test binary again for one scenario. The path is passed in rather than
/// re-derived, so the path a teardown verifies is provably the one that was launched.
fn projector_child(
    name: &str,
    root: &std::path::Path,
    launched: &std::path::Path,
) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(launched);
    command
        .args(["--exact", name, "--nocapture"])
        .env(PROJECTOR_CHILD_ROOT, root)
        .env("FERRYX_DATA_DIR", root.join("data"))
        .env("FERRYX_RUNTIME_DIR", root.join("runtime"))
        .env("HOME", root)
        .env("USERPROFILE", root)
        .kill_on_drop(true);
    #[cfg(unix)]
    command.env("SHELL", "/bin/sh");
    command
}

/// Bound for one child scenario. A blocked child must fail this parent naming the stage it
/// reported, not hang the suite; a bound is a bound, never a sleep or a poll.

const PROJECTOR_CHILD_BOUND: Duration = Duration::from_secs(90);

/// Bounds for the two waits a blocked child forces. Neither may be unbounded, and neither failure
/// may be reported as a success.
const PROJECTOR_CLEANUP_BOUND: Duration = Duration::from_secs(10);
const PROJECTOR_DRAIN_BOUND: Duration = Duration::from_secs(10);
/// Bound for waiting on the PTY reader tasks a close must end before the child may exit.
const PROJECTOR_READER_BOUND: Duration = Duration::from_secs(10);

/// Stage markers the child prints and flushes, so a blocked child names the phase it did not
/// complete instead of leaving the next reader to guess at ConPTY or a lock. The markers are
/// printed by the shared helpers (fixture, spawn, teardown), so the last one printed is the
/// phase that did not return: a projection blocked after spawn reports the spawn marker.
fn projector_child_stage(stage: &str) {
    use std::io::Write;
    println!("PROJECTOR-STAGE {stage}");
    let _ = std::io::stdout().flush();
}

fn projector_child_last_stage(combined: &str) -> String {
    combined
        .lines()
        .filter_map(|line| {
            line.split_once("PROJECTOR-STAGE ")

                .map(|(_, stage)| stage.trim().to_owned())
        })
        .next_back()
        .unwrap_or_else(|| "none".to_owned())
}

/// The kernel's image path for a live pid: the identity a teardown compares before it signals.
///
/// Linux reads the `/proc` link the crate already uses for process paths; macOS uses
/// `proc_pidpath`, the kernel-resolved image `manual_ssh` reads; Windows uses
/// `QueryFullProcessImageNameW` through the crate's own kernel32 declarations. A target with no
/// image lookup returns nothing, so its teardown refuses to signal rather than guessing.
#[cfg(target_os = "linux")]
fn process_image_path(pid: u32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/exe")).ok()
}

#[cfg(target_os = "macos")]
fn process_image_path(pid: u32) -> Option<PathBuf> {
    // PROC_PIDPATHINFO_MAXSIZE (4 * MAXPATHLEN).
    let mut buffer = vec![0u8; 4 * 1024];
    let len = unsafe {
        libc::proc_pidpath(
            pid as libc::pid_t,
            buffer.as_mut_ptr().cast(),
            buffer.len() as u32,
        )
    };
    if len <= 0 {
        return None;
    }
    buffer.truncate(len as usize);
    String::from_utf8(buffer).ok().map(PathBuf::from)
}

/// The same raw kernel32 declarations `ipc::windows_process_cwd` and the PTY suspend path use:
/// `windows-sys` is pulled with a narrow feature list, so a process API is reached through this
/// crate's established extern block instead of widening the manifest.
#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
    fn QueryFullProcessImageNameW(
        process: *mut std::ffi::c_void,
        flags: u32,
        name: *mut u16,
        size: *mut u32,
    ) -> i32;
    fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
}

/// Owns the process handle so every early return closes it exactly once.
#[cfg(windows)]
struct ProjectorProcessHandle(*mut std::ffi::c_void);

#[cfg(windows)]
impl Drop for ProjectorProcessHandle {
    fn drop(&mut self) {
        // SAFETY: owned non-null handle from OpenProcess; closed exactly once.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

/// The image path of a live Windows process, as the kernel reports it.
#[cfg(windows)]
fn process_image_path(pid: u32) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    // PROCESS_QUERY_LIMITED_INFORMATION is the documented minimum for the image name.
    // SAFETY: scalar inputs, no borrowed buffers; the returned handle is owned.
    let raw = unsafe { OpenProcess(0x1000, 0, pid) };
    if raw.is_null() {
        return None;
    }
    let handle = ProjectorProcessHandle(raw);
    let mut buffer = vec![0u16; 4 * 1024];
    let mut size = u32::try_from(buffer.len()).ok()?;
    // FORMAT_WIN32 (0) yields the drive-letter path `current_exe` also reports, so the two are
    // comparable without a device-path translation step.
    // SAFETY: the buffer is initialized and writable and holds exactly `size` WCHARs; the API
    // updates `size` in place with the number of WCHARs written.
    let ok = unsafe { QueryFullProcessImageNameW(handle.0, 0, buffer.as_mut_ptr(), &mut size) };
    if ok == 0 {
        return None;
    }
    buffer.truncate(usize::try_from(size).ok()?);
    Some(PathBuf::from(std::ffi::OsString::from_wide(&buffer)))
}

/// Targets with no image lookup here (BSD and others) refuse to signal rather than guess.
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn process_image_path(_pid: u32) -> Option<PathBuf> {
    None
}

fn same_executable(image: &std::path::Path, launched: &std::path::Path) -> bool {
    let image = std::fs::canonicalize(image).unwrap_or_else(|_| image.to_path_buf());
    let launched = std::fs::canonicalize(launched).unwrap_or_else(|_| launched.to_path_buf());
    image == launched
}

/// Signalled teardown for a child that exceeded its bound, returning a receipt of what it did.
///
/// The pid is the one `spawn` returned and the process is still unreaped, so the kernel cannot
/// have handed it to anyone else; on top of that the pid's kernel image must equal the binary
/// this parent launched before any signal is sent. Matching a process by name, command line or
/// any other pattern is not done here, and when the image cannot be read nothing is signalled and
/// the refusal is reported for a human to resolve.
fn terminate_projector_child(
    child: &mut tokio::process::Child,
    launched: &std::path::Path,
    pid: Option<u32>,
) -> String {
    let Some(pid) = pid else {
        return "no pid was recorded at spawn: nothing signalled".to_owned();
    };
    match process_image_path(pid) {
        Some(image) if same_executable(&image, launched) => match child.start_kill() {
            Ok(()) => format!(
                "signalled pid {pid} after verifying its image {}",
                image.display()
            ),
            Err(error) => format!("signalling the verified pid {pid} failed: {error}"),
        },
        Some(image) => format!(
            "refused to signal pid {pid}: its image {} is not the launched {}",
            image.display(),
            launched.display()
        ),
        None => format!("refused to signal pid {pid}: its image could not be read"),
    }
}

/// Reads a child pipe to EOF into a shared buffer.
///
/// The buffer is shared instead of returned, so a parent that stops waiting for the drain still
/// keeps every byte that arrived - including the stage marker that names a blocked phase.
fn drain_projector_pipe(
    mut pipe: impl tokio::io::AsyncRead + Unpin + Send + 'static,
    buffer: Arc<std::sync::Mutex<Vec<u8>>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        let mut chunk = [0u8; 8192];
        loop {
            match pipe.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    if let Ok(mut buffer) = buffer.lock() {
                        buffer.extend_from_slice(&chunk[..read]);
                    }
                }
            }
        }
    })
}

/// Runs one projector case in a private child process and proves the case actually ran.
///
/// The child is awaited under a bound while both of its pipes are drained concurrently, so a
/// child that blocks cannot deadlock against a parent that reads only after exit and cannot hang
/// the suite. A child that exceeds the bound is signalled only after its pid is proved to be the
/// binary this parent launched, and no teardown outcome is reported as a success it did not
/// observe.
async fn projector_child_case(name: &str) {
    let launched = std::env::current_exe().unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut child = projector_child(name, root.path(), &launched)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let pid = child.id();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    // Drain both pipes into shared buffers while the child runs: a child that fills a pipe buffer
    // cannot deadlock against a parent that reads only after it exits, and a descendant that
    // inherited a pipe can leave the drain unfinished without taking the captured text with it.
    let stdout_buffer = Arc::new(std::sync::Mutex::new(Vec::new()));
    let stderr_buffer = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut stdout_task = drain_projector_pipe(stdout, Arc::clone(&stdout_buffer));
    let mut stderr_task = drain_projector_pipe(stderr, Arc::clone(&stderr_buffer));
    let waited = tokio::time::timeout(PROJECTOR_CHILD_BOUND, child.wait()).await;
    let timed_out = waited.is_err();
    let teardown = if timed_out {
        terminate_projector_child(&mut child, &launched, pid)
    } else {
        "not needed: the child exited inside the bound".to_owned()
    };
    // The wait after a signal is bounded too, and a child still unreaped after it is reported as
    // unreaped rather than reaped: this parent only claims a cleanup it actually observed. Both
    // arms keep the wait's own result, so a wait that failed is reported as that failure instead
    // of being flattened into a timeout that never happened.
    let reaped = match waited {
        Ok(status) => Some(status),
        Err(_) => tokio::time::timeout(PROJECTOR_CLEANUP_BOUND, child.wait()).await.ok(),
    };
    let drained = tokio::time::timeout(PROJECTOR_DRAIN_BOUND, async {
        let _ = (&mut stdout_task).await;
        let _ = (&mut stderr_task).await;
    })
    .await
    .is_ok();
    if !drained {
        stdout_task.abort();
        stderr_task.abort();
    }
    // Read after the drain attempt, so a drain cut short still keeps the bytes that arrived.
    let stdout = String::from_utf8_lossy(&stdout_buffer.lock().unwrap()).into_owned();
    let stderr = String::from_utf8_lossy(&stderr_buffer.lock().unwrap()).into_owned();
    let combined = format!("{stdout}{stderr}");
    let stage = projector_child_last_stage(&combined);
    let receipt = root.path().to_owned();
    root.close().unwrap();
    let cleanup = match &reaped {
        None => "CLEANUP FAILED: still unreaped after the bounded wait".to_owned(),
        Some(Err(error)) => format!("CLEANUP FAILED: the bounded wait failed: {error}"),
        Some(Ok(_)) if !drained => {
            "drain cut short: a descendant still held the pipes, captured text preserved".to_owned()
        }
        Some(Ok(_)) => "reaped and drained".to_owned(),
    };
    // Distinguish a scenario stall from a shutdown stall: the child prints its scenario sentinel
    // only after its assertions, so a sentinel plus a timeout means the body finished and the
    // process did not exit - the stage the earlier revision could not name.
    let body = if combined.contains("F1-PROJECTOR") && !combined.contains("F1-PROJECTOR-SKIPPED")
    {
        "body completed, the process did not exit"
    } else {
        "body did not complete"
    };
    assert!(
        !timed_out,
        "{name} (pid {pid:?}) exceeded {PROJECTOR_CHILD_BOUND:?}; last stage={stage}; body={body}; teardown={teardown}; cleanup={cleanup}; stdout={stdout:?} stderr={stderr:?}"
    );
    // The exit must be observed, not assumed: a wait that failed and a wait that never returned
    // are each reported as themselves, so neither can be read as a pass.
    let status = match reaped {
        Some(Ok(status)) => status,
        Some(Err(error)) => {
            panic!("{name}: waiting for the child (pid {pid:?}) failed: {error}")
        }
        None => panic!(
            "{name}: the child (pid {pid:?}) was not observed to exit inside the bounded wait"
        ),
    };
    assert!(
        status.success(),
        "{name} failed: stdout={stdout:?} stderr={stderr:?}"
    );

    // Exact filter, count and status: the name must select exactly this one child and it must
    // pass, so a mistyped name - which reports "0 passed" and still exits 0 - cannot pass here.
    assert!(
        combined.contains("test result: ok. 1 passed; 0 failed"),
        "{name} did not run as exactly one passing test: stdout={stdout:?} stderr={stderr:?}"
    );
    // The child must have reached its own projector assertion instead of taking the standalone
    // no-op return, so a body that never ran can never satisfy this parent.
    assert!(
        combined.contains("F1-PROJECTOR"),
        "{name} never reached the projector assertion: stdout={stdout:?} stderr={stderr:?}"
    );
    assert!(
        !combined.contains("F1-PROJECTOR-SKIPPED"),
        "{name} skipped its scenario body: stdout={stdout:?} stderr={stderr:?}"
    );
    eprintln!(
        "F1-PROJECTOR parent {name}: one test passed, last stage={stage}, cleanup={cleanup}, private root={} removed={}",
        receipt.display(),
        !receipt.exists()
    );
}


/// Child-mode sentinel: a child body runs only when the parent armed this root, so selecting the
/// child directly or running the full suite is a no-op instead of a failure - the standalone
/// handling `provider_resume_child` already uses. An unarmed child never reaches the fixture, so
/// it never reads or creates the real machine identity.
fn projector_child_root_if_armed() -> Option<PathBuf> {
    std::env::var_os(PROJECTOR_CHILD_ROOT).map(PathBuf::from)
}

fn projector_child_standalone_return() {
    eprintln!("F1-PROJECTOR-SKIPPED {PROJECTOR_CHILD_ROOT} unset");
}

/// Private fixture: a registered plain workspace plus a daemon whose durable identity, catalog and
/// session metadata all live inside the child's own root.
async fn projector_child_fixture() -> Option<(PathBuf, DaemonServer, Arc<DaemonSessionService>, PathBuf)> {
    let root = projector_child_root_if_armed()?;
    projector_child_stage("fixture");
    // Armed means the parent aimed the whole identity surface at a private fixture, so refuse a
    // root that could let the projector read or create the real machine identity.
    assert!(
        root.is_absolute(),
        "projector child root must be absolute: {root:?}"
    );
    assert!(
        std::env::var_os("FERRYX_DATA_DIR")
            .is_some_and(|data| PathBuf::from(data).starts_with(&root)),
        "projector child must run with FERRYX_DATA_DIR inside its private root: {root:?}"
    );
    let project = root.join("project");
    std::fs::create_dir_all(project.join("sub")).unwrap();
    let owner = DaemonServer::new_with_paths(
        Some(root.join("data/config")),
        Some(root.join("data/auth")),
    );
    let service = owner.session_service.clone();
    service
        .workspace_service
        .register("projector-ws", project.to_str().unwrap())
        .unwrap();

    Some((root, owner, service, project))
}

async fn projector_child_spawn(
    service: &Arc<DaemonSessionService>,
    request_id: &str,
    cwd: Option<String>,
    cols: u16,
    rows: u16,
) -> String {
    projector_child_stage("spawn");
    service
        .handle_spawn(request_id, "projector-ws", None, cwd, cols, rows, None, None, None)
        .await
        .unwrap()
}

/// The reader phase of a child's session at the moment the body finished, for the sentinel.
///
/// Diagnostic only: a body that returns while its reader is still in `before-read` names the
/// blocking call the process is stuck on, instead of only proving the thread had not finished.
fn projector_child_reader_phase(service: &Arc<DaemonSessionService>, id: &str) -> &'static str {
    service
        .terminal_service
        .get_session(id)
        .map_or("no-session", |session| session.reader_phase())
}

async fn projector_child_cleanup(service: &Arc<DaemonSessionService>) {
    projector_child_stage("teardown");
    // Hold each session so its reader state can still be read once the close removes it.
    let readers: Vec<_> = service
        .terminal_service
        .list_sessions()
        .into_iter()
        .filter_map(|id| service.terminal_service.get_session(&id))
        .collect();
    for id in service.terminal_service.list_sessions() {
        service.terminal_service.close_session(&id).await.unwrap();
    }
    // A PTY reader is a blocking task, and blocking tasks cannot be aborted - `PtySession::drop`
    // calls `abort()` on one that is already running, which does nothing. A reader still parked
    // in its blocking read therefore survives the close, keeps the runtime's blocking pool busy,
    // and is invisible until the runtime shuts down after the test body. Wait for the readers
    // under a bound and name which happened, so a post-body stall reports its own cause.
    let deadline = Instant::now() + PROJECTOR_READER_BOUND;
    loop {
        if readers.iter().all(|session| session.is_reader_finished()) {
            projector_child_stage("reader-finished");
            break;
        }
        if Instant::now() >= deadline {
            projector_child_stage("reader-pending");
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[tokio::test]
async fn projector_desktop_gui_child_canonical_local_target() {
    let Some((root, owner, service, project)) = projector_child_fixture().await else {
        projector_child_standalone_return();
        return;
    };
    let epoch = Epoch(4_242);
    let id = projector_child_spawn(
        &service,
        "projector-request-sub",
        Some(project.join("sub").to_string_lossy().into_owned()),
        100,
        30,
    )
    .await;
    let catalog = service.workspace_service.catalog().unwrap();
    let projected = service
        .project_desktop_gui_session(&id, epoch, &catalog)
        .unwrap();
    assert_eq!(projected.target.session_id, id);
    assert_eq!(projected.target.daemon_epoch, epoch);
    assert_eq!(projected.workspace_id, "projector-ws");
    assert_eq!(projected.cwd, "sub");
    assert_eq!(projected.worktree, None);
    assert_eq!((projected.cols, projected.rows), (100, 30));
    assert!(projected.running);
    assert!(projected.end_sequence.0 >= projected.start_sequence.0);
    assert!(uuid::Uuid::parse_str(&projected.target.machine_id).is_ok());
    assert_ne!(projected.target.machine_id, id);
    // The served identity is this machine's durable one, persisted inside the fixture, not
    // derived from the session id or a journal record.
    let identity_path = crate::remote::auth::canonical_identity_dir()
        .unwrap()
        .join("identity.json");
    assert!(
        identity_path.starts_with(&root),
        "{identity_path:?} escaped the fixture"
    );
    let identity: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&identity_path).unwrap()).unwrap();
    assert_eq!(
        identity["machineId"].as_str().unwrap(),
        projected.target.machine_id
    );
    // A session sitting at the workspace root projects as ".", not an absolute host path.
    let at_root = projector_child_spawn(&service, "projector-request-root", None, 80, 24).await;
    let projected_root = service
        .project_desktop_gui_session(&at_root, epoch, &catalog)
        .unwrap();
    assert_eq!(projected_root.cwd, ".");
    assert_eq!((projected_root.cols, projected_root.rows), (80, 24));
    projector_child_cleanup(&service).await;
    drop(service);
    drop(owner);
    // Printed last, so a timeout that still shows this stage proves the body returned and the
    // process failed to exit after it - a shutdown stall, not a scenario stall.
    projector_child_stage("body-complete");
    eprintln!(
        "F1-PROJECTOR canonical: session={id} machine_id={} cwd=sub root_cwd={} fixture_root={} reader_phase={}",
        projected.target.machine_id,
        projected_root.cwd,
        root.display(),
        projector_child_reader_phase(&service, &id)
    );
}

#[tokio::test]
async fn projector_desktop_gui_child_foreign_workspace() {
    let Some((_root, owner, service, _project)) = projector_child_fixture().await else {
        projector_child_standalone_return();
        return;
    };
    let epoch = Epoch(4_243);
    let id = projector_child_spawn(&service, "projector-request-owner", None, 80, 24).await;
    let catalog = service.workspace_service.catalog().unwrap();
    assert!(service
        .project_desktop_gui_session(&id, epoch, &catalog)
        .is_ok());
    // A worktree identity naming another workspace is an ownership change, not a path to serve.
    let mut meta = service.session_metadata.read().get(&id).cloned().unwrap();
    meta.worktree = Some(crate::worktree::WorktreeIdentity {
        ws_id: "another-ws".into(),
        slug: "detached".into(),
    });
    service.session_metadata.write().insert(id.clone(), meta);
    assert_eq!(
        service
            .project_desktop_gui_session(&id, epoch, &catalog)
            .unwrap_err(),
        "SESSION_OWNERSHIP_CHANGED"
    );
    // A session naming a workspace the catalog does not hold is refused, not served from a guess.
    let mut meta = service.session_metadata.read().get(&id).cloned().unwrap();
    meta.worktree = None;
    meta.workspace_id = "unregistered-ws".into();
    service.session_metadata.write().insert(id.clone(), meta);
    assert_eq!(
        service
            .project_desktop_gui_session(&id, epoch, &catalog)
            .unwrap_err(),
        "SESSION_NOT_FOUND"
    );
    projector_child_cleanup(&service).await;
    drop(service);
    drop(owner);
    // Printed last, so a timeout that still shows this stage proves the body returned and the
    // process failed to exit after it - a shutdown stall, not a scenario stall.
    projector_child_stage("body-complete");
    eprintln!(
        "F1-PROJECTOR foreign workspace: session={id} worktree-mismatch=refused unregistered=refused reader_phase={}",
        projector_child_reader_phase(&service, &id)
    );
}

#[tokio::test]
async fn projector_desktop_gui_child_root_escape() {
    let Some((root, owner, service, project)) = projector_child_fixture().await else {
        projector_child_standalone_return();
        return;
    };
    let epoch = Epoch(4_244);
    let id = projector_child_spawn(
        &service,
        "projector-request-escape",
        Some(project.join("sub").to_string_lossy().into_owned()),
        80,
        24,
    )
    .await;
    let catalog = service.workspace_service.catalog().unwrap();
    // The resolved workspace root is the fence: a cwd above it is refused.
    let mut meta = service.session_metadata.read().get(&id).cloned().unwrap();
    meta.cwd = root.clone();
    service.session_metadata.write().insert(id.clone(), meta);
    assert_eq!(
        service
            .project_desktop_gui_session(&id, epoch, &catalog)
            .unwrap_err(),
        "SESSION_OWNERSHIP_CHANGED"
    );
    // A cwd that cannot be resolved is refused rather than served raw.
    let mut meta = service.session_metadata.read().get(&id).cloned().unwrap();
    meta.cwd = project.join("does-not-exist");
    service.session_metadata.write().insert(id.clone(), meta);
    assert_eq!(
        service
            .project_desktop_gui_session(&id, epoch, &catalog)
            .unwrap_err(),
        "INVALID_PATH"
    );
    projector_child_cleanup(&service).await;
    drop(service);
    drop(owner);
    // Printed last, so a timeout that still shows this stage proves the body returned and the
    // process failed to exit after it - a shutdown stall, not a scenario stall.
    projector_child_stage("body-complete");
    eprintln!(
        "F1-PROJECTOR root escape: session={id} outside-root=refused missing-cwd=refused reader_phase={}",
        projector_child_reader_phase(&service, &id)
    );
}

#[tokio::test]
async fn projector_desktop_gui_child_non_ready_workspace() {
    let Some((_root, owner, service, _project)) = projector_child_fixture().await else {
        projector_child_standalone_return();
        return;
    };
    let epoch = Epoch(4_245);
    let id = projector_child_spawn(&service, "projector-request-nonready", None, 80, 24).await;
    let ready = service.workspace_service.catalog().unwrap();
    assert!(service
        .project_desktop_gui_session(&id, epoch, &ready)
        .is_ok());
    let set_availability = |availability: Availability| {
        service
            .workspace_service
            .catalog
            .lock()
            .as_mut()
            .unwrap()
            .workspaces
            .get_mut("projector-ws")
            .unwrap()
            .availability = availability;
    };
    set_availability(Availability::Missing);
    let non_ready = service.workspace_service.catalog().unwrap();
    assert_eq!(
        service
            .project_desktop_gui_session(&id, epoch, &non_ready)
            .unwrap_err(),
        "SESSION_NOT_FOUND"
    );
    set_availability(Availability::Ready);
    let restored = service.workspace_service.catalog().unwrap();
    assert!(service
        .project_desktop_gui_session(&id, epoch, &restored)
        .is_ok());
    projector_child_cleanup(&service).await;
    drop(service);
    drop(owner);
    // Printed last, so a timeout that still shows this stage proves the body returned and the
    // process failed to exit after it - a shutdown stall, not a scenario stall.
    projector_child_stage("body-complete");
    eprintln!(
        "F1-PROJECTOR non-ready: session={id} availability=missing-refused ready-projectable reader_phase={}",
        projector_child_reader_phase(&service, &id)
    );
}

#[tokio::test]
async fn projector_projects_canonical_local_target_identity_and_cwd() {
    projector_child_case(
        "daemon::session_service::machine_tests::projector_desktop_gui_child_canonical_local_target",
    )
    .await;
}

#[tokio::test]
async fn projector_refuses_foreign_workspace_ownership() {
    projector_child_case(
        "daemon::session_service::machine_tests::projector_desktop_gui_child_foreign_workspace",
    )
    .await;
}

#[tokio::test]
async fn projector_refuses_cwd_outside_workspace_root() {
    projector_child_case(
        "daemon::session_service::machine_tests::projector_desktop_gui_child_root_escape",
    )
    .await;
}

#[tokio::test]
async fn projector_refuses_non_ready_workspace() {
    projector_child_case(
        "daemon::session_service::machine_tests::projector_desktop_gui_child_non_ready_workspace",
    )
    .await;
}
