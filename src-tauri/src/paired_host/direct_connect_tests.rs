use super::*;
use axum::{http::StatusCode, routing::post, Json, Router};

fn attempt(base_url: String) -> DirectAttempt {
    DirectAttempt {
        host_id: "host".into(),
        base_url,
        bearer: "token".into(),
        own_machine_id: "client-machine".into(),
        host_machine_id: "host-machine".into(),
        signing_key: SigningKey::from_bytes(&[1; 32]),
        host_key: SigningKey::from_bytes(&[2; 32]).verifying_key(),
        stun_servers: DEFAULT_STUN_SERVERS
            .iter()
            .map(|name| (*name).to_owned())
            .collect(),
    }
}

fn envelope() -> DirectOfferEnvelope {
    let key = SigningKey::from_bytes(&[1; 32]);
    DirectOfferEnvelope {
        version: 1,
        source_machine_id: "client-machine".into(),
        target_machine_id: "host-machine".into(),
        offer: DirectConnectionOffer::new(
            offer_session_id("client-machine", "host-machine"),
            std::net::SocketAddrV4::new(std::net::Ipv4Addr::LOCALHOST, 4000),
            vec![1; 16],
            [3; 32],
            DirectRole::Initiator,
            &key,
        ),
    }
}

async fn serve(router: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    format!("http://{addr}/host/abc")
}

#[test]
fn offer_url_keeps_relay_host_prefix() {
    let url = offer_url("https://relay.example.com/host/abc").unwrap();
    assert_eq!(url.as_str(), "https://relay.example.com/host/abc/api/v1/direct/offer");
    let trailing = offer_url("https://relay.example.com/host/abc/").unwrap();
    assert_eq!(trailing.path(), "/host/abc/api/v1/direct/offer");
}

#[tokio::test]
async fn older_host_without_the_route_means_unsupported() {
    let base = serve(Router::new()).await;
    let err = post_offer(&reqwest::Client::new(), &attempt(base), &envelope())
        .await
        .unwrap_err();
    assert_eq!(err, DirectConnectError::Unsupported);
}

#[tokio::test]
async fn forbidden_offer_stays_on_relay() {
    let router = Router::new().route(
        "/host/abc/api/v1/direct/offer",
        post(|| async { StatusCode::FORBIDDEN }),
    );
    let base = serve(router).await;
    let err = post_offer(&reqwest::Client::new(), &attempt(base), &envelope())
        .await
        .unwrap_err();
    assert_eq!(err, DirectConnectError::Unsupported);
}

#[tokio::test]
async fn garbage_answer_is_untrusted() {
    let router = Router::new().route(
        "/host/abc/api/v1/direct/offer",
        post(|| async { Json(serde_json::json!({ "version": 1, "answer": "nope" })) }),
    );
    let base = serve(router).await;
    let err = post_offer(&reqwest::Client::new(), &attempt(base), &envelope())
        .await
        .unwrap_err();
    assert_eq!(err, DirectConnectError::Untrusted);
}

#[tokio::test]
async fn unreachable_host_is_a_transport_error() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let err = post_offer(&reqwest::Client::new(), &attempt(format!("http://{addr}")), &envelope())
        .await
        .unwrap_err();
    assert!(matches!(err, DirectConnectError::Transport(_)));
}

#[test]
fn stun_config_defaults_disables_and_names_malformed_entries() {
    assert_eq!(
        stun_servers_from(None).unwrap(),
        DEFAULT_STUN_SERVERS.to_vec()
    );
    for off in ["", "  ", "off", "OFF", "none", "0"] {
        assert_eq!(
            stun_servers_from(Some(off)).unwrap(),
            Vec::<String>::new(),
            "{off:?}"
        );
    }
    assert_eq!(
        stun_servers_from(Some(
            "stun.example.net:3478, 192.0.2.7:19302 stun-1.example.net:3478"
        ))
        .unwrap(),
        [
            "stun.example.net:3478",
            "192.0.2.7:19302",
            "stun-1.example.net:3478"
        ]
    );
    for bad in [
        "stun.example.net",
        "stun:stun.example.net:3478",
        "host:0",
        "host:99999",
        ":3478",
        // IPv6 is refused: the direct path's sockets and resolution are IPv4-only.
        "[2001:db8::1]:3478",
        "[::1]:3478",
        "[garbage]:3478",
        "-:1",
        "a..b:1",
        "stun-.example.net:3478",
        "stun.example.net.:3478",
    ] {
        assert_eq!(
            stun_servers_from(Some(bad)).unwrap_err(),
            StunConfigError::InvalidEntry(bad.to_owned()),
            "{bad:?}"
        );
    }
    assert!(StunConfigError::InvalidEntry("x".into())
        .to_string()
        .contains(STUN_SERVERS_ENV));
}

#[tokio::test]
async fn empty_stun_list_disables_the_direct_path_and_keeps_the_relay() {
    let offers = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = std::sync::Arc::clone(&offers);
    let router = Router::new().route(
        "/host/abc/api/v1/direct/offer",
        post(move || {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            async { StatusCode::FORBIDDEN }
        }),
    );
    let mut disabled = attempt(serve(router).await);
    disabled.host_id = "stun-disabled-host".into();
    disabled.stun_servers = Vec::new();

    // The negotiator never reaches the offer round trip without a STUN server.
    let Err(err) = negotiate(&reqwest::Client::new(), &disabled).await else {
        panic!("negotiate must fail without a STUN server");
    };
    assert!(matches!(err, DirectConnectError::Transport(_)), "{err:?}");
    assert_eq!(offers.load(std::sync::atomic::Ordering::SeqCst), 0);

    // No worker starts, so the host keeps resolving to the relay base.
    let routes: &'static DirectRoutes = Box::leak(Box::new(DirectRoutes::new()));
    ensure_direct(
        routes,
        reqwest::Client::new(),
        disabled,
        1,
        std::future::pending(),
    );
    assert!(!is_running("stun-disabled-host", 1));
    assert!(routes.direct_for("stun-disabled-host").is_none());
    assert_eq!(offers.load(std::sync::atomic::Ordering::SeqCst), 0);
}
