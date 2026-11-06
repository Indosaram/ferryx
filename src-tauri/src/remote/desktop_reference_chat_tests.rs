//! Desktop chat-lens bridge: trusted local authority stays unforgeable, the allowlist is closed,
//! and the owner/epoch/host fences of the shared reference-chat handlers still apply.

use super::*;
use crate::daemon::protocol::{
    DaemonRequest, DesktopReferenceChatKind, DesktopReferenceChatRequest,
};

fn daemon_state() -> (tempfile::TempDir, Arc<RemoteGatewayState>) {
    let root = tempfile::tempdir().expect("tempdir");
    let daemon = crate::daemon::server::DaemonServer::new_with_paths(
        Some(root.path().join("data/config")),
        Some(root.path().join("data/auth")),
    );
    (root, daemon.remote_state().clone())
}

fn request(
    state: &RemoteGatewayState,
    kind: DesktopReferenceChatKind,
    session: &str,
) -> DesktopReferenceChatRequest {
    let identity = desktop_reference_chat_identity(state);
    let params = match kind {
        DesktopReferenceChatKind::Submit => serde_json::json!({ "text": "hello", "origin": "chat" }),
        DesktopReferenceChatKind::Stop => serde_json::json!({ "capability": "providerInterrupt" }),
        _ => serde_json::Value::Null,
    };
    serde_json::from_value(serde_json::json!({
        "kind": kind,
        "requestId": uuid::Uuid::new_v4().to_string(),
        "target": {
            "target": {
                "hostId": identity.host_id,
                "ownerId": identity.owner_id,
                "epoch": identity.epoch,
                "backendSessionId": session,
            },
            "providerSessionId": null,
        },
        "registryId": "claude",
        "params": params,
    }))
    .expect("a frontend-shaped request")
}

fn code(reply: &crate::daemon::protocol::DesktopReferenceChatReply) -> String {
    reply.body["error"]["code"].as_str().unwrap_or_default().to_string()
}

#[tokio::test]
async fn desktop_submit_reuses_a_live_pty_without_a_network_listener() {
    let (_root, state) = daemon_state();
    let terminal = state.terminal_service.clone();
    let (session, _exit) = crate::ipc::run_blocking(move || Ok(terminal.spawn_shell(80, 24)?))
        .await.expect("spawn isolated test pane");
    let mut submit = request(&state, DesktopReferenceChatKind::Submit, &session);
    submit.registry_id = None;
    submit.params = serde_json::json!({ "text": "echo desktop-chat-probe", "origin": "chat" });
    let first = desktop_reference_chat(&state, submit.clone()).await;
    let duplicate = desktop_reference_chat(&state, submit).await;
    state.terminal_service.close_session(&session).await.expect("close owned test pane");
    assert_eq!(first.status, 200, "{:?}", first.body);
    assert_eq!(first.body["data"]["receipt"]["stage"], "accepted");
    assert_eq!(duplicate.status, 200, "{:?}", duplicate.body);
    assert_eq!(first.body, duplicate.body);
}

#[test]
fn only_allowlisted_kinds_deserialize() {
    for kind in ["history", "prompt", "submit", "stop", "answer"] {
        serde_json::from_value::<DesktopReferenceChatKind>(serde_json::json!(kind)).expect(kind);
    }
    for kind in ["control", "files", "screen", "payload", "acknowledgeOutcome"] {
        assert!(
            serde_json::from_value::<DesktopReferenceChatKind>(serde_json::json!(kind)).is_err(),
            "{kind} must not be reachable from the desktop bridge"
        );
    }
}

