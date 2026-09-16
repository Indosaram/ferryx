use crate::{
    daemon::server::DaemonServer,
    remote::{auth::{DeviceAccessScope, DevicePermission}, server::create_remote_router},
};
#[path = "journal_spawn_contention_tests.rs"]
mod spawn;

use futures_util::FutureExt;
use std::{sync::{Arc, Mutex}, time::Duration};

#[tokio::test]
async fn journal_writer_preserves_real_machine_socket_attachment() {
    use futures_util::{SinkExt, StreamExt};
    use serde_json::{json, Value};
    let (root, owner, token, workspace) = crate::ipc::run_blocking(|| {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        std::fs::create_dir(&project).unwrap();
        let owner = DaemonServer::new_with_paths(Some(root.path().join("config")), Some(root.path().join("auth")));
        let state = owner.remote_state();
        let workspace = state.machine_services.as_ref().unwrap().workspaces.register_machine(project.to_str().unwrap()).unwrap();
        let pin = state.auth_manager.create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine).unwrap();
        let (token, _) = state.auth_manager.exchange_pairing_code(&pin, "journal-socket").unwrap();
        Ok((root, owner, token, workspace))
    }).await.unwrap();
    let state = owner.remote_state().clone();
    let service = state.machine_services.as_ref().unwrap().workspaces.clone();
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
    let client = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(10)).build().unwrap();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let mut writer = None;
    let outcome = std::panic::AssertUnwindSafe(async {
        let created = client.post(format!("http://{address}/api/v1/sessions")).bearer_auth(&token)
            .body(json!({"requestId":uuid::Uuid::new_v4().to_string(),"workspaceId":workspace,"cols":80,"rows":24,
                "worktree":null,"inheritFromSessionId":null,"cwdRelative":null,"startup":{"kind":"shell"}}).to_string())
            .send().await.unwrap();
        assert_eq!(created.status(), 201);
        let session: Value = serde_json::from_str(&created.text().await.unwrap()).unwrap();
        let id = session["target"]["sessionId"].as_str().unwrap();
        let target = format!("/api/v1/terminal/{id}");
        let ticket: Value = serde_json::from_str(&client.post(format!("http://{address}/api/v1/socket-ticket"))
            .bearer_auth(&token).header("content-type", "application/json").body(json!({"target":target}).to_string())
            .send().await.unwrap().text().await.unwrap()).unwrap();
        let (held_tx, held_rx) = tokio::sync::oneshot::channel();
        let held_tx = Mutex::new(Some(held_tx));
        let release_rx = Mutex::new(release_rx);
        *service.journal.probe.write() = Some(Arc::new(move |phase| {
            if phase == "persist" {
                if let Some(tx) = held_tx.lock().unwrap().take() {
                    let _ = tx.send(());
                    release_rx.lock().unwrap().recv_timeout(Duration::from_secs(30)).unwrap();
                }
            }
        }));
        let workspaces = service.clone();
        writer = Some(std::thread::spawn(move || workspaces.journal.begin("fixture",
            &uuid::Uuid::new_v4().to_string(), "fixture", "digest", "none").unwrap()));
        tokio::time::timeout(Duration::from_secs(5), held_rx).await.unwrap().unwrap();
        let url = format!("ws://{address}{target}?ticket={}&daemonEpoch={}",
            ticket["ticket"].as_str().unwrap(), session["target"]["daemonEpoch"].as_str().unwrap());
        let (mut socket, _) = tokio::time::timeout(Duration::from_secs(5),
            tokio_tungstenite::connect_async(url)).await.expect("journal writer blocked WS upgrade").unwrap();
        let attached = tokio::time::timeout(Duration::from_secs(5), socket.next()).await.unwrap().unwrap().unwrap();
        let attached: Value = serde_json::from_str(attached.to_text().unwrap()).unwrap();
        assert_eq!(attached["type"], "attached");
        assert_eq!(attached["target"], session["target"]);
        let marker = format!("JOURNAL_SOCKET_{}", uuid::Uuid::new_v4().simple());
        socket.send(tokio_tungstenite::tungstenite::Message::Binary(format!("{marker}\r").into_bytes().into())).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            let mut output = Vec::new();
            loop {
                let frame = socket.next().await.unwrap().unwrap();
                if let tokio_tungstenite::tungstenite::Message::Binary(bytes) = frame {
                    output.extend_from_slice(crate::remote::terminal_wire::decode_frame(&bytes).unwrap().terminal_bytes);
                    if output.windows(marker.len()).any(|part| part == marker.as_bytes()) { break; }
                }
            }
        }).await.expect("journal writer blocked real PTY echo");
        assert!(client.get(format!("http://{address}/health")).send().await.unwrap().status().is_success());
        socket.close(None).await.unwrap();
        assert!(owner.terminal_service().get_session(id).is_some());
        eprintln!("A09_SOCKET writer_held=true attached_exact_owner=true real_pty_echo=true independent_http=true detach_preserved_pty=true");
    }).catch_unwind().await;
    let _ = release_tx.send(());
    if let Some(writer) = writer { writer.join().unwrap(); }
    *service.journal.probe.write() = None;
    for id in owner.terminal_service().list_sessions() {
        owner.terminal_service().close_session(&id).await.unwrap();
    }
    let _ = stop_tx.send(());
    tokio::time::timeout(Duration::from_secs(10), joined_rx).await.unwrap().unwrap();
    gateway.join().unwrap();
    assert!(tokio::net::TcpStream::connect(address).await.is_err());
    drop((service, state, owner));
    crate::ipc::run_blocking(move || { root.close().unwrap(); Ok(()) }).await.unwrap();
    eprintln!("A09_SOCKET cleanup writer_joined=true gateway_joined=true listener_refused=true ptys_closed=true root_removed=true");
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
}

