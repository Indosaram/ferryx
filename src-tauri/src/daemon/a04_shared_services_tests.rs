use super::*;
use anyhow::{ensure, Context};
use tokio::io::AsyncBufReadExt;

async fn request(socket: &Path, value: serde_json::Value) -> anyhow::Result<serde_json::Value> {
    let stream = UnixStream::connect(socket).await?;
    let (read, mut write) = stream.into_split();
    let mut bytes = serde_json::to_vec(&value)?;
    bytes.push(b'\n');
    write.write_all(&bytes).await?;
    let mut line = String::new();
    tokio::time::timeout(
        Duration::from_secs(10),
        BufReader::new(read).read_line(&mut line),
    )
    .await??;
    Ok(serde_json::from_str(&line)?)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Injection {
    None,
    IpcSetup,
    HttpSetup,
    Scenario,
    Cleanup,
    Cancel,
    Deadline,
}

async fn join_listener(
    mut task: tokio::task::JoinHandle<std::io::Result<()>>,
) -> anyhow::Result<()> {
    match tokio::time::timeout(Duration::from_secs(10), &mut task).await {
        Ok(result) => {
            result??;
            Ok(())
        }
        Err(error) => {
            task.abort();
            // Retain and join the handle: dropping it would detach the listener.
            match task.await {
                Err(join) if join.is_cancelled() => {}
                other => {
                    other??;
                }
            }
            Err(error.into())
        }
    }
}

async fn scenario(root: &Path, injection: Injection) -> anyhow::Result<()> {
    let server = Arc::new(DaemonServer::new_with_paths(
        Some(root.join("config.json")),
        Some(root.join("auth.json")),
    ));
    let services = server.remote_state.machine_services.as_ref().context("machine services")?;
    ensure!(Arc::ptr_eq(&services.sessions, &server.session_service), "session authority differs");
    ensure!(Arc::ptr_eq(&services.workspaces, &server.session_service.workspace_service), "workspace authority differs");
    let backend: Arc<dyn crate::remote::backend::RemoteSessionBackend> = server.session_service.clone();
    ensure!(Arc::ptr_eq(&backend, &server.remote_state.session_backend), "HTTP backend differs");
    println!("A04 AUTHORITY session={:p} workspace={:p} backend={:p}", Arc::as_ptr(&services.sessions), Arc::as_ptr(&services.workspaces), Arc::as_ptr(&backend));
    let socket = root.join("fixture.sock");
    let mut ipc_resource = None;
    let mut http_resource = None;
    let mut owned_sessions = HashMap::new();
    let mut injected = false;
    let mut cancellation_observed = false;
    let (cancel, mut cancelled) = tokio::sync::oneshot::channel();
    let (reached, at_seam) = tokio::sync::oneshot::channel();
    // The deadline/cancellation is triggered only after the real PTY exists.
    // No elapsed-time luck is used to reach the resource seam.
    let result = {
        let setup_and_scenario = async {
            let listener = UnixListener::bind(&socket)?;
            let owner = Arc::clone(&server);
            let (stop, mut stopped) = tokio::sync::oneshot::channel();
            let ipc = tokio::spawn(async move {
                let mut clients = tokio::task::JoinSet::new();
                let mut result = loop {
                    tokio::select! {
                        _ = &mut stopped => break Ok(()),
                        result = listener.accept() => {
                            let (stream, _) = match result {
                                Ok(value) => value,
                                Err(error) => break Err(error),
                            };
                            let owner = Arc::clone(&owner);
                            clients.spawn(owner.handle_client(stream));
                        }
                    }
                };
                // Quiesce in-flight IPC before enumerating PTYs. An aborted spawn
                // could otherwise publish a PTY after cleanup took its snapshot.
                while let Some(client) = clients.join_next().await {
                    match client {
                        Ok(()) => {}
                        Err(error) => {
                            result = Err(std::io::Error::other(error));
                        }
                    }
                }
                result
            });
            ipc_resource = Some((stop, ipc));
            if injection == Injection::IpcSetup {
                injected = true;
                anyhow::bail!("injected IPC setup failure");
            }
            // Controlled RED mutation: HTTP receives a different authority, not the
            // production shared handles. No production bypass or alternate spawn path.
            let detached;
            let gateway = if std::env::var_os("A04_SPLIT_AUTHORITY").is_some() {
                detached = DaemonServer::new_with_paths(
                    Some(root.join("other-config.json")),
                    Some(root.join("other-auth.json")),
                );
                Arc::clone(detached.remote_state())
            } else {
                Arc::clone(server.remote_state())
            };
            let tcp = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
            let address = tcp.local_addr()?;
            let (http_stop, http_stopped) = tokio::sync::oneshot::channel();
            let router = crate::remote::server::create_remote_router(Arc::clone(&gateway));
            let http = tokio::spawn(async move {
                axum::serve(
                    tcp,
                    router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
                )
                .with_graceful_shutdown(async {
                    let _ = http_stopped.await;
                })
                .await
            });
            http_resource = Some((http_stop, http));
            if injection == Injection::HttpSetup {
                injected = true;
                anyhow::bail!("injected HTTP setup failure");
            }
            let handshake = request(
                &socket,
                serde_json::json!({"type":"handshake", "version":3}),
            )
            .await?;
            ensure!(
                handshake["pid"] == std::process::id(),
                "wrong IPC owner PID"
            );
            let spawn = |id: &str, cwd: Option<&str>| {
                serde_json::json!({
                    "type":"spawn", "clientRequestId":id, "workspaceId":"a04", "cols":80,"rows":24,
                    "shell":"/bin/sh", "cwd":cwd
                })
            };
            let denied = request(&socket, spawn("unregistered", None)).await?;
            ensure!(denied["type"] == "error", "unregistered spawn admitted");
            let project = root.join("project");
            let directory = project.clone();
            crate::ipc::run_blocking(move || {
                std::fs::create_dir(directory)
                    .map_err(|e| crate::ipc::IpcError::internal(e.to_string()))
            })
            .await
            .map_err(|error| anyhow::anyhow!("{error:?}"))?;
            let registered = request(&socket, serde_json::json!({"type":"registerWorkspace", "workspaceId":"a04", "repoRoot":project})).await?;
            ensure!(
                registered["type"] == "registerWorkspaceOk",
                "registration failed: {registered}"
            );
            let denied = request(&socket, spawn("escape", root.to_str())).await?;
            ensure!(denied["type"] == "error", "cwd escape admitted");
            let created = request(&socket, spawn("original", None)).await?;
            let id = created["sessionId"].as_str().context("spawn session ID")?;
            let session = server
                .terminal_service
                .get_session(id)
                .context("original PTY")?;
            let pid = session.pid().context("original PID")?;
            owned_sessions.insert(id.to_string(), Arc::clone(&session));
            println!(
                "A04 OWNED owner_pid={} session={id} pty_pid={pid} root={}",
                std::process::id(),
                root.display()
            );
            if injection == Injection::Cleanup {
                let sibling = request(&socket, spawn("cleanup-sibling", None)).await?;
                let sibling_id = sibling["sessionId"]
                    .as_str()
                    .context("sibling session ID")?;
                let sibling = server
                    .terminal_service
                    .get_session(sibling_id)
                    .context("sibling PTY")?;
                owned_sessions.insert(sibling_id.to_string(), Arc::clone(&sibling));
                println!(
                    "A04 OWNED owner_pid={} session={sibling_id} pty_pid={:?} root={}",
                    std::process::id(),
                    sibling.pid(),
                    root.display()
                );
            }
            if injection == Injection::Scenario {
                injected = true;
                anyhow::bail!("injected scenario failure");
            }
            if matches!(injection, Injection::Cancel | Injection::Deadline) {
                reached
                    .send(())
                    .map_err(|_| anyhow::anyhow!("seam receiver closed"))?;
                std::future::pending::<()>().await;
            }
            let details = request(
                &socket,
                serde_json::json!({"type":"describeSession", "sessionId":id}),
            )
            .await?;
            ensure!(
                details["session"]["cwd"] == project.to_str().unwrap(),
                "IPC cwd mismatch"
            );
            let process_cwd =
                crate::ipc::run_blocking(move || Ok(crate::ipc::terminal::process_cwd(pid)))
                    .await
                    .map_err(|error| anyhow::anyhow!("{error:?}"))?;
            ensure!(
                process_cwd.as_ref() == Some(&project),
                "OS process cwd mismatch"
            );
            println!("A04 LIVE owner_pid={} pty_pid={pid} ipc={} http={address} metadata={details} process_cwd={process_cwd:?}", std::process::id(), socket.display());
            let code = gateway
                .auth_manager
                .create_pairing_code(DevicePermission::Control);
            let (token, _) = gateway.auth_manager.exchange_pairing_code(&code, "A04")?;
            let client = reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(10))
                .build()?;
            let response = client
                .get(format!("http://{address}/api/v1/sessions"))
                .bearer_auth(&token)
                .send()
                .await?;
            ensure!(response.status() == 200, "HTTP listing status");
            let body: serde_json::Value = serde_json::from_slice(&response.bytes().await?)?;
            println!(
                "A04 HTTP status=200 sessions={body} registry_ipc={} registry_http={}",
                server.workspace_registry.revision(),
                gateway.workspace_registry.revision()
            );
            ensure!(
                body.as_array()
                    .context("session list")?
                    .iter()
                    .any(|row| row["sessionId"] == id && row["workspaceId"] == "a04"),
                "HTTP cannot see original IPC session and workspace"
            );
            ensure!(session.pid() == Some(pid), "original process replaced");
            Ok::<_, anyhow::Error>(())
        };
        tokio::pin!(setup_and_scenario);
        let control = async {
            at_seam.await.context("PTY seam sender closed")?;
            cancel
                .send(())
                .map_err(|_| anyhow::anyhow!("cancel receiver closed"))?;
            Ok::<_, anyhow::Error>(())
        };
        tokio::pin!(control);
        let deadline = tokio::time::sleep(Duration::from_secs(30));
        tokio::pin!(deadline);
        let mut control_done = false;

        loop {
            tokio::select! {
                biased;
                value = &mut setup_and_scenario => break value,
                value = &mut control, if !control_done && matches!(injection, Injection::Cancel | Injection::Deadline) => {
                    if let Err(error) = value { break Err(error); }
                    control_done = true;
                    cancellation_observed = true;
                    if injection == Injection::Deadline {
                        deadline.as_mut().reset(tokio::time::Instant::now());
                    }
                }
                _ = &mut cancelled, if injection == Injection::Cancel => {
                    break Err(anyhow::anyhow!("scenario cancelled at_owned_pty={cancellation_observed}"));
                }
                _ = &mut deadline => break Err(anyhow::anyhow!("scenario deadline at_owned_pty={cancellation_observed}")),
            }
        }
    };
    if matches!(injection, Injection::Cancel | Injection::Deadline) && result.is_err() {
        // The control sender is consumed only at the owned-PTY barrier.
        injected = cancellation_observed;
    }
    // Teardown precedes propagating every scenario assertion failure.
    // Accumulate errors; never let one failed close skip another resource.
    let mut errors = Vec::new();
    let mut listeners_joined = true;
    for (name, resource) in [("IPC", ipc_resource), ("HTTP", http_resource)] {
        if let Some((stop, task)) = resource {
            if stop.send(()).is_err() {
                errors.push(format!("{name} stop receiver closed"));
            }
            if let Err(error) = join_listener(task).await {
                listeners_joined = false;
                errors.push(format!("{name}: {error:#}"));
                println!("A04 CLEANUP listener={name} joined=false");
            } else {
                println!("A04 CLEANUP listener={name} joined=true");
            }
        }
    }
    let mut closed = 0;
    for id in server.terminal_service.list_sessions() {
        let Some(session) = server.terminal_service.get_session(&id) else {
            errors.push(format!("missing owned PTY {id}"));
            continue;
        };
        owned_sessions.entry(id).or_insert(session);
    }
    for (id, session) in owned_sessions {
        let pid = session.pid();
        let close_result = if injection == Injection::Cleanup && closed == 0 {
            injected = true;
            Err(crate::terminal::PtyError::Other(
                "injected close failure".into(),
            ))
        } else {
            server.handle_close(&id).await
        };
        if let Err(error) = close_result {
            errors.push(format!("close {id}: {error}"));
            // Retry the real operation while retaining the original handle;
            // a failed first close must not skip this PTY or its sibling.
            if let Err(error) = server.handle_close(&id).await {
                errors.push(format!("close retry {id}: {error}"));
            }
        }
        if !session.is_reaped() {
            // Keep the original child handle, even if registry close failed.
            let owned = Arc::clone(&session);
            if let Err(error) = crate::ipc::run_blocking(move || {
                owned
                    .kill()
                    .map_err(|e| crate::ipc::IpcError::internal(e.to_string()))?;
                owned
                    .wait_and_reap()
                    .map_err(|e| crate::ipc::IpcError::internal(e.to_string()))
            })
            .await
            {
                errors.push(format!("reap {id}: {error}"));
            }
        }
        if !session.is_reaped() {
            errors.push(format!("PTY {id} not reaped"));
        }
        closed += 1;
        println!(
            "A04 CLEANUP session={id} pty_pid={pid:?} reaped={}",
            session.is_reaped()
        );
    }
    let owned_socket = socket.clone();
    if let Err(error) = crate::ipc::run_blocking(move || match std::fs::remove_file(owned_socket) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(crate::ipc::IpcError::internal(error.to_string())),
    })
    .await
    {
        errors.push(format!("socket removal: {error}"));
    }
    ensure!(!socket.exists(), "owned socket remains; errors={errors:?}");
    ensure!(
        server.terminal_service.list_sessions().is_empty(),
        "owned sessions remain; errors={errors:?}"
    );
    println!("A04 CLEANUP listeners_joined={listeners_joined} socket_removed=true closed={closed} injection={injection:?} errors={errors:?}");
    let expected_closed = match injection {
        Injection::None => None,
        Injection::IpcSetup | Injection::HttpSetup => Some(0),
        Injection::Scenario | Injection::Cancel | Injection::Deadline => Some(1),
        Injection::Cleanup => Some(2),
    };
    if let Some(expected) = expected_closed {
        ensure!(closed == expected, "expected {expected} owned PTYs, closed {closed}");
    }
    if injection == Injection::Cleanup {
        ensure!(
            injected && errors.len() == 1 && closed == 2,
            "cleanup injection not isolated: {errors:?}"
        );
        return result;
    }
    ensure!(errors.is_empty(), "cleanup errors: {errors:?}");
    if injection != Injection::None {
        ensure!(
            injected && result.is_err(),
            "injection not observed: {injection:?}"
        );
        return Ok(());
    }
    result
}

