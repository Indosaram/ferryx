//! Proof-only actual owner HTTP/UDS seams, isolated in an exact-selected process.
use crate::daemon::{
    client::DaemonClient,
    protocol::{DaemonRequest, DaemonResponse},
    server::DaemonServer,
};
use crate::remote::{
    auth::{DeviceAccessScope, DevicePermission},
    server::create_remote_router,
};
use futures_util::FutureExt;
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

const LIMIT: Duration = Duration::from_secs(45);
const CHILD: &str = "remote::workspace_api::worktrees::wire_proof_tests::private_wire_owner";

#[tokio::test]
async fn non_head_and_partial_prune_wires() {
    let root = tokio::task::spawn_blocking(|| {
        tempfile::Builder::new()
            .prefix("a08-wire-")
            .tempdir_in("/tmp")
            .unwrap()
    })
    .await
    .unwrap();
    let path = root.path().to_owned();
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", CHILD, "--nocapture"])
        .env("A08_WIRE_ROOT", &path)
        .env("FERRYX_RUNTIME_DIR", path.join("runtime"))
        .env("FERRYX_DATA_DIR", path.join("data"))
        .env("FERRYX_SESSION_DIR", path.join("sessions"))
        .env("HOME", path.join("home"))
        .env("XDG_CONFIG_HOME", path.join("home/config"))
        .env("XDG_DATA_HOME", path.join("home/data"))
        .current_dir(&path)
        .kill_on_drop(true);
    let mut child = command.spawn().unwrap();
    let pid = child.id().unwrap();
    let waited = tokio::time::timeout(Duration::from_secs(180), child.wait()).await;
    let timed_out = waited.is_err();
    let status = match waited {
        Ok(status) => status.unwrap(),
        Err(_) => {
            child.kill().await.unwrap();
            child.wait().await.unwrap()
        }
    };
    let receipt = path.clone();
    tokio::task::spawn_blocking(move || {
        root.close().unwrap();
        assert!(!path.exists());
    })
    .await
    .unwrap();
    eprintln!("A08_WIRE_PARENT owner_pid={pid} reaped=true status={status} timeout={timed_out} root={} absent=true", receipt.display());
    assert!(!timed_out && status.success());
}

