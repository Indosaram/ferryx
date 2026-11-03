use super::*;
use std::sync::{mpsc, Mutex};

const WATCHDOG: Duration = Duration::from_secs(5);

#[derive(Debug, Default)]
struct Gate {
    remaining: Option<usize>,
    written: usize,
    blocked: usize,
    fail_write: Option<io::ErrorKind>,
    block_flush: bool,
}

#[derive(Debug)]
struct GatedTcp {
    tcp: TcpStream,
    gate: Arc<Mutex<Gate>>,
}

impl Read for GatedTcp {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.tcp.read(bytes)
    }
}

impl Write for GatedTcp {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut gate = self.gate.lock().unwrap();
        if let Some(error) = gate.fail_write.take() {
            return Err(error.into());
        }
        if gate.remaining == Some(0) {
            gate.blocked += 1;
            return Err(io::ErrorKind::WouldBlock.into());
        }
        let limit = gate
            .remaining
            .map_or(bytes.len(), |n| n.min(7).min(bytes.len()));
        let count = self.tcp.write(&bytes[..limit])?;
        gate.written += count;
        if let Some(remaining) = &mut gate.remaining {
            *remaining -= count;
        }
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.gate.lock().unwrap().block_flush {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        self.tcp.flush()
    }
}

fn pair() -> (
    TlsPskStream<GatedTcp>,
    TlsPskStream<TcpStream>,
    Arc<Mutex<Gate>>,
) {
    let psk = PskIdentity::pairing("write-pressure", &[23; 32]).unwrap();
    let listener = TlsPskServer::new([psk.clone()])
        .unwrap()
        .bind("127.0.0.1:0")
        .unwrap();
    let tcp = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    tcp.set_read_timeout(Some(WATCHDOG)).unwrap();
    tcp.set_write_timeout(Some(WATCHDOG)).unwrap();
    let host = std::thread::spawn(move || {
        let stream = listener.accept().unwrap();
        stream
            .ssl_stream()
            .get_ref()
            .set_read_timeout(Some(WATCHDOG))
            .unwrap();
        stream
    });
    let gate = Arc::new(Mutex::new(Gate::default()));
    let client = TlsPskClient::new(psk)
        .unwrap()
        .connect_stream(GatedTcp {
            tcp,
            gate: gate.clone(),
        })
        .unwrap();
    (client, host.join().unwrap(), gate)
}

#[test]
fn partial_ciphertext_retry_preserves_frames() {
    let (mut client, mut server, gate) = pair();
    let first = (0..48 * 1024).map(|i| (i % 251) as u8).collect::<Vec<_>>();
    let second = b"second distinct frame";
    let (done_tx, done_rx) = mpsc::channel();
    let host = std::thread::spawn(move || {
        let first = server.read_frame();
        let second = server.read_frame();
        done_tx.send((first, second)).unwrap();
    });
    {
        let mut gate = gate.lock().unwrap();
        gate.remaining = Some(17);
        gate.written = 0;
    }
    let accepted_first = client.write_frame(&first);
    {
        let mut gate = gate.lock().unwrap();
        assert_eq!(
            gate.written, 17,
            "exact partial TLS record, not timing pressure"
        );
        assert!(gate.blocked > 0);
        gate.remaining = None;
    }
    let accepted_second = client.write_frame(second);
    drop(client);
    let observed = done_rx.recv_timeout(WATCHDOG).unwrap();
    host.join().unwrap();
    assert!(
        matches!(accepted_first, Err(TlsPskError::Io(ref e)) if e.kind() == io::ErrorKind::WouldBlock)
            && accepted_second.is_ok(),
        "accepted first={accepted_first:?}, second={accepted_second:?}; peer={observed:?}"
    );
    assert_eq!(observed.0.unwrap(), first);
    assert_eq!(observed.1.unwrap(), second);
}

