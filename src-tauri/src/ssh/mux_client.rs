//! One SSH connection per host, shared by every bridge channel to that host.
//!
//! `HostMux` runs `ferryx-remote-helper bridge --mux` over a single supervised `ssh` child and
//! hands out `MuxStream`s, each an ordinary helper connection inside it. `BridgeConnection` reads
//! and writes a `MuxStream` exactly as it does a per-connection `ssh` child's stdio, so the RPC
//! layer above is unchanged. Wire format: `ferryx_scope/ssh/process/mux.rs`.
//!
//! Invariants the code alone does not convey:
//! - A mux is keyed by `master::ConnectionKey` plus the helper root, so two hosts never share one
//!   and a rotated helper root gets its own.
//! - Transport loss fails every open stream at once with EOF. Each session's existing reconnect
//!   loop then redials, and concurrent redials coalesce on the registry's per-key lock, so a
//!   host with N panes pays one SSH handshake, not N.
//! - A helper without `bridge --mux` answers with a usage error instead of the hello. The key is
//!   then remembered as legacy for `LEGACY_RETRY` and callers fall back to `bridge --stdio`.

use super::bridge::{BridgeConnection, BridgeError};
use super::helper_runtime::process::{
    read_mux_frame, write_mux_frame, MuxFrame, MuxKind, MUX_HELLO_MAGIC, MUX_PROTOCOL,
    MUX_SEGMENT,
};
use super::helper_setup::HelperLocation;
use super::master::ConnectionKey;
use super::runtime::RemoteEnvironment;
use super::SshHost;
use std::collections::HashMap;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::{Arc, LazyLock, Weak};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::{mpsc, Mutex};

/// The client pings this often; the helper's idle bound is far longer.
pub const MUX_PING_INTERVAL: Duration = Duration::from_secs(15);
/// A mux with no open stream stays connected this long, so tab churn does not redial.
pub const MUX_IDLE_GRACE: Duration = Duration::from_secs(300);
/// No frame at all from the helper for this long means the transport is gone even if `ssh` has
/// not noticed yet; every stream then fails and reconnects.
pub const MUX_SILENCE_LIMIT: Duration = Duration::from_secs(45);
const HELLO_TIMEOUT: Duration = Duration::from_secs(20);
/// How long a host that answered without `bridge --mux` keeps the per-connection path.
const LEGACY_RETRY: Duration = Duration::from_secs(600);
/// Outbound queue depth in segments. A writer that falls this far behind applies backpressure
/// to the writing stream rather than buffering without bound.
const OUTBOUND_DEPTH: usize = 256;

/// Set `FERRYX_SSH_MUX=0` to force one `ssh` per helper connection, as before.
pub fn enabled() -> bool {
    std::env::var("FERRYX_SSH_MUX").map_or(true, |value| value != "0")
}

