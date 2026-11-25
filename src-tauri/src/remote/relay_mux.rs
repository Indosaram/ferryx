//! Multiplexes many gateway streams over one Noise-encrypted `SecureStream`.
//!
//! Both relay opaque sessions (`remote/relay_client.rs`) and direct/SSH-forwarded attach sessions
//! (`remote/attach_router.rs`) pass their completed `SecureStream` through
//! `session_transport::serve_secure_gateway`. If the first decrypted frame equals
//! [`RELAY_MUX_HELLO`], the daemon answers [`RELAY_MUX_HELLO_OK`] and runs [`serve_mux`], opening
//! a fresh loopback TCP connection to the local gateway for each mux channel. Any other first
//! frame is treated as a legacy single-stream HTTP/1.1 connection and proxied unchanged.
//!
//! Wire format inside each decrypted Noise message:
//!
//! ```text
//! u32 BE channel_id | u8 kind | payload
//! ```
//!
//! Channel 0 carries `Ping` / `Pong` keepalives; channels >= 1 carry independent bidirectional
//! byte streams (`Open`, `Data`, `Close`). Outbound `Data` payloads are chunked to
//! [`RELAY_MUX_SEGMENT`] (32 KiB, well under `MAX_ATTACH_FRAME = 65519`) and served round-robin
//! across active channels so one busy terminal cannot stall another pane's input or HTTP RPC.

use crate::remote::attach_crypto::{
    read_frame_or_end, write_frame, AttachError, SecureStream, MAX_ATTACH_FRAME,
};
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::sync::{mpsc, Notify};

pub const RELAY_MUX_HELLO: &[u8] = b"FXMUX1\r\n";
pub const RELAY_MUX_HELLO_OK: &[u8] = b"FXMUX1OK\r\n";
pub const RELAY_MUX_SEGMENT: usize = 32 * 1024;
pub const RELAY_MUX_MAX_CHANNELS: usize = 1024;
pub const RELAY_MUX_IDLE_TIMEOUT: Duration = Duration::from_secs(120);
pub const RELAY_MUX_PING_INTERVAL: Duration = Duration::from_secs(15);
pub const RELAY_MUX_SILENCE_LIMIT: Duration = Duration::from_secs(45);
const CHANNEL_INBOUND_SLOTS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MuxKind {
    Open = 1,
    Data = 2,
    Close = 3,
    Ping = 4,
    Pong = 5,
}

impl MuxKind {
    fn from_u8(value: u8) -> Option<Self> {
        Some(match value {
            1 => Self::Open,
            2 => Self::Data,
            3 => Self::Close,
            4 => Self::Ping,
            5 => Self::Pong,
            _ => return None,
        })
    }
}

pub fn encode_mux_frame(channel: u32, kind: MuxKind, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(5 + payload.len());
    out.extend_from_slice(&channel.to_be_bytes());
    out.push(kind as u8);
    out.extend_from_slice(payload);
    out
}

pub fn decode_mux_frame(bytes: &[u8]) -> Option<(u32, MuxKind, &[u8])> {
    if bytes.len() < 5 {
        return None;
    }
    let channel = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let kind = MuxKind::from_u8(bytes[4])?;
    Some((channel, kind, &bytes[5..]))
}

#[derive(Debug)]
pub enum MuxHandshakeError {
    /// Older daemon forwarded `FXMUX1` to HTTP/1.1 instead of acknowledging it; fall back to legacy.
    Unsupported,
    Transport(AttachError),
}

struct FairQueueState {
    control: VecDeque<Vec<u8>>,
    channels: HashMap<u32, VecDeque<Vec<u8>>>,
    order: VecDeque<u32>,
    dead: bool,
}

struct FairQueue {
    state: Mutex<FairQueueState>,
    notify: Notify,
}

