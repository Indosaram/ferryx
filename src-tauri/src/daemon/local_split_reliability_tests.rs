use super::*;
use crate::daemon::protocol::SplitOperationResult;
use futures_util::FutureExt;
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

struct Fixture {
    root: tempfile::TempDir,
    owner: Arc<DaemonServer>,
    repo: PathBuf,
}

impl Fixture {
    async fn new() -> Self {
        crate::ipc::run_blocking(|| {
            let root = tempfile::tempdir().unwrap();
            let repo = root.path().join("repo");
            fs::create_dir(&repo).unwrap();
            crate::worktree::run_git(&repo, &["init", "--quiet"]).unwrap();
            crate::worktree::run_git(&repo, &["-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "--allow-empty", "-m", "base"]).unwrap();
            let owner = Arc::new(DaemonServer::new_with_paths(Some(root.path().join("config")), Some(root.path().join("auth"))));
            owner.workspace_service.register("a", repo.to_str().unwrap()).unwrap();
            owner.workspace_service.register("b", repo.to_str().unwrap()).unwrap();
            Ok(Self { root, owner, repo: fs::canonicalize(repo).unwrap() })
        }).await.unwrap()
    }

    fn create(&self, request: &str, workspace: &str) -> serde_json::Value {
        serde_json::json!({"type":"spawn","clientRequestId":request,"workspaceId":workspace,
            "worktree":null,"cwd":self.repo,"cols":80,"rows":24,"shell":null,"startup":null,
            "localSplit":{"originEpoch":self.owner.epoch(),
                "expiresAtUnixMs":self.owner.admission_time_unix_ms()+600000,"remainingMs":9000}})
    }

    async fn clean(self) {
        *self.owner.split_probe.write() = None;
        *self.owner.cwd_probe.write() = None;
        *self.owner.workspace_service.transaction_probe.write() = None;
        self.owner.drain_split_fixture().await;
        for id in self.owner.terminal_service.list_sessions() {
            let receiver = self.owner.machine_lifecycles.lock().get(&id).cloned();
            self.owner.terminal_service.close_session(&id).await.unwrap();
            if let Some(mut receiver) = receiver {
                tokio::time::timeout(Duration::from_secs(5), async {
                    while !*receiver.borrow() { receiver.changed().await.unwrap(); }
                }).await.unwrap();
            }
        }
        assert!(self.owner.terminal_service.list_sessions().is_empty());
        drop(self.owner);
        crate::ipc::run_blocking(move || { self.root.close().unwrap(); Ok(()) }).await.unwrap();
        eprintln!("LOCAL_SPLIT_CLEANUP {{\"ownedPtysClosed\":true,\"lifecycleJoined\":true,\"rootRemoved\":true}}");
    }
}

async fn wire(owner: Arc<DaemonServer>, value: serde_json::Value) -> DaemonResponse {
    #[cfg(unix)]
    let (client, server) = {
        let root = tempfile::tempdir().unwrap();
        let listener = tokio::net::UnixListener::bind(root.path().join("wire.sock")).unwrap();
        let client = tokio::time::timeout(Duration::from_secs(5), tokio::net::UnixStream::connect(root.path().join("wire.sock"))).await.unwrap().unwrap();
        let (server, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept()).await.unwrap().unwrap();
        (client, server)
    };
    #[cfg(not(unix))]
    let (client, server) = {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = tokio::time::timeout(Duration::from_secs(5), tokio::net::TcpStream::connect(listener.local_addr().unwrap())).await.unwrap().unwrap();
        let (server, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept()).await.unwrap().unwrap();
        (client, server)
    };
    #[cfg(not(unix))]
    let token = owner.transport_token.clone();
    #[cfg(unix)]
    let token: Option<String> = None;
    let (finished, joined) = tokio::sync::oneshot::channel();
    let service = owner.session_service.clone();
    service.split_wire_tasks.lock().push(tokio::spawn(async move {
        owner.handle_client(server).await;
        let _ = finished.send(());
    }));
    let mut stream = tokio::io::BufReader::new(client);
    let handshake = serde_json::json!({"type":"handshake","version":DAEMON_PROTOCOL_VERSION,"token":token});
    tokio::time::timeout(Duration::from_secs(5), stream.get_mut().write_all(format!("{handshake}\n").as_bytes())).await.unwrap().unwrap();
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(5), stream.read_line(&mut line)).await.unwrap().unwrap();
    assert!(matches!(serde_json::from_str::<DaemonResponse>(&line).unwrap(), DaemonResponse::HandshakeOk { .. }));
    eprintln!("LOCAL_SPLIT_WIRE {{\"request\":{value},\"handshake\":{}}}", line.trim());
    tokio::time::timeout(Duration::from_secs(5), stream.get_mut().write_all(format!("{value}\n").as_bytes())).await.unwrap().unwrap();
    line.clear();
    let response = tokio::time::timeout(Duration::from_secs(5), stream.read_line(&mut line)).await;
    drop(stream);
    tokio::time::timeout(Duration::from_secs(5), joined).await.unwrap().unwrap();
    response.unwrap().unwrap();
    eprintln!("LOCAL_SPLIT_WIRE {{\"response\":{},\"connectionJoined\":true}}", line.trim());
    serde_json::from_str(&line).unwrap()
}

fn operation(request: &serde_json::Value, cancel: bool) -> serde_json::Value {
    serde_json::json!({"type":if cancel {"cancelSpawnOperation"} else {"spawnOperationStatus"},
        "clientRequestId":request["clientRequestId"],"originEpoch":request["localSplit"]["originEpoch"],
        "expiresAtUnixMs":request["localSplit"]["expiresAtUnixMs"]})
}

struct Hold {
    entered: tokio::sync::oneshot::Receiver<()>,
    release: Option<std::sync::mpsc::Sender<()>>,
}

impl Hold {
    fn install(owner: &DaemonServer, request: &str, stage: &'static str) -> Self {
        let (entered, received) = tokio::sync::oneshot::channel();
        let (release, released) = std::sync::mpsc::channel();
        let entered = Mutex::new(Some(entered));
        let released = Mutex::new(released);
        let request = request.to_owned();
        *owner.split_probe.write() = Some(Arc::new(move |id, phase| {
            if id == request && phase == stage {
                if let Some(entered) = entered.lock().take() {
                    entered.send(()).unwrap();
                    released.lock().recv_timeout(Duration::from_secs(5)).unwrap();
                }
            }
        }));
        Self { entered: received, release: Some(release) }
    }
    async fn entered(&mut self) {
        tokio::time::timeout(Duration::from_secs(5), &mut self.entered).await.unwrap().unwrap();
    }
    fn release(&mut self) { if let Some(release) = self.release.take() { let _ = release.send(()); } }
}
impl Drop for Hold { fn drop(&mut self) { self.release(); } }

fn spawned(response: DaemonResponse) -> String {
    match response { DaemonResponse::SpawnOk { session_id, .. } => session_id, other => panic!("{other:?}") }
}

#[tokio::test]
async fn local_split_reliability_wire_rapid_fifo() {
    let fixture = Fixture::new().await;
    let outcome = std::panic::AssertUnwindSafe(async {
    let held = fixture.owner.workspace_service.spawn_queue("a").lock_owned().await;
    let (events, mut observed) = tokio::sync::mpsc::unbounded_channel();
    *fixture.owner.split_probe.write() = Some(Arc::new(move |id, stage| {
        if matches!(stage, "workspaceQueued" | "workspaceAcquired") {
            events.send((id.to_owned(), stage.to_owned())).unwrap();
        }
    }));
    let mut tasks = tokio::task::JoinSet::new();
    let mut order = Vec::new();
    for _ in 0..16 {
        let id = uuid::Uuid::new_v4().to_string();
        order.push(id.clone());
        tasks.spawn(wire(fixture.owner.clone(), fixture.create(&id, "a")));
        let event = tokio::time::timeout(Duration::from_secs(5), observed.recv()).await.unwrap().unwrap();
        assert_eq!(event, (id, "workspaceQueued".into()));
    }
    drop(held);
    for id in order {
        assert_eq!(tokio::time::timeout(Duration::from_secs(5), observed.recv()).await.unwrap().unwrap(), (id, "workspaceAcquired".into()));
    }
    let mut ids = std::collections::HashSet::new();
    while let Some(result) = tasks.join_next().await { ids.insert(spawned(result.unwrap())); }
    assert_eq!(ids.len(), 16);
    }).catch_unwind().await;
    fixture.clean().await;
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
}

#[tokio::test]
async fn local_split_reliability_wire_delete_publication() {
    let fixture = Fixture::new().await;
    let outcome = std::panic::AssertUnwindSafe(async {
    let created = wire(fixture.owner.clone(), serde_json::json!({"type":"createWorktree","workspaceId":"a", "worktree":{"wsId":"a","slug":"held"},"baseRef":null})).await;
    assert!(matches!(created, DaemonResponse::CreateWorktreeOk { .. }));
    let id = uuid::Uuid::new_v4().to_string();
    let mut request = fixture.create(&id, "a");
    request["worktree"] = serde_json::json!({"wsId":"a","slug":"held"});
    request["cwd"] = serde_json::json!(fixture.repo.join(".orca-worktrees/a/held"));
    let mut hold = Hold::install(&fixture.owner, &id, "childCreated");
    let create = tokio::spawn(wire(fixture.owner.clone(), request));
    hold.entered().await;
    let (requested, observed) = tokio::sync::oneshot::channel();
    let requested = Mutex::new(Some(requested));
    *fixture.owner.workspace_service.transaction_probe.write() = Some(Arc::new(move |stage| {
        if stage == "workspaceGateRequested" { if let Some(tx) = requested.lock().take() { let _ = tx.send(()); } }
    }));
    let deletion = serde_json::json!({"type":"deleteWorktree","workspaceId":"a","worktree":{"wsId":"a","slug":"held"},"deleteBranch":false,"destructive":true});
    let delete = tokio::spawn(wire(fixture.owner.clone(), deletion));
    tokio::time::timeout(Duration::from_secs(5), observed).await.unwrap().unwrap();
    assert!(!delete.is_finished());
    hold.release();
    let session = spawned(create.await.unwrap());
    let response = delete.await.unwrap();
    assert!(matches!(response, DaemonResponse::WorktreeError { error } if error.code == crate::ipc::IpcErrorCode::WorktreeBusy));
    assert!(fixture.owner.terminal_service.get_session(&session).is_some());
    }).catch_unwind().await;
    fixture.clean().await;
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
}

#[tokio::test]
async fn preparation_deleted_or_escaped_cwd() {
    let fixture = Fixture::new().await;
    let outcome = std::panic::AssertUnwindSafe(async {
    for path in [fixture.repo.join("deleted"), fixture.root.path().to_owned()] {
        let mut request = fixture.create(&uuid::Uuid::new_v4().to_string(), "a");
        request["cwd"] = serde_json::json!(path);
        assert!(matches!(wire(fixture.owner.clone(), request).await, DaemonResponse::Error { .. }));
    }
    assert!(fixture.owner.terminal_service.list_sessions().is_empty());
    }).catch_unwind().await;
    fixture.clean().await;
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
}

#[tokio::test]
async fn local_split_reliability_wire_duplicate_and_conflict() {
    let fixture = Fixture::new().await;
    let outcome = std::panic::AssertUnwindSafe(async {
    let id = uuid::Uuid::new_v4().to_string();
    let request = fixture.create(&id, "a");
    let mut hold = Hold::install(&fixture.owner, &id, "preChild");
    let first = tokio::spawn(wire(fixture.owner.clone(), request.clone()));
    hold.entered().await;
        for field in ["cwd", "shell", "cols", "startup"] {
            let mut changed = request.clone();
            changed[field] = match field { "cwd" => serde_json::json!(fixture.repo.join("changed")),
                "shell" => serde_json::json!("different-shell"), "cols" => serde_json::json!(81),
                _ => serde_json::json!({"kind":"agentResume","agentType":"claude","providerSession":{"key":"sessionId","id":"different-provider"}}) };
            let response = wire(fixture.owner.clone(), changed).await;
            assert!(matches!(response, DaemonResponse::Error { code: Some(code), .. } if code == "SPAWN_REQUEST_CONFLICT"));
        }
        let mut changed = request.clone(); changed["localSplit"]["expiresAtUnixMs"] = serde_json::json!(request["localSplit"]["expiresAtUnixMs"].as_u64().unwrap()+1);
        assert!(matches!(wire(fixture.owner.clone(), changed).await, DaemonResponse::Error { code: Some(code), .. } if code == "SPAWN_REQUEST_CONFLICT"));
        let mut follower = request.clone(); follower["localSplit"]["remainingMs"] = serde_json::json!(4000);
        let follower = tokio::spawn(wire(fixture.owner.clone(), follower));
        hold.release();
        let first = spawned(first.await.unwrap());
        assert_eq!(first, spawned(follower.await.unwrap()));
        assert_eq!(fixture.owner.terminal_service.list_sessions(), vec![first]);
    }).catch_unwind().await;
    fixture.clean().await;
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
}

#[tokio::test]
async fn local_split_reliability_wire_unrelated_remote_progress() {
    let fixture = Fixture::new().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let workspace = crate::ssh::projects::identity("private-ssh", "/private/project");
    let store = fixture.owner.ssh_store_path.clone();
    let stored_workspace = workspace.clone();
    crate::ipc::run_blocking(move || {
        fs::write(&store, serde_json::to_vec(&serde_json::json!({"hosts":[{
            "id":"private-ssh","label":"Private fixture","hostname":"127.0.0.1",
            "port":address.port(),"source":"manual","authMethod":"agent"}]})).unwrap()).unwrap();
        fs::write(crate::ssh::projects::store_path(&store), serde_json::to_vec(&serde_json::json!({
            stored_workspace.clone():{"workspaceId":stored_workspace,"hostId":"private-ssh","repoRoot":"/private/project","gitRoot":null,"platform":"posix"}
        })).unwrap()).unwrap();
        Ok(())
    }).await.unwrap();
    let (connected, connection) = tokio::sync::oneshot::channel();
    let (release, mut released) = tokio::sync::watch::channel(false);
    let endpoint = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        connected.send(()).unwrap();
        while !*released.borrow() { if released.changed().await.is_err() { break; } }
        drop(socket);
        drop(listener);
    });
    let outcome = std::panic::AssertUnwindSafe(async {
    let id = uuid::Uuid::new_v4().to_string();
    let mut remote = fixture.create(&id, &workspace);
    remote.as_object_mut().unwrap().remove("localSplit");
    remote["cwd"] = serde_json::Value::Null;
    remote["startup"] = serde_json::json!({"kind":"remoteSsh","hostStorePath":fixture.owner.ssh_store_path});
    let first = tokio::spawn(wire(fixture.owner.clone(), remote));
    tokio::time::timeout(Duration::from_secs(5), connection).await.unwrap().unwrap();
    let result = wire(fixture.owner.clone(), fixture.create(&uuid::Uuid::new_v4().to_string(), "b")).await;
    assert!(fixture.owner.terminal_service.get_session(&spawned(result)).is_some());
    release.send_replace(true);
    let first = first.await.unwrap();
    assert!(matches!(first, DaemonResponse::Error { .. }));
    }).catch_unwind().await;
    release.send_replace(true);
    tokio::time::timeout(Duration::from_secs(5), endpoint).await.unwrap().unwrap();
    fixture.clean().await;
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
}

