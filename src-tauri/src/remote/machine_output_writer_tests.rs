use super::*;
use crate::remote::server::{
    send_machine_output_overflow_close, MACHINE_OUTPUT_OVERFLOW_CLOSE_CODE,
    MACHINE_OUTPUT_OVERFLOW_CLOSE_REASON,
};
use futures_util::{FutureExt, SinkExt, StreamExt};
use tokio::time::Instant;
use tokio_tungstenite::{
    tungstenite::{protocol::Role, Message as Wire},
    WebSocketStream,
};

struct AbortOnDrop<T>(Option<tokio::task::JoinHandle<T>>);

impl<T> AbortOnDrop<T> {
    fn new(handle: tokio::task::JoinHandle<T>) -> Self {
        Self(Some(handle))
    }
}

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            handle.abort();
        }
    }
}

async fn transport() -> (
    impl Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
    tokio::net::TcpStream,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dial = tokio::net::TcpSocket::new_v4().unwrap();
    dial.set_send_buffer_size(1024).unwrap_or_else(|error| {
        panic!(
            "failed to configure the small TCP send buffer required by this backpressure test on {}: {error}",
            std::env::consts::OS
        )
    });
    let (connected, accepted) = tokio::join!(
        dial.connect(listener.local_addr().unwrap()),
        listener.accept()
    );
    let stream = connected.unwrap();
    let (held, _) = accepted.unwrap();
    let _ = stream.set_nodelay(true);
    let _ = held.set_nodelay(true);
    let websocket = WebSocketStream::from_raw_socket(stream, Role::Server, None).await;
    let sender = websocket.with(|message: Message| {
        std::future::ready(Ok(match message {
            Message::Binary(bytes) => Wire::Binary(bytes),
            Message::Text(text) => Wire::Text(text.as_str().into()),
            Message::Ping(bytes) => Wire::Ping(bytes),
            Message::Pong(bytes) => Wire::Pong(bytes),
            Message::Close(frame) => Wire::Close(frame.map(|frame| {
                tokio_tungstenite::tungstenite::protocol::CloseFrame {
                    code: tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::from(frame.code),
                    reason: frame.reason.to_string().into(),
                }
            })),
        }))
    });
    (sender, held)
}

async fn connected_websocket_pair() -> (
    impl Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
    WebSocketStream<tokio::net::TcpStream>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dial = tokio::net::TcpSocket::new_v4().unwrap();
    let (connected, accepted) = tokio::join!(
        dial.connect(listener.local_addr().unwrap()),
        listener.accept()
    );
    let stream = connected.unwrap();
    let (held, _) = accepted.unwrap();
    let _ = stream.set_nodelay(true);
    let _ = held.set_nodelay(true);
    let server_ws = WebSocketStream::from_raw_socket(stream, Role::Server, None).await;
    let client_ws = WebSocketStream::from_raw_socket(held, Role::Client, None).await;
    let sender = server_ws.with(|message: Message| {
        std::future::ready(Ok(match message {
            Message::Binary(bytes) => Wire::Binary(bytes),
            Message::Text(text) => Wire::Text(text.as_str().into()),
            Message::Ping(bytes) => Wire::Ping(bytes),
            Message::Pong(bytes) => Wire::Pong(bytes),
            Message::Close(frame) => Wire::Close(frame.map(|frame| {
                tokio_tungstenite::tungstenite::protocol::CloseFrame {
                    code: tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::from(frame.code),
                    reason: frame.reason.to_string().into(),
                }
            })),
        }))
    });
    (sender, client_ws)
}

async fn fill_transport_to_pending<S>(sender: &mut S) -> Result<(), String>
where
    S: Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    let chunk = Message::Binary(vec![0xaa; 64 * 1024].into());
    let deadline = Instant::now() + Duration::from_secs(5);
    for iteration in 0..500 {
        if Instant::now() > deadline {
            return Err("Timed out attempting to saturate transport buffer".into());
        }
        tokio::time::timeout(Duration::from_millis(500), sender.feed(chunk.clone()))
            .await
            .map_err(|_| "feed timed out")?
            .map_err(|e| format!("feed error on iteration {iteration}: {e}"))?;
        let mut flush = sender.flush();
        if futures_util::poll!(&mut flush).is_pending() {
            return Ok(());
        }
    }
    Err("Failed to saturate transport buffer within 500 attempts".into())
}

