use crate::{
    daemon::server::DaemonServer,
    remote::{
        auth::{DeviceAccessScope, DevicePermission},
        server::create_remote_router,
    },
};

#[tokio::test]
async fn r1_topology_after_gate_http() {
    use futures_util::FutureExt;
    use std::{sync::{Arc, Mutex}, time::Duration};
    for parent_git in [true, false] {
        let root = tempfile::tempdir().unwrap();
        let plain = root.path().join("parent/plain");
        std::fs::create_dir_all(&plain).unwrap();
        let canonical = std::fs::canonicalize(&plain).unwrap();
        let server = DaemonServer::new_with_paths(Some(root.path().join("data/config")), Some(root.path().join("data/auth")));
        let state = server.remote_state().clone();
        let service = &state.machine_services.as_ref().unwrap().workspaces;
        let pin = state.auth_manager.create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine).unwrap();
        let (token, _) = state.auth_manager.exchange_pairing_code(&pin, "topology").unwrap();
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let entered_tx = Mutex::new(Some(entered_tx));
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        *service.transaction_probe.write() = Some(Arc::new(move |phase| {
            if phase == "beforeGate" {
                entered_tx.lock().unwrap().take().unwrap().send(()).unwrap();
                release_rx.lock().unwrap().recv_timeout(Duration::from_secs(20)).unwrap();
            }
        }));
        let mut gate = Some(service.mutation_gate.lock());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let gateway_state = state.clone();
        let mut gateway = tokio::spawn(async move {
            axum::serve(listener, create_remote_router(gateway_state)).with_graceful_shutdown(async { let _ = stopped.await; }).await.unwrap();
        });
        let client = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(15)).build().unwrap();
        let payload = serde_json::json!({"requestId": uuid::Uuid::new_v4().to_string(), "repoPath": plain}).to_string();
        let call = client.post(format!("http://{addr}/api/v1/workspace/projects")).bearer_auth(&token).body(payload.clone());
        let mut request = tokio::spawn(async move { call.send().await.unwrap() });
        let outcome = std::panic::AssertUnwindSafe(async {
            tokio::time::timeout(Duration::from_secs(10), entered_rx).await.unwrap().unwrap();
            let target = if parent_git { plain.parent().unwrap() } else { plain.as_path() };
            let mut child = tokio::process::Command::new("git").args(["init", "--quiet"]).arg(target).kill_on_drop(true).spawn().unwrap();
            let pid = child.id().unwrap();
            let status = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
            if status.is_err() { child.start_kill().unwrap(); child.wait().await.unwrap(); }
            eprintln!("R1 topology git pid={pid} reaped=true parent_git={parent_git}");
            assert!(status.unwrap().unwrap().success());
            assert_eq!(std::fs::canonicalize(&plain).unwrap(), canonical);
            release_tx.send(()).unwrap();
            drop(gate.take());
            let response = (&mut request).await.unwrap();
            let status = response.status().as_u16();
            let body = response.text().await.unwrap();
            let catalog = service.catalog().unwrap();
            eprintln!("R1 topology parent_git={parent_git} status={status} rows={} revision={} body={body}", catalog.workspaces.len(), catalog.revision.0);
            if parent_git {
                assert_eq!(status, 400, "stale prepared root must not commit");
                let error: super::machine_protocol::ErrorEnvelope = serde_json::from_str(&body).unwrap();
                assert_eq!(error.error.code, "INVALID_PATH");
                assert!(catalog.workspaces.is_empty());
                assert_eq!(catalog.revision.0, 0);
            } else {
                assert_eq!(status, 201);
                let project: super::machine_protocol::Project = serde_json::from_str(&body).unwrap();
                assert_eq!(project.git_root.as_deref(), canonical.to_str());
                assert!(project.git_common_dir.is_some());
                assert_eq!(catalog.workspaces.len(), 1);
            }
            let replay = client.post(format!("http://{addr}/api/v1/workspace/projects")).bearer_auth(&token).body(payload).send().await.unwrap();
            assert_eq!(replay.status().as_u16(), status);
            assert_eq!(replay.text().await.unwrap(), body);
        }).catch_unwind().await;
        let _ = release_tx.send(());
        drop(gate.take());
        if !request.is_finished() { request.abort(); let _ = request.await; }
        let drained = tokio::time::timeout(Duration::from_secs(10), service.project_mutations.clone().acquire_many_owned(8)).await;
        *service.transaction_probe.write() = None;
        let _ = stop.send(());
        let joined = tokio::time::timeout(Duration::from_secs(10), &mut gateway).await;
        if joined.is_err() { gateway.abort(); let _ = gateway.await; }
        drop(gate);
        drop(state); drop(server);
        let receipt = root.path().to_owned(); root.close().unwrap();
        eprintln!("R1 topology cleanup parent_git={parent_git} workers_drained={} gateway_joined={} removed={}", drained.is_ok(), joined.is_ok(), !receipt.exists());
        drop(drained.unwrap().unwrap()); joined.unwrap().unwrap();
        if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
    }
}

