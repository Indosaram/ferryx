use super::*;
use crate::remote::state::{InterfaceResolver, RemoteGatewayState, RemoteNetworkMode};
use crate::terminal::{PtyManager, TerminalOutputHub, TerminalService};
use crate::worktree::WorkspaceRegistry;
use std::net::Ipv4Addr;
use std::sync::Arc;

struct LanInterfaceResolver {
    lan_ip: Ipv4Addr,
}

impl InterfaceResolver for LanInterfaceResolver {
    fn local_network_address(&self) -> Result<Ipv4Addr, String> {
        Ok(self.lan_ip)
    }

    fn tailscale_address(&self) -> Result<Ipv4Addr, String> {
        Err("no active Tailscale interface".into())
    }
}

struct TailscaleInterfaceResolver {
    tailscale_ip: Ipv4Addr,
}

impl InterfaceResolver for TailscaleInterfaceResolver {
    fn local_network_address(&self) -> Result<Ipv4Addr, String> {
        Err("no local network interface".into())
    }

    fn tailscale_address(&self) -> Result<Ipv4Addr, String> {
        Ok(self.tailscale_ip)
    }
}

fn create_test_state() -> Arc<RemoteGatewayState> {
    let pty = Arc::new(PtyManager::new());
    let hub = Arc::new(TerminalOutputHub::default());
    let terminal_service = Arc::new(TerminalService::new(pty, hub));
    Arc::new(RemoteGatewayState::new(
        terminal_service,
        WorkspaceRegistry::new(),
    ))
}

#[tokio::test]
async fn test_p19_non_loopback_direct_without_insecure_opt_in_refuses_to_serve() {
    // Reset insecure direct opt-in to default (false)
    set_allow_insecure_direct(false);

    let state = create_test_state();
    {
        let mut config = state.config.write();
        config.mode = RemoteNetworkMode::LocalNetwork;
        config.port = 0;
    }

    // Use a non-loopback IP that is active and bindable on this host
    let local_lan_ip = crate::remote::state::SystemInterfaceResolver
        .local_network_address()
        .expect("active LAN IP on workstation");
    assert!(!local_lan_ip.is_loopback(), "must be non-loopback LAN IP");

    let resolver = Arc::new(LanInterfaceResolver { lan_ip: local_lan_ip });

    // 1. start_remote_server_with_resolver_and_insecure_opt_in(..., false) must NOT bind the non-loopback external interface
    let (handle, local_addr) = start_remote_server_with_resolver_and_insecure_opt_in(
        Arc::clone(&state),
        resolver.clone(),
        false,
    )
    .await
    .expect("gateway starts loopback baseline");

    assert!(local_addr.ip().is_loopback(), "baseline primary listener is loopback");
    assert!(
        !handle.is_external_bound(),
        "P19: non-loopback direct gateway must NOT bind external interface without explicit insecure opt-in"
    );
    assert!(
        matches!(handle.gate_status(), DirectGatewayGateStatus::InsecureLanGated { .. }),
        "P19: gateway must surface explicit InsecureLanGated status"
    );
    handle.stop();

    // 2. Strict mode must return an Err refusing to serve
    let strict_result = start_remote_server_strict_with_resolver(Arc::clone(&state), resolver).await;
    assert!(
        strict_result.is_err(),
        "P19: strict direct gateway startup must refuse to serve non-loopback LAN without opt-in"
    );
    let err = strict_result.err().unwrap();
    assert!(
        err.contains("Insecure direct gateway on local network interface")
            || err.contains("LocalNetwork mode binds unencrypted HTTP/WebSocket without TLS"),
        "error must cite insecure direct gateway refusal: {err}"
    );
}

#[tokio::test]
async fn test_p19_overlay_mode_tailscale_unaffected() {
    // Without insecure opt-in, overlay mode (Tailscale) must remain unaffected
    set_allow_insecure_direct(false);

    let state = create_test_state();
    {
        let mut config = state.config.write();
        config.mode = RemoteNetworkMode::Tailscale;
        config.port = 0;
    }

    // Get active Tailscale IP from workstation
    let tailscale_ip = crate::remote::state::SystemInterfaceResolver
        .tailscale_address()
        .expect("active Tailscale interface on workstation");

    let resolver = Arc::new(TailscaleInterfaceResolver { tailscale_ip });

    let (handle, local_addr) = start_remote_server_with_resolver_and_insecure_opt_in(
        Arc::clone(&state),
        resolver,
        false,
    )
    .await
    .expect("tailscale overlay mode starts");

    assert!(local_addr.ip().is_loopback());
    assert!(
        handle.is_external_bound(),
        "P19: Tailscale overlay mode must bind external interface without requiring insecure opt-in"
    );
    assert!(
        matches!(handle.gate_status(), DirectGatewayGateStatus::OverlaySecure { .. }),
        "P19: Tailscale gate status must be OverlaySecure"
    );

    handle.stop();
}

#[tokio::test]
async fn test_p19_non_loopback_direct_with_insecure_opt_in_binds_and_serves() {
    // Explicitly opt in to insecure LAN mode
    set_allow_insecure_direct(true);

    let state = create_test_state();
    {
        let mut config = state.config.write();
        config.mode = RemoteNetworkMode::LocalNetwork;
        config.port = 0;
    }

    let local_lan_ip = crate::remote::state::SystemInterfaceResolver
        .local_network_address()
        .expect("active LAN IP on workstation");

    let resolver = Arc::new(LanInterfaceResolver { lan_ip: local_lan_ip });

    let (handle, local_addr) = start_remote_server_with_resolver_and_insecure_opt_in(
        Arc::clone(&state),
        resolver,
        true,
    )
    .await
    .expect("gateway starts with explicit insecure opt-in");

    assert!(local_addr.ip().is_loopback());
    assert!(
        handle.is_external_bound(),
        "With explicit insecure opt-in, non-loopback LAN interface must be bound"
    );
    assert!(
        matches!(handle.gate_status(), DirectGatewayGateStatus::InsecureLanAllowed { .. }),
        "Gate status must be InsecureLanAllowed"
    );

    handle.stop();

    // Reset back to secure default
    set_allow_insecure_direct(false);
}
