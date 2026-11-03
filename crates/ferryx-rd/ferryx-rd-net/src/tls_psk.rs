//! OpenSSL TLS 1.2 PSK transport and v3 TCP framing.

#[cfg(test)]
#[path = "nonblocking_write_tests.rs"]
mod nonblocking_write_tests;

use std::{
    collections::{HashMap, VecDeque},
    io::{self, Read, Write},
    net::{TcpListener, TcpStream, ToSocketAddrs},
    sync::Arc,
    time::{Duration, Instant},
};

use ferryx_rd_proto::{TcpFrameEvent, TcpFrameReader, TcpFrameWriter};
use openssl::{
    error::ErrorStack,
    hash::MessageDigest,
    pkcs5,
    ssl::{
        HandshakeError, SslAcceptor, SslConnector, SslContextBuilder, SslMethod, SslStream,
        SslVerifyMode, SslVersion,
    },
};
use thiserror::Error;

use crate::udp_gcm::hkdf_sha256;

pub const BOOTSTRAP_IDENTITY: &str = "maho-b1";
pub const PAIRING_IDENTITY_PREFIX: &str = "maho-p1.";
pub const PSK_CIPHER_LIST: &str = "PSK-AES128-GCM-SHA256:PSK-AES256-GCM-SHA384";
pub const MAX_PAIRING_ATTEMPTS: usize = 5;
pub const PAIRING_ATTEMPT_WINDOW: Duration = Duration::from_secs(60);
pub const PAIRING_LOCKOUT: Duration = Duration::from_secs(300);
/// Wire-protocol v3 constant: HKDF salt input shared with every peer
/// implementation. Never rename.
const BOOTSTRAP_SALT: &[u8] = b"erd/bootstrap/v3";
pub(crate) const BOOTSTRAP_STRETCH_ROUNDS: usize = 600_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PskIdentity {
    identity: String,
    key: Vec<u8>,
}

impl PskIdentity {
    pub fn bootstrap(pin: &str) -> Result<Self, TlsPskError> {
        Ok(Self {
            identity: BOOTSTRAP_IDENTITY.to_owned(),
            key: bootstrap_psk(pin)?.to_vec(),
        })
    }

    pub fn pairing(pairing_id: &str, key: &[u8]) -> Result<Self, TlsPskError> {
        if pairing_id.is_empty() {
            return Err(TlsPskError::InvalidIdentity);
        }
        if key.len() != 32 {
            return Err(TlsPskError::InvalidKeyLength);
        }
        Self::new(
            format!("{PAIRING_IDENTITY_PREFIX}{pairing_id}"),
            key.to_vec(),
        )
    }

    pub fn new(identity: impl Into<String>, key: Vec<u8>) -> Result<Self, TlsPskError> {
        let identity = identity.into();
        if identity.is_empty() || identity.as_bytes().contains(&0) {
            return Err(TlsPskError::InvalidIdentity);
        }
        if key.is_empty() {
            return Err(TlsPskError::InvalidKeyLength);
        }
        Ok(Self { identity, key })
    }

    pub fn identity(&self) -> &str {
        &self.identity
    }

    pub fn key(&self) -> &[u8] {
        &self.key
    }
}

