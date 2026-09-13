#![cfg(unix)]

#[path = "support/a10_observed_listener.rs"]
mod observed;
use ferryx_lib::{daemon::server::DaemonServer, remote::{server::create_remote_router, terminal_wire::decode_frame}};
use futures_util::{FutureExt, SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{panic::AssertUnwindSafe, time::Duration};
use tokio_tungstenite::{client_async, tungstenite::{client::IntoClientRequest, Message, Error, error::ProtocolError}};

const DEADLINE: Duration = Duration::from_secs(10);
type Socket = tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>;

async fn json_reply(request: reqwest::RequestBuilder) -> Value {
    let response = request.send().await.unwrap();
    let status = response.status();
    let body = response.text().await.unwrap();
    assert!(status.is_success(), "{status}: {body}");
    serde_json::from_str(&body).unwrap()
}

async fn attach(base: &str, target: &Value) -> Socket {
    let url = format!("{}/api/v1/terminal/{}?daemonEpoch={}&afterSequence=0", base.replace("http:", "ws:"), target["sessionId"].as_str().unwrap(), target["daemonEpoch"].as_str().unwrap());
    let mut request = url.into_client_request().unwrap();
    request.headers_mut().insert("authorization", "Bearer owner-token".parse().unwrap());
    let dial = tokio::net::TcpSocket::new_v4().unwrap();
    dial.set_recv_buffer_size(1024).unwrap();
    let address = base.strip_prefix("http://").unwrap().parse().unwrap();
    let stream = dial.connect(address).await.unwrap();
    let (mut socket, _) = client_async(request, stream).await.unwrap();
    let Message::Text(text) = timeout_message(&mut socket).await.unwrap() else { panic!("attached first") };
    assert_eq!(serde_json::from_str::<Value>(&text).unwrap()["type"], "attached");
    socket
}

async fn timeout_message(socket: &mut Socket) -> Option<Message> {
    tokio::time::timeout(DEADLINE, socket.next()).await.unwrap().map(|result| result.expect("valid live WebSocket message"))
}

#[tokio::test]
async fn continuous_pty_overflow_cancels_pending_upgraded_socket_and_reconnects() {
    // Given an actual authenticated gateway, owned PTYs and attached sockets.
    let (root, owner) = tokio::task::spawn_blocking(|| {
        let root = tempfile::tempdir().unwrap();
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        std::fs::write(root.path().join("auth"), json!({"devices":{"owner":{"id":"owner","name":"owner","permission":"control","accessScope":"machine","createdAt":now,"lastSeenAt":now}},"tokens":{"owner-token":"owner"}}).to_string()).unwrap();
        let owner = DaemonServer::new_with_paths(Some(root.path().join("config")), Some(root.path().join("auth")));
        (root, owner)
    }).await.unwrap();
    let backend = owner.terminal_service().clone();
    let state = owner.remote_state().clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (accepted, mut connections) = tokio::sync::mpsc::channel(32);
    let listener = observed::ObservedListener { listener, accepted };
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let router = create_remote_router(state.clone());
    let task = tokio::spawn(async move { axum::serve(listener, router).with_graceful_shutdown(async { stopped.await.unwrap(); }).await.unwrap(); });
    let result = AssertUnwindSafe(async {
        let base = format!("http://{addr}");
        let client = reqwest::Client::builder().no_proxy().timeout(DEADLINE).build().unwrap();
        let project = json_reply(client.post(format!("{base}/api/v1/workspace/projects")).bearer_auth("owner-token")
            .body(json!({"requestId":uuid::Uuid::new_v4(),"repoPath":root.path()}).to_string())).await;
        let mut sessions = Vec::new();
        for _ in 0..2 {
            sessions.push(json_reply(client.post(format!("{base}/api/v1/sessions")).bearer_auth("owner-token")
                .body(json!({"requestId":uuid::Uuid::new_v4(),"workspaceId":project["workspaceId"],"worktree":null,"inheritFromSessionId":null,"cwdRelative":null,"cols":80,"rows":24,"startup":{"kind":"shell"}}).to_string())).await);
        }
        let id = sessions[0]["target"]["sessionId"].as_str().unwrap();
        let sibling_id = sessions[1]["target"]["sessionId"].as_str().unwrap();
        let original_pid = backend.get_session(id).unwrap().pid().unwrap();
        let mut held = attach(&base, &sessions[0]["target"]).await;
        let mut sibling = attach(&base, &sessions[1]["target"]).await;
        let held_addr = held.get_ref().local_addr().unwrap();
        let mut progress = tokio::time::timeout(DEADLINE, async {
            loop {
                let (peer, progress) = connections.recv().await.unwrap();
                if peer == held_addr { break progress; }
            }
        }).await.expect("held socket accepted within deadline");
        assert!(!progress.borrow().pending && !progress.borrow().dropped);
        let (_, mut raw) = backend.attach(id).unwrap();
        // When the actual shell produces continuously while its WS peer never reads.
        // Signals are installed before triggering input; the IO observer never gates IO.
        held.send(Message::Binary(b"dd if=/dev/zero bs=65536 count=1024 2>/dev/null; printf '\\nA10_%s:END\\n' DRAINED\r".to_vec().into())).await.unwrap();
        let start = tokio::time::Instant::now();
        let (transport, produced) = tokio::join!(
            tokio::time::timeout(Duration::from_secs(5), async {
                progress.wait_for(|state| state.pending).await.unwrap();
                progress.wait_for(|state| state.dropped).await.unwrap();
            }),
            tokio::time::timeout(DEADLINE, async {
                let mut total = 0usize;
                let mut tail = Vec::new();
                loop {
                    let bytes = raw.recv().await.expect("continuously drained real PTY broadcast");
                    total += bytes.len();
                    tail.extend_from_slice(&bytes);
                    if tail.windows(b"A10_DRAINED:END".len()).any(|w| w == b"A10_DRAINED:END") { break total; }
                    if tail.len() > 128 { tail.drain(..tail.len() - 128); }
                }
            }),
        );
        transport.unwrap();
        let produced = produced.unwrap();
        assert!(produced >= 64 * 1024 * 1024);
        assert!(start.elapsed() < Duration::from_secs(10), "not the read/write deadline");
        // Classify only expected abrupt termination; other protocol errors fail.
        tokio::time::timeout(DEADLINE, async {
            loop {
                match held.next().await {
                    None | Some(Ok(Message::Close(_))) => break,
                    Some(Err(Error::Protocol(ProtocolError::ResetWithoutClosingHandshake))) => break,
                    Some(Err(Error::Io(error))) if matches!(error.kind(), std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::UnexpectedEof) => break,
                    Some(Err(error)) => panic!("unexpected transport/protocol failure: {error}"),
                    Some(Ok(Message::Binary(frame))) => { decode_frame(&frame).expect("valid buffered terminal frame"); }
                    Some(Ok(_)) => {}
                }
            }
        }).await.unwrap();
        assert_eq!(backend.get_session(id).unwrap().pid(), Some(original_pid));
        sibling.send(Message::Binary(b"printf '\\nA10_%s:%s:END\\n' SIBLING \"$$\"\r".to_vec().into())).await.unwrap();
        let proof = tokio::time::timeout(DEADLINE, async {
            let mut bytes = Vec::new();
            loop {
                if let Some(Message::Binary(frame)) = timeout_message(&mut sibling).await {
                    bytes.extend_from_slice(decode_frame(&frame).unwrap().terminal_bytes);
                    let text = String::from_utf8_lossy(&bytes);
                    if let Some((_, tail)) = text.split_once("A10_SIBLING:") {
                        if let Some((pid, _)) = tail.split_once(":END") { break pid.parse::<u32>().unwrap(); }
                    }
                }
            }
        }).await.unwrap();
        assert_eq!(backend.get_session(sibling_id).unwrap().pid(), Some(proof));
        let mut resumed = attach(&base, &sessions[0]["target"]).await;
        let Some(Message::Binary(frame)) = timeout_message(&mut resumed).await else { panic!("gap replay") };
        let replay = decode_frame(&frame).unwrap();
        assert!(matches!(replay.metadata, ferryx_lib::remote::terminal_wire::Metadata::Replay { gap: Some(_), .. }));
        assert_eq!(backend.get_session(id).unwrap().pid(), Some(original_pid));
        eprintln!("A10_OUTPUT_LIVE_SOCKET original_pid={original_pid} sibling_pid={proof} pty_bytes={produced} actual_tcp_pending=true server_transport_dropped=true before_deadline=true reconnect_cursor=0 reconnect_gap=true");
    }).catch_unwind().await;
    state.auth_manager.revoke_device("owner");
    let mut cleanup = Vec::new();
    for id in backend.list_sessions() {
        let pty = backend.get_session(&id).unwrap();
        let pid = pty.pid();
        let closed = backend.close_session(&id).await;
        cleanup.push((pid, closed, pty.is_reaped(), pty.is_reader_finished()));
    }
    stop.send(()).unwrap();
    tokio::time::timeout(DEADLINE, task).await.unwrap().unwrap();
    drop(owner); drop(state);
    tokio::task::spawn_blocking(move || root.close().unwrap()).await.unwrap();
    for (pid, closed, reaped, reader_finished) in cleanup {
        closed.unwrap();
        assert!(reaped && reader_finished);
        eprintln!("A10_OUTPUT_SOCKET_CLEANUP pid={pid:?} reaped={reaped} reader_finished={reader_finished} listener_joined=true root_removed=true");
    }
    if let Err(panic) = result { std::panic::resume_unwind(panic); }
}
