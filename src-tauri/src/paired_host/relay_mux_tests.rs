use super::*;
use crate::remote::attach_crypto::AttachInitiator;
use crate::remote::attach_identity::AttachIdentity;
use crate::remote::relay_mux::RELAY_MUX_HELLO;
use crate::remote::session_transport::{establish_session, serve_secure_gateway, SessionTransport};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn random_attach_identity() -> AttachIdentity {
    let secret = x25519_dalek::StaticSecret::random_from_rng(rand::rngs::OsRng);
    AttachIdentity {
        public_key: STANDARD.encode(x25519_dalek::PublicKey::from(&secret).as_bytes()),
        private_key: STANDARD.encode(secret.to_bytes()),
    }
}

async fn make_mux_pair() -> (Arc<RelayMuxClient>, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                while let Ok(n) = s.read(&mut buf).await {
                    if n == 0 || s.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
            });
        }
    });

    let machine = random_attach_identity();
    let dev_secret = x25519_dalek::StaticSecret::random_from_rng(rand::rngs::OsRng);
    let dev_priv = dev_secret.to_bytes();
    let (client_io, server_io) = tokio::io::duplex(256 * 1024);

    let server_task = tokio::spawn({
        let machine = machine.clone();
        async move {
            let SessionTransport::Attached(secure) =
                establish_session(server_io, true, Some(&machine), "m1", "s1", "1", |_| true)
                    .await
                    .unwrap()
            else {
                panic!("expected attached")
            };
            let _ = serve_secure_gateway(secure, move || async move {
                tokio::net::TcpStream::connect(addr).await
            })
            .await;
        }
    });

    let initiator =
        AttachInitiator::new(&dev_priv, &machine.public_key, "m1", "s1", "1").unwrap();
    let (client_secure, _) = initiator.connect(client_io).await.unwrap();
    let mux = RelayMuxClient::handshake(
        client_secure,
        Duration::from_secs(30),
        Duration::from_secs(90),
    )
    .await
    .unwrap();
    (mux, server_task)
}

#[tokio::test]
async fn concurrent_callers_coalesce_into_a_single_dial() {
    let registry = Arc::new(Mutex::new(Registry::new()));
    let dials = Arc::new(AtomicUsize::new(0));
    let gate = Arc::new(tokio::sync::Barrier::new(1));
    let key = MuxKey {
        host_id: "host-a".into(),
        generation: 1,
    };

    let mut tasks = Vec::new();
    for i in 0..8usize {
        let registry = Arc::clone(&registry);
        let dials = Arc::clone(&dials);
        let key = key.clone();
        let _ = &gate;
        tasks.push(tokio::spawn(async move {
            let outcome = open_channel_in(&registry, key, true, || async move {
                dials.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(40)).await;
                let (mux, _srv) = make_mux_pair().await;
                Ok((mux, AttachPath::Relay))
            })
            .await
            .unwrap();
            let OpenChannelOutcome::Stream { mut stream, path, .. } = outcome else {
                panic!("expected stream");
            };
            assert_eq!(path, AttachPath::Relay);
            let msg = format!("hi-{i}").into_bytes();
            stream.write_all(&msg).await.unwrap();
            let mut buf = vec![0u8; msg.len()];
            stream.read_exact(&mut buf).await.unwrap();
            assert_eq!(buf, msg);
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    assert_eq!(dials.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn unsupported_host_caches_legacy_fallback_without_redialing() {
    let registry = Mutex::new(Registry::new());
    let dials = AtomicUsize::new(0);
    let key = MuxKey {
        host_id: "legacy-host".into(),
        generation: 2,
    };

    let first = open_channel_in(&registry, key.clone(), true, || async {
        dials.fetch_add(1, Ordering::SeqCst);
        let machine = random_attach_identity();
        let dev_secret = x25519_dalek::StaticSecret::random_from_rng(rand::rngs::OsRng);
        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        tokio::spawn({
            let machine = machine.clone();
            async move {
                let SessionTransport::Attached(mut srv) =
                    establish_session(server_io, true, Some(&machine), "m1", "s1", "1", |_| true)
                        .await
                        .unwrap()
                else {
                    return;
                };
                let frame = srv.recv_frame().await.unwrap();
                assert_eq!(frame, RELAY_MUX_HELLO);
                let _ = srv.send_frame(b"HTTP/1.1 400 Bad Request\r\n\r\n").await;
            }
        });
        let init = AttachInitiator::new(&dev_secret.to_bytes(), &machine.public_key, "m1", "s1", "1").unwrap();
        let (client_secure, _) = init.connect(client_io).await.unwrap();
        match handshake_stream(client_secure).await {
            Ok(mux) => Ok((mux, AttachPath::Relay)),
            Err(err) => Err(Ok(err)),
        }
    })
    .await
    .unwrap();
    assert!(matches!(first, OpenChannelOutcome::UseLegacy));

    let second = open_channel_in(&registry, key, true, || async {
        dials.fetch_add(1, Ordering::SeqCst);
        unreachable!("legacy cache must bypass dial")
    })
    .await
    .unwrap();
    assert!(matches!(second, OpenChannelOutcome::UseLegacy));
    assert_eq!(dials.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn dead_mux_is_evicted_and_redialed_on_next_open() {
    let registry = Mutex::new(Registry::new());
    let dials = AtomicUsize::new(0);
    let key = MuxKey {
        host_id: "host-recover".into(),
        generation: 1,
    };

    let (mux1, srv1) = make_mux_pair().await;
    let mux1_ref = Arc::clone(&mux1);
    let res1 = open_channel_in(&registry, key.clone(), true, || async {
        dials.fetch_add(1, Ordering::SeqCst);
        Ok((mux1, AttachPath::SshForward))
    })
    .await
    .unwrap();
    assert!(matches!(
        res1,
        OpenChannelOutcome::Stream {
            path: AttachPath::SshForward,
            reused: false,
            ..
        }
    ));

    mux1_ref.mark_dead();
    srv1.abort();

    let (mux2, _srv2) = make_mux_pair().await;
    let res2 = open_channel_in(&registry, key, true, || async {
        dials.fetch_add(1, Ordering::SeqCst);
        Ok((mux2, AttachPath::Relay))
    })
    .await
    .unwrap();
    assert!(matches!(
        res2,
        OpenChannelOutcome::Stream {
            path: AttachPath::Relay,
            reused: false,
            ..
        }
    ));
    assert_eq!(dials.load(Ordering::SeqCst), 2);
}
