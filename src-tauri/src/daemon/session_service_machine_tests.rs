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
// test-only scope; that branch remains source-read only.

const PROJECTOR_CHILD_ROOT: &str = "FERRYX_PROJECTOR_CHILD_ROOT";

fn projector_child(name: &str, root: &std::path::Path) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
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

/// Runs one projector case in a private child process and proves the case actually ran.
async fn projector_child_case(name: &str) {
    let root = tempfile::tempdir().unwrap();
    let output = projector_child(name, root.path()).output().await.unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let receipt = root.path().to_owned();
    root.close().unwrap();
    assert!(
        output.status.success(),
        "{name} failed: stdout={stdout:?} stderr={stderr:?}"
    );
    assert!(
        stdout.contains("F1-PROJECTOR") || stderr.contains("F1-PROJECTOR"),
        "{name} never reached the projector assertion: stdout={stdout:?} stderr={stderr:?}"
    );
    eprintln!(
        "F1-PROJECTOR parent {name}: private root={} removed={}",
        receipt.display(),
        !receipt.exists()
    );
}

fn projector_child_root() -> PathBuf {
    PathBuf::from(std::env::var_os(PROJECTOR_CHILD_ROOT).expect("child runs with a private root"))
}

/// Private fixture: a registered plain workspace plus a daemon whose durable identity, catalog and
/// session metadata all live inside the child's own root.
async fn projector_child_fixture() -> (PathBuf, DaemonServer, Arc<DaemonSessionService>, PathBuf) {
    let root = projector_child_root();
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
    (root, owner, service, project)
}

async fn projector_child_spawn(
    service: &Arc<DaemonSessionService>,
    request_id: &str,
    cwd: Option<String>,
    cols: u16,
    rows: u16,
) -> String {
    service
        .handle_spawn(request_id, "projector-ws", None, cwd, cols, rows, None, None, None)
        .await
        .unwrap()
}

async fn projector_child_cleanup(service: &Arc<DaemonSessionService>) {
    for id in service.terminal_service.list_sessions() {
        service.terminal_service.close_session(&id).await.unwrap();
    }
}

#[tokio::test]
async fn projector_desktop_gui_child_canonical_local_target() {
    let (root, owner, service, project) = projector_child_fixture().await;
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
    eprintln!(
        "F1-PROJECTOR canonical: session={id} machine_id={} cwd=sub root_cwd={} fixture_root={}",
        projected.target.machine_id,
        projected_root.cwd,
        root.display()
    );
}

#[tokio::test]
async fn projector_desktop_gui_child_foreign_workspace() {
    let (_root, owner, service, _project) = projector_child_fixture().await;
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
    eprintln!(
        "F1-PROJECTOR foreign workspace: session={id} worktree-mismatch=refused unregistered=refused"
    );
}

#[tokio::test]
async fn projector_desktop_gui_child_root_escape() {
    let (root, owner, service, project) = projector_child_fixture().await;
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
    eprintln!(
        "F1-PROJECTOR root escape: session={id} outside-root=refused missing-cwd=refused"
    );
}

#[tokio::test]
async fn projector_desktop_gui_child_non_ready_workspace() {
    let (_root, owner, service, _project) = projector_child_fixture().await;
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
    eprintln!(
        "F1-PROJECTOR non-ready: session={id} availability=missing-refused ready-projectable"
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