#[test]
fn daemon_requests_roundtrip_with_the_nested_frontend_target() {
    let (_root, state) = daemon_state();
    let wire = DaemonRequest::ReferenceChat {
        request: request(&state, DesktopReferenceChatKind::History, "s-1"),
    };
    let json = serde_json::to_string(&wire).expect("serialize");
    let back: DaemonRequest = serde_json::from_str(&json).expect("the flattened wire reads back");
    let DaemonRequest::ReferenceChat { request } = back else { panic!("variant") };
    assert_eq!(request.target.target.backend_session_id, "s-1");
    assert_eq!(request.target.target.owner_id, state.reference_owner_id);
    let identity: DaemonRequest =
        serde_json::from_str(r#"{"type":"referenceChatIdentity"}"#).expect("identity");
    assert!(matches!(identity, DaemonRequest::ReferenceChatIdentity));
}

#[test]
fn identity_is_the_gateway_own_owner_and_epoch() {
    let (_root, state) = daemon_state();
    state.daemon_epoch.store(42, std::sync::atomic::Ordering::Release);
    let identity = desktop_reference_chat_identity(&state);
    assert_eq!(identity.owner_id, state.reference_owner_id);
    assert_eq!(identity.epoch, "42");
    assert_eq!(identity.host_id, reference_files::reference_host_id());
}

#[tokio::test]
async fn trust_exists_only_inside_the_bridge_scope() {
    let (_root, state) = daemon_state();
    let mut forged = HeaderMap::new();
    forged.insert("authorization", "Bearer desktop-local".parse().unwrap());
    forged.insert("x-ferryx-desktop", "1".parse().unwrap());
    assert!(authenticate_machine_request(&state, &forged).is_err());
    assert!(authenticate_machine_request(&state, &HeaderMap::new()).is_err());
    let device = DESKTOP_REFERENCE_TRUST
        .scope(desktop_reference_device(), async {
            authenticate_machine_request(&state, &HeaderMap::new())
        })
        .await
        .expect("trusted inside the scope");
    assert_eq!(device.permission, DevicePermission::Control);
    assert!(authenticate_machine_request(&state, &HeaderMap::new()).is_err());
}

#[tokio::test]
async fn a_foreign_owner_or_epoch_is_target_expired() {
    let (_root, state) = daemon_state();
    let session = uuid::Uuid::new_v4().to_string();
    for kind in [DesktopReferenceChatKind::History, DesktopReferenceChatKind::Submit] {
        let mut owner = request(&state, kind, &session);
        owner.target.target.owner_id = "someone-else".into();
        let reply = desktop_reference_chat(&state, owner).await;
        assert_eq!((reply.status, code(&reply).as_str()), (410, "TARGET_EXPIRED"), "{:?}", reply.body);

        let mut epoch = request(&state, kind, &session);
        epoch.target.target.epoch = crate::scoped_contracts::Epoch(epoch.target.target.epoch.0 + 1);
        let reply = desktop_reference_chat(&state, epoch).await;
        assert_eq!((reply.status, code(&reply).as_str()), (410, "TARGET_EXPIRED"), "{:?}", reply.body);
    }
}

#[tokio::test]
async fn a_foreign_host_is_forbidden_and_an_unknown_session_not_found() {
    let (_root, state) = daemon_state();
    let session = uuid::Uuid::new_v4().to_string();
    let mut host = request(&state, DesktopReferenceChatKind::Prompt, &session);
    host.target.target.host_id = format!("{}-other", host.target.target.host_id);
    let reply = desktop_reference_chat(&state, host).await;
    assert_eq!((reply.status, code(&reply).as_str()), (403, "FORBIDDEN"), "{:?}", reply.body);

    for kind in [
        DesktopReferenceChatKind::History,
        DesktopReferenceChatKind::Prompt,
        DesktopReferenceChatKind::Stop,
    ] {
        let reply = desktop_reference_chat(&state, request(&state, kind, &session)).await;
        assert_eq!((reply.status, code(&reply).as_str()), (404, "NOT_FOUND"), "{kind:?}: {:?}", reply.body);
    }
}

#[tokio::test]
async fn malformed_read_params_are_invalid_request() {
    let (_root, state) = daemon_state();
    let mut bad = request(&state, DesktopReferenceChatKind::History, "s-1");
    bad.params = serde_json::json!({ "cursor": "not-a-cursor" });
    let reply = desktop_reference_chat(&state, bad).await;
    assert_eq!((reply.status, code(&reply).as_str()), (400, "INVALID_REQUEST"), "{:?}", reply.body);
}
