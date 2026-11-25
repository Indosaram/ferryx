#![cfg(unix)]
use super::*;
use crate::ssh::bridge::{BridgeConnection, PROTOCOL_VERSION};
use crate::ssh::helper_runtime::process::{bridge_mux_with_idle_timeout, serve_test_daemon};
use std::time::Duration;

const BOUND: Duration = Duration::from_secs(10);

fn private_tempdir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    crate::ssh::helper_runtime::private_file(dir.path()).unwrap();
    dir
}

struct Peer {
    mux: Arc<HostMux>,
    _helper: std::thread::JoinHandle<Result<(), String>>,
    _dir: tempfile::TempDir,
}

async fn peer(host: &str) -> Peer {
    let dir = private_tempdir();
    serve_test_daemon(dir.path(), host);
    let (client_read, helper_write) = std::io::pipe().unwrap();
    let (helper_read, client_write) = std::io::pipe().unwrap();
    let root = dir.path().to_path_buf();
    let helper = std::thread::spawn(move || {
        bridge_mux_with_idle_timeout(&root, helper_read, helper_write, Duration::from_secs(60))
    });
    let reader = tokio::net::unix::pipe::Receiver::from_owned_fd(client_read.into()).unwrap();
    let writer = tokio::net::unix::pipe::Sender::from_owned_fd(client_write.into()).unwrap();
    let mut reader = tokio::io::BufReader::new(reader);
    let hello = tokio::time::timeout(BOUND, read_frame_async(&mut reader))
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!((hello.channel, hello.kind), (0, MuxKind::Hello));
    let mux = HostMux::start(writer, reader, None);
    Peer { mux, _helper: helper, _dir: dir }
}

async fn connection(peer: &Peer) -> BridgeConnection {
    BridgeConnection::from_mux_stream(peer.mux.open_stream().unwrap())
}

#[tokio::test]
async fn bridge_connections_over_one_mux_reach_the_helper() {
    let peer = peer("qa-mux-client").await;
    let mut first = connection(&peer).await;
    let mut second = connection(&peer).await;
    let a = tokio::time::timeout(BOUND, first.handshake()).await.unwrap().unwrap();
    let b = tokio::time::timeout(BOUND, second.handshake()).await.unwrap().unwrap();
    assert_eq!(a.protocol, PROTOCOL_VERSION);
    assert_eq!(a.host_id, "qa-mux-client");
    assert_eq!((b.host_id, b.owner_id, b.epoch), (a.host_id, a.owner_id, a.epoch));
}

#[tokio::test]
async fn many_channels_share_one_transport_concurrently() {
    let peer = peer("qa-mux-fanout").await;
    let mut tasks = Vec::new();
    for _ in 0..40 {
        let mut connection = connection(&peer).await;
        tasks.push(tokio::spawn(async move {
            tokio::time::timeout(BOUND, connection.handshake()).await.unwrap().unwrap().host_id
        }));
    }
    for task in tasks {
        assert_eq!(task.await.unwrap(), "qa-mux-fanout");
    }
    assert!(peer.mux.alive());
}

#[tokio::test]
async fn a_dropped_connection_does_not_disturb_its_neighbours() {
    let peer = peer("qa-mux-drop").await;
    let first = connection(&peer).await;
    let mut second = connection(&peer).await;
    drop(first);
    let reply = tokio::time::timeout(BOUND, second.handshake()).await.unwrap().unwrap();
    assert_eq!(reply.host_id, "qa-mux-drop");
}