#[tokio::test]
async fn a07_project_http_registration() {
    project_http_fixture(false).await;
}

#[tokio::test]
async fn a07_http_request_failure_joins_fixture() {
    project_http_fixture(true).await;
}

async fn project_http_fixture(inject_request_failure: bool) {
    use futures_util::FutureExt;
    let (root, plain, server) = crate::ipc::run_blocking(|| {
        let root = tempfile::tempdir().unwrap();
        let plain = root.path().join("plain");
        std::fs::create_dir(&plain).unwrap();
        let server = DaemonServer::new_with_paths(
            Some(root.path().join("data/config")),
            Some(root.path().join("data/auth")),
        );
        Ok((root, plain, server))
    })
    .await
    .unwrap();
    let state = server.remote_state().clone();
    let pin = state
        .auth_manager
        .create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine)
        .unwrap();
    let (token, _) = state
        .auth_manager
        .exchange_pairing_code(&pin, "A07 fixture")
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut task = tokio::spawn(async move {
        axum::serve(listener, create_remote_router(state))
            .with_graceful_shutdown(async {
                rx.await.unwrap();
            })
            .await
            .unwrap();
    });
    let outcome = std::panic::AssertUnwindSafe(async {
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap();
    let request_id = uuid::Uuid::new_v4().to_string();
    let payload = serde_json::json!({"requestId": request_id, "repoPath": plain});
    if inject_request_failure {
        // Unsupported scheme fails deterministically, without a timing race or
        // relying on a supposedly unused port remaining unbound.
        client.get("invalid-a07://fixture").send().await.unwrap();
    }
    let response = client
        .post(format!("http://{addr}/api/v1/workspace/projects"))
        .bearer_auth(&token)
        .header("content-type", "application/json")
        .body(payload.to_string())
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body = response.text().await.unwrap();
    if status.as_u16() == 201 {
        let project: super::machine_protocol::Project = serde_json::from_str(&body).unwrap();
        let retry = client
            .post(format!("http://{addr}/api/v1/workspace/projects"))
            .bearer_auth(&token)
            .body(payload.to_string())
            .send()
            .await
            .unwrap();
        assert_eq!(retry.status().as_u16(), 201);
        assert_eq!(retry.text().await.unwrap(), body);
        let conflict = client
            .post(format!("http://{addr}/api/v1/workspace/projects"))
            .bearer_auth(&token)
            .body(serde_json::json!({"requestId": request_id, "repoPath": "/"}).to_string())
            .send()
            .await
            .unwrap();
        assert_eq!(conflict.status().as_u16(), 409);
        let refused = client
            .post(format!("http://{addr}/api/v1/workspace/projects"))
            .bearer_auth(&token)
            .body(
                serde_json::json!({"requestId": uuid::Uuid::new_v4().to_string(), "repoPath": "/"})
                    .to_string(),
            )
            .send()
            .await
            .unwrap();
        assert_eq!(refused.status().as_u16(), 400);
        let inventory = client
            .get(format!("http://{addr}/api/v1/workspace/projects"))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap();
        let inventory: super::machine_protocol::Projects =
            serde_json::from_slice(&inventory.bytes().await.unwrap()).unwrap();
        assert_eq!(inventory.projects.len(), 1);
        #[cfg(unix)]
        {
            let alias = root.path().join("alias");
            std::os::unix::fs::symlink(&plain, &alias).unwrap();
            let state = server.remote_state();
            let pin = state
                .auth_manager
                .create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine)
                .unwrap();
            let (other, device) = state
                .auth_manager
                .exchange_pairing_code(&pin, "other device")
                .unwrap();
            let one = client.post(format!("http://{addr}/api/v1/workspace/projects")).bearer_auth(&token).body(serde_json::json!({"requestId": uuid::Uuid::new_v4().to_string(), "repoPath": plain}).to_string()).send();
            let two = client.post(format!("http://{addr}/api/v1/workspace/projects")).bearer_auth(&other).body(serde_json::json!({"requestId": uuid::Uuid::new_v4().to_string(), "repoPath": alias}).to_string()).send();
            let (one, two) = tokio::join!(one, two);
            for reply in [one.unwrap(), two.unwrap()] {
                assert_eq!(reply.status().as_u16(), 200);
                let p: super::machine_protocol::Project =
                    serde_json::from_slice(&reply.bytes().await.unwrap()).unwrap();
                assert_eq!(p.workspace_id, project.workspace_id);
            }
            assert!(state.auth_manager.revoke_device(&device.id));
            assert_eq!(
                client
                    .get(format!("http://{addr}/api/v1/workspace/projects"))
                    .bearer_auth(other)
                    .send()
                    .await
                    .unwrap()
                    .status()
                    .as_u16(),
                401
            );
        }
        let git_root = root.path().join("git");
        let nested = git_root.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        assert!(std::process::Command::new("git")
            .args(["init", "--quiet"])
            .arg(&git_root)
            .status()
            .unwrap()
            .success());
        let added = client.post(format!("http://{addr}/api/v1/workspace/projects")).bearer_auth(&token).body(serde_json::json!({"requestId": uuid::Uuid::new_v4().to_string(), "repoPath": nested}).to_string()).send().await.unwrap();
        assert_eq!(added.status().as_u16(), 201);
        let git: super::machine_protocol::Project =
            serde_json::from_slice(&added.bytes().await.unwrap()).unwrap();
        assert_eq!(
            git.git_root,
            Some(
                std::fs::canonicalize(&git_root)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .into()
            )
        );
        assert!(git.git_common_dir.is_some());
        assert!(git.git_head.is_none());
        let removed = client.delete(format!("http://{addr}/api/v1/workspace/projects/{}", project.workspace_id)).bearer_auth(&token).body(serde_json::json!({"requestId": uuid::Uuid::new_v4().to_string(), "expectedRevision": git.revision}).to_string()).send().await.unwrap();
        assert_eq!(removed.status().as_u16(), 204);
        assert!(plain.is_dir());
        eprintln!(
            "A07 HTTP register/replay/digest-conflict/root-refusal/list/unregister disk-preserved"
        );
    }
    assert_eq!(status.as_u16(), 201, "actual HTTP registration: {body}");
    }).catch_unwind().await;
    let signaled = tx.send(());
    let joined = tokio::time::timeout(std::time::Duration::from_secs(10), &mut task).await;
    if joined.is_err() {
        task.abort();
        let aborted = task.await;
        assert!(
            aborted.is_err(),
            "aborted HTTP server unexpectedly succeeded"
        );
    }
    drop(server);
    let restore = std::panic::AssertUnwindSafe(async {
        if inject_request_failure || outcome.is_err() {
            return;
        }
        let config = root.path().join("data/config");
        let auth = root.path().join("data/auth");
        let restored = crate::ipc::run_blocking(move || {
            Ok(DaemonServer::new_with_paths(Some(config), Some(auth)))
        })
        .await
        .unwrap();
        let catalog = restored
            .remote_state()
            .machine_services
            .as_ref()
            .unwrap()
            .workspaces
            .catalog()
            .unwrap();
        assert_eq!(catalog.workspaces.len(), 1);
        assert!(catalog
            .workspaces
            .values()
            .all(|r| r.repo_root.ends_with("git")));
        drop(restored);
    })
    .catch_unwind()
    .await;
    let receipt = root.path().to_owned();
    crate::ipc::run_blocking(move || {
        root.close().unwrap();
        Ok(())
    })
    .await
    .unwrap();
    eprintln!("A07 cleanup HTTP task joined; temporary root removed; injected={inject_request_failure}; root={}", receipt.display());
    signaled.expect("signal fixture shutdown");
    joined
        .expect("bounded fixture shutdown")
        .expect("join fixture HTTP task");
    if inject_request_failure {
        assert!(outcome.is_err(), "request failure injection must fail");
    } else if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    if let Err(panic) = restore {
        std::panic::resume_unwind(panic);
    }
}

