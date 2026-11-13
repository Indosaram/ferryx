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
use std::collections::VecDeque;
use std::sync::{
    atomic::{AtomicU8, AtomicUsize, Ordering},
    Arc, Mutex,
};

/// The same host, now advertising a direct origin. Used to prove the probe loop adopts what a
/// host publishes about itself between pairings.
fn caps_with_origin() -> serde_json::Value {
    let mut value = caps();
    // Stamped at call time: an origin older than PUBLISHED_ORIGIN_MAX_AGE_MS is dropped by the
    // router before it can become a candidate, so a fixed stamp would pass today and rot later.
    value["publishedOrigins"] = json!([
        {"kind": "tailscale", "origin": "http://100.101.102.103:43821",
         "observedAt": super::super::direct_wire::unix_now_ms()}
    ]);
    value
}

const OK: u8 = 0;
const NO_CONTROL_CHANNEL: u8 = 1;
const BAD_GATEWAY: u8 = 2;
const REJECTED: u8 = 3;
/// The relay's 409 `MACHINE_OFFLINE`: no live control channel could check the token.
const MACHINE_OFFLINE: u8 = 4;
/// A valid answer from a host that also advertises a direct origin.
const PUBLISHING: u8 = 5;

fn caps() -> serde_json::Value {
    json!({"apiVersion":1,"machineId":"a","daemonEpoch":"1","platform":"linux","accessScope":"machine","permission":"control","capabilities":[],"limits":{"directoryEntries":10,"terminalSessions":10}})
}

struct Fixture {
    root: tempfile::TempDir,
    service: PairedHostService,
    host: HostView,
    mode: Arc<AtomicU8>,
    /// Answers served one per Capabilities request, in order, before falling back to `mode`.
    script: Arc<Mutex<VecDeque<u8>>>,
    /// Capabilities requests the relay answered.
    hits: Arc<AtomicUsize>,
    events: tokio::sync::mpsc::UnboundedReceiver<HostReachability>,
    task: tokio::task::JoinHandle<()>,
}