impl FairQueue {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(FairQueueState {
                control: VecDeque::new(),
                channels: HashMap::new(),
                order: VecDeque::new(),
                dead: false,
            }),
            notify: Notify::new(),
        })
    }

    fn push_control(&self, frame: Vec<u8>) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.dead {
            return false;
        }
        state.control.push_back(frame);
        drop(state);
        self.notify.notify_one();
        true
    }

    fn push_channel_frame(&self, channel: u32, frame: Vec<u8>) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.dead {
            return false;
        }
        let existed = state.channels.contains_key(&channel);
        state.channels.entry(channel).or_default().push_back(frame);
        if !existed {
            state.order.push_back(channel);
        }
        drop(state);
        self.notify.notify_one();
        true
    }

    fn push_data(&self, channel: u32, payload: &[u8]) -> bool {
        if payload.is_empty() {
            return true;
        }
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.dead {
            return false;
        }
        let existed = state.channels.contains_key(&channel);
        let queue = state.channels.entry(channel).or_default();
        for chunk in payload.chunks(RELAY_MUX_SEGMENT) {
            queue.push_back(encode_mux_frame(channel, MuxKind::Data, chunk));
        }
        if !existed {
            state.order.push_back(channel);
        }
        drop(state);
        self.notify.notify_one();
        true
    }

    fn mark_dead(&self) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state.dead = true;
        state.control.clear();
        state.channels.clear();
        state.order.clear();
        drop(state);
        self.notify.notify_waiters();
    }

    async fn pop_next(&self) -> Option<Vec<u8>> {
        loop {
            {
                let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
                if let Some(frame) = state.control.pop_front() {
                    return Some(frame);
                }
                while let Some(channel) = state.order.pop_front() {
                    let next = state
                        .channels
                        .get_mut(&channel)
                        .and_then(|queue| queue.pop_front());
                    let has_more = state
                        .channels
                        .get(&channel)
                        .is_some_and(|queue| !queue.is_empty());
                    if has_more {
                        state.order.push_back(channel);
                    } else {
                        state.channels.remove(&channel);
                    }
                    if let Some(frame) = next {
                        return Some(frame);
                    }
                }
                if state.dead {
                    return None;
                }
            }
            self.notify.notified().await;
        }
    }
}

async fn run_writer<W: AsyncWrite + Unpin>(
    mut writer: W,
    transport: Arc<Mutex<snow::TransportState>>,
    queue: Arc<FairQueue>,
) {
    while let Some(plaintext) = queue.pop_next().await {
        if plaintext.len() > MAX_ATTACH_FRAME {
            queue.mark_dead();
            return;
        }
        let encrypted = {
            let mut cipher = vec![0u8; plaintext.len() + 16];
            let mut state = transport.lock().unwrap_or_else(|p| p.into_inner());
            match state.write_message(&plaintext, &mut cipher) {
                Ok(written) => {
                    cipher.truncate(written);
                    cipher
                }
                Err(_) => {
                    queue.mark_dead();
                    return;
                }
            }
        };
        if write_frame(&mut writer, &encrypted).await.is_err() {
            queue.mark_dead();
            return;
        }
    }
}