/// An old helper rejects `bridge --mux` with its usage line; that, and only that, means the
/// host has no mux.
fn is_unsupported_mux(stderr: &str) -> bool {
    stderr.contains("Usage: ferryx-remote-helper") && !stderr.contains("--mux")
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct MuxKey {
    connection: ConnectionKey,
    root: String,
}

impl MuxKey {
    fn of(host: &SshHost, location: &HelperLocation) -> Self {
        Self { connection: ConnectionKey::of(host), root: location.root.clone() }
    }
}

enum Slot {
    /// Strong: the registry keeps an idle mux connected through `MUX_IDLE_GRACE`.
    Live(Arc<HostMux>),
    Legacy(Instant),
}

struct Registry {
    slots: std::sync::Mutex<HashMap<MuxKey, Slot>>,
        /// One dial at a time per key. The value is the outcome of the last finished dial, so callers
    /// that queued behind a failed dial share its error instead of each dialing again.
    dialing: std::sync::Mutex<HashMap<MuxKey, Arc<Mutex<Option<(Instant, String)>>>>>,
}

static REGISTRY: LazyLock<Registry> = LazyLock::new(|| Registry {
    slots: std::sync::Mutex::new(HashMap::new()),
    dialing: std::sync::Mutex::new(HashMap::new()),
});

pub enum Acquired {
    Stream(MuxStream),
    /// The helper on this host has no mux; use one `ssh` per connection.
    Legacy,
}

pub async fn open_stream(
    host: &SshHost,
    env: &RemoteEnvironment,
    location: &HelperLocation,
) -> Result<Acquired, BridgeError> {
    open_stream_with(MuxKey::of(host, location), || HostMux::dial(host, env, location)).await
}

async fn open_stream_with<F, Fut>(key: MuxKey, dial: F) -> Result<Acquired, BridgeError>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<Arc<HostMux>, DialError>>,
{
    if let Some(acquired) = existing(&key) {
        return acquired;
    }
    let gate = {
        let mut dialing = REGISTRY.dialing.lock().unwrap_or_else(|p| p.into_inner());
        Arc::clone(dialing.entry(key.clone()).or_default())
    };
    let queued_at = Instant::now();
    let mut last_failure = gate.lock().await;
    if let Some(acquired) = existing(&key) {
        return acquired;
    }
    if let Some((failed_at, message)) = last_failure.as_ref() {
        if *failed_at >= queued_at {
            // A transport failure, so the caller's reconnect loop keeps retrying on its own
            // backoff instead of treating the pane as permanently disconnected.
            return Err(BridgeError::Io(std::io::Error::new(
                std::io::ErrorKind::ConnectionAborted,
                format!("Shared SSH connection failed: {message}"),
            )));
        }
    }
    let dialed = dial().await;
    *last_failure = match &dialed {
        Err(DialError::Bridge(error)) => Some((Instant::now(), error.to_string())),
        _ => None,
    };
    match dialed {
        Ok(mux) => {
            REGISTRY
                .slots
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(key, Slot::Live(Arc::clone(&mux)));
            mux.open().map(Acquired::Stream)
        }
        Err(DialError::Unsupported) => {
            REGISTRY
                .slots
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(key, Slot::Legacy(Instant::now()));
            Ok(Acquired::Legacy)
        }
        Err(DialError::Bridge(error)) => Err(error),
    }
}

fn existing(key: &MuxKey) -> Option<Result<Acquired, BridgeError>> {
    let mut slots = REGISTRY.slots.lock().unwrap_or_else(|p| p.into_inner());
    match slots.get(key) {
        Some(Slot::Live(mux)) if mux.is_alive() => Some(mux.open().map(Acquired::Stream)),
        Some(Slot::Live(_)) => {
            slots.remove(key);
            None
        }
        Some(Slot::Legacy(since)) if since.elapsed() < LEGACY_RETRY => Some(Ok(Acquired::Legacy)),
        Some(Slot::Legacy(_)) => {
            slots.remove(key);
            None
        }
        None => None,
    }
}

/// True when a live mux already reaches the helper for `host`/`location`: the helper daemon is
/// running, so `ensure_started` (one more `ssh` round trip) can be skipped.
pub fn is_live(host: &SshHost, location: &HelperLocation) -> bool {
    let key = MuxKey::of(host, location);
    matches!(
        REGISTRY.slots.lock().unwrap_or_else(|p| p.into_inner()).get(&key),
        Some(Slot::Live(mux)) if mux.is_alive()
    )
}

/// The remote environment last detected for a host whose mux is live. It cannot change while
/// the same SSH connection stays up, so reusing it saves a detection round trip per pane.
pub fn cached_environment(host: &SshHost) -> Option<RemoteEnvironment> {
    let connection = ConnectionKey::of(host);
    let environments = ENVIRONMENTS.lock().unwrap_or_else(|p| p.into_inner());
    let environment = environments.get(&connection)?.clone();
    let live = REGISTRY
        .slots
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .any(|(key, slot)| {
            key.connection == connection
                && matches!(slot, Slot::Live(mux) if mux.is_alive())
        });
    live.then_some(environment)
}

pub fn remember_environment(host: &SshHost, environment: &RemoteEnvironment) {
    ENVIRONMENTS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(ConnectionKey::of(host), environment.clone());
}

static ENVIRONMENTS: LazyLock<std::sync::Mutex<HashMap<ConnectionKey, RemoteEnvironment>>> =
    LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));