#[tokio::test]
async fn local_split_reliability_wire_lost_reply_retry() {
    let fixture = Fixture::new().await;
    let outcome = std::panic::AssertUnwindSafe(async {
    let id = uuid::Uuid::new_v4().to_string();
    let request = fixture.create(&id, "a");
    let mut hold = Hold::install(&fixture.owner, &id, "published");
    let waiter = tokio::spawn(wire(fixture.owner.clone(), request.clone()));
    hold.entered().await;
    waiter.abort();
    let _ = waiter.await;
    hold.release();
    fixture.owner.drain_split_fixture().await;
    let status = wire(fixture.owner.clone(), operation(&request, false)).await;
    let retry = match status {
        DaemonResponse::SpawnOperationOk { operation: SplitOperationResult::Created { session_id, .. } } => session_id,
        other => panic!("{other:?}"),
    };
    assert_eq!(fixture.owner.terminal_service.list_sessions(), vec![retry]);
    }).catch_unwind().await;
    fixture.clean().await;
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
}

#[tokio::test]
async fn local_split_reliability_wire_cancel_publication() {
    let fixture = Fixture::new().await;
    let outcome = std::panic::AssertUnwindSafe(async {
    let request = fixture.create(&uuid::Uuid::new_v4().to_string(), "a");
    assert!(matches!(wire(fixture.owner.clone(), operation(&request, true)).await,
        DaemonResponse::SpawnOperationOk { operation: SplitOperationResult::Cancelled }));
    assert!(matches!(wire(fixture.owner.clone(), request).await, DaemonResponse::Error { code: Some(code), .. } if code == "SPAWN_CANCELLED"));
    for stage in ["preChild", "childCreated", "published"] {
        let id = uuid::Uuid::new_v4().to_string();
        let request = fixture.create(&id, "a");
        let mut hold = Hold::install(&fixture.owner, &id, stage);
        let create = tokio::spawn(wire(fixture.owner.clone(), request.clone()));
        hold.entered().await;
        let accepted = fixture.owner.session_service.split_operation(&id, fixture.owner.epoch(), request["localSplit"]["expiresAtUnixMs"].as_u64().unwrap(), true).unwrap();
        assert!(matches!(accepted, SplitOperationResult::Pending { cancel_requested: true }));
        hold.release();
        let _ = create.await.unwrap();
        assert!(matches!(wire(fixture.owner.clone(), operation(&request, true)).await,
            DaemonResponse::SpawnOperationOk { operation: SplitOperationResult::Cancelled }));
        assert!(fixture.owner.terminal_service.list_sessions().is_empty());
    }
    }).catch_unwind().await;
    fixture.clean().await;
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
}