struct AbortOnDrop(tokio::task::AbortHandle);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub async fn serve_mux<S, G, F, Fut>(
    secure: SecureStream<S>,
    connect_gateway: F,
    idle_timeout: Duration,
) -> Result<(), AttachError>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    G: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = io::Result<G>> + Send + 'static,
{
    let (mut stream_rx, stream_tx) = tokio::io::split(secure.inner);
    let transport = Arc::new(Mutex::new(secure.transport));
    let queue = FairQueue::new();
    let writer_task = tokio::spawn(run_writer(
        stream_tx,
        Arc::clone(&transport),
        Arc::clone(&queue),
    ));
    let _writer_abort = AbortOnDrop(writer_task.abort_handle());

    let channels: Arc<Mutex<HashMap<u32, mpsc::Sender<Vec<u8>>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let connect_gateway = Arc::new(connect_gateway);

    let read_result = loop {
        let raw = match tokio::time::timeout(
            idle_timeout,
            read_frame_or_end(&mut stream_rx, MAX_ATTACH_FRAME + 16),
        )
        .await
        {
            Ok(Ok(Some(frame))) => frame,
            Ok(Ok(None)) => break Ok(()),
            Ok(Err(error)) => break Err(error),
            Err(_) => break Ok(()),
        };
        let plaintext = {
            let mut buf = vec![0u8; raw.len().max(1)];
            let mut state = transport.lock().unwrap_or_else(|p| p.into_inner());
            match state.read_message(&raw, &mut buf) {
                Ok(n) => {
                    buf.truncate(n);
                    buf
                }
                Err(error) => break Err(AttachError::DecryptFailed(error.to_string())),
            }
        };
        let Some((channel, kind, payload)) = decode_mux_frame(&plaintext) else {
            break Err(AttachError::DecryptFailed("malformed mux frame".into()));
        };
        match kind {
            MuxKind::Ping => {
                queue.push_control(encode_mux_frame(0, MuxKind::Pong, payload));
            }
            MuxKind::Pong => {}
            MuxKind::Open => {
                if channel == 0 {
                    continue;
                }
                let (tx, mut rx) = mpsc::channel::<Vec<u8>>(CHANNEL_INBOUND_SLOTS);
                let admitted = {
                    let mut map = channels.lock().unwrap_or_else(|p| p.into_inner());
                    if map.len() >= RELAY_MUX_MAX_CHANNELS || map.contains_key(&channel) {
                        false
                    } else {
                        map.insert(channel, tx);
                        true
                    }
                };
                if !admitted {
                    queue.push_channel_frame(channel, encode_mux_frame(channel, MuxKind::Close, &[]));
                    continue;
                }
                let connect = Arc::clone(&connect_gateway);
                let queue = Arc::clone(&queue);
                let channels = Arc::clone(&channels);
                tokio::spawn(async move {
                    let Ok(gateway) = connect().await else {
                        channels
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .remove(&channel);
                        queue.push_channel_frame(channel, encode_mux_frame(channel, MuxKind::Close, &[]));
                        return;
                    };
                    let (mut gw_rx, mut gw_tx) = tokio::io::split(gateway);
                    let to_gateway = tokio::spawn(async move {
                        while let Some(bytes) = rx.recv().await {
                            if gw_tx.write_all(&bytes).await.is_err() {
                                break;
                            }
                        }
                        let _ = gw_tx.shutdown().await;
                    });
                    let mut buf = vec![0u8; RELAY_MUX_SEGMENT];
                    loop {
                        match gw_rx.read(&mut buf).await {
                            Ok(0) | Err(_) => break,
                            Ok(n) => {
                                if !queue.push_data(channel, &buf[..n]) {
                                    break;
                                }
                            }
                        }
                    }
                    to_gateway.abort();
                    channels
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .remove(&channel);
                    queue.push_channel_frame(channel, encode_mux_frame(channel, MuxKind::Close, &[]));
                });
            }
            MuxKind::Data => {
                let sender = channels
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .get(&channel)
                    .cloned();
                if let Some(sender) = sender {
                    if sender.send(payload.to_vec()).await.is_err() {
                        channels
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .remove(&channel);
                    }
                }
            }
            MuxKind::Close => {
                // Dropping the sender lets `to_gateway` flush and half-close `gw_tx` cleanly.
                channels
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .remove(&channel);
            }
        }
    };

    channels.lock().unwrap_or_else(|p| p.into_inner()).clear();
    queue.mark_dead();
    let _ = writer_task.await;
    read_result
}

struct ClientChannels {
    inbound: HashMap<u32, mpsc::UnboundedSender<Vec<u8>>>,
    dead: bool,
    last_inbound: Instant,
}

pub struct RelayMuxClient {
    queue: Arc<FairQueue>,
    channels: Mutex<ClientChannels>,
    next_channel: AtomicU32,
}