#[derive(Debug, Error)]
pub enum TlsPskError {
    #[error("PSK identity must be non-empty UTF-8 without NUL bytes")]
    InvalidIdentity,
    #[error("invalid PSK key length")]
    InvalidKeyLength,
    #[error("OpenSSL setup failed: {0}")]
    OpenSsl(#[from] ErrorStack),
    #[error("TLS handshake failed: {0}")]
    Handshake(String),
    #[error("TCP I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("invalid TCP frame: {0}")]
    Frame(#[from] ferryx_rd_proto::CodecError),
    #[error("peer sent an invalid frame length {0}")]
    InvalidFrameLength(u32),
}

/// Host-side bootstrap failure tracker. A new pairing window resets lockout.
#[derive(Debug, Default)]
pub struct BootstrapLockout {
    pairing_active: bool,
    failure_count: usize,
    failure_window_start: Option<Instant>,
    locked_until: Option<Instant>,
}

impl BootstrapLockout {
    pub fn begin_pairing(&mut self) {
        self.pairing_active = true;
        self.failure_count = 0;
        self.failure_window_start = None;
        self.locked_until = None;
    }

    pub fn cancel_pairing(&mut self) {
        self.pairing_active = false;
    }

    pub fn is_allowed(&self, now: Instant) -> bool {
        self.pairing_active
            && self
                .locked_until
                .map_or(true, |locked_until| now >= locked_until)
    }

    /// Records a failed bootstrap handshake and returns whether it locked out.
    pub fn record_failure(&mut self, now: Instant) -> bool {
        if self.failure_window_start.map_or(true, |start| {
            now.saturating_duration_since(start) > PAIRING_ATTEMPT_WINDOW
        }) {
            self.failure_count = 0;
            self.failure_window_start = Some(now);
        }
        self.failure_count += 1;
        if self.failure_count >= MAX_PAIRING_ATTEMPTS {
            self.locked_until = Some(now + PAIRING_LOCKOUT);
            self.pairing_active = false;
            return true;
        }
        false
    }
}

pub struct TlsPskClient {
    connector: SslConnector,
}

impl TlsPskClient {
    pub fn new(psk: PskIdentity) -> Result<Self, TlsPskError> {
        let mut builder = SslConnector::builder(SslMethod::tls_client())?;
        configure_context(&mut builder)?;
        builder.set_psk_client_callback(move |_, _, identity_buffer, psk_buffer| {
            // OpenSSL's TLS 1.2 API requires a C-string identity. The identity
            // itself is copied byte-exactly; the trailing NUL is only the API
            // terminator and is not part of the offered identity.
            let identity = psk.identity.as_bytes();
            if identity.len() + 1 > identity_buffer.len() || psk.key.len() > psk_buffer.len() {
                return Err(ErrorStack::get());
            }
            identity_buffer[..identity.len()].copy_from_slice(identity);
            identity_buffer[identity.len()] = 0;
            psk_buffer[..psk.key.len()].copy_from_slice(&psk.key);
            Ok(psk.key.len())
        });
        Ok(Self {
            connector: builder.build(),
        })
    }

    pub fn connect<A: ToSocketAddrs>(
        &self,
        address: A,
    ) -> Result<TlsPskStream<TcpStream>, TlsPskError> {
        let tcp = TcpStream::connect(address)?;
        let _ = tcp.set_nodelay(true);
        self.connect_stream(tcp)
    }

    pub fn connect_stream<S: Read + Write + std::fmt::Debug>(
        &self,
        stream: S,
    ) -> Result<TlsPskStream<S>, TlsPskError> {
        let configuration = self
            .connector
            .configure()?
            .use_server_name_indication(false)
            .verify_hostname(false);
        let stream = configuration
            .connect("maho-psk", stream)
            .map_err(handshake_error)?;
        Ok(TlsPskStream::new(stream))
    }
}

#[derive(Clone)]
pub struct TlsPskServer {
    acceptor: SslAcceptor,
}

impl TlsPskServer {
    pub fn new(psks: impl IntoIterator<Item = PskIdentity>) -> Result<Self, TlsPskError> {
        let keys = psks
            .into_iter()
            .map(|psk| (psk.identity.into_bytes(), psk.key))
            .collect::<HashMap<_, _>>();
        if keys.is_empty() {
            return Err(TlsPskError::InvalidKeyLength);
        }
        let keys = Arc::new(keys);

        let mut builder = SslAcceptor::mozilla_intermediate_v5(SslMethod::tls_server())?;
        configure_context(&mut builder)?;
        builder.set_psk_server_callback(move |_, identity, psk_buffer| {
            let Some(key) = identity.and_then(|identity| keys.get(identity)) else {
                return Ok(0);
            };
            if key.len() > psk_buffer.len() {
                return Err(ErrorStack::get());
            }
            psk_buffer[..key.len()].copy_from_slice(key);
            Ok(key.len())
        });
        Ok(Self {
            acceptor: builder.build(),
        })
    }

    pub fn bind<A: ToSocketAddrs>(&self, address: A) -> Result<TlsPskListener, TlsPskError> {
        Ok(TlsPskListener {
            listener: TcpListener::bind(address)?,
            server: self.clone(),
        })
    }

    pub fn accept_stream<S: Read + Write + std::fmt::Debug>(
        &self,
        stream: S,
    ) -> Result<TlsPskStream<S>, TlsPskError> {
        let stream = self.acceptor.accept(stream).map_err(handshake_error)?;
        Ok(TlsPskStream::new(stream))
    }

    pub fn accept_stream_until(
        &self,
        stream: TcpStream,
        deadline: Instant,
    ) -> Result<TlsPskStream<TcpStream>, TlsPskError> {
        let readiness = DeadlineReadiness::new(&stream)?;
        stream.set_nonblocking(true)?;
        check_deadline(deadline)?;
        let mut result = self.acceptor.accept(stream);
        loop {
            check_deadline(deadline)?;
            match result {
                Ok(stream) => {
                    stream.get_ref().set_nonblocking(false)?;
                    return Ok(TlsPskStream::new(stream));
                }
                Err(HandshakeError::WouldBlock(mid)) => {
                    let interest = handshake_interest(mid.error().code());
                    let mut next = None;
                    let mut mid = Some(mid);
                    readiness.wait_io(deadline, interest, || {
                        let resumed = mid.take().unwrap().handshake();
                        let blocked = matches!(&resumed, Err(HandshakeError::WouldBlock(m))
                            if handshake_interest(m.error().code()) == interest);
                        if blocked {
                            if let Err(HandshakeError::WouldBlock(m)) = resumed {
                                mid = Some(m);
                            }
                            Err(io::ErrorKind::WouldBlock.into())
                        } else {
                            next = Some(resumed);
                            Ok(())
                        }
                    })?;
                    result = next.unwrap();
                }
                Err(error) => return Err(handshake_error(error)),
            }
        }
    }
}

fn check_deadline(deadline: Instant) -> io::Result<()> {
    if Instant::now() >= deadline {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "TLS deadline expired",
        ))
    } else {
        Ok(())
    }
}