#[test]
fn a07_journal_restart_and_device_scope() {
    use super::{
        machine_operation_journal::{Begin, MachineOperationJournal},
        machine_protocol::{Operation, OperationOutcome},
    };
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("journal.json");
    let id = uuid::Uuid::new_v4().to_string();
    let journal = MachineOperationJournal::open(path.clone());
    assert!(matches!(
        journal
            .begin("a", &id, "registerProject", "digest", "resource")
            .unwrap(),
        Begin::New
    ));
    assert!(journal.reconcile("b", &id).unwrap().is_none());
    assert_eq!(
        journal
            .begin("a", &id, "registerProject", "different", "resource")
            .err()
            .unwrap(),
        "REQUEST_CONFLICT"
    );
    drop(journal);
    let journal = MachineOperationJournal::open(path.clone());
    assert!(matches!(
        journal.reconcile("a", &id).unwrap().unwrap().operation,
        Operation::OutcomeUnknown { .. }
    ));
    journal
        .complete("a", &id, 204, OperationOutcome::NoContent)
        .unwrap();
    drop(journal);
    let journal = MachineOperationJournal::open(path.clone());
    assert!(
        matches!(journal.begin("a", &id, "registerProject", "digest", "resource").unwrap(), Begin::Existing(r) if matches!(r.operation, Operation::Completed { .. }))
    );
    drop(journal);
    let mut store: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    for record in store["records"].as_object_mut().unwrap().values_mut() {
        record["completedAt"] = serde_json::json!(0);
    }
    std::fs::write(&path, serde_json::to_vec(&store).unwrap()).unwrap();
    let journal = MachineOperationJournal::open(path.clone());
    journal
        .begin(
            "a",
            &uuid::Uuid::new_v4().to_string(),
            "registerProject",
            "next",
            "next",
        )
        .unwrap();
    assert!(
        matches!(journal.begin("a", &id, "registerProject", "digest", "resource").unwrap(), Begin::Existing(r) if matches!(r.operation, Operation::ResultExpired { .. }))
    );
    drop(journal);
    let journal = MachineOperationJournal::open(path);
    assert!(matches!(
        journal.reconcile("a", &id).unwrap().unwrap().operation,
        Operation::ResultExpired { .. }
    ));
    drop(journal);
    root.close().unwrap();
}