impl RelayMuxClient {
    pub async fn handshake<S>(
        mut secure: SecureStream<S>,
        ping_interval: Duration,
        silence_limit: Duration,
    ) -> Result<Arc<Self>, MuxHandshakeError>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        secure
            .send_frame(RELAY_MUX_HELLO)
            .await
            .map_err(MuxHandshakeError::Transport)?;
        let reply = tokio::time::timeout(Duration::from_secs(10), secure.recv_frame())
            .await
            .map_err(|_| MuxHandshakeError::Transport(AttachError::StreamClosed))?;
        match reply {
            Ok(bytes) if bytes.as_slice() == RELAY_MUX_HELLO_OK => {}
            // An older daemon forwards `FXMUX1\r\n` to Axum, which answers HTTP 400 and closes.
            Ok(_) | Err(AttachError::StreamClosed) => return Err(MuxHandshakeError::Unsupported),
            Err(error) => return Err(MuxHandshakeError::Transport(error)),
        }

        let (stream_rx, stream_tx) = tokio::io::split(secure.inner);
        let transport = Arc::new(Mutex::new(secure.transport));
        let queue = FairQueue::new();
        let client = Arc::new(Self {
            queue: Arc::clone(&queue),
            channels: Mutex::new(ClientChannels {
                inbound: HashMap::new(),
                dead: false,
                last_inbound: Instant::now(),
            }),
            next_channel: AtomicU32::new(1),
        });

        tokio::spawn(run_writer(
            stream_tx,
            Arc::clone(&transport),
            Arc::clone(&queue),
        ));
        tokio::spawn(client_read_task(
            stream_rx,
            Arc::clone(&transport),
            Arc::downgrade(&client),
        ));
        if !ping_interval.is_zero() {
            tokio::spawn(client_ping_task(
                Arc::downgrade(&client),
                ping_interval,
                silence_limit,
            ));
        }

        Ok(client)
    }

    pub fn is_live(&self) -> bool {
        !self.channels.lock().unwrap_or_else(|p| p.into_inner()).dead
    }

    pub fn mark_dead(&self) {
        let mut state = self.channels.lock().unwrap_or_else(|p| p.into_inner());
        state.dead = true;
        state.inbound.clear();
        drop(state);
        self.queue.mark_dead();
    }

    pub fn open_stream(self: &Arc<Self>) -> io::Result<RelayMuxStream> {
        let mut channel = self.next_channel.fetch_add(1, Ordering::Relaxed);
        if channel == 0 {
            channel = self.next_channel.fetch_add(1, Ordering::Relaxed);
        }
        let (tx, rx) = mpsc::unbounded_channel();
        {
            let mut state = self.channels.lock().unwrap_or_else(|p| p.into_inner());
            if state.dead {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "relay mux transport closed",
                ));
            }
            if state.inbound.len() >= RELAY_MUX_MAX_CHANNELS {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "relay mux channel limit reached",
                ));
            }
            state.inbound.insert(channel, tx);
        }
        if !self
            .queue
            .push_channel_frame(channel, encode_mux_frame(channel, MuxKind::Open, &[]))
        {
            self.channels
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .inbound
                .remove(&channel);
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "relay mux transport closed",
            ));
        }
        Ok(RelayMuxStream {
            mux: Arc::clone(self),
            channel,
            inbound: rx,
            pending: Vec::new(),
            pending_at: 0,
            eof: false,
            write_closed: AtomicBool::new(false),
        })
    }

    fn close_write(&self, channel: u32) {
        self.queue
            .push_channel_frame(channel, encode_mux_frame(channel, MuxKind::Close, &[]));
    }

    fn detach_channel(&self, channel: u32) {
        self.channels
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .inbound
            .remove(&channel);
    }
}