#[tokio::test]
async fn blocked_tcp_write_is_cancelled_when_output_overflows() {
    let (mut sender, held) = transport().await;
    let (status, mut termination) = watch::channel(None);
    fill_transport_to_pending(&mut sender)
        .await
        .expect("transport buffer must saturate to pending");
    let write = machine_send(
        &mut sender,
        Message::Binary(vec![0; 64 * 1024].into()),
        &mut termination,
    );
    tokio::pin!(write);
    assert!(futures_util::poll!(&mut write).is_pending());
    status.send_replace(Some(MachineOutputError::Overflow));
    assert!(write.now_or_never().unwrap().is_err());
    drop(held);
}

#[tokio::test]
async fn blocked_tcp_write_expires_when_ten_seconds_elapse() {
    let (mut sender, held) = transport().await;
    let (_status, mut termination) = watch::channel(None);
    tokio::time::pause();
    fill_transport_to_pending(&mut sender)
        .await
        .expect("transport buffer must saturate to pending");
    let write = machine_send(
        &mut sender,
        Message::Binary(vec![0; 64 * 1024].into()),
        &mut termination,
    );
    tokio::pin!(write);
    assert!(futures_util::poll!(&mut write).is_pending());
    tokio::time::advance(Duration::from_secs(10)).await;
    assert!(write.await.is_err());
    drop(held);
}

#[test]
fn serialized_controls_and_frames_are_rejected_when_wire_allowance_is_exceeded() {
    let exact = Message::Text("x".repeat(CONTROL_SLOT_BYTES - WS_HEADER_BYTES).into());
    let over = Message::Text("x".repeat(CONTROL_SLOT_BYTES - WS_HEADER_BYTES + 1).into());
    let admitted = [
        machine_control(exact).is_ok(),
        machine_control(over).is_ok(),
        machine_frame(vec![0; MACHINE_FRAME_OVERHEAD - WS_HEADER_BYTES], 0).is_ok(),
        machine_frame(vec![0; MACHINE_FRAME_OVERHEAD - WS_HEADER_BYTES + 1], 0).is_ok(),
    ];
    assert_eq!(admitted, [true, false, true, false]);
}