/// A loopback relay whose Capabilities answer is the next entry of `script`, else `mode`.
/// The host is paired while the relay answers, so the book starts at Online for its
/// generation.
async fn fixture() -> Fixture {
    let root = crate::ipc::run_blocking(|| Ok(tempfile::tempdir().unwrap()))
        .await
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let mode = Arc::new(AtomicU8::new(OK));
    let served = Arc::clone(&mode);
    let script = Arc::new(Mutex::new(VecDeque::new()));
    let scripted = Arc::clone(&script);
    let hits = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&hits);
    let router = Router::new()
        .route("/api/v1/pair/exchange", post(|| async {
            Json(json!({"token":"fixture-secret","machineId":"a","device":{"id":"d","name":"d","permission":"control","accessScope":"machine","createdAt":1,"lastSeenAt":1}}))
        }))
        .route("/host/a/api/v1/capabilities", get(move || {
            counted.fetch_add(1, Ordering::SeqCst);
            let next = scripted.lock().unwrap().pop_front();
            let mode = next.unwrap_or_else(|| served.load(Ordering::SeqCst));
            async move {
                match mode {
                    OK => Json(caps()).into_response(),
                    PUBLISHING => Json(caps_with_origin()).into_response(),
                    // The relay's bodyless answer for a machine with no live control channel.
                    NO_CONTROL_CHANNEL => axum::http::StatusCode::NOT_FOUND.into_response(),
                    BAD_GATEWAY => axum::http::StatusCode::BAD_GATEWAY.into_response(),
                    MACHINE_OFFLINE => (
                        axum::http::StatusCode::CONFLICT,
                        Json(json!({"code":"MACHINE_OFFLINE","message":"machine is offline"})),
                    )
                        .into_response(),
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
    Fixture { root, service, host, mode, script, hits, events, task }
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
    /// Serves `answers` to the next Capabilities requests, in order, and resets the count.
    fn serve(&self, answers: &[u8]) {
        *self.script.lock().unwrap() = answers.iter().copied().collect();
        self.hits.store(0, Ordering::SeqCst);
    }
    async fn auth_status(&self) -> super::super::inventory::AuthStatus {
        self.service.list().await.unwrap()[0].auth_status
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

/// A host that starts advertising a direct path is adopted by the probe loop, with no
/// re-pair: this is what keeps a machine that moved reachable without waiting for the user.
#[tokio::test]
async fn probe_adopts_an_origin_the_host_publishes_later() {
    let f = fixture().await;
    assert!(
        f.service.list().await.unwrap()[0].published_origins.is_empty(),
        "the fixture host advertises nothing at pairing time"
    );
    f.mode.store(PUBLISHING, Ordering::SeqCst);
    assert_eq!(f.probe().await, ReachabilityState::Online);
    let listed = f.service.list().await.unwrap().remove(0);
    assert_eq!(listed.published_origins.len(), 1);
    assert_eq!(listed.published_origins[0].origin, "http://100.101.102.103:43821");
    // The attached path now builds a direct candidate from that origin, relay appended last.
    let candidates = super::super::path_select::candidates_for_published(
        &listed.published_origins,
        &listed.relay_origin,
        &listed.machine_id,
    );
    let paths: Vec<_> = candidates.iter().map(|candidate| candidate.path).collect();
    assert_eq!(
        paths,
        vec![
            super::super::path_select::AttachPath::Tailscale,
            super::super::path_select::AttachPath::Relay
        ],
        "the advertised overlay is raced first and the relay stays the guaranteed last resort"
    );
    assert_eq!(candidates[0].expected_machine_id.as_deref(), Some("a"));
    f.finish().await;
}

/// A publication is bound to the generation that answered. Once the row is re-paired - a new
/// generation - an answer captured for the old one must not rewrite the new row's hints.
#[tokio::test]
async fn an_origin_published_for_an_old_generation_is_refused() {
    let f = fixture().await;
    let stale_generation = f.host.generation;
    f.mode.store(PUBLISHING, Ordering::SeqCst);
    assert_eq!(f.probe().await, ReachabilityState::Online);
    let current = f.service.list().await.unwrap().remove(0);
    assert_eq!(current.published_origins.len(), 1);

    // Advance the row to a new generation, exactly as a confirmed auth failure does.
    f.service
        .revoke_confirmed_auth_failure(current.host_id.clone(), stale_generation, "UNAUTHORIZED")
        .await;
    let advanced = f.service.list().await.unwrap().remove(0);
    assert_ne!(
        advanced.generation, stale_generation,
        "the row must really have moved to a new generation"
    );
    let carried = advanced.published_origins.clone();

    // A publication captured for the generation that no longer exists must not land: it could
    // otherwise revive reachability for an identity the user has since re-paired.
    let refused = f
        .service
        .publish_origins(
            current.host_id.clone(),
            stale_generation,
            vec![super::super::inventory::PublishedOrigin {
                kind: super::super::inventory::PublishedOriginKind::Lan,
                origin: "http://192.168.1.9:43821".into(),
                observed_at: super::super::direct_wire::unix_now_ms(),
            }],
        )
        .await;
    assert!(refused.is_err(), "a publication from a dead generation must not land");
    let after = f.service.list().await.unwrap().remove(0);
    assert_eq!(after.published_origins, carried);
    f.finish().await;
}

/// A host that publishes nothing leaves the stored hints alone rather than clearing them, so a
/// momentary empty read does not cost the user their direct path.
#[tokio::test]
async fn a_host_that_publishes_nothing_keeps_its_stored_origins() {
    let f = fixture().await;
    f.mode.store(PUBLISHING, Ordering::SeqCst);
    assert_eq!(f.probe().await, ReachabilityState::Online);
    let with_origin = f.service.list().await.unwrap().remove(0);
    assert_eq!(with_origin.published_origins.len(), 1);

    f.mode.store(OK, Ordering::SeqCst);
    assert_eq!(f.probe().await, ReachabilityState::Online);
    let after = f.service.list().await.unwrap().remove(0);
    assert_eq!(
        after.published_origins, with_origin.published_origins,
        "an empty publication is silence, not a retraction"
    );
    f.finish().await;
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
    f.serve(&[]);
    assert_eq!(f.probe().await, ReachabilityState::Checking);
    assert_eq!(f.hits.load(Ordering::SeqCst), 2, "the 401 is confirmed by one re-check");
    assert!(f.events.try_recv().is_err(), "a rejected credential publishes no reachability");
    let rows = f.service.list().await.unwrap();
    assert_eq!(rows[0].auth_status, super::super::inventory::AuthStatus::Revoked);
    assert!(!rows[0].online);
    f.finish().await;
}

#[tokio::test]
async fn d_relay_401_then_recheck_200_keeps_the_pairing_online() {
    let f = fixture().await;
    f.serve(&[REJECTED, OK]);
    assert_eq!(f.probe().await, ReachabilityState::Online);
    assert_eq!(f.hits.load(Ordering::SeqCst), 2);
    assert_eq!(f.auth_status().await, super::super::inventory::AuthStatus::Paired);
    assert_eq!(f.entry().await.state, ReachabilityState::Online);
    assert!(f.service.list().await.unwrap()[0].online);
    f.finish().await;
}

#[tokio::test]
async fn d_relay_401_then_recheck_502_keeps_the_pairing_and_reads_offline() {
    let f = fixture().await;
    f.serve(&[REJECTED, BAD_GATEWAY]);
    assert_eq!(f.probe().await, ReachabilityState::Offline);
    assert_eq!(f.hits.load(Ordering::SeqCst), 2);
    assert_eq!(f.auth_status().await, super::super::inventory::AuthStatus::Paired);
    let entry = f.entry().await;
    assert_eq!((entry.state, entry.last_error), (ReachabilityState::Offline, None));
    f.finish().await;
}

#[tokio::test]
async fn d_relay_401_then_recheck_409_machine_offline_keeps_the_pairing() {
    let f = fixture().await;
    f.serve(&[REJECTED, MACHINE_OFFLINE]);
    assert_eq!(f.probe().await, ReachabilityState::Offline);
    assert_eq!(f.auth_status().await, super::super::inventory::AuthStatus::Paired);
    assert_eq!(f.entry().await.last_error, None, "MACHINE_OFFLINE is plain offline");
    f.finish().await;
}

#[tokio::test]
async fn relay_409_machine_offline_reads_offline_without_an_error() {
    let f = fixture().await;
    f.mode.store(MACHINE_OFFLINE, Ordering::SeqCst);
    f.serve(&[]);
    assert_eq!(f.probe().await, ReachabilityState::Offline);
    assert_eq!(f.hits.load(Ordering::SeqCst), 1, "only a rejection triggers a re-check");
    let entry = f.entry().await;
    assert_eq!((entry.state, entry.last_error), (ReachabilityState::Offline, None));
    f.finish().await;
}

/// The daemon's `PairedHostRevoke` handler calls `confirm_and_revoke` with the operation's
/// UNAUTHORIZED: the re-check alone decides.
#[tokio::test]
async fn confirm_and_revoke_skips_the_revoke_when_the_recheck_succeeds() {
    let f = fixture().await;
    f.serve(&[OK]);
    let state = MachineClient::new()
        .confirm_and_revoke(&f.service, f.host.host_id.clone(), f.host.generation, "UNAUTHORIZED")
        .await;
    assert_eq!(state, ReachabilityState::Online);
    assert_eq!(f.hits.load(Ordering::SeqCst), 1);
    assert_eq!(f.auth_status().await, super::super::inventory::AuthStatus::Paired);
    f.finish().await;
}

#[tokio::test]
async fn confirm_and_revoke_revokes_when_the_recheck_answers_401() {
    let f = fixture().await;
    f.serve(&[REJECTED]);
    let state = MachineClient::new()
        .confirm_and_revoke(&f.service, f.host.host_id.clone(), f.host.generation, "UNAUTHORIZED")
        .await;
    assert_eq!(state, ReachabilityState::Checking);
    assert_eq!(f.hits.load(Ordering::SeqCst), 1);
    assert_eq!(f.auth_status().await, super::super::inventory::AuthStatus::Revoked);
    f.finish().await;
}

#[tokio::test]
async fn confirm_and_revoke_skips_the_revoke_when_the_recheck_is_unreachable() {
    let f = fixture().await;
    f.serve(&[NO_CONTROL_CHANNEL]);
    let state = MachineClient::new()
        .confirm_and_revoke(&f.service, f.host.host_id.clone(), f.host.generation, "UNAUTHORIZED")
        .await;
    assert_eq!(state, ReachabilityState::Offline);
    assert_eq!(f.auth_status().await, super::super::inventory::AuthStatus::Paired);
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
        unavailable_reason: None,
        published_origins: Vec::new(),
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
                        Ok(DaemonRequest::PairedHostRouteList) => json!({
                            "type": "pairedHostRouteListOk",
                            "entries": [{"hostId":"https://relay.example/host/a","state":"direct"}]
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

#[cfg(unix)]
#[tokio::test]
async fn g_daemon_without_the_route_capability_reports_unsupported() {
    let (_root, client, task) = fake_daemon(vec![
        "pairedHostInventoryV1",
        super::super::reachability::PAIRED_HOST_REACHABILITY_CAPABILITY,
    ])
    .await;
    let view = tokio::time::timeout(Duration::from_secs(10), crate::ipc::paired_host::route_list_view(&client))
        .await
        .expect("bounded")
        .unwrap();
    assert_eq!(serde_json::to_value(&view).unwrap(), json!({"supported": false, "entries": []}));
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn g_daemon_with_the_route_capability_returns_routes_in_the_ui_shape() {
    let (_root, client, task) = fake_daemon(vec![
        "pairedHostInventoryV1",
        super::super::route_view::PAIRED_HOST_ROUTE_CAPABILITY,
    ])
    .await;
    let view = tokio::time::timeout(Duration::from_secs(10), crate::ipc::paired_host::route_list_view(&client))
        .await
        .expect("bounded")
        .unwrap();
    assert_eq!(
        serde_json::to_value(&view).unwrap(),
        json!({"supported": true, "entries": [{"hostId":"https://relay.example/host/a","state":"direct"}]})
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
        unavailable_reason: None,
        published_origins: Vec::new(),
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
