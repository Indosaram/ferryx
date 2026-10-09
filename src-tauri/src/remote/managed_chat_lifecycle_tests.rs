use super::*;
use crate::remote::{auth::{DeviceAccessScope, DevicePermission}, managed_chat_api::*, machine_protocol::{CreateSessionRequest, RemoteTerminalTarget, Startup}};
use crate::scoped_contracts::{ChatDraft, Epoch};
use serde_json::Value;
use std::time::{Duration, Instant};

async fn body(response: Response) -> Value {
    serde_json::from_slice(&axum::body::to_bytes(response.into_body(), 65536).await.unwrap()).unwrap()
}

#[test]
fn explicit_launch_binds_history_and_dispatches_real_callbacks() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        eprintln!("managed-chat phase: daemon setup");
        // Given a private daemon, authenticated machine grant, and a controlled provider process.
    let (directory, daemon, workspace) = tokio::task::spawn_blocking(|| {
        let directory = tempfile::tempdir().unwrap();
        let project = directory.path().join("project");
        std::fs::create_dir(&project).unwrap();
        let daemon = crate::daemon::server::DaemonServer::new_with_paths(Some(directory.path().join("config")), Some(directory.path().join("auth")));
        let workspace = daemon.session_service.workspace_service.register_machine(project.to_str().unwrap()).unwrap();
        (directory, daemon, workspace)
    }).await.unwrap();
        eprintln!("managed-chat phase: gateway setup");
    let sessions = daemon.session_service.clone();
    let state = Arc::new(RemoteGatewayState::new_with_paths_and_service(
        sessions.clone(), daemon.terminal_service().clone(), sessions.workspace_service.registry.clone(),
        Some(directory.path().join("gateway-config")), Some(directory.path().join("gateway-auth")),
    ).with_machine_services(sessions.clone()));
    state.daemon_epoch.store(123, std::sync::atomic::Ordering::Relaxed);
    eprintln!("managed-chat phase: identity");
    let identity = super::super::server::load_gateway_identity(state.clone()).await.unwrap();
    eprintln!("managed-chat phase: pairing");
    let pin = state.auth_manager.create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine).unwrap();
    let (token, device) = state.auth_manager.exchange_pairing_code(&pin, "managed-fixture").unwrap();
    let terminal_target = RemoteTerminalTarget { machine_id: identity.machine_id.clone(), daemon_epoch: Epoch(123), session_id: uuid::Uuid::new_v4().to_string() };
    eprintln!("managed-chat phase: terminal spawn");
    let session_id = sessions.spawn_machine(CreateSessionRequest {
        request_id: uuid::Uuid::new_v4().to_string(), workspace_id: workspace, worktree: None,
        cols: 80, rows: 24, inherit_from_session_id: None, cwd_relative: None, startup: Startup::Shell,
    }, device.id.clone(), "managed-fixture".into(), terminal_target, Instant::now() + Duration::from_secs(10), Arc::new(|| Ok(()))).await.unwrap();
    let target = TargetRef { host_id: identity.machine_id, owner_id: device.id, epoch: Epoch(123), backend_session_id: session_id.clone() };
    let mut headers = HeaderMap::new();
    headers.insert("authorization", format!("Bearer {token}").parse().unwrap());
    let mut command = tokio::process::Command::new(if cfg!(windows) { "python" } else { "python3" });
    command.arg("-u").arg("-c").arg(include_str!("../ferryx_scope/chat/peer.py"));
    *state.managed_chat_command.lock() = Some(command);
    let mut events = sessions.workspace_service.machine_events.subscribe();
    eprintln!("managed-chat phase: provider initialize and thread start");
    let result = async {
        // When the explicit start boundary creates and registers the provider.
        let response = managed_chat_start(State(state.clone()), headers.clone(), Json(ManagedChatStartRequest { request_id: "start".into(), target: target.clone(), provider: CanonicalProvider::Codex })).await;
        assert_eq!(response.status(), StatusCode::OK);
        let started = body(response).await;
        eprintln!("managed-chat phase: provider registered");
        assert_eq!(started["data"]["threadId"], "thread-qa");
        assert_eq!(sessions.session_provider_session(&session_id).unwrap().id, "thread-qa");
        let mut replacement = tokio::process::Command::new(if cfg!(windows) { "python" } else { "python3" });
        replacement.arg("-u").arg("-c").arg(include_str!("../ferryx_scope/chat/peer.py"));
        *state.managed_chat_command.lock() = Some(replacement);
        let response = managed_chat_start(State(state.clone()), headers.clone(), Json(ManagedChatStartRequest { request_id: "replacement".into(), target: target.clone(), provider: CanonicalProvider::Codex })).await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert!(state.managed_chat_command.lock().take().is_some());
        eprintln!("managed-chat phase: turn start");
        let response = managed_chat_send(State(state.clone()), headers.clone(), Json(ManagedChatSendRequest { request_id: "send".into(), target: target.clone(), draft: ChatDraft { text: "hello".into(), attachments: vec![] } })).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body(response).await["data"]["requestId"], "send");
        eprintln!("managed-chat phase: approval frame");
        let approval = tokio::time::timeout(Duration::from_secs(10), async {
            loop { let event = events.recv().await.unwrap(); if event["type"] == "callback" { break event; } }
        }).await.unwrap();
        assert_eq!(approval["callback"]["id"], "callback-real");
        let callback_incarnation = approval["callback"]["callbackIncarnation"].as_u64().unwrap();
        eprintln!("managed-chat phase: approval reply");
        let response = managed_chat_reply(State(state.clone()), headers.clone(), Json(ManagedChatReplyRequest {
            request_id: "reply".into(), target: target.clone(), callback_id: json!("callback-real"), thread_id: "thread-qa".into(), turn_id: "turn-qa".into(), callback_incarnation, kind: Some("approval".into()), result: json!({"decision":"accept"}),
        })).await;
        assert_eq!(response.status(), StatusCode::OK);
        // Then the peer emits its next request only after consuming the real callback response.
        let question = tokio::time::timeout(Duration::from_secs(10), async {
            loop { let event = events.recv().await.unwrap(); if event["type"] == "callback" { break event; } }
        }).await.unwrap();
        assert_eq!(question["callback"]["id"], "17");
        eprintln!("managed-chat phase: provider stop");
        let response = managed_chat_stop(State(state.clone()), headers.clone(), Json(ManagedChatStopRequest { request_id: "stop".into(), target: target.clone() })).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(!MANAGED_PROVIDERS.lock().contains_key(&session_id));
        assert!(LIVE_CALLBACKS.lock().list_pending(&session_id, None).is_empty());
        assert_eq!(sessions.session_provider_session(&session_id).unwrap().id, "thread-qa");
    };
    use futures_util::FutureExt;
    let result = std::panic::AssertUnwindSafe(result).catch_unwind().await;
    let provider = MANAGED_PROVIDERS.lock().get(&session_id).cloned();
    if let Some(provider) = provider { provider.stop_agent(&target).await.unwrap(); }
    unregister_managed_provider(&session_id);
    eprintln!("managed-chat phase: terminal close");
    // Machine spawn owns journal/metadata lifecycle state. Use its matching close
    // operation, which releases ownership before awaiting lifecycle completion.
    sessions.close_machine(
        &target.owner_id,
        &uuid::Uuid::new_v4().to_string(),
        "managed-fixture-close",
        &session_id,
        target.epoch,
        Epoch(123),
        Arc::new(|| Ok(())),
    ).await.unwrap();
    eprintln!("managed-chat phase: terminal lifecycle completion");
    sessions.wait_machine_lifecycle(&session_id).await.unwrap();
    eprintln!("managed-chat phase: cleanup complete");
    drop(state);
    drop(sessions);
    drop(daemon);
    drop(directory);
    if let Err(panic) = result { std::panic::resume_unwind(panic); }
    });
    runtime.shutdown_timeout(Duration::from_secs(2));
}

#[tokio::test]
async fn unauthenticated_start_never_consumes_provider_command() {
    // Given no authorization and a command that must never be executed.
    let state = Arc::new(RemoteGatewayState::new(Arc::new(crate::terminal::TerminalService::default()), crate::worktree::WorkspaceRegistry::new()));
    *state.managed_chat_command.lock() = Some(tokio::process::Command::new("must-not-run"));
    // When start is requested without a bearer grant.
    let response = managed_chat_start(State(state.clone()), HeaderMap::new(), Json(ManagedChatStartRequest {
        request_id: "denied".into(), target: TargetRef { host_id: "h".into(), owner_id: "o".into(), epoch: Epoch(1), backend_session_id: "s".into() }, provider: CanonicalProvider::Codex,
    })).await;
    // Then admission fails before spawn or fixture consumption.
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(state.managed_chat_command.lock().is_some());
}