#[tokio::test]
async fn local_split_reliability_wire_expiry_epoch() {
    let fixture = Fixture::new().await;
    let outcome = std::panic::AssertUnwindSafe(async {
    let request = fixture.create(&uuid::Uuid::new_v4().to_string(), "a");
    assert!(matches!(wire(fixture.owner.clone(), operation(&request, false)).await,
        DaemonResponse::SpawnOperationOk { operation: SplitOperationResult::Absent { can_create: true } }));
    let mut expired = request.clone(); expired["localSplit"]["expiresAtUnixMs"] = serde_json::json!(0);
    assert!(matches!(wire(fixture.owner.clone(), expired).await, DaemonResponse::Error { code: Some(code), .. } if code == "SPAWN_REQUEST_EXPIRED"));
    let mut epoch = request.clone(); epoch["localSplit"]["originEpoch"] = serde_json::json!(0);
    assert!(matches!(wire(fixture.owner.clone(), operation(&epoch, false)).await,
        DaemonResponse::SpawnOperationOk { operation: SplitOperationResult::Unknown { .. } }));
    assert!(matches!(wire(fixture.owner.clone(), epoch).await, DaemonResponse::Error { code: Some(code), .. } if code == "SPAWN_EPOCH_CHANGED"));
    assert!(matches!(wire(fixture.owner.clone(), operation(&request, true)).await,
        DaemonResponse::SpawnOperationOk { operation: SplitOperationResult::Cancelled }));
    fixture.owner.advance_admission_clock_for_test(600_000);
    assert!(matches!(wire(fixture.owner.clone(), request.clone()).await,
        DaemonResponse::Error { code: Some(code), .. } if code == "SPAWN_REQUEST_EXPIRED"));
    assert!(matches!(wire(fixture.owner.clone(), operation(&request, false)).await,
        DaemonResponse::SpawnOperationOk { operation: SplitOperationResult::Absent { can_create: false } }));
    assert!(fixture.owner.terminal_service.list_sessions().is_empty());
    }).catch_unwind().await;
    fixture.clean().await;
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
}

