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
    BridgeConnection::from_mux_stream(peer.mux.open_stream().await.unwrap())
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
    let mut first = BridgeConnection::from_mux_stream(mux.open_stream().await.unwrap());
    let mut second = BridgeConnection::from_mux_stream(mux.open_stream().await.unwrap());
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
    assert!(matches!(mux.open_stream().await, Err(BridgeError::ConnectionClosed)));
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
    let first = peer.mux.open_stream().await.unwrap();
    drop(first);
    assert!(peer.mux.alive(), "closing the last stream must not end the transport");
    let mut again = connection(&peer).await;
    let reply = tokio::time::timeout(BOUND, again.handshake()).await.unwrap().unwrap();
    assert_eq!(reply.host_id, "qa-mux-grace");
}

#[tokio::test]
async fn channel_ids_are_not_reused_while_open() {
    let peer = peer("qa-mux-ids").await;
    let mut streams = Vec::with_capacity(64);
    for _ in 0..64 {
        streams.push(peer.mux.open_stream().await.unwrap());
    }
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

#[tokio::test]
async fn open_stream_evicts_a_dead_cached_mux_and_redials() {
    let key = test_key("dead-cached-mux");
    // Construct a cached mux whose outbound channel is already closed while `dead` is still false:
    // this reproduces the exact race where `is_alive()` returns true just before `open()` fails.
    let (outbound, outbound_rx) = mpsc::channel(1);
    drop(outbound_rx);
    let broken_mux = Arc::new(HostMux {
        outbound,
        channels: std::sync::Mutex::new(Channels {
            next_id: 1,
            inbound: HashMap::new(),
            open_streams: 0,
            dead: false,
            idle_since: None,
            last_inbound: Instant::now(),
        }),
        _transport: std::sync::Mutex::new(None),
    });
    REGISTRY
        .slots
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(key.clone(), Slot::Live(Arc::clone(&broken_mux)));

    let fresh = peer("qa-mux-redial").await;
    let dials = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let dials_for_cb = Arc::clone(&dials);
    let fresh_mux = Arc::clone(&fresh.mux);

    let acquired = open_stream_with(key.clone(), || async move {
        dials_for_cb.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(fresh_mux)
    })
    .await
    .expect("must evict the dead cached mux and redial transparently");

    assert_eq!(dials.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(!broken_mux.alive(), "evicted broken mux must be marked dead");
    let Acquired::Stream(stream) = acquired else {
        panic!("expected a live mux stream after redial");
    };
    let mut conn = BridgeConnection::from_mux_stream(stream);
    let reply = tokio::time::timeout(BOUND, conn.handshake()).await.unwrap().unwrap();
    assert_eq!(reply.host_id, "qa-mux-redial");
    invalidate_key(&key);
}

#[tokio::test]
async fn open_waits_for_outbound_backpressure_and_release_delivers_close() {
    let (outbound, mut outbound_rx) = mpsc::channel(1);
    // Saturate the bounded outbound queue to simulate bursty traffic from other panes.
    outbound
        .send(MuxFrame { channel: 99, kind: MuxKind::Data, payload: vec![1, 2, 3] })
        .await
        .unwrap();

    let mux = Arc::new(HostMux {
        outbound: outbound.clone(),
        channels: std::sync::Mutex::new(Channels {
            next_id: 1,
            inbound: HashMap::new(),
            open_streams: 0,
            dead: false,
            idle_since: None,
            last_inbound: Instant::now(),
        }),
        _transport: std::sync::Mutex::new(None),
    });

    let opener = {
        let mux = Arc::clone(&mux);
        tokio::spawn(async move { mux.open_stream().await })
    };

    // Drain the filler frame; the waiting open must now proceed and emit MuxKind::Open.
    let filler = tokio::time::timeout(BOUND, outbound_rx.recv()).await.unwrap().unwrap();
    assert_eq!((filler.channel, filler.kind), (99, MuxKind::Data));

    let stream = tokio::time::timeout(BOUND, opener)
        .await
        .unwrap()
        .unwrap()
        .expect("open must wait for backpressure instead of failing with ConnectionClosed");
    let open_frame = tokio::time::timeout(BOUND, outbound_rx.recv()).await.unwrap().unwrap();
    let ch = stream.channel();
    assert_eq!((open_frame.channel, open_frame.kind), (ch, MuxKind::Open));

    // Saturate outbound again before dropping the stream; release must still deliver Close.
    outbound
        .send(MuxFrame { channel: 99, kind: MuxKind::Data, payload: vec![4, 5, 6] })
        .await
        .unwrap();
    drop(stream);

    let filler2 = tokio::time::timeout(BOUND, outbound_rx.recv()).await.unwrap().unwrap();
    assert_eq!((filler2.channel, filler2.kind), (99, MuxKind::Data));
    let close_frame = tokio::time::timeout(BOUND, outbound_rx.recv()).await.unwrap().unwrap();
    assert_eq!((close_frame.channel, close_frame.kind), (ch, MuxKind::Close));
}

#[tokio::test]
async fn invalidate_key_evicts_live_mux_and_preserves_legacy() {
    let live_key = test_key("invalidate-live");
    let peer = peer("qa-mux-invalidate").await;
    REGISTRY
        .slots
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(live_key.clone(), Slot::Live(Arc::clone(&peer.mux)));
    assert!(peer.mux.alive());

    invalidate_key(&live_key);
    assert!(!peer.mux.alive(), "invalidated mux must be marked dead");
    assert!(existing_slot(&live_key).is_none());

    let legacy_key = test_key("invalidate-legacy");
    REGISTRY
        .slots
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(legacy_key.clone(), Slot::Legacy(Instant::now()));
    invalidate_key(&legacy_key);
    assert!(matches!(existing_slot(&legacy_key), Some(ExistingSlot::Legacy)));
    REGISTRY.slots.lock().unwrap_or_else(|p| p.into_inner()).remove(&legacy_key);
}