/// The `ssh` child pid of every live mux, for diagnostics and the real-transport tests.
pub fn live_transport_pids() -> Vec<u32> {
    REGISTRY
        .slots
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .values()
        .filter_map(|slot| match slot {
            Slot::Live(mux) if mux.is_alive() => mux.transport_pid(),
            _ => None,
        })
        .collect()
}

pub fn live_count() -> usize {
    REGISTRY
        .slots
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .values()
        .filter(|slot| matches!(slot, Slot::Live(mux) if mux.is_alive()))
        .count()
}

enum DialError {
    Unsupported,
    Bridge(BridgeError),
}

impl From<BridgeError> for DialError {
    fn from(error: BridgeError) -> Self {
        Self::Bridge(error)
    }
}

struct Channels {
    next_id: u32,
    inbound: HashMap<u32, mpsc::UnboundedSender<Vec<u8>>>,
    open_streams: usize,
    dead: bool,
    idle_since: Option<Instant>,
    last_inbound: Instant,
}

pub struct HostMux {
    outbound: mpsc::Sender<MuxFrame>,
    channels: std::sync::Mutex<Channels>,
    /// Owns the `ssh` child; dropping it ends the transport.
    _transport: std::sync::Mutex<Option<BridgeConnection>>,
}

impl HostMux {
    fn is_alive(&self) -> bool {
        !self.channels.lock().unwrap_or_else(|p| p.into_inner()).dead
    }

    fn open(self: &Arc<Self>) -> Result<MuxStream, BridgeError> {
        let (sender, receiver) = mpsc::unbounded_channel();
        let id = {
            let mut channels = self.channels.lock().unwrap_or_else(|p| p.into_inner());
            if channels.dead {
                return Err(BridgeError::ConnectionClosed);
            }
            let mut id = channels.next_id;
            while id == 0 || channels.inbound.contains_key(&id) {
                id = id.wrapping_add(1);
            }
            channels.next_id = id.wrapping_add(1);
            channels.inbound.insert(id, sender);
            channels.open_streams += 1;
            channels.idle_since = None;
            id
        };
        self.outbound
            .try_send(MuxFrame { channel: id, kind: MuxKind::Open, payload: Vec::new() })
            .map_err(|_| BridgeError::ConnectionClosed)?;
        Ok(MuxStream {
            mux: Arc::clone(self),
            channel: id,
            inbound: receiver,
            pending: Vec::new(),
            pending_at: 0,
            eof: false,
            sending: None,
            closed: false,
        })
    }

    fn release(&self, channel: u32) {
        let mut channels = self.channels.lock().unwrap_or_else(|p| p.into_inner());
        if channels.inbound.remove(&channel).is_some() {
            channels.open_streams = channels.open_streams.saturating_sub(1);
            if channels.open_streams == 0 {
                channels.idle_since = Some(Instant::now());
            }
        }
        drop(channels);
        let _ = self
            .outbound
            .try_send(MuxFrame { channel, kind: MuxKind::Close, payload: Vec::new() });
    }

    fn mark_dead(&self) {
        let mut channels = self.channels.lock().unwrap_or_else(|p| p.into_inner());
        channels.dead = true;
        // Dropping every sender is what makes each stream read EOF.
        channels.inbound.clear();
        drop(channels);
        // Ends the `ssh` child now rather than when the last reference goes away.
        drop(self._transport.lock().unwrap_or_else(|p| p.into_inner()).take());
    }

