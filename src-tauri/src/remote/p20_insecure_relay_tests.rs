use super::*;
use crate::remote::relay_client::{to_ws_base, validate_relay_url, RelayUrlSecurityError};
use crate::remote::state::{RemoteGatewayState, RemoteNetworkMode};
use crate::terminal::{PtyManager, TerminalOutputHub, TerminalService};
use crate::worktree::WorkspaceRegistry;
use std::sync::Arc;

fn create_test_state() -> Arc<RemoteGatewayState> {
    let pty = Arc::new(PtyManager::new());
    let hub = Arc::new(TerminalOutputHub::default());
    let terminal_service = Arc::new(TerminalService::new(pty, hub));
    Arc::new(RemoteGatewayState::new(
        terminal_service,
        WorkspaceRegistry::new(),
    ))
}

#[test]
fn test_p20_non_loopback_http_relay_url_rejected_with_typed_error() {
    // Without development opt-in:
    // Non-loopback public http:// URL must be rejected with typed error
    let err_public = validate_relay_url("http://relay.example.com", false)
        .expect_err("non-loopback public http relay URL must be rejected");
    assert_eq!(err_public.code(), "INSECURE_PUBLIC_RELAY_FORBIDDEN");
    assert!(
        matches!(
            err_public,
            RelayUrlSecurityError::InsecurePublicRelayForbidden { .. }
        ),
        "expected InsecurePublicRelayForbidden, got {err_public:?}"
    );

    // Non-loopback public IPv4 http:// URL must also be rejected
    let err_public_ip = validate_relay_url("http://203.0.113.195:8787", false)
        .expect_err("non-loopback public IP http relay URL must be rejected");
    assert_eq!(err_public_ip.code(), "INSECURE_PUBLIC_RELAY_FORBIDDEN");

    // RFC1918 private http:// URL without explicit opt-in must be rejected
    let err_private = validate_relay_url("http://192.168.1.100:8787", false)
        .expect_err("RFC1918 http relay without opt-in must be rejected");
    assert_eq!(err_private.code(), "INSECURE_RELAY_OPT_IN_REQUIRED");
    assert!(
        matches!(
            err_private,
            RelayUrlSecurityError::InsecureDevelopmentRelayRequiresOptIn { .. }
        ),
        "expected InsecureDevelopmentRelayRequiresOptIn, got {err_private:?}"
    );

    // Verify structured error envelope contract: {code, message, details}
    let envelope = err_public.to_error_envelope();
    assert_eq!(envelope["code"], "INSECURE_PUBLIC_RELAY_FORBIDDEN");
    assert!(envelope["message"].is_string());
    assert_eq!(envelope["details"]["publicNetwork"], true);
}

#[test]
fn test_p20_loopback_http_and_non_loopback_https_allowed() {
    // Loopback http:// must remain allowed for local development
    let loopback_named = validate_relay_url("http://localhost:8787", false)
        .expect("localhost http:// must be allowed");
    assert_eq!(loopback_named, "ws://localhost:8787");

    let loopback_v4 = validate_relay_url("http://127.0.0.1:8787", false)
        .expect("127.0.0.1 http:// must be allowed");
    assert_eq!(loopback_v4, "ws://127.0.0.1:8787");

    let loopback_v6 =
        validate_relay_url("http://[::1]:8787", false).expect("::1 http:// must be allowed");
    assert_eq!(loopback_v6, "ws://[::1]:8787");

    // Non-loopback https:// must be allowed
    let public_https = validate_relay_url("https://relay.example.com", false)
        .expect("https://relay.example.com must be allowed");
    assert_eq!(public_https, "wss://relay.example.com");

    let public_wss = validate_relay_url("wss://relay.example.com", false)
        .expect("wss://relay.example.com must be allowed");
    assert_eq!(public_wss, "wss://relay.example.com");

    let lan_https = validate_relay_url("https://192.168.1.100:8787", false)
        .expect("https LAN relay must be allowed");
    assert_eq!(lan_https, "wss://192.168.1.100:8787");
}

#[test]
fn test_p20_ws_auto_conversion_only_for_allowed_cases() {
    // Allowed cases: auto-converted
    assert_eq!(
        to_ws_base("http://localhost:8787").unwrap(),
        "ws://localhost:8787"
    );
    assert_eq!(
        to_ws_base("https://relay.example.com/").unwrap(),
        "wss://relay.example.com"
    );
    assert_eq!(
        to_ws_base("wss://relay.example.com").unwrap(),
        "wss://relay.example.com"
    );

    // Disallowed case: must NOT auto-convert to plaintext ws://
    let err = to_ws_base("http://relay.example.com")
        .expect_err("non-loopback http relay must NOT auto-convert to ws://");
    assert_eq!(err.code(), "INSECURE_PUBLIC_RELAY_FORBIDDEN");
}

#[test]
fn test_p20_development_insecure_flag_allows_rfc1918_but_forbids_public() {
    // With development flag enabled:
    // RFC1918 192.168.x.x is allowed
    let lan_192 = validate_relay_url("http://192.168.1.100:8787", true)
        .expect("RFC1918 192.168.x.x with dev flag must be allowed");
    assert_eq!(lan_192, "ws://192.168.1.100:8787");

    // RFC1918 10.x.x.x is allowed
    let lan_10 = validate_relay_url("http://10.0.0.5:8787", true)
        .expect("RFC1918 10.x.x.x with dev flag must be allowed");
    assert_eq!(lan_10, "ws://10.0.0.5:8787");

    // RFC1918 172.16.x.x is allowed
    let lan_172 = validate_relay_url("http://172.16.1.5:8787", true)
        .expect("RFC1918 172.16.x.x with dev flag must be allowed");
    assert_eq!(lan_172, "ws://172.16.1.5:8787");

    // Public network http:// remains FORBIDDEN even with dev flag!
    let public_err = validate_relay_url("http://relay.example.com", true)
        .expect_err("public http relay must be forbidden even with dev flag");
    assert_eq!(public_err.code(), "INSECURE_PUBLIC_RELAY_FORBIDDEN");
}

#[tokio::test]
async fn test_p20_server_startup_rejects_insecure_custom_relay() {
    let state = create_test_state();
    {
        let mut config = state.config.write();
        config.mode = RemoteNetworkMode::Relay;
        config.port = 0;
        config.relay_url = Some("http://relay.example.com".to_string());
    }

    let result = start_remote_server(state).await;
    assert!(
        result.is_err(),
        "P20: server startup must reject non-loopback http relay URL"
    );
    let err = result.err().unwrap();
    assert!(
        err.contains("Invalid relay") || err.contains("forbidden") || err.contains("Insecure"),
        "error must cite insecure relay URL rejection: {err}"
    );
}