#[tokio::test]
#[cfg(unix)]
async fn r12_unreadable_root() {
    use std::os::unix::fs::PermissionsExt;
    use axum::{extract::State, http::HeaderMap};
    let (root, plain, server) = crate::ipc::run_blocking(|| {
        let root = tempfile::tempdir().unwrap();
        let plain = root.path().join("plain");
        std::fs::create_dir(&plain).unwrap();
        let server = DaemonServer::new_with_paths(Some(root.path().join("data/config")), Some(root.path().join("data/auth")));
        server.remote_state().machine_services.as_ref().unwrap().workspaces.register_machine(plain.to_str().unwrap()).unwrap();
        Ok((root, plain, server))
    }).await.unwrap();
    let state = server.remote_state().clone();
    let pin = state.auth_manager.create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine).unwrap();
    let (token, _) = state.auth_manager.exchange_pairing_code(&pin, "repair").unwrap();
    let mut headers = HeaderMap::new();
    headers.insert("authorization", format!("Bearer {token}").parse().unwrap());
    std::fs::set_permissions(&plain, std::fs::Permissions::from_mode(0)).unwrap();
    let response = super::workspace_api::list(State(state.clone()), headers.clone()).await;
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    std::fs::set_permissions(&plain, std::fs::Permissions::from_mode(0o700)).unwrap();
    let replacement = root.path().join("replacement");
    std::fs::create_dir(&replacement).unwrap();
    std::fs::remove_dir(&plain).unwrap();
    std::os::unix::fs::symlink(&replacement, &plain).unwrap();
    let replaced = super::workspace_api::list(State(state), headers).await;
    let replaced = axum::body::to_bytes(replaced.into_body(), 1024 * 1024).await.unwrap();
    drop(server);
    let receipt = root.path().to_owned();
    root.close().unwrap();
    eprintln!("R12 cleanup removed={}", !receipt.exists());
    let inventory: super::machine_protocol::Projects = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(inventory.projects[0].availability, super::machine_protocol::Availability::PermissionDenied);
    let replaced: super::machine_protocol::Projects = serde_json::from_slice(&replaced).unwrap();
    assert_eq!(replaced.projects[0].availability, super::machine_protocol::Availability::Invalid);
}

