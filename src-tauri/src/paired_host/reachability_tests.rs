//! Todo 2 acceptance: reachability book, record/emit API and the truthful legacy online flag.
use super::*;
use crate::paired_host::{
    inventory::{GrantScope, Inventory, Pairing},
    service::{PairRequest, PairedHostService, Secret},
};
use std::time::Duration;

fn pairing(machine: &str) -> Pairing {
    Pairing {
        relay_origin: "https://relay.example".into(),
        machine_id: machine.into(),
        display_label: format!("Machine {machine}"),
        grant_scope: GrantScope::Machine,
        device_token: format!("private-fixture-bearer-{machine}"),
    }
}

/// A service over an inventory that already holds one host paired with no network.
async fn seeded(machine: &'static str) -> (tempfile::TempDir, PairedHostService, crate::paired_host::inventory::HostView) {
    let (root, host) = crate::ipc::run_blocking(move || {
        let root = tempfile::tempdir().unwrap();
        let host = Inventory::open(root.path()).unwrap().pair(&pairing(machine)).unwrap();
        Ok((root, host))
    })
    .await
    .unwrap();
    let service = PairedHostService::open(root.path().to_owned());
    (root, service, host)
}

fn counting_sink(
    service: &mut PairedHostService,
) -> tokio::sync::mpsc::UnboundedReceiver<HostReachability> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    service.set_reachability_sink(Arc::new(move |entry| {
        let _ = tx.send(entry);
    }));
    rx
}

#[test]
fn wire_shape_matches_the_ui_mirror() {
    let entry = HostReachability {
        host_id: "https://relay.example/host/a".into(),
        generation: Epoch(7),
        state: ReachabilityState::Offline,
        checked_at_ms: 42,
        last_error: Some("HOST_UNAVAILABLE".into()),
    };
    let value = serde_json::to_value(&entry).unwrap();
    assert_eq!(
        value,
        serde_json::json!({
            "hostId": "https://relay.example/host/a",
            "generation": "7",
            "state": "offline",
            "checkedAtMs": 42,
            "lastError": "HOST_UNAVAILABLE",
        })
    );
    assert_eq!(serde_json::from_value::<HostReachability>(value).unwrap(), entry);
    assert_eq!(serde_json::to_value(ReachabilityState::Checking).unwrap(), "checking");
    assert_eq!(serde_json::to_value(ReachabilityState::Online).unwrap(), "online");
}

#[tokio::test]
async fn a_unprobed_host_lists_checking_and_is_not_online() {
    let (_root, service, host) = seeded("a").await;
    assert!(!host.online, "a pair without a probe must not report online");
    let rows = service.list().await.unwrap();
    assert!(!rows[0].online);
    let entries = service.reachability_list().await.unwrap();
    assert_eq!(entries, vec![HostReachability::checking(host.host_id.clone(), host.generation)]);
}

#[tokio::test]
async fn b_repeated_state_emits_once_and_a_change_emits_again() {
    let (_root, mut service, host) = seeded("b").await;
    let mut events = counting_sink(&mut service);
    for _ in 0..2 {
        assert!(service
            .record_reachability(host.host_id.clone(), host.generation, ReachabilityState::Online, None)
            .await);
    }
    // The sink runs before record_reachability returns, so try_recv is deterministic.
    let first = events.try_recv().expect("first Online must emit");
    assert_eq!(first.state, ReachabilityState::Online);
    assert!(events.try_recv().is_err(), "a repeated Online must not emit");
    assert!(service.list().await.unwrap()[0].online, "legacy online follows Online");

    assert!(service
        .record_reachability(host.host_id.clone(), host.generation, ReachabilityState::Offline, Some("TIMEOUT".into()))
        .await);
    let second = events.try_recv().expect("Offline must emit a second event");
    assert_eq!(second.state, ReachabilityState::Offline);
    assert_eq!(second.last_error.as_deref(), Some("TIMEOUT"));
    assert!(events.try_recv().is_err());
    assert!(!service.list().await.unwrap()[0].online, "legacy online follows Offline");
    assert_eq!(service.reachability_list().await.unwrap(), vec![second]);
}

#[tokio::test]
async fn c_stale_generation_is_ignored_and_not_emitted() {
    let (_root, mut service, host) = seeded("c").await;
    let mut events = counting_sink(&mut service);
    let stale = Epoch(host.generation.0 + 1);
    assert!(!service
        .record_reachability(host.host_id.clone(), stale, ReachabilityState::Online, None)
        .await);
    assert!(events.try_recv().is_err(), "a stale observation must not emit");
    assert!(!service.list().await.unwrap()[0].online);
    assert_eq!(
        service.reachability_list().await.unwrap()[0].state,
        ReachabilityState::Checking
    );
}

#[tokio::test]
async fn d_service_pair_yields_online() {
    use axum::{routing::{get, post}, Json};
    let root = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let router = axum::Router::new()
        .route("/api/v1/pair/exchange", post(|| async {
            Json(serde_json::json!({"token":"fixture-secret","machineId":"a","device":{"id":"d","name":"d","permission":"control","accessScope":"machine","createdAt":1,"lastSeenAt":1}}))
        }))
        .route("/host/a/api/v1/capabilities", get(|| async {
            Json(serde_json::json!({"apiVersion":1,"machineId":"a","daemonEpoch":"1","platform":"linux","accessScope":"machine","permission":"control","capabilities":[],"limits":{"directoryEntries":10,"terminalSessions":10}}))
        }));
    let mut tasks = tokio::task::JoinSet::new();
    tasks.spawn(async move { axum::serve(listener, router).await });
    let mut service = PairedHostService::open_test_loopback(root.path().join("data"));
    let mut events = counting_sink(&mut service);
    let host = tokio::time::timeout(
        Duration::from_secs(20),
        service.pair(PairRequest {
            relay_origin: origin,
            pin: Secret("fixture".into()),
            display_label: "fixture".into(),
        }),
    )
    .await
    .expect("pair bounded")
    .unwrap();
    tasks.shutdown().await;
    assert!(host.online, "the authenticate round-trip proves the host reachable");
    let event = events.try_recv().expect("pair must publish Online");
    assert_eq!((event.state, event.generation), (ReachabilityState::Online, host.generation));
    let entries = service.reachability_list().await.unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].state, ReachabilityState::Online);
    assert!(service.list().await.unwrap()[0].online);
    root.close().unwrap();
}

#[tokio::test]
async fn e_forget_drops_the_entry() {
    let (_root, service, host) = seeded("e").await;
    assert!(service
        .record_reachability(host.host_id.clone(), host.generation, ReachabilityState::Online, None)
        .await);
    assert!(service.reachability.lock().unwrap().get(&host.host_id, host.generation).is_some());
    service.forget(host.host_id.clone(), host.generation).await.unwrap();
    assert!(service.reachability.lock().unwrap().get(&host.host_id, host.generation).is_none());
    assert!(service.reachability_list().await.unwrap().is_empty());
}

#[tokio::test]
async fn revoke_drops_the_entry_and_the_revoked_generation_reads_checking() {
    let (_root, service, host) = seeded("r").await;
    assert!(service
        .record_reachability(host.host_id.clone(), host.generation, ReachabilityState::Online, None)
        .await);
    service
        .revoke_confirmed_auth_failure(host.host_id.clone(), host.generation, "UNAUTHORIZED")
        .await;
    assert!(service.reachability.lock().unwrap().get(&host.host_id, host.generation).is_none());
    let entries = service.reachability_list().await.unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].state, ReachabilityState::Checking);
    assert!(!service.list().await.unwrap()[0].online);
}