#[test]
fn reopened_pending_operation_remains_outcome_unknown_after_refresh() {
    // Given an operation whose owner stopped before recording its outcome.
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("operations.json");
    let request = uuid::Uuid::new_v4().to_string();
    let original = super::MachineOperationJournal::open(path.clone());
    original.begin("device", &request, "fixture", "digest", "resource").unwrap();
    drop(original);
    // When the replacement reconciles through the durable refresh path.
    let replacement = super::MachineOperationJournal::open(path);
    let record = replacement.reconcile("device", &request).unwrap().unwrap();
    // Then startup uncertainty must not revert to a live pending operation.
    assert!(matches!(record.operation, crate::remote::machine_protocol::Operation::OutcomeUnknown { .. }));
}

#[tokio::test]
async fn journal_writer_does_not_stall_session_http_executor() {
    contention(false, false, false).await;
}

#[tokio::test]
async fn journal_writer_does_not_stall_mutation_reconciliation() {
    contention(true, false, false).await;
}

#[tokio::test]
async fn journal_read_revocation_retains_admission_until_worker_drains() {
    contention(false, true, false).await;
}

#[tokio::test]
async fn journal_mutation_revocation_retains_admission_until_worker_drains() {
    contention(true, true, false).await;
}

#[tokio::test]
async fn journal_read_deadline_retains_admission_until_worker_drains() {
    contention(false, false, true).await;
}

#[tokio::test]
async fn journal_mutation_deadline_retains_admission_until_worker_drains() {
    contention(true, false, true).await;
}

