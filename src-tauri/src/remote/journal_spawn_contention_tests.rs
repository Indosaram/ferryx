use super::*;

#[tokio::test]
async fn journal_final_spawn_revocation() { spawn_contention(false).await; }

#[tokio::test]
async fn journal_final_spawn_deadline() { spawn_contention(true).await; }

async fn spawn_contention(expire: bool) {
    let (root, server, token, device, workspace) = crate::ipc::run_blocking(|| {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("project")).unwrap();
        let server = DaemonServer::new_with_paths(Some(root.path().join("config")), Some(root.path().join("auth")));
        let state = server.remote_state();
        let workspace = state.machine_services.as_ref().unwrap().workspaces.register_machine(root.path().join("project").to_str().unwrap()).unwrap();
        let pin = state.auth_manager.create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine).unwrap();
        let (token, device) = state.auth_manager.exchange_pairing_code(&pin, "late-spawn").unwrap();
        Ok((root, server, token, device.id, workspace))
    }).await.unwrap();
    let state = server.remote_state().clone();
    let sessions = state.machine_services.as_ref().unwrap().sessions.clone();
    let service = state.machine_services.as_ref().unwrap().workspaces.clone();
    let slots = service.project_mutations.clone();
    let capacity = slots.available_permits();
    let (spawn_lock, terminals) = sessions.journal_spawn_probe_handles();
    let mut gate = Some(spawn_lock.clone().lock_owned().await);
    let (queued_tx, queued_rx) = tokio::sync::oneshot::channel();
    let queued_tx = Mutex::new(Some(queued_tx));
    *service.transaction_probe.write() = Some(Arc::new(move |phase| {
        if phase == "sessionBeforeSpawnGate" {
            if let Some(tx) = queued_tx.lock().unwrap().take() { let _ = tx.send(()); }
        }
    }));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    let (joined_tx, joined_rx) = tokio::sync::oneshot::channel();
    let gateway_state = state.clone();
    let gateway = std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            axum::serve(tokio::net::TcpListener::from_std(listener).unwrap(), create_remote_router(gateway_state))
                .with_graceful_shutdown(async { let _ = stop_rx.await; }).await.unwrap();
        });
        let _ = joined_tx.send(());
    });
    let client = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(50)).build().unwrap();
    let request = uuid::Uuid::new_v4().to_string();
    let call = client.post(format!("http://{address}/api/v1/sessions")).bearer_auth(&token).body(serde_json::json!({
        "requestId":request,"workspaceId":workspace,"cols":80,"rows":24,"worktree":null,
        "inheritFromSessionId":null,"cwdRelative":null,"startup":{"kind":"shell"}
    }).to_string());
    let started = std::time::Instant::now();
    let mut response = Some(tokio::spawn(async move { call.send().await }));
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let mut writer = None;
    let outcome = std::panic::AssertUnwindSafe(async {
        // This signal occurs only after the first HTTP reconciliation completed.
        tokio::time::timeout(Duration::from_secs(10), queued_rx).await.unwrap().unwrap();
        let (held_tx, held_rx) = tokio::sync::oneshot::channel();
        let (read_tx, read_rx) = tokio::sync::oneshot::channel();
        let held_tx = Mutex::new(Some(held_tx));
        let read_tx = Mutex::new(Some(read_tx));
        let release_rx = Mutex::new(release_rx);
        *service.journal.probe.write() = Some(Arc::new(move |phase| match phase {
            "persist" => if let Some(tx) = held_tx.lock().unwrap().take() {
                let _ = tx.send(());
                release_rx.lock().unwrap().recv_timeout(Duration::from_secs(60)).unwrap();
            },
            "reconcile" => if let Some(tx) = read_tx.lock().unwrap().take() { let _ = tx.send(()); },
            _ => {}
        }));
        let workspaces = service.clone();
        writer = Some(std::thread::spawn(move || workspaces.journal.begin("fixture", &uuid::Uuid::new_v4().to_string(), "fixture", "digest", "none").unwrap()));
        tokio::time::timeout(Duration::from_secs(10), held_rx).await.unwrap().unwrap();
        drop(gate.take());
        tokio::time::timeout(Duration::from_secs(10), read_rx).await.unwrap().unwrap();
        let health = client.get(format!("http://{address}/health")).timeout(Duration::from_millis(500)).send().await;
        assert!(health.is_ok(), "late spawn journal blocked unrelated HTTP: {health:?}");
        assert!(spawn_lock.try_lock().is_err());
        if !expire {
            let auth = state.auth_manager.clone(); let device = device.clone();
            crate::ipc::run_blocking(move || Ok(auth.revoke_device(&device).unwrap())).await.unwrap();
        }
        let result = tokio::time::timeout(Duration::from_secs(45), response.as_mut().unwrap()).await.unwrap().unwrap().unwrap();
        response.take();
        assert_eq!(result.status().as_u16(), if expire { 504 } else { 401 });
        if expire { assert!(started.elapsed() >= Duration::from_secs(40)); }
        assert_eq!(slots.available_permits(), capacity - 1);
        assert!(spawn_lock.try_lock().is_err());
        eprintln!("A09_FINAL expire={expire} valid_root=true second_reconcile=true independent_health=true response_before_release=true admission_and_spawn_retained=true");
    }).catch_unwind().await;
    // Revoke even on RED before releasing the writer: cleanup must never spawn a PTY.
    let auth = state.auth_manager.clone(); let revoke_device = device.clone();
    crate::ipc::run_blocking(move || Ok(auth.revoke_device(&revoke_device).unwrap())).await.unwrap();
    drop(gate.take());
    let _ = release_tx.send(());
    if let Some(writer) = writer { writer.join().unwrap(); }
    if let Some(response) = response { let _ = tokio::time::timeout(Duration::from_secs(10), response).await.unwrap(); }
    let drained = tokio::time::timeout(Duration::from_secs(10), slots.acquire_many_owned(capacity as u32)).await.unwrap().unwrap();
    let spawn_drained = tokio::time::timeout(Duration::from_secs(10), spawn_lock.clone().lock_owned()).await.unwrap();
    let no_pty = terminals.list_sessions().is_empty();
    let journal = service.clone();
    let no_intent = crate::ipc::run_blocking(move || Ok(journal.journal.reconcile(&device, &request).unwrap().is_none())).await.unwrap();
    *service.journal.probe.write() = None;
    *service.transaction_probe.write() = None;
    let _ = stop_tx.send(());
    tokio::time::timeout(Duration::from_secs(10), joined_rx).await.unwrap().unwrap();
    gateway.join().unwrap();
    assert!(tokio::net::TcpStream::connect(address).await.is_err());
    drop((drained, spawn_drained, sessions, service, state, server));
    crate::ipc::run_blocking(move || { root.close().unwrap(); Ok(()) }).await.unwrap();
    eprintln!("A09_FINAL pid={} cleanup writer_joined=true gateway_joined=true listener_refused=true root_removed=true no_pty={no_pty} no_intent={no_intent}", std::process::id());
    assert!(no_pty && no_intent);
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
}