#[tokio::test]
async fn preparation_unregister_cannot_be_resurrected() {
    let fixture = Fixture::new().await;
    let outcome = std::panic::AssertUnwindSafe(async {
    let id = uuid::Uuid::new_v4().to_string();
    let request = fixture.create(&id, "a");
    let mut hold = Hold::install(&fixture.owner, &id, "ownerEntered");
    let create = tokio::spawn(wire(fixture.owner.clone(), request));
    hold.entered().await;
    let removed = wire(fixture.owner.clone(), serde_json::json!({"type":"unregisterWorkspace","workspaceId":"a"})).await;
    hold.release();
    let created = create.await.unwrap();
    assert!(matches!(removed, DaemonResponse::UnregisterWorkspaceOk));
    assert!(matches!(created, DaemonResponse::Error { .. }));
    assert!(fixture.owner.terminal_service.list_sessions().is_empty());
    }).catch_unwind().await;
    fixture.clean().await;
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
}

#[tokio::test]
async fn preparation_describe_invalid_probe_preserves_fallback() {
    let fixture = Fixture::new().await;
    let outcome = std::panic::AssertUnwindSafe(async {
    let id = spawned(wire(fixture.owner.clone(), fixture.create(&uuid::Uuid::new_v4().to_string(), "a")).await);
    for probe in [None, Some(PathBuf::from("relative")), Some(fixture.repo.join("missing")),
        Some(PathBuf::from("bad\npath")), Some(PathBuf::from("cwd|rtd info error: No such file or directory")),
        Some(fixture.repo.join(".git/HEAD"))] {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = calls.clone();
        *fixture.owner.cwd_probe.write() = Some(Arc::new(move |_| {
            observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            probe.clone()
        }));
        let response = wire(fixture.owner.clone(), serde_json::json!({"type":"describeSession","sessionId":id})).await;
        assert!(matches!(response, DaemonResponse::DescribeSessionOk { session } if session.cwd.as_deref() == fixture.repo.to_str()));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
    }).catch_unwind().await;
    fixture.clean().await;
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
}

#[tokio::test]
async fn local_split_reliability_wire_handover_owner() {
    let fixture = Fixture::new().await;
    let outcome = std::panic::AssertUnwindSafe(async {
    let id = uuid::Uuid::new_v4().to_string();
    let mut hold = Hold::install(&fixture.owner, &id, "preChild");
    let create = tokio::spawn(wire(fixture.owner.clone(), fixture.create(&id, "a")));
    hold.entered().await;
    assert_eq!(fixture.owner.handover_manager.commit_handover_v5(&fixture.owner.terminal_service).unwrap_err(), "HANDOVER_BUSY");
    #[cfg(unix)]
    assert_eq!(fixture.owner.handover_manager.prepare_handover(&fixture.owner.terminal_service).unwrap_err(), "HANDOVER_BUSY");
    hold.release();
    spawned(create.await.unwrap());
    assert_eq!(fixture.owner.handover_manager.status(), crate::daemon::handover::HandoverStatus::Active);
    }).catch_unwind().await;
    fixture.clean().await;
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
}

#[tokio::test]
async fn preparation_probe_deadline_and_singleflight() {
    let fixture = Fixture::new().await;
    let (release, released) = std::sync::mpsc::channel();
    let (entered, observed) = tokio::sync::oneshot::channel();
    let (finished, done) = tokio::sync::oneshot::channel();
    let entered = Mutex::new(Some(entered));
    let finished = Mutex::new(Some(finished));
    let released = Mutex::new(released);
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen = calls.clone();
    let outcome = std::panic::AssertUnwindSafe(async {
        let id = spawned(wire(fixture.owner.clone(), fixture.create(&uuid::Uuid::new_v4().to_string(), "a")).await);
        *fixture.owner.cwd_probe.write() = Some(Arc::new(move |_| {
            seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if let Some(tx) = entered.lock().take() { tx.send(()).unwrap(); }
            released.lock().recv_timeout(Duration::from_secs(5)).unwrap();
            if let Some(tx) = finished.lock().take() { let _ = tx.send(()); }
            None
        }));
        let service = fixture.owner.session_service.clone();
        let first_id = id.clone();
        let deadline = tokio::time::Instant::now() + Duration::from_millis(500);
        let first = tokio::spawn(async move { service.describe_session_until(&first_id, deadline).await });
        tokio::time::timeout(Duration::from_secs(5), observed).await.unwrap().unwrap();
        struct ClockGuard;
        impl Drop for ClockGuard { fn drop(&mut self) { tokio::time::resume(); } }
        tokio::time::pause();
        let clock = ClockGuard;
        tokio::time::advance(Duration::from_millis(501)).await;
        let timed = first.await.unwrap();
        drop(clock);
        assert!(matches!(timed, DaemonResponse::DescribeSessionOk { session } if session.cwd.as_deref() == fixture.repo.to_str()));
        let busy = wire(fixture.owner.clone(), serde_json::json!({"type":"describeSession","sessionId":id})).await;
        assert!(matches!(busy, DaemonResponse::DescribeSessionOk { session } if session.cwd.is_none()));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }).catch_unwind().await;
    let _ = release.send(());
    if calls.load(std::sync::atomic::Ordering::SeqCst) != 0 {
        tokio::time::timeout(Duration::from_secs(5), done).await.unwrap().unwrap();
    }
    fixture.clean().await;
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
}

#[tokio::test]
async fn local_split_reliability_wire_immediate_exit_before_owner_finish() {
    let fixture = Fixture::new().await;
    let outcome = std::panic::AssertUnwindSafe(async {
        let id = uuid::Uuid::new_v4().to_string();
        let request = fixture.create(&id, "a");
        let mut hold = Hold::install(&fixture.owner, &id, "published");
        let create = tokio::spawn(wire(fixture.owner.clone(), request.clone()));
        hold.entered().await;
        let sessions = fixture.owner.terminal_service.list_sessions();
        assert_eq!(sessions.len(), 1);
        let backend = &sessions[0];
        let mut lifecycle = fixture.owner.machine_lifecycles.lock().get(backend).unwrap().clone();
        fixture.owner.terminal_service.close_session(backend).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while !*lifecycle.borrow() { lifecycle.changed().await.unwrap(); }
        }).await.unwrap();
        hold.release();
        assert!(matches!(create.await.unwrap(), DaemonResponse::Error { .. }));
        assert!(matches!(wire(fixture.owner.clone(), operation(&request, false)).await,
            DaemonResponse::SpawnOperationOk { operation: SplitOperationResult::Exited }));
        assert!(matches!(wire(fixture.owner.clone(), request).await, DaemonResponse::Error { .. }));
        assert!(fixture.owner.terminal_service.list_sessions().is_empty());
    }).catch_unwind().await;
    fixture.clean().await;
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
}