#[test]
fn a04_shared_authority_runtime() -> anyhow::Result<()> {
    run_private_fixture(Injection::None)
}

#[test]
fn a04_cleanup_failure_paths() -> anyhow::Result<()> {
    for injection in [
        Injection::IpcSetup,
        Injection::HttpSetup,
        Injection::Scenario,
        Injection::Cleanup,
        Injection::Cancel,
        Injection::Deadline,
    ] {
        run_private_fixture(injection)?;
    }
    Ok(())
}

fn run_private_fixture(injection: Injection) -> anyhow::Result<()> {
    if let Some(root) = std::env::var_os("A04_PRIVATE_ROOT") {
        let injection = match std::env::var("A04_INJECTION").as_deref() {
            Ok("IpcSetup") => Injection::IpcSetup,
            Ok("HttpSetup") => Injection::HttpSetup,
            Ok("Scenario") => Injection::Scenario,
            Ok("Cleanup") => Injection::Cleanup,
            Ok("Cancel") => Injection::Cancel,
            Ok("Deadline") => Injection::Deadline,
            Ok("None") => Injection::None,
            other => anyhow::bail!("invalid fixture injection {other:?}"),
        };
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        let result = runtime.block_on(scenario(Path::new(&root), injection));
        runtime.shutdown_timeout(Duration::from_secs(5));
        return result;
    }
    let root = tempfile::Builder::new().prefix("a04-").tempdir_in("/tmp")?;
    let path = root.path().canonicalize()?;
    println!("A04 ROOT path={}", path.display());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(async {
        let mut child = tokio::process::Command::new(std::env::current_exe()?)
            .args([
                "daemon::server::a04_shared_services_tests::a04_shared_authority_runtime",
                "--exact",
                "--nocapture",
            ])
            .env("A04_PRIVATE_ROOT", &path)
            .env("HOME", &path)
            .env("A04_INJECTION", format!("{injection:?}"))
            .env("FERRYX_DATA_DIR", &path)
            .env("FERRYX_RUNTIME_DIR", &path)
            .env_remove("FERRYX_MACHINE_TOKEN")
            .env_remove("FERRYX_RELAY_URL")
            .kill_on_drop(true)
            .spawn()?;
        let pid = child.id().context("fixture PID")?;
        // The deadline lives INSIDE the child, outside the scenario future but
        // inside server ownership. Do not kill this owner before it reaps PTYs.
        let status = child.wait().await?;
        println!("A04 CLEANUP child_pid={pid} reaped=true exit={status}");
        ensure!(status.success(), "private runtime failed");
        Ok::<_, anyhow::Error>(())
    });
    root.close()?;
    println!("A04 CLEANUP private_root_removed=true");
    result
}