fn handshake_interest(code: openssl::ssl::ErrorCode) -> tokio::io::Interest {
    if code == openssl::ssl::ErrorCode::WANT_WRITE {
        tokio::io::Interest::WRITABLE
    } else {
        tokio::io::Interest::READABLE
    }
}

// These synchronous APIs run on the host's blocking connection thread, not a
// Tokio executor. Register a duplicate handle; OpenSSL retains the original.
struct DeadlineReadiness {
    socket: tokio::net::TcpStream,
    runtime: tokio::runtime::Runtime,
}

impl DeadlineReadiness {
    fn new(stream: &TcpStream) -> io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let socket = stream.try_clone()?;
        socket.set_nonblocking(true)?;
        let socket = {
            let _guard = runtime.enter();
            tokio::net::TcpStream::from_std(socket)?
        };
        Ok(Self { socket, runtime })
    }

    fn wait_io<T>(
        &self,
        deadline: Instant,
        interest: tokio::io::Interest,
        mut operation: impl FnMut() -> io::Result<T>,
    ) -> io::Result<T> {
        self.runtime.block_on(async {
            tokio::time::timeout_at(deadline.into(), async {
                loop {
                    check_deadline(deadline)?;
                    self.socket.ready(interest).await?;
                    check_deadline(deadline)?;
                    match self.socket.try_io(interest, &mut operation) {
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => continue,
                        result => return result,
                    }
                }
            })
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "TLS deadline expired"))?
        })
    }
}

pub struct TlsPskListener {
    listener: TcpListener,
    server: TlsPskServer,
}

impl TlsPskListener {
    pub fn local_addr(&self) -> io::Result<std::net::SocketAddr> {
        self.listener.local_addr()
    }

    pub fn accept(&self) -> Result<TlsPskStream<TcpStream>, TlsPskError> {
        let (stream, _) = self.listener.accept()?;
        let _ = stream.set_nodelay(true);
        self.server.accept_stream(stream)
    }
}

pub struct TlsPskStream<S> {
    stream: SslStream<S>,
    frame_reader: TcpFrameReader,
    pending_events: VecDeque<TcpFrameEvent>,
    read_needs_write: bool,
    outbound: VecDeque<PendingFrame>,
    outbound_bytes: usize,
    write_needs_read: bool,
    write_failed: bool,
}

// Bound both tiny-frame metadata and retained plaintext (including the active
// frame). A maximum-size protocol frame can always fit in an empty queue.
const MAX_OUTBOUND_FRAMES: usize = 64;
const MAX_OUTBOUND_BYTES: usize = ferryx_rd_proto::MAX_TCP_FRAME_SIZE + 4;
const TLS_WRITE_CHUNK: usize = 16 * 1024;

struct PendingFrame {
    bytes: Vec<u8>,
    offset: usize,
}

impl<S: Read + Write> TlsPskStream<S> {
    fn new(stream: SslStream<S>) -> Self {
        Self {
            stream,
            frame_reader: TcpFrameReader::new(),
            pending_events: VecDeque::new(),
            read_needs_write: false,
            outbound: VecDeque::new(),
            outbound_bytes: 0,
            write_needs_read: false,
            write_failed: false,
        }
    }

    pub fn ssl_stream(&self) -> &SslStream<S> {
        &self.stream
    }

    pub fn ssl_stream_mut(&mut self) -> &mut SslStream<S> {
        &mut self.stream
    }

    pub fn negotiated_identity(&self) -> Option<&[u8]> {
        self.stream.ssl().psk_identity()
    }