#[tokio::test]
async fn local_split_reliability_wire_provider_reservation() {
    let fixture = Fixture::new().await;
    let outcome = std::panic::AssertUnwindSafe(async {
        let id = uuid::Uuid::new_v4().to_string();
        let mut request = fixture.create(&id, "a");
        request.as_object_mut().unwrap().remove("localSplit");
        request["startup"] = serde_json::json!({"kind":"agentResume","agentType":"claude",
            "providerSession":{"key":"sessionId","id":"private-provider"}});
        request["cwd"] = serde_json::json!(fixture.repo.join("missing"));
        let mut hold = Hold::install(&fixture.owner, &id, "preparation");
        let first = tokio::spawn(wire(fixture.owner.clone(), request.clone()));
        hold.entered().await;
        let mut competitor = request.clone();
        competitor["clientRequestId"] = serde_json::json!(uuid::Uuid::new_v4().to_string());
        competitor["workspaceId"] = serde_json::json!("b");
        assert!(matches!(wire(fixture.owner.clone(), competitor.clone()).await,
            DaemonResponse::AgentSessionConflict { .. }));
        hold.release();
        assert!(matches!(first.await.unwrap(), DaemonResponse::Error { .. }));
        assert_eq!(fixture.owner.provider_claim_len_for_test(), 0);
        assert!(matches!(wire(fixture.owner.clone(), competitor).await, DaemonResponse::Error { .. }));
        assert_eq!(fixture.owner.provider_claim_len_for_test(), 0);
        assert!(fixture.owner.terminal_service.list_sessions().is_empty());
    }).catch_unwind().await;
    fixture.clean().await;
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
}

