//! Unreliable QUIC datagram channels multiplexed over one direct connection.
//!
//! Wire frame: `[DATAGRAM_WIRE_VERSION][channel_id: u64 LE][payload]`. Malformed frames and
//! frames for unknown channels are dropped silently; they never close the QUIC connection.
//! Delivery to each channel is bounded and non-blocking, so a slow consumer loses its own
//! datagrams instead of stalling the connection's reader task.
use bytes::{BufMut, Bytes, BytesMut};
use parking_lot::Mutex;
use quinn::{Connection, SendDatagramError};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Weak};
use tokio::sync::mpsc::{self, error::TrySendError};
use tokio::sync::watch;
use tokio::task::JoinHandle;

pub const DATAGRAM_WIRE_VERSION: u8 = 0x01;
pub const DATAGRAM_HEADER_BYTES: usize = 9;
/// Per-channel queue depth; datagrams beyond it are dropped for that channel only.
pub const DATAGRAM_CHANNEL_CAPACITY: usize = 256;

pub fn encode_datagram_frame(channel_id: u64, payload: &[u8]) -> Bytes {
    let mut frame = BytesMut::with_capacity(DATAGRAM_HEADER_BYTES + payload.len());
    frame.put_u8(DATAGRAM_WIRE_VERSION);
    frame.put_u64_le(channel_id);
    frame.put_slice(payload);
    frame.freeze()
}

/// `None` for an empty, truncated or unknown-version frame. The payload is a zero-copy slice.
pub fn decode_datagram_frame(frame: Bytes) -> Option<(u64, Bytes)> {
    if frame.len() < DATAGRAM_HEADER_BYTES || frame[0] != DATAGRAM_WIRE_VERSION {
        return None;
    }
    let mut id = [0u8; 8];
    id.copy_from_slice(&frame[1..DATAGRAM_HEADER_BYTES]);
    Some((u64::from_le_bytes(id), frame.slice(DATAGRAM_HEADER_BYTES..)))
}

pub struct DatagramMux {
    channels: Mutex<HashMap<u64, mpsc::Sender<Bytes>>>,
}

/// Removes its channel from the mux on drop, unless the id was re-registered since.
pub struct DatagramChannelGuard {
    mux: Weak<DatagramMux>,
    channel_id: u64,
    sender: mpsc::WeakSender<Bytes>,
}

impl Drop for DatagramChannelGuard {
    fn drop(&mut self) {
        let Some(mux) = self.mux.upgrade() else {
            return;
        };
        let mut channels = mux.channels.lock();
        let ours = match (channels.get(&self.channel_id), self.sender.upgrade()) {
            (Some(current), Some(sender)) => current.same_channel(&sender),
            _ => false,
        };
        if ours {
            channels.remove(&self.channel_id);
        }
    }
}

impl DatagramMux {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            channels: Mutex::new(HashMap::new()),
        })
    }

    /// Registers (or replaces) `channel_id`. Keep the guard alive as long as the receiver.
    pub fn register(
        self: &Arc<Self>,
        channel_id: u64,
    ) -> (mpsc::Receiver<Bytes>, DatagramChannelGuard) {
        let (tx, rx) = mpsc::channel(DATAGRAM_CHANNEL_CAPACITY);
        let guard = DatagramChannelGuard {
            mux: Arc::downgrade(self),
            channel_id,
            sender: tx.downgrade(),
        };
        self.channels.lock().insert(channel_id, tx);
        (rx, guard)
    }

    pub fn remove(&self, channel_id: u64) {
        self.channels.lock().remove(&channel_id);
    }

    pub fn len(&self) -> usize {
        self.channels.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Drops every sender, so each registered receiver observes end-of-stream.
    pub fn clear(&self) {
        let drained: Vec<_> = self.channels.lock().drain().collect();
        drop(drained);
    }

    /// Never blocks: a full channel drops this datagram, a closed one is unregistered.
    fn dispatch(&self, channel_id: u64, payload: Bytes) {
        let mut channels = self.channels.lock();
        let Some(sender) = channels.get(&channel_id) else {
            return;
        };
        match sender.try_send(payload) {
            Ok(()) | Err(TrySendError::Full(_)) => {}
            Err(TrySendError::Closed(_)) => {
                channels.remove(&channel_id);
            }
        }
    }
}