async fn client_read_task<R: AsyncRead + Unpin>(
    mut reader: R,
    transport: Arc<Mutex<snow::TransportState>>,
    mux: std::sync::Weak<RelayMuxClient>,
) {
    loop {
        let raw = match read_frame_or_end(&mut reader, MAX_ATTACH_FRAME + 16).await {
            Ok(Some(frame)) => frame,
            Ok(None) | Err(_) => break,
        };
        let Some(mux) = mux.upgrade() else {
            return;
        };
        let plaintext = {
            let mut buf = vec![0u8; raw.len().max(1)];
            let mut state = transport.lock().unwrap_or_else(|p| p.into_inner());
            match state.read_message(&raw, &mut buf) {
                Ok(n) => {
                    buf.truncate(n);
                    buf
                }
                Err(_) => break,
            }
        };
        let Some((channel, kind, payload)) = decode_mux_frame(&plaintext) else {
            break;
        };
        {
            let mut state = mux.channels.lock().unwrap_or_else(|p| p.into_inner());
            state.last_inbound = Instant::now();
            match kind {
                MuxKind::Data => {
                    if let Some(sender) = state.inbound.get(&channel) {
                        let _ = sender.send(payload.to_vec());
                    }
                }
                MuxKind::Close => {
                    state.inbound.remove(&channel);
                }
                MuxKind::Ping => {
                    drop(state);
                    mux.queue
                        .push_control(encode_mux_frame(0, MuxKind::Pong, payload));
                }
                MuxKind::Pong | MuxKind::Open => {}
            }
        }
    }
    if let Some(mux) = mux.upgrade() {
        mux.mark_dead();
    }
}

async fn client_ping_task(
    mux: std::sync::Weak<RelayMuxClient>,
    ping_interval: Duration,
    silence_limit: Duration,
) {
    let mut ticker = tokio::time::interval(ping_interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    ticker.tick().await;
    loop {
        ticker.tick().await;
        let Some(mux) = mux.upgrade() else {
            return;
        };
        let expired = {
            let state = mux.channels.lock().unwrap_or_else(|p| p.into_inner());
            if state.dead {
                return;
            }
            state.last_inbound.elapsed() >= silence_limit
        };
        if expired {
            mux.mark_dead();
            return;
        }
        if !mux.queue.push_control(encode_mux_frame(0, MuxKind::Ping, &[])) {
            mux.mark_dead();
            return;
        }
    }
}

pub struct RelayMuxStream {
    mux: Arc<RelayMuxClient>,
    channel: u32,
    inbound: mpsc::UnboundedReceiver<Vec<u8>>,
    pending: Vec<u8>,
    pending_at: usize,
    eof: bool,
    write_closed: AtomicBool,
}

impl Drop for RelayMuxStream {
    fn drop(&mut self) {
        if !self.write_closed.swap(true, Ordering::AcqRel) {
            self.mux.close_write(self.channel);
        }
        self.mux.detach_channel(self.channel);
    }
}

impl AsyncRead for RelayMuxStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        loop {
            if self.pending_at < self.pending.len() {
                let available = &self.pending[self.pending_at..];
                let n = available.len().min(buf.remaining());
                buf.put_slice(&available[..n]);
                self.pending_at += n;
                return Poll::Ready(Ok(()));
            }
            if self.eof {
                return Poll::Ready(Ok(()));
            }
            match self.inbound.poll_recv(cx) {
                Poll::Ready(Some(bytes)) => {
                    self.pending = bytes;
                    self.pending_at = 0;
                }
                Poll::Ready(None) => {
                    self.eof = true;
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

impl AsyncWrite for RelayMuxStream {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.write_closed.load(Ordering::Acquire) {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "relay mux channel closed",
            )));
        }
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }
        if !self.mux.queue.push_data(self.channel, buf) {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "relay mux transport closed",
            )));
        }
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if !self.write_closed.swap(true, Ordering::AcqRel) {
            self.mux.close_write(self.channel);
        }
        Poll::Ready(Ok(()))
    }
}

#[cfg(test)]
#[path = "relay_mux_tests.rs"]
mod tests;
