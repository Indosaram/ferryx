use super::*;
use crate::remote::state::{
    set_test_overlay_proof_override, test_local_cgnat_address, InterfaceResolver,
    RemoteGatewayState, RemoteNetworkMode,
};
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

use super::DIRECT_GATE_TEST_MUTEX;

/// A bindable CGNAT address for the overlay gate tests.
///
/// The overlay *proof* is injected by [`OverlayProofGuard`], so this needs an address the VM
/// actually owns, not the Tailscale CLI. The address must sit on a non-loopback interface: the
/// resolver enumerates interfaces with `IFF_UP` and without `IFF_LOOPBACK`, so an `lo0` alias is
/// invisible here. The precheck job provisions one (`ifconfig <iface> alias 100.64.1.2 up`).
fn overlay_test_address() -> Ipv4Addr {
    test_local_cgnat_address().unwrap_or_else(|| {
        panic!(
            "overlay gate tests need a CGNAT address on a non-loopback interface; \
             provision one with: sudo ifconfig <iface> alias 100.64.1.2 up"
        )
    })
}

/// Injects overlay proof for one gate test and restores the real probe afterwards, so a leaked
/// override cannot flip a sibling test's classification.
struct OverlayProofGuard;

impl OverlayProofGuard {
    fn proven() -> Self {
        set_test_overlay_proof_override(Some(|_: &Ipv4Addr| Some(true)));
        Self
    }
}

impl Drop for OverlayProofGuard {
    fn drop(&mut self) {
        set_test_overlay_proof_override(None::<fn(&Ipv4Addr) -> Option<bool>>);
    }
}

#[tokio::test]
async fn test_p19_non_loopback_direct_without_insecure_opt_in_refuses_to_serve() {
    let _guard = DIRECT_GATE_TEST_MUTEX.lock().await;
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

    let resolver = Arc::new(LanInterfaceResolver {
        lan_ip: local_lan_ip,
    });

    // 1. start_remote_server_with_resolver_and_insecure_opt_in(..., false) must NOT bind the non-loopback external interface
    let (handle, local_addr) = start_remote_server_with_resolver_and_insecure_opt_in(
        Arc::clone(&state),
        resolver.clone(),
        false,
    )
    .await
    .expect("gateway starts loopback baseline");

    assert!(
        local_addr.ip().is_loopback(),
        "baseline primary listener is loopback"
    );
    assert!(
        !handle.is_external_bound(),
        "P19: non-loopback direct gateway must NOT bind external interface without explicit insecure opt-in"
    );
    assert!(
        matches!(
            handle.gate_status(),
            DirectGatewayGateStatus::InsecureLanGated { .. }
        ),
        "P19: gateway must surface explicit InsecureLanGated status"
    );
    handle.stop();

    // 2. Strict mode must return an Err refusing to serve
    let strict_result =
        start_remote_server_strict_with_resolver(Arc::clone(&state), resolver).await;
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
    let _guard = DIRECT_GATE_TEST_MUTEX.lock().await;
    // Without insecure opt-in, overlay mode (Tailscale) must remain unaffected
    set_allow_insecure_direct(false);

    let state = create_test_state();
    {
        let mut config = state.config.write();
        config.mode = RemoteNetworkMode::Tailscale;
        config.port = 0;
    }

    let _overlay = OverlayProofGuard::proven();
    let tailscale_ip = overlay_test_address();

    let resolver = Arc::new(TailscaleInterfaceResolver { tailscale_ip });

    let (handle, local_addr) =
        start_remote_server_with_resolver_and_insecure_opt_in(Arc::clone(&state), resolver, false)
            .await
            .expect("tailscale overlay mode starts");

    assert!(local_addr.ip().is_loopback());
    assert!(
        handle.is_external_bound(),
        "P19: Tailscale overlay mode must bind external interface without requiring insecure opt-in"
    );
    assert!(
        matches!(
            handle.gate_status(),
            DirectGatewayGateStatus::OverlaySecure { .. }
        ),
        "P19: Tailscale gate status must be OverlaySecure"
    );

    handle.stop();
}

#[tokio::test]
async fn test_p19_non_loopback_direct_with_insecure_opt_in_binds_and_serves() {
    let _guard = DIRECT_GATE_TEST_MUTEX.lock().await;
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

    let resolver = Arc::new(LanInterfaceResolver {
        lan_ip: local_lan_ip,
    });

    let (handle, local_addr) =
        start_remote_server_with_resolver_and_insecure_opt_in(Arc::clone(&state), resolver, true)
            .await
            .expect("gateway starts with explicit insecure opt-in");

    assert!(local_addr.ip().is_loopback());
    assert!(
        handle.is_external_bound(),
        "With explicit insecure opt-in, non-loopback LAN interface must be bound"
    );
    assert!(
        matches!(
            handle.gate_status(),
            DirectGatewayGateStatus::InsecureLanAllowed { .. }
        ),
        "Gate status must be InsecureLanAllowed"
    );

    handle.stop();

    // Reset back to secure default
    set_allow_insecure_direct(false);
}

#[tokio::test]
async fn test_p19_cgnat_address_without_overlay_proof_is_gated() {
    let _guard = DIRECT_GATE_TEST_MUTEX.lock().await;
    set_allow_insecure_direct(false);

    let state = create_test_state();
    {
        let mut config = state.config.write();
        config.mode = RemoteNetworkMode::LocalNetwork;
        config.port = 0;
    }

    // A CGNAT-range address (100.64.0.0/10) that is NOT a proven Tailscale interface
    let cgnat_ip = Ipv4Addr::new(100, 64, 1, 2);
    let resolver = Arc::new(LanInterfaceResolver { lan_ip: cgnat_ip });

    let (handle, local_addr) = start_remote_server_with_resolver_and_insecure_opt_in(
        Arc::clone(&state),
        resolver.clone(),
        false,
    )
    .await
    .expect("gateway starts loopback baseline");

    assert!(local_addr.ip().is_loopback());
    assert!(
        !handle.is_external_bound(),
        "CGNAT address on physical/non-tailscale interface must NOT be exempted as overlay"
    );
    assert!(
        matches!(
            handle.gate_status(),
            DirectGatewayGateStatus::InsecureLanGated { .. }
        ),
        "CGNAT address without authoritative overlay proof must be InsecureLanGated, got {:?}",
        handle.gate_status()
    );
    handle.stop();
}

#[tokio::test]
async fn test_p19_cgnat_address_with_authoritative_overlay_proof_is_exempt() {
    let _guard = DIRECT_GATE_TEST_MUTEX.lock().await;
    set_allow_insecure_direct(false);

    let state = create_test_state();
    {
        let mut config = state.config.write();
        config.mode = RemoteNetworkMode::LocalNetwork;
        config.port = 0;
    }

    let _overlay = OverlayProofGuard::proven();
    let tailscale_ip = overlay_test_address();

    let resolver = Arc::new(LanInterfaceResolver {
        lan_ip: tailscale_ip,
    });

    let (handle, local_addr) =
        start_remote_server_with_resolver_and_insecure_opt_in(Arc::clone(&state), resolver, false)
            .await
            .expect("authoritative overlay address starts without opt-in");

    assert!(local_addr.ip().is_loopback());
    assert!(
        handle.is_external_bound(),
        "Authoritative overlay interface must be bound without requiring insecure opt-in"
    );
    assert!(
        matches!(
            handle.gate_status(),
            DirectGatewayGateStatus::OverlaySecure { .. }
        ),
        "Gate status must be OverlaySecure, got {:?}",
        handle.gate_status()
    );

    handle.stop();
}
