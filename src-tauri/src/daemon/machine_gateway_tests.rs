use super::*;
use tokio::io::AsyncReadExt;

#[tokio::test]
async fn idle_connection_expires_when_peer_sends_no_request() {
    // Given: a connected peer with no HTTP bytes and an observable connection lifetime.
    let (peer, stream) = tokio::io::duplex(64);
    let (closed, _) = tokio::sync::watch::channel(false);
    let mut connection = Connection {
        stream,
        closed,
        read_deadline: None,
        write_deadline: None,
    };
    let mut byte = [0];
    // When: the owner waits for input from the idle peer.
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(12),
        connection.read(&mut byte),
    )
    .await;
    drop(peer);
    // Then: the transport itself enforces the progress deadline, not the watchdog.
    assert_eq!(
        result
            .expect("transport deadline precedes watchdog")
            .expect_err("idle read rejected")
            .kind(),
        io::ErrorKind::TimedOut
    );
}

struct Counted(Arc<std::sync::atomic::AtomicUsize>);

impl Drop for Counted {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

#[tokio::test]
async fn an_open_gateway_connection_holds_no_guard_between_requests() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::AsyncWriteExt;
    // Given: a gateway whose guard counts in-flight requests, and a handler that reports the
    // count it observed while it ran.
    let held = Arc::new(AtomicUsize::new(0));
    let admit = {
        let held = held.clone();
        move || {
            held.fetch_add(1, Ordering::SeqCst);
            Ok(Some(Counted(held.clone())))
        }
    };
    let during = held.clone();
    let router = axum::Router::new().route(
        "/probe",
        axum::routing::get(move || {
            let during = during.clone();
            async move { during.load(Ordering::SeqCst).to_string() }
        }),
    );
    let (mut client, server) = tokio::io::duplex(4096);
    let (closed, mut receiver) = tokio::sync::watch::channel(false);
    let listener = Listener {
        connection: Some(Connection { stream: server, closed, read_deadline: None, write_deadline: None }),
    };
    let serving = tokio::spawn(async move {
        let _ = axum::serve(listener, guard_each_request(router, admit))
            .with_graceful_shutdown(async move {
                drop(receiver.wait_for(|closed| *closed).await);
            })
            .await;
    });
    // When: one keep-alive request is served and the connection stays open afterwards.
    client
        .write_all(b"GET /probe HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();
    let mut response = Vec::new();
    while !response.ends_with(b"\r\n\r\n1") {
        let mut byte = [0];
        tokio::time::timeout(std::time::Duration::from_secs(5), client.read_exact(&mut byte))
            .await
            .expect("response arrives")
            .expect("response byte");
        response.push(byte[0]);
    }
    // Then: the request held the guard while it ran, and the idle open connection holds none,
    // so a handover commit waiting for in-flight requests is not blocked by it.
    assert_eq!(held.load(Ordering::SeqCst), 0, "{}", String::from_utf8_lossy(&response));
    drop(client);
    serving.abort();
}

#[tokio::test]
async fn a_refused_admission_answers_service_unavailable() {
    use tokio::io::AsyncWriteExt;
    // Given: a gateway whose daemon is retiring and refuses every new request.
    let router = axum::Router::new().route("/probe", axum::routing::get(|| async { "served" }));
    let admit = || -> Result<Option<()>, String> { Err("HOST_UNAVAILABLE".into()) };
    let (mut client, server) = tokio::io::duplex(4096);
    let (closed, _receiver) = tokio::sync::watch::channel(false);
    let listener = Listener {
        connection: Some(Connection { stream: server, closed, read_deadline: None, write_deadline: None }),
    };
    let serving = tokio::spawn(async move {
        let _ = axum::serve(listener, guard_each_request(router, admit)).await;
    });
    // When: a request arrives.
    client
        .write_all(b"GET /probe HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut response = Vec::new();
    tokio::time::timeout(std::time::Duration::from_secs(5), client.read_to_end(&mut response))
        .await
        .expect("response arrives")
        .expect("response read");
    // Then: it is refused with the code and the handler never runs.
    let text = String::from_utf8_lossy(&response);
    assert!(text.starts_with("HTTP/1.1 503"), "{text}");
    assert!(text.contains("HOST_UNAVAILABLE") && !text.contains("served"), "{text}");
    serving.abort();
}