#[tokio::test]
async fn r12_revocation_fences() {
    use axum::{extract::State, http::HeaderMap, body::Bytes};
    for phase in ["prepare", "beforeGate", "beforeCommit", "beforeResponse", "listProbe"] {
        let root = tempfile::tempdir().unwrap();
        let plain = root.path().join("plain");
        std::fs::create_dir(&plain).unwrap();
        let server = DaemonServer::new_with_paths(Some(root.path().join("data/config")), Some(root.path().join("data/auth")));
        let state = server.remote_state().clone();
        let service = &state.machine_services.as_ref().unwrap().workspaces;
        let gate = if phase == "beforeGate" { Some(service.mutation_gate.lock()) } else { None };
        let pin = state.auth_manager.create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine).unwrap();
        let (token, device) = state.auth_manager.exchange_pairing_code(&pin, "repair").unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("authorization", format!("Bearer {token}").parse().unwrap());
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let entered_tx = std::sync::Mutex::new(Some(entered_tx));
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let release_rx = std::sync::Mutex::new(release_rx);
        *service.transaction_probe.write() = Some(std::sync::Arc::new(move |at| {
            if at == phase {
                entered_tx.lock().unwrap().take().unwrap().send(()).unwrap();
                release_rx.lock().unwrap().recv_timeout(std::time::Duration::from_secs(10)).unwrap();
            }
        }));
        let id = uuid::Uuid::new_v4().to_string();
        let payload = Bytes::from(serde_json::json!({"requestId": id.clone(), "repoPath": plain}).to_string());
        let task = if phase == "listProbe" {
            tokio::spawn(super::workspace_api::list(State(state.clone()), headers))
        } else {
            tokio::spawn(super::workspace_api::register(State(state.clone()), headers, payload))
        };
        let entered = tokio::time::timeout(std::time::Duration::from_secs(10), entered_rx).await;
        state.auth_manager.revoke_device(&device.id);
        let response = task.await.unwrap();
        release_tx.send(()).unwrap();
        drop(gate);
        // Acquiring every slot signals that the detached blocking worker really
        // finished; it is not a timing inference from the HTTP response.
        let budget = if phase == "listProbe" { &service.project_reads } else { &service.project_mutations };
        let slots = tokio::time::timeout(std::time::Duration::from_secs(10), budget.clone().acquire_many_owned(8)).await.unwrap().unwrap();
        let rows = service.catalog().unwrap().workspaces.len();
        drop(slots);
        *service.transaction_probe.write() = None;
        drop(state); drop(server);
        let receipt = root.path().to_owned(); root.close().unwrap();
        assert!(entered.unwrap().is_ok());
        assert_eq!(response.status().as_u16(), 401);
        let body = axum::body::to_bytes(response.into_body(), 65536).await.unwrap();
        let error: super::machine_protocol::ErrorEnvelope = serde_json::from_slice(&body).unwrap();
        if phase != "listProbe" { assert_eq!(error.error.request_id, id, "revocation must reconcile the original request"); }
        assert_eq!(rows, usize::from(phase == "beforeResponse"));
        eprintln!("R12 phase={phase} revoked=401 rows={rows} workers_finished=true removed={}", !receipt.exists());
    }
}

#[test]
fn r12_crash_owner() {
    use std::io::{BufRead, Write};
    let Some(root) = std::env::var_os("R12_ROOT") else { return; };
    let root = std::path::PathBuf::from(root);
    let server = DaemonServer::new_with_paths(Some(root.join("data/config")), Some(root.join("data/auth")));
    let state = server.remote_state().clone();
    let phase = std::env::var("R12_PHASE").unwrap();
    let request = std::env::var("R12_REQUEST").unwrap();
    let credentials = root.join("credentials.json");
    let token = if credentials.exists() {
        serde_json::from_slice::<String>(&std::fs::read(&credentials).unwrap()).unwrap()
    } else {
        let pin = state.auth_manager.create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine).unwrap();
        let (token, _) = state.auth_manager.exchange_pairing_code(&pin, "crash").unwrap();
        super::auth::write_private_json(&credentials, &token).unwrap(); token
    };
    if phase != "replay" {
        *state.machine_services.as_ref().unwrap().workspaces.transaction_probe.write() = Some(std::sync::Arc::new(move |at| {
            if at == phase {
                // Serial libtest prints its test-name prefix without a newline.
                println!("\nR12_WINDOW"); std::io::stdout().flush().unwrap();
                let mut line = String::new(); std::io::stdin().lock().read_line(&mut line).unwrap();
                panic!("kill window unexpectedly released");
            }
        }));
    }
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
        let gateway = tokio::spawn(async move { axum::serve(listener, create_remote_router(state)).with_graceful_shutdown(async { let _ = stop_rx.await; }).await.unwrap(); });
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let deleting = std::env::var_os("R12_DELETE").is_some();
        let call = if deleting {
            let id = std::fs::read_to_string(root.join("workspace-id")).unwrap();
            client.delete(format!("http://{addr}/api/v1/workspace/projects/{id}"))
                .body(serde_json::json!({"requestId": request, "expectedRevision": "1"}).to_string())
        } else {
            client.post(format!("http://{addr}/api/v1/workspace/projects"))
                .body(serde_json::json!({"requestId": request, "repoPath": root.join("plain")}).to_string())
        };
        let response = call.bearer_auth(token).send().await.unwrap();
        let status = response.status(); let body = response.bytes().await.unwrap();
        stop_tx.send(()).unwrap(); gateway.await.unwrap();
        if deleting {
            assert_eq!(status.as_u16(), 204);
            assert!(root.join("plain").is_dir());
            println!("\nR12_REPLAY unregister 204");
        } else {
            assert_eq!(status.as_u16(), 201);
            let p: super::machine_protocol::Project = serde_json::from_slice(&body).unwrap();
            assert_eq!(p.revision.0, 1);
            std::fs::write(root.join("workspace-id"), &p.workspace_id).unwrap();
            println!("\nR12_REPLAY {} {}", p.workspace_id, p.revision.0);
        }
    });
}