    /// Admits one frame without performing I/O. WouldBlock means the bounded
    /// queue is full and this payload was NOT admitted. Frames are never coalesced.
    pub fn queue_frame(&mut self, payload: &[u8]) -> Result<(), TlsPskError> {
        if self.write_failed {
            return Err(io::Error::from(io::ErrorKind::BrokenPipe).into());
        }
        if payload.is_empty() || payload.len() > ferryx_rd_proto::MAX_TCP_FRAME_SIZE {
            return Err(ferryx_rd_proto::CodecError::InvalidFrameLength(
                u32::try_from(payload.len()).unwrap_or(u32::MAX),
            )
            .into());
        }
        let frame_length = payload.len() + 4;
        if self.outbound.len() == MAX_OUTBOUND_FRAMES
            || frame_length > MAX_OUTBOUND_BYTES - self.outbound_bytes
        {
            return Err(io::Error::from(io::ErrorKind::WouldBlock).into());
        }
        let frame = TcpFrameWriter::encode(payload)?;
        self.outbound_bytes += frame_length;
        self.outbound.push_back(PendingFrame {
            bytes: frame,
            offset: 0,
        });
        Ok(())
    }

    /// Blocking convenience API. On I/O WouldBlock the admitted frame remains
    /// owned here: resume with flush_pending_frames, NOT by resubmitting payload.
    /// Nonblocking owners should use queue_frame and readiness-driven write steps.
    pub fn write_frame(&mut self, payload: &[u8]) -> Result<(), TlsPskError> {
        self.queue_frame(payload)?;
        self.flush_pending_frames()
    }

    pub fn has_pending_frames(&self) -> bool {
        !self.outbound.is_empty()
    }

    pub fn write_needs_read(&self) -> bool {
        self.write_needs_read
    }

    pub fn flush_pending_frames(&mut self) -> Result<(), TlsPskError> {
        if self.write_failed {
            return Err(io::Error::from(io::ErrorKind::BrokenPipe).into());
        }
        while self.has_pending_frames() {
            self.write_frame_step()?;
        }
        Ok(())
    }

    /// At most one bounded SSL_write (or transport flush). The front allocation,
    /// offset and slice length survive WANT_READ/WANT_WRITE unchanged, as OpenSSL
    /// requires. Only acknowledged plaintext advances the offset.
    pub fn write_frame_step(&mut self) -> Result<(), TlsPskError> {
        if self.write_failed {
            return Err(io::Error::from(io::ErrorKind::BrokenPipe).into());
        }
        let Some(frame) = self.outbound.front_mut() else {
            return Ok(());
        };
        self.write_needs_read = false;
        let result = if frame.offset == frame.bytes.len() {
            self.stream.flush().map(|()| {
                self.outbound_bytes -= self.outbound.pop_front().unwrap().bytes.len();
            })
        } else {
            let end = (frame.offset + TLS_WRITE_CHUNK).min(frame.bytes.len());
            match self.stream.ssl_write(&frame.bytes[frame.offset..end]) {
                Ok(0) => Err(io::ErrorKind::WriteZero.into()),
                Ok(count) => {
                    frame.offset += count;
                    Ok(())
                }
                Err(error)
                    if matches!(
                        error.code(),
                        openssl::ssl::ErrorCode::WANT_READ | openssl::ssl::ErrorCode::WANT_WRITE
                    ) =>
                {
                    self.write_needs_read = error.code() == openssl::ssl::ErrorCode::WANT_READ;
                    Err(io::ErrorKind::WouldBlock.into())
                }
                Err(error) => Err(error.into_io_error().unwrap_or_else(io::Error::other)),
            }
        };
        match result {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => Ok(()),
            Err(error) => {
                if error.kind() != io::ErrorKind::WouldBlock {
                    // A terminal write can have committed ciphertext. Continuing
                    // with another payload is never safe, even if I/O recovers.
                    self.write_failed = true;
                    self.outbound.clear();
                    self.outbound_bytes = 0;
                }
                Err(error.into())
            }
            Ok(()) => Ok(()),
        }
    }

    /// Reads one framed payload, buffering partial reads and coalesced frames.
    /// Zero or oversized lengths drop the complete pending framing buffer.
    pub fn read_frame(&mut self) -> Result<Vec<u8>, TlsPskError> {
        loop {
            if let Some(frame) = self.read_frame_step()? {
                return Ok(frame);
            }
        }
    }

