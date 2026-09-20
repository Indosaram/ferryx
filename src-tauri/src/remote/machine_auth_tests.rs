use super::*;

// Exercise the real router on a private loopback listener. No daemon discovery,
// environment mutation, persistent auth store, PTY, or filesystem probe is used.
async fn admission_status(method: &str, path: &str, authenticated: bool) -> u16 {
    router_response(method, path, authenticated).await.0
}

async fn router_response(
    method: &str,
    path: &str,
    authenticated: bool,
) -> (u16, reqwest::header::HeaderMap, Vec<u8>) {
    let state = Arc::new(RemoteGatewayState::new(
        Arc::new(TerminalService::default()),
        crate::worktree::WorkspaceRegistry::new(),
    ));
    let token = if authenticated {
        let pin = state
            .auth_manager
            .create_pairing_code(DevicePermission::Control);
        Some(
            state
                .auth_manager
                .exchange_pairing_code(&pin, "mirror fixture")
                .unwrap()
                .0,
        )
    } else {
        None
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(listener, create_remote_router(state))
            .with_graceful_shutdown(async {
                shutdown_rx.await.expect("shutdown signal");
            })
            .await
            .expect("private HTTP server");
    });
    let response = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| e.to_string())?;
        let method = method
            .parse::<reqwest::Method>()
            .map_err(|e| e.to_string())?;
        let mut request = client.request(method, format!("http://{addr}{path}"));
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        let response = request.send().await.map_err(|e| e.to_string())?;
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let body = response.bytes().await.map_err(|e| e.to_string())?;
        Ok::<_, String>((status, headers, body.to_vec()))
    })
    .await;
    let shutdown = shutdown_tx.send(());
    let joined = server.await;
    shutdown.expect("signal private server");
    joined.expect("join private server");
    let response = response
        .expect("bounded HTTP request")
        .expect("HTTP request");
    assert!(response.2.len() <= 4096, "bounded machine response");
    response
}

#[tokio::test]
async fn a03_capabilities_require_authentication() {
    assert_eq!(
        admission_status("GET", "/api/v1/capabilities", false).await,
        401
    );
}

#[tokio::test]
async fn a03_legacy_control_denies_directory_browsing() {
    assert_eq!(
        admission_status("GET", "/api/v1/fs/directories", true).await,
        403
    );
}

#[tokio::test]
async fn a03_legacy_control_denies_machine_creation() {
    assert_eq!(
        admission_status("POST", "/api/v1/sessions", true).await,
        403
    );
}

#[tokio::test]
async fn a03_router_auth_error_is_typed_and_no_store() {
    let (status, headers, bytes) = router_response("GET", "/api/v1/capabilities", false).await;
    assert_eq!(status, 401);
    assert_eq!(
        headers.get("cache-control").and_then(|v| v.to_str().ok()),
        Some("no-store")
    );
    let error = crate::remote::machine_protocol::decode_json::<
        crate::remote::machine_protocol::ErrorEnvelope,
    >(&bytes, 4096)
    .unwrap();
    assert_eq!(error.error.code, "UNAUTHORIZED");
}

#[tokio::test]
async fn a03_router_scope_error_satisfies_a02_contract() {
    let (status, headers, bytes) = router_response("GET", "/api/v1/fs/directories", true).await;
    assert_eq!(status, 403);
    assert_eq!(
        headers.get("cache-control").and_then(|v| v.to_str().ok()),
        Some("no-store")
    );
    let error = crate::remote::machine_protocol::decode_json::<
        crate::remote::machine_protocol::ErrorEnvelope,
    >(&bytes, 4096)
    .unwrap();
    assert_eq!(error.error.code, "MACHINE_ACCESS_REQUIRED");
}

#[test]
fn a03_owner_machine_purpose_survives_persistence_and_exchange() {
    use crate::remote::auth::DeviceAccessScope;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("auth.json");
    let owner = AuthManager::with_persistence(Some(path.clone()));
    let pin = owner
        .create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine)
        .unwrap();
    drop(owner);
    let gateway = AuthManager::with_persistence(Some(path.clone()));
    let (token, device) = gateway.exchange_pairing_code(&pin, "desktop").unwrap();
    assert_eq!(device.access_scope, DeviceAccessScope::Machine);
    let reopened = AuthManager::with_persistence(Some(path));
    assert_eq!(
        reopened.validate_token(&token).unwrap().access_scope,
        DeviceAccessScope::Machine
    );
    assert!(reopened.revoke_device(&device.id).unwrap());
    assert!(reopened.validate_token(&token).is_err());
}