#[tokio::test]
async fn r12_real_kill_windows() {
    use tokio::io::{AsyncBufReadExt, BufReader};
    for phase in ["afterCatalog", "afterJournal"] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_owned(); std::fs::create_dir(root.join("plain")).unwrap();
        let request = uuid::Uuid::new_v4().to_string();
        let delete_request = uuid::Uuid::new_v4().to_string();
        for (current, deleting) in [(phase, false), ("replay", false), (phase, true), ("replay", true)] {
            let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
            command.args(["--exact", "remote::workspace_api_tests::r12_crash_owner", "--nocapture"])
                .env("R12_ROOT", &root).env("R12_PHASE", current).env("R12_REQUEST", if deleting { &delete_request } else { &request })
                .env("HOME", &root).env("FERRYX_DATA_DIR", root.join("data")).env("FERRYX_RUNTIME_DIR", root.join("runtime"))
                .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).kill_on_drop(true);
            if deleting { command.env("R12_DELETE", "1"); }
            let mut child = command.spawn().unwrap(); let pid = child.id().unwrap();
            // Child::wait closes its owned stdin before reaping. Keep the barrier
            // writer alive independently so EOF cannot race SIGKILL delivery.
            let barrier_stdin = child.stdin.take().unwrap();
            let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
            let result = tokio::time::timeout(std::time::Duration::from_secs(20), async {
                while let Some(line) = lines.next_line().await? {
                    if (current != "replay" && line == "R12_WINDOW") || (current == "replay" && line.starts_with("R12_REPLAY ")) {
                        eprintln!("R12 signal phase={current} deleting={deleting} pid={pid} {line}");
                        return Ok::<_, std::io::Error>(true);
                    }
                } Ok(false)
            }).await;
            let kill = if current != "replay" || !matches!(result, Ok(Ok(true))) { child.start_kill() } else { Ok(()) };
            let waited = tokio::time::timeout(std::time::Duration::from_secs(20), child.wait()).await;
            let status = match waited {
                Ok(Ok(status)) => status,
                failure => {
                    let cleanup_kill = child.start_kill();
                    let cleanup_reap = child.wait().await;
                    drop(barrier_stdin);
                    temp.close().unwrap();
                    panic!("child wait failed: {failure:?}; kill={kill:?}; cleanup kill={cleanup_kill:?} reap={cleanup_reap:?}");
                }
            };
            drop(barrier_stdin);
            eprintln!("R12 process phase={current} deleting={deleting} pid={pid} reaped=true status={status}");
            if kill.is_err() || !matches!(result, Ok(Ok(true))) {
                temp.close().unwrap(); panic!("child barrier failed: signal={result:?} kill={kill:?}");
            }
            if current == "replay" { assert!(status.success()); } else {
                #[cfg(unix)] {
                    use std::os::unix::process::ExitStatusExt;
                    assert_eq!(status.signal(), Some(libc::SIGKILL), "{status}");
                }
                // std::process::Child::kill uses TerminateProcess(handle, 1).
                #[cfg(windows)]
                assert_eq!(status.code(), Some(1), "{status}");
            }
        }
        temp.close().unwrap(); eprintln!("R12 kill-window={phase} same_device_request_replay=201 revision=1 removed={}", !root.exists());
    }
}