#[tokio::test]
async fn preparation_registered_split_has_no_registration_or_list() {
    let fixture = Fixture::new().await;
    let outcome = std::panic::AssertUnwindSafe(async {
        assert!(matches!(wire(fixture.owner.clone(), serde_json::json!({"type":"createWorktree","workspaceId":"a","worktree":{"wsId":"a","slug":"managed"},"baseRef":null})).await,
            DaemonResponse::CreateWorktreeOk { .. }));
        let managed = fixture.repo.join(".orca-worktrees/a/managed");
        let subdir = managed.join("sub");
        let path = subdir.clone();
        crate::ipc::run_blocking(move || { fs::create_dir(path).unwrap(); Ok(()) }).await.unwrap();
        let registrations = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = registrations.clone();
        *fixture.owner.workspace_service.transaction_probe.write() = Some(Arc::new(move |stage| {
            if stage == "registerWorkspace" { observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst); }
        }));
        let commands = Arc::new(Mutex::new(Vec::new()));
        let observed = commands.clone();
        *fixture.owner.split_git_observer.write() = Some(Arc::new(move |_, args| {
            observed.lock().push(args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>());
        }));
        let root = spawned(wire(fixture.owner.clone(), fixture.create(&uuid::Uuid::new_v4().to_string(), "a")).await);
        let mut request = fixture.create(&uuid::Uuid::new_v4().to_string(), "a");
        request["worktree"] = serde_json::json!({"wsId":"a","slug":"managed"});
        request["cwd"] = serde_json::json!(subdir);
        let managed_id = spawned(wire(fixture.owner.clone(), request).await);
        assert_ne!(root, managed_id);
        assert_eq!(fixture.owner.terminal_service.get_session(&managed_id).unwrap().worktree_path(), Some(managed));
        assert_eq!(registrations.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert!(!commands.lock().iter().any(|args| args.iter().any(|arg| arg == "--porcelain") && args.iter().any(|arg| arg == "list")));
        assert!(!commands.lock().is_empty());
    }).catch_unwind().await;
    fixture.clean().await;
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
}