    async fn dial(
        host: &SshHost,
        env: &RemoteEnvironment,
        location: &HelperLocation,
    ) -> Result<Arc<Self>, DialError> {
        let plan = super::direct::mux_plan(host, env, location).map_err(BridgeError::from)?;
        let mut transport = BridgeConnection::spawn_plan(&plan).await?;
        let (writer, mut reader) = transport
            .take_streams()
            .ok_or_else(|| BridgeError::ProcessSpawn("Mux transport stdio unavailable".into()))?;

        let hello = tokio::time::timeout(HELLO_TIMEOUT, read_frame_async(&mut reader))
            .await
            .map_err(|_| BridgeError::Timeout("mux hello".into()))?;
        let hello = match hello {
            Ok(Some(frame)) if frame.channel == 0 && frame.kind == MuxKind::Hello => frame,
            // EOF or a non-mux frame. Only an old helper's usage error means "no mux here";
            // anything else (auth, network, a dead helper) is a real failure the caller retries.
            Ok(_) | Err(_) => {
                let stderr = transport.drained_stderr().await;
                return Err(if is_unsupported_mux(&stderr) {
                    DialError::Unsupported
                } else {
                    DialError::Bridge(BridgeError::ProcessExited { code: None, stderr })
                });
            }
        };
        let hello: serde_json::Value =
            serde_json::from_slice(&hello.payload).map_err(|_| DialError::Unsupported)?;
        if hello.get("magic").and_then(serde_json::Value::as_str) != Some(MUX_HELLO_MAGIC)
            || hello.get("protocol").and_then(serde_json::Value::as_u64)
                != Some(u64::from(MUX_PROTOCOL))
        {
            return Err(DialError::Unsupported);
        }

        Ok(Self::start(writer, reader, Some(transport)))
    }

    /// Runs a mux over an already-greeted transport. `transport` owns the `ssh` child, if any.
    pub(crate) fn start<W, R>(
        writer: W,
        reader: R,
        transport: Option<BridgeConnection>,
    ) -> Arc<Self>
    where
        W: AsyncWrite + Unpin + Send + 'static,
        R: AsyncRead + Unpin + Send + 'static,
    {
        let (outbound, outbound_rx) = mpsc::channel(OUTBOUND_DEPTH);
        let mux = Arc::new(Self {
            outbound,
            channels: std::sync::Mutex::new(Channels {
                next_id: 1,
                inbound: HashMap::new(),
                open_streams: 0,
                dead: false,
                idle_since: None,
                last_inbound: Instant::now(),
            }),
            _transport: std::sync::Mutex::new(transport),
        });
        tokio::spawn(write_task(writer, outbound_rx, Arc::downgrade(&mux)));
        tokio::spawn(read_task(reader, Arc::downgrade(&mux)));
        tokio::spawn(ping_task(Arc::downgrade(&mux)));
        mux
    }

    pub(crate) fn open_stream(self: &Arc<Self>) -> Result<MuxStream, BridgeError> {
        self.open()
    }

    pub(crate) fn alive(&self) -> bool {
        self.is_alive()
    }

    fn transport_pid(&self) -> Option<u32> {
        self._transport
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .and_then(BridgeConnection::child_id)
    }
}

async fn read_frame_async<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<Option<MuxFrame>> {
    use tokio::io::AsyncReadExt;
    let mut len = [0u8; 4];
    match reader.read(&mut len[..1]).await? {
        0 => return Ok(None),
        _ => reader.read_exact(&mut len[1..]).await?,
    };
    let len = u32::from_be_bytes(len) as usize;
    if !(5..=MUX_SEGMENT + 64).contains(&len) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "mux frame length"));
    }
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body).await?;
    let mut framed = Vec::with_capacity(4 + len);
    framed.extend_from_slice(&(len as u32).to_be_bytes());
    framed.extend_from_slice(&body);
    read_mux_frame(&mut io::Cursor::new(framed))
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

async fn write_task<W: AsyncWrite + Unpin>(
    mut writer: W,
    mut frames: mpsc::Receiver<MuxFrame>,
    mux: Weak<HostMux>,
) {
    use tokio::io::AsyncWriteExt;
    while let Some(frame) = frames.recv().await {
        let mut bytes = Vec::with_capacity(9 + frame.payload.len());
        if write_mux_frame(&mut bytes, &frame).is_err()
            || writer.write_all(&bytes).await.is_err()
            || writer.flush().await.is_err()
        {
            break;
        }
    }
    if let Some(mux) = mux.upgrade() {
        mux.mark_dead();
    }
}