#[tokio::test]
async fn r12_list_gate_and_admission() {
    use axum::{extract::State, http::HeaderMap};
    let temp = tempfile::tempdir().unwrap();
    let server = DaemonServer::new_with_paths(Some(temp.path().join("data/config")), Some(temp.path().join("data/auth")));
    let state = server.remote_state().clone();
    let service = &state.machine_services.as_ref().unwrap().workspaces;
    let pin = state.auth_manager.create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine).unwrap();
    let (token, _) = state.auth_manager.exchange_pairing_code(&pin, "limits").unwrap();
    let mut headers = HeaderMap::new(); headers.insert("authorization", format!("Bearer {token}").parse().unwrap());
    let all = service.project_mutations.clone().acquire_many_owned(8).await.unwrap();
    let capacity = super::workspace_api::register(State(state.clone()), headers.clone(), axum::body::Bytes::new()).await;
    assert_eq!(capacity.status().as_u16(), 429); drop(all);
    let capacity_body = axum::body::to_bytes(capacity.into_body(), 65536).await.unwrap();
    let capacity_error: super::machine_protocol::ErrorEnvelope = serde_json::from_slice(&capacity_body).unwrap();
    let reads = service.project_reads.clone().acquire_many_owned(8).await.unwrap();
    let denied = super::workspace_api::list(State(state.clone()), HeaderMap::new()).await;
    assert_eq!(denied.status().as_u16(), 401);
    let pin = state.auth_manager.create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Mirror).unwrap();
    let (mirror, _) = state.auth_manager.exchange_pairing_code(&pin, "mirror").unwrap();
    let mut mirror_headers = HeaderMap::new(); mirror_headers.insert("authorization", format!("Bearer {mirror}").parse().unwrap());
    let denied = super::workspace_api::list(State(state.clone()), mirror_headers).await;
    assert_eq!(denied.status().as_u16(), 403);
    drop(reads);
    let bare_auth = temp.path().join("service-less/auth.json");
    let bare = std::sync::Arc::new(super::state::RemoteGatewayState::new_with_paths_backend(
        state.session_backend.clone(), crate::worktree::WorkspaceRegistry::new(),
        Some(temp.path().join("service-less/config.json")), Some(bare_auth.clone()),
    ));
    assert_eq!(super::workspace_api::list(State(bare.clone()), HeaderMap::new()).await.status().as_u16(), 401);
    for (scope, expected) in [(DeviceAccessScope::Mirror, 403), (DeviceAccessScope::Machine, 503)] {
        let pin = bare.auth_manager.create_scoped_pairing_code(DevicePermission::Control, scope).unwrap();
        let (token, _) = bare.auth_manager.exchange_pairing_code(&pin, "service-less").unwrap();
        let mut auth = HeaderMap::new(); auth.insert("authorization", format!("Bearer {token}").parse().unwrap());
        assert_eq!(super::workspace_api::list(State(bare.clone()), auth).await.status().as_u16(), expected);
    }
    assert!(bare_auth.is_file());
    drop(bare);
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let entered_tx = std::sync::Mutex::new(Some(entered_tx));
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let release_rx = std::sync::Mutex::new(release_rx);
    *service.transaction_probe.write() = Some(std::sync::Arc::new(move |phase| {
        if phase == "listProbe" {
            entered_tx.lock().unwrap().take().unwrap().send(()).unwrap();
            release_rx.lock().unwrap().recv_timeout(std::time::Duration::from_secs(10)).unwrap();
        }
    }));
    let request = tokio::spawn(super::workspace_api::list(State(state.clone()), headers));
    let entered = tokio::time::timeout(std::time::Duration::from_secs(10), entered_rx).await;
    let gate_free = service.mutation_gate.try_lock().is_some();
    request.abort(); let joined = request.await;
    let retained = service.project_reads.available_permits() == 7;
    release_tx.send(()).unwrap();
    let finished = tokio::time::timeout(std::time::Duration::from_secs(10), service.project_reads.clone().acquire_many_owned(8)).await.unwrap().unwrap();
    drop(finished); *service.transaction_probe.write() = None;
    drop(state); drop(server); let root = temp.path().to_owned(); temp.close().unwrap();
    assert!(entered.unwrap().is_ok()); assert!(gate_free); assert!(joined.unwrap_err().is_cancelled()); assert!(retained);
    eprintln!("R12 mutation_capacity=429 list_probe_global_gate_free=true cancelled_worker_slot_retained=true worker_finished=true removed={}", !root.exists());
    assert_eq!(capacity_error.error.code, "CAPACITY_EXCEEDED");
    assert!(capacity_error.error.retryable, "capacity refusal must permit retry");
}

#[tokio::test]
async fn r12_router_incomplete_body() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let root = tempfile::tempdir().unwrap();
    let server = DaemonServer::new_with_paths(Some(root.path().join("data/config")), Some(root.path().join("data/auth")));
    let state = server.remote_state().clone();
    let pin = state.auth_manager.create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine).unwrap();
    let (token, _) = state.auth_manager.exchange_pairing_code(&pin, "body").unwrap();
    let (entry_tx, mut entry_rx) = tokio::sync::mpsc::unbounded_channel();
    *state.machine_services.as_ref().unwrap().workspaces.transaction_probe.write() = Some(std::sync::Arc::new(move |phase| { if phase == "bodyEntry" { entry_tx.send(()).unwrap(); } }));
    let retained_state = state.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let gateway = tokio::spawn(async move { axum::serve(listener, create_remote_router(state)).with_graceful_shutdown(async { let _ = stopped.await; }).await.unwrap(); });
    let mut socket = tokio::net::TcpStream::connect(addr).await.unwrap();
    socket.write_all(format!("POST /api/v1/workspace/projects HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\nContent-Length: 100\r\nConnection: close\r\n\r\n{{").as_bytes()).await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), entry_rx.recv()).await.unwrap().unwrap();
    let service = &retained_state.machine_services.as_ref().unwrap().workspaces;
    assert_eq!(service.project_mutations.available_permits(), 7);
    let remaining = service.project_mutations.clone().acquire_many_owned(7).await.unwrap();
    let capacity = reqwest::Client::builder().no_proxy().build().unwrap().post(format!("http://{addr}/api/v1/workspace/projects")).bearer_auth(&token).body("{}").send().await.unwrap();
    assert_eq!(capacity.status().as_u16(), 429);
    drop(capacity); drop(remaining);
    let mut bytes = vec![0; 4096];
    // Time is the contract: an unfinished body must have a ten-second read
    // deadline, rather than retaining its request until the peer closes it.
    let result = tokio::time::timeout(std::time::Duration::from_secs(12), socket.read(&mut bytes)).await;
    drop(socket); stop.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), gateway).await.unwrap().unwrap();
    *service.transaction_probe.write() = None;
    assert_eq!(service.project_mutations.available_permits(), 8);
    drop(retained_state);
    drop(server); root.close().unwrap();
    eprintln!("R12 incomplete body sockets_closed=true gateway_joined=true root_removed=true");
    let length = result.expect("incomplete body exceeded read deadline").unwrap();
    let response = String::from_utf8_lossy(&bytes[..length]);
    assert!(response.starts_with("HTTP/1.1 504"), "{response}");
    assert!(response.contains("no-store"));
}

