use super::*;
use futures_util::FutureExt;
use tokio_tungstenite::{
    tungstenite::{protocol::Role, Message as Wire},
    WebSocketStream,
};

async fn transport() -> (
    impl Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
    tokio::net::TcpStream,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dial = tokio::net::TcpSocket::new_v4().unwrap();
    dial.set_send_buffer_size(1024).unwrap();
    let (connected, accepted) = tokio::join!(
        dial.connect(listener.local_addr().unwrap()),
        listener.accept()
    );
    let stream = connected.unwrap();
    let (held, _) = accepted.unwrap();

    tokio::time::timeout(Duration::from_secs(5), stream.writable())
        .await
        .expect("TCP stream writable timeout")
        .expect("TCP stream writable error");

    let chunk = [0u8; 64 * 1024];
    let mut filled = 0usize;
    let max_fill = 128 * 1024 * 1024;
    loop {
        match stream.try_write(&chunk) {
            Ok(0) => panic!("TCP stream closed while saturating"),
            Ok(n) => {
                filled += n;
                if filled >= max_fill {
                    panic!("TCP buffer exceeded 128 MiB without blocking");
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(filled > 0, "TCP stream returned WouldBlock with 0 bytes filled");
                break;
            }
            Err(e) => panic!("unexpected error while saturating TCP stream: {:?}", e),
        }
    }
    let websocket = WebSocketStream::from_raw_socket(stream, Role::Server, None).await;
    let sender = websocket.with(|message: Message| {
        std::future::ready(Ok(match message {
            Message::Binary(bytes) => Wire::Binary(bytes),
            Message::Text(text) => Wire::Text(text.as_str().into()),
            Message::Ping(bytes) => Wire::Ping(bytes),
            Message::Pong(bytes) => Wire::Pong(bytes),
            Message::Close(_) => Wire::Close(None),
        }))
    });
    (sender, held)
}

#[tokio::test]
async fn blocked_tcp_write_is_cancelled_when_output_overflows() {
    // Given a real TCP WebSocket writer with a peer that never reads.
    let (mut sender, held) = transport().await;
    let (status, mut termination) = watch::channel(None);
    let write = machine_send(
        &mut sender,
        Message::Binary(vec![0; 16 * 1024 * 1024].into()),
        &mut termination,
    );
    tokio::pin!(write);
    let poll_res = futures_util::poll!(&mut write);
    assert!(
        poll_res.is_pending(),
        "expected write to be pending on saturated TCP stream, got: {:?}",
        poll_res
    );
    // When the independently subscribed hub status reports overflow.
    status.send_replace(Some(MachineOutputError::Overflow));
    // Then cancellation completes without peer progress or a clock advance.
    assert!(write.now_or_never().unwrap().is_err());
    drop(held);
}

#[tokio::test]
async fn blocked_tcp_write_expires_when_ten_seconds_elapse() {
    // Given an actual stalled TCP writer, observed pending before time changes.
    let (mut sender, held) = transport().await;
    let (_status, mut termination) = watch::channel(None);
    tokio::time::pause();
    let write = machine_send(
        &mut sender,
        Message::Binary(vec![0; 16 * 1024 * 1024].into()),
        &mut termination,
    );
    tokio::pin!(write);
    let poll_res = futures_util::poll!(&mut write);
    assert!(
        poll_res.is_pending(),
        "expected write to be pending on saturated TCP stream, got: {:?}",
        poll_res
    );
    // When the exact progress deadline elapses under controlled time.
    tokio::time::advance(Duration::from_secs(10)).await;
    // Then the bounded writer fails while the peer is still held.
    assert!(write.await.is_err());
    drop(held);
}

#[test]
fn serialized_controls_and_frames_are_rejected_when_wire_allowance_is_exceeded() {
    // Given exact-size and one-byte-over serialized messages.
    let exact = Message::Text("x".repeat(CONTROL_SLOT_BYTES - WS_HEADER_BYTES).into());
    let over = Message::Text("x".repeat(CONTROL_SLOT_BYTES - WS_HEADER_BYTES + 1).into());
    // When applying the wire admission boundary.
    let admitted = [
        machine_control(exact).is_ok(),
        machine_control(over).is_ok(),
        machine_frame(vec![0; MACHINE_FRAME_OVERHEAD - WS_HEADER_BYTES], 0).is_ok(),
        machine_frame(vec![0; MACHINE_FRAME_OVERHEAD - WS_HEADER_BYTES + 1], 0).is_ok(),
    ];
    // Then header-inclusive boundaries are exact.
    assert_eq!(admitted, [true, false, true, false]);
}