#[tokio::test]
async fn local_split_reliability_wire_original_deadline_no_late_child() {
    let fixture = Fixture::new().await;
    let outcome = std::panic::AssertUnwindSafe(async {
        let id = uuid::Uuid::new_v4().to_string();
        let mut request = fixture.create(&id, "a");
        request["localSplit"]["remainingMs"] = serde_json::json!(0);
        let response = wire(fixture.owner.clone(), request.clone()).await;
        assert!(matches!(response, DaemonResponse::Error { .. }));
        let status = wire(fixture.owner.clone(), operation(&request, false)).await;
        assert!(matches!(status, DaemonResponse::SpawnOperationOk { operation: SplitOperationResult::Failed { no_child: _, .. } }));
        assert!(fixture.owner.terminal_service.list_sessions().is_empty());
    }).catch_unwind().await;
    fixture.clean().await;
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
}

#[tokio::test]
async fn local_split_reliability_wire_cancel_retries_cleanup_without_closing_adopted_session() {
    let fixture = Fixture::new().await;
    let outcome = std::panic::AssertUnwindSafe(async {
        let unrelated = spawned(wire(fixture.owner.clone(), fixture.create(&uuid::Uuid::new_v4().to_string(), "b")).await);
        let request = fixture.create(&uuid::Uuid::new_v4().to_string(), "a");
        let owned = spawned(wire(fixture.owner.clone(), request.clone()).await);
        fixture.owner.split_close_failure.store(true, std::sync::atomic::Ordering::Release);
        let failed = wire(fixture.owner.clone(), operation(&request, true)).await;
        assert!(matches!(failed, DaemonResponse::SpawnOperationOk { operation: SplitOperationResult::Pending { cancel_requested: true } }));
        assert!(fixture.owner.terminal_service.get_session(&owned).is_some());
        let (first, second) = tokio::join!(wire(fixture.owner.clone(), operation(&request, true)),
            wire(fixture.owner.clone(), operation(&request, true)));
        assert!(matches!(first, DaemonResponse::SpawnOperationOk { operation: SplitOperationResult::Cancelled }));
        assert!(matches!(second, DaemonResponse::SpawnOperationOk { operation: SplitOperationResult::Cancelled }));
        assert_eq!(fixture.owner.terminal_service.list_sessions(), vec![unrelated]);
    }).catch_unwind().await;
    fixture.clean().await;
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
}