#[test]
fn a04_services_have_no_strong_owner_cycle() {
    let root = tempfile::tempdir().unwrap();
    let server = DaemonServer::new_with_paths(Some(root.path().join("config.json")), Some(root.path().join("auth.json")));
    let sessions = Arc::downgrade(&server.session_service);
    let workspaces = Arc::downgrade(&server.session_service.workspace_service);
    let gateway = Arc::downgrade(server.remote_state());
    let handover = Arc::downgrade(&server.handover_manager);
    let terminal = Arc::downgrade(server.terminal_service());
    drop(server);
    assert!(sessions.upgrade().is_none());
    assert!(workspaces.upgrade().is_none());
    assert!(gateway.upgrade().is_none());
    assert!(handover.upgrade().is_none());
    assert!(terminal.upgrade().is_none());
}

#[test]
fn a04_legacy_constructors_have_no_machine_authority() {
    let terminal = Arc::new(TerminalService::default());
    let registry = WorkspaceRegistry::new();
    let state = RemoteGatewayState::new(Arc::clone(&terminal), registry.clone());
    assert!(state.machine_services.is_none());
    let state = RemoteGatewayState::new_with_paths(Arc::clone(&terminal), registry.clone(), None, None);
    assert!(state.machine_services.is_none());
    let state = RemoteGatewayState::new_with_backend(terminal, registry);
    assert!(state.machine_services.is_none());
}