/// Clears the mux when the reader ends for any reason, including an abort.
struct ClearOnDrop(Arc<DatagramMux>);

impl Drop for ClearOnDrop {
    fn drop(&mut self) {
        self.0.clear();
    }
}

pub fn spawn_datagram_reader(connection: Connection, mux: Arc<DatagramMux>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mux = ClearOnDrop(mux);
        loop {
            let frame = match connection.read_datagram().await {
                Ok(frame) => frame,
                Err(error) => {
                    tracing::debug!("direct datagram reader stopped: {error}");
                    break;
                }
            };
            if let Some((channel_id, payload)) = decode_datagram_frame(frame) {
                mux.0.dispatch(channel_id, payload);
            }
        }
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatagramSendOutcome {
    Sent,
    /// The datagram was NOT sent; the caller must use its reliable path instead.
    FallbackNeeded,
}

pub fn send_datagram_checked(
    connection: &Connection,
    channel_id: u64,
    payload: &[u8],
) -> DatagramSendOutcome {
    let fits = connection
        .max_datagram_size()
        .is_some_and(|max| payload.len() + DATAGRAM_HEADER_BYTES <= max);
    if !fits {
        return DatagramSendOutcome::FallbackNeeded;
    }
    match connection.send_datagram(encode_datagram_frame(channel_id, payload)) {
        Ok(()) => DatagramSendOutcome::Sent,
        Err(
            SendDatagramError::TooLarge
            | SendDatagramError::UnsupportedByPeer
            | SendDatagramError::Disabled,
        ) => DatagramSendOutcome::FallbackNeeded,
        Err(error) => {
            tracing::debug!("direct datagram send failed: {error}");
            DatagramSendOutcome::FallbackNeeded
        }
    }
}

#[derive(Clone)]
pub struct DirectDatagramPeer {
    pub connection: Connection,
    pub mux: Arc<DatagramMux>,
}

/// Host side: maps the bridge's loopback TCP source address (what the local gateway sees as
/// the peer) to the direct QUIC connection carrying that request.
#[derive(Default)]
pub struct LoopbackPeerRegistry {
    peers: Mutex<HashMap<SocketAddr, DirectDatagramPeer>>,
    len_tx: watch::Sender<usize>,
}

/// Removes its loopback address from the registry on drop.
pub struct LoopbackPeerGuard {
    registry: Weak<LoopbackPeerRegistry>,
    local_addr: SocketAddr,
}

impl Drop for LoopbackPeerGuard {
    fn drop(&mut self) {
        if let Some(registry) = self.registry.upgrade() {
            let mut peers = registry.peers.lock();
            peers.remove(&self.local_addr);
            registry.len_tx.send_replace(peers.len());
        }
    }
}

impl LoopbackPeerRegistry {
    pub fn register(
        self: &Arc<Self>,
        local_addr: SocketAddr,
        peer: DirectDatagramPeer,
    ) -> LoopbackPeerGuard {
        let mut peers = self.peers.lock();
        peers.insert(local_addr, peer);
        self.len_tx.send_replace(peers.len());
        LoopbackPeerGuard {
            registry: Arc::downgrade(self),
            local_addr,
        }
    }

    pub fn lookup(&self, peer_addr: &SocketAddr) -> Option<DirectDatagramPeer> {
        self.peers.lock().get(peer_addr).cloned()
    }

    pub fn len(&self) -> usize {
        self.peers.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Observes the entry count; lets callers await a registration or removal.
    pub fn subscribe_len(&self) -> watch::Receiver<usize> {
        self.len_tx.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paired_host::direct::{
        bridge_quic_connection_to_tcp_with_registry, create_quic_endpoint, generate_ephemeral_cert,
    };
    use crate::paired_host::direct_route::{DirectGuard, DirectRoutes};
    use quinn::Endpoint;
    use std::time::{Duration, Instant};
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;

    const BOUND: Duration = Duration::from_secs(10);

    struct Pair {
        server_endpoint: Endpoint,
        client_endpoint: Endpoint,
        server: Connection,
        client: Connection,
    }

    impl Drop for Pair {
        fn drop(&mut self) {
            self.server_endpoint.close(0u32.into(), b"done");
            self.client_endpoint.close(0u32.into(), b"done");
        }
    }

    async fn connected_pair() -> Pair {
        let server_sock = std::net::UdpSocket::bind("127.0.0.1:0").expect("bind server udp");
        let client_sock = std::net::UdpSocket::bind("127.0.0.1:0").expect("bind client udp");
        let server_addr = server_sock.local_addr().expect("server addr");
        let server_cert = generate_ephemeral_cert().expect("server cert");
        let client_cert = generate_ephemeral_cert().expect("client cert");
        let server_endpoint =
            create_quic_endpoint(server_sock, &server_cert, &client_cert.cert_der, true)
                .expect("server endpoint");
        let client_endpoint =
            create_quic_endpoint(client_sock, &client_cert, &server_cert.cert_der, false)
                .expect("client endpoint");
        let connect = async {
            client_endpoint
                .connect(server_addr, "ferryx-direct")
                .map_err(|e| e.to_string())?
                .await
                .map_err(|e| e.to_string())
        };
        let accept = async {
            let incoming = server_endpoint
                .accept()
                .await
                .ok_or_else(|| "server endpoint closed".to_string())?;
            incoming.await.map_err(|e| e.to_string())
        };
        let (client, server) = tokio::try_join!(connect, accept).expect("quic handshake");
        Pair {
            server_endpoint,
            client_endpoint,
            server,
            client,
        }
    }

    #[test]
    fn frame_round_trips() {
        let frame = encode_datagram_frame(0x0102_0304_0506_0708, b"pixels");
        assert_eq!(frame.len(), DATAGRAM_HEADER_BYTES + 6);
        assert_eq!(frame[0], DATAGRAM_WIRE_VERSION);
        assert_eq!(&frame[1..9], &0x0102_0304_0506_0708u64.to_le_bytes());
        let (id, payload) = decode_datagram_frame(frame).expect("decodes");
        assert_eq!(id, 0x0102_0304_0506_0708);
        assert_eq!(&payload[..], b"pixels");

        let (id, payload) = decode_datagram_frame(encode_datagram_frame(u64::MAX, &[]))
            .expect("empty payload is a valid frame");
        assert_eq!(id, u64::MAX);
        assert!(payload.is_empty());
    }

    #[test]
    fn malformed_frames_are_rejected() {
        assert_eq!(decode_datagram_frame(Bytes::new()), None);
        assert_eq!(decode_datagram_frame(Bytes::from_static(&[DATAGRAM_WIRE_VERSION])), None);
        assert_eq!(
            decode_datagram_frame(Bytes::from_static(&[DATAGRAM_WIRE_VERSION, 1, 2, 3, 4, 5, 6, 7])),
            None
        );
        let mut bad_version = encode_datagram_frame(7, b"x").to_vec();
        bad_version[0] = 0x02;
        assert_eq!(decode_datagram_frame(Bytes::from(bad_version)), None);
    }

    #[tokio::test]
    async fn mux_guard_removes_only_its_own_registration() {
        let mux = DatagramMux::new();
        let (mut first_rx, first_guard) = mux.register(5);
        let (mut second_rx, second_guard) = mux.register(5);
        assert_eq!(mux.len(), 1);
        // Replacing the channel closes the first receiver.
        assert_eq!(first_rx.recv().await, None);

        drop(first_guard);
        assert_eq!(mux.len(), 1, "a stale guard must not remove the replacement");
        mux.dispatch(5, Bytes::from_static(b"live"));
        assert_eq!(second_rx.recv().await.as_deref(), Some(&b"live"[..]));

        drop(second_guard);
        assert!(mux.is_empty());
    }

    #[tokio::test]
    async fn mux_full_channel_drops_without_blocking_and_closed_channel_unregisters() {
        let mux = DatagramMux::new();
        let (mut rx, _guard) = mux.register(1);
        for i in 0..(DATAGRAM_CHANNEL_CAPACITY + 44) {
            mux.dispatch(1, Bytes::from((i as u32).to_le_bytes().to_vec()));
        }
        let mut received = 0;
        while rx.try_recv().is_ok() {
            received += 1;
        }
        assert_eq!(received, DATAGRAM_CHANNEL_CAPACITY);

        drop(rx);
        mux.dispatch(1, Bytes::from_static(b"nobody"));
        assert!(mux.is_empty(), "a closed receiver is unregistered on dispatch");

        let (mut other_rx, _other_guard) = mux.register(2);
        mux.remove(2);
        assert_eq!(other_rx.recv().await, None);
        let (mut cleared_rx, _cleared_guard) = mux.register(3);
        mux.clear();
        assert!(mux.is_empty());
        assert_eq!(cleared_rx.recv().await, None);
    }

    #[tokio::test]
    async fn loopback_multiplexed_channels_have_no_cross_talk() {
        tokio::time::timeout(BOUND, async {
            let pair = connected_pair().await;
            let mux = DatagramMux::new();
            let reader = spawn_datagram_reader(pair.server.clone(), Arc::clone(&mux));
            let (mut rx_a, _guard_a) = mux.register(0xA);
            let (mut rx_b, _guard_b) = mux.register(0xB);

            let max = pair.client.max_datagram_size().expect("peer accepts datagrams");
            assert!(max >= 1120 + DATAGRAM_HEADER_BYTES, "max datagram size {max}");

            // Lockstep batches keep in-flight datagrams small, so the test never relies on a
            // 200-packet burst surviving loopback socket buffers.
            let mut seqs_a = Vec::new();
            let mut seqs_b = Vec::new();
            for batch in 0..20u32 {
                for seq in batch * 10..batch * 10 + 10 {
                    let channel = if seq % 2 == 0 { 0xA } else { 0xB };
                    let mut payload = vec![channel as u8; 1120];
                    payload[..4].copy_from_slice(&seq.to_le_bytes());
                    assert_eq!(
                        send_datagram_checked(&pair.client, channel, &payload),
                        DatagramSendOutcome::Sent
                    );
                }
                for (rx, channel, seqs) in [
                    (&mut rx_a, 0xAu8, &mut seqs_a),
                    (&mut rx_b, 0xBu8, &mut seqs_b),
                ] {
                    for _ in 0..5 {
                        let payload = rx.recv().await.expect("channel open");
                        assert_eq!(payload.len(), 1120);
                        assert!(payload[4..].iter().all(|b| *b == channel), "cross-talk on {channel:#x}");
                        let seq = u32::from_le_bytes(payload[..4].try_into().expect("4 bytes"));
                        assert_eq!(seq % 2 == 0, channel == 0xA);
                        seqs.push(seq);
                    }
                }
            }
            for seqs in [&mut seqs_a, &mut seqs_b] {
                seqs.sort_unstable();
                seqs.dedup();
                assert_eq!(seqs.len(), 100);
            }
            assert!(rx_a.try_recv().is_err() && rx_b.try_recv().is_err());
            reader.abort();
        })
        .await
        .expect("test bounded");
    }

    #[tokio::test]
    async fn unknown_channel_and_malformed_frames_are_dropped_without_closing() {
        tokio::time::timeout(BOUND, async {
            let pair = connected_pair().await;
            let mux = DatagramMux::new();
            let reader = spawn_datagram_reader(pair.server.clone(), Arc::clone(&mux));
            let (mut rx, _guard) = mux.register(1);

            assert_eq!(
                send_datagram_checked(&pair.client, 999, b"unknown"),
                DatagramSendOutcome::Sent
            );
            pair.client
                .send_datagram(Bytes::from_static(&[0x7f, 1, 2]))
                .expect("send malformed");
            assert_eq!(
                send_datagram_checked(&pair.client, 1, b"known"),
                DatagramSendOutcome::Sent
            );

            assert_eq!(rx.recv().await.as_deref(), Some(&b"known"[..]));
            assert!(pair.server.close_reason().is_none());
            assert!(pair.client.close_reason().is_none());
            assert!(!reader.is_finished());
            assert_eq!(mux.len(), 1);
            reader.abort();
        })
        .await
        .expect("test bounded");
    }

    #[tokio::test]
    async fn oversized_payload_needs_fallback() {
        tokio::time::timeout(BOUND, async {
            let pair = connected_pair().await;
            assert_eq!(
                send_datagram_checked(&pair.client, 1, &[0u8; 4096]),
                DatagramSendOutcome::FallbackNeeded
            );
            assert!(pair.client.close_reason().is_none());
        })
        .await
        .expect("test bounded");
    }

    #[tokio::test]
    async fn reader_exit_clears_mux() {
        tokio::time::timeout(BOUND, async {
            let pair = connected_pair().await;
            let mux = DatagramMux::new();
            let reader = spawn_datagram_reader(pair.server.clone(), Arc::clone(&mux));
            let (mut rx, _guard) = mux.register(1);
            pair.client.close(0u32.into(), b"bye");
            reader.await.expect("reader ends on close");
            assert!(mux.is_empty());
            assert_eq!(rx.recv().await, None);
        })
        .await
        .expect("test bounded");
    }

    #[tokio::test]
    async fn bridge_registers_loopback_peer_until_stream_closes() {
        tokio::time::timeout(BOUND, async {
            let pair = connected_pair().await;
            let echo = TcpListener::bind("127.0.0.1:0").await.expect("bind echo");
            let echo_addr = echo.local_addr().expect("echo addr");
            let registry = Arc::new(LoopbackPeerRegistry::default());
            let mut registry_len = registry.subscribe_len();
            let mux = DatagramMux::new();
            let bridge = bridge_quic_connection_to_tcp_with_registry(
                pair.server.clone(),
                echo_addr,
                Some((Arc::clone(&registry), Arc::clone(&mux))),
            );

            let (mut send, mut recv) = pair.client.open_bi().await.expect("open bi");
            send.write_all(b"ping").await.expect("write");
            let (mut tcp, peer_addr) = echo.accept().await.expect("bridge dials gateway");
            let mut buf = [0u8; 4];
            tokio::io::AsyncReadExt::read_exact(&mut tcp, &mut buf).await.expect("gateway read");
            assert_eq!(&buf, b"ping");
            // The first byte reached the gateway, so registration (which precedes pumping) is done.
            let peer = registry.lookup(&peer_addr).expect("loopback peer registered");
            assert_eq!(registry.len(), 1);
            assert_eq!(peer.connection.stable_id(), pair.server.stable_id());
            assert!(Arc::ptr_eq(&peer.mux, &mux));
            drop(peer);

            tcp.write_all(b"pong").await.expect("gateway write");
            tcp.shutdown().await.expect("gateway half close");
            send.finish().expect("finish");
            let echoed = recv.read_to_end(16).await.expect("read reply");
            assert_eq!(echoed, b"pong");

            registry_len
                .wait_for(|n| *n == 0)
                .await
                .expect("registry alive");
            assert!(registry.lookup(&peer_addr).is_none());
            bridge.abort();
        })
        .await
        .expect("test bounded");
    }

    #[tokio::test]
    async fn direct_guard_drop_aborts_reader_and_clears_channels() {
        tokio::time::timeout(BOUND, async {
            let pair = connected_pair().await;
            let mux = DatagramMux::new();
            let reader = spawn_datagram_reader(pair.client.clone(), Arc::clone(&mux));
            let mut guard = DirectGuard::new(pair.client.clone(), pair.client_endpoint.clone())
                .with_datagram_mux(Arc::clone(&mux));
            guard.track(reader.abort_handle());
            let (mut rx, _channel) = mux.register(9);

            let routes = DirectRoutes::new();
            let epoch = routes.begin_attempt("h", Instant::now()).expect("attempt");
            assert!(routes.datagram_handle("h", epoch).is_none(), "not direct yet");
            assert!(routes.publish("h", epoch, "http://127.0.0.1:1".into(), guard));
            let handle = routes.datagram_handle("h", epoch).expect("direct route has datagrams");
            assert!(Arc::ptr_eq(&handle.mux, &mux));
            assert_eq!(handle.connection.stable_id(), pair.client.stable_id());
            assert!(routes.datagram_handle("h", epoch + 1).is_none(), "stale epoch");
            drop(handle);

            routes.reset("h");
            assert!(reader.await.expect_err("reader aborted").is_cancelled());
            assert!(mux.is_empty());
            assert_eq!(rx.recv().await, None);
            assert!(routes.datagram_handle("h", epoch).is_none());
        })
        .await
        .expect("test bounded");
    }

    #[test]
    fn default_guard_has_no_datagram_peer() {
        assert!(DirectGuard::default().datagram_peer().is_none());
    }
}
