use base64::{engine::general_purpose::STANDARD, Engine as _};
use ferryx_lib::paired_host::attach::attach_relay_session;
use ferryx_lib::remote::attach_crypto::WebSocketByteStream;
use ferryx_lib::remote::relay_server::{relay_router, RelayState};
use ferryx_lib::remote::session_transport::{establish_session, SessionTransport};
use std::time::Duration;
use tokio::time::timeout;

#[tokio::test]
async fn attach_handshake_hides_sentinel_from_relay() {
    let state = RelayState::new(vec![]);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");
    let router = relay_router(state.clone());
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });

    let (generation, _notices, _grants) = state.register_control_channel("opaque-load".into());
    let session = "opaque-e2e-sentinel-test";
    assert!(state.issue_session("opaque-load", session, Some(generation), true));

    let machine = {
        let secret = x25519_dalek::StaticSecret::random_from_rng(rand::rngs::OsRng);
        ferryx_lib::remote::attach_identity::AttachIdentity {
            public_key: STANDARD.encode(x25519_dalek::PublicKey::from(&secret).as_bytes()),
            private_key: STANDARD.encode(secret.to_bytes()),
        }
    };
    let client_dir = tempfile::tempdir().unwrap();
    let device_public = ferryx_lib::remote::attach_client::load_or_generate_client_attach_identity(
        client_dir.path(),
    )
    .expect("client identity")
    .public_key;

    let daemon = tokio::spawn({
        let base = base.clone();
        let machine = machine.clone();
        async move {
            let ws_base = base.replace("http://", "ws://");
            let (ws, _) = tokio_tungstenite::connect_async(format!("{ws_base}/tunnel/data/{session}"))
                .await
                .unwrap();
            let expected = STANDARD.decode(&device_public).expect("device key");
            let transport = establish_session(
                WebSocketByteStream::new(ws),
                true,
                Some(&machine),
                "opaque-load",
                session,
                "3",
                move |key: &[u8; 32]| key.as_slice() == expected.as_slice(),
            )
            .await
            .expect("attached session");
            let SessionTransport::Attached(mut secure) = transport else {
                panic!("expected an attached transport");
            };
            secure.recv_frame().await.expect("decrypted frame")
        }
    });

    let mut secure = attach_relay_session(
        &base,
        session,
        "opaque-load",
        &machine.public_key,
        "3",
        client_dir.path(),
    )
    .await
    .expect("account attach handshake");
    let sentinel = b"FERRYX_E2EE_SENTINEL".to_vec();
    secure.send_frame(&sentinel).await.expect("send");

    let decrypted = timeout(Duration::from_secs(10), daemon)
        .await
        .expect("daemon replied in time")
        .expect("daemon task");
    assert_eq!(decrypted, sentinel, "only the daemon could read this");
    server.abort();
}