    /// Performs at most one TLS read, retaining incomplete framing for the next step.
    pub fn read_frame_step(&mut self) -> Result<Option<Vec<u8>>, TlsPskError> {
        if self.write_failed {
            return Err(io::Error::from(io::ErrorKind::BrokenPipe).into());
        }
        if self.pending_events.is_empty() {
            let mut buffer = [0_u8; 64 * 1024];
            self.read_needs_write = false;
            let count = match self.stream.ssl_read(&mut buffer) {
                Ok(count) => count,
                Err(error) if error.code() == openssl::ssl::ErrorCode::ZERO_RETURN => 0,
                Err(error)
                    if matches!(
                        error.code(),
                        openssl::ssl::ErrorCode::WANT_READ | openssl::ssl::ErrorCode::WANT_WRITE
                    ) =>
                {
                    self.read_needs_write = error.code() == openssl::ssl::ErrorCode::WANT_WRITE;
                    return Err(io::Error::from(io::ErrorKind::WouldBlock).into());
                }
                Err(error) => {
                    return Err(error
                        .into_io_error()
                        .unwrap_or_else(io::Error::other)
                        .into())
                }
            };
            if count == 0 {
                return Err(TlsPskError::Io(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "TLS stream closed while reading frame",
                )));
            }
            self.pending_events
                .extend(self.frame_reader.push(&buffer[..count]));
        }
        match self.pending_events.pop_front() {
            Some(TcpFrameEvent::Frame(frame)) => Ok(Some(frame)),
            Some(TcpFrameEvent::DroppedInvalidLength(length)) => {
                Err(TlsPskError::InvalidFrameLength(length))
            }
            None => Ok(None),
        }
    }

    pub fn read_needs_write(&self) -> bool {
        self.read_needs_write
    }
}

impl TlsPskStream<TcpStream> {
    /// Reads a framing step under an absolute deadline. Partial frames survive
    /// subsequent calls. The socket is returned to blocking mode on every exit.
    /// Call from a blocking thread, like the other synchronous TLS APIs.
    pub fn read_frame_until(&mut self, deadline: Instant) -> Result<Option<Vec<u8>>, TlsPskError> {
        check_deadline(deadline)?;
        let readiness = DeadlineReadiness::new(self.stream.get_ref())?;
        let result = (|| loop {
            check_deadline(deadline)?;
            match self.read_frame_step() {
                Err(TlsPskError::Io(error)) if error.kind() == io::ErrorKind::WouldBlock => {}
                result => return result,
            }
            let interest = if self.read_needs_write {
                tokio::io::Interest::WRITABLE
            } else {
                tokio::io::Interest::READABLE
            };
            let result = readiness.wait_io(deadline, interest, || {
                    let result = self.read_frame_step();
                    let next_interest = if self.read_needs_write { tokio::io::Interest::WRITABLE } else { tokio::io::Interest::READABLE };
                    if matches!(&result, Err(TlsPskError::Io(e)) if e.kind() == io::ErrorKind::WouldBlock) && next_interest == interest {
                        Err(io::ErrorKind::WouldBlock.into())
                    } else {
                        Ok(result)
                    }
                })?;
            match result {
                Err(TlsPskError::Io(error)) if error.kind() == io::ErrorKind::WouldBlock => {
                    continue
                }
                result => return result,
            }
        })();
        self.stream.get_ref().set_nonblocking(false)?;
        result
    }
}

/// Shuts down both halves of a connected socket, guaranteeing our FIN is sent.
///
/// On macOS `shutdown(Both)` on a socket whose peer already sent FIN fails with `NotConnected`
/// and leaves our write half open, so the peer never observes EOF while another handle to the
/// same socket (a `try_clone`) is alive. Linux sends the FIN regardless. In that case the write
/// half is shut down explicitly; a socket that is truly disconnected still reports `NotConnected`.
pub fn shutdown_socket(socket: &TcpStream) -> io::Result<()> {
    match socket.shutdown(std::net::Shutdown::Both) {
        Err(error) if error.kind() == io::ErrorKind::NotConnected => {
            socket.shutdown(std::net::Shutdown::Write)
        }
        result => result,
    }
}

pub fn bootstrap_psk(pin: &str) -> Result<[u8; 32], TlsPskError> {
    if pin.len() != 8 || !pin.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(TlsPskError::InvalidIdentity);
    }
    let mut stretched = [0_u8; 32];
    pkcs5::pbkdf2_hmac(
        pin.as_bytes(),
        BOOTSTRAP_SALT,
        BOOTSTRAP_STRETCH_ROUNDS,
        MessageDigest::sha256(),
        &mut stretched,
    )?;
    // Wire-protocol v3 constant: HKDF info input, never rename.
    let derived = hkdf_sha256(&stretched, BOOTSTRAP_SALT, b"erd/tls-psk", 32);
    let mut output = [0_u8; 32];
    output.copy_from_slice(&derived);
    Ok(output)
}