async fn read_task<R: AsyncRead + Unpin>(mut reader: R, mux: Weak<HostMux>) {
    loop {
        let frame = match read_frame_async(&mut reader).await {
            Ok(Some(frame)) => frame,
            Ok(None) | Err(_) => break,
        };
        let Some(mux) = mux.upgrade() else { return };
        mux.channels.lock().unwrap_or_else(|p| p.into_inner()).last_inbound = Instant::now();
        match frame.kind {
            MuxKind::Data => {
                let channels = mux.channels.lock().unwrap_or_else(|p| p.into_inner());
                if let Some(sender) = channels.inbound.get(&frame.channel) {
                    let _ = sender.send(frame.payload);
                }
            }
            MuxKind::Close => {
                // The helper ended this channel: its stream reads EOF.
                mux.channels
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .inbound
                    .remove(&frame.channel);
            }
            MuxKind::Pong | MuxKind::Hello | MuxKind::Ping | MuxKind::Open => {}
        }
    }
    if let Some(mux) = mux.upgrade() {
        mux.mark_dead();
    }
}

async fn ping_task(mux: Weak<HostMux>) {
    let mut ticker = tokio::time::interval(MUX_PING_INTERVAL);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    ticker.tick().await;
    loop {
        ticker.tick().await;
        let Some(mux) = mux.upgrade() else { return };
        let expired = {
            let channels = mux.channels.lock().unwrap_or_else(|p| p.into_inner());
            if channels.dead {
                return;
            }
            channels.idle_since.is_some_and(|since| since.elapsed() >= MUX_IDLE_GRACE)
                || channels.last_inbound.elapsed() >= MUX_SILENCE_LIMIT
        };
        if expired {
            mux.mark_dead();
            return;
        }
        if mux
            .outbound
            .send(MuxFrame { channel: 0, kind: MuxKind::Ping, payload: Vec::new() })
            .await
            .is_err()
        {
            return;
        }
    }
}

type SendFuture = Pin<Box<dyn std::future::Future<Output = bool> + Send>>;

/// One helper connection inside a `HostMux`. Reads EOF when the channel or the mux ends.
pub struct MuxStream {
    mux: Arc<HostMux>,
    channel: u32,
    inbound: mpsc::UnboundedReceiver<Vec<u8>>,
    pending: Vec<u8>,
    pending_at: usize,
    eof: bool,
    /// A segment whose queueing is waiting on outbound capacity, and how many bytes it carries.
    sending: Option<(SendFuture, usize)>,
    closed: bool,
}

impl MuxStream {
    pub fn channel(&self) -> u32 {
        self.channel
    }
}

impl AsyncRead for MuxStream {
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
                Poll::Ready(None) => self.eof = true,
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

impl AsyncWrite for MuxStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.closed {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }
        if self.sending.is_none() {
            let len = data.len().min(MUX_SEGMENT);
            let frame = MuxFrame {
                channel: self.channel,
                kind: MuxKind::Data,
                payload: data[..len].to_vec(),
            };
            let outbound = self.mux.outbound.clone();
            self.sending =
                Some((Box::pin(async move { outbound.send(frame).await.is_ok() }), len));
        }
        let (future, len) = self.sending.as_mut().expect("send in progress");
        let len = *len;
        match future.as_mut().poll(cx) {
            Poll::Ready(sent) => {
                self.sending = None;
                if sent {
                    Poll::Ready(Ok(len))
                } else {
                    Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()))
                }
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if !self.closed {
            self.closed = true;
            self.mux.release(self.channel);
        }
        Poll::Ready(Ok(()))
    }
}

impl Drop for MuxStream {
    fn drop(&mut self) {
        if !self.closed {
            self.closed = true;
            self.mux.release(self.channel);
        }
    }
}

#[cfg(all(test, unix))]
#[path = "mux_client_tests.rs"]
mod mux_client_tests;
