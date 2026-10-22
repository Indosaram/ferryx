use super::*;
use crate::paired_host::direct_route::DirectGuard;
use serde_json::json;
use std::time::{Duration, Instant};

const BOUND: Duration = Duration::from_secs(5);

async fn next(rx: &mut broadcast::Receiver<DaemonRemoteEvent>) -> DaemonRemoteEvent {
    tokio::time::timeout(BOUND, rx.recv()).await.expect("bounded").expect("event")
}

#[test]
fn host_route_wire_shape_is_camel_case_and_never_carries_the_origin() {
    let routes = DirectRoutes::new();
    let epoch = routes.begin_attempt("h", Instant::now()).unwrap();
    assert!(routes.publish("h", epoch, "http://127.0.0.1:4711".into(), DirectGuard::default()));
    let wire = serde_json::to_value(HostRoute::current(&routes, "h".into())).unwrap();
    assert_eq!(wire, json!({"hostId": "h", "state": "direct"}));
    assert!(!wire.to_string().contains("127.0.0.1"), "the direct origin never reaches the renderer");
    for (state, text) in [
        (HostRouteState::Relay, "relay"),
        (HostRouteState::Attempting, "attempting"),
        (HostRouteState::Direct, "direct"),
        (HostRouteState::Disconnected, "disconnected"),
    ] {
        assert_eq!(serde_json::to_value(state).unwrap(), json!(text));
    }
    assert_eq!(
        serde_json::to_value(HostRoute::current(&routes, "unknown".into())).unwrap(),
        json!({"hostId": "unknown", "state": "relay"}),
        "a host without a route entry reports the relay"
    );
    assert!(serde_json::from_value::<HostRoute>(json!({"hostId": "h", "state": "direct", "origin": "x"})).is_err());
}

#[test]
fn route_list_request_and_response_round_trip() {
    use crate::daemon::protocol::{DaemonRequest, DaemonResponse};
    assert_eq!(
        serde_json::to_value(DaemonRequest::PairedHostRouteList).unwrap(),
        json!({"type": "pairedHostRouteList"})
    );
    let response: DaemonResponse = serde_json::from_value(json!({
        "type": "pairedHostRouteListOk", "entries": [{"hostId": "h", "state": "attempting"}]
    }))
    .unwrap();
    match response {
        DaemonResponse::PairedHostRouteListOk { entries } => assert_eq!(
            entries,
            vec![HostRoute { host_id: "h".into(), state: HostRouteState::Attempting }]
        ),
        other => panic!("unexpected response {other:?}"),
    }
    assert_ne!(PAIRED_HOST_ROUTE_CHANGED_EVENT, crate::ipc::paired_host::PAIRED_HOST_INVENTORY_CHANGED_EVENT);
    assert_ne!(PAIRED_HOST_ROUTE_CHANGED_EVENT, super::super::reachability::PAIRED_HOST_REACHABILITY_CHANGED_EVENT);
}

#[tokio::test]
async fn forwarder_publishes_each_transition_as_a_remote_event() {
    let routes = DirectRoutes::new();
    let (sink, mut out) = broadcast::channel(16);
    let forwarder = tokio::spawn(forward_route_events(
        routes.clone(),
        routes.subscribe(),
        sink,
        || async { Ok::<_, ()>(Vec::new()) },
        || true,
    ));
    let epoch = routes.begin_attempt("h", Instant::now()).unwrap();
    assert!(routes.publish("h", epoch, "http://127.0.0.1:1".into(), DirectGuard::default()));
    routes.mark_disconnected("h", epoch, Instant::now());
    let mut seen = Vec::new();
    for _ in 0..3 {
        let event = next(&mut out).await;
        assert_eq!(event.event, PAIRED_HOST_ROUTE_CHANGED_EVENT);
        seen.push(event.payload);
    }
    assert_eq!(
        seen,
        vec![
            json!({"hostId": "h", "state": "attempting"}),
            json!({"hostId": "h", "state": "direct"}),
            json!({"hostId": "h", "state": "disconnected"}),
        ]
    );
    forwarder.abort();
}

#[tokio::test]
async fn forwarder_resyncs_every_listed_host_after_lagging() {
    let routes = DirectRoutes::new();
    let events = routes.subscribe();
    // Overflow the route channel (capacity 64) before the forwarder reads anything.
    for i in 0..80 {
        let epoch = routes.begin_attempt(&format!("noise-{i}"), Instant::now()).unwrap();
        routes.fail(&format!("noise-{i}"), epoch, Instant::now());
    }
    let epoch = routes.begin_attempt("live", Instant::now()).unwrap();
    assert!(routes.publish("live", epoch, "http://127.0.0.1:2".into(), DirectGuard::default()));
    let (sink, mut out) = broadcast::channel(256);
    let forwarder = tokio::spawn(forward_route_events(
        routes.clone(),
        events,
        sink,
        || async { Ok::<_, ()>(vec!["live".to_string(), "idle".to_string()]) },
        || true,
    ));
    let first = next(&mut out).await;
    let second = next(&mut out).await;
    assert_eq!(first.payload, json!({"hostId": "live", "state": "direct"}));
    assert_eq!(second.payload, json!({"hostId": "idle", "state": "relay"}));
    forwarder.abort();
}

#[tokio::test]
async fn forwarder_stops_once_its_owner_is_gone() {
    let routes = DirectRoutes::new();
    let (sink, mut out) = broadcast::channel(16);
    let forwarder = tokio::spawn(forward_route_events(
        routes.clone(),
        routes.subscribe(),
        sink,
        || async { Ok::<_, ()>(Vec::new()) },
        || false,
    ));
    routes.begin_attempt("h", Instant::now()).unwrap();
    tokio::time::timeout(BOUND, forwarder).await.expect("bounded").unwrap();
    assert!(out.try_recv().is_err(), "a stopped forwarder publishes nothing");
}