#[test]
fn acknowledged_plaintext_and_retry_allocation_survive_would_block() {
    let (mut client, mut server, gate) = pair();
    let payload = vec![42; 48 * 1024];
    // TLS 1.2 AES-GCM: 5-byte record header + 8-byte explicit nonce +
    // 16KiB plaintext + 16-byte tag, followed by 17 bytes of the next record.
    gate.lock().unwrap().remaining = Some(TLS_WRITE_CHUNK + 29 + 17);
    client.queue_frame(&payload).unwrap();
    let allocation = client.outbound[0].bytes.as_ptr();
    client.write_frame_step().unwrap();
    assert_eq!(client.outbound[0].offset, TLS_WRITE_CHUNK);
    assert!(
        matches!(client.write_frame_step(), Err(TlsPskError::Io(e)) if e.kind() == io::ErrorKind::WouldBlock)
    );
    client.queue_frame(b"tail").unwrap();
    assert_eq!(client.outbound[0].bytes.as_ptr(), allocation);
    assert_eq!(client.outbound[0].offset, TLS_WRITE_CHUNK);
    // A second unsuccessful retry must not advance or replace the SSL buffer.
    assert!(
        matches!(client.write_frame_step(), Err(TlsPskError::Io(e)) if e.kind() == io::ErrorKind::WouldBlock)
    );
    assert_eq!(client.outbound[0].bytes.as_ptr(), allocation);
    assert_eq!(client.outbound[0].offset, TLS_WRITE_CHUNK);
    gate.lock().unwrap().remaining = None;
    client.flush_pending_frames().unwrap();
    assert_eq!(server.read_frame().unwrap(), payload);
    assert_eq!(server.read_frame().unwrap(), b"tail");
    assert_eq!(client.outbound_bytes, 0);
}

#[test]
fn queue_bounds_reject_without_admission_and_recover_after_drain() {
    let (mut client, mut server, _) = pair();
    for i in 0..MAX_OUTBOUND_FRAMES {
        client.queue_frame(&[i as u8]).unwrap();
    }
    assert!(
        matches!(client.queue_frame(b"rejected"), Err(TlsPskError::Io(e)) if e.kind() == io::ErrorKind::WouldBlock)
    );
    assert_eq!(client.outbound.len(), MAX_OUTBOUND_FRAMES);
    client.flush_pending_frames().unwrap();
    for i in 0..MAX_OUTBOUND_FRAMES {
        assert_eq!(server.read_frame().unwrap(), [i as u8]);
    }
    client.write_frame(b"after rejection").unwrap();
    assert_eq!(server.read_frame().unwrap(), b"after rejection");
    client
        .queue_frame(&vec![1; ferryx_rd_proto::MAX_TCP_FRAME_SIZE])
        .unwrap();
    assert_eq!(client.outbound_bytes, MAX_OUTBOUND_BYTES);
    assert!(
        matches!(client.queue_frame(b"byte limit"), Err(TlsPskError::Io(e)) if e.kind() == io::ErrorKind::WouldBlock)
    );
    assert_eq!(client.outbound.len(), 1);
}

#[test]
fn flush_would_block_does_not_replay_plaintext() {
    let (mut client, mut server, gate) = pair();
    gate.lock().unwrap().block_flush = true;
    assert!(
        matches!(client.write_frame(b"once"), Err(TlsPskError::Io(e)) if e.kind() == io::ErrorKind::WouldBlock)
    );
    assert_eq!(client.outbound[0].offset, client.outbound[0].bytes.len());
    let written = gate.lock().unwrap().written;
    assert!(
        matches!(client.flush_pending_frames(), Err(TlsPskError::Io(e)) if e.kind() == io::ErrorKind::WouldBlock)
    );
    assert_eq!(gate.lock().unwrap().written, written);
    gate.lock().unwrap().block_flush = false;
    client.write_frame(b"twice").unwrap();
    assert_eq!(server.read_frame().unwrap(), b"once");
    assert_eq!(server.read_frame().unwrap(), b"twice");
}

#[test]
fn terminal_error_cancels_queue_and_forbids_new_ciphertext() {
    let (mut client, _server, gate) = pair();
    client.queue_frame(b"first").unwrap();
    client.queue_frame(b"second").unwrap();
    gate.lock().unwrap().fail_write = Some(io::ErrorKind::ConnectionReset);
    assert!(client.write_frame_step().is_err());
    let written = gate.lock().unwrap().written;
    assert!(!client.has_pending_frames());
    assert_eq!(client.outbound_bytes, 0);
    assert!(
        matches!(client.write_frame(b"must not send"), Err(TlsPskError::Io(e)) if e.kind() == io::ErrorKind::BrokenPipe)
    );
    assert!(client.flush_pending_frames().is_err());
    assert_eq!(gate.lock().unwrap().written, written);
}