#[cfg(unix)]
#[tokio::test]
async fn preparation_git_deadline_cleans_owned_child() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let repo = fixture.repo.clone();
    crate::ipc::run_blocking(move || {
        let hook = repo.join(".git/hooks/pre-commit");
        fs::write(&hook, format!("#!/usr/bin/env python3\nimport os,socket\npid=os.fork()\nif pid: os.waitpid(pid,0)\nelse:\n s=socket.create_connection(('127.0.0.1',{port}))\n s.sendall(b'ready\\n')\n s.recv(1)\n")).unwrap();
        fs::set_permissions(hook, fs::Permissions::from_mode(0o700)).unwrap();
        Ok(())
    }).await.unwrap();
    let repo = fixture.repo.clone();
    let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let cancellation = Arc::new(tokio::sync::Notify::new());
    let flag = cancelled.clone();
    let notification = cancellation.clone();
    let worker = tokio::spawn(crate::ipc::run_blocking(move || {
        Ok(crate::worktree::git::with_preparation_git_budget(Instant::now()+Duration::from_secs(4), flag,
            Some(notification), || crate::worktree::run_git(&repo, &["-c","user.name=Fixture","-c","user.email=fixture@example.invalid","commit","--allow-empty","-m","held"])))
    }));
    let outcome = std::panic::AssertUnwindSafe(async {
        let (socket, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept()).await.unwrap().unwrap();
        let mut socket = tokio::io::BufReader::new(socket);
        let mut line = String::new();
        socket.read_line(&mut line).await.unwrap();
        assert_eq!(line, "ready\n");
        cancelled.store(true, std::sync::atomic::Ordering::Release);
        cancellation.notify_waiters();
        line.clear();
        assert_eq!(tokio::time::timeout(Duration::from_secs(5), socket.read_line(&mut line)).await.unwrap().unwrap(), 0);
    }).catch_unwind().await;
    cancelled.store(true, std::sync::atomic::Ordering::Release);
    cancellation.notify_waiters();
    let result = tokio::time::timeout(Duration::from_secs(5), worker).await.unwrap().unwrap().unwrap();
    drop(listener);
    fixture.clean().await;
    assert!(result.is_err());
    eprintln!("LOCAL_SPLIT_GIT {{\"workerJoined\":true,\"descendantSocketEof\":true,\"noPty\":true}}");
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
}

#[tokio::test]
async fn local_split_reliability_wire_cancel_queued_owner() {
    let fixture = Fixture::new().await;
    let outcome = std::panic::AssertUnwindSafe(async {
        let held = fixture.owner.workspace_service.spawn_queue("a").lock_owned().await;
        let (entered, observed) = tokio::sync::oneshot::channel();
        let entered = Mutex::new(Some(entered));
        *fixture.owner.split_probe.write() = Some(Arc::new(move |_, stage| {
            if stage == "workspaceQueued" { if let Some(tx) = entered.lock().take() { let _ = tx.send(()); } }
        }));
        let request = fixture.create(&uuid::Uuid::new_v4().to_string(), "a");
        let create = tokio::spawn(wire(fixture.owner.clone(), request.clone()));
        tokio::time::timeout(Duration::from_secs(5), observed).await.unwrap().unwrap();
        let accepted = fixture.owner.session_service.split_operation(request["clientRequestId"].as_str().unwrap(),
            fixture.owner.epoch(), request["localSplit"]["expiresAtUnixMs"].as_u64().unwrap(), true).unwrap();
        assert!(matches!(accepted, SplitOperationResult::Pending { cancel_requested: true }));
        drop(held);
        assert!(matches!(create.await.unwrap(), DaemonResponse::Error { .. }));
        assert!(matches!(wire(fixture.owner.clone(), operation(&request, false)).await,
            DaemonResponse::SpawnOperationOk { operation: SplitOperationResult::Cancelled }));
        assert!(fixture.owner.terminal_service.list_sessions().is_empty());
    }).catch_unwind().await;
    fixture.clean().await;
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
}