fn configure_context(builder: &mut SslContextBuilder) -> Result<(), ErrorStack> {
    builder.set_min_proto_version(Some(SslVersion::TLS1_2))?;
    // TLS 1.3 external PSK is incompatible with the Apple Network.framework
    // peer. Keep max TLS 1.3 for policy parity, but disable TLS 1.3 cipher
    // suites so negotiation is pinned to the interoperable TLS 1.2 PSK path.
    builder.set_max_proto_version(Some(SslVersion::TLS1_3))?;
    builder.set_cipher_list(PSK_CIPHER_LIST)?;
    // OpenSSL's legacy PSK callback is TLS 1.2-only. NO_TLSV1_3 prevents it
    // from selecting a TLS 1.3 suite while retaining the explicit v3 policy
    // maximum above for stacks that later gain compatible external PSKs.
    builder.set_options(openssl::ssl::SslOptions::NO_TLSV1_3);
    builder.set_verify_callback(SslVerifyMode::PEER, |_, _| true);
    Ok(())
}

fn handshake_error<S: std::fmt::Debug>(error: HandshakeError<S>) -> TlsPskError {
    TlsPskError::Handshake(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::{
        io::Write,
        net::TcpStream,
        sync::{Arc, Barrier},
        thread,
    };

    use ferryx_rd_proto::MAX_TCP_FRAME_SIZE;

    use super::*;

    // Time is the behavior under test: this writer supplies an incomplete TLS
    // record at intervals shorter than a per-read timeout. A completion channel
    // stops it immediately; it never relies on a sleep to synchronize threads.
    fn trickle_record(mut tcp: TcpStream, stop: std::sync::mpsc::Receiver<()>) -> usize {
        tcp.write_all(&[22, 3, 3, 0x40, 0]).unwrap();
        let mut count = 0;
        loop {
            match stop.recv_timeout(Duration::from_millis(20)) {
                Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return count,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if tcp.write_all(&[0]).is_err() {
                        return count;
                    }
                    count += 1;
                }
            }
        }
    }

    #[test]
    fn shutdown_socket_sends_fin_after_peer_half_close() {
        use std::io::Read;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let local = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut peer, _) = listener.accept().unwrap();
        // A second handle keeps the descriptor open, as the session's interrupt handle does.
        let _alias = local.try_clone().unwrap();
        peer.shutdown(std::net::Shutdown::Write).unwrap();
        let mut probe = [0_u8; 1];
        assert_eq!((&local).read(&mut probe).unwrap(), 0);
        shutdown_socket(&local).unwrap();
        // The peer must see EOF without any timeout-based wait.
        assert_eq!(peer.read(&mut probe).unwrap(), 0);
    }

    #[test]
    fn deadline_handshake_trickle_cannot_extend_budget() {
        let server =
            TlsPskServer::new([PskIdentity::pairing("trickle", &[7; 32]).unwrap()]).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let tcp = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (accepted, _) = listener.accept().unwrap();
        let (stop_tx, stop_rx) = std::sync::mpsc::channel();
        let writer = thread::spawn(move || trickle_record(tcp, stop_rx));
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            let result =
                server.accept_stream_until(accepted, Instant::now() + Duration::from_millis(200));
            done_tx.send(matches!(result, Err(TlsPskError::Io(e)) if e.kind() == io::ErrorKind::TimedOut)).unwrap();
        });
        let result = done_rx.recv_timeout(Duration::from_secs(2));
        stop_tx.send(()).ok();
        writer.join().unwrap();
        worker.join().unwrap();
        assert_eq!(
            result,
            Ok(true),
            "record trickle must not renew handshake deadline"
        );
    }

    #[test]
    fn deadline_frame_retains_partial_data_and_rejects_expired_budget() {
        let psk = PskIdentity::pairing("frame", &[7; 32]).unwrap();
        let server = TlsPskServer::new([psk.clone()]).unwrap();
        let listener = server.bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (partial_tx, partial_rx) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            let mut stream = listener.accept().unwrap();
            assert_eq!(
                stream
                    .read_frame_until(Instant::now() + Duration::from_secs(5))
                    .unwrap(),
                None
            );
            partial_tx.send(()).unwrap();
            assert_eq!(
                stream
                    .read_frame_until(Instant::now() + Duration::from_secs(5))
                    .unwrap(),
                Some(b"frame".to_vec())
            );
            assert!(
                matches!(stream.read_frame_until(Instant::now()), Err(TlsPskError::Io(e)) if e.kind() == io::ErrorKind::TimedOut)
            );
        });
        let mut client = TlsPskClient::new(psk).unwrap().connect(address).unwrap();
        let frame = TcpFrameWriter::encode(b"frame").unwrap();
        client.ssl_stream_mut().write_all(&frame[..2]).unwrap();
        partial_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        client.ssl_stream_mut().write_all(&frame[2..]).unwrap();
        worker.join().unwrap();
    }

    #[test]
    fn deadline_frame_internal_record_trickle_cannot_extend_budget() {
        let psk = PskIdentity::pairing("record", &[7; 32]).unwrap();
        let server = TlsPskServer::new([psk.clone()]).unwrap();
        let listener = server.bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            let mut stream = listener.accept().unwrap();
            let result = stream.read_frame_until(Instant::now() + Duration::from_millis(200));
            done_tx.send(matches!(result, Err(TlsPskError::Io(e)) if e.kind() == io::ErrorKind::TimedOut)).unwrap();
        });
        let client = TlsPskClient::new(psk).unwrap().connect(address).unwrap();
        let raw = client.ssl_stream().get_ref().try_clone().unwrap();
        let (stop_tx, stop_rx) = std::sync::mpsc::channel();
        let writer = thread::spawn(move || trickle_record(raw, stop_rx));
        let result = done_rx.recv_timeout(Duration::from_secs(2));
        stop_tx.send(()).ok();
        writer.join().unwrap();
        drop(client);
        worker.join().unwrap();
        assert_eq!(
            result,
            Ok(true),
            "partial TLS record must not renew frame deadline"
        );
    }

    #[test]
    fn deadline_silent_peer_then_valid_psk() {
        let psk = PskIdentity::pairing("deadline", &[7; 32]).unwrap();
        let server = TlsPskServer::new([psk.clone()]).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            let (tcp, _) = listener.accept().unwrap();
            let result =
                server.accept_stream_until(tcp, Instant::now() + Duration::from_millis(100));
            done_tx.send(matches!(result, Err(TlsPskError::Io(ref e)) if e.kind() == io::ErrorKind::TimedOut)).unwrap();
            let (tcp, _) = listener.accept().unwrap();
            let mut stream = server
                .accept_stream_until(tcp, Instant::now() + Duration::from_secs(5))
                .unwrap();
            assert_eq!(
                stream.negotiated_identity(),
                Some(psk.identity().as_bytes())
            );
            assert_eq!(stream.read_frame().unwrap(), b"deadline echo");
            stream.write_frame(b"deadline echo").unwrap();
        });
        let silent = TcpStream::connect(address).unwrap();
        let timed_out = done_rx.recv_timeout(Duration::from_secs(2));
        drop(silent);
        assert_eq!(
            timed_out,
            Ok(true),
            "silent peer must reach the total handshake deadline"
        );
        let client =
            TlsPskClient::new(PskIdentity::pairing("deadline", &[7; 32]).unwrap()).unwrap();
        let tcp = TcpStream::connect(address).unwrap();
        tcp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        tcp.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut stream = client.connect_stream(tcp).unwrap();
        stream.write_frame(b"deadline echo").unwrap();
        assert_eq!(stream.read_frame().unwrap(), b"deadline echo");
        worker.join().unwrap();
    }

    #[test]
    fn bootstrap_key_is_deterministic_per_pin() {
        let expected = [
            0x2f, 0x88, 0x83, 0xb2, 0xf5, 0x6f, 0x8d, 0xe0, 0x2a, 0xc8, 0x8b, 0x3b, 0xf9, 0x21,
            0x3b, 0x70, 0x1d, 0xde, 0x0e, 0x35, 0x31, 0xc0, 0x4c, 0x9f, 0x29, 0xc5, 0xf0, 0x5f,
            0xd7, 0x4a, 0xe6, 0xfb,
        ];
        assert_eq!(bootstrap_psk("12345678").unwrap(), expected);
        assert_eq!(
            bootstrap_psk("12345678").unwrap(),
            bootstrap_psk("12345678").unwrap()
        );
        assert_ne!(
            bootstrap_psk("12345678").unwrap(),
            bootstrap_psk("87654321").unwrap()
        );
    }

    #[test]
    fn pairing_identity_uses_opaque_id() {
        let psk = PskIdentity::pairing("550e8400-e29b-41d4-a716-446655440000", &[7; 32]).unwrap();
        assert_eq!(
            psk.identity(),
            "maho-p1.550e8400-e29b-41d4-a716-446655440000"
        );
        assert_eq!(psk.key(), &[7; 32]);
    }

    #[test]
    fn bootstrap_lockout_matches_identity_semantics() {
        let start = Instant::now();
        let mut lockout = BootstrapLockout::default();
        lockout.begin_pairing();
        for attempt in 1..MAX_PAIRING_ATTEMPTS {
            assert!(!lockout.record_failure(start + Duration::from_secs(attempt as u64)));
        }
        assert!(lockout.record_failure(start + Duration::from_secs(5)));
        assert!(!lockout.is_allowed(start + Duration::from_secs(6)));
        // MahoIdentity cancels the pending pairing when lockout is reached;
        // expiry alone does not reopen bootstrap without a fresh window.
        assert!(!lockout.is_allowed(start + PAIRING_LOCKOUT + Duration::from_secs(6)));
        lockout.begin_pairing();
        assert!(lockout.is_allowed(start));
    }

    #[test]
    fn psk_loopback_completes_handshake_and_framed_echo() {
        let psk = PskIdentity::bootstrap("12345678").unwrap();
        let server = TlsPskServer::new([psk.clone()]).unwrap();
        let listener = server.bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server_thread = thread::spawn(move || {
            let mut stream = listener.accept().unwrap();
            assert_eq!(
                stream.negotiated_identity(),
                Some(BOOTSTRAP_IDENTITY.as_bytes())
            );
            let request = stream.read_frame().unwrap();
            assert_eq!(request, b"partial framed echo");
            stream.write_frame(&request).unwrap();
        });

        let client = TlsPskClient::new(psk).unwrap();
        let mut stream = client.connect(address).unwrap();
        assert_eq!(stream.ssl_stream().ssl().version_str(), "TLSv1.2");
        stream.write_frame(b"partial framed echo").unwrap();
        assert_eq!(stream.read_frame().unwrap(), b"partial framed echo");
        server_thread.join().unwrap();
    }

    #[test]
    fn rejected_queue_frame_does_not_allocate() {
        let psk = PskIdentity::new("queue-cost", vec![0x51; 32]).unwrap();
        let listener = TlsPskServer::new([psk.clone()])
            .unwrap()
            .bind("127.0.0.1:0")
            .unwrap();
        let address = listener.local_addr().unwrap();
        let host = thread::spawn(move || listener.accept().unwrap());
        let mut client = TlsPskClient::new(psk).unwrap().connect(address).unwrap();
        let _server = host.join().unwrap();
        for _ in 0..MAX_OUTBOUND_FRAMES {
            client.queue_frame(b"x").unwrap();
        }
        let retained_bytes = client.outbound_bytes;
        let payload = vec![0x37; 1024 * 1024];
        let (result, count) = crate::udp_gcm::tests::allocations(|| client.queue_frame(&payload));
        assert!(
            matches!(result, Err(TlsPskError::Io(error)) if error.kind() == io::ErrorKind::WouldBlock)
        );
        assert_eq!(client.outbound.len(), MAX_OUTBOUND_FRAMES);
        assert_eq!(client.outbound_bytes, retained_bytes);
        assert!(matches!(
            client.queue_frame(&[]),
            Err(TlsPskError::Frame(
                ferryx_rd_proto::CodecError::InvalidFrameLength(0)
            ))
        ));
        let oversized = vec![0; MAX_TCP_FRAME_SIZE + 1];
        assert!(matches!(
            client.queue_frame(&oversized),
            Err(TlsPskError::Frame(
                ferryx_rd_proto::CodecError::InvalidFrameLength(_)
            ))
        ));
        eprintln!("allocations for rejected 1 MiB TLS payload: {count}");
        assert_eq!(count, 0, "queue rejection must precede payload allocation");
    }

    #[test]
    fn framing_handles_partial_reads_and_drops_oversized_buffer() {
        let psk = PskIdentity::bootstrap("12345678").unwrap();
        let server = TlsPskServer::new([psk.clone()]).unwrap();
        let listener = server.bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let server_barrier = Arc::clone(&barrier);
        let observed = Arc::new(std::sync::Mutex::new(Vec::new()));
        let server_observed = Arc::clone(&observed);
        let server_thread = thread::spawn(move || {
            let mut stream = listener.accept().unwrap();
            let first = stream.read_frame().unwrap();
            server_observed.lock().unwrap().push(first);
            assert!(matches!(
                stream.read_frame(),
                Err(TlsPskError::InvalidFrameLength(length))
                    if length == (MAX_TCP_FRAME_SIZE as u32) + 1
            ));
            server_barrier.wait();
        });

        let client = TlsPskClient::new(psk).unwrap();
        let tcp = TcpStream::connect(address).unwrap();
        let mut stream = client.connect_stream(tcp).unwrap();
        let frame = TcpFrameWriter::encode(b"split").unwrap();
        stream.ssl_stream_mut().write_all(&frame[..2]).unwrap();
        stream.ssl_stream_mut().flush().unwrap();
        stream.ssl_stream_mut().write_all(&frame[2..]).unwrap();
        stream
            .ssl_stream_mut()
            .write_all(&((MAX_TCP_FRAME_SIZE as u32) + 1).to_le_bytes())
            .unwrap();
        stream
            .ssl_stream_mut()
            .write_all(b"discarded trailing bytes")
            .unwrap();
        stream.ssl_stream_mut().flush().unwrap();
        barrier.wait();
        server_thread.join().unwrap();
        assert_eq!(*observed.lock().unwrap(), vec![b"split".to_vec()]);
    }
}