#[tokio::test]
async fn two_consumer_tcp_stall_terminates_viewer_while_controller_progresses() {
    use crate::terminal::output_hub::TerminalOutputHub;

    let hub = TerminalOutputHub::new(10 * 1024 * 1024);
    let session_id = "writer-two-consumer-test";
    hub.register_session(session_id);

    let mut ctrl_attachment = hub.subscribe_machine(session_id, None).unwrap().unwrap();
    let (mut ctrl_sender, mut ctrl_client) = connected_websocket_pair().await;
    let mut ctrl_term = ctrl_attachment.receiver.termination();

    let mut view_attachment = hub.subscribe_machine(session_id, None).unwrap().unwrap();
    let (mut view_sender, held_view_peer) = transport().await;
    let mut view_term = view_attachment.receiver.termination();

    let (ctrl_tx, mut ctrl_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(32);
    let _ctrl_writer_guard = AbortOnDrop::new(tokio::spawn(async move {
        while let Ok(charged) = ctrl_attachment.receiver.recv().await {
            let chunk_bytes = charged.value.bytes.to_vec();
            let frame = machine_frame(chunk_bytes.clone(), chunk_bytes.len()).unwrap();
            if machine_send(&mut ctrl_sender, frame, &mut ctrl_term).await.is_err() {
                break;
            }
            if ctrl_tx.send(chunk_bytes).await.is_err() {
                break;
            }
        }
    }));

    let (ack_tx, mut ack_rx) = tokio::sync::mpsc::channel::<()>(32);
    let _ctrl_client_guard = AbortOnDrop::new(tokio::spawn(async move {
        while let Some(msg) = ctrl_client.next().await {
            if let Ok(Wire::Binary(_)) = msg {
                let _ = ack_tx.send(()).await;
            }
        }
    }));

    let view_writer_task = tokio::spawn(async move {
        while let Ok(charged) = view_attachment.receiver.recv().await {
            let chunk_bytes = charged.value.bytes.to_vec();
            let frame = machine_frame(chunk_bytes.clone(), chunk_bytes.len()).unwrap();
            if machine_send(&mut view_sender, frame, &mut view_term).await.is_err() {
                return Err("VIEWER_TERMINATED");
            }
        }
        Ok(())
    });
    let mut _view_writer_guard = AbortOnDrop::new(view_writer_task);

    let burst_payload = vec![0x33; 64 * 1024];
    for _ in 0..25 {
        hub.publish(session_id, burst_payload.clone());
        let _ = tokio::time::timeout(Duration::from_millis(500), ack_rx.recv()).await;
    }

    if let Some(handle) = _view_writer_guard.0.as_mut() {
        let view_res = tokio::time::timeout(Duration::from_secs(5), handle).await
            .expect("Viewer writer must terminate within deadline")
            .expect("Task join succeeded");
        assert_eq!(view_res, Err("VIEWER_TERMINATED"));
    }

    hub.publish(session_id, b"final_check".to_vec());
    let ack = tokio::time::timeout(Duration::from_secs(2), ack_rx.recv()).await;
    assert!(
        matches!(ack, Ok(Some(()))),
        "Controller must receive an output frame after viewer termination; got {ack:?}"
    );

    drop(held_view_peer);
}

#[tokio::test]
async fn two_consumer_stalled_viewer_reconnects_after_teardown() {
    use crate::terminal::output_hub::TerminalOutputHub;
    use crate::terminal::machine_output::MACHINE_OUTPUT_BYTES;

    let hub = TerminalOutputHub::new(10 * 1024 * 1024);
    let session_id = "writer-reconnect-test";
    hub.register_session(session_id);

    let c1 = hub.publish(session_id, b"msg1".to_vec()).unwrap();

    let viewer1 = hub.subscribe_machine(session_id, None).unwrap().unwrap();
    let budget1 = viewer1.receiver.budget.clone();
    drop(viewer1);
    assert_eq!(budget1.available_permits(), MACHINE_OUTPUT_BYTES);

    let c2 = hub.publish(session_id, b"msg2".to_vec()).unwrap();

    let mut reconnected = hub.subscribe_machine(session_id, Some(c1.sequence)).unwrap().unwrap();
    assert_eq!(reconnected.snapshot.value.history, b"msg2");
    assert_eq!(reconnected.snapshot.value.history_start_sequence, Some(c2.sequence));

    let c3 = hub.publish(session_id, b"msg3".to_vec()).unwrap();
    let live = reconnected.receiver.recv().await.unwrap();
    assert_eq!(live.value.sequence, c3.sequence);
    assert_eq!(live.value.bytes.as_ref(), b"msg3");
}

#[tokio::test]
async fn output_overflow_close_has_stable_code_and_reason() {
    let (mut sender, mut client) = connected_websocket_pair().await;

    send_machine_output_overflow_close(&mut sender).await;

    let close = client.next().await.unwrap().unwrap();
    let Wire::Close(Some(frame)) = close else {
        panic!("expected explicit overflow close frame");
    };
    assert_eq!(u16::from(frame.code), MACHINE_OUTPUT_OVERFLOW_CLOSE_CODE);
    assert_eq!(frame.reason, MACHINE_OUTPUT_OVERFLOW_CLOSE_REASON);
}

#[tokio::test]
async fn normal_disconnect_does_not_use_overflow_close_signal() {
    let (mut sender, mut client) = connected_websocket_pair().await;
    sender.send(Message::Close(None)).await.unwrap();
    sender.flush().await.unwrap();

    let close = client.next().await.unwrap().unwrap();
    let Wire::Close(frame) = close else {
        panic!("expected ordinary close frame");
    };
    assert!(!frame.is_some_and(|frame| {
        u16::from(frame.code) == MACHINE_OUTPUT_OVERFLOW_CLOSE_CODE
            && frame.reason == MACHINE_OUTPUT_OVERFLOW_CLOSE_REASON
    }));
}