#[tokio::test]
async fn transport_loss_fails_every_open_connection() {
    let dir = private_tempdir();
    serve_test_daemon(dir.path(), "qa-mux-loss");
    let (client_read, helper_write) = std::io::pipe().unwrap();
    let (helper_read, client_write) = std::io::pipe().unwrap();
    let reader = tokio::net::unix::pipe::Receiver::from_owned_fd(client_read.into()).unwrap();
    let writer = tokio::net::unix::pipe::Sender::from_owned_fd(client_write.into()).unwrap();
    let mux = HostMux::start(writer, tokio::io::BufReader::new(reader), None);
    let mut first = BridgeConnection::from_mux_stream(mux.open_stream().unwrap());
    let mut second = BridgeConnection::from_mux_stream(mux.open_stream().unwrap());
    drop(helper_write);
    drop(helper_read);

    let a = tokio::time::timeout(BOUND, first.handshake()).await.unwrap();
    let b = tokio::time::timeout(BOUND, second.handshake()).await.unwrap();
    assert!(a.is_err() && b.is_err(), "{a:?} {b:?}");
    tokio::time::timeout(BOUND, async {
        while mux.alive() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("mux marks itself dead after transport loss");
    assert!(matches!(mux.open_stream(), Err(BridgeError::ConnectionClosed)));
}

#[tokio::test]
async fn large_replies_arrive_intact_through_segmentation() {
    let peer = peer("qa-mux-large").await;
    let mut connection = connection(&peer).await;
    tokio::time::timeout(BOUND, connection.handshake()).await.unwrap().unwrap();
    let project = tempfile::tempdir().unwrap();
    let name = "x".repeat(200 * 1024);
    let file = project.path().join("big.txt");
    std::fs::write(&file, &name).unwrap();
    let registered = tokio::time::timeout(
        BOUND,
        connection.project_register("p", &project.path().to_string_lossy()),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(registered, "p");
    let oversized = tokio::time::timeout(
        BOUND,
        connection.request(
            "project.register",
            serde_json::json!({"id": name, "path": project.path().to_string_lossy()}),
            BOUND,
        ),
    )
    .await
    .unwrap();
    assert!(
        oversized.is_ok() || matches!(oversized, Err(BridgeError::Remote(_))),
        "a 200 KiB request travels as several segments and gets a framed answer: {oversized:?}"
    );
}

#[test]
fn only_a_usage_error_without_mux_means_unsupported() {
    assert!(is_unsupported_mux(
        "Usage: ferryx-remote-helper start|daemon --root <private-root> --host-id <id> | bridge --stdio [--root <private-root>]"
    ));
    assert!(!is_unsupported_mux(
        "Usage: ferryx-remote-helper start|daemon --root <private-root> --host-id <id> | bridge --stdio|--mux [--root <private-root>]"
    ));
    assert!(!is_unsupported_mux("Permission denied (publickey)."));
    assert!(!is_unsupported_mux(""));
}

#[tokio::test]
async fn idle_grace_keeps_a_mux_with_no_streams_alive() {
    let peer = peer("qa-mux-grace").await;
    let first = peer.mux.open_stream().unwrap();
    drop(first);
    assert!(peer.mux.alive(), "closing the last stream must not end the transport");
    let mut again = connection(&peer).await;
    let reply = tokio::time::timeout(BOUND, again.handshake()).await.unwrap().unwrap();
    assert_eq!(reply.host_id, "qa-mux-grace");
}

#[tokio::test]
async fn channel_ids_are_not_reused_while_open() {
    let peer = peer("qa-mux-ids").await;
    let streams: Vec<_> = (0..64).map(|_| peer.mux.open_stream().unwrap()).collect();
    let mut ids: Vec<u32> = streams.iter().map(MuxStream::channel).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 64);
    assert!(!ids.contains(&0), "channel 0 is reserved");
}

fn test_key(name: &str) -> MuxKey {
    MuxKey {
        connection: ConnectionKey::of(&crate::ssh::SshHost {
            id: name.into(),
            label: name.into(),
            hostname: format!("{name}.test"),
            username: None,
            port: None,
            identity_file: Some("/tmp/ferryx-test-identity".into()),
            jump_host: None,
            source: crate::ssh::SshHostSource::Manual,
            auth_method: crate::ssh::SshAuthMethod::Key,
            disabled: None,
            shell: None,
        }),
        root: format!("/tmp/{name}"),
    }
}

#[tokio::test]
async fn concurrent_opens_dial_once_and_share_the_mux() {
    let peer = peer("qa-mux-coalesce").await;
    let key = test_key("coalesce");
    let dials = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (release, released) = tokio::sync::oneshot::channel::<()>();
    let released = Arc::new(Mutex::new(Some(released)));
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let (key, dials, released, mux) =
            (key.clone(), Arc::clone(&dials), Arc::clone(&released), Arc::clone(&peer.mux));
        tasks.push(tokio::spawn(async move {
            open_stream_with(key, || async move {
                dials.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if let Some(wait) = released.lock().await.take() {
                    let _ = wait.await;
                }
                Ok(mux)
            })
            .await
        }));
    }
    tokio::task::yield_now().await;
    release.send(()).unwrap();
    for task in tasks {
        assert!(matches!(task.await.unwrap(), Ok(Acquired::Stream(_))));
    }
    assert_eq!(dials.load(std::sync::atomic::Ordering::SeqCst), 1, "one dial serves every caller");
}

#[tokio::test]
async fn callers_queued_behind_a_failed_dial_share_its_error() {
    let key = test_key("failed-dial");
    let dials = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (release, released) = tokio::sync::oneshot::channel::<()>();
    let released = Arc::new(Mutex::new(Some(released)));
    let mut tasks = Vec::new();
    for _ in 0..5 {
        let (key, dials, released) = (key.clone(), Arc::clone(&dials), Arc::clone(&released));
        tasks.push(tokio::spawn(async move {
            open_stream_with(key, || async move {
                dials.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if let Some(wait) = released.lock().await.take() {
                    let _ = wait.await;
                }
                Err(DialError::Bridge(BridgeError::ConnectionClosed))
            })
            .await
        }));
    }
    tokio::task::yield_now().await;
    release.send(()).unwrap();
    for task in tasks {
        assert!(task.await.unwrap().is_err());
    }
    assert_eq!(dials.load(std::sync::atomic::Ordering::SeqCst), 1, "a failure is not redialed by every waiter");

    let later = open_stream_with(key, || async { Err(DialError::Unsupported) }).await;
    assert!(matches!(later, Ok(Acquired::Legacy)), "a later caller dials again");
}
