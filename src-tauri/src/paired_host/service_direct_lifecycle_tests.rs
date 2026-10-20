//! Credential lifecycle must synchronously drop the direct route of the replaced
//! generation, and a stale (already superseded) failure must leave the new route.
use super::*;
use crate::paired_host::direct_route::DirectGuard;
use axum::{
    routing::{get, post},
    Json,
};
use serde_json::json;

struct Fixture {
    root: tempfile::TempDir,
    service: PairedHostService,
    origin: String,
    tasks: tokio::task::JoinSet<std::io::Result<()>>,
}

impl Fixture {
    async fn start() -> Self {
        let root = tempfile::tempdir().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let router = axum::Router::new()
            .route("/api/v1/pair/exchange", post(|Json(input): Json<serde_json::Value>| async move {
                Json(json!({"token": input["pin"].as_str().unwrap(), "machineId": "a",
                    "device": {"id":"fixture", "name":"fixture", "permission":"control", "accessScope":"machine", "createdAt":1, "lastSeenAt":1}}))
            }))
            .route("/host/{machine}/api/v1/capabilities", get(|axum::extract::Path(machine): axum::extract::Path<String>| async move {
                Json(json!({"apiVersion":1,"machineId":machine,"daemonEpoch":"1","platform":"macos","accessScope":"machine","permission":"control","capabilities":[],"limits":{"directoryEntries":10,"terminalSessions":10}}))
            }));
        let mut tasks = tokio::task::JoinSet::new();
        tasks.spawn(async move { axum::serve(listener, router).await });
        let service = PairedHostService::open_test_loopback(root.path().join("data"));
        Self { root, service, origin, tasks }
    }

    async fn pair(&self, pin: &str) -> HostView {
        self.service
            .pair(PairRequest {
                relay_origin: self.origin.clone(),
                pin: Secret(pin.into()),
                display_label: "fixture".into(),
            })
            .await
            .unwrap()
    }

    async fn finish(mut self) {
        self.tasks.shutdown().await;
        self.root.close().unwrap();
    }
}

/// Publishes a live direct route for `host` without any network (guard is inert).
fn plant_route(host: &str) {
    let epoch = GLOBAL_DIRECT_ROUTES
        .begin_attempt(host, std::time::Instant::now())
        .expect("no route or attempt is live for this host");
    assert!(GLOBAL_DIRECT_ROUTES.publish(host, epoch, "http://127.0.0.1:9".into(), DirectGuard::default()));
    assert!(has_route(host));
}

fn has_route(host: &str) -> bool {
    GLOBAL_DIRECT_ROUTES.origin_for(host).is_some()
}

async fn bounded(test: impl std::future::Future<Output = ()>) {
    tokio::time::timeout(Duration::from_secs(10), test)
        .await
        .expect("lifecycle test exceeded its bound");
}

#[tokio::test]
async fn re_pair_drops_superseded_direct_route() {
    bounded(async {
        let fixture = Fixture::start().await;
        let first = fixture.pair("first").await;
        plant_route(&first.host_id);
        let second = fixture.pair("second").await;
        assert_eq!(second.generation.0, first.generation.0 + 1);
        assert!(!has_route(&second.host_id));
        fixture.finish().await;
    })
    .await;
}

#[tokio::test]
async fn revoke_on_auth_failure_drops_direct_route() {
    bounded(async {
        let fixture = Fixture::start().await;
        let host = fixture.pair("first").await;
        plant_route(&host.host_id);
        fixture
            .service
            .revoke_on_auth_failure(host.host_id.clone(), host.generation)
            .await;
        assert!(!has_route(&host.host_id));
        fixture.finish().await;
    })
    .await;
}

#[tokio::test]
async fn forget_drops_direct_route() {
    bounded(async {
        let fixture = Fixture::start().await;
        let host = fixture.pair("first").await;
        plant_route(&host.host_id);
        fixture
            .service
            .forget(host.host_id.clone(), host.generation)
            .await
            .unwrap();
        assert!(!has_route(&host.host_id));
        fixture.finish().await;
    })
    .await;
}

#[tokio::test]
async fn stale_generation_failure_keeps_fresh_direct_route() {
    bounded(async {
        let fixture = Fixture::start().await;
        let old = fixture.pair("old").await;
        let fresh = fixture.pair("fresh").await;
        let host = fresh.host_id.clone();
        plant_route(&host);
        fixture
            .service
            .revoke_on_auth_failure(host.clone(), old.generation)
            .await;
        assert!(fixture.service.forget(host.clone(), old.generation).await.is_err());
        assert!(has_route(&host));
        stop_direct(&GLOBAL_DIRECT_ROUTES, &host, fresh.generation.0);
        assert!(!has_route(&host));
        fixture.finish().await;
    })
    .await;
}