async fn contention(mutation: bool, revoke: bool, expire: bool) {
    // Given a real writer holding the journal mutex through durable publication.
    let (root, server, token, device_id) = crate::ipc::run_blocking(|| {
        let root = tempfile::tempdir().unwrap();
        let server = DaemonServer::new_with_paths(
            Some(root.path().join("config")), Some(root.path().join("auth")));
        let auth = &server.remote_state().auth_manager;
        let pin = auth.create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine).unwrap();
        let (token, device) = auth.exchange_pairing_code(&pin, "journal-contention").unwrap();
        Ok((root, server, token, device.id))
    }).await.unwrap();
    let state = server.remote_state().clone();
    let service = state.machine_services.as_ref().unwrap().workspaces.clone();
    let slots = if mutation { service.project_mutations.clone() } else { service.project_reads.clone() };
    let capacity = slots.available_permits();
    let auth_state = state.clone();
    let (held_tx, held_rx) = tokio::sync::oneshot::channel();
    let (read_tx, read_rx) = tokio::sync::oneshot::channel();
    let held_tx = Mutex::new(Some(held_tx));
    let read_tx = Mutex::new(Some(read_tx));
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let release_rx = Mutex::new(release_rx);
    *service.journal.probe.write() = Some(Arc::new(move |phase| match phase {
        "persist" => {
            if let Some(tx) = held_tx.lock().unwrap().take() {
                tx.send(()).unwrap();
                release_rx.lock().unwrap().recv_timeout(Duration::from_secs(60)).unwrap();
            }
        }
        phase if phase == if mutation { "reconcile" } else { "sessions" } => {
            if let Some(tx) = read_tx.lock().unwrap().take() { let _ = tx.send(()); }
        }
        _ => {}
    }));
    let writer_service = service.clone();
    let writer = std::thread::spawn(move || {
        writer_service.journal.begin("fixture", &uuid::Uuid::new_v4().to_string(),
            "fixture", "digest", "no-resource").unwrap();
    });
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    let (joined_tx, joined_rx) = tokio::sync::oneshot::channel();
    let gateway = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            axum::serve(tokio::net::TcpListener::from_std(listener).unwrap(), create_remote_router(state))
                .with_graceful_shutdown(async { let _ = stop_rx.await; }).await.unwrap();
        });
        joined_tx.send(()).unwrap();
    });
    let client = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(50)).build().unwrap();
    let mut list = None;
    let mut revoked_response = None;
    let outcome = std::panic::AssertUnwindSafe(async {
        tokio::time::timeout(Duration::from_secs(10), held_rx).await.unwrap().unwrap();
        // When an authenticated HTTP request reaches that held journal lock.
        let url = format!("http://{address}/api/v1/sessions");
        let call = if mutation {
            client.post(url).body(serde_json::json!({
                "requestId":uuid::Uuid::new_v4().to_string(),
                "workspaceId":"missing-project","cols":80,"rows":24,
                "worktree":null,"inheritFromSessionId":null,"cwdRelative":null,
                "startup":{"kind":"shell"}
            }).to_string())
        } else { client.get(url) }.bearer_auth(&token);
        let started = std::time::Instant::now();
        list = Some(tokio::spawn(async move { call.send().await }));
        tokio::time::timeout(Duration::from_secs(10), read_rx).await.unwrap().unwrap();
        // Then the gateway's single executor must still serve independent HTTP.
        // The watchdog runs on a different runtime, so a blocked gateway cannot
        // prevent this bounded latency assertion from firing.
        let health = client.get(format!("http://{address}/health"))
            .timeout(Duration::from_millis(500)).send().await;
        assert!(health.is_ok(), "journal mutex blocked unrelated HTTP executor: {health:?}");
        eprintln!("JOURNAL_CONTENTION mutation={mutation} independent_http_responded=true");
        if revoke {
            crate::ipc::run_blocking(move || Ok(auth_state.auth_manager.revoke_device(&device_id).unwrap()))
                .await.unwrap().then_some(()).unwrap();
        }
        if revoke || expire {
            let response = tokio::time::timeout(Duration::from_secs(if expire { 45 } else { 2 }), list.as_mut().unwrap())
                .await.unwrap().unwrap().unwrap();
            list.take();
            assert_eq!(response.status().as_u16(), if revoke { 401 } else { 504 });
            if expire {
                assert!(started.elapsed() >= Duration::from_secs(if mutation { 40 } else { 10 }));
            }
            assert_eq!(slots.available_permits(), capacity - 1);
            revoked_response = Some(response);
            eprintln!("JOURNAL_CONTENTION mutation={mutation} revoke={revoke} expire={expire} response_before_release=true admission_retained=true");
        }
    }).catch_unwind().await;
    let _ = release_tx.send(());
    let writer_joined = writer.join();
    let response = match list {
        Some(list) => Some(tokio::time::timeout(Duration::from_secs(10), list).await),
        None => None,
    };
    let drained = tokio::time::timeout(Duration::from_secs(10),
        slots.clone().acquire_many_owned(u32::try_from(capacity).unwrap())).await;
    *service.journal.probe.write() = None;
    let _ = stop_tx.send(());
    tokio::time::timeout(Duration::from_secs(10), joined_rx).await.unwrap().unwrap();
    gateway.join().unwrap();
    assert!(tokio::net::TcpStream::connect(address).await.is_err());
    drop(service);
    drop(server);
    root.close().unwrap();
    eprintln!("JOURNAL_CONTENTION cleanup writer_joined=true gateway_joined=true listener_refused=true root_removed=true");
    writer_joined.unwrap();
    drop(drained.unwrap().unwrap());
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
    let response = match revoked_response {
        Some(response) => response,
        None => response.unwrap().unwrap().unwrap().unwrap(),
    };
    let status = response.status().as_u16();
    let body = response.text().await.unwrap();
    assert_eq!(status, if revoke { 401 } else if expire { 504 } else if mutation { 404 } else { 200 }, "{body}");
}
