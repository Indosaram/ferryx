use super::*;

#[test]
fn receive_timestamp_is_preserved() {
    let queue = FrameQueue::new();
    let received = Instant::now();
    let frame = ferryx_rd_app::AssembledFrame {
        header: ferryx_rd_proto::FrameHeader {
            frame_id: 0,
            width: 2,
            height: 2,
            is_key_frame: true,
            total_chunks: 1,
            total_size: 1,
        },
        data: vec![1],
        timestamp_ms: 123,
    };
    queue.push((frame.clone(), received)).unwrap();
    assert_eq!(
        queue.recv_timeout(Duration::ZERO).unwrap(),
        (frame, received)
    );
}

#[test]
fn stop_disconnects_empty_consumer() {
    let queue = Arc::new(FrameQueue::new());
    let consumer = queue.clone();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    // Stop-before-receive deterministically characterizes disconnected reads.
    // The shared queue's private test separately proves registered-waiter wake.
    queue.stop().unwrap();
    let worker = std::thread::spawn(move || {
        entered_tx.send(()).unwrap();
        done_tx
            .send(consumer.recv_timeout(Duration::from_secs(30)))
            .unwrap();
    });
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let result = done_rx.recv_timeout(Duration::from_secs(2));
    worker.join().unwrap();
    assert_eq!(result.unwrap(), Err(mpsc::RecvTimeoutError::Disconnected));
}
