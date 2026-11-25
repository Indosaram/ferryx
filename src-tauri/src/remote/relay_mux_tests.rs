use super::*;
use crate::remote::attach_crypto::AttachInitiator;
use crate::remote::attach_identity::AttachIdentity;
use crate::remote::session_transport::{establish_session, serve_secure_gateway, SessionTransport};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::sync::atomic::AtomicUsize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn random_attach_identity() -> AttachIdentity {
    let secret = x25519_dalek::StaticSecret::random_from_rng(rand::rngs::OsRng);
    AttachIdentity {
        public_key: STANDARD.encode(x25519_dalek::PublicKey::from(&secret).as_bytes()),
        private_key: STANDARD.encode(secret.to_bytes()),
    }
}

async fn encrypted_pair() -> (
    SecureStream<tokio::io::DuplexStream>,
    SecureStream<tokio::io::DuplexStream>,
) {
    let machine = random_attach_identity();
    let device_secret = x25519_dalek::StaticSecret::random_from_rng(rand::rngs::OsRng);
    let device_private = device_secret.to_bytes();
    let (client_io, server_io) = tokio::io::duplex(256 * 1024);

    let server_task = tokio::spawn({
        let machine = machine.clone();
        async move {
            match establish_session(server_io, true, Some(&machine), "m1", "s1", "1", |_| true)
                .await
                .expect("responder")
            {
                SessionTransport::Attached(secure) => secure,
                SessionTransport::Plain(_) => panic!("expected encrypted transport"),
            }
        }
    });

    let initiator =
        AttachInitiator::new(&device_private, &machine.public_key, "m1", "s1", "1").unwrap();
    let (client_secure, _) = initiator.connect(client_io).await.expect("initiator");
    let server_secure = server_task.await.expect("server join");
    (client_secure, server_secure)
}

async fn start_echo_server() -> (std::net::SocketAddr, Arc<AtomicUsize>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let accepted = Arc::new(AtomicUsize::new(0));
    let accepted_bg = Arc::clone(&accepted);
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            accepted_bg.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let mut buf = vec![0u8; 16 * 1024];
                loop {
                    match socket.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if socket.write_all(&buf[..n]).await.is_err() {
                                break;
                            }
                        }
                    }
                }
                let _ = socket.shutdown().await;
            });
        }
    });
    (addr, accepted)
}

#[tokio::test]
async fn many_concurrent_channels_share_one_encrypted_tunnel() {
    let (addr, accepted) = start_echo_server().await;
    let (client_secure, server_secure) = encrypted_pair().await;

    let server_task = tokio::spawn(async move {
        serve_secure_gateway(server_secure, move || async move {
            tokio::net::TcpStream::connect(addr).await
        })
        .await
    });

    let mux = RelayMuxClient::handshake(
        client_secure,
        Duration::from_secs(30),
        Duration::from_secs(90),
    )
    .await
    .expect("mux handshake");

    let mut tasks = Vec::new();
    for i in 0..32u32 {
        let mux = Arc::clone(&mux);
        tasks.push(tokio::spawn(async move {
            let mut stream = mux.open_stream().expect("open channel");
            let payload = format!("channel-{i}-{}", "x".repeat(4096)).into_bytes();
            stream.write_all(&payload).await.unwrap();
            stream.shutdown().await.unwrap();
            let mut reply = Vec::new();
            stream.read_to_end(&mut reply).await.unwrap();
            assert_eq!(reply, payload);
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    assert_eq!(accepted.load(Ordering::SeqCst), 32);
    mux.mark_dead();
    let _ = tokio::time::timeout(Duration::from_secs(5), server_task).await;
}

#[tokio::test]
async fn legacy_http_client_still_works_through_serve_secure_gateway() {
    let (addr, accepted) = start_echo_server().await;
    let (mut client_secure, server_secure) = encrypted_pair().await;

    let server_task = tokio::spawn(async move {
        serve_secure_gateway(server_secure, move || async move {
            tokio::net::TcpStream::connect(addr).await
        })
        .await
    });

    let req = b"GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n";
    client_secure.send_frame(req).await.unwrap();
    let echoed = client_secure.recv_frame().await.unwrap();
    assert_eq!(echoed, req);
    assert_eq!(accepted.load(Ordering::SeqCst), 1);
    drop(client_secure);
    let _ = tokio::time::timeout(Duration::from_secs(5), server_task).await;
}

#[tokio::test]
async fn older_daemon_replying_http_400_returns_unsupported() {
    let (client_secure, mut server_secure) = encrypted_pair().await;

    let old_daemon = tokio::spawn(async move {
        let first = server_secure.recv_frame().await.unwrap();
        assert_eq!(first, RELAY_MUX_HELLO);
        server_secure
            .send_frame(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n")
            .await
            .unwrap();
    });

    let err = RelayMuxClient::handshake(
        client_secure,
        Duration::from_secs(30),
        Duration::from_secs(90),
    )
    .await
    .err()
    .expect("must reject legacy reply");
    assert!(matches!(err, MuxHandshakeError::Unsupported), "{err:?}");
    old_daemon.await.unwrap();
}

#[tokio::test]
async fn large_burst_on_one_channel_does_not_starve_another_channel() {
    let (addr, _) = start_echo_server().await;
    let (client_secure, server_secure) = encrypted_pair().await;

    let server_task = tokio::spawn(async move {
        serve_secure_gateway(server_secure, move || async move {
            tokio::net::TcpStream::connect(addr).await
        })
        .await
    });

    let mux = RelayMuxClient::handshake(
        client_secure,
        Duration::from_secs(30),
        Duration::from_secs(90),
    )
    .await
    .expect("mux handshake");

    let mut heavy = mux.open_stream().unwrap();
    let mut light = mux.open_stream().unwrap();

    let big = vec![b'A'; 512 * 1024];
    heavy.write_all(&big).await.unwrap();
    light.write_all(b"ping-keystroke").await.unwrap();
    light.shutdown().await.unwrap();

    let mut light_reply = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), light.read_to_end(&mut light_reply))
        .await
        .expect("light channel must not starve behind heavy burst")
        .unwrap();
    assert_eq!(light_reply, b"ping-keystroke");

    heavy.shutdown().await.unwrap();
    let mut heavy_reply = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), heavy.read_to_end(&mut heavy_reply))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(heavy_reply.len(), big.len());

    mux.mark_dead();
    let _ = tokio::time::timeout(Duration::from_secs(5), server_task).await;
}

#[tokio::test]
async fn transport_loss_closes_open_streams_with_eof() {
    let (addr, _) = start_echo_server().await;
    let (client_secure, server_secure) = encrypted_pair().await;

    let server_task = tokio::spawn(async move {
        serve_secure_gateway(server_secure, move || async move {
            tokio::net::TcpStream::connect(addr).await
        })
        .await
    });

    let mux = RelayMuxClient::handshake(
        client_secure,
        Duration::from_secs(30),
        Duration::from_secs(90),
    )
    .await
    .expect("mux handshake");

    let mut stream = mux.open_stream().unwrap();
    stream.write_all(b"hello").await.unwrap();
    let mut buf = [0u8; 5];
    stream.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"hello");

    server_task.abort();
    let mut tail = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut tail))
        .await
        .expect("stream wakes on transport loss")
        .unwrap();
    assert!(!mux.is_live());
}