#[test]
fn a03_old_persisted_grants_remain_mirror() {
    use crate::remote::auth::{DeviceAccessScope, DeviceInfo};
    let old: DeviceInfo = serde_json::from_value(serde_json::json!({
        "id": "legacy", "name": "phone", "permission": "control",
        "createdAt": 1, "lastSeenAt": 1
    }))
    .unwrap();
    assert_eq!(old.access_scope, DeviceAccessScope::Mirror);
}

#[test]
fn a03_cli_requires_explicit_machine_purpose() {
    use crate::cli::{parse_pair_cli, PairCliCommand};
    assert_eq!(
        parse_pair_cli(["ferryx", "pair", "generate"]).unwrap(),
        PairCliCommand::GeneratePin
    );
    assert_eq!(
        parse_pair_cli(["ferryx", "pair", "generate", "--access", "machine"]).unwrap(),
        PairCliCommand::GenerateMachinePin
    );
    assert!(parse_pair_cli(["ferryx", "pair", "generate", "--access", "invalid"]).is_err());
}

#[tokio::test]
async fn a03_rejected_generation_cannot_redeem_machine_authority() {
    use crate::remote::auth::DeviceAccessScope;
    use crate::remote::protocol::RegisterPairingPinAck;
    use crate::remote::relay_client::{PairingCoordinator, RegisterPairingPinRequest};
    use std::time::Duration;
    for stale in [true, false] {
        let auth = AuthManager::new();
        let (tx, mut rx) = tokio::sync::mpsc::channel::<RegisterPairingPinRequest>(1);
        let coordinator = PairingCoordinator::new_with_auth("fixture", tx, auth.clone());
        let generator = tokio::spawn(async move {
            coordinator
                .generate_scoped_pairing(
                    Duration::from_secs(60),
                    DevicePermission::Control,
                    DeviceAccessScope::Machine,
                )
                .await
        });
        let request = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        let pin = request.registration.pin.clone();
        let capability = request.registration.pairing_token.clone();
        request
            .ack
            .send(Ok(RegisterPairingPinAck {
                generation: if stale {
                    Some(0)
                } else {
                    request.registration.generation
                },
                pin: pin.clone(),
                machine_id: "fixture".into(),
                status: if stale { "ready" } else { "rejected" }.into(),
            }))
            .unwrap();
        let result = tokio::time::timeout(Duration::from_secs(5), generator)
            .await
            .unwrap()
            .unwrap();
        assert!(result.is_err());
        assert!(auth.exchange_pairing_code(&pin, "attacker").is_err());
        assert!(auth.exchange_pairing_code(&capability, "attacker").is_err());
        assert!(auth.list_devices().is_empty());
    }
}

#[test]
fn a03_machine_view_pin_cannot_issue_a_device() {
    use crate::remote::auth::DeviceAccessScope;
    let auth = AuthManager::new();
    let pin = auth.create_scoped_pairing_code(DevicePermission::View, DeviceAccessScope::Machine);
    let exchange = pin.and_then(|pin| {
        auth.exchange_pairing_code(&pin, "view machine")
            .map_err(|_| crate::remote::auth::MachineGrantError)
    });
    assert!(
        exchange.is_err(),
        "Machine View must not produce a device grant"
    );
    assert!(auth.list_devices().is_empty());
}

#[test]
fn a03_machine_view_capability_cannot_issue_a_device() {
    use crate::remote::auth::DeviceAccessScope;
    let auth = AuthManager::new();
    assert!(auth
        .register_scoped_pairing_capability(
            "fixture-capability",
            DevicePermission::View,
            DeviceAccessScope::Machine
        )
        .is_err());
    assert!(
        auth.exchange_pairing_code("fixture-capability", "view machine")
            .is_err(),
        "Machine View must not produce a device grant"
    );
}

#[tokio::test]
async fn a03_owner_coordinator_rejects_machine_view_without_publication() {
    use crate::remote::auth::DeviceAccessScope;
    use crate::remote::relay_client::{PairingCoordinator, RegisterPairingPinRequest};
    let auth = AuthManager::new();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<RegisterPairingPinRequest>(1);
    let coordinator = PairingCoordinator::new_with_auth("fixture", tx, auth.clone());
    assert!(coordinator
        .generate_scoped_pairing(
            std::time::Duration::from_secs(60),
            DevicePermission::View,
            DeviceAccessScope::Machine
        )
        .await
        .is_err());
    assert!(matches!(
        rx.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));
    assert!(auth.list_devices().is_empty());
}

#[test]
fn a03_mirror_view_issuance_preserves_view() {
    use crate::remote::auth::DeviceAccessScope;
    let auth = AuthManager::new();
    let pin = auth.create_pairing_code(DevicePermission::View);
    let (_, device) = auth.exchange_pairing_code(&pin, "mirror viewer").unwrap();
    assert_eq!(device.permission, DevicePermission::View);
    assert_eq!(device.access_scope, DeviceAccessScope::Mirror);
}