#[tokio::test]
async fn r12_router_auth_admission() {
    use futures_util::FutureExt;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let root = tempfile::tempdir().unwrap();
    let server = DaemonServer::new_with_paths(Some(root.path().join("data/config")), Some(root.path().join("data/auth")));
    let state = server.remote_state().clone();
    let service = &state.machine_services.as_ref().unwrap().workspaces;
    let pin = state.auth_manager.create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine).unwrap();
    let (token, device) = state.auth_manager.exchange_pairing_code(&pin, "auth-budget").unwrap();
    let (entered_tx, mut entered_rx) = tokio::sync::mpsc::unbounded_channel();
    let release = std::sync::Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
    let hook_release = release.clone();
    *service.transaction_probe.write() = Some(std::sync::Arc::new(move |phase| {
        if phase == "authEntry" {
            entered_tx.send(()).unwrap();
            let (lock, condition) = &*hook_release;
            let released = lock.lock().unwrap();
            let result = condition.wait_timeout_while(released, std::time::Duration::from_secs(15), |v| !*v).unwrap();
            assert!(*result.0, "auth fixture release timed out");
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap(); let addr = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel(); let gateway_state = state.clone();
    let gateway = tokio::spawn(async move { axum::serve(listener, create_remote_router(gateway_state)).with_graceful_shutdown(async { let _ = stopped.await; }).await.unwrap(); });
    let mut sockets = Vec::new();
    let result = std::panic::AssertUnwindSafe(async {
        for _ in 0..16 {
            let mut socket = tokio::net::TcpStream::connect(addr).await.unwrap();
            socket.write_all(format!("GET /api/v1/workspace/projects HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
            sockets.push(socket);
            tokio::time::timeout(std::time::Duration::from_secs(5), entered_rx.recv()).await.unwrap().unwrap();
        }
        // Auth slots are all occupied by exact entry signals, not inferred from
        // elapsed time. Unrelated health remains runnable on the real router.
        assert_eq!(super::workspace_api::AUTH_SLOTS.available_permits(), 0);
        let health = reqwest::Client::builder().no_proxy().build().unwrap().get(format!("http://{addr}/api/v1/health")).send().await.unwrap();
        assert_eq!(health.status().as_u16(), 200);
        state.auth_manager.revoke_device(&device.id);
    }).catch_unwind().await;
    { let (lock, condition) = &*release; *lock.lock().unwrap() = true; condition.notify_all(); }
    for mut socket in sockets {
        let mut response = Vec::new();
        let read = tokio::time::timeout(std::time::Duration::from_secs(10), socket.read_to_end(&mut response)).await;
        if result.is_ok() { assert!(read.unwrap().is_ok()); assert!(response.starts_with(b"HTTP/1.1 401")); }
    }
    let auth_finished = tokio::time::timeout(std::time::Duration::from_secs(10), super::workspace_api::AUTH_SLOTS.clone().acquire_many_owned(16)).await.unwrap().unwrap();
    drop(auth_finished);
    *service.transaction_probe.write() = None;
    stop.send(()).unwrap(); tokio::time::timeout(std::time::Duration::from_secs(10), gateway).await.unwrap().unwrap();
    drop(state); drop(server); root.close().unwrap();
    eprintln!("R12 real router auth_entries=16 health=200 revoked=401 sockets_closed=true listener_joined=true root_removed=true");
    if let Err(panic) = result { std::panic::resume_unwind(panic); }
}
