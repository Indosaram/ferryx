use super::*;
use tokio::io::AsyncReadExt;

#[tokio::test]
async fn idle_connection_expires_when_peer_sends_no_request() {
    // Given: a connected peer with no HTTP bytes and an observable connection lifetime.
    let (peer, stream) = tokio::io::duplex(64);
    let (closed, _) = tokio::sync::watch::channel(false);
    let mut connection = Connection { stream, closed, read_deadline: None, write_deadline: None };
    let mut byte = [0];
    // When: the owner waits for input from the idle peer.
    let result = tokio::time::timeout(std::time::Duration::from_secs(12), connection.read(&mut byte)).await;
    drop(peer);
    // Then: the transport itself enforces the progress deadline, not the watchdog.
    assert_eq!(result.expect("transport deadline precedes watchdog").expect_err("idle read rejected").kind(), io::ErrorKind::TimedOut);
}