#[test]
fn private_wire_owner() {
    let Some(root) = std::env::var_os("A08_WIRE_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    for dir in ["runtime", "data", "sessions", "home", "repo"] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let outcome = runtime.block_on(std::panic::AssertUnwindSafe(wires(&root)).catch_unwind());
    drop(runtime);
    // This private process spawns no PTYs or unrelated subprocesses. All synchronous
    // Git calls and the budgeted owner's Git calls must have waited their children.
    let mut status = 0;
    assert_eq!(unsafe { libc::waitpid(-1, &mut status, libc::WNOHANG) }, -1);
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
    eprintln!(
        "A08_WIRE_OWNER pid={} runtime_joined=true git_children=ECHILD failed={}",
        std::process::id(),
        outcome.is_err()
    );
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

async fn git(path: &Path, args: &[&str]) -> Result<String, String> {
    let path = path.to_owned();
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    tokio::task::spawn_blocking(move || {
        crate::worktree::run_git(path, &args.iter().map(String::as_str).collect::<Vec<_>>())
            .map(|s| s.trim().to_owned())
            .map_err(|e| e.to_string())
    })
    .await
    .unwrap()
}

async fn http(
    client: &reqwest::Client,
    method: reqwest::Method,
    endpoint: &str,
    token: &str,
    body: Value,
) -> (u16, Value) {
    eprintln!("A08_WIRE_HTTP_REQUEST method={method} endpoint={endpoint} body={body}");
    let response = client
        .request(method, endpoint)
        .bearer_auth(token)
        .header("content-type", "application/json")
        .body(body.to_string())
        .send()
        .await
        .unwrap();
    let status = response.status().as_u16();
    let bytes = response.bytes().await.unwrap();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    eprintln!("A08_WIRE_HTTP_RESPONSE status={status} body={value}");
    (status, value)
}

async fn uds(client: &DaemonClient, request: DaemonRequest) -> Value {
    eprintln!(
        "A08_WIRE_UDS_REQUEST {}",
        serde_json::to_value(&request).unwrap()
    );
    let response = tokio::time::timeout(LIMIT, client.send_request(request))
        .await
        .unwrap()
        .unwrap();
    let wire = serde_json::to_value(response).unwrap();
    eprintln!("A08_WIRE_UDS_RESPONSE {wire}");
    wire
}

async fn wires(root: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let repo = root.join("repo");
    git(&repo, &["init", "--quiet"]).await.unwrap();
    for message in ["base", "head"] {
        git(
            &repo,
            &[
                "-c",
                "user.name=A08",
                "-c",
                "user.email=a08@example.invalid",
                "commit",
                "--allow-empty",
                "-m",
                message,
            ],
        )
        .await
        .unwrap();
    }
    let base = git(&repo, &["rev-parse", "HEAD~1"]).await.unwrap();
    let head = git(&repo, &["rev-parse", "HEAD"]).await.unwrap();
    assert_ne!(base, head);
    let (owner, workspace, machine, mirror, repo) = tokio::task::spawn_blocking({
        let root = root.to_owned();
        move || {
            let owner = Arc::new(DaemonServer::new_with_paths(
                Some(root.join("data/config")),
                Some(root.join("data/auth")),
            ));
            let state = owner.remote_state();
            let repo = std::fs::canonicalize(root.join("repo")).unwrap();
            let workspace = "wire-proof".to_owned();
            state
                .machine_services
                .as_ref()
                .unwrap()
                .workspaces
                .register(&workspace, repo.to_str().unwrap())
                .unwrap();
            let token = |scope| {
                let pin = state
                    .auth_manager
                    .create_scoped_pairing_code(DevicePermission::Control, scope)
                    .unwrap();
                state
                    .auth_manager
                    .exchange_pairing_code(&pin, "wire-proof")
                    .unwrap()
                    .0
            };
            let machine = token(DeviceAccessScope::Machine);
            let mirror = token(DeviceAccessScope::Mirror);
            std::fs::set_permissions(root.join("runtime"), std::fs::Permissions::from_mode(0o700))
                .unwrap();
            (owner, workspace, machine, mirror, repo)
        }
    })
    .await
    .unwrap();
    let socket = root.join("runtime/daemon.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    tokio::task::spawn_blocking({
        let socket = socket.clone();
        move || std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o600)).unwrap()
    })
    .await
    .unwrap();
    let (stop_uds, mut stopped_uds) = tokio::sync::oneshot::channel();
    let uds_owner = owner.clone();
    let uds_task = tokio::spawn(async move {
        let mut clients = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                _ = &mut stopped_uds => break,
                accepted = listener.accept() => { let (stream, _) = accepted.unwrap(); clients.spawn(uds_owner.clone().handle_client(stream)); }
            }
        }
        clients.shutdown().await;
    });
    let tcp = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = tcp.local_addr().unwrap();
    let (stop_http, stopped_http) = tokio::sync::oneshot::channel();
    let state = owner.remote_state().clone();
    let gateway = tokio::spawn(async move {
        axum::serve(tcp, create_remote_router(state))
            .with_graceful_shutdown(async {
                let _ = stopped_http.await;
            })
            .await
            .unwrap();
    });
    let service = owner
        .remote_state()
        .machine_services
        .as_ref()
        .unwrap()
        .workspaces
        .clone();
    let local = DaemonClient::new_with_socket(socket.clone());
    let outcome = std::panic::AssertUnwindSafe(async {
        let handshake = tokio::time::timeout(LIMIT, local.send_request(DaemonRequest::Handshake { version: crate::daemon::protocol::DAEMON_PROTOCOL_VERSION })).await.unwrap().unwrap();
        assert!(matches!(handshake, DaemonResponse::HandshakeOk { pid, .. } if pid == std::process::id()));
        eprintln!("A08_WIRE_HANDSHAKE owner_pid={} exact=true socket={} http={address}", std::process::id(), socket.display());
        let client = reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).timeout(LIMIT).build().unwrap();
        let endpoint = format!("http://{address}/api/v1/workspace/worktrees");
        let (status, row) = http(&client, reqwest::Method::POST, &endpoint, &machine, json!({"requestId":uuid::Uuid::new_v4(),"workspaceId":workspace,"worktree":{"wsId":workspace,"slug":"non-head"},"baseRef":"HEAD~1"})).await;
        assert_eq!(status, 201, "first fault-free response");
        let path = repo.join(".orca-worktrees").join(&workspace).join("non-head");
        assert_eq!(row["path"], path.to_str().unwrap());
        assert_eq!(row["head"], base);
        assert_eq!(git(&path, &["rev-parse", "HEAD"]).await.unwrap(), base);
        eprintln!("A08_WIRE_NON_HEAD first_http=201 base={base} original_head={head} actual_checkout_equal=true");
        for legacy in [false, true] {
            let slug = if legacy { "legacy-prune" } else { "local-prune" };
            let identity = crate::worktree::WorktreeIdentity { ws_id: workspace.clone(), slug: slug.into() };
            let created = uds(&local, DaemonRequest::CreateWorktree { workspace_id: workspace.clone(), worktree: identity.clone(), base_ref: None }).await;
            assert_eq!(created["type"], "createWorktreeOk");
            // Both owner paths call worktree_gate on the same blocking worker that
            // executes prune. Install the existing thread-local real-Git fault there.
            *service.transaction_probe.write() = Some(Arc::new(|phase| if phase == "workspaceGateRequested" {
                crate::worktree::manager::PRUNE_PROBE.with(|probe| *probe.borrow_mut() = Some(Box::new(|repo, before| {
                    let config = repo.join(".git/config"); let backup = repo.join("prune-config-backup");
                    if before {
                        let original = std::fs::read(&config).unwrap();
                        let text = String::from_utf8(original.clone()).unwrap();
                        assert!(text.contains("repositoryformatversion = 0"));
                        std::fs::write(&backup, original).unwrap();
                        std::fs::write(config, text.replace("repositoryformatversion = 0", "repositoryformatversion = 999")).unwrap();
                    } else {
                        std::fs::write(config, std::fs::read(&backup).unwrap()).unwrap();
                        std::fs::remove_file(backup).unwrap();
                    }
                })));
            }));
            let mut events = service.subscribe_worktree_changes();
            let error = if legacy {
                let (status, body) = http(&client, reqwest::Method::DELETE, &endpoint, &mirror, json!({"workspaceId":workspace,"worktree":{"wsId":workspace,"slug":slug},"deleteBranch":true})).await;
                assert_eq!(status, 409);
                body["error"].clone()
            } else {
                let body = uds(&local, DaemonRequest::DeleteWorktree { workspace_id: workspace.clone(), worktree: identity, delete_branch: true, destructive: false }).await;
                assert_eq!(body["type"], "worktreeError"); body["error"].clone()
            };
            *service.transaction_probe.write() = None;
            let event = tokio::time::timeout(LIMIT, events.recv()).await.unwrap().unwrap();
            assert!(event.removed); assert_eq!(event.workspace_id, workspace);
            assert_eq!(error["code"], "WORKTREE_REMOVED_PRUNE_FAILED");
            assert_eq!(error["details"]["worktreeRemoved"], true);
            assert_eq!(error["details"]["pruned"], false);
            let path = repo.join(".orca-worktrees").join(&workspace).join(slug);
            if legacy {
                assert_eq!(error["details"]["branchDeleted"], false);
                assert!(!error.to_string().contains(repo.to_str().unwrap()));
                assert!(error["details"].get("path").is_none());
                assert!(error["details"].get("cause").is_none());
            } else {
                assert_eq!(error["details"]["path"], path.to_str().unwrap());
                let cause = &error["details"]["cause"];
                assert_eq!(cause["code"], "GIT_ERROR");
                assert_ne!(cause["details"]["exitCode"].as_i64().unwrap(), 0);
                assert!(cause["details"]["command"].as_str().unwrap().contains("prune"));
                assert!(cause["details"]["stderr"].as_str().unwrap().contains("999"));
            }
            assert!(!tokio::fs::try_exists(&path).await.unwrap());
            assert_eq!(git(&repo, &["rev-parse", &format!("refs/heads/orca/{workspace}/{slug}")]).await.unwrap(), head);
            assert_eq!(git(&repo, &["config", "core.repositoryformatversion"]).await.unwrap(), "0");
            assert!(!tokio::fs::try_exists(repo.join("prune-config-backup")).await.unwrap());
            eprintln!("A08_WIRE_PRUNE legacy={legacy} removed=true branch_preserved=true config_restored=true publication_received=true");
        }
    }).catch_unwind().await;
    *service.transaction_probe.write() = None;
    drop(local);
    let _ = stop_uds.send(());
    let _ = stop_http.send(());
    tokio::time::timeout(LIMIT, uds_task)
        .await
        .unwrap()
        .unwrap();
    tokio::time::timeout(LIMIT, gateway).await.unwrap().unwrap();
    // Also restore a fixture mutation if an assertion interrupted the fault probe.
    tokio::task::spawn_blocking({
        let repo = repo.clone();
        move || {
            let backup = repo.join("prune-config-backup");
            if backup.exists() {
                std::fs::write(repo.join(".git/config"), std::fs::read(&backup).unwrap()).unwrap();
                std::fs::remove_file(backup).unwrap();
            }
        }
    })
    .await
    .unwrap();
    tokio::fs::remove_file(&socket).await.unwrap();
    assert!(tokio::net::UnixStream::connect(&socket).await.is_err());
    assert!(tokio::net::TcpStream::connect(address).await.is_err());
    drop(service);
    drop(owner);
    eprintln!("A08_WIRE_LISTENERS joined=true clients_joined=true tcp_refused=true uds_refused=true socket_removed=true failed={}", outcome.is_err());
    outcome.unwrap();
}
