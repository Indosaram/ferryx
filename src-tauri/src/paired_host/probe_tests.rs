//! Todo 3 acceptance: daemon probe, operation-path reachability feed, probe loop and the
//! capability-gated list, plus rolling-handover wire compatibility.
use super::super::reachability::{HostReachability, ReachabilityState};
use super::super::service::{PairRequest, Secret};
use super::*;
use axum::{
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde_json::json;
use std::sync::{
    atomic::{AtomicU8, Ordering},
    Arc,
};

const OK: u8 = 0;
const NO_CONTROL_CHANNEL: u8 = 1;
const BAD_GATEWAY: u8 = 2;
const REJECTED: u8 = 3;

fn caps() -> serde_json::Value {
    json!({"apiVersion":1,"machineId":"a","daemonEpoch":"1","platform":"linux","accessScope":"machine","permission":"control","capabilities":[],"limits":{"directoryEntries":10,"terminalSessions":10}})
}

struct Fixture {
    root: tempfile::TempDir,
    service: PairedHostService,
    host: HostView,
    mode: Arc<AtomicU8>,
    events: tokio::sync::mpsc::UnboundedReceiver<HostReachability>,
    task: tokio::task::JoinHandle<()>,
}

/// A loopback relay whose Capabilities answer is switched by `mode`. The host is paired
/// while the relay answers, so the book starts at Online for its generation.
async fn fixture() -> Fixture {
    let root = crate::ipc::run_blocking(|| Ok(tempfile::tempdir().unwrap()))
        .await
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let mode = Arc::new(AtomicU8::new(OK));
    let served = Arc::clone(&mode);
    let router = Router::new()
        .route("/api/v1/pair/exchange", post(|| async {
            Json(json!({"token":"fixture-secret","machineId":"a","device":{"id":"d","name":"d","permission":"control","accessScope":"machine","createdAt":1,"lastSeenAt":1}}))
        }))
        .route("/host/a/api/v1/capabilities", get(move || {
            let mode = served.load(Ordering::SeqCst);
            async move {
                match mode {
                    OK => Json(caps()).into_response(),
                    // The relay's bodyless answer for a machine with no live control channel.
                    NO_CONTROL_CHANNEL => axum::http::StatusCode::NOT_FOUND.into_response(),
                    BAD_GATEWAY => axum::http::StatusCode::BAD_GATEWAY.into_response(),
                    _ => axum::http::StatusCode::UNAUTHORIZED.into_response(),
                }
            }
        }));
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let mut service = PairedHostService::open_test_loopback(root.path().join("data"));
    let host = service
        .pair(PairRequest {
            relay_origin: origin,
            pin: Secret("fixture".into()),
            display_label: "fixture".into(),
        })
        .await
        .unwrap();
    // Subscribed after pair: every event below comes from the code under test.
    let (tx, events) = tokio::sync::mpsc::unbounded_channel();
    service.set_reachability_sink(Arc::new(move |entry| {
        let _ = tx.send(entry);
    }));
    Fixture { root, service, host, mode, events, task }
}

impl Fixture {
    async fn force(&mut self, state: ReachabilityState) {
        assert!(self
            .service
            .record_reachability(self.host.host_id.clone(), self.host.generation, state, None)
            .await);
        while self.events.try_recv().is_ok() {}
    }
    async fn probe(&self) -> ReachabilityState {
        tokio::time::timeout(
            Duration::from_secs(20),
            MachineClient::new().probe(&self.service, self.host.host_id.clone(), self.host.generation),
        )
        .await
        .expect("probe bounded")
    }
    async fn entry(&self) -> HostReachability {
        let mut entries = self.service.reachability_list().await.unwrap();
        assert_eq!(entries.len(), 1);
        entries.remove(0)
    }
    async fn finish(self) {
        self.task.abort();
        let _ = self.task.await;
        crate::ipc::run_blocking(move || {
            self.root.close().unwrap();
            Ok(())
        })
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn a_valid_capabilities_reads_online_with_one_event() {
    let mut f = fixture().await;
    f.force(ReachabilityState::Offline).await;
    assert_eq!(f.probe().await, ReachabilityState::Online);
    let event = f.events.try_recv().expect("Offline -> Online must emit");
    assert_eq!(event.state, ReachabilityState::Online);
    assert!(f.events.try_recv().is_err(), "exactly one event");
    assert_eq!(f.probe().await, ReachabilityState::Online);
    assert!(f.events.try_recv().is_err(), "a repeated Online emits nothing");
    assert!(f.service.list().await.unwrap()[0].online);
    f.finish().await;
}

#[tokio::test]
async fn b_relay_404_without_machine_error_reads_offline() {
    let mut f = fixture().await;
    f.mode.store(NO_CONTROL_CHANNEL, Ordering::SeqCst);
    assert_eq!(f.probe().await, ReachabilityState::Offline);
    let entry = f.entry().await;
    assert_eq!(entry.state, ReachabilityState::Offline);
    assert_eq!(entry.last_error, None, "no control channel is plain offline, not an error");
    assert_eq!(f.events.try_recv().unwrap().state, ReachabilityState::Offline);
    assert!(!f.service.list().await.unwrap()[0].online);
    f.finish().await;
}

#[tokio::test]
async fn c_relay_502_reads_offline() {
    let mut f = fixture().await;
    f.mode.store(BAD_GATEWAY, Ordering::SeqCst);
    assert_eq!(f.probe().await, ReachabilityState::Offline);
    let entry = f.entry().await;
    assert_eq!((entry.state, entry.last_error), (ReachabilityState::Offline, None));
    assert_eq!(f.events.try_recv().unwrap().state, ReachabilityState::Offline);
    f.finish().await;
}

#[tokio::test]
async fn d_relay_401_revokes_and_leaves_reachability_unchanged() {
    let mut f = fixture().await;
    f.mode.store(REJECTED, Ordering::SeqCst);
    assert_eq!(f.probe().await, ReachabilityState::Checking);
    assert!(f.events.try_recv().is_err(), "a rejected credential publishes no reachability");
    let rows = f.service.list().await.unwrap();
    assert_eq!(rows[0].auth_status, super::super::inventory::AuthStatus::Revoked);
    assert!(!rows[0].online);
    f.finish().await;
}

#[tokio::test]
async fn e_operation_success_records_online() {
    let mut f = fixture().await;
    f.force(ReachabilityState::Offline).await;
    MachineClient::new()
        .execute(
            &f.service,
            OperationRequest {
                host_id: f.host.host_id.clone(),
                generation: f.host.generation,
                operation: Operation::Capabilities,
            },
        )
        .await
        .unwrap();
    assert_eq!(f.entry().await.state, ReachabilityState::Online);
    assert_eq!(f.events.try_recv().unwrap().state, ReachabilityState::Online);
    // And an operation that cannot reach the host records Offline before erroring.
    f.mode.store(BAD_GATEWAY, Ordering::SeqCst);
    let error = MachineClient::new()
        .execute(
            &f.service,
            OperationRequest {
                host_id: f.host.host_id.clone(),
                generation: f.host.generation,
                operation: Operation::Capabilities,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "HOST_UNAVAILABLE");
    assert_eq!(f.entry().await.state, ReachabilityState::Offline);
    f.finish().await;
}

#[tokio::test]
async fn f_probe_loop_flips_offline_to_online_when_the_relay_answers() {
    let mut f = fixture().await;
    f.mode.store(BAD_GATEWAY, Ordering::SeqCst);
    let alive = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let running = Arc::clone(&alive);
    let probe_loop = tokio::spawn(run_probe_loop(
        f.service.clone(),
        Duration::from_millis(50),
        move || running.load(Ordering::SeqCst),
    ));
    // The sink was subscribed in fixture() before the loop started.
    let offline = tokio::time::timeout(Duration::from_secs(2), f.events.recv())
        .await
        .expect("first probe must publish Offline")
        .unwrap();
    assert_eq!(offline.state, ReachabilityState::Offline);
    f.mode.store(OK, Ordering::SeqCst);
    let online = tokio::time::timeout(Duration::from_secs(2), f.events.recv())
        .await
        .expect("a later tick must publish Online")
        .unwrap();
    assert_eq!(online.state, ReachabilityState::Online);
    assert!(f.service.list().await.unwrap()[0].online);
    alive.store(false, Ordering::SeqCst);
    tokio::time::timeout(Duration::from_secs(2), probe_loop)
        .await
        .expect("loop stops once the daemon is gone")
        .unwrap();
    f.finish().await;
}

/// The probe filter alone decides this: a Paired host whose grant is not Machine. The
/// inventory never stores that pair (load rejects Paired+Mirror), and capture would refuse it
/// anyway, so it is checked on the predicate itself, where only the grant clause excludes it.
#[test]
fn probe_loop_skips_hosts_without_a_machine_grant() {
    use super::super::inventory::{AuthStatus, GrantScope};
    let host = |auth_status, grant_scope| HostView {
        host_id: "https://relay.example/host/a".into(),
        relay_origin: "https://relay.example".into(),
        machine_id: "a".into(),
        display_label: "A".into(),
        grant_scope,
        generation: Epoch(1),
        auth_status,
        online: false,
        ssh_host_id: None,
        device_id: None,
    };
    assert!(is_probe_target(&host(AuthStatus::Paired, GrantScope::Machine)));
    assert!(
        !is_probe_target(&host(AuthStatus::Paired, GrantScope::Mirror)),
        "a paired host without a machine grant is never probed"
    );
    assert!(!is_probe_target(&host(AuthStatus::Revoked, GrantScope::Machine)));
}

#[cfg(unix)]
async fn fake_daemon(
    capabilities: Vec<&'static str>,
) -> (tempfile::TempDir, crate::daemon::client::DaemonClient, tokio::task::JoinHandle<()>) {
    use crate::daemon::protocol::{DaemonRequest, DAEMON_PROTOCOL_VERSION};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let root = tempfile::tempdir().unwrap();
    let socket = root.path().join("d.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let task = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let caps = capabilities.clone();
            tokio::spawn(async move {
                let (reader, mut writer) = stream.into_split();
                let mut lines = BufReader::new(reader).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let reply = match serde_json::from_str::<DaemonRequest>(&line) {
                        Ok(DaemonRequest::Handshake { .. }) => json!({
                            "type": "handshakeOk", "version": DAEMON_PROTOCOL_VERSION,
                            "pid": 1, "epoch": 1
                        }),
                        Ok(DaemonRequest::GetCapabilities) => json!({
                            "type": "capabilitiesOk", "capabilities": caps
                        }),
                        Ok(DaemonRequest::PairedHostReachabilityList) => json!({
                            "type": "pairedHostReachabilityListOk",
                            "entries": [{"hostId":"https://relay.example/host/a","generation":"2","state":"offline","checkedAtMs":5,"lastError":null}]
                        }),
                        _ => json!({"type": "error", "message": "unknown request"}),
                    };
                    let mut bytes = serde_json::to_vec(&reply).unwrap();
                    bytes.push(b'\n');
                    if writer.write_all(&bytes).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    (root, crate::daemon::client::DaemonClient::new_with_socket(socket), task)
}

#[cfg(unix)]
#[tokio::test]
async fn g_daemon_without_the_capability_reports_unsupported() {
    let (_root, client, task) = fake_daemon(vec!["pairedHostInventoryV1"]).await;
    let view = tokio::time::timeout(
        Duration::from_secs(10),
        crate::ipc::paired_host::reachability_list_view(&client),
    )
    .await
    .expect("bounded")
    .unwrap();
    assert_eq!(serde_json::to_value(&view).unwrap(), json!({"supported": false, "entries": []}));
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn g_daemon_with_the_capability_returns_entries_in_the_ui_shape() {
    let (_root, client, task) = fake_daemon(vec![
        "pairedHostInventoryV1",
        super::super::reachability::PAIRED_HOST_REACHABILITY_CAPABILITY,
    ])
    .await;
    let view = tokio::time::timeout(
        Duration::from_secs(10),
        crate::ipc::paired_host::reachability_list_view(&client),
    )
    .await
    .expect("bounded")
    .unwrap();
    assert_eq!(
        serde_json::to_value(&view).unwrap(),
        json!({"supported": true, "entries": [{"hostId":"https://relay.example/host/a","generation":"2","state":"offline","checkedAtMs":5,"lastError":null}]})
    );
    task.abort();
}

/// HostView exactly as defined at origin/main c7c3b549, before this change.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(dead_code)]
struct LegacyHostView {
    host_id: String,
    relay_origin: String,
    machine_id: String,
    display_label: String,
    grant_scope: super::super::inventory::GrantScope,
    generation: Epoch,
    auth_status: super::super::inventory::AuthStatus,
    online: bool,
    ssh_host_id: Option<String>,
    #[serde(default)]
    device_id: Option<String>,
}

#[test]
fn compat_old_and_new_host_view_wire_shapes_parse_both_ways() {
    // An old daemon's PairedHostListOk, as serialized before this change.
    let old_daemon = r#"{"type":"pairedHostListOk","hosts":[{"hostId":"https://relay.example/host/a","relayOrigin":"https://relay.example","machineId":"a","displayLabel":"A","grantScope":"machine","generation":"1","authStatus":"paired","online":true,"sshHostId":null,"deviceId":"d"}]}"#;
    match serde_json::from_str::<crate::daemon::protocol::DaemonResponse>(old_daemon).unwrap() {
        crate::daemon::protocol::DaemonResponse::PairedHostListOk { hosts } => {
            assert_eq!(hosts.len(), 1);
            assert!(hosts[0].online);
        }
        other => panic!("unexpected response {other:?}"),
    }
    // A new daemon's HostView read by an old GUI's struct: no new field may appear.
    let new_view = HostView {
        host_id: "https://relay.example/host/a".into(),
        relay_origin: "https://relay.example".into(),
        machine_id: "a".into(),
        display_label: "A".into(),
        grant_scope: super::super::inventory::GrantScope::Machine,
        generation: Epoch(1),
        auth_status: super::super::inventory::AuthStatus::Paired,
        online: false,
        ssh_host_id: None,
        device_id: Some("d".into()),
    };
    let wire = serde_json::to_string(&new_view).unwrap();
    let legacy: LegacyHostView = serde_json::from_str(&wire).expect("old GUI parses new HostView");
    assert!(!legacy.online);
    // The new request round-trips, and the reachability event payload never collides with
    // the inventory event the old GUI listens for.
    let request = serde_json::to_value(crate::daemon::protocol::DaemonRequest::PairedHostReachabilityList).unwrap();
    assert_eq!(request, json!({"type": "pairedHostReachabilityList"}));
    assert_ne!(
        super::super::reachability::PAIRED_HOST_REACHABILITY_CHANGED_EVENT,
        crate::ipc::paired_host::PAIRED_HOST_INVENTORY_CHANGED_EVENT
    );
}
